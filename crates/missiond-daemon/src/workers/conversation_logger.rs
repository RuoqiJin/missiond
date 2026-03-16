//! Conversation Logger Worker — processes Claude Code JSONL watcher events.
//!
//! Handles three event types from the CCTasksWatcher broadcast:
//! - NewMessages: compaction detection, progress tracking, message ingestion
//! - NewEvents: system event sync
//! - SessionInactive: session completion, timeline building, embedding triggers

use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use missiond_core::WatcherEvent;
use missiond_core::cc_tasks::CCMessageLine;

use crate::events_sync;
use crate::infra::message_handler::handle_new_messages;
use crate::infra::session_util::detect_compaction;
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
            Ok(WatcherEvent::NewMessages { session_id, project_path, jsonl_path, messages }) => {
                handle_new_messages_event(s, session_id, project_path, jsonl_path, messages).await;
            }
            Ok(WatcherEvent::NewEvents { session_id, events }) => {
                events_sync::handle_new_events(s.mission.db(), session_id, events);
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
) {
    let mut is_pty = s.pty_session_uuids.read().await.contains(&session_id);

    // Compaction detection
    let mut compaction_task_id: Option<String> = None;
    if !is_pty {
        if let Some((slot_id, old_uuid, old_task_id)) = detect_compaction(s, &session_id, &project_path) {
            info!(
                slot_id = %slot_id,
                old_session = %old_uuid,
                new_session = %session_id,
                "Compaction detected: session replaced by context compaction"
            );
            let db = s.mission.db();
            let _ = db.mark_conversation_compacted(&old_uuid);
            let _ = db.set_slot_session(&slot_id, &session_id);
            s.pty_session_uuids.write().await.remove(&old_uuid);
            s.pty_session_uuids.write().await.insert(session_id.clone());
            compaction_task_id = old_task_id;
            is_pty = true;

            // Inherit session→task bindings across compaction (Gemini P0: prevent binding chain break)
            inherit_task_bindings(s, &old_uuid, &session_id);

            // Fix A: 补齐 compacted session 的向量/摘要
            // handle_session_inactive() 跳过 compacted session，此处显式补发
            let _ = s.embedding_tx.try_send(EmbeddingTask::ProcessSession(old_uuid.clone()));
            info!(old_session = %old_uuid, "Compaction: queued embedding for compacted session");
        }
    }

    // Progress tracking
    if is_pty {
        if let Ok(Some(slot_id)) = s.mission.db().get_slot_for_session(&session_id) {
            let mut progress = s.slot_progress.write().await;
            let sp = progress.entry(slot_id).or_default();
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

    let db_messages: Vec<_> = messages.into_iter()
        .filter(|m| m.message_type != "tool_use")
        .collect();
    handle_new_messages(s, session_id.clone(), project_path, jsonl_path, db_messages, is_pty);

    if let Some(tid) = compaction_task_id {
        let _ = s.mission.db().set_conversation_task_id(&session_id, &tid);
    }
}

async fn handle_session_inactive(s: &AppState, session_id: &str) {
    if let Ok(Some(conv)) = s.mission.db().get_conversation(session_id) {
        if conv.status == "compacted" {
            debug!(session = %session_id, "Skipping inactive check for compacted session");
            return;
        }
    }
    if let Err(e) = s.mission.db().complete_conversation(session_id) {
        warn!(session = %session_id, error = %e, "Failed to complete conversation");
    } else {
        info!(session = %session_id, "Conversation marked completed");
        build_session_timeline(s, session_id);
        let _ = s.embedding_tx.try_send(EmbeddingTask::ProcessSession(session_id.to_string()));
        // Auto-progress extraction: if this session was working on a Board task, extract progress
        submit_board_progress_extraction(s, session_id);
    }
}

/// Build session timeline if this parent session has compaction fragments.
fn build_session_timeline(s: &AppState, session_id: &str) {
    let db = s.mission.db();
    let frags = db.get_compaction_fragments(session_id).unwrap_or_default();
    if frags.is_empty() {
        return;
    }
    let mut entries = Vec::new();
    for (idx, (frag_id, started_at, msg_count)) in frags.iter().enumerate() {
        let summary = db.get_last_assistant_content(frag_id).unwrap_or(None);
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
        match db.set_session_timeline(session_id, &json) {
            Ok(true) => info!(session = %session_id, fragments = frags.len(), "Session timeline built"),
            Ok(false) => debug!(session = %session_id, "Session timeline already exists"),
            Err(e) => warn!(session = %session_id, error = %e, "Failed to build session timeline"),
        }
    }
}

/// Reconcile: re-scan active sessions' JSONL to recover lost messages after broadcast lag.
async fn reconcile(s: &AppState) {
    let db = s.mission.db();
    let convs = db.list_conversations(Some("active"), 100, Some("all"), None, None, None, None)
        .unwrap_or_default();
    let mut reconciled = 0usize;
    for conv in &convs {
        if let Some(ref path) = conv.jsonl_path {
            events_sync::reconcile_conversation_messages(db, &conv.id, path).await;
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
fn submit_board_progress_extraction(s: &AppState, session_id: &str) {
    // 1. Remove bindings (session is ending)
    let bindings = match s.session_task_bindings.lock() {
        Ok(mut map) => map.remove(session_id).unwrap_or_default(),
        Err(_) => return,
    };
    if bindings.is_empty() { return; }

    // 2. Filter: only keep tasks still in Running status
    let db = s.mission.db();
    let active_bindings: Vec<_> = bindings.into_iter().filter(|b| {
        db.get_board_task(&b.task_id)
            .ok().flatten()
            .map(|t| t.status == missiond_core::types::BoardTaskStatus::Running)
            .unwrap_or(false)
    }).collect();
    if active_bindings.is_empty() { return; }

    // 3. Check message volume (< 4 → skip, not enough signal)
    // Gemini ARB: lowered from 10 to 4 — short efficient sessions still deserve progress extraction
    let msgs = db.get_conversation_messages(session_id, None, 4)
        .unwrap_or_default();
    if msgs.len() < 4 { return; }

    // 4. Build conversation summary (Rust-side, token-safe — Gemini P1)
    let summary = build_conversation_summary_for_progress(s, session_id, 50);

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
    match s.mission.submit("memory", &prompt) {
        Ok(task_id) => {
            // Pin to slow memory slot
            let _ = s.mission.db().update_task(
                &task_id,
                &missiond_core::types::TaskUpdate {
                    slot_id: Some(MEMORY_SLOW_SLOT_ID.to_string()),
                    ..Default::default()
                },
            );
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
fn build_conversation_summary_for_progress(s: &AppState, session_id: &str, max_messages: usize) -> String {
    let messages = s.mission.db()
        .get_conversation_messages(session_id, None, max_messages as i64)
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
