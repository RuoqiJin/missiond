use missiond_core::event::events::{ExecutionEvent, QuestionEvent};
use missiond_core::types::Plan;
use serde_json::{json, Value};

use crate::state::AppState;

use super::super::evidence_collector::{self, AppendOutcome, EventRef, EvidenceEntry};
use super::acceptance::{derive_acceptance_pause_id, AcceptanceEvaluation, AcceptanceStatus};
use super::parser::DagNode;
use super::rollback::{RollbackEvaluation, RollbackStatus};
use super::ExecutionOutcome;

/// wave-17 / task 05 — append one `dag_finalized` evidence row. Mirrors the
/// per-node evidence layout (same source + kind taxonomy) so audit
/// dashboards that pivot on `state_transition` see the finalize entry next
/// to the per-node entries it summarises. Updates `evidence_path` /
/// `evidence_error` on the response payload so callers see the same
/// freshness signal the per-node writes already provide.
pub(super) async fn emit_evidence_dag_finalized(
    state: &AppState,
    plan: &Plan,
    args: &Value,
    aggregate_status: &str,
    plan_status_after: Option<&str>,
    plan_status_update_error: Option<&str>,
    distill_block: Option<&Value>,
    payload: &mut Value,
) {
    let project_arg = args.get("project").and_then(|v| v.as_str());
    let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg = args.get("target_project").and_then(|v| v.as_str());

    let final_plan_status = plan_status_after.unwrap_or("unchanged");
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::NOTE,
    )
    .with_state_transition("dag_finalized")
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("event_kind", json!("plan_dag_finalized"))
    .with_extra("plan_id", json!(plan.id))
    .with_extra("plan_version", json!(plan.version))
    .with_extra("aggregate_status", json!(aggregate_status))
    .with_extra("final_plan_status", json!(final_plan_status));
    if let Some(err) = plan_status_update_error {
        entry = entry.with_extra("plan_status_update_error", json!(err));
    }
    if let Some(d) = distill_block {
        // Distill block on evidence is the same shape the response carries
        // (triggered + reason + mode + optional result/warning) so audit
        // consumers can correlate without a second JSON parse.
        entry = entry.with_extra("distill", d.clone());
    } else {
        entry = entry.with_extra(
            "distill",
            json!({"triggered": false, "reason": "not_requested"}),
        );
    }
    let append_outcome = evidence_collector::append(
        state,
        plan.id,
        project_arg,
        cwd_arg,
        target_project_arg,
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %plan.id,
            error = %error,
            "DAG finalize: evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        payload["evidence_path"] = json!(p);
    }
    if let Some(e) = err {
        payload["evidence_error"] = json!(e);
    }
}

/// Pre-built immutable evidence parameters that vary per call to
/// `action_execute_dag_v1`. The scheduler captures these once so each
/// per-node evidence emit doesn't re-thread the same args through.
pub(super) struct EvidenceCtx<'a> {
    pub(super) plan_id: uuid::Uuid,
    /// wave-17 / task 03 — captured here so the deterministic
    /// acceptance pause id (which carries the plan version segment for
    /// resolver routing) can be derived without re-fetching the plan
    /// row from every emit site.
    pub(super) plan_version: i32,
    pub(super) project_arg: Option<&'a str>,
    pub(super) cwd_arg: Option<&'a str>,
    pub(super) target_project_arg: Option<&'a str>,
}

/// `EventRef::unavailable` reason kept for the legacy fallback path —
/// publish *and* deterministic-id construction must both fail before we
/// surrender to it. Wave-14 :: Task 02 wires `PlanNodeStateChanged` so the
/// normal path now writes `EventRef::new(...)` either with the live `Seq`
/// from the bus or with the deterministic
/// `plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>` id when the bus
/// publish fails. The `unavailable` placeholder is unreachable today (the
/// deterministic id is always derivable) but kept on the call surface so
/// the contract `evidence_collector` documents (`unavailable` → "we tried
/// to correlate but couldn't") stays implementable if a future caller
/// genuinely cannot stamp an id.
#[allow(dead_code)]
const EVENT_REF_UNAVAILABLE_REASON: &str =
    "plan_dag scheduler could not derive a live or deterministic \
     ExecutionEvent reference; this is a fallback path";

