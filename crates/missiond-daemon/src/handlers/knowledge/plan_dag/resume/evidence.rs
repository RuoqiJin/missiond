use missiond_core::types::Plan;
use serde_json::json;

use crate::handlers::knowledge::evidence_collector::{self, AppendOutcome, EvidenceEntry};
use crate::handlers::knowledge::review_gate::{PlanNodeResumeInput, ReviewDecision};
use crate::state::AppState;

use super::super::{
    publish_plan_node_state_change, DagNode, EvidenceCtx, ExecutionOutcome,
    PLAN_NODE_DEFAULT_ATTEMPT,
};

/// Wave-17 / task 01 — emit a single `paused -> review_resolved`
/// evidence row capturing the resume decision (approved / rejected /
/// needs_changes) for the audit trail. Always runs, regardless of
/// whether we go on to dispatch the node, so the row records the
/// human / operator intent even when the decision keeps the node
/// paused.
///
/// Returns `Some(event_id)` when a `PlanNodeStateChanged` lifecycle
/// event was published (or fell back to the deterministic id) so the
/// caller can splice it onto the response under `resume_event_id`.
/// Returns `None` only when the `EventRef` builder yielded an
/// `unavailable` ref — currently unreachable but kept loose so the
/// helper stays decoupled from ref-availability assumptions.
pub(super) async fn emit_resume_decision_evidence(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    plan: &Plan,
    node: &DagNode,
    dispatch_strategy: &str,
    input: &PlanNodeResumeInput,
    outcome: &mut ExecutionOutcome,
) -> Option<String> {
    let to_state = match input.decision {
        ReviewDecision::Approved => "resume_approved",
        ReviewDecision::Rejected => "resume_rejected",
        ReviewDecision::NeedsChanges => "resume_needs_changes",
    };
    let attempt = PLAN_NODE_DEFAULT_ATTEMPT;
    let (event_ref, lifecycle_warning) = publish_plan_node_state_change(
        state,
        ctx.plan_id,
        node,
        dispatch_strategy,
        attempt,
        "paused",
        to_state,
        Some(format!(
            "review_resume:{}:qid={}",
            input.decision.as_str(),
            input.question_id
        )),
    )
    .await;
    if let Some(w) = lifecycle_warning {
        outcome.bus_publish_warnings.push(w);
    }
    let event_id = event_ref.event_id.clone();

    let mut entry = EvidenceEntry::new(
        evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_state_transition(format!("paused -> {}", to_state))
    .add_execution_event(event_ref)
    .with_extra("scheduler_mode", json!("dag_v1"))
    .with_extra("node_id", json!(node.id))
    .with_extra("plan_id", json!(ctx.plan_id))
    .with_extra("target_tool", json!(node.target))
    .with_extra("target", json!(node.target))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("attempt", json!(attempt))
    .with_extra("review_resume", json!(true))
    .with_extra("review_question_id", json!(input.question_id))
    .with_extra("review_decision", json!(input.decision.as_str()))
    .with_extra("plan_version", json!(plan.version));
    if let Some(actor) = input.actor.as_deref() {
        entry = entry.with_extra("resume_actor", json!(actor));
    }
    if let Some(note) = input.note.as_deref() {
        entry = entry.with_extra("resume_note", json!(note));
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
            plan_id = %ctx.plan_id,
            node_id = %node.id,
            decision = %input.decision.as_str(),
            error = %error,
            "DAG resume: paused->review_* evidence append failed"
        );
    }
    let (path, err) = append_outcome.into_legacy_tuple();
    if let Some(p) = path {
        outcome.evidence_path = Some(p);
    }
    if let Some(e) = err {
        outcome.evidence_error = Some(e);
    }
    event_id
}
