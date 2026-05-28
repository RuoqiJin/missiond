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
    BoardEvent, ExecutionEvent, IncidentEvent, MemoryEvent, MessageEvent, QuestionEvent,
    SessionEndStatus, SessionEvent, SlotEvent, SystemEvent, TaskEvent, WorkerEvent,
};
use missiond_core::event::subscription::{Subscription, SubscriptionOpts};
use missiond_core::event::DomainEvent;
use missiond_core::types::CreateBoardTaskInput;
use serde_json::Value;
use tokio::sync::watch;
use tracing::{debug, info, warn};

use crate::bus::BusServices;
use crate::decision_engine::process_pending_master_questions;
use crate::experience_harvester;
use crate::extraction::{check_deep_analysis, check_kb_consolidation, check_realtime_extraction};
use crate::handlers::knowledge::directive::{
    handle_review_resolved_event as directive_handle_review_resolved, DirectiveSubscriberOutcome,
};
use crate::handlers::knowledge::plan::{
    handle_review_resolved_event as plan_handle_review_resolved, PlanSubscriberOutcome,
};
use crate::handlers::knowledge::plan_dag::{
    handle_review_resolved_plan_node_event as plan_node_handle_review_resolved,
    PlanNodeResumeListenerOutcome,
};
use crate::handlers::knowledge::review_gate::{
    is_plan_node_review_action, plan_review_resolved_dispatch, ReviewResolvedDispatch,
};
use crate::handlers::knowledge::workflow::{
    handle_review_resolved_event as workflow_handle_review_resolved, WorkflowSubscriberOutcome,
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
    spawn_autopilot_board_event_sub(bus.clone(), state.clone(), shutdown_rx.clone());
    spawn_autopilot_slot_event_sub(bus.clone(), state.clone(), shutdown_rx.clone());
    spawn_deployment_event_response_sub(bus.clone(), state.clone(), shutdown_rx.clone());
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

    info!(
        "v2 event-bus subscribers started (8 router consumers + 2 autopilot handoff nerves + 1 deployment event response nerve + 1 incident reactor + 1 review-resolution listener + 1 event-ref cache populator)"
    );
}