/// Domain tag used in `EventRef::source` for plan-node lifecycle entries.
/// Mirrors `Domain::Execution::as_str()` (kept as a `&'static str` here so
/// we don't pull the enum reference into every evidence call site).
pub(super) const EVENT_REF_SOURCE_EXECUTION: &str = "execution";

/// Kind tag matching `ExecutionEvent::PlanNodeStateChanged.kind()`. Kept
/// duplicated here so test assertions can pin the wire form without taking
/// a dep on the event-trait reflection.
pub(super) const EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED: &str = "plan_node_state_changed";

/// Build the deterministic event id used as a stable correlation key when
/// the live bus publish either succeeds (used in the publish dedupe context)
/// or fails (used as the `EventRef::event_id`). Format
/// `plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>` matches the wave-14
/// task brief verbatim so external consumers can grep on it.
pub(super) fn deterministic_plan_node_event_id(
    plan_id: uuid::Uuid,
    node_id: &str,
    attempt: u32,
    from: &str,
    to: &str,
) -> String {
    format!(
        "plan-node:{}:{}:{}:{}-{}",
        plan_id, node_id, attempt, from, to
    )
}

/// Build the `PlanNodeStateChanged` payload for a single transition.
/// Pure helper so unit tests can pin the wire shape without standing up an
/// `AppState`.
pub(super) fn build_plan_node_state_changed_event(
    plan_id: uuid::Uuid,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    from: &str,
    to: &str,
    reason: Option<String>,
) -> ExecutionEvent {
    ExecutionEvent::PlanNodeStateChanged {
        plan_id: plan_id.to_string(),
        node_id: node.id.clone(),
        from: from.to_string(),
        to: to.to_string(),
        target: Some(node.target.clone()),
        dispatch_strategy: Some(dispatch_strategy.to_string()),
        target_project: node.target_project.clone(),
        attempt: Some(attempt),
        reason,
    }
}

