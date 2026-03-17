//! Three-Layer Conversation Log Pipeline
//!
//! Layer 1 (Ingestor): Ensure conversation exists + batch insert raw messages (zero discard)
//! Layer 2 (Classifier): Role mapping + audit extraction + rule-based labeling
//! Layer 3 (Emitter): Timeline events + token ledger + briefing wake

use tracing::{error, info, warn};

use crate::event_bus::{DaemonEvent, TraceContext};
use crate::state::AppState;
use crate::events_sync;

// ════════════════════════════════════════════════════════════════════════════
// Orchestrator: coordinates the three layers
// ════════════════════════════════════════════════════════════════════════════

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

    // Extract parent session ID for subagent conversations
    let parent_session_id = if session_id.starts_with("agent-") {
        missiond_core::db::extract_parent_session_id(&jsonl_path)
    } else {
        None
    };

    // ── Layer 1: Ingestor ──
    let inserted_ids = ingest(
        db, &session_id, &project_path, &jsonl_path, &messages,
        source, slot_id.as_deref(), parent_session_id.as_deref(),
        is_slot_session,
    );
    let inserted_ids = match inserted_ids {
        Some(ids) if !ids.is_empty() => ids,
        _ => return, // Nothing inserted, nothing to classify/emit
    };

    // ── Layer 2: Classifier (audit + labels) ──
    classify(db, &session_id, &messages, &inserted_ids);

    // ── Layer 3: Emitter (timeline events + token ledger) ──
    emit(
        state, db, &session_id, &inserted_ids, &messages,
        is_pty, slot_id.as_deref(), parent_session_id.as_deref(),
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Layer 1: Ingestor — ensure conversation + batch insert messages
// ════════════════════════════════════════════════════════════════════════════

fn ingest(
    db: &missiond_core::db::MissionDB,
    session_id: &str,
    project_path: &str,
    jsonl_path: &str,
    messages: &[missiond_core::CCMessageLine],
    source: &str,
    slot_id: Option<&str>,
    parent_session_id: Option<&str>,
    is_slot_session: bool,
) -> Option<Vec<i64>> {
    // Ensure conversation exists; re-activate if completed
    let existing_conv = db.get_conversation(session_id).unwrap_or(None);
    if let Some(ref conv) = existing_conv {
        if conv.status == "completed" {
            if let Err(e) = db.reactivate_conversation(session_id) {
                warn!(session = %session_id, error = %e, "Failed to re-activate conversation");
            } else {
                info!(session = %session_id, "Re-activated completed conversation");
            }
        }
    }
    if existing_conv.is_none() {
        let first = messages.first();
        let conv = missiond_core::types::Conversation {
            id: session_id.to_string(),
            project: Some(first.map(|m| m.cwd.clone()).unwrap_or_else(|| project_path.to_string())),
            slot_id: slot_id.map(|s| s.to_string()),
            source: source.to_string(),
            model: first.and_then(|m| m.message.model.clone()),
            git_branch: first.and_then(|m| m.git_branch.clone()),
            jsonl_path: Some(jsonl_path.to_string()),
            parent_session_id: parent_session_id.map(|s| s.to_string()),
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
            conversation_type: missiond_core::db::derive_conversation_type(slot_id, session_id),
            updated_at: None,
            llm_summary: None,
            embedding_provider: None,
            session_timeline: None,
            timeline_built_at: None,
        };
        if let Err(e) = db.upsert_conversation(&conv) {
            error!(session = %session_id, error = %e, "Failed to create conversation");
            return None;
        }
    }

    // Build message batch with storage-layer metadata
    let batch: Vec<missiond_core::types::ConversationMessage> = messages.iter()
        .filter_map(|msg| {
            let text_content = events_sync::extract_text_content(&msg.message.content);
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

            // Role mapping (will also be stored as label in Layer 2)
            let role = if is_tool_result {
                "tool_result".to_string()
            } else if is_thinking {
                "thinking".to_string()
            } else if msg.message.role == "user" && is_slot_session {
                "system".to_string()
            } else if msg.message.role == "user" && parent_session_id.is_some() {
                "agent_user".to_string()
            } else if msg.message.role == "user" && is_compact_summary(db, session_id, msg) {
                "compact_summary".to_string()
            } else {
                msg.message.role.clone()
            };

            let raw_content = events_sync::sanitize_raw_content(&msg.message.content);
            let tool_name = events_sync::extract_tool_names_csv(&msg.message.content);

            // Storage layer: structural metadata
            let has_image = content_types.iter().any(|t| *t == "image");
            let has_tool_use = content_types.iter().any(|t| *t == "tool_use");
            let has_tool_result_flag = content_types.iter().any(|t| *t == "tool_result");
            let content_types_json = if content_types.is_empty() { None } else {
                Some(serde_json::to_string(&content_types).unwrap_or_default())
            };
            let token_count = msg.message.usage.as_ref().map(|u| {
                u.input_tokens + u.output_tokens + u.cache_creation_input_tokens + u.cache_read_input_tokens
            }).filter(|&t| t > 0);

            Some(missiond_core::types::ConversationMessage {
                id: 0,
                session_id: session_id.to_string(),
                raw_role: Some(msg.message.role.clone()),
                role,
                content,
                raw_content,
                message_uuid: Some(msg.uuid.clone()),
                parent_uuid: msg.parent_uuid.clone(),
                model: msg.message.model.clone(),
                timestamp: msg.timestamp.clone(),
                metadata: None,
                tool_name,
                content_types: content_types_json,
                has_image,
                has_tool_use,
                has_tool_result: has_tool_result_flag,
                token_count,
            })
        })
        .collect();

    match db.insert_conversation_messages_batch(&batch) {
        Ok(inserted_ids) if !inserted_ids.is_empty() => {
            info!(session = %session_id, count = inserted_ids.len(), "Logged conversation messages");
            Some(inserted_ids)
        }
        Err(e) => {
            error!(session = %session_id, error = %e, "Failed to insert conversation messages");
            None
        }
        _ => None,
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Layer 2: Classifier — audit extraction + rule-based labels
// ════════════════════════════════════════════════════════════════════════════

fn classify(
    db: &missiond_core::db::MissionDB,
    session_id: &str,
    messages: &[missiond_core::CCMessageLine],
    inserted_ids: &[i64],
) {
    // ── Audit: extract tool_use/tool_result into conversation_tool_calls ──
    let mut tool_calls = Vec::new();
    let mut tool_results = Vec::new();
    for msg in messages {
        let role = &msg.message.role;
        let content = &msg.message.content;
        if role == "assistant" {
            tool_calls.extend(events_sync::extract_tool_calls_from_assistant(
                session_id,
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

    // ── Rule-based labels (synchronous, no LLM) ──
    apply_rule_labels(db, inserted_ids);
}

/// Apply rule-based labels to newly inserted messages (single batch write).
/// Labels: role_mapped, has_code_change, has_mcp_call, has_tool_use, has_tool_result, has_image
fn apply_rule_labels(db: &missiond_core::db::MissionDB, inserted_ids: &[i64]) {
    // Collect all labels first, then flush as a single batch (avoids N+1 auto-commit fsync).
    let mut role_labels: Vec<(i64, String)> = Vec::new();
    let mut flag_labels: Vec<(i64, &'static str)> = Vec::new();

    for &msg_id in inserted_ids {
        let msg = match db.get_conversation_message_by_id(msg_id) {
            Ok(Some(m)) => m,
            _ => continue,
        };

        if let Some(ref raw_role) = msg.raw_role {
            if raw_role != &msg.role {
                role_labels.push((msg_id, msg.role.clone()));
            }
        }

        if let Some(ref tn) = msg.tool_name {
            if tn.contains("Write") || tn.contains("Edit") {
                flag_labels.push((msg_id, "has_code_change"));
            }
            if tn.contains("mcp__") {
                flag_labels.push((msg_id, "has_mcp_call"));
            }
        }

        if msg.has_tool_result { flag_labels.push((msg_id, "has_tool_result")); }
        if msg.has_tool_use   { flag_labels.push((msg_id, "has_tool_use")); }
        if msg.has_image      { flag_labels.push((msg_id, "has_image")); }
    }

    // Build batch: Vec<(msg_id, label, value, source)>
    let mut batch: Vec<(i64, &str, String, &str)> = Vec::with_capacity(role_labels.len() + flag_labels.len());
    for (msg_id, role) in &role_labels {
        batch.push((*msg_id, "role_mapped", role.clone(), "rule"));
    }
    for (msg_id, label) in &flag_labels {
        batch.push((*msg_id, label, "true".to_string(), "rule"));
    }

    if !batch.is_empty() {
        // Convert to borrowed tuple slice for label_set_batch
        let refs: Vec<(i64, &str, &str, &str)> = batch.iter()
            .map(|(id, l, v, s)| (*id, *l, v.as_str(), *s))
            .collect();
        match db.label_set_batch(&refs) {
            Ok(count) => {
                tracing::debug!(count, "Applied rule-based labels (batch)");
            }
            Err(e) => {
                warn!(error = %e, "Failed to apply rule-based labels");
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Layer 3: Emitter — timeline events + token ledger + briefing
// ════════════════════════════════════════════════════════════════════════════

fn emit(
    state: &AppState,
    db: &missiond_core::db::MissionDB,
    session_id: &str,
    inserted_ids: &[i64],
    messages: &[missiond_core::CCMessageLine],
    is_pty: bool,
    slot_id: Option<&str>,
    parent_session_id: Option<&str>,
) {
    // ── Timeline events for conversation messages ──
    for &msg_id in inserted_ids {
        if let Ok(Some(db_msg)) = db.get_conversation_message_by_id(msg_id) {
            let emit_role = if is_pty {
                matches!(db_msg.role.as_str(), "system" | "assistant" | "thinking")
            } else {
                matches!(db_msg.role.as_str(), "user" | "assistant" | "thinking")
            };
            if !emit_role { continue; }

            let orig_msg = messages.iter()
                .find(|m| db_msg.message_uuid.as_deref() == Some(&m.uuid));

            let preview = if db_msg.role == "thinking" {
                let text = &db_msg.content;
                if text.len() > 200 {
                    let mut end = 200;
                    while end > 0 && !text.is_char_boundary(end) { end -= 1; }
                    format!("{}...", &text[..end])
                } else {
                    text.clone()
                }
            } else {
                let visible_text = orig_msg
                    .map(|m| events_sync::extract_visible_text(&m.message.content))
                    .unwrap_or_default();
                if visible_text.is_empty() {
                    let tool_names = orig_msg
                        .map(|m| events_sync::extract_tool_names(&m.message.content))
                        .unwrap_or_default();
                    if tool_names.is_empty() { continue; }
                    format!("[{}]", tool_names.join(", "))
                } else if visible_text.len() > 200 {
                    let mut end = 200;
                    while end > 0 && !visible_text.is_char_boundary(end) { end -= 1; }
                    format!("{}...", &visible_text[..end])
                } else {
                    visible_text
                }
            };

            let content_chars = db_msg.content.len();
            let msg_span_id = uuid::Uuid::new_v4().to_string();
            state.event_bus.publish_traced(
                DaemonEvent::ConversationMessageLogged {
                    message_id: msg_id,
                    session_id: session_id.to_string(),
                    parent_session_id: parent_session_id.map(|s| s.to_string()),
                    slot_id: slot_id.map(|s| s.to_string()),
                    role: db_msg.role.clone(),
                    content_chars,
                    preview,
                },
                TraceContext {
                    trace_id: Some(session_id.to_string()),
                    span_id: Some(msg_span_id.clone()),
                    ..Default::default()
                },
            );
            if db_msg.role == "assistant" {
                state.last_msg_span.lock().unwrap()
                    .insert(session_id.to_string(), msg_span_id);
            }
            if content_chars > 300 {
                state.briefing_notify.notify_one();
            }
        }
    }

    // ── Auto-instrumentation: high-value tool completions ──
    emit_tool_completions(state, db, session_id, messages, slot_id);

    // ── Token usage ledger ──
    emit_token_usage(db, session_id, messages, slot_id);
}

/// Emit timeline events for high-value tool completions (Bash, Write, Edit, MCP).
fn emit_tool_completions(
    state: &AppState,
    db: &missiond_core::db::MissionDB,
    session_id: &str,
    messages: &[missiond_core::CCMessageLine],
    slot_id: Option<&str>,
) {
    const HIGH_VALUE_TOOLS: &[&str] = &["Bash", "Write", "Edit"];
    let mut tool_results = Vec::new();
    for msg in messages {
        if msg.message.role == "user" {
            tool_results.extend(events_sync::extract_tool_results_from_user(&msg.message.content));
        }
    }
    for (tool_use_id, summary, _raw, status) in &tool_results {
        if let Ok(Some(tc)) = db.get_tool_call_by_id(tool_use_id) {
            let is_high_value = HIGH_VALUE_TOOLS.iter().any(|t| tc.tool_name == *t)
                || tc.tool_name.starts_with("mcp__");
            if is_high_value {
                let is_error = status == "error";
                let tool_summary = {
                    let s = format!("{}: {}", tc.tool_name, summary);
                    if s.len() > 200 {
                        let mut end = 200;
                        while end > 0 && !s.is_char_boundary(end) { end -= 1; }
                        format!("{}...", &s[..end])
                    } else {
                        s
                    }
                };
                state.event_bus.publish_traced(
                    DaemonEvent::ToolCompleted {
                        session_id: session_id.to_string(),
                        slot_id: slot_id.map(|s| s.to_string()),
                        tool_name: tc.tool_name.clone(),
                        status: status.clone(),
                        is_error,
                        input_summary: tc.input_summary.clone(),
                        output_summary: summary.clone(),
                    },
                    TraceContext {
                        trace_id: Some(session_id.to_string()),
                        span_id: Some(tool_use_id.clone()),
                        summary: Some(tool_summary),
                        ..Default::default()
                    },
                );
            }
        }
    }
}

/// Write token usage to append-only ledger.
fn emit_token_usage(
    db: &missiond_core::db::MissionDB,
    session_id: &str,
    messages: &[missiond_core::CCMessageLine],
    slot_id: Option<&str>,
) {
    let slot_task_id = slot_id
        .and_then(|sid| db.get_running_slot_task(sid).ok().flatten());
    for msg in messages {
        if let Some(ref usage) = msg.message.usage {
            let total = usage.input_tokens + usage.output_tokens
                + usage.cache_creation_input_tokens + usage.cache_read_input_tokens;
            if total == 0 {
                continue;
            }
            if let Err(e) = db.insert_token_usage(
                session_id,
                slot_id,
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

// ════════════════════════════════════════════════════════════════════════════
// PTY text-complete handler (fallback for non-JSONL slots)
// ════════════════════════════════════════════════════════════════════════════

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
        raw_role: Some("assistant".to_string()),
        role: "assistant".to_string(),
        content,
        raw_content: None,
        message_uuid: Some(msg_uuid),
        parent_uuid: None,
        model: None,
        timestamp: ts,
        metadata: Some(format!("{{\"turn_id\":{}}}", turn_id)),
        tool_name: None,
        content_types: None,
        has_image: false,
        has_tool_use: false,
        has_tool_result: false,
        token_count: None,
    };

    match db.insert_conversation_message(&conv_msg) {
        Ok(_) => {
            info!(slot = %slot_id, turn = turn_id, "Logged PTY assistant output");
        }
        Err(e) => {
            error!(slot = %slot_id, turn = turn_id, error = %e, "Failed to insert PTY message");
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Compact Summary Detection
// ════════════════════════════════════════════════════════════════════════════

const COMPACT_PREFIX: &str = "This session is being continued from a previous conversation";

/// Detect if a user message is actually a compact summary injected by Claude Code.
/// Primary: parentUuid matches a compact_boundary event UUID (structural).
/// Fallback: content starts with the known compact summary prefix (heuristic).
fn is_compact_summary(
    db: &missiond_core::db::MissionDB,
    session_id: &str,
    msg: &missiond_core::CCMessageLine,
) -> bool {
    let text = events_sync::extract_text_content(&msg.message.content);
    if !text.starts_with(COMPACT_PREFIX) {
        return false;
    }

    // Text matched — warn if compact_boundary event is missing (race condition or data gap)
    let has_boundary = msg.parent_uuid.as_ref()
        .and_then(|uuid| db.is_compact_boundary_event(session_id, uuid).ok())
        .unwrap_or(false);

    if !has_boundary {
        warn!(
            session = %session_id,
            "Compact summary detected by text pattern only — compact_boundary event not found"
        );
    }

    true
}
