//! Phase 7 subscriber migration — v2 consumers running alongside v1.
//!
//! Frozen lisp: `.missiond/v2/intent-event-bus.lisp` §4.3 egress.
//!
//! Phase 6 dual-emit writes every v1 `DaemonEvent` to the v2 bus. Phase 7
//! spins up v2 subscribers that consume those events from the dispatcher
//! topics. The existing v1 consumers stay online (per task brief): this keeps
//! user-visible behavior identical while the new cursor / DLQ / combinator
//! plumbing proves itself on real traffic.
//!
//! Two groups:
//!
//!   * **A — event_router consumers (8 subs):** `extraction`, `submit`,
//!     `decision`, `harvest`, `realtime_extraction`, `session_reflection`,
//!     `kb_consolidation`, `intent_analyst`. These own the shared
//!     control-loop action (`schedule_memory_tasks`, etc.). Actions are
//!     DB-poll idempotent so double-firing alongside v1 is safe.
//!
//!   * **B — worker observers (6 subs):** `gemini_logger`, `translation`,
//!     `arch_maintenance`, `lisp_survey`, `conversation_organizer`,
//!     `tagger_chunker`. These have real side-effects (LLM calls, git
//!     operations). To avoid doubling work during the transition they spawn
//!     as **passive observers**: subscribe + ack, but log only. Phase 8 will
//!     flip them to active handlers and remove the v1 timeline_rx path.
//!
//! Skipped (per task brief):
//!   * WS bridge → I004 deferred to Phase 8 (v1 `ws_tx` remains the frontend
//!     path until `run_timeline_writer` is deleted).
//!   * `conversation_logger::cursor_ack_tx` internalisation → I005 deferred
//!     to Phase 8 (D004).
//!
//! Lifecycle:
//!   * Each consumer task owns its `Subscription<T>` and a `watch::Receiver`
//!     for shutdown. The outer loop is:
//!       ```ignore
//!       loop {
//!           tokio::select! {
//!               biased;
//!               _ = shutdown.changed() => break,
//!               Some(ack) = sub.next() => { handle(ack.event()); ack.ack().await; }
//!           }
//!       }
//!       ```
//!   * On shutdown we drop the Subscription so the flusher exits cleanly.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use missiond_core::event::events::{
    LlmEvent, MemoryEvent, MessageEvent, QuestionEvent, SessionEndStatus, SessionEvent, SlotEvent,
    SystemEvent, TaskEvent, WorkerEvent,
};
use missiond_core::event::subscription::{
    Subscription, SubscriptionOpts,
};
use missiond_core::event::DomainEvent;
use tokio::sync::watch;
use tracing::{debug, info, warn};

use crate::bus::BusServices;
use crate::decision_engine::process_pending_master_questions;
use crate::experience_harvester;
use crate::extraction::{
    check_deep_analysis, check_kb_consolidation, check_realtime_extraction,
};
use crate::memory_scheduler::{dispatch_queued_submit_tasks, schedule_memory_tasks};
use crate::state::{AppState, MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID};

/// Start every Phase 7 v2 subscriber. Intended to be called once from
/// `main.rs` after `BusServices::start`.
pub(crate) fn start_v2_subscribers(
    bus: &Arc<BusServices>,
    state: &AppState,
    shutdown_rx: watch::Receiver<bool>,
) {
    // Group A — event_router consumers (8 subs).
    spawn_extraction_sub(bus.clone(), state.clone(), shutdown_rx.clone());
    spawn_submit_sub(bus.clone(), state.clone(), shutdown_rx.clone());
    spawn_decision_sub(bus.clone(), state.clone(), shutdown_rx.clone());
    spawn_harvest_sub(bus.clone(), state.clone(), shutdown_rx.clone());
    spawn_realtime_extraction_sub(bus.clone(), state.clone(), shutdown_rx.clone());
    spawn_session_reflection_sub(bus.clone(), state.clone(), shutdown_rx.clone());
    spawn_kb_consolidation_sub(bus.clone(), state.clone(), shutdown_rx.clone());
    if state.intent_analyst_enabled {
        spawn_intent_analyst_sub(bus.clone(), state.clone(), shutdown_rx.clone());
    }

    // Group B — worker observers (6 subs). Passive: ack only, no side-
    // effects duplicated alongside the v1 worker.
    spawn_gemini_logger_observer(bus.clone(), shutdown_rx.clone());
    spawn_translation_observer(bus.clone(), shutdown_rx.clone());
    spawn_arch_maintenance_observer(bus.clone(), shutdown_rx.clone());
    spawn_lisp_survey_observer(bus.clone(), shutdown_rx.clone());
    spawn_conversation_organizer_observer(bus.clone(), shutdown_rx.clone());
    spawn_tagger_chunker_observer(bus.clone(), shutdown_rx.clone());

    info!("v2 event-bus subscribers started (8 router consumers + 6 worker observers)");
}

