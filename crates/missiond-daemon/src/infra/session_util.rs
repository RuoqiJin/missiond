//! Session utility functions.

use crate::state::AppState;

/// Detect if a new unknown session is a compacted replacement for an active slot session.
///
/// When Claude Code runs out of context, it compacts into a new session (new JSONL file).
/// The old session stops being written to, but the PTY process continues.
/// We detect this by checking if any active slot has a session in the same project directory.
///
/// Returns (slot_id, old_session_id, old_task_id) if compaction is detected.
pub(crate) fn detect_compaction(
    state: &AppState,
    new_session_id: &str,
    new_project: &str,
) -> Option<(String, String, Option<String>)> {
    let db = state.mission.db();
    let all_slot_sessions = db.get_all_slot_sessions().ok()?;

    for (slot_id, old_uuid) in &all_slot_sessions {
        if old_uuid == new_session_id {
            continue; // Same session, not compaction
        }
        let old_conv = db.get_conversation(old_uuid).ok()??;
        // Must be same project and still active
        if old_conv.project.as_deref() != Some(new_project) || old_conv.status != "active" {
            continue;
        }
        // The old session should have been written to recently (within 10 min)
        // to avoid false positives with stale slot sessions.
        // Use updated_at (last message time) when available, fall back to started_at.
        let last_active = old_conv.updated_at.as_deref()
            .unwrap_or(&old_conv.started_at);
        if let Ok(t) = chrono::DateTime::parse_from_rfc3339(last_active) {
            let age = chrono::Utc::now().signed_duration_since(t);
            if age > chrono::Duration::minutes(10) {
                continue; // No messages in last 10 min — not a live compaction
            }
        }
        return Some((
            slot_id.clone(),
            old_uuid.clone(),
            old_conv.task_id.clone(),
        ));
    }
    None
}
