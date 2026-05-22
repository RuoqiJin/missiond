//! mission_plan — manager surface for the plan table.
//!
//! Lisp authority:
//!   - intent-memory.lisp :: module directive-layer :: plumbing plan-execution
//!                                                    + file-first-artifacts plan-lisp
//!   - intent-flow.lisp :: F-intent-alignment-plan-execution-loop ::
//!                          s4 plan-authoring + s5 plan-review-gate
//!   - intent-intent-layer.lisp :: section unified-entry-pipeline ::
//!                                    role plan-compiler + role plan-runner
//!   - intent-tools.lisp :: implemented-surface mission_plan
//!
//! Action coverage:
//!   compile          — plan-compiler actor v0:
//!                        compiler_mode="dry_run" (default) → no LLM, preview shape
//!                        compiler_mode="sonnet"            → SonnetGateway interactive call,
//!                                                            validates lisp shape +
//!                                                            board_task anchor; persist=true
//!                                                            inserts as awaiting_approval
//!                                                            with compiler_model + compiled_from
//!                      directive approval gate: status ∈ {approved, compiled} unless
//!                      allow_unapproved=true.
//!   list             — full (plan_list_recent or plan_list_by_task)
//!   get              — full (plan_get)
//!   by_task          — full (plan_list_by_task)
//!   approve          — full (plan_update_status → approved, stamps approved_at)
//!   mark             — full (plan_update_status to any FSM target)
//!   supersede        — full (plan_supersede)
//!   execute          — plan-runner v0:
//!                        execute_mode=bridge (default) returns enriched
//!                          next_call descriptor (runner_status="bridge_only");
//!                        execute_mode=internal dispatches the chosen target
//!                          handler (mission_execution / mission_task_delegate /
//!                          mission_flow_run) inside MissionD, appends a
//!                          plan_runner_dispatch evidence entry on success, and
//!                          transitions plan status to executing.
//!                      dispatch_strategy is recorded in response + evidence
//!                      (mission_execution companion-log persistence is future).
//!   record_evidence  — full: persists evidence sidecar at
//!                      <project>/.missiond/v3/runtime/plans/<plan_id>.evidence.json
//!                      while retaining .missiond/v2/plans as a legacy
//!                      compatibility fallback.

use anyhow::{anyhow, Result};
use chrono::{SecondsFormat, Utc};
use missiond_mcp::tools::{error_codes, ToolContent, ToolError, ToolResult};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::str::FromStr;

use crate::context::v3_blueprint_runtime::RouterRuntimeConfig;
use crate::handlers::knowledge::review_gate::{
    apply_compile_review_gates, build_llm_auto_approve_proposal_system_prompt,
    build_llm_auto_approve_proposal_user_prompt, enforce_apply_gate_preflight,
    enforce_proposal_invariants, evaluate_llm_approve_apply_gate, evaluate_review_automation,
    llm_auto_approve_proposal_mode_was_explicit, maybe_emit_review_question_resolved,
    parse_compile_review_gate, parse_llm_approve_apply_gate_input, parse_llm_auto_approve_proposal,
    parse_llm_auto_approve_proposal_mode, parse_plan_node_resume_input,
    parse_resolution_review_question_id, parse_review_automation_policy, parse_review_gate_policy,
    parse_review_question_id_struct, parse_review_resolution_input, resolution_wire_string,
    review_automation_policy_was_explicit, review_gate_policy_was_explicit,
    stamp_llm_approve_apply_gate_payload, stamp_llm_auto_approve_proposal_payload,
    stamp_needs_changes_next_step, stamp_proposal_hash_payload, stamp_resolution_payload,
    stamp_review_automation_payload, validate_review_resolution_envelope, AutomationStatus,
    LlmApproveApplyGateInput, LlmAutoApproveProposalBundle, LlmAutoApproveProposalMode,
    LlmAutoApproveProposalStatus, ParsedReviewQuestionId, ResolutionOutcome,
    ReviewAutomationContext, ReviewAutomationPolicy, ReviewDecision, ReviewResolutionInput,
};
use crate::minimax_client::ChatMessage;
use crate::slot_orchestrator::project_root::{resolve_target_project_root, ResolutionError};
use crate::state::AppState;
use missiond_core::types::{Plan, PlanStatus};

const DEFAULT_LIST_LIMIT: i64 = 50;
const MAX_LIST_LIMIT: i64 = 500;
const COMPANION_DIR: &str = ".missiond/v3/runtime/plans";
const LEGACY_COMPANION_DIR: &str = ".missiond/v2/plans";

