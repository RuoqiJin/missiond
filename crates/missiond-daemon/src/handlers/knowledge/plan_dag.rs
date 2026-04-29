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
use std::collections::HashMap;

use crate::state::AppState;
use missiond_core::types::{Plan, PlanStatus};

use super::agent_execution::scopes_overlap_pure;
use super::plan::tool_result_payload;

mod claim_lease;
use claim_lease::{
    build_planned_claims, derive_node_claim_scopes, derive_plan_dag_claim_id,
    parse_claim_lease_secs, parse_claimer_name, parse_enforce_claims, ClaimAcquire, ClaimRegistry,
    PlanDagClaim,
};

mod acceptance;
use acceptance::{
    apply_acceptance_fan_in, derive_acceptance_pause_id, evaluate_node_acceptance,
    AcceptanceEvaluation, AcceptanceStatus,
};

mod rollback;
use rollback::RollbackPolicy;
use rollback::{run_cascade_rollback, run_rollback, RollbackEvaluation};

mod projection;
use projection::{build_node_hint_summary, build_nodes_summary, build_retry_plan};

mod scheduler;
use scheduler::{
    build_node_inner_args, compute_concurrency_plan, parse_max_parallel_nodes, propagate_taint,
};

mod mode;
pub(super) use mode::{detect_scheduler_mode, refuse_llm_inference_in_dag_mode};

mod finalization;
pub(super) use finalization::parse_finalize_plan;
use finalization::{
    build_distill_block, build_finalization_block, parse_distill_mode_arg,
    parse_distill_on_success, validate_finalize_args, FINALIZE_DISTILL_MODE_DRY_RUN,
};

mod lifecycle;
use lifecycle::PLAN_NODE_DEFAULT_ATTEMPT;
use lifecycle::{
    emit_evidence_acceptance, emit_evidence_claim_conflict, emit_evidence_claim_released,
    emit_evidence_claimed, emit_evidence_dag_finalized, emit_evidence_finished,
    emit_evidence_rollback, emit_evidence_running, emit_evidence_skipped, emit_paused_review_gate,
    plan_node_should_retry, publish_plan_node_state_change, EvidenceCtx,
};

mod resume;
pub(super) use resume::action_execute_resume;
#[cfg(test)]
pub(super) use resume::validate_resume_request;
pub(crate) use resume::{handle_review_resolved_plan_node_event, PlanNodeResumeListenerOutcome};

mod parser;
use parser::{build_validated_dag, ReviewGateKind, FAILURE_POLICY_FAIL_FAST};
pub(super) use parser::{DagNode, ParsedDag};

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

/// wave-17 / task 05 — drive the optional distill trigger. Returns the
/// `distill` block (or `None` when no trigger was requested). Pure async
/// orchestration: validation already ran in `validate_finalize_args` so
/// here we only branch on the runtime aggregate.
///
/// Decision matrix:
///
///   * `distill_on_success=false`              → return `None`
///   * `aggregate != dag_succeeded`            → block with `triggered=false`
///                                               and a recorded skip reason
///   * `plan_status_after != "succeeded"`      → block with `triggered=false`
///                                               (defensive: the workflow
///                                               distill handler also gates
///                                               on plan.status==Succeeded;
///                                               if the FSM update failed we
///                                               do NOT call distill because
///                                               the gate would refuse anyway)
///   * otherwise                               → call workflow distill,
///                                               surface its result + a
///                                               warning when it errored
async fn maybe_run_distill_trigger(
    state: &AppState,
    args: &Value,
    plan: &Plan,
    aggregate_status: &str,
    plan_status_after: Option<&str>,
) -> Option<Value> {
    if !parse_distill_on_success(args) {
        return None;
    }
    let distill_mode = match parse_distill_mode_arg(args) {
        Ok(m) => m,
        Err(_) => {
            // Unreachable: validate_finalize_args already returned the
            // structured error before we got here. Defensive return so a
            // future refactor cannot silently bypass the validator.
            return Some(build_distill_block(
                false,
                "distill_mode_invalid_unreachable",
                FINALIZE_DISTILL_MODE_DRY_RUN,
                None,
                false,
            ));
        }
    };
    if aggregate_status != "dag_succeeded" {
        return Some(build_distill_block(
            false,
            "aggregate_not_succeeded",
            distill_mode,
            None,
            false,
        ));
    }
    if plan_status_after != Some("succeeded") {
        return Some(build_distill_block(
            false,
            "plan_status_not_succeeded_after_finalize",
            distill_mode,
            None,
            false,
        ));
    }
    // Build the distill args object. We forward the project-resolution
    // signals (`project` / `cwd` / `target_project`) verbatim so the
    // distill handler's evidence-sidecar reader resolves the same root the
    // DAG run wrote into. `persist=false` by default — the wave-17 / task
    // 05 trigger is an automatic preview pass, not a stamp-the-registry
    // call. Callers that want persistence still issue an explicit
    // `mission_workflow(action=distill, persist=true)` themselves.
    let mut distill_args = serde_json::Map::new();
    distill_args.insert("action".to_string(), json!("distill"));
    distill_args.insert("plan_id".to_string(), json!(plan.id.to_string()));
    distill_args.insert("distill_mode".to_string(), json!(distill_mode));
    if let Some(p) = args.get("project").and_then(|v| v.as_str()) {
        distill_args.insert("project".to_string(), json!(p));
    }
    if let Some(c) = args.get("cwd").and_then(|v| v.as_str()) {
        distill_args.insert("cwd".to_string(), json!(c));
    }
    if let Some(tp) = args.get("target_project").and_then(|v| v.as_str()) {
        distill_args.insert("target_project".to_string(), json!(tp));
    }
    let distill_call_args = Value::Object(distill_args);
    let distill_result =
        super::workflow::handle(state, "mission_workflow", distill_call_args).await;
    match distill_result {
        Ok(tr) => {
            let inner_payload = tool_result_payload(&tr);
            let inner_is_error = tr.is_error.unwrap_or(false);
            let reason = if inner_is_error {
                "distill_invoked_returned_error"
            } else {
                "distill_invoked_ok"
            };
            Some(build_distill_block(
                true,
                reason,
                distill_mode,
                Some(inner_payload),
                inner_is_error,
            ))
        }
        Err(e) => {
            // Unexpected handler-level error (bubbled `Result::Err`). Surface
            // it as a warning + non-fatal: the plan final state is preserved
            // because we already updated it to Succeeded above.
            tracing::warn!(
                plan_id = %plan.id,
                error = %e,
                "DAG finalize: distill trigger handler returned error"
            );
            Some(build_distill_block(
                true,
                "distill_invoked_handler_error",
                distill_mode,
                Some(json!({"error": e.to_string()})),
                true,
            ))
        }
    }
}

