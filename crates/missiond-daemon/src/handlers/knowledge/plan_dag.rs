//! mission_plan — DAG scheduler v2 (bounded ready-node concurrency).
//!
//! This module is loaded by `mission_plan(action=execute, scheduler_mode="dag_v1")`.
//! It is intentionally separated from `plan.rs` so the v0 single-node runner
//! stays untouched as the default contract.
//!
//! v2 changes (Wave 13 / Task 02) keep the parser / validator / dry-run wire
//! shape identical to v1 — they only upgrade the runtime: a wave-based
//! scheduler now dispatches up to `max_parallel_nodes` ready nodes
//! concurrently within the same async task, observes a richer node lifecycle
//! (`pending / ready / running / succeeded / failed / skipped`), and writes
//! one evidence-collector entry per state transition (start + finish for
//! every running node, plus an explicit skip entry for every taint /
//! condition / fail-fast-aborted node).
//!
//! Lisp authority:
//!   - intent-flow.lisp        :: F-intent-alignment-plan-execution-loop ::
//!                                 s6 execution-runner
//!   - intent-intent-layer.lisp :: section unified-entry-pipeline ::
//!                                 role plan-runner
//!   - intent-tools.lisp       :: implemented-surface mission_plan ::
//!                                 :execute-contract
//!
//! Scope (v2) — what this scheduler DOES support:
//!   * Top-level `(node :id ... :target ...)` forms inside an outer
//!     `(plan|plan-draft|PLAN ...)` envelope.
//!   * Field allowlist:
//!       :id (required, unique)
//!       :target (required; one of mission_execution / mission_task_delegate /
//!                mission_flow_run)
//!       :objective
//!       :depends-on (vector / list of node id strings)
//!       :condition
//!       :failure-policy (`fail-fast` (default) | `continue`)
//!       :timeout-ms
//!       :dispatch-strategy
//!       :target-project
//!       :requested-cwd
//!       :flow-id
//!   * Validation:
//!       - Unique `:id` per node.
//!       - All `:depends-on` ids must exist in the same DAG.
//!       - The dependency graph must be acyclic (Kahn topo sort).
//!       - `:target` must be on the inner-dispatch whitelist.
//!   * Execution mode:
//!       - Wave-based scheduler driven by a `tokio::task::JoinSet`. Each
//!         wave drains up to `max_parallel_nodes` ready nodes (default 1
//!         keeps the v1 strictly-sequential contract intact). Ready-node
//!         selection is deterministic (sorted by node id) so test output is
//!         reproducible across runs.
//!       - Node lifecycle: `pending → ready → running → succeeded | failed`
//!         for executed nodes, `pending → skipped` for taint / condition /
//!         fail-fast-aborted nodes. Each transition writes one
//!         `plan_dag_node_dispatch` evidence entry tagged with
//!         `state_transition`.
//!       - `failure-policy=fail-fast` (default): the failing node taints its
//!         transitive downstream and the scheduler stops dispatching new
//!         waves. In-flight nodes from the *current* wave are awaited so the
//!         caller still sees their final state — they are never abandoned
//!         mid-flight. Any nodes still `pending` after the in-flight wave
//!         drains are marked `skipped` with reason `fail_fast_aborted`.
//!       - `failure-policy=continue`: the failing node taints only its own
//!         transitive downstream (marked `skipped_upstream_failed`);
//!         independent ready nodes keep being dispatched in subsequent waves.
//!   * `dry_run=true`: returns the planned DAG, the topological order, and
//!     the projected concurrency waves (groups of node ids the scheduler
//!     would launch together given `max_parallel_nodes`) without
//!     dispatching anything and without writing evidence.
//!   * Evidence sidecar: every node-state transition appends one
//!     `plan_dag_node_dispatch` entry via the typed evidence collector.
//!
//! Out of scope (v2) — explicitly NOT supported:
//!   * Per-node retry policy.
//!   * Rollback / compensation.
//!   * Condition evaluation (`:condition` is captured into evidence but never
//!     executed; non-empty condition currently forces the node to be marked
//!     `skipped_condition`).
//!   * Free-form Lisp interpretation. Unknown sub-forms (anything that isn't a
//!     `(node ...)` at top level) are recorded into `node_hint_summary.unsupported_forms`
//!     so callers can see what was ignored.
//!   * Unsupported per-node fields (anything outside the allowlist above) are
//!     captured into `node_hint_summary.unsupported_fields[node_id]` so the
//!     audit trail never silently drops author intent.
//!   * Per-node retry / per-attempt bookkeeping. v2 dispatches every node
//!     exactly once; the `attempt` slot on the `PlanNodeStateChanged` event
//!     and on the evidence entry is hard-coded to `1` so the wire shape is
//!     ready for a retry-aware future scheduler without forcing readers to
//!     handle absence as a special case.
//!
//! Live `ExecutionEvent` bus integration (wave-14 / Task 02): every node
//! transition (`ready -> running`, `running -> succeeded|failed`,
//! `pending -> skipped`) now publishes a `PlanNodeStateChanged` event on
//! the execution bus and stamps the resulting live `Seq` (or the
//! deterministic
//! `plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>` id when publish
//! fails) into the evidence entry's `execution_events` array via
//! `EventRef::new(...)`. Bus publish failure is observability-only — it
//! never aborts the dispatch, it only records a warning string in
//! `outcome.bus_publish_warnings` so the response surfaces the degraded
//! observability path.

