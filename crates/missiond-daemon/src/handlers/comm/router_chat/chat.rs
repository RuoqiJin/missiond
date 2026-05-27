use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;
use tracing::{info, warn};

use crate::context::v3_blueprint_runtime::RouterRuntimeConfig;
use crate::context_budget::{apply_context_budget, MAX_ROUTER_PAYLOAD_BYTES};
use crate::embedding_worker::resolve_llm_credentials;
use crate::gemini_client::REQUEST_CALLER;
use crate::llm::gemini_file_api::{detect_mime, GeminiFileApi, PreparedFile};
use crate::state::AppState;

use super::files::{
    is_file_denied, resolve_gemini_api_key, FILE_MAX_SIZE_BINARY, FILE_MAX_SIZE_TEXT,
};

pub(super) async fn handle_chat(state: &AppState, args: Value) -> Result<ToolResult> {
    let started_at = Instant::now();
    let params: serde_json::Value =
        serde_json::from_value(args).map_err(|e| anyhow!("Invalid params: {}", e))?;
    let router_config = RouterRuntimeConfig::load_for_current_dir()
        .map_err(|e| anyhow!("V3_BLUEPRINT_CONFIG_ERROR: {}", e))?;

    // Support `message` shorthand: single string → [{role: "user", content: message}]
    let mut messages: Vec<serde_json::Value> =
        if let Some(msg) = params.get("message").and_then(|v| v.as_str()) {
            vec![serde_json::json!({"role": "user", "content": msg})]
        } else {
            params
                .get("messages")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .ok_or_else(|| anyhow!("'messages' or 'message' is required"))?
        };

    let context_mode = params
        .get("context")
        .and_then(|v| v.as_str())
        .unwrap_or("none");
    let route_task_class = params
        .get("task_class")
        .or_else(|| params.get("taskClass"))
        .and_then(|v| v.as_str())
        .unwrap_or(context_mode)
        .to_string();
    let explicit_model = params
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let mut model = explicit_model
        .clone()
        .unwrap_or_else(|| router_config.default_chat_model.clone());
    let mut route_recommendation = None;
    if explicit_model.is_none() {
        match state
            .shared_memory
            .recommended_model_for_task_class(&route_task_class)
            .await
        {
            Ok(Some(recommendation)) => {
                if let Some(recommended_model) = recommendation.get("model").and_then(Value::as_str)
                {
                    model = recommended_model.to_string();
                    route_recommendation = Some(recommendation);
                }
            }
            Ok(None) => {}
            Err(err) => {
                warn!(error = %err, task_class = %route_task_class, "router chat model outcome scoring unavailable; using compiled policy default")
            }
        }
    }
    let has_files = params
        .get("files")
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    let max_tokens: u32 = params
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .unwrap_or(if has_files {
            router_config.file_chat_default_max_tokens
        } else {
            router_config.chat_default_max_tokens
        });
    let search_enabled = params
        .get("search")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let idle_timeout = Some(
        params
            .get("idle_timeout")
            .and_then(|v| v.as_u64())
            .map(Duration::from_secs)
            .unwrap_or_else(|| router_config.router_chat_idle_timeout()),
    );
    let channel = params
        .get("channel")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let api_key_alias = params
        .get("api_key_alias")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let task_id = params
        .get("task_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // If task_id provided, load conversation context (rolling summary + active history)
    // Separate context (not saved) from new_messages (saved) for robustness
    let new_messages = messages.clone(); // preserve original new messages for saving
    let conv_id = if let Some(ref tid) = task_id {
        let cid = state
            .store
            .router_chat_get_or_create(tid, &model)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;

        // Rolling summary architecture: summary + active (unsummarized) messages
        let (summary_opt, cursor) = state
            .store
            .router_chat_get_summary(&cid)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        let active_history = state
            .store
            .router_chat_load_active_history(&cid, cursor)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;

        if summary_opt.is_some() || !active_history.is_empty() {
            let mut context_msgs: Vec<serde_json::Value> = Vec::new();

            // Inject rolling summary as system context
            if let Some(ref summary) = summary_opt {
                context_msgs.push(serde_json::json!({
                                "role": "system",
                                "content": format!("[对话历史摘要] 以下是之前对话的核心要点，请基于此上下文继续对话：\n\n{}", summary)
                            }));
            }
            context_msgs.extend(active_history);

            // Assemble: context + new messages
            let active_count = context_msgs.len();
            context_msgs.extend(messages);
            messages = context_msgs;
            info!(
                task_id = %tid, context_msgs = active_count,
                new_msgs = new_messages.len(),
                has_summary = summary_opt.is_some(), cursor,
                "Router chat: loaded context (summary + active)"
            );
        }
        Some(cid)
    } else {
        None
    };

    // Auto-inject context into first user message if requested
    if context_mode != "none" {
        let mut context_parts: Vec<String> = Vec::new();

        if context_mode == "kb" || context_mode == "both" {
            let entries = state
                .store
                .kb_list(None)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let kb_lines: Vec<String> = entries
                .iter()
                .filter(|e| e.category != "credential")
                .map(|e| {
                    format!(
                        "[{}] {}: {}",
                        e.category,
                        e.key,
                        e.summary.replace('\n', " ")
                    )
                })
                .collect();
            context_parts.push(format!(
                "\n\n[Knowledge Base ({} entries)]\n{}",
                entries.len(),
                kb_lines.join("\n")
            ));
        }

        if context_mode == "board" || context_mode == "both" {
            let tasks = state
                .store
                .list_board_tasks(None, false)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let task_lines: Vec<String> = tasks
                .iter()
                .map(|t| {
                    let desc_preview: String = t.description.chars().take(2000).collect();
                    let project = t.project.as_deref().unwrap_or("");
                    let mut line = format!(
                        "[{}|{}|{}] {}",
                        t.status.as_str(),
                        t.priority,
                        t.category,
                        t.title
                    );
                    if !project.is_empty() {
                        line.push_str(&format!(" (project: {})", project));
                    }
                    if !desc_preview.is_empty() {
                        line.push_str(&format!(" -- {}", desc_preview));
                    }
                    line
                })
                .collect();
            context_parts.push(format!(
                "\n\n[Mission Board ({} tasks)]\n{}",
                tasks.len(),
                task_lines.join("\n")
            ));
        }

        if !context_parts.is_empty() {
            // Append context to the first user message
            if let Some(first_user) = messages
                .iter_mut()
                .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
            {
                let original = first_user
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
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
        let file_api = gemini_api_key
            .as_ref()
            .map(|k| GeminiFileApi::new_with_router_runtime_config(k.clone(), &router_config));

        for file_val in files {
            let path_str = file_val
                .as_str()
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
                        path.display(),
                        e
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
                            path.display(),
                            content
                        ));
                    }
                }
                Err(_) => {
                    // Binary file — try multimodal upload via Gemini File API
                    let mime = detect_mime(&path);
                    if let Some(ref api) = file_api {
                        match api.prepare_file(&path, state.store.as_ref()).await {
                            Ok(prepared) => {
                                info!(path = %path.display(), mime, "Multimodal file prepared");
                                multimodal_files.push(prepared);
                                multimodal_use_direct_api = true;
                                // Add a text note so the model knows about the file
                                file_contents.push(format!(
                                    "\n\n[Attached multimodal file: {} ({})]",
                                    path.display(),
                                    mime
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
            if let Some(last_user) = messages
                .iter_mut()
                .rev()
                .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
            {
                let original = last_user
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                last_user["content"] =
                    serde_json::json!(format!("{}{}", original, file_contents.join("")));
            }
            info!("Router chat: attached {} file(s)", files.len());
        }
    }

    // Apply context budget before sending
    // When files are attached, refuse to truncate — error out instead of silent data loss
    let budget_result = if has_files {
        let total_bytes: usize = messages
            .iter()
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
        crate::context_budget::ContextBudgetResult {
            trimmed: false,
            note: None,
        }
    } else {
        let r = apply_context_budget(&mut messages, MAX_ROUTER_PAYLOAD_BYTES);
        if r.trimmed {
            info!(
                "Router chat: context budget applied — {}",
                r.note.as_deref().unwrap_or("trimmed")
            );
        }
        r
    };

    // --- Send to LLM: multimodal direct API or normal Router/CLI ---
    let (content, finish_reason_owned, usage, resp_model_owned, tool_calls_owned);
    let mut retry_diagnostics = Vec::new();

    if multimodal_use_direct_api && !multimodal_files.is_empty() {
        // Multimodal path: call Gemini generateContent API directly with file parts.
        // Bypasses Router/CLI because OpenAI format doesn't support fileData/inlineData.
        let api_key = resolve_gemini_api_key().ok_or_else(|| {
            anyhow!("gemini_api_key required for multimodal but not found in llm.yaml")
        })?;
        let file_api = GeminiFileApi::new_with_router_runtime_config(api_key, &router_config);

        // Extract the full text prompt from messages
        let text_prompt: String = messages
            .iter()
            .filter_map(|m| {
                let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("");
                let c = m.get("content").and_then(|c| c.as_str()).unwrap_or("");
                if c.is_empty() {
                    return None;
                }
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

        let response_text = file_api
            .generate_content(&model, &text_prompt, &multimodal_files, max_tokens)
            .await?;

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
            return Err(anyhow!(
                "'channel' 参数仅在 CLI 模式下生效，当前为 HTTP Router 模式"
            ));
        }

        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "max_tokens": max_tokens,
        });
        if search_enabled {
            body["tools"] = serde_json::json!([{"type": "google_search"}]);
        }
        // Inject channel + alias into body for GeminiClient CLI branch to extract
        if let Some(ref ch) = channel {
            body["_channel"] = serde_json::json!(ch);
        }
        if let Some(ref alias) = api_key_alias {
            body["_api_key_alias"] = serde_json::json!(alias);
        }

        let total_chars: usize = messages
            .iter()
            .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
            .map(|s| s.len())
            .sum();
        info!(
            "Router chat: {} messages ({} chars) to {} via {}",
            messages.len(),
            total_chars,
            model,
            url
        );

        let max_attempts = router_config.router_chat_retry_max_attempts.max(1);
        let mut attempt = 1_u32;
        let mut retry_delay = router_config.router_chat_retry_initial_backoff();
        let max_retry_delay = router_config.router_chat_retry_max_backoff();
        let result = loop {
            let attempt_result = REQUEST_CALLER
                .scope("router_chat".to_string(), async {
                    state
                        .gemini
                        .send_with_timeout(&state.http_client, &url, &jwt, &body, idle_timeout)
                        .await
                })
                .await;
            match attempt_result {
                Ok(result) => break result,
                Err(err) => {
                    let error = format!("{:#}", err);
                    if attempt >= max_attempts || !is_router_chat_transient_error(&error) {
                        return Err(anyhow!(
                            "mission_router_chat failed after {} attempt(s): {}",
                            attempt,
                            error
                        ));
                    }
                    let delay_ms = retry_delay.as_millis() as u64;
                    warn!(
                        attempt,
                        max_attempts,
                        delay_ms,
                        error = %error,
                        "mission_router_chat transient failure, retrying"
                    );
                    retry_diagnostics.push(serde_json::json!({
                        "attempt": attempt,
                        "delay_ms": delay_ms,
                        "error": missiond_core::util::safe_byte_truncate(&error, 500),
                    }));
                    tokio::time::sleep(retry_delay).await;
                    retry_delay = next_router_chat_retry_delay(retry_delay, max_retry_delay);
                    attempt += 1;
                }
            }
        };

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
        resp_model_owned = result
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(&model)
            .to_string();
        // Extract tool calls from CLI response (if any)
        tool_calls_owned = result.get("tool_calls").cloned();
    };

    let finish_reason = finish_reason_owned.as_str();
    let resp_model = resp_model_owned.as_str();
    let latency_ms = started_at.elapsed().as_millis() as i64;
    let input_tokens = usage_token(usage.as_ref(), &["prompt_tokens", "input_tokens"]);
    let output_tokens = usage_token(usage.as_ref(), &["completion_tokens", "output_tokens"]);
    let total_tokens = usage_token(usage.as_ref(), &["total_tokens"]);
    let provider = if multimodal_use_direct_api && !multimodal_files.is_empty() {
        "gemini-direct"
    } else if state.gemini.is_cli_mode() {
        "gemini-cli"
    } else {
        "router-http"
    };
    let _ = state
        .storage()
        .shared_memory
        .handle_action(&serde_json::json!({
            "action": "model_route_outcome_put",
            "project_id": params.get("project_id").or_else(|| params.get("projectId")).and_then(|v| v.as_str()),
            "task_id": task_id.as_deref(),
            "provider": provider,
            "model": resp_model,
            "task_class": route_task_class.as_str(),
            "latency_ms": latency_ms,
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "total_tokens": total_tokens,
            "outcome": finish_reason,
            "metadata": {
                "has_files": has_files,
                "search": search_enabled,
                "retry_count": retry_diagnostics.len(),
                "context_mode": context_mode,
                "route_decision": {
                    "source": if route_recommendation.is_some() { "model_route_outcomes" } else { "compiled_policy" },
                    "recommendation": route_recommendation.clone()
                }
            }
        }))
        .await
        .map_err(|err| {
            warn!(error = %err, "mission_router_chat failed to record model route outcome");
            err
        });

    // When files are attached, output truncation is unacceptable — return error with partial content
    if has_files && (finish_reason == "length" || finish_reason == "max_tokens") {
        return Err(anyhow!(
            "输出被截断（finish_reason={}，max_tokens={}）。附件模式下不允许截断。\n请增大 max_tokens 或简化 prompt 后重试。\n\n--- 部分响应 ---\n{}",
            finish_reason,
            max_tokens,
            missiond_core::util::safe_byte_truncate(&content, 500)
        ));
    }

    let mut resp = serde_json::json!({
        "model": resp_model,
        "response": content,
        "usage": usage,
        "route_decision": {
            "task_class": route_task_class,
            "source": if route_recommendation.is_some() { "model_route_outcomes" } else { "compiled_policy" },
            "recommendation": route_recommendation
        }
    });
    if let Some(tc) = tool_calls_owned {
        resp["tool_calls"] = tc;
    }
    if finish_reason == "length" || finish_reason == "max_tokens" {
        resp["warning"] = serde_json::json!(
            "⚠️ 输出被截断：LLM 达到 max_tokens 限制，返回内容不完整。可增大 max_tokens 参数重试。"
        );
        resp["finish_reason"] = serde_json::json!(finish_reason);
    }
    if let Some(note) = budget_result.note {
        resp["context_budget"] = serde_json::json!(note);
    }
    if !retry_diagnostics.is_empty() {
        resp["retry_diagnostics"] = serde_json::json!({
            "attempts": retry_diagnostics.len() + 1,
            "max_attempts": router_config.router_chat_retry_max_attempts,
            "retries": retry_diagnostics,
        });
    }

    // Save only NEW messages + assistant response (new_messages is pre-separated from context)
    if let Some(ref cid) = conv_id {
        let mut save_msgs: Vec<(String, String)> = new_messages
            .iter()
            .map(|msg| {
                let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                let msg_content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
                (role.to_string(), msg_content.to_string())
            })
            .collect();
        // Add assistant response
        save_msgs.push(("assistant".to_string(), content.to_string()));

        if let Err(e) = state
            .store
            .router_chat_append_messages(cid, &save_msgs)
            .await
        {
            warn!("Failed to save router chat history: {}", e);
        } else {
            info!(conv_id = %cid, saved = save_msgs.len(), "Router chat: saved messages to history");
        }
        resp["conversation_id"] = serde_json::json!(cid);
    }

    Ok(ToolResult::json_pretty(&resp))
}

fn usage_token(usage: Option<&Value>, keys: &[&str]) -> Option<i64> {
    let usage = usage?;
    keys.iter()
        .find_map(|key| usage.get(*key).and_then(Value::as_i64))
}

fn is_router_chat_transient_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    if lower.contains("v3_blueprint_config_error")
        || lower.contains("invalid params")
        || lower.contains("missing")
        || lower.contains("permission")
        || lower.contains("denied")
        || lower.contains("auth token")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("terminalquotaerror")
        || lower.contains("daily quota")
        || lower.contains("failed to parse gemini response")
    {
        return false;
    }

    lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("deadline")
        || lower.contains("connection")
        || lower.contains("transport")
        || lower.contains("network")
        || lower.contains("temporarily unavailable")
        || lower.contains("too many requests")
        || lower.contains("router returned 408")
        || lower.contains("router returned 429")
        || lower.contains("router returned 500")
        || lower.contains("router returned 502")
        || lower.contains("router returned 503")
        || lower.contains("router returned 504")
        || lower.contains("gemini request failed")
        || lower.contains("gemini request queue timeout")
}

fn next_router_chat_retry_delay(current: Duration, max: Duration) -> Duration {
    std::cmp::min(current.checked_mul(2).unwrap_or(max), max)
}

#[cfg(test)]
mod tests {
    use super::{is_router_chat_transient_error, next_router_chat_retry_delay};
    use std::time::Duration;

    #[test]
    fn router_chat_retry_classifier_accepts_transient_errors() {
        assert!(is_router_chat_transient_error(
            "Router returned 503: upstream"
        ));
        assert!(is_router_chat_transient_error(
            "Gemini request queue timeout (30s)"
        ));
        assert!(is_router_chat_transient_error(
            "Gemini request failed: connection reset"
        ));
    }

    #[test]
    fn router_chat_retry_classifier_rejects_hard_errors() {
        assert!(!is_router_chat_transient_error(
            "V3_BLUEPRINT_CONFIG_ERROR: missing router-runtime-policy"
        ));
        assert!(!is_router_chat_transient_error(
            "xjp-router auth token not configured"
        ));
        assert!(!is_router_chat_transient_error(
            "Failed to parse Gemini response: eof"
        ));
        assert!(!is_router_chat_transient_error(
            "TerminalQuotaError: exhausted your daily quota"
        ));
    }

    #[test]
    fn router_chat_retry_delay_is_bounded() {
        let max = Duration::from_millis(1000);
        assert_eq!(
            next_router_chat_retry_delay(Duration::from_millis(250), max),
            Duration::from_millis(500)
        );
        assert_eq!(
            next_router_chat_retry_delay(Duration::from_millis(750), max),
            max
        );
    }
}