/// Incident reactor — subscribes to IncidentEvent and triages via
/// `aiops::process_incident`. Replaces the old `incident_rx` MPSC consumer.
fn spawn_incident_reactor(
    bus: Arc<BusServices>,
    state: AppState,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let Some(mut sub) =
            subscribe_or_warn::<IncidentEvent>(&bus, "v2_incident_reactor", "incident_reactor")
                .await
        else {
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
fn spawn_extraction_sub(
    bus: Arc<BusServices>,
    state: AppState,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let Some(mut sub) =
            subscribe_or_warn::<SlotEvent>(&bus, "v2_router_extraction", "router_extraction").await
        else {
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
        let Some(mut sub) =
            subscribe_or_warn::<TaskEvent>(&bus, "v2_router_submit", "router_submit").await
        else {
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

/// Autopilot nerve A — BoardTask writes should wake board dispatch through the
/// event bus. The subscriber only nudges the dedicated Autopilot task and then
/// acks; it never runs `dispatch_board_tasks` inline, because a real pty.send
/// can legitimately last for the BoardTask timeout window.
fn spawn_autopilot_board_event_sub(
    bus: Arc<BusServices>,
    state: AppState,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<BoardEvent>(
            &bus,
            "v2_autopilot_board_event",
            "autopilot_board_event",
        )
        .await
        else {
            return;
        };
        info!("v2[autopilot_board_event]: subscription live");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    let should_wake = board_event_should_wake_autopilot(ack.event());
                    crate::organism::autopilot_organ::process_autopilot_board_event(
                        &state,
                        ack.event(),
                        should_wake,
                    )
                    .await;
                    if should_wake && !crate::organism::autopilot_organ::autopilot_organ_active() {
                        state.board_dispatch_notify.notify_one();
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[autopilot_board_event]: shutdown");
    });
}

/// Autopilot nerve B — a slot becoming idle is the other natural trigger for
/// queued BoardTasks. This duplicates the legacy direct notify path
/// intentionally: event-bus causality is now the canonical path, while the
/// direct notify remains a harmless fast-path until every producer has moved
/// to pure bus emission.
fn spawn_autopilot_slot_event_sub(
    bus: Arc<BusServices>,
    state: AppState,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let Some(mut sub) =
            subscribe_or_warn::<SlotEvent>(&bus, "v2_autopilot_slot_event", "autopilot_slot_event")
                .await
        else {
            return;
        };
        info!("v2[autopilot_slot_event]: subscription live");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    let should_wake = slot_event_should_wake_autopilot(ack.event());
                    crate::organism::autopilot_organ::process_autopilot_slot_event(
                        &state,
                        ack.event(),
                        should_wake,
                    )
                    .await;
                    if should_wake && !crate::organism::autopilot_organ::autopilot_organ_active() {
                        state.board_dispatch_notify.notify_one();
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[autopilot_slot_event]: shutdown");
    });
}

fn board_event_should_wake_autopilot(event: &BoardEvent) -> bool {
    match event {
        BoardEvent::TaskCreated { .. } => true,
        BoardEvent::Updated { status, .. } => status.eq_ignore_ascii_case("open"),
        BoardEvent::StatusChanged { new_status, .. } => new_status.eq_ignore_ascii_case("open"),
        BoardEvent::NoteAdded { .. } | BoardEvent::Claimed { .. } | BoardEvent::Deleted { .. } => {
            false
        }
    }
}

fn slot_event_should_wake_autopilot(event: &SlotEvent) -> bool {
    matches!(event, SlotEvent::BecameIdle { .. })
}

/// Deployment EventBridge nerve — deploy-center remains the release authority,
/// but MissionD turns failure-class deployment events into visible deploy-ops
/// BoardTasks so a deployment lane can investigate from durable evidence.
fn spawn_deployment_event_response_sub(
    bus: Arc<BusServices>,
    state: AppState,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<SystemEvent>(
            &bus,
            "v2_deployment_event_response",
            "deployment_event_response",
        )
        .await
        else {
            return;
        };
        info!("v2[deployment_event_response]: subscription live");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    if let SystemEvent::ExternalServiceEvent {
                        service_id,
                        event_id,
                        event_kind,
                        summary,
                        trace_id,
                        payload_json,
                    } = ack.event()
                    {
                        let board_input = deployment_event_board_task_input(
                            service_id,
                            event_id,
                            event_kind,
                            summary,
                            trace_id.as_deref(),
                            payload_json,
                        )
                        .or_else(|| {
                            router_event_board_task_input(
                                service_id,
                                event_id,
                                event_kind,
                                summary,
                                trace_id.as_deref(),
                                payload_json,
                            )
                        });
                        if let Some(input) = board_input {
                            match state.store.create_board_task(&input).await {
                                Ok(task) => {
                                    let ev = BoardEvent::TaskCreated {
                                        task_id: task.id.to_string(),
                                        title: task.title.clone(),
                                        category: task.category.clone(),
                                    };
                                    crate::engine::master_control::notify_board_event_direct(&ev);
                                    let _ = state.bus.publish_board(ev).await;
                                }
                                Err(err) => {
                                    warn!(
                                        service_id = %service_id,
                                        event_id = %event_id,
                                        event_kind = %event_kind,
                                        error = %err,
                                        "v2[deployment_event_response]: failed to create deploy-ops BoardTask"
                                    );
                                }
                            }
                        }
                    }
                    ack.ack().await;
                }
            }
        }
        info!("v2[deployment_event_response]: shutdown");
    });
}

