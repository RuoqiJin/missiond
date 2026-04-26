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

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use missiond_core::event::events::{
    ExecutionEvent, IncidentEvent, MemoryEvent, MessageEvent, QuestionEvent, SessionEndStatus,
    SessionEvent, SlotEvent, TaskEvent, WorkerEvent,
};
use missiond_core::event::subscription::{Subscription, SubscriptionOpts};
use missiond_core::event::DomainEvent;
use tokio::sync::watch;
use tracing::{debug, info, warn};

use crate::bus::BusServices;
use crate::decision_engine::process_pending_master_questions;
use crate::experience_harvester;
use crate::extraction::{
    check_deep_analysis, check_kb_consolidation, check_realtime_extraction,
};
use crate::handlers::knowledge::directive::{
    handle_review_resolved_event as directive_handle_review_resolved,
    DirectiveSubscriberOutcome,
};
use crate::handlers::knowledge::plan::{
    handle_review_resolved_event as plan_handle_review_resolved, PlanSubscriberOutcome,
};
use crate::handlers::knowledge::review_gate::{
    plan_review_resolved_dispatch, ReviewResolvedDispatch,
};
use crate::handlers::knowledge::workflow::{
    handle_review_resolved_event as workflow_handle_review_resolved,
    WorkflowSubscriberOutcome,
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

    // Incident reactor: every `IncidentEvent::Reported` → aiops::process_incident.
    spawn_incident_reactor(bus.clone(), state.clone(), shutdown_rx.clone());

    // wave-16 :: review-gate Resolved listener — reads QuestionEvent::Resolved
    // and routes deterministic `review:*` ids through the same explicit
    // resolution validators each manager handler owns. Independent
    // Subscription so it doesn't compete with the decision-engine
    // Created-path consumer above.
    spawn_review_resolution_sub(bus.clone(), state.clone(), shutdown_rx.clone());

    // wave-16 / task 07 :: passive ExecutionEvent cache populator. Mirrors
    // every published `PlanNodeStateChanged` into the resolver's in-memory
    // cache so downstream evidence call sites that no longer carry the
    // live `Seq` from the publish path can recover the event id post-hoc.
    // Strictly observation-only — never publishes / mutates DB.
    spawn_event_ref_cache_sub(bus.clone(), shutdown_rx.clone());

    info!("v2 event-bus subscribers started (8 router consumers + 1 incident reactor + 1 review-resolution listener + 1 event-ref cache populator)");
}

/// Incident reactor — subscribes to IncidentEvent and triages via
/// `aiops::process_incident`. Replaces the old `incident_rx` MPSC consumer.
fn spawn_incident_reactor(bus: Arc<BusServices>, state: AppState, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<IncidentEvent>(&bus, "v2_incident_reactor", "incident_reactor").await else {
            return;
        };
        info!("v2[incident_reactor]: subscription live");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    if let IncidentEvent::Reported { incident } = ack.event() {
                        crate::aiops::process_incident(&state, incident.clone()).await;
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[incident_reactor]: shutdown");
    });
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
                match crate::engine::learning_engine::intent_analyst::process_session_intents(&state, &session_id).await {
                    Ok(count) if count > 0 => {
                        debug!(session = %session_id, intents = count, "v2[intent_analyst]: analysis complete");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        debug!(session = %session_id, error = %e, "v2[intent_analyst]: analysis failed");
                    }
                }
            }
        }
        info!("v2[intent_analyst]: shutdown");
    });
}

// Workers (translation / arch_maintenance / lisp_survey / conversation_organizer /
// tagger_chunker / gemini_logger) now own their own v2 subscriptions directly
// (Phase 8 flip). No passive observers remain in this module.