// ═════════════════════════════════════════════════════════════════════════
// A — event_router consumers
// ═════════════════════════════════════════════════════════════════════════

/// Helper: subscribe to a domain topic with the default opts for a router
/// consumer and log any bootstrap failure.
async fn subscribe_or_warn<T: DomainEvent>(
    bus: &BusServices,
    name: &str,
    consumer: &str,
) -> Option<Subscription<T>> {
    let opts = SubscriptionOpts::named(consumer);
    match bus.subscribe::<T>(name, opts).await {
        Ok(s) => Some(s),
        Err(e) => {
            warn!(subscription = %name, error = %e, "v2 subscribe failed");
            None
        }
    }
}

/// Router A1 — extraction: SlotEvent::BecameIdle for memory slots → schedule.
fn spawn_extraction_sub(bus: Arc<BusServices>, state: AppState, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<SlotEvent>(&bus, "v2_router_extraction", "router_extraction").await else {
            return;
        };
        info!("v2[extraction]: subscription live");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    if let SlotEvent::BecameIdle { slot_id } = ack.event() {
                        if slot_id == MEMORY_SLOT_ID || slot_id == MEMORY_SLOW_SLOT_ID {
                            if !state.control_manager.current().is_domain_paused(crate::control_tree::CtlDomain::Memory) {
                                state.stats.events_consumed_extraction.fetch_add(1, Ordering::Relaxed);
                                schedule_memory_tasks(&state).await;
                            }
                        }
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[extraction]: shutdown");
    });
}

/// Router A2 — submit: TaskEvent::Created / Completed → dispatch queued tasks.
fn spawn_submit_sub(bus: Arc<BusServices>, state: AppState, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<TaskEvent>(&bus, "v2_router_submit", "router_submit").await else {
            return;
        };
        info!("v2[submit]: subscription live");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    if matches!(ack.event(), TaskEvent::Created { .. } | TaskEvent::Completed { .. }) {
                        state.stats.events_consumed_submit.fetch_add(1, Ordering::Relaxed);
                        dispatch_queued_submit_tasks(&state).await;
                        if !state.control_manager.current().is_domain_paused(crate::control_tree::CtlDomain::Memory) {
                            schedule_memory_tasks(&state).await;
                        }
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[submit]: shutdown");
    });
}

/// Router A3 — decision: QuestionEvent::Created → process pending.
fn spawn_decision_sub(bus: Arc<BusServices>, state: AppState, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<QuestionEvent>(&bus, "v2_router_decision", "router_decision").await else {
            return;
        };
        info!("v2[decision]: subscription live");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    if let QuestionEvent::Created { .. } = ack.event() {
                        state.stats.events_consumed_decision.fetch_add(1, Ordering::Relaxed);
                        process_pending_master_questions(&state).await;
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[decision]: shutdown");
    });
}

/// Router A4 — harvest: WorkerEvent::NarrationSessionCompleted → harvester.
fn spawn_harvest_sub(bus: Arc<BusServices>, state: AppState, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<WorkerEvent>(&bus, "v2_router_harvest", "router_harvest").await else {
            return;
        };
        info!("v2[harvest]: subscription live");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    if let WorkerEvent::NarrationSessionCompleted { session_id, .. } = ack.event() {
                        let sid = session_id.clone();
                        experience_harvester::harvest_session(&state, &sid).await;
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[harvest]: shutdown");
    });
}