use anyhow::Result;
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};
#[cfg(test)]
pub(super) use std::collections::HashMap;

use crate::state::AppState;
use missiond_core::types::Plan;

use super::agent_execution::scopes_overlap_pure;
#[cfg(test)]
use super::plan::tool_result_payload;

mod claim_lease;
use claim_lease::{
    build_planned_claims, parse_claim_lease_secs, parse_claimer_name, parse_enforce_claims,
};
#[cfg(test)]
#[allow(unused_imports)]
use claim_lease::{
    derive_node_claim_scopes, derive_plan_dag_claim_id, ClaimAcquire, ClaimRegistry, PlanDagClaim,
};

mod acceptance;
#[cfg(test)]
#[allow(unused_imports)]
use acceptance::{
    apply_acceptance_fan_in, derive_acceptance_pause_id, evaluate_node_acceptance, AcceptanceStatus,
};

mod rollback;
use rollback::RollbackPolicy;
#[cfg(test)]
#[allow(unused_imports)]
use rollback::{run_cascade_rollback, run_rollback};

mod projection;
use projection::{build_node_hint_summary, build_nodes_summary, build_retry_plan};

mod outcome;
#[cfg(test)]
#[allow(unused_imports)]
use outcome::NodeLifecycle;
use outcome::{ExecutionOutcome, NodeResult, NodeState};

mod scheduler;
#[cfg(test)]
#[allow(unused_imports)]
use scheduler::propagate_taint;
use scheduler::{compute_concurrency_plan, parse_max_parallel_nodes};

mod dispatch;
use dispatch::{dispatch_node, DispatchOutcome, TaskContractDispatchCtx};
#[cfg(test)]
use dispatch::{node_to_workstation_hints, workstation_outcome_to_dispatch_pair};

mod mode;
pub(super) use mode::{detect_scheduler_mode, refuse_llm_inference_in_dag_mode};

mod finalization;
pub(super) use finalization::parse_finalize_plan;
use finalization::{build_finalization_block, maybe_run_distill_trigger, validate_finalize_args};

mod lifecycle;
use lifecycle::PLAN_NODE_DEFAULT_ATTEMPT;
#[cfg(test)]
#[allow(unused_imports)]
use lifecycle::{
    emit_evidence_acceptance, emit_evidence_claim_conflict, emit_evidence_claim_released,
    emit_evidence_claimed, emit_evidence_rollback, emit_evidence_skipped, emit_paused_review_gate,
    plan_node_should_retry,
};
use lifecycle::{
    emit_evidence_dag_finalized, emit_evidence_finished, emit_evidence_running,
    publish_plan_node_state_change, EvidenceCtx,
};

