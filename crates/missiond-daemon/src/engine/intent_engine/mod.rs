//! Intent Engine — 从意图到执行的正向控制流。
//!
//! 合并: autopilot (orchestrator) + flow_engine (phase lifecycle)
//!       + memory_scheduler (slot dispatcher) + workflow_executor (skill MCP runner)

pub mod autopilot;
pub mod autopilot_workflow;
pub mod flow_engine;
pub mod memory_scheduler;
pub mod workflow_executor;

use crate::state::{AppState, MEMORY_SLOT_ID};

/// Standardized API for Learning Engine to request a slot by ID.
/// Learning modules should call this instead of directly importing memory_scheduler.
pub(crate) async fn request_execution_slot(state: &AppState, slot_id: &str) -> bool {
    memory_scheduler::ensure_memory_slot_by_id(state, slot_id).await
}

/// Convenience: request the default memory slot (MEMORY_SLOT_ID).
pub(crate) async fn request_default_slot(state: &AppState) -> bool {
    memory_scheduler::ensure_memory_slot_by_id(state, MEMORY_SLOT_ID).await
}