/// Publish a `PlanNodeStateChanged` event and return the `EventRef` to
/// embed in the evidence entry. On bus success we surface the live `Seq` as
/// the event id; on failure we fall back to the deterministic id derived
/// from `plan_id`/`node_id`/`attempt`/`from`/`to` so the audit trail still
/// carries a stable correlation key, and we record a warning string the
/// caller can lift into `outcome.bus_publish_warnings`.
///
/// The function NEVER aborts the dispatch on a publish failure — the
/// scheduler's main loop only consults the returned warning to decide
/// whether to surface `bus_publish_warnings` on the response. This matches
/// the wave-14 / task 02 brief: bus publish failure is observability-only.
pub(super) async fn publish_plan_node_state_change(
    state: &AppState,
    plan_id: uuid::Uuid,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    from: &str,
    to: &str,
    reason: Option<String>,
) -> (EventRef, Option<String>) {
    let ev = build_plan_node_state_changed_event(
        plan_id,
        node,
        dispatch_strategy,
        attempt,
        from,
        to,
        reason,
    );
    let deterministic_id = deterministic_plan_node_event_id(plan_id, &node.id, attempt, from, to);
    match state.bus.publish_execution_with_seq(ev).await {
        Ok(seq) => (
            EventRef::new(
                EVENT_REF_SOURCE_EXECUTION,
                EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
                seq.0.to_string(),
            ),
            None,
        ),
        Err(err) => {
            // Wave-17 / task 06 — try the resolver before surrendering to
            // the deterministic-id fallback. The resolver checks its
            // in-memory cache first (a previous attempt for the same
            // transition may already have cached a real `Seq`), then
            // falls through to a bounded read-only scan of the persistent
            // event log so refs survive daemon restarts. Lookup failure
            // NEVER aborts the dispatch — on every error path we keep the
            // deterministic id so the audit trail still carries a stable
            // correlation key.
            let plan_id_str = plan_id.to_string();
            let recovered = state
                .bus
                .event_ref_resolver
                .lookup_or_query_plan_node_state_change(
                    state.bus.log.as_ref(),
                    &plan_id_str,
                    &node.id,
                    attempt,
                    from,
                    to,
                )
                .await;
            if recovered.status == evidence_collector::EventRefStatus::Log {
                let warning = format!(
                    "plan_node_state_changed bus publish failed for {} ({} -> {}): {}; \
                     evidence ref recovered from event log",
                    node.id, from, to, err
                );
                tracing::warn!(
                    plan_id = %plan_id,
                    node_id = %node.id,
                    from = %from,
                    to = %to,
                    error = %err,
                    "DAG scheduler: PlanNodeStateChanged bus publish failed; recovered event ref from log"
                );
                return (recovered, Some(warning));
            }
            let warning = format!(
                "plan_node_state_changed bus publish failed for {} ({} -> {}): {}; \
                 evidence ref falls back to deterministic id `{}`",
                node.id, from, to, err, deterministic_id
            );
            tracing::warn!(
                plan_id = %plan_id,
                node_id = %node.id,
                from = %from,
                to = %to,
                error = %err,
                "DAG scheduler: PlanNodeStateChanged bus publish failed; deterministic event ref retained"
            );
            (
                EventRef::new(
                    EVENT_REF_SOURCE_EXECUTION,
                    EVENT_REF_KIND_PLAN_NODE_STATE_CHANGED,
                    deterministic_id,
                ),
                Some(warning),
            )
        }
    }
}

/// Per-node attempt counter. v2 has no retry policy so every transition
/// reports `attempt=1`; encapsulating the constant in a helper keeps the
/// retry-aware future scheduler a single-call-site change.
pub(super) const PLAN_NODE_DEFAULT_ATTEMPT: u32 = 1;

/// wave-16 / task 05 — pure retry decision. Extracted out of the wave
/// loop so the predicate can be unit-tested without standing up an
/// `AppState`. `should_retry` is true iff the failed attempt is not
/// classified non-retryable AND attempts remain AND the wave is not
/// already in fail-fast abort. The wave loop honours this decision
/// deterministically so the tests below pin the contract.
pub(super) fn plan_node_should_retry(
    current_attempt: u32,
    max_attempts: u32,
    non_retryable: bool,
    abort_new_dispatch: bool,
) -> bool {
    if non_retryable || abort_new_dispatch {
        return false;
    }
    let attempts_remaining = max_attempts.saturating_sub(current_attempt);
    attempts_remaining > 0
}

/// Emit `ready -> running` evidence at the moment the scheduler hands a node
/// to its dispatch task. Kept structurally identical to the success/failure
/// branches so audit dashboards can pivot on `state_transition` alone.
///
/// Wave-14 / Task 02: also publishes a `PlanNodeStateChanged` event on the
/// execution bus and stamps the resulting live `Seq` (or the deterministic
/// fallback id when publish fails) onto the evidence entry's
/// `execution_events` array. Bus publish failures land in
/// `outcome.bus_publish_warnings` so the response surfaces the degraded
/// observability path without aborting the dispatch.
pub(super) async fn emit_evidence_running(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    outcome: &mut ExecutionOutcome,
) {
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "ready",
        "running",
        None,
    )
    .await;
    if let Some(w) = &warning {
        outcome.bus_publish_warnings.push(w.clone());
    }
    let entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("ready -> running")
    .with_primary_event_ref(&event_ref, warning)
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt));
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: ready->running evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
}

