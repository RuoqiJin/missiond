use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use crate::context::v3_blueprint_runtime::ConversationIngestionRuntimeConfig;
use crate::lenient;
use crate::state::AppState;

fn load_conversation_config() -> Result<ConversationIngestionRuntimeConfig> {
    ConversationIngestionRuntimeConfig::load_for_current_dir()
        .map_err(|err| anyhow!("V3_BLUEPRINT_CONFIG_ERROR: {}", err))
}

pub(super) async fn handle_events(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    let config = load_conversation_config()?;
    match name {
        "mission_conversation_events" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                #[serde(alias = "session_id")]
                session_id: Option<String>,
                #[serde(alias = "event_type")]
                event_type: Option<String>,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                limit: Option<i64>,
            }
            let Args {
                session_id,
                event_type,
                limit,
            } = serde_json::from_value(args).unwrap_or(Args {
                session_id: None,
                event_type: None,
                limit: None,
            });
            if let Some(sid) = &session_id {
                let events = state
                    .store
                    .get_conversation_events(
                        sid,
                        event_type.as_deref(),
                        limit.unwrap_or(config.conversation_events_default_limit),
                    )
                    .await
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json(&serde_json::json!({
                    "sessionId": sid,
                    "events": events,
                    "count": events.len(),
                })))
            } else {
                // No sessionId → return event type summary
                let summary = state
                    .store
                    .get_event_type_summary(None)
                    .await
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                let summary_obj: Vec<serde_json::Value> = summary
                    .iter()
                    .map(|(t, c)| serde_json::json!({ "eventType": t, "count": c }))
                    .collect();
                Ok(ToolResult::json(&serde_json::json!({
                    "summary": summary_obj,
                    "totalTypes": summary.len(),
                })))
            }
        }

        "mission_agent_trajectory" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                #[serde(alias = "tool_use_id")]
                tool_use_id: String,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                limit: Option<i64>,
            }
            let Args { tool_use_id, limit } = serde_json::from_value(args)?;
            let msgs = state
                .store
                .get_agent_trajectory(
                    &tool_use_id,
                    limit.unwrap_or(config.agent_trajectory_default_limit),
                )
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            // Extract agentId from first message metadata
            let agent_id = msgs
                .first()
                .and_then(|m| m.metadata.as_ref())
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .and_then(|v| {
                    v.get("agentId")
                        .and_then(|a| a.as_str())
                        .map(|s| s.to_string())
                });
            // Strip raw_content for LLM consumption
            let msgs_lite: Vec<serde_json::Value> = msgs
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "id": m.id,
                        "role": m.role,
                        "content": m.content,
                        "timestamp": m.timestamp,
                    })
                })
                .collect();
            Ok(ToolResult::json(&serde_json::json!({
                "toolUseId": tool_use_id,
                "agentId": agent_id,
                "messages": msgs_lite,
                "count": msgs_lite.len(),
            })))
        }

        "mission_conversation_message" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let message_id = args_val
                .get("message_id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| anyhow!("missing message_id"))?;
            // Include translation if available
            let translation = state
                .store
                .get_translation(message_id)
                .await
                .ok()
                .flatten()
                .map(|(t, _)| t);
            match state
                .store
                .get_conversation_message_by_id(message_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?
            {
                Some(msg) => Ok(ToolResult::json_pretty(&serde_json::json!({
                    "id": msg.id,
                    "session_id": msg.session_id,
                    "role": msg.role,
                    "content": msg.content,
                    "raw_content": msg.raw_content,
                    "model": msg.model,
                    "timestamp": msg.timestamp,
                    "translation": translation,
                }))),
                None => Ok(ToolResult::error("Message not found")),
            }
        }

        // mission_session_narrations removed v0.4.23 Phase 6 (tables dropped).

        // ===== Activity Report =====
        "mission_activity_report" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                since: String,
                until: Option<String>,
            }
            let Args { since, until } = serde_json::from_value(args)?;
            let until =
                until.unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
            // 1. Conversations
            let convs = state
                .store
                .list_conversations(
                    None,
                    500,
                    Some("all"),
                    None,
                    Some(&since),
                    Some(&until),
                    None,
                )
                .await
                .unwrap_or_default();
            let mut by_source: std::collections::HashMap<String, i64> =
                std::collections::HashMap::new();
            let mut by_type: std::collections::HashMap<String, i64> =
                std::collections::HashMap::new();
            for c in &convs {
                *by_source.entry(c.source.clone()).or_default() += 1;
                *by_type.entry(c.conversation_type.clone()).or_default() += 1;
            }

            // 2. Board tasks — time-based filtering via static parse helpers
            let since_iso = missiond_core::db::shared::parse_since(&since).replace(' ', "T");
            let until_iso = missiond_core::db::shared::parse_until(&until).replace(' ', "T");
            let board_created = state
                .store
                .query_board_tasks_in_range("created_at", &since_iso, &until_iso)
                .await
                .unwrap_or_default();
            let board_completed = state
                .store
                .query_board_tasks_in_range_with_status("done", &since_iso, &until_iso)
                .await
                .unwrap_or_default();

            // 3. Timeline stats
            let timeline_stats = state
                .store
                .query_timeline_stats(Some(&since), Some(&until))
                .await
                .ok();

            // 4. Git commits (graceful degradation)
            let git_commits = {
                let git_since = missiond_core::db::shared::parse_since(&since);
                let git_until = missiond_core::db::shared::parse_until(&until);
                match std::process::Command::new("git")
                    .args([
                        "log",
                        "--oneline",
                        &format!("--after={}", git_since),
                        &format!("--before={}", git_until),
                        "--all",
                    ])
                    .output()
                {
                    Ok(output) if output.status.success() => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let commits: Vec<serde_json::Value> = stdout
                            .lines()
                            .filter(|l| !l.is_empty())
                            .map(|l| {
                                let (hash, msg) = l.split_once(' ').unwrap_or((l, ""));
                                serde_json::json!({"hash": hash, "message": msg})
                            })
                            .collect();
                        serde_json::json!({"total": commits.len(), "commits": commits})
                    }
                    _ => {
                        serde_json::json!({"error": "git not available or not a repository", "total": 0})
                    }
                }
            };

            Ok(ToolResult::json_pretty(&serde_json::json!({
                "since": since,
                "until": until,
                "conversations": {
                    "total": convs.len(),
                    "by_source": by_source,
                    "by_type": by_type,
                },
                "board": {
                    "created": board_created,
                    "completed": board_completed,
                },
                "timeline": timeline_stats,
                "git": git_commits,
            })))
        }

        // ===== Consolidated: Embedding Ops =====
        _ => Err(anyhow!("Unknown conversation event tool: {name}")),
    }
}
