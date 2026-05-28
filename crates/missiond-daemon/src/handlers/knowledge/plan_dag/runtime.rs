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
use super::lifecycle::EvidenceCtx;
use super::outcome::{ExecutionOutcome, NodeResult};
use super::parser::ParsedDag;

mod acceptance;
mod bookkeeping;
mod claiming;
mod claims;
mod drain;
mod failures;
mod gates;
mod retry;
mod rollbacks;
mod skips;
mod spawn;
mod success;
use bookkeeping::{
    build_node_map, build_successor_map, build_topo_index, compute_ready_ids, has_running_nodes,
    initialize_lifecycle, stitch_results_topologically,
};
use claiming::{prepare_dispatch_claim, DispatchClaimDecision};
use drain::drain_dispatch_wave;
use gates::filter_ready_nodes_for_gates;
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
        // handler. Under the hard-cut kernel contract, the scheduler
        // never dispatches a node after a work_leases conflict.
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
            .await?
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

        // 7. Drain wave; the drain stage owns finish evidence plus
        //    success/retry/failure routing while this facade keeps the
        //    high-level wave loop readable.
        if let Some(aborter) = drain_dispatch_wave(
            state,
            &ctx,
            plan,
            parsed,
            order,
            &by_id,
            &mut join_set,
            &mut attempts_made,
            claim_lease_secs,
            &claimer_name,
            abort_new_dispatch,
            &task_contract_ctx,
            &mut claim_registry,
            &mut active_claims_by_node,
            &mut lifecycle,
            &mut results_by_id,
            &mut tainted_by,
            &succs,
            &mut outcome,
        )
        .await?
        {
            abort_new_dispatch = true;
            abort_aborter = Some(aborter);
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
