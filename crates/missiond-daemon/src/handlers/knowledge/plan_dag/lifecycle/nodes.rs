use serde_json::{json, Value};

use crate::handlers::knowledge::evidence_collector::{self, AppendOutcome, EvidenceEntry};
use crate::state::AppState;

use super::super::acceptance::{
    derive_acceptance_pause_id, AcceptanceEvaluation, AcceptanceStatus,
};
use super::super::rollback::{RollbackEvaluation, RollbackStatus};
use super::super::{DagNode, ExecutionOutcome};
use super::{publish_plan_node_state_change, EvidenceCtx, PLAN_NODE_DEFAULT_ATTEMPT};

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
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_running(
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
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_finished(
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
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_rollback(
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
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_acceptance(
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
pub(in crate::handlers::knowledge::plan_dag) async fn emit_evidence_skipped(
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
