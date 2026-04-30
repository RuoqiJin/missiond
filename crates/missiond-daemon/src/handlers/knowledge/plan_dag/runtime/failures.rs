use std::collections::HashMap;

use missiond_core::types::Plan;
use serde_json::Value;

use crate::state::AppState;

use super::super::claim_lease::ClaimRegistry;
use super::super::lifecycle::EvidenceCtx;
use super::super::outcome::{ExecutionOutcome, NodeLifecycle, NodeResult, NodeState};
use super::super::parser::{DagNode, ParsedDag, FAILURE_POLICY_FAIL_FAST};
use super::super::scheduler::propagate_taint;
use super::claims::release_claim_if_recorded;
use super::rollbacks::evaluate_and_emit_rollback;

pub(super) async fn record_final_failure<'a>(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    plan: &Plan,
    parsed: &ParsedDag,
    order: &[String],
    node: &DagNode,
    node_id: &str,
    target: String,
    dispatch_strategy: String,
    inner_payload: Value,
    classification: std::result::Result<(), String>,
    non_retryable: bool,
    current_attempt: u32,
    max_attempts: u32,
    claim_registry: &mut ClaimRegistry,
    active_claims_by_node: &mut HashMap<String, String>,
    lifecycle: &mut HashMap<String, NodeLifecycle>,
    results_by_id: &mut HashMap<String, NodeResult>,
    tainted_by: &mut HashMap<String, String>,
    succs: &HashMap<&'a str, Vec<&'a str>>,
    outcome: &mut ExecutionOutcome,
) -> bool {
    // Final failure: exhausted retries OR non-retryable OR fail-fast already
    // aborted this wave.
    lifecycle.insert(node_id.to_string(), NodeLifecycle::Failed);
    release_claim_if_recorded(
        state,
        ctx,
        node,
        &dispatch_strategy,
        current_attempt,
        "failed",
        claim_registry,
        active_claims_by_node,
        outcome,
    )
    .await;

    let reason = classification
        .err()
        .unwrap_or_else(|| "inner handler returned error".to_string());

    // Conservative rollback pass. Runs after the final failed attempt and
    // before downstream taint propagation. Inactive rollback policies stay
    // suppressed inside `evaluate_and_emit_rollback`.
    let rollback_eval = evaluate_and_emit_rollback(
        state,
        ctx,
        plan,
        node,
        parsed,
        order,
        &dispatch_strategy,
        current_attempt,
        outcome,
    )
    .await;
    results_by_id.insert(
        node_id.to_string(),
        NodeResult {
            id: node_id.to_string(),
            target,
            state: NodeState::Failed { reason },
            dispatch_strategy,
            inner_payload,
            attempts_made: current_attempt,
            max_attempts,
            retry_skipped_non_retryable: non_retryable,
            rollback: rollback_eval,
            // Dispatch-failure path skips the acceptance phase: failure
            // dominates.
            acceptance: None,
        },
    );

    // Taint propagation remains governed by the original failure, not the
    // rollback outcome.
    propagate_taint(node, succs, tainted_by);
    node.failure_policy == FAILURE_POLICY_FAIL_FAST
}