mod resume;
pub(super) use resume::action_execute_resume;
#[cfg(test)]
pub(super) use resume::validate_resume_request;
pub(crate) use resume::{handle_review_resolved_plan_node_event, PlanNodeResumeListenerOutcome};

mod parser;
#[cfg(test)]
#[allow(unused_imports)]
use parser::FAILURE_POLICY_FAIL_FAST;
use parser::{build_validated_dag, ReviewGateKind};
pub(super) use parser::{DagNode, ParsedDag};

mod runtime;
use runtime::execute_with_concurrency;

/// Public entrypoint invoked from `plan::action_execute_internal` when
/// `scheduler_mode="dag_v1"` is set on the call.
pub(super) async fn action_execute_dag_v1(
    state: &AppState,
    args: &Value,
    plan: &Plan,
) -> Result<ToolResult> {
    // Plan must be re-fetched by caller for status checks; we just need the
    // sexp_text and the id here.
    let dry_run = args
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let max_parallel_nodes = parse_max_parallel_nodes(args);

    // wave-17 / task 05 — validate the finalize knobs up-front so a typo
    // (`distill_mode="sonet"`) or an invalid combo (`distill_on_success=true`
    // without `finalize_plan=true`) fails fast rather than after the DAG
    // executes. Validation runs in dry-run mode too: an authoring mistake
    // should surface during the preview pass, not at the next live run.
    if let Some(err) = validate_finalize_args(args) {
        return Ok(err);
    }

    // wave-19 / task 06 — task-contract emit knob validated up-front so
    // a typo (`task_contract_mode="emi"`) fails fast before the DAG
    // executes. Default `Off` is byte-compatible with pre-wave19.
    let task_contract_ctx = match TaskContractDispatchCtx::from_args(args) {
        Ok(c) => c,
        Err(err) => return Ok(err),
    };

    let (parsed, order) = match build_validated_dag(&plan.sexp_text) {
        Ok(v) => v,
        Err(e) => return Ok(e.into_tool_result()),
    };

    let nodes_summary = build_nodes_summary(&parsed.nodes, &order);
    let node_hint_summary = build_node_hint_summary(&parsed);
    let concurrency_plan = compute_concurrency_plan(&parsed.nodes, &order, max_parallel_nodes);
    let retry_plan = build_retry_plan(&parsed.nodes, &order);

    // wave-17 / task 02 — claim / lease knobs surface on every response
    // (live and dry-run) so callers can tell which discipline mode the
    // run used. `planned_claims` is the per-node claim metadata
    // projection — empty registry, no overlap detection across nodes
    // — used by dry-run to preview every node's claim shape without
    // dispatching.
    let claim_lease_secs = parse_claim_lease_secs(args);
    let claimer_name = parse_claimer_name(args);
    let enforce_claims = parse_enforce_claims(args);
    let planned_claims = build_planned_claims(
        &parsed.nodes,
        &order,
        plan.id,
        &claimer_name,
        claim_lease_secs,
        enforce_claims,
    );

    if dry_run {
        let mut payload = json!({
            "status": "dry_run",
            "execute_mode": "internal",
            "scheduler_mode": "dag_v1",
            "runner_status": "dry_run_no_dispatch",
            "plan_id": plan.id,
            "board_task_id": plan.board_task_id,
            "node_count": parsed.nodes.len(),
            "max_parallel_nodes": max_parallel_nodes,
            "nodes": nodes_summary,
            "topological_order": order,
            "concurrency_plan": concurrency_plan,
            "node_hint_summary": node_hint_summary,
            // wave-16 / task 05 — projected retry budget per node so
            // dry-run callers can preview the attempt ceiling without
            // dispatching. Empty array when no node opted into a retry
            // policy (preserves the v2 baseline byte-shape for callers
            // that did not declare retry).
            "retry_plan": retry_plan,
            // wave-17 / task 02 — projected claim metadata per node so
            // dry-run callers can preview every node's claim shape
            // without dispatching. Always populated (every node carries
            // at least the synthetic plan/<id>/node/<id> fallback).
            "planned_claims": planned_claims,
            "claim_lease_secs": claim_lease_secs,
            "claimer_name": claimer_name,
            "enforce_claims": enforce_claims,
        });
        // wave-19 / task 06 — surface the resolved emission mode in
        // dry-run responses too so callers can preview the contract
        // policy without dispatching. Quiet when mode=Off so the
        // pre-wave19 byte-shape is preserved.
        if task_contract_ctx.mode.is_enabled() {
            payload["task_contract_mode"] = json!(task_contract_ctx.mode.as_str());
        }
        // wave-20 / task 04 — surface the dispatch-contract mode on
        // dry-run too so callers can preview the SSOT routing decision
        // without dispatching. Quiet when mode is the default
        // `rendered` so the pre-wave20 byte-shape is preserved for
        // legacy callers.
        if task_contract_ctx.dispatch_contract_mode.is_machine() {
            payload["dispatch_contract_mode"] =
                json!(task_contract_ctx.dispatch_contract_mode.as_str());
        }
        return Ok(ToolResult::json_pretty(&payload));
    }

    let outcome = execute_with_concurrency(
        state,
        args,
        plan,
        &parsed,
        &order,
        max_parallel_nodes,
        task_contract_ctx.clone(),
    )
    .await?;
    let aggregate_status = outcome.aggregate_status();
    let evidence_path = outcome.evidence_path.clone();
    let evidence_error = outcome.evidence_error.clone();
    let bus_publish_warnings = outcome.bus_publish_warnings.clone();
    let plan_status_update = match outcome.target_plan_status() {
        Some(target) => match state.store.plan_update_status(plan.id, target).await {
            Ok(_) => Ok(target.as_str().to_string()),
            Err(e) => {
                tracing::warn!(plan_id = %plan.id, error = %e, "DAG scheduler: plan status update failed");
                Err(e.to_string())
            }
        },
        None => Ok(plan.status.as_str().to_string()),
    };

    // wave-16 / task 04 — paused-node response surfaces. We compute these
    // unconditionally so callers see a stable shape: empty arrays when no
    // node carried a review gate, populated arrays when at least one
    // node paused. Keeping the keys present (even when empty) lets
    // downstream consumers `?.length` instead of branching on key
    // existence.
    let paused_nodes = outcome.paused_nodes_json();
    let paused_node_ids = outcome.paused_node_ids();
    let review_question_ids = outcome.review_question_ids();

    let mut payload = json!({
        "status": aggregate_status,
        "aggregate_status": aggregate_status,
        "execute_mode": "internal",
        "scheduler_mode": "dag_v1",
        "runner_status": outcome.runner_status(),
        "plan_id": plan.id,
        "board_task_id": plan.board_task_id,
        "node_count": parsed.nodes.len(),
        "max_parallel_nodes": max_parallel_nodes,
        "node_results": outcome.node_results_json(),
        // `nodes` retained as the v1-compatible alias for `node_results` so
        // any caller that already pivots on the older field keeps working.
        "nodes": outcome.node_results_json(),
        "skipped_nodes": outcome.skipped_nodes_json(),
        // wave-16 / task 04 — paused-node surfaces (always present so the
        // shape is stable; empty when no review-gate paused this run).
        "paused_nodes": paused_nodes,
        "paused_node_ids": paused_node_ids,
        "review_question_ids": review_question_ids,
        "topological_order": order,
        "concurrency_plan": concurrency_plan,
        "node_hint_summary": node_hint_summary,
        // wave-16 / task 05 — declared retry budget per node, included
        // on every (non-dry-run) response too so the row that records
        // the policy survives alongside the actual attempt counts.
        "retry_plan": retry_plan,
        // wave-17 / task 02 — claim / lease knobs echoed onto every
        // response so callers can tell which discipline mode the run
        // used. `planned_claims` is the dry-run-style projection so
        // observers can diff "what we would have claimed" against the
        // per-evidence `claim_id` rows the live run actually wrote.
        "planned_claims": planned_claims,
        "claim_lease_secs": claim_lease_secs,
        "claimer_name": claimer_name,
        "enforce_claims": enforce_claims,
        "evidence_path": evidence_path,
    });
    let (plan_status_after, plan_status_update_error) = match &plan_status_update {
        Ok(s) => {
            payload["plan_status"] = json!(s);
            (Some(s.clone()), None)
        }
        Err(e) => {
            payload["status_update_error"] = json!(e);
            (None, Some(e.clone()))
        }
    };
    if let Some(err) = evidence_error {
        payload["evidence_error"] = json!(err);
    }
    if !bus_publish_warnings.is_empty() {
        payload["bus_publish_warnings"] = json!(bus_publish_warnings);
    }

    // wave-17 / task 05 — finalize + distill trigger v0. Conservative: only
    // fires when the caller explicitly opts in. Without `finalize_plan=true`
    // we exit here with the wave-17 / task 04 byte-shape preserved.
    if parse_finalize_plan(args) {
        let distill_block = maybe_run_distill_trigger(
            state,
            args,
            plan,
            aggregate_status,
            plan_status_after.as_deref(),
        )
        .await;

        // Surface the finalization block on the response so callers can grep
        // one place for the rule + status mapping.
        let finalization = build_finalization_block(
            aggregate_status,
            plan_status_after.as_deref(),
            plan_status_update_error.as_deref(),
            distill_block.clone(),
        );
        payload["finalization"] = finalization.clone();

        // Audit trail: one evidence row recording the final aggregate status
        // + plan-status mapping. Quiet (no panic) when the sidecar write
        // fails — the surface already carries `evidence_error` for that.
        emit_evidence_dag_finalized(
            state,
            plan,
            args,
            aggregate_status,
            plan_status_after.as_deref(),
            plan_status_update_error.as_deref(),
            distill_block.as_ref(),
            &mut payload,
        )
        .await;
    }
    Ok(ToolResult::json_pretty(&payload))
}