/// Emit `running -> succeeded` (success branch) or `running -> failed`
/// (failure branch) evidence after the dispatch task returns. The two
/// branches keep the byte shape of v1's `ready -> {succeeded|failed}` legacy
/// passthrough fields so existing audit consumers do not need updates.
///
/// Wave-14 / Task 02: also publishes a `PlanNodeStateChanged` event on the
/// execution bus and stamps the resulting live `Seq` (or the deterministic
/// fallback id when publish fails) onto the evidence entry's
/// `execution_events` array. The `reason` annotation on the failure branch
/// surfaces the inner-handler error message so bus consumers can route
/// without re-fetching the sidecar payload.
pub(super) async fn emit_evidence_finished(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    inner_payload: &Value,
    succeeded: bool,
    attempt: u32,
    outcome: &mut ExecutionOutcome,
) {
    let to_state = if succeeded { "succeeded" } else { "failed" };
    let reason = if succeeded {
        None
    } else {
        // Best-effort: surface the inner-handler's `error` field so bus
        // consumers see the same string the response carries. Fallback to
        // the canonical "inner handler returned error" when no `error`
        // string is present (mirrors `dispatch_node` classification).
        let s = inner_payload
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("inner handler returned error")
            .to_string();
        Some(s)
    };
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "running",
        to_state,
        reason,
    )
    .await;
    if let Some(w) = &warning {
        outcome.bus_publish_warnings.push(w.clone());
    }
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_primary_event_ref(&event_ref, warning)
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt));
    if succeeded {
        // Success branch — populate `inner_dispatch` (canonical typed slot)
        // AND `inner_result` (legacy alias) so wave-12 typed readers and
        // pre-wave12 dashboard greps both keep working byte-for-byte.
        entry = entry
            .with_inner_dispatch(inner_payload.clone())
            .with_state_transition("running -> succeeded")
            .with_extra("inner_result", inner_payload.clone());
    } else {
        // Failure branch — keep the legacy `inner_error` extra slot for
        // readers that historically filtered on it; intentionally do NOT
        // call `with_inner_dispatch` so success vs failure stay shape-distinct.
        entry = entry
            .with_state_transition("running -> failed")
            .with_extra("inner_error", inner_payload.clone());
    }
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: running->{} evidence append failed",
            to_state
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
}

