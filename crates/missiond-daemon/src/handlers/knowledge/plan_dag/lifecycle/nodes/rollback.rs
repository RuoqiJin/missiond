use serde_json::json;

use crate::handlers::knowledge::evidence_collector::{self, AppendOutcome, EvidenceEntry};
use crate::state::AppState;

use super::super::super::rollback::{RollbackEvaluation, RollbackStatus};
use super::super::super::{DagNode, ExecutionOutcome};
use super::super::{publish_plan_node_state_change, EvidenceCtx};

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
