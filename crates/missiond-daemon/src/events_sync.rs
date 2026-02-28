//! JSONL event sync: routing, backfill, and TTL cleanup.
//!
//! Extracted from main.rs to reduce file size.
//! - handle_new_events: routes raw JSONL values to conversation_messages / conversation_events
//! - backfill_conversation_events: one-time historical data backfill on startup

use missiond_core::db::MissionDB;
use missiond_core::types::ToolCallRecord;
use serde_json::Value;
use tracing::{info, warn};

/// Extract readable text from Claude message content (string or content blocks).
/// Full content — no truncation in storage layer.
pub fn extract_text_content(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|item| {
                let block_type = item.get("type")?.as_str()?;
                match block_type {
                    "text" => item.get("text")?.as_str().map(String::from),
                    "image" => {
                        let media_type = item
                            .pointer("/source/media_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        Some(format!("[图片: {media_type}]"))
                    }
                    "tool_use" => {
                        let name = item
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown");
                        let input_str = item
                            .get("input")
                            .map(|input| {
                                if let Value::Object(map) = input {
                                    map.iter()
                                        .map(|(k, v)| {
                                            let val = match v {
                                                Value::String(s) => format!("\"{}\"", s),
                                                _ => v.to_string(),
                                            };
                                            format!("{k}: {val}")
                                        })
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                } else {
                                    input.to_string()
                                }
                            })
                            .unwrap_or_default();
                        if input_str.is_empty() {
                            Some(format!("[Tool: {name}]"))
                        } else {
                            Some(format!("[Tool: {name}] {input_str}"))
                        }
                    }
                    "thinking" => {
                        let text = item.get("thinking")?.as_str()?;
                        Some(format!("[thinking] {text}"))
                    }
                    "tool_result" => {
                        let text = if let Some(Value::String(s)) = item.get("content") {
                            s.clone()
                        } else if let Some(Value::Array(inner)) = item.get("content") {
                            inner
                                .iter()
                                .filter_map(|i| {
                                    let t = i.get("type")?.as_str()?;
                                    match t {
                                        "text" => i.get("text")?.as_str().map(String::from),
                                        "tool_reference" => {
                                            let name = i
                                                .get("tool_name")
                                                .and_then(|n| n.as_str())
                                                .unwrap_or("?");
                                            Some(format!("[ref: {name}]"))
                                        }
                                        _ => None,
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join("\n")
                        } else {
                            String::new()
                        };
                        if text.is_empty() {
                            if let Some(err) = item.get("error").and_then(|e| e.as_str()) {
                                Some(format!("[error: {err}]"))
                            } else {
                                Some("[tool_result]".to_string())
                            }
                        } else {
                            Some(text)
                        }
                    }
                    _ => None,
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Store raw content JSON for DB storage.
/// Full preservation including base64 images — user requires complete data capture.
pub fn sanitize_raw_content(content: &Value) -> Option<String> {
    serde_json::to_string(content).ok()
}

/// Stable replacement for str::floor_char_boundary (unstable).
fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Generate a concise input summary for audit trace display.
fn generate_input_summary(input: &Value) -> String {
    let map = match input.as_object() {
        Some(m) => m,
        None => return input.to_string().chars().take(200).collect(),
    };
    let parts: Vec<String> = map
        .iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| {
            let val = match v {
                Value::String(s) => {
                    if s.len() > 80 {
                        format!("\"{}...\"", &s[..floor_char_boundary(s, 77)])
                    } else {
                        format!("\"{s}\"")
                    }
                }
                Value::Array(arr) => format!("[{} items]", arr.len()),
                Value::Object(_) => "{...}".to_string(),
                _ => v.to_string(),
            };
            format!("{k}: {val}")
        })
        .collect();
    parts.join(", ")
}

/// Generate a concise output summary from tool_result content.
fn generate_output_summary(content: &Value) -> String {
    // Check for error pattern
    if let Some(obj) = content.as_object() {
        if let Some(err) = obj.get("error").and_then(|e| e.as_str()) {
            return format!("Error: {}", &err[..err.len().min(100)]);
        }
        if obj.get("deleted").and_then(|d| d.as_bool()) == Some(true) {
            return "Deleted".to_string();
        }
        if let Some(actions) = obj.get("actions").and_then(|a| a.as_array()) {
            return format!("{} actions", actions.len());
        }
        if let Some(id) = obj.get("id").and_then(|i| i.as_str()) {
            return format!("Created: {}", &id[..id.len().min(20)]);
        }
    }
    // Array of results (e.g. batch operations)
    if let Some(arr) = content.as_array() {
        if arr.is_empty() {
            return "Empty result".to_string();
        }
        // Check first item for common patterns
        if let Some(first) = arr.first().and_then(|v| v.as_object()) {
            if first.contains_key("deleted") {
                let deleted = arr.iter().filter(|v| v.get("deleted").and_then(|d| d.as_bool()) == Some(true)).count();
                return format!("{} deleted", deleted);
            }
        }
        return format!("{} items", arr.len());
    }
    if let Some(s) = content.as_str() {
        if s.len() > 100 {
            return format!("{}...", &s[..floor_char_boundary(s, 97)]);
        }
        return s.to_string();
    }
    "Success".to_string()
}

/// Extract tool_use blocks from an assistant message's content blocks.
/// Returns ToolCallRecord entries with status=pending (output filled later).
pub fn extract_tool_calls_from_assistant(
    session_id: &str,
    timestamp: &str,
    content: &Value,
) -> Vec<ToolCallRecord> {
    let blocks = match content.as_array() {
        Some(arr) => arr,
        None => return Vec::new(),
    };
    blocks
        .iter()
        .filter_map(|block| {
            if block.get("type")?.as_str()? != "tool_use" {
                return None;
            }
            let id = block.get("id")?.as_str()?.to_string();
            let name = block.get("name")?.as_str()?.to_string();
            let input = block.get("input").cloned().unwrap_or(Value::Null);
            Some(ToolCallRecord {
                id,
                session_id: session_id.to_string(),
                message_id: None,
                tool_name: name,
                input_summary: Some(generate_input_summary(&input)),
                raw_input: serde_json::to_string(&input).ok(),
                output_summary: None,
                raw_output: None,
                status: "pending".to_string(),
                duration_ms: None,
                timestamp: timestamp.to_string(),
            })
        })
        .collect()
}

/// Extract tool_result blocks from a user message's content blocks.
/// Returns (tool_use_id, output_summary, raw_output, status) tuples for updating.
pub fn extract_tool_results_from_user(content: &Value) -> Vec<(String, String, String, String)> {
    let blocks = match content.as_array() {
        Some(arr) => arr,
        None => return Vec::new(),
    };
    blocks
        .iter()
        .filter_map(|block| {
            if block.get("type")?.as_str()? != "tool_result" {
                return None;
            }
            let tool_use_id = block.get("tool_use_id")?.as_str()?.to_string();
            let result_content = block.get("content").cloned().unwrap_or(Value::Null);
            let is_error = block
                .get("is_error")
                .and_then(|e| e.as_bool())
                .unwrap_or(false);

            // Parse result content for summary
            let parsed = if let Some(s) = result_content.as_str() {
                // Try to parse string as JSON
                serde_json::from_str::<Value>(s).unwrap_or(Value::String(s.to_string()))
            } else if let Some(arr) = result_content.as_array() {
                // Extract text from content blocks
                let text: String = arr
                    .iter()
                    .filter_map(|item| {
                        if item.get("type")?.as_str()? == "text" {
                            item.get("text")?.as_str().map(String::from)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if text.is_empty() {
                    result_content.clone()
                } else {
                    serde_json::from_str::<Value>(&text)
                        .unwrap_or(Value::String(text))
                }
            } else {
                result_content.clone()
            };

            let summary = if is_error {
                let err_text = parsed.as_str().unwrap_or("unknown error");
                format!("Error: {}", &err_text[..err_text.len().min(100)])
            } else {
                generate_output_summary(&parsed)
            };
            let status = if is_error { "error" } else { "success" };
            let raw = serde_json::to_string(&result_content).unwrap_or_default();
            Some((tool_use_id, summary, raw, status.to_string()))
        })
        .collect()
}

/// Handle NewEvents from JSONL watcher: progress, system, queue-operation, file-history-snapshot.
/// - agent_progress → conversation_messages with agent_* roles
/// - everything else → conversation_events table
pub fn handle_new_events(db: &MissionDB, session_id: String, events: Vec<Value>) {
    let mut conv_events: Vec<missiond_core::types::ConversationEvent> = Vec::new();
    let mut agent_messages: Vec<missiond_core::types::ConversationMessage> = Vec::new();

    for val in &events {
        let msg_type = val.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let timestamp = val
            .get("timestamp")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        match msg_type {
            "progress" => {
                let data_type = val
                    .pointer("/data/type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match data_type {
                    "agent_progress" => {
                        let agent_id = val
                            .pointer("/data/agentId")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let parent_tool_use_id = val
                            .get("parentToolUseID")
                            .and_then(|v| v.as_str())
                            .map(String::from);

                        if let Some(inner_msg) = val.pointer("/data/message") {
                            if let Some(message) = inner_msg.get("message") {
                                let role = message
                                    .get("role")
                                    .and_then(|r| r.as_str())
                                    .unwrap_or("assistant");
                                let agent_role = format!("agent_{role}");
                                let content_val =
                                    message.get("content").cloned().unwrap_or(Value::Null);
                                let text_content = extract_text_content(&content_val);
                                let model = message
                                    .get("model")
                                    .and_then(|m| m.as_str())
                                    .map(String::from);
                                let inner_timestamp = inner_msg
                                    .get("timestamp")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or(&timestamp)
                                    .to_string();

                                if !text_content.is_empty() {
                                    let uuid = val
                                        .get("uuid")
                                        .and_then(|u| u.as_str())
                                        .map(String::from)
                                        .unwrap_or_else(|| {
                                            format!("agent-{}-{}", agent_id, inner_timestamp)
                                        });

                                    let prompt = val
                                        .pointer("/data/prompt")
                                        .and_then(|p| p.as_str())
                                        .filter(|s| !s.is_empty());
                                    let metadata = {
                                        let mut meta = serde_json::Map::new();
                                        meta.insert(
                                            "agentId".to_string(),
                                            Value::String(agent_id.clone()),
                                        );
                                        if let Some(p) = prompt {
                                            meta.insert(
                                                "prompt".to_string(),
                                                Value::String(p.to_string()),
                                            );
                                        }
                                        serde_json::to_string(&meta).ok()
                                    };

                                    agent_messages.push(
                                        missiond_core::types::ConversationMessage {
                                            id: 0,
                                            session_id: session_id.clone(),
                                            role: agent_role,
                                            content: text_content,
                                            raw_content: serde_json::to_string(&content_val).ok(),
                                            message_uuid: Some(uuid),
                                            parent_uuid: parent_tool_use_id.clone(),
                                            model,
                                            timestamp: inner_timestamp,
                                            metadata,
                                        },
                                    );
                                }
                            }
                        }
                    }
                    "hook_progress" => {
                        let hook_name = val
                            .pointer("/data/hookName")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let hook_event = val
                            .pointer("/data/hookEvent")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        conv_events.push(missiond_core::types::ConversationEvent {
                            id: 0,
                            session_id: session_id.clone(),
                            event_type: "hook_progress".to_string(),
                            content: Some(format!("{hook_event}:{hook_name}")),
                            raw_data: serde_json::to_string(val).ok(),
                            timestamp: timestamp.clone(),
                        });
                    }
                    _ => {
                        conv_events.push(missiond_core::types::ConversationEvent {
                            id: 0,
                            session_id: session_id.clone(),
                            event_type: format!("progress:{data_type}"),
                            content: None,
                            raw_data: serde_json::to_string(val).ok(),
                            timestamp: timestamp.clone(),
                        });
                    }
                }
            }
            "system" => {
                let subtype = val
                    .get("subtype")
                    .and_then(|s| s.as_str())
                    .unwrap_or("system");
                let content = match subtype {
                    "turn_duration" => {
                        let duration_ms =
                            val.get("durationMs").and_then(|d| d.as_i64()).unwrap_or(0);
                        Some(format!("{}ms", duration_ms))
                    }
                    "compact_boundary" => {
                        let pre_tokens = val
                            .pointer("/compactMetadata/preTokens")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let trigger = val
                            .pointer("/compactMetadata/trigger")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        Some(format!("trigger={trigger}, preTokens={pre_tokens}"))
                    }
                    _ => val.get("content").and_then(|c| c.as_str()).map(String::from),
                };
                conv_events.push(missiond_core::types::ConversationEvent {
                    id: 0,
                    session_id: session_id.clone(),
                    event_type: subtype.to_string(),
                    content,
                    raw_data: serde_json::to_string(val).ok(),
                    timestamp: timestamp.clone(),
                });
            }
            "queue-operation" => {
                let operation = val
                    .get("operation")
                    .and_then(|o| o.as_str())
                    .unwrap_or("");
                let content = val
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                conv_events.push(missiond_core::types::ConversationEvent {
                    id: 0,
                    session_id: session_id.clone(),
                    event_type: format!("queue:{operation}"),
                    content: if content.is_empty() {
                        None
                    } else {
                        Some(content)
                    },
                    raw_data: serde_json::to_string(val).ok(),
                    timestamp: timestamp.clone(),
                });
            }
            "file-history-snapshot" => {
                conv_events.push(missiond_core::types::ConversationEvent {
                    id: 0,
                    session_id: session_id.clone(),
                    event_type: "file_history_snapshot".to_string(),
                    content: None,
                    raw_data: serde_json::to_string(val).ok(),
                    timestamp: timestamp.clone(),
                });
            }
            _ => {
                // Catch-all: includes demoted parse failures and unknown future types
                conv_events.push(missiond_core::types::ConversationEvent {
                    id: 0,
                    session_id: session_id.clone(),
                    event_type: format!("unknown:{msg_type}"),
                    content: None,
                    raw_data: serde_json::to_string(val).ok(),
                    timestamp: timestamp.clone(),
                });
            }
        }
    }

    if !agent_messages.is_empty() {
        match db.insert_conversation_messages_batch(&agent_messages) {
            Ok(ids) if !ids.is_empty() => {
                info!(session = %session_id, count = ids.len(), "Logged agent sub-conversation messages");
            }
            Err(e) => {
                warn!(session = %session_id, error = %e, "Failed to insert agent messages");
            }
            _ => {}
        }
    }

    if !conv_events.is_empty() {
        match db.insert_conversation_events_batch(&conv_events) {
            Ok(count) if count > 0 => {
                info!(session = %session_id, count, "Logged conversation events");
            }
            Err(e) => {
                warn!(session = %session_id, error = %e, "Failed to insert conversation events");
            }
            _ => {}
        }
    }
}

/// One-time backfill: scan historical JSONL files and populate conversation_events
/// for sessions that don't yet have events.
pub async fn backfill_conversation_events(db: &MissionDB) {
    // Get sessions that already have events (skip them)
    let sessions_with_events = db.get_sessions_with_events().unwrap_or_default();

    // Get all conversations with jsonl_path
    let conversations = db.get_conversations_with_jsonl().unwrap_or_default();

    let to_backfill: Vec<_> = conversations
        .into_iter()
        .filter(|(id, _)| !sessions_with_events.contains(id))
        .collect();

    if to_backfill.is_empty() {
        info!("Event backfill: all sessions already have events, skipping");
        return;
    }

    info!(
        total = to_backfill.len(),
        "Event backfill: starting for sessions without events"
    );

    let mut backfilled = 0usize;
    let mut total_events = 0usize;
    let mut errors = 0usize;

    for (session_id, jsonl_path) in &to_backfill {
        let path = std::path::Path::new(jsonl_path);
        if !path.exists() {
            continue;
        }

        // Read entire file as raw JSON values
        let raw_lines = match missiond_core::cc_tasks::read_new_lines_raw(path, 0).await {
            Ok((lines, _)) => lines,
            Err(_) => {
                errors += 1;
                continue;
            }
        };

        // Filter for event types only (not user/assistant/tool_use/tool_result)
        let event_lines: Vec<Value> = raw_lines
            .into_iter()
            .filter(|val| {
                let msg_type = val.get("type").and_then(|t| t.as_str()).unwrap_or("");
                matches!(
                    msg_type,
                    "progress" | "system" | "queue-operation" | "file-history-snapshot"
                )
            })
            .collect();

        if event_lines.is_empty() {
            continue;
        }

        // Reuse existing handle_new_events logic
        handle_new_events(db, session_id.clone(), event_lines);
        total_events += 1;
        backfilled += 1;

        // Yield periodically to avoid blocking the runtime
        if backfilled % 50 == 0 {
            info!(
                progress = backfilled,
                total = to_backfill.len(),
                "Event backfill progress"
            );
            tokio::task::yield_now().await;
        }
    }

    info!(backfilled, total_events, errors, "Event backfill complete");

    // TTL cleanup: delete progress events older than 30 days
    let cutoff = (chrono::Utc::now() - chrono::TimeDelta::days(30)).to_rfc3339();
    match db.cleanup_old_events(&cutoff) {
        Ok(n) if n > 0 => info!(deleted = n, "Cleaned up old progress events (>30 days)"),
        Err(e) => warn!(error = %e, "Failed to cleanup old events"),
        _ => {}
    }
}

/// One-time backfill: scan historical conversation_messages and populate conversation_tool_calls
/// for sessions that don't yet have tool call records.
pub async fn backfill_tool_calls(db: &MissionDB) {
    // Sessions already backfilled — skip
    let sessions_with_tc = db.get_sessions_with_tool_calls().unwrap_or_default();

    // All conversations
    let conversations = db.get_conversations_with_jsonl().unwrap_or_default();
    let to_backfill: Vec<_> = conversations
        .into_iter()
        .filter(|(id, _)| !sessions_with_tc.contains(id))
        .collect();

    if to_backfill.is_empty() {
        info!("Tool call backfill: all sessions already have tool calls, skipping");
        return;
    }

    info!(
        total = to_backfill.len(),
        "Tool call backfill: starting for sessions without tool calls"
    );

    let mut backfilled = 0usize;
    let mut total_tc = 0usize;
    let mut total_results = 0usize;

    for (session_id, _jsonl_path) in &to_backfill {
        // Read stored messages from DB (raw_content has the full JSON content array)
        let messages = match db.get_messages_for_tool_call_backfill(session_id) {
            Ok(msgs) => msgs,
            Err(_) => continue,
        };

        if messages.is_empty() {
            continue;
        }

        let mut tool_calls = Vec::new();
        let mut tool_results = Vec::new();

        for (role, raw_content, timestamp) in &messages {
            // Parse raw_content as JSON
            let content: Value = match serde_json::from_str(raw_content) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if role == "assistant" {
                tool_calls.extend(extract_tool_calls_from_assistant(session_id, timestamp, &content));
            } else if role == "user" || role == "system" || role == "tool_result" {
                // "system" = slot sessions where user messages are stored as system role
                // "tool_result" = stored as separate role in DB
                tool_results.extend(extract_tool_results_from_user(&content));
            }
        }

        if tool_calls.is_empty() && tool_results.is_empty() {
            continue;
        }

        // Insert tool calls
        if !tool_calls.is_empty() {
            match db.insert_tool_calls_batch(&tool_calls) {
                Ok(count) => total_tc += count,
                Err(e) => {
                    warn!(session = %session_id, error = %e, "Tool call backfill: insert failed");
                    continue;
                }
            }
        }

        // Update tool results
        for (tool_use_id, summary, raw, status) in &tool_results {
            if let Err(e) = db.update_tool_call_output(tool_use_id, summary, raw, status) {
                warn!(tool_use_id, error = %e, "Tool call backfill: update output failed");
            } else {
                total_results += 1;
            }
        }

        backfilled += 1;

        if backfilled % 50 == 0 {
            info!(
                progress = backfilled,
                total = to_backfill.len(),
                tool_calls = total_tc,
                "Tool call backfill progress"
            );
            tokio::task::yield_now().await;
        }
    }

    // Second pass: update pending tool calls that are missing output
    // (previous backfill may have inserted tool_use but missed tool_result)
    let pending_count = db.count_pending_tool_calls().unwrap_or(0);
    if pending_count > 0 {
        info!(pending_count, "Tool call backfill: updating pending tool calls with missing output");
        let sessions_with_pending = db.get_sessions_with_pending_tool_calls().unwrap_or_default();
        let mut patched = 0usize;
        for session_id in &sessions_with_pending {
            let messages = match db.get_messages_for_tool_call_backfill(session_id) {
                Ok(msgs) => msgs,
                Err(_) => continue,
            };
            for (role, raw_content, _ts) in &messages {
                if role != "user" && role != "system" && role != "tool_result" {
                    continue;
                }
                let content: Value = match serde_json::from_str(raw_content) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let results = extract_tool_results_from_user(&content);
                for (tool_use_id, summary, raw, status) in &results {
                    if let Ok(true) = db.update_tool_call_output(tool_use_id, summary, raw, status) {
                        patched += 1;
                    }
                }
            }
        }
        info!(patched, "Tool call backfill: patched pending tool calls with output");
    }

    info!(
        backfilled,
        total_tc,
        total_results,
        "Tool call backfill complete"
    );
}