fn deployment_event_board_task_input(
    service_id: &str,
    event_id: &str,
    event_kind: &str,
    summary: &str,
    trace_id: Option<&str>,
    payload_json: &str,
) -> Option<CreateBoardTaskInput> {
    if service_id != "deploy-center" || !deployment_event_is_actionable(event_kind) {
        return None;
    }
    let payload: Value = serde_json::from_str(payload_json).unwrap_or(Value::Null);
    let project_id = external_event_field(&payload, "project_id")
        .or_else(|| external_event_field(&payload, "projectId"))
        .unwrap_or_else(|| "deploy-center".to_string());
    let subject = external_event_field(&payload, "subject").unwrap_or_else(|| project_id.clone());
    let correlation_id = external_event_field(&payload, "correlation_id")
        .or_else(|| external_event_field(&payload, "correlationId"))
        .or_else(|| trace_id.map(str::to_string));
    let deploy_event_id = payload
        .get("deploy_event_id")
        .and_then(Value::as_i64)
        .map(|id| id.to_string())
        .unwrap_or_else(|| event_id.to_string());
    let description = format!(
        "Deployment EventBridge created this task from a durable deploy-center event.\n\nService: {service_id}\nEvent: {event_kind}\nEvent ID: {event_id}\nDeploy event row: {deploy_event_id}\nProject: {project_id}\nSubject: {subject}\nCorrelation: {}\nTrace: {}\n\nSummary:\n{summary}\n\n## Dispatch metadata\n- task_class: deploy-ops\n- pool_hint: claude-code-deploy-ops\n- engine_hint: claude-code\n- read_scope: deploy-center provenance, deploy_events, deploy_logs, MissionD EventBridge envelope, mission_infra_query skill_evidence, project deployment SSOT/workflow evidence\n- write_scope: \n- must_not_touch: production DNS, Cloudflare, secrets, direct production mutation\n- acceptance: deploy-center provenance queried | deploy event row inspected | project deployment facts and skill evidence checked before action | xjp_build_wait/xjp_deploy_watch or deploy-center event wait used for CI/build waiting | rollback/redeploy proposal uses deploy-center policy or explicit Board approval\n- output_contract: return Findings / Evidence / Recommendations / Verification with structured smoke/provenance evidence\n\nNext checks:\n1. Query MissionD project deployment facts and mission_infra_query(action=skill_evidence|reconcile) for the project before choosing scripts, hosts, agents, or login paths.\n2. Query deploy-center provenance for the project/service before using curl/git as fallback.\n3. Inspect deploy_events/deploy_logs around the event id and correlation id.\n4. If CI/build waiting is needed, use xjp_build_wait/xjp_deploy_watch or deploy-center event waits; do not run repeated gh api Actions polling loops.\n5. Propose rollback or redeploy only through deploy-center policy or explicit Board approval.\n6. Do not mutate DNS, Cloudflare, secrets, or production state from this task without approval.",
        correlation_id.as_deref().unwrap_or(""),
        trace_id.unwrap_or(""),
    );
    let runtime_metadata = serde_json::json!({
        "schema": "missiond.board-task-runtime-metadata.v1",
        "source": "eventbridge",
        "control_state": "task_contracts",
        "dispatch_metadata": {
            "task_class": "deploy-ops",
            "pool_hint": "claude-code-deploy-ops",
            "engine_hint": "claude-code",
            "eventbridge_service_id": service_id,
            "event_id": event_id,
            "event_kind": event_kind,
            "deploy_event_id": deploy_event_id,
            "project_id": project_id,
            "subject": subject,
            "correlation_id": correlation_id,
            "trace_id": trace_id,
            "output_contract": "return Findings / Evidence / Recommendations / Verification with structured smoke/provenance evidence",
            "acceptance": "deploy-center provenance queried | deploy event row inspected | project deployment facts and skill evidence checked before action | xjp_build_wait/xjp_deploy_watch or deploy-center event wait used for CI/build waiting | rollback/redeploy proposal uses deploy-center policy or explicit Board approval"
        },
        "read_scope": [
            "deploy-center provenance",
            "deploy_events",
            "deploy_logs",
            "MissionD EventBridge envelope",
            "mission_infra_query skill_evidence",
            "project deployment SSOT/workflow evidence"
        ],
        "write_scope": [],
        "must_not_touch": [
            "production DNS",
            "Cloudflare",
            "secrets",
            "direct production mutation"
        ],
        "capability_grant_ids": [],
        "sandbox_profile": "read-only",
        "projection_policy": "description_notes_are_projection_only"
    });
    Some(CreateBoardTaskInput {
        title: format!("Deploy event response: {event_kind} ({project_id})"),
        description: Some(description),
        priority: Some("high".to_string()),
        category: Some("ops".to_string()),
        project: Some(project_id),
        auto_execute: Some(false),
        hidden: Some(false),
        dedupe_key: Some(format!("deployment-event-response:{service_id}:{event_id}")),
        context_intent: Some("deploy-ops".to_string()),
        runtime_metadata: Some(runtime_metadata),
        ..Default::default()
    })
}

