//! Runtime wave loop for mission_plan DAG execution.
//!
//! The parent plan_dag.rs owns the action facade, dry-run projection, and
//! finalization glue. This module owns live node execution: claim, dispatch,
//! retry, acceptance, rollback, taint, and ordered outcome stitching.

use anyhow::Result;
use missiond_core::types::Plan;
use serde_json::Value;
use std::collections::HashMap;

use crate::state::AppState;

use super::claim_lease::{
    parse_claim_lease_secs, parse_claimer_name, parse_enforce_claims, ClaimRegistry,
};
use super::dispatch::{DispatchOutcome, TaskContractDispatchCtx};
use super::lifecycle::{emit_evidence_finished, EvidenceCtx};
use super::outcome::{ExecutionOutcome, NodeResult};
use super::parser::{ParsedDag, FAILURE_POLICY_FAIL_FAST};
use super::scheduler::propagate_taint;

mod acceptance;
mod bookkeeping;
mod claiming;
mod claims;
mod failures;
mod gates;
mod retry;
mod rollbacks;
mod skips;
mod spawn;
use acceptance::evaluate_success_acceptance;
use bookkeeping::{
    build_node_map, build_successor_map, build_topo_index, compute_ready_ids, has_running_nodes,
    initialize_lifecycle, stitch_results_topologically,
};
use claiming::{prepare_dispatch_claim, DispatchClaimDecision};
use claims::release_claim_if_recorded;
use failures::record_final_failure;
use gates::filter_ready_nodes_for_gates;
use retry::retry_failed_node_if_allowed;
use rollbacks::evaluate_and_emit_rollback;
use skips::{force_skip_fail_fast_pending, materialize_tainted_pending_skips};
use spawn::spawn_dispatch_attempt;