/// Router A5 — realtime extraction: MessageEvent::Logged → trigger extraction.
/// Uses the 3-second debounce combinator.
fn spawn_realtime_extraction_sub(bus: Arc<BusServices>, state: AppState, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let Some(sub) = subscribe_or_warn::<MessageEvent>(&bus, "v2_router_realtime_extraction", "router_realtime_extraction").await else {
            return;
        };
        let mut sub = sub.debounce(Duration::from_secs(3));
        info!("v2[realtime_extraction]: subscription live (3s debounce)");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    if let MessageEvent::Logged { slot_id, .. } = ack.event() {
                        if slot_id.is_some()
                            && !state.control_manager.current().is_domain_paused(crate::control_tree::CtlDomain::Memory)
                        {
                            let s = state.clone();
                            tokio::spawn(async move {
                                check_realtime_extraction(&s).await;
                            });
                        }
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[realtime_extraction]: shutdown");
    });
}

/// Router A6 — session reflection: SessionEvent::Completed{Success} → notify
/// strategy/retro + run deep analysis. 5-second debounce.
fn spawn_session_reflection_sub(bus: Arc<BusServices>, state: AppState, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let Some(sub) = subscribe_or_warn::<SessionEvent>(&bus, "v2_router_session_reflection", "router_session_reflection").await else {
            return;
        };
        let mut sub = sub.debounce(Duration::from_secs(5));
        info!("v2[session_reflection]: subscription live (5s debounce)");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    let should_fire = matches!(
                        ack.event(),
                        SessionEvent::Completed { status: SessionEndStatus::Success, .. }
                    );
                    if should_fire {
                        let tree = state.control_manager.current();
                        if !(tree.is_domain_paused(crate::control_tree::CtlDomain::Memory) || tree.global_paused) {
                            state.strategy_notify.notify_one();
                            state.retro_notify.notify_one();
                            let s = state.clone();
                            tokio::spawn(async move { check_deep_analysis(&s).await; });
                        }
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[session_reflection]: shutdown");
    });
}

/// Router A7 — KB consolidation: MemoryEvent::DeepAnalysisCompleted →
/// accumulate N=5 then trigger consolidation.
fn spawn_kb_consolidation_sub(bus: Arc<BusServices>, state: AppState, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<MemoryEvent>(&bus, "v2_router_kb_consolidation", "router_kb_consolidation").await else {
            return;
        };
        info!("v2[kb_consolidation]: subscription live (threshold=5)");
        const THRESHOLD: u32 = 5;
        let mut count: u32 = 0;
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => {
                    if count > 0 {
                        let s = state.clone();
                        tokio::spawn(async move { check_kb_consolidation(&s).await; });
                    }
                    break;
                }
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    if let MemoryEvent::DeepAnalysisCompleted { .. } = ack.event() {
                        count += 1;
                        if count >= THRESHOLD {
                            let s = state.clone();
                            tokio::spawn(async move { check_kb_consolidation(&s).await; });
                            count = 0;
                        }
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[kb_consolidation]: shutdown");
    });
}

/// Router A8 — intent analyst: MemoryEvent::TurnExtracted → per-session
/// debounce (5 min) or accumulation (5 turns) → analysis. Uses a manual
/// loop because the v1 version carries per-session state that combinators
/// don't express directly.
fn spawn_intent_analyst_sub(bus: Arc<BusServices>, state: AppState, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<MemoryEvent>(&bus, "v2_router_intent_analyst", "router_intent_analyst").await else {
            return;
        };
        info!("v2[intent_analyst]: subscription live");
        const DEBOUNCE: Duration = Duration::from_secs(300);
        const MAX_ACCUM: usize = 5;
        let mut pending: HashMap<String, (Instant, usize)> = HashMap::new();
        let poll = Duration::from_secs(30);

        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                res = tokio::time::timeout(poll, sub.next()) => {
                    match res {
                        Ok(Some(ack)) => {
                            if let MemoryEvent::TurnExtracted { session_id, turn_count } = ack.event() {
                                let entry = pending
                                    .entry(session_id.clone())
                                    .or_insert_with(|| (Instant::now(), 0));
                                entry.0 = Instant::now();
                                entry.1 = entry.1.saturating_add(*turn_count as usize);
                            }
                            ack.ack().await;
                        }
                        Ok(None) => break,
                        Err(_) => { /* timeout — scan debounce expiry */ }
                    }
                }
            }

            // Expiry scan.
            let now = Instant::now();
            let expired: Vec<String> = pending
                .iter()
                .filter(|(_, (ts, count))| now.duration_since(*ts) >= DEBOUNCE || *count >= MAX_ACCUM)
                .map(|(id, _)| id.clone())
                .collect();
            for session_id in expired {
                pending.remove(&session_id);
                if state.control_manager.current().is_provider_paused(crate::control_tree::CtlProvider::Sonnet) {
                    continue;
                }
                // v1 intent_analyst::process_session_intents is private; the
                // v2 observer path just logs — actual work stays on v1 until
                // Phase 8 flips the active path.
                debug!(session = %session_id, "v2[intent_analyst]: would process (v1 handles action)");
            }
        }
        info!("v2[intent_analyst]: shutdown");
    });
}