// ═════════════════════════════════════════════════════════════════════════
// wave-16 :: review-gate Resolved listener
//
// The decision subscriber above (`spawn_decision_sub`) handles
// `QuestionEvent::Created`. This listener handles the symmetric
// `QuestionEvent::Resolved` path: when an inbound Resolved event carries
// a deterministic `review:*` envelope, the planner classifies it and the
// per-scope handler bridges the resolution back into the same DB
// transition the explicit caller-side path uses.
//
// Conservatism contract:
//   * Non-review ids are ignored after ack (other consumers may handle).
//   * Malformed review ids → log warning + ack; never mutate.
//   * Unknown resolution strings → log warning + ack; never mutate.
//   * Recognised review id + recognised resolution → route through the
//     handler-side bridge (validates envelope against current DB state,
//     performs DB transition only on Approved + transition action).
//   * The subscriber NEVER re-publishes a Resolved event (the inbound
//     event we just consumed IS the downstream signal).
//   * The subscriber NEVER auto-approves arbitrary text — only
//     deterministic `review:*` ids resolve through this path.
// ═════════════════════════════════════════════════════════════════════════
fn spawn_review_resolution_sub(
    bus: Arc<BusServices>,
    state: AppState,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<QuestionEvent>(
            &bus,
            "v2_review_resolution",
            "review_resolution",
        )
        .await
        else {
            return;
        };
        info!("v2[review_resolution]: subscription live");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    if let QuestionEvent::Resolved { question_id, resolution } = ack.event() {
                        handle_review_resolved(&state, question_id, resolution).await;
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[review_resolution]: shutdown");
    });
}

/// Per-event handler. Pure routing layer — defers all matching to
/// `plan_review_resolved_dispatch` (pure planner) and all DB work to the
/// per-scope handler bridges. Errors are logged + ignored; the bus
/// message has already been (or will be) acked by the spawn loop above.
async fn handle_review_resolved(state: &AppState, question_id: &str, resolution: &str) {
    let dispatch = plan_review_resolved_dispatch(question_id, resolution);
    match dispatch {
        ReviewResolvedDispatch::IgnoreNonReviewId => {
            // Quiet — non-review ids are off-route by design.
            debug!(
                question_id = %question_id,
                "v2[review_resolution]: non-review id, ignoring"
            );
        }
        ReviewResolvedDispatch::IgnoreMalformedId(err) => {
            warn!(
                question_id = %question_id,
                error = %err.message(),
                "v2[review_resolution]: malformed review id, ignoring"
            );
        }
        ReviewResolvedDispatch::IgnoreUnsupportedScope { scope } => {
            warn!(
                question_id = %question_id,
                scope = %scope,
                "v2[review_resolution]: unsupported review scope, ignoring"
            );
        }
        ReviewResolvedDispatch::IgnoreUnknownResolution { resolution } => {
            warn!(
                question_id = %question_id,
                resolution = %resolution,
                "v2[review_resolution]: unknown resolution string, ignoring (no auto-approve for arbitrary text)"
            );
        }
        ReviewResolvedDispatch::Route { parsed, decision } => match parsed.scope.as_str() {
            "directive" => {
                let outcome =
                    directive_handle_review_resolved(state, &parsed, decision).await;
                log_directive_outcome(question_id, &outcome);
            }
            "plan" => {
                let outcome = plan_handle_review_resolved(state, &parsed, decision).await;
                log_plan_outcome(question_id, &outcome);
            }
            "workflow" => {
                let outcome =
                    workflow_handle_review_resolved(state, &parsed, decision).await;
                log_workflow_outcome(question_id, &outcome);
            }
            other => {
                // Defensive: planner should have rejected via
                // IgnoreUnsupportedScope before we got here.
                warn!(
                    question_id = %question_id,
                    scope = %other,
                    "v2[review_resolution]: planner allowed unknown scope through; ignoring"
                );
            }
        },
    }
}

