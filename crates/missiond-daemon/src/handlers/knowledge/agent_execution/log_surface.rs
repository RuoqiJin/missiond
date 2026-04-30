use crate::state::AppState;
use missiond_core::event::events::ExecutionEvent;
use tracing::warn;

pub(super) use super::log_list::action_list;
pub(super) use super::log_open::action_open;

/// Forward an `ExecutionEvent` to the v2 bus and log (but never propagate)
/// publish failures. Companion-log writes are already durable on disk; the
/// bus event is a live projection.
pub(super) async fn emit_execution_event(state: &AppState, ev: ExecutionEvent) {
    if let Err(e) = state.bus.publish_execution(ev).await {
        warn!(error = %e, "failed to publish ExecutionEvent (companion log already durable)");
    }
}
