//! Event Router — spawns event-driven consumer tasks for the daemon.
//!
//! Extracted from main.rs (Phase 2 S7, R4) to prevent main.rs from becoming
//! a new God Module. Each consumer runs in its own tokio task and filters
//! relevant DaemonEvent variants.
//!
//! Architecture:
//! - Trailing-edge debounce: events absorbed during window, handler fires once at expiry
//! - Sweeper reconciliation: 30min + startup scan catches events lost to Lagged/restart
//! - Graceful shutdown: consumers flush pending work on SIGTERM via watch channel
//!
//! Consumers subscribe to broadcast<TimelineEvent>, access inner DaemonEvent via .event field.

use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::watch;

use crate::event_bus::{DaemonEvent, TimelineEvent};
use crate::experience_harvester;
use crate::state::{AppState, MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID};
use crate::memory_scheduler::{schedule_memory_tasks, dispatch_queued_submit_tasks};
use crate::decision_engine::process_pending_master_questions;
use crate::extraction::{check_realtime_extraction, check_deep_analysis, check_kb_consolidation, check_kb_reflection};
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
pub(crate) fn start_event_consumers(
    state: &AppState,
    timeline_tx: &broadcast::Sender<TimelineEvent>,
    shutdown_rx: watch::Receiver<bool>,
) {
    spawn_extraction_consumer(state, timeline_tx, shutdown_rx.clone());
    spawn_submit_consumer(state, timeline_tx, shutdown_rx.clone());
    spawn_decision_consumer(state, timeline_tx, shutdown_rx.clone());
    spawn_harvest_consumer(state, timeline_tx, shutdown_rx.clone());
    spawn_realtime_extraction_consumer(state, timeline_tx, shutdown_rx.clone());
    spawn_session_reflection_consumer(state, timeline_tx, shutdown_rx.clone());
    spawn_kb_consolidation_consumer(state, timeline_tx, shutdown_rx);
    spawn_sweeper(state);
}

/// Extraction lane: SlotBecameIdle → schedule_memory_tasks.
/// Trailing-edge debounce: 500ms window, fires after window expires.
fn spawn_extraction_consumer(state: &AppState, timeline_tx: &broadcast::Sender<TimelineEvent>, shutdown_rx: watch::Receiver<bool>) {
    let s = state.clone();
    let mut rx = timeline_tx.subscribe();
    let mut shutdown = shutdown_rx;
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
                if *shutdown.borrow() { return; }
            } else {
                tokio::select! {
                    biased;
                    _ = shutdown.changed() => {
                        tracing::info!("extraction_consumer: shutdown");
                        return;
                    }
                    result = rx.recv() => match result {
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
                        Err(RecvError::Closed) => return,
                    }
                }
            }
        }
    });
}

/// Submit dispatcher: TaskCreated/TaskCompleted → dispatch.
/// Trailing-edge debounce: 100ms window.
fn spawn_submit_consumer(state: &AppState, timeline_tx: &broadcast::Sender<TimelineEvent>, shutdown_rx: watch::Receiver<bool>) {
    let s = state.clone();
    let mut rx = timeline_tx.subscribe();
    let mut shutdown = shutdown_rx;
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
                if *shutdown.borrow() { return; }
            } else {
                tokio::select! {
                    biased;
                    _ = shutdown.changed() => {
                        tracing::info!("submit_consumer: shutdown");
                        return;
                    }
                    result = rx.recv() => match result {
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
                        Err(RecvError::Closed) => return,
                    }
                }
            }
        }
    });
}

/// Decision engine: QuestionCreated → process.
/// Trailing-edge debounce: 100ms window.
fn spawn_decision_consumer(state: &AppState, timeline_tx: &broadcast::Sender<TimelineEvent>, shutdown_rx: watch::Receiver<bool>) {
    let s = state.clone();
    let mut rx = timeline_tx.subscribe();
    let mut shutdown = shutdown_rx;
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
                if *shutdown.borrow() { return; }
            } else {
                tokio::select! {
                    biased;
                    _ = shutdown.changed() => {
                        tracing::info!("decision_consumer: shutdown");
                        return;
                    }
                    result = rx.recv() => match result {
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
                        Err(RecvError::Closed) => return,
                    }
                }
            }
        }
    });
}

/// Experience Harvest: NarrationSessionCompleted → harvest_session.
/// No debounce — fires once per session completion, already infrequent.
fn spawn_harvest_consumer(state: &AppState, timeline_tx: &broadcast::Sender<TimelineEvent>, shutdown_rx: watch::Receiver<bool>) {
    let s = state.clone();
    let mut rx = timeline_tx.subscribe();
    let mut shutdown = shutdown_rx;
    tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => {
                    tracing::info!("harvest_consumer: shutdown");
                    return;
                }
                result = rx.recv() => match result {
                    Ok(te) => {
                        if let DaemonEvent::NarrationSessionCompleted { session_id, .. } = &te.event {
                            let sid = session_id.clone();
                            experience_harvester::harvest_session(&s, &sid).await;
                        }
                    }
                    Err(RecvError::Lagged(n)) => {
                        tracing::debug!(skipped = n, "Harvest consumer lagged (non-critical)");
                    }
                    Err(RecvError::Closed) => return,
                }
            }
        }
    });
}