/// Terminal node state recorded in `NodeResult`. Mirrors the v1 enum so the
/// per-node JSON shape (`state` discriminant + `failed_dep` extra) stays
/// byte-identical for downstream readers; v2 added `SkippedFailFastAbort`
/// to distinguish "we never dispatched you because an unrelated upstream
/// failed under fail-fast" from "your direct dependency failed", and
/// wave-16 / task 04 added `Paused` for the per-node `:review-gate
/// "question-event"` state. `Paused` is the first non-terminal state that
/// surfaces in the per-node JSON — the resume listener (wave-16 / task 02
/// territory) is expected to revive the node in a follow-up dispatch.
#[derive(Debug, Clone)]
enum NodeState {
    Succeeded,
    Failed {
        reason: String,
    },
    SkippedUpstreamFailed {
        failed_dep: String,
    },
    SkippedCondition,
    /// `failure-policy=fail-fast` aborted the scheduler before this node was
    /// ever ready. Distinct from `SkippedUpstreamFailed` because the failing
    /// upstream is not necessarily a transitive dependency — under fail-fast
    /// every still-pending node is force-skipped once the abort flag flips.
    SkippedFailFastAbort {
        aborter: String,
    },
    /// wave-16 / task 04 — node carried `:review-gate "question-event"`,
    /// the scheduler emitted (or attempted to emit) `QuestionEvent::Created`
    /// with [`question_id`] and STOPPED at this node instead of dispatching
    /// the target tool. `bus_publish_warning` carries the warning string
    /// when the publish call errored — the node still pauses (a failed
    /// gate is a real gate; downstream cannot advance) but the response
    /// surfaces the degraded observability path so callers can retry.
    Paused {
        question_id: String,
        bus_publish_warning: Option<String>,
    },
}

/// Per-node lifecycle phase. Drives the wave-scheduler bookkeeping; mapped to
/// `state` discriminants in the response only after the node terminates. The
/// intermediate phases (`Pending`, `Ready`, `Claimed`, `Running`) never leak
/// into the response — they live entirely in the scheduler's internal state
/// map.
///
/// `Ready` is the brief moment between the scheduler computing the ready set
/// and dispatching it to the JoinSet. The current loop transitions
/// `Pending -> Claimed -> Running` (wave-17 / task 02 added the explicit
/// `Claimed` step between ready-set selection and JoinSet hand-off so the
/// claim/lease registry can stamp metadata before the inner handler runs).
/// The variant `Ready` is kept in the enum to satisfy the wave-13/02 spec
/// lifecycle list and to leave room for a future scheduler that materialises
/// a persistent ready queue (`#[allow(dead_code)]` is intentional for now).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeLifecycle {
    Pending,
    #[allow(dead_code)]
    Ready,
    /// wave-17 / task 02 — node has had its claim registered (or
    /// recorded best-effort under `enforce_claims=false`) but the
    /// inner handler has not yet been invoked. Mostly invisible from
    /// the outside: the dispatch path moves through `Claimed` for one
    /// wave-loop cycle before flipping to `Running`. Surfaces on the
    /// `pending -> claimed` evidence row + bus event so observers can
    /// pivot on the new transition without reconstructing it from
    /// `pending -> running` reasoning.
    Claimed,
    Running,
    Succeeded,
    Failed,
    Skipped,
    /// wave-16 / task 04 — node opted into a `:review-gate "question-event"`
    /// gate and the scheduler emitted `QuestionEvent::Created` instead of
    /// dispatching the target tool. Treated as a non-terminal "stop"
    /// state by the wave loop: the scheduler does NOT retry it within
    /// the same call (auto-resume is wave-16 / task 02 territory), and
    /// the node's downstream stays pending until a follow-up resume.
    Paused,
}