/// wave-17 / task 04 — emit one rollback-phase evidence entry per
/// failed node that opted into a rollback policy. Runs ONLY after
/// `emit_evidence_finished` for the failure branch and BEFORE
/// `propagate_taint`, so audit dashboards can pivot on the
/// `failed -> rollback_*` transition between the failure row and any
/// downstream `pending -> skipped` rows.
///
/// The entry's `state_transition` reflects the rollback decision
/// (`failed -> rollback_descriptor_ready`,
/// `failed -> rollback_dispatched`, `failed -> rollback_refused`,
/// `failed -> rollback_failed`) so audit dashboards can pivot on a
/// single string. Entries surface every field on
/// [`RollbackEvaluation::to_json`] PLUS the typed top-level
/// `rollback_status` / `rollback_policy` slots so legacy dashboards
/// can grep without descending into the `rollback` block.
///
/// Bus publish failure on the lifecycle event is observability-only —
/// the warning lands on `outcome.bus_publish_warnings` and the
/// evidence ref falls back to the deterministic id; the rollback
/// decision itself is unaffected.
pub(super) async fn emit_evidence_rollback(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    evaluation: &RollbackEvaluation,
    outcome: &mut ExecutionOutcome,
) {
    let to_state = match evaluation.status {
        RollbackStatus::NotRequested => "rollback_skipped",
        RollbackStatus::DescriptorReady => "rollback_descriptor_ready",
        RollbackStatus::Dispatched => "rollback_dispatched",
        RollbackStatus::Refused => "rollback_refused",
        RollbackStatus::Failed => "rollback_failed",
    };
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "failed",
        to_state,
        Some(format!(
            "rollback:{}:policy={}:reason={}",
            evaluation.status.as_wire(),
            evaluation.policy.as_wire(),
            evaluation.reason
        )),
    )
    .await;
    if let Some(w) = warning {
        outcome.bus_publish_warnings.push(w);
    }
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition(format!("failed -> {}", to_state))
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt))
    .with_extra("rollback_policy", json!(evaluation.policy.as_wire()))
    .with_extra("rollback_status", json!(evaluation.status.as_wire()))
    .with_extra("rollback_reason", json!(evaluation.reason))
    .with_extra("rollback_owned_files", json!(evaluation.owned_files))
    .with_extra(
        "rollback_acceptance_commands",
        json!(evaluation.acceptance_commands),
    )
    .with_extra("rollback_acceptance_commands_executed", json!(false));
    if let Some(obj) = evaluation.objective.as_deref() {
        entry = entry.with_extra("rollback_objective", json!(obj));
    }
    if let Some(preview) = evaluation.task_brief_preview.as_deref() {
        entry = entry.with_extra("rollback_task_brief_preview", json!(preview));
    }
    if let Some(p) = evaluation.task_brief_path.as_deref() {
        entry = entry.with_extra("rollback_task_brief_path", json!(p));
    }
    if let Some(inner) = evaluation.inner_payload.clone() {
        entry = entry.with_extra("rollback_inner_result", inner);
    }
    // wave-18 / task 04 — cascade rollback evidence extras. Surfaced
    // alongside the node-local rollback fields so audit dashboards can
    // grep `rollback_cascade_*` without descending into the embedded
    // `cascade` JSON. Quiet (omitted) when the cascade evaluator never
    // produced a signal so the wave-17 / task 04 byte shape stays
    // untouched for plans that did not opt into cascading.
    if let Some(cascade) = evaluation.cascade.as_ref() {
        if !cascade.is_inactive() {
            let comp_ids: Vec<&str> = cascade
                .compensations
                .iter()
                .map(|c| c.node_id.as_str())
                .collect();
            entry = entry
                .with_extra("rollback_cascade_mode", json!(cascade.mode.as_wire()))
                .with_extra("rollback_cascade_root", json!(cascade.cascade_root))
                .with_extra("rollback_cascade_compensation_node_ids", json!(comp_ids))
                .with_extra(
                    "rollback_cascade_compensation_count",
                    json!(cascade.compensations.len()),
                )
                .with_extra("rollback_cascade_reason", json!(cascade.reason))
                .with_extra("rollback_cascade", cascade.to_json());
        }
    }
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: failed->rollback_* evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
}

