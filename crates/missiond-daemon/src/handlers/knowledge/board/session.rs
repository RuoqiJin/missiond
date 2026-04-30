use super::*;

/// Record implicit session->task binding for auto-progress extraction.
/// Called when Claude Code interacts with a Board task via MCP.
pub(super) fn record_session_task_binding(state: &AppState, task_id: &str, task_title: &str) {
    if let Some(session_id) = current_session_id() {
        if let Ok(mut map) = state.session_task_bindings.lock() {
            let bindings = map.entry(session_id).or_default();
            // Dedup: don't record same task twice in same session.
            if !bindings.iter().any(|b| b.task_id == task_id) {
                bindings.push(SessionTaskBinding {
                    task_id: task_id.to_string(),
                    task_title: task_title.to_string(),
                    bound_at: chrono::Utc::now().timestamp(),
                });
            }
        }
    }
}