fn log_directive_outcome(qid: &str, outcome: &DirectiveSubscriberOutcome) {
    match outcome {
        DirectiveSubscriberOutcome::Approved => {
            info!(question_id = %qid, "v2[review_resolution]: directive approved via bus");
        }
        DirectiveSubscriberOutcome::Archived => {
            info!(question_id = %qid, "v2[review_resolution]: directive archived via bus");
        }
        DirectiveSubscriberOutcome::KeptArtifact { decision } => {
            info!(question_id = %qid, decision = decision.as_str(), "v2[review_resolution]: directive kept (rejected/needs_changes)");
        }
        DirectiveSubscriberOutcome::CompileNoOp { decision } => {
            debug!(question_id = %qid, decision = decision.as_str(), "v2[review_resolution]: directive compile-action no-op");
        }
        DirectiveSubscriberOutcome::ArtifactIdNotUuid { artifact_id, error } => {
            warn!(question_id = %qid, artifact_id = %artifact_id, error = %error, "v2[review_resolution]: directive artifact_id not a UUID");
        }
        DirectiveSubscriberOutcome::NotFound { artifact_id } => {
            warn!(question_id = %qid, artifact_id = %artifact_id, "v2[review_resolution]: directive not found");
        }
        DirectiveSubscriberOutcome::EnvelopeRejected { code, message } => {
            warn!(question_id = %qid, code = %code, error = %message, "v2[review_resolution]: directive envelope rejected");
        }
        DirectiveSubscriberOutcome::DbError { detail } => {
            warn!(question_id = %qid, error = %detail, "v2[review_resolution]: directive DB error");
        }
    }
}

fn log_plan_outcome(qid: &str, outcome: &PlanSubscriberOutcome) {
    match outcome {
        PlanSubscriberOutcome::Approved => {
            info!(question_id = %qid, "v2[review_resolution]: plan approved via bus");
        }
        PlanSubscriberOutcome::MarkNeedsExplicitCall => {
            warn!(question_id = %qid, "v2[review_resolution]: plan mark requires explicit caller (qid envelope lacks target status)");
        }
        PlanSubscriberOutcome::SupersedeNeedsExplicitCall => {
            warn!(question_id = %qid, "v2[review_resolution]: plan supersede requires explicit caller (qid envelope lacks new_plan_id)");
        }
        PlanSubscriberOutcome::KeptArtifact { decision } => {
            info!(question_id = %qid, decision = decision.as_str(), "v2[review_resolution]: plan kept (rejected/needs_changes)");
        }
        PlanSubscriberOutcome::CompileNoOp { decision } => {
            debug!(question_id = %qid, decision = decision.as_str(), "v2[review_resolution]: plan compile-action no-op");
        }
        PlanSubscriberOutcome::ArtifactIdNotUuid { artifact_id, error } => {
            warn!(question_id = %qid, artifact_id = %artifact_id, error = %error, "v2[review_resolution]: plan artifact_id not a UUID");
        }
        PlanSubscriberOutcome::NotFound { artifact_id } => {
            warn!(question_id = %qid, artifact_id = %artifact_id, "v2[review_resolution]: plan not found");
        }
        PlanSubscriberOutcome::EnvelopeRejected { code, message } => {
            warn!(question_id = %qid, code = %code, error = %message, "v2[review_resolution]: plan envelope rejected");
        }
        PlanSubscriberOutcome::DbError { detail } => {
            warn!(question_id = %qid, error = %detail, "v2[review_resolution]: plan DB error");
        }
    }
}