pub(super) async fn execute_with_concurrency(
    state: &AppState,
    args: &Value,
    plan: &Plan,
    parsed: &ParsedDag,
    order: &[String],
    max_parallel_nodes: usize,
    task_contract_ctx: TaskContractDispatchCtx,
) -> Result<ExecutionOutcome> {
    let max_parallel = max_parallel_nodes.max(1);
    let by_id = build_node_map(&parsed.nodes);
    // Reverse-adjacency for failure propagation.
    let succs = build_successor_map(&parsed.nodes);
    // Topo position so we can write results in topological order at the end
    // (matches v1's shape — `nodes` array is topologically ordered).
    let topo_index = build_topo_index(order);

    let mut lifecycle = initialize_lifecycle(&parsed.nodes);
    let mut tainted_by: HashMap<String, String> = HashMap::new();
    let mut results_by_id: HashMap<String, NodeResult> = HashMap::new();
    // wave-16 / task 05 — per-node attempt counter. Bumped each time
    // the scheduler hands the node to a dispatch task (whether the
    // first attempt or a retry). Used to stamp the evidence + bus
    // payload `attempt` field, and to decide when retries are
    // exhausted (`attempts_made == effective_max_attempts`).
    let mut attempts_made: HashMap<String, u32> = HashMap::new();
    let mut outcome = ExecutionOutcome::default();
    let mut abort_new_dispatch = false;
    let mut abort_aborter: Option<String> = None;

    // wave-17 / task 02 — claim / lease discipline. The registry is
    // per-DAG-run scratch state (NOT a global lock service). Per-node
    // active claim ids live in `active_claims_by_node` so the wave loop
    // can release them as nodes terminate (succeeded / failed / paused
    // / claim-conflict-aborted). The three knobs come from the call
    // args and surface on the response so callers can tell which
    // discipline mode the run used.
    let claim_lease_secs = parse_claim_lease_secs(args);
    let claimer_name = parse_claimer_name(args);
    let enforce_claims = parse_enforce_claims(args);
    let mut claim_registry = ClaimRegistry::new();
    let mut active_claims_by_node: HashMap<String, String> = HashMap::new();

    let ctx = EvidenceCtx {
        plan_id: plan.id,
        plan_version: plan.version,
        project_arg: args.get("project").and_then(|v| v.as_str()),
        cwd_arg: args.get("cwd").and_then(|v| v.as_str()),
        target_project_arg: args.get("target_project").and_then(|v| v.as_str()),
    };

    loop {
        // 1. Materialise tainted-pending skips up-front so they're recorded
        //    in the response in topological order even when the wave that
        //    causes the taint runs concurrently with their would-have-been
        //    siblings.
        materialize_tainted_pending_skips(
            state,
            &ctx,
            order,
            &by_id,
            &mut lifecycle,
            &tainted_by,
            &mut results_by_id,
            &mut outcome,
        )
        .await;

        // 2. Compute ready set: Pending nodes whose dependencies are all
        //    Succeeded. Sorted by id for deterministic dispatch order.
        let ready_ids = compute_ready_ids(order, &lifecycle, &by_id);

        // 3. If fail-fast aborted and no Running, force-skip remaining
        //    Pending nodes and stop.
        let any_running = has_running_nodes(&lifecycle);
        if abort_new_dispatch && !any_running {
            let aborter = abort_aborter.clone().unwrap_or_default();
            force_skip_fail_fast_pending(
                state,
                &ctx,
                order,
                &by_id,
                &mut lifecycle,
                aborter,
                &mut results_by_id,
                &mut outcome,
            )
            .await;
            break;
        }

        // 4. If nothing ready and nothing running, we're done.
        if ready_ids.is_empty() && !any_running {
            break;
        }

        // 5. Filter ready set by condition gate, then review gate. Nodes
        //    with non-empty `:condition` skip in v2 just like v1 — taint
        //    propagated. Nodes with `:review-gate "question-event"` pause
        //    in place (wave-16 / task 04) — the scheduler emits
        //    `QuestionEvent::Created` and refuses to dispatch the target
        //    tool. Paused nodes do NOT propagate taint (they are not a
        //    failure — auto-resume is wave-16 / task 02 territory) but
        //    their downstream stays Pending until a follow-up call
        //    revives them.
        let to_dispatch = filter_ready_nodes_for_gates(
            state,
            &ctx,
            plan,
            &ready_ids,
            max_parallel,
            &by_id,
            &succs,
            &mut lifecycle,
            &mut tainted_by,
            &mut results_by_id,
            &mut outcome,
        )
        .await;

        if to_dispatch.is_empty() {
            // Either everything ready was condition-gated (loop again to pick
            // up the new tainted skips) or nothing's ready and something is
            // still running — in either case, short-circuit if no JoinSet
            // work is needed and no progress was made on this iteration.
            if !any_running {
                continue;
            }
            // Shouldn't happen because we'd hit step 4 already, but be safe.
        }

        // 6. Mark dispatched nodes Running, write start evidence, spawn.
        // wave-16 / task 05 — every spawn (first attempt or retry) bumps
        // the per-node `attempts_made` counter and stamps the resulting
        // attempt number onto the evidence + bus payload so audit
        // dashboards can route on `attempt > 1` without reconstructing
        // the retry policy from scratch.
        //
        // wave-17 / task 02 — every dispatched node passes through the
        // `pending -> claimed -> running` ladder. Claim acquisition runs
        // BEFORE the spawn so `enforce_claims=true` can fail-fast on
        // an unresolvable overlap without ever touching the inner
        // handler. Under `enforce_claims=false` the registry still
        // records best-effort metadata (so observers can tell the
        // discipline ran) but the scheduler never blocks dispatch on
        // an overlap.
        let mut join_set: tokio::task::JoinSet<Result<DispatchOutcome>> =
            tokio::task::JoinSet::new();
        for node in to_dispatch {
            let dispatch_strategy = node
                .dispatch_strategy
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            let attempt = {
                let entry = attempts_made.entry(node.id.clone()).or_insert(0);
                *entry += 1;
                *entry
            };

            match prepare_dispatch_claim(
                state,
                &ctx,
                plan.id,
                &node,
                &dispatch_strategy,
                attempt,
                claim_lease_secs,
                &claimer_name,
                enforce_claims,
                &mut claim_registry,
                &mut active_claims_by_node,
                &mut lifecycle,
                &mut results_by_id,
                &mut tainted_by,
                &succs,
                &mut outcome,
            )
            .await
            {
                DispatchClaimDecision::Dispatch => {}
                DispatchClaimDecision::ConflictFailed { fail_fast_abort } => {
                    if fail_fast_abort {
                        abort_new_dispatch = true;
                        abort_aborter = Some(node.id.clone());
                    }
                    continue;
                }
            }

            spawn_dispatch_attempt(
                state,
                &ctx,
                plan,
                node,
                &dispatch_strategy,
                attempt,
                &task_contract_ctx,
                &mut lifecycle,
                &mut outcome,
                &mut join_set,
            )
            .await;
        }

        // 7. Drain wave; for each result decide success/failure, update
        //    lifecycle + taint, write finish evidence.
        //
        // wave-16 / task 05 — on failure, consult the per-node retry
        // policy. If the node opted in (`effective_max_attempts > 1`)
        // AND the failure is retryable (not a deterministic
        // safe-descriptor refusal) AND attempts remain, re-spawn the
        // node into the SAME wave's JoinSet with the next attempt
        // number. The node stays `Running`; only when retries are
        // exhausted (or the failure is non-retryable) do we mark it
        // `Failed` + propagate taint + maybe trip fail-fast.
        while let Some(joined) = join_set.join_next().await {
            let dispatch_outcome = match joined {
                Ok(Ok(o)) => o,
                Ok(Err(e)) => {
                    // Rare: the inner handler returned an `anyhow::Error`
                    // (panic-equivalent). Treat as a fatal scheduler error so
                    // the caller sees something — bubbling up here aborts the
                    // whole dispatch, which is the right thing for an
                    // unhandled exception.
                    return Err(e);
                }
                Err(join_err) => {
                    // tokio task panicked. Same reasoning as above.
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
            // The attempt # we are currently finishing. Authoritative
            // because it was bumped at spawn time.
            let current_attempt = attempts_made.get(&node_id).copied().unwrap_or(1);
            let max_attempts = node.effective_max_attempts();
            emit_evidence_finished(
                state,
                &ctx,
                &node,
                &dispatch_strategy,
                &inner_payload,
                succeeded,
                current_attempt,
                &mut outcome,
            )
            .await;
            if succeeded {
                // wave-17 / task 03 — deterministic acceptance phase.
                // Runs ONLY on the success branch (failure already
                // dominates the lifecycle). NEVER executes shell — the
                // evaluator is a pure projection over `(node, payload)`
                // and decides one of: NotEvaluated (no hints — preserve
                // wave-13 behaviour), Accepted, Rejected, ManualRequired.
                // The first three terminate the node (succeeded /
                // failed / paused) without further dispatch.
                //
                // wave-18 / task 03 — `apply_acceptance_fan_in` then
                // overlays cross-node fan-in on top of the per-node
                // result. The validator already proved every fan-in dep
                // is a transitive `:depends-on` ancestor, so the prior
                // node's result is guaranteed to live in `results_by_id`
                // by the time this branch runs.
                let acceptance_outcome = evaluate_success_acceptance(
                    state,
                    &ctx,
                    plan,
                    &node,
                    &dispatch_strategy,
                    current_attempt,
                    &inner_payload,
                    &results_by_id,
                    &mut outcome,
                )
                .await;
                let acceptance_rejected = acceptance_outcome.is_rejected();
                lifecycle.insert(node_id.clone(), acceptance_outcome.next_lifecycle);
                // wave-17 / task 02 — release the claim now that the
                // terminal state is set. Best-effort: we only release
                // when the registry actually recorded the claim
                // (compat-mode conflicts skip the registry insert).
                release_claim_if_recorded(
                    state,
                    &ctx,
                    &node,
                    &dispatch_strategy,
                    current_attempt,
                    acceptance_outcome.terminal_state_label,
                    &mut claim_registry,
                    &mut active_claims_by_node,
                    &mut outcome,
                )
                .await;
                // wave-17 / task 04 — acceptance-rejected nodes are
                // node-level failures and warrant the same rollback
                // pass as a dispatch-time failure. Runs BEFORE
                // `propagate_taint` so the downstream behaviour is
                // governed by the existing failure-policy contract.
                // Skipped for accepted / paused / not-evaluated
                // branches (the node is not in a "final failed"
                // state for those).
                let acc_rollback_eval = if acceptance_rejected {
                    evaluate_and_emit_rollback(
                        state,
                        &ctx,
                        plan,
                        &node,
                        parsed,
                        order,
                        &dispatch_strategy,
                        current_attempt,
                        &mut outcome,
                    )
                    .await
                } else {
                    None
                };
                // wave-17 / task 03 — Rejected acceptance also taints
                // downstream and may trip fail-fast (matches the
                // dispatch-failure path so consumers get one shape for
                // any non-success terminal state).
                if acceptance_rejected {
                    propagate_taint(&node, &succs, &mut tainted_by);
                    if node.failure_policy == FAILURE_POLICY_FAIL_FAST {
                        abort_new_dispatch = true;
                        abort_aborter = Some(node_id.clone());
                    }
                }
                results_by_id.insert(
                    node_id.clone(),
                    NodeResult {
                        id: node_id,
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
                continue;
            }

            if retry_failed_node_if_allowed(
                state,
                &ctx,
                plan,
                node.clone(),
                &node_id,
                &dispatch_strategy,
                current_attempt,
                max_attempts,
                non_retryable,
                abort_new_dispatch,
                claim_lease_secs,
                &claimer_name,
                &mut attempts_made,
                &mut claim_registry,
                &mut active_claims_by_node,
                &task_contract_ctx,
                &mut lifecycle,
                &mut outcome,
                &mut join_set,
            )
            .await
            {
                continue;
            }

            if record_final_failure(
                state,
                &ctx,
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
                &mut claim_registry,
                &mut active_claims_by_node,
                &mut lifecycle,
                &mut results_by_id,
                &mut tainted_by,
                &succs,
                &mut outcome,
            )
            .await
            {
                abort_new_dispatch = true;
                abort_aborter = Some(node_id.clone());
            }
        }
    }

    if abort_new_dispatch {
        outcome.aborted_fail_fast = true;
    }

    // Stitch results back into topological order so the response array's
    // shape matches v1.
    outcome.results = stitch_results_topologically(results_by_id, &topo_index);

    Ok(outcome)
}
