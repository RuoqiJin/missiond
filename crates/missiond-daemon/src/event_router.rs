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

/// Exponential backoff for Lagged recovery: 100ms → 200ms → … → 2000ms cap.
/// Adds ±25% jitter to avoid thundering herd.
fn lagged_backoff(consecutive_lags: u32) -> Duration {
    let base_ms: u64 = 100;
    let max_ms: u64 = 2000;
    let raw = base_ms.saturating_mul(1u64 << consecutive_lags.min(5));
    let capped = raw.min(max_ms);
    // Jitter: ±25%
    let jitter_range = capped / 4;
    let jitter = if jitter_range > 0 {
        (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as u64) % (jitter_range * 2)
    } else { 0 };
    let with_jitter = capped.saturating_sub(jitter_range).saturating_add(jitter);
    Duration::from_millis(with_jitter)
}

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
        let mut consecutive_lags: u32 = 0;
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
                consecutive_lags = 0;
                s.stats.events_consumed_extraction.fetch_add(1, Ordering::Relaxed);
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
                        consecutive_lags += 1;
                        let backoff = lagged_backoff(consecutive_lags);
                        tracing::warn!(skipped = n, backoff_ms = backoff.as_millis() as u64, consecutive = consecutive_lags, "Extraction consumer lagged");
                        s.stats.events_lagged_extraction.fetch_add(1, Ordering::Relaxed);
                        s.stats.events_lagged_total_skipped.fetch_add(n, Ordering::Relaxed);
                        tokio::time::sleep(backoff).await;
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
        let mut consecutive_lags: u32 = 0;
        loop {
            if pending {
                let deadline = tokio::time::Instant::now() + Duration::from_millis(100);
                loop {
                    match tokio::time::timeout_at(deadline, rx.recv()).await {
                        Ok(Ok(DaemonEvent::TaskCreated { .. }))
                        | Ok(Ok(DaemonEvent::TaskCompleted { .. })) => {
                            // Absorb
                        }
                        Ok(Err(RecvError::Lagged(_))) => {}
                        Ok(Err(RecvError::Closed)) => return,
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                consecutive_lags = 0;
                s.stats.events_consumed_submit.fetch_add(1, Ordering::Relaxed);
                dispatch_queued_submit_tasks(&s).await;
                if !s.memory_paused.load(Ordering::Relaxed) {
                    schedule_memory_tasks(&s).await;
                }
                pending = false;
            } else {
                match rx.recv().await {
                    Ok(DaemonEvent::TaskCreated { .. })
                    | Ok(DaemonEvent::TaskCompleted { .. }) => {
                        pending = true;
                    }
                    Err(RecvError::Lagged(n)) => {
                        consecutive_lags += 1;
                        let backoff = lagged_backoff(consecutive_lags);
                        tracing::warn!(skipped = n, backoff_ms = backoff.as_millis() as u64, consecutive = consecutive_lags, "Submit consumer lagged");
                        s.stats.events_lagged_submit.fetch_add(1, Ordering::Relaxed);
                        s.stats.events_lagged_total_skipped.fetch_add(n, Ordering::Relaxed);
                        tokio::time::sleep(backoff).await;
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
        let mut consecutive_lags: u32 = 0;
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
                consecutive_lags = 0;
                s.stats.events_consumed_decision.fetch_add(1, Ordering::Relaxed);
                process_pending_master_questions(&s).await;
                pending = false;
            } else {
                match rx.recv().await {
                    Ok(DaemonEvent::QuestionCreated { .. }) => {
                        pending = true;
                    }
                    Err(RecvError::Lagged(n)) => {
                        consecutive_lags += 1;
                        let backoff = lagged_backoff(consecutive_lags);
                        tracing::warn!(skipped = n, backoff_ms = backoff.as_millis() as u64, consecutive = consecutive_lags, "Decision consumer lagged");
                        s.stats.events_lagged_decision.fetch_add(1, Ordering::Relaxed);
                        s.stats.events_lagged_total_skipped.fetch_add(n, Ordering::Relaxed);
                        tokio::time::sleep(backoff).await;
                        process_pending_master_questions(&s).await;
                    }
                    Err(RecvError::Closed) => break,
                    _ => {}
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_first_lag_is_200ms() {
        // consecutive_lags=1: 100 * 2^1 = 200ms ±25% → 150..250
        let d = lagged_backoff(1);
        assert!(d.as_millis() >= 150 && d.as_millis() <= 250,
            "first lag backoff should be ~200ms, got {}ms", d.as_millis());
    }

    #[test]
    fn backoff_doubles_each_step() {
        for lag in 1..=6 {
            let d = lagged_backoff(lag);
            let raw = (100u64 * (1u64 << lag.min(5))).min(2000);
            let lower = raw * 3 / 4;
            let upper = raw * 5 / 4;
            assert!(d.as_millis() >= lower as u128 && d.as_millis() <= upper as u128,
                "lag={}: expected {}ms±25%, got {}ms", lag, raw, d.as_millis());
        }
    }

    #[test]
    fn backoff_caps_at_2000ms() {
        let d = lagged_backoff(10);
        assert!(d.as_millis() <= 2500, "should cap at ~2000ms, got {}ms", d.as_millis());
    }

    #[test]
    fn backoff_zero_consecutive() {
        let d = lagged_backoff(0);
        assert!(d.as_millis() >= 75 && d.as_millis() <= 125,
            "zero consecutive should be ~100ms, got {}ms", d.as_millis());
    }
}
