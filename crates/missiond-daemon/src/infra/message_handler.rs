//! Three-Layer Conversation Log Pipeline
//!
//! Layer 1 (Ingestor): Ensure conversation exists + batch insert raw messages (zero discard)
//! Layer 2 (Classifier): Role mapping + audit extraction + rule-based labeling
//! Layer 3 (Emitter): Timeline events + token ledger + briefing wake

use std::collections::HashSet;
use tracing::{error, info, warn};

use crate::events_sync;
use crate::state::AppState;
use missiond_core::event::events::{MessageEvent, SystemEvent};

// ════════════════════════════════════════════════════════════════════════════
// Orchestrator: coordinates the three layers
// ════════════════════════════════════════════════════════════════════════════

pub(crate) async fn handle_new_messages(
    state: &AppState,
    session_id: String,
    project_path: String,
    jsonl_path: String,
    messages: Vec<missiond_core::CCMessageLine>,
    route: &crate::infra::ingestion_router::IngestionRoute,
) -> bool {
    // All routing decisions come from the IngestionRoute (the "waybill").
    // This handler is a "blind executor" — it does not make classification decisions.
    let source = route.source.as_str();
    let slot_id = route.slot_id.as_deref();
    let is_slot_session = slot_id.is_some();
    let is_pty = route.is_pty;
    let parent_session_id = route.parent_session_id.as_deref();

    // ── Layer 1: Ingestor ──
    let ingest_result = ingest(
        state,
        &session_id,
        &project_path,
        &jsonl_path,
        &messages,
        source,
        slot_id,
        parent_session_id,
        is_slot_session,
        &route.conversation_type,
    )
    .await;
    let IngestResult {
        inserted_ids,
        inserted_uuids,
        uuid_to_id,
        semantic_roles,
    } = match ingest_result {
        IngestOutcome::Inserted(r) => r,
        IngestOutcome::AlreadyDurable | IngestOutcome::NoPersistableMessages => {
            return true;
        }
        IngestOutcome::Failed => {
            return false;
        }
    };

    // ── Layer 2: Classifier (audit + labels) ──
    classify(
        state,
        &session_id,
        &messages,
        &inserted_ids,
        &semantic_roles,
    )
    .await;

    // ── Layer 3: Emitter (timeline events + token ledger) ──
    emit(
        state,
        &session_id,
        &inserted_ids,
        &inserted_uuids,
        &uuid_to_id,
        &messages,
        is_pty,
        slot_id,
        parent_session_id,
    )
    .await;
    true
}

// ════════════════════════════════════════════════════════════════════════════
// Layer 1: Ingestor — ensure conversation + batch insert messages
// ════════════════════════════════════════════════════════════════════════════

/// Result of Layer 1 ingestion: DB IDs + UUIDs of newly inserted messages.
struct IngestResult {
    inserted_ids: Vec<i64>,
    /// UUIDs of messages that were actually new (not ON CONFLICT skipped).
    /// Used by Layer 3 to filter emit_tool_completions/emit_token_usage
    /// so they only process new messages, preventing duplicate side effects.
    inserted_uuids: HashSet<String>,
    /// UUID → DB ID mapping for token_usage_ledger message_id dedup.
    uuid_to_id: std::collections::HashMap<String, i64>,
    /// Semantic roles mapped by message UUID
    semantic_roles: std::collections::HashMap<String, String>,
}

enum IngestOutcome {
    Inserted(IngestResult),
    /// The source cursor can advance because every persistable message UUID is
    /// already present in the durable store.
    AlreadyDurable,
    /// The source batch carried no conversation rows after structural filtering.
    NoPersistableMessages,
    Failed,
}

