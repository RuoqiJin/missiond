//! Conversation Logger Worker — processes Claude Code JSONL watcher events.
//!
//! Handles three event types from the CCTasksWatcher broadcast:
//! - NewMessages: routing via IngestionRouter + progress tracking + message ingestion
//! - NewEvents: system event sync
//! - SessionInactive: session completion, timeline building, embedding triggers

use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use missiond_core::WatcherEvent;
use missiond_core::cc_tasks::CCMessageLine;

use crate::events_sync;
use crate::infra::ingestion_router;
use crate::infra::message_handler::handle_new_messages;
use crate::state::{AppState, CurrentToolInfo, EmbeddingTask, SlotProgress, MEMORY_SLOW_SLOT_ID};

pub(crate) struct ConversationLoggerWorker {
    pub conv_logger_rx: broadcast::Receiver<WatcherEvent>,
}

impl super::BackgroundWorker for ConversationLoggerWorker {
    fn name(&self) -> &'static str { "conversation_logger" }

    async fn run(self, state: Arc<AppState>, _ctx: super::WorkerContext) {
        let mut rx = self.conv_logger_rx;
        run_loop(&state, &mut rx).await;
    }
}

async fn run_loop(s: &AppState, rx: &mut broadcast::Receiver<WatcherEvent>) {
    loop {
        match rx.recv().await {
            Ok(WatcherEvent::NewMessages { session_id, project_path, jsonl_path, messages, read_end_offset, source }) => {
                handle_new_messages_event(s, session_id, project_path, jsonl_path.clone(), messages, source).await;
                // Ack: cursor is safe to persist — messages have been written to PG
                let _ = s.cursor_ack_tx.send((jsonl_path, read_end_offset));
            }
            Ok(WatcherEvent::NewEvents { session_id, events }) => {
                events_sync::handle_new_events(s, session_id, events).await;
            }
            Ok(WatcherEvent::SessionInactive(session)) => {
                handle_session_inactive(s, &session.session_id).await;
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!(skipped = n, "Conversation logger lagged — triggering reconciliation");
                reconcile(s).await;
            }
            Err(_) => {}
        }
    }
}