const COMPILER_MODE_DRY_RUN: &str = "dry_run";
const COMPILER_MODE_SONNET: &str = "sonnet";
/// Token cap for the planner call. Plans are sexp DAGs — comfortably under 4K
/// tokens — but we leave headroom for nested phases / acceptance fields.
const SONNET_MAX_TOKENS: u32 = 4096;
/// Allowed top-level heads for the compiled plan sexp. Mirrors the planner
/// system prompt and `intent-memory.lisp :: plan-lisp` shape (PLAN.lisp).
const ALLOWED_PLAN_HEADS: &[&str] = &["plan", "plan-draft", "PLAN"];
/// Workstation-dispatch strategies surfaced in
/// `intent-tools.lisp :: workstation-dispatch-record`. Anything outside this
/// set is normalised to "unknown" so the evidence trail stays clean.
const VALID_DISPATCH_STRATEGIES: &[&str] = &[
    "resident-lisp",
    "fresh-code-alignment",
    "agent-team",
    "mixed",
    "prompt-fallback",
    "unknown",
];

pub(super) fn load_sonnet_compiler_model() -> Result<String> {
    RouterRuntimeConfig::load_for_current_dir()
        .map(|config| config.queued_sonnet_model)
        .map_err(|err| anyhow!("V3_BLUEPRINT_CONFIG_ERROR: {}", err))
}

mod execute_hints;
#[cfg(test)]
pub(super) use execute_hints::parse_plan_hints;
pub(crate) use execute_hints::{
    canonicalize_strategy, normalize_target, parse_plan_hints_for_plan,
    plan_contract_json_from_sexp, resolve_dispatch_strategy, scan_keyword_pairs,
    split_lisp_string_list, ParsedPlanHints, ResolvedExec, AGENT_TEAM_OBJECTIVE_HINT,
};

mod task_contract;
#[cfg(test)]
pub(super) use task_contract::{
    build_task_contract_lisp, is_task_contract_eligible, render_command_for, task_contract_path,
    write_task_contract_under_root, TaskContractInputs,
};
pub(super) use task_contract::{
    emit_task_contract, lisp_escape_string, parse_dispatch_contract_mode,
    parse_task_contract_emit_mode, render_lisp_string_list, task_contract_inputs_from_hints,
    task_contract_inputs_from_hints_with_trace, DispatchContractMode, TaskContractEmissionRecord,
    TaskContractEmitMode,
};

mod distill_chain;
use distill_chain::{apply_distill_chain, validate_distill_chain_args};
#[cfg(test)]
use distill_chain::{
    attach_distill_chain_to_payload, build_distill_chain_block, chain_eligibility_skip_reason,
    derive_fallback_chain_id, distill_chain_requested, json_shape_label, parse_distill_chain_id,
    parse_distill_chain_mode, parse_distill_chain_name, CHAIN_STATUS_RECORDED,
    CHAIN_STATUS_RECORDED_DISTILL_WARNING, CHAIN_STATUS_RECORDED_WITH_DISTILL,
    CHAIN_STATUS_SKIPPED_NO_FINALIZATION, CHAIN_STATUS_SKIPPED_PLAN_NOT_SUCCEEDED,
};

mod dispatch_response;
pub(super) use dispatch_response::merge_task_contract_block;
use dispatch_response::{
    attach_session_trace_response_fields, build_internal_dispatch_success_response,
    build_task_contract_dry_run_response, build_task_contract_failure_response,
    build_workstation_dispatch_response, validate_session_trace_path_arg,
};

mod evidence_sidecar;
use evidence_sidecar::action_record_evidence;
pub(super) use evidence_sidecar::append_plan_evidence_entry;

mod compile_authoring;
use compile_authoring::{action_compile, collect_string_list};
#[cfg(test)]
use compile_authoring::{
    build_planner_system_prompt, build_planner_user_prompt, derive_dry_run_plan_objective,
    extract_plan_file_args, parens_balanced, render_dry_run_plan_sexp, strip_fenced_code_block,
    top_level_head, validate_compiled_plan_sexp, DryRunPlanSexpInput,
};

mod approval_review;
#[cfg(test)]
use approval_review::PLAN_REVIEW_ACTIONS;
use approval_review::{action_approve, action_mark, action_supersede};
pub(crate) use approval_review::{handle_review_resolved_event, PlanSubscriberOutcome};

