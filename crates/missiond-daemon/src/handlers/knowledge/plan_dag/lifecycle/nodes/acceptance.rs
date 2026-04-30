use serde_json::json;

use crate::handlers::knowledge::evidence_collector::{self, AppendOutcome, EvidenceEntry};
use crate::state::AppState;

use super::super::super::acceptance::{
    derive_acceptance_pause_id, AcceptanceEvaluation, AcceptanceStatus,
};
use super::super::super::{DagNode, ExecutionOutcome};
use super::super::{publish_plan_node_state_change, EvidenceCtx};

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