// The live wave-loop runtime lives in plan_dag/runtime.rs so this file stays
// the action facade plus dry-run/finalization projection.

// ═════════════════════════════════════════════════════════════════════════
// wave-17 / task 01 — paused-node resume hook
//
// Wave-16 / task 04 paused PLAN DAG nodes that opted into
// `:review-gate "question-event"`, emitting a deterministic review
// question id of the form
// `review:plan:<plan_id>:v<version>:plan-node:<sha256(node_id)[..16]>`.
// Wave-17 / task 01 closes the loop by accepting an explicit resume
// input on `mission_plan(action=execute)` AND wiring the
// `QuestionEvent::Resolved` listener (wave-16 / task 02) so an approved
// resolution for a plan-node id re-dispatches exactly the paused node.
//
// This is NOT general auto-approval. Only ids whose envelope round-trips
// to a paused-eligible node (`:review-gate "question-event"` set in the
// plan) are routed through this helper. Non-plan-node review ids keep
// their existing wave-16 / task 02 behaviour.
//
// Behaviour matrix:
//   * `approved`       → re-dispatch the paused node (fresh attempt 1,
//                         since `paused` is non-terminal — not a failed
//                         attempt). Lifecycle event: paused -> running ->
//                         {succeeded|failed}. Plan status stays untouched
//                         because the resume only revives one node — the
//                         caller is expected to drive downstream nodes
//                         via a follow-up execute call.
//   * `rejected`       → no dispatch. Node stays paused (the
//                         failure-policy semantics already pin downstream
//                         pending). Evidence records the rejection
//                         decision for the audit trail.
//   * `needs_changes`  → no dispatch. Node stays paused. Evidence
//                         records the request and the response surfaces
//                         a `next_step` recommendation so the caller
//                         knows to recompile / re-pause the gate.
// ═════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests;
