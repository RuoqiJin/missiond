use anyhow::{anyhow, Result};
use missiond_core::db::conversation_query::{
    audit_historical_classification, HistoricalClassificationInput,
};
use missiond_mcp::tools::ToolResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::{AppState, EmbeddingTask};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClassificationAuditRow {
    session_id: String,
    source: String,
    project: Option<String>,
    slot_id: Option<String>,
    task_id: Option<String>,
    stored_conversation_type: String,
    expected_conversation_type: String,
    reason: &'static str,
    confidence: f32,
    message_count: i64,
    first_message_preview: String,
    raw_role_present: bool,
}

fn preview(content: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in content.chars().take(max_chars) {
        out.push(ch);
    }
    out.replace('\n', " ")
}

async fn classification_audit_rows(
    state: &AppState,
    session_id: Option<&str>,
    source: Option<&str>,
    limit: i64,
    since: Option<&str>,
    min_confidence: f32,
) -> Result<Vec<ClassificationAuditRow>> {
    let conversations = if let Some(sid) = session_id {
        state
            .store
            .get_conversation(sid)
            .await
            .map_err(|e| anyhow!("DB error: {e}"))?
            .into_iter()
            .collect()
    } else {
        state
            .store
            .list_conversations(
                None,
                limit,
                Some("all"),
                None,
                since,
                None,
                source.or(Some("claude_code")),
            )
            .await
            .map_err(|e| anyhow!("DB error: {e}"))?
    };

    let mut rows = Vec::new();
    for conv in conversations {
        let first_messages = state
            .store
            .get_conversation_messages(&conv.id, Some(0), 4)
            .await
            .unwrap_or_default();
        let first_message = first_messages.first().map(|m| m.content.as_str());
        let raw_role_present = first_messages.iter().any(|m| m.raw_role.is_some());
        let slot_category = conv
            .slot_id
            .as_deref()
            .and_then(|slot_id| state.mission.get_slot_category(slot_id));
        let input = HistoricalClassificationInput {
            session_id: &conv.id,
            stored_conversation_type: &conv.conversation_type,
            source: &conv.source,
            slot_id: conv.slot_id.as_deref(),
            slot_category: slot_category.as_deref(),
            task_id: conv.task_id.as_deref(),
            raw_role_present,
            first_message,
        };
        let Some(finding) = audit_historical_classification(&input) else {
            continue;
        };
        if finding.confidence < min_confidence {
            continue;
        }
        rows.push(ClassificationAuditRow {
            session_id: finding.session_id,
            source: conv.source,
            project: conv.project,
            slot_id: conv.slot_id,
            task_id: conv.task_id,
            stored_conversation_type: finding.stored_conversation_type,
            expected_conversation_type: finding.expected_conversation_type,
            reason: finding.reason,
            confidence: finding.confidence,
            message_count: conv.message_count,
            first_message_preview: first_message.map(|s| preview(s, 240)).unwrap_or_default(),
            raw_role_present: finding.raw_role_present,
        });
    }
    Ok(rows)
}

async fn rebuild_turns_for_session(state: &AppState, session_id: &str) -> Result<usize> {
    crate::workers::local::tagger_chunker::rebuild_session_turns(state, session_id)
        .await
        .map_err(|e| anyhow!("turn backfill failed for {session_id}: {e}"))
}