mod field_inference;
#[cfg(test)]
use field_inference::*;
use field_inference::{
    apply_safe_augmentation, attach_apply_gate_block, attach_persisted_apply_block,
    compute_apply_gate, compute_plan_field_inference, execute_persisted_apply,
    parse_workstation_inference_mode, read_recent_evidence_entries,
    refuse_workstation_inference_in_dag_mode, request_llm_proposals, validate_apply_gate_args,
    PersistedApplyOutcome, PersistedApplyStatus, PlanInferenceInput, WorkstationInferenceMode,
    WORKSTATION_INFER_MODE_SONNET_SUGGEST,
};
pub(super) use field_inference::{parse_infer_plan_fields_mode, InferPlanFieldsMode};

mod execution_runtime;
use execution_runtime::action_execute;
#[cfg(test)]
use execution_runtime::*;

mod internal_dispatch;
#[cfg(test)]
use internal_dispatch::derive_objective_from_plan;
pub(super) use internal_dispatch::{build_internal_dispatch_args, tool_result_payload};
use internal_dispatch::{truncate_chars, DERIVED_OBJECTIVE_MAX};

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a.to_string(),
        None => return Ok(ToolResult::structured_error(
            ToolError::new(error_codes::MISSING_PARAM, "mission_plan requires `action`")
                .with_suggestion(
                "actions: compile|list|get|by_task|approve|mark|supersede|execute|record_evidence",
            ),
        )),
    };

    match action.as_str() {
        "compile" => action_compile(state, &args).await,
        "list" => action_list(state, &args).await,
        "get" => action_get(state, &args).await,
        "by_task" => action_by_task(state, &args).await,
        "approve" => action_approve(state, &args).await,
        "mark" => action_mark(state, &args).await,
        "supersede" => action_supersede(state, &args).await,
        "execute" => action_execute(state, &args).await,
        "record_evidence" => action_record_evidence(state, &args).await,
        other => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("unknown mission_plan action `{}`", other),
            )
            .with_suggestion(
                "valid: compile|list|get|by_task|approve|mark|supersede|execute|record_evidence",
            ),
        )),
    }
}

// compile/authoring implementation moved to plan/compile_authoring.rs.

// ───────────────────────────────────────────────────────────────────────
// list / get / by_task — store-backed reads
// ───────────────────────────────────────────────────────────────────────

async fn action_list(state: &AppState, args: &Value) -> Result<ToolResult> {
    let status = args
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| PlanStatus::from_str(s).map_err(|e| anyhow!(e)))
        .transpose()?;
    let limit = args
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT);
    let rows = state
        .store
        .plan_list_recent(status, limit)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&json!({
        "plans": rows,
        "count": rows.len(),
        "filter": { "status": status.map(|s| s.as_str().to_string()) },
        "limit": limit,
    })))
}

async fn action_get(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "plan_id")?;
    match state
        .store
        .plan_get(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => Ok(ToolResult::json_pretty(&p)),
        None => Ok(ToolResult::structured_error(
            ToolError::new(error_codes::NOT_FOUND, format!("plan `{}` not found", id))
                .with_suggestion("use action=list or action=by_task"),
        )),
    }
}

async fn action_by_task(state: &AppState, args: &Value) -> Result<ToolResult> {
    let task_id = require_str(args, "board_task_id")?;
    let rows = state
        .store
        .plan_list_by_task(task_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&json!({
        "board_task_id": task_id,
        "plans": rows,
        "versions": rows.len(),
    })))
}

// approval/review implementation moved to plan/approval_review.rs.

// field inference / apply-gate implementation moved to plan/field_inference.rs.

// execute runtime implementation moved to plan/execution_runtime.rs.
// internal dispatch argument projection moved to plan/internal_dispatch.rs.

// ── wave-23 / task 05 — session-trace + dispatch response egress helpers ──
//
// Moved to plan/dispatch_response.rs. The mission_plan handler keeps the
// public facade stable and imports the response builders above.

// ───────────────────────────────────────────────────────────────────────
// record_evidence sidecar egress
//
// Moved to plan/evidence_sidecar.rs. The mission_plan handler keeps the
// action facade stable and imports the sidecar writer above.

// ───────────────────────────────────────────────────────────────────────
// wave-18 / task 05 — cross-plan distill chain v0
//
// Moved to plan/distill_chain.rs. The mission_plan handler keeps the public
// facade stable and re-exports the parser/orchestrator helpers above.