async fn handle_new_messages_event(
    s: &AppState,
    session_id: String,
    project_path: String,
    jsonl_path: String,
    messages: Vec<CCMessageLine>,
    event_source: String,
) {
    // ── Step 1: IngestionRouter classifies the batch (sole decision point) ──
    let (route, compaction) = ingestion_router::classify(
        s, &session_id, &event_source, &jsonl_path, &project_path, &messages
    ).await;

    // ── Step 2: Compaction side-effects (binding inheritance + embedding queue) ──
    if let Some(ref old_sid) = compaction.old_session_id {
        inherit_task_bindings(s, old_sid, &session_id);
        let _ = s.embedding_tx.try_send(EmbeddingTask::ProcessSession(old_sid.clone()));
        info!(old_session = %old_sid, "Compaction: queued embedding for compacted session");
    }

    // ── Step 3: Progress tracking for PTY slots ──
    if route.is_pty {
        if let Some(ref slot_id) = route.slot_id {
            let mut progress = s.slot_progress.write().await;
            let sp = progress.entry(slot_id.clone()).or_default();
            if sp.session_id != session_id {
                *sp = SlotProgress { session_id: session_id.clone(), ..Default::default() };
            }
            for msg in &messages {
                if let Some(blocks) = msg.message.content.as_array() {
                    for block in blocks {
                        match block.get("type").and_then(|t| t.as_str()) {
                            Some("tool_use") => {
                                let name = block.get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                *sp.tool_counts.entry(name.clone()).or_insert(0) += 1;
                                sp.total_calls += 1;
                                sp.current_tool = Some(CurrentToolInfo {
                                    name,
                                    started_at: msg.timestamp.clone(),
                                });
                                sp.last_activity = Some(msg.timestamp.clone());
                            }
                            Some("tool_result") => {
                                sp.total_results += 1;
                                sp.current_tool = None;
                                if block.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false) {
                                    sp.error_count += 1;
                                }
                                sp.last_activity = Some(msg.timestamp.clone());
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // ── Step 4: Ghost session filter (防污网) ──
    // Skip sessions that have no assistant messages — these are /exit ghost sessions
    // produced by slot restarts. No business value, must not pollute the DB.
    let db_messages: Vec<_> = messages.into_iter()
        .filter(|m| m.message_type != "tool_use")
        .collect();
    if is_garbage_session(&db_messages) {
        debug!(session = %session_id, msgs = db_messages.len(), "Garbage session filtered (no assistant content)");
        return;
    }

    // ── Step 5: Hand off to message_handler with the pre-computed route ──
    handle_new_messages(s, session_id.clone(), project_path, jsonl_path, db_messages, &route).await;

    // ── Step 6: Compaction task inheritance ──
    if let Some(tid) = compaction.task_id {
        let _ = s.store.set_conversation_task_id(&session_id, &tid).await;
    }
}

/// Ghost session detector: returns true if the batch has no business value.
/// Ghost sessions are produced by slot restarts (/exit command, no AI interaction).
/// Pattern: all messages are user role, content is local commands (/exit, caveat, stdout).
fn is_garbage_session(messages: &[CCMessageLine]) -> bool {
    if messages.is_empty() {
        return false; // Empty batch is just incremental — let it through
    }
    // Quick check: if ANY message is from the assistant, it's a real session
    if messages.iter().any(|m| m.message.role == "assistant") {
        return false;
    }
    // All user-only: check if it's a local-command-only session (/exit pattern)
    messages.iter().all(|m| {
        let content = m.message.content.as_str().unwrap_or("");
        content.contains("<command-name>")
            || content.contains("<local-command-stdout>")
            || content.contains("<local-command-caveat>")
    })
}

async fn handle_session_inactive(s: &AppState, session_id: &str) {
    if let Ok(Some(conv)) = s.store.get_conversation(session_id).await {
        if conv.status == "compacted" {
            debug!(session = %session_id, "Skipping inactive check for compacted session");
            return;
        }
    }
    if let Err(e) = s.store.complete_conversation(session_id).await {
        warn!(session = %session_id, error = %e, "Failed to complete conversation");
    } else {
        info!(session = %session_id, "Conversation marked completed");
        build_session_timeline(s, session_id).await;
        let _ = s.embedding_tx.try_send(EmbeddingTask::ProcessSession(session_id.to_string()));
        // Auto-progress extraction: if this session was working on a Board task, extract progress
        submit_board_progress_extraction(s, session_id).await;
    }
}

/// Build session timeline if this parent session has compaction fragments.
async fn build_session_timeline(s: &AppState, session_id: &str) {
    let frags = s.store.get_compaction_fragments(session_id).await.unwrap_or_default();
    if frags.is_empty() {
        return;
    }
    let mut entries = Vec::new();
    for (idx, (frag_id, started_at, msg_count)) in frags.iter().enumerate() {
        let summary = s.store.get_last_assistant_content(frag_id).await.unwrap_or(None);
        let summary_tokens = summary.as_ref().map(|s| s.len() / 4).unwrap_or(0);
        entries.push(serde_json::json!({
            "fragment_id": frag_id,
            "shard_index": idx,
            "started_at": started_at,
            "message_count": msg_count,
            "summary_tokens": summary_tokens,
            "summary": summary,
            "segment_embedding_id": null,
        }));
    }
    if let Ok(json) = serde_json::to_string(&entries) {
        match s.store.set_session_timeline(session_id, &json).await {
            Ok(true) => info!(session = %session_id, fragments = frags.len(), "Session timeline built"),
            Ok(false) => debug!(session = %session_id, "Session timeline already exists"),
            Err(e) => warn!(session = %session_id, error = %e, "Failed to build session timeline"),
        }
    }
}

/// Reconcile: re-scan active sessions' JSONL to recover lost messages after broadcast lag.
async fn reconcile(s: &AppState) {
    let convs = s.store.list_conversations(Some("active"), 100, Some("all"), None, None, None, None).await
        .unwrap_or_default();
    let mut reconciled = 0usize;
    for conv in &convs {
        if let Some(ref path) = conv.jsonl_path {
            events_sync::reconcile_conversation_messages(s, &conv.id, path).await;
            reconciled += 1;
        }
    }
    if reconciled > 0 {
        info!(reconciled, "Lag reconciliation complete");
    }
}

/// Inherit session→task bindings across context compaction.
/// When Claude Code triggers auto-compact, the old session_id is replaced by a new one.
/// Without this, the binding chain breaks and auto-progress extraction silently fails.
fn inherit_task_bindings(s: &AppState, old_session_id: &str, new_session_id: &str) {
    if let Ok(mut map) = s.session_task_bindings.lock() {
        if let Some(bindings) = map.get(old_session_id).cloned() {
            if !bindings.is_empty() {
                let count = bindings.len();
                let new_bindings = map.entry(new_session_id.to_string()).or_default();
                for b in &bindings {
                    if !new_bindings.iter().any(|nb| nb.task_id == b.task_id) {
                        new_bindings.push(b.clone());
                    }
                }
                info!(
                    old = %old_session_id, new = %new_session_id, tasks = count,
                    "Task bindings inherited across compaction"
                );
            }
        }
    }
}

/// Submit board progress extraction when a session ends with bound Board tasks.
/// Daemon-side: pre-assembles a token-safe conversation summary, then dispatches
/// a structured JSON extraction task to the slow memory worker.
async fn submit_board_progress_extraction(s: &AppState, session_id: &str) {
    // 1. Remove bindings (session is ending)
    let bindings = match s.session_task_bindings.lock() {
        Ok(mut map) => map.remove(session_id).unwrap_or_default(),
        Err(_) => return,
    };
    if bindings.is_empty() { return; }

    // 2. Filter: only keep tasks still in Running status
    let mut active_bindings = Vec::new();
    for b in bindings {
        let is_running = s.store.get_board_task(&b.task_id).await
            .ok().flatten()
            .map(|t| t.status == missiond_core::types::BoardTaskStatus::Running)
            .unwrap_or(false);
        if is_running {
            active_bindings.push(b);
        }
    }
    if active_bindings.is_empty() { return; }

    // 3. Check message volume (< 4 → skip, not enough signal)
    // Gemini ARB: lowered from 10 to 4 — short efficient sessions still deserve progress extraction
    let msgs = s.store.get_conversation_messages(session_id, None, 4).await
        .unwrap_or_default();
    if msgs.len() < 4 { return; }

    // 4. Build conversation summary (Rust-side, token-safe — Gemini P1)
    let summary = build_conversation_summary_for_progress(s, session_id, 50).await;

    // 5. Build prompt with task info
    let task_list: Vec<String> = active_bindings.iter()
        .map(|b| format!("- {} (ID: {})", b.task_title, b.task_id))
        .collect();

    let prompt = format!(
        "[board_progress]\n{}\n\n关联任务:\n{}\n\n会话摘要:\n{}",
        s.prompts.extraction_board_progress(),
        task_list.join("\n"),
        summary,
    );

    // 6. Submit as memory task (dispatched to slot-memory-slow by memory_scheduler via role matching)
    match crate::state::submit_task(s.store.as_ref(), "memory", &prompt).await {
        Ok(task_id) => {
            // Pin to slow memory slot
            let _ = s.store.update_task(
                &task_id,
                &missiond_core::types::TaskUpdate {
                    slot_id: Some(MEMORY_SLOW_SLOT_ID.to_string()),
                    ..Default::default()
                },
            ).await;
            info!(
                session = %session_id,
                tasks = active_bindings.len(),
                submit_task = %task_id,
                "Board progress extraction submitted"
            );
        }
        Err(e) => {
            warn!(error = %e, "Failed to submit board progress extraction");
        }
    }
}

/// Rust-side conversation summary builder (token-safe).
/// Extracts user + assistant messages, truncates long content, caps total at ~4000 tokens.
/// Gemini ARB: tail-biased — recent messages are most important for is_done judgment.
/// Strategy: build from tail, then reverse so output reads chronologically.
async fn build_conversation_summary_for_progress(s: &AppState, session_id: &str, max_messages: usize) -> String {
    let messages = s.store
        .get_conversation_messages(session_id, None, max_messages as i64).await
        .unwrap_or_default();

    // Build from tail (most recent first) to ensure latest progress is preserved
    let mut parts: Vec<String> = Vec::new();
    let mut total_len = 0usize;
    for msg in messages.iter().rev() {
        if msg.role != "user" && msg.role != "assistant" { continue; }
        let content = if msg.content.len() > 500 {
            let mut end = 500;
            while end > 0 && !msg.content.is_char_boundary(end) { end -= 1; }
            format!("{}...[truncated]", &msg.content[..end])
        } else {
            msg.content.clone()
        };
        let line = format!("[{}] {}\n", msg.role, content);
        total_len += line.len();
        if total_len > 15000 { // ~4000 tokens hard cap
            parts.push("[...earlier messages truncated]\n".to_string());
            break;
        }
        parts.push(line);
    }
    parts.reverse(); // Restore chronological order
    parts.join("")
}
