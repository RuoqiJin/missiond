//! Runtime wave loop for mission_plan DAG execution.
//!
//! The parent plan_dag.rs owns the action facade, dry-run projection, and
//! finalization glue. This module owns live node execution: claim, dispatch,
//! retry, acceptance, rollback, taint, and ordered outcome stitching.

use anyhow::Result;
use missiond_core::types::Plan;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::state::AppState;

use super::acceptance::{
    apply_acceptance_fan_in, derive_acceptance_pause_id, evaluate_node_acceptance, AcceptanceStatus,
};
use super::claim_lease::{
    derive_node_claim_scopes, derive_plan_dag_claim_id, parse_claim_lease_secs, parse_claimer_name,
    parse_enforce_claims, ClaimAcquire, ClaimRegistry, PlanDagClaim,
};
use super::dispatch::{dispatch_node, DispatchOutcome, TaskContractDispatchCtx};
use super::lifecycle::{
    emit_evidence_acceptance, emit_evidence_claim_conflict, emit_evidence_claim_released,
    emit_evidence_claimed, emit_evidence_finished, emit_evidence_rollback, emit_evidence_running,
    emit_evidence_skipped, emit_paused_review_gate, plan_node_should_retry, EvidenceCtx,
};
use super::outcome::{ExecutionOutcome, NodeLifecycle, NodeResult, NodeState};
use super::parser::{DagNode, ParsedDag, ReviewGateKind, FAILURE_POLICY_FAIL_FAST};
use super::rollback::{run_cascade_rollback, run_rollback};
use super::scheduler::propagate_taint;

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
    let by_id: HashMap<String, DagNode> = parsed
        .nodes
        .iter()
        .map(|n| (n.id.clone(), n.clone()))
        .collect();

    // Reverse-adjacency for failure propagation.
    let mut succs: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in &parsed.nodes {
        for dep in &n.depends_on {
            succs.entry(dep.as_str()).or_default().push(n.id.as_str());
        }
    }
    // Topo position so we can write results in topological order at the end
    // (matches v1's shape — `nodes` array is topologically ordered).
    let topo_index: HashMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();

    let mut lifecycle: HashMap<String, NodeLifecycle> = parsed
        .nodes
        .iter()
        .map(|n| (n.id.clone(), NodeLifecycle::Pending))
        .collect();
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
        let mut became_skipped: Vec<(String, NodeState)> = Vec::new();
        for id in order {
            if !matches!(lifecycle.get(id.as_str()), Some(NodeLifecycle::Pending)) {
                continue;
            }
            if let Some(failed_dep) = tainted_by.get(id.as_str()).cloned() {
                became_skipped.push((id.clone(), NodeState::SkippedUpstreamFailed { failed_dep }));
            }
        }
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
                &ctx,
                &node,
                &dispatch_strategy,
                skip_reason,
                skip_detail,
                &mut outcome,
            )
            .await;
            let target_clone = node.target.clone();
            results_by_id.insert(
                id.clone(),
                NodeResult::skipped(id, target_clone, state_skip, dispatch_strategy),
            );
        }

        // 2. Compute ready set: Pending nodes whose dependencies are all
        //    Succeeded. Sorted by id for deterministic dispatch order.
        let mut ready_ids: Vec<String> = Vec::new();
        for id in order {
            if !matches!(lifecycle.get(id.as_str()), Some(NodeLifecycle::Pending)) {
                continue;
            }
            let node = match by_id.get(id.as_str()) {
                Some(n) => n,
                None => continue,
            };
            let deps_done = node
                .depends_on
                .iter()
                .all(|dep| matches!(lifecycle.get(dep.as_str()), Some(NodeLifecycle::Succeeded)));
            if deps_done {
                ready_ids.push(id.clone());
            }
        }
        ready_ids.sort();

        // 3. If fail-fast aborted and no Running, force-skip remaining
        //    Pending nodes and stop.
        let any_running = lifecycle
            .values()
            .any(|s| matches!(s, NodeLifecycle::Running));
        if abort_new_dispatch && !any_running {
            let aborter = abort_aborter.clone().unwrap_or_default();
            // Force-skip every still-pending node (including ones already in
            // the just-computed ready set — fail-fast supersedes ready).
            let pending_ids: Vec<String> = order
                .iter()
                .filter(|id| matches!(lifecycle.get(id.as_str()), Some(NodeLifecycle::Pending)))
                .cloned()
                .collect();
            for id in pending_ids {
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
                    &ctx,
                    &node,
                    &dispatch_strategy,
                    "fail_fast_aborted",
                    Some(("aborter", aborter.clone())),
                    &mut outcome,
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
        let mut to_dispatch: Vec<DagNode> = Vec::new();
        for id in &ready_ids {
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
                    &ctx,
                    node,
                    &dispatch_strategy,
                    "condition_gated",
                    node.condition.as_ref().map(|c| ("condition", c.clone())),
                    &mut outcome,
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
                propagate_taint(node, &succs, &mut tainted_by);
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
                let (question_id, bus_publish_warning) = emit_paused_review_gate(
                    state,
                    &ctx,
                    plan,
                    node,
                    &dispatch_strategy,
                    &mut outcome,
                )
                .await;
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

            // wave-17 / task 02 — try to acquire a claim covering the
            // node's derived scopes. The acquire runs against the
            // shared per-DAG registry; conflicts are decided by the
            // shared `scopes_overlap_pure` predicate.
            let (scopes, scope_source) = derive_node_claim_scopes(&node, plan.id);
            let claim_id = derive_plan_dag_claim_id(plan.id, &node.id, attempt);
            let acquire_now = chrono::Utc::now();
            let acquire_outcome = claim_registry.try_acquire(
                claim_id.clone(),
                claimer_name.clone(),
                scopes.clone(),
                scope_source,
                claim_lease_secs,
                acquire_now,
            );

            match acquire_outcome {
                ClaimAcquire::Acquired(claim) => {
                    lifecycle.insert(node.id.clone(), NodeLifecycle::Claimed);
                    emit_evidence_claimed(
                        state,
                        &ctx,
                        &node,
                        &dispatch_strategy,
                        attempt,
                        &claim,
                        "acquired",
                        None,
                        &mut outcome,
                    )
                    .await;
                    active_claims_by_node.insert(node.id.clone(), claim.claim_id.clone());
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
                        // Strict mode — refuse to dispatch. Mark the
                        // node failed, emit `pending -> failed` with
                        // the structured CLAIM_CONFLICT reason, do NOT
                        // spawn the inner handler.
                        lifecycle.insert(node.id.clone(), NodeLifecycle::Failed);
                        emit_evidence_claim_conflict(
                            state,
                            &ctx,
                            &node,
                            &dispatch_strategy,
                            attempt,
                            &attempted_claim_id,
                            &attempted_scopes,
                            attempted_scope_source,
                            &conflicting_claim_id,
                            &conflicting_claimer,
                            &conflicting_scope,
                            &offending_scope,
                            &mut outcome,
                        )
                        .await;
                        let reason = format!(
                            "CLAIM_CONFLICT: scope `{}` overlaps active claim {} \
                             held by `{}` over `{}`",
                            offending_scope,
                            conflicting_claim_id,
                            conflicting_claimer,
                            conflicting_scope
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
                                dispatch_strategy: dispatch_strategy.clone(),
                                inner_payload,
                                attempts_made: attempt,
                                max_attempts: node.effective_max_attempts(),
                                retry_skipped_non_retryable: true,
                                // wave-17 / task 04 — claim-conflict
                                // refusal happens BEFORE any handler
                                // runs; the rollback evaluator never
                                // gets a chance to reason about it
                                // because the failure is purely a
                                // coordination event, not a node-level
                                // failure that warrants compensation.
                                rollback: None,
                                // wave-17 / task 03 — claim-conflict
                                // refusal happens BEFORE the inner
                                // handler runs; acceptance phase is
                                // never reached for this node.
                                acceptance: None,
                            },
                        );
                        // Taint propagation — the failed node still
                        // taints downstream so the rest of the DAG
                        // sees the failure as a real one, AND
                        // fail-fast trips when policy says so.
                        propagate_taint(&node, &succs, &mut tainted_by);
                        if node.failure_policy == FAILURE_POLICY_FAIL_FAST {
                            abort_new_dispatch = true;
                            abort_aborter = Some(node.id.clone());
                        }
                        continue;
                    }
                    // Compat mode — best-effort record the claim into
                    // the registry under a synthetic id so the audit
                    // row carries the metadata. We synthesise a
                    // record (NOT inserted into the registry to avoid
                    // poisoning future overlap checks) and attach the
                    // conflict snapshot so dashboards can spot
                    // "compat mode papered over a real conflict".
                    let synthetic_claim = PlanDagClaim {
                        claim_id: attempted_claim_id.clone(),
                        claimer: claimer_name.clone(),
                        scopes: attempted_scopes,
                        scope_source: attempted_scope_source,
                        acquired_at: acquire_now,
                        lease_expires_at: acquire_now + chrono::Duration::seconds(claim_lease_secs),
                        released_at: None,
                    };
                    lifecycle.insert(node.id.clone(), NodeLifecycle::Claimed);
                    emit_evidence_claimed(
                        state,
                        &ctx,
                        &node,
                        &dispatch_strategy,
                        attempt,
                        &synthetic_claim,
                        "recorded_compat",
                        Some((
                            conflicting_claim_id,
                            conflicting_claimer,
                            conflicting_scope,
                            offending_scope,
                        )),
                        &mut outcome,
                    )
                    .await;
                    // No registry entry, no per-node active claim
                    // map entry — release skip is intentional: we
                    // never registered the claim, so there's nothing
                    // to release. Audit row already captured the
                    // metadata; downstream nodes still see the held
                    // scope on the original conflicting claim, which
                    // is the right thing for compat mode.
                }
            }

            lifecycle.insert(node.id.clone(), NodeLifecycle::Running);
            emit_evidence_running(
                state,
                &ctx,
                &node,
                &dispatch_strategy,
                attempt,
                &mut outcome,
            )
            .await;
            let state_clone = state.clone();
            let plan_clone = plan.clone();
            let task_contract_ctx_clone = task_contract_ctx.clone();
            join_set.spawn(async move {
                dispatch_node(state_clone, plan_clone, node, task_contract_ctx_clone).await
            });
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
                let acceptance_base = evaluate_node_acceptance(&node, &inner_payload, true);
                let prior_results_view: HashMap<String, &NodeResult> =
                    results_by_id.iter().map(|(k, v)| (k.clone(), v)).collect();
                let acceptance =
                    apply_acceptance_fan_in(acceptance_base, &node, &prior_results_view);
                let acceptance_active = !acceptance.is_inactive();
                if acceptance_active {
                    emit_evidence_acceptance(
                        state,
                        &ctx,
                        &node,
                        &dispatch_strategy,
                        current_attempt,
                        &acceptance,
                        &mut outcome,
                    )
                    .await;
                }
                let terminal_state_label = match acceptance.status {
                    AcceptanceStatus::NotEvaluated | AcceptanceStatus::Accepted => "succeeded",
                    AcceptanceStatus::Rejected => "failed",
                    AcceptanceStatus::ManualRequired => "paused",
                };
                let next_node_state: NodeState = match acceptance.status {
                    AcceptanceStatus::NotEvaluated | AcceptanceStatus::Accepted => {
                        NodeState::Succeeded
                    }
                    AcceptanceStatus::Rejected => NodeState::Failed {
                        reason: format!("acceptance_rejected: {}", acceptance.reason),
                    },
                    AcceptanceStatus::ManualRequired => {
                        let qid = derive_acceptance_pause_id(plan.id, plan.version, &node.id);
                        NodeState::Paused {
                            question_id: qid,
                            bus_publish_warning: None,
                        }
                    }
                };
                let next_lifecycle = match acceptance.status {
                    AcceptanceStatus::NotEvaluated | AcceptanceStatus::Accepted => {
                        NodeLifecycle::Succeeded
                    }
                    AcceptanceStatus::Rejected => NodeLifecycle::Failed,
                    AcceptanceStatus::ManualRequired => NodeLifecycle::Paused,
                };
                lifecycle.insert(node_id.clone(), next_lifecycle);
                // wave-17 / task 02 — release the claim now that the
                // terminal state is set. Best-effort: we only release
                // when the registry actually recorded the claim
                // (compat-mode conflicts skip the registry insert).
                if let Some(claim_id) = active_claims_by_node.remove(&node_id) {
                    if let Some(released) = claim_registry.release(&claim_id, chrono::Utc::now()) {
                        emit_evidence_claim_released(
                            state,
                            &ctx,
                            &node,
                            &dispatch_strategy,
                            current_attempt,
                            &released,
                            terminal_state_label,
                            &mut outcome,
                        )
                        .await;
                    }
                }
                // wave-17 / task 04 — acceptance-rejected nodes are
                // node-level failures and warrant the same rollback
                // pass as a dispatch-time failure. Runs BEFORE
                // `propagate_taint` so the downstream behaviour is
                // governed by the existing failure-policy contract.
                // Skipped for accepted / paused / not-evaluated
                // branches (the node is not in a "final failed"
                // state for those).
                let acc_rollback_eval = if matches!(acceptance.status, AcceptanceStatus::Rejected) {
                    let mut eval = run_rollback(state, plan, &node).await;
                    // wave-18 / task 04 — cascade rollback pass after
                    // node-local rollback. Fold into the same evaluation
                    // so audit dashboards see a single rollback block.
                    if node.has_active_rollback_cascade() {
                        let cascade =
                            run_cascade_rollback(state, plan, &node, &parsed.nodes, order).await;
                        if !cascade.is_inactive() {
                            eval.cascade = Some(cascade);
                        }
                    }
                    if !eval.is_inactive() {
                        emit_evidence_rollback(
                            state,
                            &ctx,
                            &node,
                            &dispatch_strategy,
                            current_attempt,
                            &eval,
                            &mut outcome,
                        )
                        .await;
                        Some(eval)
                    } else {
                        None
                    }
                } else {
                    None
                };
                // wave-17 / task 03 — Rejected acceptance also taints
                // downstream and may trip fail-fast (matches the
                // dispatch-failure path so consumers get one shape for
                // any non-success terminal state).
                if matches!(acceptance.status, AcceptanceStatus::Rejected) {
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
                        state: next_node_state,
                        dispatch_strategy,
                        inner_payload,
                        attempts_made: current_attempt,
                        max_attempts,
                        retry_skipped_non_retryable: false,
                        rollback: acc_rollback_eval,
                        acceptance: if acceptance_active {
                            Some(acceptance)
                        } else {
                            None
                        },
                    },
                );
                continue;
            }

            // Failure path — decide retry vs final failure. The
            // predicate is `plan_node_should_retry` so unit tests can
            // pin the decision without standing up the wave loop.
            let should_retry = plan_node_should_retry(
                current_attempt,
                max_attempts,
                non_retryable,
                abort_new_dispatch,
            );
            if should_retry {
                // wave-17 / task 02 — release the failed-attempt
                // claim BEFORE re-acquiring on retry so the new
                // attempt's claim id (with the bumped attempt
                // suffix) replaces the prior one in the registry
                // without overlap. Best-effort: skip if the original
                // attempt never registered a claim (compat-mode
                // conflict).
                if let Some(claim_id) = active_claims_by_node.remove(&node_id) {
                    if let Some(released) = claim_registry.release(&claim_id, chrono::Utc::now()) {
                        emit_evidence_claim_released(
                            state,
                            &ctx,
                            &node,
                            &dispatch_strategy,
                            current_attempt,
                            &released,
                            "failed_will_retry",
                            &mut outcome,
                        )
                        .await;
                    }
                }
                // Optional sleep between attempts. Skipped when absent
                // / 0 so the common no-back-off case stays cheap.
                if let Some(delay_ms) = node.effective_retry_delay_ms() {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
                // Bump the attempt counter, re-emit `ready -> running`
                // for the retry attempt, and re-spawn into the SAME
                // JoinSet so the wave loop drains it without
                // reshuffling the ready set. Lifecycle stays Running.
                let next_attempt = {
                    let entry = attempts_made.entry(node_id.clone()).or_insert(0);
                    *entry += 1;
                    *entry
                };
                // wave-17 / task 02 — re-acquire claim for retry
                // attempt. Fresh claim id includes the bumped
                // attempt suffix so the audit trail captures every
                // attempt's claim metadata distinctly.
                let (retry_scopes, retry_scope_source) = derive_node_claim_scopes(&node, plan.id);
                let retry_claim_id = derive_plan_dag_claim_id(plan.id, &node_id, next_attempt);
                let retry_now = chrono::Utc::now();
                let retry_acquire = claim_registry.try_acquire(
                    retry_claim_id.clone(),
                    claimer_name.clone(),
                    retry_scopes.clone(),
                    retry_scope_source,
                    claim_lease_secs,
                    retry_now,
                );
                match retry_acquire {
                    ClaimAcquire::Acquired(retry_claim) => {
                        lifecycle.insert(node_id.clone(), NodeLifecycle::Claimed);
                        emit_evidence_claimed(
                            state,
                            &ctx,
                            &node,
                            &dispatch_strategy,
                            next_attempt,
                            &retry_claim,
                            "acquired",
                            None,
                            &mut outcome,
                        )
                        .await;
                        active_claims_by_node.insert(node_id.clone(), retry_claim.claim_id.clone());
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
                        // Compat / enforce both end here for retries
                        // — we already mid-flight and cannot fail
                        // the prior attempt over a retry-claim
                        // conflict. Surface the audit row as
                        // recorded_compat (the claim is informational
                        // only on retries) and continue.
                        let synthetic = PlanDagClaim {
                            claim_id: retry_claim_id.clone(),
                            claimer: claimer_name.clone(),
                            scopes: attempted_scopes,
                            scope_source: attempted_scope_source,
                            acquired_at: retry_now,
                            lease_expires_at: retry_now
                                + chrono::Duration::seconds(claim_lease_secs),
                            released_at: None,
                        };
                        lifecycle.insert(node_id.clone(), NodeLifecycle::Claimed);
                        emit_evidence_claimed(
                            state,
                            &ctx,
                            &node,
                            &dispatch_strategy,
                            next_attempt,
                            &synthetic,
                            "recorded_compat",
                            Some((
                                conflicting_claim_id,
                                conflicting_claimer,
                                conflicting_scope,
                                offending_scope,
                            )),
                            &mut outcome,
                        )
                        .await;
                    }
                }
                lifecycle.insert(node_id.clone(), NodeLifecycle::Running);
                emit_evidence_running(
                    state,
                    &ctx,
                    &node,
                    &dispatch_strategy,
                    next_attempt,
                    &mut outcome,
                )
                .await;
                let state_clone = state.clone();
                let plan_clone = plan.clone();
                let node_clone = node.clone();
                let task_contract_ctx_clone = task_contract_ctx.clone();
                join_set.spawn(async move {
                    dispatch_node(state_clone, plan_clone, node_clone, task_contract_ctx_clone)
                        .await
                });
                continue;
            }

            // Final failure — exhausted retries OR non-retryable OR
            // fail-fast already aborted this wave.
            lifecycle.insert(node_id.clone(), NodeLifecycle::Failed);
            // wave-17 / task 02 — release the claim on terminal
            // failure (best-effort, compat-mode conflicts skip).
            if let Some(claim_id) = active_claims_by_node.remove(&node_id) {
                if let Some(released) = claim_registry.release(&claim_id, chrono::Utc::now()) {
                    emit_evidence_claim_released(
                        state,
                        &ctx,
                        &node,
                        &dispatch_strategy,
                        current_attempt,
                        &released,
                        "failed",
                        &mut outcome,
                    )
                    .await;
                }
            }
            let reason = classification
                .err()
                .unwrap_or_else(|| "inner handler returned error".to_string());
            // wave-17 / task 04 — conservative rollback pass. Runs
            // AFTER the final failed attempt and BEFORE downstream
            // taint propagation. Skipped entirely when the node did
            // not opt into a rollback policy so the wave-13 byte
            // shape stays untouched.
            //
            // The rollback evaluator decides one of:
            //   * NotRequested      — no rollback hints / explicit
            //                          `:rollback-policy "none"`.
            //                          Evidence emit suppressed.
            //   * DescriptorReady   — `:rollback-policy "descriptor"`;
            //                          captures intent + brief preview,
            //                          NEVER dispatches.
            //   * Dispatched        — `:rollback-policy "workstation"`
            //                          + every safety gate passed +
            //                          inner handler returned Ok.
            //   * Refused           — `:rollback-policy "workstation"`
            //                          + at least one safety gate
            //                          failed (or substrate-side
            //                          SafeDescriptor refusal).
            //   * Failed            — `:rollback-policy "workstation"`
            //                          dispatched but the inner
            //                          handler returned an error.
            //
            // Downstream taint propagation runs identically afterwards
            // — the rollback pass NEVER changes failure-policy
            // semantics. This keeps the wave-13 / wave-16 contract
            // intact: `:failure-policy fail-fast` still trips the
            // wave-loop abort flag based on the original failure,
            // not the rollback outcome.
            let mut rollback_eval = run_rollback(state, plan, &node).await;
            // wave-18 / task 04 — cascade rollback pass after the
            // node-local rollback. The cascade evaluator is conservative:
            // it never runs unless the failed node opted in via
            // `:rollback-cascade "plan" | "dispatch-safe"`. Folding the
            // outcome into the same `RollbackEvaluation` keeps audit
            // dashboards on a single block per failed node.
            if node.has_active_rollback_cascade() {
                let cascade = run_cascade_rollback(state, plan, &node, &parsed.nodes, order).await;
                if !cascade.is_inactive() {
                    rollback_eval.cascade = Some(cascade);
                }
            }
            let rollback_active = !rollback_eval.is_inactive();
            if rollback_active {
                emit_evidence_rollback(
                    state,
                    &ctx,
                    &node,
                    &dispatch_strategy,
                    current_attempt,
                    &rollback_eval,
                    &mut outcome,
                )
                .await;
            }
            results_by_id.insert(
                node_id.clone(),
                NodeResult {
                    id: node_id.clone(),
                    target,
                    state: NodeState::Failed { reason },
                    dispatch_strategy,
                    inner_payload,
                    attempts_made: current_attempt,
                    max_attempts,
                    retry_skipped_non_retryable: non_retryable,
                    rollback: if rollback_active {
                        Some(rollback_eval)
                    } else {
                        None
                    },
                    // wave-17 / task 03 — dispatch-failure path skips
                    // the acceptance phase (failure dominates).
                    acceptance: None,
                },
            );
            // Taint propagates regardless of policy — it just changes
            // whether *unrelated* nodes also get aborted (fail-fast) or
            // can keep running (continue). wave-17 / task 04: the
            // rollback evaluation does NOT alter this — downstream
            // behaviour stays governed by the existing failure-policy
            // contract.
            propagate_taint(&node, &succs, &mut tainted_by);
            if node.failure_policy == FAILURE_POLICY_FAIL_FAST {
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
    let mut ordered: Vec<(usize, NodeResult)> = results_by_id
        .into_iter()
        .filter_map(|(id, r)| topo_index.get(id.as_str()).map(|&i| (i, r)))
        .collect();
    ordered.sort_by_key(|(i, _)| *i);
    outcome.results = ordered.into_iter().map(|(_, r)| r).collect();

    Ok(outcome)
}