// ───────────────────────────────────────────────────────────────────────
// helpers
// ───────────────────────────────────────────────────────────────────────

pub(super) fn parse_id_arg(args: &Value, key: &str) -> Result<uuid::Uuid> {
    let raw = args
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("`{}` required", key))?;
    uuid::Uuid::parse_str(raw).map_err(|e| anyhow!("`{}` is not a UUID: {}", key, e))
}

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("`{}` required", key))
}

fn iso_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn plan_evidence_sidecar_path(project_root: &std::path::Path, plan_id: uuid::Uuid) -> PathBuf {
    project_root
        .join(COMPANION_DIR)
        .join(format!("{}.evidence.json", plan_id))
}

fn legacy_plan_evidence_sidecar_path(
    project_root: &std::path::Path,
    plan_id: uuid::Uuid,
) -> PathBuf {
    project_root
        .join(LEGACY_COMPANION_DIR)
        .join(format!("{}.evidence.json", plan_id))
}

fn existing_plan_evidence_sidecar_path(
    project_root: &std::path::Path,
    plan_id: uuid::Uuid,
) -> PathBuf {
    let canonical = plan_evidence_sidecar_path(project_root, plan_id);
    if canonical.exists() {
        return canonical;
    }
    let legacy = legacy_plan_evidence_sidecar_path(project_root, plan_id);
    if legacy.exists() {
        return legacy;
    }
    canonical
}

fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