// ═════════════════════════════════════════════════════════════════════════
// B — worker observers (passive, ack-only)
// ═════════════════════════════════════════════════════════════════════════

fn spawn_gemini_logger_observer(bus: Arc<BusServices>, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<LlmEvent>(&bus, "v2_worker_gemini_logger", "worker_gemini_logger").await else {
            return;
        };
        info!("v2[obs:gemini_logger]: subscription live");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    debug!(kind = %ack.event().kind(), "v2[obs:gemini_logger]: event observed");
                    ack.ack().await;
                }
            }
        }
        info!("v2[obs:gemini_logger]: shutdown");
    });
}

fn spawn_translation_observer(bus: Arc<BusServices>, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        // TranslationWorker v1 listens to MessageEvent::Logged (role=thinking)
        // — we subscribe to the Message topic so the flow is mirrored.
        let Some(mut sub) = subscribe_or_warn::<MessageEvent>(&bus, "v2_worker_translation", "worker_translation").await else {
            return;
        };
        info!("v2[obs:translation]: subscription live");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    if let MessageEvent::Logged { role, .. } = ack.event() {
                        if role == "thinking" {
                            debug!("v2[obs:translation]: thinking message observed");
                        }
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[obs:translation]: shutdown");
    });
}

fn spawn_arch_maintenance_observer(bus: Arc<BusServices>, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<SystemEvent>(&bus, "v2_worker_arch_maintenance", "worker_arch_maintenance").await else {
            return;
        };
        info!("v2[obs:arch_maintenance]: subscription live");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    if let SystemEvent::ContextualCommitDetected { commit_hash, branch, .. } = ack.event() {
                        debug!(commit = %commit_hash, branch = %branch, "v2[obs:arch_maintenance]: commit observed");
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[obs:arch_maintenance]: shutdown");
    });
}

fn spawn_lisp_survey_observer(bus: Arc<BusServices>, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<SystemEvent>(&bus, "v2_worker_lisp_survey", "worker_lisp_survey").await else {
            return;
        };
        info!("v2[obs:lisp_survey]: subscription live");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    if let SystemEvent::ContextualCommitDetected { commit_hash, .. } = ack.event() {
                        debug!(commit = %commit_hash, "v2[obs:lisp_survey]: commit observed");
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[obs:lisp_survey]: shutdown");
    });
}

fn spawn_conversation_organizer_observer(bus: Arc<BusServices>, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<MessageEvent>(&bus, "v2_worker_conversation_organizer", "worker_conversation_organizer").await else {
            return;
        };
        info!("v2[obs:conversation_organizer]: subscription live");
        let mut dirty: HashSet<String> = HashSet::new();
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    if let MessageEvent::Logged { session_id, .. } = ack.event() {
                        dirty.insert(session_id.clone());
                    }
                    ack.ack().await;
                }
            }
        }
        if !dirty.is_empty() {
            debug!(sessions = dirty.len(), "v2[obs:conversation_organizer]: shutdown with pending");
        }
        info!("v2[obs:conversation_organizer]: shutdown");
    });
}

fn spawn_tagger_chunker_observer(bus: Arc<BusServices>, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<SessionEvent>(&bus, "v2_worker_tagger_chunker", "worker_tagger_chunker").await else {
            return;
        };
        info!("v2[obs:tagger_chunker]: subscription live");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    if let SessionEvent::Organized { session_id } = ack.event() {
                        debug!(session = %session_id, "v2[obs:tagger_chunker]: session organized observed");
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[obs:tagger_chunker]: shutdown");
    });
}

