use anyhow::{anyhow, Result};
use missiond_core::types::{CliEngine, Conversation, Slot};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tracing::{info, warn};

use crate::context::v3_blueprint_runtime::WorkstationRuntimeConfig;
use crate::engine::master_control;
use crate::lenient;
use crate::state::AppState;

#[derive(Deserialize)]
struct InboxArgs {
    #[serde(
        rename = "unreadOnly",
        default,
        deserialize_with = "lenient::option_bool"
    )]
    unread_only: Option<bool>,
    #[serde(default)]
    limit: Option<usize>,
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_slots" => Ok(ToolResult::json(&projected_mission_slots(state).await)),
        "mission_master_status" => Ok(ToolResult::json(
            &master_control::mission_master_status(state).await,
        )),
        "mission_convergence_status" => {
            Ok(ToolResult::json(&mission_convergence_status(state).await))
        }
        "mission_inbox" => {
            let InboxArgs { unread_only, limit } =
                serde_json::from_value(args).unwrap_or(InboxArgs {
                    unread_only: None,
                    limit: None,
                });
            let messages = state
                .store
                .get_inbox_messages(unread_only.unwrap_or(true), limit.unwrap_or(10) as i64)
                .await?;
            Ok(ToolResult::json(&messages))
        }
        "mission_pause" => handle_pause(state, args),
        "mission_slot_history" => handle_slot_history(state, args).await,
        _ => Err(anyhow!("Unknown compute slot tool: {name}")),
    }
}

async fn mission_convergence_status(state: &AppState) -> Value {
    let root = convergence_repo_root(state);
    let output = tokio::time::timeout(
        Duration::from_secs(45),
        Command::new("node")
            .args([
                "scripts/check-v3-final-convergence.mjs",
                "--json",
                "--static-only",
            ])
            .current_dir(&root)
            .output(),
    )
    .await;

    match output {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            match serde_json::from_str::<Value>(&stdout) {
                Ok(mut value) => {
                    value["source"] = json!("mission_convergence_status");
                    value["repoRoot"] = json!(root.display().to_string());
                    value
                }
                Err(err) => json!({
                    "schema": "missiond.convergence-status.v1",
                    "ok": false,
                    "failed_stage": "parse-final-convergence-json",
                    "blocking_items": [{
                        "kind": "runtime-error",
                        "stage": "parse-final-convergence-json",
                        "message": err.to_string()
                    }],
                    "next_action": "fix final convergence JSON output",
                    "runtime_status": {
                        "mode": "static-only",
                        "repoRoot": root.display().to_string()
                    },
                    "smoke_task_ids": []
                }),
            }
        }
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            json!({
                "schema": "missiond.convergence-status.v1",
                "ok": false,
                "failed_stage": "final-convergence-static-gate",
                "blocking_items": [{
                    "kind": "failed-check",
                    "stage": "final-convergence-static-gate",
                    "exit_code": output.status.code(),
                    "stdout_tail": tail_text(&stdout, 20),
                    "stderr_tail": tail_text(&stderr, 20)
                }],
                "next_action": "inspect blocking_items[0], repair the named stage, and rerun node scripts/check-v3-final-convergence.mjs --json --static-only",
                "runtime_status": {
                    "mode": "static-only",
                    "repoRoot": root.display().to_string()
                },
                "smoke_task_ids": []
            })
        }
        Ok(Err(err)) => json!({
            "schema": "missiond.convergence-status.v1",
            "ok": false,
            "failed_stage": "spawn-final-convergence-gate",
            "blocking_items": [{
                "kind": "runtime-error",
                "stage": "spawn-final-convergence-gate",
                "message": err.to_string()
            }],
            "next_action": "ensure node and final convergence checker are available to the daemon environment",
            "runtime_status": {
                "mode": "static-only",
                "repoRoot": root.display().to_string()
            },
            "smoke_task_ids": []
        }),
        Err(_) => json!({
            "schema": "missiond.convergence-status.v1",
            "ok": false,
            "failed_stage": "final-convergence-timeout",
            "blocking_items": [{
                "kind": "runtime-error",
                "stage": "final-convergence-timeout",
                "message": "static final convergence status timed out after 45s"
            }],
            "next_action": "profile or split the static final convergence gate",
            "runtime_status": {
                "mode": "static-only",
                "repoRoot": root.display().to_string()
            },
            "smoke_task_ids": []
        }),
    }
}