/// wave-17 / task 03 — emit one acceptance-phase evidence entry per
/// successfully-dispatched node that opted into the acceptance contract.
/// Runs ONLY after `emit_evidence_finished` for the success branch; the
/// scheduler skips the call entirely for nodes that did not declare
/// acceptance hints so the wave-13 byte shape is preserved.
///
/// The entry's `state_transition` reflects the acceptance decision
/// (`succeeded -> acceptance_accepted`, `succeeded -> acceptance_rejected`,
/// `succeeded -> acceptance_manual_required`) so audit dashboards can
/// pivot on a single string. The entry surfaces:
///   * `acceptance_status` — wire form of [`AcceptanceStatus`].
///   * `acceptance_mode` — wire form of [`AcceptanceMode`] when set.
///   * `acceptance_commands` — declared commands surfaced verbatim,
///     **NEVER executed**. They are recorded so observers / out-of-band
///     pipelines can see what the author wanted to verify.
///   * `acceptance_evidence_keys` — declared required keys.
///   * `acceptance_reason` — human-readable explanation.
///
/// Bus publish failure on the lifecycle event is observability-only —
/// the warning lands on `outcome.bus_publish_warnings` and the
/// evidence ref falls back to the deterministic id; the acceptance
/// decision itself is unaffected.
pub(super) async fn emit_evidence_acceptance(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    evaluation: &AcceptanceEvaluation,
    outcome: &mut ExecutionOutcome,
) {
    let to_state = match evaluation.status {
        AcceptanceStatus::NotEvaluated => "acceptance_skipped",
        AcceptanceStatus::Accepted => "acceptance_accepted",
        AcceptanceStatus::Rejected => "acceptance_rejected",
        AcceptanceStatus::ManualRequired => "acceptance_manual_required",
    };
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "succeeded",
        to_state,
        Some(format!(
            "acceptance:{}:mode={}:reason={}",
            evaluation.status.as_wire(),
            evaluation.mode.map(|m| m.as_wire()).unwrap_or("none"),
            evaluation.reason
        )),
    )
    .await;
    if let Some(w) = warning {
        outcome.bus_publish_warnings.push(w);
    }
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition(format!("succeeded -> {}", to_state))
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt))
    .with_extra("acceptance_status", json!(evaluation.status.as_wire()))
    .with_extra("acceptance_reason", json!(evaluation.reason))
    .with_extra("acceptance_commands", json!(evaluation.commands))
    .with_extra("acceptance_commands_executed", json!(false))
    .with_extra("acceptance_evidence_keys", json!(evaluation.evidence_keys));
    if let Some(mode) = evaluation.mode {
        entry = entry.with_extra("acceptance_mode", json!(mode.as_wire()));
    }
    // wave-18 / task 03 — record the cross-node fan-in outcome so
    // observers can pin the gate decision (mode + source nodes + result
    // + reason) without re-walking prior nodes' evidence. Quiet (the
    // entire `acceptance_fan_in` block is omitted) when the author did
    // not opt into fan-in so the wave-17 byte-shape is preserved.
    if let Some(f) = &evaluation.fan_in {
        entry = entry
            .with_extra("acceptance_fan_in", f.to_json())
            .with_extra("acceptance_fan_in_mode", json!(f.mode.as_wire()))
            .with_extra("acceptance_fan_in_source_nodes", json!(f.source_nodes))
            .with_extra("acceptance_fan_in_passed", json!(f.passed))
            .with_extra("acceptance_fan_in_reason", json!(f.reason));
    }
    if matches!(evaluation.status, AcceptanceStatus::ManualRequired) {
        // Surface the deterministic pause id so downstream resolvers can
        // address the gate without re-deriving the format. Distinct from
        // the wave-16 review-gate id space (`acceptance:` prefix vs
        // `review:`) so the wave-17 / task 01 paused-node resume helper
        // never accidentally consumes an acceptance pause.
        entry = entry.with_extra(
            "acceptance_pause_id",
            json!(derive_acceptance_pause_id(
                ctx.plan_id,
                ctx.plan_version,
                &node.id,
            )),
        );
    }
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: succeeded->acceptance_* evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
}

/// Emit a `pending -> skipped` evidence entry for nodes the scheduler never
/// dispatches (taint propagation, condition gating, fail-fast abort). The
/// `skip_reason` and `skip_detail` fields surface why the skip happened so
/// audit consumers can route on a single transition string.
///
/// Wave-14 / Task 02: also publishes a `PlanNodeStateChanged` event with
/// `from=pending, to=skipped, reason=<skip_reason[:detail]>` so bus consumers
/// can route the same way without re-fetching the sidecar. Bus publish
/// failures land in `outcome.bus_publish_warnings` and the evidence ref
/// degrades to the deterministic id (still live-shape, not unavailable).
pub(super) async fn emit_evidence_skipped(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    skip_reason: &str,
    skip_detail: Option<(&'static str, String)>,
    outcome: &mut ExecutionOutcome,
) {
    let event_reason = match &skip_detail {
        Some((_, detail)) => Some(format!("{}:{}", skip_reason, detail)),
        None => Some(skip_reason.to_string()),
    };
    let (event_ref, warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        PLAN_NODE_DEFAULT_ATTEMPT,
        "pending",
        "skipped",
        event_reason,
    )
    .await;
    if let Some(w) = warning {
        outcome.bus_publish_warnings.push(w);
    }
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("pending -> skipped")
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT))
    .with_extra("skip_reason", json!(skip_reason));
    if let Some((k, v)) = skip_detail {
        entry = entry.with_extra(k, json!(v));
    }
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: pending->skipped evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
}

