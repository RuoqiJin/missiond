//! Ingestion Router — sole decision point for conversation classification.
//!
//! Centralizes all routing decisions that were previously scattered across
//! conversation_logger.rs, message_handler.rs, and slot_orchestrator/mod.rs.
//!
//! The Router produces an `IngestionRoute` (the "waybill") that tells
//! downstream handlers exactly how to store the data — no guessing needed.

use tracing::info;

use crate::state::AppState;
use crate::infra::session_util::detect_compaction;

/// Routing result — the "waybill" attached to every incoming message batch.
/// Downstream handlers (message_handler, etc.) use this directly without
/// making any classification decisions of their own.
#[derive(Debug, Clone)]
pub(crate) struct IngestionRoute {
    /// Origin CLI: "claude_code", "gemini_cli", "codex_cli", "pty_jsonl"
    pub source: String,
    /// Domain type: "user", "worker", "meta", "subagent", "compaction"
    pub conversation_type: String,
    /// Slot ID if this session belongs to a managed PTY slot
    pub slot_id: Option<String>,
    /// Parent session for subagent conversations
    pub parent_session_id: Option<String>,
    /// Whether this session is managed by a PTY slot
    pub is_pty: bool,
}

/// Compaction side-effect data returned alongside the route.
/// The caller (conversation_logger) needs this to handle compaction bookkeeping.
#[derive(Debug, Default)]
pub(crate) struct CompactionInfo {
    /// Task ID inherited from the compacted session
    pub task_id: Option<String>,
    /// Old session ID that was compacted (for embedding queue)
    pub old_session_id: Option<String>,
}

/// Classify an incoming message batch and produce a routing decision.
///
/// This is the **sole** entry point for all ingestion routing decisions.
/// It checks: source hint from watcher, slot registry, compaction state,
/// session ID patterns (agent-*, -acompact-), and parent linkage.
pub(crate) async fn classify(
    state: &AppState,
    session_id: &str,
    event_source: &str,
    jsonl_path: &str,
    project_path: &str,
) -> (IngestionRoute, CompactionInfo) {
    let mut is_pty = state.pty_session_uuids.read().await.contains(session_id);
    let mut compaction = CompactionInfo::default();

    // Expectation ticket claim: if this is a new claude_code session, check if a slot
    // recently spawned in the same project. Late-binding resolves the cross-process race
    // where Claude Code generates a UUID that MissionD cannot predict.
    if !is_pty && event_source == "claude_code" {
        if let Some(slot_id) = claim_pending_spawn(state, project_path).await {
            info!(
                session_id,
                slot_id = %slot_id,
                "IngestionRouter: claimed expectation ticket"
            );
            let _ = state.store.set_slot_session(&slot_id, session_id).await;
            state.pty_session_uuids.write().await.insert(session_id.to_string());
            is_pty = true;
        }
    }

    // Compaction detection: a non-PTY session that actually belongs to a slot
    // (context compaction creates a new session_id for the same slot)
    if !is_pty {
        if let Some((slot_id, old_uuid, old_task_id)) = detect_compaction(state, session_id, project_path).await {
            info!(
                slot_id = %slot_id,
                old_session = %old_uuid,
                new_session = %session_id,
                "IngestionRouter: compaction detected"
            );
            let _ = state.store.mark_conversation_compacted(&old_uuid).await;
            let _ = state.store.set_slot_session(&slot_id, session_id).await;
            state.pty_session_uuids.write().await.remove(&old_uuid);
            state.pty_session_uuids.write().await.insert(session_id.to_string());
            is_pty = true;

            compaction.task_id = old_task_id;
            compaction.old_session_id = Some(old_uuid);
        }
    }

    // Resolve slot_id from registry
    let slot_id = if is_pty {
        state.store.get_slot_for_session(session_id).await.unwrap_or(None)
    } else {
        None
    };

    // Determine source: PTY Claude Code → "pty_jsonl", others pass through
    let source = if is_pty && event_source == "claude_code" {
        "pty_jsonl".to_string()
    } else {
        event_source.to_string()
    };

    // Determine conversation_type via the central brain
    let conversation_type = missiond_core::db::derive_conversation_type(
        slot_id.as_deref(), session_id,
    );

    // Extract parent session ID for subagent conversations
    let parent_session_id = if session_id.starts_with("agent-") {
        missiond_core::db::extract_parent_session_id(jsonl_path)
    } else {
        None
    };

    // Determine chat_type for non-standard sources
    let route = IngestionRoute {
        source,
        conversation_type,
        slot_id,
        parent_session_id,
        is_pty,
    };

    (route, compaction)
}

/// Claim a pending slot spawn expectation ticket.
/// Matches by project path within a 30-second window. Consumes the ticket on match.
async fn claim_pending_spawn(state: &AppState, project_path: &str) -> Option<String> {
    let now = tokio::time::Instant::now();
    let max_age = std::time::Duration::from_secs(30);

    let mut spawns = state.pending_slot_spawns.write().await;

    // Find matching ticket: same project path, within time window
    let idx = spawns.iter().position(|(path, _, ts)| {
        path == project_path && now.duration_since(*ts) < max_age
    })?;

    let (_, slot_id, _) = spawns.remove(idx);

    // GC: also remove any expired tickets
    spawns.retain(|(_, _, ts)| now.duration_since(*ts) < max_age);

    Some(slot_id)
}