/// wave-16 / task 05 — `Default` is implemented to make wave-13/14/15
/// test fixtures resilient against the retry-bookkeeping fields added
/// in this wave. Production construction sites (`execute_with_concurrency`
/// + the `NodeResult::skipped` helper) always populate every field
/// explicitly; the default impl only catches test fixtures using
/// `..Default::default()` so adding a new bookkeeping field doesn't
/// require touching every old test.
#[derive(Debug, Clone)]
struct NodeResult {
    id: String,
    target: String,
    state: NodeState,
    dispatch_strategy: String,
    inner_payload: Value,
    /// wave-16 / task 05 — number of dispatch attempts the scheduler
    /// actually consumed for this node. Always ≥ 1 for executed nodes
    /// (we count the first dispatch as attempt 1); equals
    /// `effective_max_attempts` only when every attempt failed. Skipped
    /// / paused nodes report `0` because the scheduler never invoked
    /// the inner handler. Surfaces on `node_results[].retry.attempts`.
    attempts_made: u32,
    /// wave-16 / task 05 — total attempts the scheduler was authorised
    /// to make for this node (= `effective_max_attempts` at dispatch
    /// time). Echoed alongside `attempts_made` so consumers can spot
    /// "exhausted retries" without re-deriving the policy.
    max_attempts: u32,
    /// wave-16 / task 05 — true iff the node failed without retrying
    /// because the failure was classified non-retryable (currently:
    /// safe-descriptor refusals from the workstation-dispatch
    /// substrate). Surfaces on the per-node response so consumers can
    /// distinguish "we exhausted attempts" from "we refused to retry".
    retry_skipped_non_retryable: bool,
    /// wave-17 / task 04 — conservative rollback decision result.
    /// `None` means the rollback evaluator never ran (skipped node,
    /// node terminated successfully, or the failed node carried no
    /// rollback hints — see `RollbackEvaluation::is_inactive`).
    /// `Some(e)` carries the full evaluation block — the scheduler
    /// stamps it onto `node_results[].rollback` so callers see what
    /// happened (descriptor recorded / dispatch attempted / refused
    /// / failed) without re-deriving from evidence.
    rollback: Option<RollbackEvaluation>,
    /// wave-17 / task 03 — deterministic acceptance phase result.
    /// `None` means the acceptance evaluator never ran for this node
    /// (skipped node, dispatch failed before acceptance, no hints
    /// declared). `Some(e)` carries the full evaluation block — the
    /// scheduler stamps it onto `node_results[].acceptance` so callers
    /// see what the evaluator decided + why.
    acceptance: Option<AcceptanceEvaluation>,
}

impl NodeResult {
    /// wave-16 / task 05 — minimal builder used by skip / pause sites
    /// that never invoked the inner handler. Keeps construction local
    /// to the scheduler so the per-call-site retry bookkeeping
    /// (`attempts_made = 0`, `max_attempts = 1`) stays consistent.
    fn skipped(id: String, target: String, state: NodeState, dispatch_strategy: String) -> Self {
        Self {
            id,
            target,
            state,
            dispatch_strategy,
            inner_payload: Value::Null,
            attempts_made: 0,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        }
    }
}

impl Default for NodeResult {
    fn default() -> Self {
        Self {
            id: String::new(),
            target: String::new(),
            state: NodeState::Succeeded,
            dispatch_strategy: String::new(),
            inner_payload: Value::Null,
            attempts_made: 0,
            max_attempts: 1,
            retry_skipped_non_retryable: false,
            rollback: None,
            acceptance: None,
        }
    }
}

#[derive(Debug, Default)]
struct ExecutionOutcome {
    results: Vec<NodeResult>,
    /// Set true iff a node with `failure-policy=fail-fast` failed and we
    /// stopped scheduling additional ready nodes.
    aborted_fail_fast: bool,
    evidence_path: Option<String>,
    evidence_error: Option<String>,
    /// Per-transition `PlanNodeStateChanged` bus publish warnings collected
    /// during this run. Bus publish is intentionally non-blocking for the
    /// main dispatch path (durable evidence already lives in the sidecar);
    /// the warnings are surfaced on the response so callers can detect a
    /// degraded observability path without scraping daemon logs. Empty
    /// when every transition published cleanly.
    bus_publish_warnings: Vec<String>,
}

