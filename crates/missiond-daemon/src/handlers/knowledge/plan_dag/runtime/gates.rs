use std::collections::HashMap;

use missiond_core::types::Plan;

use crate::state::AppState;

use super::super::lifecycle::{emit_evidence_skipped, emit_paused_review_gate, EvidenceCtx};
use super::super::outcome::{ExecutionOutcome, NodeLifecycle, NodeResult, NodeState};
use super::super::parser::{DagNode, ReviewGateKind};
use super::super::scheduler::propagate_taint;

pub(super) async fn filter_ready_nodes_for_gates<'a>(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    plan: &Plan,
    ready_ids: &[String],
    max_parallel: usize,
    by_id: &HashMap<String, DagNode>,
    succs: &HashMap<&'a str, Vec<&'a str>>,
    lifecycle: &mut HashMap<String, NodeLifecycle>,
    tainted_by: &mut HashMap<String, String>,
    results_by_id: &mut HashMap<String, NodeResult>,
    outcome: &mut ExecutionOutcome,
) -> Vec<DagNode> {
    let mut to_dispatch: Vec<DagNode> = Vec::new();
    for id in ready_ids {
        let node = match by_id.get(id.as_str()) {
            Some(n) => n,
            None => continue,
        };
        let has_condition = node
            .condition
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if has_condition {
            lifecycle.insert(id.clone(), NodeLifecycle::Skipped);
            let dispatch_strategy = node
                .dispatch_strategy
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            emit_evidence_skipped(
                state,
                ctx,
                node,
                &dispatch_strategy,
                "condition_gated",
                node.condition.as_ref().map(|c| ("condition", c.clone())),
                outcome,
            )
            .await;
            results_by_id.insert(
                id.clone(),
                NodeResult::skipped(
                    id.clone(),
                    node.target.clone(),
                    NodeState::SkippedCondition,
                    dispatch_strategy,
                ),
            );
            propagate_taint(node, succs, tainted_by);
            continue;
        }
        // wave-16 / task 04 — review-gate paused state. The first real
        // non-terminal node state in v2: emit `QuestionEvent::Created`
        // (best-effort; failure still pauses) + a pending->paused
        // evidence row, mark the node `Paused`, do NOT call the
        // target tool. Downstream stays pending; auto-resume lives
        // in wave-16 / task 02's `QuestionEvent::Resolved` listener.
        if let ReviewGateKind::QuestionEvent = node.review_gate_kind() {
            lifecycle.insert(id.clone(), NodeLifecycle::Paused);
            let dispatch_strategy = node
                .dispatch_strategy
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            let (question_id, bus_publish_warning) =
                emit_paused_review_gate(state, ctx, plan, node, &dispatch_strategy, outcome).await;
            results_by_id.insert(
                id.clone(),
                NodeResult::skipped(
                    id.clone(),
                    node.target.clone(),
                    NodeState::Paused {
                        question_id,
                        bus_publish_warning,
                    },
                    dispatch_strategy,
                ),
            );
            continue;
        }
        to_dispatch.push(node.clone());
        if to_dispatch.len() >= max_parallel {
            break;
        }
    }
    to_dispatch
}
