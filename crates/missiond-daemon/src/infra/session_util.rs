//! Session utility functions.

use crate::state::AppState;

/// Detect if a new unknown session is a compacted replacement for an active slot session.
///
/// When Claude Code runs out of context, it compacts into a new session (new JSONL file).
/// The old session stops being written to, but the PTY process continues.
/// We detect this by checking if any active slot has a session in the same project directory.
///
/// Returns (slot_id, old_session_id, old_task_id) if compaction is detected.
pub(crate) async fn detect_compaction(
    state: &AppState,
    new_session_id: &str,
    new_jsonl_path: &str,
) -> Option<(String, String, Option<String>)> {
    // 1. Hard requirement: Must be a compaction fragment
    if !new_session_id.contains("-acompact-") {
        return None;
    }

    // 2. Extract the globally unique root session ID from the file path
    let root_id = missiond_core::db::extract_parent_session_id(new_jsonl_path)?;

    let all_slot_sessions = state.store.get_all_slot_sessions().await.ok()?;

    for (slot_id, current_uuid) in &all_slot_sessions {
        if current_uuid == new_session_id {
            continue; // Same session
        }

        // 3. Deterministic match: Does the slot's current session belong to the same root tree?
        let is_match = if current_uuid == &root_id {
            true // The current session IS the root
        } else {
            // Check if the current session's root matches
            if let Ok(Some(current_conv)) = state.store.get_conversation(current_uuid).await {
                if let Some(current_path) = current_conv.jsonl_path {
                    missiond_core::db::extract_parent_session_id(&current_path)
                        == Some(root_id.clone())
                } else {
                    false
                }
            } else {
                false
            }
        };

        if is_match {
            if let Ok(Some(old_conv)) = state.store.get_conversation(current_uuid).await {
                return Some((
                    slot_id.clone(),
                    current_uuid.clone(),
                    old_conv.task_id.clone(),
                ));
            }
        }
    }
    None
}