fn convergence_repo_root(state: &AppState) -> PathBuf {
    state
        .mission
        .list_slots()
        .into_iter()
        .find(|slot| slot.config.id == master_control::MASTER_SLOT_ID)
        .and_then(|slot| slot.config.project_root.or(slot.config.cwd))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn tail_text(text: &str, lines: usize) -> String {
    text.lines()
        .rev()
        .take(lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

async fn projected_mission_slots(state: &AppState) -> Vec<Value> {
    let slots = state.mission.list_slots();
    let Ok(config) = WorkstationRuntimeConfig::load_for_current_dir() else {
        return slots
            .into_iter()
            .filter_map(|slot| serde_json::to_value(slot).ok())
            .collect();
    };
    let v3_slot_ids: HashSet<String> = config
        .startup_slots()
        .iter()
        .filter_map(|slot| slot.slot_id.clone())
        .chain(
            config
                .workstation_pool()
                .iter()
                .map(|worker| worker.slot_id.clone()),
        )
        .collect();

    let projected: Vec<Slot> = slots
        .into_iter()
        .filter(|slot| !is_stopped_legacy_sonnet_residual(slot, &v3_slot_ids))
        .collect();

    let mut out = Vec::with_capacity(projected.len());
    for slot in projected {
        let mut value = serde_json::to_value(&slot).unwrap_or_else(|_| json!({}));
        if let Some(info) = state.pty.get_status(&slot.config.id).await {
            value["ptyState"] = json!(serde_json::to_value(&info.state)
                .ok()
                .and_then(|v| v.as_str().map(ToString::to_string))
                .unwrap_or_else(|| format!("{:?}", info.state)));
            if let Some(recognition) = info.recognition {
                if let Ok(recognition) = serde_json::to_value(recognition) {
                    value["ptyRecognition"] = recognition;
                }
            }
        }

        let mut latest_is_codex_placeholder = false;
        if let Ok(Some(session_id)) = state.store.get_slot_session(&slot.config.id).await {
            value["sessionId"] = json!(session_id);
            if let Ok(Some(conv)) = state.store.get_conversation(&session_id).await {
                if conversation_source_matches_engine(slot.config.engine, &conv.source) {
                    latest_is_codex_placeholder = slot.config.engine == CliEngine::Codex
                        && conv.message_count == 0
                        && conv.id.starts_with("pty-slot-");
                    value["latestConversation"] = conversation_summary_json(&conv);
                } else {
                    value["latestConversationMismatch"] = json!({
                        "id": conv.id,
                        "source": conv.source,
                        "expectedSource": canonical_source_for_engine(slot.config.engine),
                    });
                }
            }
        }
        if slot.config.engine == CliEngine::Codex
            && (value.get("latestConversation").is_none() || latest_is_codex_placeholder)
        {
            if let Some(conv) = latest_provider_conversation_for_slot(state, &slot).await {
                value["providerConversationId"] = json!(conv.id.clone());
                value["latestConversation"] = conversation_summary_json(&conv);
            }
        }
        if slot.config.id == master_control::MASTER_SLOT_ID {
            let mcp_enabled = master_control::probe_codex_mcp_ready().await;
            let approval = master_control::probe_codex_mcp_approval_ready();
            value["mcpReady"] = json!(mcp_enabled && approval.ready);
            value["mcpEnabled"] = json!(mcp_enabled);
            value["mcpApprovalReady"] = json!(approval.ready);
            value["mcpApprovalMissingTools"] = json!(approval.missing_tools);
            value["mcpSource"] = json!("~/.codex/config.toml");
        }
        out.push(value);
    }

    out
}

fn canonical_source_for_engine(engine: CliEngine) -> &'static str {
    match engine {
        CliEngine::ClaudeCode => "claude_code",
        CliEngine::Gemini => "gemini_cli",
        CliEngine::Codex => "codex_cli",
    }
}

fn conversation_source_matches_engine(engine: CliEngine, source: &str) -> bool {
    source == canonical_source_for_engine(engine)
}

async fn latest_provider_conversation_for_slot(
    state: &AppState,
    slot: &Slot,
) -> Option<Conversation> {
    let source = canonical_source_for_engine(slot.config.engine);
    let conversations = state
        .store
        .list_conversations(None, 10, None, None, None, None, Some(source))
        .await
        .ok()?;
    conversations
        .into_iter()
        .find(|conv| conversation_matches_slot_project(conv, slot))
}

fn conversation_matches_slot_project(conv: &Conversation, slot: &Slot) -> bool {
    let Some(project) = conv.project.as_deref() else {
        return false;
    };
    slot.config
        .project_root
        .as_deref()
        .or(slot.config.cwd.as_deref())
        .is_some_and(|root| project == root || project.starts_with(root))
}

fn conversation_summary_json(conv: &Conversation) -> Value {
    json!({
        "id": conv.id,
        "source": conv.source,
        "chatType": conv.chat_type,
        "conversationType": conv.conversation_type,
        "messageCount": conv.message_count,
        "status": conv.status,
        "updatedAt": conv.updated_at,
    })
}

fn is_stopped_legacy_sonnet_residual(slot: &Slot, v3_slot_ids: &HashSet<String>) -> bool {
    if v3_slot_ids.contains(&slot.config.id) {
        return false;
    }
    slot.session_id.is_none()
        && slot.config.engine == CliEngine::ClaudeCode
        && slot.config.project_root.is_none()
        && slot
            .config
            .model
            .as_deref()
            .is_some_and(|model| model.contains("sonnet"))
}

fn handle_pause(state: &AppState, args: Value) -> Result<ToolResult> {
    use std::sync::atomic::Ordering;

    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("status");
    let home = crate::helpers::default_mission_home();
    let flag = home.join("global_paused");

    match action {
        "pause" => {
            state.global_paused.store(true, Ordering::Relaxed);
            let now = chrono::Utc::now().timestamp();
            state.global_paused_at.store(now, Ordering::Relaxed);
            let _ = std::fs::write(&flag, now.to_string());
            warn!("Global pause activated via MCP tool");
            Ok(ToolResult::text(
                "✅ 全局暂停已激活。所有工位任务分派已停止。",
            ))
        }
        "resume" => {
            state.global_paused.store(false, Ordering::Relaxed);
            state.global_paused_at.store(0, Ordering::Relaxed);
            let _ = std::fs::remove_file(&flag);
            info!("Global pause deactivated via MCP tool");
            Ok(ToolResult::text("▶️ 全局暂停已解除。工位恢复正常工作。"))
        }
        _ => {
            let paused = state.control_manager.current().global_paused;
            let since = state.global_paused_at.load(Ordering::Relaxed);
            let msg = if paused {
                format!(
                    "⏸ 当前处于全局暂停状态 (始于 {})",
                    chrono::DateTime::from_timestamp(since, 0)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_else(|| "未知时间".to_string())
                )
            } else {
                "🟢 当前工作正常，未暂停。".to_string()
            };
            Ok(ToolResult::text(msg))
        }
    }
}

async fn handle_slot_history(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SlotHistoryArgs {
        slot_id: Option<String>,
        task_type: Option<String>,
        status: Option<String>,
        #[serde(default, deserialize_with = "lenient::option_i64")]
        limit: Option<i64>,
        #[serde(default, deserialize_with = "lenient::option_bool")]
        stats: Option<bool>,
    }
    let args: SlotHistoryArgs = serde_json::from_value(args)?;
    if args.stats.unwrap_or(false) {
        let stats = state
            .store
            .slot_task_stats(args.slot_id.as_deref())
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        Ok(ToolResult::json_pretty(&stats))
    } else {
        let tasks = state
            .store
            .list_slot_tasks(
                args.slot_id.as_deref(),
                args.task_type.as_deref(),
                args.status.as_deref(),
                args.limit.unwrap_or(20),
            )
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        Ok(ToolResult::json_pretty(&tasks))
    }
}
