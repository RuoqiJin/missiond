use std::collections::HashMap;

use missiond_core::types::Plan;
use serde_json::Value;

use crate::state::AppState;

use super::super::claim_lease::ClaimRegistry;
use super::super::lifecycle::EvidenceCtx;
use super::super::outcome::{ExecutionOutcome, NodeLifecycle, NodeResult};
use super::super::parser::{DagNode, ParsedDag, FAILURE_POLICY_FAIL_FAST};
use super::super::scheduler::propagate_taint;
use super::acceptance::evaluate_success_acceptance;
use super::claims::release_claim_if_recorded;
use super::rollbacks::evaluate_and_emit_rollback;

pub(super) async fn record_successful_dispatch<'a>(
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
    let acceptance_outcome = evaluate_success_acceptance(
        state,
        ctx,
        plan,
        node,
        &dispatch_strategy,
        current_attempt,
        &inner_payload,
        results_by_id,
        outcome,
    )
    .await;
    let acceptance_rejected = acceptance_outcome.is_rejected();
    lifecycle.insert(node_id.to_string(), acceptance_outcome.next_lifecycle);

    release_claim_if_recorded(
        state,
        ctx,
        node,
        &dispatch_strategy,
        current_attempt,
        acceptance_outcome.terminal_state_label,
        claim_registry,
        active_claims_by_node,
        outcome,
    )
    .await;

    let acc_rollback_eval = if acceptance_rejected {
        evaluate_and_emit_rollback(
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
        .await
    } else {
        None
    };

    if acceptance_rejected {
        propagate_taint(node, succs, tainted_by);
    }

    results_by_id.insert(
        node_id.to_string(),
        NodeResult {
            id: node_id.to_string(),
            target,
            state: acceptance_outcome.next_node_state,
            dispatch_strategy,
            inner_payload,
            attempts_made: current_attempt,
            max_attempts,
            retry_skipped_non_retryable: false,
            rollback: acc_rollback_eval,
            acceptance: if acceptance_outcome.acceptance_active {
                Some(acceptance_outcome.acceptance)
            } else {
                None
            },
        },
    );

    acceptance_rejected && node.failure_policy == FAILURE_POLICY_FAIL_FAST
}