// ===== Reflection consumers — event-driven dispatch =====

/// Realtime extraction: ConversationMessageLogged → check_realtime_extraction().
/// Trailing-edge debounce: 3s window, then dispatches extraction.
fn spawn_realtime_extraction_consumer(state: &AppState, timeline_tx: &broadcast::Sender<TimelineEvent>, shutdown_rx: watch::Receiver<bool>) {
    let s = state.clone();
    let mut rx = timeline_tx.subscribe();
    let mut shutdown = shutdown_rx;
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
                if *shutdown.borrow() {
                    tracing::info!("realtime_extraction_consumer: shutdown after flush");
                    return;
                }
            } else {
                tokio::select! {
                    biased;
                    _ = shutdown.changed() => {
                        tracing::info!("realtime_extraction_consumer: shutdown");
                        return;
                    }
                    result = rx.recv() => match result {
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
                        Err(RecvError::Closed) => return,
                    }
                }
            }
        }
    });
}

/// Session reflection: SessionCompleted → deep analysis + retrospective.
/// Trailing-edge debounce: 5s window. Replaces Strategy Worker + Retro Worker polling.
fn spawn_session_reflection_consumer(state: &AppState, timeline_tx: &broadcast::Sender<TimelineEvent>, shutdown_rx: watch::Receiver<bool>) {
    let s = state.clone();
    let mut rx = timeline_tx.subscribe();
    let mut shutdown = shutdown_rx;
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
                if *shutdown.borrow() {
                    tracing::info!("session_reflection_consumer: shutdown after flush");
                    return;
                }
            } else {
                tokio::select! {
                    biased;
                    _ = shutdown.changed() => {
                        tracing::info!("session_reflection_consumer: shutdown");
                        return;
                    }
                    result = rx.recv() => match result {
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
                        Err(RecvError::Closed) => return,
                    }
                }
            }
        }
    });
}

/// KB consolidation: DeepAnalysisCompleted → accumulate N → trigger consolidation.
/// Counter accumulation, fires check_kb_consolidation at threshold.
fn spawn_kb_consolidation_consumer(state: &AppState, timeline_tx: &broadcast::Sender<TimelineEvent>, shutdown_rx: watch::Receiver<bool>) {
    let s = state.clone();
    let mut rx = timeline_tx.subscribe();
    let mut shutdown = shutdown_rx;
    let threshold: u32 = 5; // Trigger consolidation after 5 deep analyses
    tokio::spawn(async move {
        let mut count: u32 = 0;
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => {
                    // Gemini audit: flush accumulated count on shutdown
                    if count > 0 {
                        tracing::info!(count, "kb_consolidation_consumer: shutdown flush");
                        let sc = s.clone();
                        tokio::spawn(async move { check_kb_consolidation(&sc).await; });
                    }
                    tracing::info!("kb_consolidation_consumer: shutdown");
                    return;
                }
                result = rx.recv() => match result {
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
                        // Gemini ARB: Lagged may have lost DeepAnalysisCompleted — defensive check
                        let sc = s.clone();
                        tokio::spawn(async move { check_kb_consolidation(&sc).await; });
                        count = 0;
                    }
                    Err(RecvError::Closed) => return,
                }
            }
        }
    });
}

// ===== Sweeper reconciliation =====

/// Sweeper: 30min periodic + startup scan.
/// Catches sessions missed by event consumers (Lagged, restart, debounce loss).
fn spawn_sweeper(state: &AppState) {
    let s = state.clone();
    tokio::spawn(async move {
        // Startup: immediate scan
        reconciliation_sweep(&s).await;

        let mut interval = tokio::time::interval(Duration::from_secs(1800)); // 30min
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            reconciliation_sweep(&s).await;
        }
    });
}

/// Full reconciliation sweep: covers all event-driven pipelines.
async fn reconciliation_sweep(state: &AppState) {
    tracing::info!("Sweeper: starting reconciliation sweep");

    // Gemini ARB: realtime extraction (ConversationMessageLogged easily Lagged)
    check_realtime_extraction(state).await;

    // Strategy + deep analysis: pending sessions
    strategy_worker::run_pending_analysis(state).await;
    check_deep_analysis(state).await;

    // Retro: sessions needing retrospective
    if let Err(e) = retro_worker::process_pending(state).await {
        tracing::warn!(error = %e, "Sweeper: retro reconciliation failed");
    }

    // KB consolidation: check if overdue
    check_kb_consolidation(state).await;

    // Phase 4c: Weekly KB reflection on low-utility entries
    check_kb_reflection(state).await;

    tracing::info!("Sweeper: reconciliation sweep complete");
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
