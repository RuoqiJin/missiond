//! Event Router — spawns event-driven consumer tasks for the daemon.
//!
//! Extracted from main.rs (Phase 2 S7, R4) to prevent main.rs from becoming
//! a new God Module. Each consumer runs in its own tokio task and filters
//! relevant DaemonEvent variants.
//!
//! Phase 3 PR1: Trailing-edge debounce — events are absorbed during the
//! debounce window, and the handler fires once when the window expires.
//!
//! Phase 6: Consumers now subscribe to broadcast<TimelineEvent> instead of
//! broadcast<DaemonEvent>. The inner DaemonEvent is accessed via .event field.

use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;

use crate::event_bus::{DaemonEvent, TimelineEvent};
use crate::experience_harvester;
use crate::state::{AppState, MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID};
use crate::memory_scheduler::{schedule_memory_tasks, dispatch_queued_submit_tasks};
use crate::decision_engine::process_pending_master_questions;
use crate::extraction::{check_realtime_extraction, check_deep_analysis, check_kb_consolidation};
use crate::workers::{retro_worker, strategy_worker};

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

// @beacon: eventbus
/// Start all event-driven consumer tasks. Call once during daemon startup.
pub(crate) fn start_event_consumers(state: &AppState, timeline_tx: &broadcast::Sender<TimelineEvent>) {
    spawn_extraction_consumer(state, timeline_tx);
    spawn_submit_consumer(state, timeline_tx);
    spawn_decision_consumer(state, timeline_tx);
    spawn_harvest_consumer(state, timeline_tx);
    // Phase 3: Reflection consumers (Shadow Mode — log-only, no action)
    spawn_realtime_extraction_consumer(state, timeline_tx);
    spawn_session_reflection_consumer(state, timeline_tx);
    spawn_kb_consolidation_consumer(state, timeline_tx);
}

