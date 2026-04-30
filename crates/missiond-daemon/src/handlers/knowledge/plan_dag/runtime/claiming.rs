use std::collections::HashMap;

use serde_json::json;

use crate::state::AppState;

use super::super::claim_lease::{
    derive_node_claim_scopes, derive_plan_dag_claim_id, ClaimAcquire, ClaimRegistry, PlanDagClaim,
};
use super::super::lifecycle::{emit_evidence_claim_conflict, EvidenceCtx};
use super::super::outcome::{ExecutionOutcome, NodeLifecycle, NodeResult, NodeState};
use super::super::parser::{DagNode, FAILURE_POLICY_FAIL_FAST};
use super::super::scheduler::propagate_taint;
use super::claims::{record_acquired_claim, record_compat_claim};

pub(super) enum DispatchClaimDecision {
    Dispatch,
    ConflictFailed { fail_fast_abort: bool },
}

pub(super) async fn prepare_dispatch_claim<'a>(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    plan_id: uuid::Uuid,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    claim_lease_secs: i64,
    claimer_name: &str,
    enforce_claims: bool,
    claim_registry: &mut ClaimRegistry,
    active_claims_by_node: &mut HashMap<String, String>,
    lifecycle: &mut HashMap<String, NodeLifecycle>,
    results_by_id: &mut HashMap<String, NodeResult>,
    tainted_by: &mut HashMap<String, String>,
    succs: &HashMap<&'a str, Vec<&'a str>>,
    outcome: &mut ExecutionOutcome,
) -> DispatchClaimDecision {
    let (scopes, scope_source) = derive_node_claim_scopes(node, plan_id);
    let claim_id = derive_plan_dag_claim_id(plan_id, &node.id, attempt);
    let acquire_now = chrono::Utc::now();
    let acquire_outcome = claim_registry.try_acquire(
        claim_id.clone(),
        claimer_name.to_string(),
        scopes.clone(),
        scope_source,
        claim_lease_secs,
        acquire_now,
    );

    match acquire_outcome {
        ClaimAcquire::Acquired(claim) => {
            record_acquired_claim(
                state,
                ctx,
                node,
                dispatch_strategy,
                attempt,
                &claim,
                lifecycle,
                active_claims_by_node,
                outcome,
            )
            .await;
            DispatchClaimDecision::Dispatch
        }
        ClaimAcquire::Conflict {
            attempted_claim_id,
            attempted_scopes,
            attempted_scope_source,
            conflicting_claim_id,
            conflicting_claimer,
            conflicting_scope,
            offending_scope,
        } => {
            if enforce_claims {
                record_strict_claim_conflict(
                    state,
                    ctx,
                    node,
                    dispatch_strategy,
                    attempt,
                    attempted_claim_id,
                    attempted_scopes,
                    attempted_scope_source,
                    conflicting_claim_id,
                    conflicting_claimer,
                    conflicting_scope,
                    offending_scope,
                    lifecycle,
                    results_by_id,
                    tainted_by,
                    succs,
                    outcome,
                )
                .await
            } else {
                let synthetic_claim = PlanDagClaim {
                    claim_id: attempted_claim_id,
                    claimer: claimer_name.to_string(),
                    scopes: attempted_scopes,
                    scope_source: attempted_scope_source,
                    acquired_at: acquire_now,
                    lease_expires_at: acquire_now + chrono::Duration::seconds(claim_lease_secs),
                    released_at: None,
                };
                record_compat_claim(
                    state,
                    ctx,
                    node,
                    dispatch_strategy,
                    attempt,
                    &synthetic_claim,
                    (
                        conflicting_claim_id,
                        conflicting_claimer,
                        conflicting_scope,
                        offending_scope,
                    ),
                    lifecycle,
                    outcome,
                )
                .await;
                DispatchClaimDecision::Dispatch
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn record_strict_claim_conflict<'a>(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    node: &DagNode,
    dispatch_strategy: &str,
    attempt: u32,
    attempted_claim_id: String,
    attempted_scopes: Vec<String>,
    attempted_scope_source: &'static str,
    conflicting_claim_id: String,
    conflicting_claimer: String,
    conflicting_scope: String,
    offending_scope: String,
    lifecycle: &mut HashMap<String, NodeLifecycle>,
    results_by_id: &mut HashMap<String, NodeResult>,
    tainted_by: &mut HashMap<String, String>,
    succs: &HashMap<&'a str, Vec<&'a str>>,
    outcome: &mut ExecutionOutcome,
) -> DispatchClaimDecision {
    lifecycle.insert(node.id.clone(), NodeLifecycle::Failed);
    emit_evidence_claim_conflict(
        state,
        ctx,
        node,
        dispatch_strategy,
        attempt,
        &attempted_claim_id,
        &attempted_scopes,
        attempted_scope_source,
        &conflicting_claim_id,
        &conflicting_claimer,
        &conflicting_scope,
        &offending_scope,
        outcome,
    )
    .await;
    let reason = format!(
        "CLAIM_CONFLICT: scope `{}` overlaps active claim {} held by `{}` over `{}`",
        offending_scope, conflicting_claim_id, conflicting_claimer, conflicting_scope
    );
    let inner_payload = json!({
        "error": reason.clone(),
        "claim_status": "conflict",
        "attempted_claim_id": attempted_claim_id,
        "attempted_claim_scopes": attempted_scopes,
        "attempted_claim_scope_source": attempted_scope_source,
        "conflicting_claim_id": conflicting_claim_id,
        "conflicting_claimer": conflicting_claimer,
        "conflicting_scope": conflicting_scope,
        "offending_scope": offending_scope,
    });
    results_by_id.insert(
        node.id.clone(),
        NodeResult {
            id: node.id.clone(),
            target: node.target.clone(),
            state: NodeState::Failed { reason },
            dispatch_strategy: dispatch_strategy.to_string(),
            inner_payload,
            attempts_made: attempt,
            max_attempts: node.effective_max_attempts(),
            retry_skipped_non_retryable: true,
            rollback: None,
            acceptance: None,
        },
    );
    propagate_taint(node, succs, tainted_by);
    DispatchClaimDecision::ConflictFailed {
        fail_fast_abort: node.failure_policy == FAILURE_POLICY_FAIL_FAST,
    }
}
