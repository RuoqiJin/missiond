use serde_json::{json, Value};

use crate::handlers::knowledge::evidence_collector::{self, AppendOutcome, EvidenceEntry};
use crate::state::AppState;

use super::super::super::{DagNode, ExecutionOutcome};
use super::super::{publish_plan_node_state_change, EvidenceCtx};

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