fn log_workflow_outcome(qid: &str, outcome: &WorkflowSubscriberOutcome) {
    match outcome {
        WorkflowSubscriberOutcome::PersistedReceipt { workflow_id, decision } => {
            info!(question_id = %qid, workflow_id = %workflow_id, decision = decision.as_str(), "v2[review_resolution]: workflow persisted receipt (no DB transition; row has no status column)");
        }
        WorkflowSubscriberOutcome::MethodologyReceipt { flow_id, decision } => {
            info!(question_id = %qid, flow_id = %flow_id, decision = decision.as_str(), "v2[review_resolution]: workflow methodology receipt");
        }
        WorkflowSubscriberOutcome::NotFound { artifact_id } => {
            warn!(question_id = %qid, artifact_id = %artifact_id, "v2[review_resolution]: workflow not found");
        }
        WorkflowSubscriberOutcome::EnvelopeRejected { code, message } => {
            warn!(question_id = %qid, code = %code, error = %message, "v2[review_resolution]: workflow envelope rejected");
        }
        WorkflowSubscriberOutcome::DbError { detail } => {
            warn!(question_id = %qid, error = %detail, "v2[review_resolution]: workflow DB error");
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════
// wave-16 / task 07 :: ExecutionEvent passive cache populator
//
// Subscribes to the execution-domain topic and mirrors every observed
// `PlanNodeStateChanged` event into the shared `EventRefResolver` cache.
// Downstream evidence-collector call sites that no longer carry the live
// `Seq` from the publish path (because they ran out-of-band of the
// dispatch task) can then call
// `bus.event_ref_resolver.lookup_plan_node_state_change(...)` to recover
// the id with `EventRefStatus::Log`.
//
// Conservatism contract:
//   * Observation-only — NEVER publishes a downstream event, NEVER mutates
//     DB. Subscriber acks every message.
//   * Cache miss / lookup failure NEVER fails the dispatch — the resolver
//     returns `EventRef::unavailable(...)` instead of erroring.
//   * Bounded retention (`EVENT_REF_CACHE_CAP=1024`) so a long-running
//     daemon doesn't grow the cache unboundedly.
//   * Other ExecutionEvent variants (Opened / Claimed / Completed / …)
//     are ignored after ack — wave-16 / task 07 only wires the plan-node
//     correlation key. Future kinds can extend the subscriber without
//     changing the dispatch path.
// ═════════════════════════════════════════════════════════════════════════
fn spawn_event_ref_cache_sub(bus: Arc<BusServices>, mut shutdown: watch::Receiver<bool>) {
    let resolver = bus.event_ref_resolver.clone();
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<ExecutionEvent>(
            &bus,
            "v2_event_ref_cache",
            "event_ref_cache",
        )
        .await
        else {
            return;
        };
        info!("v2[event_ref_cache]: subscription live (passive cache populator)");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    if let ExecutionEvent::PlanNodeStateChanged {
                        plan_id,
                        node_id,
                        from,
                        to,
                        attempt,
                        ..
                    } = ack.event()
                    {
                        // Live `Seq` lives on the AckHandle itself.
                        let seq = ack.seq().0.to_string();
                        let attempt_n = attempt.unwrap_or(1);
                        resolver.record_plan_node_state_change(
                            plan_id,
                            node_id,
                            attempt_n,
                            from,
                            to,
                            "execution",
                            "plan_node_state_changed",
                            seq,
                        );
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[event_ref_cache]: shutdown");
    });
}

// ───────────────────────────────────────────────────────────────────────
// tests — pure routing layer (no bus, no DB)
//
// Direct subscriber tests would require building a full bus + AppState;
// the brief explicitly endorses "extract a pure planner and test that"
// for that case. The pure planner tests live in
// `handlers::knowledge::review_gate::tests`. Here we pin a couple of
// router-shape invariants on the dispatch enum so future refactors don't
// silently regress (e.g. someone adding an enum variant without wiring
// the warn! arm).
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::knowledge::review_gate::ReviewDecision;

    #[test]
    fn dispatch_routes_resident_three_scopes_per_envelope() {
        // Pin the assumption that the planner only routes on the three
        // wave-14 scopes; the subscriber's match arm relies on this.
        for (qid, scope) in [
            ("review:directive:abc:v1:approve", "directive"),
            ("review:plan:abc:v1:approve", "plan"),
            ("review:workflow:abc:v1:compile", "workflow"),
        ] {
            let d = plan_review_resolved_dispatch(qid, "approved");
            match d {
                ReviewResolvedDispatch::Route { parsed, decision } => {
                    assert_eq!(parsed.scope, scope);
                    assert_eq!(decision, ReviewDecision::Approved);
                }
                other => panic!("expected Route for `{}`, got {:?}", qid, other),
            }
        }
    }

    #[test]
    fn dispatch_unknown_resolution_does_not_route() {
        // A wave-14-shaped review id with garbage resolution string MUST
        // hit IgnoreUnknownResolution rather than Route — this is the
        // "no auto-approve for arbitrary text" guarantee.
        let d = plan_review_resolved_dispatch(
            "review:directive:abc:v1:approve",
            "looks-good-to-me",
        );
        assert!(matches!(
            d,
            ReviewResolvedDispatch::IgnoreUnknownResolution { .. }
        ));
    }

    #[test]
    fn dispatch_non_review_id_does_not_route() {
        // Decision-engine `master:*` ids must be ignored — the subscriber
        // never auto-approves non-review questions.
        let d = plan_review_resolved_dispatch("master:question:abc", "approved");
        assert!(matches!(d, ReviewResolvedDispatch::IgnoreNonReviewId));
    }
}

