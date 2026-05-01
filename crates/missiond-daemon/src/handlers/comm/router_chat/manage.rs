use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;
use tracing::{info, warn};

use crate::context::v3_blueprint_runtime::RouterRuntimeConfig;
use crate::embedding_worker::resolve_llm_credentials;
use crate::gemini_client::REQUEST_CALLER;
use crate::state::AppState;

pub(super) async fn handle_consolidated(state: &AppState, args: Value) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("list");
    match action {
        "history" => handle_legacy(state, "mission_router_chat_history", args).await,
        "list" => handle_legacy(state, "mission_router_chat_list", args).await,
        "delete" => handle_legacy(state, "mission_router_chat_delete", args).await,
        "clear" => handle_legacy(state, "mission_router_chat_clear", args).await,
        "delete_message" => handle_legacy(state, "mission_router_chat_delete_message", args).await,
        "restore" => handle_legacy(state, "mission_router_chat_restore", args).await,
        "stats" => handle_legacy(state, "mission_router_chat_stats", args).await,
        "compress" => handle_legacy(state, "mission_router_chat_compress", args).await,
        _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
    }
}

pub(super) async fn handle_legacy(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    let router_config = RouterRuntimeConfig::load_for_current_dir()
        .map_err(|e| anyhow!("V3_BLUEPRINT_CONFIG_ERROR: {}", e))?;
    match name {
        "mission_router_chat_history" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let task_id = args_val
                .get("task_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("task_id is required"))?;
            let conv_id = state
                .store
                .router_chat_get_or_create(task_id, &router_config.default_chat_model)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let history = state
                .store
                .router_chat_load_history(&conv_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            if history.is_empty() {
                return Ok(ToolResult::text(format!(
                    "任务 {} 暂无 Gemini 对话记录",
                    task_id
                )));
            }
            let resp = serde_json::json!({
                "task_id": task_id,
                "conversation_id": conv_id,
                "message_count": history.len(),
                "messages": history,
            });
            Ok(ToolResult::json_pretty(&resp))
        }
        "mission_router_chat_list" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let limit = args_val.get("limit").and_then(|v| v.as_i64()).unwrap_or(50);
            let convs = state
                .store
                .router_chat_list(limit)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let count = convs.len();
            Ok(ToolResult::json_pretty(&serde_json::json!({
                "count": count,
                "conversations": convs,
            })))
        }
        "mission_router_chat_stats" => {
            let stats = state
                .store
                .router_chat_stats()
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&stats))
        }
        "mission_router_chat_clear" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let conv_id = args_val.get("conversation_id").and_then(|v| v.as_str());
            let task_id = args_val.get("task_id").and_then(|v| v.as_str());
            // count: default 2 (last round = 1 user + 1 assistant), -1 for all
            let count_raw = args_val.get("count").and_then(|v| v.as_i64()).unwrap_or(2);
            let count = if count_raw < 0 { None } else { Some(count_raw) };
            match (conv_id, task_id) {
                (Some(cid), _) => {
                    let (archived, remaining) = state
                        .store
                        .router_chat_clear(cid, count)
                        .await
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json(&serde_json::json!({
                        "conversation_id": cid,
                        "archived_messages": archived,
                        "remaining_messages": remaining,
                        "note": "已归档到 router_chat_archive 表，可用 mission_router_chat_restore 恢复",
                    })))
                }
                (None, Some(tid)) => {
                    let archived = state
                        .store
                        .router_chat_clear_by_task(tid, count)
                        .await
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json(&serde_json::json!({
                        "task_id": tid,
                        "archived_messages": archived,
                        "note": "已归档到 router_chat_archive 表，可用 mission_router_chat_restore 恢复",
                    })))
                }
                _ => Ok(ToolResult::error("需要提供 conversation_id 或 task_id")),
            }
        }
        "mission_router_chat_delete" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let conv_id = args_val.get("conversation_id").and_then(|v| v.as_str());
            let task_id = args_val.get("task_id").and_then(|v| v.as_str());
            match (conv_id, task_id) {
                (Some(cid), _) => {
                    let (conv_del, msg_del) = state
                        .store
                        .router_chat_delete(cid)
                        .await
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json(&serde_json::json!({
                        "deleted_conversations": conv_del,
                        "deleted_messages": msg_del,
                    })))
                }
                (None, Some(tid)) => {
                    let (conv_del, msg_del) = state
                        .store
                        .router_chat_delete_by_task(tid)
                        .await
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json(&serde_json::json!({
                        "task_id": tid,
                        "deleted_conversations": conv_del,
                        "deleted_messages": msg_del,
                    })))
                }
                _ => Ok(ToolResult::error("需要提供 conversation_id 或 task_id")),
            }
        }
        "mission_router_chat_delete_message" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let message_id = args_val
                .get("message_id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| anyhow!("需要提供 message_id (整数)"))?;
            match state
                .store
                .router_chat_delete_message(message_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?
            {
                Some(conversation_id) => Ok(ToolResult::json(&serde_json::json!({
                    "deleted_message_id": message_id,
                    "conversation_id": conversation_id,
                    "note": "已归档到 router_chat_archive，可用 mission_router_chat_restore 恢复",
                }))),
                None => Ok(ToolResult::error(&format!(
                    "消息 {} 不存在或不属于 router_chat 对话",
                    message_id
                ))),
            }
        }
        "mission_router_chat_restore" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let conv_id = args_val
                .get("conversation_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("需要提供 conversation_id"))?;
            let restored = state
                .store
                .router_chat_restore(conv_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json(&serde_json::json!({
                "conversation_id": conv_id,
                "restored_messages": restored,
            })))
        }
        "mission_router_chat_compress" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let conv_id = args_val.get("conversation_id").and_then(|v| v.as_str());
            let task_id_arg = args_val.get("task_id").and_then(|v| v.as_str());
            // Default: compress oldest 20 messages into summary, keep recent ones active
            let batch_size = args_val
                .get("batch_size")
                .and_then(|v| v.as_i64())
                .unwrap_or(20);
            // Minimum messages to keep in active window (don't compress below this)
            let keep_recent = args_val
                .get("keep_recent")
                .and_then(|v| v.as_i64())
                .unwrap_or(10);

            // Resolve conversation ID
            let cid = match (conv_id, task_id_arg) {
                (Some(c), _) => c.to_string(),
                (None, Some(tid)) => state
                    .store
                    .router_chat_get_or_create(tid, &router_config.default_chat_model)
                    .await
                    .map_err(|e| anyhow!("DB error: {}", e))?,
                _ => return Ok(ToolResult::error("需要提供 conversation_id 或 task_id")),
            };

            // Get current summary state
            let (existing_summary, cursor) = state
                .store
                .router_chat_get_summary(&cid)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let unsummarized = state
                .store
                .router_chat_unsummarized_count(&cid, cursor)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            // Guard: don't compress if too few messages
            if unsummarized <= keep_recent {
                return Ok(ToolResult::json(&serde_json::json!({
                    "conversation_id": cid,
                    "status": "skip",
                    "reason": format!("只有 {} 条未摘要消息，低于保留阈值 {}", unsummarized, keep_recent),
                    "unsummarized_count": unsummarized,
                })));
            }

            // Load the oldest batch for compression (leave keep_recent in active window)
            let compress_count = (unsummarized - keep_recent).min(batch_size);
            let to_compress = state
                .store
                .router_chat_load_compressible(&cid, cursor, compress_count)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            if to_compress.is_empty() {
                return Ok(ToolResult::json(&serde_json::json!({
                    "conversation_id": cid,
                    "status": "skip",
                    "reason": "没有可压缩的消息",
                })));
            }

            // Format messages for summarization, with char budget to prevent token overflow
            let mut messages_text = String::new();
            let mut actual_count = 0usize;
            for (_, role, content) in &to_compress {
                let entry = format!("[{}]: {}\n\n", role, content);
                if messages_text.len() + entry.len() > router_config.compress_char_budget_chars
                    && actual_count > 0
                {
                    info!(conv_id = %cid, budget = router_config.compress_char_budget_chars,
                                      "Router chat compress: char budget reached, truncating batch at {} of {} messages",
                                      actual_count, to_compress.len());
                    break;
                }
                messages_text.push_str(&entry);
                actual_count += 1;
            }
            let new_cursor = to_compress
                .get(actual_count.saturating_sub(1))
                .map(|(id, _, _)| *id)
                .unwrap_or(cursor);

            // Use Gemini (google one channel) for summarization — Gemini summarizes its own content best
            let system_prompt = "You are a conversation summarizer. Your job is to maintain a rolling summary of a conversation between a user and an AI assistant (Gemini). Keep technical details, decisions, task IDs, and established context. Drop irrelevant chit-chat. Output only the updated summary in the same language as the conversation (Chinese if the conversation is in Chinese). Be concise but comprehensive.";

            let user_prompt = if let Some(ref prev) = existing_summary {
                format!(
                                "[之前的摘要]\n{}\n\n[需要合并的新对话]\n{}\n\n请更新摘要，合并新对话的关键信息。",
                                prev, messages_text
                            )
            } else {
                format!(
                    "[需要摘要的对话]\n{}\n\n请生成对话摘要，保留关键技术细节和决策。",
                    messages_text
                )
            };

            let compress_messages = vec![
                serde_json::json!({"role": "system", "content": system_prompt}),
                serde_json::json!({"role": "user", "content": user_prompt}),
            ];

            let (base_url, jwt) = resolve_llm_credentials().await?;
            let compress_url = format!("{}/v1/chat/completions", base_url);
            let compress_body = serde_json::json!({
                "model": &router_config.compress_model,
                "messages": compress_messages,
                "max_tokens": router_config.compress_max_tokens,
                "_channel": &router_config.compress_channel,
            });

            let summary_result = REQUEST_CALLER
                .scope("router_chat_compress".to_string(), async {
                    state
                        .gemini
                        .send_with_timeout(
                            &state.http_client,
                            &compress_url,
                            &jwt,
                            &compress_body,
                            None,
                        )
                        .await
                })
                .await
                .and_then(|resp| {
                    resp.pointer("/choices/0/message/content")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .ok_or_else(|| anyhow!("Gemini compress: empty response"))
                });

            match summary_result {
                Ok(new_summary) => {
                    // Defensive validation: reject garbage summaries
                    let reject_patterns = ["I cannot", "I'm sorry", "I apologize", "As an AI"];
                    let is_garbage = new_summary.len() < 20
                        || reject_patterns.iter().any(|p| new_summary.starts_with(p));
                    if is_garbage {
                        warn!(conv_id = %cid, summary_len = new_summary.len(),
                                          "Router chat compress: rejected low-quality summary");
                        return Ok(ToolResult::json(&serde_json::json!({
                            "conversation_id": cid,
                            "status": "error",
                            "error": "Gemini 返回的摘要质量不合格（过短或拒绝服务），已丢弃",
                            "summary_preview": new_summary.chars().take(100).collect::<String>(),
                            "note": "原始消息未受影响，可重试",
                        })));
                    }

                    // Snapshot previous summary before overwrite (for rollback)
                    if let Some(ref prev) = existing_summary {
                        if let Err(e) = state
                            .store
                            .router_chat_append_messages(
                                &cid,
                                &[("_summary_snapshot".to_string(), prev.clone())],
                            )
                            .await
                        {
                            warn!(conv_id = %cid, error = %e, "Failed to snapshot previous summary");
                        }
                    }

                    let updated = state
                        .store
                        .router_chat_update_summary(&cid, &new_summary, new_cursor, cursor)
                        .await
                        .map_err(|e| anyhow!("DB error: {}", e))?;

                    if updated {
                        info!(
                            conv_id = %cid, compressed = actual_count,
                            new_cursor, summary_len = new_summary.len(),
                            "Router chat: compressed messages into rolling summary"
                        );
                        Ok(ToolResult::json_pretty(&serde_json::json!({
                            "conversation_id": cid,
                            "status": "ok",
                            "compressed_messages": actual_count,
                            "new_cursor": new_cursor,
                            "remaining_active": unsummarized - actual_count as i64,
                            "summary_chars": new_summary.len(),
                            "summary_preview": new_summary.chars().take(200).collect::<String>(),
                        })))
                    } else {
                        Ok(ToolResult::json(&serde_json::json!({
                            "conversation_id": cid,
                            "status": "conflict",
                            "reason": "游标已被其他压缩任务推进（乐观锁冲突），本次跳过",
                        })))
                    }
                }
                Err(e) => {
                    warn!(conv_id = %cid, error = %e, "Router chat compress: Gemini summarization failed");
                    Ok(ToolResult::json(&serde_json::json!({
                        "conversation_id": cid,
                        "status": "error",
                        "error": format!("Gemini 摘要生成失败: {}", e),
                        "note": "原始消息未受影响，可重试",
                    })))
                }
            }
        }
        _ => Err(anyhow!("Unknown router_chat tool: {name}")),
    }
}
