//! JSONL event sync: routing, backfill, and TTL cleanup.
//!
//! Extracted from main.rs to reduce file size.
//! - handle_new_events: routes raw JSONL values to conversation_messages / conversation_events
//! - backfill_conversation_events: one-time historical data backfill on startup

use missiond_core::db::MissionDB;
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