impl ExecutionOutcome {
    fn node_results_json(&self) -> Value {
        let mut out: Vec<Value> = Vec::with_capacity(self.results.len());
        for r in &self.results {
            let (state_str, extra) = match &r.state {
                NodeState::Succeeded => ("succeeded", None),
                NodeState::Failed { reason } => ("failed", Some(("reason", reason.clone()))),
                NodeState::SkippedUpstreamFailed { failed_dep } => (
                    "skipped_upstream_failed",
                    Some(("failed_dep", failed_dep.clone())),
                ),
                NodeState::SkippedCondition => ("skipped_condition", None),
                NodeState::SkippedFailFastAbort { aborter } => (
                    "skipped_fail_fast_abort",
                    Some(("aborter", aborter.clone())),
                ),
                NodeState::Paused { question_id, .. } => {
                    ("paused", Some(("review_question_id", question_id.clone())))
                }
            };
            let mut e = json!({
                "id": r.id,
                "target": r.target,
                "state": state_str,
                "dispatch_strategy": r.dispatch_strategy,
                "inner_result": r.inner_payload,
            });
            if let Some((k, v)) = extra {
                e[k] = json!(v);
            }
            // wave-16 / task 04 — surface the optional bus warning on
            // paused nodes so callers can grep one place for "the gate
            // emit was degraded for this node".
            if let NodeState::Paused {
                bus_publish_warning: Some(w),
                ..
            } = &r.state
            {
                e["review_question_warning"] = json!(w);
            }
            // wave-16 / task 05 — retry observability surface. We
            // emit the `retry` block whenever the node is one whose
            // policy authorised more than one attempt OR the dispatch
            // actually consumed more than one attempt OR the failure
            // was classified non-retryable. Nodes with the v2-baseline
            // single-attempt contract that succeeded on attempt 1 stay
            // quiet so the wave-15 byte-shape is preserved.
            if r.max_attempts > 1 || r.attempts_made > 1 || r.retry_skipped_non_retryable {
                let mut retry = json!({
                    "attempts": r.attempts_made,
                    "max_attempts": r.max_attempts,
                });
                if r.retry_skipped_non_retryable {
                    retry["non_retryable"] = json!(true);
                }
                e["retry"] = retry;
            }
            // wave-17 / task 03 — acceptance evaluation surface. Quiet
            // (omitted) when the evaluator never ran OR ran but found no
            // hints declared so the wave-16 byte-shape is preserved for
            // callers that did not opt into the acceptance contract.
            if let Some(acc) = r.acceptance.as_ref() {
                if !acc.is_inactive() {
                    e["acceptance"] = acc.to_json();
                }
            }
            // wave-17 / task 04 — rollback evaluation surface. Quiet
            // (omitted) when the rollback evaluator never ran OR
            // produced an inactive evaluation (no hints declared) so
            // the wave-17 / task 03 byte-shape is preserved for
            // callers that did not opt into the rollback contract.
            if let Some(rb) = r.rollback.as_ref() {
                if !rb.is_inactive() {
                    e["rollback"] = rb.to_json();
                }
            }
            out.push(e);
        }
        Value::Array(out)
    }

    /// wave-16 / task 04 — project the subset of results that landed in
    /// the `paused` non-terminal state so callers (and the wave-16 / task
    /// 02 resume listener) can address them without re-walking the full
    /// results array. Order matches the topological-order placement of
    /// each result.
    fn paused_nodes_json(&self) -> Value {
        let mut out: Vec<Value> = Vec::new();
        for r in &self.results {
            if let NodeState::Paused {
                question_id,
                bus_publish_warning,
            } = &r.state
            {
                let mut e = json!({
                    "id": r.id,
                    "target": r.target,
                    "state": "paused",
                    "review_question_id": question_id,
                });
                if let Some(w) = bus_publish_warning {
                    e["review_question_warning"] = json!(w);
                }
                out.push(e);
            }
        }
        Value::Array(out)
    }

    /// wave-16 / task 04 — paused node ids in topological-order placement.
    /// Surfaced as a separate flat array on the response so callers that
    /// just want "which nodes need a follow-up resume" don't have to walk
    /// the richer `paused_nodes` block.
    fn paused_node_ids(&self) -> Vec<String> {
        self.results
            .iter()
            .filter_map(|r| match &r.state {
                NodeState::Paused { .. } => Some(r.id.clone()),
                _ => None,
            })
            .collect()
    }

    /// wave-16 / task 04 — review-question ids for every paused node, in
    /// the same order as `paused_node_ids`. The two arrays are the
    /// ergonomic split of the richer `paused_nodes` block.
    fn review_question_ids(&self) -> Vec<String> {
        self.results
            .iter()
            .filter_map(|r| match &r.state {
                NodeState::Paused { question_id, .. } => Some(question_id.clone()),
                _ => None,
            })
            .collect()
    }

