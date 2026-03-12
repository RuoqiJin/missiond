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
use crate::state::{AppState, CurrentToolInfo, EmbeddingTask, SlotProgress};

pub(crate) struct ConversationLoggerWorker {
    pub conv_logger_rx: broadcast::Receiver<WatcherEvent>,
}

impl super::BackgroundWorker for ConversationLoggerWorker {
    fn name(&self) -> &'static str { "conversation_logger" }

    async fn run(self, state: Arc<AppState>) {
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
    let convs = db.list_conversations(Some("active"), 100, Some("all"), None)
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
