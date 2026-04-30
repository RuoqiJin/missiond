use missiond_core::types::Plan;

use crate::state::AppState;

use super::super::lifecycle::{emit_evidence_rollback, EvidenceCtx};
use super::super::outcome::ExecutionOutcome;
use super::super::parser::{DagNode, ParsedDag};
use super::super::rollback::{run_cascade_rollback, run_rollback, RollbackEvaluation};

pub(super) async fn evaluate_and_emit_rollback(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    plan: &Plan,
    node: &DagNode,
    parsed: &ParsedDag,
    order: &[String],
    dispatch_strategy: &str,
    attempt: u32,
    outcome: &mut ExecutionOutcome,
) -> Option<RollbackEvaluation> {
    let mut evaluation = run_rollback(state, plan, node).await;
    if node.has_active_rollback_cascade() {
        let cascade = run_cascade_rollback(state, plan, node, &parsed.nodes, order).await;
        if !cascade.is_inactive() {
            evaluation.cascade = Some(cascade);
        }
    }
    if evaluation.is_inactive() {
        return None;
    }
    emit_evidence_rollback(
        state,
        ctx,
        node,
        dispatch_strategy,
        attempt,
        &evaluation,
        outcome,
    )
    .await;
    Some(evaluation)
}