/// wave-16 / task 04 — emit a `pending -> paused` evidence entry and best-
/// effort `QuestionEvent::Created` for a node that opted into a
/// `:review-gate "question-event"` gate. The deterministic question id is
/// derived via `derive_plan_node_review_question_id` (scope=`plan`,
/// topic-hash=node_id) so wave-16 / task 02's resolution listener can
/// route on the existing `Route { scope=plan, ... }` outcome.
///
/// Bus publish failure is a real gate — the node still pauses (we refuse
/// to dispatch past a failed gate, mirroring the wave-14 fail-fast posture
/// for review-gates) but the warning lands on the response via
/// `outcome.bus_publish_warnings` AND on the per-node `NodeState::Paused`
/// payload so the row can be re-emitted later without losing the id.
pub(super) async fn emit_paused_review_gate(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    plan: &Plan,
    node: &DagNode,
    dispatch_strategy: &str,
    outcome: &mut ExecutionOutcome,
) -> (String, Option<String>) {
    let question_id = super::super::review_gate::derive_plan_node_review_question_id(
        &plan.id.to_string(),
        plan.version,
        &node.id,
        node.review_action.as_deref(),
    );
    // Best-effort `QuestionEvent::Created` publish. Bus failure DOES NOT
    // dispatch the node — a failed gate is still a real gate (we refuse
    // to advance past it). The warning goes to both the per-node payload
    // AND the run-level `bus_publish_warnings` array so callers can
    // grep one place for every degraded gate emit.
    let mut bus_warning: Option<String> = None;
    let ev = QuestionEvent::Created {
        question_id: question_id.clone(),
    };
    if let Err(err) = state.bus.publish_question(ev).await {
        let warning = format!(
            "plan_node_review_gate question publish failed for node `{}` (qid `{}`): {}; \
             node remains paused — review gate is enforced even when the bus is degraded",
            node.id, question_id, err
        );
        tracing::warn!(
            plan_id = %plan.id,
            node_id = %node.id,
            question_id = %question_id,
            error = %err,
            "DAG scheduler: review-gate QuestionEvent::Created publish failed; node still paused"
        );
        outcome.bus_publish_warnings.push(warning.clone());
        bus_warning = Some(warning);
    }

    // Also publish the lifecycle `pending -> paused` transition on the
    // execution bus so observers see the same state-change notification
    // they get for every other lifecycle move.
    let (event_ref, lifecycle_warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        PLAN_NODE_DEFAULT_ATTEMPT,
        "pending",
        "paused",
        Some(format!("review_gate:question-event:{}", question_id)),
    )
    .await;
    if let Some(w) = lifecycle_warning {
        outcome.bus_publish_warnings.push(w);
    }

    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition("pending -> paused")
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(PLAN_NODE_DEFAULT_ATTEMPT))
    .with_extra("review_gate", json!("question-event"))
    .with_extra("review_question_id", json!(question_id));
    if let Some(action) = node.review_action.as_deref() {
        entry = entry.with_extra("review_action", json!(action));
    }
    if let Some(text) = node.review_text.as_deref() {
        entry = entry.with_extra("review_text", json!(text));
    }
    if let Some(w) = bus_warning.as_deref() {
        entry = entry.with_extra("review_question_warning", json!(w));
    }
    let append_outcome = evidence_collector::append(
        state,
        ctx.plan_id,
        ctx.project_arg,
        ctx.cwd_arg,
        ctx.target_project_arg.or(node.target_project.as_deref()),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &append_outcome {
        tracing::warn!(
            plan_id = %ctx.plan_id, node_id = %node.id, error = %error,
            "DAG scheduler: pending->paused evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
    (question_id, bus_warning)
}

mod claims;

#[allow(unused_imports)]
pub(super) use claims::*;