    /// True iff at least one node landed in the `paused` state for this
    /// run — used by aggregate_status / runner_status to surface
    /// `dag_paused` so callers can route on a single status discriminant.
    fn any_paused(&self) -> bool {
        self.results
            .iter()
            .any(|r| matches!(r.state, NodeState::Paused { .. }))
    }

    /// Project the subset of results that ended in a `skipped_*` discriminant
    /// so callers can grep without re-walking the full results array. Order
    /// matches the topological-order placement of each result.
    fn skipped_nodes_json(&self) -> Value {
        let mut out: Vec<Value> = Vec::new();
        for r in &self.results {
            let (state_str, extra) = match &r.state {
                NodeState::SkippedUpstreamFailed { failed_dep } => (
                    "skipped_upstream_failed",
                    Some(("failed_dep", failed_dep.clone())),
                ),
                NodeState::SkippedCondition => ("skipped_condition", None),
                NodeState::SkippedFailFastAbort { aborter } => (
                    "skipped_fail_fast_abort",
                    Some(("aborter", aborter.clone())),
                ),
                _ => continue,
            };
            let mut e = json!({
                "id": r.id,
                "target": r.target,
                "state": state_str,
            });
            if let Some((k, v)) = extra {
                e[k] = json!(v);
            }
            out.push(e);
        }
        Value::Array(out)
    }

    fn any_failed(&self) -> bool {
        self.results
            .iter()
            .any(|r| matches!(r.state, NodeState::Failed { .. }))
    }

    fn all_succeeded(&self) -> bool {
        !self.results.is_empty()
            && self
                .results
                .iter()
                .all(|r| matches!(r.state, NodeState::Succeeded))
    }

    fn aggregate_status(&self) -> &'static str {
        if self.aborted_fail_fast {
            return "dag_failed";
        }
        if self.all_succeeded() {
            return "dag_succeeded";
        }
        if self.any_failed() {
            return "dag_partial";
        }
        // wave-16 / task 04 — paused-only runs surface a dedicated
        // aggregate so callers can route on a single status. We pick
        // `dag_paused` (rather than `dag_partial`) only when no failure
        // is present; a mixed paused+failed run still reads as partial
        // because failure is the louder signal.
        if self.any_paused() {
            return "dag_paused";
        }
        // Some nodes may have been skipped without any outright failure
        // (e.g. condition gating). Treat that as partial too.
        "dag_partial"
    }

    fn runner_status(&self) -> &'static str {
        if self.aborted_fail_fast {
            "fail_fast_aborted"
        } else if self.all_succeeded() {
            "all_nodes_dispatched"
        } else if !self.any_failed() && self.any_paused() {
            "review_gate_paused"
        } else {
            "partial_dispatched"
        }
    }

    fn target_plan_status(&self) -> Option<PlanStatus> {
        if self.all_succeeded() {
            Some(PlanStatus::Succeeded)
        } else if self.aborted_fail_fast || self.any_failed() {
            Some(PlanStatus::Failed)
        } else {
            // Paused runs leave the plan in its current Executing /
            // Approved state so a follow-up resume can advance the DAG.
            // Returning None here means `action_execute_dag_v1` won't
            // call `plan_update_status` for this run.
            None
        }
    }
}

/// Outcome of dispatching a single node — produced inside the spawned task
/// so the scheduler's main loop can decide success/failure + record evidence
/// without holding any per-node lock during the dispatch itself.
struct DispatchOutcome {
    node_id: String,
    target: String,
    dispatch_strategy: String,
    inner_payload: Value,
    /// `Ok(())` when the inner handler returned a non-error tool result;
    /// `Err(reason)` when either inner-args building or the inner handler
    /// surfaced an error. The reason string is what we surface in the
    /// per-node response under `reason` and in the `running -> failed`
    /// evidence entry's failure annotation.
    classification: std::result::Result<(), String>,
    /// wave-16 / task 05 — true when the failure originated from a
    /// workstation-dispatch safe-descriptor refusal (unsupported
    /// target / project root unresolved / missing objective). These
    /// failures are deterministic policy checks — re-running them
    /// without changing the inputs would refuse identically. The
    /// scheduler honours this flag by skipping the retry loop and
    /// surfacing `retry_skipped_non_retryable=true` on the response.
    non_retryable: bool,
}

/// Project a parsed DAG node into the workstation-dispatch hint contract.
/// Mirrors `ParsedPlanHints::to_workstation_hints` so the v0 DAG path and
/// the v0 single-node runner build identical briefs for the same hints.
fn node_to_workstation_hints(
    node: &DagNode,
) -> super::workstation_dispatch::WorkstationDispatchHints {
    super::workstation_dispatch::WorkstationDispatchHints {
        objective: node.objective.clone(),
        scope: node.scope.clone(),
        owned_files: super::plan::split_lisp_string_list(node.owned_files_raw.as_deref()),
        forbidden_files: super::plan::split_lisp_string_list(node.forbidden_files_raw.as_deref()),
        acceptance_commands: super::plan::split_lisp_string_list(
            node.acceptance_commands_raw.as_deref(),
        ),
        commit_policy: node.commit_policy.clone(),
        target_project: node.target_project.clone(),
        requested_cwd: node.requested_cwd.clone(),
        dispatch_strategy: node.dispatch_strategy.clone(),
    }
}

