use missiond_core::event::events::QuestionEvent;
use missiond_core::types::Plan;
use serde_json::json;

use crate::handlers::knowledge::evidence_collector::{self, AppendOutcome, EvidenceEntry};
use crate::state::AppState;

use super::super::{DagNode, ExecutionOutcome};
use super::{publish_plan_node_state_change, EvidenceCtx, PLAN_NODE_DEFAULT_ATTEMPT};

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
pub(in crate::handlers::knowledge::plan_dag) async fn emit_paused_review_gate(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    plan: &Plan,
    node: &DagNode,
    dispatch_strategy: &str,
    outcome: &mut ExecutionOutcome,
) -> (String, Option<String>) {
    let question_id = crate::handlers::knowledge::review_gate::derive_plan_node_review_question_id(
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
