use std::collections::HashMap;

use crate::state::AppState;

use super::super::lifecycle::{emit_evidence_skipped, EvidenceCtx};
use super::super::outcome::{ExecutionOutcome, NodeLifecycle, NodeResult, NodeState};
use super::super::parser::DagNode;
use super::bookkeeping::{collect_tainted_pending, pending_ids};

pub(super) async fn materialize_tainted_pending_skips(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    order: &[String],
    by_id: &HashMap<String, DagNode>,
    lifecycle: &mut HashMap<String, NodeLifecycle>,
    tainted_by: &HashMap<String, String>,
    results_by_id: &mut HashMap<String, NodeResult>,
    outcome: &mut ExecutionOutcome,
) {
    let mut became_skipped = collect_tainted_pending(order, lifecycle, tainted_by);
    for (id, state_skip) in became_skipped.drain(..) {
        let node = match by_id.get(&id) {
            Some(n) => n.clone(),
            None => continue,
        };
        lifecycle.insert(id.clone(), NodeLifecycle::Skipped);
        let dispatch_strategy = node
            .dispatch_strategy
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let (skip_reason, skip_detail) = match &state_skip {
            NodeState::SkippedUpstreamFailed { failed_dep } => {
                ("upstream_failed", Some(("failed_dep", failed_dep.clone())))
            }
            _ => ("upstream_failed", None),
        };
        emit_evidence_skipped(
            state,
            ctx,
            &node,
            &dispatch_strategy,
            skip_reason,
            skip_detail,
            outcome,
        )
        .await;
        let target_clone = node.target.clone();
        results_by_id.insert(
            id.clone(),
            NodeResult::skipped(id, target_clone, state_skip, dispatch_strategy),
        );
    }
}

pub(super) async fn force_skip_fail_fast_pending(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    order: &[String],
    by_id: &HashMap<String, DagNode>,
    lifecycle: &mut HashMap<String, NodeLifecycle>,
    aborter: String,
    results_by_id: &mut HashMap<String, NodeResult>,
    outcome: &mut ExecutionOutcome,
) {
    // Force-skip every still-pending node (including ones already in the
    // just-computed ready set — fail-fast supersedes ready).
    for id in pending_ids(order, lifecycle) {
        let node = match by_id.get(&id) {
            Some(n) => n.clone(),
            None => continue,
        };
        lifecycle.insert(id.clone(), NodeLifecycle::Skipped);
        let dispatch_strategy = node
            .dispatch_strategy
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        emit_evidence_skipped(
            state,
            ctx,
            &node,
            &dispatch_strategy,
            "fail_fast_aborted",
            Some(("aborter", aborter.clone())),
            outcome,
        )
        .await;
        let target_clone = node.target.clone();
        results_by_id.insert(
            id.clone(),
            NodeResult::skipped(
                id,
                target_clone,
                NodeState::SkippedFailFastAbort {
                    aborter: aborter.clone(),
                },
                dispatch_strategy,
            ),
        );
    }
}
