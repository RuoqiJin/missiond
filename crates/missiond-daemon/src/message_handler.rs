use tracing::{error, info, warn};

use crate::event_bus::{DaemonEvent, TraceContext};
use crate::state::AppState;
use crate::events_sync;

pub(crate) fn handle_new_messages(
    state: &AppState,
    session_id: String,
    project_path: String,
    jsonl_path: String,
    messages: Vec<missiond_core::CCMessageLine>,
    is_pty: bool,
) {
    let db = state.mission.db();

    // Determine slot_id if this session belongs to a PTY slot
    let slot_id = if is_pty {
        db.get_slot_for_session(&session_id).unwrap_or(None)
    } else {
        None
    };
    let is_slot_session = slot_id.is_some();
    let source = if is_pty { "pty_jsonl" } else { "claude_cli" };

    // Ensure conversation exists; re-activate if completed
    let existing_conv = db.get_conversation(&session_id).unwrap_or(None);
    if let Some(ref conv) = existing_conv {
        if conv.status == "completed" {
            if let Err(e) = db.reactivate_conversation(&session_id) {
                warn!(session = %session_id, error = %e, "Failed to re-activate conversation");
            } else {
                info!(session = %session_id, "Re-activated completed conversation");
            }
        }
    }
    if existing_conv.is_none() {
        let first = messages.first();
        // Extract parent session ID for subagent conversations
        let parent_session_id = if session_id.starts_with("agent-") {
            missiond_core::db::extract_parent_session_id(&jsonl_path)
        } else {
            None
        };
        let conv = missiond_core::types::Conversation {
            id: session_id.clone(),
            project: Some(first.map(|m| m.cwd.clone()).unwrap_or(project_path)),
            slot_id: slot_id.clone(),
            source: source.to_string(),
            model: first.and_then(|m| m.message.model.clone()),
            git_branch: first.and_then(|m| m.git_branch.clone()),
            jsonl_path: Some(jsonl_path),
            parent_session_id,
            task_id: None,
            message_count: 0,
            started_at: first
                .map(|m| m.timestamp.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            ended_at: None,
            status: "active".to_string(),
            analyzed_at: None,
            analysis_version: 0,
            analysis_retries: 0,
            deep_analyzed_message_id: 0,
            chat_type: None,
            conversation_type: missiond_core::db::derive_conversation_type(slot_id.as_deref(), &session_id),
            updated_at: None,
            llm_summary: None,
            embedding_provider: None,
            session_timeline: None,
            timeline_built_at: None,
        };
        if let Err(e) = db.upsert_conversation(&conv) {
            error!(session = %session_id, error = %e, "Failed to create conversation");
            return;
        }
    }

    let batch: Vec<missiond_core::types::ConversationMessage> = messages.iter()
        .filter_map(|msg| {
            let text_content = events_sync::extract_text_content(&msg.message.content);
            // Detect special content types from JSONL content blocks
            let content_types: Vec<&str> = msg.message.content.as_array()
                .map(|arr| arr.iter()
                    .filter_map(|b| b.get("type").and_then(|t| t.as_str()))
                    .collect())
                .unwrap_or_default();
            let is_tool_result = !content_types.is_empty() && content_types.iter().all(|t| *t == "tool_result");
            let is_thinking = !content_types.is_empty() && content_types.iter().all(|t| *t == "thinking");
            // Keep tool_result and thinking messages even if content extraction is empty
            if text_content.is_empty() && !is_tool_result {
                return None;
            }
            let content = if text_content.is_empty() {
                "[tool_result]".to_string()
            } else {
                text_content
            };
            let role = if is_tool_result {
                "tool_result".to_string()
            } else if is_thinking {
                "thinking".to_string()
            } else if msg.message.role == "user" && is_slot_session {
                // Slot sessions: "user" messages are daemon-sent system prompts, not the human
                "system".to_string()
            } else {
                msg.message.role.clone()
            };
            let raw_content = events_sync::sanitize_raw_content(&msg.message.content);
            Some(missiond_core::types::ConversationMessage {
                id: 0,
                session_id: session_id.clone(),
                role,
                content,
                raw_content,
                message_uuid: Some(msg.uuid.clone()),
                parent_uuid: msg.parent_uuid.clone(),
                model: msg.message.model.clone(),
                timestamp: msg.timestamp.clone(),
                metadata: None,
            })
        })
        .collect();

    match db.insert_conversation_messages_batch(&batch) {
        Ok(inserted_ids) if !inserted_ids.is_empty() => {
            info!(session = %session_id, count = inserted_ids.len(), "Logged conversation messages");

            // Emit timeline events for user/assistant messages with visible text.
            // Match inserted DB IDs back to original JSONL messages for text-only preview.
            if !is_pty {
                for &msg_id in &inserted_ids {
                    if let Ok(Some(db_msg)) = db.get_conversation_message_by_id(msg_id) {
                        if db_msg.role != "user" && db_msg.role != "assistant" { continue; }
                        // Find original JSONL message to extract visible-only text
                        let visible_text = messages.iter()
                            .find(|m| db_msg.message_uuid.as_deref() == Some(&m.uuid))
                            .map(|m| events_sync::extract_visible_text(&m.message.content))
                            .unwrap_or_default();
                        if visible_text.is_empty() { continue; } // skip pure tool_use messages
                        let preview = if visible_text.len() > 200 {
                            let mut end = 200;
                            while end > 0 && !visible_text.is_char_boundary(end) { end -= 1; }
                            format!("{}...", &visible_text[..end])
                        } else {
                            visible_text
                        };
                        state.event_bus.publish_traced(
                            DaemonEvent::ConversationMessageLogged {
                                message_id: msg_id,
                                session_id: session_id.clone(),
                                role: db_msg.role.clone(),
                                content_chars: db_msg.content.len(),
                                preview,
                            },
                            TraceContext {
                                trace_id: Some(session_id.clone()),
                                ..Default::default()
                            },
                        );
                    }
                }
            }
        }
        Err(e) => {
            error!(session = %session_id, error = %e, "Failed to insert conversation messages");
        }
        _ => {}
    }

    // ── Audit: extract tool_use/tool_result into conversation_tool_calls ──
    {
        let mut tool_calls = Vec::new();
        let mut tool_results = Vec::new();
        for msg in &messages {
            let role = &msg.message.role;
            let content = &msg.message.content;
            if role == "assistant" {
                tool_calls.extend(events_sync::extract_tool_calls_from_assistant(
                    &session_id,
                    &msg.timestamp,
                    content,
                ));
            } else if role == "user" {
                tool_results.extend(events_sync::extract_tool_results_from_user(content));
            }
        }
        if !tool_calls.is_empty() {
            match db.insert_tool_calls_batch(&tool_calls) {
                Ok(count) if count > 0 => {
                    info!(session = %session_id, count, "Extracted tool calls for audit");
                }
                Err(e) => {
                    warn!(session = %session_id, error = %e, "Failed to insert tool calls");
                }
                _ => {}
            }
        }
        for (tool_use_id, summary, raw, status) in &tool_results {
            if let Err(e) = db.update_tool_call_output(tool_use_id, summary, raw, status) {
                warn!(tool_use_id, error = %e, "Failed to update tool call output");
            }
        }
    }

    // ── Token usage ledger ─────────────────────────────────────────
    // Extract usage from assistant messages and write to append-only ledger.
    let slot_task_id = slot_id.as_deref()
        .and_then(|sid| db.get_running_slot_task(sid).ok().flatten());
    for msg in &messages {
        if let Some(ref usage) = msg.message.usage {
            let total = usage.input_tokens + usage.output_tokens
                + usage.cache_creation_input_tokens + usage.cache_read_input_tokens;
            if total == 0 {
                continue;
            }
            if let Err(e) = db.insert_token_usage(
                &session_id,
                slot_id.as_deref(),
                slot_task_id.as_deref(),
                msg.message.model.as_deref(),
                usage.input_tokens,
                usage.cache_creation_input_tokens,
                usage.cache_read_input_tokens,
                usage.output_tokens,
            ) {
                warn!(session = %session_id, error = %e, "Failed to insert token usage");
            }
        }
    }
}

pub(crate) fn handle_pty_text_complete(
    state: &AppState,
    slot_id: String,
    turn_id: u64,
    content: String,
    timestamp: i64,
) {
    let db = state.mission.db();

    // If this slot has a captured JSONL session UUID, JSONL provides richer data.
    // Skip inferior PTY TextComplete logging to avoid dual-write.
    if db.get_slot_session(&slot_id).unwrap_or(None).is_some() {
        return;
    }

    let session_id = format!("pty-{}", slot_id);

    // Ensure conversation exists for this PTY session
    if db.get_conversation(&session_id).unwrap_or(None).is_none() {
        let ts = chrono::DateTime::from_timestamp(timestamp, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| timestamp.to_string());
        let conv = missiond_core::types::Conversation {
            id: session_id.clone(),
            project: None,
            slot_id: Some(slot_id.clone()),
            source: "pty".to_string(),
            model: None,
            git_branch: None,
            jsonl_path: None,
            parent_session_id: None,
            task_id: None,
            message_count: 0,
            started_at: ts,
            ended_at: None,
            status: "active".to_string(),
            analyzed_at: None,
            analysis_version: 0,
            analysis_retries: 0,
            deep_analyzed_message_id: 0,
            chat_type: None,
            conversation_type: missiond_core::db::derive_conversation_type(Some(&slot_id), &session_id),
            updated_at: None,
            llm_summary: None,
            embedding_provider: None,
            session_timeline: None,
            timeline_built_at: None,
        };
        if let Err(e) = db.upsert_conversation(&conv) {
            error!(slot = %slot_id, error = %e, "Failed to create PTY conversation");
            return;
        }
    }

    let msg_uuid = format!("pty-{}-turn-{}", slot_id, turn_id);

    let ts = chrono::DateTime::from_timestamp(timestamp, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| timestamp.to_string());

    let conv_msg = missiond_core::types::ConversationMessage {
        id: 0,
        session_id: session_id.clone(),
        role: "assistant".to_string(),
        content,
        raw_content: None,
        message_uuid: Some(msg_uuid),
        parent_uuid: None,
        model: None,
        timestamp: ts,
        metadata: Some(format!("{{\"turn_id\":{}}}", turn_id)),
    };

    // INSERT OR IGNORE handles dedup via UNIQUE index on message_uuid
    match db.insert_conversation_message(&conv_msg) {
        Ok(_) => {
            info!(slot = %slot_id, turn = turn_id, "Logged PTY assistant output");
        }
        Err(e) => {
            error!(slot = %slot_id, turn = turn_id, error = %e, "Failed to insert PTY message");
        }
    }
}