fn deployment_event_is_actionable(event_kind: &str) -> bool {
    matches!(
        event_kind,
        "build_failed"
            | "deploy_failed"
            | "smoke_failed"
            | "rollback_failed"
            | "agent_update_failed"
    )
}

fn router_event_board_task_input(
    service_id: &str,
    event_id: &str,
    event_kind: &str,
    summary: &str,
    trace_id: Option<&str>,
    payload_json: &str,
) -> Option<CreateBoardTaskInput> {
    if service_id != "router" || !router_event_is_actionable(event_kind) {
        return None;
    }
    let payload: Value = serde_json::from_str(payload_json).unwrap_or(Value::Null);
    let project_id = external_event_field(&payload, "project_id")
        .or_else(|| external_event_field(&payload, "projectId"))
        .unwrap_or_else(|| "router".to_string());
    let provider = external_event_field(&payload, "provider")
        .or_else(|| {
            payload
                .get("provider")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| {
            payload
                .get("payload")
                .and_then(|p| p.get("provider"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown-provider".to_string());
    let model = payload
        .get("payload")
        .and_then(|p| p.get("model"))
        .and_then(Value::as_str)
        .or_else(|| payload.get("model").and_then(Value::as_str))
        .unwrap_or("unknown-model");
    let description = format!(
        "Router EventBridge created this task from a durable usage/anomaly event.\n\nService: {service_id}\nEvent: {event_kind}\nEvent ID: {event_id}\nProject: {project_id}\nProvider: {provider}\nModel: {model}\nTrace: {}\n\nSummary:\n{summary}\n\n## Dispatch metadata\n- task_class: router-ops\n- pool_hint: claude-code-default\n- engine_hint: claude-code\n- read_scope: router usage_logs, router event envelope, provider/channel policy, MissionD EventBridge timeline\n- write_scope: \n- must_not_touch: production DNS, secrets, provider credentials, deploy mutation\n- acceptance: router usage burst inspected | provider/model attribution verified | remediation or quota/auth follow-up proposed | no hidden translation/background LLM retry loop\n- output_contract: return Findings / Evidence / Recommendations / Verification with event_id and provider/model attribution\n\nNext checks:\n1. Query router usage logs around the event time and provider/model.\n2. Determine whether this is expected user workload, runaway background worker, auth/quota failure, or provider outage.\n3. If credentials or provider quota are implicated, create a Decision Inbox item or deploy-center/secret-store follow-up; do not mutate secrets directly.",
        trace_id.unwrap_or(""),
    );
    let runtime_metadata = serde_json::json!({
        "schema": "missiond.board-task-runtime-metadata.v1",
        "source": "eventbridge",
        "control_state": "task_contracts",
        "dispatch_metadata": {
            "task_class": "router-ops",
            "pool_hint": "claude-code-default",
            "engine_hint": "claude-code",
            "eventbridge_service_id": service_id,
            "event_id": event_id,
            "event_kind": event_kind,
            "project_id": project_id,
            "provider": provider,
            "model": model,
            "trace_id": trace_id,
            "output_contract": "return Findings / Evidence / Recommendations / Verification with event_id and provider/model attribution",
            "acceptance": "router usage burst inspected | provider/model attribution verified | remediation or quota/auth follow-up proposed | no hidden translation/background LLM retry loop"
        },
        "read_scope": [
            "router usage_logs",
            "router event envelope",
            "provider/channel policy",
            "MissionD EventBridge timeline"
        ],
        "write_scope": [],
        "must_not_touch": [
            "production DNS",
            "secrets",
            "provider credentials",
            "deploy mutation"
        ],
        "capability_grant_ids": [],
        "sandbox_profile": "read-only",
        "projection_policy": "description_notes_are_projection_only"
    });
    Some(CreateBoardTaskInput {
        title: format!("Router event response: {event_kind} ({provider}/{model})"),
        description: Some(description),
        priority: Some("high".to_string()),
        category: Some("ops".to_string()),
        project: Some(project_id),
        auto_execute: Some(false),
        hidden: Some(false),
        dedupe_key: Some(format!("router-event-response:{service_id}:{event_id}")),
        context_intent: Some("router-ops".to_string()),
        runtime_metadata: Some(runtime_metadata),
        ..Default::default()
    })
}

fn router_event_is_actionable(event_kind: &str) -> bool {
    matches!(
        event_kind,
        "usage_burst" | "provider_error_burst" | "provider_auth_failure_burst" | "quota_exhaustion"
    )
}

fn external_event_field(payload: &Value, key: &str) -> Option<String> {
    payload
        .get("_envelope")
        .and_then(|v| v.get(key))
        .or_else(|| payload.get(key))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Router A3 — decision: QuestionEvent::Created → process pending.
fn spawn_decision_sub(bus: Arc<BusServices>, state: AppState, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let Some(mut sub) =
            subscribe_or_warn::<QuestionEvent>(&bus, "v2_router_decision", "router_decision").await
        else {
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
        let Some(mut sub) =
            subscribe_or_warn::<WorkerEvent>(&bus, "v2_router_harvest", "router_harvest").await
        else {
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
fn spawn_realtime_extraction_sub(
    bus: Arc<BusServices>,
    state: AppState,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let Some(sub) = subscribe_or_warn::<MessageEvent>(
            &bus,
            "v2_router_realtime_extraction",
            "router_realtime_extraction",
        )
        .await
        else {
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
fn spawn_session_reflection_sub(
    bus: Arc<BusServices>,
    state: AppState,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let Some(sub) = subscribe_or_warn::<SessionEvent>(
            &bus,
            "v2_router_session_reflection",
            "router_session_reflection",
        )
        .await
        else {
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
fn spawn_kb_consolidation_sub(
    bus: Arc<BusServices>,
    state: AppState,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<MemoryEvent>(
            &bus,
            "v2_router_kb_consolidation",
            "router_kb_consolidation",
        )
        .await
        else {
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
fn spawn_intent_analyst_sub(
    bus: Arc<BusServices>,
    state: AppState,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let Some(mut sub) = subscribe_or_warn::<MemoryEvent>(
            &bus,
            "v2_router_intent_analyst",
            "router_intent_analyst",
        )
        .await
        else {
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
                .filter(|(_, (ts, count))| {
                    now.duration_since(*ts) >= DEBOUNCE || *count >= MAX_ACCUM
                })
                .map(|(id, _)| id.clone())
                .collect();
            for session_id in expired {
                pending.remove(&session_id);
                if state
                    .control_manager
                    .current()
                    .is_provider_paused(crate::control_tree::CtlProvider::Sonnet)
                {
                    continue;
                }
                match crate::engine::learning_engine::intent_analyst::process_session_intents(
                    &state,
                    &session_id,
                )
                .await
                {
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
        let Some(mut sub) =
            subscribe_or_warn::<QuestionEvent>(&bus, "v2_review_resolution", "review_resolution")
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
                let outcome = directive_handle_review_resolved(state, &parsed, decision).await;
                log_directive_outcome(question_id, &outcome);
            }
            "plan" => {
                // wave-17 / task 01 — branch on action so deterministic
                // plan-node review ids (`action="plan-node"`) route
                // through the DAG resume helper while the wave-15
                // manager-action ids (compile / approve / mark /
                // supersede) keep the existing handler. Non-plan-node
                // ids that happen to encode `plan-node` for an
                // unsupported scope already failed earlier in the
                // planner; we only need to split here.
                if is_plan_node_review_action(&parsed.action) {
                    let outcome = plan_node_handle_review_resolved(state, &parsed, decision).await;
                    log_plan_node_resume_outcome(question_id, &outcome);
                } else {
                    let outcome = plan_handle_review_resolved(state, &parsed, decision).await;
                    log_plan_outcome(question_id, &outcome);
                }
            }
            "workflow" => {
                let outcome = workflow_handle_review_resolved(state, &parsed, decision).await;
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

fn log_plan_node_resume_outcome(qid: &str, outcome: &PlanNodeResumeListenerOutcome) {
    match outcome {
        PlanNodeResumeListenerOutcome::Dispatched {
            plan_id,
            node_id,
            succeeded,
        } => {
            if *succeeded {
                info!(
                    question_id = %qid,
                    plan_id = %plan_id,
                    node_id = %node_id,
                    "v2[review_resolution]: plan-node resume dispatched (approved)"
                );
            } else {
                warn!(
                    question_id = %qid,
                    plan_id = %plan_id,
                    node_id = %node_id,
                    "v2[review_resolution]: plan-node resume dispatched but inner handler failed"
                );
            }
        }
        PlanNodeResumeListenerOutcome::KeptPaused {
            plan_id,
            node_id,
            decision,
        } => {
            info!(
                question_id = %qid,
                plan_id = %plan_id,
                node_id = %node_id,
                decision = %decision,
                "v2[review_resolution]: plan-node kept paused (rejected/needs_changes)"
            );
        }
        PlanNodeResumeListenerOutcome::ArtifactIdNotUuid { artifact_id, error } => {
            warn!(
                question_id = %qid,
                artifact_id = %artifact_id,
                error = %error,
                "v2[review_resolution]: plan-node artifact_id not a UUID"
            );
        }
        PlanNodeResumeListenerOutcome::NotFound { artifact_id } => {
            warn!(
                question_id = %qid,
                artifact_id = %artifact_id,
                "v2[review_resolution]: plan-node plan not found"
            );
        }
        PlanNodeResumeListenerOutcome::ValidationRejected {
            plan_id,
            code,
            message,
        } => {
            warn!(
                question_id = %qid,
                plan_id = %plan_id,
                code = %code,
                error = %message,
                "v2[review_resolution]: plan-node resume validation rejected"
            );
        }
        PlanNodeResumeListenerOutcome::DagBuildFailed { plan_id, detail } => {
            warn!(
                question_id = %qid,
                plan_id = %plan_id,
                error = %detail,
                "v2[review_resolution]: plan-node DAG build failed"
            );
        }
        PlanNodeResumeListenerOutcome::DispatchError { plan_id, detail } => {
            warn!(
                question_id = %qid,
                plan_id = %plan_id,
                error = %detail,
                "v2[review_resolution]: plan-node resume dispatch error"
            );
        }
    }
}

fn log_workflow_outcome(qid: &str, outcome: &WorkflowSubscriberOutcome) {
    match outcome {
        WorkflowSubscriberOutcome::PersistedReceipt {
            workflow_id,
            decision,
        } => {
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
        let Some(mut sub) =
            subscribe_or_warn::<ExecutionEvent>(&bus, "v2_event_ref_cache", "event_ref_cache")
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
    fn board_events_wake_autopilot_only_for_new_or_reopened_work() {
        assert!(board_event_should_wake_autopilot(
            &BoardEvent::TaskCreated {
                task_id: "t".to_string(),
                title: "do it".to_string(),
                category: "dev".to_string(),
            }
        ));
        assert!(board_event_should_wake_autopilot(
            &BoardEvent::StatusChanged {
                task_id: "t".to_string(),
                old_status: "blocked".to_string(),
                new_status: "Open".to_string(),
            }
        ));
        assert!(board_event_should_wake_autopilot(&BoardEvent::Updated {
            task_id: "t".to_string(),
            status: "open".to_string(),
            category: "dev".to_string(),
        }));
        assert!(!board_event_should_wake_autopilot(&BoardEvent::NoteAdded {
            task_id: "t".to_string(),
            note_id: "n".to_string(),
            content_preview: "done".to_string(),
        }));
    }

    #[test]
    fn slot_became_idle_wakes_autopilot() {
        assert!(slot_event_should_wake_autopilot(&SlotEvent::BecameIdle {
            slot_id: "slot-a".to_string(),
        }));
        assert!(!slot_event_should_wake_autopilot(
            &SlotEvent::TaskDispatched {
                slot_id: "slot-a".to_string(),
                task_id: Some("t".to_string()),
                purpose: "board_auto_execute".to_string(),
                prompt_chars: 10,
                preview: "x".to_string(),
                cited_kb_ids: Vec::new(),
            }
        ));
    }

    #[test]
    fn deployment_event_response_task_declares_deploy_ops_lane() {
        let payload = serde_json::json!({
            "project_id": "auth",
            "subject": "xjp-auth-center",
            "correlation_id": "deploy-123",
            "deploy_event_id": 42
        })
        .to_string();
        let input = deployment_event_board_task_input(
            "deploy-center",
            "evt-1",
            "deploy_failed",
            "deploy failed during smoke",
            Some("trace-1"),
            &payload,
        )
        .expect("actionable deploy event should create task input");
        assert_eq!(input.context_intent.as_deref(), Some("deploy-ops"));
        assert_eq!(input.category.as_deref(), Some("ops"));
        let description = input.description.as_deref().expect("description");
        for expected in [
            "## Dispatch metadata",
            "- task_class: deploy-ops",
            "- pool_hint: claude-code-deploy-ops",
            "- engine_hint: claude-code",
            "deploy-center provenance queried",
        ] {
            assert!(
                description.contains(expected),
                "missing {expected}: {description}"
            );
        }
        let runtime_metadata = input.runtime_metadata.expect("runtime metadata");
        assert_eq!(
            runtime_metadata
                .get("control_state")
                .and_then(Value::as_str),
            Some("task_contracts")
        );
        assert_eq!(
            runtime_metadata
                .get("dispatch_metadata")
                .and_then(|v| v.get("task_class"))
                .and_then(Value::as_str),
            Some("deploy-ops")
        );
        assert_eq!(
            runtime_metadata
                .get("dispatch_metadata")
                .and_then(|v| v.get("pool_hint"))
                .and_then(Value::as_str),
            Some("claude-code-deploy-ops")
        );
        assert_eq!(
            runtime_metadata
                .get("sandbox_profile")
                .and_then(Value::as_str),
            Some("read-only")
        );
        assert!(
            runtime_metadata
                .get("read_scope")
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty()),
            "EventBridge task contracts must carry typed read_scope"
        );
    }

    #[test]
    fn router_event_response_task_declares_runtime_contract() {
        let payload = serde_json::json!({
            "project_id": "missiond",
            "payload": {
                "provider": "anthropic",
                "model": "claude-sonnet"
            }
        })
        .to_string();
        let input = router_event_board_task_input(
            "router",
            "evt-router-1",
            "usage_burst",
            "usage anomaly",
            Some("trace-router"),
            &payload,
        )
        .expect("actionable router event should create task input");
        assert_eq!(input.context_intent.as_deref(), Some("router-ops"));
        let runtime_metadata = input.runtime_metadata.expect("runtime metadata");
        assert_eq!(
            runtime_metadata
                .get("dispatch_metadata")
                .and_then(|v| v.get("task_class"))
                .and_then(Value::as_str),
            Some("router-ops")
        );
        assert_eq!(
            runtime_metadata
                .get("dispatch_metadata")
                .and_then(|v| v.get("pool_hint"))
                .and_then(Value::as_str),
            Some("claude-code-default")
        );
        assert_eq!(
            runtime_metadata
                .get("sandbox_profile")
                .and_then(Value::as_str),
            Some("read-only")
        );
    }

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
        let d =
            plan_review_resolved_dispatch("review:directive:abc:v1:approve", "looks-good-to-me");
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

    #[test]
    fn dispatch_routes_plan_node_action_separately_from_manager_actions() {
        // Wave-17 / task 01 — the planner routes both
        // `review:plan:*:plan-node:<hash>` (DAG paused-node ids) and
        // `review:plan:*:approve` (manager-action ids) under
        // scope=plan. The subscriber's `handle_review_resolved` MUST
        // branch on `parsed.action` so plan-node ids hit the resume
        // helper while manager-action ids stay on the wave-15 bridge.
        // This test pins the predicate the subscriber relies on.
        use crate::handlers::knowledge::review_gate::is_plan_node_review_action;

        // Manager-action id — NOT routed through the resume helper.
        let d = plan_review_resolved_dispatch("review:plan:abc:v1:approve", "approved");
        match d {
            ReviewResolvedDispatch::Route { parsed, .. } => {
                assert_eq!(parsed.scope, "plan");
                assert!(!is_plan_node_review_action(&parsed.action));
            }
            other => panic!("expected Route, got {:?}", other),
        }

        // Plan-node id — MUST route through the resume helper.
        let d = plan_review_resolved_dispatch(
            "review:plan:abc:v1:plan-node:0123456789abcdef",
            "approved",
        );
        match d {
            ReviewResolvedDispatch::Route { parsed, decision } => {
                assert_eq!(parsed.scope, "plan");
                assert!(is_plan_node_review_action(&parsed.action));
                assert_eq!(decision, ReviewDecision::Approved);
                assert_eq!(parsed.topic_hash.as_deref(), Some("0123456789abcdef"));
            }
            other => panic!("expected Route for plan-node id, got {:?}", other),
        }
    }
}
