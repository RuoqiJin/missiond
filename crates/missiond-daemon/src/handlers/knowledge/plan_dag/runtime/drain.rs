use std::collections::HashMap;

use anyhow::Result;
use missiond_core::types::Plan;

use crate::state::AppState;

use super::super::claim_lease::ClaimRegistry;
use super::super::dispatch::{DispatchOutcome, TaskContractDispatchCtx};
use super::super::lifecycle::{emit_evidence_finished, EvidenceCtx};
use super::super::outcome::{ExecutionOutcome, NodeLifecycle, NodeResult};
use super::super::parser::{DagNode, ParsedDag};
use super::failures::record_final_failure;
use super::retry::retry_failed_node_if_allowed;
use super::success::record_successful_dispatch;

pub(super) async fn drain_dispatch_wave<'a>(
    state: &AppState,
    ctx: &EvidenceCtx<'_>,
    plan: &Plan,
    parsed: &ParsedDag,
    order: &[String],
    by_id: &HashMap<String, DagNode>,
    join_set: &mut tokio::task::JoinSet<Result<DispatchOutcome>>,
    attempts_made: &mut HashMap<String, u32>,
    claim_lease_secs: i64,
    claimer_name: &str,
    abort_new_dispatch: bool,
    task_contract_ctx: &TaskContractDispatchCtx,
    claim_registry: &mut ClaimRegistry,
    active_claims_by_node: &mut HashMap<String, String>,
    lifecycle: &mut HashMap<String, NodeLifecycle>,
    results_by_id: &mut HashMap<String, NodeResult>,
    tainted_by: &mut HashMap<String, String>,
    succs: &HashMap<&'a str, Vec<&'a str>>,
    outcome: &mut ExecutionOutcome,
) -> Result<Option<String>> {
    let mut abort_new_dispatch = abort_new_dispatch;
    let mut abort_aborter = None;

    while let Some(joined) = join_set.join_next().await {
        let dispatch_outcome = match joined {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => {
                return Err(e);
            }
            Err(join_err) => {
                return Err(anyhow::anyhow!(
                    "DAG scheduler: dispatch task join failed: {}",
                    join_err
                ));
            }
        };
        let DispatchOutcome {
            node_id,
            target,
            dispatch_strategy,
            inner_payload,
            classification,
            non_retryable,
        } = dispatch_outcome;
        let node = match by_id.get(&node_id) {
            Some(n) => n.clone(),
            None => continue,
        };
        let succeeded = classification.is_ok();
        // The attempt # we are currently finishing. Authoritative because it
        // was bumped at spawn time.
        let current_attempt = attempts_made.get(&node_id).copied().unwrap_or(1);
        let max_attempts = node.effective_max_attempts();
        emit_evidence_finished(
            state,
            ctx,
            &node,
            &dispatch_strategy,
            &inner_payload,
            succeeded,
            current_attempt,
            outcome,
        )
        .await;
        if succeeded {
            if record_successful_dispatch(
                state,
                ctx,
                plan,
                parsed,
                order,
                &node,
                &node_id,
                target,
                dispatch_strategy,
                inner_payload,
                current_attempt,
                max_attempts,
                claim_registry,
                active_claims_by_node,
                lifecycle,
                results_by_id,
                tainted_by,
                succs,
                outcome,
            )
            .await
            {
                abort_new_dispatch = true;
                abort_aborter = Some(node_id.clone());
            }
            continue;
        }

        if retry_failed_node_if_allowed(
            state,
            ctx,
            plan,
            node.clone(),
            &node_id,
            &dispatch_strategy,
            current_attempt,
            max_attempts,
            non_retryable,
            abort_new_dispatch,
            claim_lease_secs,
            claimer_name,
            attempts_made,
            claim_registry,
            active_claims_by_node,
            task_contract_ctx,
            lifecycle,
            outcome,
            join_set,
        )
        .await?
        {
            continue;
        }

        if record_final_failure(
            state,
            ctx,
            plan,
            parsed,
            order,
            &node,
            &node_id,
            target,
            dispatch_strategy,
            inner_payload,
            classification,
            non_retryable,
            current_attempt,
            max_attempts,
            claim_registry,
            active_claims_by_node,
            lifecycle,
            results_by_id,
            tainted_by,
            succs,
            outcome,
        )
        .await
        {
            abort_new_dispatch = true;
            abort_aborter = Some(node_id.clone());
        }
    }

    Ok(abort_aborter)
}