/// Convert a workstation-dispatch outcome into the
/// `(inner_payload, classification, non_retryable)` triple `dispatch_node`
/// uses to populate `DispatchOutcome`. Keeps the per-node DAG contract
/// intact: the response JSON carries the workstation-dispatch envelope
/// under `inner_result`, and the outcome's status drives the
/// success/failure classification.
///
/// wave-16 / task 05 — `non_retryable` is true ONLY for
/// `SafeDescriptor` outcomes, because those refusals are deterministic
/// policy checks (unsupported target / project root unresolved /
/// missing objective). Re-running the same inputs would refuse
/// identically; the scheduler respects this and bypasses the retry
/// loop. `InnerError` (the substrate handler returned an error
/// payload) IS retryable — that path may have transient causes.
fn workstation_outcome_to_dispatch_pair(
    node: &DagNode,
    dispatch_strategy: &str,
    outcome: super::workstation_dispatch::WorkstationDispatchOutcome,
    decision: &super::workstation_dispatch::DispatchDecision,
) -> (Value, std::result::Result<(), String>, bool) {
    let status = outcome.status();
    let envelope =
        super::workstation_dispatch::outcome_to_response_fields(&outcome, dispatch_strategy);
    let mut non_retryable = false;
    let classification: std::result::Result<(), String> = match &outcome {
        super::workstation_dispatch::WorkstationDispatchOutcome::Dispatched { .. } => Ok(()),
        super::workstation_dispatch::WorkstationDispatchOutcome::DryRun { .. } => Ok(()),
        super::workstation_dispatch::WorkstationDispatchOutcome::InnerError {
            inner_payload,
            ..
        } => Err(inner_payload
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("workstation_dispatch inner handler returned error")
            .to_string()),
        super::workstation_dispatch::WorkstationDispatchOutcome::SafeDescriptor {
            reason, ..
        } => {
            // Safe-descriptor refusals are deterministic policy checks
            // — flag the failure as non-retryable so the wave loop
            // skips the retry pass entirely.
            non_retryable = true;
            Err(format!(
                "workstation_dispatch refused to dispatch node `{}`: {}",
                node.id,
                reason.detail()
            ))
        }
    };
    let mut payload = json!({
        "workstation_dispatch_status": status,
        "node_id": node.id,
        // wave-16 / task 03 — surface routing provenance per node so the
        // DAG response makes the explicit/inferred split visible without
        // re-deriving from the plan body.
        "workstation_dispatch_source": decision.source.as_str(),
    });
    if let Some(reason) = decision.reason.as_deref() {
        if let Some(map) = payload.as_object_mut() {
            map.insert(
                "workstation_dispatch_inference_reason".to_string(),
                json!(reason),
            );
        }
    }
    if let Some(map) = envelope.as_object() {
        if let Some(payload_map) = payload.as_object_mut() {
            for (k, v) in map {
                payload_map.insert(k.clone(), v.clone());
            }
        }
    }
    (payload, classification, non_retryable)
}

/// wave-19 / task 06 — per-DAG-run task-contract emission context. The
/// scheduler resolves the mode + project-resolution signals once at the
/// top of `action_execute_dag_v1` and clones one of these into every
/// `dispatch_node` task so the per-node emit does not have to re-parse
/// the caller args (and stays aligned with the single-node runner's
/// project-root resolution path). All fields are owned (no borrowed
/// references) so the struct survives `tokio::JoinSet::spawn`'s
/// `'static` requirement.
///
/// wave-20 / task 04 — extended with `dispatch_contract_mode` so DAG
/// nodes can opt the workstation substrate into machine-driven dispatch
/// (read the emitted task.lisp directly). The mode is parsed once at
/// the scheduler entry point and cloned into every per-node task —
/// per-node mode overrides would defeat the cross-node SSOT contract.
#[derive(Debug, Clone)]
pub(super) struct TaskContractDispatchCtx {
    pub mode: super::plan::TaskContractEmitMode,
    pub dispatch_contract_mode: super::plan::DispatchContractMode,
    pub project_arg: Option<String>,
    pub cwd_arg: Option<String>,
    pub target_project_arg: Option<String>,
}

impl TaskContractDispatchCtx {
    pub(super) fn off() -> Self {
        Self {
            mode: super::plan::TaskContractEmitMode::Off,
            dispatch_contract_mode: super::plan::DispatchContractMode::Rendered,
            project_arg: None,
            cwd_arg: None,
            target_project_arg: None,
        }
    }

