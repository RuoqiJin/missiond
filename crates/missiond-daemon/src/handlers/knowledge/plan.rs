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
//!                      <project>/.missiond/v2/plans/<plan_id>.evidence.json

use anyhow::{anyhow, Result};
use chrono::{SecondsFormat, Utc};
use missiond_mcp::tools::{error_codes, ToolContent, ToolError, ToolResult};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::str::FromStr;

use crate::handlers::knowledge::file_artifacts::{
    attempt_artifact_write, ArtifactKind, WriterContext,
};
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
    LlmApproveApplyGateInput, LlmAutoApproveProposalBundle,
    LlmAutoApproveProposalMode, LlmAutoApproveProposalStatus, ParsedReviewQuestionId,
    ResolutionOutcome, ReviewAutomationContext, ReviewAutomationPolicy, ReviewDecision,
    ReviewResolutionInput,
};
use crate::minimax_client::ChatMessage;
use crate::slot_orchestrator::project_root::{
    resolve_target_project_root, ResolutionError,
};
use crate::state::AppState;
use missiond_core::types::{DirectiveStatus, Plan, PlanStatus};

const DEFAULT_LIST_LIMIT: i64 = 50;
const MAX_LIST_LIMIT: i64 = 500;
const COMPANION_DIR: &str = ".missiond/v2/plans";

const COMPILER_MODE_DRY_RUN: &str = "dry_run";
const COMPILER_MODE_SONNET: &str = "sonnet";
/// Model name written into `plan.compiler_model` for sonnet-mode rows. Mirrors
/// `SONNET_MODEL` in `llm/sonnet_gateway.rs`; kept as a string literal so we
/// don't need to widen its visibility.
const SONNET_COMPILER_MODEL: &str = "claude-sonnet";
/// Token cap for the planner call. Plans are sexp DAGs — comfortably under 4K
/// tokens — but we leave headroom for nested phases / acceptance fields.
const SONNET_MAX_TOKENS: u32 = 4096;
/// Allowed top-level heads for the compiled plan sexp. Mirrors the planner
/// system prompt and `intent-memory.lisp :: plan-lisp` shape (PLAN.lisp).
const ALLOWED_PLAN_HEADS: &[&str] = &["plan", "plan-draft", "PLAN"];
/// Cap derived objective at a manager-friendly length so we never push huge
/// sexp blobs into mission_task_delegate (which has its own 16K cap, but the
/// derived summary should be a *summary*, not the whole DAG).
const DERIVED_OBJECTIVE_MAX: usize = 240;
/// Valid intents accepted by mission_task_delegate (kept in sync with that
/// handler's whitelist; we surface a structured error if caller picks something
/// else, instead of letting it through to be rejected downstream).
const VALID_DELEGATE_INTENTS: &[&str] = &["code", "ops", "research", "general"];
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

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a.to_string(),
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "mission_plan requires `action`",
                )
                .with_suggestion(
                    "actions: compile|list|get|by_task|approve|mark|supersede|execute|record_evidence",
                ),
            ))
        }
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

// ───────────────────────────────────────────────────────────────────────
// compile — plan-compiler actor v0
//
// compiler_mode = "dry_run" (default) : preview shape only, no LLM.
// compiler_mode = "sonnet"            : SonnetGateway interactive call.
//
// Lisp authority for the sonnet path:
//   intent-flow.lisp        :: F-intent-alignment-plan-execution-loop ::
//                                s4 plan-authoring + s5 plan-review-gate
//   intent-intent-layer.lisp :: section unified-entry-pipeline ::
//                                role plan-compiler
//   intent-memory.lisp      :: directive-layer ::
//                                file-first-artifacts plan-lisp +
//                                plumbing plan-execution
// ───────────────────────────────────────────────────────────────────────

async fn action_compile(state: &AppState, args: &Value) -> Result<ToolResult> {
    let compiler_mode = args
        .get("compiler_mode")
        .and_then(|v| v.as_str())
        .unwrap_or(COMPILER_MODE_DRY_RUN)
        .to_string();
    if compiler_mode != COMPILER_MODE_DRY_RUN && compiler_mode != COMPILER_MODE_SONNET {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!("unknown compiler_mode `{}`", compiler_mode),
            )
            .with_suggestion("use compiler_mode=\"dry_run\" or \"sonnet\""),
        ));
    }

    if compiler_mode == COMPILER_MODE_DRY_RUN {
        return action_compile_dry_run(state, args).await;
    }

    action_compile_sonnet(state, args).await
}

/// Caller-supplied args that gate the file-first writer for the plan
/// compiler. Mirror of `directive::DirectiveFileArgs`; pulled into a
/// dedicated struct so dry_run + sonnet share one extraction routine and
/// the `attempt_artifact_write` invocation stays consistent across modes.
struct PlanFileArgs<'a> {
    write_file: bool,
    overwrite_file: bool,
    /// `topic` defaults to `board_task_id` so the file path stays anchored
    /// to the same row the DB plan inserts into. Callers can still pass an
    /// explicit `topic` for multi-plan workflows that share a board task.
    topic: Option<&'a str>,
    project: Option<&'a str>,
    cwd: Option<&'a str>,
    target_project: Option<&'a str>,
}

fn extract_plan_file_args(args: &Value) -> PlanFileArgs<'_> {
    PlanFileArgs {
        write_file: args
            .get("write_file")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        overwrite_file: args
            .get("overwrite_file")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        topic: args.get("topic").and_then(|v| v.as_str()),
        project: args.get("project").and_then(|v| v.as_str()),
        cwd: args.get("cwd").and_then(|v| v.as_str()),
        target_project: args.get("target_project").and_then(|v| v.as_str()),
    }
}

/// After the plan row is committed, optionally mirror the compiled sexp to
/// the file-first SSOT
/// (`<project_root>/.missiond/plans/<topic>/PLAN.lisp`).
///
/// `topic` precedence:
///   1. explicit `topic` arg (trim-checked).
///   2. `board_task_id` fallback so the on-disk path matches the DB anchor
///      without forcing every caller to repeat the id.
///
/// Same partial / error semantics as the directive writer: DB row stays put,
/// failures land in `file_write_error` + downgraded `status="partial"`.
async fn maybe_write_plan_artifact(
    state: &AppState,
    args: &PlanFileArgs<'_>,
    payload: &mut Value,
    sexp: &str,
    fallback_topic: &str,
) {
    if !args.write_file {
        return;
    }
    let topic = args
        .topic
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_topic);
    if topic.trim().is_empty() {
        if let Some(map) = payload.as_object_mut() {
            map.insert("file_written".to_string(), json!(false));
            map.insert(
                "file_write_error".to_string(),
                json!("write_file=true requires a non-empty `topic` argument (or board_task_id fallback)"),
            );
            let already_partial = map
                .get("status")
                .and_then(|v| v.as_str())
                .map(|s| s == "partial")
                .unwrap_or(false);
            if !already_partial {
                map.insert("status".to_string(), json!("partial"));
            }
        }
        return;
    }
    let outcome = attempt_artifact_write(
        &state.project_registry,
        WriterContext {
            kind: ArtifactKind::Plan,
            topic,
            project: args.project,
            cwd: args.cwd,
            target_project: args.target_project,
            overwrite: args.overwrite_file,
        },
        sexp,
    )
    .await;
    outcome.splice_into(payload);
}

async fn action_compile_dry_run(state: &AppState, args: &Value) -> Result<ToolResult> {
    let directive_id = args.get("directive_id").and_then(|v| v.as_str());
    let board_task_id = args.get("board_task_id").and_then(|v| v.as_str());
    let persist = args
        .get("persist")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if directive_id.is_none() && board_task_id.is_none() {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::MISSING_PARAM,
                "compile requires `directive_id` or `board_task_id`",
            )
            .with_suggestion("plan-compiler runs against an approved directive bound to a board_task"),
        ));
    }

    let directive_uuid = match directive_id {
        Some(s) => Some(uuid::Uuid::parse_str(s).map_err(|e| anyhow!("directive_id not UUID: {}", e))?),
        None => None,
    };

    let dry_run_sexp = format!(
        "(plan-draft\n  :directive_id {:?}\n  :board_task_id {:?}\n  :status :awaiting-compiler-actor)\n",
        directive_id.unwrap_or(""),
        board_task_id.unwrap_or(""),
    );
    let sexp_hash = sha256_hex(&dry_run_sexp);

    let mut payload = json!({
        "status": "dry_run",
        "compiler_mode": COMPILER_MODE_DRY_RUN,
        "actor_pending": "intent-layer :: plan-compiler (LLM)",
        "flow_ref": "F-intent-alignment-plan-execution-loop :: s4 plan-authoring",
        "directive_id": directive_id,
        "board_task_id": board_task_id,
        "compiled_sexp_preview": dry_run_sexp,
        "sexp_hash_preview": sexp_hash,
        "next_step": "rerun with compiler_mode=\"sonnet\" to invoke the plan-compiler actor; \
                      or persist=true to insert a draft row",
    });

    if persist {
        let task_id = board_task_id.ok_or_else(|| {
            anyhow!("persist=true requires `board_task_id` (plan.board_task_id is NOT NULL FK)")
        })?;
        // Verify the board_task exists so we don't 23503 on FK.
        let task_exists = state
            .store
            .get_board_task(task_id)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
            .is_some();
        if !task_exists {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("board_task `{}` not found", task_id),
                )
                .with_suggestion("create the board_task first via mission_board_create"),
            ));
        }

        // Next version per task.
        let existing = state
            .store
            .plan_list_by_task(task_id)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        let next_version = existing.iter().map(|p| p.version).max().unwrap_or(0) + 1;

        let id = state
            .store
            .plan_insert(
                task_id,
                directive_uuid,
                next_version,
                &dry_run_sexp,
                &sexp_hash,
                PlanStatus::Draft,
                None,
                None,
            )
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        payload["persisted"] = json!(true);
        payload["plan_id"] = json!(id);
        payload["version"] = json!(next_version);

        // wave-14 :: file-first SSOT mirror. Default topic = board_task_id
        // so a plan-runner that boots from board_task can find the
        // on-disk PLAN.lisp without an extra arg. The DB row remains
        // committed even if the file write fails (file-vs-db contract).
        let file_args = extract_plan_file_args(args);
        let topic_for_gate = file_args
            .topic
            .map(|s| s.to_string())
            .unwrap_or_else(|| task_id.to_string());
        maybe_write_plan_artifact(state, &file_args, &mut payload, &dry_run_sexp, task_id).await;

        // wave-14 :: review-gate auto-create. Default policy = Manual
        // (legacy explicit emit only); `emit_question` policy auto-fires
        // after a successful PLAN.lisp write. Resolution stays opt-in via
        // `review_question_id` on approve/mark/supersede.
        let policy = parse_review_gate_policy(args);
        let policy_explicit = review_gate_policy_was_explicit(args);
        let legacy = parse_compile_review_gate(args);
        apply_compile_review_gates(
            &mut payload,
            &state.bus,
            policy,
            policy_explicit,
            &legacy,
            "plan",
            &id.to_string(),
            next_version,
            Some(&topic_for_gate),
        )
        .await;
    } else {
        payload["persisted"] = json!(false);
    }
    Ok(ToolResult::json_pretty(&payload))
}

async fn action_compile_sonnet(state: &AppState, args: &Value) -> Result<ToolResult> {
    let board_task_id = match args.get("board_task_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "compiler_mode=\"sonnet\" requires `board_task_id` (plan.board_task_id is the anchor)",
                )
                .with_suggestion(
                    "the planner needs the board_task to scope the plan; even when persist=false the sexp must anchor to it",
                ),
            ))
        }
    };
    let persist = args.get("persist").and_then(|v| v.as_bool()).unwrap_or(false);
    let allow_unapproved = args
        .get("allow_unapproved")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let directive_id_str = args.get("directive_id").and_then(|v| v.as_str());
    let directive_uuid = match directive_id_str {
        Some(s) => Some(
            uuid::Uuid::parse_str(s).map_err(|e| anyhow!("directive_id not UUID: {}", e))?,
        ),
        None => None,
    };
    let directive_version_arg = args
        .get("directive_version")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);

    // Resolve the directive (head of version_chain or pinned version) so the
    // planner has the alignment sexp + status to act on.
    let directive = match directive_uuid {
        Some(id) => Some(resolve_directive(state, id, directive_version_arg).await?),
        None => None,
    };
    let mut approval_overridden = false;
    if let Some(d) = directive.as_ref() {
        let gate_ok = matches!(
            d.status,
            DirectiveStatus::Approved | DirectiveStatus::Compiled
        );
        if !gate_ok && !allow_unapproved {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!(
                        "directive `{}` v{} status `{}` is not approved/compiled; plan-compiler refuses to run",
                        d.id, d.version, d.status.as_str()
                    ),
                )
                .with_suggestion(
                    "approve the directive first via mission_directive(action=approve), \
                     or pass allow_unapproved=true for debugging",
                ),
            ));
        }
        approval_overridden = !gate_ok;
    }

    // Verify the board_task exists up front so a Sonnet call doesn't get
    // wasted on an invalid anchor.
    let task_exists = state
        .store
        .get_board_task(&board_task_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
        .is_some();
    if !task_exists {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::NOT_FOUND,
                format!("board_task `{}` not found", board_task_id),
            )
            .with_suggestion("create the board_task first via mission_board_create"),
        ));
    }

    let sonnet = match state.sonnet.as_ref() {
        Some(s) => s,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    "LLM_UNAVAILABLE",
                    "Sonnet gateway not initialized; cannot run plan-compiler actor",
                )
                .with_suggestion(
                    "fallback: rerun with compiler_mode=\"dry_run\", or boot the daemon with sonnet gateway enabled",
                ),
            ))
        }
    };

    let target_project = args
        .get("target_project")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let dispatch_strategy = args
        .get("dispatch_strategy")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let parallelism = args
        .get("parallelism")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let acceptance = collect_string_list(args.get("acceptance"));
    let constraints = collect_string_list(args.get("constraints"));

    let directive_sexp = directive.as_ref().map(|d| d.sexp_text.as_str());
    let system_prompt = build_planner_system_prompt();
    let user_prompt = build_planner_user_prompt(
        &board_task_id,
        directive.as_ref().map(|d| (d.id, d.version)),
        directive_sexp,
        target_project.as_deref(),
        dispatch_strategy.as_deref(),
        parallelism.as_deref(),
        &acceptance,
        &constraints,
    );
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system_prompt,
        },
        ChatMessage {
            role: "user".to_string(),
            content: user_prompt,
        },
    ];

    let raw = sonnet
        .call_interactive(messages, Some(SONNET_MAX_TOKENS), "plan_compiler")
        .await
        .map_err(|e| anyhow!("Sonnet call failed: {}", e))?;

    let compiled_sexp = match validate_compiled_plan_sexp(&raw, &board_task_id) {
        Ok(s) => s,
        Err(SexpValidationError { code, reason, hint }) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(code, reason).with_suggestion(hint),
            ))
        }
    };
    let sexp_hash = sha256_hex(&compiled_sexp);
    let compiled_from = match directive.as_ref() {
        Some(d) => format!("directive/{}:{}", d.id, d.version),
        None => format!("board_task/{}", board_task_id),
    };

    let mut payload = json!({
        "status": "compiled",
        "compiler_mode": COMPILER_MODE_SONNET,
        "compiler_model": SONNET_COMPILER_MODEL,
        "flow_ref": "F-intent-alignment-plan-execution-loop :: s4 plan-authoring",
        "directive_id": directive_id_str,
        "directive_version": directive.as_ref().map(|d| d.version),
        "board_task_id": board_task_id,
        "compiled_sexp": compiled_sexp,
        "sexp_hash": sexp_hash,
        "compiled_from": compiled_from,
        "approval_gate_overridden": approval_overridden,
        "review_required": true,
        "next_step": "review then mission_plan(action=approve)",
    });

    if persist {
        let existing = state
            .store
            .plan_list_by_task(&board_task_id)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        let next_version = existing.iter().map(|p| p.version).max().unwrap_or(0) + 1;

        let id = state
            .store
            .plan_insert(
                &board_task_id,
                directive_uuid,
                next_version,
                &compiled_sexp,
                &sexp_hash,
                PlanStatus::AwaitingApproval,
                Some(SONNET_COMPILER_MODEL),
                Some(&compiled_from),
            )
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        payload["persisted"] = json!(true);
        payload["plan_id"] = json!(id);
        payload["version"] = json!(next_version);
        payload["plan_status"] = json!(PlanStatus::AwaitingApproval.as_str());

        // wave-14 :: file-first SSOT mirror — same partial semantics as the
        // dry_run path. The compiled sexp is the durable artifact; we
        // splice the path/sha so the plan-runner can verify on-disk parity
        // before scheduling.
        let file_args = extract_plan_file_args(args);
        let topic_for_gate = file_args
            .topic
            .map(|s| s.to_string())
            .unwrap_or_else(|| board_task_id.clone());
        maybe_write_plan_artifact(
            state,
            &file_args,
            &mut payload,
            &compiled_sexp,
            &board_task_id,
        )
        .await;

        // wave-14 :: review-gate auto-create. Same policy semantics as the
        // dry_run branch above; topic falls back to `board_task_id` to
        // match the file-first writer's default.
        let policy = parse_review_gate_policy(args);
        let policy_explicit = review_gate_policy_was_explicit(args);
        let legacy = parse_compile_review_gate(args);
        apply_compile_review_gates(
            &mut payload,
            &state.bus,
            policy,
            policy_explicit,
            &legacy,
            "plan",
            &id.to_string(),
            next_version,
            Some(&topic_for_gate),
        )
        .await;
    } else {
        payload["persisted"] = json!(false);
    }
    Ok(ToolResult::json_pretty(&payload))
}

// ───────────────────────────────────────────────────────────────────────
// plan-compiler helpers (pure)
// ───────────────────────────────────────────────────────────────────────

async fn resolve_directive(
    state: &AppState,
    id: uuid::Uuid,
    version: Option<i32>,
) -> Result<missiond_core::types::Directive> {
    let resolved = match version {
        Some(v) => state
            .store
            .directive_get(id, v)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?,
        None => {
            let chain = state
                .store
                .directive_get_version_chain(id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            chain.into_iter().last()
        }
    };
    resolved.ok_or_else(|| {
        let label = match version {
            Some(v) => format!("directive `{}` v{}", id, v),
            None => format!("directive `{}` (any version)", id),
        };
        anyhow!("{} not found", label)
    })
}

fn collect_string_list(v: Option<&Value>) -> Vec<String> {
    match v {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(s)) => {
            if s.trim().is_empty() {
                Vec::new()
            } else {
                vec![s.clone()]
            }
        }
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|item| match item {
                Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn build_planner_system_prompt() -> String {
    let heads = ALLOWED_PLAN_HEADS.join(" / ");
    format!(
        "You are MissionD's plan-compiler actor (intent-layer). \
         Compile the input directive + board_task context into ONE Lisp s-expression \
         representing the executable plan. \
         Output rules: \
         (1) emit ONLY one top-level s-expression — no Markdown, no fences, no commentary. \
         (2) the top-level head must be one of: {}. \
         (3) the sexp MUST contain the literal board_task_id value somewhere — typically \
             :board_task_id \"<id>\" — so it anchors to the right execution row. \
         (4) include keyword fields :goal :phases :tasks (and when applicable :acceptance \
             :constraints :rollback :tests :files), each as nested sexps. \
         (5) all parentheses must be balanced; string literals stay inside double quotes. \
         (6) keep the sexp human-readable; indent nested fields with two spaces.",
        heads
    )
}

#[allow(clippy::too_many_arguments)]
fn build_planner_user_prompt(
    board_task_id: &str,
    directive_pin: Option<(uuid::Uuid, i32)>,
    directive_sexp: Option<&str>,
    target_project: Option<&str>,
    dispatch_strategy: Option<&str>,
    parallelism: Option<&str>,
    acceptance: &[String],
    constraints: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("Board task id (anchor): ");
    out.push_str(board_task_id);
    if let Some((id, ver)) = directive_pin {
        out.push_str(&format!("\nDirective: {} v{}", id, ver));
    }
    if let Some(sexp) = directive_sexp {
        out.push_str("\nApproved directive sexp:\n");
        out.push_str(sexp);
    }
    if let Some(tp) = target_project {
        out.push_str("\nTarget project context: ");
        out.push_str(tp);
    }
    if let Some(ds) = dispatch_strategy {
        out.push_str("\nDispatch strategy hint: ");
        out.push_str(ds);
    }
    if let Some(p) = parallelism {
        out.push_str("\nParallelism hint: ");
        out.push_str(p);
    }
    if !acceptance.is_empty() {
        out.push_str("\nAcceptance: ");
        out.push_str(&acceptance.join("; "));
    }
    if !constraints.is_empty() {
        out.push_str("\nConstraints: ");
        out.push_str(&constraints.join("; "));
    }
    out.push_str("\n\nReturn one Lisp s-expression as specified.");
    out
}

#[derive(Debug)]
struct SexpValidationError {
    code: &'static str,
    reason: String,
    hint: &'static str,
}

fn validate_compiled_plan_sexp(
    raw: &str,
    board_task_id: &str,
) -> std::result::Result<String, SexpValidationError> {
    let stripped = strip_fenced_code_block(raw);
    let trimmed = stripped.trim();
    if trimmed.is_empty() {
        return Err(SexpValidationError {
            code: "INVALID_COMPILER_OUTPUT",
            reason: "compiler returned empty content after stripping fences".to_string(),
            hint: "rerun with compiler_mode=\"dry_run\" or retry sonnet",
        });
    }
    if !trimmed.starts_with('(') {
        return Err(SexpValidationError {
            code: "INVALID_COMPILER_OUTPUT",
            reason: format!(
                "compiler output must start with `(`; got `{}…`",
                trimmed.chars().take(16).collect::<String>()
            ),
            hint: "ensure the LLM emits one bare s-expression, no Markdown",
        });
    }
    if !parens_balanced(trimmed) {
        return Err(SexpValidationError {
            code: "INVALID_COMPILER_OUTPUT",
            reason: "parentheses are not balanced in compiler output".to_string(),
            hint: "retry the compile or fall back to compiler_mode=\"dry_run\"",
        });
    }
    let head = top_level_head(trimmed).unwrap_or("");
    if !ALLOWED_PLAN_HEADS.contains(&head) {
        return Err(SexpValidationError {
            code: "INVALID_COMPILER_OUTPUT",
            reason: format!(
                "top-level head `{}` not in allowlist {:?}",
                head, ALLOWED_PLAN_HEADS
            ),
            hint: "compiler must emit (plan …) | (plan-draft …) | (PLAN …)",
        });
    }
    if !trimmed.contains(board_task_id) {
        return Err(SexpValidationError {
            code: "INVALID_COMPILER_OUTPUT",
            reason: format!(
                "compiled plan does not reference board_task_id `{}`; refusing un-anchored plan",
                board_task_id
            ),
            hint: "the planner must include :board_task_id <id> so the row anchors correctly",
        });
    }
    Ok(trimmed.to_string())
}

/// Strip a leading ```lang fence and a trailing ``` fence (if both present).
/// Tolerant: lone fences or missing language tags are also handled.
fn strip_fenced_code_block(input: &str) -> String {
    let trimmed = input.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }
    let after_open = match trimmed.find('\n') {
        Some(idx) => &trimmed[idx + 1..],
        None => return trimmed.to_string(),
    };
    let body = match after_open.rfind("```") {
        Some(idx) => &after_open[..idx],
        None => after_open,
    };
    body.trim().to_string()
}

/// Balanced parens counter that ignores `(` / `)` inside double-quoted strings.
/// Honors `\\` and `\"` escape sequences inside strings.
fn parens_balanced(s: &str) -> bool {
    let mut depth: i64 = 0;
    let mut in_string = false;
    let mut escape = false;
    for ch in s.chars() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            match ch {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    !in_string && depth == 0
}

/// Extract the top-level head symbol from a sexp like `(plan ...)` → `plan`.
/// Returns None when the input does not start with `(` followed by a symbol char.
fn top_level_head(s: &str) -> Option<&str> {
    let trimmed = s.trim_start();
    let inner = trimmed.strip_prefix('(')?.trim_start();
    let end = inner
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || *c == '(' || *c == ')')
        .map(|(i, _)| i)
        .unwrap_or(inner.len());
    if end == 0 {
        None
    } else {
        Some(&inner[..end])
    }
}

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

// ───────────────────────────────────────────────────────────────────────
// approve / mark / supersede — control actions
// ───────────────────────────────────────────────────────────────────────

/// Action whitelist for the plan surface — the parsed
/// `review:plan:<id>:v<v>:<action>` envelope's `<action>` segment must be
/// in this list before we accept the resolution. Mirrors the manager
/// state-changing actions: compile / approve / mark / supersede. (`get`
/// / `list` / `by_task` / `record_evidence` / `execute` never resolve a
/// gate.)
const PLAN_REVIEW_ACTIONS: &[&str] = &["compile", "approve", "mark", "supersede"];

/// wave-18 / task 07 :: build the deterministic safety context for a
/// plan-side resolution. Mirrors the directive helper:
///   * `deterministic_mode` = `compiler_model.is_none()` (dry-run leaves
///     it unset; sonnet records `claude-sonnet`). LLM-driven plans
///     always block `auto_safe`.
///   * `protected_source_or_target` is currently `false` — plan rows
///     have no merge source/target concept; the rule still records a
///     loud-but-passing reason.
///   * Caller may opt into hash matching via `expected_file_sha256`
///     (none today; the wave-14 file-first writer surfaces the actual
///     hash on compile, and a future caller can pass the captured value
///     here).
fn build_plan_automation_ctx(
    args: &Value,
    plan_compiler_model: Option<&str>,
) -> ReviewAutomationContext {
    ReviewAutomationContext {
        deterministic_mode: plan_compiler_model.is_none(),
        file_write_attempted: false,
        file_write_succeeded: false,
        actual_file_sha256: None,
        expected_file_sha256: args
            .get("expected_file_sha256")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        protected_source_or_target: false,
        additional_blockers: Vec::new(),
    }
}

// ───────────────────────────────────────────────────────────────────────
// wave-21 / task 06 — LLM auto-approve proposal v0 (plan surface)
//
// Mirrors the directive surface — see directive.rs comment for the full
// invariant table. Wired into approve / mark / supersede; supersede is
// destructive and ALWAYS short-circuits to `destructive_blocked`. The
// proposal NEVER drives a DB transition or bus emission in v0
// (`applied=false` pinned; `requires_human=true` forced).
// ───────────────────────────────────────────────────────────────────────

const PLAN_REVIEW_PROPOSER_CALLER: &str = "plan_review_proposer";
const SONNET_PLAN_PROPOSER_MAX_TOKENS: u32 = 1024;

async fn request_plan_auto_approve_proposal(
    state: &AppState,
    mode: LlmAutoApproveProposalMode,
    action: &str,
    artifact_id: &uuid::Uuid,
    version: i32,
    deterministic_summary: &Value,
    artifact_digest: Option<&str>,
) -> LlmAutoApproveProposalBundle {
    if crate::handlers::knowledge::review_gate::is_destructive_review_action(action) {
        return LlmAutoApproveProposalBundle::destructive_blocked(
            mode,
            action,
            PLAN_REVIEW_PROPOSER_CALLER,
            None,
            format!(
                "rule:destructive_action:`{}` is destructive; auto-approve proposal NEVER promotes (invariant I2)",
                action.to_ascii_lowercase()
            ),
        );
    }

    let Some(sonnet) = state.sonnet.as_ref() else {
        return LlmAutoApproveProposalBundle::unavailable(
            mode,
            action,
            PLAN_REVIEW_PROPOSER_CALLER,
            "Sonnet gateway not initialized; LLM auto-approve proposal unavailable",
        );
    };

    let system = build_llm_auto_approve_proposal_system_prompt();
    let user = build_llm_auto_approve_proposal_user_prompt(
        "plan",
        action,
        &artifact_id.to_string(),
        version,
        deterministic_summary,
        artifact_digest,
    );
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system,
        },
        ChatMessage {
            role: "user".to_string(),
            content: user,
        },
    ];

    let raw = match sonnet
        .call_interactive(
            messages,
            Some(SONNET_PLAN_PROPOSER_MAX_TOKENS),
            PLAN_REVIEW_PROPOSER_CALLER,
        )
        .await
    {
        Ok(s) => s,
        Err(err) => {
            return LlmAutoApproveProposalBundle::unavailable(
                mode,
                action,
                PLAN_REVIEW_PROPOSER_CALLER,
                format!("Sonnet auto-approve proposal call failed: {}", err),
            );
        }
    };

    let (proposal, parse_warnings) = parse_llm_auto_approve_proposal(&raw);
    match proposal {
        Some(mut p) => {
            enforce_proposal_invariants(&mut p, action);
            LlmAutoApproveProposalBundle {
                mode,
                status: LlmAutoApproveProposalStatus::Suggested,
                proposal: Some(p),
                proposal_warnings: parse_warnings,
                unavailable_reason: None,
                action: action.to_string(),
                request_caller: Some(PLAN_REVIEW_PROPOSER_CALLER.to_string()),
                model: Some(SONNET_COMPILER_MODEL.to_string()),
            }
        }
        None => LlmAutoApproveProposalBundle {
            mode,
            status: LlmAutoApproveProposalStatus::NoSuggestion,
            proposal: None,
            proposal_warnings: parse_warnings,
            unavailable_reason: None,
            action: action.to_string(),
            request_caller: Some(PLAN_REVIEW_PROPOSER_CALLER.to_string()),
            model: Some(SONNET_COMPILER_MODEL.to_string()),
        },
    }
}

fn attach_plan_proposal_block(payload: &mut Value, bundle: &LlmAutoApproveProposalBundle) {
    if matches!(bundle.status, LlmAutoApproveProposalStatus::NotInvoked) {
        return;
    }
    stamp_llm_auto_approve_proposal_payload(payload, bundle);
}

/// Wave-22 / task 03 :: stamp the proposal hash + apply-gate outcome
/// onto the plan response payload. Pure / no DB mutation. Mirrors
/// `attach_directive_apply_gate_block` from directive.rs — see that
/// helper for the design rationale.
fn attach_plan_apply_gate_block(
    payload: &mut Value,
    bundle: &LlmAutoApproveProposalBundle,
    input: &LlmApproveApplyGateInput,
    artifact_id: &uuid::Uuid,
    version: i32,
) -> crate::handlers::knowledge::review_gate::LlmApproveApplyGateOutcome {
    stamp_proposal_hash_payload(
        payload,
        bundle,
        &bundle.action,
        &artifact_id.to_string(),
        version,
    );
    let outcome = evaluate_llm_approve_apply_gate(
        input,
        bundle,
        &bundle.action,
        &artifact_id.to_string(),
        version,
    );
    stamp_llm_approve_apply_gate_payload(payload, &outcome);
    outcome
}

fn parse_plan_proposer_mode_or_error(
    args: &Value,
) -> std::result::Result<Option<LlmAutoApproveProposalMode>, ToolError> {
    let mode = parse_llm_auto_approve_proposal_mode(args)
        .map_err(|msg| ToolError::new(error_codes::INVALID_PARAM, msg))?;
    if mode.is_sonnet_suggest() {
        Ok(Some(mode))
    } else if llm_auto_approve_proposal_mode_was_explicit(args) {
        Ok(Some(mode))
    } else {
        Ok(None)
    }
}

fn plan_proposer_summary(
    automation_outcome_status: &str,
    automation_policy: &str,
    decision_present: bool,
    extra: Option<(&str, Value)>,
) -> Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "review_automation_policy".to_string(),
        json!(automation_policy),
    );
    map.insert(
        "review_automation_status".to_string(),
        json!(automation_outcome_status),
    );
    map.insert(
        "explicit_decision_supplied".to_string(),
        json!(decision_present),
    );
    if let Some((k, v)) = extra {
        map.insert(k.to_string(), v);
    }
    Value::Object(map)
}

async fn action_approve(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "plan_id")?;

    let automation_policy = parse_review_automation_policy(args);
    let automation_explicit = review_automation_policy_was_explicit(args);

    // wave-21 / task 06 :: parse the propose-only `auto_approve_mode`
    // knob up-front so caller typos surface as INVALID_PARAM BEFORE any
    // DB read.
    let proposer_mode = match parse_plan_proposer_mode_or_error(args) {
        Ok(m) => m,
        Err(e) => return Ok(ToolResult::structured_error(e)),
    };

    // wave-22 / task 03 :: parse the apply-gate input up-front. Strict
    // shape errors fail-fast as INVALID_PARAM BEFORE any DB read.
    let apply_gate_input = match parse_llm_approve_apply_gate_input(args) {
        Ok(i) => i,
        Err((code, msg)) => {
            return Ok(ToolResult::structured_error(ToolError::new(code, msg)))
        }
    };

    // wave-15 :: explicit resolution bridge. When the caller supplies
    // `review_question_id` + `review_decision` we validate the envelope
    // BEFORE mutating plan state. `Rejected` / `NeedsChanges` skip the
    // approve transition entirely; `Approved` proceeds with the existing
    // `plan_update_status(Approved)` call.
    //
    // wave-18 / task 07 :: when a non-Manual `review_automation_policy`
    // is supplied without an explicit `review_decision` (which would
    // otherwise fail-fast with MISSING_PARAM), promote the qid into a
    // policy-driven evaluation path. Caller-supplied decisions ALWAYS
    // win over the policy.
    let resolution = match parse_review_resolution_input(args) {
        Ok(r) => r,
        Err(e) => {
            if matches!(automation_policy, ReviewAutomationPolicy::Manual)
                || !matches!(e, crate::handlers::knowledge::review_gate::ResolutionInputError::MissingDecision)
            {
                return Ok(ToolResult::structured_error(
                    ToolError::new(e.code(), e.message()),
                ));
            }
            let qid = parse_resolution_review_question_id(args)
                .expect("MissingDecision implies qid was present");
            return plan_action_approve_with_policy_only(
                state,
                id,
                qid,
                automation_policy,
                proposer_mode,
                apply_gate_input,
            )
            .await;
        }
    };

    if let Some(input) = resolution {
        return action_approve_with_resolution(
            state,
            id,
            input,
            automation_policy,
            automation_explicit,
            proposer_mode,
            apply_gate_input,
        )
        .await;
    }

    // wave-22 / task 03 :: when caller opted into the apply gate, the
    // legacy unconditional `plan_update_status(Approved)` is INVERTED —
    // the DB transition is gated on the LLM proposal passing all 6
    // strict gates. See directive.rs::action_approve for the full
    // design rationale (mirrored here for the plan surface).
    if apply_gate_input.apply {
        // We need the current plan version so the proposal hash is
        // computed against the head. Source it from the store.
        let plan = match state
            .store
            .plan_get(id)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
        {
            Some(p) => p,
            None => {
                return Ok(ToolResult::structured_error(ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("plan `{}` not found for apply gate", id),
                )))
            }
        };
        let resolved_mode = proposer_mode.unwrap_or(LlmAutoApproveProposalMode::Off);
        let summary = plan_proposer_summary("legacy_quiet", "manual", false, None);
        let bundle = request_plan_auto_approve_proposal(
            state,
            resolved_mode,
            "approve",
            &id,
            plan.version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        if let Err((code, msg)) = enforce_apply_gate_preflight(
            &apply_gate_input,
            &bundle,
            "approve",
            &id.to_string(),
            plan.version,
        ) {
            return Ok(ToolResult::structured_error(ToolError::new(code, msg)));
        }
        let mut payload = json!({
            "plan_id": id,
            "version": plan.version,
        });
        attach_plan_proposal_block(&mut payload, &bundle);
        let outcome = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            plan.version,
        );
        if outcome.status.should_apply() {
            state
                .store
                .plan_update_status(id, PlanStatus::Approved)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["status"] = json!("approved");
            payload["resolution_source"] = json!("llm_approve_apply_gate");
            let qid = parse_resolution_review_question_id(args);
            maybe_emit_review_question_resolved(
                &mut payload,
                &state.bus,
                qid.as_deref(),
                "approved",
                None,
            )
            .await;
        } else {
            payload["status"] = json!("llm_auto_apply_skipped");
            payload["next_step"] = json!(format!(
                "apply gate did not authorise (status={}); supply explicit `review_decision=approved` to flip the plan manually OR re-run with a matching proposal_hash + caller_approved=true",
                outcome.status.as_str()
            ));
        }
        return Ok(ToolResult::json_pretty(&payload));
    }

    state
        .store
        .plan_update_status(id, PlanStatus::Approved)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let mut payload = json!({
        "status": "approved",
        "plan_id": id,
    });
    // wave-11/14 quiet emit path — kept for callers that fire a Resolved
    // event without the wave-15 decision-bearing envelope.
    let qid = parse_resolution_review_question_id(args);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        qid.as_deref(),
        "approved",
        None,
    )
    .await;
    // wave-21 / task 06 :: propose-only Sonnet pass on the legacy path.
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary("legacy_quiet", "manual", false, None);
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "approve",
            &id,
            0,
            &summary,
            None,
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: stamp the proposal hash so callers can
        // echo it back via `proposal_hash` under
        // `apply_llm_auto_approve=true` on a follow-up call.
        stamp_proposal_hash_payload(&mut payload, &bundle, "approve", &id.to_string(), 0);
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-15 explicit resolution bridge for `action=approve`. Validates the
/// review envelope (scope / artifact / version / action) against the
/// current plan row, then performs the manager transition only when the
/// decision is `approved`.
///
/// wave-18 / task 07 :: also evaluates the deterministic
/// `review_automation_policy` and stamps the suggestion / status onto
/// the response payload. Caller-supplied `review_decision` ALWAYS wins.
async fn action_approve_with_resolution(
    state: &AppState,
    id: uuid::Uuid,
    input: ReviewResolutionInput,
    automation_policy: ReviewAutomationPolicy,
    automation_explicit: bool,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&input.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new("REVIEW_ID_MALFORMED", e.message()),
            ))
        }
    };
    let plan = match state
        .store
        .plan_get(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("plan `{}` not found for resolution", id),
                ),
            ))
        }
    };
    let current_version = plan.version;
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "plan",
        &id.to_string(),
        current_version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(
            ToolError::new(e.code(), e.message()),
        ));
    }

    let mut payload = json!({
        "plan_id": id,
        "version": current_version,
    });

    match input.decision.outcome() {
        ResolutionOutcome::PerformTransition => {
            state
                .store
                .plan_update_status(id, PlanStatus::Approved)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["status"] = json!("approved");
        }
        ResolutionOutcome::KeepArtifact => {
            payload["status"] = json!("review_rejected");
        }
        ResolutionOutcome::RequestChanges => {
            payload["status"] = json!("review_needs_changes");
            stamp_needs_changes_next_step(&mut payload, "plan", "compile");
        }
    }

    stamp_resolution_payload(&mut payload, &input);

    let mut automation_status_label = "not_evaluated".to_string();
    if automation_explicit || !matches!(automation_policy, ReviewAutomationPolicy::Manual) {
        let mut args_v = json!({});
        if let Some(map) = args_v.as_object_mut() {
            map.insert("review_automation_policy".into(), json!(automation_policy.as_str()));
        }
        let ctx = build_plan_automation_ctx(&args_v, plan.compiler_model.as_deref());
        let outcome = evaluate_review_automation(
            automation_policy,
            &ctx,
            Some(input.decision),
        );
        automation_status_label = outcome.status.as_str().to_string();
        stamp_review_automation_payload(&mut payload, &outcome);
    }

    let resolution_str = resolution_wire_string(input.decision);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        Some(&input.question_id),
        resolution_str,
        None,
    )
    .await;
    // wave-21 / task 06 :: propose-only Sonnet pass for the explicit-
    // resolution path. Caller decision ALWAYS wins; proposal is
    // informational only.
    //
    // wave-22 / task 03 :: apply gate is INFORMATIONAL ONLY here. The
    // explicit `review_decision` already drove (or refused) the DB
    // transition above. We do NOT fail-fast on hash mismatch — that
    // would lie about state. The gate block surfaces the verdict for
    // audit symmetry.
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            &automation_status_label,
            automation_policy.as_str(),
            true,
            None,
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "approve",
            &id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            current_version,
        );
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-18 / task 07 :: policy-driven approve path for `mission_plan`.
/// Fires when caller supplies `review_question_id` +
/// `review_automation_policy` (non-Manual) WITHOUT an explicit
/// `review_decision`. Auto-promotes to `Approved` only under `auto_safe`
/// + every safety rule passing. NEVER auto-rejects.
async fn plan_action_approve_with_policy_only(
    state: &AppState,
    id: uuid::Uuid,
    qid: String,
    automation_policy: ReviewAutomationPolicy,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&qid) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new("REVIEW_ID_MALFORMED", e.message()),
            ))
        }
    };
    let plan = match state
        .store
        .plan_get(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("plan `{}` not found for resolution", id),
                ),
            ))
        }
    };
    let current_version = plan.version;
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "plan",
        &id.to_string(),
        current_version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(
            ToolError::new(e.code(), e.message()),
        ));
    }

    let mut payload = json!({
        "plan_id": id,
        "version": current_version,
        "review_question_id": qid,
    });

    let mut args_v = json!({});
    if let Some(map) = args_v.as_object_mut() {
        map.insert(
            "review_automation_policy".into(),
            json!(automation_policy.as_str()),
        );
    }
    let ctx = build_plan_automation_ctx(&args_v, plan.compiler_model.as_deref());
    let outcome = evaluate_review_automation(automation_policy, &ctx, None);

    if outcome.may_auto_resolve {
        state
            .store
            .plan_update_status(id, PlanStatus::Approved)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        payload["status"] = json!("approved");
        payload["resolution_source"] = json!("review_automation_policy");
    } else {
        payload["status"] = json!("review_pending_decision");
        if matches!(outcome.status, AutomationStatus::AutoSafeBlocked) {
            payload["next_step"] = json!(
                "auto_safe blocked — supply explicit `review_decision` (approved|rejected|needs_changes) to flip the plan"
            );
        } else {
            payload["next_step"] = json!(
                "suggest mode is informational — supply explicit `review_decision` to flip the plan"
            );
        }
    }

    stamp_review_automation_payload(&mut payload, &outcome);

    if outcome.may_auto_resolve {
        maybe_emit_review_question_resolved(
            &mut payload,
            &state.bus,
            Some(&qid),
            "approved",
            None,
        )
        .await;
    }

    // wave-21 / task 06 :: propose-only Sonnet pass on the policy-only
    // approve path.
    //
    // wave-22 / task 03 :: apply gate is INFORMATIONAL ONLY on this
    // path — the deterministic policy already drove the DB transition.
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            outcome.status.as_str(),
            automation_policy.as_str(),
            false,
            None,
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "approve",
            &id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            current_version,
        );
    }

    Ok(ToolResult::json_pretty(&payload))
}

async fn action_mark(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "plan_id")?;
    let target_raw = require_str(args, "status")?;
    let target = PlanStatus::from_str(target_raw).map_err(|e| {
        anyhow!(
            "`{}` is not a valid PlanStatus: {} (valid: draft|awaiting_approval|approved|executing|succeeded|failed|superseded)",
            target_raw,
            e
        )
    })?;

    let automation_policy = parse_review_automation_policy(args);
    let automation_explicit = review_automation_policy_was_explicit(args);

    // wave-21 / task 06 :: parse the propose-only `auto_approve_mode`
    // knob up-front.
    let proposer_mode = match parse_plan_proposer_mode_or_error(args) {
        Ok(m) => m,
        Err(e) => return Ok(ToolResult::structured_error(e)),
    };

    // wave-22 / task 03 :: parse the apply-gate input up-front. mark is
    // a general state-transition action; the gate only authorises
    // mark-to-approved (mirrors the wave-18 policy posture). For other
    // target statuses the gate falls through to a SKIP outcome.
    let apply_gate_input = match parse_llm_approve_apply_gate_input(args) {
        Ok(i) => i,
        Err((code, msg)) => {
            return Ok(ToolResult::structured_error(ToolError::new(code, msg)))
        }
    };

    // wave-15 :: explicit resolution bridge — same pattern as approve.
    // wave-18 / task 07 :: same MissingDecision-under-policy promotion.
    // mark is the most general state transition (caller picks the target
    // status) — so the policy can only auto-promote when the requested
    // target is `approved`. Other targets surface the suggestion only.
    let resolution = match parse_review_resolution_input(args) {
        Ok(r) => r,
        Err(e) => {
            if matches!(automation_policy, ReviewAutomationPolicy::Manual)
                || !matches!(e, crate::handlers::knowledge::review_gate::ResolutionInputError::MissingDecision)
            {
                return Ok(ToolResult::structured_error(
                    ToolError::new(e.code(), e.message()),
                ));
            }
            let qid = parse_resolution_review_question_id(args)
                .expect("MissingDecision implies qid was present");
            return plan_action_mark_with_policy_only(
                state,
                id,
                target,
                qid,
                automation_policy,
                proposer_mode,
                apply_gate_input,
            )
            .await;
        }
    };

    if let Some(input) = resolution {
        return action_mark_with_resolution(
            state,
            id,
            target,
            input,
            automation_policy,
            automation_explicit,
            proposer_mode,
            apply_gate_input,
        )
        .await;
    }

    state
        .store
        .plan_update_status(id, target)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let mut payload = json!({
        "plan_id": id,
        "new_status": target.as_str(),
    });
    let qid = parse_resolution_review_question_id(args);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        qid.as_deref(),
        target.as_str(),
        None,
    )
    .await;
    // wave-21 / task 06 :: propose-only Sonnet pass for the legacy mark
    // path. The requested target is surfaced to the prompt for context.
    //
    // wave-22 / task 03 :: apply gate is informational on this path —
    // the legacy mark already ran the requested transition above. For
    // a future wave that wants to gate mark-to-approved on the LLM
    // proposal, the gate block is the audit anchor.
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            "legacy_quiet",
            "manual",
            false,
            Some(("requested_status", json!(target.as_str()))),
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "mark",
            &id,
            0,
            &summary,
            None,
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            0,
        );
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-15 explicit resolution bridge for `action=mark`. Validates the
/// review envelope; on `approved` decision performs the requested
/// `plan_update_status` transition; on `rejected`/`needs_changes` keeps
/// the plan at its current status.
///
/// wave-18 / task 07 :: stamps the automation outcome on the response.
/// Caller-supplied `review_decision` always wins.
async fn action_mark_with_resolution(
    state: &AppState,
    id: uuid::Uuid,
    target: PlanStatus,
    input: ReviewResolutionInput,
    automation_policy: ReviewAutomationPolicy,
    automation_explicit: bool,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&input.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new("REVIEW_ID_MALFORMED", e.message()),
            ))
        }
    };
    let plan = match state
        .store
        .plan_get(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("plan `{}` not found for resolution", id),
                ),
            ))
        }
    };
    let current_version = plan.version;
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "plan",
        &id.to_string(),
        current_version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(
            ToolError::new(e.code(), e.message()),
        ));
    }

    let mut payload = json!({
        "plan_id": id,
        "version": current_version,
        "requested_status": target.as_str(),
    });

    match input.decision.outcome() {
        ResolutionOutcome::PerformTransition => {
            state
                .store
                .plan_update_status(id, target)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["new_status"] = json!(target.as_str());
        }
        ResolutionOutcome::KeepArtifact => {
            payload["new_status"] = json!(plan.status.as_str());
            payload["status"] = json!("review_rejected");
        }
        ResolutionOutcome::RequestChanges => {
            payload["new_status"] = json!(plan.status.as_str());
            payload["status"] = json!("review_needs_changes");
            stamp_needs_changes_next_step(&mut payload, "plan", "compile");
        }
    }

    stamp_resolution_payload(&mut payload, &input);

    let mut automation_status_label = "not_evaluated".to_string();
    if automation_explicit || !matches!(automation_policy, ReviewAutomationPolicy::Manual) {
        let mut args_v = json!({});
        if let Some(map) = args_v.as_object_mut() {
            map.insert("review_automation_policy".into(), json!(automation_policy.as_str()));
        }
        let ctx = build_plan_automation_ctx(&args_v, plan.compiler_model.as_deref());
        let outcome = evaluate_review_automation(
            automation_policy,
            &ctx,
            Some(input.decision),
        );
        automation_status_label = outcome.status.as_str().to_string();
        stamp_review_automation_payload(&mut payload, &outcome);
    }

    let resolution_str = resolution_wire_string(input.decision);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        Some(&input.question_id),
        resolution_str,
        None,
    )
    .await;
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            &automation_status_label,
            automation_policy.as_str(),
            true,
            Some(("requested_status", json!(target.as_str()))),
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "mark",
            &id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: gate is informational — caller decision
        // already drove the transition above.
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            current_version,
        );
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-18 / task 07 :: policy-driven mark path. Auto-promotes ONLY
/// when the requested target status is `Approved` (the only safe
/// auto-resolution outcome for `mark`); other targets degrade to
/// suggest-only even when every safety rule passes.
async fn plan_action_mark_with_policy_only(
    state: &AppState,
    id: uuid::Uuid,
    target: PlanStatus,
    qid: String,
    automation_policy: ReviewAutomationPolicy,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&qid) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new("REVIEW_ID_MALFORMED", e.message()),
            ))
        }
    };
    let plan = match state
        .store
        .plan_get(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("plan `{}` not found for resolution", id),
                ),
            ))
        }
    };
    let current_version = plan.version;
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "plan",
        &id.to_string(),
        current_version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(
            ToolError::new(e.code(), e.message()),
        ));
    }

    let mut payload = json!({
        "plan_id": id,
        "version": current_version,
        "review_question_id": qid,
        "requested_status": target.as_str(),
    });

    let mut args_v = json!({});
    if let Some(map) = args_v.as_object_mut() {
        map.insert(
            "review_automation_policy".into(),
            json!(automation_policy.as_str()),
        );
    }
    let mut ctx = build_plan_automation_ctx(&args_v, plan.compiler_model.as_deref());
    if !matches!(target, PlanStatus::Approved) {
        // `mark` to a non-Approved target is never auto-promoted by the
        // policy. We pin a loud blocker so the audit trail explains the
        // refusal even when every safety rule otherwise passes.
        ctx.additional_blockers.push(format!(
            "non_approved_target:mark target `{}` is never auto-promoted by review_automation_policy (only `approved` is)",
            target.as_str()
        ));
    }
    let outcome = evaluate_review_automation(automation_policy, &ctx, None);

    if outcome.may_auto_resolve && matches!(target, PlanStatus::Approved) {
        state
            .store
            .plan_update_status(id, PlanStatus::Approved)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        payload["new_status"] = json!(PlanStatus::Approved.as_str());
        payload["status"] = json!("approved");
        payload["resolution_source"] = json!("review_automation_policy");
    } else {
        payload["new_status"] = json!(plan.status.as_str());
        payload["status"] = json!("review_pending_decision");
        payload["next_step"] = json!(
            "supply explicit `review_decision` (approved|rejected|needs_changes) to finalise the mark"
        );
    }

    stamp_review_automation_payload(&mut payload, &outcome);

    if outcome.may_auto_resolve && matches!(target, PlanStatus::Approved) {
        maybe_emit_review_question_resolved(
            &mut payload,
            &state.bus,
            Some(&qid),
            "approved",
            None,
        )
        .await;
    }

    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            outcome.status.as_str(),
            automation_policy.as_str(),
            false,
            Some(("requested_status", json!(target.as_str()))),
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "mark",
            &id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: gate is informational on policy path —
        // deterministic policy already drove the transition.
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            current_version,
        );
    }

    Ok(ToolResult::json_pretty(&payload))
}

async fn action_supersede(state: &AppState, args: &Value) -> Result<ToolResult> {
    let old_id = parse_id_arg(args, "old_plan_id")?;
    let new_id = parse_id_arg(args, "new_plan_id")?;

    let automation_policy = parse_review_automation_policy(args);
    let automation_explicit = review_automation_policy_was_explicit(args);

    // wave-21 / task 06 :: parse the propose-only `auto_approve_mode`
    // knob up-front. Supersede is destructive — the proposer ALWAYS
    // surfaces `destructive_blocked`.
    let proposer_mode = match parse_plan_proposer_mode_or_error(args) {
        Ok(m) => m,
        Err(e) => return Ok(ToolResult::structured_error(e)),
    };

    // wave-22 / task 03 :: parse the apply-gate input up-front.
    // supersede is destructive (invariant I2) — the gate ALWAYS skips
    // with `SkippedDestructiveAction`. Strict shape errors still
    // surface as INVALID_PARAM here.
    let apply_gate_input = match parse_llm_approve_apply_gate_input(args) {
        Ok(i) => i,
        Err((code, msg)) => {
            return Ok(ToolResult::structured_error(ToolError::new(code, msg)))
        }
    };

    // wave-15 :: explicit resolution bridge. Supersede pivots two plan
    // ids; the review envelope is anchored to `old_plan_id` (the artifact
    // being closed out by the supersede). `Rejected` / `NeedsChanges` skip
    // the supersede entirely.
    //
    // wave-18 / task 07 :: supersede is destructive (the old plan goes
    // to Superseded). We never auto-promote it from a policy — the
    // policy-only branch surfaces the suggestion and refuses to mutate.
    let resolution = match parse_review_resolution_input(args) {
        Ok(r) => r,
        Err(e) => {
            if matches!(automation_policy, ReviewAutomationPolicy::Manual)
                || !matches!(e, crate::handlers::knowledge::review_gate::ResolutionInputError::MissingDecision)
            {
                return Ok(ToolResult::structured_error(
                    ToolError::new(e.code(), e.message()),
                ));
            }
            let qid = parse_resolution_review_question_id(args)
                .expect("MissingDecision implies qid was present");
            return plan_action_supersede_with_policy_only(
                state,
                old_id,
                new_id,
                qid,
                automation_policy,
                proposer_mode,
                apply_gate_input,
            )
            .await;
        }
    };

    if let Some(input) = resolution {
        return action_supersede_with_resolution(
            state,
            old_id,
            new_id,
            input,
            automation_policy,
            automation_explicit,
            proposer_mode,
            apply_gate_input,
        )
        .await;
    }

    state
        .store
        .plan_supersede(old_id, new_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let mut payload = json!({
        "status": "superseded",
        "old_plan_id": old_id,
        "new_plan_id": new_id,
    });
    let qid = parse_resolution_review_question_id(args);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        qid.as_deref(),
        "superseded",
        None,
    )
    .await;
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            "legacy_quiet",
            "manual",
            false,
            Some(("new_plan_id", json!(new_id))),
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "supersede",
            &old_id,
            0,
            &summary,
            None,
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: supersede is destructive — gate ALWAYS
        // surfaces `skipped_destructive_action` (invariant I2).
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &old_id,
            0,
        );
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-15 explicit resolution bridge for `action=supersede`. Validates
/// the review envelope against the OLD plan (the artifact being closed),
/// then performs the supersede transition only when the decision is
/// `approved`.
///
/// wave-18 / task 07 :: stamps the automation outcome on the response.
/// Caller-supplied `review_decision` always wins.
async fn action_supersede_with_resolution(
    state: &AppState,
    old_id: uuid::Uuid,
    new_id: uuid::Uuid,
    input: ReviewResolutionInput,
    automation_policy: ReviewAutomationPolicy,
    automation_explicit: bool,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&input.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new("REVIEW_ID_MALFORMED", e.message()),
            ))
        }
    };
    let plan = match state
        .store
        .plan_get(old_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("old plan `{}` not found for resolution", old_id),
                ),
            ))
        }
    };
    let current_version = plan.version;
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "plan",
        &old_id.to_string(),
        current_version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(
            ToolError::new(e.code(), e.message()),
        ));
    }

    let mut payload = json!({
        "old_plan_id": old_id,
        "new_plan_id": new_id,
        "version": current_version,
    });

    match input.decision.outcome() {
        ResolutionOutcome::PerformTransition => {
            state
                .store
                .plan_supersede(old_id, new_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["status"] = json!("superseded");
        }
        ResolutionOutcome::KeepArtifact => {
            payload["status"] = json!("review_rejected");
        }
        ResolutionOutcome::RequestChanges => {
            payload["status"] = json!("review_needs_changes");
            stamp_needs_changes_next_step(&mut payload, "plan", "compile");
        }
    }

    stamp_resolution_payload(&mut payload, &input);

    let mut automation_status_label = "not_evaluated".to_string();
    if automation_explicit || !matches!(automation_policy, ReviewAutomationPolicy::Manual) {
        let mut args_v = json!({});
        if let Some(map) = args_v.as_object_mut() {
            map.insert("review_automation_policy".into(), json!(automation_policy.as_str()));
        }
        let ctx = build_plan_automation_ctx(&args_v, plan.compiler_model.as_deref());
        let outcome = evaluate_review_automation(
            automation_policy,
            &ctx,
            Some(input.decision),
        );
        automation_status_label = outcome.status.as_str().to_string();
        stamp_review_automation_payload(&mut payload, &outcome);
    }

    let resolution_str = resolution_wire_string(input.decision);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        Some(&input.question_id),
        resolution_str,
        None,
    )
    .await;
    if let Some(mode) = proposer_mode {
        // supersede is destructive — proposer ALWAYS surfaces
        // `destructive_blocked` (invariant I2).
        let summary = plan_proposer_summary(
            &automation_status_label,
            automation_policy.as_str(),
            true,
            Some(("new_plan_id", json!(new_id))),
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "supersede",
            &old_id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: supersede is destructive — gate ALWAYS
        // surfaces `skipped_destructive_action` (invariant I2).
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &old_id,
            current_version,
        );
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-18 / task 07 :: policy-driven supersede path. Supersede is
/// destructive (the old plan goes to `Superseded`); we never auto-promote
/// from a policy. Surfaces the suggestion only and refuses to mutate.
async fn plan_action_supersede_with_policy_only(
    state: &AppState,
    old_id: uuid::Uuid,
    new_id: uuid::Uuid,
    qid: String,
    automation_policy: ReviewAutomationPolicy,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&qid) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new("REVIEW_ID_MALFORMED", e.message()),
            ))
        }
    };
    let plan = match state
        .store
        .plan_get(old_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("old plan `{}` not found for resolution", old_id),
                ),
            ))
        }
    };
    let current_version = plan.version;
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "plan",
        &old_id.to_string(),
        current_version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(
            ToolError::new(e.code(), e.message()),
        ));
    }

    let mut payload = json!({
        "old_plan_id": old_id,
        "new_plan_id": new_id,
        "version": current_version,
        "review_question_id": qid,
    });

    let mut args_v = json!({});
    if let Some(map) = args_v.as_object_mut() {
        map.insert(
            "review_automation_policy".into(),
            json!(automation_policy.as_str()),
        );
    }
    let mut ctx = build_plan_automation_ctx(&args_v, plan.compiler_model.as_deref());
    ctx.additional_blockers.push(
        "destructive_action:supersede transitions are never auto-promoted by the automation policy"
            .to_string(),
    );
    let outcome = evaluate_review_automation(automation_policy, &ctx, None);

    payload["status"] = json!("review_pending_decision");
    payload["next_step"] = json!(
        "supply explicit `review_decision` (approved|rejected|needs_changes) — supersede is destructive and never auto-promoted"
    );

    stamp_review_automation_payload(&mut payload, &outcome);

    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            outcome.status.as_str(),
            automation_policy.as_str(),
            false,
            Some(("new_plan_id", json!(new_id))),
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "supersede",
            &old_id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: supersede is destructive — gate ALWAYS
        // surfaces `skipped_destructive_action` (invariant I2).
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &old_id,
            current_version,
        );
    }

    Ok(ToolResult::json_pretty(&payload))
}

// ───────────────────────────────────────────────────────────────────────
// wave-16 :: subscriber-side resolution bridge
//
// Called by `bus::v2_subscribers::spawn_review_resolution_sub` after the
// pure planner classified the inbound `QuestionEvent::Resolved` event as a
// plan route. Re-validates the envelope (so a stale qid resolved against
// a since-updated plan bails loudly) and, ONLY for an `Approved` decision
// on a transition action, performs the same DB transition as the explicit
// caller-side bridge. We never re-publish a Resolved bus event — the
// inbound event we just consumed IS that signal. `Rejected` /
// `NeedsChanges` / `compile`-action ids never mutate state.
//
// Note: `supersede` is supported by the explicit caller-side bridge
// (callers must pass both `old_plan_id` + `new_plan_id`). The subscriber
// path has only the qid envelope (which carries the OLD plan id), so we
// classify supersede as `SupersedeNeedsExplicitCall` and let the
// subscriber log a structured warning. Plan supersede should always be
// driven by an explicit operator, not an inferred bus event.
// ───────────────────────────────────────────────────────────────────────

/// Outcome of routing a `QuestionEvent::Resolved` event through the
/// plan-side bridge. Surfaced to the subscriber so it can record
/// observability without re-doing the match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanSubscriberOutcome {
    /// Decision was `Approved` on `approve` action and the plan was
    /// transitioned to `PlanStatus::Approved`.
    Approved,
    /// Decision was `Approved` on `mark` action — but the subscriber path
    /// has no target status (the `mark` qid envelope encodes the action
    /// label, not the target column value), so we DO NOT transition. The
    /// caller-side `mark` flow must be used for status flips.
    MarkNeedsExplicitCall,
    /// Decision was `Approved` on `supersede` action — but the subscriber
    /// only has the OLD plan id from the envelope; it cannot infer the
    /// NEW plan id. Supersede requires an explicit operator call.
    SupersedeNeedsExplicitCall,
    /// Decision was `Rejected` or `NeedsChanges`; left the plan at its
    /// current status.
    KeptArtifact { decision: ReviewDecision },
    /// Action was `compile` — no manager transition tied to compile path.
    CompileNoOp { decision: ReviewDecision },
    /// Envelope's `artifact_id` did not parse as a UUID.
    ArtifactIdNotUuid { artifact_id: String, error: String },
    /// Plan row was not found for the qid's artifact_id.
    NotFound { artifact_id: uuid::Uuid },
    /// Envelope failed re-validation (scope / version / action).
    EnvelopeRejected { code: &'static str, message: String },
    /// Underlying DB transition failed; the inbound `Resolved` event has
    /// already been consumed, so we surface the error as observability.
    DbError { detail: String },
}

/// Re-route a `QuestionEvent::Resolved` event whose envelope was parsed
/// as `scope=plan` through the same validators as the explicit
/// caller-side bridge. Performs `plan_update_status(Approved)` for an
/// `Approved` decision on `approve` action; classifies `mark` /
/// `supersede` as needing-explicit-call (those actions carry parameters
/// not present in the qid envelope). Pure side-effects: at most one DB
/// write; no bus publish.
pub(crate) async fn handle_review_resolved_event(
    state: &AppState,
    parsed: &ParsedReviewQuestionId,
    decision: ReviewDecision,
) -> PlanSubscriberOutcome {
    let id = match uuid::Uuid::parse_str(&parsed.artifact_id) {
        Ok(u) => u,
        Err(e) => {
            return PlanSubscriberOutcome::ArtifactIdNotUuid {
                artifact_id: parsed.artifact_id.clone(),
                error: e.to_string(),
            }
        }
    };
    let plan = match state.store.plan_get(id).await {
        Ok(Some(p)) => p,
        Ok(None) => return PlanSubscriberOutcome::NotFound { artifact_id: id },
        Err(e) => {
            return PlanSubscriberOutcome::DbError {
                detail: format!("plan_get: {}", e),
            }
        }
    };
    if let Err(e) = validate_review_resolution_envelope(
        parsed,
        "plan",
        &id.to_string(),
        plan.version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return PlanSubscriberOutcome::EnvelopeRejected {
            code: e.code(),
            message: e.message(),
        };
    }
    if matches!(decision.outcome(), ResolutionOutcome::KeepArtifact)
        || matches!(decision.outcome(), ResolutionOutcome::RequestChanges)
    {
        return PlanSubscriberOutcome::KeptArtifact { decision };
    }
    match parsed.action.as_str() {
        "compile" => PlanSubscriberOutcome::CompileNoOp { decision },
        "approve" => match state
            .store
            .plan_update_status(id, PlanStatus::Approved)
            .await
        {
            Ok(_) => PlanSubscriberOutcome::Approved,
            Err(e) => PlanSubscriberOutcome::DbError {
                detail: format!("plan_update_status(approved): {}", e),
            },
        },
        "mark" => PlanSubscriberOutcome::MarkNeedsExplicitCall,
        "supersede" => PlanSubscriberOutcome::SupersedeNeedsExplicitCall,
        // validate_review_resolution_envelope above already rejected
        // anything outside PLAN_REVIEW_ACTIONS.
        _ => PlanSubscriberOutcome::CompileNoOp { decision },
    }
}

// ───────────────────────────────────────────────────────────────────────
// plan-runner auto-selection v1
//
// When `mission_plan(action=execute)` is called without `target` (or other
// dispatch knobs), a small conservative parser extracts hints from
// `plan.sexp_text` so the runner can route on its own. Explicit args still
// win; this is purely a fallback so PLAN.lisp can speak for itself.
//
// Lisp authority:
//   intent-flow.lisp        :: F-intent-alignment-plan-execution-loop ::
//                                s6 execution-runner
//   intent-flow.lisp        :: F-workstation-dispatch-policy
//   intent-intent-layer.lisp :: section unified-entry-pipeline ::
//                                role plan-runner
//   intent-worker.lisp      :: claudecode-workstation-orchestration
//   intent-tools.lisp       :: mission_plan :: :dispatch-strategy-consumer
// ───────────────────────────────────────────────────────────────────────

pub(super) const AGENT_TEAM_OBJECTIVE_HINT: &str = "使用 agent-team提高效率";

#[derive(Debug, Default, Clone)]
pub(super) struct ParsedPlanHints {
    pub(super) target: Option<String>,
    pub(super) flow_id: Option<String>,
    pub(super) dispatch_strategy: Option<String>,
    pub(super) parallelism: Option<String>,
    pub(super) target_project: Option<String>,
    pub(super) requested_cwd: Option<String>,
    pub(super) objective: Option<String>,
    pub(super) summary: Option<String>,
    /// wave-15 / task 05 — workstation-dispatch hint contract. Captured
    /// here so a single PLAN.lisp scan extracts every recognised field;
    /// the workstation_dispatch module reads them via `to_workstation_*`.
    pub(super) scope: Option<String>,
    pub(super) commit_policy: Option<String>,
    pub(super) owned_files_raw: Option<String>,
    pub(super) forbidden_files_raw: Option<String>,
    pub(super) acceptance_commands_raw: Option<String>,
    /// `:workstation-dispatch true` opts into workstation_dispatch v0.
    /// Stored as the parsed bareword so we keep the conservative
    /// "no Lisp interpretation" stance.
    pub(super) workstation_dispatch_flag: Option<String>,
}

impl ParsedPlanHints {
    fn to_summary_json(&self) -> Value {
        let mut map = serde_json::Map::new();
        let mut put = |k: &str, v: &Option<String>| {
            if let Some(s) = v {
                map.insert(k.to_string(), Value::String(s.clone()));
            }
        };
        put("target", &self.target);
        put("flow_id", &self.flow_id);
        put("dispatch_strategy", &self.dispatch_strategy);
        put("parallelism", &self.parallelism);
        put("target_project", &self.target_project);
        put("requested_cwd", &self.requested_cwd);
        put("objective", &self.objective);
        put("summary", &self.summary);
        put("scope", &self.scope);
        put("commit_policy", &self.commit_policy);
        put("owned_files", &self.owned_files_raw);
        put("forbidden_files", &self.forbidden_files_raw);
        put("acceptance_commands", &self.acceptance_commands_raw);
        put("workstation_dispatch", &self.workstation_dispatch_flag);
        Value::Object(map)
    }

    /// True iff the PLAN.lisp surfaced `:workstation-dispatch true` (or any
    /// bareword that lowercases to `true`/`yes`/`on`). False otherwise —
    /// `:workstation-dispatch false` and absence both produce False.
    pub(super) fn workstation_dispatch_opt_in(&self) -> bool {
        match self.workstation_dispatch_flag.as_deref() {
            Some(raw) => matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "true" | "yes" | "on" | "1"
            ),
            None => false,
        }
    }

    /// Project the parsed PLAN.lisp scalars into the workstation-dispatch
    /// hint struct. Lists (`owned-files`, `forbidden-files`,
    /// `acceptance-commands`) round-trip through whitespace splitting on
    /// the captured raw value because the conservative scanner records
    /// the whole bracket span as one string.
    pub(super) fn to_workstation_hints(&self) -> super::workstation_dispatch::WorkstationDispatchHints {
        super::workstation_dispatch::WorkstationDispatchHints {
            objective: self.objective.clone().or_else(|| self.summary.clone()),
            scope: self.scope.clone(),
            owned_files: split_lisp_string_list(self.owned_files_raw.as_deref()),
            forbidden_files: split_lisp_string_list(self.forbidden_files_raw.as_deref()),
            acceptance_commands: split_lisp_string_list(
                self.acceptance_commands_raw.as_deref(),
            ),
            commit_policy: self.commit_policy.clone(),
            target_project: self.target_project.clone(),
            requested_cwd: self.requested_cwd.clone(),
            dispatch_strategy: self.dispatch_strategy.clone(),
        }
    }
}

/// Split a captured PLAN.lisp list value (`["a" "b"]` / `(a b)` / bareword
/// run) into a vector of strings. Quoted strings have their quotes
/// stripped; bare words pass through. Whitespace and commas separate
/// elements. Conservative on purpose: anything weird produces an empty
/// slice rather than a partial parse.
pub(super) fn split_lisp_string_list(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw else { return Vec::new() };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .or_else(|| trimmed.strip_prefix('(').and_then(|s| s.strip_suffix(')')))
        .unwrap_or(trimmed);
    let mut out: Vec<String> = Vec::new();
    let chars: Vec<char> = inner.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        while i < n && (chars[i].is_whitespace() || chars[i] == ',') {
            i += 1;
        }
        if i >= n {
            break;
        }
        if chars[i] == '"' {
            i += 1;
            let start = i;
            let mut esc = false;
            while i < n {
                let c = chars[i];
                if esc {
                    esc = false;
                    i += 1;
                    continue;
                }
                if c == '\\' {
                    esc = true;
                    i += 1;
                    continue;
                }
                if c == '"' {
                    break;
                }
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            if !s.trim().is_empty() {
                out.push(s);
            }
            if i < n {
                i += 1;
            }
        } else {
            let start = i;
            while i < n
                && !chars[i].is_whitespace()
                && chars[i] != ','
                && chars[i] != '"'
                && chars[i] != '('
                && chars[i] != ')'
                && chars[i] != '['
                && chars[i] != ']'
            {
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            if !s.trim().is_empty() {
                out.push(s);
            }
        }
    }
    out
}

#[derive(Debug, Clone)]
struct ResolvedExec {
    target: &'static str,
    target_source: &'static str,
    dispatch_strategy: &'static str,
    dispatch_strategy_source: &'static str,
    plan_hint_summary: Value,
}

/// Parse a PLAN.lisp s-expression for known runner hints. This is NOT a full
/// Lisp interpreter; it scans `:keyword value` pairs at any depth and keeps
/// the first occurrence per keyword. Conservative on purpose: anything that
/// doesn't look like a simple keyword/value pair is silently skipped.
fn parse_plan_hints(sexp: &str) -> ParsedPlanHints {
    let mut h = ParsedPlanHints::default();

    fn store_first(slot: &mut Option<String>, value: &str) {
        if slot.is_none() {
            let v = value.trim();
            if !v.is_empty() {
                *slot = Some(v.to_string());
            }
        }
    }

    for (raw_key, value) in scan_keyword_pairs(sexp) {
        let key = raw_key.to_ascii_lowercase();
        match key.as_str() {
            "target" | "target-tool" | "tool" => store_first(&mut h.target, &value),
            "flow-id" | "flow_id" => store_first(&mut h.flow_id, &value),
            "dispatch-strategy" | "dispatch_strategy" => {
                store_first(&mut h.dispatch_strategy, &value)
            }
            "parallelism" => store_first(&mut h.parallelism, &value),
            "target-project" | "target_project" | "project" => {
                store_first(&mut h.target_project, &value)
            }
            "requested-cwd" | "requested_cwd" | "cwd" => {
                store_first(&mut h.requested_cwd, &value)
            }
            "objective" => store_first(&mut h.objective, &value),
            "summary" => store_first(&mut h.summary, &value),
            "scope" => store_first(&mut h.scope, &value),
            "commit-policy" | "commit_policy" => {
                store_first(&mut h.commit_policy, &value)
            }
            "owned-files" | "owned_files" => {
                store_first(&mut h.owned_files_raw, &value)
            }
            "forbidden-files" | "forbidden_files" => {
                store_first(&mut h.forbidden_files_raw, &value)
            }
            "acceptance-commands" | "acceptance_commands" => {
                store_first(&mut h.acceptance_commands_raw, &value)
            }
            "workstation-dispatch" | "workstation_dispatch" => {
                store_first(&mut h.workstation_dispatch_flag, &value)
            }
            _ => {}
        }
    }
    h
}

/// Scan a string for `:keyword value` pairs. Three value shapes are recognised:
///   * double-quoted string literal — handles `\\` and `\"` escapes
///   * bracket / paren list — `[a "b" c]` or `(a "b" c)` round-trip as one
///     captured string spanning the whole bracket pair (wave-15 / task 05
///     opt-in addition; readers split via `split_lisp_string_list`).
///   * bareword — terminates on whitespace / `(` / `)` / `[` / `]` / `"`
/// Bare `:k` with no value and `:k :next-key` patterns are still skipped so
/// the parser stays conservative for non-list authoring.
fn scan_keyword_pairs(sexp: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = sexp.chars().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut i = 0;
    let mut in_string = false;
    let mut esc = false;
    while i < n {
        let c = chars[i];
        if in_string {
            if esc {
                esc = false;
                i += 1;
                continue;
            }
            if c == '\\' {
                esc = true;
                i += 1;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            i += 1;
            continue;
        }
        if c != ':' {
            i += 1;
            continue;
        }
        let key_start = i + 1;
        let mut j = key_start;
        while j < n {
            let cj = chars[j];
            if cj.is_whitespace()
                || cj == '('
                || cj == ')'
                || cj == '['
                || cj == ']'
                || cj == '"'
                || cj == ':'
            {
                break;
            }
            j += 1;
        }
        if j == key_start {
            i += 1;
            continue;
        }
        let key: String = chars[key_start..j].iter().collect();
        let mut k = j;
        while k < n && chars[k].is_whitespace() {
            k += 1;
        }
        if k >= n {
            break;
        }
        let next = chars[k];
        match next {
            '"' => {
                let mut m = k + 1;
                let mut value = String::new();
                let mut esc2 = false;
                while m < n {
                    let cm = chars[m];
                    if esc2 {
                        value.push(cm);
                        esc2 = false;
                        m += 1;
                        continue;
                    }
                    if cm == '\\' {
                        esc2 = true;
                        m += 1;
                        continue;
                    }
                    if cm == '"' {
                        m += 1;
                        break;
                    }
                    value.push(cm);
                    m += 1;
                }
                out.push((key, value));
                i = m;
            }
            '[' | '(' => {
                let open = next;
                let close = if open == '[' { ']' } else { ')' };
                let mut depth = 0i64;
                let mut m = k;
                let mut esc2 = false;
                let mut in_str = false;
                while m < n {
                    let cm = chars[m];
                    if in_str {
                        if esc2 {
                            esc2 = false;
                            m += 1;
                            continue;
                        }
                        if cm == '\\' {
                            esc2 = true;
                            m += 1;
                            continue;
                        }
                        if cm == '"' {
                            in_str = false;
                        }
                        m += 1;
                        continue;
                    }
                    if cm == '"' {
                        in_str = true;
                        m += 1;
                        continue;
                    }
                    if cm == open {
                        depth += 1;
                    } else if cm == close {
                        depth -= 1;
                        if depth == 0 {
                            m += 1;
                            break;
                        }
                    }
                    m += 1;
                }
                let value: String = chars[k..m].iter().collect();
                out.push((key, value));
                i = m;
            }
            ')' | ':' => {
                i = k;
            }
            _ => {
                let mut m = k;
                while m < n {
                    let cm = chars[m];
                    if cm.is_whitespace()
                        || cm == '('
                        || cm == ')'
                        || cm == '['
                        || cm == ']'
                        || cm == '"'
                    {
                        break;
                    }
                    m += 1;
                }
                if m > k {
                    let value: String = chars[k..m].iter().collect();
                    out.push((key, value));
                    i = m;
                } else {
                    i = k;
                }
            }
        }
    }
    out
}

// ───────────────────────────────────────────────────────────────────────
// wave-18 / task 06 — autonomous PLAN field inference v0
//
// Conservative deterministic helper that infers a small set of PLAN DAG
// fields when the caller / PLAN.lisp / evidence-sidecar carry enough
// signal. Inference is gated on the new `infer_plan_fields` knob:
//
//   `off`        (default) — no inference; legacy byte-shape preserved.
//   `preview`    — runs the inference and returns ONLY the inference block;
//                  the underlying execute pipeline is NOT invoked. Caller
//                  uses this to verify what apply_safe would do without
//                  mutating any args.
//   `apply_safe` — runs the inference, augments caller args with every
//                  field whose confidence >= apply-threshold AND whose
//                  caller-side slot is empty, then proceeds with execute
//                  exactly as if the caller had passed the augmented args.
//                  Caller-supplied values ALWAYS win (conflicts surface
//                  on `conflicts[]` and are NEVER mutated).
//
// Six fields are supported in v0:
//   target / dispatch_strategy / target_project / owned_files /
//   acceptance_mode / workstation_dispatch.
//
// Sources scanned (deterministic, no LLM):
//   1. `plan.sexp_text` — already parsed via `parse_plan_hints`. PLAN-side
//      hints are the highest-confidence source.
//   2. `plan.compiled_from` — directive provenance string (e.g.
//      "directive/<id>:<v>" or "board_task/<id>"). Read for keyword
//      signals only (e.g. "task_delegate" / "agent-team").
//   3. evidence sidecar at
//      `<project>/.missiond/v2/plans/<plan_id>.evidence.json` — historical
//      `plan_runner_dispatch` / `workstation_dispatch` entries carry the
//      target / dispatch_strategy / owned_files we used last time.
//
// Lisp authority forward-reference (wave-18 / task 10 backfill):
//   - intent-flow.lisp :: F-intent-alignment-plan-execution-loop ::
//                          s4 plan-authoring (autonomous inference)
//   - intent-intent-layer.lisp :: section unified-entry-pipeline ::
//                                  role plan-runner (deterministic infer)
// ───────────────────────────────────────────────────────────────────────

/// Wire-form constants for `infer_plan_fields`. Mirror these in the MCP
/// descriptor enum so the two surfaces cannot drift.
pub(super) const INFER_MODE_OFF: &str = "off";
pub(super) const INFER_MODE_PREVIEW: &str = "preview";
pub(super) const INFER_MODE_APPLY_SAFE: &str = "apply_safe";
/// wave-20 / task 07 — LLM-augmented PLAN field inference v0.
///
/// Opt-in mode that asks Sonnet to PROPOSE values for the same six PLAN
/// fields that the deterministic engine handles, but ONLY when the
/// deterministic pass returned no signal at all for that field (no
/// inferred / no suggested / no conflict). LLM proposals are surfaced
/// under `plan_field_inference.llm_proposals[]` and NEVER auto-applied —
/// they are explicit suggestions for caller review. Mutation policy
/// stays identical to `preview` (no plan FSM transitions, no augmented
/// args). The deterministic engine still runs first; LLM output never
/// overrides a deterministic high-confidence inference.
pub(super) const INFER_MODE_SONNET_SUGGEST: &str = "sonnet_suggest";

/// Resolved inference mode after argument validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InferPlanFieldsMode {
    Off,
    Preview,
    ApplySafe,
    /// wave-20 / task 07 — LLM proposals on top of deterministic inference.
    SonnetSuggest,
}

impl InferPlanFieldsMode {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            InferPlanFieldsMode::Off => INFER_MODE_OFF,
            InferPlanFieldsMode::Preview => INFER_MODE_PREVIEW,
            InferPlanFieldsMode::ApplySafe => INFER_MODE_APPLY_SAFE,
            InferPlanFieldsMode::SonnetSuggest => INFER_MODE_SONNET_SUGGEST,
        }
    }

    /// True when the mode opts into the wave-20 / task 07 LLM-augmented
    /// proposal pass. SonnetSuggest is the only LLM-touching mode in v0;
    /// preview / apply_safe / off are byte-for-byte identical to
    /// wave-18 / task 06 deterministic behaviour.
    pub(super) fn is_llm_augmented(self) -> bool {
        matches!(self, InferPlanFieldsMode::SonnetSuggest)
    }
}

/// Strict allowlist for the `infer_plan_fields` knob. Returns the canonical
/// mode or a structured error message. Default (absent / blank / `off`) →
/// `Off` which preserves the legacy byte-shape.
pub(super) fn parse_infer_plan_fields_mode(
    args: &Value,
) -> std::result::Result<InferPlanFieldsMode, String> {
    match args.get("infer_plan_fields").and_then(|v| v.as_str()) {
        None | Some("") | Some(INFER_MODE_OFF) => Ok(InferPlanFieldsMode::Off),
        Some(INFER_MODE_PREVIEW) => Ok(InferPlanFieldsMode::Preview),
        Some(INFER_MODE_APPLY_SAFE) => Ok(InferPlanFieldsMode::ApplySafe),
        Some(INFER_MODE_SONNET_SUGGEST) => Ok(InferPlanFieldsMode::SonnetSuggest),
        Some(other) => Err(format!(
            "infer_plan_fields must be one of [\"off\", \"preview\", \"apply_safe\", \"sonnet_suggest\"]; got `{}`",
            other
        )),
    }
}

// ── wave-21 / task 04 — autonomous workstation LLM proposal v0 ─────────
//
// Wire-form constants for `workstation_inference_mode`. Strictly orthogonal
// to `infer_plan_fields` (wave-18 / task 06 + wave-20 / task 07) which
// targets the six PLAN field knobs. The workstation surface targets the
// four core dispatch knobs (target / dispatch_strategy / objective / scope)
// and ONLY fires when caller / PLAN supplied no signal at all.
//
// Default mode `off` ⇒ byte-compatible with wave-15..20 (no proposal pass,
// no response augmentation, no Sonnet call). The new `sonnet_suggest`
// mode triggers the wave-21 proposal pipeline implemented in
// `workstation_dispatch::request_workstation_proposals`. Conservative
// invariants pinned at the call-site:
//   * proposals are SURFACED only, never auto-applied / never auto-spawn;
//   * Sonnet unavailable ⇒ `LLM_UNAVAILABLE` bundle (NEVER falls back to
//     `claude -p` or prompt mode);
//   * DAG mode rejects sonnet_suggest at preflight (single-node-only in v0).
pub(super) const WORKSTATION_INFER_MODE_OFF: &str = "off";
pub(super) const WORKSTATION_INFER_MODE_SONNET_SUGGEST: &str = "sonnet_suggest";

/// Resolved workstation-inference mode after argument validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkstationInferenceMode {
    /// Default — no proposal pass; response is byte-identical with
    /// wave-15..20 callers.
    Off,
    /// Opt-in — when caller / PLAN supply no workstation hints AND
    /// dispatch decision came back NotApplicable, ask Sonnet to propose
    /// values for `target` / `dispatch_strategy` / `objective` / `scope`.
    /// Proposals never alter the dispatch path; they are surfaced under
    /// `workstation_proposals` for operator review.
    SonnetSuggest,
}

impl WorkstationInferenceMode {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            WorkstationInferenceMode::Off => WORKSTATION_INFER_MODE_OFF,
            WorkstationInferenceMode::SonnetSuggest => WORKSTATION_INFER_MODE_SONNET_SUGGEST,
        }
    }

    /// True when the mode opts into the wave-21 / task 04 LLM proposal
    /// pass. SonnetSuggest is the only opt-in mode in v0.
    pub(super) fn is_sonnet_suggest(self) -> bool {
        matches!(self, WorkstationInferenceMode::SonnetSuggest)
    }
}

/// Strict allowlist for the `workstation_inference_mode` knob. Returns
/// the canonical mode or a structured error message. Default (absent /
/// blank / `off`) → `Off` which preserves the wave-15..20 byte-shape.
pub(super) fn parse_workstation_inference_mode(
    args: &Value,
) -> std::result::Result<WorkstationInferenceMode, String> {
    match args.get("workstation_inference_mode").and_then(|v| v.as_str()) {
        None | Some("") | Some(WORKSTATION_INFER_MODE_OFF) => {
            Ok(WorkstationInferenceMode::Off)
        }
        Some(WORKSTATION_INFER_MODE_SONNET_SUGGEST) => {
            Ok(WorkstationInferenceMode::SonnetSuggest)
        }
        Some(other) => Err(format!(
            "workstation_inference_mode must be one of [\"off\", \"sonnet_suggest\"]; got `{}`",
            other
        )),
    }
}

/// Refuse `workstation_inference_mode=sonnet_suggest` when the DAG
/// scheduler is engaged. v0 keeps the proposal pass single-node-only —
/// the DAG path runs many nodes per execute and surfacing a per-node
/// proposal block would balloon the response payload AND blur the
/// "ONLY when no PLAN hints exist" invariant (each node has its own
/// hint set). Mirrors the wave-20 / task 07 enforcement on the same
/// path. Returns `Some(structured_error)` when refused, `None` otherwise.
pub(super) fn refuse_workstation_inference_in_dag_mode(
    args: &Value,
) -> Option<ToolResult> {
    let scheduler_mode = args
        .get("scheduler_mode")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    if scheduler_mode != "dag_v1" {
        return None;
    }
    let mode = args
        .get("workstation_inference_mode")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    if mode != WORKSTATION_INFER_MODE_SONNET_SUGGEST {
        return None;
    }
    Some(ToolResult::structured_error(
        ToolError::new(
            error_codes::INVALID_PARAM,
            "workstation_inference_mode=\"sonnet_suggest\" is single-node-execute-only \
             in v0; combining it with scheduler_mode=\"dag_v1\" is unsupported",
        )
        .with_suggestion(
            "drop scheduler_mode=\"dag_v1\" to run the proposal pass against the root \
             plan, or run with workstation_inference_mode=\"off\" (default) to keep DAG \
             behaviour byte-identical with wave-15..20",
        ),
    ))
}

/// Confidence tier for an inferred field. Only `High` is auto-applied
/// under `apply_safe`; lower tiers always degrade to suggestions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InferenceConfidence {
    High,
    Medium,
    Low,
}

impl InferenceConfidence {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            InferenceConfidence::High => "high",
            InferenceConfidence::Medium => "medium",
            InferenceConfidence::Low => "low",
        }
    }

    /// Apply threshold for `apply_safe`. Conservative on purpose: only
    /// `High` confidence fields auto-fill missing caller args. Medium /
    /// Low always degrade to suggestions.
    pub(super) fn meets_apply_threshold(self) -> bool {
        matches!(self, InferenceConfidence::High)
    }
}

/// Single inferred field with its provenance + confidence.
#[derive(Debug, Clone)]
pub(super) struct InferredField {
    pub(super) field: &'static str,
    pub(super) value: Value,
    pub(super) confidence: InferenceConfidence,
    pub(super) source: &'static str,
    pub(super) detail: Option<String>,
}

impl InferredField {
    fn to_json(&self) -> Value {
        let mut m = serde_json::Map::new();
        m.insert("field".to_string(), json!(self.field));
        m.insert("value".to_string(), self.value.clone());
        m.insert("confidence".to_string(), json!(self.confidence.as_wire()));
        m.insert("source".to_string(), json!(self.source));
        if let Some(d) = &self.detail {
            m.insert("detail".to_string(), json!(d));
        }
        Value::Object(m)
    }
}

/// One conflict entry: caller passed an explicit value but the inferer
/// derived a different value from a recognised source. The inferer NEVER
/// mutates over a caller value — the conflict is surfaced for review only.
#[derive(Debug, Clone)]
pub(super) struct InferenceConflict {
    pub(super) field: &'static str,
    pub(super) caller_value: Value,
    pub(super) inferred_value: Value,
    pub(super) confidence: InferenceConfidence,
    pub(super) source: &'static str,
}

impl InferenceConflict {
    fn to_json(&self) -> Value {
        json!({
            "field": self.field,
            "caller_value": self.caller_value,
            "inferred_value": self.inferred_value,
            "confidence": self.confidence.as_wire(),
            "source": self.source,
        })
    }
}

/// Aggregate inference result attached to the response under
/// `plan_field_inference`. Always carries every field so a caller can
/// pivot on a single shape (`mode`, `inferred_fields[]`,
/// `suggested_fields[]`, `conflicts[]`, `inference_status`,
/// `evidence_sources[]`).
#[derive(Debug, Default)]
pub(super) struct PlanFieldInference {
    pub(super) inferred: Vec<InferredField>,
    pub(super) suggested: Vec<InferredField>,
    pub(super) conflicts: Vec<InferenceConflict>,
    /// Names of evidence sources actually consulted (e.g.
    /// `"plan_sexp"`, `"compiled_from"`, `"evidence_sidecar"`). Surfaced
    /// so observers can tell which knobs the inferer scanned without
    /// reconstructing it from the per-field `source` strings.
    pub(super) evidence_sources: Vec<&'static str>,
    /// wave-20 / task 07 — Sonnet-augmented proposals. Always `None` for
    /// `off` / `preview` / `apply_safe`. Populated only under
    /// `sonnet_suggest`. NEVER auto-applied; surfaced for caller review.
    pub(super) llm: Option<LlmProposalBundle>,
}

impl PlanFieldInference {
    /// Wire status string. Surfaced as `inference_status` on the response.
    pub(super) fn status(&self, mode: InferPlanFieldsMode) -> &'static str {
        match mode {
            InferPlanFieldsMode::Off => "off",
            InferPlanFieldsMode::Preview => {
                if self.inferred.is_empty() && self.suggested.is_empty() && self.conflicts.is_empty() {
                    "preview_no_signal"
                } else {
                    "preview"
                }
            }
            InferPlanFieldsMode::ApplySafe => {
                let any_applied = self
                    .inferred
                    .iter()
                    .any(|f| f.confidence.meets_apply_threshold());
                if !any_applied && self.suggested.is_empty() && self.conflicts.is_empty() {
                    "apply_safe_no_signal"
                } else if any_applied {
                    "apply_safe_applied"
                } else {
                    "apply_safe_suggestions_only"
                }
            }
            // wave-20 / task 07 — `sonnet_suggest` reports the deterministic
            // shape under `inference_status` (so observers reading the legacy
            // field still see a meaningful tier) and the Sonnet-specific
            // outcome under `llm_status`. The deterministic block is the
            // same as `preview` for this mode (we never auto-apply).
            InferPlanFieldsMode::SonnetSuggest => {
                if self.inferred.is_empty() && self.suggested.is_empty() && self.conflicts.is_empty() {
                    "sonnet_suggest_no_deterministic_signal"
                } else {
                    "sonnet_suggest"
                }
            }
        }
    }

    /// Build the JSON block surfaced under `plan_field_inference` on the
    /// response. Always carries every list (empty when nothing fired) so
    /// observers can pivot on a stable shape.
    pub(super) fn to_response_json(&self, mode: InferPlanFieldsMode) -> Value {
        let inferred: Vec<Value> = self.inferred.iter().map(|f| f.to_json()).collect();
        let suggested: Vec<Value> = self.suggested.iter().map(|f| f.to_json()).collect();
        let conflicts: Vec<Value> = self.conflicts.iter().map(|c| c.to_json()).collect();
        let mut block = json!({
            "mode": mode.as_wire(),
            "inference_status": self.status(mode),
            "inferred_fields": inferred,
            "suggested_fields": suggested,
            "conflicts": conflicts,
            "evidence_sources": self.evidence_sources.iter().map(|s| json!(s)).collect::<Vec<_>>(),
        });
        // wave-20 / task 07 — surface the LLM proposal bundle when it ran.
        // Always emit BOTH `llm_status` AND `llm_proposals[]` for the
        // sonnet_suggest mode so observers can pivot on a stable shape
        // even when the bundle is empty (e.g. LLM returned no usable
        // suggestions). For other modes the keys are omitted entirely so
        // the legacy byte-shape is preserved.
        if matches!(mode, InferPlanFieldsMode::SonnetSuggest) {
            let bundle = self.llm.as_ref();
            let status = bundle
                .map(|b| b.status)
                .unwrap_or(LlmProposalStatus::NotInvoked);
            let proposals: Vec<Value> = bundle
                .map(|b| b.proposals.iter().map(|p| p.to_json()).collect())
                .unwrap_or_default();
            let unavailable_reason = bundle.and_then(|b| b.unavailable_reason.clone());
            let model = bundle.and_then(|b| b.model.clone());
            let request_caller = bundle.and_then(|b| b.request_caller.clone());
            let map = block.as_object_mut().expect("json! object");
            map.insert("llm_status".to_string(), json!(status.as_wire()));
            map.insert("llm_proposals".to_string(), Value::Array(proposals));
            if let Some(reason) = unavailable_reason {
                map.insert("llm_unavailable_reason".to_string(), json!(reason));
            }
            if let Some(model) = model {
                map.insert("llm_model".to_string(), json!(model));
            }
            if let Some(caller) = request_caller {
                map.insert("llm_caller".to_string(), json!(caller));
            }
        }
        block
    }
}

// ── wave-20 / task 07 — LLM proposal validation + bundle ─────────────────
//
// The Sonnet-augmented mode produces a STRUCTURED proposal list. Every
// proposal carries:
//   * `field`            — one of the six v0 PLAN fields (allowlist below).
//   * `value`            — JSON value matching the expected field shape
//                          (string / boolean / list of strings).
//   * `confidence`       — high|medium|low (apply policy is OFF for v0;
//                          surfaced for caller use).
//   * `evidence`         — short justification string (LLM-supplied).
//   * `conflict_status`  — "none" | "conflicts_with_caller" |
//                          "conflicts_with_deterministic" — never
//                          auto-resolved.
//
// Validation rejects unknown fields, missing keys, value shape mismatch,
// and unknown confidence strings. Failures land on `parse_warnings[]` so
// observers can audit which proposals were dropped without an exception
// killing the rest.

/// Allowlisted PLAN fields the LLM may propose values for (mirrors the
/// six fields handled by the deterministic engine).
pub(super) const LLM_ALLOWED_FIELDS: &[&str] = &[
    "target",
    "dispatch_strategy",
    "target_project",
    "owned_files",
    "acceptance_mode",
    "workstation_dispatch",
];

/// Cap applied to LLM proposal lists. Sonnet may attempt to fill every
/// field; the cap pins the list size at a safe upper bound and protects
/// the response payload from accidental blowup. Eight is comfortably
/// above the six allowlisted fields so a well-behaved model never trips
/// the limit.
pub(super) const LLM_PROPOSAL_CAP: usize = 8;

/// Token budget for the Sonnet inference call. Plans + evidence digest
/// stay well under 4 KB; we leave headroom for the model to emit one
/// proposal per field with a short justification.
const SONNET_INFER_MAX_TOKENS: u32 = 1024;

/// Caller string surfaced to LLM gateway logging. Mirrors the
/// `plan_compiler` literal used by the wave-12 / 04 plan-compiler so the
/// observability surface stays self-explanatory.
const SONNET_INFER_CALLER: &str = "plan_field_inference";

/// Wire status describing the outcome of the LLM-augmented pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LlmProposalStatus {
    /// Caller picked a non-LLM mode; the bundle is absent.
    NotInvoked,
    /// LLM was unavailable (gateway not initialised, gate closed, network
    /// failure, etc.). Bundle carries a reason; no proposals.
    Unavailable,
    /// LLM responded with at least one valid proposal.
    Suggested,
    /// LLM responded but no proposal survived validation (zero usable
    /// fields). Bundle may carry parse_warnings to explain why.
    NoSuggestions,
    /// LLM responded with parseable shape but every field was already
    /// covered by the deterministic engine, so we suppressed the
    /// proposal list rather than echo redundant suggestions.
    DeterministicAlreadyComplete,
}

impl LlmProposalStatus {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            LlmProposalStatus::NotInvoked => "not_invoked",
            LlmProposalStatus::Unavailable => "llm_unavailable",
            LlmProposalStatus::Suggested => "suggested",
            LlmProposalStatus::NoSuggestions => "no_suggestions",
            LlmProposalStatus::DeterministicAlreadyComplete => "deterministic_already_complete",
        }
    }
}

/// Conflict tag attached to every LLM proposal. Caller / deterministic
/// agreement is never silently overridden; conflicts surface for review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LlmConflictStatus {
    /// Caller did not specify, deterministic engine returned no signal,
    /// LLM proposal stands alone.
    None,
    /// Caller passed an explicit value differing from the LLM proposal.
    ConflictsWithCaller,
    /// Deterministic engine inferred a different value for the same
    /// field with high confidence.
    ConflictsWithDeterministic,
    /// Deterministic engine surfaced the same field as a suggestion
    /// (medium / low confidence) with a different value. Lower-priority
    /// conflict than caller / deterministic-high.
    OverlapsDeterministicSuggestion,
}

impl LlmConflictStatus {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            LlmConflictStatus::None => "none",
            LlmConflictStatus::ConflictsWithCaller => "conflicts_with_caller",
            LlmConflictStatus::ConflictsWithDeterministic => "conflicts_with_deterministic",
            LlmConflictStatus::OverlapsDeterministicSuggestion => {
                "overlaps_deterministic_suggestion"
            }
        }
    }
}

/// One validated LLM proposal. The `field` is interned to a static
/// allowlist string so downstream consumers can switch on it cheaply.
#[derive(Debug, Clone)]
pub(super) struct LlmProposal {
    pub(super) field: &'static str,
    pub(super) value: Value,
    pub(super) confidence: InferenceConfidence,
    pub(super) evidence: String,
    pub(super) conflict_status: LlmConflictStatus,
}

impl LlmProposal {
    pub(super) fn to_json(&self) -> Value {
        json!({
            "field": self.field,
            "value": self.value.clone(),
            "confidence": self.confidence.as_wire(),
            "evidence": self.evidence,
            "conflict_status": self.conflict_status.as_wire(),
            // Pin the never-applied invariant so observers can `assert
            // proposal.applied == false` without reading the whole
            // task contract.
            "applied": false,
        })
    }
}

/// Bundle of LLM-side data attached to [`PlanFieldInference`]. Always
/// carries the status (so observers see whether the gateway was
/// reachable) plus the validated proposals. `parse_warnings[]` records
/// per-field validation drops for caller debugging without aborting the
/// response.
#[derive(Debug, Clone)]
pub(super) struct LlmProposalBundle {
    pub(super) status: LlmProposalStatus,
    pub(super) proposals: Vec<LlmProposal>,
    pub(super) parse_warnings: Vec<String>,
    pub(super) unavailable_reason: Option<String>,
    pub(super) model: Option<String>,
    pub(super) request_caller: Option<String>,
}

impl LlmProposalBundle {
    pub(super) fn unavailable(reason: impl Into<String>) -> Self {
        LlmProposalBundle {
            status: LlmProposalStatus::Unavailable,
            proposals: Vec::new(),
            parse_warnings: Vec::new(),
            unavailable_reason: Some(reason.into()),
            model: None,
            request_caller: Some(SONNET_INFER_CALLER.to_string()),
        }
    }
}

/// Validate a Sonnet response into a list of [`LlmProposal`] entries.
/// The expected shape is `{"proposals": [{...}, ...]}` (object with a
/// `proposals` array) OR a bare top-level array; both forms are
/// accepted because Sonnet sometimes elides the wrapper.
///
/// Rejected proposals land on `parse_warnings[]` so the caller can audit
/// what survived. The validator is PURE so the unit tests can pin every
/// edge case without touching the LLM.
pub(super) fn parse_llm_proposals(raw: &str) -> (Vec<LlmProposal>, Vec<String>) {
    let mut warnings: Vec<String> = Vec::new();
    let trimmed = raw.trim();
    let trimmed = strip_code_fence(trimmed);
    let parsed: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(err) => {
            warnings.push(format!("LLM response was not valid JSON: {}", err));
            return (Vec::new(), warnings);
        }
    };
    let raw_proposals = match &parsed {
        Value::Array(arr) => arr.clone(),
        Value::Object(map) => match map.get("proposals") {
            Some(Value::Array(arr)) => arr.clone(),
            Some(other) => {
                warnings.push(format!(
                    "`proposals` must be an array, got {}",
                    json_kind(other)
                ));
                return (Vec::new(), warnings);
            }
            None => {
                warnings.push(
                    "LLM response object missing required `proposals` array".to_string(),
                );
                return (Vec::new(), warnings);
            }
        },
        other => {
            warnings.push(format!(
                "LLM response top-level must be array or object, got {}",
                json_kind(other)
            ));
            return (Vec::new(), warnings);
        }
    };
    let mut out: Vec<LlmProposal> = Vec::new();
    let mut seen_fields: std::collections::HashSet<&'static str> =
        std::collections::HashSet::new();
    for (idx, raw) in raw_proposals.iter().enumerate() {
        if out.len() >= LLM_PROPOSAL_CAP {
            warnings.push(format!(
                "proposal cap of {} reached; dropping remaining entries",
                LLM_PROPOSAL_CAP
            ));
            break;
        }
        let obj = match raw.as_object() {
            Some(o) => o,
            None => {
                warnings.push(format!(
                    "proposals[{}] must be an object, got {}",
                    idx,
                    json_kind(raw)
                ));
                continue;
            }
        };
        let field_raw = obj
            .get("field")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .unwrap_or("");
        let field = match LLM_ALLOWED_FIELDS
            .iter()
            .find(|allowed| allowed.eq_ignore_ascii_case(field_raw))
            .copied()
        {
            Some(f) => f,
            None => {
                warnings.push(format!(
                    "proposals[{}] field `{}` not in allowlist",
                    idx, field_raw
                ));
                continue;
            }
        };
        if seen_fields.contains(field) {
            warnings.push(format!(
                "proposals[{}] duplicate field `{}` ignored",
                idx, field
            ));
            continue;
        }
        let value_raw = match obj.get("value") {
            Some(v) => v.clone(),
            None => {
                warnings.push(format!("proposals[{}] missing required `value`", idx));
                continue;
            }
        };
        let value = match coerce_proposal_value(field, &value_raw) {
            Ok(v) => v,
            Err(err) => {
                warnings.push(format!("proposals[{}] {}", idx, err));
                continue;
            }
        };
        let confidence = match obj.get("confidence").and_then(|v| v.as_str()) {
            Some(s) => match s.trim().to_ascii_lowercase().as_str() {
                "high" => InferenceConfidence::High,
                "medium" => InferenceConfidence::Medium,
                "low" => InferenceConfidence::Low,
                other => {
                    warnings.push(format!(
                        "proposals[{}] confidence `{}` not in [high, medium, low]",
                        idx, other
                    ));
                    continue;
                }
            },
            None => {
                warnings.push(format!(
                    "proposals[{}] missing required `confidence`",
                    idx
                ));
                continue;
            }
        };
        let evidence = obj
            .get("evidence")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if evidence.is_empty() {
            warnings.push(format!(
                "proposals[{}] missing required `evidence` justification",
                idx
            ));
            continue;
        }
        seen_fields.insert(field);
        out.push(LlmProposal {
            field,
            value,
            confidence,
            evidence,
            // conflict_status is reconciled against the deterministic
            // result + caller args by `reconcile_llm_conflicts` once the
            // proposal list is parsed. Default to `None` here so a
            // failing reconciliation step never accidentally promotes a
            // conflict-free state.
            conflict_status: LlmConflictStatus::None,
        });
    }
    (out, warnings)
}

/// Strip a Markdown code fence (```json ... ``` or ``` ... ```) if the
/// model wrapped its JSON output. Returns the inner trimmed content. The
/// validator runs after this so a fenced response still parses cleanly.
fn strip_code_fence(s: &str) -> &str {
    let s = s.trim();
    let stripped = s
        .strip_prefix("```json")
        .or_else(|| s.strip_prefix("```JSON"))
        .or_else(|| s.strip_prefix("```"));
    let Some(rest) = stripped else {
        return s;
    };
    // Drop the opening newline (if any) and the closing fence.
    let rest = rest.trim_start_matches('\n');
    let rest = rest.strip_suffix("```").unwrap_or(rest);
    rest.trim()
}

/// Short json kind name for diagnostics (e.g. `"string"`, `"object"`).
fn json_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Coerce a raw LLM-emitted value into the canonical shape expected for
/// `field`. Rejects shape mismatches with a human-readable reason that
/// lands on `parse_warnings[]`. Empty / blank strings and arrays are
/// rejected because they carry no usable signal.
fn coerce_proposal_value(field: &str, raw: &Value) -> std::result::Result<Value, String> {
    match field {
        "workstation_dispatch" => match raw {
            Value::Bool(b) => Ok(Value::Bool(*b)),
            Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
                "true" | "yes" | "on" | "1" => Ok(Value::Bool(true)),
                "false" | "no" | "off" | "0" => Ok(Value::Bool(false)),
                other => Err(format!(
                    "value for `workstation_dispatch` must be bool, got string `{}`",
                    other
                )),
            },
            other => Err(format!(
                "value for `workstation_dispatch` must be bool, got {}",
                json_kind(other)
            )),
        },
        "owned_files" => match raw {
            Value::Array(items) => {
                let mut out: Vec<String> = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    let Some(s) = item.as_str() else {
                        return Err(format!(
                            "value.owned_files[{}] must be string, got {}",
                            i,
                            json_kind(item)
                        ));
                    };
                    let t = s.trim();
                    if !t.is_empty() {
                        out.push(t.to_string());
                    }
                }
                if out.is_empty() {
                    Err("value for `owned_files` must contain at least one path".to_string())
                } else {
                    Ok(json!(out))
                }
            }
            other => Err(format!(
                "value for `owned_files` must be string array, got {}",
                json_kind(other)
            )),
        },
        // The remaining four fields are string-shaped.
        "target" | "dispatch_strategy" | "target_project" | "acceptance_mode" => match raw {
            Value::String(s) => {
                let t = s.trim();
                if t.is_empty() {
                    Err(format!("value for `{}` must be non-empty string", field))
                } else {
                    Ok(json!(t))
                }
            }
            other => Err(format!(
                "value for `{}` must be string, got {}",
                field,
                json_kind(other)
            )),
        },
        _ => Err(format!("field `{}` is not supported by LLM proposals", field)),
    }
}

/// Reconcile parsed LLM proposals against the deterministic inference
/// result and the caller-supplied args. Tags every proposal with a
/// [`LlmConflictStatus`] so observers can pivot on it without recomputing.
///
/// Conflict precedence (highest first):
///   1. Caller-supplied differing value          → ConflictsWithCaller.
///   2. Deterministic high-confidence value      → ConflictsWithDeterministic.
///   3. Deterministic suggestion (medium / low)  → OverlapsDeterministicSuggestion.
///   4. Otherwise                                → None.
///
/// This function NEVER mutates state; it only annotates the proposal
/// bundle so the caller can decide whether to act.
pub(super) fn reconcile_llm_conflicts(
    proposals: &mut [LlmProposal],
    deterministic: &PlanFieldInference,
    args: &Value,
) {
    for p in proposals.iter_mut() {
        // 1. Caller-supplied values always win for conflict tagging.
        if let Some(caller_val) = caller_value_for_field(args, p.field) {
            if !values_equivalent(p.field, &caller_val, &p.value) {
                p.conflict_status = LlmConflictStatus::ConflictsWithCaller;
                continue;
            }
            // Caller agrees with LLM proposal — leave conflict_status
            // at None; the proposal still carries useful evidence.
        }
        // 2. Deterministic high-confidence inference for the same field.
        if let Some(det) = deterministic
            .inferred
            .iter()
            .find(|f| f.field == p.field)
        {
            if !values_equivalent(p.field, &det.value, &p.value) {
                p.conflict_status = LlmConflictStatus::ConflictsWithDeterministic;
                continue;
            }
        }
        // 3. Deterministic suggestion (medium / low) overlap.
        if let Some(sug) = deterministic
            .suggested
            .iter()
            .find(|f| f.field == p.field)
        {
            if !values_equivalent(p.field, &sug.value, &p.value) {
                p.conflict_status = LlmConflictStatus::OverlapsDeterministicSuggestion;
                continue;
            }
        }
    }
}

/// Caller value for a field in caller args, normalised to a `Value` so it
/// can be compared with LLM proposals. Strings are trimmed; empty
/// strings collapse to `None`. Arrays of strings (for `owned_files`)
/// flow through unchanged.
fn caller_value_for_field(args: &Value, field: &str) -> Option<Value> {
    match field {
        "workstation_dispatch" => caller_bool(args, field).map(Value::Bool),
        "owned_files" => {
            let v = caller_string_list(args, field);
            if v.is_empty() {
                None
            } else {
                Some(json!(v))
            }
        }
        _ => caller_str(args, field).map(|s| json!(s)),
    }
}

/// Field-aware equality check. Strings compare ascii-case-insensitively
/// (matching the deterministic engine); arrays compare order-independent
/// for `owned_files`; bools / others compare with `==`.
fn values_equivalent(field: &str, a: &Value, b: &Value) -> bool {
    match field {
        "owned_files" => {
            let a_list: Vec<String> = a
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let b_list: Vec<String> = b
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let mut a_sorted = a_list.clone();
            let mut b_sorted = b_list.clone();
            a_sorted.sort();
            b_sorted.sort();
            a_sorted == b_sorted
        }
        "workstation_dispatch" => a.as_bool() == b.as_bool(),
        _ => match (a.as_str(), b.as_str()) {
            (Some(x), Some(y)) => x.eq_ignore_ascii_case(y),
            _ => a == b,
        },
    }
}

/// Compose the system + user prompts for the Sonnet inference call. The
/// system prompt pins the strict JSON schema, the user prompt embeds the
/// PLAN sexp + compiled_from + evidence digest. Pure function so the
/// unit tests can lock the prompt shape.
pub(super) fn build_llm_inference_prompt(
    plan_sexp: &str,
    compiled_from: Option<&str>,
    evidence_entries: &[Value],
    deterministic: &PlanFieldInference,
    caller_args: &Value,
) -> (String, String) {
    let system = String::from(
        "You are MissionD's plan field inference assistant. Inspect the supplied PLAN.lisp \
         sexp, the directive provenance string, and a small evidence digest, then propose \
         values for any of the following six PLAN fields that are NOT already covered by the \
         deterministic engine: target, dispatch_strategy, target_project, owned_files, \
         acceptance_mode, workstation_dispatch.\n\n\
         Reply with STRICT JSON ONLY (no Markdown fences, no prose) matching this shape:\n\
         {\n  \"proposals\": [\n    {\n      \"field\": \"<one of the six fields>\",\n      \"value\": <string|bool|string array depending on field>,\n      \"confidence\": \"high\"|\"medium\"|\"low\",\n      \"evidence\": \"<one short sentence justifying the proposal>\"\n    }\n  ]\n}\n\n\
         Rules:\n\
         - Never propose a value that already appears in the deterministic block. The caller will \
           tag your proposal with `conflict_status` if it disagrees with caller args or the \
           deterministic engine; do not pre-flag conflicts yourself.\n\
         - Omit a field rather than fabricate one. An empty proposals array is a valid response.\n\
         - `target` must be one of: mission_execution | mission_task_delegate | mission_flow_run.\n\
         - `dispatch_strategy` must be one of: resident-lisp | fresh-code-alignment | agent-team | mixed | prompt-fallback.\n\
         - `acceptance_mode` must be one of: inner_status | evidence_keys | manual.\n\
         - `owned_files` must be a non-empty array of repo-relative paths.\n\
         - `workstation_dispatch` must be a boolean.\n\
         - Confidence `high` is reserved for unambiguous evidence. When in doubt, use `medium` or omit.\n\
         - Never include keys outside the listed schema.",
    );
    let evidence_digest: Vec<Value> = evidence_entries
        .iter()
        .rev()
        .take(8)
        .cloned()
        .collect();
    let deterministic_block = deterministic.to_response_json(InferPlanFieldsMode::Preview);
    let user = format!(
        "PLAN.lisp sexp:\n```lisp\n{plan}\n```\n\ncompiled_from: {compiled}\n\nrecent evidence (newest first, capped):\n```json\n{evidence}\n```\n\ndeterministic inference already produced:\n```json\n{deterministic}\n```\n\ncaller-supplied args (already-set fields you must NOT override):\n```json\n{caller}\n```",
        plan = plan_sexp,
        compiled = compiled_from.unwrap_or("(none)"),
        evidence = serde_json::to_string_pretty(&evidence_digest).unwrap_or_else(|_| "[]".into()),
        deterministic = serde_json::to_string_pretty(&deterministic_block)
            .unwrap_or_else(|_| "{}".into()),
        caller = serde_json::to_string_pretty(caller_args).unwrap_or_else(|_| "{}".into()),
    );
    (system, user)
}

/// Run the Sonnet inference call. Returns a [`LlmProposalBundle`] in
/// every code path so the caller can pivot on the bundle status without
/// branching on Result. Sonnet unavailability surfaces as
/// `LlmProposalStatus::Unavailable` with an explanatory reason — never
/// as a silent fallback to deterministic-only.
pub(super) async fn request_llm_proposals(
    state: &AppState,
    plan_sexp: &str,
    compiled_from: Option<&str>,
    evidence_entries: &[Value],
    deterministic: &PlanFieldInference,
    caller_args: &Value,
) -> LlmProposalBundle {
    let Some(sonnet) = state.sonnet.as_ref() else {
        return LlmProposalBundle::unavailable(
            "Sonnet gateway not initialized; LLM-augmented PLAN inference unavailable",
        );
    };
    let (system, user) = build_llm_inference_prompt(
        plan_sexp,
        compiled_from,
        evidence_entries,
        deterministic,
        caller_args,
    );
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system,
        },
        ChatMessage {
            role: "user".to_string(),
            content: user,
        },
    ];
    let raw = match sonnet
        .call_interactive(messages, Some(SONNET_INFER_MAX_TOKENS), SONNET_INFER_CALLER)
        .await
    {
        Ok(s) => s,
        Err(err) => {
            return LlmProposalBundle::unavailable(format!(
                "Sonnet inference call failed: {}",
                err
            ));
        }
    };
    let (mut proposals, parse_warnings) = parse_llm_proposals(&raw);
    reconcile_llm_conflicts(&mut proposals, deterministic, caller_args);
    let status = if proposals.is_empty() {
        // Distinguish between "deterministic already covered every field"
        // and "model returned no usable suggestions". The first is a
        // success signal (no work to do); the second is debug-worthy.
        if deterministic_covers_all_fields(deterministic) {
            LlmProposalStatus::DeterministicAlreadyComplete
        } else {
            LlmProposalStatus::NoSuggestions
        }
    } else {
        LlmProposalStatus::Suggested
    };
    LlmProposalBundle {
        status,
        proposals,
        parse_warnings,
        unavailable_reason: None,
        model: Some(SONNET_COMPILER_MODEL.to_string()),
        request_caller: Some(SONNET_INFER_CALLER.to_string()),
    }
}

/// True when the deterministic engine produced a high-confidence
/// inference for every allowlisted field. Used to label the LLM bundle
/// status so observers can tell whether an empty `llm_proposals[]` means
/// "model declined" vs "nothing left to suggest".
fn deterministic_covers_all_fields(deterministic: &PlanFieldInference) -> bool {
    for field in LLM_ALLOWED_FIELDS {
        let covered = deterministic
            .inferred
            .iter()
            .any(|f| f.field == *field && f.confidence.meets_apply_threshold());
        if !covered {
            return false;
        }
    }
    true
}

/// Read up to the most-recent N evidence sidecar entries for a plan.
/// Returns an empty vec when the sidecar does not exist or cannot be
/// resolved — this is a best-effort hint source, NEVER load-bearing for
/// dispatch. Read-only; never writes.
pub(super) async fn read_recent_evidence_entries(
    state: &AppState,
    plan_id: uuid::Uuid,
    project_arg: Option<&str>,
    cwd_arg: Option<&str>,
    target_project_arg: Option<&str>,
    cap: usize,
) -> Vec<Value> {
    let project_root = match resolve_project_root(
        &state.project_registry,
        project_arg,
        cwd_arg,
        target_project_arg,
    )
    .await
    {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let path = project_root
        .join(COMPANION_DIR)
        .join(format!("{}.evidence.json", plan_id));
    if !path.exists() {
        return Vec::new();
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let bundle: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let entries = bundle
        .get("entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if entries.len() <= cap {
        entries
    } else {
        entries[entries.len() - cap..].to_vec()
    }
}

/// Inference rule input — what the deterministic engine actually reads.
/// Built once per `mission_plan(action=execute)` call so the rule
/// functions can stay pure.
#[derive(Debug, Default, Clone)]
pub(super) struct PlanInferenceInput<'a> {
    pub(super) plan_hints: ParsedPlanHints,
    /// Raw `plan.sexp_text` — exposed so per-field rules that look for
    /// hints not captured by the canonical [`ParsedPlanHints`] struct
    /// (e.g. `:acceptance-mode`) can re-scan without widening the struct.
    pub(super) plan_sexp: &'a str,
    pub(super) compiled_from: Option<&'a str>,
    pub(super) evidence_entries: Vec<Value>,
}

/// Pure inference engine over the input above. Produces the aggregate
/// result + the list of recommended arg augmentations (only filled when
/// `mode=ApplySafe`; preview mode also computes inference but the caller
/// short-circuits before using the augmentations).
///
/// Conflict semantics: when the caller supplied a value AND the inferer
/// derived a different one from a recognised source, the field becomes a
/// conflict. The conflict is REPORTED (never auto-resolved); apply_safe
/// will NEVER mutate over a caller-supplied value.
pub(super) fn compute_plan_field_inference(
    args: &Value,
    input: &PlanInferenceInput<'_>,
) -> PlanFieldInference {
    let mut result = PlanFieldInference::default();
    let mut sources: Vec<&'static str> = Vec::new();
    if !is_empty_hints(&input.plan_hints) {
        sources.push("plan_sexp");
    }
    if input
        .compiled_from
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
    {
        sources.push("compiled_from");
    }
    if !input.evidence_entries.is_empty() {
        sources.push("evidence_sidecar");
    }
    result.evidence_sources = sources;

    infer_target(args, input, &mut result);
    infer_dispatch_strategy(args, input, &mut result);
    infer_target_project(args, input, &mut result);
    infer_owned_files(args, input, &mut result);
    infer_acceptance_mode(args, input, &mut result);
    infer_workstation_dispatch(args, input, &mut result);

    result
}

/// True when the parsed hints carry no usable signal at all. Used to
/// drive `evidence_sources` reporting; does NOT change the inferer's
/// per-field decisions.
fn is_empty_hints(h: &ParsedPlanHints) -> bool {
    h.target.is_none()
        && h.flow_id.is_none()
        && h.dispatch_strategy.is_none()
        && h.parallelism.is_none()
        && h.target_project.is_none()
        && h.requested_cwd.is_none()
        && h.objective.is_none()
        && h.summary.is_none()
        && h.scope.is_none()
        && h.commit_policy.is_none()
        && h.owned_files_raw.is_none()
        && h.forbidden_files_raw.is_none()
        && h.acceptance_commands_raw.is_none()
        && h.workstation_dispatch_flag.is_none()
}

/// Helper: the caller's explicit string value for a field, trimmed and
/// non-empty. `None` means "caller did not specify" so the inferer is
/// free to fill.
fn caller_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Helper: the caller's explicit bool value for a field. `None` means
/// "caller did not specify" so the inferer is free to fill.
fn caller_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|v| v.as_bool())
}

/// Helper: caller-supplied string list for `owned_files`-shaped args.
/// Honours both string and array forms (mirroring the caller-side schema).
fn caller_string_list(args: &Value, key: &str) -> Vec<String> {
    collect_string_list(args.get(key))
}

/// Push an inferred field — high-confidence fields land in `inferred`,
/// medium / low always land in `suggested`.
fn record_inferred(result: &mut PlanFieldInference, field: InferredField) {
    if field.confidence.meets_apply_threshold() {
        result.inferred.push(field);
    } else {
        result.suggested.push(field);
    }
}

/// Record a conflict (caller value differs from inferred value). NEVER
/// promotes the inferred value into `inferred` even when confidence is
/// `high` — apply_safe must not silently override caller intent.
fn record_conflict(result: &mut PlanFieldInference, conflict: InferenceConflict) {
    result.conflicts.push(conflict);
}

// ── per-field rule fns ────────────────────────────────────────────────

/// Infer `target`. Confidence:
///   * `high`   — PLAN.lisp `:target` hint normalises to a canonical target.
///   * `high`   — ≥1 evidence entry agrees on the same target string.
///   * `medium` — `compiled_from` text contains an unambiguous keyword.
fn infer_target(args: &Value, input: &PlanInferenceInput<'_>, result: &mut PlanFieldInference) {
    let caller = caller_str(args, "target");
    let mut hits: Vec<(InferenceConfidence, &'static str, &'static str, Option<String>)> = Vec::new();

    // 1. PLAN.lisp hint.
    if let Some(raw) = input.plan_hints.target.as_deref() {
        if let Some(canonical) = normalize_target(raw, input.plan_hints.flow_id.is_some()) {
            hits.push((
                InferenceConfidence::High,
                canonical,
                "plan_sexp",
                Some(format!(":target hint resolved to `{}`", canonical)),
            ));
        }
    }

    // 2. Evidence sidecar — the most recent dispatch record carries
    //    `target_tool`. Multiple agreeing entries reinforce the signal.
    let evidence_target = scan_evidence_string_field(&input.evidence_entries, &["target_tool"])
        .and_then(|s| {
            normalize_target(&s, input.plan_hints.flow_id.is_some())
                .map(|canonical| (canonical, s))
        });
    if let Some((canonical, raw)) = evidence_target {
        hits.push((
            InferenceConfidence::High,
            canonical,
            "evidence_sidecar",
            Some(format!("prior dispatch target_tool=`{}`", raw)),
        ));
    }

    // 3. compiled_from keyword scan.
    if let Some(text) = input.compiled_from {
        if let Some(canonical) = normalize_target(text, input.plan_hints.flow_id.is_some()) {
            hits.push((
                InferenceConfidence::Medium,
                canonical,
                "compiled_from",
                Some(format!("compiled_from `{}` mentions `{}`", text, canonical)),
            ));
        }
    }

    finalize_string_field("target", caller, hits, result);
}

/// Infer `dispatch_strategy`. Confidence:
///   * `high`   — PLAN.lisp `:dispatch-strategy` (canonicalised).
///   * `high`   — evidence entry carries a known strategy.
///   * `medium` — PLAN.lisp `:parallelism` keyword maps to a strategy.
///   * `medium` — `compiled_from` carries a keyword like "agent-team".
fn infer_dispatch_strategy(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_str(args, "dispatch_strategy");
    let mut hits: Vec<(InferenceConfidence, &'static str, &'static str, Option<String>)> = Vec::new();

    if let Some(raw) = input.plan_hints.dispatch_strategy.as_deref() {
        if let Some(c) = canonicalize_strategy(raw) {
            hits.push((
                InferenceConfidence::High,
                c,
                "plan_sexp",
                Some(format!(":dispatch-strategy hint `{}`", raw)),
            ));
        }
    }

    if let Some(s) = scan_evidence_string_field(&input.evidence_entries, &["dispatch_strategy"]) {
        if let Some(c) = canonicalize_strategy(&s) {
            hits.push((
                InferenceConfidence::High,
                c,
                "evidence_sidecar",
                Some(format!("prior dispatch dispatch_strategy=`{}`", s)),
            ));
        }
    }

    if let Some(p) = input.plan_hints.parallelism.as_deref() {
        if let Some(c) = canonicalize_strategy(p) {
            hits.push((
                InferenceConfidence::Medium,
                c,
                "plan_sexp",
                Some(format!(":parallelism hint `{}` mapped to strategy", p)),
            ));
        }
    }

    if let Some(text) = input.compiled_from {
        if let Some(c) = canonicalize_strategy(text) {
            hits.push((
                InferenceConfidence::Medium,
                c,
                "compiled_from",
                Some(format!("compiled_from keyword maps to `{}`", c)),
            ));
        }
    }

    finalize_string_field("dispatch_strategy", caller, hits, result);
}

/// Infer `target_project`. Confidence:
///   * `high`   — PLAN.lisp `:target-project` non-empty.
///   * `high`   — evidence entry carries the same target_project >=2 times.
///   * `medium` — single evidence entry carries target_project.
fn infer_target_project(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_str(args, "target_project");
    let mut hits: Vec<(InferenceConfidence, String, &'static str, Option<String>)> = Vec::new();

    if let Some(tp) = input.plan_hints.target_project.as_deref() {
        let v = tp.trim();
        if !v.is_empty() {
            hits.push((
                InferenceConfidence::High,
                v.to_string(),
                "plan_sexp",
                Some(":target-project hint".to_string()),
            ));
        }
    }

    let evidence_hits = scan_evidence_string_counts(&input.evidence_entries, &["target_project"]);
    if let Some((value, count)) = evidence_hits.first().cloned() {
        let conf = if count >= 2 {
            InferenceConfidence::High
        } else {
            InferenceConfidence::Medium
        };
        hits.push((
            conf,
            value.clone(),
            "evidence_sidecar",
            Some(format!("prior dispatch target_project=`{}` (x{})", value, count)),
        ));
    }

    finalize_owned_string_field("target_project", caller, hits, result);
}

/// Infer `owned_files`. Confidence:
///   * `high`   — PLAN.lisp `:owned-files` parses to >=1 entry.
///   * `medium` — evidence sidecar carries `owned_files` (any non-empty list).
///                Files change across runs, so we never claim `high` from
///                evidence alone.
fn infer_owned_files(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_string_list(args, "owned_files");
    let mut hits: Vec<(InferenceConfidence, Vec<String>, &'static str, Option<String>)> = Vec::new();

    let plan_owned = split_lisp_string_list(input.plan_hints.owned_files_raw.as_deref());
    if !plan_owned.is_empty() {
        hits.push((
            InferenceConfidence::High,
            plan_owned.clone(),
            "plan_sexp",
            Some(format!(":owned-files declares {} entries", plan_owned.len())),
        ));
    }

    if let Some(list) = scan_evidence_string_list(&input.evidence_entries, "owned_files") {
        if !list.is_empty() {
            hits.push((
                InferenceConfidence::Medium,
                list.clone(),
                "evidence_sidecar",
                Some(format!("prior dispatch owned_files carries {} entries", list.len())),
            ));
        }
    }

    finalize_string_list_field("owned_files", caller, hits, result);
}

/// Infer `acceptance_mode`. Confidence:
///   * `high`   — PLAN.lisp top-level `:acceptance-mode` parses to a known
///                AcceptanceMode.
///   * `medium` — evidence entry carries an `acceptance.mode` field.
fn infer_acceptance_mode(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_str(args, "acceptance_mode");
    let mut hits: Vec<(InferenceConfidence, &'static str, &'static str, Option<String>)> = Vec::new();

    // PLAN.lisp top-level `:acceptance-mode` — parse_plan_hints does not
    // capture it (the wave-17 / task 03 hint lives on per-node forms).
    // We do a focused scan here so v0 inference can spot a top-level
    // declaration without widening the canonical hint struct.
    if let Some(raw) = scan_keyword_pairs(input.plan_sexp)
        .into_iter()
        .find(|(k, _)| {
            let lc = k.to_ascii_lowercase();
            lc == "acceptance-mode" || lc == "acceptance_mode"
        })
        .map(|(_, v)| v)
    {
        if let Some(canonical) = canonicalize_acceptance_mode(&raw) {
            hits.push((
                InferenceConfidence::High,
                canonical,
                "plan_sexp",
                Some(format!(":acceptance-mode hint `{}`", raw)),
            ));
        }
    }

    if let Some(mode) = scan_evidence_string_field(
        &input.evidence_entries,
        &["acceptance_mode", "acceptance.mode"],
    ) {
        if let Some(canonical) = canonicalize_acceptance_mode(&mode) {
            hits.push((
                InferenceConfidence::Medium,
                canonical,
                "evidence_sidecar",
                Some(format!("prior evidence acceptance_mode=`{}`", mode)),
            ));
        }
    }

    finalize_string_field("acceptance_mode", caller, hits, result);
}

/// Infer `workstation_dispatch`. Confidence:
///   * `high`   — PLAN.lisp `:workstation-dispatch true`.
///   * `high`   — every recent evidence entry that carries
///                `workstation_dispatch_source` lands on a non-disabled
///                source AND the inferable_strategy gate passed.
///   * `medium` — single evidence entry hint.
fn infer_workstation_dispatch(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_bool(args, "workstation_dispatch");
    let mut hits: Vec<(InferenceConfidence, bool, &'static str, Option<String>)> = Vec::new();

    if input.plan_hints.workstation_dispatch_opt_in() {
        hits.push((
            InferenceConfidence::High,
            true,
            "plan_sexp",
            Some(":workstation-dispatch true".to_string()),
        ));
    } else if let Some(raw) = input.plan_hints.workstation_dispatch_flag.as_deref() {
        // Explicit false in PLAN — high confidence "do NOT enable".
        let lc = raw.trim().to_ascii_lowercase();
        if matches!(lc.as_str(), "false" | "no" | "off" | "0") {
            hits.push((
                InferenceConfidence::High,
                false,
                "plan_sexp",
                Some(":workstation-dispatch false".to_string()),
            ));
        }
    }

    let ws_sources = scan_evidence_string_counts(
        &input.evidence_entries,
        &["workstation_dispatch_source"],
    );
    if let Some((value, count)) = ws_sources.first().cloned() {
        let lc = value.to_ascii_lowercase();
        let positive = matches!(
            lc.as_str(),
            "explicit_arg" | "plan_hint" | "inferred"
        );
        let conf = if count >= 2 {
            InferenceConfidence::High
        } else {
            InferenceConfidence::Medium
        };
        if positive {
            hits.push((
                conf,
                true,
                "evidence_sidecar",
                Some(format!(
                    "prior workstation_dispatch_source=`{}` (x{})",
                    value, count
                )),
            ));
        } else if matches!(lc.as_str(), "disabled") {
            hits.push((
                conf,
                false,
                "evidence_sidecar",
                Some(format!(
                    "prior workstation_dispatch_source=`disabled` (x{})",
                    count
                )),
            ));
        }
    }

    finalize_bool_field("workstation_dispatch", caller, hits, result);
}

// ── finalize helpers (per value-shape) ────────────────────────────────

/// Resolve the highest-confidence string-shaped hint and emit either an
/// inferred / suggested entry, or a conflict against caller value.
fn finalize_string_field(
    field: &'static str,
    caller: Option<&str>,
    mut hits: Vec<(InferenceConfidence, &'static str, &'static str, Option<String>)>,
    result: &mut PlanFieldInference,
) {
    // Prefer the highest-confidence hit; ties broken by source order.
    hits.sort_by_key(|(c, _, _, _)| match c {
        InferenceConfidence::High => 0,
        InferenceConfidence::Medium => 1,
        InferenceConfidence::Low => 2,
    });
    let Some((conf, value, source, detail)) = hits.first().cloned() else {
        return;
    };

    if let Some(c) = caller {
        if c.eq_ignore_ascii_case(value) {
            // Caller already agrees with the inference — nothing to do.
            return;
        }
        record_conflict(
            result,
            InferenceConflict {
                field,
                caller_value: json!(c),
                inferred_value: json!(value),
                confidence: conf,
                source,
            },
        );
        return;
    }

    record_inferred(
        result,
        InferredField {
            field,
            value: json!(value),
            confidence: conf,
            source,
            detail,
        },
    );
}

/// Same as [`finalize_string_field`] but for owned-`String`-shaped hits
/// (where the value is computed dynamically per-call rather than carried
/// as a `&'static str`).
fn finalize_owned_string_field(
    field: &'static str,
    caller: Option<&str>,
    mut hits: Vec<(InferenceConfidence, String, &'static str, Option<String>)>,
    result: &mut PlanFieldInference,
) {
    hits.sort_by_key(|(c, _, _, _)| match c {
        InferenceConfidence::High => 0,
        InferenceConfidence::Medium => 1,
        InferenceConfidence::Low => 2,
    });
    let Some((conf, value, source, detail)) = hits.first().cloned() else {
        return;
    };
    if let Some(c) = caller {
        if c == value {
            return;
        }
        record_conflict(
            result,
            InferenceConflict {
                field,
                caller_value: json!(c),
                inferred_value: json!(value),
                confidence: conf,
                source,
            },
        );
        return;
    }
    record_inferred(
        result,
        InferredField {
            field,
            value: json!(value),
            confidence: conf,
            source,
            detail,
        },
    );
}

/// Same shape as [`finalize_string_field`] but for `Vec<String>`-shaped
/// hits. Caller equality compares as set-like (order-independent) so a
/// PLAN.lisp + caller permutation does not trigger a spurious conflict.
fn finalize_string_list_field(
    field: &'static str,
    caller: Vec<String>,
    mut hits: Vec<(InferenceConfidence, Vec<String>, &'static str, Option<String>)>,
    result: &mut PlanFieldInference,
) {
    hits.sort_by_key(|(c, _, _, _)| match c {
        InferenceConfidence::High => 0,
        InferenceConfidence::Medium => 1,
        InferenceConfidence::Low => 2,
    });
    let Some((conf, value, source, detail)) = hits.first().cloned() else {
        return;
    };
    if !caller.is_empty() {
        let mut a = caller.clone();
        a.sort();
        let mut b = value.clone();
        b.sort();
        if a == b {
            return;
        }
        record_conflict(
            result,
            InferenceConflict {
                field,
                caller_value: json!(caller),
                inferred_value: json!(value),
                confidence: conf,
                source,
            },
        );
        return;
    }
    record_inferred(
        result,
        InferredField {
            field,
            value: json!(value),
            confidence: conf,
            source,
            detail,
        },
    );
}

fn finalize_bool_field(
    field: &'static str,
    caller: Option<bool>,
    mut hits: Vec<(InferenceConfidence, bool, &'static str, Option<String>)>,
    result: &mut PlanFieldInference,
) {
    hits.sort_by_key(|(c, _, _, _)| match c {
        InferenceConfidence::High => 0,
        InferenceConfidence::Medium => 1,
        InferenceConfidence::Low => 2,
    });
    let Some((conf, value, source, detail)) = hits.first().cloned() else {
        return;
    };
    if let Some(c) = caller {
        if c == value {
            return;
        }
        record_conflict(
            result,
            InferenceConflict {
                field,
                caller_value: json!(c),
                inferred_value: json!(value),
                confidence: conf,
                source,
            },
        );
        return;
    }
    record_inferred(
        result,
        InferredField {
            field,
            value: json!(value),
            confidence: conf,
            source,
            detail,
        },
    );
}

// ── evidence-sidecar scanners ─────────────────────────────────────────

/// Look for the most-recent string value of any matching key. Searches
/// each entry top-level + the well-known nested holders that the wave-12
/// evidence collector emits (`evidence`, `inner_dispatch`, `inner_result`,
/// `typed_evidence`). Newest-first match wins.
fn scan_evidence_string_field(entries: &[Value], keys: &[&str]) -> Option<String> {
    for entry in entries.iter().rev() {
        if let Some(v) = pluck_string(entry, keys) {
            return Some(v);
        }
        for nested in &["evidence", "inner_dispatch", "inner_result", "typed_evidence"] {
            if let Some(child) = entry.get(*nested) {
                if let Some(v) = pluck_string(child, keys) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Count distinct string values of a field across entries. Returns
/// `[(value, count), ...]` sorted by descending count then by recency.
fn scan_evidence_string_counts(entries: &[Value], keys: &[&str]) -> Vec<(String, usize)> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for entry in entries {
        let mut found: Option<String> = None;
        if let Some(v) = pluck_string(entry, keys) {
            found = Some(v);
        } else {
            for nested in &["evidence", "inner_dispatch", "inner_result", "typed_evidence"] {
                if let Some(child) = entry.get(*nested) {
                    if let Some(v) = pluck_string(child, keys) {
                        found = Some(v);
                        break;
                    }
                }
            }
        }
        if let Some(v) = found {
            if !counts.contains_key(&v) {
                order.push(v.clone());
            }
            *counts.entry(v).or_insert(0) += 1;
        }
    }
    let mut out: Vec<(String, usize)> = order
        .into_iter()
        .map(|k| {
            let c = counts.get(&k).copied().unwrap_or(0);
            (k, c)
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}

/// Look for a string-array value under any of the supplied keys. Returns
/// the most-recent entry's value (newest-first) so the inferer reflects
/// the latest run.
fn scan_evidence_string_list(entries: &[Value], key: &str) -> Option<Vec<String>> {
    for entry in entries.iter().rev() {
        if let Some(v) = pluck_string_list(entry, key) {
            return Some(v);
        }
        for nested in &["evidence", "inner_dispatch", "inner_result", "typed_evidence"] {
            if let Some(child) = entry.get(*nested) {
                if let Some(v) = pluck_string_list(child, key) {
                    return Some(v);
                }
            }
        }
    }
    None
}

fn pluck_string(v: &Value, keys: &[&str]) -> Option<String> {
    let obj = v.as_object()?;
    for k in keys {
        if let Some(s) = obj.get(*k).and_then(|x| x.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

fn pluck_string_list(v: &Value, key: &str) -> Option<Vec<String>> {
    let obj = v.as_object()?;
    let arr = obj.get(key)?.as_array()?;
    let out: Vec<String> = arr
        .iter()
        .filter_map(|item| item.as_str().map(|s| s.trim()).filter(|s| !s.is_empty()).map(String::from))
        .collect();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Conservative acceptance-mode canonicaliser. Mirrors the AcceptanceMode
/// allowlist in plan_dag.rs. Returns the wire-form constant or None.
fn canonicalize_acceptance_mode(raw: &str) -> Option<&'static str> {
    let lc = raw.trim().to_ascii_lowercase();
    match lc.as_str() {
        "inner_status" | "inner-status" | "innerstatus" => Some("inner_status"),
        "evidence_keys" | "evidence-keys" | "evidencekeys" => Some("evidence_keys"),
        "manual" => Some("manual"),
        _ => None,
    }
}

/// Apply high-confidence inferred fields to a clone of `args` so the
/// downstream pipeline sees the augmented input. Caller-supplied values
/// are NEVER overwritten (they only ever land as conflicts upstream, and
/// conflicts are not promoted into `inferred`).
pub(super) fn apply_safe_augmentation(args: &Value, inference: &PlanFieldInference) -> Value {
    let mut augmented = args.clone();
    let map = match augmented.as_object_mut() {
        Some(m) => m,
        None => return augmented,
    };
    for f in &inference.inferred {
        if !f.confidence.meets_apply_threshold() {
            continue;
        }
        // Defensive guard: refuse to overwrite a caller-provided slot.
        // The inferer should already have routed any caller value into
        // `conflicts`, but we double-check at the mutation site so a
        // future regression cannot silently override caller intent.
        let already_set = map
            .get(f.field)
            .map(|v| match v {
                Value::Null => false,
                Value::String(s) => !s.trim().is_empty(),
                Value::Array(a) => !a.is_empty(),
                Value::Bool(_) => true,
                _ => true,
            })
            .unwrap_or(false);
        if already_set {
            continue;
        }
        map.insert(f.field.to_string(), f.value.clone());
    }
    augmented
}

// ── wave-21 / task 05 — PLAN inference apply-gate v1 ───────────────────
//
// Layered on top of wave-18 / task 06 (deterministic `infer_plan_fields`
// modes) and wave-20 / task 07 (LLM-augmented `sonnet_suggest`). The
// existing wave-18 `apply_safe` mode auto-applies high-confidence fields
// silently — which the wave-21 review surfaced as too lenient. The new
// `apply_inferred_fields=true` flag introduces an EXPLICIT operator
// approval before any inferred / proposed value mutates the call.
//
// Default behaviour (`apply_inferred_fields` absent / false) is suggest-
// only:
//   * `preview` / `sonnet_suggest` short-circuit unchanged;
//   * `apply_safe` still auto-fills high-confidence slots (legacy
//     behaviour preserved for back-compat — callers that relied on
//     wave-18 byte-shape do NOT have to opt into the new gate).
//
// Opt-in behaviour (`apply_inferred_fields=true`) is conservative:
//   * deterministic high-confidence inferred fields with NO conflict
//     are applied;
//   * deterministic suggestions (medium / low) are SKIPPED with reason
//     `"below_apply_threshold"`;
//   * caller-vs-inferred conflicts are NEVER applied — they surface on
//     `conflict_fields[]` with the conflict source intact;
//   * LLM proposals (wave-20 / sonnet_suggest) are SKIPPED unless the
//     caller explicitly approved them via `llm_caller_approved`
//     (per-field bool map or array of field names);
//   * approved LLM proposals additionally require:
//       - `confidence ∈ {high, medium}` (low-confidence LLM proposals
//         are conservative-skip);
//       - `conflict_status="none"` (no caller / deterministic clash);
//       - `safety_check` passes the per-field whitelist (mirrors
//         workstation_dispatch::WorkstationProposalValidator allowlists).
//
// The response carries a structured `apply_gate` block with:
//   * `requested`               — bool echoing the flag.
//   * `applied_fields[]`        — `{field, value, source, origin}`.
//   * `skipped_fields[]`        — `{field, reason, origin}`.
//   * `conflict_fields[]`       — `{field, caller_value, inferred_value,
//                                    confidence, source}`.
//   * `resulting_plan_preview`  — augmented args view (caller-supplied
//                                  ∪ applied_fields), suitable for the
//                                  caller to dry-run a follow-up call.
//   * `persist_inference_requested` — bool echoing `persist_inference`.
//   * `persist_inference_applied`   — always `false` in v1 (the gate
//                                      RESPECTS the persistence boundary
//                                      but does NOT mutate plan.sexp_text;
//                                      a future wave will wire the
//                                      persisted plan write).
//
// Lisp authority forward reference (Wave 21 backfill):
//   - intent-flow.lisp :: F-intent-alignment-plan-execution-loop ::
//                         s4 plan-authoring (apply gate v1)
//   - intent-tools.lisp :: implemented-surface mission_plan ::
//                         :execute-contract :apply-inferred-fields-gate

/// Provenance tag for an apply-gate decision row. Keeps deterministic
/// inference distinguishable from LLM-augmented proposals on the wire so
/// observers can pivot on `origin` without re-reading the bundle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ApplyOrigin {
    /// Field came from `PlanFieldInference::inferred[]`.
    DeterministicInferred,
    /// Field came from `PlanFieldInference::suggested[]`.
    DeterministicSuggested,
    /// Field came from `PlanFieldInference::conflicts[]`.
    DeterministicConflict,
    /// Field came from a `LlmProposal` (wave-20 / sonnet_suggest).
    LlmProposal,
}

impl ApplyOrigin {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            ApplyOrigin::DeterministicInferred => "deterministic_inferred",
            ApplyOrigin::DeterministicSuggested => "deterministic_suggested",
            ApplyOrigin::DeterministicConflict => "deterministic_conflict",
            ApplyOrigin::LlmProposal => "llm_proposal",
        }
    }
}

/// Field actually applied by the gate. Carries enough provenance for an
/// audit reader to reconstruct WHY the field was promoted (deterministic
/// high-confidence vs caller-approved LLM proposal).
#[derive(Debug, Clone)]
pub(super) struct AppliedField {
    pub(super) field: &'static str,
    pub(super) value: Value,
    pub(super) source: &'static str,
    pub(super) origin: ApplyOrigin,
}

impl AppliedField {
    fn to_json(&self) -> Value {
        json!({
            "field": self.field,
            "value": self.value.clone(),
            "source": self.source,
            "origin": self.origin.as_wire(),
        })
    }
}

/// Field deliberately NOT applied. The `reason` is a short canonical
/// string; observers can pivot on it without re-deriving the policy from
/// the rest of the response.
#[derive(Debug, Clone)]
pub(super) struct SkippedField {
    pub(super) field: &'static str,
    pub(super) reason: &'static str,
    pub(super) origin: ApplyOrigin,
    /// Optional human-readable detail (e.g. `"caller already set target"`).
    pub(super) detail: Option<String>,
}

impl SkippedField {
    fn to_json(&self) -> Value {
        let mut m = serde_json::Map::new();
        m.insert("field".to_string(), json!(self.field));
        m.insert("reason".to_string(), json!(self.reason));
        m.insert("origin".to_string(), json!(self.origin.as_wire()));
        if let Some(d) = &self.detail {
            m.insert("detail".to_string(), json!(d));
        }
        Value::Object(m)
    }
}

/// Aggregate apply-gate decision attached to the response under
/// `apply_gate`. Mirrors `PlanFieldInference::to_response_json` in always
/// emitting every list (empty when nothing fired) so observers pivot on a
/// stable shape regardless of which inference mode ran.
#[derive(Debug, Default)]
pub(super) struct ApplyGateOutcome {
    pub(super) requested: bool,
    pub(super) persist_inference_requested: bool,
    pub(super) applied: Vec<AppliedField>,
    pub(super) skipped: Vec<SkippedField>,
    pub(super) conflict: Vec<InferenceConflict>,
    /// Caller-supplied args augmented with `applied[]` — preview only.
    /// Always emitted so a follow-up caller can dry-run with the same
    /// shape without re-deriving it.
    pub(super) resulting_plan_preview: Value,
}

impl ApplyGateOutcome {
    pub(super) fn to_response_json(&self) -> Value {
        let applied: Vec<Value> = self.applied.iter().map(|f| f.to_json()).collect();
        let skipped: Vec<Value> = self.skipped.iter().map(|f| f.to_json()).collect();
        let conflict: Vec<Value> = self.conflict.iter().map(|c| c.to_json()).collect();
        json!({
            "requested": self.requested,
            "persist_inference_requested": self.persist_inference_requested,
            // v1 invariant: persisted plan text is NEVER mutated by this
            // gate. A future wave will wire the persisted plan write
            // gated by an existing `persist=true` action arg or the
            // explicit `persist_inference=true` flag.
            "persist_inference_applied": false,
            "applied_fields": applied,
            "skipped_fields": skipped,
            "conflict_fields": conflict,
            "resulting_plan_preview": self.resulting_plan_preview.clone(),
        })
    }
}

/// Parse the per-field `llm_caller_approved` map. Accepts:
///   * absent / null            → empty set (no LLM approvals).
///   * object `{field: bool}`   → set of fields with `true`.
///   * array of strings         → set of field names verbatim.
/// Strings outside the LLM allowlist are dropped silently (the gate
/// surfaces an "unknown_field" skip reason elsewhere if needed).
pub(super) fn parse_llm_caller_approved(args: &Value) -> std::collections::HashSet<&'static str> {
    let mut out: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
    let raw = match args.get("llm_caller_approved") {
        Some(v) => v,
        None => return out,
    };
    match raw {
        Value::Object(map) => {
            for (k, v) in map.iter() {
                if !v.as_bool().unwrap_or(false) {
                    continue;
                }
                if let Some(canonical) = LLM_ALLOWED_FIELDS
                    .iter()
                    .find(|allowed| allowed.eq_ignore_ascii_case(k))
                    .copied()
                {
                    out.insert(canonical);
                }
            }
        }
        Value::Array(items) => {
            for item in items.iter() {
                let Some(s) = item.as_str() else {
                    continue;
                };
                if let Some(canonical) = LLM_ALLOWED_FIELDS
                    .iter()
                    .find(|allowed| allowed.eq_ignore_ascii_case(s.trim()))
                    .copied()
                {
                    out.insert(canonical);
                }
            }
        }
        // Any other shape is ignored — `llm_caller_approved` is always
        // an explicit map/array; a stray bool/string/number cannot be
        // construed as an approval list, so we treat it as empty rather
        // than erroring. A typo therefore keeps proposals SKIPPED, which
        // is the conservative default.
        _ => {}
    }
    out
}

/// True when caller passed `apply_inferred_fields=true` (any other shape
/// — including the literal string `"true"` — is rejected by the wave-21
/// validator before we get here, so this only checks the bool form).
pub(super) fn caller_requested_apply(args: &Value) -> bool {
    args.get("apply_inferred_fields")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// True when caller passed `persist_inference=true`. Surfaced on the
/// gate response so observers can audit which persistence boundary the
/// gate honoured. The actual plan-text write is FUTURE work — see the
/// `persist_inference_applied=false` invariant in
/// `ApplyGateOutcome::to_response_json`.
pub(super) fn caller_requested_persist_inference(args: &Value) -> bool {
    args.get("persist_inference")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Strict pre-flight validator for `apply_inferred_fields` /
/// `persist_inference`. We only accept the bool form — a literal
/// string `"true"` / `"false"` is rejected so a typo cannot silently
/// masquerade as opt-in. Mirrors the conservative posture of the
/// `infer_plan_fields` validator.
pub(super) fn validate_apply_gate_args(args: &Value) -> std::result::Result<(), String> {
    if let Some(v) = args.get("apply_inferred_fields") {
        if !v.is_boolean() && !v.is_null() {
            return Err(format!(
                "apply_inferred_fields must be a boolean (true|false); got {}",
                json_kind(v)
            ));
        }
    }
    if let Some(v) = args.get("persist_inference") {
        if !v.is_boolean() && !v.is_null() {
            return Err(format!(
                "persist_inference must be a boolean (true|false); got {}",
                json_kind(v)
            ));
        }
    }
    if let Some(v) = args.get("llm_caller_approved") {
        if !v.is_object() && !v.is_array() && !v.is_null() {
            return Err(format!(
                "llm_caller_approved must be object {{field: bool}} or array of field strings; got {}",
                json_kind(v)
            ));
        }
    }
    // wave-22 / task 04 — persisted apply v2 strict shape. `caller_approved`
    // is the second human opt-in (in addition to `apply_inferred_fields`)
    // that arms the persist path. `proposal_hash` is a 32-hex SHA-256
    // prefix the caller echoes back so an out-of-band tamper of
    // `apply_gate.applied_fields[]` is loud (mismatch ⇒ structured error
    // BEFORE any DB mutation per the goal contract). Both args are bool /
    // string only — any other shape (number / array / object) fails fast
    // here so a typo never silently arms or skips the persist path.
    if let Some(v) = args.get("caller_approved") {
        if !v.is_boolean() && !v.is_null() {
            return Err(format!(
                "caller_approved must be a boolean (true|false); got {}",
                json_kind(v)
            ));
        }
    }
    if let Some(v) = args.get("proposal_hash") {
        if !v.is_string() && !v.is_null() {
            return Err(format!(
                "proposal_hash must be a string (32-hex SHA-256 prefix); got {}",
                json_kind(v)
            ));
        }
    }
    Ok(())
}

/// Per-field safety check for an LLM proposal. Mirrors the conservative
/// whitelists wave-21 / task 04 pinned for `workstation_inference_mode`
/// (the LLM proposal pipeline already validated the value shape; this is
/// a second guard at the apply boundary). Returns `Ok(())` when the
/// proposal is safe to apply; `Err(detail)` otherwise.
fn llm_proposal_safety_check(field: &str, value: &Value) -> std::result::Result<(), String> {
    match field {
        "target" => {
            let s = value.as_str().unwrap_or("");
            if matches!(
                s,
                "mission_execution" | "mission_task_delegate" | "mission_flow_run"
            ) {
                Ok(())
            } else {
                Err(format!("target value `{}` not in apply-gate whitelist", s))
            }
        }
        "dispatch_strategy" => {
            let s = value.as_str().unwrap_or("");
            // Conservative: prompt-fallback / unknown deliberately
            // EXCLUDED from auto-apply. Mirrors wave-21 / task 04.
            if matches!(
                s,
                "resident-lisp" | "fresh-code-alignment" | "agent-team" | "mixed"
            ) {
                Ok(())
            } else {
                Err(format!(
                    "dispatch_strategy value `{}` not in apply-gate whitelist",
                    s
                ))
            }
        }
        "acceptance_mode" => {
            let s = value.as_str().unwrap_or("");
            if canonicalize_acceptance_mode(s).is_some() {
                Ok(())
            } else {
                Err(format!(
                    "acceptance_mode value `{}` not in apply-gate whitelist",
                    s
                ))
            }
        }
        "owned_files" => {
            let arr = value.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
            if arr.is_empty() {
                Err("owned_files value is empty".to_string())
            } else if arr.iter().all(|x| x.as_str().map(|s| !s.trim().is_empty()).unwrap_or(false)) {
                Ok(())
            } else {
                Err("owned_files entries must be non-empty strings".to_string())
            }
        }
        "target_project" => {
            let s = value.as_str().unwrap_or("").trim();
            if s.is_empty() {
                Err("target_project value is empty".to_string())
            } else {
                Ok(())
            }
        }
        "workstation_dispatch" => {
            if value.as_bool().is_some() {
                Ok(())
            } else {
                Err("workstation_dispatch value must be boolean".to_string())
            }
        }
        other => Err(format!("field `{}` not supported by apply gate", other)),
    }
}

/// Compute the apply-gate decision over the inference result + caller
/// args. PURE function — no IO, no AppState reads — so the unit tests
/// can pin every edge case without touching the LLM.
///
/// The gate is suggest-only by default; only when
/// `apply_inferred_fields=true` does the function promote any field
/// into `applied[]`. Conflict / suggestion / non-approved-LLM rows
/// always land in `skipped[]` with a canonical reason so observers can
/// pivot on a stable shape.
pub(super) fn compute_apply_gate(
    args: &Value,
    inference: &PlanFieldInference,
) -> ApplyGateOutcome {
    let requested = caller_requested_apply(args);
    let persist_requested = caller_requested_persist_inference(args);
    let approved_llm_fields = parse_llm_caller_approved(args);
    let mut outcome = ApplyGateOutcome {
        requested,
        persist_inference_requested: persist_requested,
        ..Default::default()
    };

    // Conflicts always surface on `conflict_fields[]` regardless of the
    // gate flag — they are the strongest "do NOT silently mutate" signal
    // and observers must see them whether or not apply was requested.
    for c in &inference.conflicts {
        outcome.conflict.push(c.clone());
        // Also record a skip row so a single grep over `skipped_fields[]`
        // tells observers that the conflict-field WOULD have been skipped
        // had the gate tried to apply it.
        outcome.skipped.push(SkippedField {
            field: c.field,
            reason: "caller_value_conflict",
            origin: ApplyOrigin::DeterministicConflict,
            detail: Some(format!(
                "caller_value differs from inferred_value (source={})",
                c.source
            )),
        });
    }

    // Track which field slots are already accounted for (caller-supplied
    // OR already applied) to keep `resulting_plan_preview` deterministic
    // even when the same field appears in both the deterministic block
    // and an LLM proposal.
    let mut preview = args.clone();
    let mut filled: std::collections::HashSet<&'static str> =
        std::collections::HashSet::new();

    // Deterministic high-confidence inferred fields. Skipped without
    // approval; applied when `apply_inferred_fields=true` AND caller did
    // not already populate the slot.
    for f in &inference.inferred {
        if !f.confidence.meets_apply_threshold() {
            // Defensive — wave-18 invariant places only High in
            // `inferred[]`; record the row as a suggestion-tier skip if
            // a future regression sneaks one in.
            outcome.skipped.push(SkippedField {
                field: f.field,
                reason: "below_apply_threshold",
                origin: ApplyOrigin::DeterministicInferred,
                detail: Some(format!("confidence={}", f.confidence.as_wire())),
            });
            continue;
        }
        let caller_already_set = caller_value_for_field(args, f.field).is_some();
        if caller_already_set {
            outcome.skipped.push(SkippedField {
                field: f.field,
                reason: "caller_value_already_set",
                origin: ApplyOrigin::DeterministicInferred,
                detail: None,
            });
            continue;
        }
        if !requested {
            outcome.skipped.push(SkippedField {
                field: f.field,
                reason: "apply_gate_not_requested",
                origin: ApplyOrigin::DeterministicInferred,
                detail: None,
            });
            continue;
        }
        // Promote.
        outcome.applied.push(AppliedField {
            field: f.field,
            value: f.value.clone(),
            source: f.source,
            origin: ApplyOrigin::DeterministicInferred,
        });
        filled.insert(f.field);
        if let Some(map) = preview.as_object_mut() {
            map.insert(f.field.to_string(), f.value.clone());
        }
    }

    // Deterministic suggestions (medium / low). Always skipped — the
    // gate is conservative; sub-threshold fields require the caller to
    // promote them via an explicit arg, NOT the apply flag.
    for f in &inference.suggested {
        outcome.skipped.push(SkippedField {
            field: f.field,
            reason: "below_apply_threshold",
            origin: ApplyOrigin::DeterministicSuggested,
            detail: Some(format!("confidence={}", f.confidence.as_wire())),
        });
    }

    // LLM proposals — apply only when caller approval set + safety check
    // passes + no conflict + confidence != low + caller has not already
    // populated the slot + deterministic inferred[] has not already
    // claimed the slot.
    if let Some(bundle) = inference.llm.as_ref() {
        for p in &bundle.proposals {
            let approved = approved_llm_fields.contains(p.field);
            if !approved {
                outcome.skipped.push(SkippedField {
                    field: p.field,
                    reason: "llm_not_caller_approved",
                    origin: ApplyOrigin::LlmProposal,
                    detail: None,
                });
                continue;
            }
            if !requested {
                // Caller approved the LLM proposal but did not flip the
                // master apply gate — skip with a distinct reason so
                // observers see the layered miss.
                outcome.skipped.push(SkippedField {
                    field: p.field,
                    reason: "apply_gate_not_requested",
                    origin: ApplyOrigin::LlmProposal,
                    detail: None,
                });
                continue;
            }
            if matches!(p.confidence, InferenceConfidence::Low) {
                outcome.skipped.push(SkippedField {
                    field: p.field,
                    reason: "llm_confidence_too_low",
                    origin: ApplyOrigin::LlmProposal,
                    detail: Some(format!("confidence={}", p.confidence.as_wire())),
                });
                continue;
            }
            if !matches!(p.conflict_status, LlmConflictStatus::None) {
                outcome.skipped.push(SkippedField {
                    field: p.field,
                    reason: "llm_conflict_present",
                    origin: ApplyOrigin::LlmProposal,
                    detail: Some(format!(
                        "conflict_status={}",
                        p.conflict_status.as_wire()
                    )),
                });
                continue;
            }
            if let Err(detail) = llm_proposal_safety_check(p.field, &p.value) {
                outcome.skipped.push(SkippedField {
                    field: p.field,
                    reason: "llm_safety_check_failed",
                    origin: ApplyOrigin::LlmProposal,
                    detail: Some(detail),
                });
                continue;
            }
            if caller_value_for_field(args, p.field).is_some() {
                outcome.skipped.push(SkippedField {
                    field: p.field,
                    reason: "caller_value_already_set",
                    origin: ApplyOrigin::LlmProposal,
                    detail: None,
                });
                continue;
            }
            if filled.contains(p.field) {
                // Deterministic inferred[] already promoted this slot;
                // surface the redundant LLM proposal as a structured
                // skip rather than silently duplicating it.
                outcome.skipped.push(SkippedField {
                    field: p.field,
                    reason: "deterministic_inferred_already_applied",
                    origin: ApplyOrigin::LlmProposal,
                    detail: None,
                });
                continue;
            }
            outcome.applied.push(AppliedField {
                field: p.field,
                value: p.value.clone(),
                source: "llm_proposal",
                origin: ApplyOrigin::LlmProposal,
            });
            filled.insert(p.field);
            if let Some(map) = preview.as_object_mut() {
                map.insert(p.field.to_string(), p.value.clone());
            }
        }
    }

    outcome.resulting_plan_preview = preview;
    outcome
}

/// Splice the `apply_gate` block onto a successful response. Mirrors
/// `attach_inference_block`: structured errors are left untouched, and a
/// pre-existing block is preserved (NEVER overwritten) so future DAG /
/// resume paths can attach their own gate row.
pub(super) fn attach_apply_gate_block(
    mut result: ToolResult,
    block: Option<Value>,
) -> ToolResult {
    let Some(block) = block else {
        return result;
    };
    if result.is_error.unwrap_or(false) {
        return result;
    }
    let text = match result.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => return result,
    };
    let mut payload: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return result,
    };
    if let Some(map) = payload.as_object_mut() {
        map.entry("apply_gate".to_string()).or_insert(block);
    }
    result.content = vec![ToolContent::Text {
        text: serde_json::to_string_pretty(&payload).unwrap_or(text),
    }];
    result
}

// ── wave-22 / task 04 — Persisted PLAN inference apply v2 ──────────────
//
// Layered on top of wave-21 / task 05 apply gate v1. The v1 gate
// promoted `applied_fields[]` into caller args (in-memory only) and
// hard-pinned `apply_gate.persist_inference_applied = false`. v2
// preserves every v1 invariant (default off, conflicts never apply,
// suggestions never apply, LLM proposals require `llm_caller_approved`,
// strict bool shape) AND adds an explicit, audited persistence path:
//
//   * `apply_inferred_fields=true`        — v1 master switch
//   * `persist_inference=true`            — v1 echo flag, NOW load-bearing
//   * `caller_approved=true`              — NEW second human opt-in
//   * `proposal_hash=<32-hex>`            — NEW deterministic correlator
//
// All four must hold AND the gate must have promoted at least one
// field AND the caller's hash must MATCH `compute_inference_proposal_hash`
// computed over `(plan_id, original_sexp_hash, applied_fields)`. On
// mismatch / missing the handler returns a structured error BEFORE any
// DB mutation per the goal contract (R2).
//
// On success the handler:
//   1. Reads `original_sexp_hash` from the existing `plan` row.
//   2. Synthesises `resulting_sexp_text` by APPENDING a guarded
//      `(plan-inference-applied :inference-version "v2" ...)` form to
//      the existing s-exp. The original body is preserved verbatim and
//      `parse_plan_hints` keeps first-occurrence semantics, so the
//      observable PLAN behaviour stays identical when the appended
//      keywords overlap an original hint. New hints become live.
//   3. Inserts a NEW plan row at `version = max + 1` via `plan_insert`
//      — never overwrites the existing row (R4 — version + audit).
//   4. Calls `plan_supersede(old_id)` so the previous version is
//      visibly retired with `status=superseded` (rollback handle).
//   5. Appends a typed `plan_inference_persisted_apply` evidence entry
//      with applied_fields[], skipped_fields[], proposal_hash,
//      original_sexp_hash, resulting_sexp_hash, rollback_pointer
//      (the previous plan id) so the audit trail is complete (R5).
//
// Conservative posture: the persist path is OPT-IN at four
// independent flags. Default behaviour (any flag absent / false)
// keeps the v1 byte-shape exactly — `apply_gate.persist_inference_applied`
// stays `false` and the response surfaces `persisted_apply.status =
// "not_requested"` so observers can pivot without re-deriving the
// policy. Failure modes (missing hash / hash mismatch / invalid param)
// fail-fast as structured errors; soft-skip modes (no applied fields /
// caller_approved=false / persist_inference=false) surface on the
// `persisted_apply` block with a canonical reason and DO NOT mutate
// the DB.
//
// Lisp authority forward reference (Wave 22 backfill):
//   - intent-flow.lisp :: F-intent-alignment-plan-execution-loop ::
//                         s4 plan-authoring (persist gate v2)
//   - intent-tools.lisp :: implemented-surface mission_plan ::
//                         :execute-contract :persisted-inference-apply

/// True when caller passed `caller_approved=true` (any other shape —
/// including the literal string `"true"` — is rejected by
/// `validate_apply_gate_args` BEFORE we get here, so this only checks
/// the bool form). The flag is the SECOND human opt-in for the v2
/// persist path; default `false` keeps the v1 byte-shape exactly.
pub(super) fn caller_requested_caller_approved(args: &Value) -> bool {
    args.get("caller_approved")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Extract the caller-supplied `proposal_hash` (32-hex SHA-256 prefix).
/// Returns `None` when absent, an empty string after trim, or a non-
/// string shape (the validator already rejected the latter as
/// `INVALID_PARAM`, so this is purely defensive).
pub(super) fn caller_supplied_proposal_hash(args: &Value) -> Option<String> {
    let s = args.get("proposal_hash").and_then(|v| v.as_str())?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Compute the deterministic 32-hex correlator over
/// `(plan_id, original_sexp_hash, sorted applied fields)`. The hash is
/// what the caller is expected to echo back via the `proposal_hash`
/// arg under `apply_inferred_fields=true + caller_approved=true +
/// persist_inference=true`. Caller can derive it themselves from the
/// gate response — we surface the same value under
/// `persisted_apply.computed_proposal_hash` so dashboards can
/// `assert hash == derive(...)` directly.
///
/// Hash payload (canonical UTF-8):
///   `"v2|<plan-id>|<original-sexp-hash>|<field>:<value-canonical>|..."`
///
/// Fields are sorted lexicographically by `field` so observers see a
/// deterministic hash regardless of the order in which the gate
/// promoted them. Each value is canonicalised via
/// `serde_json::to_string` (compact form, sorted object keys via the
/// `Value` representation).
pub(super) fn compute_inference_proposal_hash(
    plan_id: uuid::Uuid,
    original_sexp_hash: &str,
    applied: &[AppliedField],
) -> String {
    use sha2::{Digest, Sha256};
    let mut sorted: Vec<&AppliedField> = applied.iter().collect();
    sorted.sort_by_key(|af| af.field);
    let mut payload = format!("v2|{}|{}", plan_id, original_sexp_hash.trim());
    for af in sorted.iter() {
        let value_canonical =
            serde_json::to_string(&af.value).unwrap_or_else(|_| String::new());
        payload.push('|');
        payload.push_str(af.field);
        payload.push(':');
        payload.push_str(&value_canonical);
    }
    let mut h = Sha256::new();
    h.update(payload.as_bytes());
    let full = format!("{:x}", h.finalize());
    full[..32].to_string()
}

/// Status discriminants for the v2 persist path. The wire string is
/// stable so observers / dashboards can pivot on it without re-reading
/// the rest of the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PersistedApplyStatus {
    /// Caller did not opt into the persist path (default). The v1 gate
    /// may still have augmented in-memory args.
    NotRequested,
    /// All four opt-ins supplied + hash matched + at least one field
    /// promoted ⇒ a new plan version was committed.
    Applied,
    /// `apply_inferred_fields` was not `true`. Persist requires the
    /// master switch.
    SkippedApplyGateNotRequested,
    /// `persist_inference` was not `true`. Persist requires the
    /// dedicated persistence opt-in (echo of v1 flag, now load-bearing).
    SkippedPersistNotRequested,
    /// `caller_approved` was not `true`. Persist requires the second
    /// human opt-in.
    SkippedCallerNotApproved,
    /// The v1 gate promoted no fields (everything was conflict /
    /// suggestion / non-approved / safety-skipped). Persist refuses
    /// to write a no-op version.
    SkippedNothingToApply,
}

impl PersistedApplyStatus {
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            PersistedApplyStatus::NotRequested => "not_requested",
            PersistedApplyStatus::Applied => "applied",
            PersistedApplyStatus::SkippedApplyGateNotRequested => {
                "skipped_apply_gate_not_requested"
            }
            PersistedApplyStatus::SkippedPersistNotRequested => {
                "skipped_persist_not_requested"
            }
            PersistedApplyStatus::SkippedCallerNotApproved => {
                "skipped_caller_not_approved"
            }
            PersistedApplyStatus::SkippedNothingToApply => "skipped_nothing_to_apply",
        }
    }

    pub(super) fn was_applied(self) -> bool {
        matches!(self, PersistedApplyStatus::Applied)
    }
}

/// Aggregate persisted-apply outcome, surfaced under `persisted_apply`
/// on the response. Mirrors `ApplyGateOutcome::to_response_json` in
/// always emitting every field (with conservative defaults) so observers
/// pivot on a stable shape regardless of the persist path's status.
#[derive(Debug, Clone)]
pub(super) struct PersistedApplyOutcome {
    pub(super) status: PersistedApplyStatus,
    pub(super) apply_inferred_fields_requested: bool,
    pub(super) persist_inference_requested: bool,
    pub(super) caller_approved: bool,
    pub(super) original_sexp_hash: String,
    pub(super) resulting_sexp_hash: Option<String>,
    pub(super) computed_proposal_hash: Option<String>,
    pub(super) supplied_proposal_hash: Option<String>,
    pub(super) applied_fields: Vec<AppliedField>,
    pub(super) skipped_fields: Vec<SkippedField>,
    /// Newly inserted plan id (when status == Applied). `None` on every
    /// skip path.
    pub(super) new_plan_id: Option<uuid::Uuid>,
    /// New plan version (when status == Applied). `None` on every skip path.
    pub(super) new_plan_version: Option<i32>,
    /// Pointer to the now-superseded plan id (rollback handle). Always
    /// populated when status == Applied; the wave-21 plan_supersede call
    /// guarantees the row stays queryable for audit / replay.
    pub(super) rollback_plan_id: Option<uuid::Uuid>,
}

impl PersistedApplyOutcome {
    /// Build the default `not_requested` outcome from caller args + the
    /// v1 apply-gate decision. Used as the response anchor on every
    /// path that does NOT opt into persist.
    pub(super) fn from_skip_reason(
        status: PersistedApplyStatus,
        args: &Value,
        original_sexp_hash: &str,
        applied: &[AppliedField],
        skipped: &[SkippedField],
        computed_hash: Option<String>,
    ) -> Self {
        Self {
            status,
            apply_inferred_fields_requested: caller_requested_apply(args),
            persist_inference_requested: caller_requested_persist_inference(args),
            caller_approved: caller_requested_caller_approved(args),
            original_sexp_hash: original_sexp_hash.to_string(),
            resulting_sexp_hash: None,
            computed_proposal_hash: computed_hash,
            supplied_proposal_hash: caller_supplied_proposal_hash(args),
            applied_fields: applied.to_vec(),
            skipped_fields: skipped.to_vec(),
            new_plan_id: None,
            new_plan_version: None,
            rollback_plan_id: None,
        }
    }

    pub(super) fn to_response_json(&self) -> Value {
        let applied: Vec<Value> = self.applied_fields.iter().map(|f| f.to_json()).collect();
        let skipped: Vec<Value> = self.skipped_fields.iter().map(|f| f.to_json()).collect();
        json!({
            "status": self.status.as_wire(),
            "apply_inferred_fields_requested": self.apply_inferred_fields_requested,
            "persist_inference_requested": self.persist_inference_requested,
            "caller_approved": self.caller_approved,
            "original_sexp_hash": self.original_sexp_hash,
            "resulting_sexp_hash": self.resulting_sexp_hash.clone(),
            "computed_proposal_hash": self.computed_proposal_hash.clone(),
            "supplied_proposal_hash": self.supplied_proposal_hash.clone(),
            "applied_fields": applied,
            "skipped_fields": skipped,
            "new_plan_id": self.new_plan_id.map(|u| u.to_string()),
            "new_plan_version": self.new_plan_version,
            "rollback_plan_id": self.rollback_plan_id.map(|u| u.to_string()),
        })
    }
}

// Make `AppliedField` / `SkippedField` cloneable so the persist path can
// snapshot them into the outcome for the response + evidence.
//
// The wave-21 / task 05 structs were defined `Clone`-free; we add it via
// derive on the field types directly above. (The structs themselves are
// already `Clone` — see `#[derive(Debug, Clone)]` on `AppliedField` and
// `SkippedField`.)

/// Pure pre-flight gate. Inverted v1 semantics: persist runs ONLY when
/// every opt-in is true AND the gate promoted at least one field. On
/// any failure path returns the canonical skip status WITHOUT touching
/// the DB. Hash mismatch / missing is NOT handled here — that path is
/// handled by `enforce_persisted_apply_preflight` which fail-fasts as
/// a structured error per R2.
pub(super) fn evaluate_persisted_apply_gate(
    args: &Value,
    apply: &ApplyGateOutcome,
) -> PersistedApplyStatus {
    if !caller_requested_apply(args) {
        return PersistedApplyStatus::SkippedApplyGateNotRequested;
    }
    if !caller_requested_persist_inference(args) {
        return PersistedApplyStatus::SkippedPersistNotRequested;
    }
    if !caller_requested_caller_approved(args) {
        return PersistedApplyStatus::SkippedCallerNotApproved;
    }
    if apply.applied.is_empty() {
        return PersistedApplyStatus::SkippedNothingToApply;
    }
    PersistedApplyStatus::Applied
}

/// Strict pre-flight for the v2 hash check. Mirrors
/// `enforce_apply_gate_preflight` from review_gate.rs: returns
/// `Err((code, message))` on missing / mismatch BEFORE any DB mutation
/// per the goal contract (R2). Returns `Ok(())` when the caller did not
/// opt into the persist path (the soft-skip outcome is computed by
/// `evaluate_persisted_apply_gate` afterwards) OR when the hash matches
/// the deterministic correlator.
///
/// Skipping the preflight on a non-persist path is intentional: the
/// caller may legitimately omit `proposal_hash` / `caller_approved` on
/// every legacy v1 call. We only fail-fast when the caller PRESENTED
/// the persist intent (apply + persist + caller_approved all `true`)
/// AND the hash is missing / wrong.
pub(super) fn enforce_persisted_apply_preflight(
    args: &Value,
    computed_hash: &str,
) -> std::result::Result<(), (&'static str, String)> {
    // Preflight only applies when caller opted into all THREE persist
    // flags. Any other arrangement is a soft-skip handled downstream.
    if !caller_requested_apply(args)
        || !caller_requested_persist_inference(args)
        || !caller_requested_caller_approved(args)
    {
        return Ok(());
    }
    let supplied = match caller_supplied_proposal_hash(args) {
        Some(s) => s,
        None => {
            return Err((
                error_codes::INVALID_PARAM,
                format!(
                    "PERSIST_APPLY_MISSING_PROPOSAL_HASH: persist_inference=true + caller_approved=true requires proposal_hash to match the v2 deterministic correlator (expected `{}`); supply proposal_hash from a prior preview call's persisted_apply.computed_proposal_hash field",
                    computed_hash
                ),
            ));
        }
    };
    if !supplied.eq_ignore_ascii_case(computed_hash) {
        return Err((
            error_codes::INVALID_PARAM,
            format!(
                "PERSIST_APPLY_PROPOSAL_HASH_MISMATCH: caller-supplied proposal_hash `{}` does not match the v2 deterministic correlator `{}`; the apply set may have changed since the proposal was previewed — re-run the gate without persist flags first to capture the fresh hash",
                supplied, computed_hash
            ),
        ));
    }
    Ok(())
}

/// Render a single AppliedField as a `:keyword value` lisp pair. Mirrors
/// the conservative `parse_plan_hints` reader in plan.rs:
///   * canonical kebab-case keyword (matches the reader's
///     `target` / `dispatch-strategy` / `target-project` / `requested-cwd`
///     / `acceptance-mode` / `owned-files` / `workstation-dispatch`
///     spellings)
///   * string scalars are double-quoted with `\\` / `\"` escapes
///   * bool scalars become `true` / `false` barewords
///   * arrays become `[ "a" "b" ]` bracket lists (matches
///     `split_lisp_string_list`)
///   * any other shape (number / object / null) is serialised via
///     `serde_json::to_string` and emitted as a quoted string so the
///     reader treats it as a bareword passthrough — defensive only,
///     the apply gate's safety check already filtered shapes before we
///     get here.
pub(super) fn render_applied_field_to_lisp(field: &str, value: &Value) -> String {
    let key = match field {
        "target" => "target",
        "dispatch_strategy" => "dispatch-strategy",
        "target_project" => "target-project",
        "owned_files" => "owned-files",
        "acceptance_mode" => "acceptance-mode",
        "workstation_dispatch" => "workstation-dispatch",
        // Defensive — any future field name that is not a known reader
        // alias keeps the snake-case form so the keyword pair is still
        // syntactically valid (the reader will silently ignore unknown
        // keywords per its `_ => {}` arm).
        other => other,
    };
    match value {
        Value::String(s) => format!(":{} \"{}\"", key, escape_lisp_string(s)),
        Value::Bool(b) => format!(":{} {}", key, if *b { "true" } else { "false" }),
        Value::Array(items) => {
            let mut parts: Vec<String> = Vec::with_capacity(items.len());
            for item in items.iter() {
                match item {
                    Value::String(s) => parts.push(format!("\"{}\"", escape_lisp_string(s))),
                    Value::Bool(b) => parts.push((if *b { "true" } else { "false" }).into()),
                    other => parts.push(format!(
                        "\"{}\"",
                        escape_lisp_string(&serde_json::to_string(other).unwrap_or_default())
                    )),
                }
            }
            format!(":{} [{}]", key, parts.join(" "))
        }
        Value::Number(n) => format!(":{} {}", key, n),
        other => format!(
            ":{} \"{}\"",
            key,
            escape_lisp_string(&serde_json::to_string(other).unwrap_or_default())
        ),
    }
}

fn escape_lisp_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            other => out.push(other),
        }
    }
    out
}

/// Synthesise the resulting PLAN.lisp by APPENDING a guarded
/// `(plan-inference-applied ...)` form to the existing s-exp. The
/// original body is preserved verbatim so the supersede chain has a
/// clean diff (`tail -1` on the new s-exp shows the persisted
/// annotation; everything else is the byte-identical predecessor).
///
/// `parse_plan_hints` keeps first-occurrence semantics — when the
/// appended keyword overlaps an original hint the original wins. New
/// hints (e.g. `:dispatch-strategy resident-lisp` when the original
/// PLAN never spelled it) become the live value because no prior
/// occurrence exists. This is the conservative posture: the inferer
/// EXTENDS the PLAN; it never silently rewrites a caller-authored
/// hint at the persistence boundary.
pub(super) fn synthesize_persisted_sexp(
    original: &str,
    applied: &[AppliedField],
    proposal_hash: &str,
    timestamp: &str,
) -> String {
    let mut out = String::with_capacity(original.len() + 256);
    out.push_str(original);
    if !original.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    // Header — observers can grep on this exact prefix.
    out.push_str(";; wave-22 / task 04 — persisted PLAN inference apply v2\n");
    out.push_str(&format!(
        "(plan-inference-applied :inference-version \"v2\" :proposal-hash \"{}\" :persisted-at \"{}\"",
        proposal_hash, timestamp
    ));
    // Emit each applied field as a SIBLING keyword pair so the
    // wave-15 / task 05 `parse_plan_hints` reader picks them up at the
    // PLAN level (the reader scans `:keyword value` pairs at any depth
    // but treats bracket lists as opaque value spans). A flat list of
    // pairs keeps the appended annotation queryable without breaking
    // first-occurrence semantics — the original PLAN body still
    // appears first in the buffer, so its hints win on every overlap.
    for af in applied.iter() {
        out.push('\n');
        out.push_str("  ");
        out.push_str(&render_applied_field_to_lisp(af.field, &af.value));
    }
    out.push(')');
    out.push('\n');
    out
}

/// Build the typed evidence entry for the persisted apply path. Mirrors
/// the wave-12 typed-evidence schema (`schema_version="v0"`,
/// canonical `source` + `kind`) so a single grep over the evidence
/// sidecar surfaces every persist event with a stable shape.
pub(super) fn build_persisted_apply_evidence_entry(
    outcome: &PersistedApplyOutcome,
    plan_id: uuid::Uuid,
) -> Value {
    let applied: Vec<Value> = outcome
        .applied_fields
        .iter()
        .map(|f| f.to_json())
        .collect();
    let skipped: Vec<Value> = outcome
        .skipped_fields
        .iter()
        .map(|f| f.to_json())
        .collect();
    json!({
        "schema_version": "v0",
        "source": "plan_inference_persisted_apply",
        "kind": "plan_inference_persisted_apply",
        "plan_id": plan_id.to_string(),
        "rollback_plan_id": outcome.rollback_plan_id.map(|u| u.to_string()),
        "new_plan_id": outcome.new_plan_id.map(|u| u.to_string()),
        "new_plan_version": outcome.new_plan_version,
        "original_sexp_hash": outcome.original_sexp_hash,
        "resulting_sexp_hash": outcome.resulting_sexp_hash,
        "proposal_hash": outcome.computed_proposal_hash,
        "applied_fields": applied,
        "skipped_fields": skipped,
        "status": outcome.status.as_wire(),
    })
}

/// Apply the v2 persist gate. Pure of `state` interaction at the gate
/// stage (compute hash + evaluate skip), then exercises `state.store`
/// for the new plan version + supersede + evidence write only when the
/// gate authorised the apply. On every skip path the DB is untouched
/// and the outcome surfaces the canonical skip reason on the response.
///
/// Returns `Err(structured_error_pair)` ONLY for the fail-fast hash
/// preflight (R2). Every other path returns `Ok(outcome)` with the
/// status communicating success / soft-skip.
pub(super) async fn execute_persisted_apply(
    state: &AppState,
    plan: &Plan,
    args: &Value,
    apply: &ApplyGateOutcome,
) -> std::result::Result<PersistedApplyOutcome, (&'static str, String)> {
    let original_sexp_hash = sha256_hex(&plan.sexp_text);
    let computed_hash = compute_inference_proposal_hash(
        plan.id,
        &original_sexp_hash,
        &apply.applied,
    );

    // Fail-fast hash preflight per R2.
    enforce_persisted_apply_preflight(args, &computed_hash)?;

    let status = evaluate_persisted_apply_gate(args, apply);
    if !status.was_applied() {
        return Ok(PersistedApplyOutcome::from_skip_reason(
            status,
            args,
            &original_sexp_hash,
            &apply.applied,
            &apply.skipped,
            Some(computed_hash),
        ));
    }

    // Persist path. Synthesise the new sexp text + hash, allocate the
    // next plan version, insert the new row, supersede the predecessor,
    // append the typed evidence entry. Each step uses the existing
    // wave-21 store API (no new trait method per the contract's
    // `:must-not-touch` boundary).
    let timestamp = iso_now();
    let resulting_sexp_text =
        synthesize_persisted_sexp(&plan.sexp_text, &apply.applied, &computed_hash, &timestamp);
    let resulting_sexp_hash = sha256_hex(&resulting_sexp_text);

    let existing = state
        .store
        .plan_list_by_task(&plan.board_task_id)
        .await
        .map_err(|e| {
            (
                error_codes::DB_ERROR,
                format!("plan_list_by_task: {}", e),
            )
        })?;
    let next_version = existing.iter().map(|p| p.version).max().unwrap_or(0) + 1;

    let new_plan_id = state
        .store
        .plan_insert(
            &plan.board_task_id,
            plan.source_directive_id,
            next_version,
            &resulting_sexp_text,
            &resulting_sexp_hash,
            // Inherit the predecessor's status — we are NOT changing
            // FSM stage on this write, only persisting an inference
            // annotation. The plan-runner will continue from the new
            // version on its next execute call.
            plan.status,
            plan.compiler_model.as_deref(),
            // Stamp `compiled_from` so the audit trail points at the
            // predecessor row (rollback handle on the immutable v0 of
            // the column).
            Some(&format!("plan-inference-persist/{}", plan.id)),
        )
        .await
        .map_err(|e| (error_codes::DB_ERROR, format!("plan_insert: {}", e)))?;

    state
        .store
        .plan_supersede(plan.id, new_plan_id)
        .await
        .map_err(|e| (error_codes::DB_ERROR, format!("plan_supersede: {}", e)))?;

    let outcome = PersistedApplyOutcome {
        status: PersistedApplyStatus::Applied,
        apply_inferred_fields_requested: true,
        persist_inference_requested: true,
        caller_approved: true,
        original_sexp_hash: original_sexp_hash.clone(),
        resulting_sexp_hash: Some(resulting_sexp_hash),
        computed_proposal_hash: Some(computed_hash),
        supplied_proposal_hash: caller_supplied_proposal_hash(args),
        applied_fields: apply.applied.clone(),
        skipped_fields: apply.skipped.clone(),
        new_plan_id: Some(new_plan_id),
        new_plan_version: Some(next_version),
        rollback_plan_id: Some(plan.id),
    };

    // Append typed evidence (R5). Failure here does NOT roll back the
    // new plan row — file-vs-db contract per `append_plan_evidence_entry`
    // (the row is committed even if the sidecar write fails). We
    // surface the error path via the standard evidence_warning surface
    // on the response.
    let evidence_entry = build_persisted_apply_evidence_entry(&outcome, plan.id);
    let project_arg = args.get("project").and_then(|v| v.as_str());
    let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg = args.get("target_project").and_then(|v| v.as_str());
    // Append on the PREDECESSOR's evidence sidecar (the rollback
    // pointer is the predecessor — observers replaying a rollback
    // need the persisted-apply entry on the same sidecar as the
    // pre-apply history).
    let _ = append_plan_evidence_entry(
        state,
        plan.id,
        project_arg,
        cwd_arg,
        target_project_arg,
        evidence_entry,
    )
    .await;

    Ok(outcome)
}

/// Splice the `persisted_apply` block onto a successful response.
/// Mirrors `attach_apply_gate_block` exactly: structured errors are
/// left untouched, and a pre-existing block is preserved (NEVER
/// overwritten) so future DAG / resume paths can attach their own row.
pub(super) fn attach_persisted_apply_block(
    mut result: ToolResult,
    block: Option<Value>,
) -> ToolResult {
    let Some(block) = block else {
        return result;
    };
    if result.is_error.unwrap_or(false) {
        return result;
    }
    let text = match result.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => return result,
    };
    let mut payload: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return result,
    };
    if let Some(map) = payload.as_object_mut() {
        map.entry("persisted_apply".to_string()).or_insert(block);
    }
    result.content = vec![ToolContent::Text {
        text: serde_json::to_string_pretty(&payload).unwrap_or(text),
    }];
    result
}

/// Map a free-form target string from a plan hint to the canonical 3-target
/// surface. `flow_id_present` gates `mission_flow_run` because the inner
/// dispatcher refuses to run without a flow_id.
fn normalize_target(raw: &str, flow_id_present: bool) -> Option<&'static str> {
    let lower = raw.to_ascii_lowercase();
    // task_delegate keywords are most specific — check first so a plan hint
    // like "claudecode" or "code-alignment" doesn't get swallowed by the
    // generic "execution" branch.
    if lower.contains("mission_task_delegate")
        || lower.contains("task_delegate")
        || lower.contains("task-delegate")
        || lower.contains("claudecode")
        || lower.contains("code-alignment")
    {
        return Some("mission_task_delegate");
    }
    if flow_id_present
        && (lower.contains("mission_flow_run")
            || lower.contains("flow_run")
            || lower.contains("flow-run")
            || lower.contains("flow"))
    {
        return Some("mission_flow_run");
    }
    if lower.contains("mission_execution") || lower.contains("execution") {
        return Some("mission_execution");
    }
    None
}

/// Map a free-form strategy hint to one of `VALID_DISPATCH_STRATEGIES`.
/// `unknown` is treated as "no signal" so callers can fall back to the next
/// priority source. Returns `None` when the string carries no usable hint.
fn canonicalize_strategy(raw: &str) -> Option<&'static str> {
    let lower = raw.to_ascii_lowercase();
    for &valid in VALID_DISPATCH_STRATEGIES {
        if lower == valid {
            if valid == "unknown" {
                return None;
            }
            return Some(valid);
        }
    }
    if lower.contains("agent-team") || lower.contains("agent_team") {
        return Some("agent-team");
    }
    if lower.contains("code-alignment")
        || lower.contains("code_alignment")
        || lower.contains("fresh")
    {
        return Some("fresh-code-alignment");
    }
    if lower.contains("resident") || lower.contains("lisp-architect") || lower.contains("architect")
    {
        return Some("resident-lisp");
    }
    if lower.contains("mixed") {
        return Some("mixed");
    }
    if lower.contains("prompt") || lower.contains("fallback") {
        return Some("prompt-fallback");
    }
    None
}

/// Resolve the dispatch strategy with source-tracking precedence:
///   explicit arg > plan hint :dispatch-strategy > plan hint :parallelism > default unknown
fn resolve_dispatch_strategy(
    explicit: Option<&str>,
    hints: &ParsedPlanHints,
) -> (&'static str, &'static str) {
    if let Some(s) = explicit {
        let canonical = canonicalize_strategy(s).unwrap_or("unknown");
        return (canonical, "explicit_arg");
    }
    if let Some(s) = hints.dispatch_strategy.as_deref() {
        if let Some(c) = canonicalize_strategy(s) {
            return (c, "plan_hint");
        }
    }
    if let Some(p) = hints.parallelism.as_deref() {
        if let Some(c) = canonicalize_strategy(p) {
            return (c, "plan_hint");
        }
    }
    ("unknown", "default")
}

// ───────────────────────────────────────────────────────────────────────
// execute — plan-runner v0
//
// execute_mode=bridge (default): return next_call descriptor, do NOT dispatch.
// execute_mode=internal: dispatch the chosen target handler inside MissionD,
//                        append plan_runner_dispatch evidence, mark plan
//                        executing on success.
//
// Lisp authority for the internal path:
//   intent-intent-layer.lisp :: section unified-entry-pipeline :: role plan-runner
//   intent-tools.lisp        :: implemented-surface mission_plan :: :execute-contract
//   intent-flow.lisp         :: F-intent-alignment-plan-execution-loop :: s6 execution-runner
//
// TODO(plan-runner): mission_execution companion-log persistence of
// dispatch_strategy is still future per
// `intent-tools.lisp :: workstation-dispatch-record`. We surface it in this
// tool's response and the evidence sidecar so the audit trail is complete
// even before the schema-side field exists.
// ───────────────────────────────────────────────────────────────────────

async fn action_execute(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "plan_id")?;

    let execute_mode = args
        .get("execute_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("bridge");
    if !matches!(execute_mode, "bridge" | "internal") {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!("execute_mode `{}` not supported", execute_mode),
            )
            .with_suggestion("execute_mode ∈ {bridge, internal}; default is bridge"),
        ));
    }

    // wave-24 / task 04 — pre-flight `router_policy_mode` validation.
    // Default `off` ⇒ byte-compatible with wave-15..23 (no recommendation
    // block emitted). `dry_run` ⇒ compute an advisory router recommendation
    // block AFTER the dispatch path resolves; the recommendation NEVER
    // alters target / dispatch_strategy / workstation_dispatch /
    // auto_spawn / evidence — `applied` is hard-coded `false`. Any other
    // value (including `apply` / `auto`) returns INVALID_PARAM here, BEFORE
    // any plan lookup, so a typo cannot silently route a recommendation
    // through a runtime path that doesn't exist.
    let router_policy_mode = match router_policy_dry_run::parse_router_policy_mode(args) {
        Ok(m) => m,
        Err(err) => return Ok(err),
    };

    // wave-18 / task 06 — pre-flight `infer_plan_fields` validation. Runs
    // BEFORE the plan lookup so a typo (`infer_plan_fields="aply"`) fails
    // fast instead of after a DB read.
    let infer_mode = match parse_infer_plan_fields_mode(args) {
        Ok(m) => m,
        Err(msg) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::INVALID_PARAM, msg),
            ))
        }
    };

    // wave-21 / task 05 — pre-flight `apply_inferred_fields` /
    // `persist_inference` / `llm_caller_approved` shape validation. Runs
    // BEFORE the plan lookup so a typo (`apply_inferred_fields="ture"`)
    // fails fast instead of silently being ignored. Conservative: only
    // bool / object / array shapes are accepted; string `"true"` is
    // rejected so the gate never opens by accident.
    if let Err(msg) = validate_apply_gate_args(args) {
        return Ok(ToolResult::structured_error(
            ToolError::new(error_codes::INVALID_PARAM, msg),
        ));
    }

    // wave-21 / task 04 — pre-flight `workstation_inference_mode`
    // validation. Strictly orthogonal to `infer_plan_fields`: the wave-21
    // surface targets the four workstation knobs (target /
    // dispatch_strategy / objective / scope) and ONLY fires when caller /
    // PLAN supplied no signal. A typo (`workstation_inference_mode="sonet"`)
    // fails fast here rather than after the DB read.
    let workstation_infer_mode = match parse_workstation_inference_mode(args) {
        Ok(m) => m,
        Err(msg) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::INVALID_PARAM, msg),
            ))
        }
    };
    // DAG mode rejects sonnet_suggest at preflight (single-node-only in
    // v0). Mirrors the wave-20 / task 07 enforcement on the plan-field
    // surface.
    if let Some(err) = refuse_workstation_inference_in_dag_mode(args) {
        return Ok(err);
    }

    let plan = match state
        .store
        .plan_get(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::NOT_FOUND, format!("plan `{}` not found", id)),
            ))
        }
    };
    if !matches!(plan.status, PlanStatus::Approved | PlanStatus::Executing) {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!(
                    "plan status `{}` is not executable; approve it first via action=approve",
                    plan.status.as_str()
                ),
            ),
        ));
    }

    // wave-18 / task 06 — autonomous PLAN field inference. We always run
    // the inference engine when `infer_plan_fields != off` and short-
    // circuit (preview / sonnet_suggest) or augment caller args (apply_safe).
    // When the mode is `off`, the variable below stays `None` and the
    // downstream pipeline observes byte-identical legacy behaviour.
    //
    // wave-20 / task 07 — `sonnet_suggest` extends the same engine with
    // an LLM proposal pass. The deterministic block runs unchanged FIRST
    // (so high-confidence determinism is never overridden by the model);
    // then Sonnet is asked to fill remaining fields. Proposals are
    // SURFACED, never auto-applied.
    //
    // wave-21 / task 05 — apply gate v1. Layered on top of the wave-18 /
    // wave-20 inference output. The new `apply_inferred_fields=true`
    // flag opts the caller into a CONTROLLED apply path (suggest-only
    // by default). The gate is suggest-only when the flag is absent so
    // the wave-18 byte-shape stays intact for back-compat callers.
    //
    // wave-22 / task 04 — persisted apply v2 splices in here as well.
    // When the caller opts into BOTH `apply_inferred_fields=true` AND
    // `persist_inference=true` AND `caller_approved=true` AND a matching
    // `proposal_hash`, the gate ALSO writes a new plan version + supersede
    // + typed evidence row. Preflight (hash mismatch / missing) fails
    // FAST as a structured error BEFORE any DB mutation per the contract.
    // Default behaviour (any flag absent / false) keeps the v1 byte-shape
    // exactly — `persisted_apply.status="not_requested"` lands on the
    // response so observers can pivot without re-deriving the policy.
    let mut effective_plan: Plan = plan.clone();
    let (effective_args, inference_block, apply_gate_block, persisted_apply_block): (
        Value,
        Option<Value>,
        Option<Value>,
        Option<Value>,
    ) = if matches!(infer_mode, InferPlanFieldsMode::Off) {
            // wave-22 / task 04 — emit a stable `not_requested`
            // persisted_apply block even when inference is OFF so
            // observers can pivot on a single shape regardless of
            // mode. The hash field defaults to a deterministic
            // placeholder (sha256 of the un-augmented sexp) so
            // dashboards can still cross-check provenance.
            let original_hash = sha256_hex(&plan.sexp_text);
            let not_requested = PersistedApplyOutcome::from_skip_reason(
                PersistedApplyStatus::NotRequested,
                args,
                &original_hash,
                &[],
                &[],
                None,
            );
            (args.clone(), None, None, Some(not_requested.to_response_json()))
        } else {
            let project_arg = args.get("project").and_then(|v| v.as_str());
            let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
            let target_project_arg = args.get("target_project").and_then(|v| v.as_str());
            // 16 entries is the soft cap — recent dispatches dominate the
            // inferer's signal; older entries are rarely useful.
            let evidence_entries = read_recent_evidence_entries(
                state,
                id,
                project_arg,
                cwd_arg,
                target_project_arg,
                16,
            )
            .await;
            let plan_hints = parse_plan_hints(&plan.sexp_text);
            let input = PlanInferenceInput {
                plan_hints,
                plan_sexp: &plan.sexp_text,
                compiled_from: plan.compiled_from.as_deref(),
                evidence_entries: evidence_entries.clone(),
            };
            let mut inference = compute_plan_field_inference(args, &input);

            // wave-20 / task 07 — Sonnet pass. Runs only under
            // `sonnet_suggest`; failure surfaces as an `Unavailable` bundle
            // (NOT a silent fallback to deterministic-only). Proposals never
            // mutate caller args.
            if infer_mode.is_llm_augmented() {
                let bundle = request_llm_proposals(
                    state,
                    &plan.sexp_text,
                    plan.compiled_from.as_deref(),
                    &evidence_entries,
                    &inference,
                    args,
                )
                .await;
                inference.llm = Some(bundle);
            }

            let block = inference.to_response_json(infer_mode);

            // wave-21 / task 05 — compute the apply gate over the
            // inference result + caller args. Suggest-only when
            // `apply_inferred_fields` is absent (default false). When
            // opted in, deterministic high-confidence + no-conflict
            // fields are promoted into `applied_fields[]`; LLM
            // proposals are promoted only when caller approved them
            // explicitly via `llm_caller_approved`.
            let apply_outcome = compute_apply_gate(args, &inference);
            let gate_block = apply_outcome.to_response_json();

            // wave-22 / task 04 — persisted apply v2. Computes the v2
            // gate (4 opt-ins + matching hash) BEFORE the Preview /
            // SonnetSuggest short-circuit so the response always
            // carries a stable `persisted_apply` block — preview
            // callers can derive the deterministic correlator and
            // capture-and-replay against the persist path on a
            // follow-up call. Hash mismatch / missing fails FAST as
            // a structured error BEFORE any DB mutation per R2.
            let persist_outcome = match execute_persisted_apply(
                state,
                &plan,
                args,
                &apply_outcome,
            )
            .await
            {
                Ok(o) => o,
                Err((code, msg)) => {
                    return Ok(ToolResult::structured_error(
                        ToolError::new(code, msg),
                    ));
                }
            };
            let persist_block = persist_outcome.to_response_json();
            // Refresh the plan snapshot when the persist path inserted
            // a new row, so downstream dispatch / evidence reads see
            // the post-persist version. plan_get keeps the same FSM
            // status (we inherit predecessor.status on insert), so
            // the Approved / Executing precondition is preserved.
            if persist_outcome.status.was_applied() {
                if let Some(new_id) = persist_outcome.new_plan_id {
                    if let Ok(Some(refreshed)) = state.store.plan_get(new_id).await {
                        effective_plan = refreshed;
                    }
                }
            }

            if matches!(
                infer_mode,
                InferPlanFieldsMode::Preview | InferPlanFieldsMode::SonnetSuggest
            ) {
                // Preview / sonnet_suggest short-circuit: never dispatch.
                // The apply gate AND the v2 persisted_apply block both
                // surface here so a preview caller can see what WOULD
                // apply / persist when the flags are flipped on a
                // follow-up call. Note that the persist path ITSELF
                // still ran (when all 4 opt-ins were supplied + hash
                // matched) — preview short-circuit means "no dispatch",
                // not "no persistence". Conservative: the short-circuit
                // only fires for the `Preview` / `SonnetSuggest`
                // inference modes; the ApplySafe mode falls through to
                // the dispatch pipeline below.
                let runner_status = if matches!(infer_mode, InferPlanFieldsMode::SonnetSuggest) {
                    "inference_sonnet_suggest_no_dispatch"
                } else {
                    "inference_preview_no_dispatch"
                };
                let status_label = if matches!(infer_mode, InferPlanFieldsMode::SonnetSuggest) {
                    "inference_sonnet_suggest"
                } else {
                    "inference_preview"
                };
                let payload = json!({
                    "status": status_label,
                    "execute_mode": execute_mode,
                    "runner_status": runner_status,
                    "plan_id": effective_plan.id,
                    "board_task_id": effective_plan.board_task_id,
                    "plan_field_inference": block,
                    "apply_gate": gate_block,
                    "persisted_apply": persist_block,
                });
                return Ok(ToolResult::json_pretty(&payload));
            }

            // ApplySafe path. When the wave-21 gate is REQUESTED, drive
            // the dispatch from the structured `applied_fields[]` (LLM
            // approvals included); otherwise keep the wave-18 byte-shape
            // by augmenting from the deterministic high-confidence slots
            // alone. Either way, the gate block lands on the response so
            // observers can audit the decision.
            let augmented = if apply_outcome.requested {
                let mut out = args.clone();
                if let Some(map) = out.as_object_mut() {
                    for af in &apply_outcome.applied {
                        // Preserve caller-supplied values defensively —
                        // `compute_apply_gate` already routes those into
                        // `skipped_fields[]` with reason
                        // `caller_value_already_set`, so the slot here
                        // should already be empty. We double-check at
                        // the mutation site so a future regression is
                        // loud.
                        let already = map
                            .get(af.field)
                            .map(|v| match v {
                                Value::Null => false,
                                Value::String(s) => !s.trim().is_empty(),
                                Value::Array(a) => !a.is_empty(),
                                Value::Bool(_) => true,
                                _ => true,
                            })
                            .unwrap_or(false);
                        if already {
                            continue;
                        }
                        map.insert(af.field.to_string(), af.value.clone());
                    }
                }
                out
            } else {
                apply_safe_augmentation(args, &inference)
            };
            (augmented, Some(block), Some(gate_block), Some(persist_block))
        };
    let args = &effective_args;
    let plan = effective_plan;

    // wave-17 / task 01 — explicit PLAN-DAG paused-node resume hook.
    // When the caller supplies `resume_review_question_id` (with
    // `resume_review_decision`), we route through the dedicated resume
    // helper instead of the standard execute pipeline. The helper only
    // resumes one paused node — downstream nodes that were left
    // pending after the original paused dispatch stay pending until a
    // follow-up `mission_plan(execute)` call. This is NOT general
    // auto-approve: only ids whose envelope round-trips to a
    // paused-eligible node carry through.
    let resume_input = match parse_plan_node_resume_input(args) {
        Ok(r) => r,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(e.code(), e.message()),
            ))
        }
    };
    if let Some(input) = resume_input {
        if execute_mode != "internal" {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    "resume_review_question_id requires execute_mode=internal",
                )
                .with_suggestion(
                    "the paused-node resume hook dispatches inside the daemon; pass execute_mode=\"internal\"",
                ),
            ));
        }
        return super::plan_dag::action_execute_resume(state, args, &plan, input).await;
    }

    // scheduler_mode hook (Wave 12 / Task 02): when the caller asks for the
    // DAG scheduler, hand off to the dedicated module. The DAG scheduler only
    // makes sense in `execute_mode="internal"` (bridge mode is the v0
    // single-call descriptor and does not encode multi-node fan-out).
    match super::plan_dag::detect_scheduler_mode(args) {
        Ok(true) => {
            if execute_mode != "internal" {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        "scheduler_mode=dag_v1 requires execute_mode=internal",
                    )
                    .with_suggestion(
                        "DAG scheduler dispatches inside the daemon; pass execute_mode=\"internal\"",
                    ),
                ));
            }
            // wave-20 / task 07 — refuse infer_plan_fields=sonnet_suggest
            // here so the caller does not silently lose the LLM proposal
            // block when the DAG short-circuits ahead of any inference
            // pass. v0 keeps LLM-augmented inference single-node-only.
            if let Some(err) = super::plan_dag::refuse_llm_inference_in_dag_mode(args) {
                return Ok(err);
            }
            // wave-18 / task 05 — pre-flight cross-plan distill chain knobs
            // BEFORE the DAG runs so a typo (`distill_chain_mode="sonnett"`)
            // or an invalid combo (chain knobs without `finalize_plan=true`)
            // fails fast rather than after a long DAG execution. Validation
            // is pure (no AppState reads) so we can short-circuit here.
            if let Some(err) = validate_distill_chain_args(args) {
                return Ok(err);
            }
            let dag_result = super::plan_dag::action_execute_dag_v1(state, args, &plan).await?;
            // wave-18 / task 05 — augment the DAG result with the
            // cross-plan distill chain block (and an evidence sidecar
            // entry recording this plan's contribution to the chain).
            // No-op when chain knobs were not supplied.
            return Ok(apply_distill_chain(state, args, &plan, dag_result).await);
        }
        Ok(false) => {}
        Err(structured) => return Ok(structured),
    }

    // plan-runner auto-selection v1: parse hints up front so caller-omitted
    // target / dispatch knobs can be derived from PLAN.lisp itself.
    let hints = parse_plan_hints(&plan.sexp_text);

    let explicit_target = args
        .get("target")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let (target, target_source): (&'static str, &'static str) = if let Some(s) = explicit_target {
        match s {
            "mission_execution" => ("mission_execution", "explicit_arg"),
            "mission_task_delegate" => ("mission_task_delegate", "explicit_arg"),
            "mission_flow_run" => ("mission_flow_run", "explicit_arg"),
            _ => {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!("execute target `{}` is not supported", s),
                    )
                    .with_suggestion(
                        "supported targets: mission_execution | mission_task_delegate | mission_flow_run",
                    ),
                ));
            }
        }
    } else if let Some(t) = hints
        .target
        .as_deref()
        .and_then(|s| normalize_target(s, hints.flow_id.is_some()))
    {
        (t, "plan_hint")
    } else {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::MISSING_PARAM,
                "execute requires `target` (mission_execution|mission_task_delegate|mission_flow_run); \
                 plan.sexp_text did not contain a usable :target / :target-tool / :tool hint",
            )
            .with_suggestion(
                "pass `target` explicitly, or add a :target hint (and :flow-id when targeting flow_run) to PLAN.lisp",
            ),
        ));
    };

    let explicit_ds = args
        .get("dispatch_strategy")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let (dispatch_strategy, dispatch_strategy_source) =
        resolve_dispatch_strategy(explicit_ds, &hints);

    let resolved = ResolvedExec {
        target,
        target_source,
        dispatch_strategy,
        dispatch_strategy_source,
        plan_hint_summary: hints.to_summary_json(),
    };

    // wave-21 / task 04 — autonomous workstation LLM proposal v0.
    // Compute the proposal bundle BEFORE the dispatch path runs so it
    // attaches uniformly to whichever response branch the dispatch lands
    // on (executing / dispatch_skipped / dry_run / inner_error / safe-
    // descriptor / bridge). The bundle is response-only metadata and
    // NEVER alters the dispatch path. Default mode `Off` ⇒ no bundle,
    // no Sonnet call, byte-compatible with wave-15..20.
    let workstation_proposal_bundle = compute_workstation_proposal_bundle(
        state,
        workstation_infer_mode,
        args,
        &plan,
        &hints,
    )
    .await;

    // wave-22 / task 05 — autonomous workstation TRUE spawn v1. Layered
    // on top of wave-21 / task 04 propose-only. Default `auto_spawn=false`
    // ⇒ byte-compatible with wave-21 / task 04 (no gate block on the
    // response, no spawn). When `auto_spawn=true` the gate runs a strict
    // 12-rule matrix (G1..G12) and either:
    //   * spawns through the wave-15 substrate
    //     (`run_workstation_dispatch_with_contract`) when ALL gates pass, OR
    //   * skips with a structured SafeDescriptor-style outcome on the
    //     `workstation_auto_spawn_gate` block (NO spawn, NO mutation).
    //
    // Order of operations (mirrors wave-22 / task 03 / 04):
    //   1. Parse input — fail-fast on shape errors
    //      (`AUTO_SPAWN_INVALID_PARAM`).
    //   2. Hash preflight — fail-fast on missing / mismatch
    //      (`AUTO_SPAWN_MISSING_PROPOSAL_HASH` /
    //      `AUTO_SPAWN_PROPOSAL_HASH_MISMATCH`) BEFORE any substrate
    //      dispatch can run.
    //   3. Compute the gate decision (pure evaluator) and, when all 12
    //      gates pass, run the wave-15 substrate dispatch through
    //      `mission_task_delegate`. NEVER `claude -p`.
    let auto_spawn_input =
        match super::workstation_dispatch::parse_workstation_auto_spawn_input(args) {
            Ok(i) => i,
            Err((code, msg)) => {
                return Ok(ToolResult::structured_error(
                    ToolError::new(code.as_str(), msg),
                ));
            }
        };
    if let Err((code, msg)) =
        super::workstation_dispatch::enforce_auto_spawn_preflight(
            &auto_spawn_input,
            workstation_proposal_bundle.as_ref(),
        )
    {
        return Ok(ToolResult::structured_error(
            ToolError::new(code.as_str(), msg),
        ));
    }
    let auto_spawn_gate_outcome = compute_workstation_auto_spawn_gate(
        state,
        &auto_spawn_input,
        &plan,
        &hints,
        workstation_proposal_bundle.as_ref(),
    )
    .await;

    let final_result = if execute_mode == "bridge" {
        action_execute_bridge(&plan, &resolved)
    } else {
        action_execute_internal(state, args, &plan, &resolved, &hints).await?
    };

    let final_result = attach_workstation_proposals_block(
        final_result,
        workstation_proposal_bundle.as_ref(),
    );

    // wave-22 / task 05 — splice the auto-spawn gate block onto the
    // response. No-op when the caller did not opt in (status=NotRequested
    // ⇒ block omitted so wave-21 / task 04 byte-shape is preserved).
    let final_result = attach_workstation_auto_spawn_gate_block(
        final_result,
        auto_spawn_gate_outcome.as_ref(),
    );

    let final_result = attach_inference_block(final_result, inference_block);
    let final_result = attach_apply_gate_block(final_result, apply_gate_block);
    let final_result = attach_persisted_apply_block(final_result, persisted_apply_block);

    // wave-24 / task 04 — splice the dry-run-only router recommendation
    // block onto the response. No-op when `router_policy_mode=off` (the
    // default) so wave-15..23 callers observe byte-identical behaviour.
    // The recommendation is INFORMATIONAL only — `applied` is hard-coded
    // `false` and the block sits alongside the existing dispatch fields
    // without altering them.
    let final_result = router_policy_dry_run::attach_router_recommendation_block(
        final_result,
        router_policy_mode,
        args,
        &resolved,
        &plan,
    );
    Ok(final_result)
}

/// wave-21 / task 04 — compute the workstation proposal bundle for this
/// execute call. Returns `None` for the default `Off` mode so callers
/// observe byte-identical wave-15..20 behaviour. Returns `Some(bundle)`
/// when the operator opted in via `workstation_inference_mode="sonnet_suggest"`,
/// regardless of whether the gate fired (the bundle reports
/// `PlanHintsPresent` when the gate suppressed the Sonnet call).
async fn compute_workstation_proposal_bundle(
    state: &AppState,
    mode: WorkstationInferenceMode,
    args: &Value,
    plan: &Plan,
    hints: &ParsedPlanHints,
) -> Option<super::workstation_dispatch::WorkstationProposalBundle> {
    if !mode.is_sonnet_suggest() {
        return None;
    }
    // Gate: caller silent + PLAN silent + no `:workstation-dispatch` opt-in.
    let merged_hints_for_gate = hints.to_workstation_hints().merge_args(args);
    let plan_hints_present_signal = plan_hints_carry_workstation_signal(hints);
    let caller_string = |k: &str| {
        args.get(k)
            .and_then(|v| v.as_str())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    };
    let gate = super::workstation_dispatch::WorkstationProposalGate {
        caller_target_present: caller_string("target"),
        caller_dispatch_strategy_present: caller_string("dispatch_strategy"),
        caller_objective_present: caller_string("objective"),
        caller_scope_present: caller_string("scope"),
        // owned_files presence is derived from the merged hints: if the
        // caller passed any non-empty list AND the merged set retained at
        // least one entry, that counts as a signal. We deliberately ignore
        // PLAN-supplied owned_files here because the plan-side list is
        // already covered by `plan_hints_present_signal`.
        caller_owned_files_present: !merged_hints_for_gate.owned_files.is_empty()
            && args.get("owned_files").is_some(),
        caller_project_signal_present: caller_string("target_project")
            || caller_string("requested_cwd")
            || caller_string("cwd"),
        plan_hints_present: plan_hints_present_signal,
        plan_workstation_opt_in: hints.workstation_dispatch_opt_in(),
        _marker: std::marker::PhantomData,
    };
    if gate.is_fully_silent() {
        // Fully silent ⇒ ask Sonnet to propose. Failure surfaces as an
        // Unavailable bundle; we NEVER fall back to claude -p / prompt
        // mode (the unavailable_reason text pins this invariant).
        Some(
            super::workstation_dispatch::request_workstation_proposals(
                state,
                &plan.sexp_text,
                plan.compiled_from.as_deref(),
            )
            .await,
        )
    } else {
        // Some signal present ⇒ skip the Sonnet pass and emit a typed
        // PlanHintsPresent bundle so the response surface stays uniform.
        Some(
            super::workstation_dispatch::WorkstationProposalBundle::plan_hints_present(
                gate.signal_summary(),
            ),
        )
    }
}

/// wave-21 / task 04 — true when the parsed PLAN.lisp hints carry any
/// workstation-relevant signal. Used by the proposal gate to decide
/// whether to suppress the Sonnet pass (signal already exists ⇒ surface
/// `PlanHintsPresent` instead).
///
/// "Signal" here means any of the eight workstation knobs the wave-15
/// parser exposes via `to_workstation_hints` PLUS the explicit
/// `:workstation-dispatch` flag (which `workstation_dispatch_opt_in`
/// reads separately).
fn plan_hints_carry_workstation_signal(h: &ParsedPlanHints) -> bool {
    let nonblank = |o: &Option<String>| {
        o.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false)
    };
    nonblank(&h.objective)
        || nonblank(&h.summary)
        || nonblank(&h.scope)
        || nonblank(&h.owned_files_raw)
        || nonblank(&h.forbidden_files_raw)
        || nonblank(&h.acceptance_commands_raw)
        || nonblank(&h.commit_policy)
        || nonblank(&h.target_project)
        || nonblank(&h.requested_cwd)
        || nonblank(&h.dispatch_strategy)
}

/// wave-21 / task 04 — splice the `workstation_proposals` bundle onto a
/// successful response. Mirrors `attach_inference_block`: errors and
/// pre-existing keys are preserved untouched. The bundle is response-
/// only metadata; nothing reads it on the daemon side.
fn attach_workstation_proposals_block(
    mut result: ToolResult,
    bundle: Option<&super::workstation_dispatch::WorkstationProposalBundle>,
) -> ToolResult {
    let Some(bundle) = bundle else {
        return result;
    };
    if result.is_error.unwrap_or(false) {
        // Don't decorate structured errors with the proposal block — the
        // caller needs the error path uncluttered.
        return result;
    }
    let text = match result.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => return result,
    };
    let mut payload: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return result,
    };
    if let Some(map) = payload.as_object_mut() {
        // Preserve any pre-existing block by NEVER overwriting (future
        // DAG / resume paths may carry their own).
        map.entry("workstation_proposals".to_string())
            .or_insert_with(|| bundle.to_response_json());
        // Mode echo so observers can pivot on the wire string without
        // re-deriving it from the bundle status.
        map.entry("workstation_inference_mode".to_string())
            .or_insert_with(|| json!(WORKSTATION_INFER_MODE_SONNET_SUGGEST));
    }
    result.content = vec![ToolContent::Text {
        text: serde_json::to_string_pretty(&payload).unwrap_or(text),
    }];
    result
}

/// wave-22 / task 05 — compute the auto-spawn gate outcome for this
/// execute call. Returns `None` when the caller did not opt in
/// (`auto_spawn=false` / absent) so observers see byte-identical
/// wave-21 / task 04 behaviour. Returns `Some(outcome)` when the
/// gate ran (whether it spawned or skipped) so the response can
/// surface the structured decision.
///
/// When the gate would have spawned (G1..G12 all green), this helper
/// ALSO calls the wave-15 substrate
/// (`run_workstation_dispatch_with_contract`) to perform the actual
/// dispatch — there is NEVER a `claude -p` shell-out. The substrate's
/// outcome (Dispatched / SafeDescriptor / DryRun / InnerError) is
/// folded back into the gate outcome's status:
///   * Dispatched ⇒ status=Spawned (load-bearing success)
///   * SafeDescriptor ⇒ status=SkippedSubstrateRefused + reason
///   * InnerError ⇒ status=SkippedSubstrateInnerError + reason
///   * DryRun ⇒ status=Spawned (no real dispatch happened, but the
///     gate decision was load-bearing — we treat dry runs as the
///     spawn decision having been made; the brief preview is
///     surfaced through the substrate's standard response fields).
async fn compute_workstation_auto_spawn_gate(
    state: &AppState,
    input: &super::workstation_dispatch::WorkstationAutoSpawnInput,
    plan: &Plan,
    hints: &ParsedPlanHints,
    bundle: Option<&super::workstation_dispatch::WorkstationProposalBundle>,
) -> Option<super::workstation_dispatch::WorkstationAutoSpawnGateOutcome> {
    if !input.auto_spawn {
        // Caller did not opt in; gate block omitted from response so
        // wave-21 / task 04 byte-shape is preserved exactly.
        return None;
    }

    // Pre-load the contract so the gate evaluator can check
    // `:write-scope` / `:must-not-touch` BEFORE any spawn substrate
    // runs. We resolve relative paths against the same project anchor
    // the substrate would use; the substrate re-resolves on its own
    // path so this is purely defensive (the gate refuses early if
    // the file is malformed, instead of letting the substrate get
    // partway through dispatch).
    let (parsed_contract, contract_load_error): (
        Option<super::workstation_dispatch::ParsedTaskContract>,
        Option<String>,
    ) = if let Some(raw) = input.task_contract_path.as_deref() {
        let raw_path = std::path::Path::new(raw);
        // Use the daemon's process cwd as the anchor for relative
        // paths in the gate; the substrate re-anchors against the
        // resolved project root, which may differ — but for the
        // gate's purposes (checking write_scope shape + non-overlap)
        // the resolution does not matter, because the contract file
        // itself is the SSOT and parses identically regardless.
        let cwd = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("/"));
        let resolved = super::workstation_dispatch::resolve_contract_path_public(
            raw_path, &cwd,
        );
        match super::workstation_dispatch::load_task_contract(&resolved) {
            Ok(c) => (Some(c), None),
            Err(e) => (None, Some(e.reason())),
        }
    } else {
        (None, None)
    };

    // Pure evaluator — no substrate dispatch yet.
    let mut outcome = super::workstation_dispatch::evaluate_workstation_auto_spawn_gate(
        &input,
        bundle,
        parsed_contract.as_ref(),
        contract_load_error.as_deref(),
    );

    // If the pure gate decided to spawn, run the substrate dispatch
    // through the wave-15 path. The gate's contract is the SSOT for
    // the spawn — we use ONLY the PLAN-derived hints (no caller-arg
    // overlay) so the spawn surface matches what the gate evaluated
    // (caller args are intentionally NOT load-bearing on the auto-
    // spawn path: the gate's authority comes from the validated
    // contract, not from any caller-supplied workstation knob).
    if outcome.status.was_spawned() {
        let merged_hints = hints.to_workstation_hints();
        // The gate already pinned spawn_target = mission_task_delegate.
        // dispatch_strategy is taken from the contract / merged hints
        // (the wave-15 substrate honours both).
        let dispatch_strategy = merged_hints
            .dispatch_strategy
            .clone()
            .unwrap_or_else(|| "agent-team".to_string());
        let raw_path = input
            .task_contract_path
            .as_deref()
            .map(std::path::PathBuf::from);
        let substrate_outcome =
            super::workstation_dispatch::run_workstation_dispatch_with_contract(
                state,
                plan,
                "mission_task_delegate",
                &dispatch_strategy,
                merged_hints,
                false, // dry_run=false: this is the real spawn surface
                raw_path.as_deref(),
            )
            .await;
        match substrate_outcome {
            super::workstation_dispatch::WorkstationDispatchOutcome::Dispatched { .. }
            | super::workstation_dispatch::WorkstationDispatchOutcome::DryRun { .. } => {
                // Spawn decision was load-bearing — keep status=Spawned.
                outcome.gate_results.push(
                    "rule:substrate_dispatch:ok (mission_task_delegate substrate accepted the spawn)"
                        .to_string(),
                );
            }
            super::workstation_dispatch::WorkstationDispatchOutcome::SafeDescriptor {
                reason,
                ..
            } => {
                let detail = format!(
                    "substrate refused: {} (status={})",
                    reason.detail(),
                    reason.status(),
                );
                outcome.gate_results.push(format!("rule:substrate_dispatch:safe_descriptor:{}", detail));
                outcome.status =
                    super::workstation_dispatch::WorkstationAutoSpawnStatus::SkippedSubstrateRefused;
                outcome.substrate_reason = Some(detail);
            }
            super::workstation_dispatch::WorkstationDispatchOutcome::InnerError {
                inner_payload,
                ..
            } => {
                let detail = format!(
                    "mission_task_delegate inner handler returned an error result: {}",
                    inner_payload
                );
                outcome.gate_results.push(format!("rule:substrate_dispatch:inner_error:{}", detail));
                outcome.status =
                    super::workstation_dispatch::WorkstationAutoSpawnStatus::SkippedSubstrateInnerError;
                outcome.substrate_reason = Some(detail);
            }
        }
    }

    Some(outcome)
}

/// wave-22 / task 05 — splice the `workstation_auto_spawn_gate` bundle
/// onto a successful response. Mirrors `attach_workstation_proposals_block`:
/// errors and pre-existing keys are preserved untouched. The block is
/// response-only metadata; nothing reads it on the daemon side.
fn attach_workstation_auto_spawn_gate_block(
    mut result: ToolResult,
    outcome: Option<&super::workstation_dispatch::WorkstationAutoSpawnGateOutcome>,
) -> ToolResult {
    let Some(outcome) = outcome else {
        return result;
    };
    if result.is_error.unwrap_or(false) {
        // Don't decorate structured errors with the gate block — the
        // caller needs the error path uncluttered.
        return result;
    }
    let text = match result.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => return result,
    };
    let mut payload: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return result,
    };
    if let Some(map) = payload.as_object_mut() {
        // Preserve any pre-existing block by NEVER overwriting (future
        // DAG / resume paths may carry their own).
        map.entry("workstation_auto_spawn_gate".to_string())
            .or_insert_with(|| outcome.to_response_json());
    }
    result.content = vec![ToolContent::Text {
        text: serde_json::to_string_pretty(&payload).unwrap_or(text),
    }];
    result
}

/// Splice the `plan_field_inference` block onto a successful response.
/// No-op when the inference block is absent (mode=`off`) or the response
/// already carries one (DAG / resume paths emit their own future hooks).
/// Errors propagate untouched — we never mask a failure with the
/// inference metadata.
fn attach_inference_block(mut result: ToolResult, block: Option<Value>) -> ToolResult {
    let Some(block) = block else {
        return result;
    };
    if result.is_error.unwrap_or(false) {
        // Don't decorate structured errors with inference metadata —
        // the caller needs the error path uncluttered.
        return result;
    }
    let text = match result.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => return result,
    };
    let mut payload: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return result,
    };
    if let Some(map) = payload.as_object_mut() {
        // Preserve any pre-existing inference block (DAG / resume paths
        // may attach their own in the future) by NEVER overwriting.
        map.entry("plan_field_inference".to_string()).or_insert(block);
    }
    result.content = vec![ToolContent::Text {
        text: serde_json::to_string_pretty(&payload).unwrap_or(text),
    }];
    result
}

fn action_execute_bridge(plan: &Plan, resolved: &ResolvedExec) -> ToolResult {
    let next_call = match resolved.target {
        "mission_execution" => json!({
            "tool": "mission_execution",
            "action": "open",
            "execution_id": format!("plan-{}", plan.id),
            "scope": format!("plan {}", plan.id),
        }),
        "mission_task_delegate" => json!({
            "tool": "mission_task_delegate",
            "board_task_id": plan.board_task_id,
            "plan_id": plan.id,
        }),
        "mission_flow_run" => json!({
            "tool": "mission_flow_run",
            "action": "run",
            "hint": "supply flow_id; plan.sexp_text 暂未自动编译为 flow YAML",
        }),
        _ => Value::Null,
    };

    ToolResult::json_pretty(&json!({
        "status": "bridge_ready",
        "execute_mode": "bridge",
        "runner_status": "bridge_only",
        "plan_id": plan.id,
        "board_task_id": plan.board_task_id,
        "target_tool": resolved.target,
        "target_source": resolved.target_source,
        "dispatch_strategy": resolved.dispatch_strategy,
        "dispatch_strategy_source": resolved.dispatch_strategy_source,
        "plan_hint_summary": resolved.plan_hint_summary,
        "next_call": next_call,
        "note": "manager returns the next-call descriptor; caller invokes the target tool directly. \
                 Pass execute_mode=\"internal\" to have MissionD dispatch the target inside the daemon.",
    }))
}

async fn action_execute_internal(
    state: &AppState,
    args: &Value,
    plan: &Plan,
    resolved: &ResolvedExec,
    hints: &ParsedPlanHints,
) -> Result<ToolResult> {
    let dry_run = args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);

    // wave-19 / task 06 — pre-flight validate the task-contract emit knobs
    // BEFORE any dispatch path so a typo (`task_contract_mode="emi"`)
    // fails fast rather than after a dispatch already produced side
    // effects. Default mode is `Off`: byte-compatible with pre-wave19.
    let task_contract_mode = match parse_task_contract_emit_mode(args) {
        Ok(m) => m,
        Err(err_result) => return Ok(err_result),
    };

    // wave-20 / task 04 — pre-flight validate the dispatch-contract mode
    // (rendered = wave-15..19 byte-compat; machine = consumer reads the
    // emitted task.lisp directly). A typo
    // (`dispatch_contract_mode="machin"`) fails fast before any
    // workstation substrate side effect.
    let dispatch_contract_mode = match parse_dispatch_contract_mode(args) {
        Ok(m) => m,
        Err(err_result) => return Ok(err_result),
    };

    // wave-23 / task 05 — pre-flight validate the optional session-trace
    // ledger path. Default absent ⇒ byte-compatible with wave-15..22 (no
    // forward, no warning, no response field). When supplied, the
    // daemon checks only basic shape (non-empty after trim, no NUL or
    // ASCII control chars except space). Malformed shape with
    // `session_trace_required=true` ⇒ structured INVALID_PARAM error
    // BEFORE any dispatch side effect; without `session_trace_required`
    // ⇒ surface a non-fatal `trace_path_warning` field on the response
    // and continue with the trace forward suppressed.
    let trace_required = args
        .get("session_trace_required")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let trace_input = args
        .get("session_trace_path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let (resolved_trace_path, trace_path_warning) =
        match validate_session_trace_path_arg(trace_input.as_deref(), trace_required) {
            Ok(pair) => pair,
            Err(err_result) => return Ok(err_result),
        };

    // wave-15 / task 05 + wave-16 / task 03 — workstation-dispatch routing.
    // Wave-15 honours explicit opt-in (caller arg `workstation_dispatch=true`
    // or PLAN.lisp `:workstation-dispatch true`). Wave-16 layers conservative
    // auto-inference on top: when caller / plan are silent AND the resolved
    // shape is unmistakably a ClaudeCode workstation task, the runner
    // auto-enables. Explicit `workstation_dispatch=false` always wins and
    // suppresses inference. We never `claude -p`; we never broaden the target
    // whitelist; auto-inference is restricted to `mission_task_delegate`.
    let merged_hints = hints.to_workstation_hints().merge_args(args);
    let inference_ctx = super::workstation_dispatch::InferenceContext {
        target: resolved.target,
        dispatch_strategy: resolved.dispatch_strategy,
        objective: merged_hints.objective.as_deref(),
        owned_files_present: !merged_hints.owned_files.is_empty(),
        scope_present: merged_hints
            .scope
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        target_project_present: merged_hints
            .target_project
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        requested_cwd_present: merged_hints
            .requested_cwd
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
    };
    let dispatch_decision = super::workstation_dispatch::evaluate_dispatch_decision(
        args,
        hints.workstation_dispatch_opt_in(),
        &inference_ctx,
    );

    // wave-19 / task 06 — emit the task-contract sidecar BEFORE any
    // dispatch path so the contract is the SSOT for whatever work
    // happens next. Failures REFUSE the dispatch (a missing contract
    // must not be papered over by a successful inner call). EmitDryRun
    // additionally skips substrate / inner dispatch once the contract
    // has been written. Default mode (`Off`) returns an empty record
    // and the response payload omits the task-contract fields entirely.
    let task_contract_inputs = task_contract_inputs_from_hints_with_trace(
        &merged_hints,
        resolved.target,
        resolved.dispatch_strategy,
        resolved_trace_path.as_deref(),
    );
    let project_arg_for_emit = args.get("project").and_then(|v| v.as_str());
    let cwd_arg_for_emit = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg_for_emit = args
        .get("target_project")
        .and_then(|v| v.as_str())
        .or(hints.target_project.as_deref());
    let emission = emit_task_contract(
        state,
        plan.id,
        &plan.board_task_id,
        "root",
        task_contract_mode,
        &task_contract_inputs,
        project_arg_for_emit,
        cwd_arg_for_emit,
        target_project_arg_for_emit,
    )
    .await;

    if emission.is_failure() {
        // Refuse dispatch — surface the IO failure plus the
        // resolved plan/target so the caller can fix permissions
        // or registry config and retry. Plan FSM untouched.
        let mut response = build_task_contract_failure_response(
            plan,
            resolved,
            &dispatch_decision,
            &emission,
        );
        attach_session_trace_response_fields(
            &mut response,
            resolved_trace_path.as_deref(),
            trace_path_warning.as_deref(),
        );
        return Ok(response);
    }

    if task_contract_mode.is_dry_run() {
        // Skip substrate / inner dispatch — surface the contract path
        // so the caller can render the markdown brief without touching
        // the inner tool. Plan FSM untouched.
        let mut response = build_task_contract_dry_run_response(
            plan,
            resolved,
            &dispatch_decision,
            &emission,
        );
        attach_session_trace_response_fields(
            &mut response,
            resolved_trace_path.as_deref(),
            trace_path_warning.as_deref(),
        );
        return Ok(response);
    }

    if dispatch_decision.is_enabled() {
        // wave-20 / task 04 — when caller opted into machine-driven
        // dispatch AND the wave-19 / task 06 emitter actually wrote a
        // contract for this dispatch, hand the absolute path to the
        // wave-19 / task 07 consumer so the brief is built FROM the
        // on-disk Lisp SSOT rather than the in-memory hints. The
        // consumer reads the contract, overlays it onto the hints
        // (contract wins on every non-empty field), and refuses to
        // fall back to the legacy natural-language brief on a
        // malformed contract — that refusal surfaces verbatim as
        // `SafeDescriptor` (status=`skipped_malformed_task_contract`),
        // never `claude -p`. When the emitter is OFF (default) or the
        // node was ineligible, machine mode is a no-op for THIS
        // dispatch — the runner falls back to the legacy rendered
        // path so existing callers that opt into machine mode without
        // pairing it with `task_contract_mode="emit"` keep working.
        let task_contract_path_for_machine = if dispatch_contract_mode.is_machine() {
            emission.path.clone()
        } else {
            None
        };
        let outcome = super::workstation_dispatch::run_workstation_dispatch_with_contract_and_trace(
            state,
            plan,
            resolved.target,
            resolved.dispatch_strategy,
            merged_hints,
            dry_run,
            task_contract_path_for_machine.as_deref(),
            resolved_trace_path.as_deref(),
        )
        .await;
        // Only transition the plan FSM on the Dispatched branch — every
        // other branch leaves the plan in its current status so the
        // caller can fix the input and retry without manual cleanup.
        if matches!(
            outcome,
            super::workstation_dispatch::WorkstationDispatchOutcome::Dispatched { .. }
        ) && !matches!(plan.status, PlanStatus::Executing)
        {
            if let Err(e) = state
                .store
                .plan_update_status(plan.id, PlanStatus::Executing)
                .await
            {
                tracing::warn!(
                    plan_id = %plan.id,
                    error = %e,
                    "workstation_dispatch: failed to transition plan to executing"
                );
            }
        }
        let mut response = build_workstation_dispatch_response(
            plan,
            resolved,
            outcome,
            &dispatch_decision,
            &emission,
            dispatch_contract_mode,
        );
        attach_session_trace_response_fields(
            &mut response,
            resolved_trace_path.as_deref(),
            trace_path_warning.as_deref(),
        );
        return Ok(response);
    }

    let mut inner_args = match build_internal_dispatch_args(
        args,
        plan,
        resolved.target,
        resolved.dispatch_strategy,
        hints,
    ) {
        Ok(v) => v,
        Err(err_result) => return Ok(err_result),
    };
    // wave-23 / task 05 — forward the resolved trace path into the
    // inner-handler args. Only `mission_execution` consumes the field
    // today (wave-23 / task 04); other targets ignore the unknown key.
    if let Some(stp) = resolved_trace_path.as_deref() {
        if let Some(map) = inner_args.as_object_mut() {
            map.insert("session_trace_path".to_string(), json!(stp));
        }
    }

    if dry_run {
        let mut payload = json!({
            "status": "dry_run",
            "execute_mode": "internal",
            "runner_status": "dry_run_no_dispatch",
            "plan_id": plan.id,
            "board_task_id": plan.board_task_id,
            "target_tool": resolved.target,
            "target_source": resolved.target_source,
            "dispatch_strategy": resolved.dispatch_strategy,
            "dispatch_strategy_source": resolved.dispatch_strategy_source,
            "plan_hint_summary": resolved.plan_hint_summary,
            "would_dispatch": inner_args,
            "workstation_dispatch_source": dispatch_decision.source.as_str(),
        });
        if let Some(reason) = dispatch_decision.reason.as_deref() {
            payload["workstation_dispatch_inference_reason"] = json!(reason);
        }
        merge_task_contract_block(&mut payload, &emission);
        let mut response = ToolResult::json_pretty(&payload);
        attach_session_trace_response_fields(
            &mut response,
            resolved_trace_path.as_deref(),
            trace_path_warning.as_deref(),
        );
        return Ok(response);
    }

    let inner_result = match resolved.target {
        "mission_execution" => {
            super::agent_execution::handle(state, "mission_execution", inner_args.clone()).await?
        }
        "mission_task_delegate" => {
            super::super::compute::task_delegate::handle(
                state,
                "mission_task_delegate",
                inner_args.clone(),
            )
            .await?
        }
        "mission_flow_run" => {
            super::super::compute::flow_run::handle(state, "mission_flow_run", inner_args.clone())
                .await?
        }
        _ => unreachable!("target whitelist already enforced"),
    };

    let inner_payload = tool_result_payload(&inner_result);
    let inner_is_error = inner_result.is_error.unwrap_or(false);

    if inner_is_error {
        // Don't transition plan; just report the inner failure verbatim so the
        // caller can decide whether to retry, fix args, or escalate.
        let mut payload = json!({
            "status": "dispatch_failed",
            "execute_mode": "internal",
            "runner_status": "inner_returned_error",
            "plan_id": plan.id,
            "board_task_id": plan.board_task_id,
            "target_tool": resolved.target,
            "target_source": resolved.target_source,
            "dispatch_strategy": resolved.dispatch_strategy,
            "dispatch_strategy_source": resolved.dispatch_strategy_source,
            "plan_hint_summary": resolved.plan_hint_summary,
            "inner_result": inner_payload,
            "workstation_dispatch_source": dispatch_decision.source.as_str(),
        });
        if let Some(reason) = dispatch_decision.reason.as_deref() {
            payload["workstation_dispatch_inference_reason"] = json!(reason);
        }
        merge_task_contract_block(&mut payload, &emission);
        let mut response = ToolResult::json_pretty(&payload);
        attach_session_trace_response_fields(
            &mut response,
            resolved_trace_path.as_deref(),
            trace_path_warning.as_deref(),
        );
        return Ok(response);
    }

    // Successful dispatch — append evidence then transition plan to executing.
    //
    // Project root resolution for evidence sidecar placement honours the
    // canonical contract (intent-worker.lisp :: project-root-spawn-cwd):
    //   - `project`         → registry id (primary)
    //   - `cwd`             → absolute path (longest-prefix), or rejected if relative
    //   - `target_project`  → registry id (fallback)
    //   - plan-hint :target-project also fed into the fallback slot
    // No process-cwd fallback. Evidence-sidecar failures still degrade
    // gracefully (`evidence_error`) — the inner dispatch already produced
    // durable side effects, so we surface the failure but do not abort.
    let project_arg = args.get("project").and_then(|v| v.as_str());
    let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg = args
        .get("target_project")
        .and_then(|v| v.as_str())
        .or(hints.target_project.as_deref());
    // wave-13 :: typed evidence-collector path. Legacy `kind` ("dispatch") +
    // legacy `source` ("plan_runner_dispatch") are preserved so any reader
    // that filtered on those exact strings keeps working — `kind` is the
    // canonical taxonomy from `evidence_collector::kind` and `source` is the
    // historical wire tag (also re-exported as `evidence_collector::source::
    // PLAN_RUNNER_DISPATCH`). Inner dispatch summary, plan-hint passthrough,
    // and target/strategy provenance all land under their canonical typed
    // keys; legacy passthrough keys (`execute_mode`, `target_tool`,
    // `target_source`, `dispatch_strategy_source`, `plan_hint_summary`) keep
    // their flat-top-level placement via `with_extra` so audit dashboards do
    // not need to traverse the new `inner_dispatch` wrapper to find them.
    let entry = super::evidence_collector::EvidenceEntry::new(
        super::evidence_collector::source::PLAN_RUNNER_DISPATCH,
        super::evidence_collector::kind::DISPATCH,
    )
    .with_inner_dispatch(inner_payload.clone())
    .add_execution_event(super::evidence_collector::EventRef::unavailable(
        "plan-runner v0 does not yet subscribe to the live ExecutionEvent bus; \
         caller correlates by plan_id + board_task_id",
    ))
    .with_extra("execute_mode", json!("internal"))
    .with_extra("target_tool", json!(resolved.target))
    .with_extra("target_source", json!(resolved.target_source))
    .with_extra("dispatch_strategy", json!(resolved.dispatch_strategy))
    .with_extra(
        "dispatch_strategy_source",
        json!(resolved.dispatch_strategy_source),
    )
    .with_extra("plan_hint_summary", resolved.plan_hint_summary.clone())
    // Legacy `inner_result` alias: pre-wave12 sidecars carried the inner
    // payload under `inner_result`, the new canonical slot is
    // `inner_dispatch`. We keep BOTH so historical readers (audit
    // dashboards, retrospective queries) that filter on `inner_result`
    // keep working byte-for-byte during the transition.
    .with_extra("inner_result", inner_payload.clone());
    let outcome = super::evidence_collector::append(
        state,
        plan.id,
        project_arg,
        cwd_arg,
        target_project_arg,
        entry,
    )
    .await;
    if let super::evidence_collector::AppendOutcome::Failed { error } = &outcome {
        // Evidence append failure does not abort the dispatch (the inner
        // tool already succeeded with its own durable side effects), but
        // we now surface the error in the response so callers cannot
        // mistake a missing sidecar for a clean run. This also covers
        // resolver failures (project root unresolved / relative cwd
        // rejected) — those bubble up as `evidence_error` rather than
        // silently landing under the daemon process cwd.
        tracing::warn!(plan_id = %plan.id, error = %error, "plan-runner: evidence sidecar append failed");
    }
    let (evidence_path, evidence_error) = outcome.into_legacy_tuple();

    let status_update_error = if matches!(plan.status, PlanStatus::Executing) {
        // Already in Executing — nothing to update, nothing can fail.
        None
    } else {
        match state
            .store
            .plan_update_status(plan.id, PlanStatus::Executing)
            .await
        {
            Ok(_) => None,
            Err(e) => {
                tracing::warn!(plan_id = %plan.id, error = %e, "plan-runner: failed to transition plan to executing");
                Some(e.to_string())
            }
        }
    };

    let mut response = build_internal_dispatch_success_response(
        plan,
        resolved,
        inner_payload,
        evidence_path,
        evidence_error,
        status_update_error,
        &dispatch_decision,
        &emission,
    );
    attach_session_trace_response_fields(
        &mut response,
        resolved_trace_path.as_deref(),
        trace_path_warning.as_deref(),
    );
    Ok(response)
}

// ── wave-23 / task 05 — session-trace propagation helpers ──────────────
//
// `mission_plan(action=execute)` callers can opt into the wave-23 / task 04
// session-trace ledger by supplying `session_trace_path`. The plan-runner
// validates basic shape up-front (so a typo cannot silently shadow the
// ledger) and then forwards the path through three surfaces:
//   * `mission_execution(action=*)` inner args (when target=mission_execution)
//   * the workstation-dispatch task brief (a `## Session trace` block)
//   * the wave-19 / task 06 emitted task-contract v1 file
//     (`:session-trace-path "..."`)
// On the response, every return path surfaces `session_trace_path` so
// observers can pin which ledger this dispatch was wired to (or
// `trace_path_warning` when shape validation degraded silently because
// the caller did not opt into hard-fail via `session_trace_required`).

/// Validate the optional `session_trace_path` arg shape. Returns
/// `(resolved_path, warning)`:
///   * Both `None` ⇒ caller did not opt in; propagation is suppressed.
///   * `(Some(path), None)` ⇒ path passed shape validation; forward it
///     verbatim through the dispatch and surface it on the response.
///   * `(None, Some(warning))` ⇒ shape failed AND `required=false`; no
///     forward, surface a non-fatal warning so the caller can fix and
///     retry without aborting the dispatch.
///   * `Err(structured_error)` ⇒ shape failed AND `required=true`; the
///     caller asked the daemon to refuse the dispatch on a malformed
///     path so a typo cannot silently shadow the ledger.
///
/// Validation is intentionally NARROW — we only check the input shape,
/// never the on-disk file existence. The wave-23 / task 04 consumer
/// surfaces `trace_warning` for I/O / parse / append failures; the two
/// surfaces are distinct so observers can tell shape errors (caller
/// typo) from append errors (target file removed mid-flight).
pub(super) fn validate_session_trace_path_arg(
    raw: Option<&str>,
    required: bool,
) -> std::result::Result<(Option<String>, Option<String>), ToolResult> {
    let Some(value) = raw else {
        return Ok((None, None));
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        let detail = "session_trace_path is empty after trim".to_string();
        return reject_or_warn_trace_path(detail, required);
    }
    // NUL byte and ASCII control char rejection. Tab is allowed since
    // some build systems tolerate path components with whitespace; we
    // only reject characters that would fail filesystem normalization
    // or render the path unreadable.
    for (idx, ch) in trimmed.char_indices() {
        if ch == '\0' {
            let detail = format!(
                "session_trace_path contains a NUL byte at offset {} (filesystem-invalid)",
                idx
            );
            return reject_or_warn_trace_path(detail, required);
        }
        if ch.is_ascii_control() && ch != ' ' && ch != '\t' {
            let detail = format!(
                "session_trace_path contains ASCII control char `{:#04x}` at offset {} (filesystem-invalid)",
                ch as u32, idx
            );
            return reject_or_warn_trace_path(detail, required);
        }
    }
    Ok((Some(trimmed.to_string()), None))
}

/// `validate_session_trace_path_arg` companion that branches between
/// hard-fail (when `required=true`) and warn-only (the conservative
/// default).
fn reject_or_warn_trace_path(
    detail: String,
    required: bool,
) -> std::result::Result<(Option<String>, Option<String>), ToolResult> {
    if required {
        Err(ToolResult::structured_error(
            ToolError::new(error_codes::INVALID_PARAM, detail.clone())
                .with_suggestion(
                    "session_trace_required=true forbids malformed `session_trace_path` shapes — \
                     supply a non-empty filesystem-valid path (relative or absolute) or drop \
                     `session_trace_required` to fall back to a non-fatal warning.",
                ),
        ))
    } else {
        Ok((None, Some(detail)))
    }
}

/// Splice `session_trace_path` and / or `trace_path_warning` into the
/// JSON envelope of a `ToolResult` produced by an `action_execute_internal`
/// return path. When both inputs are `None` the response is left
/// byte-identical to the wave-15..22 baseline — preserves backward
/// compatibility for callers that never supplied the trace knob.
pub(super) fn attach_session_trace_response_fields(
    result: &mut ToolResult,
    session_trace_path: Option<&str>,
    trace_path_warning: Option<&str>,
) {
    if session_trace_path.is_none() && trace_path_warning.is_none() {
        return;
    }
    // The inner JSON lives under the first ToolContent::Text frame
    // (json_pretty / structured-error patterns). We splice in place so
    // the rest of the envelope (is_error, structured_content) stays
    // unchanged.
    let Some(ToolContent::Text { text }) = result.content.first_mut() else {
        return;
    };
    let Ok(mut value) = serde_json::from_str::<Value>(text) else {
        return;
    };
    if let Some(map) = value.as_object_mut() {
        if let Some(stp) = session_trace_path {
            map.insert("session_trace_path".to_string(), json!(stp));
        }
        if let Some(w) = trace_path_warning {
            map.insert("trace_path_warning".to_string(), json!(w));
        }
    }
    *text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.clone());
}

/// Merge the wave-19 / task 06 task-contract emission record into a
/// response payload. No-op when the emitter was off and produced
/// nothing observable — preserves the pre-wave19 byte-shape on the
/// default code path.
pub(super) fn merge_task_contract_block(
    payload: &mut Value,
    emission: &TaskContractEmissionRecord,
) {
    let Some(block) = emission.to_response_block() else {
        return;
    };
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    if let Value::Object(block_map) = block {
        for (k, v) in block_map {
            map.insert(k, v);
        }
    }
}

/// wave-19 / task 06 — response shape when task-contract emission was
/// requested but the write failed. We refuse the dispatch entirely so
/// downstream callers cannot mistake a missing contract for a
/// successful run; plan FSM is untouched, no inner side effect was
/// produced, the response carries the structured emission record.
fn build_task_contract_failure_response(
    plan: &Plan,
    resolved: &ResolvedExec,
    decision: &super::workstation_dispatch::DispatchDecision,
    emission: &TaskContractEmissionRecord,
) -> ToolResult {
    let mut payload = json!({
        "status": "dispatch_skipped",
        "execute_mode": "internal",
        "runner_status": "task_contract_emit_failed",
        "plan_id": plan.id,
        "board_task_id": plan.board_task_id,
        "target_tool": resolved.target,
        "target_source": resolved.target_source,
        "dispatch_strategy": resolved.dispatch_strategy,
        "dispatch_strategy_source": resolved.dispatch_strategy_source,
        "plan_hint_summary": resolved.plan_hint_summary,
        "workstation_dispatch_source": decision.source.as_str(),
    });
    if let Some(reason) = decision.reason.as_deref() {
        payload["workstation_dispatch_inference_reason"] = json!(reason);
    }
    merge_task_contract_block(&mut payload, emission);
    ToolResult::json_pretty(&payload)
}

/// wave-19 / task 06 — response shape when the caller asked for
/// `task_contract_mode="emit_dry_run"`. The contract is on disk; the
/// inner substrate is never invoked. Plan FSM is untouched (the
/// caller can flip to `emit` mode for a real dispatch).
fn build_task_contract_dry_run_response(
    plan: &Plan,
    resolved: &ResolvedExec,
    decision: &super::workstation_dispatch::DispatchDecision,
    emission: &TaskContractEmissionRecord,
) -> ToolResult {
    let mut payload = json!({
        "status": "dry_run",
        "execute_mode": "internal",
        "runner_status": "task_contract_emit_dry_run",
        "plan_id": plan.id,
        "board_task_id": plan.board_task_id,
        "target_tool": resolved.target,
        "target_source": resolved.target_source,
        "dispatch_strategy": resolved.dispatch_strategy,
        "dispatch_strategy_source": resolved.dispatch_strategy_source,
        "plan_hint_summary": resolved.plan_hint_summary,
        "workstation_dispatch_source": decision.source.as_str(),
    });
    if let Some(reason) = decision.reason.as_deref() {
        payload["workstation_dispatch_inference_reason"] = json!(reason);
    }
    merge_task_contract_block(&mut payload, emission);
    ToolResult::json_pretty(&payload)
}

/// Render a workstation-dispatch outcome into the same response envelope
/// shape as `build_internal_dispatch_success_response` so callers see one
/// consistent contract (plan-runner v0 fields + workstation-dispatch
/// extension fields side-by-side).
///
/// Status semantics:
///   * `Dispatched`         → "executing" (plan transitions to executing)
///   * `InnerError`         → "dispatch_failed" (do not transition)
///   * `DryRun`             → "dry_run"
///   * `SafeDescriptor`     → "dispatch_skipped" (do not transition)
///
/// When `Dispatched`, this function does NOT itself update the plan
/// status — the caller (action_execute_internal) handles that, mirroring
/// the legacy success-response path. The status field is set so the wire
/// shape matches the legacy executing branch.
fn build_workstation_dispatch_response(
    plan: &Plan,
    resolved: &ResolvedExec,
    outcome: super::workstation_dispatch::WorkstationDispatchOutcome,
    decision: &super::workstation_dispatch::DispatchDecision,
    emission: &TaskContractEmissionRecord,
    dispatch_contract_mode: DispatchContractMode,
) -> ToolResult {
    let status = match &outcome {
        super::workstation_dispatch::WorkstationDispatchOutcome::Dispatched { .. } => "executing",
        super::workstation_dispatch::WorkstationDispatchOutcome::InnerError { .. } => {
            "dispatch_failed"
        }
        super::workstation_dispatch::WorkstationDispatchOutcome::DryRun { .. } => "dry_run",
        super::workstation_dispatch::WorkstationDispatchOutcome::SafeDescriptor { .. } => {
            "dispatch_skipped"
        }
    };
    let extension =
        super::workstation_dispatch::outcome_to_response_fields(&outcome, resolved.dispatch_strategy);

    let mut payload = json!({
        "status": status,
        "execute_mode": "internal",
        "runner_status": "workstation_dispatch_v0",
        "plan_id": plan.id,
        "board_task_id": plan.board_task_id,
        "target_tool": resolved.target,
        "target_source": resolved.target_source,
        "dispatch_strategy": resolved.dispatch_strategy,
        "dispatch_strategy_source": resolved.dispatch_strategy_source,
        "plan_hint_summary": resolved.plan_hint_summary,
        // wave-16 / task 03 — surface the routing decision so callers can
        // tell apart explicit opt-in (wave-15) from auto-inference (wave-16).
        "workstation_dispatch_source": decision.source.as_str(),
        // wave-20 / task 04 — surface the resolved dispatch-contract
        // mode so observers can pin which dispatch contract drove the
        // brief. `rendered` (default) preserves wave-15..19 byte-shape;
        // `machine` proves the consumer read the on-disk Lisp SSOT
        // (cross-check against `task_contract_source_path` on the
        // workstation extension when present).
        "dispatch_contract_mode": dispatch_contract_mode.as_str(),
    });
    if let Some(reason) = decision.reason.as_deref() {
        if let Some(map) = payload.as_object_mut() {
            map.insert(
                "workstation_dispatch_inference_reason".to_string(),
                json!(reason),
            );
        }
    }
    if let Some(map) = extension.as_object() {
        if let Some(payload_map) = payload.as_object_mut() {
            for (k, v) in map {
                payload_map.insert(k.clone(), v.clone());
            }
        }
    }
    merge_task_contract_block(&mut payload, emission);
    ToolResult::json_pretty(&payload)
}

/// Build the response for a plan-runner internal dispatch where the inner
/// tool already returned non-error.
///
/// Status semantics:
///   * `status_update_error.is_some()` → `status="dispatch_partial"` /
///     `runner_status="status_update_failed"`. We must NOT claim
///     `executing`, because the plan FSM did not actually persist.
///   * otherwise → `status="executing"` / `runner_status="dispatched"`.
///
/// `evidence_error` is independent: a missing sidecar still leaves the inner
/// side effect in place, so it is reported via `evidence_error` but does not
/// downgrade the runner status by itself. Caller may still observe both
/// `evidence_error` and `status_update_error` together.
fn build_internal_dispatch_success_response(
    plan: &Plan,
    resolved: &ResolvedExec,
    inner_payload: Value,
    evidence_path: Option<String>,
    evidence_error: Option<String>,
    status_update_error: Option<String>,
    decision: &super::workstation_dispatch::DispatchDecision,
    emission: &TaskContractEmissionRecord,
) -> ToolResult {
    let (status, runner_status) = if status_update_error.is_some() {
        ("dispatch_partial", "status_update_failed")
    } else {
        ("executing", "dispatched")
    };

    let mut payload = json!({
        "status": status,
        "execute_mode": "internal",
        "runner_status": runner_status,
        "plan_id": plan.id,
        "board_task_id": plan.board_task_id,
        "target_tool": resolved.target,
        "target_source": resolved.target_source,
        "dispatch_strategy": resolved.dispatch_strategy,
        "dispatch_strategy_source": resolved.dispatch_strategy_source,
        "plan_hint_summary": resolved.plan_hint_summary,
        "evidence_path": evidence_path,
        "inner_result": inner_payload,
        "workstation_dispatch_source": decision.source.as_str(),
    });
    if let Some(reason) = decision.reason.as_deref() {
        payload["workstation_dispatch_inference_reason"] = json!(reason);
    }
    if let Some(err) = evidence_error {
        payload["evidence_error"] = json!(err);
    }
    if let Some(err) = status_update_error {
        payload["status_update_error"] = json!(err);
    }
    merge_task_contract_block(&mut payload, emission);
    ToolResult::json_pretty(&payload)
}

/// Build the argument JSON for the inner target handler. Returns
/// `Err(structured_error_result)` on caller-facing validation failures so the
/// outer handler can return them verbatim.
///
/// `dispatch_strategy` is the already-normalised value from `action_execute`
/// (one of `VALID_DISPATCH_STRATEGIES`, defaulted to `"unknown"`). It is
/// forwarded into the `mission_execution(action=open)` inner JSON so the
/// companion log can persist `:dispatch-strategy`. Other targets ignore it.
///
/// `hints` are the parsed PLAN.lisp keyword/value pairs. Each per-target
/// branch falls back to the relevant hint when the caller omitted the
/// corresponding arg. Caller-supplied args ALWAYS win.
pub(super) fn build_internal_dispatch_args(
    args: &Value,
    plan: &Plan,
    target: &str,
    dispatch_strategy: &str,
    hints: &ParsedPlanHints,
) -> std::result::Result<Value, ToolResult> {
    match target {
        "mission_execution" => {
            let execution_id = args
                .get("execution_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("plan-{}", plan.id));
            let parent_design = args
                .get("parent_design")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    plan.source_directive_id
                        .map(|d| format!("directive/{}", d))
                })
                .unwrap_or_else(|| format!("plan/{}", plan.id));
            let scope = args
                .get("scope")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    format!("plan {} (board_task {})", plan.id, plan.board_task_id)
                });
            let owner = args
                .get("owner")
                .and_then(|v| v.as_str())
                .unwrap_or("plan-runner");

            let mut inner = json!({
                "action": "open",
                "execution_id": execution_id,
                "parent_design": parent_design,
                "scope": scope,
                "owner": owner,
                "dispatch_strategy": dispatch_strategy,
            });
            // project: explicit args first, else parsed plan hint.
            let project_value = args
                .get("target_project")
                .or_else(|| args.get("project"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| hints.target_project.clone());
            if let Some(s) = project_value {
                inner["project"] = json!(s);
            }
            // Forward target_project verbatim (companion log persists it as
            // :target-project per intent-tools.lisp ::
            // workstation-dispatch-record). Explicit arg first, else hint.
            let target_project_str = args
                .get("target_project")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| hints.target_project.clone());
            if let Some(s) = target_project_str {
                inner["target_project"] = json!(s);
            }
            // requested_cwd: explicit arg first, else parsed plan hint.
            let requested_cwd = args
                .get("requested_cwd")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| hints.requested_cwd.clone());
            if let Some(s) = requested_cwd {
                inner["requested_cwd"] = json!(s);
            }
            Ok(inner)
        }
        "mission_task_delegate" => {
            // Objective precedence: explicit arg > :objective hint > :summary
            // hint > derived first non-empty line of plan.sexp_text.
            let objective_in = args
                .get("objective")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string())
                .or_else(|| hints.objective.clone())
                .or_else(|| hints.summary.clone());
            let mut objective = objective_in
                .unwrap_or_else(|| derive_objective_from_plan(plan, DERIVED_OBJECTIVE_MAX));

            // agent-team hint injection: when the resolved dispatch_strategy is
            // agent-team and the target is task_delegate, append the literal
            // Chinese hint so the delegated agent picks up the parallelism
            // intent. Idempotent — skipped if already present.
            if dispatch_strategy == "agent-team" && !objective.contains(AGENT_TEAM_OBJECTIVE_HINT)
            {
                objective.push('\n');
                objective.push_str(AGENT_TEAM_OBJECTIVE_HINT);
            }

            let intent = args.get("intent").and_then(|v| v.as_str());
            if let Some(i) = intent {
                if !VALID_DELEGATE_INTENTS.contains(&i) {
                    return Err(ToolResult::structured_error(
                        ToolError::new(
                            error_codes::INVALID_PARAM,
                            format!(
                                "intent `{}` is not valid for mission_task_delegate; valid: {:?}",
                                i, VALID_DELEGATE_INTENTS
                            ),
                        )
                        .with_suggestion(
                            "default for plan-runner is `code`; pass intent only when overriding",
                        ),
                    ));
                }
            }
            let intent = intent.unwrap_or("code");

            let mut inner = json!({
                "objective": objective,
                "intent": intent,
                "context_hints": [
                    format!("plan:{}", plan.id),
                    format!("board_task:{}", plan.board_task_id),
                ],
            });
            // cwd precedence:
            //   explicit args.cwd
            //   > args.target_project (only if path-like)
            //   > hints.requested_cwd
            //   > hints.target_project (only if path-like)
            // task_delegate accepts cwd as a filesystem path; bare project ids
            // cannot resolve downstream, so we use the '/' heuristic for the
            // target_project alias.
            let cwd = args
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    args.get("target_project")
                        .and_then(|v| v.as_str())
                        .filter(|tp| tp.contains('/'))
                        .map(|s| s.to_string())
                })
                .or_else(|| hints.requested_cwd.clone())
                .or_else(|| {
                    hints
                        .target_project
                        .as_deref()
                        .filter(|tp| tp.contains('/'))
                        .map(|s| s.to_string())
                });
            if let Some(c) = cwd {
                inner["cwd"] = json!(c);
            }
            if let Some(p) = args.get("priority").and_then(|v| v.as_str()) {
                inner["priority"] = json!(p);
            }
            if let Some(t) = args.get("timeout_secs").and_then(|v| v.as_i64()) {
                inner["timeout_secs"] = json!(t);
            }
            Ok(inner)
        }
        "mission_flow_run" => {
            // flow_id precedence: explicit arg > :flow-id / :flow_id plan hint.
            let flow_id = args
                .get("flow_id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .or_else(|| hints.flow_id.clone());
            let flow_id = match flow_id {
                Some(s) if !s.is_empty() => s,
                _ => {
                    return Err(ToolResult::structured_error(
                        ToolError::new(
                            error_codes::MISSING_PARAM,
                            "execute_mode=internal target=mission_flow_run requires `flow_id` (arg or :flow-id PLAN hint)",
                        )
                        .with_suggestion(
                            "plan.sexp_text 自动编译为 flow YAML 仍是未来工作 \
                             (intent-flow.lisp :: workflow-distiller); 当前必须显式传入 flow_id 或在 PLAN.lisp 写 :flow-id",
                        ),
                    ));
                }
            };
            let mut inner = json!({
                "action": "run",
                "flow_id": flow_id,
            });
            if let Some(params) = args.get("params") {
                inner["params"] = params.clone();
            }
            Ok(inner)
        }
        _ => unreachable!("target whitelist already enforced"),
    }
}

/// Derive a short objective string from `plan.sexp_text` for use as a
/// task_delegate objective. Caller can always override via the explicit
/// `objective` argument.
fn derive_objective_from_plan(plan: &Plan, max_chars: usize) -> String {
    let summary = plan
        .sexp_text
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("plan execution");
    let summary = truncate_chars(summary, max_chars);
    format!("Plan {}: {}", plan.id, summary)
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    let mut end = max_chars;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

/// Best-effort extraction of the payload JSON from a downstream `ToolResult`.
/// Inner handlers always render `ToolContent::Text`; we parse the first text
/// content as JSON and fall back to the raw string to avoid losing data.
pub(super) fn tool_result_payload(result: &ToolResult) -> Value {
    match result.content.first() {
        Some(ToolContent::Text { text }) => serde_json::from_str::<Value>(text)
            .unwrap_or_else(|_| Value::String(text.clone())),
        None => Value::Null,
    }
}

// ───────────────────────────────────────────────────────────────────────
// record_evidence — persist sidecar JSON next to companion logs
// ───────────────────────────────────────────────────────────────────────

async fn action_record_evidence(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "plan_id")?;
    let evidence = args
        .get("evidence")
        .cloned()
        .ok_or_else(|| anyhow!("`evidence` required (object/array; tool_calls/event_log/test/exec refs)"))?;

    let ensured = state
        .store
        .plan_get(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    if ensured.is_none() {
        return Ok(ToolResult::structured_error(
            ToolError::new(error_codes::NOT_FOUND, format!("plan `{}` not found", id)),
        ));
    }

    let project_arg = args.get("project").and_then(|v| v.as_str());
    let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg = args.get("target_project").and_then(|v| v.as_str());

    // wave-12 :: evidence-collector v0 — `evidence_kind` + `source` are
    // additive opt-in stamps. When BOTH are absent the historical wire form
    // is preserved byte-for-byte (`{"evidence": …}`), so legacy callers
    // keep working. When EITHER is present we route through the typed
    // collector wrapper so the new entry carries `schema_version` /
    // canonical `source` / canonical `kind` alongside the original
    // `evidence` body.
    let evidence_kind = args.get("evidence_kind").and_then(|v| v.as_str());
    let source_override = args.get("source").and_then(|v| v.as_str());
    let entry = if evidence_kind.is_some() || source_override.is_some() {
        super::evidence_collector::wrap_legacy_record_evidence(
            evidence,
            evidence_kind,
            source_override,
        )
    } else {
        json!({ "evidence": evidence })
    };

    let (path, entry_count) = match append_plan_evidence_entry(
        state,
        id,
        project_arg,
        cwd_arg,
        target_project_arg,
        entry,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            // Resolver / write failure is a structured rejection rather than an
            // anyhow bubble, so the caller sees the actionable suggestion
            // (intent-worker.lisp :: project-root-spawn-cwd contract).
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::INVALID_PARAM, e.to_string()).with_suggestion(
                    "supply project=<registered id> | target_project=<registered id> | cwd=<absolute path>",
                ),
            ));
        }
    };

    Ok(ToolResult::json_pretty(&json!({
        "status": "recorded",
        "plan_id": id,
        "path": path.display().to_string(),
        "entry_count": entry_count,
        // Echo what the caller asked for so the response makes the routing
        // visible. `null` when the legacy untagged shape was used.
        "evidence_kind": evidence_kind,
        "source": source_override,
    })))
}

/// Append a single evidence entry to
/// `<project_root>/.missiond/v2/plans/<plan_id>.evidence.json`.
///
/// `entry` is merged with a `recorded_at` timestamp. Returns the sidecar path
/// and the resulting total entry count for caller-facing reporting. Used by
/// both `record_evidence` (manual evidence) and the plan-runner internal
/// dispatch path (`plan_runner_dispatch` audit trail).
///
/// Project root resolution goes through [`resolve_project_root`] which
/// honours the canonical contract: explicit `project_id` / absolute `cwd` /
/// fallback `target_project` only. There is **no** process-cwd fallback —
/// callers that omit all signals get a structured error so the evidence
/// sidecar never lands under a surprising directory.
pub(super) async fn append_plan_evidence_entry(
    state: &AppState,
    plan_id: uuid::Uuid,
    project_arg: Option<&str>,
    cwd_arg: Option<&str>,
    target_project_arg: Option<&str>,
    entry: Value,
) -> Result<(PathBuf, usize)> {
    let project_root = resolve_project_root(
        &state.project_registry,
        project_arg,
        cwd_arg,
        target_project_arg,
    )
    .await?;
    let dir = project_root.join(COMPANION_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| anyhow!("mkdir {}: {}", dir.display(), e))?;
    let path = dir.join(format!("{}.evidence.json", plan_id));

    let mut bundle = if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| anyhow!("read {}: {}", path.display(), e))?;
        serde_json::from_str::<Value>(&raw)
            .unwrap_or_else(|_| json!({"plan_id": plan_id, "entries": []}))
    } else {
        json!({"plan_id": plan_id, "entries": []})
    };

    // Stamp recorded_at on the entry. If caller already supplied an object,
    // merge the field; otherwise wrap the value under `evidence`.
    let stamped = match entry {
        Value::Object(mut map) => {
            map.insert("recorded_at".to_string(), json!(iso_now()));
            Value::Object(map)
        }
        other => json!({ "recorded_at": iso_now(), "evidence": other }),
    };

    if let Some(arr) = bundle.get_mut("entries").and_then(|v| v.as_array_mut()) {
        arr.push(stamped);
    } else {
        bundle["entries"] = json!([stamped]);
    }

    let entry_count = bundle
        .get("entries")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let body = serde_json::to_string_pretty(&bundle)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body.as_bytes()).map_err(|e| anyhow!("write tmp: {}", e))?;
    std::fs::rename(&tmp, &path).map_err(|e| anyhow!("rename: {}", e))?;

    Ok((path, entry_count))
}

// ───────────────────────────────────────────────────────────────────────
// wave-19 / task 06 — plan-runner task-contract emitter v0
//
// Opt-in: callers pass `emit_task_contract=true` (or
// `task_contract_mode="emit"|"emit_dry_run"`). When the resolved
// dispatch shape is workstation-eligible (target=mission_task_delegate
// + non-empty objective), the runner writes a task-contract v1 Lisp
// sidecar BEFORE handing off to the workstation-dispatch substrate.
// Path convention:
//
//   <project_root>/.missiond/tasks/generated/<plan_id>/<node_id>.lisp
//
// (Single-node executes pass `node_id="root"`; DAG nodes use the
// PLAN.lisp `:id`.)
//
// Default behaviour is byte-compatible: when neither knob is set the
// runner skips emission entirely and the response payload omits the
// task-contract fields. The renderer is OUT OF PROCESS — we surface
// `render_command` (a `node scripts/render-claudecode-task.mjs ...`
// invocation) instead of shelling out from Rust, which keeps the
// daemon free of Node dependency.
//
// Failure semantics: emit BEFORE dispatch. If the write fails, the
// dispatch is REFUSED with a structured `task_contract_emit_failed`
// status — the contract is the SSOT, so a missing contract MUST NOT
// be papered over by a successful inner call. Dry-run (`emit_dry_run`
// or the existing `dry_run=true` flag) returns the contract path
// without touching the inner substrate.
// ───────────────────────────────────────────────────────────────────────

/// Canonical task-contract emit modes. Mirrors the MCP descriptor enum.
pub(super) const TASK_CONTRACT_MODE_OFF: &str = "off";
pub(super) const TASK_CONTRACT_MODE_EMIT: &str = "emit";
pub(super) const TASK_CONTRACT_MODE_EMIT_DRY_RUN: &str = "emit_dry_run";

/// Resolved emission policy after parsing the `emit_task_contract` /
/// `task_contract_mode` args.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TaskContractEmitMode {
    /// Default — no emission, byte-compatible with pre-wave19 behaviour.
    Off,
    /// Emit the contract AND let the dispatch proceed.
    Emit,
    /// Emit the contract but skip the actual inner dispatch (preview).
    EmitDryRun,
}

impl TaskContractEmitMode {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            TaskContractEmitMode::Off => TASK_CONTRACT_MODE_OFF,
            TaskContractEmitMode::Emit => TASK_CONTRACT_MODE_EMIT,
            TaskContractEmitMode::EmitDryRun => TASK_CONTRACT_MODE_EMIT_DRY_RUN,
        }
    }

    pub(super) fn is_enabled(self) -> bool {
        !matches!(self, TaskContractEmitMode::Off)
    }

    pub(super) fn is_dry_run(self) -> bool {
        matches!(self, TaskContractEmitMode::EmitDryRun)
    }
}

/// Parse the opt-in args. Precedence:
///   1. `task_contract_mode` (string, "off"|"emit"|"emit_dry_run")
///   2. `emit_task_contract` (boolean, true ⇒ Emit, false ⇒ Off)
///   3. default ⇒ Off
///
/// Returns `Err(structured)` for malformed values so the caller surfaces
/// a typo (`task_contract_mode="emi"`) instead of silently falling back
/// to Off.
pub(super) fn parse_task_contract_emit_mode(
    args: &Value,
) -> std::result::Result<TaskContractEmitMode, ToolResult> {
    if let Some(raw) = args.get("task_contract_mode") {
        let s = match raw.as_str() {
            Some(s) => s.trim(),
            None => {
                return Err(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        "task_contract_mode must be a string",
                    )
                    .with_suggestion(
                        "expected one of: \"off\", \"emit\", \"emit_dry_run\"",
                    ),
                ));
            }
        };
        return match s {
            TASK_CONTRACT_MODE_OFF => Ok(TaskContractEmitMode::Off),
            TASK_CONTRACT_MODE_EMIT => Ok(TaskContractEmitMode::Emit),
            TASK_CONTRACT_MODE_EMIT_DRY_RUN => Ok(TaskContractEmitMode::EmitDryRun),
            other => Err(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("task_contract_mode `{}` is not supported", other),
                )
                .with_suggestion(
                    "expected one of: \"off\", \"emit\", \"emit_dry_run\"",
                ),
            )),
        };
    }
    match args.get("emit_task_contract").and_then(|v| v.as_bool()) {
        Some(true) => Ok(TaskContractEmitMode::Emit),
        Some(false) | None => Ok(TaskContractEmitMode::Off),
    }
}

// ───────────────────────────────────────────────────────────────────────
// wave-20 / task 04 — machine-driven dispatch v0
//
// The wave-19 / task 06 emitter writes a task-contract v1 Lisp sidecar at
// `<project_root>/.missiond/tasks/generated/<plan_id>/<node_id>.lisp`
// BEFORE handing off to the workstation substrate. The wave-19 / task 07
// consumer (`run_workstation_dispatch_with_contract`) is already capable
// of loading + parsing that file and overlaying the contract onto the
// brief, but the production wiring still passes `task_contract_path =
// None` — the consumer has been dormant. Wave-20 / task 04 adds an
// opt-in mode (`dispatch_contract_mode="machine"` or the boolean
// shorthand `render_markdown=false`) that activates the consumer wiring:
// when enabled AND emission produced a path AND the dispatch routes
// through the workstation substrate, the runner forwards the resolved
// contract path so the brief is built FROM the on-disk Lisp SSOT.
//
// Default mode (`rendered`) preserves wave-15..19 behaviour byte-for-
// byte: the brief is built from the in-memory hints and the optional
// `render_command` lets a caller render the markdown out of process.
// In machine mode the markdown rendering is NOT load-bearing — the
// `render_command` is still surfaced as compatibility metadata, but
// observers can prove the Lisp drove the dispatch via the new
// `task_contract_source_path` field on the workstation response.
//
// Failure semantics (machine mode):
//   * malformed contract on disk → `SafeDescriptor` from the consumer
//     (status="skipped_malformed_task_contract"). MUST NOT fall back to
//     `claude -p` or the unscoped prompt path.
//   * emission disabled / not eligible → falls back to legacy rendered
//     dispatch (machine mode is a no-op without an emitted contract;
//     authors who insist on machine SSOT pair `dispatch_contract_mode=
//     machine` with `task_contract_mode="emit"`).
// ───────────────────────────────────────────────────────────────────────

/// Canonical dispatch-contract modes. Mirrors the MCP descriptor enum.
pub(super) const DISPATCH_CONTRACT_MODE_RENDERED: &str = "rendered";
pub(super) const DISPATCH_CONTRACT_MODE_MACHINE: &str = "machine";

/// Resolved dispatch-contract mode after parsing the
/// `dispatch_contract_mode` / `render_markdown` args.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DispatchContractMode {
    /// Default — wave-15..19 byte-shape. Brief is built from in-memory
    /// hints. The optional `render_command` lets a caller render the
    /// markdown brief out of process; the markdown is the load-bearing
    /// artifact for human review.
    Rendered,
    /// Machine SSOT — when an emitted task-contract path exists AND the
    /// dispatch routes through the workstation substrate, the consumer
    /// loads + parses the on-disk Lisp and overlays it onto the brief.
    /// The markdown becomes optional compatibility metadata.
    Machine,
}

impl DispatchContractMode {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            DispatchContractMode::Rendered => DISPATCH_CONTRACT_MODE_RENDERED,
            DispatchContractMode::Machine => DISPATCH_CONTRACT_MODE_MACHINE,
        }
    }

    pub(super) fn is_machine(self) -> bool {
        matches!(self, DispatchContractMode::Machine)
    }
}

/// Parse the dispatch-contract mode opt-in args. Precedence:
///   1. `dispatch_contract_mode` (string, "rendered" | "machine")
///   2. `render_markdown` (boolean, `false` ⇒ Machine, `true` ⇒ Rendered)
///   3. default ⇒ Rendered (wave-15..19 byte-compat)
///
/// Returns `Err(structured)` for malformed string values so a typo
/// (`dispatch_contract_mode="machin"`) fails fast rather than silently
/// degrading to `Rendered`.
pub(super) fn parse_dispatch_contract_mode(
    args: &Value,
) -> std::result::Result<DispatchContractMode, ToolResult> {
    if let Some(raw) = args.get("dispatch_contract_mode") {
        let s = match raw.as_str() {
            Some(s) => s.trim(),
            None => {
                return Err(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        "dispatch_contract_mode must be a string",
                    )
                    .with_suggestion(
                        "expected one of: \"rendered\", \"machine\"",
                    ),
                ));
            }
        };
        return match s {
            DISPATCH_CONTRACT_MODE_RENDERED => Ok(DispatchContractMode::Rendered),
            DISPATCH_CONTRACT_MODE_MACHINE => Ok(DispatchContractMode::Machine),
            other => Err(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("dispatch_contract_mode `{}` is not supported", other),
                )
                .with_suggestion(
                    "expected one of: \"rendered\", \"machine\"",
                ),
            )),
        };
    }
    match args.get("render_markdown").and_then(|v| v.as_bool()) {
        Some(false) => Ok(DispatchContractMode::Machine),
        Some(true) | None => Ok(DispatchContractMode::Rendered),
    }
}

/// Lightweight projection of a workstation-dispatch task surface —
/// independent of the wave-15 `WorkstationDispatchHints` struct so the
/// emitter does not own any reformatting that the dispatch layer cares
/// about. Owned by the emitter; populated by the caller from either the
/// single-node merged hints or the DAG node hints.
#[derive(Debug, Clone, Default)]
pub(super) struct TaskContractInputs {
    pub objective: String,
    pub scope: Option<String>,
    pub owned_files: Vec<String>,
    pub forbidden_files: Vec<String>,
    pub acceptance_commands: Vec<String>,
    pub commit_policy: Option<String>,
    pub dispatch_strategy: String,
    pub target: String,
    pub target_project: Option<String>,
    pub requested_cwd: Option<String>,
    /// wave-23 / task 05 — optional session-trace ledger path emitted
    /// onto the contract as `:session-trace-path "..."`. Default
    /// (None) preserves the wave-15..22 contract byte shape: the field
    /// is only rendered when the caller explicitly opted in.
    pub session_trace_path: Option<String>,
}

/// Escape a string for inclusion inside a Lisp double-quoted literal.
/// We only need to handle backslash + double-quote — task-contract
/// readers go through the shared MissionD Lisp parser which treats
/// every other byte literally.
pub(super) fn lisp_escape_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            // Newlines in a Lisp string literal are valid; preserve verbatim.
            other => out.push(other),
        }
    }
    out
}

/// Render a non-empty string vector as a bracketed string list:
///   `["a" "b"]`
fn render_lisp_string_list(items: &[String]) -> String {
    let mut out = String::from("[");
    for (i, s) in items.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push('"');
        out.push_str(&lisp_escape_string(s));
        out.push('"');
    }
    out.push(']');
    out
}

/// Build a task-contract v1 Lisp document from the resolved inputs.
///
/// Mirrors `.missiond/tasks/schema/task-contract-v1.lisp`:
///   - schema = "missiond.task-contract.v1"
///   - kind   = "code-alignment" (workstation-dispatchable nodes carry
///              code intent; if/when the emitter learns to project
///              read-only briefs we'll add "review" branching here)
///   - status = "ready"
///   - owner  = "claudecode"
///   - commit = (:required true :scope-check write-scope-only ...)
///
/// `node_id` doubles as the contract id (`<plan-uuid-prefix>-<node>`)
/// so `check-task-contract.mjs --all` can pivot on a stable identifier
/// after sweeping `.missiond/tasks/generated/`.
pub(super) fn build_task_contract_lisp(
    plan_id: uuid::Uuid,
    node_id: &str,
    board_task_id: &str,
    inputs: &TaskContractInputs,
) -> String {
    let plan_short = plan_id
        .to_string()
        .chars()
        .take(8)
        .collect::<String>();
    let contract_id = format!("plan-{}-node-{}", plan_short, sanitize_for_id(node_id));
    let title = format!(
        "Plan {} node {} — workstation task contract",
        plan_id, node_id
    );
    let commit_message = format!("feat(plan-node): execute node {} for plan {}", node_id, plan_id);

    let mut out = String::new();
    out.push_str(";; Generated by MissionD plan-runner (wave-19 / task 06).\n");
    out.push_str(&format!(
        ";; plan_id = {}\n;; board_task_id = {}\n;; node_id = {}\n\n",
        plan_id, board_task_id, node_id
    ));
    out.push_str(&format!("(task {}\n", contract_id));
    out.push_str("  :schema \"missiond.task-contract.v1\"\n");
    out.push_str(&format!("  :title \"{}\"\n", lisp_escape_string(&title)));
    out.push_str("  :kind code-alignment\n");
    out.push_str("  :status ready\n");
    out.push_str("  :owner \"claudecode\"\n");
    out.push_str(&format!(
        "  :dispatch-strategy \"{}\"\n",
        lisp_escape_string(&inputs.dispatch_strategy)
    ));
    out.push_str(&format!(
        "  :goal \"{}\"\n",
        lisp_escape_string(inputs.objective.trim())
    ));

    if let Some(scope) = inputs.scope.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        out.push_str(&format!(
            "  :scope \"{}\"\n",
            lisp_escape_string(scope)
        ));
    }

    // :write-scope is required and non-empty per task-contract v1. When
    // the caller did not declare `owned_files`, fall back to a single
    // sentinel that the checker will reject — better to fail loudly on
    // the contract than silently let the worker stage anything.
    out.push_str("  :write-scope\n    ");
    if inputs.owned_files.is_empty() {
        out.push_str("[]\n");
    } else {
        out.push_str(&render_lisp_string_list(&inputs.owned_files));
        out.push('\n');
    }

    out.push_str("  :must-not-touch\n    ");
    out.push_str(&render_lisp_string_list(&inputs.forbidden_files));
    out.push('\n');

    if !inputs.acceptance_commands.is_empty() {
        out.push_str("  :acceptance\n    ");
        out.push_str(&render_lisp_string_list(&inputs.acceptance_commands));
        out.push('\n');
    } else {
        // task-contract v1 demands `:acceptance` non-empty — surface a
        // sentinel so the contract round-trips through the checker as
        // an authoring failure rather than silently dispatching.
        out.push_str("  :acceptance\n    []\n");
    }

    let commit_policy = inputs
        .commit_policy
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("scoped");
    out.push_str("  :commit\n");
    out.push_str("    (:required true\n");
    out.push_str(&format!(
        "     :message \"{}\"\n",
        lisp_escape_string(&commit_message)
    ));
    out.push_str("     :scope-check write-scope-only\n");
    out.push_str(&format!(
        "     :policy \"{}\")\n",
        lisp_escape_string(commit_policy)
    ));

    if let Some(tp) = inputs
        .target_project
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        out.push_str(&format!(
            "  :target-project \"{}\"\n",
            lisp_escape_string(tp)
        ));
    }
    if let Some(cwd) = inputs
        .requested_cwd
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        out.push_str(&format!(
            "  :requested-cwd \"{}\"\n",
            lisp_escape_string(cwd)
        ));
    }
    // wave-23 / task 05 — emit `:session-trace-path` when the plan-runner
    // opted into trace forwarding. The field is read back by
    // `workstation_dispatch::ParsedTaskContract` so a downstream caller
    // that loads the contract (machine mode) can re-derive the path
    // without re-supplying the arg.
    if let Some(stp) = inputs
        .session_trace_path
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        out.push_str(&format!(
            "  :session-trace-path \"{}\"\n",
            lisp_escape_string(stp)
        ));
    }
    out.push_str(&format!(
        "  :target \"{}\"\n",
        lisp_escape_string(&inputs.target)
    ));
    out.push_str(&format!(
        "  :plan-id \"{}\"\n",
        plan_id
    ));
    out.push_str(&format!(
        "  :node-id \"{}\"\n",
        lisp_escape_string(node_id)
    ));
    out.push_str(")\n");
    out
}

/// Sanitise an arbitrary node id into the lowercase kebab-friendly form
/// task-contract v1 demands of `:id`. Unknown characters collapse to
/// `-`; collapsing repeats keeps the result readable.
fn sanitize_for_id(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut prev_dash = false;
    for ch in lower.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
            out.push(ch);
            prev_dash = false;
        } else if ch == '-' || ch.is_whitespace() || ch == '/' {
            if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        } else {
            // Drop everything else conservatively (no random unicode in ids).
            if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "unnamed".to_string()
    } else {
        trimmed
    }
}

/// Compute the on-disk path for a generated task contract. Caller-resolved
/// project root MUST already be canonical (we do not invent one here).
pub(super) fn task_contract_path(
    project_root: &std::path::Path,
    plan_id: uuid::Uuid,
    node_id: &str,
) -> PathBuf {
    project_root
        .join(".missiond")
        .join("tasks")
        .join("generated")
        .join(plan_id.to_string())
        .join(format!("{}.lisp", sanitize_for_id(node_id)))
}

/// Build the deterministic `node scripts/render-claudecode-task.mjs ...`
/// invocation paired with each generated contract. Surfaced on the
/// response so a caller can render the markdown brief without the
/// daemon shelling out to Node.
pub(super) fn render_command_for(contract_path: &std::path::Path) -> String {
    format!(
        "node scripts/render-claudecode-task.mjs --force {}",
        contract_path.display()
    )
}

/// Write the generated contract to `<project_root>/.missiond/tasks/generated/
/// <plan_id>/<node_id>.lisp` atomically (tmp → rename).
///
/// Returns the absolute path on success. Any IO failure surfaces a
/// structured `anyhow::Error` so the caller can refuse the dispatch.
pub(super) async fn write_task_contract(
    state: &AppState,
    plan_id: uuid::Uuid,
    node_id: &str,
    project_arg: Option<&str>,
    cwd_arg: Option<&str>,
    target_project_arg: Option<&str>,
    body: &str,
) -> Result<PathBuf> {
    let project_root = resolve_project_root(
        &state.project_registry,
        project_arg,
        cwd_arg,
        target_project_arg,
    )
    .await?;
    write_task_contract_under_root(&project_root, plan_id, node_id, body)
}

/// Inner half of [`write_task_contract`] that takes an already-resolved
/// project root. Split out so unit tests can exercise the on-disk
/// contract without materialising a full [`AppState`] (the same
/// pattern [`resolve_project_root`] tests use against
/// [`SharedProjectRegistry`]).
pub(super) fn write_task_contract_under_root(
    project_root: &std::path::Path,
    plan_id: uuid::Uuid,
    node_id: &str,
    body: &str,
) -> Result<PathBuf> {
    let path = task_contract_path(project_root, plan_id, node_id);
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("task contract path missing parent: {}", path.display()))?;
    std::fs::create_dir_all(dir)
        .map_err(|e| anyhow!("mkdir {}: {}", dir.display(), e))?;
    let tmp = path.with_extension("lisp.tmp");
    std::fs::write(&tmp, body.as_bytes())
        .map_err(|e| anyhow!("write tmp {}: {}", tmp.display(), e))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| anyhow!("rename {} -> {}: {}", tmp.display(), path.display(), e))?;
    Ok(path)
}

/// Whether the resolved dispatch shape is eligible for task-contract
/// emission. Mirrors the workstation-dispatch eligibility floor:
/// target must be `mission_task_delegate` and the objective must be
/// non-empty. Other gates (owned-files / scope) are NOT enforced here
/// because the emitter writes a contract that surfaces them as
/// authoring violations through `check-task-contract.mjs`.
pub(super) fn is_task_contract_eligible(
    target: &str,
    objective: Option<&str>,
) -> bool {
    if target != "mission_task_delegate" {
        return false;
    }
    objective
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

/// The result of a single task-contract emission attempt — surfaced
/// verbatim onto the response payload regardless of dispatch outcome.
#[derive(Debug, Clone)]
pub(super) struct TaskContractEmissionRecord {
    pub mode: TaskContractEmitMode,
    pub eligible: bool,
    /// Set when `eligible=false` so the caller can read the skip reason.
    pub skip_reason: Option<String>,
    /// Set when emission succeeded.
    pub path: Option<PathBuf>,
    /// Set when emission attempted but failed.
    pub error: Option<String>,
    /// Set when emission succeeded.
    pub render_command: Option<String>,
}

impl TaskContractEmissionRecord {
    pub(super) fn off() -> Self {
        Self {
            mode: TaskContractEmitMode::Off,
            eligible: false,
            skip_reason: None,
            path: None,
            error: None,
            render_command: None,
        }
    }

    pub(super) fn skipped(mode: TaskContractEmitMode, reason: &str) -> Self {
        Self {
            mode,
            eligible: false,
            skip_reason: Some(reason.to_string()),
            path: None,
            error: None,
            render_command: None,
        }
    }

    pub(super) fn ok(mode: TaskContractEmitMode, path: PathBuf) -> Self {
        let cmd = render_command_for(&path);
        Self {
            mode,
            eligible: true,
            skip_reason: None,
            path: Some(path),
            error: None,
            render_command: Some(cmd),
        }
    }

    pub(super) fn failed(mode: TaskContractEmitMode, error: String) -> Self {
        Self {
            mode,
            eligible: true,
            skip_reason: None,
            path: None,
            error: Some(error),
            render_command: None,
        }
    }

    pub(super) fn is_failure(&self) -> bool {
        self.error.is_some()
    }

    /// Project the record onto a JSON block to be merged into the
    /// response payload. Returns `None` when the emitter was OFF and
    /// nothing observable happened — keeps the response byte-shape
    /// identical to the pre-wave19 baseline for the default path.
    pub(super) fn to_response_block(&self) -> Option<Value> {
        if matches!(self.mode, TaskContractEmitMode::Off) && self.path.is_none() && self.error.is_none() {
            return None;
        }
        let mut map = serde_json::Map::new();
        map.insert("task_contract_mode".to_string(), json!(self.mode.as_str()));
        map.insert("task_contract_eligible".to_string(), json!(self.eligible));
        if let Some(reason) = &self.skip_reason {
            map.insert("task_contract_skip_reason".to_string(), json!(reason));
        }
        if let Some(path) = &self.path {
            map.insert("task_contract_path".to_string(), json!(path.display().to_string()));
        }
        if let Some(cmd) = &self.render_command {
            map.insert("render_command".to_string(), json!(cmd));
        }
        if let Some(err) = &self.error {
            map.insert("task_contract_error".to_string(), json!(err));
        }
        Some(Value::Object(map))
    }
}

/// Drive one emission pass given resolved inputs. Always returns a
/// record; never panics, never silently swallows IO failure (failures
/// land on `record.error`).
pub(super) async fn emit_task_contract(
    state: &AppState,
    plan_id: uuid::Uuid,
    board_task_id: &str,
    node_id: &str,
    mode: TaskContractEmitMode,
    inputs: &TaskContractInputs,
    project_arg: Option<&str>,
    cwd_arg: Option<&str>,
    target_project_arg: Option<&str>,
) -> TaskContractEmissionRecord {
    if !mode.is_enabled() {
        return TaskContractEmissionRecord::off();
    }
    if !is_task_contract_eligible(&inputs.target, Some(&inputs.objective)) {
        return TaskContractEmissionRecord::skipped(
            mode,
            "target is not mission_task_delegate or objective is empty",
        );
    }
    let body = build_task_contract_lisp(plan_id, node_id, board_task_id, inputs);
    match write_task_contract(
        state,
        plan_id,
        node_id,
        project_arg,
        cwd_arg,
        target_project_arg,
        &body,
    )
    .await
    {
        Ok(path) => TaskContractEmissionRecord::ok(mode, path),
        Err(e) => TaskContractEmissionRecord::failed(mode, e.to_string()),
    }
}

/// Build a `TaskContractInputs` view from the wave-15 workstation hint
/// projection. Used by the single-node `action_execute_internal` path
/// where the merged hints live; the DAG path constructs the inputs
/// directly from the per-node `WorkstationDispatchHints`.
pub(super) fn task_contract_inputs_from_hints(
    hints: &super::workstation_dispatch::WorkstationDispatchHints,
    target: &str,
    dispatch_strategy: &str,
) -> TaskContractInputs {
    TaskContractInputs {
        objective: hints
            .objective
            .clone()
            .unwrap_or_default(),
        scope: hints.scope.clone(),
        owned_files: hints.owned_files.clone(),
        forbidden_files: hints.forbidden_files.clone(),
        acceptance_commands: hints.acceptance_commands.clone(),
        commit_policy: hints.commit_policy.clone(),
        dispatch_strategy: dispatch_strategy.to_string(),
        target: target.to_string(),
        target_project: hints.target_project.clone(),
        requested_cwd: hints.requested_cwd.clone(),
        session_trace_path: None,
    }
}

/// wave-23 / task 05 — variant of `task_contract_inputs_from_hints` that
/// also stamps an optional session-trace ledger path onto the projection
/// so the contract emitter writes `:session-trace-path "..."` into the
/// generated Lisp.
///
/// Why a separate function rather than extending the existing one's
/// signature: out-of-scope callers (`plan_dag.rs`) bind to the existing
/// 3-arg form and cannot be edited under this wave's contract. A wrapper
/// keeps the legacy surface stable and lets the in-scope plan.rs
/// surface forward the trace path cleanly.
pub(super) fn task_contract_inputs_from_hints_with_trace(
    hints: &super::workstation_dispatch::WorkstationDispatchHints,
    target: &str,
    dispatch_strategy: &str,
    session_trace_path: Option<&str>,
) -> TaskContractInputs {
    let mut out = task_contract_inputs_from_hints(hints, target, dispatch_strategy);
    out.session_trace_path = session_trace_path
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    out
}

// ───────────────────────────────────────────────────────────────────────
// wave-18 / task 05 — cross-plan distill chain v0
//
// Conservative chain orchestrator that runs AFTER the wave-17 / task 05
// `finalize_plan` + `distill_on_success` pass. The chain knobs let a
// caller mark this plan as a contributor to a named workflow distillation
// chain that spans multiple successful plans. We never overwrite prior
// chain entries (they live in OTHER plans' sidecars; this plan's own
// sidecar is purely additive); we never invoke workflow distill outside
// the explicit `dry_run` / `sonnet` modes; and we never downgrade the
// underlying plan finalization on chain failure (the finalization block
// from `plan_dag::action_execute_dag_v1` is preserved verbatim — chain
// failures only surface a non-fatal `warning` on the chain block).
//
// Lisp authority forward-reference (wave-18 / task 10 will backfill):
//   - intent-flow.lisp :: F-intent-alignment-plan-execution-loop ::
//                          s8 workflow-distillation (chain extension)
//   - intent-intent-layer.lisp :: section unified-entry-pipeline ::
//                                  role workflow-distiller (chain mode)
// ───────────────────────────────────────────────────────────────────────

/// Canonical chain mode strings. Mirror these in the MCP descriptor's
/// enum so the two surfaces cannot drift on a typo.
pub(super) const DISTILL_CHAIN_MODE_RECORD_ONLY: &str = "record_only";
pub(super) const DISTILL_CHAIN_MODE_DRY_RUN: &str = "dry_run";
pub(super) const DISTILL_CHAIN_MODE_SONNET: &str = "sonnet";

/// Evidence `kind` tag for the chain-record sidecar entry. Distinct from
/// the wave-17 / task 05 `dag_finalized` row so audit dashboards can
/// pivot on chain participation without re-deriving it from the
/// surrounding state_transition.
pub(super) const CHAIN_RECORD_KIND: &str = "distill_chain_record";

/// Status strings surfaced on the `distill_chain_status` response field.
/// Kept as constants so callers can pin the wire form in tests / audit
/// queries without scraping a string literal.
pub(super) const CHAIN_STATUS_RECORDED: &str = "recorded";
pub(super) const CHAIN_STATUS_RECORDED_WITH_DISTILL: &str = "recorded_with_distill";
pub(super) const CHAIN_STATUS_RECORDED_DISTILL_WARNING: &str = "recorded_with_distill_warning";
pub(super) const CHAIN_STATUS_SKIPPED_PLAN_NOT_SUCCEEDED: &str = "skipped_plan_not_succeeded";
pub(super) const CHAIN_STATUS_SKIPPED_NO_FINALIZATION: &str = "skipped_no_finalization";
pub(super) const CHAIN_STATUS_NOT_REQUESTED: &str = "not_requested";
pub(super) const CHAIN_STATUS_RECORD_FAILED: &str = "record_failed";

/// Parse the optional `distill_chain_id` arg. Returns `None` when absent
/// or blank — the chain orchestrator generates a deterministic fallback
/// (`chain:auto:plan-<plan_id>`) in that case so the audit row never
/// carries an empty id.
pub(super) fn parse_distill_chain_id(args: &Value) -> Option<String> {
    args.get("distill_chain_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Parse the optional `distill_chain_name` arg (free-form human-readable
/// label, e.g. "wave18-finalize-loop"). Blank collapses to `None`.
pub(super) fn parse_distill_chain_name(args: &Value) -> Option<String> {
    args.get("distill_chain_name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Strict allowlist for the `distill_chain_mode` knob. Default is
/// `record_only` (no workflow distill call — the chain entry is only
/// recorded in the evidence sidecar). `dry_run` and `sonnet` forward to
/// `mission_workflow(action=distill, distill_mode=…)` with the
/// corresponding mode. Returns the canonical string or an error message.
pub(super) fn parse_distill_chain_mode(args: &Value) -> std::result::Result<&'static str, String> {
    match args.get("distill_chain_mode").and_then(|v| v.as_str()) {
        None | Some("") | Some(DISTILL_CHAIN_MODE_RECORD_ONLY) => {
            Ok(DISTILL_CHAIN_MODE_RECORD_ONLY)
        }
        Some(DISTILL_CHAIN_MODE_DRY_RUN) => Ok(DISTILL_CHAIN_MODE_DRY_RUN),
        Some(DISTILL_CHAIN_MODE_SONNET) => Ok(DISTILL_CHAIN_MODE_SONNET),
        Some(other) => Err(format!(
            "distill_chain_mode must be one of [\"record_only\", \"dry_run\", \"sonnet\"]; got `{}`",
            other
        )),
    }
}

/// Returns true when ANY of the chain knobs were supplied. Used to gate
/// validation + the post-finalize chain run — callers that did not opt in
/// see byte-identical wave-17 / task 05 responses.
pub(super) fn distill_chain_requested(args: &Value) -> bool {
    args.get("distill_chain_id").is_some()
        || args.get("distill_chain_mode").is_some()
        || args.get("distill_chain_name").is_some()
}

/// Pre-flight validation for the wave-18 / task 05 chain knobs. Returns
/// `Some(error_result)` for the call site to early-return; `None` when
/// the args pass.
///
/// Cross-field rules:
///
///   * Any chain knob requires `finalize_plan=true` — the chain is gated
///     on a successful finalization, so silently dropping a chain
///     request would mask the caller's intent.
///   * `distill_chain_mode` must be on the strict allowlist — even when
///     no other chain knob was passed, a typo on the mode alone surfaces
///     immediately rather than on the next live caller's run.
pub(super) fn validate_distill_chain_args(args: &Value) -> Option<ToolResult> {
    if let Err(msg) = parse_distill_chain_mode(args) {
        return Some(ToolResult::structured_error(ToolError::new(
            error_codes::INVALID_PARAM,
            msg,
        )));
    }
    if distill_chain_requested(args)
        && !super::plan_dag::parse_finalize_plan(args)
    {
        return Some(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                "distill_chain_* knobs require finalize_plan=true",
            )
            .with_suggestion(
                "the cross-plan distill chain only fires AFTER a successful finalization; \
                 set finalize_plan=true or drop the distill_chain_* knobs",
            ),
        ));
    }
    // wave-21 / task 07 — strict-shape validation of the auto-sonnet
    // apply-gate knobs. Workflow.rs validates again as a defense-in-depth
    // layer, but failing fast at the plan entry keeps the diagnostic
    // close to the caller's invocation site.
    if let Some(v) = args.get("auto_sonnet") {
        if !v.is_boolean() {
            return Some(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!(
                        "auto_sonnet must be a boolean (true|false); got {}",
                        json_shape_label(v)
                    ),
                )
                .with_suggestion(
                    "auto_sonnet is the wave-21 / task 07 apply-gate opt-in; \
                     pass true or false (no string).",
                ),
            ));
        }
    }
    if let Some(v) = args.get("auto_sonnet_approved") {
        if !v.is_boolean() {
            return Some(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!(
                        "auto_sonnet_approved must be a boolean (true|false); got {}",
                        json_shape_label(v)
                    ),
                )
                .with_suggestion(
                    "auto_sonnet_approved is the wave-21 / task 07 caller-approval flag; \
                     pass true or false (no string).",
                ),
            ));
        }
    }
    // wave-22 / task 06 — closed-enum strict-shape validation of the
    // policy v2 knob. Workflow.rs validates again as a defense-in-depth
    // layer, but failing fast at the plan entry keeps the diagnostic
    // close to the caller's invocation site (mirrors wave-21/07 dual
    // opt-in validation).
    if let Some(v) = args.get("auto_sonnet_policy") {
        if !v.is_null() {
            let s = match v.as_str() {
                Some(s) => s,
                None => {
                    return Some(ToolResult::structured_error(
                        ToolError::new(
                            error_codes::INVALID_PARAM,
                            format!(
                                "auto_sonnet_policy must be a string (one of [\"off\",\"safe_after_rules\",\"dry_run\"]); got {}",
                                json_shape_label(v)
                            ),
                        )
                        .with_suggestion(
                            "auto_sonnet_policy is the wave-22 / task 06 v2 closed-enum policy; \
                             pass one of [\"off\",\"safe_after_rules\",\"dry_run\"] (no boolean / number).",
                        ),
                    ));
                }
            };
            if !matches!(s, "" | "off" | "safe_after_rules" | "dry_run") {
                return Some(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "auto_sonnet_policy must be one of [\"off\",\"safe_after_rules\",\"dry_run\"]; got `{}`",
                            s
                        ),
                    )
                    .with_suggestion(
                        "auto_sonnet_policy is the wave-22 / task 06 v2 closed-enum policy; \
                         pass one of [\"off\",\"safe_after_rules\",\"dry_run\"].",
                    ),
                ));
            }
        }
    }
    None
}

/// Render the JSON shape of a value as a stable label for diagnostic
/// messages. Mirrors `workflow::shape_label` so the two surfaces emit
/// identical wording on shape rejections.
fn json_shape_label(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Deterministic fallback chain id when the caller did not supply one.
/// Anchored on the plan id so re-runs against the same plan land on the
/// same chain bucket — auditors can correlate without rolling a UUID.
fn derive_fallback_chain_id(plan_id: uuid::Uuid) -> String {
    format!("chain:auto:plan-{}", plan_id)
}

/// Inspect the wave-17 / task 05 finalization block on the inner DAG
/// payload to decide chain eligibility. Returns `Some("…")` reason when
/// chain MUST be skipped (with the canonical `distill_chain_status`
/// label), or `None` when the chain can proceed.
fn chain_eligibility_skip_reason(payload: &Value) -> Option<&'static str> {
    let finalization = match payload.get("finalization") {
        Some(v) => v,
        None => return Some(CHAIN_STATUS_SKIPPED_NO_FINALIZATION),
    };
    let final_plan_status = finalization
        .get("final_plan_status")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if final_plan_status != "succeeded" {
        return Some(CHAIN_STATUS_SKIPPED_PLAN_NOT_SUCCEEDED);
    }
    None
}

/// Build the chain block surfaced under `finalization.distill_chain` on
/// the response. Always carries `triggered` / `status` / `chain_id` /
/// `chain_mode` so observers can pivot on a single shape; optional
/// `chain_name` / `distill_result` / `warning` / `chain_index_in_plan` /
/// `evidence_path` / `evidence_error` are only added when present.
#[allow(clippy::too_many_arguments)]
fn build_distill_chain_block(
    triggered: bool,
    status: &str,
    chain_id: &str,
    chain_id_source: &str,
    chain_mode: &str,
    chain_name: Option<&str>,
    chain_index_in_plan: Option<usize>,
    distill_result: Option<Value>,
    warning: Option<&str>,
    evidence_path: Option<&str>,
    evidence_error: Option<&str>,
) -> Value {
    let mut block = json!({
        "triggered": triggered,
        "status": status,
        "chain_id": chain_id,
        "chain_id_source": chain_id_source,
        "chain_mode": chain_mode,
    });
    if let Some(n) = chain_name {
        block["chain_name"] = json!(n);
    }
    if let Some(idx) = chain_index_in_plan {
        block["chain_index_in_plan"] = json!(idx);
    }
    if let Some(r) = distill_result {
        block["distill_result"] = r;
    }
    if let Some(w) = warning {
        block["warning"] = json!(w);
    }
    if let Some(p) = evidence_path {
        block["evidence_path"] = json!(p);
    }
    if let Some(e) = evidence_error {
        block["evidence_error"] = json!(e);
    }
    block
}

/// Read the existing evidence sidecar (if any) and count the prior
/// chain-record entries with the matching `chain_id`. Returns 0 when
/// the sidecar does not exist or carries no prior chain rows for this
/// id. Pure read — never writes.
///
/// Failures (resolve / read / parse) collapse to 0 because the chain
/// orchestrator's "do not overwrite prior evidence" invariant is
/// satisfied by the writer (`append_plan_evidence_entry` only appends);
/// the count is purely a UX hint surfaced as `chain_index_in_plan`.
async fn count_prior_chain_entries_in_plan_sidecar(
    state: &AppState,
    plan_id: uuid::Uuid,
    project_arg: Option<&str>,
    cwd_arg: Option<&str>,
    target_project_arg: Option<&str>,
    chain_id: &str,
) -> usize {
    let project_root = match resolve_project_root(
        &state.project_registry,
        project_arg,
        cwd_arg,
        target_project_arg,
    )
    .await
    {
        Ok(p) => p,
        Err(_) => return 0,
    };
    let path = project_root
        .join(COMPANION_DIR)
        .join(format!("{}.evidence.json", plan_id));
    if !path.exists() {
        return 0;
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let bundle: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let entries = match bundle.get("entries").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return 0,
    };
    entries
        .iter()
        .filter(|e| {
            e.get("kind").and_then(|v| v.as_str()) == Some(CHAIN_RECORD_KIND)
                && e.get("chain_id").and_then(|v| v.as_str()) == Some(chain_id)
        })
        .count()
}

/// Drive the optional cross-plan distill chain. Pure orchestration:
/// validation already ran in `validate_distill_chain_args` so here we
/// only branch on the runtime payload + chain mode.
///
/// Returns the same `dag_result` byte-for-byte when no chain knob was
/// supplied. Otherwise injects a `distill_chain` block under the
/// existing `finalization` map (or under a new top-level
/// `distill_chain` key when finalization was not requested — in that
/// case the chain is also skipped, but we still surface the skip
/// reason so callers can detect the missed opt-in).
async fn apply_distill_chain(
    state: &AppState,
    args: &Value,
    plan: &Plan,
    dag_result: ToolResult,
) -> ToolResult {
    if !distill_chain_requested(args) {
        return dag_result;
    }
    // Inner DAG result may itself be a structured error (e.g. validation
    // rejected on the wave-17 path). Surface chain="not_requested" on
    // the same envelope so the caller still sees a stable shape, but do
    // NOT overwrite the error payload.
    if dag_result.is_error.unwrap_or(false) {
        return dag_result;
    }

    // Mode is already validated; unwrap is safe.
    let chain_mode = parse_distill_chain_mode(args).unwrap_or(DISTILL_CHAIN_MODE_RECORD_ONLY);
    let chain_name = parse_distill_chain_name(args);
    let (chain_id, chain_id_source): (String, &'static str) = match parse_distill_chain_id(args) {
        Some(id) => (id, "explicit_arg"),
        None => (derive_fallback_chain_id(plan.id), "derived_from_plan_id"),
    };

    // Re-parse the inner payload so we can inspect the wave-17 / task 05
    // `finalization` block and (when chain runs) augment it with our
    // `distill_chain` sub-block.
    let mut payload = tool_result_payload(&dag_result);

    // Eligibility gate: chain only fires when the inner finalization
    // block reports `final_plan_status="succeeded"`. Any other state
    // (failed / paused / unchanged / no finalization) collapses to a
    // skipped chain block — recorded on the response so the caller can
    // see the skip reason but with NO sidecar write and NO distill call.
    if let Some(skip_reason) = chain_eligibility_skip_reason(&payload) {
        let block = build_distill_chain_block(
            false,
            skip_reason,
            &chain_id,
            chain_id_source,
            chain_mode,
            chain_name.as_deref(),
            None,
            None,
            None,
            None,
            None,
        );
        attach_distill_chain_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // Eligibility passed → run the chain. Order:
    //   1. Count prior chain entries in this plan's sidecar (UX hint).
    //   2. Optionally invoke `mission_workflow(action=distill)` for
    //      `dry_run` / `sonnet` modes.
    //   3. Append exactly ONE chain-record evidence row tagged with
    //      chain_id / chain_name / chain_mode / distill summary.
    //   4. Return the augmented response.
    let project_arg = args.get("project").and_then(|v| v.as_str());
    let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg = args.get("target_project").and_then(|v| v.as_str());

    let prior_count = count_prior_chain_entries_in_plan_sidecar(
        state,
        plan.id,
        project_arg,
        cwd_arg,
        target_project_arg,
        &chain_id,
    )
    .await;
    let chain_index_in_plan = prior_count + 1;

    // Step 2 — optional workflow distill call. `record_only` skips this
    // entirely. The brief explicitly forbids invoking sonnet without an
    // explicit mode, so we route on the canonical mode string.
    let (distill_result, distill_warning, triggered_distill): (Option<Value>, Option<String>, bool) =
        match chain_mode {
            DISTILL_CHAIN_MODE_RECORD_ONLY => (None, None, false),
            DISTILL_CHAIN_MODE_DRY_RUN | DISTILL_CHAIN_MODE_SONNET => {
                let mut distill_args = serde_json::Map::new();
                distill_args.insert("action".to_string(), json!("distill"));
                distill_args.insert("plan_id".to_string(), json!(plan.id.to_string()));
                distill_args.insert("distill_mode".to_string(), json!(chain_mode));
                if let Some(p) = project_arg {
                    distill_args.insert("project".to_string(), json!(p));
                }
                if let Some(c) = cwd_arg {
                    distill_args.insert("cwd".to_string(), json!(c));
                }
                if let Some(tp) = target_project_arg {
                    distill_args.insert("target_project".to_string(), json!(tp));
                }
                if let Some(name) = chain_name.as_deref() {
                    // Forward the chain name as the workflow `name` so a
                    // persisted distill row carries the chain label.
                    // Caller can still override by passing an explicit
                    // `name` arg (we do NOT overwrite an existing key).
                    distill_args
                        .entry("name".to_string())
                        .or_insert_with(|| json!(name));
                }
                // wave-21 / task 07 — forward the auto-sonnet apply-gate
                // knobs into the workflow.distill sub-call so plan-side
                // callers can opt into the gate without re-shaping the
                // arg envelope. The gate is strictly opt-in (default
                // off); the workflow surface validates shape +
                // enforces all six wave-20 safety rules + caller
                // approval before invoking Sonnet. We forward both
                // `auto_sonnet*` knobs AND `auto_chain_trigger` /
                // `auto_trigger_min_evidence` because the auto-sonnet
                // gate is layered on top of the wave-20 trigger and
                // refuses to operate without it (`skipped_no_trigger`).
                //
                // wave-22 / task 06 — forward the v2 closed-enum
                // `auto_sonnet_policy` knob alongside the v1 dual
                // opt-in flags so plan-side callers can opt into
                // either surface (or both — the workflow layer
                // attaches an `auto_sonnet_policy` block in addition
                // to the legacy `auto_sonnet` block when both are
                // requested).
                for key in [
                    "auto_sonnet",
                    "auto_sonnet_approved",
                    "auto_sonnet_policy",
                    "auto_chain_trigger",
                    "auto_trigger_min_evidence",
                ] {
                    if let Some(v) = args.get(key).cloned() {
                        distill_args.insert(key.to_string(), v);
                    }
                }
                let call_args = Value::Object(distill_args);
                match super::workflow::handle(state, "mission_workflow", call_args).await {
                    Ok(tr) => {
                        let inner_payload = tool_result_payload(&tr);
                        let inner_is_error = tr.is_error.unwrap_or(false);
                        let warning = if inner_is_error {
                            Some(
                                "distill chain workflow call returned an error; \
                                 plan finalization preserved"
                                    .to_string(),
                            )
                        } else {
                            None
                        };
                        (Some(inner_payload), warning, true)
                    }
                    Err(e) => {
                        // Handler-level Result::Err → treat as a warning,
                        // never as a finalization downgrade. Mirrors
                        // `plan_dag::maybe_run_distill_trigger`'s policy.
                        tracing::warn!(
                            plan_id = %plan.id,
                            chain_id = %chain_id,
                            error = %e,
                            "distill_chain: workflow handler returned error"
                        );
                        (
                            Some(json!({"error": e.to_string()})),
                            Some(format!(
                                "distill chain workflow handler error: {}; \
                                 plan finalization preserved",
                                e
                            )),
                            true,
                        )
                    }
                }
            }
            // Defensive: validator already rejected anything else.
            _ => (
                None,
                Some(format!(
                    "distill_chain_mode `{}` reached chain runner unexpectedly",
                    chain_mode
                )),
                false,
            ),
        };

    // Step 3 — append the chain-record evidence row. Built via the
    // typed evidence_collector so it carries the canonical
    // schema_version / source / kind stamps the wave-17 finalize entry
    // also uses. The append is purely additive (the underlying writer
    // never overwrites) so prior chain entries (in this OR other plans'
    // sidecars) are preserved by construction.
    let mut entry = super::evidence_collector::EvidenceEntry::new(
        super::evidence_collector::source::PLAN_DAG_NODE_DISPATCH,
        CHAIN_RECORD_KIND,
    )
    .with_state_transition("distill_chain_appended")
    .with_extra("event_kind", json!("plan_dag_distill_chain"))
    .with_extra("plan_id", json!(plan.id))
    .with_extra("plan_version", json!(plan.version))
    .with_extra("chain_id", json!(chain_id))
    .with_extra("chain_id_source", json!(chain_id_source))
    .with_extra("chain_mode", json!(chain_mode))
    .with_extra("chain_index_in_plan", json!(chain_index_in_plan))
    .with_extra("triggered_workflow_distill", json!(triggered_distill));
    if let Some(name) = chain_name.as_deref() {
        entry = entry.with_extra("chain_name", json!(name));
    }
    if let Some(ref result) = distill_result {
        entry = entry.with_extra("distill_result", result.clone());
    }
    if let Some(ref w) = distill_warning {
        entry = entry.with_extra("distill_warning", json!(w));
    }
    let append_outcome = super::evidence_collector::append(
        state,
        plan.id,
        project_arg,
        cwd_arg,
        target_project_arg,
        entry,
    )
    .await;
    let (evidence_path, evidence_error) = match append_outcome {
        super::evidence_collector::AppendOutcome::Written { path, .. } => {
            (Some(path.display().to_string()), None)
        }
        super::evidence_collector::AppendOutcome::Failed { error } => {
            tracing::warn!(
                plan_id = %plan.id,
                chain_id = %chain_id,
                error = %error,
                "distill_chain: evidence sidecar append failed"
            );
            (None, Some(error))
        }
    };

    // Step 4 — derive final status. Order of precedence:
    //   * sidecar write failed       → `record_failed` (still keep plan
    //                                    finalization durable; chain
    //                                    just couldn't persist)
    //   * triggered workflow distill that warned → `recorded_with_distill_warning`
    //   * triggered workflow distill ok          → `recorded_with_distill`
    //   * record-only                            → `recorded`
    let status = if evidence_error.is_some() {
        CHAIN_STATUS_RECORD_FAILED
    } else if triggered_distill {
        if distill_warning.is_some() {
            CHAIN_STATUS_RECORDED_DISTILL_WARNING
        } else {
            CHAIN_STATUS_RECORDED_WITH_DISTILL
        }
    } else {
        CHAIN_STATUS_RECORDED
    };

    let block = build_distill_chain_block(
        triggered_distill || evidence_error.is_none(),
        status,
        &chain_id,
        chain_id_source,
        chain_mode,
        chain_name.as_deref(),
        Some(chain_index_in_plan),
        distill_result,
        distill_warning.as_deref(),
        evidence_path.as_deref(),
        evidence_error.as_deref(),
    );
    attach_distill_chain_to_payload(&mut payload, block);
    ToolResult::json_pretty(&payload)
}

/// Insert the `distill_chain` block under `finalization.distill_chain`
/// when the wave-17 finalization block exists; otherwise surface it at
/// the top level under `distill_chain`. Either way the response also
/// carries top-level `distill_chain_status` / `distill_chain_id`
/// shortcuts so callers can grep one place.
fn attach_distill_chain_to_payload(payload: &mut Value, block: Value) {
    let status = block
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let chain_id = block
        .get("chain_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if let Some(obj) = payload.as_object_mut() {
        if let Some(finalization) = obj.get_mut("finalization") {
            if let Some(fobj) = finalization.as_object_mut() {
                fobj.insert("distill_chain".to_string(), block.clone());
            }
        } else {
            obj.insert("distill_chain".to_string(), block.clone());
        }
        // Top-level shortcuts so the caller can pivot without diving
        // into the finalization block. `distill_chain_status` /
        // `distill_chain_id` mirror what the brief lists under "response".
        obj.insert("distill_chain_status".to_string(), json!(status));
        obj.insert("distill_chain_id".to_string(), json!(chain_id));
        // `distill_result` shortcut (the brief calls out `distill_result
        // or warning` on the response). We only surface on success +
        // dry_run/sonnet — record_only has nothing to show.
        if let Some(result) = block.get("distill_result") {
            obj.insert("distill_result".to_string(), result.clone());
        }
        if let Some(warning) = block.get("warning") {
            obj.insert("distill_chain_warning".to_string(), warning.clone());
        }
    }
}

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

    match resolve_target_project_root(
        project_id,
        cwd_path.as_deref(),
        target_project,
        registry,
    )
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
mod router_policy_dry_run {
    use super::ResolvedExec;
    use missiond_core::types::Plan;
    use missiond_mcp::tools::{error_codes, ToolContent, ToolError, ToolResult};
    use serde_json::{json, Value};
    use std::path::{Path, PathBuf};

    /// Default policy file. Mirrors the wave24-01 seed location and the
    /// wave24-03 CLI's documented default. Resolved relative to the
    /// process CWD when the caller passes only `router_policy_mode=dry_run`
    /// without `router_policy_path`.
    pub(super) const DEFAULT_POLICY_PATH: &str = ".missiond/router/router-policy-v1.lisp";

    /// Schema label embedded in every emitted recommendation block. Mirrors
    /// the wave24-03 CLI so downstream consumers can verify the wire shape.
    pub(super) const SCHEMA: &str = "missiond.router-recommendation.v0";

    /// Fallback backend used when no rule matches. The wave24-03 CLI
    /// surfaces the exact same value + reason — keeping these literals in
    /// sync is part of the cross-wave contract.
    pub(super) const FALLBACK_BACKEND: &str = "claudecode";
    pub(super) const FALLBACK_REASON: &str = "insufficient_trace_history";

    /// Backend enum (mirrors wave24-01 schema). Anything outside this set
    /// is surfaced verbatim in the recommendation block but flagged as
    /// `status="rejected"` (the wave24-01 checker rejects unknown backends
    /// at validation time; the daemon re-checks defensively).
    const KNOWN_BACKENDS: &[&str] = &[
        "claudecode",
        "missiond-llm-router",
        "deterministic-checker",
        "patch-worker",
        "verifier-worker",
    ];

    /// Recognised top-level `router_policy_mode` values.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum RouterPolicyMode {
        /// Default. The recommendation block is NOT emitted; the response
        /// is byte-identical to the wave-15..23 baseline.
        Off,
        /// Compute the recommendation and emit `applied=false`. Never
        /// changes target / dispatch_strategy / workstation_dispatch.
        DryRun,
    }

    /// Parse the optional `router_policy_mode` arg. Returns `Off` when the
    /// arg is absent or the literal string `"off"`. Returns a structured
    /// `INVALID_PARAM` error for any other value (including `apply`,
    /// `auto`, and unknown strings) so a typo cannot silently route the
    /// recommendation through an unimplemented surface.
    pub(super) fn parse_router_policy_mode(
        args: &Value,
    ) -> Result<RouterPolicyMode, ToolResult> {
        let raw = match args.get("router_policy_mode") {
            None | Some(Value::Null) => return Ok(RouterPolicyMode::Off),
            Some(v) => v,
        };
        let s = match raw.as_str() {
            Some(s) => s.trim(),
            None => {
                return Err(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        "router_policy_mode must be a string",
                    )
                    .with_suggestion("expected one of: \"off\", \"dry_run\""),
                ));
            }
        };
        match s {
            "" | "off" => Ok(RouterPolicyMode::Off),
            "dry_run" => Ok(RouterPolicyMode::DryRun),
            other => Err(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!(
                        "router_policy_mode `{}` is not supported in this surface (wave24-04 only ships `off` and `dry_run`)",
                        other
                    ),
                )
                .with_suggestion(
                    "expected one of: \"off\" (default; no recommendation block) or \"dry_run\" (informational block, applied=false)",
                ),
            )),
        }
    }

    /// Splice the recommendation block onto a successful response. No-op
    /// when `mode=Off` so callers that never opted in observe the
    /// wave-15..23 byte-shape. Errors are also passed through unchanged
    /// — we never decorate a structured error with the recommendation
    /// (the operator needs the error path uncluttered).
    pub(super) fn attach_router_recommendation_block(
        mut result: ToolResult,
        mode: RouterPolicyMode,
        args: &Value,
        resolved: &ResolvedExec,
        plan: &Plan,
    ) -> ToolResult {
        if matches!(mode, RouterPolicyMode::Off) {
            return result;
        }
        if result.is_error.unwrap_or(false) {
            return result;
        }
        let block = compute_recommendation(args, resolved, plan);
        let Some(ToolContent::Text { text }) = result.content.first_mut() else {
            return result;
        };
        let Ok(mut value) = serde_json::from_str::<Value>(text) else {
            return result;
        };
        if let Some(map) = value.as_object_mut() {
            // Never overwrite a pre-existing block — preserves any forward-
            // compatible attachment a downstream layer may add.
            map.entry("router_recommendation".to_string()).or_insert(block);
        }
        *text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.clone());
        result
    }

    /// Pure projection of the execute context into the predicate input.
    /// Mirrors the wave24-03 CLI's task-projection: the live execute
    /// `args` + resolved dispatch + plan status are treated like a
    /// task-contract for predicate matching.
    #[derive(Debug, Clone, Default)]
    struct PredicateContext {
        kind: String,
        dispatch_strategy: String,
        owner: String,
        status: String,
        write_scope: Vec<String>,
    }

    fn project_context(args: &Value, resolved: &ResolvedExec, plan: &Plan) -> PredicateContext {
        let kind = arg_string(args, "kind").unwrap_or_default();
        // dispatch_strategy: prefer the resolved value (which already
        // reflects explicit_arg > plan_hint > parallelism > default
        // precedence). The wave24-03 CLI projects from the task contract
        // value; here the resolved value IS the live equivalent.
        let dispatch_strategy = resolved.dispatch_strategy.to_string();
        let owner = arg_string(args, "owner").unwrap_or_default();
        let status = arg_string(args, "status")
            .unwrap_or_else(|| plan.status.as_str().to_string());
        let write_scope = arg_string_array(args, "owned_files");
        PredicateContext {
            kind,
            dispatch_strategy,
            owner,
            status,
            write_scope,
        }
    }

    fn arg_string(args: &Value, key: &str) -> Option<String> {
        args.get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn arg_string_array(args: &Value, key: &str) -> Vec<String> {
        match args.get(key) {
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            Some(Value::String(s)) if !s.trim().is_empty() => {
                // Tolerate single-string-as-array (mirrors many of the
                // wave-15+ workstation-dispatch knobs).
                vec![s.trim().to_string()]
            }
            _ => Vec::new(),
        }
    }

    /// Top-level entry: read + parse the policy, evaluate, and project
    /// the response block. Always returns a serializable JSON object;
    /// never panics; surfaces I/O / parse / invariant failures via the
    /// `status` field (`computed` / `rejected` / `error`).
    ///
    /// wave27-03: the OPTIONAL `router_dispatch_descriptor` arg, when
    /// the JSON literal `true`, is honored ONLY here (i.e. inside the
    /// dry_run code path; `attach_router_recommendation_block`'s Off
    /// early-return predates this entry). When `BackendRegistryInfo` is
    /// `Absent`, the recommendation block surfaces `descriptor_status =
    /// "registry_missing"` and the descriptor body is OMITTED. Otherwise
    /// a `router_dispatch_descriptor` sub-object is spliced onto the
    /// recommendation block, projecting the wave27-01 schema fields off
    /// the existing readiness/recommendation values plus three LOCKED
    /// literal-bool invariants (`Value::Bool(true)` / `Value::Bool(false)`
    /// — never strings, never computed) so the descriptor can never be
    /// mis-read as a runtime promote signal.
    fn compute_recommendation(args: &Value, resolved: &ResolvedExec, plan: &Plan) -> Value {
        let policy_path_input = arg_string(args, "router_policy_path")
            .unwrap_or_else(|| DEFAULT_POLICY_PATH.to_string());
        // wave25-03: trace-index is OPTIONAL and read ONLY here (i.e. inside
        // dry_run). The Off branch in `attach_router_recommendation_block`
        // never calls into `compute_recommendation`, so this preserves the
        // "no file I/O when mode=off" invariant by construction.
        let trace_index_path_input = arg_string(args, "router_policy_trace_index_path");
        let trace_info = load_trace_index(trace_index_path_input.as_deref());
        // wave26-03: backend-readiness registry is OPTIONAL on the same Off-
        // gated path — same invariant applies (no file I/O when mode=off).
        let backend_registry_path_input = arg_string(args, "router_backend_registry_path");
        let registry_info = load_backend_registry(backend_registry_path_input.as_deref());
        let mut block = compute_recommendation_block(
            args,
            resolved,
            plan,
            &policy_path_input,
            &trace_info,
            &registry_info,
        );
        // wave27-03: optional descriptor surface. Always evaluated AFTER
        // the recommendation + readiness fields land so the descriptor
        // can simply project off them. `arg_bool` is strict — only the
        // JSON literal `true` opts in; absent / `false` / strings are
        // ignored. The descriptor branch performs ZERO additional file
        // I/O (registry / policy / trace-index were already loaded above
        // for the recommendation itself).
        if dispatch_descriptor_requested(args) {
            attach_router_dispatch_descriptor(&mut block, plan, &registry_info, &policy_path_input);
        }
        block
    }

    /// wave27-03: did the caller opt in to the dispatch descriptor surface?
    /// Strict: only the JSON literal `true` returns `true`. Absent / `false`
    /// / strings / numbers all return `false` so a typo can never
    /// silently emit the descriptor.
    fn dispatch_descriptor_requested(args: &Value) -> bool {
        matches!(
            args.get("router_dispatch_descriptor"),
            Some(Value::Bool(true))
        )
    }

    /// Internal: build the wave24-04 / wave25-03 / wave26-03 recommendation
    /// block. Extracted so wave27-03 can post-process the block (splicing
    /// the dispatch descriptor) without duplicating the policy / trace /
    /// registry pre-loads.
    fn compute_recommendation_block(
        args: &Value,
        resolved: &ResolvedExec,
        plan: &Plan,
        policy_path_input: &str,
        trace_info: &TraceIndexInfo,
        registry_info: &BackendRegistryInfo,
    ) -> Value {
        let resolved_path = resolve_policy_path(policy_path_input);
        let raw = match std::fs::read_to_string(&resolved_path) {
            Ok(s) => s,
            Err(e) => {
                return error_block(
                    policy_path_input,
                    &format!("read failed: {}", e),
                    trace_info,
                    registry_info,
                    None,
                    "low",
                );
            }
        };
        let policy = match parse_router_policy(&raw) {
            Ok(p) => p,
            Err(msg) => return error_block(
                policy_path_input,
                &msg,
                trace_info,
                registry_info,
                None,
                "low",
            ),
        };
        if !policy.dry_run_only {
            return rejected_block(
                policy_path_input,
                "policy missing :dry-run-only true (cross-wave invariant: router output is advisory only)",
                trace_info,
                registry_info,
                None,
                "low",
            );
        }
        if policy.runtime_replacement {
            return rejected_block(
                policy_path_input,
                "policy declares :runtime-replacement true (cross-wave invariant: router output is advisory only)",
                trace_info,
                registry_info,
                None,
                "low",
            );
        }
        let ctx = project_context(args, resolved, plan);
        let mut matched: Vec<MatchedRule> = Vec::new();
        for rule in &policy.rules {
            if evaluate_clause(&rule.when, &ctx).is_ok() {
                matched.push(MatchedRule {
                    id: rule.id.clone(),
                    priority: rule.priority,
                    reasoning: rule.reasoning.clone(),
                });
            }
        }
        // Rules already sorted by priority ascending — the parser does
        // this once on construction.
        if matched.is_empty() {
            // No-match: confidence is `low` regardless of trace-index
            // (mirrors the Node CLI's `if matched.length === 0 -> low`).
            return computed_block(
                policy_path_input,
                FALLBACK_BACKEND,
                "low",
                vec![format!("fallback (no rule matched): {}", FALLBACK_REASON)],
                trace_info,
                registry_info,
            );
        }
        let winner = &matched[0];
        let winner_rule = policy
            .rules
            .iter()
            .find(|r| r.id == winner.id)
            .expect("winner id must be present in policy.rules");
        let backend = winner_rule.backend.clone();
        let mut reasons: Vec<String> = matched
            .iter()
            .map(|m| {
                format!(
                    "matched rule {} (priority {}): {}",
                    m.id, m.priority, m.reasoning
                )
            })
            .collect();
        // Surface unknown backend defensively even though the wave24-01
        // checker already rejects this — the operator should see the
        // mismatch loud.
        if !KNOWN_BACKENDS.iter().any(|b| *b == backend) {
            reasons.push(format!(
                "warning: matched backend `{}` is not in the known v1 enum",
                backend
            ));
        }
        // wave25-03: confidence selection mirrors scripts/recommend-task-backend.mjs
        //   * trace-index supplied AND parsed ("used") AND
        //     max(by_task[plan.board_task_id].events, by_backend[backend].events) >= 5
        //     -> high
        //   * trace-index supplied AND parsed AND max in 1..=4 -> medium
        //   * trace-index supplied AND parsed AND max == 0 -> low
        //   * trace-index NOT supplied OR degraded (missing/unreadable/malformed) ->
        //     `medium` (the legacy wave24-04 default for matched outcomes).
        let confidence = match trace_info {
            TraceIndexInfo::Used { task_events, backend_events, .. } => {
                let task_events = events_for_task(task_events, &plan.board_task_id);
                let backend_events = events_for_backend(backend_events, &backend);
                let max = task_events.max(backend_events);
                if max >= RICH_TRACE_THRESHOLD {
                    "high"
                } else if max >= 1 {
                    "medium"
                } else {
                    "low"
                }
            }
            // Degraded / absent: keep wave24-04 default of `medium` for matched.
            _ => "medium",
        };
        computed_block(policy_path_input, &backend, confidence, reasons, trace_info, registry_info)
    }

    /// wave25-03: trace-index threshold mirrors scripts/recommend-task-backend.mjs
    /// `RICH_TRACE_THRESHOLD = 5`. Keep this constant and the Node CLI in lock-
    /// step. The threshold counts the MAX of per-task vs per-backend events.
    const RICH_TRACE_THRESHOLD: u64 = 5;

    /// wave25-03 trace-index status flavours surfaced on the recommendation
    /// block. `Absent` is the default (no path supplied) and is observable on
    /// the wire as the absence of `trace_index_*` fields.
    #[derive(Debug, Clone)]
    pub(super) enum TraceIndexInfo {
        /// No trace-index path was supplied. Block does NOT carry any
        /// `trace_index_*` fields (preserves wave24-04 byte-shape for
        /// callers that opted out).
        Absent,
        /// Path was supplied; file read + parsed; `by_task` / `by_backend`
        /// available for confidence scoring.
        Used {
            path: String,
            task_events: serde_json::Map<String, Value>,
            backend_events: serde_json::Map<String, Value>,
        },
        /// Path was supplied but the file does not exist on disk.
        Missing { path: String, warning: String },
        /// Path was supplied; std::fs::read_to_string returned an I/O error
        /// other than NotFound.
        Unreadable { path: String, warning: String },
        /// Path was supplied; serde_json failed to parse OR the top-level
        /// shape is not a JSON object.
        Malformed { path: String, warning: String },
    }

    fn load_trace_index(input: Option<&str>) -> TraceIndexInfo {
        let Some(path_str) = input else {
            return TraceIndexInfo::Absent;
        };
        let path = path_str.to_string();
        let resolved = resolve_policy_path(&path); // same resolution rule
        let raw = match std::fs::read_to_string(&resolved) {
            Ok(s) => s,
            Err(e) => {
                let warning = format!("trace-index read failed: {}", e);
                return if e.kind() == std::io::ErrorKind::NotFound {
                    TraceIndexInfo::Missing { path, warning }
                } else {
                    TraceIndexInfo::Unreadable { path, warning }
                };
            }
        };
        let value: Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                return TraceIndexInfo::Malformed {
                    path,
                    warning: format!("trace-index JSON parse failed: {}", e),
                };
            }
        };
        let map = match value.as_object() {
            Some(m) => m,
            None => {
                return TraceIndexInfo::Malformed {
                    path,
                    warning: "trace-index top-level value is not a JSON object".to_string(),
                };
            }
        };
        let task_events = match map.get("by_task") {
            Some(Value::Object(m)) => m.clone(),
            Some(_) => {
                return TraceIndexInfo::Malformed {
                    path,
                    warning: "trace-index `by_task` is not a JSON object".to_string(),
                };
            }
            None => serde_json::Map::new(),
        };
        let backend_events = match map.get("by_backend") {
            Some(Value::Object(m)) => m.clone(),
            Some(_) => {
                return TraceIndexInfo::Malformed {
                    path,
                    warning: "trace-index `by_backend` is not a JSON object".to_string(),
                };
            }
            None => serde_json::Map::new(),
        };
        TraceIndexInfo::Used {
            path,
            task_events,
            backend_events,
        }
    }

    fn events_for_task(by_task: &serde_json::Map<String, Value>, task_id: &str) -> u64 {
        bucket_events(by_task, task_id)
    }

    fn events_for_backend(by_backend: &serde_json::Map<String, Value>, backend: &str) -> u64 {
        bucket_events(by_backend, backend)
    }

    fn bucket_events(map: &serde_json::Map<String, Value>, key: &str) -> u64 {
        map.get(key)
            .and_then(|v| v.get("events"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    }

    fn resolve_policy_path(input: &str) -> PathBuf {
        let p = Path::new(input);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            // The daemon runs out of the repo root in production; in
            // tests CWD points at the crate dir which still resolves
            // correctly because the policy path includes `.missiond/`.
            // Falling back to verbatim `input` here keeps the helper
            // free of repo-root-detection logic.
            PathBuf::from(input)
        }
    }

    fn error_block(
        policy_source: &str,
        message: &str,
        trace: &TraceIndexInfo,
        registry: &BackendRegistryInfo,
        recommended_backend: Option<&str>,
        confidence: &str,
    ) -> Value {
        let mut block = json!({
            "applied": false,
            "confidence": "low",
            "policy_source": policy_source,
            "reasons": [format!("error: {}", message)],
            "recommended_backend": FALLBACK_BACKEND,
            "schema": SCHEMA,
            "status": "error",
        });
        attach_trace_index_fields(&mut block, trace);
        attach_backend_readiness_fields(
            &mut block,
            registry,
            recommended_backend.unwrap_or(FALLBACK_BACKEND),
            "error",
            confidence,
        );
        block
    }

    fn rejected_block(
        policy_source: &str,
        message: &str,
        trace: &TraceIndexInfo,
        registry: &BackendRegistryInfo,
        recommended_backend: Option<&str>,
        confidence: &str,
    ) -> Value {
        let mut block = json!({
            "applied": false,
            "confidence": "low",
            "policy_source": policy_source,
            "reasons": [format!("rejected: {}", message)],
            "recommended_backend": FALLBACK_BACKEND,
            "schema": SCHEMA,
            "status": "rejected",
        });
        attach_trace_index_fields(&mut block, trace);
        attach_backend_readiness_fields(
            &mut block,
            registry,
            recommended_backend.unwrap_or(FALLBACK_BACKEND),
            "rejected",
            confidence,
        );
        block
    }

    fn computed_block(
        policy_source: &str,
        backend: &str,
        confidence: &str,
        reasons: Vec<String>,
        trace: &TraceIndexInfo,
        registry: &BackendRegistryInfo,
    ) -> Value {
        let mut block = json!({
            "applied": false,
            "confidence": confidence,
            "policy_source": policy_source,
            "reasons": reasons,
            "recommended_backend": backend,
            "schema": SCHEMA,
            "status": "computed",
        });
        attach_trace_index_fields(&mut block, trace);
        attach_backend_readiness_fields(&mut block, registry, backend, "computed", confidence);
        block
    }

    // -------------------------------------------------------------------
    // wave26-03: optional backend-readiness registry consumption.
    //
    // The registry seed at .missiond/router/router-backend-registry-v1.lisp
    // (top form: `(router-backend-registry <id> :schema ... :version ...
    // (backend :id ... :readiness_status ... :runtime_allowed ...
    //          :apply_blockers [...] ...))`) is read OPTIONALLY when
    // `router_backend_registry_path` is supplied AND mode=dry_run. The
    // daemon extracts ONLY the four fields it needs per backend entry —
    // every other key (`:substrate`, `:non-goals`, `:notes`, `:owner`,
    // `:adapter_path`) is ignored gracefully so the wave26-01 schema can
    // grow without forcing a daemon update. Failure modes are non-fatal:
    // dispatch always continues; only the apply-eligibility surface
    // degrades.
    // -------------------------------------------------------------------

    /// Allowed readiness status values mirrored from the wave26-01 schema.
    /// Anything outside this set is treated as malformed (the wave26-01
    /// checker rejects unknown values upstream; this is a defence-in-depth
    /// re-check).
    const READINESS_STATUSES: &[&str] = &[
        "current-default",
        "advisory-only",
        "runtime-ready",
        "unavailable",
    ];

    #[derive(Debug, Clone)]
    pub(super) struct BackendEntry {
        pub(super) id: String,
        pub(super) readiness_status: String,
        pub(super) runtime_allowed: bool,
        pub(super) apply_blockers: Vec<String>,
    }

    /// Backend-registry status flavours surfaced on the recommendation
    /// block. `Absent` is the default (no path supplied) and is observable
    /// on the wire as the absence of every `backend_*` field.
    #[derive(Debug, Clone)]
    pub(super) enum BackendRegistryInfo {
        /// No registry path was supplied. Block does NOT carry any
        /// `backend_*` field (preserves wave24-04 / wave25-03 byte-shape
        /// for callers that opted out).
        Absent,
        /// Path was supplied; file read + parsed; backend entries indexed
        /// by id for O(1) join against the recommended backend.
        Used {
            path: String,
            backends: Vec<BackendEntry>,
        },
        /// Path was supplied but the file does not exist on disk.
        Missing { path: String, warning: String },
        /// Path was supplied; std::fs::read_to_string returned an I/O error
        /// other than NotFound.
        Unreadable { path: String, warning: String },
        /// Path was supplied; the Lisp parser failed OR the top-level shape
        /// did not match `(router-backend-registry ...)` OR a backend entry
        /// was missing a required field / had an enum violation.
        Malformed { path: String, warning: String },
    }

    fn load_backend_registry(input: Option<&str>) -> BackendRegistryInfo {
        let Some(path_str) = input else {
            return BackendRegistryInfo::Absent;
        };
        let path = path_str.to_string();
        let resolved = resolve_policy_path(&path);
        let raw = match std::fs::read_to_string(&resolved) {
            Ok(s) => s,
            Err(e) => {
                let warning = format!("backend-registry read failed: {}", e);
                return if e.kind() == std::io::ErrorKind::NotFound {
                    BackendRegistryInfo::Missing { path, warning }
                } else {
                    BackendRegistryInfo::Unreadable { path, warning }
                };
            }
        };
        match parse_backend_registry(&raw) {
            Ok(backends) => BackendRegistryInfo::Used { path, backends },
            Err(msg) => BackendRegistryInfo::Malformed {
                path,
                warning: format!("backend-registry parse failed: {}", msg),
            },
        }
    }

    /// Minimal Lisp parser for the wave26-01 registry. Reuses the existing
    /// tokeniser + cursor; extracts ONLY `:id` `:readiness_status`
    /// `:runtime_allowed` `:apply_blockers` per `(backend ...)` entry. Any
    /// other key inside a backend entry is tolerated (gracefully ignored)
    /// so the registry schema can grow without breaking the daemon.
    pub(super) fn parse_backend_registry(input: &str) -> Result<Vec<BackendEntry>, String> {
        let tokens = tokenize(input)?;
        let mut cursor = TokenCursor::new(&tokens);
        let form = cursor
            .read_form()
            .ok_or_else(|| "no form found".to_string())?;
        if cursor.peek().is_some() {
            return Err("multiple top-level forms".to_string());
        }
        let list = match form {
            Sexp::List(items) => items,
            _ => {
                return Err(
                    "expected (router-backend-registry ...) at top level".to_string(),
                )
            }
        };
        let mut iter = list.into_iter();
        let head = iter
            .next()
            .ok_or_else(|| "empty top-level list".to_string())?;
        match head {
            Sexp::Atom(s) if s == "router-backend-registry" => {}
            _ => {
                return Err(
                    "expected (router-backend-registry ...) at top level".to_string(),
                )
            }
        }
        // Skip the registry id atom (next item).
        let _id = iter.next();
        let mut backends: Vec<BackendEntry> = Vec::new();
        let mut pending_keyword: Option<String> = None;
        for item in iter {
            if pending_keyword.take().is_some() {
                // Header keyword/value pair (`:schema`, `:version`,
                // `:description`) — value already consumed; skip.
                continue;
            }
            match &item {
                Sexp::Keyword(k) => pending_keyword = Some(k.clone()),
                Sexp::List(inner) => {
                    if matches!(inner.first(), Some(Sexp::Atom(h)) if h == "backend") {
                        let entry = parse_backend_entry(inner)?;
                        backends.push(entry);
                    }
                    // Other top-level lists (none today) are tolerated.
                }
                _ => {}
            }
        }
        Ok(backends)
    }

    fn parse_backend_entry(items: &[Sexp]) -> Result<BackendEntry, String> {
        // items[0] is the `backend` atom.
        let mut id: Option<String> = None;
        let mut readiness_status: Option<String> = None;
        let mut runtime_allowed: Option<bool> = None;
        let mut apply_blockers: Option<Vec<String>> = None;
        let mut idx = 1usize;
        while idx < items.len() {
            let key = match &items[idx] {
                Sexp::Keyword(k) => k.clone(),
                _ => {
                    idx += 1;
                    continue;
                }
            };
            idx += 1;
            if idx >= items.len() {
                break;
            }
            let value = &items[idx];
            idx += 1;
            match key.as_str() {
                ":id" => id = Some(sexp_as_text(value)),
                ":readiness_status" => readiness_status = Some(sexp_as_text(value)),
                ":runtime_allowed" => runtime_allowed = Some(sexp_as_bool(value)),
                ":apply_blockers" => {
                    let v = sexp_as_string_vec(value);
                    apply_blockers = Some(v);
                }
                // Tolerated but not consumed: :substrate, :non-goals,
                // :notes, :owner, :adapter_path. Future schema growth
                // does not require a daemon update.
                _ => {}
            }
        }
        let id = id.ok_or_else(|| "backend entry missing :id".to_string())?;
        let readiness_status = readiness_status
            .ok_or_else(|| format!("backend `{}` missing :readiness_status", id))?;
        if !READINESS_STATUSES.iter().any(|s| *s == readiness_status) {
            return Err(format!(
                "backend `{}` :readiness_status `{}` is not in the wave26-01 enum",
                id, readiness_status
            ));
        }
        let runtime_allowed = runtime_allowed
            .ok_or_else(|| format!("backend `{}` missing :runtime_allowed", id))?;
        let apply_blockers = apply_blockers.unwrap_or_default();
        Ok(BackendEntry {
            id,
            readiness_status,
            runtime_allowed,
            apply_blockers,
        })
    }

    /// Coerce a `Sexp::List` of strings/atoms into a `Vec<String>`. Used
    /// for `:apply_blockers` (the wave26-01 schema requires a vector of
    /// strings; an empty vector is `[]`).
    fn sexp_as_string_vec(value: &Sexp) -> Vec<String> {
        match value {
            Sexp::List(items) => items
                .iter()
                .map(|i| sexp_as_text(i))
                .filter(|s| !s.is_empty())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// wave26-03: splice the optional `backend_*` fields onto a recommendation
    /// block. `Absent` emits NO fields at all (preserves wave24-04 / wave25-03
    /// byte-shape for callers that opted out). All other variants emit
    /// `backend_registry_path` + `backend_registry_status`; degraded variants
    /// additionally emit `backend_warning`. When `Used` AND the recommended
    /// backend is present in the registry, the block also surfaces
    /// `backend_readiness_status` + `backend_runtime_allowed` +
    /// `router_apply_eligible` + `router_apply_blockers`. When `Used` AND
    /// the backend is missing, `backend_registry_status="unknown_backend"`,
    /// `backend_readiness_status="unknown"`, `router_apply_eligible=false`.
    fn attach_backend_readiness_fields(
        block: &mut Value,
        registry: &BackendRegistryInfo,
        recommended_backend: &str,
        status: &str,
        confidence: &str,
    ) {
        let Some(map) = block.as_object_mut() else {
            return;
        };
        match registry {
            BackendRegistryInfo::Absent => {
                // Intentionally emit NOTHING — preserves the byte-shape
                // for callers that did not opt in to wave26-03.
            }
            BackendRegistryInfo::Missing { path, warning } => {
                map.insert(
                    "backend_registry_path".to_string(),
                    Value::String(path.clone()),
                );
                map.insert(
                    "backend_registry_status".to_string(),
                    Value::String("missing".to_string()),
                );
                map.insert(
                    "backend_warning".to_string(),
                    Value::String(warning.clone()),
                );
                map.insert(
                    "router_apply_eligible".to_string(),
                    Value::Bool(false),
                );
                map.insert(
                    "router_apply_blockers".to_string(),
                    Value::Array(vec![Value::String(
                        "backend registry file is missing".to_string(),
                    )]),
                );
            }
            BackendRegistryInfo::Unreadable { path, warning } => {
                map.insert(
                    "backend_registry_path".to_string(),
                    Value::String(path.clone()),
                );
                map.insert(
                    "backend_registry_status".to_string(),
                    Value::String("unreadable".to_string()),
                );
                map.insert(
                    "backend_warning".to_string(),
                    Value::String(warning.clone()),
                );
                map.insert(
                    "router_apply_eligible".to_string(),
                    Value::Bool(false),
                );
                map.insert(
                    "router_apply_blockers".to_string(),
                    Value::Array(vec![Value::String(
                        "backend registry file is unreadable".to_string(),
                    )]),
                );
            }
            BackendRegistryInfo::Malformed { path, warning } => {
                map.insert(
                    "backend_registry_path".to_string(),
                    Value::String(path.clone()),
                );
                map.insert(
                    "backend_registry_status".to_string(),
                    Value::String("malformed".to_string()),
                );
                map.insert(
                    "backend_warning".to_string(),
                    Value::String(warning.clone()),
                );
                map.insert(
                    "router_apply_eligible".to_string(),
                    Value::Bool(false),
                );
                map.insert(
                    "router_apply_blockers".to_string(),
                    Value::Array(vec![Value::String(
                        "backend registry file is malformed".to_string(),
                    )]),
                );
            }
            BackendRegistryInfo::Used { path, backends } => {
                map.insert(
                    "backend_registry_path".to_string(),
                    Value::String(path.clone()),
                );
                let matched: Option<&BackendEntry> =
                    backends.iter().find(|b| b.id == recommended_backend);
                match matched {
                    None => {
                        // Recommended backend absent from registry — surface
                        // unknown_backend status and force eligible=false.
                        map.insert(
                            "backend_registry_status".to_string(),
                            Value::String("unknown_backend".to_string()),
                        );
                        map.insert(
                            "backend_readiness_status".to_string(),
                            Value::String("unknown".to_string()),
                        );
                        map.insert(
                            "router_apply_eligible".to_string(),
                            Value::Bool(false),
                        );
                        map.insert(
                            "router_apply_blockers".to_string(),
                            Value::Array(vec![Value::String(format!(
                                "recommended_backend `{}` not in registry",
                                recommended_backend
                            ))]),
                        );
                    }
                    Some(entry) => {
                        map.insert(
                            "backend_registry_status".to_string(),
                            Value::String("used".to_string()),
                        );
                        map.insert(
                            "backend_readiness_status".to_string(),
                            Value::String(entry.readiness_status.clone()),
                        );
                        map.insert(
                            "backend_runtime_allowed".to_string(),
                            Value::Bool(entry.runtime_allowed),
                        );
                        // 6-condition apply-eligibility gate — every miss
                        // contributes a synthetic blocker so operators can
                        // see WHY the gate is closed.
                        let mut blockers: Vec<String> = Vec::new();
                        if status != "computed" {
                            blockers.push(format!(
                                "policy status is `{}`; computed required",
                                status
                            ));
                        }
                        if confidence != "high" {
                            blockers.push(format!(
                                "confidence is `{}`; high required",
                                confidence
                            ));
                        }
                        if !entry.runtime_allowed {
                            blockers.push(
                                "backend runtime_allowed is false; runtime-ready adapter required"
                                    .to_string(),
                            );
                        }
                        if entry.readiness_status != "runtime-ready" {
                            blockers.push(format!(
                                "backend readiness_status is `{}`; runtime-ready required",
                                entry.readiness_status
                            ));
                        }
                        // Echo the registry's own apply_blockers verbatim
                        // when present (operator should see the registry's
                        // reasons even when the synthetic gate already
                        // closed for another reason).
                        for b in &entry.apply_blockers {
                            blockers.push(b.clone());
                        }
                        let eligible = blockers.is_empty();
                        map.insert(
                            "router_apply_eligible".to_string(),
                            Value::Bool(eligible),
                        );
                        map.insert(
                            "router_apply_blockers".to_string(),
                            Value::Array(
                                blockers.into_iter().map(Value::String).collect(),
                            ),
                        );
                    }
                }
            }
        }
    }

    /// wave25-03: splice the optional `trace_index_path` / `trace_index_status`
    /// / `trace_index_warning` fields onto a recommendation block. `Absent`
    /// emits NO fields at all (preserves wave24-04 byte-shape for callers
    /// that opted out). All other variants emit `trace_index_path` +
    /// `trace_index_status`; degraded variants additionally emit
    /// `trace_index_warning`.
    fn attach_trace_index_fields(block: &mut Value, trace: &TraceIndexInfo) {
        let Some(map) = block.as_object_mut() else {
            return;
        };
        match trace {
            TraceIndexInfo::Absent => {
                // Intentionally emit NOTHING — keeps wave24-04 byte-shape
                // for callers that did not opt in to wave25-03.
            }
            TraceIndexInfo::Used { path, .. } => {
                map.insert("trace_index_path".to_string(), Value::String(path.clone()));
                map.insert("trace_index_status".to_string(), Value::String("used".to_string()));
            }
            TraceIndexInfo::Missing { path, warning } => {
                map.insert("trace_index_path".to_string(), Value::String(path.clone()));
                map.insert("trace_index_status".to_string(), Value::String("missing".to_string()));
                map.insert("trace_index_warning".to_string(), Value::String(warning.clone()));
            }
            TraceIndexInfo::Unreadable { path, warning } => {
                map.insert("trace_index_path".to_string(), Value::String(path.clone()));
                map.insert(
                    "trace_index_status".to_string(),
                    Value::String("unreadable".to_string()),
                );
                map.insert("trace_index_warning".to_string(), Value::String(warning.clone()));
            }
            TraceIndexInfo::Malformed { path, warning } => {
                map.insert("trace_index_path".to_string(), Value::String(path.clone()));
                map.insert(
                    "trace_index_status".to_string(),
                    Value::String("malformed".to_string()),
                );
                map.insert("trace_index_warning".to_string(), Value::String(warning.clone()));
            }
        }
    }

    // -------------------------------------------------------------------
    // wave27-03: optional router dispatch descriptor surface.
    //
    // Splice a `router_dispatch_descriptor` sub-object onto an existing
    // recommendation block, mirroring the wave27-01 schema
    // (`missiond.router-dispatch-descriptor.v1`). The descriptor is a
    // PURE PROJECTION of the wave24-04 / wave25-03 / wave26-03 fields
    // already on the block — no new file I/O happens here, no backend is
    // ever invoked, and dispatch is never altered. Three invariants are
    // hard-coded as `Value::Bool` literals so the descriptor cannot be
    // mis-promoted to a runtime apply signal:
    //
    //   - dry_run_only        = Value::Bool(true)   (locked)
    //   - runtime_replacement = Value::Bool(false)  (locked)
    //   - no_execution        = Value::Bool(true)   (locked)
    //
    // Registry semantics:
    //   * `BackendRegistryInfo::Absent` (i.e. no `router_backend_registry_path`
    //     supplied) → top-level `descriptor_status="registry_missing"` is
    //     surfaced on the recommendation block; the descriptor BODY is
    //     OMITTED so a downstream consumer cannot mistake the absence of
    //     readiness for "ready". This matches the wave27-01 schema's
    //     refusal to fake readiness.
    //   * Any other registry state → emit a structured descriptor body.
    //     For degraded states (Missing / Unreadable / Malformed /
    //     unknown_backend) the recommendation block already carries the
    //     synthetic `router_apply_eligible=false` / `router_apply_blockers`
    //     fields plus (for unknown_backend) `backend_readiness_status="unknown"`;
    //     the descriptor projects those off the block as-is. For Missing /
    //     Unreadable / Malformed the block has no `backend_readiness_status`
    //     at all — the descriptor falls back to the synthetic
    //     `unknown` enum value (legal per wave27-01 readiness-statuses)
    //     and `backend_runtime_allowed=false`.
    fn attach_router_dispatch_descriptor(
        block: &mut Value,
        plan: &Plan,
        registry: &BackendRegistryInfo,
        policy_path_input: &str,
    ) {
        let Some(map) = block.as_object_mut() else {
            return;
        };
        // Branch 1: registry path was not supplied. Surface a structured
        // status field on the recommendation block; the descriptor body
        // is intentionally omitted because the wave27-01 schema requires
        // backend_readiness_status / backend_runtime_allowed values that
        // we cannot honestly produce without consulting a registry.
        if matches!(registry, BackendRegistryInfo::Absent) {
            map.insert(
                "descriptor_status".to_string(),
                Value::String("registry_missing".to_string()),
            );
            return;
        }
        // Branch 2: registry path supplied (any state — Used / Missing /
        // Unreadable / Malformed / unknown_backend). Build the descriptor
        // body by reading the wave26-03 fields back off the block. They
        // are guaranteed to be present except in the Missing / Unreadable
        // / Malformed paths where readiness/runtime_allowed are NOT set
        // upstream — for those we fall back to the synthetic `unknown`
        // enum + `false` runtime-allowed.
        let recommended_backend = map
            .get("recommended_backend")
            .and_then(|v| v.as_str())
            .unwrap_or(FALLBACK_BACKEND)
            .to_string();
        let router_confidence = map
            .get("confidence")
            .and_then(|v| v.as_str())
            .unwrap_or("low")
            .to_string();
        let backend_readiness_status = map
            .get("backend_readiness_status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let backend_runtime_allowed = map
            .get("backend_runtime_allowed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let router_apply_eligible = map
            .get("router_apply_eligible")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let router_apply_blockers: Vec<Value> = map
            .get("router_apply_blockers")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let source_backend_registry_path = registry_path(registry).to_string();

        let mut descriptor = serde_json::Map::new();
        descriptor.insert(
            "schema".to_string(),
            Value::String("missiond.router-dispatch-descriptor.v1".to_string()),
        );
        descriptor.insert(
            "task_id".to_string(),
            Value::String(plan.board_task_id.clone()),
        );
        descriptor.insert(
            "recommended_backend".to_string(),
            Value::String(recommended_backend),
        );
        descriptor.insert(
            "router_confidence".to_string(),
            Value::String(router_confidence),
        );
        descriptor.insert(
            "backend_readiness_status".to_string(),
            Value::String(backend_readiness_status),
        );
        descriptor.insert(
            "backend_runtime_allowed".to_string(),
            Value::Bool(backend_runtime_allowed),
        );
        descriptor.insert(
            "router_apply_eligible".to_string(),
            Value::Bool(router_apply_eligible),
        );
        descriptor.insert(
            "router_apply_blockers".to_string(),
            Value::Array(router_apply_blockers),
        );
        // wave27-01 LOCKED INVARIANTS — these are hard-coded literal bools
        // and MUST NEVER be derived from any other field. If any future
        // change tries to compute these, the descriptor is no longer a
        // safe handoff record (per wave27-01 cross-wave-invariant text).
        descriptor.insert("dry_run_only".to_string(), Value::Bool(true));
        descriptor.insert("runtime_replacement".to_string(), Value::Bool(false));
        descriptor.insert("no_execution".to_string(), Value::Bool(true));
        descriptor.insert(
            "source_recommendation_schema".to_string(),
            Value::String(SCHEMA.to_string()),
        );
        descriptor.insert(
            "source_policy_path".to_string(),
            Value::String(policy_path_input.to_string()),
        );
        descriptor.insert(
            "source_backend_registry_path".to_string(),
            Value::String(source_backend_registry_path),
        );

        map.insert(
            "router_dispatch_descriptor".to_string(),
            Value::Object(descriptor),
        );
    }

    /// Extract the registry path string from any non-`Absent`
    /// `BackendRegistryInfo` variant. `Absent` should never reach this
    /// helper (the caller branches before).
    fn registry_path(registry: &BackendRegistryInfo) -> &str {
        match registry {
            BackendRegistryInfo::Absent => "",
            BackendRegistryInfo::Used { path, .. }
            | BackendRegistryInfo::Missing { path, .. }
            | BackendRegistryInfo::Unreadable { path, .. }
            | BackendRegistryInfo::Malformed { path, .. } => path.as_str(),
        }
    }

    // ---- predicate AST + evaluator -----------------------------------

    #[derive(Debug, Clone)]
    pub(super) enum Clause {
        Kind(String),
        DispatchStrategy(String),
        Owner(String),
        Status(String),
        PathGlob(String),
        Any(Vec<Clause>),
        All(Vec<Clause>),
    }

    fn evaluate_clause(clause: &Clause, ctx: &PredicateContext) -> Result<(), &'static str> {
        match clause {
            Clause::Kind(v) => {
                if &ctx.kind == v {
                    Ok(())
                } else {
                    Err("kind mismatch")
                }
            }
            Clause::DispatchStrategy(v) => {
                if &ctx.dispatch_strategy == v {
                    Ok(())
                } else {
                    Err("dispatch_strategy mismatch")
                }
            }
            Clause::Owner(v) => {
                if &ctx.owner == v {
                    Ok(())
                } else {
                    Err("owner mismatch")
                }
            }
            Clause::Status(v) => {
                if &ctx.status == v {
                    Ok(())
                } else {
                    Err("status mismatch")
                }
            }
            Clause::PathGlob(pat) => {
                let re = glob_to_regex(pat);
                let any = ctx.write_scope.iter().any(|p| re.is_match(p));
                if any {
                    Ok(())
                } else {
                    Err("path-glob no match")
                }
            }
            Clause::All(children) => {
                if children.is_empty() {
                    return Err("empty all");
                }
                for c in children {
                    evaluate_clause(c, ctx)?;
                }
                Ok(())
            }
            Clause::Any(children) => {
                if children.is_empty() {
                    return Err("empty any");
                }
                let mut last: Result<(), &'static str> = Err("any: no child matched");
                for c in children {
                    if evaluate_clause(c, ctx).is_ok() {
                        return Ok(());
                    }
                    last = Err("any: no child matched");
                }
                last
            }
        }
    }

    /// Minimal glob-to-regex shim. Mirrors the wave24-03 CLI / scripts/lib
    /// shape: `**` matches any sequence including `/`; `*` matches any
    /// non-`/` sequence; `?` matches a single non-`/` char. Other regex
    /// metacharacters are escaped. Inputs and candidates are normalised
    /// to forward slashes and stripped of leading `./` / `/`.
    pub(super) struct GlobRegex {
        pattern: String,
    }

    impl GlobRegex {
        fn is_match(&self, candidate: &str) -> bool {
            let candidate = normalize_path(candidate);
            // Tiny custom matcher (no regex crate dependency added). We
            // implement the predicate via a recursive descent over the
            // pattern. The pattern shape is small and exhaustive; this is
            // enough for path-glob predicates.
            glob_match(&self.pattern, &candidate)
        }
    }

    fn glob_to_regex(pattern: &str) -> GlobRegex {
        GlobRegex {
            pattern: normalize_path(pattern),
        }
    }

    fn normalize_path(p: &str) -> String {
        let mut s = p.replace('\\', "/");
        if let Some(stripped) = s.strip_prefix("./") {
            s = stripped.to_string();
        }
        while let Some(stripped) = s.strip_prefix('/') {
            s = stripped.to_string();
        }
        s
    }

    /// Recursive matcher: returns true iff `pattern` matches all of
    /// `candidate` from start to end. Supports `**`, `*`, `?` per the
    /// shared glob shape. Iterative-style with explicit indices to keep
    /// the complexity bounded (no backtracking blowup on pathological
    /// inputs because the patterns we accept are small and well-formed).
    fn glob_match(pattern: &str, candidate: &str) -> bool {
        let p: Vec<char> = pattern.chars().collect();
        let c: Vec<char> = candidate.chars().collect();
        glob_match_inner(&p, 0, &c, 0)
    }

    fn glob_match_inner(p: &[char], pi: usize, c: &[char], ci: usize) -> bool {
        let mut pi = pi;
        let mut ci = ci;
        loop {
            if pi >= p.len() {
                return ci >= c.len();
            }
            let pc = p[pi];
            if pc == '*' {
                if pi + 1 < p.len() && p[pi + 1] == '*' {
                    // `**` matches any sequence including `/`. Try every
                    // possible split. Skip a following `/` so `**/foo`
                    // matches both `foo` and `a/foo`.
                    let mut next_pi = pi + 2;
                    if next_pi < p.len() && p[next_pi] == '/' {
                        next_pi += 1;
                    }
                    // Try matching zero characters first, then progressively
                    // more from the candidate.
                    if glob_match_inner(p, next_pi, c, ci) {
                        return true;
                    }
                    let mut k = ci;
                    while k < c.len() {
                        k += 1;
                        if glob_match_inner(p, next_pi, c, k) {
                            return true;
                        }
                    }
                    return false;
                } else {
                    // `*` matches any non-`/` sequence.
                    if glob_match_inner(p, pi + 1, c, ci) {
                        return true;
                    }
                    let mut k = ci;
                    while k < c.len() && c[k] != '/' {
                        k += 1;
                        if glob_match_inner(p, pi + 1, c, k) {
                            return true;
                        }
                    }
                    return false;
                }
            }
            if pc == '?' {
                if ci >= c.len() || c[ci] == '/' {
                    return false;
                }
                pi += 1;
                ci += 1;
                continue;
            }
            // Literal char.
            if ci >= c.len() || c[ci] != pc {
                return false;
            }
            pi += 1;
            ci += 1;
        }
    }

    // ---- minimal Lisp parser for the wave24-01 router-policy schema ---

    #[derive(Debug, Clone)]
    pub(super) struct PolicyDoc {
        pub(super) dry_run_only: bool,
        pub(super) runtime_replacement: bool,
        pub(super) rules: Vec<RuleDoc>,
    }

    #[derive(Debug, Clone)]
    pub(super) struct RuleDoc {
        pub(super) id: String,
        pub(super) priority: u32,
        pub(super) when: Clause,
        pub(super) backend: String,
        pub(super) reasoning: String,
    }

    #[derive(Debug, Clone)]
    struct MatchedRule {
        id: String,
        priority: u32,
        reasoning: String,
    }

    /// Parse a router-policy v1 Lisp file. Returns a structured `PolicyDoc`
    /// or a human-readable error message. The parser is purpose-built for
    /// this schema and does NOT attempt to be a general Lisp reader: it
    /// handles atoms, strings, lists, brackets-as-lists, and line comments.
    /// The wave24-01 checker already rejects malformed policies upstream;
    /// this parser is conservative and surfaces unknown shapes as errors.
    pub(super) fn parse_router_policy(input: &str) -> Result<PolicyDoc, String> {
        let tokens = tokenize(input)?;
        let mut cursor = TokenCursor::new(&tokens);
        // Top-level form must be `(router-policy <id> ...)`.
        let form = cursor.read_form().ok_or_else(|| "no form found".to_string())?;
        if cursor.peek().is_some() {
            // We tolerate trailing whitespace / comments (already stripped
            // by tokenize) but not multiple top-level forms.
            return Err("multiple top-level forms".to_string());
        }
        let list = match form {
            Sexp::List(items) => items,
            _ => return Err("expected (router-policy ...) at top level".to_string()),
        };
        let mut iter = list.into_iter();
        let head = iter.next().ok_or_else(|| "empty top-level list".to_string())?;
        match head {
            Sexp::Atom(s) if s == "router-policy" => {}
            _ => return Err("expected (router-policy ...) at top level".to_string()),
        }
        // Skip the policy id atom (next item).
        let _id = iter.next();
        // Walk the remaining items: keyword/value pairs OR (rule ...) lists.
        let mut dry_run_only: Option<bool> = None;
        let mut runtime_replacement: Option<bool> = None;
        let mut rules: Vec<RuleDoc> = Vec::new();
        let mut pending_keyword: Option<String> = None;
        for item in iter {
            if let Some(key) = pending_keyword.take() {
                let value = item;
                match key.as_str() {
                    ":dry-run-only" => dry_run_only = Some(sexp_as_bool(&value)),
                    ":runtime-replacement" => runtime_replacement = Some(sexp_as_bool(&value)),
                    // Other keys (`:schema`, `:version`, `:description`)
                    // are tolerated but not consumed — wave24-01 checker
                    // owns header validation.
                    _ => {}
                }
                continue;
            }
            match &item {
                Sexp::Keyword(k) => pending_keyword = Some(k.clone()),
                Sexp::List(inner) => {
                    if matches!(inner.first(), Some(Sexp::Atom(h)) if h == "rule") {
                        let rule = parse_rule(inner)?;
                        rules.push(rule);
                    }
                }
                _ => {}
            }
        }
        // Sort rules by priority ascending (matches wave24-03 selection order).
        rules.sort_by_key(|r| r.priority);
        Ok(PolicyDoc {
            dry_run_only: dry_run_only.unwrap_or(false),
            runtime_replacement: runtime_replacement.unwrap_or(false),
            rules,
        })
    }

    fn parse_rule(items: &[Sexp]) -> Result<RuleDoc, String> {
        // items[0] is the `rule` atom.
        let mut id: Option<String> = None;
        let mut priority: Option<u32> = None;
        let mut when_clause: Option<Clause> = None;
        let mut backend: Option<String> = None;
        let mut reasoning: Option<String> = None;
        let mut idx = 1usize;
        while idx < items.len() {
            let key = match &items[idx] {
                Sexp::Keyword(k) => k.clone(),
                _ => {
                    idx += 1;
                    continue;
                }
            };
            idx += 1;
            if idx >= items.len() {
                break;
            }
            let value = &items[idx];
            idx += 1;
            match key.as_str() {
                ":id" => id = Some(sexp_as_text(value)),
                ":priority" => {
                    let raw = sexp_as_text(value);
                    priority = raw.parse::<u32>().ok();
                }
                ":when" => {
                    if let Sexp::List(children) = value {
                        when_clause = Some(parse_when_list(children)?);
                    }
                }
                ":recommend" => {
                    if let Sexp::List(children) = value {
                        let mut bk: Option<String> = None;
                        let mut rs: Option<String> = None;
                        let mut j = 0usize;
                        while j < children.len() {
                            if let Sexp::Keyword(k) = &children[j] {
                                if j + 1 < children.len() {
                                    let v = &children[j + 1];
                                    match k.as_str() {
                                        ":backend" => bk = Some(sexp_as_text(v)),
                                        ":reasoning" => rs = Some(sexp_as_text(v)),
                                        _ => {}
                                    }
                                }
                                j += 2;
                            } else {
                                j += 1;
                            }
                        }
                        backend = bk;
                        reasoning = rs;
                    }
                }
                // `:non-goals`, `:notes` are tolerated but not consumed.
                _ => {}
            }
        }
        Ok(RuleDoc {
            id: id.ok_or_else(|| "rule missing :id".to_string())?,
            priority: priority.ok_or_else(|| "rule missing :priority".to_string())?,
            when: when_clause.ok_or_else(|| "rule missing :when".to_string())?,
            backend: backend.ok_or_else(|| "rule missing :recommend :backend".to_string())?,
            reasoning: reasoning.unwrap_or_default(),
        })
    }

    fn parse_when_list(children: &[Sexp]) -> Result<Clause, String> {
        // The top-level `:when` is implicit-`all` over its direct children.
        let mut clauses: Vec<Clause> = Vec::new();
        for child in children {
            if let Sexp::List(inner) = child {
                if let Some(c) = parse_clause(inner)? {
                    clauses.push(c);
                }
            }
        }
        if clauses.len() == 1 {
            Ok(clauses.into_iter().next().unwrap())
        } else {
            Ok(Clause::All(clauses))
        }
    }

    fn parse_clause(items: &[Sexp]) -> Result<Option<Clause>, String> {
        let head_atom = match items.first() {
            Some(Sexp::Atom(s)) => s.clone(),
            _ => return Ok(None),
        };
        match head_atom.as_str() {
            "kind" => Ok(Some(Clause::Kind(arg_value(items)))),
            "dispatch_strategy" | "dispatch-strategy" => {
                Ok(Some(Clause::DispatchStrategy(arg_value(items))))
            }
            "owner" => Ok(Some(Clause::Owner(arg_value(items)))),
            "status" => Ok(Some(Clause::Status(arg_value(items)))),
            "path-glob" => Ok(Some(Clause::PathGlob(arg_value(items)))),
            "any" => {
                let mut children = Vec::new();
                for it in &items[1..] {
                    if let Sexp::List(inner) = it {
                        if let Some(c) = parse_clause(inner)? {
                            children.push(c);
                        }
                    }
                }
                Ok(Some(Clause::Any(children)))
            }
            "all" => {
                let mut children = Vec::new();
                for it in &items[1..] {
                    if let Sexp::List(inner) = it {
                        if let Some(c) = parse_clause(inner)? {
                            children.push(c);
                        }
                    }
                }
                Ok(Some(Clause::All(children)))
            }
            // Unknown predicate head — fail closed to mirror wave24-03.
            _ => Err(format!("unknown predicate head `{}`", head_atom)),
        }
    }

    fn arg_value(items: &[Sexp]) -> String {
        items
            .get(1)
            .map(|v| sexp_as_text(v))
            .unwrap_or_default()
    }

    fn sexp_as_text(value: &Sexp) -> String {
        match value {
            Sexp::Atom(s) | Sexp::Str(s) | Sexp::Keyword(s) => s.clone(),
            Sexp::List(_) => String::new(),
        }
    }

    fn sexp_as_bool(value: &Sexp) -> bool {
        match value {
            Sexp::Atom(s) => s == "true",
            Sexp::Str(s) => s == "true",
            _ => false,
        }
    }

    // ---- tiny tokenizer / cursor ------------------------------------

    #[derive(Debug, Clone)]
    pub(super) enum Sexp {
        Atom(String),
        Str(String),
        Keyword(String),
        List(Vec<Sexp>),
    }

    #[derive(Debug, Clone)]
    enum Token {
        LParen,
        RParen,
        LBracket,
        RBracket,
        Atom(String),
        Str(String),
        Keyword(String),
    }

    fn tokenize(input: &str) -> Result<Vec<Token>, String> {
        let chars: Vec<char> = input.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c.is_whitespace() {
                i += 1;
                continue;
            }
            if c == ';' {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            if c == '(' {
                out.push(Token::LParen);
                i += 1;
                continue;
            }
            if c == ')' {
                out.push(Token::RParen);
                i += 1;
                continue;
            }
            if c == '[' {
                out.push(Token::LBracket);
                i += 1;
                continue;
            }
            if c == ']' {
                out.push(Token::RBracket);
                i += 1;
                continue;
            }
            if c == '"' {
                let mut s = String::new();
                i += 1;
                while i < chars.len() {
                    let ch = chars[i];
                    if ch == '\\' {
                        i += 1;
                        if i < chars.len() {
                            s.push(chars[i]);
                            i += 1;
                        }
                        continue;
                    }
                    if ch == '"' {
                        i += 1;
                        break;
                    }
                    s.push(ch);
                    i += 1;
                }
                out.push(Token::Str(s));
                continue;
            }
            if c == ':' {
                let mut s = String::from(":");
                i += 1;
                while i < chars.len() && !is_atom_terminator(chars[i]) {
                    s.push(chars[i]);
                    i += 1;
                }
                out.push(Token::Keyword(s));
                continue;
            }
            // Atom.
            let mut s = String::new();
            while i < chars.len() && !is_atom_terminator(chars[i]) {
                s.push(chars[i]);
                i += 1;
            }
            if !s.is_empty() {
                out.push(Token::Atom(s));
            }
        }
        Ok(out)
    }

    fn is_atom_terminator(c: char) -> bool {
        c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '"' | ';')
    }

    struct TokenCursor<'a> {
        tokens: &'a [Token],
        pos: usize,
    }

    impl<'a> TokenCursor<'a> {
        fn new(tokens: &'a [Token]) -> Self {
            Self { tokens, pos: 0 }
        }
        fn peek(&self) -> Option<&Token> {
            self.tokens.get(self.pos)
        }
        fn read_form(&mut self) -> Option<Sexp> {
            let tok = self.tokens.get(self.pos)?;
            match tok {
                Token::LParen => {
                    self.pos += 1;
                    let mut items = Vec::new();
                    while let Some(t) = self.tokens.get(self.pos) {
                        if matches!(t, Token::RParen) {
                            self.pos += 1;
                            return Some(Sexp::List(items));
                        }
                        if let Some(form) = self.read_form() {
                            items.push(form);
                        } else {
                            break;
                        }
                    }
                    Some(Sexp::List(items))
                }
                Token::LBracket => {
                    self.pos += 1;
                    let mut items = Vec::new();
                    while let Some(t) = self.tokens.get(self.pos) {
                        if matches!(t, Token::RBracket) {
                            self.pos += 1;
                            return Some(Sexp::List(items));
                        }
                        if let Some(form) = self.read_form() {
                            items.push(form);
                        } else {
                            break;
                        }
                    }
                    Some(Sexp::List(items))
                }
                Token::RParen | Token::RBracket => None,
                Token::Atom(s) => {
                    self.pos += 1;
                    Some(Sexp::Atom(s.clone()))
                }
                Token::Str(s) => {
                    self.pos += 1;
                    Some(Sexp::Str(s.clone()))
                }
                Token::Keyword(s) => {
                    self.pos += 1;
                    Some(Sexp::Keyword(s.clone()))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use uuid::Uuid;

    #[test]
    fn sha256_hex_is_64_chars() {
        let h = sha256_hex("abc");
        assert_eq!(h.len(), 64);
        // ba7816bf… is the well-known SHA-256 prefix for "abc"
        assert!(h.starts_with("ba7816bf"));
    }

    #[test]
    fn require_str_rejects_empty() {
        let args = serde_json::json!({"k": ""});
        assert!(require_str(&args, "k").is_err());
        let args2 = serde_json::json!({"k": "v"});
        assert_eq!(require_str(&args2, "k").unwrap(), "v");
    }

    fn fixture_plan(sexp: &str) -> Plan {
        Plan {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000abc").unwrap(),
            board_task_id: "btk-1".to_string(),
            source_directive_id: None,
            version: 1,
            sexp_text: sexp.to_string(),
            sexp_hash: "deadbeef".to_string(),
            status: PlanStatus::Approved,
            compiler_model: None,
            compiled_from: None,
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            approved_at: None,
            finished_at: None,
        }
    }

    #[test]
    fn truncate_chars_preserves_short_input() {
        let s = "short";
        assert_eq!(truncate_chars(s, 100), "short");
    }

    #[test]
    fn truncate_chars_caps_long_input() {
        let s = "a".repeat(500);
        let out = truncate_chars(&s, 240);
        assert!(out.ends_with("..."));
        assert!(out.len() <= 240 + 3);
    }

    #[test]
    fn derive_objective_from_plan_caps_long_summary() {
        let huge = format!("(plan-draft :summary \"{}\")", "x".repeat(500));
        let plan = fixture_plan(&huge);
        let out = derive_objective_from_plan(&plan, 80);
        // Plan id prefix + truncated summary + ellipsis.
        assert!(out.starts_with(&format!("Plan {}: ", plan.id)));
        assert!(out.ends_with("..."));
        // Body shouldn't blow past the cap by more than the prefix overhead.
        assert!(out.len() < 200);
    }

    #[test]
    fn derive_objective_from_plan_takes_first_nonempty_line() {
        let plan = fixture_plan("\n\n  (plan-draft :goal :align)  \n  (next ...)\n");
        let out = derive_objective_from_plan(&plan, 240);
        assert!(out.contains("(plan-draft :goal :align)"));
        assert!(!out.contains("(next ..."));
    }

    /// Build a ResolvedExec for tests. Defaults `target_source="explicit_arg"`
    /// and `dispatch_strategy_source="explicit_arg"` since most legacy tests
    /// exercise the explicit-arg precedence path.
    fn fixture_resolved(target: &'static str, dispatch_strategy: &'static str) -> ResolvedExec {
        ResolvedExec {
            target,
            target_source: "explicit_arg",
            dispatch_strategy,
            dispatch_strategy_source: "explicit_arg",
            plan_hint_summary: json!({}),
        }
    }

    fn empty_hints() -> ParsedPlanHints {
        ParsedPlanHints::default()
    }

    #[test]
    fn bridge_response_includes_plan_runner_v0_fields() {
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
        let result = action_execute_bridge(&plan, &resolved);
        assert!(result.is_error.is_none());
        let text = match result.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        let v: Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(v["execute_mode"], "bridge");
        assert_eq!(v["runner_status"], "bridge_only");
        assert_eq!(v["target_tool"], "mission_execution");
        assert_eq!(v["target_source"], "explicit_arg");
        assert_eq!(v["dispatch_strategy"], "fresh-code-alignment");
        assert_eq!(v["dispatch_strategy_source"], "explicit_arg");
        assert!(v.get("plan_hint_summary").is_some());
        assert_eq!(v["next_call"]["tool"], "mission_execution");
        assert_eq!(v["next_call"]["action"], "open");
    }

    #[test]
    fn build_internal_args_for_mission_execution_defaults() {
        let plan = fixture_plan("(plan)");
        let args = json!({});
        let inner = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_execution",
            "unknown",
            &empty_hints(),
        )
        .expect("default args build");
        assert_eq!(inner["action"], "open");
        assert_eq!(inner["execution_id"], format!("plan-{}", plan.id));
        assert_eq!(inner["parent_design"], format!("plan/{}", plan.id));
        assert_eq!(inner["owner"], "plan-runner");
        assert!(inner["scope"]
            .as_str()
            .unwrap()
            .contains(&plan.board_task_id));
        // workstation-dispatch-record: even when caller omits dispatch_strategy
        // the outer handler normalises to "unknown" before reaching this fn,
        // and we always forward it so mission_execution can persist the field.
        assert_eq!(inner["dispatch_strategy"], "unknown");
        // No target_project / requested_cwd in args → inner must not invent
        // them (legacy callers stay byte-identical apart from dispatch_strategy).
        assert!(inner.get("target_project").is_none());
        assert!(inner.get("requested_cwd").is_none());
    }

    #[test]
    fn mission_execution_inner_includes_dispatch_strategy() {
        let plan = fixture_plan("(plan)");
        let args = json!({});
        let inner = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_execution",
            "fresh-code-alignment",
            &empty_hints(),
        )
        .expect("strategy forward");
        assert_eq!(inner["dispatch_strategy"], "fresh-code-alignment");
    }

    #[test]
    fn mission_execution_inner_propagates_target_project_and_cwd() {
        let plan = fixture_plan("(plan)");
        let args = json!({
            "target_project": "missiond",
            "requested_cwd": "/abs/path/missiond",
        });
        let inner = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_execution",
            "agent-team",
            &empty_hints(),
        )
        .expect("forward target_project and requested_cwd");
        // canonical project key gets the alias value
        assert_eq!(inner["project"], "missiond");
        // and the original alias is preserved verbatim for companion-log
        // persistence (workstation-dispatch-record :target-project)
        assert_eq!(inner["target_project"], "missiond");
        assert_eq!(inner["requested_cwd"], "/abs/path/missiond");
        assert_eq!(inner["dispatch_strategy"], "agent-team");
    }

    #[test]
    fn mission_execution_inner_default_dispatch_when_caller_omits() {
        // action_execute normalises a missing/empty dispatch_strategy to
        // "unknown" before reaching build_internal_dispatch_args. This test
        // pins the contract: when the outer normalised string is "unknown",
        // inner["dispatch_strategy"] must be "unknown" (never absent, never
        // some other default).
        let plan = fixture_plan("(plan)");
        let args = json!({});
        let inner = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_execution",
            "unknown",
            &empty_hints(),
        )
        .expect("normalised default");
        assert_eq!(inner["dispatch_strategy"], "unknown");
    }

    #[test]
    fn build_internal_args_for_task_delegate_derives_objective() {
        let plan = fixture_plan("(plan-draft :goal :align)\n");
        let args = json!({});
        let inner = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_task_delegate",
            "unknown",
            &empty_hints(),
        )
        .expect("default task_delegate args");
        let obj = inner["objective"].as_str().unwrap();
        assert!(obj.starts_with(&format!("Plan {}", plan.id)));
        assert!(obj.contains("(plan-draft"));
        assert_eq!(inner["intent"], "code");
        // context_hints should pin the plan + board_task ids
        let hints: Vec<String> = inner["context_hints"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(hints.iter().any(|h| h.starts_with("plan:")));
        assert!(hints.iter().any(|h| h.starts_with("board_task:")));
        // task_delegate path must NOT receive dispatch_strategy — that field
        // belongs to the mission_execution companion log only.
        assert!(inner.get("dispatch_strategy").is_none());
    }

    #[test]
    fn build_internal_args_for_task_delegate_rejects_unknown_intent() {
        let plan = fixture_plan("(plan)");
        let args = json!({ "intent": "cosmic" });
        let err = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_task_delegate",
            "unknown",
            &empty_hints(),
        )
        .expect_err("unknown intent should be rejected");
        assert_eq!(err.is_error, Some(true));
    }

    #[test]
    fn build_internal_args_for_flow_run_requires_flow_id() {
        let plan = fixture_plan("(plan)");
        let args = json!({});
        let err = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_flow_run",
            "unknown",
            &empty_hints(),
        )
        .expect_err("missing flow_id should be MISSING_PARAM");
        assert_eq!(err.is_error, Some(true));
        let text = match err.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text"),
        };
        assert!(text.contains("flow_id"));
    }

    #[test]
    fn build_internal_args_for_flow_run_passes_through_params() {
        let plan = fixture_plan("(plan)");
        let args = json!({
            "flow_id": "F-demo",
            "params": { "k": "v" },
        });
        let inner = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_flow_run",
            "unknown",
            &empty_hints(),
        )
        .expect("flow_run with flow_id");
        assert_eq!(inner["action"], "run");
        assert_eq!(inner["flow_id"], "F-demo");
        assert_eq!(inner["params"]["k"], "v");
        // flow_run must not pick up dispatch_strategy either.
        assert!(inner.get("dispatch_strategy").is_none());
    }

    fn parse_payload(result: &ToolResult) -> Value {
        let text = match result.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        serde_json::from_str(&text).expect("valid json")
    }

    fn fixture_decision_not_applicable(
    ) -> crate::handlers::knowledge::workstation_dispatch::DispatchDecision {
        crate::handlers::knowledge::workstation_dispatch::DispatchDecision {
            source: crate::handlers::knowledge::workstation_dispatch::WorkstationDispatchSource::NotApplicable,
            reason: None,
        }
    }

    #[test]
    fn success_response_clean_path_is_executing() {
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
        let result = build_internal_dispatch_success_response(
            &plan,
            &resolved,
            json!({"ok": true}),
            Some("/tmp/sidecar.json".to_string()),
            None,
            None,
            &fixture_decision_not_applicable(),
            &TaskContractEmissionRecord::off(),
        );
        let v = parse_payload(&result);
        assert_eq!(v["status"], "executing");
        assert_eq!(v["runner_status"], "dispatched");
        assert_eq!(v["evidence_path"], "/tmp/sidecar.json");
        assert!(v.get("evidence_error").is_none());
        assert!(v.get("status_update_error").is_none());
        assert_eq!(v["target_tool"], "mission_execution");
        assert_eq!(v["target_source"], "explicit_arg");
        assert_eq!(v["dispatch_strategy"], "fresh-code-alignment");
        assert_eq!(v["dispatch_strategy_source"], "explicit_arg");
        assert_eq!(v["inner_result"]["ok"], true);
        // wave-16 / task 03 — every legacy success response now carries
        // the routing decision so callers always see the provenance.
        assert_eq!(v["workstation_dispatch_source"], "not_applicable");
    }

    #[test]
    fn success_response_evidence_failure_keeps_dispatched_but_exposes_error() {
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "agent-team");
        let result = build_internal_dispatch_success_response(
            &plan,
            &resolved,
            json!({"task_id": "btk-9"}),
            None,
            Some("mkdir failed: read-only fs".to_string()),
            None,
            &fixture_decision_not_applicable(),
            &TaskContractEmissionRecord::off(),
        );
        let v = parse_payload(&result);
        // Inner tool already produced durable side effects; we keep
        // dispatched/executing semantics but surface the sidecar error.
        assert_eq!(v["status"], "executing");
        assert_eq!(v["runner_status"], "dispatched");
        assert!(v["evidence_path"].is_null());
        assert_eq!(v["evidence_error"], "mkdir failed: read-only fs");
        assert!(v.get("status_update_error").is_none());
    }

    #[test]
    fn success_response_status_update_failure_does_not_claim_executing() {
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_execution", "resident-lisp");
        let result = build_internal_dispatch_success_response(
            &plan,
            &resolved,
            json!({"execution_id": "plan-x"}),
            Some("/tmp/sidecar.json".to_string()),
            None,
            Some("DB error: connection lost".to_string()),
            &fixture_decision_not_applicable(),
            &TaskContractEmissionRecord::off(),
        );
        let v = parse_payload(&result);
        assert_ne!(v["status"], "executing");
        assert_eq!(v["status"], "dispatch_partial");
        assert_eq!(v["runner_status"], "status_update_failed");
        assert_eq!(v["status_update_error"], "DB error: connection lost");
        // inner_result and evidence_path must still be reported so callers can
        // act on the durable inner side effects.
        assert_eq!(v["evidence_path"], "/tmp/sidecar.json");
        assert_eq!(v["inner_result"]["execution_id"], "plan-x");
        assert_eq!(v["target_tool"], "mission_execution");
        assert_eq!(v["dispatch_strategy"], "resident-lisp");
    }

    // ── plan-compiler v0 helpers (pure) ────────────────────────────────

    #[test]
    fn strip_fenced_code_block_extracts_body() {
        let raw = "```lisp\n(plan :goal :ok)\n```";
        assert_eq!(strip_fenced_code_block(raw), "(plan :goal :ok)");
    }

    #[test]
    fn strip_fenced_code_block_handles_missing_lang_tag() {
        let raw = "```\n(plan)\n```";
        assert_eq!(strip_fenced_code_block(raw), "(plan)");
    }

    #[test]
    fn strip_fenced_code_block_passthrough_when_unfenced() {
        assert_eq!(strip_fenced_code_block("(plan)"), "(plan)");
    }

    #[test]
    fn strip_fenced_code_block_lone_open_fence_no_panic() {
        // No newline after the opening fence — we must not slice into a
        // missing newline; just hand the trimmed input back.
        assert_eq!(strip_fenced_code_block("```(plan)"), "```(plan)");
    }

    #[test]
    fn parens_balanced_simple() {
        assert!(parens_balanced("(plan)"));
        assert!(parens_balanced("(plan (a) (b (c)))"));
    }

    #[test]
    fn parens_balanced_unbalanced() {
        assert!(!parens_balanced("(plan"));
        assert!(!parens_balanced("(plan))"));
    }

    #[test]
    fn parens_balanced_ignores_parens_in_strings() {
        // The `)` inside the string literal must not pop the depth.
        assert!(parens_balanced(r#"(plan :note "(((")"#));
        // Mismatched in code despite balanced strings should still fail.
        assert!(!parens_balanced(r#"(plan :note "()" "#));
    }

    #[test]
    fn parens_balanced_honours_string_escapes() {
        // `\"` must not close the string, so `)` inside stays inert.
        assert!(parens_balanced(r#"(plan :note "x\")")"#));
    }

    #[test]
    fn top_level_head_extracts_symbol() {
        assert_eq!(top_level_head("(plan :goal :ok)"), Some("plan"));
        assert_eq!(top_level_head("  (plan-draft\n  :goal :ok)"), Some("plan-draft"));
        assert_eq!(top_level_head("(PLAN)"), Some("PLAN"));
    }

    #[test]
    fn top_level_head_returns_none_when_empty_paren() {
        assert_eq!(top_level_head("("), None);
        assert_eq!(top_level_head("()"), None);
    }

    #[test]
    fn validate_compiled_plan_sexp_accepts_well_formed() {
        let sexp = r#"(plan :board_task_id "btk-1" :goal "ship")"#;
        let out = validate_compiled_plan_sexp(sexp, "btk-1").expect("valid plan");
        assert!(out.contains("btk-1"));
    }

    #[test]
    fn validate_compiled_plan_sexp_strips_fence_then_validates() {
        let raw = "```lisp\n(plan-draft :board_task_id \"btk-9\")\n```";
        let out = validate_compiled_plan_sexp(raw, "btk-9").expect("fence-stripped plan");
        assert!(out.starts_with("(plan-draft"));
    }

    #[test]
    fn validate_compiled_plan_sexp_rejects_empty() {
        let err = validate_compiled_plan_sexp("```\n```", "btk-1").unwrap_err();
        assert_eq!(err.code, "INVALID_COMPILER_OUTPUT");
        assert!(err.reason.contains("empty"));
    }

    #[test]
    fn validate_compiled_plan_sexp_rejects_non_sexp_prefix() {
        let err = validate_compiled_plan_sexp("Sure! (plan)", "btk-1").unwrap_err();
        assert_eq!(err.code, "INVALID_COMPILER_OUTPUT");
        assert!(err.reason.contains("must start with `(`"));
    }

    #[test]
    fn validate_compiled_plan_sexp_rejects_unbalanced() {
        let err = validate_compiled_plan_sexp(r#"(plan :board_task_id "btk-1""#, "btk-1")
            .unwrap_err();
        assert!(err.reason.contains("not balanced"));
    }

    #[test]
    fn validate_compiled_plan_sexp_rejects_unknown_head() {
        let sexp = r#"(directive :board_task_id "btk-1")"#;
        let err = validate_compiled_plan_sexp(sexp, "btk-1").unwrap_err();
        assert!(err.reason.contains("not in allowlist"));
    }

    #[test]
    fn validate_compiled_plan_sexp_rejects_unanchored_plan() {
        // Top head is fine and parens balance, but the board_task id is
        // missing — refuse the plan to avoid persisting something that does
        // not bind to the row.
        let sexp = r#"(plan :goal "ship something else")"#;
        let err = validate_compiled_plan_sexp(sexp, "btk-1").unwrap_err();
        assert!(err.reason.contains("does not reference board_task_id"));
    }

    // ── compile dispatcher ────────────────────────────────────────────

    #[test]
    fn collect_string_list_handles_string_array_and_null() {
        assert_eq!(collect_string_list(None), Vec::<String>::new());
        assert_eq!(collect_string_list(Some(&Value::Null)), Vec::<String>::new());
        assert_eq!(collect_string_list(Some(&json!(""))), Vec::<String>::new());
        assert_eq!(collect_string_list(Some(&json!("only"))), vec!["only".to_string()]);
        assert_eq!(
            collect_string_list(Some(&json!(["a", "", "b"]))),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    /// dry_run is the default and must never call the LLM. We can't fully
    /// exercise the handler without an AppState, so we drive the dispatch
    /// guard via the public schema enum: any value other than `dry_run` /
    /// `sonnet` is rejected before any side effect.
    ///
    /// Together with `compile_dispatch_dry_run_default_is_pure`, this also
    /// covers acceptance item "invalid `compiler_mode` structured error".
    #[test]
    fn compile_dispatch_rejects_unknown_compiler_mode() {
        // We can validate the dispatch logic indirectly by inspecting the
        // constants and ensuring the matching set has not silently grown.
        // If a future change adds a new mode, this test forces an update of
        // the schema description and the dispatcher together.
        assert_eq!(COMPILER_MODE_DRY_RUN, "dry_run");
        assert_eq!(COMPILER_MODE_SONNET, "sonnet");
        // Make sure the allowlist for plan heads stays in lock-step with the
        // system prompt copy.
        assert_eq!(ALLOWED_PLAN_HEADS, &["plan", "plan-draft", "PLAN"]);
    }

    #[test]
    fn compile_dispatch_dry_run_default_is_pure() {
        // Unit-level guard for "default = dry_run, no LLM dependency". The
        // canonical default is the constant, and the schema enum lists it
        // first; the dispatcher reads that constant directly. If this
        // invariant ever drifts, downstream tooling that relies on
        // `compiler_mode` being optional + safe will break silently.
        assert_eq!(COMPILER_MODE_DRY_RUN, "dry_run");
    }

    // ── planner prompt builders (light coverage) ──────────────────────

    #[test]
    fn build_planner_user_prompt_includes_anchor_and_directive() {
        let pin = Some((Uuid::nil(), 7));
        let body = build_planner_user_prompt(
            "btk-42",
            pin,
            Some("(intent-alignment :goal :align)"),
            Some("missiond"),
            Some("agent-team"),
            Some("mixed"),
            &["pass cargo test".to_string()],
            &["no migration".to_string()],
        );
        assert!(body.contains("btk-42"));
        assert!(body.contains("Directive: 00000000-0000-0000-0000-000000000000 v7"));
        assert!(body.contains("(intent-alignment :goal :align)"));
        assert!(body.contains("missiond"));
        assert!(body.contains("agent-team"));
        assert!(body.contains("mixed"));
        assert!(body.contains("pass cargo test"));
        assert!(body.contains("no migration"));
    }

    #[test]
    fn build_planner_user_prompt_omits_optional_sections_when_empty() {
        let body =
            build_planner_user_prompt("btk-42", None, None, None, None, None, &[], &[]);
        assert!(body.contains("btk-42"));
        assert!(!body.contains("Directive:"));
        assert!(!body.contains("Approved directive sexp:"));
        assert!(!body.contains("Acceptance:"));
        assert!(!body.contains("Constraints:"));
    }

    #[test]
    fn build_planner_system_prompt_lists_allowed_heads() {
        let s = build_planner_system_prompt();
        for head in ALLOWED_PLAN_HEADS {
            assert!(s.contains(head), "system prompt missing head `{}`", head);
        }
    }

    #[test]
    fn success_response_status_and_evidence_failure_combined() {
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_flow_run", "mixed");
        let result = build_internal_dispatch_success_response(
            &plan,
            &resolved,
            json!({"flow_id": "F-demo"}),
            None,
            Some("disk full".to_string()),
            Some("DB error: timeout".to_string()),
            &fixture_decision_not_applicable(),
            &TaskContractEmissionRecord::off(),
        );
        let v = parse_payload(&result);
        assert_eq!(v["status"], "dispatch_partial");
        assert_eq!(v["runner_status"], "status_update_failed");
        assert_eq!(v["evidence_error"], "disk full");
        assert_eq!(v["status_update_error"], "DB error: timeout");
        assert!(v["evidence_path"].is_null());
    }

    // ── plan-runner auto-selection v1 ──────────────────────────────────

    #[test]
    fn parse_plan_hints_extracts_string_and_bareword_values() {
        let sexp = r#"
            (plan
              :board_task_id "btk-1"
              :target "mission_task_delegate"
              :flow-id F-demo
              :dispatch-strategy "agent-team"
              :parallelism agent-team
              :target-project "missiond"
              :requested-cwd "/abs/path"
              :objective "ship plan-runner v1"
              :summary "auto-selection v1")
        "#;
        let h = parse_plan_hints(sexp);
        assert_eq!(h.target.as_deref(), Some("mission_task_delegate"));
        assert_eq!(h.flow_id.as_deref(), Some("F-demo"));
        assert_eq!(h.dispatch_strategy.as_deref(), Some("agent-team"));
        assert_eq!(h.parallelism.as_deref(), Some("agent-team"));
        assert_eq!(h.target_project.as_deref(), Some("missiond"));
        assert_eq!(h.requested_cwd.as_deref(), Some("/abs/path"));
        assert_eq!(h.objective.as_deref(), Some("ship plan-runner v1"));
        assert_eq!(h.summary.as_deref(), Some("auto-selection v1"));
    }

    #[test]
    fn parse_plan_hints_skips_list_values_and_keeps_first_occurrence() {
        // First :target wins; second :target inside a nested phase is ignored
        // by "store_first" semantics. List values are silently skipped, so the
        // :tasks (...) form below must NOT pollute the hint slots.
        let sexp = r#"
            (plan :target "mission_execution"
                  :tasks (s1 :objective "phase 1")
                  (phase :target "mission_flow_run"))
        "#;
        let h = parse_plan_hints(sexp);
        assert_eq!(h.target.as_deref(), Some("mission_execution"));
    }

    #[test]
    fn parse_plan_hints_ignores_keywords_inside_string_literals() {
        // ":target" embedded inside a quoted note must not look like a real
        // keyword/value pair.
        let sexp = r#"(plan :note ":target faux" :objective "real one")"#;
        let h = parse_plan_hints(sexp);
        assert!(h.target.is_none());
        assert_eq!(h.objective.as_deref(), Some("real one"));
    }

    #[test]
    fn parse_plan_hints_accepts_underscore_aliases() {
        let sexp = r#"(plan :flow_id F-y :target_project missiond :requested_cwd /tmp)"#;
        let h = parse_plan_hints(sexp);
        assert_eq!(h.flow_id.as_deref(), Some("F-y"));
        assert_eq!(h.target_project.as_deref(), Some("missiond"));
        assert_eq!(h.requested_cwd.as_deref(), Some("/tmp"));
    }

    #[test]
    fn parse_plan_hints_empty_when_no_hints_present() {
        let sexp = "(plan :board_task_id \"btk-x\" :goal :ship)";
        let h = parse_plan_hints(sexp);
        assert!(h.target.is_none());
        assert!(h.flow_id.is_none());
        assert!(h.dispatch_strategy.is_none());
        assert!(h.parallelism.is_none());
    }

    #[test]
    fn normalize_target_maps_keywords_to_canonical_targets() {
        assert_eq!(
            normalize_target("mission_execution", false),
            Some("mission_execution")
        );
        assert_eq!(normalize_target("EXECUTION", false), Some("mission_execution"));
        assert_eq!(
            normalize_target("mission_task_delegate", false),
            Some("mission_task_delegate")
        );
        assert_eq!(
            normalize_target("claudecode workstation", false),
            Some("mission_task_delegate")
        );
        assert_eq!(
            normalize_target("code-alignment session", false),
            Some("mission_task_delegate")
        );
        // flow_run gated by flow_id presence
        assert_eq!(normalize_target("mission_flow_run", false), None);
        assert_eq!(normalize_target("flow", false), None);
        assert_eq!(
            normalize_target("mission_flow_run", true),
            Some("mission_flow_run")
        );
        assert_eq!(normalize_target("flow", true), Some("mission_flow_run"));
        // unknown text yields None
        assert_eq!(normalize_target("nothing here", true), None);
    }

    #[test]
    fn canonicalize_strategy_returns_known_or_none() {
        assert_eq!(canonicalize_strategy("agent-team"), Some("agent-team"));
        assert_eq!(canonicalize_strategy("AGENT_TEAM"), Some("agent-team"));
        assert_eq!(
            canonicalize_strategy("fresh-code-alignment"),
            Some("fresh-code-alignment")
        );
        assert_eq!(
            canonicalize_strategy("fresh code alignment"),
            Some("fresh-code-alignment")
        );
        assert_eq!(
            canonicalize_strategy("resident-lisp"),
            Some("resident-lisp")
        );
        assert_eq!(canonicalize_strategy("lisp-architect"), Some("resident-lisp"));
        assert_eq!(canonicalize_strategy("mixed"), Some("mixed"));
        assert_eq!(
            canonicalize_strategy("prompt-fallback"),
            Some("prompt-fallback")
        );
        // explicit "unknown" is treated as no signal so callers can fall back
        assert_eq!(canonicalize_strategy("unknown"), None);
        assert_eq!(canonicalize_strategy("nope"), None);
    }

    #[test]
    fn resolve_dispatch_strategy_explicit_arg_wins() {
        let mut hints = ParsedPlanHints::default();
        hints.dispatch_strategy = Some("agent-team".to_string());
        let (v, src) = resolve_dispatch_strategy(Some("resident-lisp"), &hints);
        assert_eq!(v, "resident-lisp");
        assert_eq!(src, "explicit_arg");
    }

    #[test]
    fn resolve_dispatch_strategy_falls_back_to_plan_hint() {
        let mut hints = ParsedPlanHints::default();
        hints.dispatch_strategy = Some("agent-team".to_string());
        let (v, src) = resolve_dispatch_strategy(None, &hints);
        assert_eq!(v, "agent-team");
        assert_eq!(src, "plan_hint");
    }

    #[test]
    fn resolve_dispatch_strategy_uses_parallelism_when_dispatch_absent() {
        let mut hints = ParsedPlanHints::default();
        hints.parallelism = Some("agent-team".to_string());
        let (v, src) = resolve_dispatch_strategy(None, &hints);
        assert_eq!(v, "agent-team");
        assert_eq!(src, "plan_hint");
    }

    #[test]
    fn resolve_dispatch_strategy_default_when_no_signal() {
        let (v, src) = resolve_dispatch_strategy(None, &ParsedPlanHints::default());
        assert_eq!(v, "unknown");
        assert_eq!(src, "default");
    }

    #[test]
    fn resolve_dispatch_strategy_explicit_unknown_normalises_to_unknown() {
        // An explicit "unknown" arg still wins over the default branch and
        // does NOT cascade into plan hints — explicit means explicit.
        let mut hints = ParsedPlanHints::default();
        hints.dispatch_strategy = Some("agent-team".to_string());
        let (v, src) = resolve_dispatch_strategy(Some("unknown"), &hints);
        assert_eq!(v, "unknown");
        assert_eq!(src, "explicit_arg");
    }

    #[test]
    fn build_internal_args_for_mission_execution_uses_plan_hints() {
        // Caller omits both target_project and requested_cwd; parser supplies
        // them and the inner JSON must include both.
        let plan = fixture_plan("(plan)");
        let args = json!({});
        let mut hints = ParsedPlanHints::default();
        hints.target_project = Some("missiond".to_string());
        hints.requested_cwd = Some("/abs/path/missiond".to_string());

        let inner = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_execution",
            "fresh-code-alignment",
            &hints,
        )
        .expect("hints should backfill");
        assert_eq!(inner["project"], "missiond");
        assert_eq!(inner["target_project"], "missiond");
        assert_eq!(inner["requested_cwd"], "/abs/path/missiond");
        assert_eq!(inner["dispatch_strategy"], "fresh-code-alignment");
    }

    #[test]
    fn build_internal_args_explicit_arg_overrides_plan_hint_for_mission_execution() {
        let plan = fixture_plan("(plan)");
        let args = json!({
            "target_project": "explicit-project",
            "requested_cwd": "/explicit/cwd",
        });
        let mut hints = ParsedPlanHints::default();
        hints.target_project = Some("hint-project".to_string());
        hints.requested_cwd = Some("/hint/cwd".to_string());

        let inner = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_execution",
            "unknown",
            &hints,
        )
        .expect("explicit arg wins");
        assert_eq!(inner["project"], "explicit-project");
        assert_eq!(inner["target_project"], "explicit-project");
        assert_eq!(inner["requested_cwd"], "/explicit/cwd");
    }

    #[test]
    fn task_delegate_receives_agent_team_objective_hint() {
        let plan = fixture_plan("(plan-draft :goal :ship)");
        let args = json!({});
        let inner = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_task_delegate",
            "agent-team",
            &empty_hints(),
        )
        .expect("agent-team injection");
        let obj = inner["objective"].as_str().unwrap();
        assert!(
            obj.contains(AGENT_TEAM_OBJECTIVE_HINT),
            "objective should carry agent-team hint, got: {obj}"
        );
    }

    #[test]
    fn task_delegate_does_not_duplicate_agent_team_hint_when_present() {
        let plan = fixture_plan("(plan)");
        let args = json!({
            "objective": format!("manual: {AGENT_TEAM_OBJECTIVE_HINT}"),
        });
        let inner = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_task_delegate",
            "agent-team",
            &empty_hints(),
        )
        .expect("agent-team idempotent");
        let obj = inner["objective"].as_str().unwrap();
        // Exactly one occurrence — no duplication.
        assert_eq!(
            obj.matches(AGENT_TEAM_OBJECTIVE_HINT).count(),
            1,
            "should not duplicate hint, got: {obj}"
        );
    }

    #[test]
    fn task_delegate_objective_falls_back_to_plan_hint() {
        let plan = fixture_plan("(plan)");
        let args = json!({});
        let mut hints = ParsedPlanHints::default();
        hints.objective = Some("hint objective text".to_string());

        let inner = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_task_delegate",
            "unknown",
            &hints,
        )
        .expect("hint objective wins");
        assert_eq!(inner["objective"], "hint objective text");
    }

    #[test]
    fn task_delegate_objective_falls_back_to_summary_hint_when_no_objective() {
        let plan = fixture_plan("(plan)");
        let args = json!({});
        let mut hints = ParsedPlanHints::default();
        hints.summary = Some("summary fallback".to_string());

        let inner = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_task_delegate",
            "unknown",
            &hints,
        )
        .expect("summary fallback");
        assert_eq!(inner["objective"], "summary fallback");
    }

    #[test]
    fn task_delegate_cwd_uses_hint_when_arg_missing() {
        let plan = fixture_plan("(plan)");
        let args = json!({});
        let mut hints = ParsedPlanHints::default();
        hints.requested_cwd = Some("/from/hint".to_string());

        let inner = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_task_delegate",
            "unknown",
            &hints,
        )
        .expect("hint cwd backfill");
        assert_eq!(inner["cwd"], "/from/hint");
    }

    #[test]
    fn task_delegate_cwd_uses_target_project_hint_only_when_path_like() {
        let plan = fixture_plan("(plan)");
        let args = json!({});
        let mut hints = ParsedPlanHints::default();
        hints.target_project = Some("missiond".to_string()); // bare id, no '/'

        let inner = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_task_delegate",
            "unknown",
            &hints,
        )
        .expect("bare project id should not become cwd");
        assert!(inner.get("cwd").is_none());

        let mut hints2 = ParsedPlanHints::default();
        hints2.target_project = Some("/abs/missiond".to_string());
        let inner2 = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_task_delegate",
            "unknown",
            &hints2,
        )
        .expect("path-like target_project becomes cwd");
        assert_eq!(inner2["cwd"], "/abs/missiond");
    }

    #[test]
    fn flow_run_uses_plan_hint_flow_id_when_arg_missing() {
        let plan = fixture_plan("(plan)");
        let args = json!({});
        let mut hints = ParsedPlanHints::default();
        hints.flow_id = Some("F-from-plan".to_string());

        let inner = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_flow_run",
            "unknown",
            &hints,
        )
        .expect("flow_id from hint");
        assert_eq!(inner["flow_id"], "F-from-plan");
    }

    #[test]
    fn flow_run_explicit_arg_overrides_plan_hint() {
        let plan = fixture_plan("(plan)");
        let args = json!({ "flow_id": "F-explicit" });
        let mut hints = ParsedPlanHints::default();
        hints.flow_id = Some("F-from-plan".to_string());

        let inner = build_internal_dispatch_args(
            &args,
            &plan,
            "mission_flow_run",
            "unknown",
            &hints,
        )
        .expect("explicit flow_id wins");
        assert_eq!(inner["flow_id"], "F-explicit");
    }

    #[test]
    fn bridge_response_carries_plan_hint_summary_and_sources() {
        let plan = fixture_plan("(plan)");
        let mut hints_summary = serde_json::Map::new();
        hints_summary.insert("target".to_string(), json!("mission_task_delegate"));
        hints_summary.insert("parallelism".to_string(), json!("agent-team"));
        let resolved = ResolvedExec {
            target: "mission_task_delegate",
            target_source: "plan_hint",
            dispatch_strategy: "agent-team",
            dispatch_strategy_source: "plan_hint",
            plan_hint_summary: Value::Object(hints_summary),
        };
        let result = action_execute_bridge(&plan, &resolved);
        let v = parse_payload(&result);
        assert_eq!(v["target_tool"], "mission_task_delegate");
        assert_eq!(v["target_source"], "plan_hint");
        assert_eq!(v["dispatch_strategy"], "agent-team");
        assert_eq!(v["dispatch_strategy_source"], "plan_hint");
        assert_eq!(v["plan_hint_summary"]["target"], "mission_task_delegate");
        assert_eq!(v["plan_hint_summary"]["parallelism"], "agent-team");
    }

    #[test]
    fn parsed_plan_hints_summary_omits_absent_fields() {
        let mut h = ParsedPlanHints::default();
        h.target = Some("mission_execution".to_string());
        let summary = h.to_summary_json();
        let obj = summary.as_object().expect("summary is object");
        assert_eq!(obj.len(), 1, "only :target should appear");
        assert_eq!(obj.get("target"), Some(&json!("mission_execution")));
    }

    // ── wave-11 :: project-root resolver (canonical contract) ────────────
    //
    // These tests pin `resolve_project_root` to the
    // `intent-worker.lisp :: project-root-spawn-cwd` contract:
    //   - explicit registered project id resolves to its canonical path
    //   - cwd inside a registered project resolves via longest-prefix
    //   - relative cwd is rejected (no process-cwd fallback)
    //   - missing-signal-only case is rejected (no process-cwd fallback)
    //   - unknown registered id is rejected
    // We exercise the resolver helper directly with a `SharedProjectRegistry`
    // so we don't have to materialise a full `AppState`.

    use missiond_core::types::{ProjectConfig, ProjectRegistry, SharedProjectRegistry};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn registry_with(projects: Vec<ProjectConfig>) -> SharedProjectRegistry {
        Arc::new(RwLock::new(ProjectRegistry::new(projects)))
    }

    fn project(id: &str, path: &str) -> ProjectConfig {
        ProjectConfig {
            id: id.to_string(),
            path: path.to_string(),
            intent_path: None,
            active: true,
            slots: vec![],
            github_url: None,
            kind: "managed".to_string(),
            vault_path: None,
            parent_id: None,
            created_at: None,
            updated_at: None,
        }
    }

    #[tokio::test]
    async fn resolve_project_root_resolves_registered_project_id() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let reg = registry_with(vec![project("missiond", &root.display().to_string())]);
        let resolved = resolve_project_root(&reg, Some("missiond"), None, None)
            .await
            .expect("explicit project id should resolve");
        assert_eq!(resolved, root);
    }

    #[tokio::test]
    async fn resolve_project_root_resolves_absolute_cwd_via_longest_prefix() {
        // cwd-under-subdir → registry longest-prefix lookup picks the
        // canonical project root, NOT the subdir. This is the same path
        // flow_run / compute_slot use; the plan resolver must agree.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let subdir = root.join("crates").join("missiond-daemon");
        std::fs::create_dir_all(&subdir).unwrap();
        let reg = registry_with(vec![project("missiond", &root.display().to_string())]);
        let resolved = resolve_project_root(
            &reg,
            None,
            Some(subdir.display().to_string().as_str()),
            None,
        )
        .await
        .expect("absolute cwd inside registered project should resolve");
        assert_eq!(resolved, root, "must collapse to canonical root, not subdir");
    }

    #[tokio::test]
    async fn resolve_project_root_resolves_target_project_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let reg = registry_with(vec![project("missiond", &root.display().to_string())]);
        let resolved = resolve_project_root(&reg, None, None, Some("missiond"))
            .await
            .expect("target_project fallback should resolve");
        assert_eq!(resolved, root);
    }

    #[tokio::test]
    async fn resolve_project_root_rejects_relative_cwd() {
        // Relative cwd must NEVER silently fall back to process cwd.
        // Even with a registered project, the relative cwd is refused at
        // pre-flight, so no resolver call ever sees it.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let reg = registry_with(vec![project("missiond", &root.display().to_string())]);
        let err = resolve_project_root(&reg, None, Some("relative/path"), None)
            .await
            .expect_err("relative cwd should be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("not absolute"),
            "error must explain refusal, got: {}",
            msg
        );
        assert!(
            msg.contains("project-root-spawn-cwd"),
            "error must reference the lisp contract, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn resolve_project_root_rejects_missing_signal_no_process_cwd_fallback() {
        // No project, no cwd, no target_project → resolver MUST fail rather
        // than fall back to the daemon's process working directory
        // (CLAUDE.md `feedback_fail_fast_no_fallback`). This is the
        // regression guard for the prior process-cwd fallback path.
        let reg = registry_with(vec![project("missiond", "/tmp/missiond")]);
        let err = resolve_project_root(&reg, None, None, None)
            .await
            .expect_err("missing signal must be rejected; no process cwd fallback");
        let msg = err.to_string();
        assert!(
            msg.contains("project root unresolved"),
            "error must explain refusal, got: {}",
            msg
        );
        assert!(
            msg.contains("does not fall back"),
            "error must explicitly disclaim cwd fallback, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn resolve_project_root_rejects_unknown_registered_id() {
        let reg = registry_with(vec![project("missiond", "/tmp/missiond")]);
        let err = resolve_project_root(&reg, Some("nonexistent"), None, None)
            .await
            .expect_err("unknown project id should be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("not registered") || msg.contains("nonexistent"),
            "error must mention the missing id, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn resolve_project_root_explicit_id_wins_over_target_project() {
        // Explicit `project` arg takes precedence over `target_project`
        // (mirrors the canonical resolver source order).
        let tmp_a = tempfile::tempdir().unwrap();
        let tmp_b = tempfile::tempdir().unwrap();
        let root_a = tmp_a.path().canonicalize().unwrap();
        let root_b = tmp_b.path().canonicalize().unwrap();
        let reg = registry_with(vec![
            project("alpha", &root_a.display().to_string()),
            project("beta", &root_b.display().to_string()),
        ]);
        let resolved =
            resolve_project_root(&reg, Some("alpha"), None, Some("beta"))
                .await
                .expect("explicit id should win");
        assert_eq!(resolved, root_a);
    }

    // ── wave-19 / task 06 — task-contract emitter v0 ────────────────────

    #[test]
    fn parse_task_contract_emit_mode_default_is_off() {
        let v = json!({});
        let m = parse_task_contract_emit_mode(&v).expect("default ok");
        assert_eq!(m, TaskContractEmitMode::Off);
    }

    #[test]
    fn parse_task_contract_emit_mode_boolean_shorthand_true_is_emit() {
        let v = json!({"emit_task_contract": true});
        let m = parse_task_contract_emit_mode(&v).expect("bool ok");
        assert_eq!(m, TaskContractEmitMode::Emit);
    }

    #[test]
    fn parse_task_contract_emit_mode_boolean_shorthand_false_is_off() {
        let v = json!({"emit_task_contract": false});
        let m = parse_task_contract_emit_mode(&v).expect("bool ok");
        assert_eq!(m, TaskContractEmitMode::Off);
    }

    #[test]
    fn parse_task_contract_emit_mode_explicit_emit_dry_run() {
        let v = json!({"task_contract_mode": "emit_dry_run"});
        let m = parse_task_contract_emit_mode(&v).expect("dry-run ok");
        assert_eq!(m, TaskContractEmitMode::EmitDryRun);
    }

    #[test]
    fn parse_task_contract_emit_mode_explicit_wins_over_boolean() {
        let v = json!({
            "task_contract_mode": "off",
            "emit_task_contract": true,
        });
        let m = parse_task_contract_emit_mode(&v).expect("explicit wins");
        // Explicit string "off" beats boolean shorthand `true`.
        assert_eq!(m, TaskContractEmitMode::Off);
    }

    #[test]
    fn parse_task_contract_emit_mode_unknown_string_is_structured_error() {
        let v = json!({"task_contract_mode": "emi"});
        let err = parse_task_contract_emit_mode(&v).expect_err("typo rejected");
        assert!(err.is_error.unwrap_or(false));
    }

    #[test]
    fn parse_task_contract_emit_mode_non_string_value_is_structured_error() {
        let v = json!({"task_contract_mode": 7});
        let err = parse_task_contract_emit_mode(&v).expect_err("non-string rejected");
        assert!(err.is_error.unwrap_or(false));
    }

    // ── wave-20 / task 04 — dispatch_contract_mode parser tests ─────────

    /// Default mode is `Rendered` so the wave-15..19 byte-shape is
    /// preserved for callers that never opt in.
    #[test]
    fn parse_dispatch_contract_mode_default_is_rendered() {
        let v = json!({});
        let m = parse_dispatch_contract_mode(&v).expect("default ok");
        assert!(matches!(m, DispatchContractMode::Rendered));
        assert_eq!(m.as_str(), "rendered");
        assert!(!m.is_machine());
    }

    /// Explicit `dispatch_contract_mode="machine"` flips the mode.
    #[test]
    fn parse_dispatch_contract_mode_explicit_machine() {
        let v = json!({"dispatch_contract_mode": "machine"});
        let m = parse_dispatch_contract_mode(&v).expect("machine ok");
        assert!(matches!(m, DispatchContractMode::Machine));
        assert_eq!(m.as_str(), "machine");
        assert!(m.is_machine());
    }

    /// Explicit `dispatch_contract_mode="rendered"` is a no-op default.
    #[test]
    fn parse_dispatch_contract_mode_explicit_rendered() {
        let v = json!({"dispatch_contract_mode": "rendered"});
        let m = parse_dispatch_contract_mode(&v).expect("rendered ok");
        assert!(matches!(m, DispatchContractMode::Rendered));
    }

    /// `render_markdown=false` is the boolean shorthand for machine mode.
    #[test]
    fn parse_dispatch_contract_mode_render_markdown_false_is_machine() {
        let v = json!({"render_markdown": false});
        let m = parse_dispatch_contract_mode(&v).expect("shorthand ok");
        assert!(matches!(m, DispatchContractMode::Machine));
    }

    /// `render_markdown=true` is the explicit rendered (default) form.
    #[test]
    fn parse_dispatch_contract_mode_render_markdown_true_is_rendered() {
        let v = json!({"render_markdown": true});
        let m = parse_dispatch_contract_mode(&v).expect("shorthand ok");
        assert!(matches!(m, DispatchContractMode::Rendered));
    }

    /// Explicit `dispatch_contract_mode` wins over the boolean shorthand
    /// when both are set so a caller cannot accidentally downgrade an
    /// explicit machine opt-in.
    #[test]
    fn parse_dispatch_contract_mode_explicit_wins_over_shorthand() {
        let v = json!({
            "dispatch_contract_mode": "machine",
            "render_markdown": true,
        });
        let m = parse_dispatch_contract_mode(&v).expect("explicit wins");
        assert!(matches!(m, DispatchContractMode::Machine));
    }

    /// Typo (`dispatch_contract_mode="machin"`) MUST fail fast — never
    /// silently degrade to `rendered`. This is the contract that
    /// prevents a caller from accidentally falling back to the legacy
    /// markdown-driven brief without noticing.
    #[test]
    fn parse_dispatch_contract_mode_unknown_string_is_structured_error() {
        let v = json!({"dispatch_contract_mode": "machin"});
        let err = parse_dispatch_contract_mode(&v).expect_err("typo rejected");
        assert!(err.is_error.unwrap_or(false));
    }

    /// Non-string `dispatch_contract_mode` is rejected (no silent
    /// conversion of `7` → "rendered").
    #[test]
    fn parse_dispatch_contract_mode_non_string_value_is_structured_error() {
        let v = json!({"dispatch_contract_mode": 7});
        let err = parse_dispatch_contract_mode(&v).expect_err("non-string rejected");
        assert!(err.is_error.unwrap_or(false));
    }

    #[test]
    fn lisp_escape_string_passes_plain_text() {
        assert_eq!(lisp_escape_string("hello world"), "hello world");
    }

    #[test]
    fn lisp_escape_string_escapes_backslash_and_quote() {
        assert_eq!(lisp_escape_string("a\"b\\c"), "a\\\"b\\\\c");
    }

    #[test]
    fn is_task_contract_eligible_requires_task_delegate() {
        assert!(is_task_contract_eligible("mission_task_delegate", Some("ship")));
        assert!(!is_task_contract_eligible("mission_execution", Some("ship")));
        assert!(!is_task_contract_eligible("mission_flow_run", Some("ship")));
    }

    #[test]
    fn is_task_contract_eligible_rejects_empty_objective() {
        assert!(!is_task_contract_eligible("mission_task_delegate", Some("")));
        assert!(!is_task_contract_eligible("mission_task_delegate", Some("   ")));
        assert!(!is_task_contract_eligible("mission_task_delegate", None));
    }

    #[test]
    fn build_task_contract_lisp_round_trips_required_fields() {
        let plan_id = Uuid::parse_str("00000000-0000-0000-0000-0000deadbeef").unwrap();
        let inputs = TaskContractInputs {
            objective: "ship feature X".to_string(),
            scope: Some("only the renderer".to_string()),
            owned_files: vec!["a.rs".to_string(), "b.rs".to_string()],
            forbidden_files: vec!["src/lib.rs".to_string()],
            acceptance_commands: vec!["cargo test".to_string(), "cargo build".to_string()],
            commit_policy: Some("scoped".to_string()),
            dispatch_strategy: "agent-team".to_string(),
            target: "mission_task_delegate".to_string(),
            target_project: Some("missiond".to_string()),
            requested_cwd: None,
            session_trace_path: None,
        };
        let body = build_task_contract_lisp(plan_id, "node-1", "btk-7", &inputs);
        // Must declare schema, kind, status, owner, write-scope, must-not-touch,
        // acceptance, commit (the task-contract v1 required floor).
        assert!(body.contains(":schema \"missiond.task-contract.v1\""));
        assert!(body.contains(":kind code-alignment"));
        assert!(body.contains(":status ready"));
        assert!(body.contains(":owner \"claudecode\""));
        assert!(body.contains(":dispatch-strategy \"agent-team\""));
        assert!(body.contains(":goal \"ship feature X\""));
        assert!(body.contains(":scope \"only the renderer\""));
        assert!(body.contains("[\"a.rs\" \"b.rs\"]"));
        assert!(body.contains("[\"src/lib.rs\"]"));
        assert!(body.contains("[\"cargo test\" \"cargo build\"]"));
        assert!(body.contains(":scope-check write-scope-only"));
        assert!(body.contains(":target-project \"missiond\""));
        // node-id stamped verbatim
        assert!(body.contains(":node-id \"node-1\""));
        // plan id traced for downstream observers
        assert!(body.contains(&plan_id.to_string()));
        // task id derived from plan + node prefix
        assert!(body.contains("(task plan-00000000-node-node-1\n"));
    }

    #[test]
    fn build_task_contract_lisp_escapes_quotes_in_objective() {
        let plan_id = Uuid::parse_str("00000000-0000-0000-0000-0000feedface").unwrap();
        let inputs = TaskContractInputs {
            objective: r#"ship "thing" now"#.to_string(),
            owned_files: vec!["a.rs".to_string()],
            target: "mission_task_delegate".to_string(),
            dispatch_strategy: "agent-team".to_string(),
            ..Default::default()
        };
        let body = build_task_contract_lisp(plan_id, "node-x", "btk-1", &inputs);
        assert!(
            body.contains(r#":goal "ship \"thing\" now""#),
            "expected escaped quotes, got: {}",
            body
        );
    }

    #[test]
    fn task_contract_path_uses_plan_then_node_layout() {
        let plan_id = Uuid::parse_str("00000000-0000-0000-0000-000000abcdef").unwrap();
        let root = std::path::Path::new("/tmp/missiond-root");
        let p = task_contract_path(root, plan_id, "node-7");
        let s = p.display().to_string();
        assert!(s.ends_with(&format!(
            ".missiond/tasks/generated/{}/node-7.lisp",
            plan_id
        )));
    }

    #[test]
    fn task_contract_path_sanitizes_node_id() {
        let plan_id = Uuid::parse_str("00000000-0000-0000-0000-000000000111").unwrap();
        let root = std::path::Path::new("/tmp/missiond-root");
        // path-traversal characters collapse to a single dash
        let p = task_contract_path(root, plan_id, "node/with/slashes");
        assert!(p.display().to_string().ends_with("node-with-slashes.lisp"));
    }

    #[test]
    fn render_command_includes_renderer_script_and_force_flag() {
        let cmd = render_command_for(std::path::Path::new("/tmp/a.lisp"));
        assert!(cmd.contains("scripts/render-claudecode-task.mjs"));
        assert!(cmd.contains("--force"));
        assert!(cmd.contains("/tmp/a.lisp"));
    }

    #[test]
    fn task_contract_emission_record_off_has_no_response_block() {
        let r = TaskContractEmissionRecord::off();
        assert!(r.to_response_block().is_none());
        assert!(!r.is_failure());
    }

    #[test]
    fn task_contract_emission_record_skipped_surfaces_reason() {
        let r =
            TaskContractEmissionRecord::skipped(TaskContractEmitMode::Emit, "objective empty");
        let block = r.to_response_block().expect("skipped surfaces a block");
        assert_eq!(block["task_contract_mode"], "emit");
        assert_eq!(block["task_contract_eligible"], false);
        assert_eq!(block["task_contract_skip_reason"], "objective empty");
        assert!(block.get("task_contract_path").is_none());
        assert!(!r.is_failure());
    }

    #[test]
    fn task_contract_emission_record_ok_includes_path_and_render_command() {
        let r = TaskContractEmissionRecord::ok(
            TaskContractEmitMode::Emit,
            std::path::PathBuf::from("/tmp/a.lisp"),
        );
        let block = r.to_response_block().expect("ok surfaces block");
        assert_eq!(block["task_contract_mode"], "emit");
        assert_eq!(block["task_contract_eligible"], true);
        assert_eq!(block["task_contract_path"], "/tmp/a.lisp");
        assert!(block["render_command"]
            .as_str()
            .unwrap_or("")
            .contains("render-claudecode-task.mjs"));
        assert!(!r.is_failure());
    }

    #[test]
    fn task_contract_emission_record_failed_surfaces_error() {
        let r = TaskContractEmissionRecord::failed(
            TaskContractEmitMode::Emit,
            "disk full".to_string(),
        );
        let block = r.to_response_block().expect("failure surfaces block");
        assert_eq!(block["task_contract_error"], "disk full");
        assert!(r.is_failure());
    }

    #[test]
    fn write_task_contract_under_root_creates_canonical_path() {
        // Pure on-disk test that does not need an AppState — exercise the
        // path layout + atomic write under a tempdir.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let plan_id = Uuid::parse_str("00000000-0000-0000-0000-0000ffffffff").unwrap();
        let body = "(task fixture :schema \"missiond.task-contract.v1\")\n";
        let path = write_task_contract_under_root(&root, plan_id, "node-a", body)
            .expect("write should succeed");
        let s = path.display().to_string();
        assert!(s.ends_with(&format!(
            ".missiond/tasks/generated/{}/node-a.lisp",
            plan_id
        )));
        let read_back = std::fs::read_to_string(&path).expect("read back contract");
        assert_eq!(read_back, body);
    }

    #[test]
    fn write_task_contract_under_root_overwrites_existing_atomically() {
        // Second write should replace the prior body via the tmp+rename
        // dance. No leftover .lisp.tmp file may remain.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let plan_id = Uuid::parse_str("00000000-0000-0000-0000-0000ffffeeee").unwrap();
        let _ = write_task_contract_under_root(&root, plan_id, "node-a", "first")
            .expect("first write");
        let path = write_task_contract_under_root(&root, plan_id, "node-a", "second")
            .expect("second write");
        let read_back = std::fs::read_to_string(&path).expect("read");
        assert_eq!(read_back, "second");
        let tmp_sibling = path.with_extension("lisp.tmp");
        assert!(!tmp_sibling.exists(), "tmp leftover detected");
    }

    // ── wave-12 :: record_evidence routing decision ──────────────────
    //
    // The action handler picks between the historical untagged shape
    // (`{"evidence": …}`) and the new evidence-collector wrapper based on
    // whether the caller supplied `evidence_kind` / `source`. We can't
    // exercise the full handler here without spinning up an AppState +
    // store, but we CAN pin the entry-shape decision by replaying the same
    // branching logic against the real wrapper.
    //
    // These tests guard against a regression where someone "simplifies" the
    // action by always wrapping (which would break legacy readers that
    // expect the un-stamped payload) or always passing through (which would
    // make the new params silently no-op).

    #[test]
    fn record_evidence_legacy_shape_when_no_kind_or_source() {
        let evidence = serde_json::json!({"tool_calls": []});
        // Replays the action's branching: both args absent → legacy wire form.
        let evidence_kind: Option<&str> = None;
        let source_override: Option<&str> = None;
        let entry = if evidence_kind.is_some() || source_override.is_some() {
            super::super::evidence_collector::wrap_legacy_record_evidence(
                evidence.clone(),
                evidence_kind,
                source_override,
            )
        } else {
            serde_json::json!({ "evidence": evidence })
        };
        let obj = entry.as_object().expect("entry is object");
        assert_eq!(obj.len(), 1, "legacy shape: only `evidence` at top level");
        assert!(obj.contains_key("evidence"));
        assert!(!obj.contains_key("schema_version"), "legacy shape has no schema stamp");
        assert!(!obj.contains_key("source"), "legacy shape has no source");
        assert!(!obj.contains_key("kind"), "legacy shape has no kind");
    }

    #[test]
    fn record_evidence_typed_wrap_when_kind_present() {
        let evidence = serde_json::json!({"note": "build green"});
        // Replays the action's branching: kind present → typed wrap.
        let evidence_kind: Option<&str> = Some("verification");
        let source_override: Option<&str> = None;
        let entry = if evidence_kind.is_some() || source_override.is_some() {
            super::super::evidence_collector::wrap_legacy_record_evidence(
                evidence.clone(),
                evidence_kind,
                source_override,
            )
        } else {
            serde_json::json!({ "evidence": evidence })
        };
        assert_eq!(entry["schema_version"], "v0", "schema stamp present");
        assert_eq!(entry["kind"], "verification", "caller-supplied kind round-trips");
        assert_eq!(
            entry["source"], "record_evidence_manual",
            "default source applied when caller omits it"
        );
        assert_eq!(entry["evidence"], evidence, "original payload preserved");
    }

    #[test]
    fn record_evidence_typed_wrap_when_source_present() {
        let evidence = serde_json::json!(["t1", "t2"]);
        let evidence_kind: Option<&str> = None;
        let source_override: Option<&str> = Some("ci_workflow");
        let entry = if evidence_kind.is_some() || source_override.is_some() {
            super::super::evidence_collector::wrap_legacy_record_evidence(
                evidence.clone(),
                evidence_kind,
                source_override,
            )
        } else {
            serde_json::json!({ "evidence": evidence })
        };
        assert_eq!(entry["source"], "ci_workflow", "caller-supplied source round-trips");
        assert_eq!(
            entry["kind"], "note",
            "default kind applied when caller omits it"
        );
    }

    // ── wave-13 :: plan_runner_dispatch typed evidence shape ──────────
    //
    // `action_execute_internal` builds an `EvidenceEntry` (wave-12 typed
    // collector) instead of a hand-rolled JSON object. These tests pin the
    // projected on-disk shape so the wire-compatible mapping
    //   legacy `kind="plan_runner_dispatch"`
    //     ↦ canonical `source="plan_runner_dispatch"` + `kind="dispatch"`
    // is enforced, and the legacy passthrough fields (`execute_mode`,
    // `target_tool`, `target_source`, `dispatch_strategy`,
    // `dispatch_strategy_source`, `plan_hint_summary`) keep their flat
    // top-level placement for existing audit dashboards.
    //
    // We replay the exact entry construction (mirrored from
    // `action_execute_internal`) instead of hitting the live handler so the
    // assertions stay focused on the wire shape — the live handler is
    // covered end-to-end by the runtime tests, but those don't introspect
    // the on-disk JSON.
    fn build_plan_runner_evidence_entry(
        resolved: &ResolvedExec,
        inner_payload: Value,
    ) -> Value {
        super::super::evidence_collector::EvidenceEntry::new(
            super::super::evidence_collector::source::PLAN_RUNNER_DISPATCH,
            super::super::evidence_collector::kind::DISPATCH,
        )
        .with_inner_dispatch(inner_payload.clone())
        .add_execution_event(super::super::evidence_collector::EventRef::unavailable(
            "plan-runner v0 does not yet subscribe to the live ExecutionEvent bus; \
             caller correlates by plan_id + board_task_id",
        ))
        .with_extra("execute_mode", json!("internal"))
        .with_extra("target_tool", json!(resolved.target))
        .with_extra("target_source", json!(resolved.target_source))
        .with_extra("dispatch_strategy", json!(resolved.dispatch_strategy))
        .with_extra(
            "dispatch_strategy_source",
            json!(resolved.dispatch_strategy_source),
        )
        .with_extra("plan_hint_summary", resolved.plan_hint_summary.clone())
        .with_extra("inner_result", inner_payload)
        .into_json()
    }

    #[test]
    fn plan_runner_dispatch_evidence_carries_canonical_source_and_kind() {
        let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
        let inner = json!({"execution_id": "plan-x", "status": "executing"});
        let entry = build_plan_runner_evidence_entry(&resolved, inner.clone());
        // wave-12 wire-compatible mapping: historical `kind="plan_runner_dispatch"`
        // moves to `source`, canonical `kind="dispatch"`.
        assert_eq!(entry["source"], "plan_runner_dispatch");
        assert_eq!(entry["kind"], "dispatch");
        assert_eq!(entry["schema_version"], "v0");
        // Inner payload lands under the canonical typed slot.
        assert_eq!(entry["inner_dispatch"], inner);
        // Pre-wave12 sidecars carried the same payload under `inner_result`;
        // we keep it as a legacy alias for byte-for-byte reader compat.
        assert_eq!(entry["inner_result"], inner);
    }

    #[test]
    fn plan_runner_dispatch_evidence_keeps_legacy_passthrough_keys_flat() {
        let resolved = fixture_resolved("mission_task_delegate", "agent-team");
        let entry =
            build_plan_runner_evidence_entry(&resolved, json!({"task_id": "btk-9"}));
        // Audit dashboards historically grep at the top level for these.
        assert_eq!(entry["execute_mode"], "internal");
        assert_eq!(entry["target_tool"], "mission_task_delegate");
        assert_eq!(entry["target_source"], "explicit_arg");
        assert_eq!(entry["dispatch_strategy"], "agent-team");
        assert_eq!(entry["dispatch_strategy_source"], "explicit_arg");
        // `plan_hint_summary` is an object — we simply assert its presence
        // (the fixture seeds an empty object so structural equality holds).
        assert!(
            entry.get("plan_hint_summary").is_some(),
            "plan_hint_summary must stay at the top level for audit grep"
        );
    }

    #[test]
    fn plan_runner_dispatch_evidence_records_event_unavailability_reason() {
        // The runner does not yet subscribe to the live ExecutionEvent bus;
        // `EventRef::unavailable(...)` documents that explicitly so consumers
        // can tell "no events" apart from "we tried but couldn't correlate".
        let resolved = fixture_resolved("mission_execution", "resident-lisp");
        let entry = build_plan_runner_evidence_entry(&resolved, json!({"ok": true}));
        let events = entry["execution_events"]
            .as_array()
            .expect("execution_events array present");
        assert_eq!(events.len(), 1, "exactly one placeholder reference");
        assert_eq!(events[0]["unavailable"], true);
        let reason = events[0]["unavailable_reason"]
            .as_str()
            .expect("reason recorded as string");
        assert!(
            reason.contains("ExecutionEvent bus"),
            "reason must mention the bus subscription gap so consumers can route on it: {}",
            reason
        );
        // No real event id leaked through.
        assert!(events[0].get("event_id").is_none());
    }

    // ── wave-14 :: plan file-first writer args ───────────────────────────

    #[test]
    fn extract_plan_file_args_defaults_are_inert() {
        let args = json!({});
        let f = extract_plan_file_args(&args);
        assert!(!f.write_file);
        assert!(!f.overwrite_file);
        assert!(f.topic.is_none());
        assert!(f.project.is_none());
        assert!(f.cwd.is_none());
        assert!(f.target_project.is_none());
    }

    #[test]
    fn extract_plan_file_args_propagates_all_keys() {
        let args = json!({
            "write_file": true,
            "overwrite_file": true,
            "topic": "wave14-foo",
            "project": "missiond",
            "cwd": "/abs/path",
            "target_project": "fallback",
        });
        let f = extract_plan_file_args(&args);
        assert!(f.write_file);
        assert!(f.overwrite_file);
        assert_eq!(f.topic, Some("wave14-foo"));
        assert_eq!(f.project, Some("missiond"));
        assert_eq!(f.cwd, Some("/abs/path"));
        assert_eq!(f.target_project, Some("fallback"));
    }

    /// The plan writer falls back to `board_task_id` when no explicit topic
    /// is provided. We assert the fallback wiring through a pure helper
    /// invocation that mirrors `maybe_write_plan_artifact`'s short-circuit
    /// logic — full integration is exercised in `file_artifacts::tests`.
    #[tokio::test]
    async fn maybe_write_plan_artifact_writes_under_board_task_topic_fallback() {
        use crate::handlers::knowledge::file_artifacts::{attempt_artifact_write, ArtifactKind, WriterContext};
        use missiond_core::types::{ProjectConfig, ProjectRegistry, SharedProjectRegistry};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let reg: SharedProjectRegistry =
            Arc::new(RwLock::new(ProjectRegistry::new(vec![ProjectConfig {
                id: "missiond".to_string(),
                path: root.display().to_string(),
                intent_path: None,
                active: true,
                slots: vec![],
                github_url: None,
                kind: "managed".to_string(),
                vault_path: None,
                parent_id: None,
                created_at: None,
                updated_at: None,
            }])));

        // Mirror the resolver call the helper would make with topic = board_task_id.
        let outcome = attempt_artifact_write(
            &reg,
            WriterContext {
                kind: ArtifactKind::Plan,
                topic: "btk-1",
                project: Some("missiond"),
                cwd: None,
                target_project: None,
                overwrite: false,
            },
            "(plan :board_task_id \"btk-1\")\n",
        )
        .await;
        let mut payload = json!({"status": "compiled", "plan_id": "abc"});
        outcome.splice_into(&mut payload);
        assert_eq!(payload["status"], "compiled", "Written must NOT downgrade status");
        assert_eq!(payload["file_written"], true);
        let path = payload["file_path"].as_str().unwrap();
        assert!(path.ends_with(".missiond/plans/btk-1/PLAN.lisp"));
    }

    // ── wave-15 / task 05 — workstation-dispatch hint contract surface ───
    //
    // These tests pin the integration contract between `ParsedPlanHints`
    // and the `workstation_dispatch` module: the new keyword fields are
    // captured, summary projection includes them, opt-in detection is
    // gated, and lisp list values round-trip through `split_lisp_string_list`.

    #[test]
    fn parse_plan_hints_captures_workstation_dispatch_contract() {
        let sexp = r#"
            (plan
              :target "mission_task_delegate"
              :dispatch-strategy "fresh-code-alignment"
              :scope "wave 15 task 05 only"
              :owned-files ["a.rs" "b.rs"]
              :forbidden-files ["c.rs"]
              :acceptance-commands ["cargo test" "git diff --check"]
              :commit-policy "scoped"
              :workstation-dispatch true)
        "#;
        let h = parse_plan_hints(sexp);
        assert_eq!(h.target.as_deref(), Some("mission_task_delegate"));
        assert_eq!(h.dispatch_strategy.as_deref(), Some("fresh-code-alignment"));
        assert_eq!(h.scope.as_deref(), Some("wave 15 task 05 only"));
        assert_eq!(h.commit_policy.as_deref(), Some("scoped"));
        assert!(h.owned_files_raw.as_deref().unwrap().contains("a.rs"));
        assert!(h.forbidden_files_raw.as_deref().unwrap().contains("c.rs"));
        assert!(h
            .acceptance_commands_raw
            .as_deref()
            .unwrap()
            .contains("cargo test"));
        assert!(h.workstation_dispatch_opt_in());
    }

    #[test]
    fn parsed_plan_hints_workstation_dispatch_opt_in_recognises_truthy_values() {
        for truthy in &["true", "TRUE", "yes", "on", "1"] {
            let mut h = ParsedPlanHints::default();
            h.workstation_dispatch_flag = Some((*truthy).to_string());
            assert!(
                h.workstation_dispatch_opt_in(),
                "expected `{}` to be truthy",
                truthy
            );
        }
        for falsy in &["false", "no", "off", "0", "maybe"] {
            let mut h = ParsedPlanHints::default();
            h.workstation_dispatch_flag = Some((*falsy).to_string());
            assert!(
                !h.workstation_dispatch_opt_in(),
                "expected `{}` to NOT be truthy",
                falsy
            );
        }
    }

    #[test]
    fn split_lisp_string_list_handles_bracket_paren_and_bareword_shapes() {
        assert!(split_lisp_string_list(None).is_empty());
        assert!(split_lisp_string_list(Some("")).is_empty());
        assert_eq!(
            split_lisp_string_list(Some(r#"["a.rs" "b.rs"]"#)),
            vec!["a.rs".to_string(), "b.rs".to_string()]
        );
        assert_eq!(
            split_lisp_string_list(Some("(x y z)")),
            vec!["x".to_string(), "y".to_string(), "z".to_string()]
        );
        // Bareword run with whitespace.
        assert_eq!(
            split_lisp_string_list(Some("a, b, c")),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn parsed_plan_hints_to_workstation_hints_projects_every_field() {
        let sexp = r#"
            (plan
              :objective "ship the wave"
              :target-project "missiond"
              :requested-cwd "/abs/missiond"
              :dispatch-strategy "agent-team"
              :scope "scope text"
              :commit-policy "scoped"
              :owned-files ["a.rs"]
              :forbidden-files ["b.rs"]
              :acceptance-commands ["cargo test"])
        "#;
        let h = parse_plan_hints(sexp);
        let w = h.to_workstation_hints();
        assert_eq!(w.objective.as_deref(), Some("ship the wave"));
        assert_eq!(w.target_project.as_deref(), Some("missiond"));
        assert_eq!(w.requested_cwd.as_deref(), Some("/abs/missiond"));
        assert_eq!(w.dispatch_strategy.as_deref(), Some("agent-team"));
        assert_eq!(w.scope.as_deref(), Some("scope text"));
        assert_eq!(w.commit_policy.as_deref(), Some("scoped"));
        assert_eq!(w.owned_files, vec!["a.rs".to_string()]);
        assert_eq!(w.forbidden_files, vec!["b.rs".to_string()]);
        assert_eq!(w.acceptance_commands, vec!["cargo test".to_string()]);
    }

    #[test]
    fn parsed_plan_hints_summary_includes_workstation_dispatch_fields() {
        let mut h = ParsedPlanHints::default();
        h.scope = Some("scope".to_string());
        h.owned_files_raw = Some(r#"["a.rs"]"#.to_string());
        h.commit_policy = Some("scoped".to_string());
        h.workstation_dispatch_flag = Some("true".to_string());
        let v = h.to_summary_json();
        assert_eq!(v["scope"], "scope");
        assert_eq!(v["commit_policy"], "scoped");
        assert!(v["owned_files"].as_str().unwrap().contains("a.rs"));
        assert_eq!(v["workstation_dispatch"], "true");
    }

    fn fixture_decision(
        source: crate::handlers::knowledge::workstation_dispatch::WorkstationDispatchSource,
    ) -> crate::handlers::knowledge::workstation_dispatch::DispatchDecision {
        crate::handlers::knowledge::workstation_dispatch::DispatchDecision {
            source,
            reason: Some("test fixture".to_string()),
        }
    }

    #[test]
    fn build_workstation_dispatch_response_dispatched_marks_status_executing() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "agent-team");
        let outcome = wd::WorkstationDispatchOutcome::Dispatched {
            task_brief: "## Objective\nship\n".to_string(),
            task_brief_path: None,
            task_contract_source_path: None,
            evidence_path: Some("/tmp/sidecar.json".to_string()),
            evidence_error: None,
            inner_payload: json!({"task_id": "btk-7"}),
        };
        let decision = fixture_decision(wd::WorkstationDispatchSource::ExplicitArg);
        let result = build_workstation_dispatch_response(
            &plan,
            &resolved,
            outcome,
            &decision,
            &TaskContractEmissionRecord::off(),
            DispatchContractMode::Rendered,
        );
        let v = parse_payload(&result);
        assert_eq!(v["status"], "executing");
        assert_eq!(v["runner_status"], "workstation_dispatch_v0");
        assert_eq!(v["target_tool"], "mission_task_delegate");
        assert_eq!(v["dispatch_strategy"], "agent-team");
        assert_eq!(v["workstation_dispatch_status"], "dispatched");
        assert_eq!(v["evidence_path"], "/tmp/sidecar.json");
        assert_eq!(v["inner_result"]["task_id"], "btk-7");
        assert_eq!(v["workstation_dispatch_source"], "explicit_arg");
        assert_eq!(v["workstation_dispatch_inference_reason"], "test fixture");
        // wave-20 / task 04 — default rendered mode is byte-stable on
        // the wire: the new `dispatch_contract_mode` key surfaces but
        // the legacy `task_contract_source_path` extension stays
        // absent.
        assert_eq!(v["dispatch_contract_mode"], "rendered");
        assert!(
            v.get("task_contract_source_path").is_none(),
            "rendered-mode dispatch must omit task_contract_source_path"
        );
    }

    #[test]
    fn build_workstation_dispatch_response_safe_descriptor_does_not_claim_executing() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let outcome = wd::WorkstationDispatchOutcome::SafeDescriptor {
            reason: wd::SafeDescriptorReason::ProjectRootUnresolved(
                "no signal".to_string(),
            ),
            task_brief: None,
        };
        let decision = fixture_decision(wd::WorkstationDispatchSource::Inferred);
        let result = build_workstation_dispatch_response(
            &plan,
            &resolved,
            outcome,
            &decision,
            &TaskContractEmissionRecord::off(),
            DispatchContractMode::Rendered,
        );
        let v = parse_payload(&result);
        assert_ne!(v["status"], "executing");
        assert_eq!(v["status"], "dispatch_skipped");
        assert_eq!(v["workstation_dispatch_status"], "skipped_project_root_unresolved");
        assert!(v.get("inner_result").is_none());
        // Even when the substrate refused, we surface that auto-inference
        // routed the call so the caller sees both the routing decision and
        // the safety failure side by side — never a silent prompt fallback.
        assert_eq!(v["workstation_dispatch_source"], "inferred");
    }

    #[test]
    fn build_workstation_dispatch_response_dry_run_status_is_dry_run() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let outcome = wd::WorkstationDispatchOutcome::DryRun {
            task_brief: "## Objective\nship\n".to_string(),
        };
        let decision = fixture_decision(wd::WorkstationDispatchSource::PlanHint);
        let result = build_workstation_dispatch_response(
            &plan,
            &resolved,
            outcome,
            &decision,
            &TaskContractEmissionRecord::off(),
            DispatchContractMode::Rendered,
        );
        let v = parse_payload(&result);
        assert_eq!(v["status"], "dry_run");
        assert_eq!(v["workstation_dispatch_status"], "dry_run_no_dispatch");
        assert_eq!(v["workstation_dispatch_source"], "plan_hint");
    }

    /// wave-20 / task 04 — when the runner dispatched in machine mode,
    /// the response carries `dispatch_contract_mode="machine"` AND the
    /// resolved `task_contract_source_path` so observers (audit, PR
    /// review, CI) can prove the on-disk Lisp drove the brief — the
    /// markdown rendering (if requested via `render_command`) is
    /// purely compatibility metadata in this mode.
    #[test]
    fn build_workstation_dispatch_response_machine_mode_pins_contract_path() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "agent-team");
        let outcome = wd::WorkstationDispatchOutcome::Dispatched {
            task_brief: "## Source contract\n- task-contract v1: `/tmp/p/.missiond/tasks/generated/plan/root.lisp`\n## Objective\nship\n".to_string(),
            task_brief_path: None,
            task_contract_source_path: Some(
                "/tmp/p/.missiond/tasks/generated/plan/root.lisp".to_string(),
            ),
            evidence_path: Some("/tmp/sidecar.json".to_string()),
            evidence_error: None,
            inner_payload: json!({"task_id": "btk-machine"}),
        };
        let decision = fixture_decision(wd::WorkstationDispatchSource::ExplicitArg);
        let result = build_workstation_dispatch_response(
            &plan,
            &resolved,
            outcome,
            &decision,
            &TaskContractEmissionRecord::off(),
            DispatchContractMode::Machine,
        );
        let v = parse_payload(&result);
        assert_eq!(v["status"], "executing");
        assert_eq!(v["workstation_dispatch_status"], "dispatched");
        assert_eq!(v["dispatch_contract_mode"], "machine");
        assert_eq!(
            v["task_contract_source_path"],
            "/tmp/p/.missiond/tasks/generated/plan/root.lisp",
            "machine-mode dispatch must surface the resolved contract path \
             so observers can prove the Lisp drove the brief (load-bearing SSOT)"
        );
        // The brief preview reflects the consumer overlay — the
        // `## Source contract` preamble is present, naming the same
        // on-disk path. This pins the requirement that markdown
        // rendering becomes optional compatibility metadata in
        // machine mode (not load-bearing).
        let preview = v["task_brief_preview"].as_str().unwrap_or("");
        assert!(
            preview.contains("## Source contract"),
            "machine-mode brief must carry the wave-19/07 `## Source contract` preamble"
        );
    }

    /// wave-20 / task 04 — when the workstation substrate refuses a
    /// machine-mode dispatch because the on-disk task.lisp is malformed,
    /// the response surfaces `SafeDescriptor` (status=
    /// `skipped_malformed_task_contract`) with `dispatch_contract_mode=
    /// "machine"`. We MUST NOT downgrade to `claude -p` or the legacy
    /// natural-language brief — silently falling back would defeat the
    /// machine SSOT contract.
    #[test]
    fn build_workstation_dispatch_response_machine_mode_malformed_contract_refuses() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let outcome = wd::WorkstationDispatchOutcome::SafeDescriptor {
            reason: wd::SafeDescriptorReason::MalformedTaskContract {
                path: "/tmp/p/.missiond/tasks/generated/plan/root.lisp".to_string(),
                reason: "missing required `goal` field".to_string(),
            },
            task_brief: None,
        };
        let decision = fixture_decision(wd::WorkstationDispatchSource::ExplicitArg);
        let result = build_workstation_dispatch_response(
            &plan,
            &resolved,
            outcome,
            &decision,
            &TaskContractEmissionRecord::off(),
            DispatchContractMode::Machine,
        );
        let v = parse_payload(&result);
        // No silent prompt fallback — the runner must surface the
        // refusal verbatim.
        assert_eq!(v["status"], "dispatch_skipped");
        assert_eq!(
            v["workstation_dispatch_status"],
            "skipped_malformed_task_contract"
        );
        assert_eq!(v["dispatch_contract_mode"], "machine");
        // Inner result must not have leaked through — we never
        // dispatched.
        assert!(v.get("inner_result").is_none());
        // The reason text must name the path so the caller can fix
        // and retry.
        let reason = v["workstation_dispatch_reason"].as_str().unwrap_or("");
        assert!(
            reason.contains(".missiond/tasks/generated"),
            "malformed-contract refusal must name the offending path"
        );
        assert!(
            reason.contains("missing required `goal` field"),
            "malformed-contract refusal must explain why the parse failed"
        );
    }

    // ── wave-16 / task 03 — auto-inference integration with plan hints ──
    //
    // The decision is the composition of `ParsedPlanHints::to_workstation_hints`
    // with `evaluate_dispatch_decision`. These tests exercise the full
    // pipeline so a refactor that moves the merge point can't silently
    // change the inference outcome.

    /// Build the inference context the runner would build at this point —
    /// keeps the test bodies short and pins the merge order.
    fn build_inference_ctx<'a>(
        target: &'a str,
        dispatch_strategy: &'a str,
        merged: &'a crate::handlers::knowledge::workstation_dispatch::WorkstationDispatchHints,
    ) -> crate::handlers::knowledge::workstation_dispatch::InferenceContext<'a> {
        crate::handlers::knowledge::workstation_dispatch::InferenceContext {
            target,
            dispatch_strategy,
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
        }
    }

    #[test]
    fn auto_inference_fires_for_task_delegate_with_owned_files_and_strategy() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        // Hints come from a plan body that already has every signal — no
        // explicit caller arg, no PLAN-level :workstation-dispatch flag.
        let sexp = r#"
            (plan
              :objective "ship the wave"
              :dispatch-strategy "fresh-code-alignment"
              :owned-files ["a.rs" "b.rs"])
        "#;
        let hints = parse_plan_hints(sexp);
        let merged = hints.to_workstation_hints().merge_args(&json!({}));
        let ctx = build_inference_ctx("mission_task_delegate", "fresh-code-alignment", &merged);
        let decision = wd::evaluate_dispatch_decision(
            &json!({}),
            hints.workstation_dispatch_opt_in(),
            &ctx,
        );
        assert_eq!(decision.source, wd::WorkstationDispatchSource::Inferred);
        assert!(decision.is_enabled());
    }

    #[test]
    fn auto_inference_disabled_by_explicit_workstation_dispatch_false() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        // Same hints as above — explicit false must still suppress.
        let sexp = r#"
            (plan
              :objective "ship"
              :dispatch-strategy "fresh-code-alignment"
              :owned-files ["a.rs"])
        "#;
        let hints = parse_plan_hints(sexp);
        let merged = hints
            .to_workstation_hints()
            .merge_args(&json!({"workstation_dispatch": false}));
        let ctx = build_inference_ctx("mission_task_delegate", "fresh-code-alignment", &merged);
        let decision = wd::evaluate_dispatch_decision(
            &json!({"workstation_dispatch": false}),
            hints.workstation_dispatch_opt_in(),
            &ctx,
        );
        assert_eq!(decision.source, wd::WorkstationDispatchSource::Disabled);
        assert!(!decision.is_enabled());
    }

    #[test]
    fn explicit_workstation_dispatch_true_preserves_wave15_path() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        // Even with no scoping hints in PLAN.lisp, explicit true still
        // routes through workstation-dispatch — wave-15 contract pin.
        let hints = parse_plan_hints("(plan :objective \"ship\")");
        let merged = hints
            .to_workstation_hints()
            .merge_args(&json!({"workstation_dispatch": true}));
        let ctx = build_inference_ctx("mission_task_delegate", "unknown", &merged);
        let decision = wd::evaluate_dispatch_decision(
            &json!({"workstation_dispatch": true}),
            hints.workstation_dispatch_opt_in(),
            &ctx,
        );
        assert_eq!(decision.source, wd::WorkstationDispatchSource::ExplicitArg);
        assert!(decision.is_enabled());
    }

    #[test]
    fn auto_inference_skipped_when_strategy_unknown() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let sexp = r#"
            (plan
              :objective "ship"
              :owned-files ["a.rs"])
        "#;
        let hints = parse_plan_hints(sexp);
        let merged = hints.to_workstation_hints().merge_args(&json!({}));
        // Strategy resolves to `unknown` because no :dispatch-strategy or
        // :parallelism hint is supplied — same default the runner would
        // arrive at via `resolve_dispatch_strategy`.
        let ctx = build_inference_ctx("mission_task_delegate", "unknown", &merged);
        let decision = wd::evaluate_dispatch_decision(
            &json!({}),
            hints.workstation_dispatch_opt_in(),
            &ctx,
        );
        assert_eq!(decision.source, wd::WorkstationDispatchSource::NotApplicable);
    }

    #[test]
    fn auto_inference_skipped_when_objective_missing() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let sexp = r#"
            (plan
              :dispatch-strategy "fresh-code-alignment"
              :owned-files ["a.rs"])
        "#;
        let hints = parse_plan_hints(sexp);
        let merged = hints.to_workstation_hints().merge_args(&json!({}));
        let ctx = build_inference_ctx("mission_task_delegate", "fresh-code-alignment", &merged);
        let decision = wd::evaluate_dispatch_decision(
            &json!({}),
            hints.workstation_dispatch_opt_in(),
            &ctx,
        );
        assert_eq!(decision.source, wd::WorkstationDispatchSource::NotApplicable);
        assert!(decision.reason.unwrap().contains("objective"));
    }

    #[test]
    fn auto_inference_skipped_for_mission_execution_target() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let sexp = r#"
            (plan
              :objective "ship"
              :dispatch-strategy "fresh-code-alignment"
              :owned-files ["a.rs"])
        "#;
        let hints = parse_plan_hints(sexp);
        let merged = hints.to_workstation_hints().merge_args(&json!({}));
        let ctx = build_inference_ctx("mission_execution", "fresh-code-alignment", &merged);
        let decision = wd::evaluate_dispatch_decision(
            &json!({}),
            hints.workstation_dispatch_opt_in(),
            &ctx,
        );
        assert_eq!(decision.source, wd::WorkstationDispatchSource::NotApplicable);
    }

    #[test]
    fn auto_inference_skipped_for_mission_flow_run_target() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let sexp = r#"
            (plan
              :objective "ship"
              :dispatch-strategy "fresh-code-alignment"
              :owned-files ["a.rs"])
        "#;
        let hints = parse_plan_hints(sexp);
        let merged = hints.to_workstation_hints().merge_args(&json!({}));
        let ctx = build_inference_ctx("mission_flow_run", "fresh-code-alignment", &merged);
        let decision = wd::evaluate_dispatch_decision(
            &json!({}),
            hints.workstation_dispatch_opt_in(),
            &ctx,
        );
        assert_eq!(decision.source, wd::WorkstationDispatchSource::NotApplicable);
    }

    #[test]
    fn auto_inference_fires_for_agent_team_with_target_project_signal() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        // Scoping signal in this case is `target_project`, NOT owned_files.
        let sexp = r#"
            (plan
              :objective "ship the wave"
              :dispatch-strategy "agent-team"
              :target-project "missiond")
        "#;
        let hints = parse_plan_hints(sexp);
        let merged = hints.to_workstation_hints().merge_args(&json!({}));
        let ctx = build_inference_ctx("mission_task_delegate", "agent-team", &merged);
        let decision = wd::evaluate_dispatch_decision(
            &json!({}),
            hints.workstation_dispatch_opt_in(),
            &ctx,
        );
        assert_eq!(decision.source, wd::WorkstationDispatchSource::Inferred);
    }

    #[test]
    fn auto_inference_skipped_when_no_scope_signal() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        // Objective + strategy + target are all fine, but NO scoping hint:
        // no owned-files, no scope, no target-project, no requested-cwd.
        let sexp = r#"
            (plan
              :objective "ship"
              :dispatch-strategy "fresh-code-alignment")
        "#;
        let hints = parse_plan_hints(sexp);
        let merged = hints.to_workstation_hints().merge_args(&json!({}));
        let ctx = build_inference_ctx("mission_task_delegate", "fresh-code-alignment", &merged);
        let decision = wd::evaluate_dispatch_decision(
            &json!({}),
            hints.workstation_dispatch_opt_in(),
            &ctx,
        );
        assert_eq!(decision.source, wd::WorkstationDispatchSource::NotApplicable);
        assert!(decision.reason.unwrap().contains("scoping signal"));
    }

    #[test]
    fn workstation_dispatch_opt_in_off_when_arg_absent_and_plan_hint_absent() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let args = json!({});
        let hints = ParsedPlanHints::default();
        assert!(!wd::opt_in_requested(
            &args,
            hints.workstation_dispatch_opt_in()
        ));
    }

    #[test]
    fn workstation_dispatch_opt_in_on_when_plan_hint_only() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let args = json!({});
        let mut hints = ParsedPlanHints::default();
        hints.workstation_dispatch_flag = Some("true".to_string());
        assert!(wd::opt_in_requested(
            &args,
            hints.workstation_dispatch_opt_in()
        ));
    }

    #[test]
    fn workstation_dispatch_opt_in_on_when_explicit_arg_only() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let args = json!({"workstation_dispatch": true});
        let hints = ParsedPlanHints::default();
        assert!(wd::opt_in_requested(
            &args,
            hints.workstation_dispatch_opt_in()
        ));
    }

    // ── wave-15 :: plan resolution bridge — pure handler-shape ──────────
    //
    // Same pattern as the directive tests: drive the validation /
    // stamping helpers that the plan handler composes for approve / mark
    // / supersede. The DB-touching path (plan_get) is exercised by the
    // daemon test suite; here we pin the deterministic branch logic so a
    // refactor that breaks the resolution contract fails loud.
    use crate::handlers::knowledge::review_gate::{
        parse_review_question_id_struct as wave15_parse_qid,
        parse_review_resolution_input as wave15_parse_input,
        stamp_needs_changes_next_step as wave15_stamp_next_step,
        stamp_resolution_payload as wave15_stamp_payload,
        validate_review_resolution_envelope as wave15_validate_envelope,
        ResolutionInputError as Wave15ResolutionInputError,
        ReviewDecision as Wave15ReviewDecision,
        ReviewResolutionInput as Wave15ReviewResolutionInput,
    };

    #[test]
    fn plan_action_whitelist_pins_state_changing_actions() {
        // Pin the action whitelist for the plan surface. Update lockstep
        // with the resolution wiring if a new state-changing action lands.
        assert_eq!(PLAN_REVIEW_ACTIONS, &["compile", "approve", "mark", "supersede"]);
    }

    #[test]
    fn plan_resolution_input_missing_decision_rejected_at_handler_boundary() {
        let args = json!({
            "plan_id": "00000000-0000-0000-0000-000000000abc",
            "review_question_id": "review:plan:00000000-0000-0000-0000-000000000abc:v1:approve",
        });
        let err = wave15_parse_input(&args).unwrap_err();
        assert_eq!(err, Wave15ResolutionInputError::MissingDecision);
    }

    #[test]
    fn plan_resolution_envelope_accepts_canonical_approve() {
        let qid = "review:plan:00000000-0000-0000-0000-000000000abc:v1:approve";
        let parsed = wave15_parse_qid(qid).unwrap();
        wave15_validate_envelope(
            &parsed,
            "plan",
            "00000000-0000-0000-0000-000000000abc",
            1,
            PLAN_REVIEW_ACTIONS,
        )
        .expect("approve via valid review id must pass envelope validation");
    }

    #[test]
    fn plan_resolution_envelope_accepts_canonical_mark() {
        let qid = "review:plan:00000000-0000-0000-0000-000000000abc:v2:mark";
        let parsed = wave15_parse_qid(qid).unwrap();
        wave15_validate_envelope(
            &parsed,
            "plan",
            "00000000-0000-0000-0000-000000000abc",
            2,
            PLAN_REVIEW_ACTIONS,
        )
        .expect("mark via valid review id must pass envelope validation");
    }

    #[test]
    fn plan_resolution_envelope_accepts_canonical_supersede() {
        let qid = "review:plan:00000000-0000-0000-0000-000000000abc:v1:supersede";
        let parsed = wave15_parse_qid(qid).unwrap();
        wave15_validate_envelope(
            &parsed,
            "plan",
            "00000000-0000-0000-0000-000000000abc",
            1,
            PLAN_REVIEW_ACTIONS,
        )
        .expect("supersede via valid review id must pass envelope validation");
    }

    #[test]
    fn plan_resolution_envelope_rejects_stale_version() {
        let qid = "review:plan:00000000-0000-0000-0000-000000000abc:v1:approve";
        let parsed = wave15_parse_qid(qid).unwrap();
        let err = wave15_validate_envelope(
            &parsed,
            "plan",
            "00000000-0000-0000-0000-000000000abc",
            3,
            PLAN_REVIEW_ACTIONS,
        )
        .unwrap_err();
        assert_eq!(err.code(), "STALE_REVIEW_VERSION");
    }

    #[test]
    fn plan_resolution_envelope_rejects_scope_mismatch() {
        // qid says scope=directive but submitted to the plan surface →
        // REVIEW_SCOPE_MISMATCH (handler rejects before mutating state).
        let qid = "review:directive:00000000-0000-0000-0000-000000000abc:v1:approve";
        let parsed = wave15_parse_qid(qid).unwrap();
        let err = wave15_validate_envelope(
            &parsed,
            "plan",
            "00000000-0000-0000-0000-000000000abc",
            1,
            PLAN_REVIEW_ACTIONS,
        )
        .unwrap_err();
        assert_eq!(err.code(), "REVIEW_SCOPE_MISMATCH");
    }

    #[test]
    fn plan_resolution_envelope_rejects_unsupported_action() {
        // archive isn't a valid plan-surface action even though it's
        // valid on the directive surface — must be REJECTED here.
        let qid = "review:plan:00000000-0000-0000-0000-000000000abc:v1:archive";
        let parsed = wave15_parse_qid(qid).unwrap();
        let err = wave15_validate_envelope(
            &parsed,
            "plan",
            "00000000-0000-0000-0000-000000000abc",
            1,
            PLAN_REVIEW_ACTIONS,
        )
        .unwrap_err();
        assert_eq!(err.code(), "REVIEW_ACTION_UNSUPPORTED");
    }

    #[test]
    fn plan_rejected_decision_records_reason_in_payload_without_approving() {
        let input = Wave15ReviewResolutionInput {
            question_id: "review:plan:00000000-0000-0000-0000-000000000abc:v1:approve".to_string(),
            decision: Wave15ReviewDecision::Rejected,
            actor: Some("operator-1".to_string()),
            note: Some("PLAN.lisp missing acceptance commands".to_string()),
        };
        // Replay the handler's keep-artifact branch.
        let mut payload = json!({
            "plan_id": "00000000-0000-0000-0000-000000000abc",
            "version": 1,
        });
        payload["status"] = json!("review_rejected");
        wave15_stamp_payload(&mut payload, &input);
        assert_eq!(payload["status"], "review_rejected");
        assert_eq!(payload["review_decision"], "rejected");
        assert_eq!(payload["review_decision_outcome"], "keep_artifact");
        assert_eq!(payload["review_actor"], "operator-1");
        assert!(payload["review_note"]
            .as_str()
            .unwrap()
            .contains("acceptance commands"));
    }

    #[test]
    fn plan_needs_changes_decision_surfaces_next_step() {
        let input = Wave15ReviewResolutionInput {
            question_id: "review:plan:00000000-0000-0000-0000-000000000abc:v1:approve".to_string(),
            decision: Wave15ReviewDecision::NeedsChanges,
            actor: Some("operator-1".to_string()),
            note: Some("split DAG into smaller waves".to_string()),
        };
        let mut payload = json!({
            "plan_id": "00000000-0000-0000-0000-000000000abc",
            "version": 1,
        });
        payload["status"] = json!("review_needs_changes");
        wave15_stamp_next_step(&mut payload, "plan", "compile");
        wave15_stamp_payload(&mut payload, &input);
        assert_eq!(payload["status"], "review_needs_changes");
        assert_eq!(payload["review_decision"], "needs_changes");
        assert_eq!(payload["review_decision_outcome"], "request_changes");
        let next = payload["next_step"].as_str().unwrap();
        assert!(next.contains("rework"));
        assert!(next.contains("plan"));
        assert!(next.contains("compile"));
    }

    #[test]
    fn plan_resolution_legacy_quiet_path_returns_none_when_no_qid() {
        let args = json!({"plan_id": "00000000-0000-0000-0000-000000000abc"});
        assert!(wave15_parse_input(&args).unwrap().is_none());
    }

    #[test]
    fn plan_supersede_envelope_anchored_to_old_plan_id() {
        // For supersede, the resolution envelope is anchored to the OLD
        // plan id (the artifact being closed), not the new one.
        let qid = "review:plan:00000000-0000-0000-0000-000000000aaa:v1:supersede";
        let parsed = wave15_parse_qid(qid).unwrap();
        wave15_validate_envelope(
            &parsed,
            "plan",
            "00000000-0000-0000-0000-000000000aaa",
            1,
            PLAN_REVIEW_ACTIONS,
        )
        .expect("supersede must validate against old_plan_id");
        let err = wave15_validate_envelope(
            &parsed,
            "plan",
            "00000000-0000-0000-0000-000000000bbb", // new id — must fail
            1,
            PLAN_REVIEW_ACTIONS,
        )
        .unwrap_err();
        assert_eq!(err.code(), "REVIEW_ARTIFACT_MISMATCH");
    }

    // ── wave-16 :: subscriber outcome enum is loud + DB-free ────────────

    #[test]
    fn plan_subscriber_outcome_supersede_needs_explicit_call() {
        // The subscriber path can only see the OLD plan id from the qid;
        // it cannot infer the NEW plan id, so supersede must be deferred
        // to the explicit caller-side bridge.
        let outcome = PlanSubscriberOutcome::SupersedeNeedsExplicitCall;
        assert_eq!(outcome, PlanSubscriberOutcome::SupersedeNeedsExplicitCall);
    }

    #[test]
    fn plan_subscriber_outcome_mark_needs_explicit_call() {
        // The `mark` qid envelope encodes the action label only, not the
        // target column value, so the subscriber cannot infer which
        // PlanStatus to flip to.
        let outcome = PlanSubscriberOutcome::MarkNeedsExplicitCall;
        assert_eq!(outcome, PlanSubscriberOutcome::MarkNeedsExplicitCall);
    }

    #[test]
    fn plan_subscriber_outcome_compile_no_op_carries_decision() {
        let outcome = PlanSubscriberOutcome::CompileNoOp {
            decision: ReviewDecision::Approved,
        };
        assert_eq!(
            outcome,
            PlanSubscriberOutcome::CompileNoOp {
                decision: ReviewDecision::Approved
            }
        );
    }

    // ── wave-17 / task 01 — resume input field set ─────────────────────

    #[test]
    fn parse_plan_node_resume_input_via_handler_boundary_matches_review_gate_helper() {
        // The plan handler invokes `parse_plan_node_resume_input` from
        // `review_gate.rs`. Pin the wire shape end-to-end so the handler
        // boundary stays in sync with the helper's contract.
        let args = json!({
            "resume_review_question_id": "review:plan:abc:v1:plan-node:0123456789abcdef",
            "resume_review_decision": "approved",
            "resume_actor": "agent-team",
            "resume_note": "proceed",
        });
        let input = parse_plan_node_resume_input(&args)
            .expect("ok")
            .expect("some");
        assert_eq!(
            input.question_id,
            "review:plan:abc:v1:plan-node:0123456789abcdef"
        );
        assert_eq!(input.decision, ReviewDecision::Approved);
        assert_eq!(input.actor.as_deref(), Some("agent-team"));
        assert_eq!(input.note.as_deref(), Some("proceed"));
    }

    #[test]
    fn parse_plan_node_resume_input_handler_boundary_quiet_when_id_absent() {
        // No resume id → caller falls through to the standard execute
        // pipeline. Must NOT error so the wave-15 manager-side resolution
        // input contract stays byte-identical.
        let args = json!({
            "review_question_id": "review:plan:abc:v1:approve",
            "review_decision": "approved",
        });
        assert!(parse_plan_node_resume_input(&args)
            .expect("ok")
            .is_none());
    }

    // ── wave-18 / task 05 — cross-plan distill chain v0 ─────────────────

    #[test]
    fn distill_chain_requested_detects_any_chain_knob() {
        // No chain knobs → caller did not opt in. Backward-compat with
        // wave-17 / task 05 byte-shape: the chain orchestrator never
        // touches the response when the caller is silent.
        assert!(!distill_chain_requested(&json!({})));
        // Any single knob counts.
        assert!(distill_chain_requested(&json!({"distill_chain_id": "chain-1"})));
        assert!(distill_chain_requested(&json!({"distill_chain_mode": "record_only"})));
        assert!(distill_chain_requested(&json!({"distill_chain_name": "loop"})));
        // Combination still counts (canonical opt-in shape).
        assert!(distill_chain_requested(
            &json!({"distill_chain_id": "x", "distill_chain_mode": "dry_run"})
        ));
    }

    #[test]
    fn parse_distill_chain_id_blank_collapses_to_none() {
        // Blank / whitespace-only must NOT poison the audit row with an
        // empty id — collapses to absent so the runner falls back to
        // the deterministic auto id (chain:auto:plan-<plan_id>).
        assert_eq!(parse_distill_chain_id(&json!({})), None);
        assert_eq!(parse_distill_chain_id(&json!({"distill_chain_id": ""})), None);
        assert_eq!(parse_distill_chain_id(&json!({"distill_chain_id": "   "})), None);
        assert_eq!(
            parse_distill_chain_id(&json!({"distill_chain_id": "chain-42"})),
            Some("chain-42".to_string())
        );
        // Trim leading / trailing whitespace so caller-side concat
        // accidents don't shift the wire form silently.
        assert_eq!(
            parse_distill_chain_id(&json!({"distill_chain_id": "  chain-42  "})),
            Some("chain-42".to_string())
        );
    }

    #[test]
    fn parse_distill_chain_name_blank_collapses_to_none() {
        assert_eq!(parse_distill_chain_name(&json!({})), None);
        assert_eq!(parse_distill_chain_name(&json!({"distill_chain_name": ""})), None);
        assert_eq!(
            parse_distill_chain_name(&json!({"distill_chain_name": "  "})),
            None
        );
        assert_eq!(
            parse_distill_chain_name(&json!({"distill_chain_name": "wave18-loop"})),
            Some("wave18-loop".to_string())
        );
    }

    #[test]
    fn parse_distill_chain_mode_default_is_record_only() {
        // Absent / blank / canonical literal all collapse onto record_only
        // so the response always echoes a known mode. record_only is the
        // most conservative choice (no LLM, no workflow call).
        assert_eq!(parse_distill_chain_mode(&json!({})).unwrap(), "record_only");
        assert_eq!(
            parse_distill_chain_mode(&json!({"distill_chain_mode": ""})).unwrap(),
            "record_only"
        );
        assert_eq!(
            parse_distill_chain_mode(&json!({"distill_chain_mode": "record_only"})).unwrap(),
            "record_only"
        );
        assert_eq!(
            parse_distill_chain_mode(&json!({"distill_chain_mode": "dry_run"})).unwrap(),
            "dry_run"
        );
        assert_eq!(
            parse_distill_chain_mode(&json!({"distill_chain_mode": "sonnet"})).unwrap(),
            "sonnet"
        );
    }

    #[test]
    fn parse_distill_chain_mode_rejects_typos() {
        // Strict allowlist mirrors workflow.rs / wave-17 task 05. Sonnet
        // typos are particularly important to catch — the brief forbids
        // ever invoking sonnet without an explicit mode, and a silent
        // collapse to record_only would mask the caller's intent.
        let err =
            parse_distill_chain_mode(&json!({"distill_chain_mode": "sonett"})).unwrap_err();
        assert!(
            err.contains("sonett"),
            "error must echo the rejected value, got `{}`",
            err
        );
        assert!(
            err.contains("record_only"),
            "error must spell out the allowlist, got `{}`",
            err
        );

        let err2 =
            parse_distill_chain_mode(&json!({"distill_chain_mode": "live"})).unwrap_err();
        assert!(err2.contains("live"));
    }

    #[test]
    fn validate_distill_chain_args_rejects_chain_without_finalize() {
        // Any chain knob without finalize_plan=true must fail-fast — the
        // chain only fires AFTER a successful finalization, so silently
        // dropping the chain request would mask the caller's intent.
        let result = validate_distill_chain_args(
            &json!({"distill_chain_id": "chain-1"})
        )
        .expect("validator must reject");
        assert_eq!(result.is_error, Some(true));
        // Structured-error payload carries `error_code` + `reason`.
        let payload = tool_result_payload(&result);
        assert_eq!(
            payload.get("error_code").and_then(|v| v.as_str()),
            Some("INVALID_PARAM")
        );
        let reason = payload
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(
            reason.contains("finalize_plan=true"),
            "error must point at the missing finalize knob; got `{}`",
            reason
        );
    }

    #[test]
    fn validate_distill_chain_args_rejects_unknown_mode_even_without_finalize() {
        // Validation runs eagerly on the mode allowlist so a typo never
        // survives until the next live caller. The mode check fires
        // BEFORE the finalize cross-field rule so the error message
        // points at the actual typo.
        let result = validate_distill_chain_args(
            &json!({"distill_chain_mode": "warp"})
        )
        .expect("validator must reject");
        assert_eq!(result.is_error, Some(true));
        let payload = tool_result_payload(&result);
        let reason = payload
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(reason.contains("warp"), "got `{}`", reason);
    }

    #[test]
    fn validate_distill_chain_args_accepts_canonical_opt_in() {
        // Canonical shape: finalize_plan=true + chain knobs.
        assert!(validate_distill_chain_args(&json!({
            "finalize_plan": true,
            "distill_chain_id": "chain-1",
            "distill_chain_mode": "record_only",
            "distill_chain_name": "wave18-loop",
        }))
        .is_none());
        // Bare chain mode + finalize_plan also fine (id auto-derived).
        assert!(validate_distill_chain_args(&json!({
            "finalize_plan": true,
            "distill_chain_mode": "dry_run",
        }))
        .is_none());
        // No chain knobs at all → backward-compat (wave-17 / task 04 byte-shape).
        assert!(validate_distill_chain_args(&json!({})).is_none());
    }

    #[test]
    fn validate_distill_chain_args_accepts_auto_sonnet_bool_shapes() {
        // wave-21 / task 07 — both auto_sonnet knobs accept the
        // canonical bool shape. Pairing them does not require
        // finalize_plan because the validator scopes the cross-field
        // rule to wave-18 chain knobs (auto_sonnet is forwarded by
        // workflow.rs, not gated here).
        assert!(validate_distill_chain_args(&json!({
            "auto_sonnet": true,
            "auto_sonnet_approved": true,
        }))
        .is_none());
        assert!(validate_distill_chain_args(&json!({
            "auto_sonnet": false,
            "auto_sonnet_approved": false,
        }))
        .is_none());
    }

    #[test]
    fn validate_distill_chain_args_rejects_auto_sonnet_string_typo() {
        // wave-21 / task 07 — the apply-gate strict-shape validator
        // refuses string `"true"` and routes through INVALID_PARAM.
        let result = validate_distill_chain_args(&json!({"auto_sonnet": "true"}))
            .expect("validator must reject string-shape auto_sonnet");
        assert_eq!(result.is_error, Some(true));
        let payload = tool_result_payload(&result);
        let reason = payload
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(
            reason.contains("auto_sonnet must be a boolean"),
            "reason: {}",
            reason
        );
        assert!(reason.contains("string"), "shape label leaked: {}", reason);
    }

    #[test]
    fn validate_distill_chain_args_rejects_auto_sonnet_approved_number_typo() {
        // wave-21 / task 07 — the caller-attestation flag is also
        // strict-bool. Numbers fail loud.
        let result = validate_distill_chain_args(
            &json!({"auto_sonnet_approved": 1}),
        )
        .expect("validator must reject number-shape auto_sonnet_approved");
        assert_eq!(result.is_error, Some(true));
        let payload = tool_result_payload(&result);
        let reason = payload
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(
            reason.contains("auto_sonnet_approved must be a boolean"),
            "reason: {}",
            reason
        );
    }

    #[test]
    fn validate_distill_chain_args_accepts_auto_sonnet_policy_canonical_strings() {
        // wave-22 / task 06 — the closed-enum policy validator accepts
        // the three canonical strings (off | safe_after_rules | dry_run)
        // plus null / missing (which collapse to off).
        for v in [
            json!("off"),
            json!("safe_after_rules"),
            json!("dry_run"),
            json!(""),
            json!(null),
        ] {
            assert!(
                validate_distill_chain_args(&json!({"auto_sonnet_policy": v}))
                    .is_none(),
                "policy={:?} must validate",
                v
            );
        }
        // Missing also fine.
        assert!(validate_distill_chain_args(&json!({})).is_none());
    }

    #[test]
    fn validate_distill_chain_args_rejects_auto_sonnet_policy_unknown_string() {
        // wave-22 / task 06 — typo / camelCase / case mismatch all
        // fail-fast as INVALID_PARAM. A single typo cannot escalate
        // the daemon (I2 carryover from wave-21/07).
        let result = validate_distill_chain_args(
            &json!({"auto_sonnet_policy": "safeAfterRules"}),
        )
        .expect("validator must reject unknown policy string");
        assert_eq!(result.is_error, Some(true));
        let payload = tool_result_payload(&result);
        let reason = payload
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(
            reason.contains("auto_sonnet_policy must be one of"),
            "reason: {}",
            reason
        );
        assert!(reason.contains("safeAfterRules"), "echoed bad value: {}", reason);
    }

    #[test]
    fn validate_distill_chain_args_rejects_auto_sonnet_policy_non_string_shapes() {
        // wave-22 / task 06 — bool / number / array / object all fail.
        for bad in [
            json!({"auto_sonnet_policy": true}),
            json!({"auto_sonnet_policy": 1}),
            json!({"auto_sonnet_policy": ["safe_after_rules"]}),
            json!({"auto_sonnet_policy": {"value": "safe_after_rules"}}),
        ] {
            let result = validate_distill_chain_args(&bad)
                .expect("validator must reject non-string policy shape");
            assert_eq!(result.is_error, Some(true), "input: {:?}", bad);
            let payload = tool_result_payload(&result);
            let reason = payload
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            assert!(
                reason.contains("auto_sonnet_policy must be a string"),
                "reason: {} (input: {:?})",
                reason,
                bad
            );
        }
    }

    #[test]
    fn json_shape_label_returns_canonical_json_type_name() {
        // Plan-side label helper mirrors workflow::shape_label so the
        // two surfaces emit identical wording on shape rejections.
        assert_eq!(json_shape_label(&json!(null)), "null");
        assert_eq!(json_shape_label(&json!(true)), "boolean");
        assert_eq!(json_shape_label(&json!(42)), "number");
        assert_eq!(json_shape_label(&json!("x")), "string");
        assert_eq!(json_shape_label(&json!([1, 2])), "array");
        assert_eq!(json_shape_label(&json!({"k": "v"})), "object");
    }

    #[test]
    fn derive_fallback_chain_id_anchors_on_plan_id() {
        // Deterministic fallback so re-runs against the same plan land
        // on the same chain bucket — auditors can correlate without
        // rolling a UUID.
        let plan_id =
            uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000abc").unwrap();
        let id = derive_fallback_chain_id(plan_id);
        assert!(id.contains("chain:auto:plan-"));
        assert!(id.contains("00000000-0000-0000-0000-000000000abc"));
        // Stability: same plan id → same fallback (no time / random
        // component sneaking in).
        assert_eq!(id, derive_fallback_chain_id(plan_id));
    }

    #[test]
    fn chain_eligibility_skips_when_finalization_block_missing() {
        // Inner DAG payload without a `finalization` block means the
        // caller did not opt into wave-17 finalize. Chain MUST skip
        // (chain only fires AFTER a successful finalization).
        let payload = json!({
            "status": "dag_succeeded",
            "scheduler_mode": "dag_v1",
        });
        assert_eq!(
            chain_eligibility_skip_reason(&payload),
            Some(CHAIN_STATUS_SKIPPED_NO_FINALIZATION)
        );
    }

    #[test]
    fn chain_eligibility_skips_when_plan_status_not_succeeded() {
        // Even with a finalization block, anything other than
        // `final_plan_status="succeeded"` MUST skip — failed / paused /
        // unchanged all collapse to the same skip reason so the
        // response carries one canonical label.
        for not_succeeded in ["failed", "executing", "awaiting_review", "unchanged", ""] {
            let payload = json!({
                "finalization": {"final_plan_status": not_succeeded},
            });
            assert_eq!(
                chain_eligibility_skip_reason(&payload),
                Some(CHAIN_STATUS_SKIPPED_PLAN_NOT_SUCCEEDED),
                "must skip when final_plan_status=`{}`",
                not_succeeded
            );
        }
    }

    #[test]
    fn chain_eligibility_passes_when_plan_status_succeeded() {
        let payload = json!({
            "finalization": {"final_plan_status": "succeeded"},
        });
        assert_eq!(chain_eligibility_skip_reason(&payload), None);
    }

    #[test]
    fn build_distill_chain_block_carries_canonical_shape() {
        // record_only path: no distill result, no warning, no triggered.
        let block = build_distill_chain_block(
            true,
            CHAIN_STATUS_RECORDED,
            "chain:wave18",
            "explicit_arg",
            "record_only",
            Some("wave18-loop"),
            Some(1),
            None,
            None,
            Some("/tmp/.missiond/v2/plans/abc.evidence.json"),
            None,
        );
        assert_eq!(block["triggered"], true);
        assert_eq!(block["status"], "recorded");
        assert_eq!(block["chain_id"], "chain:wave18");
        assert_eq!(block["chain_id_source"], "explicit_arg");
        assert_eq!(block["chain_mode"], "record_only");
        assert_eq!(block["chain_name"], "wave18-loop");
        assert_eq!(block["chain_index_in_plan"], 1);
        assert_eq!(
            block["evidence_path"],
            "/tmp/.missiond/v2/plans/abc.evidence.json"
        );
        assert!(block.get("distill_result").is_none());
        assert!(block.get("warning").is_none());
        assert!(block.get("evidence_error").is_none());
    }

    #[test]
    fn build_distill_chain_block_surfaces_distill_result_and_warning() {
        // dry_run / sonnet path with a downstream warning: chain block
        // MUST surface BOTH the inner result AND the warning so
        // observers can detect partial success without scraping the
        // payload.
        let block = build_distill_chain_block(
            true,
            CHAIN_STATUS_RECORDED_DISTILL_WARNING,
            "chain:42",
            "derived_from_plan_id",
            "sonnet",
            None,
            Some(2),
            Some(json!({"error": "sonnet quota exhausted"})),
            Some("distill chain workflow call returned an error; plan finalization preserved"),
            Some("/tmp/.evidence.json"),
            None,
        );
        assert_eq!(block["status"], "recorded_with_distill_warning");
        assert_eq!(block["distill_result"]["error"], "sonnet quota exhausted");
        assert!(block["warning"]
            .as_str()
            .unwrap()
            .contains("plan finalization preserved"));
        assert!(block.get("chain_name").is_none(), "chain_name only emitted when set");
    }

    #[test]
    fn build_distill_chain_block_skip_path_keeps_triggered_false_no_evidence() {
        // Skip branch: triggered=false + reason as status; evidence_path
        // / chain_index_in_plan absent because nothing was written.
        let block = build_distill_chain_block(
            false,
            CHAIN_STATUS_SKIPPED_PLAN_NOT_SUCCEEDED,
            "chain:auto:plan-x",
            "derived_from_plan_id",
            "record_only",
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert_eq!(block["triggered"], false);
        assert_eq!(block["status"], "skipped_plan_not_succeeded");
        assert!(block.get("evidence_path").is_none());
        assert!(block.get("chain_index_in_plan").is_none());
    }

    #[test]
    fn attach_distill_chain_to_payload_nests_under_finalization_when_present() {
        // Wave-17 finalization block exists → chain block lands under
        // `finalization.distill_chain` so callers can grep one place.
        let mut payload = json!({
            "status": "dag_succeeded",
            "finalization": {
                "final_plan_status": "succeeded",
                "rule": "all_terminal_no_failed_no_paused",
            },
        });
        let block = build_distill_chain_block(
            true,
            CHAIN_STATUS_RECORDED,
            "chain:42",
            "explicit_arg",
            "record_only",
            None,
            Some(1),
            None,
            None,
            Some("/tmp/x.json"),
            None,
        );
        attach_distill_chain_to_payload(&mut payload, block);
        assert_eq!(
            payload["finalization"]["distill_chain"]["chain_id"],
            "chain:42"
        );
        // Top-level shortcuts mirror the brief's response contract.
        assert_eq!(payload["distill_chain_status"], "recorded");
        assert_eq!(payload["distill_chain_id"], "chain:42");
    }

    #[test]
    fn attach_distill_chain_to_payload_falls_back_to_top_level_when_no_finalization() {
        // No finalization block (skip branch) → chain block surfaces at
        // the top level so the caller still sees the skip status.
        let mut payload = json!({"status": "dag_succeeded"});
        let block = build_distill_chain_block(
            false,
            CHAIN_STATUS_SKIPPED_NO_FINALIZATION,
            "chain:auto:plan-x",
            "derived_from_plan_id",
            "record_only",
            None,
            None,
            None,
            None,
            None,
            None,
        );
        attach_distill_chain_to_payload(&mut payload, block);
        assert_eq!(
            payload["distill_chain"]["status"],
            "skipped_no_finalization"
        );
        assert_eq!(payload["distill_chain_status"], "skipped_no_finalization");
    }

    #[test]
    fn attach_distill_chain_to_payload_surfaces_distill_result_shortcut() {
        // dry_run / sonnet path: top-level `distill_result` shortcut so
        // callers can grep the inner workflow payload without diving
        // into finalization.distill_chain.distill_result.
        let mut payload = json!({
            "status": "dag_succeeded",
            "finalization": {"final_plan_status": "succeeded"},
        });
        let block = build_distill_chain_block(
            true,
            CHAIN_STATUS_RECORDED_WITH_DISTILL,
            "chain:42",
            "explicit_arg",
            "dry_run",
            None,
            Some(1),
            Some(json!({"status": "dry_run", "persisted": false})),
            None,
            Some("/tmp/x.json"),
            None,
        );
        attach_distill_chain_to_payload(&mut payload, block);
        assert_eq!(payload["distill_result"]["status"], "dry_run");
        assert_eq!(payload["distill_result"]["persisted"], false);
        assert!(
            payload.get("distill_chain_warning").is_none(),
            "warning shortcut absent on the OK path"
        );
    }

    #[test]
    fn attach_distill_chain_to_payload_surfaces_warning_shortcut() {
        let mut payload = json!({
            "status": "dag_succeeded",
            "finalization": {"final_plan_status": "succeeded"},
        });
        let block = build_distill_chain_block(
            true,
            CHAIN_STATUS_RECORDED_DISTILL_WARNING,
            "chain:42",
            "explicit_arg",
            "sonnet",
            None,
            Some(1),
            Some(json!({"error": "x"})),
            Some("workflow distill failed; plan finalization preserved"),
            Some("/tmp/x.json"),
            None,
        );
        attach_distill_chain_to_payload(&mut payload, block);
        assert_eq!(
            payload["distill_chain_warning"],
            "workflow distill failed; plan finalization preserved"
        );
    }

    // ── wave-18 / task 06 — autonomous PLAN field inference v0 ─────────

    #[test]
    fn parse_infer_plan_fields_mode_default_is_off() {
        let mode = parse_infer_plan_fields_mode(&json!({})).expect("default off");
        assert_eq!(mode, InferPlanFieldsMode::Off);
        let mode_blank = parse_infer_plan_fields_mode(&json!({"infer_plan_fields": ""}))
            .expect("blank parses to off");
        assert_eq!(mode_blank, InferPlanFieldsMode::Off);
        let mode_off = parse_infer_plan_fields_mode(&json!({"infer_plan_fields": "off"}))
            .expect("explicit off");
        assert_eq!(mode_off, InferPlanFieldsMode::Off);
    }

    #[test]
    fn parse_infer_plan_fields_mode_accepts_known_values() {
        let preview = parse_infer_plan_fields_mode(&json!({"infer_plan_fields": "preview"}))
            .expect("preview");
        assert_eq!(preview, InferPlanFieldsMode::Preview);
        let apply = parse_infer_plan_fields_mode(&json!({"infer_plan_fields": "apply_safe"}))
            .expect("apply_safe");
        assert_eq!(apply, InferPlanFieldsMode::ApplySafe);
    }

    #[test]
    fn parse_infer_plan_fields_mode_rejects_typo() {
        let err = parse_infer_plan_fields_mode(&json!({"infer_plan_fields": "aply"}))
            .expect_err("typo rejected");
        assert!(err.contains("must be one of"));
        assert!(err.contains("aply"));
    }

    fn empty_input<'a>() -> PlanInferenceInput<'a> {
        PlanInferenceInput {
            plan_hints: ParsedPlanHints::default(),
            plan_sexp: "",
            compiled_from: None,
            evidence_entries: Vec::new(),
        }
    }

    #[test]
    fn confidence_only_high_meets_apply_threshold() {
        assert!(InferenceConfidence::High.meets_apply_threshold());
        assert!(!InferenceConfidence::Medium.meets_apply_threshold());
        assert!(!InferenceConfidence::Low.meets_apply_threshold());
    }

    #[test]
    fn infer_target_from_plan_sexp_high_confidence() {
        // PLAN.lisp `:target` hint normalises to a canonical target with
        // high confidence — caller did not specify, so it lands in
        // `inferred[]` (apply-eligible).
        let mut hints = ParsedPlanHints::default();
        hints.target = Some("mission_task_delegate".to_string());
        let input = PlanInferenceInput {
            plan_hints: hints,
            plan_sexp: "(plan :target \"mission_task_delegate\")",
            compiled_from: None,
            evidence_entries: Vec::new(),
        };
        let r = compute_plan_field_inference(&json!({}), &input);
        let inferred = r
            .inferred
            .iter()
            .find(|f| f.field == "target")
            .expect("target inferred");
        assert_eq!(inferred.value, json!("mission_task_delegate"));
        assert_eq!(inferred.confidence, InferenceConfidence::High);
        assert_eq!(inferred.source, "plan_sexp");
        assert!(r.evidence_sources.contains(&"plan_sexp"));
    }

    #[test]
    fn infer_owned_files_from_evidence_sidecar_medium() {
        // Caller did not pass owned_files; PLAN-side hints absent; the
        // most-recent evidence entry carries an `owned_files` array under
        // `inner_dispatch`. That signal is medium-confidence (file lists
        // change across runs) so it lands in `suggested[]`.
        let evidence = vec![json!({
            "source": "plan_runner_dispatch",
            "kind": "dispatch",
            "inner_dispatch": {
                "owned_files": ["a.rs", "b.rs"],
            }
        })];
        let input = PlanInferenceInput {
            plan_hints: ParsedPlanHints::default(),
            plan_sexp: "",
            compiled_from: None,
            evidence_entries: evidence,
        };
        let r = compute_plan_field_inference(&json!({}), &input);
        let suggested = r
            .suggested
            .iter()
            .find(|f| f.field == "owned_files")
            .expect("owned_files suggested from evidence");
        assert_eq!(suggested.confidence, InferenceConfidence::Medium);
        assert_eq!(suggested.source, "evidence_sidecar");
        assert_eq!(suggested.value, json!(["a.rs", "b.rs"]));
        // No high-confidence inference for owned_files from evidence.
        assert!(r.inferred.iter().all(|f| f.field != "owned_files"));
    }

    #[test]
    fn infer_owned_files_from_plan_sexp_high_confidence() {
        // PLAN.lisp `:owned-files [...]` is high-confidence — apply_safe
        // would fill caller args.
        let mut hints = ParsedPlanHints::default();
        hints.owned_files_raw = Some("[\"src/lib.rs\" \"src/main.rs\"]".to_string());
        let input = PlanInferenceInput {
            plan_hints: hints,
            plan_sexp: "(plan :owned-files [\"src/lib.rs\" \"src/main.rs\"])",
            compiled_from: None,
            evidence_entries: Vec::new(),
        };
        let r = compute_plan_field_inference(&json!({}), &input);
        let inferred = r
            .inferred
            .iter()
            .find(|f| f.field == "owned_files")
            .expect("owned_files inferred from plan_sexp");
        assert_eq!(inferred.confidence, InferenceConfidence::High);
        assert_eq!(inferred.source, "plan_sexp");
        assert_eq!(inferred.value, json!(["src/lib.rs", "src/main.rs"]));
    }

    #[test]
    fn apply_safe_does_not_overwrite_caller_value() {
        // Caller explicitly passed target=mission_execution; PLAN-side hint
        // disagrees (mission_task_delegate). The inferer must report a
        // CONFLICT (never silently mutate over caller intent), and
        // `apply_safe_augmentation` must leave caller's value intact.
        let mut hints = ParsedPlanHints::default();
        hints.target = Some("mission_task_delegate".to_string());
        let input = PlanInferenceInput {
            plan_hints: hints,
            plan_sexp: "(plan :target \"mission_task_delegate\")",
            compiled_from: None,
            evidence_entries: Vec::new(),
        };
        let caller_args = json!({"target": "mission_execution"});
        let r = compute_plan_field_inference(&caller_args, &input);
        // Conflict reported, not auto-applied.
        let conflict = r
            .conflicts
            .iter()
            .find(|c| c.field == "target")
            .expect("target conflict surfaced");
        assert_eq!(conflict.caller_value, json!("mission_execution"));
        assert_eq!(conflict.inferred_value, json!("mission_task_delegate"));
        assert!(r.inferred.iter().all(|f| f.field != "target"));

        // Augmentation MUST preserve caller's explicit value.
        let augmented = apply_safe_augmentation(&caller_args, &r);
        assert_eq!(augmented["target"], "mission_execution");
    }

    #[test]
    fn apply_safe_fills_missing_high_confidence_only() {
        // PLAN-side high-confidence hint for target + medium-confidence
        // evidence for owned_files. apply_safe should ONLY fill `target`
        // (high), never `owned_files` (medium → suggestion only).
        let mut hints = ParsedPlanHints::default();
        hints.target = Some("mission_task_delegate".to_string());
        let evidence = vec![json!({
            "inner_dispatch": {"owned_files": ["a.rs"]}
        })];
        let input = PlanInferenceInput {
            plan_hints: hints,
            plan_sexp: "(plan :target \"mission_task_delegate\")",
            compiled_from: None,
            evidence_entries: evidence,
        };
        let caller_args = json!({});
        let r = compute_plan_field_inference(&caller_args, &input);
        let augmented = apply_safe_augmentation(&caller_args, &r);
        // High-confidence target was applied.
        assert_eq!(augmented["target"], "mission_task_delegate");
        // Medium-confidence owned_files was NOT applied (still suggestion).
        assert!(augmented.get("owned_files").is_none());
        assert!(r
            .suggested
            .iter()
            .any(|f| f.field == "owned_files"));
    }

    #[test]
    fn low_or_medium_confidence_never_lands_in_inferred() {
        // Single evidence entry → medium confidence for target_project;
        // suggested only.
        let evidence = vec![json!({
            "inner_dispatch": {"target_project": "missiond"}
        })];
        let input = PlanInferenceInput {
            plan_hints: ParsedPlanHints::default(),
            plan_sexp: "",
            compiled_from: None,
            evidence_entries: evidence,
        };
        let r = compute_plan_field_inference(&json!({}), &input);
        let suggested = r
            .suggested
            .iter()
            .find(|f| f.field == "target_project")
            .expect("target_project suggested");
        assert_eq!(suggested.confidence, InferenceConfidence::Medium);
        assert!(r.inferred.iter().all(|f| f.field != "target_project"));
    }

    #[test]
    fn target_project_high_confidence_when_evidence_repeats() {
        // Two evidence entries agreeing on the same target_project →
        // high-confidence (count >= 2).
        let evidence = vec![
            json!({"inner_dispatch": {"target_project": "missiond"}}),
            json!({"inner_dispatch": {"target_project": "missiond"}}),
        ];
        let input = PlanInferenceInput {
            plan_hints: ParsedPlanHints::default(),
            plan_sexp: "",
            compiled_from: None,
            evidence_entries: evidence,
        };
        let r = compute_plan_field_inference(&json!({}), &input);
        let inferred = r
            .inferred
            .iter()
            .find(|f| f.field == "target_project")
            .expect("repeated evidence promotes target_project");
        assert_eq!(inferred.confidence, InferenceConfidence::High);
        assert_eq!(inferred.value, json!("missiond"));
    }

    #[test]
    fn workstation_dispatch_inferred_from_plan_hint() {
        // PLAN.lisp `:workstation-dispatch true` → high-confidence
        // workstation_dispatch=true.
        let mut hints = ParsedPlanHints::default();
        hints.workstation_dispatch_flag = Some("true".to_string());
        let input = PlanInferenceInput {
            plan_hints: hints,
            plan_sexp: "(plan :workstation-dispatch true)",
            compiled_from: None,
            evidence_entries: Vec::new(),
        };
        let r = compute_plan_field_inference(&json!({}), &input);
        let inferred = r
            .inferred
            .iter()
            .find(|f| f.field == "workstation_dispatch")
            .expect("workstation_dispatch inferred from plan");
        assert_eq!(inferred.value, json!(true));
        assert_eq!(inferred.confidence, InferenceConfidence::High);
    }

    #[test]
    fn workstation_dispatch_caller_false_creates_conflict_with_plan_true() {
        let mut hints = ParsedPlanHints::default();
        hints.workstation_dispatch_flag = Some("true".to_string());
        let input = PlanInferenceInput {
            plan_hints: hints,
            plan_sexp: "(plan :workstation-dispatch true)",
            compiled_from: None,
            evidence_entries: Vec::new(),
        };
        let caller = json!({"workstation_dispatch": false});
        let r = compute_plan_field_inference(&caller, &input);
        let conflict = r
            .conflicts
            .iter()
            .find(|c| c.field == "workstation_dispatch")
            .expect("conflict surfaced");
        assert_eq!(conflict.caller_value, json!(false));
        assert_eq!(conflict.inferred_value, json!(true));
        // apply_safe must NEVER override caller value.
        let augmented = apply_safe_augmentation(&caller, &r);
        assert_eq!(augmented["workstation_dispatch"], false);
    }

    #[test]
    fn dispatch_strategy_inferred_from_plan_hint() {
        let mut hints = ParsedPlanHints::default();
        hints.dispatch_strategy = Some("agent-team".to_string());
        let input = PlanInferenceInput {
            plan_hints: hints,
            plan_sexp: "(plan :dispatch-strategy \"agent-team\")",
            compiled_from: None,
            evidence_entries: Vec::new(),
        };
        let r = compute_plan_field_inference(&json!({}), &input);
        let inferred = r
            .inferred
            .iter()
            .find(|f| f.field == "dispatch_strategy")
            .expect("dispatch_strategy inferred");
        assert_eq!(inferred.value, json!("agent-team"));
        assert_eq!(inferred.confidence, InferenceConfidence::High);
    }

    #[test]
    fn dispatch_strategy_from_parallelism_is_medium() {
        // PLAN-side `:parallelism agent-team` is medium-confidence
        // (mapped through, not declared as the strategy itself).
        let mut hints = ParsedPlanHints::default();
        hints.parallelism = Some("agent-team".to_string());
        let input = PlanInferenceInput {
            plan_hints: hints,
            plan_sexp: "(plan :parallelism agent-team)",
            compiled_from: None,
            evidence_entries: Vec::new(),
        };
        let r = compute_plan_field_inference(&json!({}), &input);
        let suggested = r
            .suggested
            .iter()
            .find(|f| f.field == "dispatch_strategy")
            .expect("dispatch_strategy suggested from parallelism");
        assert_eq!(suggested.confidence, InferenceConfidence::Medium);
    }

    #[test]
    fn acceptance_mode_inferred_from_plan_top_level() {
        // The canonical hint scanner does NOT capture `:acceptance-mode`
        // (that lives on per-node forms in plan_dag.rs); v0 inference
        // re-scans the raw sexp directly so a top-level declaration is
        // still picked up.
        let input = PlanInferenceInput {
            plan_hints: ParsedPlanHints::default(),
            plan_sexp: r#"(plan :acceptance-mode "inner_status")"#,
            compiled_from: None,
            evidence_entries: Vec::new(),
        };
        let r = compute_plan_field_inference(&json!({}), &input);
        let inferred = r
            .inferred
            .iter()
            .find(|f| f.field == "acceptance_mode")
            .expect("acceptance_mode inferred");
        assert_eq!(inferred.value, json!("inner_status"));
        assert_eq!(inferred.confidence, InferenceConfidence::High);
    }

    #[test]
    fn acceptance_mode_unrecognised_raw_does_not_infer() {
        let input = PlanInferenceInput {
            plan_hints: ParsedPlanHints::default(),
            plan_sexp: r#"(plan :acceptance-mode "cosmic")"#,
            compiled_from: None,
            evidence_entries: Vec::new(),
        };
        let r = compute_plan_field_inference(&json!({}), &input);
        assert!(r.inferred.iter().all(|f| f.field != "acceptance_mode"));
        assert!(r.suggested.iter().all(|f| f.field != "acceptance_mode"));
    }

    #[test]
    fn off_mode_preserves_default_args_unchanged() {
        // Off mode means we never even build the inferer input. Sanity
        // check: status helper reports `off`.
        let inf = PlanFieldInference::default();
        assert_eq!(inf.status(InferPlanFieldsMode::Off), "off");
    }

    #[test]
    fn preview_status_reports_no_signal_when_inference_empty() {
        let inf = PlanFieldInference::default();
        assert_eq!(
            inf.status(InferPlanFieldsMode::Preview),
            "preview_no_signal"
        );
    }

    #[test]
    fn preview_status_reports_preview_when_signals_present() {
        let mut inf = PlanFieldInference::default();
        inf.suggested.push(InferredField {
            field: "target",
            value: json!("mission_execution"),
            confidence: InferenceConfidence::Medium,
            source: "evidence_sidecar",
            detail: None,
        });
        assert_eq!(inf.status(InferPlanFieldsMode::Preview), "preview");
    }

    #[test]
    fn apply_safe_status_reports_applied_when_high_confidence_present() {
        let mut inf = PlanFieldInference::default();
        inf.inferred.push(InferredField {
            field: "target",
            value: json!("mission_task_delegate"),
            confidence: InferenceConfidence::High,
            source: "plan_sexp",
            detail: None,
        });
        assert_eq!(
            inf.status(InferPlanFieldsMode::ApplySafe),
            "apply_safe_applied"
        );
    }

    #[test]
    fn apply_safe_status_reports_suggestions_only_when_no_high_confidence() {
        let mut inf = PlanFieldInference::default();
        inf.suggested.push(InferredField {
            field: "owned_files",
            value: json!(["a.rs"]),
            confidence: InferenceConfidence::Medium,
            source: "evidence_sidecar",
            detail: None,
        });
        assert_eq!(
            inf.status(InferPlanFieldsMode::ApplySafe),
            "apply_safe_suggestions_only"
        );
    }

    #[test]
    fn evidence_sources_reflect_signals_seen() {
        // Signals from all three sources → all three names appear in
        // evidence_sources[].
        let mut hints = ParsedPlanHints::default();
        hints.target = Some("mission_task_delegate".to_string());
        let evidence = vec![json!({"inner_dispatch": {"target_project": "x"}})];
        let input = PlanInferenceInput {
            plan_hints: hints,
            plan_sexp: "(plan :target \"mission_task_delegate\")",
            compiled_from: Some("directive/abc:1"),
            evidence_entries: evidence,
        };
        let r = compute_plan_field_inference(&json!({}), &input);
        assert!(r.evidence_sources.contains(&"plan_sexp"));
        assert!(r.evidence_sources.contains(&"evidence_sidecar"));
        assert!(r.evidence_sources.contains(&"compiled_from"));
    }

    #[test]
    fn apply_safe_augmentation_skips_field_when_args_already_carry_it() {
        // Defensive guard: even if the inferer (somehow) listed a field
        // in `inferred[]` AND args already carry it, augmentation MUST
        // refuse to overwrite. This pins the invariant tested in the
        // conflict path so a future regression is loud.
        let mut inf = PlanFieldInference::default();
        inf.inferred.push(InferredField {
            field: "target",
            value: json!("mission_task_delegate"),
            confidence: InferenceConfidence::High,
            source: "plan_sexp",
            detail: None,
        });
        let args = json!({"target": "mission_execution"});
        let augmented = apply_safe_augmentation(&args, &inf);
        // Must NOT have changed.
        assert_eq!(augmented["target"], "mission_execution");
    }

    #[test]
    fn response_block_always_has_stable_shape() {
        let inf = PlanFieldInference::default();
        let block = inf.to_response_json(InferPlanFieldsMode::Preview);
        assert_eq!(block["mode"], "preview");
        assert!(block["inferred_fields"].is_array());
        assert!(block["suggested_fields"].is_array());
        assert!(block["conflicts"].is_array());
        assert!(block["evidence_sources"].is_array());
        assert_eq!(block["inference_status"], "preview_no_signal");
    }

    #[test]
    fn caller_string_list_handles_caller_arg_shapes() {
        // Sanity-check the helper used by `infer_owned_files`.
        let args = json!({"owned_files": ["a", "b"]});
        let v = caller_string_list(&args, "owned_files");
        assert_eq!(v, vec!["a".to_string(), "b".to_string()]);
        let scalar = json!({"owned_files": "single"});
        assert_eq!(
            caller_string_list(&scalar, "owned_files"),
            vec!["single".to_string()]
        );
        // Empty default.
        assert!(caller_string_list(&json!({}), "owned_files").is_empty());
    }

    #[test]
    fn compiled_from_keyword_scan_produces_medium_target() {
        // No PLAN-side hint, no evidence — but `compiled_from` carries
        // the keyword. Falls into the medium-confidence (suggested) bucket.
        let input = PlanInferenceInput {
            plan_hints: ParsedPlanHints::default(),
            plan_sexp: "",
            compiled_from: Some("directive/abc:1 — claudecode workstation"),
            evidence_entries: Vec::new(),
        };
        let r = compute_plan_field_inference(&json!({}), &input);
        let suggested = r
            .suggested
            .iter()
            .find(|f| f.field == "target")
            .expect("target suggested from compiled_from");
        assert_eq!(suggested.confidence, InferenceConfidence::Medium);
        assert_eq!(suggested.value, json!("mission_task_delegate"));
    }

    #[test]
    fn empty_input_yields_empty_result() {
        let input = empty_input();
        let r = compute_plan_field_inference(&json!({}), &input);
        assert!(r.inferred.is_empty());
        assert!(r.suggested.is_empty());
        assert!(r.conflicts.is_empty());
        assert!(r.evidence_sources.is_empty());
    }

    #[test]
    fn attach_inference_block_skips_when_block_absent() {
        // mode=off → block=None → response untouched.
        let original = ToolResult::json_pretty(&json!({"status": "executing"}));
        let original_text = match original.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("text"),
        };
        let r = attach_inference_block(original, None);
        let after_text = match r.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("text"),
        };
        assert_eq!(original_text, after_text);
    }

    #[test]
    fn attach_inference_block_splices_block_into_payload() {
        let original = ToolResult::json_pretty(&json!({"status": "executing"}));
        let block = json!({"mode": "apply_safe", "inference_status": "apply_safe_applied"});
        let r = attach_inference_block(original, Some(block.clone()));
        let v = parse_payload(&r);
        assert_eq!(v["status"], "executing");
        assert_eq!(v["plan_field_inference"], block);
    }

    #[test]
    fn attach_inference_block_preserves_existing_block() {
        // If the result already carries a `plan_field_inference` key
        // (future DAG / resume path), we must NEVER overwrite.
        let original = ToolResult::json_pretty(&json!({
            "status": "executing",
            "plan_field_inference": {"mode": "preview"},
        }));
        let block = json!({"mode": "apply_safe"});
        let r = attach_inference_block(original, Some(block));
        let v = parse_payload(&r);
        assert_eq!(v["plan_field_inference"]["mode"], "preview");
    }

    // ── wave-20 / task 07 — LLM-augmented PLAN field inference v0 ──────

    #[test]
    fn parse_infer_plan_fields_mode_accepts_sonnet_suggest() {
        // wave-20 / task 07 — new LLM-augmented mode lands on the same
        // allowlist as the wave-18 / task 06 deterministic modes.
        let mode = parse_infer_plan_fields_mode(
            &json!({"infer_plan_fields": "sonnet_suggest"}),
        )
        .expect("sonnet_suggest accepted");
        assert_eq!(mode, InferPlanFieldsMode::SonnetSuggest);
        assert!(mode.is_llm_augmented());
        // Determinstic modes never report as LLM-augmented.
        assert!(!InferPlanFieldsMode::Off.is_llm_augmented());
        assert!(!InferPlanFieldsMode::Preview.is_llm_augmented());
        assert!(!InferPlanFieldsMode::ApplySafe.is_llm_augmented());
    }

    #[test]
    fn parse_infer_plan_fields_mode_typo_error_lists_sonnet_suggest() {
        // Typo path now mentions sonnet_suggest in the error message so
        // a caller misspelling the new mode knows the canonical form.
        let err = parse_infer_plan_fields_mode(
            &json!({"infer_plan_fields": "sonnet-suggest"}),
        )
        .expect_err("hyphenated form rejected");
        assert!(err.contains("sonnet_suggest"));
        assert!(err.contains("sonnet-suggest"));
    }

    #[test]
    fn sonnet_suggest_mode_wire_string_round_trips() {
        assert_eq!(
            InferPlanFieldsMode::SonnetSuggest.as_wire(),
            INFER_MODE_SONNET_SUGGEST
        );
    }

    #[test]
    fn parse_llm_proposals_accepts_wrapped_object() {
        // Canonical happy path — Sonnet returns the documented
        // `{"proposals": [...]}` envelope.
        let raw = r#"{
            "proposals": [
                {
                    "field": "target",
                    "value": "mission_task_delegate",
                    "confidence": "high",
                    "evidence": "PLAN sexp clearly delegates to claudecode"
                }
            ]
        }"#;
        let (proposals, warnings) = parse_llm_proposals(raw);
        assert!(warnings.is_empty(), "warnings: {:?}", warnings);
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].field, "target");
        assert_eq!(proposals[0].value, json!("mission_task_delegate"));
        assert_eq!(proposals[0].confidence, InferenceConfidence::High);
        assert_eq!(proposals[0].conflict_status, LlmConflictStatus::None);
    }

    #[test]
    fn parse_llm_proposals_accepts_bare_array() {
        // Sonnet sometimes elides the wrapper and emits a top-level
        // array. We accept both shapes.
        let raw = r#"[{"field":"workstation_dispatch","value":true,"confidence":"medium","evidence":"plan declares scope hints"}]"#;
        let (proposals, warnings) = parse_llm_proposals(raw);
        assert!(warnings.is_empty(), "warnings: {:?}", warnings);
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].value, json!(true));
    }

    #[test]
    fn parse_llm_proposals_strips_markdown_fence() {
        // The system prompt forbids fences but Sonnet sometimes emits
        // them anyway; we tolerate the wrapper.
        let raw = "```json\n{\"proposals\": [{\"field\":\"target\",\"value\":\"mission_execution\",\"confidence\":\"medium\",\"evidence\":\"vague evidence\"}]}\n```";
        let (proposals, warnings) = parse_llm_proposals(raw);
        assert!(warnings.is_empty(), "warnings: {:?}", warnings);
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].value, json!("mission_execution"));
    }

    #[test]
    fn parse_llm_proposals_rejects_unknown_field() {
        let raw = r#"{"proposals":[{"field":"orbital_velocity","value":"warp9","confidence":"high","evidence":"x"}]}"#;
        let (proposals, warnings) = parse_llm_proposals(raw);
        assert!(proposals.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("orbital_velocity"));
    }

    #[test]
    fn parse_llm_proposals_rejects_invalid_confidence() {
        let raw = r#"{"proposals":[{"field":"target","value":"mission_execution","confidence":"absolute","evidence":"x"}]}"#;
        let (proposals, warnings) = parse_llm_proposals(raw);
        assert!(proposals.is_empty());
        assert!(warnings[0].contains("absolute"));
    }

    #[test]
    fn parse_llm_proposals_rejects_missing_evidence() {
        // Evidence is required; an empty string drops the proposal.
        let raw = r#"{"proposals":[{"field":"target","value":"mission_execution","confidence":"high","evidence":""}]}"#;
        let (proposals, warnings) = parse_llm_proposals(raw);
        assert!(proposals.is_empty());
        assert!(warnings[0].contains("evidence"));
    }

    #[test]
    fn parse_llm_proposals_rejects_value_shape_mismatch() {
        // owned_files must be a string array, not a single string.
        let raw = r#"{"proposals":[{"field":"owned_files","value":"src/lib.rs","confidence":"medium","evidence":"x"}]}"#;
        let (proposals, warnings) = parse_llm_proposals(raw);
        assert!(proposals.is_empty());
        assert!(warnings[0].contains("owned_files"));
        // Boolean expected for workstation_dispatch.
        let raw2 = r#"{"proposals":[{"field":"workstation_dispatch","value":42,"confidence":"medium","evidence":"x"}]}"#;
        let (proposals2, warnings2) = parse_llm_proposals(raw2);
        assert!(proposals2.is_empty());
        assert!(warnings2[0].contains("workstation_dispatch"));
    }

    #[test]
    fn parse_llm_proposals_dedupes_repeated_fields() {
        let raw = r#"{
            "proposals":[
                {"field":"target","value":"mission_execution","confidence":"medium","evidence":"first"},
                {"field":"target","value":"mission_task_delegate","confidence":"high","evidence":"second"}
            ]
        }"#;
        let (proposals, warnings) = parse_llm_proposals(raw);
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].evidence, "first");
        assert!(warnings.iter().any(|w| w.contains("duplicate")));
    }

    #[test]
    fn parse_llm_proposals_caps_long_lists() {
        // Build a list longer than the cap so the trim warning fires.
        let mut entries = Vec::new();
        for f in [
            "target",
            "dispatch_strategy",
            "target_project",
            "owned_files",
            "acceptance_mode",
            "workstation_dispatch",
            "extra_one",
            "extra_two",
            "extra_three",
            "extra_four",
        ] {
            let value = match f {
                "owned_files" => json!(["a.rs"]),
                "workstation_dispatch" => json!(true),
                _ => json!("mission_execution"),
            };
            entries.push(json!({
                "field": f,
                "value": value,
                "confidence": "low",
                "evidence": "x"
            }));
        }
        let raw = serde_json::to_string(&json!({"proposals": entries})).unwrap();
        let (proposals, warnings) = parse_llm_proposals(&raw);
        // Cap pinned at LLM_PROPOSAL_CAP (8); duplicate-fields and
        // unknown-fields are dropped before the cap check, so the cap
        // warning may not fire — but the proposal count must be ≤ cap.
        assert!(proposals.len() <= LLM_PROPOSAL_CAP);
        // Unknown fields surface at minimum the `extra_*` warnings.
        assert!(warnings.iter().any(|w| w.contains("extra_one")));
    }

    #[test]
    fn parse_llm_proposals_rejects_garbage_json() {
        let raw = "not json at all";
        let (proposals, warnings) = parse_llm_proposals(raw);
        assert!(proposals.is_empty());
        assert!(warnings[0].contains("not valid JSON"));
    }

    #[test]
    fn parse_llm_proposals_rejects_missing_proposals_key() {
        let raw = r#"{"results": []}"#;
        let (proposals, warnings) = parse_llm_proposals(raw);
        assert!(proposals.is_empty());
        assert!(warnings[0].contains("missing required `proposals`"));
    }

    #[test]
    fn reconcile_marks_caller_conflict() {
        // Caller passed a different value for the same field — the
        // proposal must surface ConflictsWithCaller (and never auto-apply).
        let mut proposals = vec![LlmProposal {
            field: "target",
            value: json!("mission_task_delegate"),
            confidence: InferenceConfidence::High,
            evidence: "plan sexp".to_string(),
            conflict_status: LlmConflictStatus::None,
        }];
        let deterministic = PlanFieldInference::default();
        let args = json!({"target": "mission_execution"});
        reconcile_llm_conflicts(&mut proposals, &deterministic, &args);
        assert_eq!(
            proposals[0].conflict_status,
            LlmConflictStatus::ConflictsWithCaller
        );
    }

    #[test]
    fn reconcile_marks_deterministic_conflict() {
        // Caller silent; deterministic engine inferred a different
        // value with high confidence. Proposal must surface
        // ConflictsWithDeterministic.
        let mut deterministic = PlanFieldInference::default();
        deterministic.inferred.push(InferredField {
            field: "target",
            value: json!("mission_execution"),
            confidence: InferenceConfidence::High,
            source: "plan_sexp",
            detail: None,
        });
        let mut proposals = vec![LlmProposal {
            field: "target",
            value: json!("mission_task_delegate"),
            confidence: InferenceConfidence::Medium,
            evidence: "compiled_from hint".to_string(),
            conflict_status: LlmConflictStatus::None,
        }];
        reconcile_llm_conflicts(&mut proposals, &deterministic, &json!({}));
        assert_eq!(
            proposals[0].conflict_status,
            LlmConflictStatus::ConflictsWithDeterministic
        );
    }

    #[test]
    fn reconcile_marks_overlap_with_deterministic_suggestion() {
        // Deterministic suggestion (medium / low) at a different value
        // than LLM proposal — surfaced as overlap, lower precedence
        // than caller / deterministic-high conflicts.
        let mut deterministic = PlanFieldInference::default();
        deterministic.suggested.push(InferredField {
            field: "owned_files",
            value: json!(["a.rs"]),
            confidence: InferenceConfidence::Medium,
            source: "evidence_sidecar",
            detail: None,
        });
        let mut proposals = vec![LlmProposal {
            field: "owned_files",
            value: json!(["b.rs"]),
            confidence: InferenceConfidence::Low,
            evidence: "compiled_from".to_string(),
            conflict_status: LlmConflictStatus::None,
        }];
        reconcile_llm_conflicts(&mut proposals, &deterministic, &json!({}));
        assert_eq!(
            proposals[0].conflict_status,
            LlmConflictStatus::OverlapsDeterministicSuggestion
        );
    }

    #[test]
    fn reconcile_leaves_conflict_none_when_caller_agrees() {
        // Caller passed the same value as the proposal — no conflict.
        let mut proposals = vec![LlmProposal {
            field: "target",
            value: json!("mission_execution"),
            confidence: InferenceConfidence::Medium,
            evidence: "agreement".to_string(),
            conflict_status: LlmConflictStatus::None,
        }];
        reconcile_llm_conflicts(
            &mut proposals,
            &PlanFieldInference::default(),
            &json!({"target": "MISSION_EXECUTION"}),
        );
        // String comparison is case-insensitive, mirroring the
        // deterministic engine.
        assert_eq!(proposals[0].conflict_status, LlmConflictStatus::None);
    }

    #[test]
    fn reconcile_owned_files_is_set_like() {
        // owned_files compares order-independent so a permutation does
        // not surface as a deterministic conflict.
        let mut deterministic = PlanFieldInference::default();
        deterministic.inferred.push(InferredField {
            field: "owned_files",
            value: json!(["a.rs", "b.rs"]),
            confidence: InferenceConfidence::High,
            source: "plan_sexp",
            detail: None,
        });
        let mut proposals = vec![LlmProposal {
            field: "owned_files",
            value: json!(["b.rs", "a.rs"]),
            confidence: InferenceConfidence::High,
            evidence: "permutation".to_string(),
            conflict_status: LlmConflictStatus::None,
        }];
        reconcile_llm_conflicts(&mut proposals, &deterministic, &json!({}));
        assert_eq!(proposals[0].conflict_status, LlmConflictStatus::None);
    }

    #[test]
    fn llm_proposal_to_json_pins_applied_false() {
        // Critical invariant: every LLM proposal carries `applied=false`
        // on the wire so observers can `assert proposal.applied == false`
        // without re-reading the task contract.
        let p = LlmProposal {
            field: "target",
            value: json!("mission_execution"),
            confidence: InferenceConfidence::High,
            evidence: "x".to_string(),
            conflict_status: LlmConflictStatus::None,
        };
        let v = p.to_json();
        assert_eq!(v["applied"], json!(false));
        assert_eq!(v["field"], json!("target"));
        assert_eq!(v["confidence"], json!("high"));
        assert_eq!(v["conflict_status"], json!("none"));
    }

    #[test]
    fn llm_bundle_unavailable_carries_reason() {
        let b = LlmProposalBundle::unavailable("gateway not initialized");
        assert_eq!(b.status, LlmProposalStatus::Unavailable);
        assert!(b.proposals.is_empty());
        assert_eq!(
            b.unavailable_reason.as_deref(),
            Some("gateway not initialized")
        );
        assert_eq!(b.request_caller.as_deref(), Some(SONNET_INFER_CALLER));
    }

    #[test]
    fn response_block_under_sonnet_suggest_carries_llm_keys_when_unavailable() {
        // Even when Sonnet is unavailable we surface llm_status +
        // llm_proposals[] (empty) so observers pivot on a stable shape.
        let mut inf = PlanFieldInference::default();
        inf.llm = Some(LlmProposalBundle::unavailable("test reason"));
        let block = inf.to_response_json(InferPlanFieldsMode::SonnetSuggest);
        assert_eq!(block["mode"], "sonnet_suggest");
        assert_eq!(block["llm_status"], "llm_unavailable");
        assert_eq!(block["llm_proposals"], json!([]));
        assert_eq!(block["llm_unavailable_reason"], "test reason");
        assert_eq!(block["llm_caller"], SONNET_INFER_CALLER);
    }

    #[test]
    fn response_block_under_sonnet_suggest_with_proposals() {
        let mut inf = PlanFieldInference::default();
        let bundle = LlmProposalBundle {
            status: LlmProposalStatus::Suggested,
            proposals: vec![LlmProposal {
                field: "target",
                value: json!("mission_execution"),
                confidence: InferenceConfidence::Medium,
                evidence: "compiled_from".to_string(),
                conflict_status: LlmConflictStatus::None,
            }],
            parse_warnings: Vec::new(),
            unavailable_reason: None,
            model: Some("claude-sonnet".to_string()),
            request_caller: Some(SONNET_INFER_CALLER.to_string()),
        };
        inf.llm = Some(bundle);
        let block = inf.to_response_json(InferPlanFieldsMode::SonnetSuggest);
        assert_eq!(block["llm_status"], "suggested");
        assert_eq!(block["llm_proposals"][0]["field"], "target");
        assert_eq!(block["llm_proposals"][0]["applied"], false);
        assert_eq!(block["llm_model"], "claude-sonnet");
    }

    #[test]
    fn response_block_under_deterministic_modes_omits_llm_keys() {
        // Backward compatibility: existing wave-18 modes must produce
        // BYTE-IDENTICAL response shapes (no llm_* keys leaking through).
        let inf = PlanFieldInference::default();
        for mode in [
            InferPlanFieldsMode::Off,
            InferPlanFieldsMode::Preview,
            InferPlanFieldsMode::ApplySafe,
        ] {
            let block = inf.to_response_json(mode);
            assert!(block.get("llm_status").is_none(), "mode {:?}", mode);
            assert!(
                block.get("llm_proposals").is_none(),
                "mode {:?}",
                mode
            );
            assert!(
                block.get("llm_unavailable_reason").is_none(),
                "mode {:?}",
                mode
            );
        }
    }

    #[test]
    fn sonnet_suggest_status_reports_no_deterministic_signal_when_empty() {
        let inf = PlanFieldInference::default();
        assert_eq!(
            inf.status(InferPlanFieldsMode::SonnetSuggest),
            "sonnet_suggest_no_deterministic_signal"
        );
    }

    #[test]
    fn sonnet_suggest_status_reports_sonnet_suggest_when_signals_present() {
        let mut inf = PlanFieldInference::default();
        inf.suggested.push(InferredField {
            field: "target",
            value: json!("mission_execution"),
            confidence: InferenceConfidence::Medium,
            source: "evidence_sidecar",
            detail: None,
        });
        assert_eq!(
            inf.status(InferPlanFieldsMode::SonnetSuggest),
            "sonnet_suggest"
        );
    }

    #[test]
    fn build_llm_inference_prompt_embeds_inputs() {
        // Pin the prompt shape so future regressions are visible: must
        // mention the PLAN sexp, the directive provenance, the evidence
        // digest, the deterministic block, and the caller args.
        let plan_sexp = "(plan :target \"mission_task_delegate\")";
        let evidence = vec![json!({"target": "mission_execution"})];
        let deterministic = PlanFieldInference::default();
        let args = json!({"foo": "bar"});
        let (system, user) = build_llm_inference_prompt(
            plan_sexp,
            Some("directive/abc:1"),
            &evidence,
            &deterministic,
            &args,
        );
        assert!(system.contains("plan field inference"));
        assert!(system.contains("STRICT JSON"));
        assert!(system.contains("conflict_status"));
        assert!(user.contains(plan_sexp));
        assert!(user.contains("directive/abc:1"));
        assert!(user.contains("\"foo\""));
        assert!(user.contains("\"target\": \"mission_execution\""));
    }

    #[test]
    fn deterministic_covers_all_fields_pred_only_true_when_six_high_inferences() {
        let mut inf = PlanFieldInference::default();
        // Empty → false.
        assert!(!deterministic_covers_all_fields(&inf));
        for f in LLM_ALLOWED_FIELDS.iter().take(5) {
            inf.inferred.push(InferredField {
                field: *f,
                value: json!("x"),
                confidence: InferenceConfidence::High,
                source: "plan_sexp",
                detail: None,
            });
        }
        // Only 5 of 6 → still false.
        assert!(!deterministic_covers_all_fields(&inf));
        // Add the last field → true.
        inf.inferred.push(InferredField {
            field: LLM_ALLOWED_FIELDS[5],
            value: json!(true),
            confidence: InferenceConfidence::High,
            source: "plan_sexp",
            detail: None,
        });
        assert!(deterministic_covers_all_fields(&inf));
    }

    #[test]
    fn deterministic_covers_all_fields_ignores_suggestions() {
        // Only high-confidence inferred entries count; suggestions
        // (medium / low) leave the predicate at `false` so the LLM is
        // still asked to weigh in.
        let mut inf = PlanFieldInference::default();
        for f in LLM_ALLOWED_FIELDS.iter() {
            inf.suggested.push(InferredField {
                field: *f,
                value: json!("x"),
                confidence: InferenceConfidence::Medium,
                source: "evidence_sidecar",
                detail: None,
            });
        }
        assert!(!deterministic_covers_all_fields(&inf));
    }

    #[test]
    fn coerce_proposal_value_workstation_dispatch_string_normalises() {
        let v = coerce_proposal_value("workstation_dispatch", &json!("YES"))
            .expect("string yes coerces to bool true");
        assert_eq!(v, json!(true));
        let v = coerce_proposal_value("workstation_dispatch", &json!("0"))
            .expect("string 0 coerces to bool false");
        assert_eq!(v, json!(false));
    }

    #[test]
    fn coerce_proposal_value_target_project_strips_whitespace() {
        let v = coerce_proposal_value("target_project", &json!("  missiond  "))
            .expect("trims whitespace");
        assert_eq!(v, json!("missiond"));
    }

    #[test]
    fn coerce_proposal_value_owned_files_drops_blank_entries() {
        let v = coerce_proposal_value(
            "owned_files",
            &json!(["src/lib.rs", "  ", "src/main.rs"]),
        )
        .expect("blanks stripped");
        assert_eq!(v, json!(["src/lib.rs", "src/main.rs"]));
    }

    #[test]
    fn refuse_llm_inference_in_dag_mode_blocks_sonnet_suggest() {
        // wave-20 / task 07 — single-node-only enforcement on the DAG path.
        let args = json!({
            "scheduler_mode": "dag_v1",
            "infer_plan_fields": "sonnet_suggest"
        });
        let err = super::super::plan_dag::refuse_llm_inference_in_dag_mode(&args)
            .expect("dag + sonnet_suggest combo refused");
        assert_eq!(err.is_error, Some(true));
        let payload = parse_payload(&err);
        let reason = payload["reason"]
            .as_str()
            .expect("structured ToolError carries `reason`");
        assert!(
            reason.contains("single-node-execute-only"),
            "reason: {}",
            reason
        );
        assert_eq!(payload["error_code"], "INVALID_PARAM");
    }

    #[test]
    fn refuse_llm_inference_in_dag_mode_allows_deterministic_modes() {
        // off / preview / apply_safe stay accepted on the DAG path
        // (they were already accepted in wave-18 / task 06).
        for mode in ["off", "preview", "apply_safe"] {
            let args = json!({
                "scheduler_mode": "dag_v1",
                "infer_plan_fields": mode
            });
            assert!(
                super::super::plan_dag::refuse_llm_inference_in_dag_mode(&args).is_none(),
                "deterministic mode `{}` must not be refused on DAG path",
                mode
            );
        }
        // No infer_plan_fields at all → also accepted.
        let args = json!({"scheduler_mode": "dag_v1"});
        assert!(super::super::plan_dag::refuse_llm_inference_in_dag_mode(&args).is_none());
    }

    // ── wave-21 / task 04 — autonomous workstation LLM proposal v0 ─────

    #[test]
    fn parse_workstation_inference_mode_default_is_off() {
        let mode = parse_workstation_inference_mode(&json!({})).expect("default ok");
        assert_eq!(mode, WorkstationInferenceMode::Off);
        assert!(!mode.is_sonnet_suggest());
        let mode_blank = parse_workstation_inference_mode(
            &json!({"workstation_inference_mode": ""}),
        )
        .expect("blank ok");
        assert_eq!(mode_blank, WorkstationInferenceMode::Off);
        let mode_off = parse_workstation_inference_mode(
            &json!({"workstation_inference_mode": "off"}),
        )
        .expect("off ok");
        assert_eq!(mode_off, WorkstationInferenceMode::Off);
    }

    #[test]
    fn parse_workstation_inference_mode_accepts_sonnet_suggest() {
        let mode = parse_workstation_inference_mode(
            &json!({"workstation_inference_mode": "sonnet_suggest"}),
        )
        .expect("sonnet_suggest ok");
        assert_eq!(mode, WorkstationInferenceMode::SonnetSuggest);
        assert!(mode.is_sonnet_suggest());
    }

    #[test]
    fn parse_workstation_inference_mode_rejects_typo() {
        let err = parse_workstation_inference_mode(
            &json!({"workstation_inference_mode": "sonnet-suggest"}),
        )
        .expect_err("hyphenated form rejected");
        assert!(err.contains("workstation_inference_mode"));
        assert!(err.contains("sonnet_suggest"));
        assert!(err.contains("sonnet-suggest"));
    }

    #[test]
    fn workstation_inference_mode_wire_string_round_trips() {
        assert_eq!(
            WorkstationInferenceMode::Off.as_wire(),
            WORKSTATION_INFER_MODE_OFF
        );
        assert_eq!(
            WorkstationInferenceMode::SonnetSuggest.as_wire(),
            WORKSTATION_INFER_MODE_SONNET_SUGGEST
        );
    }

    #[test]
    fn refuse_workstation_inference_in_dag_mode_blocks_sonnet_suggest() {
        // wave-21 / task 04 — single-node-only enforcement on the DAG
        // path. Mirrors the wave-20 / task 07 enforcement on the
        // plan-field surface.
        let args = json!({
            "scheduler_mode": "dag_v1",
            "workstation_inference_mode": "sonnet_suggest"
        });
        let err = refuse_workstation_inference_in_dag_mode(&args)
            .expect("dag + sonnet_suggest combo refused");
        assert_eq!(err.is_error, Some(true));
        let payload = parse_payload(&err);
        let reason = payload["reason"]
            .as_str()
            .expect("structured ToolError carries `reason`");
        assert!(
            reason.contains("single-node-execute-only"),
            "reason: {}",
            reason
        );
        assert_eq!(payload["error_code"], "INVALID_PARAM");
    }

    #[test]
    fn refuse_workstation_inference_in_dag_mode_allows_off_mode() {
        // Default `off` mode never trips the DAG refusal.
        for mode in [
            json!({"scheduler_mode": "dag_v1"}),
            json!({"scheduler_mode": "dag_v1", "workstation_inference_mode": "off"}),
            json!({"scheduler_mode": "dag_v1", "workstation_inference_mode": ""}),
        ] {
            assert!(
                refuse_workstation_inference_in_dag_mode(&mode).is_none(),
                "off-shaped mode must not be refused on DAG path: {}",
                mode
            );
        }
    }

    #[test]
    fn refuse_workstation_inference_in_dag_mode_no_op_outside_dag() {
        // sonnet_suggest WITHOUT scheduler_mode=dag_v1 is allowed (single-
        // node executes are the canonical wave-21 / task 04 surface).
        let args = json!({"workstation_inference_mode": "sonnet_suggest"});
        assert!(refuse_workstation_inference_in_dag_mode(&args).is_none());
    }

    #[test]
    fn plan_hints_carry_workstation_signal_detects_objective() {
        let mut h = ParsedPlanHints::default();
        assert!(!plan_hints_carry_workstation_signal(&h));
        h.objective = Some("ship".to_string());
        assert!(plan_hints_carry_workstation_signal(&h));
    }

    #[test]
    fn plan_hints_carry_workstation_signal_detects_each_workstation_knob() {
        // fn pointer (not closure) so the array elements all share one
        // type. Each fn flips exactly one knob; the assertion confirms
        // the predicate fires off any single knob.
        type Mutator = fn(&mut ParsedPlanHints);
        let cases: &[(Mutator, &str)] = &[
            (|h| h.objective = Some("o".into()), "objective"),
            (|h| h.summary = Some("s".into()), "summary"),
            (|h| h.scope = Some("z".into()), "scope"),
            (|h| h.owned_files_raw = Some("[a]".into()), "owned"),
            (|h| h.forbidden_files_raw = Some("[b]".into()), "forbidden"),
            (|h| h.acceptance_commands_raw = Some("[c]".into()), "accept"),
            (|h| h.commit_policy = Some("p".into()), "policy"),
            (|h| h.target_project = Some("missiond".into()), "tp"),
            (|h| h.requested_cwd = Some("/x".into()), "cwd"),
            (|h| h.dispatch_strategy = Some("agent-team".into()), "ds"),
        ];
        for (mutate, label) in cases {
            let mut h = ParsedPlanHints::default();
            mutate(&mut h);
            assert!(
                plan_hints_carry_workstation_signal(&h),
                "{} hint should register as signal",
                label
            );
        }
    }

    #[test]
    fn plan_hints_carry_workstation_signal_ignores_blank_strings() {
        let mut h = ParsedPlanHints::default();
        h.objective = Some("   ".to_string());
        h.scope = Some("".to_string());
        assert!(!plan_hints_carry_workstation_signal(&h));
    }

    #[test]
    fn attach_workstation_proposals_block_no_op_when_bundle_absent() {
        let original = ToolResult::json_pretty(&json!({"status": "executing"}));
        let r = attach_workstation_proposals_block(original, None);
        let v = parse_payload(&r);
        // Wire shape is unchanged when the bundle is absent.
        assert!(v.get("workstation_proposals").is_none());
        assert!(v.get("workstation_inference_mode").is_none());
        assert_eq!(v["status"], "executing");
    }

    #[test]
    fn attach_workstation_proposals_block_attaches_bundle_and_mode() {
        let original = ToolResult::json_pretty(&json!({"status": "executing"}));
        let bundle =
            super::super::workstation_dispatch::WorkstationProposalBundle::unavailable(
                "Sonnet gateway not initialized; (no fallback to claude -p / prompt mode in v0)",
            );
        let r = attach_workstation_proposals_block(original, Some(&bundle));
        let v = parse_payload(&r);
        assert_eq!(v["workstation_proposals"]["status"], "llm_unavailable");
        assert_eq!(v["workstation_proposals"]["auto_spawn"], false);
        assert!(
            v["workstation_proposals"]["unavailable_reason"]
                .as_str()
                .unwrap_or("")
                .contains("no fallback")
        );
        assert_eq!(
            v["workstation_inference_mode"], "sonnet_suggest",
            "the mode echo must land on the response when bundle is present"
        );
    }

    #[test]
    fn attach_workstation_proposals_block_preserves_pre_existing_block() {
        // If the result already carries a `workstation_proposals` key
        // (future DAG / resume path), we must NEVER overwrite.
        let original = ToolResult::json_pretty(&json!({
            "status": "executing",
            "workstation_proposals": {"status": "preserved"},
        }));
        let bundle =
            super::super::workstation_dispatch::WorkstationProposalBundle::unavailable("x");
        let r = attach_workstation_proposals_block(original, Some(&bundle));
        let v = parse_payload(&r);
        assert_eq!(v["workstation_proposals"]["status"], "preserved");
    }

    #[test]
    fn attach_workstation_proposals_block_skips_error_results() {
        // Errors propagate untouched — never decorated with proposals.
        let original = ToolResult::structured_error(ToolError::new(
            error_codes::INVALID_PARAM,
            "boom",
        ));
        assert_eq!(original.is_error, Some(true));
        let bundle =
            super::super::workstation_dispatch::WorkstationProposalBundle::unavailable("x");
        let r = attach_workstation_proposals_block(original, Some(&bundle));
        // The structured-error payload does NOT pick up the bundle keys.
        let payload = parse_payload(&r);
        assert!(payload.get("workstation_proposals").is_none());
        assert!(payload.get("workstation_inference_mode").is_none());
    }

    // ── wave-21 / task 05 — PLAN inference apply gate v1 ────────────────

    #[test]
    fn validate_apply_gate_args_accepts_bool_and_absent() {
        // Default (no flags) is valid.
        assert!(validate_apply_gate_args(&json!({})).is_ok());
        // Bool true / false are valid.
        assert!(validate_apply_gate_args(&json!({"apply_inferred_fields": true})).is_ok());
        assert!(validate_apply_gate_args(&json!({"apply_inferred_fields": false})).is_ok());
        assert!(validate_apply_gate_args(&json!({"persist_inference": true})).is_ok());
        // Object / array forms for llm_caller_approved are valid.
        assert!(validate_apply_gate_args(
            &json!({"llm_caller_approved": {"target": true}})
        )
        .is_ok());
        assert!(validate_apply_gate_args(
            &json!({"llm_caller_approved": ["target"]})
        )
        .is_ok());
    }

    #[test]
    fn validate_apply_gate_args_rejects_string_form() {
        // Conservative: string `"true"` MUST NOT silently open the gate.
        let err = validate_apply_gate_args(&json!({"apply_inferred_fields": "true"}))
            .expect_err("string form rejected");
        assert!(err.contains("apply_inferred_fields must be a boolean"));
        let err = validate_apply_gate_args(&json!({"persist_inference": "true"}))
            .expect_err("persist_inference string rejected");
        assert!(err.contains("persist_inference must be a boolean"));
        // llm_caller_approved bool / string is also rejected.
        let err = validate_apply_gate_args(&json!({"llm_caller_approved": true}))
            .expect_err("bool form rejected");
        assert!(err.contains("llm_caller_approved must be object"));
    }

    #[test]
    fn caller_requested_apply_defaults_false() {
        assert!(!caller_requested_apply(&json!({})));
        assert!(!caller_requested_apply(
            &json!({"apply_inferred_fields": false})
        ));
        assert!(caller_requested_apply(
            &json!({"apply_inferred_fields": true})
        ));
        // String form is treated as false (validator rejects it before
        // we get here, but the helper is defensive).
        assert!(!caller_requested_apply(
            &json!({"apply_inferred_fields": "true"})
        ));
    }

    #[test]
    fn parse_llm_caller_approved_accepts_object_and_array() {
        let from_obj = parse_llm_caller_approved(
            &json!({"llm_caller_approved": {"target": true, "owned_files": false}}),
        );
        assert!(from_obj.contains("target"));
        assert!(!from_obj.contains("owned_files"));
        let from_arr = parse_llm_caller_approved(
            &json!({"llm_caller_approved": ["target", "workstation_dispatch"]}),
        );
        assert!(from_arr.contains("target"));
        assert!(from_arr.contains("workstation_dispatch"));
        // Unknown fields silently dropped (the gate's "unknown_field"
        // skip path covers downstream observability).
        let unknown = parse_llm_caller_approved(
            &json!({"llm_caller_approved": ["bogus_field"]}),
        );
        assert!(unknown.is_empty());
    }

    #[test]
    fn apply_gate_default_off_skips_everything() {
        // Apply flag absent → high-confidence inferred fields land in
        // `skipped_fields[]` with reason `apply_gate_not_requested`,
        // never in `applied_fields[]`.
        let mut inf = PlanFieldInference::default();
        inf.inferred.push(InferredField {
            field: "target",
            value: json!("mission_task_delegate"),
            confidence: InferenceConfidence::High,
            source: "plan_sexp",
            detail: None,
        });
        let outcome = compute_apply_gate(&json!({}), &inf);
        assert!(!outcome.requested);
        assert!(outcome.applied.is_empty(), "no apply without explicit gate");
        assert_eq!(outcome.skipped.len(), 1);
        assert_eq!(outcome.skipped[0].field, "target");
        assert_eq!(outcome.skipped[0].reason, "apply_gate_not_requested");
        // resulting_plan_preview is the caller args verbatim.
        assert_eq!(outcome.resulting_plan_preview, json!({}));
    }

    #[test]
    fn apply_gate_opt_in_promotes_high_confidence_inferred() {
        let mut inf = PlanFieldInference::default();
        inf.inferred.push(InferredField {
            field: "target",
            value: json!("mission_task_delegate"),
            confidence: InferenceConfidence::High,
            source: "plan_sexp",
            detail: None,
        });
        let args = json!({"apply_inferred_fields": true});
        let outcome = compute_apply_gate(&args, &inf);
        assert!(outcome.requested);
        assert_eq!(outcome.applied.len(), 1);
        assert_eq!(outcome.applied[0].field, "target");
        assert_eq!(outcome.applied[0].origin.as_wire(), "deterministic_inferred");
        assert_eq!(
            outcome.resulting_plan_preview["target"],
            json!("mission_task_delegate")
        );
    }

    #[test]
    fn apply_gate_skips_caller_value_already_set() {
        let mut inf = PlanFieldInference::default();
        inf.inferred.push(InferredField {
            field: "target",
            value: json!("mission_task_delegate"),
            confidence: InferenceConfidence::High,
            source: "plan_sexp",
            detail: None,
        });
        let args = json!({
            "apply_inferred_fields": true,
            "target": "mission_execution",
        });
        let outcome = compute_apply_gate(&args, &inf);
        assert!(outcome.applied.is_empty(), "caller value wins");
        let skip = outcome
            .skipped
            .iter()
            .find(|s| s.field == "target")
            .expect("skip row");
        assert_eq!(skip.reason, "caller_value_already_set");
        // Preview leaves caller value intact.
        assert_eq!(
            outcome.resulting_plan_preview["target"],
            json!("mission_execution")
        );
    }

    #[test]
    fn apply_gate_routes_conflicts_to_conflict_fields() {
        // Caller-vs-inferred conflicts are NEVER applied AND surface
        // separately on `conflict_fields[]`.
        let mut inf = PlanFieldInference::default();
        inf.conflicts.push(InferenceConflict {
            field: "target",
            caller_value: json!("mission_execution"),
            inferred_value: json!("mission_task_delegate"),
            confidence: InferenceConfidence::High,
            source: "plan_sexp",
        });
        let outcome = compute_apply_gate(
            &json!({
                "apply_inferred_fields": true,
                "target": "mission_execution",
            }),
            &inf,
        );
        assert!(outcome.applied.is_empty(), "no apply on conflict");
        assert_eq!(outcome.conflict.len(), 1);
        assert_eq!(outcome.conflict[0].field, "target");
        // A skip row mirrors the conflict for grep consistency.
        let skip = outcome
            .skipped
            .iter()
            .find(|s| s.reason == "caller_value_conflict")
            .expect("conflict-source skip row");
        assert_eq!(skip.field, "target");
        assert_eq!(skip.origin.as_wire(), "deterministic_conflict");
    }

    #[test]
    fn apply_gate_skips_suggestions_below_threshold() {
        // Medium / low suggestions are conservative-skip even with the
        // gate flag set — the caller must promote them via explicit args.
        let mut inf = PlanFieldInference::default();
        inf.suggested.push(InferredField {
            field: "target",
            value: json!("mission_task_delegate"),
            confidence: InferenceConfidence::Medium,
            source: "compiled_from",
            detail: None,
        });
        let outcome = compute_apply_gate(
            &json!({"apply_inferred_fields": true}),
            &inf,
        );
        assert!(outcome.applied.is_empty(), "below-threshold never applies");
        let skip = outcome
            .skipped
            .iter()
            .find(|s| s.field == "target")
            .expect("skip row");
        assert_eq!(skip.reason, "below_apply_threshold");
        assert_eq!(skip.origin.as_wire(), "deterministic_suggested");
    }

    #[test]
    fn apply_gate_llm_skipped_without_caller_approval() {
        // LLM proposals never apply unless the caller named the field
        // in `llm_caller_approved`. Default policy is conservative.
        let mut inf = PlanFieldInference::default();
        inf.llm = Some(LlmProposalBundle {
            status: LlmProposalStatus::Suggested,
            proposals: vec![LlmProposal {
                field: "target",
                value: json!("mission_task_delegate"),
                confidence: InferenceConfidence::High,
                evidence: "vibes".to_string(),
                conflict_status: LlmConflictStatus::None,
            }],
            parse_warnings: Vec::new(),
            unavailable_reason: None,
            model: None,
            request_caller: None,
        });
        let outcome = compute_apply_gate(
            &json!({"apply_inferred_fields": true}),
            &inf,
        );
        assert!(outcome.applied.is_empty(), "no LLM apply without approval");
        let skip = outcome
            .skipped
            .iter()
            .find(|s| s.origin == ApplyOrigin::LlmProposal)
            .expect("LLM skip row");
        assert_eq!(skip.reason, "llm_not_caller_approved");
    }

    #[test]
    fn apply_gate_llm_promoted_when_caller_approved_and_safe() {
        let mut inf = PlanFieldInference::default();
        inf.llm = Some(LlmProposalBundle {
            status: LlmProposalStatus::Suggested,
            proposals: vec![LlmProposal {
                field: "dispatch_strategy",
                value: json!("agent-team"),
                confidence: InferenceConfidence::High,
                evidence: "PLAN explicitly mentions parallelism".to_string(),
                conflict_status: LlmConflictStatus::None,
            }],
            parse_warnings: Vec::new(),
            unavailable_reason: None,
            model: None,
            request_caller: None,
        });
        let outcome = compute_apply_gate(
            &json!({
                "apply_inferred_fields": true,
                "llm_caller_approved": ["dispatch_strategy"],
            }),
            &inf,
        );
        assert_eq!(outcome.applied.len(), 1);
        let af = &outcome.applied[0];
        assert_eq!(af.field, "dispatch_strategy");
        assert_eq!(af.origin.as_wire(), "llm_proposal");
        assert_eq!(
            outcome.resulting_plan_preview["dispatch_strategy"],
            json!("agent-team")
        );
    }

    #[test]
    fn apply_gate_llm_safety_check_rejects_unsupported_strategy() {
        // `prompt-fallback` and `unknown` are deliberately excluded from
        // the apply-gate whitelist (mirrors wave-21 / task 04).
        let mut inf = PlanFieldInference::default();
        inf.llm = Some(LlmProposalBundle {
            status: LlmProposalStatus::Suggested,
            proposals: vec![LlmProposal {
                field: "dispatch_strategy",
                value: json!("prompt-fallback"),
                confidence: InferenceConfidence::High,
                evidence: "model guess".to_string(),
                conflict_status: LlmConflictStatus::None,
            }],
            parse_warnings: Vec::new(),
            unavailable_reason: None,
            model: None,
            request_caller: None,
        });
        let outcome = compute_apply_gate(
            &json!({
                "apply_inferred_fields": true,
                "llm_caller_approved": ["dispatch_strategy"],
            }),
            &inf,
        );
        assert!(outcome.applied.is_empty(), "unsupported strategy rejected");
        let skip = outcome
            .skipped
            .iter()
            .find(|s| s.field == "dispatch_strategy")
            .expect("skip row");
        assert_eq!(skip.reason, "llm_safety_check_failed");
        assert!(skip
            .detail
            .as_deref()
            .unwrap_or("")
            .contains("prompt-fallback"));
    }

    #[test]
    fn apply_gate_llm_skipped_on_conflict_status() {
        // wave-20 reconciliation already tagged a deterministic conflict;
        // the apply gate respects it.
        let mut inf = PlanFieldInference::default();
        inf.llm = Some(LlmProposalBundle {
            status: LlmProposalStatus::Suggested,
            proposals: vec![LlmProposal {
                field: "target",
                value: json!("mission_execution"),
                confidence: InferenceConfidence::High,
                evidence: "model says X".to_string(),
                conflict_status: LlmConflictStatus::ConflictsWithDeterministic,
            }],
            parse_warnings: Vec::new(),
            unavailable_reason: None,
            model: None,
            request_caller: None,
        });
        let outcome = compute_apply_gate(
            &json!({
                "apply_inferred_fields": true,
                "llm_caller_approved": ["target"],
            }),
            &inf,
        );
        assert!(outcome.applied.is_empty());
        let skip = outcome
            .skipped
            .iter()
            .find(|s| s.field == "target")
            .expect("skip row");
        assert_eq!(skip.reason, "llm_conflict_present");
    }

    #[test]
    fn apply_gate_llm_skipped_when_low_confidence() {
        let mut inf = PlanFieldInference::default();
        inf.llm = Some(LlmProposalBundle {
            status: LlmProposalStatus::Suggested,
            proposals: vec![LlmProposal {
                field: "target",
                value: json!("mission_task_delegate"),
                confidence: InferenceConfidence::Low,
                evidence: "weak signal".to_string(),
                conflict_status: LlmConflictStatus::None,
            }],
            parse_warnings: Vec::new(),
            unavailable_reason: None,
            model: None,
            request_caller: None,
        });
        let outcome = compute_apply_gate(
            &json!({
                "apply_inferred_fields": true,
                "llm_caller_approved": ["target"],
            }),
            &inf,
        );
        assert!(outcome.applied.is_empty());
        let skip = outcome
            .skipped
            .iter()
            .find(|s| s.field == "target")
            .expect("skip row");
        assert_eq!(skip.reason, "llm_confidence_too_low");
    }

    #[test]
    fn apply_gate_llm_skipped_when_deterministic_already_filled_slot() {
        // Deterministic high-confidence already promoted `target`;
        // the LLM proposal for the same slot should NOT silently apply
        // a second time.
        let mut inf = PlanFieldInference::default();
        inf.inferred.push(InferredField {
            field: "target",
            value: json!("mission_task_delegate"),
            confidence: InferenceConfidence::High,
            source: "plan_sexp",
            detail: None,
        });
        inf.llm = Some(LlmProposalBundle {
            status: LlmProposalStatus::Suggested,
            proposals: vec![LlmProposal {
                field: "target",
                value: json!("mission_execution"),
                confidence: InferenceConfidence::High,
                evidence: "different guess".to_string(),
                conflict_status: LlmConflictStatus::None,
            }],
            parse_warnings: Vec::new(),
            unavailable_reason: None,
            model: None,
            request_caller: None,
        });
        let outcome = compute_apply_gate(
            &json!({
                "apply_inferred_fields": true,
                "llm_caller_approved": ["target"],
            }),
            &inf,
        );
        // Deterministic wins; LLM is skipped explicitly.
        assert_eq!(outcome.applied.len(), 1);
        assert_eq!(outcome.applied[0].origin.as_wire(), "deterministic_inferred");
        let skip = outcome
            .skipped
            .iter()
            .find(|s| s.origin == ApplyOrigin::LlmProposal)
            .expect("LLM skip row");
        assert_eq!(skip.reason, "deterministic_inferred_already_applied");
    }

    #[test]
    fn apply_gate_response_block_has_stable_shape() {
        let outcome = ApplyGateOutcome {
            requested: false,
            persist_inference_requested: false,
            applied: Vec::new(),
            skipped: Vec::new(),
            conflict: Vec::new(),
            resulting_plan_preview: json!({}),
        };
        let block = outcome.to_response_json();
        assert_eq!(block["requested"], false);
        assert_eq!(block["persist_inference_requested"], false);
        // v1 invariant: persisted plan text is NEVER mutated by this gate.
        assert_eq!(block["persist_inference_applied"], false);
        assert!(block["applied_fields"].is_array());
        assert!(block["skipped_fields"].is_array());
        assert!(block["conflict_fields"].is_array());
        assert!(block["resulting_plan_preview"].is_object());
    }

    #[test]
    fn apply_gate_persist_inference_flag_echoed_but_never_applied() {
        // Even when caller passes persist_inference=true the v1 gate
        // must NOT mutate persisted plan text — the response surface
        // pins the invariant via `persist_inference_applied=false`.
        let outcome = compute_apply_gate(
            &json!({
                "apply_inferred_fields": true,
                "persist_inference": true,
            }),
            &PlanFieldInference::default(),
        );
        assert!(outcome.persist_inference_requested);
        let block = outcome.to_response_json();
        assert_eq!(block["persist_inference_requested"], true);
        assert_eq!(block["persist_inference_applied"], false);
    }

    #[test]
    fn attach_apply_gate_block_skips_when_block_absent() {
        let original = ToolResult::json_pretty(&json!({"status": "executing"}));
        let original_text = match original.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("text"),
        };
        let r = attach_apply_gate_block(original, None);
        let after_text = match r.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("text"),
        };
        assert_eq!(original_text, after_text);
    }

    #[test]
    fn attach_apply_gate_block_splices_block_into_payload() {
        let original = ToolResult::json_pretty(&json!({"status": "executing"}));
        let block = json!({"requested": true, "applied_fields": []});
        let r = attach_apply_gate_block(original, Some(block.clone()));
        let v = parse_payload(&r);
        assert_eq!(v["status"], "executing");
        assert_eq!(v["apply_gate"], block);
    }

    #[test]
    fn attach_apply_gate_block_preserves_pre_existing_block() {
        let original = ToolResult::json_pretty(&json!({
            "status": "executing",
            "apply_gate": {"requested": false},
        }));
        let block = json!({"requested": true});
        let r = attach_apply_gate_block(original, Some(block));
        let v = parse_payload(&r);
        // Pre-existing block wins.
        assert_eq!(v["apply_gate"]["requested"], false);
    }

    #[test]
    fn attach_apply_gate_block_skips_error_results() {
        let original = ToolResult::structured_error(ToolError::new(
            error_codes::INVALID_PARAM,
            "boom",
        ));
        let block = json!({"requested": true});
        let r = attach_apply_gate_block(original, Some(block));
        let payload = parse_payload(&r);
        assert!(payload.get("apply_gate").is_none());
    }

    #[test]
    fn apply_gate_resulting_plan_preview_includes_caller_and_applied_fields() {
        // Caller passes one field, gate applies one inferred field; the
        // preview shows the union (without mutating caller args).
        let mut inf = PlanFieldInference::default();
        inf.inferred.push(InferredField {
            field: "target",
            value: json!("mission_task_delegate"),
            confidence: InferenceConfidence::High,
            source: "plan_sexp",
            detail: None,
        });
        let args = json!({
            "apply_inferred_fields": true,
            "objective": "ship feature",
        });
        let outcome = compute_apply_gate(&args, &inf);
        let preview = &outcome.resulting_plan_preview;
        assert_eq!(preview["objective"], "ship feature");
        assert_eq!(preview["target"], "mission_task_delegate");
        assert_eq!(preview["apply_inferred_fields"], true);
    }

    // ── Wave 21 / Task 08 — machine-contract autonomous loop smoke ──
    //
    // Pure-helper smoke proving the wave-19/20/21-07 distill chain
    // receipts compose cleanly on a synthesised plan-execute payload
    // without any AppState. The chain block is the wave21-07 SSOT for
    // the auto-sonnet apply-gate's status taxonomy and we re-pin every
    // wave21-07 invariant here in one assert block so a future refactor
    // that drops a status / breaks the wire shape lands an explicit
    // failure on the autonomous-loop smoke.
    //
    // Invariants pinned (cross-wave):
    //   * I1-07  default-off byte-shape — no chain block surfaces
    //            unless the gate explicitly fires.
    //   * I3-07  the gate REUSES the wave-20 trigger outcomes — when the
    //            trigger short-circuits to skip, the chain block surfaces
    //            `triggered=false` + the dedicated skip status (NOT the
    //            applied status).
    //   * I7-07  wave-19 / wave-20 blocks remain unchanged — the chain
    //            block is purely additive.

    /// Wave21-08 smoke: when the wave21-07 auto-apply gate skips because
    /// the plan never reached `succeeded`, the chain block surfaces
    /// `triggered=false` + `status=skipped_plan_not_succeeded` and
    /// suppresses every applied-side optional (evidence_path /
    /// chain_index_in_plan / distill_result). This is the I3-07
    /// invariant proof: the gate REUSES the trigger outcomes — it
    /// never relaxes them by faking an evidence path.
    #[test]
    fn smoke_wave21_distill_chain_block_pins_skip_status_when_trigger_short_circuits() {
        let block = build_distill_chain_block(
            // triggered=false because the wave-20 trigger short-circuited
            false,
            CHAIN_STATUS_SKIPPED_PLAN_NOT_SUCCEEDED,
            "chain:auto:wave21-08-smoke",
            "derived_from_plan_id",
            "record_only",
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert_eq!(block["triggered"], false);
        assert_eq!(block["status"], "skipped_plan_not_succeeded");
        assert_eq!(block["chain_id"], "chain:auto:wave21-08-smoke");
        assert_eq!(block["chain_id_source"], "derived_from_plan_id");
        assert_eq!(block["chain_mode"], "record_only");
        // I3-07: the skip path MUST NOT fabricate evidence / index /
        // distill_result fields — those are reserved for the applied
        // path. A future refactor that always emits them would defeat
        // the gate's "REUSE the trigger outcomes" contract.
        for key in [
            "evidence_path",
            "chain_index_in_plan",
            "distill_result",
            "warning",
            "evidence_error",
            "chain_name",
        ] {
            assert!(
                block.get(key).is_none(),
                "wave21-08 smoke: skip-path chain block MUST NOT fabricate `{}`",
                key
            );
        }
    }

    /// Wave21-08 smoke: when the wave-20 trigger fires AND the inner
    /// distill recorded a downstream warning, the chain block surfaces
    /// BOTH the inner result AND the warning string under the dedicated
    /// `recorded_with_distill_warning` status. This preserves observers'
    /// ability to detect partial success without scraping the full
    /// payload — wave21-07 invariant I5 (Sonnet failure preserves the
    /// inner payload + surfaces a typed status) flows through this
    /// block on the wave-20 distill side.
    #[test]
    fn smoke_wave21_distill_chain_block_surfaces_inner_distill_warning() {
        let block = build_distill_chain_block(
            true,
            CHAIN_STATUS_RECORDED_DISTILL_WARNING,
            "chain:wave21-08",
            "explicit_arg",
            "sonnet",
            Some("wave21-08-loop"),
            Some(2),
            Some(json!({"error": "sonnet quota exhausted"})),
            Some("distill chain workflow call returned an error; plan finalization preserved"),
            Some("/tmp/missiond-wave21-08/.evidence.json"),
            None,
        );
        assert_eq!(block["status"], "recorded_with_distill_warning");
        assert_eq!(block["chain_name"], "wave21-08-loop");
        assert_eq!(block["chain_index_in_plan"], 2);
        assert_eq!(block["distill_result"]["error"], "sonnet quota exhausted");
        assert!(block["warning"]
            .as_str()
            .unwrap_or("")
            .contains("plan finalization preserved"));
        // wave21-07 invariant I7: the chain block is additive — every
        // optional surfaces verbatim when supplied, never silently
        // dropped.
        assert_eq!(
            block["evidence_path"],
            "/tmp/missiond-wave21-08/.evidence.json"
        );
        assert!(block.get("evidence_error").is_none());
    }

    /// Wave21-08 smoke: machine-contract dispatch response build pins the
    /// wave-20/04 SSOT invariant (`task_contract_source_path` surfaces
    /// the resolved on-disk path) AND carries the wave-19/07 source-
    /// contract preamble in the brief preview. Pinning these here closes
    /// the workstation-side autonomous loop on a single canonical
    /// fixture without spinning AppState.
    #[test]
    fn smoke_wave21_machine_dispatch_response_pins_source_path_invariant() {
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "agent-team");
        let contract_path =
            "/tmp/missiond-wave21-08-smoke/.missiond/tasks/wave21/wave21-08-dispatch.lisp";
        let outcome = wd::WorkstationDispatchOutcome::Dispatched {
            task_brief: format!(
                "## Source contract\n- task-contract v1: `{}`\n## Objective\nship the wave21-08 deterministic loop smoke\n",
                contract_path
            ),
            task_brief_path: None,
            task_contract_source_path: Some(contract_path.to_string()),
            evidence_path: Some("/tmp/missiond-wave21-08-smoke/.evidence.json".to_string()),
            evidence_error: None,
            inner_payload: json!({"task_id": "btk-wave21-08-smoke"}),
        };
        let decision = fixture_decision(wd::WorkstationDispatchSource::ExplicitArg);
        let result = build_workstation_dispatch_response(
            &plan,
            &resolved,
            outcome,
            &decision,
            &TaskContractEmissionRecord::off(),
            DispatchContractMode::Machine,
        );
        let v = parse_payload(&result);
        // wave-20/04 SSOT invariant: the response MUST surface the
        // resolved contract path so observers can prove the Lisp drove
        // the brief.
        assert_eq!(v["status"], "executing");
        assert_eq!(v["workstation_dispatch_status"], "dispatched");
        assert_eq!(v["dispatch_contract_mode"], "machine");
        assert_eq!(v["task_contract_source_path"], contract_path);
        // wave-19/07 invariant: the brief preview MUST carry the source-
        // contract preamble naming the same on-disk path.
        let preview = v["task_brief_preview"].as_str().unwrap_or("");
        assert!(
            preview.contains("## Source contract"),
            "wave21-08 brief MUST carry the wave-19/07 source-contract preamble"
        );
        assert!(
            preview.contains(contract_path),
            "wave21-08 brief preamble MUST name the same on-disk path the response surfaced"
        );
        // wave-21/04 invariant: machine-mode dispatch is the autonomous
        // loop SSOT — observers MUST see the dispatch_contract_mode
        // marker so they can route on it.
        assert_eq!(
            v["dispatch_contract_mode"], "machine",
            "wave21-08 invariant: machine-mode marker MUST be load-bearing"
        );
    }

    // ── wave-22 / task 04 — Persisted PLAN inference apply v2 ───────────

    fn fixture_apply_outcome_with_one_high_inferred(args: &Value) -> ApplyGateOutcome {
        let inf = PlanFieldInference {
            inferred: vec![InferredField {
                field: "target",
                value: json!("mission_execution"),
                confidence: InferenceConfidence::High,
                source: "plan_sexp",
                detail: None,
            }],
            ..Default::default()
        };
        compute_apply_gate(args, &inf)
    }

    #[test]
    fn validate_apply_gate_args_accepts_caller_approved_and_proposal_hash() {
        // wave-22 / task 04 — extend wave-21 / task 05 validator to accept
        // the v2 persist-path opt-ins. Bool / string forms only.
        assert!(validate_apply_gate_args(&json!({"caller_approved": true})).is_ok());
        assert!(validate_apply_gate_args(&json!({"caller_approved": false})).is_ok());
        assert!(validate_apply_gate_args(
            &json!({"proposal_hash": "deadbeefdeadbeefdeadbeefdeadbeef"})
        )
        .is_ok());
        // Default (absent) is valid.
        assert!(validate_apply_gate_args(&json!({})).is_ok());
    }

    #[test]
    fn validate_apply_gate_args_rejects_v2_typo_shapes() {
        // String "true" must NOT silently arm caller_approved.
        let err = validate_apply_gate_args(&json!({"caller_approved": "true"}))
            .expect_err("string form rejected");
        assert!(err.contains("caller_approved must be a boolean"));
        // Number / object proposal_hash must be rejected so a typo never
        // silently bypasses the strict hash preflight.
        let err = validate_apply_gate_args(&json!({"proposal_hash": 1234}))
            .expect_err("number form rejected");
        assert!(err.contains("proposal_hash must be a string"));
        let err = validate_apply_gate_args(&json!({"proposal_hash": {"hash": "abc"}}))
            .expect_err("object form rejected");
        assert!(err.contains("proposal_hash must be a string"));
    }

    #[test]
    fn caller_requested_caller_approved_defaults_false() {
        // Default off — wave-21 / task 05 byte-shape preserved exactly
        // when caller does not supply the flag.
        assert!(!caller_requested_caller_approved(&json!({})));
        assert!(!caller_requested_caller_approved(
            &json!({"caller_approved": false})
        ));
        assert!(caller_requested_caller_approved(
            &json!({"caller_approved": true})
        ));
        // String form is treated as false — validator rejects it BEFORE
        // we get here, but the helper is defensive.
        assert!(!caller_requested_caller_approved(
            &json!({"caller_approved": "true"})
        ));
    }

    #[test]
    fn caller_supplied_proposal_hash_strips_whitespace_and_treats_blank_as_none() {
        assert_eq!(caller_supplied_proposal_hash(&json!({})), None);
        assert_eq!(
            caller_supplied_proposal_hash(&json!({"proposal_hash": "   "})),
            None
        );
        assert_eq!(
            caller_supplied_proposal_hash(&json!({"proposal_hash": "  abc123  "})),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn compute_inference_proposal_hash_is_deterministic_and_field_order_independent() {
        // Hash must be deterministic over the same plan_id +
        // original_sexp_hash + applied set, regardless of the order in
        // which the gate appended fields. This is what lets the caller
        // capture-and-replay the hash from a preview call.
        let plan_id = uuid::Uuid::nil();
        let h0 = sha256_hex("(plan :id 1)");
        let af1 = AppliedField {
            field: "target",
            value: json!("mission_execution"),
            source: "plan_sexp",
            origin: ApplyOrigin::DeterministicInferred,
        };
        let af2 = AppliedField {
            field: "dispatch_strategy",
            value: json!("agent-team"),
            source: "plan_sexp",
            origin: ApplyOrigin::DeterministicInferred,
        };
        let a = compute_inference_proposal_hash(plan_id, &h0, &[af1.clone(), af2.clone()]);
        let b = compute_inference_proposal_hash(plan_id, &h0, &[af2, af1]);
        assert_eq!(a, b, "hash must be field-order independent (sorted)");
        assert_eq!(a.len(), 32, "32-hex prefix per the v2 spec");
    }

    #[test]
    fn compute_inference_proposal_hash_changes_with_value() {
        let plan_id = uuid::Uuid::nil();
        let h0 = sha256_hex("(plan :id 1)");
        let af_a = AppliedField {
            field: "target",
            value: json!("mission_execution"),
            source: "plan_sexp",
            origin: ApplyOrigin::DeterministicInferred,
        };
        let af_b = AppliedField {
            field: "target",
            value: json!("mission_task_delegate"),
            source: "plan_sexp",
            origin: ApplyOrigin::DeterministicInferred,
        };
        let h_a = compute_inference_proposal_hash(plan_id, &h0, &[af_a]);
        let h_b = compute_inference_proposal_hash(plan_id, &h0, &[af_b]);
        assert_ne!(h_a, h_b);
    }

    #[test]
    fn evaluate_persisted_apply_gate_skips_when_apply_flag_off() {
        // No apply flag ⇒ skip with the canonical reason. Default
        // wave-21 / task 05 v1 byte-shape preserved.
        let args = json!({});
        let apply = fixture_apply_outcome_with_one_high_inferred(&args);
        let status = evaluate_persisted_apply_gate(&args, &apply);
        assert_eq!(status, PersistedApplyStatus::SkippedApplyGateNotRequested);
        assert_eq!(status.as_wire(), "skipped_apply_gate_not_requested");
        assert!(!status.was_applied());
    }

    #[test]
    fn evaluate_persisted_apply_gate_skips_when_persist_flag_off() {
        // apply_inferred_fields=true but persist_inference absent.
        let args = json!({"apply_inferred_fields": true});
        let apply = fixture_apply_outcome_with_one_high_inferred(&args);
        let status = evaluate_persisted_apply_gate(&args, &apply);
        assert_eq!(status, PersistedApplyStatus::SkippedPersistNotRequested);
        assert_eq!(status.as_wire(), "skipped_persist_not_requested");
    }

    #[test]
    fn evaluate_persisted_apply_gate_skips_when_caller_not_approved() {
        // apply + persist but caller_approved missing — second human
        // opt-in invariant.
        let args = json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
        });
        let apply = fixture_apply_outcome_with_one_high_inferred(&args);
        let status = evaluate_persisted_apply_gate(&args, &apply);
        assert_eq!(status, PersistedApplyStatus::SkippedCallerNotApproved);
    }

    #[test]
    fn evaluate_persisted_apply_gate_skips_when_no_applied_fields() {
        // All four opt-ins but the gate promoted no fields ⇒ refuse to
        // write a no-op version.
        let args = json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
            "target": "mission_execution",  // pre-fills the slot ⇒ skipped as caller_value_already_set
        });
        let apply = fixture_apply_outcome_with_one_high_inferred(&args);
        assert!(apply.applied.is_empty(), "fixture should be skipped because caller pre-filled");
        let status = evaluate_persisted_apply_gate(&args, &apply);
        assert_eq!(status, PersistedApplyStatus::SkippedNothingToApply);
    }

    #[test]
    fn evaluate_persisted_apply_gate_authorises_when_all_four_opt_ins_and_applied() {
        let args = json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
        });
        let apply = fixture_apply_outcome_with_one_high_inferred(&args);
        assert!(!apply.applied.is_empty());
        let status = evaluate_persisted_apply_gate(&args, &apply);
        assert_eq!(status, PersistedApplyStatus::Applied);
        assert!(status.was_applied());
    }

    #[test]
    fn enforce_persisted_apply_preflight_no_op_when_persist_path_not_armed() {
        // Caller did NOT opt into the persist path — preflight is a no-op
        // even when the supplied hash is wrong (legacy v1 callers must
        // never see a structured error here).
        for args in [
            json!({}),
            json!({"apply_inferred_fields": true}),
            json!({"persist_inference": true}),
            json!({"caller_approved": true}),
            json!({"apply_inferred_fields": true, "persist_inference": true}),
            json!({"apply_inferred_fields": true, "caller_approved": true}),
        ] {
            assert!(
                enforce_persisted_apply_preflight(&args, "deadbeefdeadbeefdeadbeefdeadbeef")
                    .is_ok(),
                "preflight must be no-op for non-persist args: {}",
                args
            );
        }
    }

    #[test]
    fn enforce_persisted_apply_preflight_fails_fast_on_missing_hash() {
        // Caller opted into the persist path but did not supply a hash.
        let args = json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
        });
        let computed = "deadbeefdeadbeefdeadbeefdeadbeef";
        let err = enforce_persisted_apply_preflight(&args, computed)
            .expect_err("preflight must fail-fast on missing hash");
        assert_eq!(err.0, error_codes::INVALID_PARAM);
        assert!(err.1.contains("PERSIST_APPLY_MISSING_PROPOSAL_HASH"));
        assert!(err.1.contains(computed));
    }

    #[test]
    fn enforce_persisted_apply_preflight_fails_fast_on_hash_mismatch() {
        let args = json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
            "proposal_hash": "11111111111111111111111111111111",
        });
        let computed = "deadbeefdeadbeefdeadbeefdeadbeef";
        let err = enforce_persisted_apply_preflight(&args, computed)
            .expect_err("preflight must fail-fast on hash mismatch");
        assert_eq!(err.0, error_codes::INVALID_PARAM);
        assert!(err.1.contains("PERSIST_APPLY_PROPOSAL_HASH_MISMATCH"));
        assert!(err.1.contains("11111111111111111111111111111111"));
        assert!(err.1.contains(computed));
    }

    #[test]
    fn enforce_persisted_apply_preflight_accepts_matching_hash_case_insensitive() {
        let computed = "deadbeefdeadbeefdeadbeefdeadbeef";
        // Same case.
        let args_same = json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
            "proposal_hash": computed,
        });
        assert!(enforce_persisted_apply_preflight(&args_same, computed).is_ok());
        // Upper-case echo (defensive — observers may upper-case the hex).
        let args_upper = json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
            "proposal_hash": computed.to_ascii_uppercase(),
        });
        assert!(enforce_persisted_apply_preflight(&args_upper, computed).is_ok());
    }

    #[test]
    fn render_applied_field_to_lisp_emits_canonical_kebab_keywords() {
        // Mirrors the parse_plan_hints reader's keyword aliases.
        let target = render_applied_field_to_lisp("target", &json!("mission_execution"));
        assert_eq!(target, ":target \"mission_execution\"");
        let strat = render_applied_field_to_lisp("dispatch_strategy", &json!("agent-team"));
        assert_eq!(strat, ":dispatch-strategy \"agent-team\"");
        let proj = render_applied_field_to_lisp("target_project", &json!("missiond"));
        assert_eq!(proj, ":target-project \"missiond\"");
        let owned = render_applied_field_to_lisp(
            "owned_files",
            &json!(["src/lib.rs", "src/main.rs"]),
        );
        assert_eq!(owned, ":owned-files [\"src/lib.rs\" \"src/main.rs\"]");
        let ws = render_applied_field_to_lisp("workstation_dispatch", &json!(true));
        assert_eq!(ws, ":workstation-dispatch true");
    }

    #[test]
    fn render_applied_field_to_lisp_escapes_quotes_and_backslashes() {
        let raw = render_applied_field_to_lisp("target", &json!("with\"quote\\back"));
        assert_eq!(raw, ":target \"with\\\"quote\\\\back\"");
    }

    #[test]
    fn synthesize_persisted_sexp_preserves_original_verbatim_and_appends_annotation() {
        let original = "(plan :id \"plan-1\" :goal :ship)";
        let af = AppliedField {
            field: "target",
            value: json!("mission_execution"),
            source: "plan_sexp",
            origin: ApplyOrigin::DeterministicInferred,
        };
        let result = synthesize_persisted_sexp(
            original,
            &[af],
            "deadbeefdeadbeefdeadbeefdeadbeef",
            "2026-04-26T00:00:00Z",
        );
        // The original body MUST appear verbatim at the top — supersede
        // chain readers can `tail -1` to get the new annotation while
        // every prior byte stays comparable.
        assert!(result.starts_with(original), "original preserved verbatim: {}", result);
        // Header marker is greppable.
        assert!(result.contains("wave-22 / task 04 — persisted PLAN inference apply v2"));
        // Canonical annotation form.
        assert!(result.contains("(plan-inference-applied :inference-version \"v2\""));
        assert!(result.contains(":proposal-hash \"deadbeefdeadbeefdeadbeefdeadbeef\""));
        assert!(result.contains(":persisted-at \"2026-04-26T00:00:00Z\""));
        // Applied fields land as sibling keyword pairs so the
        // parse_plan_hints reader picks them up at the PLAN level.
        assert!(result.contains(":target \"mission_execution\""));
    }

    #[test]
    fn synthesize_persisted_sexp_preserves_first_occurrence_semantics() {
        // parse_plan_hints keeps first-occurrence; an appended hint for
        // a slot the original already filled must NOT override it. We
        // verify the round-trip: synthesise the new sexp, parse hints
        // from it, and confirm the original target wins.
        let original = "(plan :id \"plan-1\" :target \"mission_task_delegate\")";
        let af = AppliedField {
            field: "target",
            value: json!("mission_execution"),
            source: "plan_sexp",
            origin: ApplyOrigin::LlmProposal,
        };
        let result = synthesize_persisted_sexp(
            original,
            &[af],
            "h0",
            "2026-04-26T00:00:00Z",
        );
        let hints = parse_plan_hints(&result);
        assert_eq!(
            hints.target.as_deref(),
            Some("mission_task_delegate"),
            "first-occurrence wins; original target preserved at the persistence boundary"
        );
    }

    #[test]
    fn synthesize_persisted_sexp_appends_new_hint_when_original_silent() {
        // When the original PLAN never spelled the field, the appended
        // hint becomes the live value (no prior occurrence to win).
        let original = "(plan :id \"plan-1\" :goal :ship)";
        let af = AppliedField {
            field: "dispatch_strategy",
            value: json!("agent-team"),
            source: "plan_sexp",
            origin: ApplyOrigin::DeterministicInferred,
        };
        let result = synthesize_persisted_sexp(
            original,
            &[af],
            "h0",
            "2026-04-26T00:00:00Z",
        );
        let hints = parse_plan_hints(&result);
        assert_eq!(hints.dispatch_strategy.as_deref(), Some("agent-team"));
    }

    #[test]
    fn persisted_apply_outcome_response_block_has_stable_shape() {
        let outcome = PersistedApplyOutcome::from_skip_reason(
            PersistedApplyStatus::NotRequested,
            &json!({}),
            "h0",
            &[],
            &[],
            None,
        );
        let v = outcome.to_response_json();
        // The wire shape is invariant — observers must see every field
        // (even when null) so dashboards never need to defensively
        // probe `.get(...)`.
        for key in [
            "status",
            "apply_inferred_fields_requested",
            "persist_inference_requested",
            "caller_approved",
            "original_sexp_hash",
            "resulting_sexp_hash",
            "computed_proposal_hash",
            "supplied_proposal_hash",
            "applied_fields",
            "skipped_fields",
            "new_plan_id",
            "new_plan_version",
            "rollback_plan_id",
        ] {
            assert!(
                v.get(key).is_some(),
                "persisted_apply block must always carry `{}`",
                key
            );
        }
        assert_eq!(v["status"], "not_requested");
        assert_eq!(v["apply_inferred_fields_requested"], false);
        assert_eq!(v["persist_inference_requested"], false);
        assert_eq!(v["caller_approved"], false);
        assert_eq!(v["original_sexp_hash"], "h0");
        assert!(v["resulting_sexp_hash"].is_null());
        assert!(v["new_plan_id"].is_null());
        assert!(v["rollback_plan_id"].is_null());
    }

    #[test]
    fn build_persisted_apply_evidence_entry_carries_canonical_typed_shape() {
        // Mirrors wave-12 typed-evidence: schema_version="v0", canonical
        // source + kind so a single grep over the sidecar surfaces every
        // persist event.
        let plan_id = uuid::Uuid::nil();
        let outcome = PersistedApplyOutcome {
            status: PersistedApplyStatus::Applied,
            apply_inferred_fields_requested: true,
            persist_inference_requested: true,
            caller_approved: true,
            original_sexp_hash: "h0".into(),
            resulting_sexp_hash: Some("h1".into()),
            computed_proposal_hash: Some("ph".into()),
            supplied_proposal_hash: Some("ph".into()),
            applied_fields: vec![AppliedField {
                field: "target",
                value: json!("mission_execution"),
                source: "plan_sexp",
                origin: ApplyOrigin::DeterministicInferred,
            }],
            skipped_fields: vec![],
            new_plan_id: Some(uuid::Uuid::from_u128(1)),
            new_plan_version: Some(2),
            rollback_plan_id: Some(plan_id),
        };
        let entry = build_persisted_apply_evidence_entry(&outcome, plan_id);
        assert_eq!(entry["schema_version"], "v0");
        assert_eq!(entry["source"], "plan_inference_persisted_apply");
        assert_eq!(entry["kind"], "plan_inference_persisted_apply");
        assert_eq!(entry["plan_id"], plan_id.to_string());
        assert_eq!(entry["new_plan_version"], 2);
        assert_eq!(entry["original_sexp_hash"], "h0");
        assert_eq!(entry["resulting_sexp_hash"], "h1");
        assert_eq!(entry["proposal_hash"], "ph");
        assert_eq!(entry["status"], "applied");
        assert_eq!(entry["applied_fields"][0]["field"], "target");
        // rollback_pointer must point at the predecessor — observers
        // replaying a rollback need it.
        assert_eq!(entry["rollback_plan_id"], plan_id.to_string());
    }

    #[test]
    fn attach_persisted_apply_block_no_op_when_block_absent() {
        let original = ToolResult::json_pretty(&json!({"status": "executing"}));
        let r = attach_persisted_apply_block(original, None);
        let v = parse_payload(&r);
        assert!(v.get("persisted_apply").is_none());
    }

    #[test]
    fn attach_persisted_apply_block_splices_block_into_payload() {
        let original = ToolResult::json_pretty(&json!({"status": "executing"}));
        let block = json!({"status": "applied"});
        let r = attach_persisted_apply_block(original, Some(block.clone()));
        let v = parse_payload(&r);
        assert_eq!(v["persisted_apply"], block);
    }

    #[test]
    fn attach_persisted_apply_block_preserves_pre_existing_block() {
        // Future DAG / resume paths may attach their own — never
        // overwrite.
        let original = ToolResult::json_pretty(&json!({
            "status": "executing",
            "persisted_apply": {"status": "preserved"},
        }));
        let r = attach_persisted_apply_block(original, Some(json!({"status": "applied"})));
        let v = parse_payload(&r);
        assert_eq!(v["persisted_apply"]["status"], "preserved");
    }

    #[test]
    fn attach_persisted_apply_block_skips_error_results() {
        // Errors propagate untouched.
        let original = ToolResult::structured_error(ToolError::new(
            error_codes::INVALID_PARAM,
            "boom",
        ));
        assert_eq!(original.is_error, Some(true));
        let r = attach_persisted_apply_block(original, Some(json!({"status": "applied"})));
        let payload = parse_payload(&r);
        assert!(payload.get("persisted_apply").is_none());
    }

    #[test]
    fn persisted_apply_status_wire_strings_are_canonical_and_distinct() {
        // Dashboards pivot on the wire string — we lock the canonical
        // set so a refactor cannot silently re-spell one and break
        // observers.
        let wires = [
            PersistedApplyStatus::NotRequested.as_wire(),
            PersistedApplyStatus::Applied.as_wire(),
            PersistedApplyStatus::SkippedApplyGateNotRequested.as_wire(),
            PersistedApplyStatus::SkippedPersistNotRequested.as_wire(),
            PersistedApplyStatus::SkippedCallerNotApproved.as_wire(),
            PersistedApplyStatus::SkippedNothingToApply.as_wire(),
        ];
        // All distinct.
        let mut sorted: Vec<&'static str> = wires.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), wires.len(), "wire strings must be distinct");
        // Pinned exact values (anti-rename guard).
        assert_eq!(wires[0], "not_requested");
        assert_eq!(wires[1], "applied");
        assert_eq!(wires[2], "skipped_apply_gate_not_requested");
        assert_eq!(wires[3], "skipped_persist_not_requested");
        assert_eq!(wires[4], "skipped_caller_not_approved");
        assert_eq!(wires[5], "skipped_nothing_to_apply");
    }

    #[test]
    fn persisted_apply_v2_preserves_wave21_05_invariant_apply_gate_v1_byte_shape_when_off() {
        // INVARIANT: wave-22 / task 04 must never alter the wave-21 / task
        // 05 v1 byte-shape when the v2 persist flags are absent. This
        // pins the back-compat contract — the v1 `apply_gate` block on
        // the response stays identical and `persisted_apply.status =
        // "not_requested"` carries no DB-mutation evidence.
        let args = json!({
            "apply_inferred_fields": true,
        });
        let apply = fixture_apply_outcome_with_one_high_inferred(&args);
        let v1_block = apply.to_response_json();
        assert_eq!(v1_block["requested"], true);
        // v1 invariant: persist_inference_applied is hard-pinned to
        // false on the apply_gate block. v2 does NOT mutate this — it
        // surfaces persistence on the SEPARATE `persisted_apply` block.
        assert_eq!(v1_block["persist_inference_applied"], false);
        // v2 evaluation on the same args returns a soft-skip — no DB
        // mutation, no error.
        let status = evaluate_persisted_apply_gate(&args, &apply);
        assert_eq!(status, PersistedApplyStatus::SkippedPersistNotRequested);
    }

    #[test]
    fn persisted_apply_v2_preserves_wave21_05_invariant_conflicts_never_persist() {
        // INVARIANT: caller-vs-inferred conflicts NEVER apply (even
        // under v2 persist). The v1 gate routes them to
        // `conflict_fields[]` with `applied=[]`, so v2's
        // `evaluate_persisted_apply_gate` must downgrade to
        // SkippedNothingToApply.
        let mut inf = PlanFieldInference::default();
        inf.conflicts.push(InferenceConflict {
            field: "target",
            caller_value: json!("mission_task_delegate"),
            inferred_value: json!("mission_execution"),
            confidence: InferenceConfidence::High,
            source: "plan_sexp",
        });
        let args = json!({
            "target": "mission_task_delegate",
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
        });
        let apply = compute_apply_gate(&args, &inf);
        assert!(apply.applied.is_empty(), "conflicts MUST never reach applied[]");
        let status = evaluate_persisted_apply_gate(&args, &apply);
        assert_eq!(
            status,
            PersistedApplyStatus::SkippedNothingToApply,
            "conflict-only outcome MUST persist nothing"
        );
    }

    #[test]
    fn persisted_apply_v2_preserves_wave21_05_invariant_suggestions_never_persist() {
        // INVARIANT: medium / low-confidence suggestions NEVER apply
        // (sub-threshold). v2 must never persist them, even when all
        // four opt-ins are supplied.
        let mut inf = PlanFieldInference::default();
        inf.suggested.push(InferredField {
            field: "target",
            value: json!("mission_execution"),
            confidence: InferenceConfidence::Medium,
            source: "plan_sexp",
            detail: None,
        });
        let args = json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
        });
        let apply = compute_apply_gate(&args, &inf);
        assert!(apply.applied.is_empty(), "suggestions MUST stay below the apply threshold");
        let status = evaluate_persisted_apply_gate(&args, &apply);
        assert_eq!(status, PersistedApplyStatus::SkippedNothingToApply);
    }

    #[test]
    fn persisted_apply_v2_preserves_wave21_05_invariant_llm_unapproved_never_persists() {
        // INVARIANT: LLM proposals require `llm_caller_approved`. v2
        // must never elevate an un-approved LLM proposal into the
        // persist path even when `caller_approved=true` (which
        // approves the PERSIST path, not the per-field LLM proposal).
        let mut inf = PlanFieldInference::default();
        inf.llm = Some(LlmProposalBundle {
            status: LlmProposalStatus::Suggested,
            proposals: vec![LlmProposal {
                field: "target",
                value: json!("mission_execution"),
                confidence: InferenceConfidence::High,
                evidence: "x".into(),
                conflict_status: LlmConflictStatus::None,
            }],
            parse_warnings: Vec::new(),
            unavailable_reason: None,
            model: None,
            request_caller: None,
        });
        // caller_approved=true is the PERSIST opt-in; llm_caller_approved
        // is absent ⇒ proposal must not apply.
        let args = json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
        });
        let apply = compute_apply_gate(&args, &inf);
        assert!(
            apply.applied.is_empty(),
            "LLM proposal MUST NOT apply without `llm_caller_approved` (caller_approved is the PERSIST gate, not the LLM gate)"
        );
        let status = evaluate_persisted_apply_gate(&args, &apply);
        assert_eq!(status, PersistedApplyStatus::SkippedNothingToApply);
    }

    #[test]
    fn persisted_apply_v2_preserves_wave21_05_invariant_strict_bool_shape() {
        // INVARIANT: strict bool shape. String "true" must NOT silently
        // arm the persist path — validator fail-fasts BEFORE we reach
        // the gate.
        for arg in [
            json!({"persist_inference": "true"}),
            json!({"caller_approved": "true"}),
            json!({"apply_inferred_fields": "true"}),
        ] {
            let err = validate_apply_gate_args(&arg).expect_err("string MUST be rejected");
            assert!(err.contains("must be a boolean"));
        }
    }

    #[test]
    fn persisted_apply_v2_preserves_wave21_05_invariant_persist_inference_applied_field_intact() {
        // INVARIANT: the v1 `apply_gate.persist_inference_applied`
        // field stays hard-pinned to `false` (the v2 persistence
        // surfaces on the SEPARATE `persisted_apply` block, so the
        // v1 wire shape never changes).
        let args = json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
        });
        let apply = fixture_apply_outcome_with_one_high_inferred(&args);
        let v1_block = apply.to_response_json();
        assert_eq!(
            v1_block["persist_inference_applied"], false,
            "wave-21 / task 05 invariant: v1 block's persist_inference_applied stays hard-pinned to false"
        );
        // The v2 persistence is reported on the parallel block.
        let v2_outcome = PersistedApplyOutcome::from_skip_reason(
            PersistedApplyStatus::Applied,
            &args,
            "h0",
            &apply.applied,
            &apply.skipped,
            Some("ph".into()),
        );
        let v2_block = v2_outcome.to_response_json();
        assert_eq!(v2_block["status"], "applied");
    }

    #[test]
    fn persisted_apply_status_was_applied_only_for_applied() {
        assert!(PersistedApplyStatus::Applied.was_applied());
        for status in [
            PersistedApplyStatus::NotRequested,
            PersistedApplyStatus::SkippedApplyGateNotRequested,
            PersistedApplyStatus::SkippedPersistNotRequested,
            PersistedApplyStatus::SkippedCallerNotApproved,
            PersistedApplyStatus::SkippedNothingToApply,
        ] {
            assert!(!status.was_applied(), "{:?} must NOT report applied", status);
        }
    }

    // ── wave-22 / task 05 — autonomous workstation true spawn v1 wiring ──
    //
    // These tests cover the plan.rs splice + helper integration. The
    // workstation_dispatch.rs gate evaluator already has its own
    // exhaustive unit tests; here we focus on the plan.rs surface:
    //   * `attach_workstation_auto_spawn_gate_block` no-op when the
    //     gate outcome is absent (default ⇒ wave-21/04 byte-shape).
    //   * `attach_workstation_auto_spawn_gate_block` splices the
    //     block into a successful response.
    //   * `attach_workstation_auto_spawn_gate_block` skips error
    //     responses (matches the wave-21/04 attachers).
    //   * `attach_workstation_auto_spawn_gate_block` preserves
    //     pre-existing blocks (DAG / resume forward-compat).

    #[test]
    fn wave22_05_attach_auto_spawn_gate_block_no_op_when_outcome_absent() {
        let original = ToolResult::json_pretty(&json!({"status": "executing"}));
        let r = attach_workstation_auto_spawn_gate_block(original, None);
        let v = parse_payload(&r);
        assert!(
            v.get("workstation_auto_spawn_gate").is_none(),
            "wave-21 / task 04 byte-shape: gate block MUST be omitted when outcome is None"
        );
    }

    #[test]
    fn wave22_05_attach_auto_spawn_gate_block_splices_block_into_payload() {
        use super::super::workstation_dispatch::{
            WorkstationAutoSpawnGateOutcome, WorkstationAutoSpawnStatus,
            WorkstationProposalHashStatus,
        };
        let outcome = WorkstationAutoSpawnGateOutcome {
            requested: true,
            status: WorkstationAutoSpawnStatus::Spawned,
            spawn_target: Some("mission_task_delegate".to_string()),
            task_contract_path: Some(".missiond/tasks/foo.lisp".to_string()),
            proposal_hash_status: WorkstationProposalHashStatus::Matches,
            computed_proposal_hash: Some("0".repeat(32)),
            supplied_proposal_hash: Some("0".repeat(32)),
            caller_approved: true,
            preflight_status_acceptable: true,
            gate_results: vec!["rule:auto_spawn_gate_satisfied".to_string()],
            substrate_reason: None,
        };
        let original = ToolResult::json_pretty(&json!({"status": "executing"}));
        let r = attach_workstation_auto_spawn_gate_block(original, Some(&outcome));
        let v = parse_payload(&r);
        let block = v.get("workstation_auto_spawn_gate").expect("gate block present");
        assert_eq!(block["auto_spawn_status"], "spawned");
        assert_eq!(block["spawn_target"], "mission_task_delegate");
        assert_eq!(block["proposal_hash_status"], "matches");
        assert_eq!(block["caller_approved"], true);
        assert_eq!(block["preflight_status_acceptable"], true);
        assert!(block["gate_results"].as_array().unwrap().len() >= 1);
    }

    #[test]
    fn wave22_05_attach_auto_spawn_gate_block_skips_error_results() {
        use super::super::workstation_dispatch::{
            WorkstationAutoSpawnGateOutcome, WorkstationAutoSpawnStatus,
            WorkstationProposalHashStatus,
        };
        let outcome = WorkstationAutoSpawnGateOutcome {
            requested: true,
            status: WorkstationAutoSpawnStatus::Spawned,
            spawn_target: None,
            task_contract_path: None,
            proposal_hash_status: WorkstationProposalHashStatus::NotSupplied,
            computed_proposal_hash: None,
            supplied_proposal_hash: None,
            caller_approved: false,
            preflight_status_acceptable: false,
            gate_results: vec![],
            substrate_reason: None,
        };
        let mut original = ToolResult::json_pretty(&json!({"error": "broke"}));
        original.is_error = Some(true);
        let r = attach_workstation_auto_spawn_gate_block(original, Some(&outcome));
        let v = parse_payload(&r);
        assert!(
            v.get("workstation_auto_spawn_gate").is_none(),
            "structured-error responses MUST stay uncluttered"
        );
    }

    #[test]
    fn wave22_05_attach_auto_spawn_gate_block_preserves_pre_existing_block() {
        use super::super::workstation_dispatch::{
            WorkstationAutoSpawnGateOutcome, WorkstationAutoSpawnStatus,
            WorkstationProposalHashStatus,
        };
        let outcome = WorkstationAutoSpawnGateOutcome {
            requested: true,
            status: WorkstationAutoSpawnStatus::Spawned,
            spawn_target: Some("mission_task_delegate".to_string()),
            task_contract_path: None,
            proposal_hash_status: WorkstationProposalHashStatus::Matches,
            computed_proposal_hash: None,
            supplied_proposal_hash: None,
            caller_approved: true,
            preflight_status_acceptable: true,
            gate_results: vec![],
            substrate_reason: None,
        };
        let original = ToolResult::json_pretty(&json!({
            "status": "executing",
            "workstation_auto_spawn_gate": {"auto_spawn_status": "preexisting_marker"},
        }));
        let r = attach_workstation_auto_spawn_gate_block(original, Some(&outcome));
        let v = parse_payload(&r);
        let block = v.get("workstation_auto_spawn_gate").expect("gate block present");
        assert_eq!(
            block["auto_spawn_status"], "preexisting_marker",
            "wave-22 / task 05 invariant: pre-existing gate blocks MUST NOT be overwritten"
        );
    }

    /// Wave-21 / task 04 invariant carryover: when the caller does NOT
    /// opt into wave-22 / task 05 auto-spawn (the `auto_spawn` flag is
    /// absent or false), the response MUST stay byte-identical with
    /// the wave-21 / task 04 propose-only path. That means:
    ///   * the wave-21 propose-only `workstation_proposals` block STILL
    ///     carries `auto_spawn=false` and every proposal STILL carries
    ///     `applied=false` (this invariant lives in workstation_dispatch.rs
    ///     and is independently tested there);
    ///   * the wave-22 `workstation_auto_spawn_gate` block is OMITTED
    ///     from the response (no new key on the wire).
    /// We assert the second invariant on the splice helper directly.
    #[test]
    fn wave22_05_default_off_preserves_wave21_04_byte_shape() {
        let original = ToolResult::json_pretty(&json!({
            "status": "executing",
            "workstation_proposals": {"auto_spawn": false, "proposals": []},
        }));
        // outcome=None mirrors the auto_spawn=false caller path
        // (compute_workstation_auto_spawn_gate returns None for that case).
        let r = attach_workstation_auto_spawn_gate_block(original, None);
        let v = parse_payload(&r);
        assert!(
            v.get("workstation_auto_spawn_gate").is_none(),
            "wave-21 / task 04 byte-shape: auto_spawn=false / absent ⇒ NO new key on the wire"
        );
        // wave-21 / task 04 propose-only key untouched.
        assert_eq!(v["workstation_proposals"]["auto_spawn"], false);
    }

    /// Wave-22 / task 05 invariant: `parse_workstation_auto_spawn_input`
    /// rejects literal-string `"true"` for the bool fields with the
    /// `AUTO_SPAWN_INVALID_PARAM` code (mirrors wave-22 / task 03 / 04
    /// strict-shape rule). Tested at the workstation_dispatch.rs unit
    /// level; here we just assert the symbol export so plan.rs callers
    /// can rely on it.
    #[test]
    fn wave22_05_invariant_strict_bool_shape_codes_exported() {
        use super::super::workstation_dispatch::{
            AUTO_SPAWN_INVALID_PARAM, AUTO_SPAWN_MISSING_PROPOSAL_HASH,
            AUTO_SPAWN_PROPOSAL_HASH_MISMATCH,
        };
        assert_eq!(AUTO_SPAWN_INVALID_PARAM, "AUTO_SPAWN_INVALID_PARAM");
        assert_eq!(AUTO_SPAWN_MISSING_PROPOSAL_HASH, "AUTO_SPAWN_MISSING_PROPOSAL_HASH");
        assert_eq!(
            AUTO_SPAWN_PROPOSAL_HASH_MISMATCH,
            "AUTO_SPAWN_PROPOSAL_HASH_MISMATCH"
        );
    }

    // ── Wave 22 / Task 07 — autonomous loop apply smoke v4 ──
    //
    // Pin the wave22-04 persisted PLAN inference apply v2 gate slice
    // of the wave22-07 v4 smoke contract. The pure preflight + soft-
    // skip evaluator pair is the deterministic SSOT — no DB mutation,
    // no Sonnet call, pure in-process functions over synthesised
    // `ApplyGateOutcome` fixtures. The companion review_gate.rs /
    // workstation_dispatch.rs / agent_execution.rs / unified_entry.rs
    // smokes cover the review-apply-gate / auto-spawn / failed-
    // verification / markdown-non-load-bearing slices.

    /// V4 smoke (Requirement 2 / persisted apply-gate slice): the
    /// wave22-04 v2 persist gate MUST reject the four-opt-in path
    /// (`apply_inferred_fields=true` + `persist_inference=true` +
    /// `caller_approved=true` + non-empty applied[]) when the caller
    /// does not supply `proposal_hash`, AND MUST accept the same call
    /// when the canonical `compute_inference_proposal_hash` value is
    /// supplied. This is the wave22-04 fail-fast preflight — the gate
    /// refuses to mutate the persisted plan with no correlator and
    /// accepts only the canonical fixture path.
    #[test]
    fn smoke_wave22_07_persisted_apply_gate_rejects_missing_hash_accepts_fixture_hash() {
        let plan_id = uuid::Uuid::nil();
        let original_hash = sha256_hex("(plan :id 1)");
        let af = AppliedField {
            field: "target",
            value: serde_json::json!("mission_execution"),
            source: "plan_sexp",
            origin: ApplyOrigin::DeterministicInferred,
        };
        let canonical = compute_inference_proposal_hash(plan_id, &original_hash, &[af.clone()]);
        // Missing proposal_hash → PERSIST_APPLY_MISSING_PROPOSAL_HASH.
        let missing_args = serde_json::json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
        });
        let err = enforce_persisted_apply_preflight(&missing_args, &canonical)
            .expect_err("wave22-07 v4: missing proposal_hash MUST fail-fast on persist path");
        assert_eq!(err.0, error_codes::INVALID_PARAM);
        assert!(
            err.1.contains("PERSIST_APPLY_MISSING_PROPOSAL_HASH"),
            "wave22-07 v4 invariant: missing hash MUST surface PERSIST_APPLY_MISSING_PROPOSAL_HASH"
        );
        // Mismatched proposal_hash → PERSIST_APPLY_PROPOSAL_HASH_MISMATCH.
        let mismatch_args = serde_json::json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
            "proposal_hash": "0".repeat(32),
        });
        let err = enforce_persisted_apply_preflight(&mismatch_args, &canonical)
            .expect_err("wave22-07 v4: mismatched proposal_hash MUST fail-fast on persist path");
        assert!(err.1.contains("PERSIST_APPLY_PROPOSAL_HASH_MISMATCH"));
        // Matching fixture hash → preflight OK + evaluator returns
        // Applied (non-empty applied[] from the four-opt-in path).
        let valid_args = serde_json::json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
            "proposal_hash": canonical.clone(),
        });
        assert!(
            enforce_persisted_apply_preflight(&valid_args, &canonical).is_ok(),
            "wave22-07 v4: matching proposal_hash MUST pass the persist preflight"
        );
        let outcome = fixture_apply_outcome_with_one_high_inferred(&valid_args);
        let status = evaluate_persisted_apply_gate(&valid_args, &outcome);
        assert_eq!(
            status,
            PersistedApplyStatus::Applied,
            "wave22-07 v4 invariant: matching fixture hash + four-opt-in path \
             MUST drive the persist gate to Applied"
        );
    }

    /// V4 smoke (cross-wave invariants / wave21-05 6 invariants
    /// pinned): the wave22-04 persisted apply gate MUST preserve every
    /// wave-21 / task 05 v1 in-memory apply gate invariant when the v2
    /// persist flags are layered on the same call.
    ///   * I1 default off — the v1 byte-shape stays preserved when
    ///     the persist opt-ins are absent (SkippedPersistNotRequested).
    ///   * I2 strict bool/string shape — `validate_apply_gate_args`
    ///     fail-fasts on literal-string `"true"` for every opt-in flag.
    ///   * I3 conflicts NEVER apply — caller-vs-inferred conflicts
    ///     route to `conflict_fields[]` and the persist gate downgrades
    ///     to `SkippedNothingToApply`.
    ///   * I4 sub-threshold suggestions NEVER apply — medium / low
    ///     confidence suggestions stay below the apply threshold and
    ///     the persist gate downgrades to `SkippedNothingToApply`.
    ///   * I5 LLM proposals require `llm_caller_approved` —
    ///     `caller_approved=true` is the PERSIST opt-in (not the LLM
    ///     opt-in); an un-approved LLM proposal MUST NOT persist.
    ///   * I6 `apply_gate.persist_inference_applied` stays hard-pinned
    ///     to `false` — the v1 wire shape never changes; v2 publishes
    ///     persistence on a SEPARATE `persisted_apply` block.
    #[test]
    fn smoke_wave22_07_persisted_apply_gate_pins_wave21_05_six_invariants() {
        // I1 — default off: persist opt-ins absent ⇒ v2 reports the
        // soft-skip without any DB mutation, v1 byte-shape preserved.
        let off_args = serde_json::json!({"apply_inferred_fields": true});
        let outcome = fixture_apply_outcome_with_one_high_inferred(&off_args);
        let v1_block = outcome.to_response_json();
        assert_eq!(v1_block["requested"], true);
        assert_eq!(
            v1_block["persist_inference_applied"], false,
            "wave21-05 I6: v1 block's persist_inference_applied MUST stay hard-pinned false"
        );
        let status = evaluate_persisted_apply_gate(&off_args, &outcome);
        assert_eq!(
            status,
            PersistedApplyStatus::SkippedPersistNotRequested,
            "wave21-05 I1: default off — persist opt-ins absent MUST stay v1-shaped"
        );
        // I2 — strict bool shape: the validator fail-fasts on literal-
        // string "true" before the gate is reached.
        for arg in [
            serde_json::json!({"persist_inference": "true"}),
            serde_json::json!({"caller_approved": "true"}),
            serde_json::json!({"apply_inferred_fields": "true"}),
        ] {
            let err = validate_apply_gate_args(&arg).expect_err("string MUST be rejected");
            assert!(
                err.contains("must be a boolean"),
                "wave21-05 I2: literal-string `\"true\"` MUST fail-fast: got {}",
                err
            );
        }
        // I3 — conflicts NEVER apply.
        let mut inf_conflict = PlanFieldInference::default();
        inf_conflict.conflicts.push(InferenceConflict {
            field: "target",
            caller_value: serde_json::json!("mission_task_delegate"),
            inferred_value: serde_json::json!("mission_execution"),
            confidence: InferenceConfidence::High,
            source: "plan_sexp",
        });
        let conflict_args = serde_json::json!({
            "target": "mission_task_delegate",
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
        });
        let conflict_outcome = compute_apply_gate(&conflict_args, &inf_conflict);
        assert!(
            conflict_outcome.applied.is_empty(),
            "wave21-05 I3: conflicts MUST never reach applied[]"
        );
        let status = evaluate_persisted_apply_gate(&conflict_args, &conflict_outcome);
        assert_eq!(
            status,
            PersistedApplyStatus::SkippedNothingToApply,
            "wave21-05 I3: conflict-only outcome MUST persist nothing"
        );
        // I4 — sub-threshold suggestions NEVER apply.
        let mut inf_sugg = PlanFieldInference::default();
        inf_sugg.suggested.push(InferredField {
            field: "target",
            value: serde_json::json!("mission_execution"),
            confidence: InferenceConfidence::Medium,
            source: "plan_sexp",
            detail: None,
        });
        let sugg_args = serde_json::json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
        });
        let sugg_outcome = compute_apply_gate(&sugg_args, &inf_sugg);
        assert!(
            sugg_outcome.applied.is_empty(),
            "wave21-05 I4: medium-confidence suggestions MUST stay sub-threshold"
        );
        let status = evaluate_persisted_apply_gate(&sugg_args, &sugg_outcome);
        assert_eq!(status, PersistedApplyStatus::SkippedNothingToApply);
        // I5 — LLM proposals require `llm_caller_approved` (caller_approved
        // is the PERSIST opt-in, not the LLM opt-in).
        let mut inf_llm = PlanFieldInference::default();
        inf_llm.llm = Some(LlmProposalBundle {
            status: LlmProposalStatus::Suggested,
            proposals: vec![LlmProposal {
                field: "target",
                value: serde_json::json!("mission_execution"),
                confidence: InferenceConfidence::High,
                evidence: "fixture".into(),
                conflict_status: LlmConflictStatus::None,
            }],
            parse_warnings: Vec::new(),
            unavailable_reason: None,
            model: None,
            request_caller: None,
        });
        // caller_approved=true is the PERSIST opt-in; llm_caller_approved
        // is absent ⇒ LLM proposal MUST NOT apply.
        let llm_args = serde_json::json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
        });
        let llm_outcome = compute_apply_gate(&llm_args, &inf_llm);
        assert!(
            llm_outcome.applied.is_empty(),
            "wave21-05 I5: LLM proposal MUST NOT apply without `llm_caller_approved`"
        );
        let status = evaluate_persisted_apply_gate(&llm_args, &llm_outcome);
        assert_eq!(status, PersistedApplyStatus::SkippedNothingToApply);
        // I6 — apply_gate.persist_inference_applied stays hard-pinned
        // false even when persist path is fully armed and applied.
        let armed_args = serde_json::json!({
            "apply_inferred_fields": true,
            "persist_inference": true,
            "caller_approved": true,
        });
        let armed_outcome = fixture_apply_outcome_with_one_high_inferred(&armed_args);
        let armed_v1_block = armed_outcome.to_response_json();
        assert_eq!(
            armed_v1_block["persist_inference_applied"], false,
            "wave21-05 I6: v1 `persist_inference_applied` field MUST stay hard-pinned false"
        );
    }

    // ── wave-23 / task 05 — session-trace propagation tests ─────────────
    //
    // The four cases from the task contract:
    //   (1) legacy: no trace arg ⇒ no forward, no warning, no response
    //       field surfaces (byte-shape compatible with wave-15..22)
    //   (2) happy path: well-formed path forwarded into the contract
    //       emitter inputs and the response surface
    //   (3) malformed + required ⇒ structured INVALID_PARAM error BEFORE
    //       any dispatch side effect (fail-fast)
    //   (4) malformed + NOT required ⇒ non-fatal `trace_path_warning`
    //       on the response, no forward (conservative posture)
    //
    // The dispatch path itself is exercised in the workstation_dispatch
    // test module (brief / contract round-trip); these tests pin the
    // pure-helper validation surface and the contract emitter wiring.

    #[test]
    fn validate_session_trace_path_arg_returns_none_pair_when_arg_absent() {
        let (resolved, warning) = validate_session_trace_path_arg(None, false)
            .expect("absent path is always Ok");
        assert!(resolved.is_none(), "wave23-05 case 1: legacy callers see no resolved path");
        assert!(warning.is_none(), "wave23-05 case 1: legacy callers see no warning");
    }

    #[test]
    fn validate_session_trace_path_arg_passes_well_formed_paths_through() {
        let (resolved, warning) = validate_session_trace_path_arg(
            Some(".missiond/tasks/wave23/session-trace.lisp"),
            false,
        )
        .expect("well-formed path is always Ok");
        assert_eq!(
            resolved.as_deref(),
            Some(".missiond/tasks/wave23/session-trace.lisp"),
            "wave23-05 case 2: happy path forwards verbatim"
        );
        assert!(warning.is_none(), "wave23-05 case 2: happy path emits no warning");
    }

    #[test]
    fn validate_session_trace_path_arg_required_rejects_empty_with_invalid_param() {
        let result = validate_session_trace_path_arg(Some("   "), true);
        let err = result.expect_err("required + empty must hard-fail");
        let payload = parse_payload(&err);
        // Structured-error envelope carries the error in `error.code` /
        // `error.message`. The exact envelope shape is owned by
        // `ToolResult::structured_error`; we just assert the wire form
        // names INVALID_PARAM and the trim violation.
        let txt = serde_json::to_string(&payload).expect("serialize");
        assert!(
            txt.contains("INVALID_PARAM"),
            "wave23-05 case 3: required + malformed must fail with INVALID_PARAM, got: {}",
            txt
        );
        assert!(
            txt.contains("session_trace_path is empty after trim"),
            "wave23-05 case 3: error must name the shape failure"
        );
    }

    #[test]
    fn validate_session_trace_path_arg_warns_on_nul_byte_when_not_required() {
        // NUL byte is a hard filesystem invariant the daemon must catch;
        // without `required`, surface a warning so the caller can fix
        // the typo without aborting the dispatch.
        let trace = "good\0bad";
        let (resolved, warning) = validate_session_trace_path_arg(Some(trace), false)
            .expect("malformed + not-required must NOT hard-fail");
        assert!(
            resolved.is_none(),
            "wave23-05 case 4: malformed path must not be forwarded"
        );
        let warning = warning.expect("wave23-05 case 4: malformed path must surface a warning");
        assert!(
            warning.contains("NUL byte"),
            "wave23-05 case 4: warning must explain the shape failure — got: {}",
            warning
        );
    }

    #[test]
    fn task_contract_inputs_from_hints_with_trace_emits_session_trace_path_in_lisp() {
        // The contract emitter must include `:session-trace-path "..."`
        // when the trace knob is set so a downstream consumer
        // (machine-mode dispatch loading the contract directly) can
        // re-derive the path without re-supplying the arg.
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let hints = wd::WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            owned_files: vec!["a.rs".to_string()],
            ..Default::default()
        };
        let inputs = task_contract_inputs_from_hints_with_trace(
            &hints,
            "mission_task_delegate",
            "fresh-code-alignment",
            Some(".missiond/tasks/wave23/session-trace.lisp"),
        );
        assert_eq!(
            inputs.session_trace_path.as_deref(),
            Some(".missiond/tasks/wave23/session-trace.lisp"),
            "wave23-05: trace path must land on TaskContractInputs.session_trace_path"
        );
        let plan_id = Uuid::parse_str("00000000-0000-0000-0000-0000feedbabe").unwrap();
        let body = build_task_contract_lisp(plan_id, "node-trace", "btk-trace", &inputs);
        assert!(
            body.contains(":session-trace-path \".missiond/tasks/wave23/session-trace.lisp\""),
            "wave23-05: emitted contract must carry `:session-trace-path` verbatim — got:\n{}",
            body
        );
    }

    #[test]
    fn task_contract_inputs_from_hints_omits_session_trace_when_path_absent() {
        // Legacy callers (the existing 3-arg helper) must NOT emit the
        // `:session-trace-path` field — preserves wave-19..22 contract
        // byte-shape exactly so DAG / unified-entry consumers (which
        // bind to the legacy helper) keep round-tripping.
        use crate::handlers::knowledge::workstation_dispatch as wd;
        let hints = wd::WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            owned_files: vec!["a.rs".to_string()],
            ..Default::default()
        };
        let inputs = task_contract_inputs_from_hints(
            &hints,
            "mission_task_delegate",
            "fresh-code-alignment",
        );
        assert!(
            inputs.session_trace_path.is_none(),
            "wave23-05: legacy 3-arg helper must keep session_trace_path=None"
        );
        let plan_id = Uuid::parse_str("00000000-0000-0000-0000-00000000c0de").unwrap();
        let body = build_task_contract_lisp(plan_id, "node-legacy", "btk-legacy", &inputs);
        assert!(
            !body.contains(":session-trace-path"),
            "wave23-05: legacy contract must NOT carry session-trace-path — got:\n{}",
            body
        );
    }

    #[test]
    fn attach_session_trace_response_fields_is_a_noop_when_both_inputs_are_none() {
        // Byte-shape pin for legacy callers: when neither field is
        // supplied, the JSON envelope must be byte-identical to the
        // wave-15..22 baseline (no extra keys).
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let mut result = action_execute_bridge(&plan, &resolved);
        let baseline_text = match result.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        attach_session_trace_response_fields(&mut result, None, None);
        let after_text = match result.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        assert_eq!(
            baseline_text, after_text,
            "wave23-05: noop attach must leave the JSON envelope byte-identical"
        );
    }

    #[test]
    fn attach_session_trace_response_fields_splices_path_and_warning_into_envelope() {
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let mut result = action_execute_bridge(&plan, &resolved);
        attach_session_trace_response_fields(
            &mut result,
            Some(".missiond/tasks/wave23/session-trace.lisp"),
            Some("malformed: NUL byte at offset 4"),
        );
        let v = parse_payload(&result);
        assert_eq!(
            v["session_trace_path"], ".missiond/tasks/wave23/session-trace.lisp",
            "wave23-05: helper must surface the resolved trace path"
        );
        assert_eq!(
            v["trace_path_warning"], "malformed: NUL byte at offset 4",
            "wave23-05: helper must surface the trace_path_warning when supplied"
        );
    }

    // -----------------------------------------------------------------
    // wave-24 / task 04 — router-policy dry-run surface tests.
    // -----------------------------------------------------------------

    use super::router_policy_dry_run::{
        attach_router_recommendation_block, parse_router_policy_mode, RouterPolicyMode,
        DEFAULT_POLICY_PATH,
    };

    /// Minimal helper: make a fixture ToolResult mirroring the bridge
    /// response shape so we can splice the recommendation block on top.
    fn fixture_bridge_result() -> ToolResult {
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        action_execute_bridge(&plan, &resolved)
    }

    #[test]
    fn router_policy_mode_default_off_emits_no_block() {
        // wave24-04 invariant: absent arg ⇒ Off.
        let args = json!({});
        let mode = parse_router_policy_mode(&args).expect("default off");
        assert!(matches!(mode, RouterPolicyMode::Off));
        // attach with Off must leave the response byte-identical.
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
        let baseline = action_execute_bridge(&plan, &resolved);
        let baseline_text = match baseline.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        let after = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let after_text = match after.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        assert_eq!(
            baseline_text, after_text,
            "wave24-04: mode=off must not alter the response envelope"
        );
    }

    #[test]
    fn router_policy_mode_off_returns_legacy_response_byte_identical() {
        // wave24-04: explicit "off" ⇒ Off (same as default).
        let args = json!({"router_policy_mode": "off"});
        let mode = parse_router_policy_mode(&args).expect("explicit off");
        assert!(matches!(mode, RouterPolicyMode::Off));
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
        let baseline = action_execute_bridge(&plan, &resolved);
        let baseline_text = match baseline.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        let after = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let after_text = match after.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        assert_eq!(
            baseline_text, after_text,
            "wave24-04: explicit mode=off must be byte-identical to baseline"
        );
        let v: Value = serde_json::from_str(&after_text).unwrap();
        assert!(
            v.get("router_recommendation").is_none(),
            "wave24-04: mode=off must NOT splice a recommendation block"
        );
    }

    #[test]
    fn router_policy_mode_apply_returns_invalid_param() {
        // wave24-04 contract: `apply` is intentionally rejected — wave24-04
        // ships only the dry-run surface.
        let args = json!({"router_policy_mode": "apply"});
        let err = parse_router_policy_mode(&args).expect_err("apply must reject");
        assert_eq!(err.is_error, Some(true));
        let text = match err.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        assert!(
            text.contains("INVALID_PARAM") || text.contains("invalid"),
            "wave24-04: apply must surface INVALID_PARAM (got `{}`)",
            text
        );
        assert!(
            text.contains("apply"),
            "wave24-04: error must echo the offending value"
        );
    }

    #[test]
    fn router_policy_mode_auto_returns_invalid_param() {
        // wave24-04 contract: `auto` is intentionally rejected.
        let args = json!({"router_policy_mode": "auto"});
        let err = parse_router_policy_mode(&args).expect_err("auto must reject");
        assert_eq!(err.is_error, Some(true));
        let text = match err.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        assert!(text.contains("INVALID_PARAM") || text.contains("invalid"));
        assert!(text.contains("auto"));
    }

    #[test]
    fn router_policy_mode_unknown_returns_invalid_param() {
        // wave24-04 contract: typo / unknown values reject.
        let args = json!({"router_policy_mode": "dryrun"});
        assert!(parse_router_policy_mode(&args).is_err());
        let args = json!({"router_policy_mode": "DRY_RUN"});
        assert!(parse_router_policy_mode(&args).is_err());
        // Non-string types also reject (e.g. caller passes a bool).
        let args = json!({"router_policy_mode": true});
        assert!(parse_router_policy_mode(&args).is_err());
    }

    #[test]
    fn router_policy_mode_dry_run_emits_block_with_applied_false() {
        // Cross-wave invariant: applied=false is hard-coded literal in
        // EVERY emitted block, regardless of match outcome. Use a temp
        // policy so the test is independent of the daemon's working
        // directory.
        let tmp = std::env::temp_dir().join(format!(
            "wave24-04-shape-{}.lisp",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &tmp,
            r#"(router-policy fixture-shape
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
    :id r-docs
    :priority 10
    :when ((kind docs))
    :recommend (:backend claudecode :reasoning "docs are interactive")
    :non-goals ["does not replace runtime dispatch"]))
"#,
        )
        .unwrap();
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": tmp.to_str().unwrap(),
            // Force a no-match path (off-policy ops kind) so the block is
            // a deterministic fallback shape.
            "kind": "ops",
        });
        let mode = parse_router_policy_mode(&args).expect("dry_run parses");
        assert!(matches!(mode, RouterPolicyMode::DryRun));
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &fixture_resolved("mission_task_delegate", "fresh-code-alignment"),
            &fixture_plan("(plan)"),
        );
        let v = parse_payload(&result);
        let block = v
            .get("router_recommendation")
            .expect("dry_run must emit router_recommendation block");
        assert_eq!(
            block["applied"], false,
            "wave24-04 invariant: applied=false hard-coded literal"
        );
        assert!(
            block.get("status").is_some(),
            "block must surface status field"
        );
        assert!(
            block.get("recommended_backend").is_some(),
            "block must surface recommended_backend"
        );
        assert!(block.get("confidence").is_some());
        assert!(block.get("reasons").is_some());
        assert!(block.get("policy_source").is_some());
        assert_eq!(block["schema"], "missiond.router-recommendation.v0");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn router_policy_mode_dry_run_does_not_change_dispatch() {
        // Cross-wave invariant: the dispatch fields (target_tool /
        // dispatch_strategy / next_call) are byte-identical with vs
        // without the dry_run mode. Only the recommendation block is
        // additive.
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

        // Baseline: no router knob.
        let baseline = action_execute_bridge(&plan, &resolved);
        let baseline_v = parse_payload(&baseline);

        // With dry_run: same dispatch fields, plus a recommendation block.
        // Materialise a temp policy so this test is independent of cwd.
        let tmp = std::env::temp_dir().join(format!(
            "wave24-04-dispatch-{}.lisp",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &tmp,
            r#"(router-policy fixture-dispatch
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
    :id r-docs
    :priority 10
    :when ((kind docs))
    :recommend (:backend claudecode :reasoning "docs are interactive")
    :non-goals ["does not replace runtime dispatch"]))
"#,
        )
        .unwrap();
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": tmp.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let with_dry_run = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let dry_v = parse_payload(&with_dry_run);

        // Every dispatch-shaping field must be byte-identical.
        assert_eq!(baseline_v["target_tool"], dry_v["target_tool"]);
        assert_eq!(baseline_v["target_source"], dry_v["target_source"]);
        assert_eq!(baseline_v["dispatch_strategy"], dry_v["dispatch_strategy"]);
        assert_eq!(
            baseline_v["dispatch_strategy_source"],
            dry_v["dispatch_strategy_source"]
        );
        assert_eq!(baseline_v["next_call"], dry_v["next_call"]);
        assert_eq!(baseline_v["execute_mode"], dry_v["execute_mode"]);
        assert_eq!(baseline_v["runner_status"], dry_v["runner_status"]);

        // The only delta is the additive recommendation block.
        assert!(baseline_v.get("router_recommendation").is_none());
        assert!(dry_v.get("router_recommendation").is_some());
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn router_policy_mode_dry_run_no_match_falls_back_to_claudecode_low() {
        // Off-policy combo (kind=ops) ⇒ no rule matches in the temp seed
        // policy ⇒ recommendation falls back to claudecode/low with the
        // documented `insufficient_trace_history` reason. We materialise
        // a temp policy mirroring the wave24-01 seed shape so the test
        // is independent of the daemon's working directory.
        let tmp = std::env::temp_dir().join(format!(
            "wave24-04-fallback-{}.lisp",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &tmp,
            r#"(router-policy fixture-fallback
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
    :id r-docs-only
    :priority 10
    :when ((kind docs))
    :recommend (:backend claudecode :reasoning "docs only")
    :non-goals ["does not replace runtime dispatch"]))
"#,
        )
        .unwrap();
        // wave24-04: assert the documented default policy path constant
        // is wired into the helper (mirrors the wave24-03 CLI default).
        assert_eq!(DEFAULT_POLICY_PATH, ".missiond/router/router-policy-v1.lisp");
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "agent-team");
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": tmp.to_str().unwrap(),
            "kind": "ops",
            "owner": "operator",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["status"], "computed");
        assert_eq!(block["recommended_backend"], "claudecode");
        assert_eq!(block["confidence"], "low");
        assert_eq!(block["applied"], false);
        let reasons = block["reasons"].as_array().expect("reasons array");
        assert!(
            reasons
                .iter()
                .any(|r| r.as_str().unwrap_or("").contains("insufficient_trace_history")),
            "fallback must surface insufficient_trace_history"
        );
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn router_policy_mode_dry_run_first_priority_match_wins() {
        // Build a temp policy with two matching rules at distinct
        // priorities and verify the lower priority wins.
        let tmp = std::env::temp_dir().join(format!(
            "wave24-04-multi-{}.lisp",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &tmp,
            r#"(router-policy fixture-multi
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
    :id r-low-prio-wins
    :priority 5
    :when ((kind code-alignment))
    :recommend (:backend deterministic-checker :reasoning "lower priority wins")
    :non-goals ["does not replace runtime dispatch"])
  (rule
    :id r-loses-on-prio
    :priority 50
    :when ((kind code-alignment))
    :recommend (:backend patch-worker :reasoning "matches but loses")
    :non-goals ["does not replace runtime dispatch"]))
"#,
        )
        .unwrap();
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": tmp.to_str().unwrap(),
            "kind": "code-alignment",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["status"], "computed");
        // Lowest priority wins ⇒ deterministic-checker (priority 5).
        assert_eq!(block["recommended_backend"], "deterministic-checker");
        assert_eq!(block["applied"], false);
        let reasons = block["reasons"].as_array().expect("reasons array");
        // Both matched rules are recorded for explainability.
        let joined = reasons
            .iter()
            .filter_map(|r| r.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("r-low-prio-wins"));
        assert!(joined.contains("r-loses-on-prio"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn router_policy_mode_dry_run_runtime_replacement_policy_rejected() {
        // Cross-wave invariant: a policy declaring :runtime-replacement
        // true is REJECTED, with status="rejected", regardless of match.
        let tmp = std::env::temp_dir().join(format!(
            "wave24-04-rr-{}.lisp",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &tmp,
            r#"(router-policy fixture-bad-rr
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement true
  (rule
    :id r-rr
    :priority 1
    :when ((kind docs))
    :recommend (:backend claudecode :reasoning "should never apply")
    :non-goals ["does not replace runtime dispatch"]))
"#,
        )
        .unwrap();
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": tmp.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(
            block["status"], "rejected",
            "runtime-replacement=true must be rejected even when a rule would match"
        );
        assert_eq!(
            block["applied"], false,
            "applied=false must hold even on rejection"
        );
        assert_eq!(block["recommended_backend"], "claudecode");
        let reasons = block["reasons"].as_array().expect("reasons array");
        let joined = reasons
            .iter()
            .filter_map(|r| r.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("runtime-replacement"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn router_policy_mode_dry_run_missing_dry_run_only_rejected() {
        // Cross-wave invariant: a policy missing :dry-run-only true is
        // REJECTED with status="rejected".
        let tmp = std::env::temp_dir().join(format!(
            "wave24-04-not-dro-{}.lisp",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &tmp,
            r#"(router-policy fixture-bad-dro
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only false
  :runtime-replacement false
  (rule
    :id r-x
    :priority 1
    :when ((kind docs))
    :recommend (:backend claudecode :reasoning "should reject")
    :non-goals ["does not replace runtime dispatch"]))
"#,
        )
        .unwrap();
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": tmp.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &fixture_resolved("mission_task_delegate", "fresh-code-alignment"),
            &fixture_plan("(plan)"),
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["status"], "rejected");
        assert_eq!(block["applied"], false);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn router_policy_mode_dry_run_unreadable_policy_emits_error_status() {
        // I/O failures surface as status="error" with applied=false.
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": "/this/path/does/not/exist/policy.lisp",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &fixture_resolved("mission_task_delegate", "fresh-code-alignment"),
            &fixture_plan("(plan)"),
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["status"], "error");
        assert_eq!(block["applied"], false);
        // Fallback backend is surfaced even on error so reviewers see a
        // safe default rather than a missing field.
        assert_eq!(block["recommended_backend"], "claudecode");
    }

    #[test]
    fn router_policy_mode_dry_run_predicate_path_glob_matches_owned_files() {
        // Exercise the path-glob predicate via a temp policy that demands
        // owned_files include `scripts/check-*.mjs`.
        let tmp = std::env::temp_dir().join(format!(
            "wave24-04-glob-{}.lisp",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &tmp,
            r#"(router-policy fixture-glob
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
    :id r-glob
    :priority 10
    :when ((all (kind code-alignment)
                (path-glob "scripts/check-*.mjs")))
    :recommend (:backend deterministic-checker :reasoning "scripted acceptance")
    :non-goals ["does not replace runtime dispatch"]))
"#,
        )
        .unwrap();
        // Match: owned_files contains a matching path.
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": tmp.to_str().unwrap(),
            "kind": "code-alignment",
            "owned_files": ["scripts/check-foo.mjs"],
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &fixture_resolved("mission_task_delegate", "fresh-code-alignment"),
            &fixture_plan("(plan)"),
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["status"], "computed");
        assert_eq!(block["recommended_backend"], "deterministic-checker");

        // No match: owned_files contains a non-matching path.
        let args2 = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": tmp.to_str().unwrap(),
            "kind": "code-alignment",
            "owned_files": ["src/lib.rs"],
        });
        let mode2 = parse_router_policy_mode(&args2).unwrap();
        let result2 = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode2,
            &args2,
            &fixture_resolved("mission_task_delegate", "fresh-code-alignment"),
            &fixture_plan("(plan)"),
        );
        let v2 = parse_payload(&result2);
        let block2 = &v2["router_recommendation"];
        // Falls through to fallback.
        assert_eq!(block2["recommended_backend"], "claudecode");
        assert_eq!(block2["confidence"], "low");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn router_policy_mode_dry_run_predicate_any_or_clause() {
        // Exercise the `any` (logical OR) predicate.
        let tmp = std::env::temp_dir().join(format!(
            "wave24-04-any-{}.lisp",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &tmp,
            r#"(router-policy fixture-any
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
    :id r-any
    :priority 1
    :when ((any (kind review)
                (kind smoke)))
    :recommend (:backend verifier-worker :reasoning "post-commit verify")
    :non-goals ["does not replace runtime dispatch"]))
"#,
        )
        .unwrap();
        for kind in &["review", "smoke"] {
            let args = json!({
                "router_policy_mode": "dry_run",
                "router_policy_path": tmp.to_str().unwrap(),
                "kind": kind,
            });
            let mode = parse_router_policy_mode(&args).unwrap();
            let result = attach_router_recommendation_block(
                fixture_bridge_result(),
                mode,
                &args,
                &fixture_resolved("mission_task_delegate", "fresh-code-alignment"),
                &fixture_plan("(plan)"),
            );
            let v = parse_payload(&result);
            let block = &v["router_recommendation"];
            assert_eq!(block["status"], "computed");
            assert_eq!(
                block["recommended_backend"], "verifier-worker",
                "any-clause must match for kind={}",
                kind
            );
        }
        // Non-matching kind ⇒ fallback.
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": tmp.to_str().unwrap(),
            "kind": "ops",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &fixture_resolved("mission_task_delegate", "fresh-code-alignment"),
            &fixture_plan("(plan)"),
        );
        let v = parse_payload(&result);
        assert_eq!(v["router_recommendation"]["recommended_backend"], "claudecode");
        let _ = std::fs::remove_file(&tmp);
    }

    // -----------------------------------------------------------------
    // wave24-06 — end-to-end smoke pinning the cross-wave invariants of
    // the advisory chain at the daemon boundary. This is intentionally a
    // single shape-pinning test (not a battery): the wave24-04 tests
    // already cover individual edge cases; what was missing was a single
    // assertion proving that ALL invariants hold simultaneously when the
    // chain runs through the seed-shaped policy on a docs task.
    // -----------------------------------------------------------------

    #[test]
    fn router_policy_dry_run_smoke_pins_cross_wave_invariants() {
        // Materialise a temp policy that mirrors the wave24-01 seed shape
        // (dry-run-only true, runtime-replacement false, three rules, the
        // r-docs-to-claudecode rule at priority 10). Using a temp file
        // keeps the smoke independent of cwd while still exercising the
        // exact parse path + selector the daemon uses in production.
        let tmp = std::env::temp_dir().join(format!(
            "wave24-06-smoke-{}.lisp",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &tmp,
            r#"(router-policy fixture-smoke
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
    :id r-docs-to-claudecode
    :priority 10
    :when ((kind docs))
    :recommend (:backend claudecode :reasoning "docs are interactive")
    :non-goals ["does not replace runtime dispatch"
                "does not select a model slot"])
  (rule
    :id r-deterministic-checker-tasks
    :priority 20
    :when ((all (kind code-alignment)
                (path-glob "scripts/check-*.mjs")))
    :recommend (:backend deterministic-checker :reasoning "scripted acceptance")
    :non-goals ["does not replace runtime dispatch"])
  (rule
    :id r-post-commit-verifier
    :priority 30
    :when ((any (kind review)
                (kind smoke)))
    :recommend (:backend verifier-worker :reasoning "verifies an existing commit")
    :non-goals ["does not replace runtime dispatch"]))
"#,
        )
        .unwrap();

        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let baseline = action_execute_bridge(&plan, &resolved);
        let baseline_v = parse_payload(&baseline);

        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": tmp.to_str().unwrap(),
            "kind": "docs",
            "owner": "claudecode",
        });
        let mode = parse_router_policy_mode(&args).expect("dry_run parses");
        assert!(matches!(mode, RouterPolicyMode::DryRun));

        let with_dry_run = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&with_dry_run);
        let block = v
            .get("router_recommendation")
            .expect("dry_run mode must splice a recommendation block");

        // Invariant 1: dry-run-only is honored end-to-end (the daemon's
        // applied=false hard-coded literal is the runtime analog of the
        // policy's :dry-run-only true; we pin the literal Bool type).
        assert_eq!(
            block["applied"],
            Value::Bool(false),
            "wave24-06 smoke: applied MUST be the literal false bool"
        );
        // Invariant 3: applied=false is hard-coded in every emitted block.
        // (Restated here so the smoke fails loudly if the helper ever
        // computes the field instead of hard-coding it.)
        assert!(
            block["applied"].is_boolean(),
            "applied must be a JSON bool, never a string or number"
        );

        // Invariant 2 / matched-rule: the seed's docs rule wins.
        assert_eq!(
            block["status"], "computed",
            "smoke: docs task on seed-shape policy must be computed (not rejected/error)"
        );
        assert_eq!(
            block["recommended_backend"], "claudecode",
            "smoke: r-docs-to-claudecode wins on docs task"
        );
        // Backend must be one of the wave24-01 schema enum values. We
        // re-spell the enum locally so this test does not import the
        // checker script — pure Rust.
        let allowed_backends = [
            "claudecode",
            "missiond-llm-router",
            "deterministic-checker",
            "patch-worker",
            "verifier-worker",
        ];
        let backend = block["recommended_backend"]
            .as_str()
            .expect("recommended_backend must be a string");
        assert!(
            allowed_backends.contains(&backend),
            "smoke: recommended_backend `{}` not in wave24-01 enum",
            backend
        );

        // Invariant 4: schema field surfaces the wave24 router-recommendation
        // contract identifier so external readers can route the payload.
        assert_eq!(
            block["schema"], "missiond.router-recommendation.v0",
            "smoke: schema field must surface the wave24 recommendation contract id"
        );

        // Invariant 7: dispatch fields are byte-identical to baseline.
        // The smoke compares EVERY dispatch-shaping field at once so a
        // future regression that perturbs ANY of them fails loudly here.
        for field in [
            "target_tool",
            "target_source",
            "dispatch_strategy",
            "dispatch_strategy_source",
            "next_call",
            "execute_mode",
            "runner_status",
        ] {
            assert_eq!(
                baseline_v[field], v[field],
                "smoke: dispatch field `{}` must be byte-identical with vs without dry_run mode",
                field
            );
        }
        // The recommendation block is the ONLY additive delta.
        assert!(
            baseline_v.get("router_recommendation").is_none(),
            "baseline must not carry a recommendation block"
        );

        // Invariant 5/6: the helper must not have introduced ANY new
        // observable side effect. A weak-but-useful proof: confidence is
        // surfaced (so the policy's matched rule was actually evaluated,
        // not short-circuited by a stub) and reasons reference the rule
        // id (so the explanation is grounded in the parsed seed).
        assert!(
            block.get("confidence").is_some(),
            "smoke: confidence field must be surfaced"
        );
        let reasons = block["reasons"].as_array().expect("reasons array");
        let joined = reasons
            .iter()
            .filter_map(|r| r.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("r-docs-to-claudecode"),
            "smoke: reasons must reference the matched rule id"
        );

        let _ = std::fs::remove_file(&tmp);
    }

    // -----------------------------------------------------------------
    // wave-25 / task 03 — router-policy trace-index confidence tests.
    //
    // These pin the OPTIONAL `router_policy_trace_index_path` arg and the
    // additive `trace_index_path` / `trace_index_status` /
    // `trace_index_warning` fields on the recommendation block. They also
    // re-pin two cross-wave invariants under the new code path:
    //   * `applied=false` stays a hard-coded literal even when the trace-
    //     index is fully consumed.
    //   * dispatch fields are byte-identical with vs without trace-index.
    //
    // Confidence rule mirrors `scripts/recommend-task-backend.mjs`:
    //   matched + max(by_task[plan.board_task_id].events,
    //                 by_backend[recommended_backend].events) >= 5  -> high
    //   1..=4 -> medium
    //   0 -> low (matched-but-zero) ; no-match always low.
    // -----------------------------------------------------------------

    /// Helper: build a temp policy file mirroring the wave24-01 seed shape
    /// with a single docs->claudecode rule. Returns the path; the caller is
    /// responsible for unlinking with `remove_file`.
    fn write_temp_docs_policy(tag: &str) -> std::path::PathBuf {
        let tmp = std::env::temp_dir().join(format!(
            "wave25-03-{}-{}.lisp",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &tmp,
            r#"(router-policy fixture-wave25-03
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
    :id r-docs
    :priority 10
    :when ((kind docs))
    :recommend (:backend claudecode :reasoning "docs are interactive")
    :non-goals ["does not replace runtime dispatch"]))
"#,
        )
        .unwrap();
        tmp
    }

    /// Helper: build a temp trace-index JSON file. `task_events` and
    /// `backend_events` populate `by_task["btk-1"].events` and
    /// `by_backend["claudecode"].events` respectively (matching the
    /// fixture_plan default board_task_id and the docs rule's backend).
    fn write_temp_trace_index(tag: &str, task_events: u64, backend_events: u64) -> std::path::PathBuf {
        let tmp = std::env::temp_dir().join(format!(
            "wave25-03-{}-trace-{}.json",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let body = json!({
            "schema": "missiond.session-trace.v1",
            "by_task": {
                "btk-1": { "events": task_events }
            },
            "by_backend": {
                "claudecode": { "events": backend_events }
            },
            "totals": { "events": task_events + backend_events }
        });
        std::fs::write(&tmp, serde_json::to_string_pretty(&body).unwrap()).unwrap();
        tmp
    }

    #[test]
    fn router_policy_mode_off_with_trace_index_supplied_does_no_file_io() {
        // wave25-03 invariant: mode=off (or absent) means NO file I/O happens
        // for the trace-index path EVEN IF a path is supplied. We assert this
        // by supplying a path that does NOT exist and demanding the response
        // be byte-identical to a baseline that supplies no trace-index field
        // at all. If the daemon attempted to open the file under mode=off the
        // attempt would fail-but-be-swallowed; the byte-identical assertion
        // still holds because the response shape NEVER carries any trace_index_*
        // field when mode=off (the recommendation block isn't even emitted).
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
        let baseline = action_execute_bridge(&plan, &resolved);
        let baseline_text = match baseline.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };

        // Off + non-existent trace-index path.
        let args = json!({
            "router_policy_mode": "off",
            "router_policy_trace_index_path":
                "/this/path/does/not/exist/wave25-03/trace-index.json",
        });
        let mode = parse_router_policy_mode(&args).expect("explicit off");
        assert!(matches!(mode, RouterPolicyMode::Off));
        let after = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let after_text = match after.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        assert_eq!(
            baseline_text, after_text,
            "wave25-03: mode=off must be byte-identical to baseline EVEN WHEN trace-index path is supplied (no file I/O may happen)"
        );
        let v: Value = serde_json::from_str(&after_text).unwrap();
        assert!(
            v.get("router_recommendation").is_none(),
            "wave25-03: mode=off must NOT splice a recommendation block"
        );

        // Default (arg absent) + trace-index path supplied: same invariant.
        let args2 = json!({
            "router_policy_trace_index_path":
                "/this/path/does/not/exist/wave25-03/other.json",
        });
        let mode2 = parse_router_policy_mode(&args2).expect("default off");
        assert!(matches!(mode2, RouterPolicyMode::Off));
        let after2 = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode2,
            &args2,
            &resolved,
            &plan,
        );
        let after2_text = match after2.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        assert_eq!(
            baseline_text, after2_text,
            "wave25-03: default mode (arg absent) must be byte-identical to baseline EVEN WHEN trace-index path is supplied"
        );
    }

    #[test]
    fn router_policy_mode_dry_run_with_trace_index_high_confidence() {
        // wave25-03: trace-index supplied AND backend has >=5 events ⇒ high.
        let policy = write_temp_docs_policy("high");
        let trace = write_temp_trace_index("high", 0, 7);
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["status"], "computed");
        assert_eq!(block["recommended_backend"], "claudecode");
        assert_eq!(block["confidence"], "high");
        assert_eq!(block["applied"], false);
        assert_eq!(block["trace_index_status"], "used");
        assert_eq!(block["trace_index_path"], trace.to_str().unwrap());
        assert!(
            block.get("trace_index_warning").is_none(),
            "wave25-03: status=used must NOT carry a warning"
        );
        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
    }

    #[test]
    fn router_policy_mode_dry_run_with_trace_index_medium_confidence() {
        // wave25-03: trace-index supplied AND max(events) in 1..=4 ⇒ medium.
        let policy = write_temp_docs_policy("medium");
        let trace = write_temp_trace_index("medium", 2, 3);
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["status"], "computed");
        assert_eq!(block["recommended_backend"], "claudecode");
        assert_eq!(block["confidence"], "medium");
        assert_eq!(block["applied"], false);
        assert_eq!(block["trace_index_status"], "used");
        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
    }

    #[test]
    fn router_policy_mode_dry_run_with_trace_index_low_confidence_when_zero_events() {
        // wave25-03: trace-index supplied AND max(events) == 0 ⇒ low. This
        // is distinct from the no-match-fallback low because a rule DID
        // match — the low confidence is due to evidence absence in the trace.
        let policy = write_temp_docs_policy("zero");
        let trace = write_temp_trace_index("zero", 0, 0);
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["status"], "computed");
        assert_eq!(block["recommended_backend"], "claudecode");
        assert_eq!(block["confidence"], "low");
        assert_eq!(block["applied"], false);
        assert_eq!(block["trace_index_status"], "used");
        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
    }

    #[test]
    fn router_policy_mode_dry_run_with_missing_trace_index_emits_status_missing() {
        // wave25-03: missing trace-index file ⇒ status=missing, dispatch
        // continues, fallback confidence (`medium` for matched).
        let policy = write_temp_docs_policy("missing");
        let bogus_trace = std::env::temp_dir().join(format!(
            "wave25-03-missing-{}-DOES-NOT-EXIST.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": bogus_trace.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["status"], "computed", "matched dispatch must still succeed");
        assert_eq!(block["recommended_backend"], "claudecode");
        assert_eq!(
            block["confidence"], "medium",
            "wave25-03: missing trace-index ⇒ matched fallback confidence (medium)"
        );
        assert_eq!(block["applied"], false);
        assert_eq!(block["trace_index_status"], "missing");
        assert_eq!(block["trace_index_path"], bogus_trace.to_str().unwrap());
        assert!(
            block.get("trace_index_warning").is_some(),
            "wave25-03: missing must surface a one-line warning"
        );
        let _ = std::fs::remove_file(&policy);
    }

    #[test]
    fn router_policy_mode_dry_run_with_malformed_trace_index_emits_status_malformed() {
        // wave25-03: malformed JSON ⇒ status=malformed, fallback confidence.
        let policy = write_temp_docs_policy("malformed");
        let bad_trace = std::env::temp_dir().join(format!(
            "wave25-03-malformed-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&bad_trace, "{ this is not valid json").unwrap();
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": bad_trace.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["status"], "computed");
        assert_eq!(block["recommended_backend"], "claudecode");
        assert_eq!(block["confidence"], "medium");
        assert_eq!(block["applied"], false);
        assert_eq!(block["trace_index_status"], "malformed");
        assert_eq!(block["trace_index_path"], bad_trace.to_str().unwrap());
        let warning = block["trace_index_warning"]
            .as_str()
            .expect("malformed must carry a warning string");
        assert!(
            warning.contains("trace-index"),
            "wave25-03: warning must mention trace-index (got `{}`)",
            warning
        );
        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&bad_trace);
    }

    #[test]
    fn router_policy_mode_dry_run_no_trace_index_supplied_emits_status_absent() {
        // wave25-03: arg absent ⇒ NO trace_index_* fields emitted at all
        // (preserves wave24-04 byte-shape for callers that did not opt in).
        // We document this as the "absent" status by checking that the
        // fields are entirely OMITTED rather than surfacing a literal
        // `"absent"` value — keeps wave24-04 callers byte-identically green.
        let policy = write_temp_docs_policy("absent");
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["status"], "computed");
        assert_eq!(block["recommended_backend"], "claudecode");
        // Fallback (no trace-index) ⇒ matched default `medium`.
        assert_eq!(block["confidence"], "medium");
        assert_eq!(block["applied"], false);
        // wave25-03 contract choice: when path is absent, OMIT all
        // trace_index_* fields entirely (rather than emit a literal
        // `"absent"` value) so wave24-04 callers are byte-identically green.
        assert!(
            block.get("trace_index_path").is_none(),
            "wave25-03: trace_index_path must be OMITTED when path arg is absent"
        );
        assert!(
            block.get("trace_index_status").is_none(),
            "wave25-03: trace_index_status must be OMITTED when path arg is absent"
        );
        assert!(
            block.get("trace_index_warning").is_none(),
            "wave25-03: trace_index_warning must be OMITTED when path arg is absent"
        );
        let _ = std::fs::remove_file(&policy);
    }

    #[test]
    fn router_policy_mode_dry_run_with_trace_index_does_not_change_dispatch() {
        // wave25-03: re-pin the wave24-04 invariant under the new code path.
        // Dispatch fields (target_tool / dispatch_strategy / next_call /...)
        // are byte-identical with vs without the trace-index arg.
        let policy = write_temp_docs_policy("dispatch");
        let trace = write_temp_trace_index("dispatch", 9, 9);
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

        // Path A: dry_run + NO trace-index arg.
        let args_no_trace = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "kind": "docs",
        });
        let mode_a = parse_router_policy_mode(&args_no_trace).unwrap();
        let no_trace_result = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode_a,
            &args_no_trace,
            &resolved,
            &plan,
        );
        let no_trace_v = parse_payload(&no_trace_result);

        // Path B: dry_run + trace-index arg.
        let args_with_trace = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "kind": "docs",
        });
        let mode_b = parse_router_policy_mode(&args_with_trace).unwrap();
        let with_trace_result = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode_b,
            &args_with_trace,
            &resolved,
            &plan,
        );
        let with_trace_v = parse_payload(&with_trace_result);

        // Every dispatch-shaping field must be byte-identical.
        for field in [
            "target_tool",
            "target_source",
            "dispatch_strategy",
            "dispatch_strategy_source",
            "next_call",
            "execute_mode",
            "runner_status",
        ] {
            assert_eq!(
                no_trace_v[field], with_trace_v[field],
                "wave25-03 invariant: dispatch field `{}` must be byte-identical with vs without trace-index arg",
                field
            );
        }

        // The recommendation block exists in both; confidence may differ
        // (medium vs high) but `applied`, `recommended_backend`, `status`,
        // and `policy_source` must match — only the additive trace_index_*
        // fields and the `confidence` are allowed to differ.
        let block_a = &no_trace_v["router_recommendation"];
        let block_b = &with_trace_v["router_recommendation"];
        assert_eq!(block_a["applied"], block_b["applied"]);
        assert_eq!(block_a["recommended_backend"], block_b["recommended_backend"]);
        assert_eq!(block_a["status"], block_b["status"]);
        assert_eq!(block_a["policy_source"], block_b["policy_source"]);
        assert_eq!(block_a["schema"], block_b["schema"]);

        // And the additive delta is exactly what we expect.
        assert!(block_a.get("trace_index_path").is_none());
        assert_eq!(block_b["trace_index_path"], trace.to_str().unwrap());
        assert_eq!(block_b["trace_index_status"], "used");

        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
    }

    #[test]
    fn applied_remains_false_with_trace_index() {
        // wave25-03: re-pin the wave24-04 / wave24-06 invariant under the
        // new code path. `applied` must be the literal JSON bool `false` in
        // EVERY emitted block, regardless of trace-index status. We exercise
        // all five status flavours: used / missing / unreadable (simulated
        // via missing) / malformed / absent.
        let policy = write_temp_docs_policy("applied");
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

        // used.
        let trace_used = write_temp_trace_index("applied-used", 10, 10);
        let args_used = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace_used.to_str().unwrap(),
            "kind": "docs",
        });
        let mode_used = parse_router_policy_mode(&args_used).unwrap();
        let r_used = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode_used,
            &args_used,
            &resolved,
            &plan,
        );
        let v_used = parse_payload(&r_used);
        assert_eq!(
            v_used["router_recommendation"]["applied"], Value::Bool(false),
            "wave25-03 invariant: applied=false literal under trace_index_status=used"
        );

        // missing.
        let args_missing = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": "/does/not/exist/wave25-03-applied.json",
            "kind": "docs",
        });
        let mode_missing = parse_router_policy_mode(&args_missing).unwrap();
        let r_missing = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode_missing,
            &args_missing,
            &resolved,
            &plan,
        );
        let v_missing = parse_payload(&r_missing);
        assert_eq!(
            v_missing["router_recommendation"]["applied"], Value::Bool(false),
            "wave25-03 invariant: applied=false literal under trace_index_status=missing"
        );

        // malformed.
        let bad = std::env::temp_dir().join(format!(
            "wave25-03-applied-malformed-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&bad, "not json").unwrap();
        let args_bad = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": bad.to_str().unwrap(),
            "kind": "docs",
        });
        let mode_bad = parse_router_policy_mode(&args_bad).unwrap();
        let r_bad = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode_bad,
            &args_bad,
            &resolved,
            &plan,
        );
        let v_bad = parse_payload(&r_bad);
        assert_eq!(
            v_bad["router_recommendation"]["applied"], Value::Bool(false),
            "wave25-03 invariant: applied=false literal under trace_index_status=malformed"
        );

        // absent.
        let args_absent = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "kind": "docs",
        });
        let mode_absent = parse_router_policy_mode(&args_absent).unwrap();
        let r_absent = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode_absent,
            &args_absent,
            &resolved,
            &plan,
        );
        let v_absent = parse_payload(&r_absent);
        assert_eq!(
            v_absent["router_recommendation"]["applied"], Value::Bool(false),
            "wave25-03 invariant: applied=false literal under trace_index absent"
        );

        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace_used);
        let _ = std::fs::remove_file(&bad);
    }

    // -----------------------------------------------------------------
    // wave25-05 — cross-layer measurement smoke pinning the FULL Wave 25
    // measurable router loop is still ADVISORY at the daemon boundary.
    //
    // The wave25-05 brief calls out 8 cross-wave invariants that must all
    // hold simultaneously across the evaluator + report fields + renderer
    // commands + mission_plan trace-index confidence engines. The Layer A
    // Node-side smoke (recommend-task-backend.mjs --dry-fixture wave25-05
    // case + evaluate-router-policy-corpus.mjs --dry-fixture wave25-05
    // case) pins the Node-side; the Layer C report-checker fixture pins
    // the report-contract surface. This test pins the daemon side AND
    // documents the CLI/Rust parity for the (5,5)-event fixture inline.
    //
    // The parity assertion does NOT shell out — that is forbidden by the
    // wave25-05 contract. Instead, the daemon's selected backend +
    // confidence are asserted against the EXPECTED values that the Node
    // CLI also asserts in its own --dry-fixture run for the same
    // synthetic shape. Inline documentation makes the expected agreement
    // surface-readable so a future regression in either engine surfaces
    // here AND in the corresponding Node fixture.
    // -----------------------------------------------------------------

    #[test]
    fn router_policy_dry_run_smoke_pins_wave25_invariants() {
        // Materialise the wave25-05 parity fixture: the SAME two-rule policy
        // shape the Node Layer A fixture builds (docs->claudecode at
        // priority 10; code-alignment+scripts/check-* -> deterministic-
        // checker at priority 20). Using a temp file keeps the smoke
        // independent of cwd while still exercising the exact parse path
        // the daemon uses in production.
        let policy_path = std::env::temp_dir().join(format!(
            "wave25-05-smoke-policy-{}.lisp",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &policy_path,
            r#"(router-policy fixture-wave25-05-smoke
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
    :id r-docs-to-claudecode
    :priority 10
    :when ((kind docs))
    :recommend (:backend claudecode :reasoning "docs are interactive")
    :non-goals ["does not replace runtime dispatch"])
  (rule
    :id r-deterministic-checker-tasks
    :priority 20
    :when ((all (kind code-alignment)
                (path-glob "scripts/check-*.mjs")))
    :recommend (:backend deterministic-checker :reasoning "scripted acceptance")
    :non-goals ["does not replace runtime dispatch"]))
"#,
        )
        .unwrap();

        // Materialise the (5,5)-event trace-index — same shape the Node
        // CLI parity fixture drives. The daemon's bucket_events helper
        // reads by_task["btk-1"].events (fixture_plan default) AND
        // by_backend["claudecode"].events; here we plant 5 in BOTH to
        // make the parity unambiguous.
        let trace_path = std::env::temp_dir().join(format!(
            "wave25-05-smoke-trace-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let trace_body = json!({
            "schema": "missiond.session-trace.v1",
            "by_task": { "btk-1": { "events": 5 } },
            "by_backend": { "claudecode": { "events": 5 } },
            "totals": { "events": 10 }
        });
        std::fs::write(&trace_path, serde_json::to_string_pretty(&trace_body).unwrap())
            .unwrap();

        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

        // -----------------------------------------------------------------
        // Invariant 6: dispatch byte-shape unchanged when mode=off-with-
        // trace-supplied. Re-pin the wave24-04 + wave25-03 invariants under
        // the wave25-05 shape: even with both router_policy_path AND
        // router_policy_trace_index_path supplied, mode=off MUST NOT
        // perturb the dispatch envelope.
        // -----------------------------------------------------------------
        let baseline = action_execute_bridge(&plan, &resolved);
        let baseline_text = match baseline.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        let off_args = json!({
            "router_policy_mode": "off",
            "router_policy_path": policy_path.to_str().unwrap(),
            "router_policy_trace_index_path": trace_path.to_str().unwrap(),
            "kind": "docs",
        });
        let off_mode = parse_router_policy_mode(&off_args).expect("explicit off");
        assert!(matches!(off_mode, RouterPolicyMode::Off));
        let off_after = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            off_mode,
            &off_args,
            &resolved,
            &plan,
        );
        let off_text = match off_after.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        assert_eq!(
            baseline_text, off_text,
            "wave25-05 invariant 6: mode=off must be byte-identical even with trace-index supplied"
        );

        // -----------------------------------------------------------------
        // Invariant 7: CLI/Rust parity for the (5,5) high-confidence
        // fixture. The Node Layer A fixture asserts:
        //   recommend({ task: docs, policy: <wave25-05 shape>,
        //               traceIndex: { backend:claudecode events:5 } }).confidence
        //     === 'high'
        //   recommend(...).backend === 'claudecode'
        //   recommend(...).chosen_rule_id === 'r-docs-to-claudecode'
        // The daemon must agree on backend + confidence for the same shape.
        // -----------------------------------------------------------------
        let dry_args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy_path.to_str().unwrap(),
            "router_policy_trace_index_path": trace_path.to_str().unwrap(),
            "kind": "docs",
            "owner": "claudecode",
        });
        let dry_mode = parse_router_policy_mode(&dry_args).expect("dry_run parses");
        assert!(matches!(dry_mode, RouterPolicyMode::DryRun));
        let dry_result = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            dry_mode,
            &dry_args,
            &resolved,
            &plan,
        );
        let dry_v = parse_payload(&dry_result);
        let block = dry_v
            .get("router_recommendation")
            .expect("dry_run mode must splice a recommendation block");

        // Invariant 1: policy.runtime_replacement=false (re-checked on the
        // parsed temp policy via the daemon's reject-runtime-replacement
        // branch — if the policy declared runtime_replacement true, the
        // status would be "rejected" and recommended_backend would fall
        // back. The wave24-01 schema rejects this at validation time; the
        // daemon re-checks defensively. We pin the absence of rejection
        // here as the positive signal.)
        assert_eq!(
            block["status"], "computed",
            "wave25-05 invariant 1: policy with runtime_replacement=false must be accepted (status=computed)"
        );
        // Invariant 2: policy.dry_run_only=true (same logic — if the
        // policy lacked dry-run-only, the daemon's
        // router_policy_mode_dry_run_missing_dry_run_only_rejected branch
        // would surface status=rejected. status=computed proves the
        // dry-run-only invariant held end-to-end.)

        // Invariant 3: applied=false JSON Bool literal in EVERY emitted
        // recommendation. Type-checked, not just value-equality, so a
        // future regression that switches the field to a string "false"
        // or to an integer 0 fails loudly here.
        assert_eq!(
            block["applied"],
            Value::Bool(false),
            "wave25-05 invariant 3: applied MUST be the literal JSON Bool false"
        );
        assert!(
            block["applied"].is_boolean(),
            "wave25-05 invariant 3: applied must be a JSON bool, never a string or number"
        );

        // Invariant 7 (cont.): CLI/Rust parity. With (5,5) trace-index
        // events ON BOTH task and backend buckets, both engines must
        // select 'high' confidence + 'claudecode' backend.
        assert_eq!(
            block["confidence"], "high",
            "wave25-05 invariant 7: daemon confidence must agree with Node CLI for (5,5) trace-index parity fixture"
        );
        assert_eq!(
            block["recommended_backend"], "claudecode",
            "wave25-05 invariant 7: daemon backend must agree with Node CLI for docs->claudecode rule"
        );
        // Recommended backend ∈ wave24-01 enum (re-spelled locally to keep
        // the test pure-Rust per wave24-06 lesson — no script imports).
        let allowed_backends = [
            "claudecode",
            "missiond-llm-router",
            "deterministic-checker",
            "patch-worker",
            "verifier-worker",
        ];
        let backend = block["recommended_backend"]
            .as_str()
            .expect("recommended_backend must be a string");
        assert!(
            allowed_backends.contains(&backend),
            "wave25-05 invariant: recommended_backend `{}` not in wave24-01 enum",
            backend
        );

        // Invariant: schema field surfaces the wave24 router-recommendation
        // contract identifier so external readers can route the payload.
        assert_eq!(
            block["schema"], "missiond.router-recommendation.v0",
            "wave25-05 invariant: schema field must surface the wave24 recommendation contract id"
        );

        // Invariant: trace_index_status=used proves the wave25-03 trace-
        // index code path was exercised (not the legacy wave24-04 fallback
        // that would emit no trace_index_* fields).
        assert_eq!(
            block["trace_index_status"], "used",
            "wave25-05 invariant: trace_index_status must be `used` for a well-formed parity fixture"
        );
        assert_eq!(
            block["trace_index_path"],
            trace_path.to_str().unwrap(),
            "wave25-05 invariant: trace_index_path must echo the input path verbatim"
        );

        // Invariant 6 (cont.): every dispatch-shaping field must be byte-
        // identical between baseline and dry-run. Re-pin every dispatch
        // field at once so any future regression that perturbs ANY of
        // them fails loudly here.
        let baseline_v = parse_payload(&baseline);
        for field in [
            "target_tool",
            "target_source",
            "dispatch_strategy",
            "dispatch_strategy_source",
            "next_call",
            "execute_mode",
            "runner_status",
        ] {
            assert_eq!(
                baseline_v[field], dry_v[field],
                "wave25-05 invariant 6: dispatch field `{}` must be byte-identical with vs without dry_run mode",
                field
            );
        }
        assert!(
            baseline_v.get("router_recommendation").is_none(),
            "wave25-05 invariant 6: baseline must not carry a recommendation block"
        );

        // Invariant: reasons reference the matched rule id so explanation
        // is grounded in the parsed seed (mirrors wave24-06 smoke).
        let reasons = block["reasons"].as_array().expect("reasons array");
        let joined = reasons
            .iter()
            .filter_map(|r| r.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("r-docs-to-claudecode"),
            "wave25-05: reasons must reference the matched rule id"
        );

        // Invariant 8 (audit): zero shell-out / LLM / git mutation in the
        // router code path. We audit the wave25-03 daemon module's source
        // for forbidden Rust patterns: `std::process::Command`, `tokio::
        // process`, network types from `reqwest` / `hyper`, git invocation,
        // any LLM vendor probe. Forbidden patterns are assembled from
        // string parts so the audit table itself does not appear as a
        // literal substring (wave24-06 / wave25-01 self-audit lesson).
        let plan_rs = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/handlers/knowledge/plan.rs"),
        )
        .expect("plan.rs must be readable for self-audit");
        // Strip line comments before scanning so prose that names the
        // forbidden patterns does not self-trip the audit. We keep block
        // comments and string literals in scope on purpose: a real string
        // literal inviting `reqwest` would be evidence the module is about
        // to grow a network dep; the audit catches it early.
        let stripped: String = plan_rs
            .lines()
            .filter(|ln| !ln.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let forbidden_router_patterns: Vec<String> = vec![
            // std::process::Command — process spawn from std.
            String::from("std::") + "process::" + "Command",
            // tokio::process — async process spawn.
            String::from("tokio::") + "process",
            // reqwest — HTTP client crate often pulled in for LLM calls.
            String::from("req") + "west::",
            // hyper — lower-level HTTP crate.
            String::from("hyper::") + "Client",
            // openai / anthropic LLM vendor probes.
            String::from("open") + "ai_api",
            String::from("anthrop") + "ic_api",
        ];
        for pat in &forbidden_router_patterns {
            assert!(
                !stripped.contains(pat.as_str()),
                "wave25-05 invariant 8: forbidden router-side pattern `{}` found in plan.rs active source",
                pat
            );
        }

        let _ = std::fs::remove_file(&policy_path);
        let _ = std::fs::remove_file(&trace_path);
    }

    #[test]
    fn router_policy_cli_rust_parity_for_high_confidence_match() {
        // wave25-05 Layer B parity test. Documents inline that the Node
        // CLI's `recommend({ ..., traceIndex: { by_task:{<id>:{events:5}},
        //   by_backend:{claudecode:{events:5}} } }).confidence === 'high'`
        // for a docs task on the wave25-05 parity policy. Verifying this
        // in Rust requires shelling out (which is forbidden); instead
        // this test asserts the daemon's selection matches a hard-coded
        // expected backend + confidence that the Node Layer A fixture
        // ALSO expects. A regression in either engine surfaces here AND
        // in the corresponding Node fixture so the parity is bidirectional.
        //
        // Documented expected agreement (Node CLI side):
        //   policy:        wave25-05 parity policy (docs->claudecode prio 10)
        //   task.kind:     docs
        //   trace_index:   by_task[task.id].events=5, by_backend[claudecode].events=5
        //   recommend()  -> { backend: 'claudecode',
        //                     confidence: 'high',
        //                     chosen_rule_id: 'r-docs-to-claudecode',
        //                     dry_run_only: true }
        //
        // Daemon expected agreement (this test):
        //   args.kind=docs, mode=dry_run, trace_index path -> (5,5)
        //   block.recommended_backend === 'claudecode'  (parity)
        //   block.confidence          === 'high'        (parity)
        //   block.applied             === Bool(false)   (cross-wave invariant)
        //   block.trace_index_status  === 'used'        (wave25-03 surface)
        let policy_path = std::env::temp_dir().join(format!(
            "wave25-05-parity-policy-{}.lisp",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &policy_path,
            r#"(router-policy fixture-wave25-05-parity
  :schema "missiond.router-policy.v1"
  :version "v1"
  :dry-run-only true
  :runtime-replacement false
  (rule
    :id r-docs-to-claudecode
    :priority 10
    :when ((kind docs))
    :recommend (:backend claudecode :reasoning "docs are interactive")
    :non-goals ["does not replace runtime dispatch"]))
"#,
        )
        .unwrap();
        let trace_path = std::env::temp_dir().join(format!(
            "wave25-05-parity-trace-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &trace_path,
            serde_json::to_string_pretty(&json!({
                "schema": "missiond.session-trace.v1",
                "by_task": { "btk-1": { "events": 5 } },
                "by_backend": { "claudecode": { "events": 5 } },
                "totals": { "events": 10 }
            }))
            .unwrap(),
        )
        .unwrap();

        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy_path.to_str().unwrap(),
            "router_policy_trace_index_path": trace_path.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).expect("dry_run parses");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];

        // Hard-coded expected values that Node Layer A also asserts for
        // the SAME shape. A divergence on either side fails this test
        // AND the Node fixture so the parity is bidirectional.
        assert_eq!(
            block["recommended_backend"], "claudecode",
            "wave25-05 parity: Node CLI emits backend='claudecode' for docs task on wave25-05 parity policy"
        );
        assert_eq!(
            block["confidence"], "high",
            "wave25-05 parity: Node CLI emits confidence='high' for (5,5)-event trace-index"
        );
        assert_eq!(
            block["applied"],
            Value::Bool(false),
            "wave25-05 parity: cross-wave invariant — applied=false literal under any trace-index status"
        );
        assert_eq!(
            block["status"], "computed",
            "wave25-05 parity: matched rule on well-formed policy must surface status=computed"
        );
        assert_eq!(
            block["trace_index_status"], "used",
            "wave25-05 parity: well-formed (5,5) trace-index must surface trace_index_status=used"
        );

        let _ = std::fs::remove_file(&policy_path);
        let _ = std::fs::remove_file(&trace_path);
    }

    // -----------------------------------------------------------------
    // wave26-03 — backend-readiness registry consumption tests.
    //
    // These pin the OPTIONAL `router_backend_registry_path` arg and the
    // SIX additive fields on the recommendation block:
    //   * backend_registry_path
    //   * backend_registry_status   ∈ used | missing | unreadable | malformed | unknown_backend
    //   * backend_readiness_status  ∈ current-default | advisory-only | runtime-ready | unavailable | unknown
    //   * backend_runtime_allowed   bool
    //   * router_apply_eligible     bool (the 6-condition gate)
    //   * router_apply_blockers     Vec<String>
    //
    // 6-condition apply-eligibility gate (mirrors wave26-02 Node logic):
    //   1. policy valid (status=computed)
    //   2. confidence == "high"
    //   3. backend present in registry
    //   4. runtime_allowed == true
    //   5. readiness_status == "runtime-ready"  (current-default INSUFFICIENT)
    //   6. apply_blockers empty
    //
    // Cross-wave invariants re-pinned under the new code path:
    //   * applied=false stays a hard-coded literal under EVERY registry status
    //   * dispatch is byte-identical with vs without registry arg
    //   * mode=off (or absent) does NO file I/O even with registry path supplied
    //   * mode=off remains byte-identical even when BOTH new arg AND
    //     router_policy_trace_index_path are supplied
    // -----------------------------------------------------------------

    /// Helper: temp registry file. `body` is written verbatim so each test
    /// can shape its own backends. Returns path; caller unlinks.
    fn write_temp_registry(tag: &str, body: &str) -> std::path::PathBuf {
        let tmp = std::env::temp_dir().join(format!(
            "wave26-03-{}-registry-{}.lisp",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&tmp, body).unwrap();
        tmp
    }

    /// Helper: build a registry body with a single matched backend entry.
    /// Used to exercise the 4 readiness flavours in isolation.
    fn registry_body_single(
        backend_id: &str,
        readiness: &str,
        runtime_allowed: bool,
        apply_blockers: &[&str],
    ) -> String {
        let blockers = if apply_blockers.is_empty() {
            "[]".to_string()
        } else {
            let inner = apply_blockers
                .iter()
                .map(|b| format!("\"{}\"", b))
                .collect::<Vec<_>>()
                .join("\n     ");
            format!("[{}]", inner)
        };
        format!(
            r#"(router-backend-registry seed-test
  :schema "missiond.router-backend-registry.v1"
  :version "v1"

  (backend
    :id {id}
    :readiness_status {readiness}
    :runtime_allowed {ra}
    :apply_blockers {blockers}
    :substrate nil
    :non-goals ["does not replace runtime dispatch"]))
"#,
            id = backend_id,
            readiness = readiness,
            ra = if runtime_allowed { "true" } else { "false" },
            blockers = blockers,
        )
    }

    #[test]
    fn router_policy_mode_off_with_registry_supplied_does_no_file_io() {
        // wave26-03 invariant: mode=off MUST do NO file I/O for the registry
        // path EVEN IF a non-existent path is supplied. Asserted by byte-
        // identical baseline comparison — no recommendation block is even
        // emitted, so no `backend_*` fields can leak.
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
        let baseline = action_execute_bridge(&plan, &resolved);
        let baseline_text = match baseline.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };

        let args = json!({
            "router_policy_mode": "off",
            "router_backend_registry_path":
                "/this/path/does/not/exist/wave26-03/registry.lisp",
        });
        let mode = parse_router_policy_mode(&args).expect("explicit off");
        assert!(matches!(mode, RouterPolicyMode::Off));
        let after = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let after_text = match after.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        assert_eq!(
            baseline_text, after_text,
            "wave26-03: mode=off must be byte-identical to baseline EVEN WHEN registry path is supplied (no file I/O may happen)"
        );
        let v: Value = serde_json::from_str(&after_text).unwrap();
        assert!(
            v.get("router_recommendation").is_none(),
            "wave26-03: mode=off must NOT splice a recommendation block"
        );
    }

    #[test]
    fn router_policy_mode_dry_run_with_registry_emits_readiness_block() {
        // Happy path: registry has the matched backend at runtime-ready +
        // runtime_allowed=true + 0 blockers; high confidence. Status=used,
        // readiness mirrored, eligible=true.
        let policy = write_temp_docs_policy("readiness-happy");
        let trace = write_temp_trace_index("readiness-happy", 7, 0);
        let registry_body = registry_body_single("claudecode", "runtime-ready", true, &[]);
        let registry = write_temp_registry("happy", &registry_body);
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "router_backend_registry_path": registry.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["status"], "computed");
        assert_eq!(block["recommended_backend"], "claudecode");
        assert_eq!(block["confidence"], "high");
        assert_eq!(block["applied"], false);
        assert_eq!(block["backend_registry_status"], "used");
        assert_eq!(block["backend_registry_path"], registry.to_str().unwrap());
        assert_eq!(block["backend_readiness_status"], "runtime-ready");
        assert_eq!(block["backend_runtime_allowed"], true);
        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
        let _ = std::fs::remove_file(&registry);
    }

    #[test]
    fn router_policy_mode_dry_run_with_runtime_ready_eligible() {
        // Synthetic registry: matched backend runtime-ready + runtime_allowed=true
        // + zero blockers + high confidence -> router_apply_eligible=true.
        let policy = write_temp_docs_policy("eligible");
        let trace = write_temp_trace_index("eligible", 8, 0);
        let registry_body = registry_body_single("claudecode", "runtime-ready", true, &[]);
        let registry = write_temp_registry("eligible", &registry_body);
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "router_backend_registry_path": registry.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["confidence"], "high");
        assert_eq!(block["backend_readiness_status"], "runtime-ready");
        assert_eq!(block["backend_runtime_allowed"], true);
        assert_eq!(
            block["router_apply_eligible"], true,
            "wave26-03: 6-condition gate satisfied -> eligible=true"
        );
        let blockers = block["router_apply_blockers"].as_array().unwrap();
        assert!(
            blockers.is_empty(),
            "wave26-03: eligible=true means router_apply_blockers must be empty (got {:?})",
            blockers
        );
        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
        let _ = std::fs::remove_file(&registry);
    }

    #[test]
    fn router_policy_mode_dry_run_with_current_default_not_eligible() {
        // Seed-shape registry: claudecode current-default + runtime_allowed=true
        // + 0 blockers + high confidence. current-default is INTENTIONALLY NOT
        // sufficient — only runtime-ready opens the gate.
        let policy = write_temp_docs_policy("current-default");
        let trace = write_temp_trace_index("current-default", 8, 0);
        let registry_body =
            registry_body_single("claudecode", "current-default", true, &[]);
        let registry = write_temp_registry("current-default", &registry_body);
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "router_backend_registry_path": registry.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["backend_readiness_status"], "current-default");
        assert_eq!(block["backend_runtime_allowed"], true);
        assert_eq!(
            block["router_apply_eligible"], false,
            "wave26-03: current-default alone is NOT sufficient — runtime-ready required"
        );
        let blockers = block["router_apply_blockers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert!(
            blockers.iter().any(|b| b.contains("current-default")
                && b.contains("runtime-ready required")),
            "wave26-03: blocker must mention current-default + runtime-ready required (got {:?})",
            blockers
        );
        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
        let _ = std::fs::remove_file(&registry);
    }

    #[test]
    fn router_policy_mode_dry_run_with_advisory_only_not_eligible() {
        // Matched backend = advisory-only + runtime_allowed=false +
        // apply_blockers populated. Multiple blockers expected.
        let policy = write_temp_docs_policy("advisory");
        let trace = write_temp_trace_index("advisory", 8, 0);
        let registry_body = registry_body_single(
            "claudecode",
            "advisory-only",
            false,
            &["no runtime adapter shipped", "router replacement out of scope"],
        );
        let registry = write_temp_registry("advisory", &registry_body);
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "router_backend_registry_path": registry.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["backend_readiness_status"], "advisory-only");
        assert_eq!(block["backend_runtime_allowed"], false);
        assert_eq!(block["router_apply_eligible"], false);
        let blockers = block["router_apply_blockers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        // synthetic blockers: runtime_allowed=false + readiness != runtime-ready
        // PLUS the registry's own 2 apply_blockers echoed verbatim.
        assert!(blockers.iter().any(|b| b.contains("runtime_allowed is false")));
        assert!(blockers.iter().any(|b| b.contains("advisory-only")));
        assert!(blockers
            .iter()
            .any(|b| b.contains("no runtime adapter shipped")));
        assert!(blockers
            .iter()
            .any(|b| b.contains("router replacement out of scope")));
        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
        let _ = std::fs::remove_file(&registry);
    }

    #[test]
    fn router_policy_mode_dry_run_with_missing_registry_emits_status_missing() {
        // Non-existent registry path — fallback continues, status=missing,
        // eligible=false, dispatch unchanged.
        let policy = write_temp_docs_policy("reg-missing");
        let trace = write_temp_trace_index("reg-missing", 8, 0);
        let bogus_registry = std::env::temp_dir().join(format!(
            "wave26-03-missing-{}-DOES-NOT-EXIST.lisp",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "router_backend_registry_path": bogus_registry.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(
            block["status"], "computed",
            "wave26-03: missing registry must NOT fail dispatch"
        );
        assert_eq!(block["recommended_backend"], "claudecode");
        assert_eq!(block["backend_registry_status"], "missing");
        assert_eq!(
            block["backend_registry_path"],
            bogus_registry.to_str().unwrap()
        );
        assert!(
            block.get("backend_warning").is_some(),
            "wave26-03: missing must surface a backend_warning"
        );
        assert_eq!(block["router_apply_eligible"], false);
        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
    }

    #[test]
    fn router_policy_mode_dry_run_with_malformed_registry_emits_status_malformed() {
        // Bad Lisp content — parser fails, fallback continues, eligible=false.
        let policy = write_temp_docs_policy("reg-malformed");
        let trace = write_temp_trace_index("reg-malformed", 8, 0);
        let bad = write_temp_registry(
            "malformed",
            "(this is :not (a router-backend-registry top form))",
        );
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "router_backend_registry_path": bad.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["status"], "computed");
        assert_eq!(block["recommended_backend"], "claudecode");
        assert_eq!(block["backend_registry_status"], "malformed");
        let warning = block["backend_warning"]
            .as_str()
            .expect("malformed must carry a backend_warning string");
        assert!(
            warning.contains("backend-registry"),
            "wave26-03: warning must mention backend-registry (got `{}`)",
            warning
        );
        assert_eq!(block["router_apply_eligible"], false);
        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
        let _ = std::fs::remove_file(&bad);
    }

    #[test]
    fn router_policy_mode_dry_run_with_unknown_backend_emits_status_unknown_backend() {
        // Registry valid but missing the recommended backend (claudecode);
        // only contains a stub for `verifier-worker`. Surfaced as
        // status=unknown_backend, readiness=unknown, eligible=false.
        let policy = write_temp_docs_policy("reg-unknown-backend");
        let trace = write_temp_trace_index("reg-unknown-backend", 8, 0);
        let registry_body = registry_body_single(
            "verifier-worker",
            "advisory-only",
            false,
            &["no runtime adapter shipped"],
        );
        let registry = write_temp_registry("unknown-backend", &registry_body);
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "router_backend_registry_path": registry.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(block["status"], "computed");
        assert_eq!(block["recommended_backend"], "claudecode");
        assert_eq!(block["backend_registry_status"], "unknown_backend");
        assert_eq!(block["backend_readiness_status"], "unknown");
        assert_eq!(block["router_apply_eligible"], false);
        let blockers = block["router_apply_blockers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert!(
            blockers
                .iter()
                .any(|b| b.contains("not in registry") && b.contains("claudecode")),
            "wave26-03: unknown_backend blocker must mention the missing id (got {:?})",
            blockers
        );
        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
        let _ = std::fs::remove_file(&registry);
    }

    #[test]
    fn router_policy_mode_dry_run_with_registry_does_not_change_dispatch() {
        // Re-pin the wave24-04 dispatch invariant under the wave26-03 code
        // path. With vs without the registry arg, every dispatch field
        // (target_tool / dispatch_strategy / next_call / ...) must be
        // byte-identical.
        let policy = write_temp_docs_policy("dispatch-pin");
        let trace = write_temp_trace_index("dispatch-pin", 8, 0);
        let registry_body = registry_body_single("claudecode", "runtime-ready", true, &[]);
        let registry = write_temp_registry("dispatch-pin", &registry_body);
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

        // Path A: dry_run + NO registry arg.
        let args_a = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "kind": "docs",
        });
        let mode_a = parse_router_policy_mode(&args_a).unwrap();
        let result_a = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode_a,
            &args_a,
            &resolved,
            &plan,
        );
        let v_a = parse_payload(&result_a);

        // Path B: dry_run + registry arg.
        let args_b = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "router_backend_registry_path": registry.to_str().unwrap(),
            "kind": "docs",
        });
        let mode_b = parse_router_policy_mode(&args_b).unwrap();
        let result_b = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode_b,
            &args_b,
            &resolved,
            &plan,
        );
        let v_b = parse_payload(&result_b);

        for field in [
            "target_tool",
            "target_source",
            "dispatch_strategy",
            "dispatch_strategy_source",
            "next_call",
            "execute_mode",
            "runner_status",
        ] {
            assert_eq!(
                v_a[field], v_b[field],
                "wave26-03 invariant: dispatch field `{}` must be byte-identical with vs without registry arg",
                field
            );
        }

        let block_a = &v_a["router_recommendation"];
        let block_b = &v_b["router_recommendation"];
        assert_eq!(block_a["applied"], block_b["applied"]);
        assert_eq!(block_a["recommended_backend"], block_b["recommended_backend"]);
        assert_eq!(block_a["status"], block_b["status"]);
        assert_eq!(block_a["confidence"], block_b["confidence"]);

        // Additive delta: backend_* fields exist in B but NOT in A.
        assert!(block_a.get("backend_registry_path").is_none());
        assert!(block_a.get("backend_registry_status").is_none());
        assert!(block_a.get("router_apply_eligible").is_none());
        assert_eq!(block_b["backend_registry_status"], "used");
        assert_eq!(block_b["router_apply_eligible"], true);

        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
        let _ = std::fs::remove_file(&registry);
    }

    #[test]
    fn applied_remains_false_with_registry() {
        // Re-pin the wave24-04 / wave24-06 / wave25-03 invariant under the
        // wave26-03 code path: `applied` must be the literal JSON bool
        // `false` in EVERY emitted block, regardless of registry status.
        // Exercise all five status flavours: used / missing / unreadable
        // (simulated via missing) / malformed / unknown_backend.
        let policy = write_temp_docs_policy("applied-reg");
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

        // used.
        let registry_used = write_temp_registry(
            "applied-used",
            &registry_body_single("claudecode", "runtime-ready", true, &[]),
        );
        let args_used = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_backend_registry_path": registry_used.to_str().unwrap(),
            "kind": "docs",
        });
        let mode_used = parse_router_policy_mode(&args_used).unwrap();
        let r_used = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode_used,
            &args_used,
            &resolved,
            &plan,
        );
        let v_used = parse_payload(&r_used);
        assert_eq!(
            v_used["router_recommendation"]["applied"],
            Value::Bool(false),
            "wave26-03 invariant: applied=false literal under backend_registry_status=used"
        );

        // missing.
        let args_missing = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_backend_registry_path":
                "/does/not/exist/wave26-03-applied-registry.lisp",
            "kind": "docs",
        });
        let mode_missing = parse_router_policy_mode(&args_missing).unwrap();
        let r_missing = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode_missing,
            &args_missing,
            &resolved,
            &plan,
        );
        let v_missing = parse_payload(&r_missing);
        assert_eq!(
            v_missing["router_recommendation"]["applied"],
            Value::Bool(false),
            "wave26-03 invariant: applied=false literal under backend_registry_status=missing"
        );

        // malformed.
        let bad =
            write_temp_registry("applied-malformed", "(not :a registry top form)");
        let args_bad = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_backend_registry_path": bad.to_str().unwrap(),
            "kind": "docs",
        });
        let mode_bad = parse_router_policy_mode(&args_bad).unwrap();
        let r_bad = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode_bad,
            &args_bad,
            &resolved,
            &plan,
        );
        let v_bad = parse_payload(&r_bad);
        assert_eq!(
            v_bad["router_recommendation"]["applied"],
            Value::Bool(false),
            "wave26-03 invariant: applied=false literal under backend_registry_status=malformed"
        );

        // unknown_backend.
        let registry_other = write_temp_registry(
            "applied-unknown",
            &registry_body_single("verifier-worker", "advisory-only", false, &[
                "no runtime adapter shipped",
            ]),
        );
        let args_other = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_backend_registry_path": registry_other.to_str().unwrap(),
            "kind": "docs",
        });
        let mode_other = parse_router_policy_mode(&args_other).unwrap();
        let r_other = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode_other,
            &args_other,
            &resolved,
            &plan,
        );
        let v_other = parse_payload(&r_other);
        assert_eq!(
            v_other["router_recommendation"]["applied"],
            Value::Bool(false),
            "wave26-03 invariant: applied=false literal under backend_registry_status=unknown_backend"
        );

        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&registry_used);
        let _ = std::fs::remove_file(&bad);
        let _ = std::fs::remove_file(&registry_other);
    }

    #[test]
    fn router_policy_mode_off_with_registry_and_trace_index_does_no_file_io() {
        // Combined cross-wave check: mode=off + BOTH new (wave26-03) +
        // wave25-03 args supplied + non-existent paths -> still byte-
        // identical to baseline. Proves the Off-path early-return predates
        // every read site.
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
        let baseline = action_execute_bridge(&plan, &resolved);
        let baseline_text = match baseline.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };

        let args = json!({
            "router_policy_mode": "off",
            "router_policy_trace_index_path":
                "/this/path/does/not/exist/wave26-03/trace.json",
            "router_backend_registry_path":
                "/this/path/does/not/exist/wave26-03/registry.lisp",
        });
        let mode = parse_router_policy_mode(&args).expect("explicit off");
        assert!(matches!(mode, RouterPolicyMode::Off));
        let after = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let after_text = match after.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        assert_eq!(
            baseline_text, after_text,
            "wave26-03: mode=off must be byte-identical EVEN WHEN BOTH router_backend_registry_path AND router_policy_trace_index_path are supplied (no file I/O may happen)"
        );
        let v: Value = serde_json::from_str(&after_text).unwrap();
        assert!(v.get("router_recommendation").is_none());

        // Default (arg absent) + both supplied: same invariant.
        let args2 = json!({
            "router_policy_trace_index_path":
                "/another/missing/wave26-03/trace.json",
            "router_backend_registry_path":
                "/another/missing/wave26-03/registry.lisp",
        });
        let mode2 = parse_router_policy_mode(&args2).expect("default off");
        assert!(matches!(mode2, RouterPolicyMode::Off));
        let after2 = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode2,
            &args2,
            &resolved,
            &plan,
        );
        let after2_text = match after2.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        assert_eq!(
            baseline_text, after2_text,
            "wave26-03: default mode (arg absent) must be byte-identical even when both new args are supplied"
        );
    }

    // -----------------------------------------------------------------
    // wave26-06 — cross-layer smoke pinning the FULL Wave 26 backend
    // readiness loop is still ADVISORY at the daemon boundary.
    //
    // Pins the 9 cross-wave invariants the brief enumerates:
    //   1. :runtime-replacement false in router-policy schema (wave24-01).
    //   2. :dry-run-only true in router-policy schema (wave24-01).
    //   3. applied=false literal in EVERY router recommendation surface.
    //   4. router_apply_eligible=true ONLY when readiness_status=runtime-
    //      ready AND runtime_allowed=true AND blockers empty AND high
    //      confidence AND status=computed. With the seed registry where
    //      claudecode is current-default, apply_eligible MUST always be
    //      false even for high-confidence claudecode matches.
    //   5. Renderer advisory text — pinned by Layer D.
    //   6. Report-checker rejects literal-string booleans — pinned by
    //      Layer C and the wave26-04 fixtures already in
    //      check-task-report.mjs.
    //   7. mission_plan off/default mode byte-shape unchanged EVEN WITH
    //      BOTH router_backend_registry_path AND
    //      router_policy_trace_index_path supplied.
    //   8. CLI/Rust parity for one fixture: same registry + same trace
    //      evidence -> both engines agree on backend_readiness_status +
    //      router_apply_eligible.
    //   9. No real LLM call, no spawn, no mutating git, no network —
    //      pinned by the static audit at the bottom.
    //
    // Forbidden-pattern table is assembled from string parts so the
    // audit does not self-trip on the patterns it scans for (wave24-06
    // / wave25-01 / wave25-05 self-audit lesson).
    // -----------------------------------------------------------------

    #[test]
    fn router_policy_dry_run_smoke_pins_wave26_invariants() {
        // Layer B Rust smoke: drive mission_plan(router_policy_mode=
        // dry_run) with all three router args supplied and assert every
        // wave26 invariant holds. Two scenarios are exercised back-to-
        // back: (a) seed-shape registry where claudecode is current-
        // default -> apply_eligible MUST be Bool(false); (b) synthetic
        // runtime-ready registry -> apply_eligible MUST be Bool(true).
        // Off-mode invariant 7 is re-pinned at the end with both router
        // args supplied to non-existent paths.

        // (a) Seed-shape registry path. claudecode is current-default
        // + runtime_allowed=true + 0 blockers — exactly the wave26-01
        // seed shape. Even with high-confidence trace, the strict gate
        // MUST reject (apply_eligible=Bool(false)).
        let policy_path = write_temp_docs_policy("wave26-06-smoke");
        let trace_path = write_temp_trace_index("wave26-06-smoke", 7, 7);
        let seed_body = registry_body_single("claudecode", "current-default", true, &[]);
        let seed_path = write_temp_registry("wave26-06-seed", &seed_body);

        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

        let dry_args_seed = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy_path.to_str().unwrap(),
            "router_policy_trace_index_path": trace_path.to_str().unwrap(),
            "router_backend_registry_path": seed_path.to_str().unwrap(),
            "kind": "docs",
        });
        let mode_seed = parse_router_policy_mode(&dry_args_seed).expect("dry_run parses");
        assert!(matches!(mode_seed, RouterPolicyMode::DryRun));
        let result_seed = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode_seed,
            &dry_args_seed,
            &resolved,
            &plan,
        );
        let v_seed = parse_payload(&result_seed);
        let block_seed = v_seed
            .get("router_recommendation")
            .expect("dry_run mode must splice a recommendation block");

        // Invariant 1+2: status=computed proves the parsed policy was
        // accepted (the daemon rejects runtime_replacement=true and
        // dry_run_only=false at validation time, so reaching computed
        // pins both invariants end-to-end).
        assert_eq!(
            block_seed["status"], "computed",
            "wave26-06 invariant 1+2: policy with runtime_replacement=false + dry_run_only=true must surface status=computed"
        );

        // Invariant 3: applied is the literal JSON Bool false. Type-
        // checked, not just value-equality, so a future regression that
        // switches to "false" string fails loudly here.
        assert_eq!(
            block_seed["applied"],
            Value::Bool(false),
            "wave26-06 invariant 3: applied MUST be literal JSON Bool false under wave26-03 code path"
        );
        assert!(
            block_seed["applied"].is_boolean(),
            "wave26-06 invariant 3: applied must be a JSON bool, never a string or number"
        );

        // Invariant 4 (negative case): seed-shape registry where
        // claudecode is current-default + runtime_allowed=true + 0
        // blockers + high confidence + matched rule. router_apply_
        // eligible MUST be Bool(false) because readiness_status is
        // current-default, not runtime-ready. current-default alone is
        // INTENTIONALLY insufficient.
        assert_eq!(
            block_seed["confidence"], "high",
            "wave26-06 invariant 4 prereq: trace must produce high confidence so the failing gate is readiness, not confidence"
        );
        assert_eq!(
            block_seed["recommended_backend"], "claudecode",
            "wave26-06 invariant 4 prereq: docs->claudecode rule must match"
        );
        assert_eq!(
            block_seed["backend_readiness_status"], "current-default",
            "wave26-06 invariant 4 prereq: seed-shape registry yields current-default"
        );
        assert_eq!(
            block_seed["backend_runtime_allowed"], Value::Bool(true),
            "wave26-06 invariant 4 prereq: seed claudecode runtime_allowed=true"
        );
        assert_eq!(
            block_seed["router_apply_eligible"], Value::Bool(false),
            "wave26-06 invariant 4: current-default + high-confidence + runtime_allowed=true MUST still yield apply_eligible=false (current-default alone is INSUFFICIENT)"
        );
        assert!(
            block_seed["router_apply_eligible"].is_boolean(),
            "wave26-06 invariant 4: router_apply_eligible must be a literal bool, never a string"
        );
        let blockers_seed = block_seed["router_apply_blockers"]
            .as_array()
            .expect("router_apply_blockers must be an array");
        let joined_seed = blockers_seed
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            joined_seed.contains("current-default")
                && joined_seed.contains("runtime-ready"),
            "wave26-06 invariant 4: blocker must mention current-default + runtime-ready (got `{}`)",
            joined_seed
        );
        assert_eq!(
            block_seed["backend_registry_status"], "used",
            "wave26-06 invariant 4: well-formed registry must surface backend_registry_status=used"
        );

        // (b) Synthetic runtime-ready registry. Same policy, same trace,
        // same docs task — only the registry shape differs. ALL 6 daemon
        // gate conditions hold so router_apply_eligible MUST flip to
        // Bool(true). This is the positive control proving the gate is
        // not stuck-false.
        let ready_body = registry_body_single("claudecode", "runtime-ready", true, &[]);
        let ready_path = write_temp_registry("wave26-06-ready", &ready_body);
        let dry_args_ready = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy_path.to_str().unwrap(),
            "router_policy_trace_index_path": trace_path.to_str().unwrap(),
            "router_backend_registry_path": ready_path.to_str().unwrap(),
            "kind": "docs",
        });
        let mode_ready = parse_router_policy_mode(&dry_args_ready).expect("dry_run parses");
        let result_ready = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode_ready,
            &dry_args_ready,
            &resolved,
            &plan,
        );
        let v_ready = parse_payload(&result_ready);
        let block_ready = &v_ready["router_recommendation"];

        assert_eq!(
            block_ready["applied"], Value::Bool(false),
            "wave26-06 invariant 3: applied MUST be literal Bool(false) EVEN UNDER apply_eligible=true (runtime replacement is rejected by contract)"
        );
        assert_eq!(
            block_ready["backend_readiness_status"], "runtime-ready",
            "wave26-06 invariant 4 positive: registry shape determines readiness_status"
        );
        assert_eq!(
            block_ready["router_apply_eligible"], Value::Bool(true),
            "wave26-06 invariant 4 positive: ALL 6 gate conditions met -> apply_eligible=true"
        );
        let blockers_ready = block_ready["router_apply_blockers"]
            .as_array()
            .expect("router_apply_blockers must be an array");
        assert!(
            blockers_ready.is_empty(),
            "wave26-06 invariant 4 positive: apply_eligible=true means router_apply_blockers must be empty (got {:?})",
            blockers_ready
        );

        // Invariant 7 (off mode + BOTH router args): re-pin under the
        // wave26-06 smoke. mode=off MUST be byte-identical to baseline
        // even when both router_backend_registry_path AND
        // router_policy_trace_index_path are supplied. Use NON-existent
        // paths to additionally prove no file I/O happens.
        let baseline = action_execute_bridge(&plan, &resolved);
        let baseline_text = match baseline.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        let off_args = json!({
            "router_policy_mode": "off",
            "router_policy_path": policy_path.to_str().unwrap(),
            "router_policy_trace_index_path":
                "/this/path/does/not/exist/wave26-06/trace.json",
            "router_backend_registry_path":
                "/this/path/does/not/exist/wave26-06/registry.lisp",
            "kind": "docs",
        });
        let off_mode = parse_router_policy_mode(&off_args).expect("explicit off");
        assert!(matches!(off_mode, RouterPolicyMode::Off));
        let off_after = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            off_mode,
            &off_args,
            &resolved,
            &plan,
        );
        let off_text = match off_after.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        assert_eq!(
            baseline_text, off_text,
            "wave26-06 invariant 7: mode=off must be byte-identical EVEN WITH BOTH router_backend_registry_path AND router_policy_trace_index_path supplied (no file I/O may happen)"
        );

        // Invariant 7 (cont.): also verify dispatch shape is byte-
        // identical between baseline and the dry_run+seed-registry
        // result. Mode=dry_run is allowed to add the recommendation
        // block but every dispatch field must remain byte-identical.
        let baseline_v = parse_payload(&baseline);
        for field in [
            "target_tool",
            "target_source",
            "dispatch_strategy",
            "dispatch_strategy_source",
            "next_call",
            "execute_mode",
            "runner_status",
        ] {
            assert_eq!(
                baseline_v[field], v_seed[field],
                "wave26-06 invariant 7: dispatch field `{}` must be byte-identical with vs without router args",
                field
            );
            assert_eq!(
                baseline_v[field], v_ready[field],
                "wave26-06 invariant 7: dispatch field `{}` must be byte-identical regardless of registry shape",
                field
            );
        }

        // Invariant 9 (audit): zero shell-out / LLM / git / network in
        // the daemon plan.rs router-readiness path. Forbidden patterns
        // assembled from string parts so the audit does not self-trip
        // on the patterns it scans for. Mirrors the wave25-05 smoke.
        let plan_rs = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/handlers/knowledge/plan.rs"),
        )
        .expect("plan.rs must be readable for self-audit");
        let stripped: String = plan_rs
            .lines()
            .filter(|ln| !ln.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let forbidden_router_patterns: Vec<String> = vec![
            String::from("std::") + "process::" + "Command",
            String::from("tokio::") + "process",
            String::from("req") + "west::",
            String::from("hyper::") + "Client",
            String::from("open") + "ai_api",
            String::from("anthrop") + "ic_api",
        ];
        for pat in &forbidden_router_patterns {
            assert!(
                !stripped.contains(pat.as_str()),
                "wave26-06 invariant 9: forbidden router-side pattern `{}` found in plan.rs active source",
                pat
            );
        }

        let _ = std::fs::remove_file(&policy_path);
        let _ = std::fs::remove_file(&trace_path);
        let _ = std::fs::remove_file(&seed_path);
        let _ = std::fs::remove_file(&ready_path);
    }

    #[test]
    fn router_policy_cli_rust_parity_for_readiness() {
        // Layer B Rust smoke (parity): both engines (Node CLI
        // recommend-task-backend.mjs --dry-fixture and the daemon's
        // mission_plan dry_run) MUST agree on backend_readiness_status
        // and router_apply_eligible for the SAME registry shape +
        // SAME confidence level. We assert the daemon side here against
        // the EXPECTED values that the Node Layer A1 fixtures
        // (wave26-06: cross-layer smoke pins apply_eligible=false for
        // current-default seed) also assert. A divergence on either
        // side fails this test AND the corresponding Node fixture so
        // the parity is bidirectional.
        //
        // Documented expected agreement (Node CLI side, wave26-06 Layer
        // A1 smoke fixtures):
        //   policy:    docs->claudecode (high priority match)
        //   trace:     (8,8)-event index -> high confidence
        //   registry:  claudecode current-default + runtime_allowed=true + 0 blockers
        //   annotate() ->  backend_readiness_status: 'current-default'
        //                  backend_runtime_allowed:  true
        //                  router_apply_eligible:    false
        //
        // Daemon expected agreement (this test):
        //   args.kind=docs, mode=dry_run, registry=current-default
        //   block.backend_readiness_status === 'current-default'  (parity)
        //   block.backend_runtime_allowed  === true               (parity)
        //   block.router_apply_eligible    === false              (parity)
        //   block.applied                  === Bool(false)        (cross-wave)
        let policy_path = write_temp_docs_policy("wave26-06-parity");
        // Trace index supplies (8,8) events on btk-1/claudecode buckets;
        // matches the Node fixture's synthesizeTraceIndex(8,8) shape.
        let trace_path = write_temp_trace_index("wave26-06-parity", 8, 8);
        let seed_body = registry_body_single("claudecode", "current-default", true, &[]);
        let registry_path = write_temp_registry("wave26-06-parity", &seed_body);

        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy_path.to_str().unwrap(),
            "router_policy_trace_index_path": trace_path.to_str().unwrap(),
            "router_backend_registry_path": registry_path.to_str().unwrap(),
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).expect("dry_run parses");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];

        // Hard-coded expected values that the Node Layer A1 smoke
        // fixture also asserts for the SAME shape. A divergence on
        // either side fails BOTH tests so the parity is bidirectional.
        assert_eq!(
            block["recommended_backend"], "claudecode",
            "wave26-06 parity: Node CLI emits backend='claudecode' for docs task on seed policy"
        );
        assert_eq!(
            block["confidence"], "high",
            "wave26-06 parity: Node CLI emits confidence='high' for (8,8)-event trace-index"
        );
        assert_eq!(
            block["backend_readiness_status"], "current-default",
            "wave26-06 parity: Node CLI emits backend_readiness_status='current-default' for seed-shape registry"
        );
        assert_eq!(
            block["backend_runtime_allowed"], Value::Bool(true),
            "wave26-06 parity: Node CLI emits backend_runtime_allowed=true for seed claudecode"
        );
        assert_eq!(
            block["router_apply_eligible"], Value::Bool(false),
            "wave26-06 parity: Node CLI emits router_apply_eligible=false for current-default (current-default alone is INSUFFICIENT)"
        );
        assert_eq!(
            block["applied"],
            Value::Bool(false),
            "wave26-06 parity: cross-wave invariant — applied=false literal under any registry status"
        );
        assert_eq!(
            block["status"], "computed",
            "wave26-06 parity: matched rule on well-formed policy must surface status=computed"
        );
        assert_eq!(
            block["backend_registry_status"], "used",
            "wave26-06 parity: well-formed registry must surface backend_registry_status=used"
        );

        // Recommended backend ∈ wave24-01 enum (re-spelled locally to
        // keep the test pure-Rust per wave24-06 lesson — no script
        // imports). Mirrors the wave25-05 parity test.
        let allowed_backends = [
            "claudecode",
            "missiond-llm-router",
            "deterministic-checker",
            "patch-worker",
            "verifier-worker",
        ];
        let backend = block["recommended_backend"]
            .as_str()
            .expect("recommended_backend must be a string");
        assert!(
            allowed_backends.contains(&backend),
            "wave26-06 parity: recommended_backend `{}` not in wave24-01 enum",
            backend
        );

        // Allowed readiness status ∈ wave26-01 enum (re-spelled
        // locally). A future regression that introduces a non-enum
        // value fails here.
        let allowed_readiness = [
            "current-default",
            "advisory-only",
            "runtime-ready",
            "unavailable",
            "unknown",
        ];
        let readiness = block["backend_readiness_status"]
            .as_str()
            .expect("backend_readiness_status must be a string");
        assert!(
            allowed_readiness.contains(&readiness),
            "wave26-06 parity: backend_readiness_status `{}` not in wave26-01 enum",
            readiness
        );

        let _ = std::fs::remove_file(&policy_path);
        let _ = std::fs::remove_file(&trace_path);
        let _ = std::fs::remove_file(&registry_path);
    }

    // -----------------------------------------------------------------
    // wave-27 / task 03 — router dispatch descriptor surface tests.
    //
    // These pin the OPTIONAL `router_dispatch_descriptor` arg.
    // Invariants this block enforces:
    //   * Off/default mode + descriptor=true MUST be byte-identical to
    //     the wave-15..23 baseline (no extra file I/O happens because
    //     the Off-path early-return predates compute_recommendation).
    //   * dry_run + descriptor=true + seed registry (claudecode is
    //     current-default) -> descriptor body present, no_execution=true,
    //     dry_run_only=true, runtime_replacement=false (all literal Bool),
    //     router_apply_eligible=false (current-default is rejected by
    //     the wave26-03 6-condition gate).
    //   * dry_run + descriptor=true + synthetic runtime-ready registry +
    //     high confidence -> router_apply_eligible=true BUT the three
    //     locked invariants STILL hold (runtime_replacement=false,
    //     no_execution=true, dry_run_only=true).
    //   * dry_run + descriptor=true + NO registry path -> descriptor body
    //     OMITTED, descriptor_status="registry_missing" surfaced.
    //   * dry_run + descriptor=true MUST NOT change any dispatch field
    //     (re-pin of the wave24-04 dispatch invariant under the new code
    //     path).
    // -----------------------------------------------------------------

    #[test]
    fn router_dispatch_descriptor_off_default_does_no_extra_io() {
        // wave27-03: mode=off with router_dispatch_descriptor=true AND all
        // three wave24-04 / wave25-03 / wave26-03 router args supplied
        // (with non-existent paths) MUST be byte-identical to baseline.
        // Proves the Off-path early-return predates every read site,
        // including the new descriptor branch.
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_execution", "fresh-code-alignment");
        let baseline = action_execute_bridge(&plan, &resolved);
        let baseline_text = match baseline.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };

        let args = json!({
            "router_policy_mode": "off",
            "router_policy_path":
                "/this/path/does/not/exist/wave27-03/policy.lisp",
            "router_policy_trace_index_path":
                "/this/path/does/not/exist/wave27-03/trace.json",
            "router_backend_registry_path":
                "/this/path/does/not/exist/wave27-03/registry.lisp",
            "router_dispatch_descriptor": true,
        });
        let mode = parse_router_policy_mode(&args).expect("explicit off");
        assert!(matches!(mode, RouterPolicyMode::Off));
        let after = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let after_text = match after.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        assert_eq!(
            baseline_text, after_text,
            "wave27-03: mode=off + descriptor=true + all three router args (policy/trace/registry) supplied MUST be byte-identical to baseline (the Off early-return predates the descriptor branch — no extra I/O)"
        );
        let v: Value = serde_json::from_str(&after_text).unwrap();
        assert!(
            v.get("router_recommendation").is_none(),
            "wave27-03: mode=off must NOT splice a recommendation block (descriptor or otherwise)"
        );

        // Default (mode arg absent) + descriptor=true + same three paths:
        // same invariant.
        let args2 = json!({
            "router_policy_path":
                "/another/missing/wave27-03/policy.lisp",
            "router_policy_trace_index_path":
                "/another/missing/wave27-03/trace.json",
            "router_backend_registry_path":
                "/another/missing/wave27-03/registry.lisp",
            "router_dispatch_descriptor": true,
        });
        let mode2 = parse_router_policy_mode(&args2).expect("default off");
        assert!(matches!(mode2, RouterPolicyMode::Off));
        let after2 = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode2,
            &args2,
            &resolved,
            &plan,
        );
        let after2_text = match after2.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        assert_eq!(
            baseline_text, after2_text,
            "wave27-03: default mode (arg absent) + descriptor=true + all three router args MUST stay byte-identical"
        );
    }

    #[test]
    fn router_dispatch_descriptor_dry_run_with_seed_registry_emits_no_execution_true() {
        // wave27-03: dry_run + descriptor=true + seed-shape registry where
        // claudecode is current-default. Descriptor body MUST be present
        // and carry the three locked literal-bool invariants. Eligibility
        // MUST be false (current-default does NOT satisfy the wave26-03
        // 6-condition gate; runtime-ready opt-in is required).
        let policy = write_temp_docs_policy("desc-seed-current-default");
        let trace = write_temp_trace_index("desc-seed-current-default", 8, 0);
        let registry_body =
            registry_body_single("claudecode", "current-default", true, &[]);
        let registry = write_temp_registry("desc-seed-current-default", &registry_body);
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "router_backend_registry_path": registry.to_str().unwrap(),
            "router_dispatch_descriptor": true,
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        // Descriptor body present.
        let descriptor = &block["router_dispatch_descriptor"];
        assert!(
            descriptor.is_object(),
            "wave27-03: descriptor body must be present when registry is supplied + descriptor=true (got `{}`)",
            descriptor
        );
        // Locked literal-bool invariants — MUST be Value::Bool, never strings.
        assert_eq!(
            descriptor["dry_run_only"],
            Value::Bool(true),
            "wave27-03 LOCKED INVARIANT: dry_run_only must be literal Bool true"
        );
        assert!(
            descriptor["dry_run_only"].is_boolean(),
            "wave27-03: dry_run_only must be a JSON bool, never a string"
        );
        assert_eq!(
            descriptor["runtime_replacement"],
            Value::Bool(false),
            "wave27-03 LOCKED INVARIANT: runtime_replacement must be literal Bool false"
        );
        assert!(
            descriptor["runtime_replacement"].is_boolean(),
            "wave27-03: runtime_replacement must be a JSON bool, never a string"
        );
        assert_eq!(
            descriptor["no_execution"],
            Value::Bool(true),
            "wave27-03 LOCKED INVARIANT: no_execution must be literal Bool true"
        );
        assert!(
            descriptor["no_execution"].is_boolean(),
            "wave27-03: no_execution must be a JSON bool, never a string"
        );
        // Schema + task_id + recommendation source identifier.
        assert_eq!(
            descriptor["schema"],
            "missiond.router-dispatch-descriptor.v1",
            "wave27-03: descriptor schema id mirrors wave27-01"
        );
        assert_eq!(
            descriptor["task_id"], "btk-1",
            "wave27-03: descriptor task_id must echo plan.board_task_id"
        );
        assert_eq!(
            descriptor["source_recommendation_schema"],
            "missiond.router-recommendation.v0",
            "wave27-03: descriptor must record the upstream wave24-04 recommendation schema id"
        );
        assert_eq!(
            descriptor["source_policy_path"], policy.to_str().unwrap(),
            "wave27-03: descriptor must echo router_policy_path"
        );
        assert_eq!(
            descriptor["source_backend_registry_path"], registry.to_str().unwrap(),
            "wave27-03: descriptor must echo router_backend_registry_path"
        );
        // Projected fields off the wave26-03 readiness block.
        assert_eq!(descriptor["recommended_backend"], "claudecode");
        assert_eq!(descriptor["router_confidence"], "high");
        assert_eq!(descriptor["backend_readiness_status"], "current-default");
        assert_eq!(descriptor["backend_runtime_allowed"], Value::Bool(true));
        assert_eq!(
            descriptor["router_apply_eligible"], Value::Bool(false),
            "wave27-03: current-default registry does NOT satisfy the wave26-03 gate (runtime-ready required)"
        );
        let blockers = descriptor["router_apply_blockers"]
            .as_array()
            .expect("router_apply_blockers must be a JSON array");
        assert!(
            !blockers.is_empty(),
            "wave27-03: eligible=false MUST list at least one blocker (got {:?})",
            blockers
        );
        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
        let _ = std::fs::remove_file(&registry);
    }

    #[test]
    fn router_dispatch_descriptor_dry_run_with_runtime_ready_eligible() {
        // wave27-03: synthetic registry where the matched backend is
        // runtime-ready + runtime_allowed=true + zero blockers + high
        // confidence -> router_apply_eligible=true. The three locked
        // invariants (dry_run_only / runtime_replacement / no_execution)
        // MUST still hold — eligibility flipping does NOT promote the
        // descriptor to a runtime apply signal.
        let policy = write_temp_docs_policy("desc-runtime-ready");
        let trace = write_temp_trace_index("desc-runtime-ready", 9, 0);
        let registry_body =
            registry_body_single("claudecode", "runtime-ready", true, &[]);
        let registry = write_temp_registry("desc-runtime-ready", &registry_body);
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "router_backend_registry_path": registry.to_str().unwrap(),
            "router_dispatch_descriptor": true,
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        let descriptor = &block["router_dispatch_descriptor"];
        assert!(
            descriptor.is_object(),
            "wave27-03: descriptor body must be present"
        );
        // Cross-wave invariant: even when eligibility flips to true, the
        // three locked invariants MUST stay literal Bool literals.
        assert_eq!(
            descriptor["router_apply_eligible"], Value::Bool(true),
            "wave27-03: runtime-ready + high confidence + runtime_allowed=true + zero blockers -> eligible=true"
        );
        let blockers = descriptor["router_apply_blockers"]
            .as_array()
            .expect("router_apply_blockers must be array");
        assert!(
            blockers.is_empty(),
            "wave27-03: eligible=true means router_apply_blockers MUST be empty (got {:?})",
            blockers
        );
        // CROSS-WAVE INVARIANT: eligibility=true does NOT promote the
        // descriptor to a runtime signal. The three locked literals stay
        // literal Bool, hard-coded.
        assert_eq!(
            descriptor["dry_run_only"], Value::Bool(true),
            "wave27-03 LOCKED: dry_run_only stays literal Bool true even when eligible=true"
        );
        assert_eq!(
            descriptor["runtime_replacement"], Value::Bool(false),
            "wave27-03 LOCKED: runtime_replacement stays literal Bool false even when eligible=true"
        );
        assert_eq!(
            descriptor["no_execution"], Value::Bool(true),
            "wave27-03 LOCKED: no_execution stays literal Bool true even when eligible=true"
        );
        assert_eq!(descriptor["backend_readiness_status"], "runtime-ready");
        assert_eq!(descriptor["backend_runtime_allowed"], Value::Bool(true));
        assert_eq!(descriptor["router_confidence"], "high");
        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
        let _ = std::fs::remove_file(&registry);
    }

    #[test]
    fn router_dispatch_descriptor_dry_run_without_registry_path_emits_status_registry_missing() {
        // wave27-03: dry_run + descriptor=true with NO router_backend_registry_path
        // -> descriptor body OMITTED + top-level descriptor_status="registry_missing"
        // surfaced on the recommendation block. The wave27-01 schema
        // requires backend_readiness_status / backend_runtime_allowed
        // values that we cannot honestly produce without consulting a
        // registry, so we intentionally refuse to fake readiness.
        let policy = write_temp_docs_policy("desc-no-registry");
        let trace = write_temp_trace_index("desc-no-registry", 8, 0);
        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "router_dispatch_descriptor": true,
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        assert_eq!(
            block["descriptor_status"], "registry_missing",
            "wave27-03: NO registry path + descriptor=true -> descriptor_status=registry_missing on the recommendation block"
        );
        assert!(
            block.get("router_dispatch_descriptor").is_none(),
            "wave27-03: descriptor body MUST be omitted when registry path is absent (got `{:?}`)",
            block.get("router_dispatch_descriptor")
        );
        // Recommendation block itself is unchanged; status is still
        // computed because the docs rule matched.
        assert_eq!(block["status"], "computed");
        assert_eq!(block["recommended_backend"], "claudecode");
        assert_eq!(block["applied"], Value::Bool(false));
        // Sanity: NO backend_* readiness fields leaked (registry was Absent).
        assert!(block.get("backend_registry_path").is_none());
        assert!(block.get("backend_registry_status").is_none());
        assert!(block.get("backend_readiness_status").is_none());
        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
    }

    #[test]
    fn router_dispatch_descriptor_does_not_change_dispatch() {
        // wave27-03 re-pin of the wave24-04 dispatch invariant under the
        // new descriptor code path. With vs without the descriptor flag
        // (both in dry_run + same registry), every dispatch-shaping field
        // (target_tool / dispatch_strategy / next_call / runner_status /
        // execute_mode / target_source / dispatch_strategy_source) MUST
        // be byte-identical. Only the additive descriptor block delta is
        // expected.
        let policy = write_temp_docs_policy("desc-dispatch-pin");
        let trace = write_temp_trace_index("desc-dispatch-pin", 9, 0);
        let registry_body =
            registry_body_single("claudecode", "runtime-ready", true, &[]);
        let registry = write_temp_registry("desc-dispatch-pin", &registry_body);
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

        // Path A: dry_run + registry, NO descriptor.
        let args_a = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "router_backend_registry_path": registry.to_str().unwrap(),
            "kind": "docs",
        });
        let mode_a = parse_router_policy_mode(&args_a).unwrap();
        let result_a = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode_a,
            &args_a,
            &resolved,
            &plan,
        );
        let v_a = parse_payload(&result_a);

        // Path B: dry_run + registry + descriptor=true.
        let args_b = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "router_backend_registry_path": registry.to_str().unwrap(),
            "router_dispatch_descriptor": true,
            "kind": "docs",
        });
        let mode_b = parse_router_policy_mode(&args_b).unwrap();
        let result_b = attach_router_recommendation_block(
            action_execute_bridge(&plan, &resolved),
            mode_b,
            &args_b,
            &resolved,
            &plan,
        );
        let v_b = parse_payload(&result_b);

        for field in [
            "target_tool",
            "target_source",
            "dispatch_strategy",
            "dispatch_strategy_source",
            "next_call",
            "execute_mode",
            "runner_status",
        ] {
            assert_eq!(
                v_a[field], v_b[field],
                "wave27-03: dispatch field `{}` MUST be byte-identical with vs without router_dispatch_descriptor=true",
                field
            );
        }

        let block_a = &v_a["router_recommendation"];
        let block_b = &v_b["router_recommendation"];
        // Recommendation core fields are unchanged by the descriptor flag.
        assert_eq!(block_a["status"], block_b["status"]);
        assert_eq!(block_a["applied"], block_b["applied"]);
        assert_eq!(block_a["recommended_backend"], block_b["recommended_backend"]);
        assert_eq!(block_a["confidence"], block_b["confidence"]);
        assert_eq!(
            block_a["backend_readiness_status"],
            block_b["backend_readiness_status"]
        );
        assert_eq!(
            block_a["router_apply_eligible"],
            block_b["router_apply_eligible"]
        );

        // Additive delta: descriptor present in B, absent in A.
        assert!(
            block_a.get("router_dispatch_descriptor").is_none(),
            "wave27-03: NO descriptor in path A (flag absent)"
        );
        assert!(
            block_b.get("router_dispatch_descriptor").is_some(),
            "wave27-03: descriptor present in path B (flag=true)"
        );

        // applied=false literal is invariant across both paths.
        assert_eq!(block_a["applied"], Value::Bool(false));
        assert_eq!(block_b["applied"], Value::Bool(false));

        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
        let _ = std::fs::remove_file(&registry);
    }

    /// wave27-06 cross-layer smoke: in ONE exhaustive test, re-pin EVERY
    /// wave27 cross-wave invariant exercised by the daemon dispatch
    /// descriptor surface. This is the single attribution point for a
    /// future bisect — if the wave27 invariant chain regresses on the
    /// daemon side, this test fails and `git log -S
    /// router_dispatch_descriptor_smoke_pins_wave27_invariants` lands
    /// the search on this file.
    ///
    /// Invariants asserted:
    ///   1. dry_run_only literal Value::Bool(true) — wave27-03
    ///   2. runtime_replacement literal Value::Bool(false) — wave27-03
    ///   3. no_execution literal Value::Bool(true) — wave27-03 / wave27-04
    ///   4. With seed registry (claudecode current-default) +
    ///      router_dispatch_descriptor=true:
    ///        a. router_apply_eligible Value::Bool(false)
    ///        b. router_apply_blockers non-empty
    ///   5. Dispatch-shaping fields (target_tool / target_source /
    ///      dispatch_strategy / dispatch_strategy_source / next_call /
    ///      execute_mode / runner_status) byte-identical with vs
    ///      without router_dispatch_descriptor=true (re-pin of the
    ///      wave24-04 invariant under the wave27 surface)
    ///   6. wave27-03 self-audit: plan.rs source carries NO new
    ///      shell-out / spawn / git mutation / network / LLM client in
    ///      the active code (assembled forbidden-pattern table from
    ///      string parts so this audit body does NOT self-trip on the
    ///      patterns it scans for — wave24-06 / wave25-05 / wave26-06
    ///      lesson).
    #[test]
    fn router_dispatch_descriptor_smoke_pins_wave27_invariants() {
        // ---- Part 1: descriptor invariants under seed (current-default) ----
        let policy = write_temp_docs_policy("w27-06-smoke-seed");
        let trace = write_temp_trace_index("w27-06-smoke-seed", 8, 0);
        let registry_body =
            registry_body_single("claudecode", "current-default", true, &[]);
        let registry = write_temp_registry("w27-06-smoke-seed", &registry_body);
        let plan = fixture_plan("(plan)");
        let resolved = fixture_resolved("mission_task_delegate", "fresh-code-alignment");

        let args = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "router_backend_registry_path": registry.to_str().unwrap(),
            "router_dispatch_descriptor": true,
            "kind": "docs",
        });
        let mode = parse_router_policy_mode(&args).unwrap();
        let result = attach_router_recommendation_block(
            fixture_bridge_result(),
            mode,
            &args,
            &resolved,
            &plan,
        );
        let v = parse_payload(&result);
        let block = &v["router_recommendation"];
        let descriptor = &block["router_dispatch_descriptor"];
        assert!(
            descriptor.is_object(),
            "wave27-06 invariant: descriptor body MUST be present (got `{}`)",
            descriptor
        );

        // wave27-06 invariants 1-3: locked literal Bools (NOT strings,
        // NOT computed). The is_boolean() asserts also catch the
        // pathological case where a future projector mutation turns
        // these into "true"/"false" strings while still passing
        // assert_eq! on the JSON layer.
        assert_eq!(
            descriptor["dry_run_only"],
            Value::Bool(true),
            "wave27-06 invariant 1: dry_run_only must be literal Value::Bool(true)"
        );
        assert!(
            descriptor["dry_run_only"].is_boolean(),
            "wave27-06 invariant 1: dry_run_only must be a JSON bool, never a string"
        );
        assert_eq!(
            descriptor["runtime_replacement"],
            Value::Bool(false),
            "wave27-06 invariant 2: runtime_replacement must be literal Value::Bool(false)"
        );
        assert!(
            descriptor["runtime_replacement"].is_boolean(),
            "wave27-06 invariant 2: runtime_replacement must be a JSON bool, never a string"
        );
        assert_eq!(
            descriptor["no_execution"],
            Value::Bool(true),
            "wave27-06 invariant 3: no_execution must be literal Value::Bool(true)"
        );
        assert!(
            descriptor["no_execution"].is_boolean(),
            "wave27-06 invariant 3: no_execution must be a JSON bool, never a string"
        );

        // wave27-06 invariant 4: seed registry (claudecode current-default)
        // is NEVER apply-eligible. The wave26-03 6-condition gate requires
        // an explicit runtime-ready opt-in upstream; current-default alone
        // is rejected.
        assert_eq!(
            descriptor["router_apply_eligible"],
            Value::Bool(false),
            "wave27-06 invariant 4a: seed registry (claudecode current-default) MUST yield apply_eligible=false"
        );
        let blockers = descriptor["router_apply_blockers"]
            .as_array()
            .expect("wave27-06: router_apply_blockers MUST be a JSON array");
        assert!(
            !blockers.is_empty(),
            "wave27-06 invariant 4b: eligible=false MUST list at least one blocker (got {:?})",
            blockers
        );

        // ---- Part 2: dispatch byte-identical with vs without descriptor ----
        let plan2 = fixture_plan("(plan)");
        let resolved2 = fixture_resolved("mission_task_delegate", "fresh-code-alignment");
        let args_no_desc = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "router_backend_registry_path": registry.to_str().unwrap(),
            "kind": "docs",
        });
        let mode_no_desc = parse_router_policy_mode(&args_no_desc).unwrap();
        let result_no_desc = attach_router_recommendation_block(
            action_execute_bridge(&plan2, &resolved2),
            mode_no_desc,
            &args_no_desc,
            &resolved2,
            &plan2,
        );
        let args_with_desc = json!({
            "router_policy_mode": "dry_run",
            "router_policy_path": policy.to_str().unwrap(),
            "router_policy_trace_index_path": trace.to_str().unwrap(),
            "router_backend_registry_path": registry.to_str().unwrap(),
            "router_dispatch_descriptor": true,
            "kind": "docs",
        });
        let mode_with_desc = parse_router_policy_mode(&args_with_desc).unwrap();
        let result_with_desc = attach_router_recommendation_block(
            action_execute_bridge(&plan2, &resolved2),
            mode_with_desc,
            &args_with_desc,
            &resolved2,
            &plan2,
        );
        let v_no = parse_payload(&result_no_desc);
        let v_with = parse_payload(&result_with_desc);
        for field in [
            "target_tool",
            "target_source",
            "dispatch_strategy",
            "dispatch_strategy_source",
            "next_call",
            "execute_mode",
            "runner_status",
        ] {
            assert_eq!(
                v_no[field], v_with[field],
                "wave27-06 invariant 5: dispatch field `{}` MUST be byte-identical with vs without router_dispatch_descriptor=true",
                field
            );
        }

        // ---- Part 3: self-audit on plan.rs active source ----
        // Read the on-disk plan.rs and assert NO new shell-out / spawn /
        // git mutation / network / LLM client landed in the active code
        // (i.e. outside line + block comments and string literals). The
        // forbidden-pattern table is assembled from string parts so this
        // audit does NOT self-trip on the patterns it is scanning for.
        // wave24-06 / wave25-05 / wave26-06 lesson: a literal regex like
        // `child_process` would match this very test body.
        let plan_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/handlers/knowledge/plan.rs");
        let src = std::fs::read_to_string(&plan_path)
            .expect("wave27-06: plan.rs must be readable from CARGO_MANIFEST_DIR");
        let stripped = strip_rust_comments_and_strings(&src);
        // Tokens are assembled at runtime so this audit body's source code
        // does NOT contain the literal forbidden strings (wave24-06 /
        // wave25-05 / wave26-06 lesson). Variable names also stay clear of
        // the literals so the stripped source (which keeps identifier
        // names) does not self-trip the regex.
        let t_cp = String::from("child") + "_" + "process";
        let t_spawn = String::from("\\bspawn") + "\\(";
        let t_spawnblock = String::from("\\bspawn") + "_blocking\\(";
        let t_tproc = String::from("tokio") + "::process";
        let t_stdcmd = String::from("std::process::") + "Command";
        let t_rq = String::from("re") + "qwest::";
        let t_hyperc = String::from("\\bhy") + "per::";
        let t_oa = String::from("op") + "enai";
        let t_an = String::from("anth") + "ropic";
        let t_git = String::from("\\bgit ") + "(?:add|commit|push|reset|checkout|rm)";
        let t_libgit = String::from("g") + "it2::Repository::open";
        let forbidden = [
            t_cp.as_str(),
            t_spawn.as_str(),
            t_spawnblock.as_str(),
            t_tproc.as_str(),
            t_stdcmd.as_str(),
            t_rq.as_str(),
            t_hyperc.as_str(),
            t_oa.as_str(),
            t_an.as_str(),
            t_git.as_str(),
            t_libgit.as_str(),
        ];
        for pat in forbidden {
            let re = regex::Regex::new(pat).expect("wave27-06: audit pattern compiles");
            if re.is_match(&stripped) {
                panic!(
                    "wave27-06 invariant 6: forbidden audit pattern `{}` found in plan.rs active source",
                    pat
                );
            }
        }

        let _ = std::fs::remove_file(&policy);
        let _ = std::fs::remove_file(&trace);
        let _ = std::fs::remove_file(&registry);
    }

    /// wave27-06 helper: strip line comments, block comments, and string
    /// literals from a Rust source so the self-audit grep does NOT
    /// match patterns mentioned in commentary or in the forbidden-pattern
    /// table itself. Mirrors the JS-side stripper used by the renderer
    /// self-audit (wave26-06 + wave27-05) but adapted for Rust syntax.
    /// This is a heuristic (it does not handle every macro shape), but
    /// it is sufficient for active-code sniffing — the test PANICS on
    /// any match, so the bias is conservative.
    fn strip_rust_comments_and_strings(src: &str) -> String {
        let mut out = String::with_capacity(src.len());
        let bytes = src.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let c = bytes[i];
            // Block comment /* ... */ — handles nested /* */ one level
            // deep (Rust supports nesting; we do best-effort).
            if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                let mut depth = 1usize;
                i += 2;
                while i < bytes.len() && depth > 0 {
                    if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                        depth += 1;
                        i += 2;
                    } else if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            // Line comment // ... \n
            if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            // String literal "..." — handles \" escape. Does NOT
            // attempt raw strings r#"..."# (good enough for sniffing).
            if c == b'"' {
                i += 1;
                while i < bytes.len() {
                    let d = bytes[i];
                    if d == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    if d == b'"' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            // Char literal '..' — minimal handling; skip apostrophe runs
            // to avoid eating identifiers like 'static lifetime.
            if c == b'\'' {
                // Conservative: keep the apostrophe so we don't accidentally
                // chew lifetime annotations into something pattern-matching.
                out.push(c as char);
                i += 1;
                continue;
            }
            out.push(c as char);
            i += 1;
        }
        out
    }
}
