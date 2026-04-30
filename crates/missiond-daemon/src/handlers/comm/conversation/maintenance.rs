use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use crate::state::{AppState, EmbeddingTask};

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
        _ => Err(anyhow!("Unknown conversation maintenance tool: {name}")),
    }
}
