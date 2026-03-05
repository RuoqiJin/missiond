//! Event Router — spawns event-driven consumer tasks for the daemon.
//!
//! Extracted from main.rs (Phase 2 S7, R4) to prevent main.rs from becoming
//! a new God Module. Each consumer runs in its own tokio task and filters
//! relevant DaemonEvent variants.

use crate::event_bus::DaemonEvent;
use crate::state::{AppState, MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID};
use crate::memory_scheduler::{schedule_memory_tasks, dispatch_queued_submit_tasks};
use crate::decision_engine::process_pending_master_questions;

/// Start all event-driven consumer tasks. Call once during daemon startup.
pub(crate) fn start_event_consumers(state: &AppState) {
    spawn_extraction_consumer(state);
    spawn_submit_consumer(state);
    spawn_decision_consumer(state);
}

/// Extraction lane: SlotBecameIdle → schedule_memory_tasks (500ms debounce).
fn spawn_extraction_consumer(state: &AppState) {
    let s = state.clone();
    let mut rx = s.event_bus.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(DaemonEvent::SlotBecameIdle { ref slot_id })
                    if slot_id == MEMORY_SLOT_ID || slot_id == MEMORY_SLOW_SLOT_ID =>
                {
                    if !s.memory_paused.load(std::sync::atomic::Ordering::Relaxed) {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        schedule_memory_tasks(&s).await;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "Extraction consumer lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                _ => {}
            }
        }
    });
}

/// Submit dispatcher: TaskCreated/TaskCompleted → dispatch (100ms debounce).
fn spawn_submit_consumer(state: &AppState) {
    let s = state.clone();
    let mut rx = s.event_bus.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(DaemonEvent::TaskCreated { .. })
                | Ok(DaemonEvent::TaskCompleted { .. }) => {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    dispatch_queued_submit_tasks(&s).await;
                    if !s.memory_paused.load(std::sync::atomic::Ordering::Relaxed) {
                        schedule_memory_tasks(&s).await;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "Submit consumer lagged");
                    dispatch_queued_submit_tasks(&s).await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                _ => {}
            }
        }
    });
}

/// Decision engine: QuestionCreated → process (100ms debounce).
fn spawn_decision_consumer(state: &AppState) {
    let s = state.clone();
    let mut rx = s.event_bus.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(DaemonEvent::QuestionCreated { .. }) => {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    process_pending_master_questions(&s).await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "Decision consumer lagged");
                    process_pending_master_questions(&s).await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                _ => {}
            }
        }
    });
}