async fn ingest(
    state: &AppState,
    session_id: &str,
    project_path: &str,
    jsonl_path: &str,
    messages: &[missiond_core::CCMessageLine],
    source: &str,
    slot_id: Option<&str>,
    parent_session_id: Option<&str>,
    is_slot_session: bool,
    conversation_type: &str,
) -> IngestOutcome {
    // Ensure conversation exists; re-activate if completed
    let existing_conv = state
        .store
        .get_conversation(session_id)
        .await
        .unwrap_or(None);
    if let Some(ref conv) = existing_conv {
        if conv.status == "completed" {
            if let Err(e) = state.store.reactivate_conversation(session_id).await {
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
            project: Some(
                first
                    .map(|m| m.cwd.clone())
                    .unwrap_or_else(|| project_path.to_string()),
            ),
            project_id: {
                let cwd = first.map(|m| m.cwd.as_str()).unwrap_or(project_path);
                let registry = state.project_registry.read().await;
                registry.resolve(cwd).map(|s| s.to_string())
            },
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
            chat_type: match source {
                "claude_code" | "pty_jsonl" => None, // default "pty"
                other => Some(other.to_string()),    // "gemini_cli", "codex_cli", etc.
            },
            conversation_type: conversation_type.to_string(),
            updated_at: None,
            llm_summary: None,
            embedding_provider: None,
            session_timeline: None,
            timeline_built_at: None,
            user_id: None,
            tenant_id: None,
            application_id: None,
            channel: "cli".to_string(),
            topic_id: None,
            topic_label: None,
            context_capsule_hash: None,
        };
        if let Err(e) = state.store.upsert_conversation(&conv).await {
            error!(session = %session_id, error = %e, "Failed to create conversation");
            return IngestOutcome::Failed;
        }
    }

    // Build message batch with storage-layer metadata
    let mut batch: Vec<missiond_core::types::ConversationMessage> = Vec::new();
    let mut semantic_roles = std::collections::HashMap::new();
    for msg in messages {
        let text_content = events_sync::extract_text_content(&msg.message.content);
        let content_types: Vec<&str> = msg
            .message
            .content
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|b| b.get("type").and_then(|t| t.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        let is_tool_result =
            !content_types.is_empty() && content_types.iter().all(|t| *t == "tool_result");

        // Keep tool_result and thinking messages even if content extraction is empty
        if text_content.is_empty() && !is_tool_result {
            continue;
        }
        let content = if text_content.is_empty() {
            "[tool_result]".to_string()
        } else {
            text_content
        };

        let is_agent_sidechain =
            msg.is_sidechain || (parent_session_id.is_some() && session_id.starts_with("agent-"));
        let is_compact =
            msg.message.role == "user" && is_compact_summary(state, session_id, msg).await;
        let role = events_sync::normalize_claude_message_role(
            &msg.message.role,
            &content_types,
            is_slot_session,
            is_interactive_conversation(conversation_type),
            is_agent_sidechain,
            is_compact,
        );

        if role != msg.message.role {
            semantic_roles.insert(msg.uuid.clone(), role.clone());
        }

        let raw_content = events_sync::sanitize_raw_content(&msg.message.content);
        let tool_name = events_sync::extract_tool_names_csv(&msg.message.content);

        // Storage layer: structural metadata
        let has_image = content_types.iter().any(|t| *t == "image");
        let has_tool_use = content_types.iter().any(|t| *t == "tool_use");
        let has_tool_result_flag = content_types.iter().any(|t| *t == "tool_result");
        let content_types_json = if content_types.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&content_types).unwrap_or_default())
        };
        let token_count = msg
            .message
            .usage
            .as_ref()
            .map(|u| {
                u.input_tokens
                    + u.output_tokens
                    + u.cache_creation_input_tokens
                    + u.cache_read_input_tokens
            })
            .filter(|&t| t > 0);

        let metadata = msg.message.stop_reason.as_ref().map(|stop_reason| {
            serde_json::json!({
                "provider": "claude_code",
                "stop_reason": stop_reason,
            })
            .to_string()
        });

        batch.push(missiond_core::types::ConversationMessage {
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
            metadata,
            tool_name,
            content_types: content_types_json,
            has_image,
            has_tool_use,
            has_tool_result: has_tool_result_flag,
            token_count,
            seq: None,
            role_display: None,
        });
    }

    if batch.is_empty() {
        return IngestOutcome::NoPersistableMessages;
    }

    match state.store.insert_conversation_messages_batch(&batch).await {
        Ok(inserted_ids) if !inserted_ids.is_empty() => {
            info!(session = %session_id, count = inserted_ids.len(), "Logged conversation messages");

            // Collect UUIDs + ID mapping of newly inserted messages for emit-layer filtering.
            // Point lookups on PK — trivially fast for typical batch sizes (5-20).
            let mut inserted_uuids = HashSet::with_capacity(inserted_ids.len());
            let mut uuid_to_id = std::collections::HashMap::with_capacity(inserted_ids.len());
            for &id in &inserted_ids {
                if let Ok(Some(msg)) = state.store.get_conversation_message_by_id(id).await {
                    if let Some(uuid) = msg.message_uuid {
                        inserted_uuids.insert(uuid.clone());
                        uuid_to_id.insert(uuid, id);
                    }
                }
            }

            IngestOutcome::Inserted(IngestResult {
                inserted_ids,
                inserted_uuids,
                uuid_to_id,
                semantic_roles,
            })
        }
        Err(e) => {
            error!(session = %session_id, error = %e, "Failed to insert conversation messages");
            IngestOutcome::Failed
        }
        Ok(_) => {
            let mut uuids: Vec<&str> = batch
                .iter()
                .filter_map(|msg| msg.message_uuid.as_deref())
                .collect();
            uuids.sort_unstable();
            uuids.dedup();
            if uuids.is_empty() {
                return IngestOutcome::Failed;
            }
            match state.store.check_message_uuids_exist(&uuids).await {
                Ok(existing) if existing.len() == uuids.len() => IngestOutcome::AlreadyDurable,
                Ok(existing) => {
                    warn!(
                        session = %session_id,
                        expected = uuids.len(),
                        found = existing.len(),
                        "Conversation batch inserted no rows but not all UUIDs are durable"
                    );
                    IngestOutcome::Failed
                }
                Err(e) => {
                    warn!(session = %session_id, error = %e, "Failed to verify duplicate conversation UUIDs");
                    IngestOutcome::Failed
                }
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Layer 2: Classifier — audit extraction + rule-based labels
// ════════════════════════════════════════════════════════════════════════════

async fn classify(
    state: &AppState,
    session_id: &str,
    messages: &[missiond_core::CCMessageLine],
    inserted_ids: &[i64],
    semantic_roles: &std::collections::HashMap<String, String>,
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
        } else if role == "user" || role == "tool_result" {
            tool_results.extend(events_sync::extract_tool_results_from_user(content));
        }
    }
    if !tool_calls.is_empty() {
        match state.store.insert_tool_calls_batch(&tool_calls).await {
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
        if let Err(e) = state
            .store
            .update_tool_call_output(tool_use_id, summary, raw, status)
            .await
        {
            warn!(tool_use_id, error = %e, "Failed to update tool call output");
        }
    }

    // ── Rule-based labels (synchronous, no LLM) ──
    apply_rule_labels(state, inserted_ids, semantic_roles).await;
}

/// Apply rule-based labels to newly inserted messages (single batch write).
/// Labels: role_mapped, has_code_change, has_mcp_call, has_tool_use, has_tool_result, has_image,
///         gemini_chat (send/receive), gemini_channel (apikey/google)
async fn apply_rule_labels(
    state: &AppState,
    inserted_ids: &[i64],
    semantic_roles: &std::collections::HashMap<String, String>,
) {
    // Collect all labels first, then flush as a single batch (avoids N+1 auto-commit fsync).
    let mut role_labels: Vec<(i64, String)> = Vec::new();
    let mut flag_labels: Vec<(i64, &'static str, &'static str)> = Vec::new();
    let mut string_labels: Vec<(i64, &'static str, String)> = Vec::new();

    for &msg_id in inserted_ids {
        let msg = match state.store.get_conversation_message_by_id(msg_id).await {
            Ok(Some(m)) => m,
            _ => continue,
        };

        if let Some(uuid) = &msg.message_uuid {
            if let Some(semantic_role) = semantic_roles.get(uuid) {
                string_labels.push((msg_id, "semantic_role", semantic_role.clone()));
            }
        }

        if let Some(ref raw_role) = msg.raw_role {
            if raw_role != &msg.role {
                role_labels.push((msg_id, msg.role.clone()));
            }
        }

        if let Some(ref tn) = msg.tool_name {
            if tn.contains("Write") || tn.contains("Edit") {
                flag_labels.push((msg_id, "has_code_change", "true"));
            }
            if tn.contains("mcp__") {
                flag_labels.push((msg_id, "has_mcp_call", "true"));
            }
            // Gemini chat tagging
            if tn.contains("mission_router_chat") {
                if msg.has_tool_use {
                    flag_labels.push((msg_id, "gemini_chat", "send"));
                    let channel = extract_gemini_channel(&msg.raw_content);
                    string_labels.push((msg_id, "gemini_channel", channel));
                }
                if msg.has_tool_result {
                    flag_labels.push((msg_id, "gemini_chat", "receive"));
                }
            }
        }

        if msg.has_tool_result {
            flag_labels.push((msg_id, "has_tool_result", "true"));
        }
        if msg.has_tool_use {
            flag_labels.push((msg_id, "has_tool_use", "true"));
        }
        if msg.has_image {
            flag_labels.push((msg_id, "has_image", "true"));
        }
    }

    // Build batch: Vec<(msg_id, label, value, source)>
    let mut batch: Vec<(i64, &str, String, &str)> =
        Vec::with_capacity(role_labels.len() + flag_labels.len() + string_labels.len());
    for (msg_id, role) in &role_labels {
        batch.push((*msg_id, "role_mapped", role.clone(), "rule"));
    }
    for (msg_id, label, value) in &flag_labels {
        batch.push((*msg_id, label, value.to_string(), "rule"));
    }
    for (msg_id, label, value) in &string_labels {
        batch.push((*msg_id, label, value.clone(), "rule"));
    }

    if !batch.is_empty() {
        // Convert to borrowed tuple slice for label_set_batch
        let refs: Vec<(i64, &str, &str, &str)> = batch
            .iter()
            .map(|(id, l, v, s)| (*id, *l, v.as_str(), *s))
            .collect();
        match state.store.label_set_batch(&refs).await {
            Ok(count) => {
                tracing::debug!(count, "Applied rule-based labels (batch)");
            }
            Err(e) => {
                warn!(error = %e, "Failed to apply rule-based labels");
            }
        }
    }
}

/// Interactive conversation types where "user" role messages are real human input.
/// Automated slot sessions keep provider `raw_role=user`, but MissionD exposes
/// them as `worker_user` so Logs can distinguish worker prompts from humans
/// without burying them under generic system noise.
fn is_interactive_conversation(conversation_type: &str) -> bool {
    matches!(conversation_type, "chat" | "jarvis" | "user")
}

/// Extract Gemini channel from raw_content JSON (tool_use input).
/// Returns "apikey" or "google" (default if unspecified).
fn extract_gemini_channel(raw_content: &Option<String>) -> String {
    #[derive(serde::Deserialize)]
    struct ContentBlock {
        #[serde(default)]
        input: Option<serde_json::Value>,
    }

    raw_content
        .as_ref()
        .and_then(|c| serde_json::from_str::<Vec<ContentBlock>>(c).ok())
        .and_then(|blocks| {
            blocks
                .iter()
                .find_map(|b| b.input.as_ref()?.get("channel")?.as_str().map(String::from))
        })
        .unwrap_or_else(|| "google".to_string())
}

// ════════════════════════════════════════════════════════════════════════════
// Layer 3: Emitter — timeline events + token ledger + briefing
// ════════════════════════════════════════════════════════════════════════════

async fn emit(
    state: &AppState,
    session_id: &str,
    inserted_ids: &[i64],
    inserted_uuids: &HashSet<String>,
    uuid_to_id: &std::collections::HashMap<String, i64>,
    messages: &[missiond_core::CCMessageLine],
    is_pty: bool,
    slot_id: Option<&str>,
    parent_session_id: Option<&str>,
) {
    // ── Timeline events for conversation messages ──
    for &msg_id in inserted_ids {
        if let Ok(Some(db_msg)) = state.store.get_conversation_message_by_id(msg_id).await {
            let emit_role = if is_pty {
                matches!(db_msg.role.as_str(), "system" | "assistant" | "thinking")
            } else {
                matches!(db_msg.role.as_str(), "user" | "assistant" | "thinking")
            };
            if !emit_role {
                continue;
            }

            let orig_msg = messages
                .iter()
                .find(|m| db_msg.message_uuid.as_deref() == Some(&m.uuid));

            let preview = if db_msg.role == "thinking" {
                let text = &db_msg.content;
                if text.len() > 200 {
                    let mut end = 200;
                    while end > 0 && !text.is_char_boundary(end) {
                        end -= 1;
                    }
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
                    if tool_names.is_empty() {
                        continue;
                    }
                    format!("[{}]", tool_names.join(", "))
                } else if visible_text.len() > 200 {
                    let mut end = 200;
                    while end > 0 && !visible_text.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}...", &visible_text[..end])
                } else {
                    visible_text
                }
            };

            let content_chars = db_msg.content.len();
            // Use deterministic span_id based on message_uuid so that
            // retries/replays produce the same span_id (enables future UNIQUE dedup).
            let msg_span_id = db_msg
                .message_uuid
                .clone()
                .unwrap_or_else(|| format!("msg-{}", msg_id));
            let _ = state
                .bus
                .publish_message(MessageEvent::Logged {
                    message_id: msg_id,
                    session_id: session_id.to_string(),
                    parent_session_id: parent_session_id.map(|s| s.to_string()),
                    slot_id: slot_id.map(|s| s.to_string()),
                    role: db_msg.role.clone(),
                    content_chars,
                    preview,
                })
                .await;
            if db_msg.role == "assistant" {
                state
                    .last_msg_span
                    .lock()
                    .unwrap()
                    .insert(session_id.to_string(), msg_span_id);
            }
            // Real-time message-level embedding: pre-filter before clone (v2 hot-path optimization)
            if !crate::workers::sonnet::embedding_worker::should_skip_fast(
                &db_msg.role,
                db_msg.content.len(),
            ) {
                let _ = state
                    .embedding_tx
                    .try_send(crate::state::EmbeddingTask::ProcessMessage {
                        message_id: msg_id,
                        session_id: session_id.to_string(),
                        role: db_msg.role.clone(),
                        content: db_msg.content.clone(),
                    });
            }
            // v1.3.0 SSOT cutover: briefing_worker removed (see
            // intent-event-bus-execution.lisp D015). Long-message hook is gone.
        }
    }

    // Filter to only newly inserted messages for side-effect functions.
    // This prevents duplicate tool_completed events and token_usage records
    // when a batch contains a mix of new and already-seen messages (e.g., after
    // reconciliation or watcher cursor loss).
    let new_messages: Vec<&missiond_core::CCMessageLine> = messages
        .iter()
        .filter(|m| inserted_uuids.contains(&m.uuid))
        .collect();

    // ── Auto-instrumentation: high-value tool completions ──
    emit_tool_completions(state, session_id, &new_messages, slot_id).await;

    // ── Token usage ledger ──
    emit_token_usage(state, session_id, &new_messages, slot_id, uuid_to_id).await;
}

/// Emit timeline events for high-value tool completions (Bash, Write, Edit, MCP).
async fn emit_tool_completions(
    state: &AppState,
    session_id: &str,
    messages: &[&missiond_core::CCMessageLine],
    slot_id: Option<&str>,
) {
    const HIGH_VALUE_TOOLS: &[&str] = &["Bash", "Write", "Edit"];
    let mut tool_results = Vec::new();
    for msg in messages {
        if msg.message.role == "user" || msg.message.role == "tool_result" {
            tool_results.extend(events_sync::extract_tool_results_from_user(
                &msg.message.content,
            ));
        }
    }
    for (tool_use_id, summary, _raw, status) in &tool_results {
        if let Ok(Some(tc)) = state.store.get_tool_call_by_id(tool_use_id).await {
            let is_high_value = HIGH_VALUE_TOOLS.iter().any(|t| tc.tool_name == *t)
                || tc.tool_name.starts_with("mcp__");
            if is_high_value {
                let is_error = status == "error";
                let tool_summary = {
                    let s = format!("{}: {}", tc.tool_name, summary);
                    if s.len() > 200 {
                        let mut end = 200;
                        while end > 0 && !s.is_char_boundary(end) {
                            end -= 1;
                        }
                        format!("{}...", &s[..end])
                    } else {
                        s
                    }
                };
                let _ = state
                    .bus
                    .publish_system(SystemEvent::ToolCompleted {
                        session_id: session_id.to_string(),
                        slot_id: slot_id.map(|s| s.to_string()),
                        tool_name: tc.tool_name.clone(),
                        status: status.clone(),
                        is_error,
                        input_summary: tc.input_summary.clone(),
                        output_summary: summary.clone(),
                    })
                    .await;
                let _ = tool_summary; // drop unused var to keep warnings quiet.
                let _ = tool_use_id;
            }
        }
    }
}

/// Write token usage to append-only ledger.
async fn emit_token_usage(
    state: &AppState,
    session_id: &str,
    messages: &[&missiond_core::CCMessageLine],
    slot_id: Option<&str>,
    uuid_to_id: &std::collections::HashMap<String, i64>,
) {
    let slot_task_id = match slot_id {
        Some(sid) => state.store.get_running_slot_task(sid).await.ok().flatten(),
        None => None,
    };
    for msg in messages {
        if let Some(ref usage) = msg.message.usage {
            let total = usage.input_tokens
                + usage.output_tokens
                + usage.cache_creation_input_tokens
                + usage.cache_read_input_tokens;
            if total == 0 {
                continue;
            }
            // Look up DB message_id via uuid→id mapping (built in ingest)
            let db_msg_id = uuid_to_id.get(&msg.uuid).copied();
            if let Err(e) = state
                .store
                .insert_token_usage(
                    session_id,
                    slot_id,
                    slot_task_id.as_deref(),
                    msg.message.model.as_deref(),
                    usage.input_tokens,
                    usage.cache_creation_input_tokens,
                    usage.cache_read_input_tokens,
                    usage.output_tokens,
                    db_msg_id,
                )
                .await
            {
                warn!(session = %session_id, error = %e, "Failed to insert token usage");
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// PTY text-complete handler (fallback for non-JSONL slots)
// ════════════════════════════════════════════════════════════════════════════

pub(crate) async fn handle_pty_text_complete(
    state: &AppState,
    slot_id: String,
    turn_id: u64,
    content: String,
    timestamp: i64,
) {
    // If this slot has a captured JSONL session UUID, JSONL provides richer data.
    // Skip inferior PTY TextComplete logging to avoid dual-write.
    if state
        .store
        .get_slot_session(&slot_id)
        .await
        .unwrap_or(None)
        .is_some()
    {
        return;
    }

    let session_id = format!("pty-{}", slot_id);
    let (source, chat_type) = match state
        .mission
        .get_slot(&slot_id)
        .map(|slot| slot.config.engine)
    {
        Some(engine) => {
            let source = crate::slot_orchestrator::canonical_source_for_engine(engine).to_string();
            let chat_type = crate::slot_orchestrator::chat_type_for_source(&source);
            (source, chat_type)
        }
        None => ("claude_code".to_string(), None),
    };
    let conversation_type = {
        let category = state.mission.get_slot_category(&slot_id);
        missiond_core::db::derive_conversation_type(
            category.as_deref(),
            Some(&slot_id),
            &session_id,
            &source,
        )
    };

    // Ensure conversation exists for this PTY session
    if let Some(existing) = state
        .store
        .get_conversation(&session_id)
        .await
        .unwrap_or(None)
    {
        if existing.source != source || existing.conversation_type != conversation_type {
            let mut updated = existing;
            updated.source = source.clone();
            updated.chat_type = chat_type.clone();
            updated.conversation_type = conversation_type.clone();
            if let Err(e) = state.store.upsert_conversation(&updated).await {
                error!(slot = %slot_id, error = %e, "Failed to update PTY conversation source");
                return;
            }
        }
    } else {
        let ts = chrono::DateTime::from_timestamp(timestamp, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| timestamp.to_string());
        let conv = missiond_core::types::Conversation {
            id: session_id.clone(),
            project: None,
            project_id: None,
            slot_id: Some(slot_id.clone()),
            source: source.clone(),
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
            chat_type: chat_type.clone(),
            conversation_type: conversation_type.clone(),
            updated_at: None,
            llm_summary: None,
            embedding_provider: None,
            session_timeline: None,
            timeline_built_at: None,
            user_id: None,
            tenant_id: None,
            application_id: None,
            channel: "cli".to_string(),
            topic_id: None,
            topic_label: None,
            context_capsule_hash: None,
        };
        if let Err(e) = state.store.upsert_conversation(&conv).await {
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
        seq: None,
        role_display: None,
    };

    match state.store.insert_conversation_message(&conv_msg).await {
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
async fn is_compact_summary(
    state: &AppState,
    session_id: &str,
    msg: &missiond_core::CCMessageLine,
) -> bool {
    let text = events_sync::extract_text_content(&msg.message.content);
    if !text.starts_with(COMPACT_PREFIX) {
        return false;
    }

    // Text matched — warn if compact_boundary event is missing (race condition or data gap)
    let has_boundary = if let Some(uuid) = msg.parent_uuid.as_ref() {
        state
            .store
            .is_compact_boundary_event(session_id, uuid)
            .await
            .unwrap_or(false)
    } else {
        false
    };

    if !has_boundary {
        warn!(
            session = %session_id,
            "Compact summary detected by text pattern only — compact_boundary event not found"
        );
    }

    true
}