    /// Build the ctx from caller args. Returns
    /// `Err(structured)` for malformed `task_contract_mode` /
    /// `dispatch_contract_mode` values so the scheduler fails fast
    /// before spawning any node task.
    pub(super) fn from_args(args: &Value) -> std::result::Result<Self, ToolResult> {
        let mode = super::plan::parse_task_contract_emit_mode(args)?;
        let dispatch_contract_mode = super::plan::parse_dispatch_contract_mode(args)?;
        Ok(Self {
            mode,
            dispatch_contract_mode,
            project_arg: args
                .get("project")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            cwd_arg: args
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            target_project_arg: args
                .get("target_project")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        })
    }
}

async fn dispatch_node(
    state: AppState,
    plan: Plan,
    node: DagNode,
    task_contract_ctx: TaskContractDispatchCtx,
) -> Result<DispatchOutcome> {
    let inner_args_built = build_node_inner_args(&node, &plan);
    let dispatch_strategy = inner_args_built.dispatch_strategy.clone();

    // wave-15 / task 05 + wave-16 / task 03 — workstation-dispatch routing
    // for DAG nodes. Wave-15 honoured an explicit per-node
    // `:workstation-dispatch true` only. Wave-16 layers conservative
    // auto-inference on top: when a node's :target is already
    // `mission_task_delegate`, the dispatch strategy resolves to a known
    // workstation strategy, the objective is non-empty, and at least one
    // scoping signal is declared, the scheduler routes through the
    // workstation substrate without requiring the explicit hint. There is
    // no per-node `workstation_dispatch=false` knob because DAG nodes are
    // declared in PLAN.lisp; the only off-switch is to mark the node with
    // a non-task-delegate target or omit the dispatch strategy.
    let merged = node_to_workstation_hints(&node);
    let inference_ctx = super::workstation_dispatch::InferenceContext {
        target: node.target.as_str(),
        dispatch_strategy: dispatch_strategy.as_str(),
        objective: merged.objective.as_deref(),
        owned_files_present: !merged.owned_files.is_empty(),
        scope_present: merged
            .scope
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        target_project_present: merged
            .target_project
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        requested_cwd_present: merged
            .requested_cwd
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
    };
    let dispatch_decision = super::workstation_dispatch::evaluate_dispatch_decision(
        &serde_json::Value::Null,
        node.workstation_dispatch_opt_in(),
        &inference_ctx,
    );

    if dispatch_decision.is_enabled() {
        // wave-19 / task 06 — emit the per-node task-contract sidecar
        // BEFORE handing the node to the workstation substrate. The
        // contract is the SSOT, so a failed write REFUSES dispatch
        // for this node; non-retryable so the wave loop does not loop
        // through the inner handler hoping the disk recovers. Default
        // mode (`Off`) returns an empty record and the per-node
        // payload omits the wave-19 fields entirely.
        let inputs =
            super::plan::task_contract_inputs_from_hints(&merged, &node.target, &dispatch_strategy);
        let emission = super::plan::emit_task_contract(
            &state,
            plan.id,
            &plan.board_task_id,
            &node.id,
            task_contract_ctx.mode,
            &inputs,
            task_contract_ctx.project_arg.as_deref(),
            task_contract_ctx.cwd_arg.as_deref(),
            task_contract_ctx.target_project_arg.as_deref(),
        )
        .await;

        if emission.is_failure() {
            // Refuse the per-node dispatch — the missing contract
            // would leave downstream consumers with no Lisp SSOT.
            // Mark non-retryable: an IO failure is unlikely to fix
            // itself by re-running the inner handler.
            let mut payload = json!({
                "node_id": node.id,
                "target": node.target,
                "workstation_dispatch_status": "skipped_task_contract_emit_failed",
                "workstation_dispatch_source": dispatch_decision.source.as_str(),
            });
            if let Some(reason) = dispatch_decision.reason.as_deref() {
                payload["workstation_dispatch_inference_reason"] = json!(reason);
            }
            super::plan::merge_task_contract_block(&mut payload, &emission);
            let reason = emission
                .error
                .clone()
                .unwrap_or_else(|| "task_contract_emit_failed".to_string());
            return Ok(DispatchOutcome {
                node_id: node.id.clone(),
                target: node.target.clone(),
                dispatch_strategy,
                inner_payload: payload,
                classification: Err(format!(
                    "task_contract emit failed for node `{}`: {}",
                    node.id, reason
                )),
                non_retryable: true,
            });
        }

        if task_contract_ctx.mode.is_dry_run() {
            // EmitDryRun — never call the substrate. We mark the
            // node succeeded (the contract write IS the work in
            // dry-run mode); downstream nodes proceed normally so
            // the caller can preview the full DAG with one pass.
            let mut payload = json!({
                "node_id": node.id,
                "target": node.target,
                "workstation_dispatch_status": "task_contract_emit_dry_run",
                "workstation_dispatch_source": dispatch_decision.source.as_str(),
            });
            if let Some(reason) = dispatch_decision.reason.as_deref() {
                payload["workstation_dispatch_inference_reason"] = json!(reason);
            }
            super::plan::merge_task_contract_block(&mut payload, &emission);
            return Ok(DispatchOutcome {
                node_id: node.id.clone(),
                target: node.target.clone(),
                dispatch_strategy,
                inner_payload: payload,
                classification: Ok(()),
                non_retryable: false,
            });
        }

        // wave-20 / task 04 — when the per-DAG-run dispatch_contract_mode
        // is `machine` AND emission produced a contract path for THIS
        // node, hand the on-disk Lisp to the wave-19 / task 07
        // consumer. The consumer overlays the contract onto the
        // hints (contract is the SSOT) and refuses to fall back to
        // the legacy natural-language brief on a malformed contract
        // (surfacing as `SafeDescriptor` →
        // status="skipped_malformed_task_contract", non-retryable
        // because re-loading a syntactically broken file deterministically
        // fails again). Default mode (`rendered`) preserves wave-15..19
        // byte-shape: `task_contract_path = None` and the brief is
        // built from the in-memory hints.
        let task_contract_path_for_machine =
            if task_contract_ctx.dispatch_contract_mode.is_machine() {
                emission.path.clone()
            } else {
                None
            };
        let outcome = super::workstation_dispatch::run_workstation_dispatch_with_contract(
            &state,
            &plan,
            &node.target,
            &dispatch_strategy,
            merged,
            false,
            task_contract_path_for_machine.as_deref(),
        )
        .await;
        let (mut inner_payload, classification, non_retryable) =
            workstation_outcome_to_dispatch_pair(
                &node,
                &dispatch_strategy,
                outcome,
                &dispatch_decision,
            );
        // wave-20 / task 04 — surface the resolved dispatch-contract
        // mode so observers (PR review, CI, audit) can pin which
        // dispatch contract drove the brief at this node. The wire
        // shape adds one new key per node — existing callers that
        // ignore it keep working.
        if let Some(map) = inner_payload.as_object_mut() {
            map.insert(
                "dispatch_contract_mode".to_string(),
                json!(task_contract_ctx.dispatch_contract_mode.as_str()),
            );
        }
        super::plan::merge_task_contract_block(&mut inner_payload, &emission);
        return Ok(DispatchOutcome {
            node_id: node.id.clone(),
            target: node.target.clone(),
            dispatch_strategy,
            inner_payload,
            classification,
            non_retryable,
        });
    }

    let inner_args = match inner_args_built.inner_args {
        Ok(v) => v,
        Err(err_payload) => {
            let reason = err_payload
                .as_object()
                .and_then(|m| m.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or("inner args build failed")
                .to_string();
            // wave-16 / task 05 — inner-args build failures are deterministic
            // (e.g. missing required `flow_id` for `mission_flow_run`).
            // Re-running with identical inputs would fail identically;
            // mark non-retryable so the wave loop skips the retry pass.
            return Ok(DispatchOutcome {
                node_id: node.id.clone(),
                target: node.target.clone(),
                dispatch_strategy,
                inner_payload: err_payload,
                classification: Err(reason),
                non_retryable: true,
            });
        }
    };

    let inner_result = match node.target.as_str() {
        "mission_execution" => {
            super::agent_execution::handle(&state, "mission_execution", inner_args.clone()).await?
        }
        "mission_task_delegate" => {
            super::super::compute::task_delegate::handle(
                &state,
                "mission_task_delegate",
                inner_args.clone(),
            )
            .await?
        }
        "mission_flow_run" => {
            super::super::compute::flow_run::handle(&state, "mission_flow_run", inner_args.clone())
                .await?
        }
        _ => unreachable!("DAG validation already enforced target whitelist"),
    };

    let inner_payload = tool_result_payload(&inner_result);
    let inner_is_error = inner_result.is_error.unwrap_or(false);
    let classification = if inner_is_error {
        Err(inner_payload
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("inner handler returned error")
            .to_string())
    } else {
        Ok(())
    };
    Ok(DispatchOutcome {
        node_id: node.id.clone(),
        target: node.target.clone(),
        dispatch_strategy,
        inner_payload,
        classification,
        // Standard inner-handler failures may have transient causes —
        // leave them retryable. The wave loop honours the per-node
        // retry policy and stops once attempts are exhausted.
        non_retryable: false,
    })
}

/// Wave-based scheduler. Drains up to `max_parallel_nodes` ready nodes per
/// wave through a `tokio::task::JoinSet`, awaits the wave, records the
/// transitions in the order results land, then recomputes ready set and
/// repeats. `max_parallel_nodes=1` produces a wave size of 1 each iteration
/// — equivalent to the v1 sequential contract.
async fn execute_with_concurrency(
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