pub(super) async fn handle_maintenance(
    state: &AppState,
    name: &str,
    args: Value,
) -> Result<ToolResult> {
    match name {
        "mission_trigger_backfill" => {
            let provider = state
                .embedding_service
                .as_ref()
                .map(|svc| svc.provider_id().to_string())
                .unwrap_or_else(|| "none".to_string());

            // Gather stats for all three systems
            let conv_missing = state
                .store
                .conversations_missing_summary(9999)
                .await
                .map(|v| v.len())
                .unwrap_or(0);
            let conv_stale = if let Some(svc) = state.embedding_service.as_ref() {
                state
                    .store
                    .conversations_stale_embedding(svc.provider_id(), 9999)
                    .await
                    .map(|v| v.len())
                    .unwrap_or(0)
            } else {
                0
            };
            let kb_missing = state
                .store
                .kb_entries_missing_embedding(None)
                .await
                .map(|v| v.len())
                .unwrap_or(0);
            let kb_stale = state
                .store
                .kb_entries_stale_embedding(&provider, 9999)
                .await
                .map(|v| v.len())
                .unwrap_or(0);
            let skill_missing = state
                .store
                .skill_topics_missing_embedding(9999)
                .await
                .map(|v| v.len())
                .unwrap_or(0);
            let skill_stale = state
                .store
                .skill_topics_stale_embedding(&provider, 9999)
                .await
                .map(|v| v.len())
                .unwrap_or(0);

            let total =
                conv_missing + conv_stale + kb_missing + kb_stale + skill_missing + skill_stale;
            if total == 0 {
                return Ok(ToolResult::json(&serde_json::json!({
                    "status": "nothing_to_do",
                    "currentProvider": provider,
                })));
            }

            let _ = state.embedding_tx.try_send(EmbeddingTask::BackfillAll);
            Ok(ToolResult::json(&serde_json::json!({
                "status": "triggered",
                "currentProvider": provider,
                "kb": { "stale": kb_stale, "missing": kb_missing },
                "skill": { "stale": skill_stale, "missing": skill_missing },
                "conversation": { "stale": conv_stale, "missing": conv_missing },
            })))
        }

        "mission_habit_scan" => {
            let unscanned = state
                .store
                .count_unscanned_conversations()
                .await
                .unwrap_or(0);

            let action = args
                .get("action")
                .and_then(|a| a.as_str())
                .unwrap_or("status");
            match action {
                "trigger" => {
                    if unscanned == 0 {
                        return Ok(ToolResult::json(&serde_json::json!({
                            "status": "nothing_to_do",
                            "unscanned": 0,
                        })));
                    }
                    // Reset cadence to allow immediate run
                    let _ = state.store.daemon_state_set("last_habit_scan_at", 0).await;
                    Ok(ToolResult::json(&serde_json::json!({
                        "status": "triggered",
                        "unscanned": unscanned,
                        "message": "Habit scan will run on next learning tick (within 60s)",
                    })))
                }
                _ => {
                    // Status: show scan progress
                    let total = state
                        .store
                        .count_scannable_conversations()
                        .await
                        .unwrap_or(0);
                    let scanned = total - unscanned;
                    Ok(ToolResult::json(&serde_json::json!({
                        "total": total,
                        "scanned": scanned,
                        "unscanned": unscanned,
                        "progress": if total > 0 { format!("{:.1}%", scanned as f64 / total as f64 * 100.0) } else { "N/A".to_string() },
                    })))
                }
            }
        }

        "mission_embedding_stats" => {
            let mut stats = state
                .store
                .embedding_stats()
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            // Add cache sizes
            let kb_cache_size = state.kb_search_cache.read().await.len();
            let policy_cache_size = state.embedding_cache.read().await.len();
            let skill_cache_size = state.skill_embedding_cache.read().await.len();
            let conv_cache_size = 0usize; // TopicCache removed in P3 — pgvector replaces in-memory
            let ast_cache_size = state.ast_embedding_cache.read().await.len();
            let provider = state
                .embedding_service
                .as_ref()
                .map(|svc| svc.provider_id())
                .unwrap_or("none");
            stats["cache"] = serde_json::json!({
                "kbSearch": kb_cache_size,
                "policyDecision": policy_cache_size,
                "skill": skill_cache_size,
                "conversation": conv_cache_size,
                "ast": ast_cache_size,
            });
            stats["currentProvider"] = serde_json::json!(provider);
            // AST embedding stats (code intelligence health)
            if let Ok(ast) = state.store.ast_stats().await {
                let coverage = if ast.total_nodes > 0 {
                    format!(
                        "{:.1}%",
                        ast.embedded_nodes as f64 / ast.total_nodes as f64 * 100.0
                    )
                } else {
                    "N/A".to_string()
                };
                stats["ast"] = serde_json::json!({
                    "totalNodes": ast.total_nodes,
                    "embeddedNodes": ast.embedded_nodes,
                    "coverage": coverage,
                    "totalFiles": ast.total_files,
                    "totalRepos": ast.total_repos,
                });
            }
            Ok(ToolResult::json_pretty(&stats))
        }

        // ===== Conversation Events & Agent Trajectory =====
        "mission_embedding_ops" => {
            #[derive(Deserialize)]
            struct Args {
                action: String,
            }
            let Args { action } = serde_json::from_value(args)?;
            match action.as_str() {
                "stats" => {
                    let msg_stats = state
                        .store
                        .message_embedding_stats()
                        .await
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    let provider = state
                        .embedding_service
                        .as_ref()
                        .map(|svc| svc.provider_id())
                        .unwrap_or("none");
                    Ok(ToolResult::json_pretty(&serde_json::json!({
                        "messageEmbeddings": msg_stats,
                        "currentProvider": provider,
                    })))
                }
                "backfill" => {
                    // Bypass backfill_enabled gate: explicit MCP trigger always runs.
                    // Read resume cursor from DB to continue where last run left off.
                    let cursor = state
                        .store
                        .backfill_get_phase("message_embeddings")
                        .await
                        .ok()
                        .flatten()
                        .map(|s| s.last_cursor)
                        .unwrap_or(0);
                    let _ = state
                        .embedding_tx
                        .try_send(EmbeddingTask::RunBackfillPhase {
                            phase: crate::state::BackfillPhase::MessageEmbeddings,
                            cursor,
                        });
                    let msg_stats = state
                        .store
                        .message_embedding_stats()
                        .await
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json_pretty(&serde_json::json!({
                        "status": "triggered",
                        "resumeCursor": cursor,
                        "messageEmbeddings": msg_stats,
                    })))
                }
                other => Err(anyhow!("Unknown embedding_ops action: {other}")),
            }
        }

        // ===== Reconcile: trigger JSONL-to-DB integrity check =====
        "mission_conversation_reconcile" => {
            let session_id = args
                .get("sessionId")
                .or_else(|| args.get("session_id"))
                .and_then(|v| v.as_str());

            if let Some(sid) = session_id {
                // Single-session reconcile
                let conv = state
                    .store
                    .get_conversation(sid)
                    .await
                    .map_err(|e| anyhow!("DB error: {e}"))?;
                match conv.and_then(|c| c.jsonl_path) {
                    Some(path) => {
                        crate::events_sync::reconcile_conversation_messages(state, sid, &path)
                            .await;
                        Ok(ToolResult::text(format!("Reconciled session {sid}")))
                    }
                    None => Ok(ToolResult::error(format!(
                        "Session {sid} has no jsonl_path"
                    ))),
                }
            } else {
                // Full reconcile (trigger immediately, don't wait for daily timer)
                let state_clone = state.clone();
                tokio::spawn(async move {
                    crate::workers::local::reconcile_worker::run_reconciliation_now(&state_clone)
                        .await;
                });
                Ok(ToolResult::text(
                    "Full reconciliation triggered in background",
                ))
            }
        }
        "mission_conversation_classification_audit" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                #[serde(alias = "session_id")]
                session_id: Option<String>,
                source: Option<String>,
                since: Option<String>,
                limit: Option<i64>,
                #[serde(alias = "min_confidence")]
                min_confidence: Option<f32>,
            }
            let Args {
                session_id,
                source,
                since,
                limit,
                min_confidence,
            } = serde_json::from_value(args).unwrap_or(Args {
                session_id: None,
                source: None,
                since: None,
                limit: Some(200),
                min_confidence: Some(0.9),
            });
            let rows = classification_audit_rows(
                state,
                session_id.as_deref(),
                source.as_deref(),
                limit.unwrap_or(200),
                since.as_deref(),
                min_confidence.unwrap_or(0.9),
            )
            .await?;
            Ok(ToolResult::json_pretty(&serde_json::json!({
                "mode": "dry-run",
                "count": rows.len(),
                "findings": rows,
            })))
        }
        "mission_conversation_classification_backfill" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                #[serde(alias = "session_id")]
                session_id: Option<String>,
                source: Option<String>,
                since: Option<String>,
                limit: Option<i64>,
                #[serde(alias = "min_confidence")]
                min_confidence: Option<f32>,
                apply: Option<bool>,
                #[serde(alias = "rebuild_turns")]
                rebuild_turns: Option<bool>,
            }
            let Args {
                session_id,
                source,
                since,
                limit,
                min_confidence,
                apply,
                rebuild_turns,
            } = serde_json::from_value(args).unwrap_or(Args {
                session_id: None,
                source: None,
                since: None,
                limit: Some(200),
                min_confidence: Some(0.93),
                apply: Some(false),
                rebuild_turns: Some(true),
            });
            let apply = apply.unwrap_or(false);
            let rebuild_turns = rebuild_turns.unwrap_or(true);
            let rows = classification_audit_rows(
                state,
                session_id.as_deref(),
                source.as_deref(),
                limit.unwrap_or(200),
                since.as_deref(),
                min_confidence.unwrap_or(0.93),
            )
            .await?;
            let mut changed = Vec::new();
            let mut skipped = Vec::new();
            if apply {
                for row in &rows {
                    let updated = state
                        .store
                        .set_conversation_type(&row.session_id, &row.expected_conversation_type)
                        .await
                        .map_err(|e| anyhow!("DB error: {e}"))?;
                    let raw_roles_backfilled = if row.source == "codex_cli" && !row.raw_role_present {
                        state
                            .store
                            .backfill_missing_raw_roles_for_session(&row.session_id)
                            .await
                            .map_err(|e| anyhow!("DB error: {e}"))?
                    } else {
                        0
                    };
                    let rebuilt_turns = if rebuild_turns {
                        Some(rebuild_turns_for_session(state, &row.session_id).await?)
                    } else {
                        None
                    };
                    changed.push(serde_json::json!({
                        "sessionId": row.session_id,
                        "from": row.stored_conversation_type,
                        "to": row.expected_conversation_type,
                        "reason": row.reason,
                        "confidence": row.confidence,
                        "updatedRows": updated,
                        "rawRolesBackfilled": raw_roles_backfilled,
                        "rebuiltTurns": rebuilt_turns,
                    }));
                }
            } else {
                skipped = rows
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "sessionId": r.session_id,
                            "from": r.stored_conversation_type,
                            "to": r.expected_conversation_type,
                            "reason": r.reason,
                            "confidence": r.confidence,
                        })
                    })
                    .collect();
            }
            Ok(ToolResult::json_pretty(&serde_json::json!({
                "mode": if apply { "apply" } else { "dry-run" },
                "candidateCount": rows.len(),
                "changed": changed,
                "skipped": skipped,
            })))
        }
        "mission_conversation_turn_backfill" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                #[serde(alias = "session_id")]
                session_id: String,
            }
            let Args { session_id } = serde_json::from_value(args)?;
            let inserted = rebuild_turns_for_session(state, &session_id).await?;
            Ok(ToolResult::json_pretty(&serde_json::json!({
                "sessionId": session_id,
                "insertedTurns": inserted,
            })))
        }
        _ => Err(anyhow!("Unknown conversation maintenance tool: {name}")),
    }
}
