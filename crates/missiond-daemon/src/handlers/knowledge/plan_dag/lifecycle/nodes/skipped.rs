use serde_json::json;

use crate::handlers::knowledge::evidence_collector::{self, AppendOutcome, EvidenceEntry};
use crate::state::AppState;

use super::super::super::{DagNode, ExecutionOutcome};
use super::super::{publish_plan_node_state_change, EvidenceCtx, PLAN_NODE_DEFAULT_ATTEMPT};

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
