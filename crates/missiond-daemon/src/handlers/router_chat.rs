use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::Value;
use tracing::{info, warn};
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;
use crate::embedding_worker::resolve_llm_credentials;
use crate::context_budget::apply_context_budget;
use crate::context_budget::MAX_ROUTER_PAYLOAD_BYTES;
use crate::gemini_client::REQUEST_CALLER;

/// Allowed directory prefixes for file attachments (path jail).
const FILE_ALLOWED_PREFIXES: &[&str] = &[
    "/Users/jinchen/Projects/",
    "/Users/jinchen/.claude/",
    "/Users/jinchen/.xjp-mission/",
];
/// Max file size for attachments (500 KB).
const FILE_MAX_SIZE: u64 = 500 * 1024;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Router Chat =====
        "mission_router_chat" => {
            let params: serde_json::Value = serde_json::from_value(args)
                .map_err(|e| anyhow!("Invalid params: {}", e))?;

            // Support `message` shorthand: single string → [{role: "user", content: message}]
            let mut messages: Vec<serde_json::Value> = if let Some(msg) = params.get("message").and_then(|v| v.as_str()) {
                vec![serde_json::json!({"role": "user", "content": msg})]
            } else {
                params.get("messages")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .ok_or_else(|| anyhow!("'messages' or 'message' is required"))?
            };

            let context_mode = params.get("context")
                .and_then(|v| v.as_str())
                .unwrap_or("none");
            let model = params.get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("gemini-3.1-pro")
                .to_string();
            let has_files = params.get("files")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            let max_tokens: u32 = params.get("max_tokens")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32)
                .unwrap_or(if has_files { 65536 } else { 16384 });
            let search_enabled = params.get("search")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let idle_timeout = params.get("idle_timeout")
                .and_then(|v| v.as_u64())
                .map(|secs| Duration::from_secs(secs));
            let task_id = params.get("task_id").and_then(|v| v.as_str()).map(|s| s.to_string());

            // If task_id provided, load conversation history and prepend
            let conv_id = if let Some(ref tid) = task_id {
                let cid = state.mission.db().router_chat_get_or_create(tid, &model)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                let history = state.mission.db().router_chat_load_history(&cid)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                if !history.is_empty() {
                    // Prepend history before new messages
                    let mut combined = history;
                    combined.extend(messages);
                    messages = combined;
                    info!(task_id = %tid, history_msgs = messages.len(), "Router chat: loaded history for task");
                }
                Some(cid)
            } else {
                None
            };

            // Auto-inject context into first user message if requested
            if context_mode != "none" {
                let mut context_parts: Vec<String> = Vec::new();

                if context_mode == "kb" || context_mode == "both" {
                    let entries = state.mission.db()
                        .kb_list(None)
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    let kb_lines: Vec<String> = entries.iter()
                        .filter(|e| e.category != "credential")
                        .map(|e| format!("[{}] {}: {}", e.category, e.key, e.summary.replace('\n', " ")))
                        .collect();
                    context_parts.push(format!("\n\n[Knowledge Base ({} entries)]\n{}", entries.len(), kb_lines.join("\n")));
                }

                if context_mode == "board" || context_mode == "both" {
                    let tasks = state.mission.db()
                        .list_board_tasks(None, false)
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    let task_lines: Vec<String> = tasks.iter()
                        .map(|t| {
                            let desc_preview: String = t.description.chars().take(2000).collect();
                            let project = t.project.as_deref().unwrap_or("");
                            let mut line = format!("[{}|{}|{}] {}",
                                t.status.as_str(), t.priority, t.category, t.title);
                            if !project.is_empty() { line.push_str(&format!(" (project: {})", project)); }
                            if !desc_preview.is_empty() { line.push_str(&format!(" -- {}", desc_preview)); }
                            line
                        })
                        .collect();
                    context_parts.push(format!("\n\n[Mission Board ({} tasks)]\n{}", tasks.len(), task_lines.join("\n")));
                }

                if !context_parts.is_empty() {
                    // Append context to the first user message
                    if let Some(first_user) = messages.iter_mut().find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user")) {
                        let original = first_user.get("content").and_then(|c| c.as_str()).unwrap_or("");
                        let enriched = format!("{}{}", original, context_parts.join(""));
                        first_user["content"] = serde_json::Value::String(enriched);
                    }
                }
            }

            // Process file attachments: read files and append to last user message
            if let Some(files) = params.get("files").and_then(|v| v.as_array()) {
                let mut file_contents = Vec::new();
                for file_val in files {
                    let path_str = file_val.as_str()
                        .ok_or_else(|| anyhow!("file path must be a string"))?;
                    let path = Path::new(path_str).canonicalize()
                        .map_err(|e| anyhow!("Cannot resolve path '{}': {}", path_str, e))?;

                    // Security: path jail check
                    if !FILE_ALLOWED_PREFIXES.iter().any(|p| path.starts_with(p)) {
                        return Err(anyhow!("File path not in allowed directories: {}", path.display()));
                    }

                    // Security: file size check
                    let metadata = tokio::fs::metadata(&path).await
                        .map_err(|e| anyhow!("Cannot stat '{}': {}", path.display(), e))?;
                    if metadata.len() > FILE_MAX_SIZE {
                        return Err(anyhow!("File too large ({} bytes, max {}): {}",
                            metadata.len(), FILE_MAX_SIZE, path.display()));
                    }

                    // Read (UTF-8 validated by read_to_string)
                    let content = tokio::fs::read_to_string(&path).await
                        .map_err(|e| anyhow!("Failed to read '{}' (must be UTF-8): {}", path.display(), e))?;

                    let filename = path.file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_else(|| path_str.to_string());
                    file_contents.push(format!("\n\n<file path=\"{}\">\n{}\n</file>", filename, content));
                }

                if !file_contents.is_empty() {
                    // Append to the last user message
                    if let Some(last_user) = messages.iter_mut().rev()
                        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
                    {
                        let original = last_user.get("content").and_then(|c| c.as_str()).unwrap_or("");
                        last_user["content"] = serde_json::json!(format!("{}{}", original, file_contents.join("")));
                    }
                    info!("Router chat: attached {} files", files.len());
                }
            }

            // Apply context budget before sending
            // When files are attached, refuse to truncate — error out instead of silent data loss
            let budget_result = if has_files {
                let total_bytes: usize = messages.iter()
                    .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                    .map(|s| s.len())
                    .sum();
                if total_bytes > MAX_ROUTER_PAYLOAD_BYTES {
                    return Err(anyhow!(
                        "附件消息总大小 ({:.1}MB) 超出上游限制 ({:.1}MB)，拒绝截断。请减少文件大小或拆分请求。",
                        total_bytes as f64 / 1_048_576.0,
                        MAX_ROUTER_PAYLOAD_BYTES as f64 / 1_048_576.0
                    ));
                }
                crate::context_budget::ContextBudgetResult { trimmed: false, note: None }
            } else {
                let r = apply_context_budget(&mut messages, MAX_ROUTER_PAYLOAD_BYTES);
                if r.trimmed {
                    info!("Router chat: context budget applied — {}", r.note.as_deref().unwrap_or("trimmed"));
                }
                r
            };

            // Resolve LLM credentials
            let (base_url, jwt) = resolve_llm_credentials().await?;

            // Call router API
            let url = format!("{}/v1/chat/completions", base_url);
            let mut body = serde_json::json!({
                "model": model,
                "messages": messages,
                "max_tokens": max_tokens,
            });
            if search_enabled {
                body["tools"] = serde_json::json!([{"type": "google_search"}]);
            }

            let total_chars: usize = messages.iter()
                .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                .map(|s| s.len())
                .sum();
            info!("Router chat: {} messages ({} chars) to {} via {}", messages.len(), total_chars, model, url);

            let result = REQUEST_CALLER.scope("router_chat".to_string(), async {
                state.gemini.send_with_timeout(&state.http_client, &url, &jwt, &body, idle_timeout).await
            }).await?;

            let content = result
                .pointer("/choices/0/message/content")
                .and_then(|v| v.as_str())
                .unwrap_or("(empty response)");
            let finish_reason = result
                .pointer("/choices/0/finish_reason")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let usage = result.get("usage");
            let resp_model = result.get("model").and_then(|v| v.as_str()).unwrap_or(&model);

            // When files are attached, output truncation is unacceptable — return error with partial content
            if has_files && (finish_reason == "length" || finish_reason == "max_tokens") {
                return Err(anyhow!(
                    "输出被截断（finish_reason={}，max_tokens={}）。附件模式下不允许截断。\n请增大 max_tokens 或简化 prompt 后重试。\n\n--- 部分响应 ---\n{}",
                    finish_reason, max_tokens, &content[..content.len().min(500)]
                ));
            }

            let mut resp = serde_json::json!({
                "model": resp_model,
                "response": content,
                "usage": usage,
            });
            if finish_reason == "length" || finish_reason == "max_tokens" {
                resp["warning"] = serde_json::json!("⚠️ 输出被截断：LLM 达到 max_tokens 限制，返回内容不完整。可增大 max_tokens 参数重试。");
                resp["finish_reason"] = serde_json::json!(finish_reason);
            }
            if let Some(note) = budget_result.note {
                resp["context_budget"] = serde_json::json!(note);
            }

            // Save new messages + assistant response to conversation history
            if let Some(ref cid) = conv_id {
                let mut new_msgs: Vec<(String, String)> = Vec::new();
                // Count how many history messages were prepended
                let history_count = state.mission.db().router_chat_load_history(cid)
                    .map(|h| h.len()).unwrap_or(0);
                // messages array = history + new_user_messages; skip history portion
                for msg in messages.iter().skip(history_count) {
                    let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                    let msg_content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    new_msgs.push((role.to_string(), msg_content.to_string()));
                }
                // Add assistant response
                new_msgs.push(("assistant".to_string(), content.to_string()));

                if let Err(e) = state.mission.db().router_chat_append_messages(cid, &new_msgs) {
                    warn!("Failed to save router chat history: {}", e);
                } else {
                    info!(conv_id = %cid, saved = new_msgs.len(), "Router chat: saved messages to history");
                }
                resp["conversation_id"] = serde_json::json!(cid);
            }

            Ok(ToolResult::json_pretty(&resp))
        }

        "mission_router_chat_history" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let task_id = args_val.get("task_id").and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("task_id is required"))?;
            let model = "gemini-3.1-pro"; // default model for lookup
            let conv_id = state.mission.db().router_chat_get_or_create(task_id, model)
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let history = state.mission.db().router_chat_load_history(&conv_id)
                .map_err(|e| anyhow!("DB error: {}", e))?;
            if history.is_empty() {
                return Ok(ToolResult::text(format!("任务 {} 暂无 Gemini 对话记录", task_id)));
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
            let convs = state.mission.db().router_chat_list(limit)
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let count = convs.len();
            Ok(ToolResult::json_pretty(&serde_json::json!({
                "count": count,
                "conversations": convs,
            })))
        }

        "mission_router_chat_stats" => {
            let stats = state.mission.db().router_chat_stats()
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
                    let (archived, remaining) = state.mission.db().router_chat_clear(cid, count)
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json(&serde_json::json!({
                        "conversation_id": cid,
                        "archived_messages": archived,
                        "remaining_messages": remaining,
                        "note": "已归档到 router_chat_archive 表，可用 mission_router_chat_restore 恢复",
                    })))
                }
                (None, Some(tid)) => {
                    let archived = state.mission.db().router_chat_clear_by_task(tid, count)
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
                    let (conv_del, msg_del) = state.mission.db().router_chat_delete(cid)
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json(&serde_json::json!({
                        "deleted_conversations": conv_del,
                        "deleted_messages": msg_del,
                    })))
                }
                (None, Some(tid)) => {
                    let (conv_del, msg_del) = state.mission.db().router_chat_delete_by_task(tid)
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


        "mission_router_chat_restore" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let conv_id = args_val.get("conversation_id").and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("需要提供 conversation_id"))?;
            let restored = state.mission.db().router_chat_restore(conv_id)
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json(&serde_json::json!({
                "conversation_id": conv_id,
                "restored_messages": restored,
            })))
        }

        _ => Err(anyhow!("Unknown router_chat tool: {name}")),
    }
}
