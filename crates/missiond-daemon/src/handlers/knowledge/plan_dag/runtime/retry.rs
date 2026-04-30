use std::collections::HashMap;

use anyhow::Result;
use missiond_core::types::Plan;

use crate::state::AppState;

use super::super::claim_lease::{
    derive_node_claim_scopes, derive_plan_dag_claim_id, ClaimAcquire, ClaimRegistry, PlanDagClaim,
};
use super::super::dispatch::{DispatchOutcome, TaskContractDispatchCtx};
use super::super::lifecycle::{plan_node_should_retry, EvidenceCtx};
use super::super::outcome::{ExecutionOutcome, NodeLifecycle};
use super::super::parser::DagNode;
use super::claims::{record_acquired_claim, record_compat_claim, release_claim_if_recorded};
use super::spawn::spawn_dispatch_attempt;

pub(super) async fn retry_failed_node_if_allowed(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    plan: &Plan,
    node: DagNode,
    node_id: &str,
    dispatch_strategy: &str,
    current_attempt: u32,
    max_attempts: u32,
    non_retryable: bool,
    abort_new_dispatch: bool,
    claim_lease_secs: i64,
    claimer_name: &str,
    attempts_made: &mut HashMap<String, u32>,
    claim_registry: &mut ClaimRegistry,
    active_claims_by_node: &mut HashMap<String, String>,
    task_contract_ctx: &TaskContractDispatchCtx,
    lifecycle: &mut HashMap<String, NodeLifecycle>,
    outcome: &mut ExecutionOutcome,
    join_set: &mut tokio::task::JoinSet<Result<DispatchOutcome>>,
) -> bool {
    // Failure path retry gate. The predicate is `plan_node_should_retry` so
    // unit tests can pin the decision without standing up the wave loop.
    if !plan_node_should_retry(
        current_attempt,
        max_attempts,
        non_retryable,
        abort_new_dispatch,
    ) {
        return false;
    }

    // Release the failed-attempt claim BEFORE re-acquiring on retry so the
    // new attempt's claim id replaces the prior one in the registry without
    // overlap. Best-effort: skip if the original attempt never registered a
    // claim (compat-mode conflict).
    release_claim_if_recorded(
        state,
        ctx,
        &node,
        dispatch_strategy,
        current_attempt,
        "failed_will_retry",
        claim_registry,
        active_claims_by_node,
        outcome,
    )
    .await;

    // Optional sleep between attempts. Skipped when absent / 0 so the common
    // no-back-off case stays cheap.
    if let Some(delay_ms) = node.effective_retry_delay_ms() {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
    }

    // Bump the attempt counter, re-emit `ready -> running` for the retry
    // attempt, and re-spawn into the SAME JoinSet so the wave loop drains it
    // without reshuffling the ready set. Lifecycle stays Running.
    let next_attempt = {
        let entry = attempts_made.entry(node_id.to_string()).or_insert(0);
        *entry += 1;
        *entry
    };

    // Re-acquire claim for retry attempt. Fresh claim id includes the bumped
    // attempt suffix so the audit trail captures every attempt's claim
    // metadata distinctly.
    let (retry_scopes, retry_scope_source) = derive_node_claim_scopes(&node, plan.id);
    let retry_claim_id = derive_plan_dag_claim_id(plan.id, node_id, next_attempt);
    let retry_now = chrono::Utc::now();
    let retry_acquire = claim_registry.try_acquire(
        retry_claim_id.clone(),
        claimer_name.to_string(),
        retry_scopes.clone(),
        retry_scope_source,
        claim_lease_secs,
        retry_now,
    );
    match retry_acquire {
        ClaimAcquire::Acquired(retry_claim) => {
            record_acquired_claim(
                state,
                ctx,
                &node,
                dispatch_strategy,
                next_attempt,
                &retry_claim,
                lifecycle,
                active_claims_by_node,
                outcome,
            )
            .await;
        }
        ClaimAcquire::Conflict {
            attempted_scopes,
            attempted_scope_source,
            conflicting_claim_id,
            conflicting_claimer,
            conflicting_scope,
            offending_scope,
            ..
        } => {
            // Compat / enforce both end here for retries: we are already
            // mid-flight and cannot fail the prior attempt over a retry-claim
            // conflict. Surface the audit row as recorded_compat and continue.
            let synthetic = PlanDagClaim {
                claim_id: retry_claim_id,
                claimer: claimer_name.to_string(),
                scopes: attempted_scopes,
                scope_source: attempted_scope_source,
                acquired_at: retry_now,
                lease_expires_at: retry_now + chrono::Duration::seconds(claim_lease_secs),
                released_at: None,
            };
            record_compat_claim(
                state,
                ctx,
                &node,
                dispatch_strategy,
                next_attempt,
                &synthetic,
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
        }
    }
    spawn_dispatch_attempt(
        state,
        ctx,
        plan,
        node,
        dispatch_strategy,
        next_attempt,
        task_contract_ctx,
        lifecycle,
        outcome,
        join_set,
    )
    .await;
    true
}
