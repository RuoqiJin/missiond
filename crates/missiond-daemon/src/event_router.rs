//! Event Router — spawns event-driven consumer tasks for the daemon.
//!
//! Extracted from main.rs (Phase 2 S7, R4) to prevent main.rs from becoming
//! a new God Module. Each consumer runs in its own tokio task and filters
//! relevant DaemonEvent variants.
//!
//! Phase 3 PR1: Trailing-edge debounce — events are absorbed during the
//! debounce window, and the handler fires once when the window expires.
//! This prevents losing tail events that arrive during a simple sleep().

use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

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

/// Extraction lane: SlotBecameIdle → schedule_memory_tasks.
/// Trailing-edge debounce: 500ms window, fires after window expires.
fn spawn_extraction_consumer(state: &AppState) {
    let s = state.clone();
    let mut rx = s.event_bus.subscribe();
    tokio::spawn(async move {
        let mut pending = false;
        loop {
            if pending {
                // Drain events during debounce window (fixed deadline, no reset)
                let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
                loop {
                    match tokio::time::timeout_at(deadline, rx.recv()).await {
                        Ok(Ok(DaemonEvent::SlotBecameIdle { ref slot_id }))
                            if slot_id == MEMORY_SLOT_ID || slot_id == MEMORY_SLOW_SLOT_ID =>
                        {
                            // Absorb — debounce window still open
                        }
                        Ok(Err(RecvError::Lagged(_))) => { /* will fire at deadline anyway */ }
                        Ok(Err(RecvError::Closed)) => return,
                        Ok(_) => {} // other events, ignore
                        Err(_) => break, // timeout → fire trailing edge
                    }
                }
                // Trailing edge fire
                if !s.memory_paused.load(Ordering::Relaxed) {
                    schedule_memory_tasks(&s).await;
                }
                pending = false;
            } else {
                match rx.recv().await {
                    Ok(DaemonEvent::SlotBecameIdle { ref slot_id })
                        if slot_id == MEMORY_SLOT_ID || slot_id == MEMORY_SLOW_SLOT_ID =>
                    {
                        if !s.memory_paused.load(Ordering::Relaxed) {
                            pending = true;
                        }
                    }
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "Extraction consumer lagged");
                        // Lagged → cooldown then catch-up
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        if !s.memory_paused.load(Ordering::Relaxed) {
                            schedule_memory_tasks(&s).await;
                        }
                    }
                    Err(RecvError::Closed) => break,
                    _ => {}
                }
            }
        }
    });
}

/// Submit dispatcher: TaskCreated/TaskCompleted → dispatch.
/// Trailing-edge debounce: 100ms window.
fn spawn_submit_consumer(state: &AppState) {
    let s = state.clone();
    let mut rx = s.event_bus.subscribe();
    tokio::spawn(async move {
        let mut pending = false;
        loop {
            if pending {
                let deadline = tokio::time::Instant::now() + Duration::from_millis(100);
                loop {
                    match tokio::time::timeout_at(deadline, rx.recv()).await {
                        Ok(Ok(DaemonEvent::TaskCreated { .. }))
                        | Ok(Ok(DaemonEvent::TasksBatchCreated { .. }))
                        | Ok(Ok(DaemonEvent::TaskCompleted { .. })) => {
                            // Absorb
                        }
                        Ok(Err(RecvError::Lagged(_))) => {}
                        Ok(Err(RecvError::Closed)) => return,
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                dispatch_queued_submit_tasks(&s).await;
                if !s.memory_paused.load(Ordering::Relaxed) {
                    schedule_memory_tasks(&s).await;
                }
                pending = false;
            } else {
                match rx.recv().await {
                    Ok(DaemonEvent::TaskCreated { .. })
                    | Ok(DaemonEvent::TasksBatchCreated { .. })
                    | Ok(DaemonEvent::TaskCompleted { .. }) => {
                        pending = true;
                    }
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "Submit consumer lagged");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        dispatch_queued_submit_tasks(&s).await;
                    }
                    Err(RecvError::Closed) => break,
                    _ => {}
                }
            }
        }
    });
}

/// Decision engine: QuestionCreated → process.
/// Trailing-edge debounce: 100ms window.
fn spawn_decision_consumer(state: &AppState) {
    let s = state.clone();
    let mut rx = s.event_bus.subscribe();
    tokio::spawn(async move {
        let mut pending = false;
        loop {
            if pending {
                let deadline = tokio::time::Instant::now() + Duration::from_millis(100);
                loop {
                    match tokio::time::timeout_at(deadline, rx.recv()).await {
                        Ok(Ok(DaemonEvent::QuestionCreated { .. })) => {}
                        Ok(Err(RecvError::Lagged(_))) => {}
                        Ok(Err(RecvError::Closed)) => return,
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                process_pending_master_questions(&s).await;
                pending = false;
            } else {
                match rx.recv().await {
                    Ok(DaemonEvent::QuestionCreated { .. }) => {
                        pending = true;
                    }
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "Decision consumer lagged");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        process_pending_master_questions(&s).await;
                    }
                    Err(RecvError::Closed) => break,
                    _ => {}
                }
            }
        }
    });
}