/// Extraction lane: SlotBecameIdle → schedule_memory_tasks.
/// Trailing-edge debounce: 500ms window, fires after window expires.
fn spawn_extraction_consumer(state: &AppState, timeline_tx: &broadcast::Sender<TimelineEvent>) {
    let s = state.clone();
    let mut rx = timeline_tx.subscribe();
    tokio::spawn(async move {
        let mut pending = false;
        let mut consecutive_lags: u32 = 0;
        loop {
            if pending {
                // Drain events during debounce window (fixed deadline, no reset)
                let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
                loop {
                    match tokio::time::timeout_at(deadline, rx.recv()).await {
                        Ok(Ok(te)) => match &te.event {
                            DaemonEvent::SlotBecameIdle { ref slot_id }
                                if slot_id == MEMORY_SLOT_ID || slot_id == MEMORY_SLOW_SLOT_ID =>
                            {
                                // Absorb — debounce window still open
                            }
                            _ => {} // other events, ignore
                        },
                        Ok(Err(RecvError::Lagged(_))) => { /* will fire at deadline anyway */ }
                        Ok(Err(RecvError::Closed)) => return,
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
                    Ok(te) => match &te.event {
                        DaemonEvent::SlotBecameIdle { ref slot_id }
                            if slot_id == MEMORY_SLOT_ID || slot_id == MEMORY_SLOW_SLOT_ID =>
                        {
                            if !s.memory_paused.load(Ordering::Relaxed) {
                                pending = true;
                            }
                        }
                        _ => {}
                    },
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
                }
            }
        }
    });
}

/// Submit dispatcher: TaskCreated/TaskCompleted → dispatch.
/// Trailing-edge debounce: 100ms window.
fn spawn_submit_consumer(state: &AppState, timeline_tx: &broadcast::Sender<TimelineEvent>) {
    let s = state.clone();
    let mut rx = timeline_tx.subscribe();
    tokio::spawn(async move {
        let mut pending = false;
        let mut consecutive_lags: u32 = 0;
        loop {
            if pending {
                let deadline = tokio::time::Instant::now() + Duration::from_millis(100);
                loop {
                    match tokio::time::timeout_at(deadline, rx.recv()).await {
                        Ok(Ok(te)) => match &te.event {
                            DaemonEvent::TaskCreated { .. }
                            | DaemonEvent::TaskCompleted { .. } => {
                                // Absorb
                            }
                            _ => {}
                        },
                        Ok(Err(RecvError::Lagged(_))) => {}
                        Ok(Err(RecvError::Closed)) => return,
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
                    Ok(te) => match &te.event {
                        DaemonEvent::TaskCreated { .. }
                        | DaemonEvent::TaskCompleted { .. } => {
                            pending = true;
                        }
                        _ => {}
                    },
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
                }
            }
        }
    });
}

/// Decision engine: QuestionCreated → process.
/// Trailing-edge debounce: 100ms window.
fn spawn_decision_consumer(state: &AppState, timeline_tx: &broadcast::Sender<TimelineEvent>) {
    let s = state.clone();
    let mut rx = timeline_tx.subscribe();
    tokio::spawn(async move {
        let mut pending = false;
        let mut consecutive_lags: u32 = 0;
        loop {
            if pending {
                let deadline = tokio::time::Instant::now() + Duration::from_millis(100);
                loop {
                    match tokio::time::timeout_at(deadline, rx.recv()).await {
                        Ok(Ok(te)) => match &te.event {
                            DaemonEvent::QuestionCreated { .. } => {}
                            _ => {}
                        },
                        Ok(Err(RecvError::Lagged(_))) => {}
                        Ok(Err(RecvError::Closed)) => return,
                        Err(_) => break,
                    }
                }
                consecutive_lags = 0;
                s.stats.events_consumed_decision.fetch_add(1, Ordering::Relaxed);
                process_pending_master_questions(&s).await;
                pending = false;
            } else {
                match rx.recv().await {
                    Ok(te) => match &te.event {
                        DaemonEvent::QuestionCreated { .. } => {
                            pending = true;
                        }
                        _ => {}
                    },
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
                }
            }
        }
    });
}

/// Experience Harvest: NarrationSessionCompleted → harvest_session.
/// No debounce needed — fires once per session completion, already infrequent.
fn spawn_harvest_consumer(state: &AppState, timeline_tx: &broadcast::Sender<TimelineEvent>) {
    let s = state.clone();
    let mut rx = timeline_tx.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(te) => {
                    if let DaemonEvent::NarrationSessionCompleted { session_id, .. } = &te.event {
                        let mc = std::sync::Arc::clone(&s.mission);
                        let sid = session_id.clone();
                        // Run in blocking context since it does DB queries
                        let _ = tokio::task::spawn_blocking(move || {
                            experience_harvester::harvest_session(mc.db(), &sid);
                        }).await;
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    tracing::debug!(skipped = n, "Harvest consumer lagged (non-critical)");
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}

// ===== Phase 3c: Reflection consumers (LIVE) =====
// Activated from Shadow Mode. Each consumer calls the real handler
// function. Old polling paths (autopilot/strategy/retro) removed.

/// Realtime extraction: ConversationMessageLogged → check_realtime_extraction().
/// Phase 3c LIVE: trailing-edge debounce 3s, then dispatches real extraction.
fn spawn_realtime_extraction_consumer(state: &AppState, timeline_tx: &broadcast::Sender<TimelineEvent>) {
    let s = state.clone();
    let mut rx = timeline_tx.subscribe();
    tokio::spawn(async move {
        let mut pending_sessions: std::collections::HashSet<String> = std::collections::HashSet::new();
        loop {
            if !pending_sessions.is_empty() {
                let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
                loop {
                    match tokio::time::timeout_at(deadline, rx.recv()).await {
                        Ok(Ok(te)) => {
                            if let DaemonEvent::ConversationMessageLogged { ref session_id, ref slot_id, .. } = te.event {
                                if slot_id.is_some() {
                                    pending_sessions.insert(session_id.clone());
                                }
                            }
                        }
                        Ok(Err(RecvError::Lagged(n))) => {
                            s.stats.events_lagged_total_skipped.fetch_add(n, Ordering::Relaxed);
                        }
                        Ok(Err(RecvError::Closed)) => return,
                        Err(_) => break,
                    }
                }
                // Gemini ARB: tokio::spawn to avoid blocking broadcast receiver
                tracing::debug!(sessions = pending_sessions.len(), "realtime_extraction: debounce fired");
                let sc = s.clone();
                tokio::spawn(async move { check_realtime_extraction(&sc).await; });
                pending_sessions.clear();
            } else {
                match rx.recv().await {
                    Ok(te) => {
                        if let DaemonEvent::ConversationMessageLogged { ref session_id, ref slot_id, .. } = te.event {
                            if slot_id.is_some() && !s.memory_paused.load(Ordering::Relaxed) {
                                pending_sessions.insert(session_id.clone());
                            }
                        }
                    }
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "Realtime extraction consumer lagged");
                        s.stats.events_lagged_total_skipped.fetch_add(n, Ordering::Relaxed);
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        }
    });
}

/// Session reflection: SessionCompleted → deep analysis + retrospective.
/// Phase 3c LIVE: trailing-edge debounce 5s, then dispatches real analysis.
/// Replaces Strategy Worker 300s poll + Retro Worker 3600s poll.
fn spawn_session_reflection_consumer(state: &AppState, timeline_tx: &broadcast::Sender<TimelineEvent>) {
    let s = state.clone();
    let mut rx = timeline_tx.subscribe();
    tokio::spawn(async move {
        let mut pending_sessions: std::collections::HashSet<String> = std::collections::HashSet::new();
        loop {
            if !pending_sessions.is_empty() {
                let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                loop {
                    match tokio::time::timeout_at(deadline, rx.recv()).await {
                        Ok(Ok(te)) => {
                            if let DaemonEvent::SessionCompleted { ref session_id, ref status, .. } = te.event {
                                if matches!(status, crate::event_bus::SessionEndStatus::Success) {
                                    pending_sessions.insert(session_id.clone());
                                }
                            }
                        }
                        Ok(Err(RecvError::Lagged(n))) => {
                            s.stats.events_lagged_total_skipped.fetch_add(n, Ordering::Relaxed);
                        }
                        Ok(Err(RecvError::Closed)) => return,
                        Err(_) => break,
                    }
                }
                // Gemini ARB: check pause guards + tokio::spawn to avoid blocking receiver
                if s.memory_paused.load(Ordering::Relaxed) || s.global_paused.load(Ordering::Relaxed) {
                    tracing::debug!("session_reflection: paused, skipping dispatch");
                    pending_sessions.clear();
                    continue;
                }
                tracing::info!(sessions = pending_sessions.len(), "session_reflection: debounce fired, dispatching analysis");
                let sc = s.clone();
                tokio::spawn(async move {
                    // Gemini ARB: unified trigger — strategy + deep analysis + retro
                    strategy_worker::run_pending_analysis(&sc).await;
                    check_deep_analysis(&sc).await;
                    if let Err(e) = retro_worker::process_pending(&sc).await {
                        tracing::warn!(error = %e, "session_reflection: retro analysis failed");
                    }
                });
                pending_sessions.clear();
            } else {
                match rx.recv().await {
                    Ok(te) => {
                        if let DaemonEvent::SessionCompleted { ref session_id, ref status, .. } = te.event {
                            if matches!(status, crate::event_bus::SessionEndStatus::Success) {
                                pending_sessions.insert(session_id.clone());
                            }
                        }
                    }
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "Session reflection consumer lagged");
                        s.stats.events_lagged_total_skipped.fetch_add(n, Ordering::Relaxed);
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        }
    });
}

/// KB consolidation: DeepAnalysisCompleted → accumulate N → trigger consolidation.
/// Phase 3c LIVE: counter accumulation, fires check_kb_consolidation at threshold.
/// Replaces 24h timer check in autopilot.
fn spawn_kb_consolidation_consumer(state: &AppState, timeline_tx: &broadcast::Sender<TimelineEvent>) {
    let s = state.clone();
    let mut rx = timeline_tx.subscribe();
    let threshold: u32 = 5; // Trigger consolidation after 5 deep analyses
    tokio::spawn(async move {
        let mut count: u32 = 0;
        loop {
            match rx.recv().await {
                Ok(te) => {
                    if let DaemonEvent::DeepAnalysisCompleted { ref session_id, .. } = te.event {
                        count += 1;
                        tracing::debug!(session_id = %session_id, count, "kb_consolidation: deep analysis completed ({}/{})", count, threshold);
                        if count >= threshold {
                            tracing::info!(count, "kb_consolidation: threshold reached, dispatching");
                            let sc = s.clone();
                            tokio::spawn(async move { check_kb_consolidation(&sc).await; });
                            count = 0;
                        }
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "KB consolidation consumer lagged, defensive trigger");
                    s.stats.events_lagged_total_skipped.fetch_add(n, Ordering::Relaxed);
                    // Gemini ARB: Lagged may have lost DeepAnalysisCompleted events — defensive check
                    let sc = s.clone();
                    tokio::spawn(async move { check_kb_consolidation(&sc).await; });
                    count = 0;
                }
                Err(RecvError::Closed) => break,
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