/// Resolve the canonical project root for plan-side file writes (evidence
/// sidecar, file-first PLAN.lisp, etc.).
///
/// Strict contract — mirrors `intent-worker.lisp ::
/// invariant project-root-spawn-cwd` and the slot orchestrator
/// (`slot_orchestrator::project_root::resolve_target_project_root`):
///   1. explicit `project` registry id → canonical root.
///   2. explicit `cwd` → must be **absolute**; falls into the canonical
///      resolver's `CwdLongestPrefix` source. Relative cwd is rejected
///      outright; we never silently fall back to the daemon's process cwd
///      (CLAUDE.md `feedback_fail_fast_no_fallback`).
///   3. fallback `target_project` registry id → canonical root.
///   4. no signal → structured error.
///
/// This replaces the prior process-cwd fallback. File writes must always
/// land under a registered project root; otherwise we surface a loud error
/// so the caller can supply the correct project signal instead of quietly
/// persisting evidence under whatever directory the daemon happened to be
/// running from when it started.
async fn resolve_project_root(
    registry: &missiond_core::types::SharedProjectRegistry,
    project_id: Option<&str>,
    cwd: Option<&str>,
    target_project: Option<&str>,
) -> Result<PathBuf> {
    let cwd_path: Option<PathBuf> = match cwd {
        Some(raw) if !raw.is_empty() => {
            let path = PathBuf::from(raw);
            if !path.is_absolute() {
                return Err(anyhow!(
                    "cwd `{}` is not absolute; plan resolver refuses to fall back to process cwd \
                     (intent-worker.lisp :: project-root-spawn-cwd contract)",
                    raw
                ));
            }
            Some(path)
        }
        _ => None,
    };

    match resolve_target_project_root(project_id, cwd_path.as_deref(), target_project, registry)
        .await
    {
        Ok(r) => Ok(r.project_root),
        Err(ResolutionError::NoSignal) => Err(anyhow!(
            "project root unresolved: pass `project=<registered id>` (or `target_project=<id>`, \
             or absolute `cwd=<abs path>`); plan resolver does not fall back to process cwd"
        )),
        Err(e) => Err(anyhow!(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// wave-24 / task 04 — router-policy dry-run surface.
//
// Adds an OPTIONAL, INFORMATIONAL recommendation block to
// `mission_plan(action=execute)` responses. The block mirrors the wave24-03
// Node CLI (`scripts/recommend-task-backend.mjs`) algorithm: parse the
// router-policy v1 Lisp file, evaluate each rule's `:when` predicates against
// the live execute context (kind / dispatch_strategy / owner / status /
// path-glob over `owned_files`), pick the lowest-priority matching rule, and
// emit a structured recommendation. `applied` is hard-coded `false` —
// router output is advisory only and the runtime dispatch path stays
// unchanged. Any policy that fails the cross-wave invariants
// (`:dry-run-only true` AND `:runtime-replacement false`) is reported with
// `status="rejected"` so the operator is loud about the misconfiguration.
//
// Implemented as a pure Rust deterministic helper — no shell-out, no Node
// spawn, no `scripts/` invocation. The Lisp parser is purpose-built for the
// tight schema (small, exhaustive, fail-closed on unknown predicate heads).
//
// Confidence policy: the wave24-03 CLI takes an optional `--trace-index`
// JSON for `high` confidence based on event counts. wave24-04 deliberately
// skipped that input (no trace-index loader in the daemon — keep this surface
// pure and additive). wave25-03 adds OPTIONAL parity: when the caller passes
// `router_policy_trace_index_path` AND `router_policy_mode=dry_run`, the
// daemon reads the file via `std::fs::read_to_string` + `serde_json` and
// mirrors the Node CLI's `scoreConfidence`:
//   * matched + max(by_task[id].events, by_backend[backend].events) >= 5 -> `high`
//   * matched + max(...) in 1..=4 -> `medium`
//   * matched + max(...) == 0 -> `low`
//   * no match (fallback) -> `low` with reason `insufficient_trace_history`
// Failure modes (path missing / I/O error / malformed JSON) NEVER fail
// dispatch — they degrade confidence to the matched/no-match fallback (medium
// for matched, low for no-match) and surface `trace_index_status` +
// `trace_index_warning` for explainability.
//
// Off/default mode is byte-identical with NO file I/O, even if a trace-index
// path is supplied. This is enforced by the Off-path early-return in
// `attach_router_recommendation_block`.
//
// wave26-03 layers an OPTIONAL `router_backend_registry_path` arg on top of
// the wave25-03 trace-index path. When supplied AND mode=dry_run the daemon
// reads the wave26-01 backend readiness registry via `std::fs::read_to_string`
// + a minimal subset of the existing Lisp parser (extracting only `:id`
// `:readiness_status` `:runtime_allowed` `:apply_blockers` per `(backend ...)`
// entry) and surfaces six additive fields on the recommendation block:
//   * backend_registry_path     — echo of input
//   * backend_registry_status   — used | missing | unreadable | malformed | unknown_backend
//   * backend_readiness_status  — current-default | advisory-only | runtime-ready | unavailable | unknown
//   * backend_runtime_allowed   — bool (verbatim from registry)
//   * router_apply_eligible     — bool, ONLY true when ALL 6 of:
//       1. policy valid (status=computed)
//       2. confidence == "high"
//       3. backend present in registry
//       4. runtime_allowed == true
//       5. readiness_status == "runtime-ready"  (current-default is NOT sufficient)
//       6. apply_blockers empty
//   * router_apply_blockers     — Vec<String>; echoes registry's apply_blockers
//                                  for the matched backend, or synthesises
//                                  explicit blockers ("confidence is medium",
//                                  "recommended_backend not in registry",
//                                  "backend readiness_status is current-default;
//                                   runtime-ready required") when the gate fails.
// `applied=false` stays a hard-coded literal even when the registry is
// consulted; dispatch is NEVER altered by registry issues. Off/default mode
// stays byte-identical with NO file I/O even when BOTH `router_backend_registry_path`
// AND `router_policy_trace_index_path` are supplied — the Off-path early-
// return in `attach_router_recommendation_block` predates both reads.
// ---------------------------------------------------------------------------
mod router_policy_dry_run;
// ---------------------------------------------------------------------------
// wave-28 / task 04 — task-runner manifest dry-run surface.
//
// Mirrors the wave28-02 Node CLI (`scripts/plan-task-runner.mjs`) but
// implemented in pure Rust. Surfaces a deterministic projection of a
// wave28-01 task-runner-manifest v1 file on the `mission_plan` execute
// response under the key `task_runner`.
//
// Hard guarantees (cross-wave invariants):
//   * NO worker spawn — purely a manifest reader + projector.
//   * NO Node child_process / spawn / shell out.
//   * NO git mutation, NO network, NO LLM call, NO mission_task_delegate.
//   * `applied` is hard-coded `Value::Bool(false)` — never computed.
//   * Off/default mode is byte-identical to the wave-15..27 baseline AND
//     does ZERO file I/O, even if `task_runner_manifest_path` is supplied.
//   * Manifest read / parse failures are non-fatal: they surface a
//     `manifest_status` enum value + a `task_runner_warning` field, and
//     dispatch ALWAYS continues.
// ---------------------------------------------------------------------------
mod task_runner_dry_run;
#[cfg(test)]
mod tests;
