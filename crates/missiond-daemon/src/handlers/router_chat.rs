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
use crate::llm::gemini_file_api::{GeminiFileApi, PreparedFile, detect_mime};

/// Denied path patterns for file attachments (security denylist).
/// Single-user local service: block known sensitive paths instead of whitelist.
const FILE_DENY_PATTERNS: &[&str] = &[
    "/.ssh/", "/.aws/", "/.gnupg/", "/.kube/", "/.docker/config", "/.netrc",
];
/// Denied file names (exact match on filename component).
const FILE_DENY_NAMES: &[&str] = &[
    ".env", ".env.local", ".env.production", ".env.development",
    "credentials.json", "service-account.json", "id_rsa", "id_ed25519",
];
/// Max text file size (1 MB). Larger files are auto-truncated, not rejected.
const FILE_MAX_SIZE_TEXT: u64 = 1024 * 1024;
/// Max binary file size (10 MB) for File API uploads.
const FILE_MAX_SIZE_BINARY: u64 = 10 * 1024 * 1024;

/// Resolve Gemini API key from llm.yaml (for multimodal File API).
fn resolve_gemini_api_key() -> Option<String> {
    let llm_yaml = missiond_core::default_mission_home().join("llm.yaml");
    if !llm_yaml.exists() { return None; }
    let content = std::fs::read_to_string(&llm_yaml).ok()?;
    let config: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;
    config.get("gemini_api_key")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Check if a file path matches the security denylist.
fn is_file_denied(path: &Path) -> Option<&'static str> {
    let path_str = path.to_string_lossy();
    for pattern in FILE_DENY_PATTERNS {
        if path_str.contains(pattern) {
            return Some(pattern);
        }
    }
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        for denied in FILE_DENY_NAMES {
            if name == *denied {
                return Some(denied);
            }
        }
    }
    None
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Consolidated tool: mission_router_chat_manage
    if name == "mission_router_chat_manage" {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("list");
        return match action {
            "history" => handle_inner(state, "mission_router_chat_history", args).await,
            "list" => handle_inner(state, "mission_router_chat_list", args).await,
            "delete" => handle_inner(state, "mission_router_chat_delete", args).await,
            "clear" => handle_inner(state, "mission_router_chat_clear", args).await,
            "delete_message" => handle_inner(state, "mission_router_chat_delete_message", args).await,
            "restore" => handle_inner(state, "mission_router_chat_restore", args).await,
            "stats" => handle_inner(state, "mission_router_chat_stats", args).await,
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    handle_inner(state, name, args).await
}

async fn handle_inner(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
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
            let channel = params.get("channel")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
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
            // Security: denylist (not whitelist) — block sensitive paths, allow everything else.
            // Soft failure: errors become placeholder content, never abort the whole request.
            // Binary files (images/video/PDF): uploaded via Gemini File API if API key available.
            let mut multimodal_files: Vec<PreparedFile> = Vec::new();
            let mut multimodal_use_direct_api = false;

            if let Some(files) = params.get("files").and_then(|v| v.as_array()) {
                let mut file_contents = Vec::new();
                let gemini_api_key = resolve_gemini_api_key();
                let file_api = gemini_api_key.as_ref().map(|k| GeminiFileApi::new(k.clone()));

                for file_val in files {
                    let path_str = file_val.as_str()
                        .ok_or_else(|| anyhow!("file path must be a string"))?;

                    // Resolve path (canonicalize validates existence + resolves symlinks)
                    let path = match Path::new(path_str).canonicalize() {
                        Ok(p) => p,
                        Err(e) => {
                            file_contents.push(format!(
                                "\n\n<file path=\"{}\">\n[Error: cannot resolve path — {}]\n</file>",
                                path_str, e
                            ));
                            continue;
                        }
                    };

                    // Security: denylist check (soft fail — report, don't abort)
                    if let Some(pattern) = is_file_denied(&path) {
                        file_contents.push(format!(
                            "\n\n<file path=\"{}\">\n[Denied: matched security denylist pattern '{}'. Sensitive files cannot be attached.]\n</file>",
                            path.display(), pattern
                        ));
                        warn!(path = %path.display(), pattern, "File denied by security denylist");
                        continue;
                    }

                    // Stat file
                    let metadata = match tokio::fs::metadata(&path).await {
                        Ok(m) => m,
                        Err(e) => {
                            file_contents.push(format!(
                                "\n\n<file path=\"{}\">\n[Error: {}]\n</file>",
                                path.display(), e
                            ));
                            continue;
                        }
                    };

                    // Hard size limit: skip files > 10MB
                    if metadata.len() > FILE_MAX_SIZE_BINARY {
                        file_contents.push(format!(
                            "\n\n<file path=\"{}\">\n[File too large: {:.1}MB, max {:.1}MB]\n</file>",
                            path.display(),
                            metadata.len() as f64 / 1_048_576.0,
                            FILE_MAX_SIZE_BINARY as f64 / 1_048_576.0
                        ));
                        continue;
                    }

                    // Try reading as UTF-8 text
                    match tokio::fs::read_to_string(&path).await {
                        Ok(content) => {
                            if content.len() > FILE_MAX_SIZE_TEXT as usize {
                                // Auto-truncate large text files instead of rejecting
                                let target_chars = FILE_MAX_SIZE_TEXT as usize / 3;
                                let truncated: String = content.chars().take(target_chars).collect();
                                file_contents.push(format!(
                                    "\n\n<file path=\"{}\">\n{}\n\n[... truncated: {:.0}KB of {:.0}KB shown ...]\n</file>",
                                    path.display(), truncated,
                                    truncated.len() as f64 / 1024.0,
                                    content.len() as f64 / 1024.0
                                ));
                            } else {
                                file_contents.push(format!(
                                    "\n\n<file path=\"{}\">\n{}\n</file>",
                                    path.display(), content
                                ));
                            }
                        }
                        Err(_) => {
                            // Binary file — try multimodal upload via Gemini File API
                            let mime = detect_mime(&path);
                            if let Some(ref api) = file_api {
                                match api.prepare_file(&path, state.mission.db()).await {
                                    Ok(prepared) => {
                                        info!(path = %path.display(), mime, "Multimodal file prepared");
                                        multimodal_files.push(prepared);
                                        multimodal_use_direct_api = true;
                                        // Add a text note so the model knows about the file
                                        file_contents.push(format!(
                                            "\n\n[Attached multimodal file: {} ({})]",
                                            path.display(), mime
                                        ));
                                    }
                                    Err(e) => {
                                        file_contents.push(format!(
                                            "\n\n<file path=\"{}\">\n[Multimodal upload failed: {}]\n</file>",
                                            path.display(), e
                                        ));
                                    }
                                }
                            } else {
                                file_contents.push(format!(
                                    "\n\n<file path=\"{}\">\n[Binary file: {} ({:.1}KB). Set gemini_api_key in llm.yaml to enable multimodal.]\n</file>",
                                    path.display(), mime, metadata.len() as f64 / 1024.0
                                ));
                            }
                        }
                    }
                }

                if !file_contents.is_empty() {
                    // Append to the last user message (full path preserved in XML tags)
                    if let Some(last_user) = messages.iter_mut().rev()
                        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
                    {
                        let original = last_user.get("content").and_then(|c| c.as_str()).unwrap_or("");
                        last_user["content"] = serde_json::json!(format!("{}{}", original, file_contents.join("")));
                    }
                    info!("Router chat: attached {} file(s)", files.len());
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

            // --- Send to LLM: multimodal direct API or normal Router/CLI ---
            let (content, finish_reason_owned, usage, resp_model_owned, tool_calls_owned);

            if multimodal_use_direct_api && !multimodal_files.is_empty() {
                // Multimodal path: call Gemini generateContent API directly with file parts.
                // Bypasses Router/CLI because OpenAI format doesn't support fileData/inlineData.
                let api_key = resolve_gemini_api_key()
                    .ok_or_else(|| anyhow!("gemini_api_key required for multimodal but not found in llm.yaml"))?;
                let file_api = GeminiFileApi::new(api_key);

                // Extract the full text prompt from messages
                let text_prompt: String = messages.iter()
                    .filter_map(|m| {
                        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("");
                        let c = m.get("content").and_then(|c| c.as_str()).unwrap_or("");
                        if c.is_empty() { return None; }
                        Some(format!("[{}]: {}", role, c))
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");

                info!(
                    model = %model,
                    multimodal_files = multimodal_files.len(),
                    prompt_len = text_prompt.len(),
                    "Router chat: using direct Gemini API for multimodal"
                );

                let response_text = file_api.generate_content(
                    &model, &text_prompt, &multimodal_files, max_tokens,
                ).await?;

                content = response_text;
                finish_reason_owned = "stop".to_string();
                usage = None::<Value>;
                resp_model_owned = model.clone();
                tool_calls_owned = None;
            } else {
                // Normal path: Router API or Gemini CLI
                let (base_url, jwt) = resolve_llm_credentials().await?;

                let url = format!("{}/v1/chat/completions", base_url);
                // HTTP mode: channel parameter is not supported
                if channel.is_some() && !state.gemini.is_cli_mode() {
                    return Err(anyhow!("'channel' 参数仅在 CLI 模式下生效，当前为 HTTP Router 模式"));
                }

                let mut body = serde_json::json!({
                    "model": model,
                    "messages": messages,
                    "max_tokens": max_tokens,
                });
                if search_enabled {
                    body["tools"] = serde_json::json!([{"type": "google_search"}]);
                }
                // Inject channel into body for GeminiClient CLI branch to extract
                if let Some(ref ch) = channel {
                    body["_channel"] = serde_json::json!(ch);
                }

                let total_chars: usize = messages.iter()
                    .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                    .map(|s| s.len())
                    .sum();
                info!("Router chat: {} messages ({} chars) to {} via {}", messages.len(), total_chars, model, url);

                let result = REQUEST_CALLER.scope("router_chat".to_string(), async {
                    state.gemini.send_with_timeout(&state.http_client, &url, &jwt, &body, idle_timeout).await
                }).await?;

                content = result
                    .pointer("/choices/0/message/content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(empty response)")
                    .to_string();
                finish_reason_owned = result
                    .pointer("/choices/0/finish_reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                usage = result.get("usage").cloned();
                resp_model_owned = result.get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&model)
                    .to_string();
                // Extract tool calls from CLI response (if any)
                tool_calls_owned = result.get("tool_calls").cloned();
            };

            let finish_reason = finish_reason_owned.as_str();
            let resp_model = resp_model_owned.as_str();

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
            if let Some(tc) = tool_calls_owned {
                resp["tool_calls"] = tc;
            }
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


        "mission_router_chat_delete_message" => {
            let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
            let message_id = args_val.get("message_id").and_then(|v| v.as_i64())
                .ok_or_else(|| anyhow!("需要提供 message_id (整数)"))?;
            match state.mission.db().router_chat_delete_message(message_id)
                .map_err(|e| anyhow!("DB error: {}", e))? {
                Some(conversation_id) => {
                    Ok(ToolResult::json(&serde_json::json!({
                        "deleted_message_id": message_id,
                        "conversation_id": conversation_id,
                        "note": "已归档到 router_chat_archive，可用 mission_router_chat_restore 恢复",
                    })))
                }
                None => Ok(ToolResult::error(&format!(
                    "消息 {} 不存在或不属于 router_chat 对话", message_id
                ))),
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
