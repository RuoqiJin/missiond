//! mission_plan — manager surface for the plan table.
//!
//! Lisp authority:
//!   - intent-memory.lisp :: module directive-layer :: plumbing plan-execution
//!   - intent-flow.lisp :: F-directive-plan-workflow-compile :: plan branch
//!   - intent-tools.lisp :: future-surface mission_plan
//!
//! Action coverage:
//!   compile          — dry-run (plan-compiler actor not yet wired);
//!                      inserts a draft plan row when persist=true and
//!                      board_task_id provided
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
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::state::AppState;
use missiond_core::types::{Plan, PlanStatus};

const DEFAULT_LIST_LIMIT: i64 = 50;
const MAX_LIST_LIMIT: i64 = 500;
const COMPANION_DIR: &str = ".missiond/v2/plans";
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
// compile — plan-compiler actor not yet implemented; dry-run preview
// ───────────────────────────────────────────────────────────────────────

async fn action_compile(state: &AppState, args: &Value) -> Result<ToolResult> {
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
        "actor_pending": "intent-layer :: plan-compiler (LLM)",
        "flow_ref": "F-directive-plan-workflow-compile :: plan branch",
        "directive_id": directive_id,
        "board_task_id": board_task_id,
        "compiled_sexp_preview": dry_run_sexp,
        "sexp_hash_preview": sexp_hash,
        "next_step": "future actor produces DAG sexp; for now insert with persist=true and refine via action=mark",
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
    } else {
        payload["persisted"] = json!(false);
    }
    Ok(ToolResult::json_pretty(&payload))
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

async fn action_approve(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "plan_id")?;
    state
        .store
        .plan_update_status(id, PlanStatus::Approved)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&json!({
        "status": "approved",
        "plan_id": id,
    })))
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
    state
        .store
        .plan_update_status(id, target)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&json!({
        "plan_id": id,
        "new_status": target.as_str(),
    })))
}

async fn action_supersede(state: &AppState, args: &Value) -> Result<ToolResult> {
    let old_id = parse_id_arg(args, "old_plan_id")?;
    let new_id = parse_id_arg(args, "new_plan_id")?;
    state
        .store
        .plan_supersede(old_id, new_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&json!({
        "status": "superseded",
        "old_plan_id": old_id,
        "new_plan_id": new_id,
    })))
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
    let target = match args.get("target").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "execute requires `target` (mission_execution|mission_task_delegate|mission_flow_run)",
                )
                .with_suggestion(
                    "execute_mode=bridge default returns a next_call descriptor; \
                     execute_mode=internal dispatches the chosen target inside MissionD",
                ),
            ))
        }
    };

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

    let dispatch_strategy_raw = args
        .get("dispatch_strategy")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let dispatch_strategy = if VALID_DISPATCH_STRATEGIES.contains(&dispatch_strategy_raw) {
        dispatch_strategy_raw
    } else {
        // Don't reject: future strategies may be added in Lisp before code
        // catches up. Just normalise so evidence stays clean.
        "unknown"
    };

    if !matches!(target, "mission_execution" | "mission_task_delegate" | "mission_flow_run") {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!("execute target `{}` is not supported", target),
            )
            .with_suggestion(
                "supported targets: mission_execution | mission_task_delegate | mission_flow_run",
            ),
        ));
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

    if execute_mode == "bridge" {
        return Ok(action_execute_bridge(&plan, target, dispatch_strategy));
    }

    action_execute_internal(state, args, &plan, target, dispatch_strategy).await
}

fn action_execute_bridge(plan: &Plan, target: &str, dispatch_strategy: &str) -> ToolResult {
    let next_call = match target {
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
        "target_tool": target,
        "dispatch_strategy": dispatch_strategy,
        "next_call": next_call,
        "note": "manager returns the next-call descriptor; caller invokes the target tool directly. \
                 Pass execute_mode=\"internal\" to have MissionD dispatch the target inside the daemon.",
    }))
}

async fn action_execute_internal(
    state: &AppState,
    args: &Value,
    plan: &Plan,
    target: &str,
    dispatch_strategy: &str,
) -> Result<ToolResult> {
    let dry_run = args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);

    let inner_args = match build_internal_dispatch_args(args, plan, target) {
        Ok(v) => v,
        Err(err_result) => return Ok(err_result),
    };

    if dry_run {
        return Ok(ToolResult::json_pretty(&json!({
            "status": "dry_run",
            "execute_mode": "internal",
            "runner_status": "dry_run_no_dispatch",
            "plan_id": plan.id,
            "board_task_id": plan.board_task_id,
            "target_tool": target,
            "dispatch_strategy": dispatch_strategy,
            "would_dispatch": inner_args,
        })));
    }

    let inner_result = match target {
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
        return Ok(ToolResult::json_pretty(&json!({
            "status": "dispatch_failed",
            "execute_mode": "internal",
            "runner_status": "inner_returned_error",
            "plan_id": plan.id,
            "board_task_id": plan.board_task_id,
            "target_tool": target,
            "dispatch_strategy": dispatch_strategy,
            "inner_result": inner_payload,
        })));
    }

    // Successful dispatch — append evidence then transition plan to executing.
    let project_arg = args
        .get("target_project")
        .or_else(|| args.get("project"))
        .and_then(|v| v.as_str());
    let evidence_entry = json!({
        "kind": "plan_runner_dispatch",
        "execute_mode": "internal",
        "target_tool": target,
        "dispatch_strategy": dispatch_strategy,
        "inner_result": inner_payload.clone(),
    });
    let (evidence_path, evidence_error) =
        match append_plan_evidence_entry(state, plan.id, project_arg, evidence_entry).await {
            Ok((p, _count)) => (Some(p.display().to_string()), None),
            Err(e) => {
                // Evidence append failure does not abort the dispatch (the inner
                // tool already succeeded with its own durable side effects), but
                // we now surface the error in the response so callers cannot
                // mistake a missing sidecar for a clean run.
                tracing::warn!(plan_id = %plan.id, error = %e, "plan-runner: evidence sidecar append failed");
                (None, Some(e.to_string()))
            }
        };

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

    Ok(build_internal_dispatch_success_response(
        plan,
        target,
        dispatch_strategy,
        inner_payload,
        evidence_path,
        evidence_error,
        status_update_error,
    ))
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
    target: &str,
    dispatch_strategy: &str,
    inner_payload: Value,
    evidence_path: Option<String>,
    evidence_error: Option<String>,
    status_update_error: Option<String>,
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
        "target_tool": target,
        "dispatch_strategy": dispatch_strategy,
        "evidence_path": evidence_path,
        "inner_result": inner_payload,
    });
    if let Some(err) = evidence_error {
        payload["evidence_error"] = json!(err);
    }
    if let Some(err) = status_update_error {
        payload["status_update_error"] = json!(err);
    }
    ToolResult::json_pretty(&payload)
}

/// Build the argument JSON for the inner target handler. Returns
/// `Err(structured_error_result)` on caller-facing validation failures so the
/// outer handler can return them verbatim.
fn build_internal_dispatch_args(
    args: &Value,
    plan: &Plan,
    target: &str,
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
            });
            if let Some(p) = args.get("target_project").or_else(|| args.get("project")) {
                if let Some(s) = p.as_str() {
                    inner["project"] = json!(s);
                }
            }
            Ok(inner)
        }
        "mission_task_delegate" => {
            let objective_in = args
                .get("objective")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string());
            let objective = objective_in
                .unwrap_or_else(|| derive_objective_from_plan(plan, DERIVED_OBJECTIVE_MAX));

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
            // task_delegate accepts cwd as a path; target_project (registry id)
            // is not directly understood downstream, so we map only the path.
            if let Some(cwd) = args.get("cwd").and_then(|v| v.as_str()) {
                inner["cwd"] = json!(cwd);
            } else if let Some(tp) = args.get("target_project").and_then(|v| v.as_str()) {
                // Only forward as cwd if it looks like a filesystem path; bare
                // project ids cannot resolve in task_delegate. Heuristic: '/'
                // present.
                if tp.contains('/') {
                    inner["cwd"] = json!(tp);
                }
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
            let flow_id = match args.get("flow_id").and_then(|v| v.as_str()) {
                Some(s) if !s.is_empty() => s,
                _ => {
                    return Err(ToolResult::structured_error(
                        ToolError::new(
                            error_codes::MISSING_PARAM,
                            "execute_mode=internal target=mission_flow_run requires `flow_id`",
                        )
                        .with_suggestion(
                            "plan.sexp_text 自动编译为 flow YAML 仍是未来工作 \
                             (intent-flow.lisp :: workflow-distiller); 当前必须显式传入 flow_id",
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
fn tool_result_payload(result: &ToolResult) -> Value {
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
    let entry = json!({ "evidence": evidence });
    let (path, entry_count) = append_plan_evidence_entry(state, id, project_arg, entry).await?;

    Ok(ToolResult::json_pretty(&json!({
        "status": "recorded",
        "plan_id": id,
        "path": path.display().to_string(),
        "entry_count": entry_count,
    })))
}

/// Append a single evidence entry to
/// `<project_root>/.missiond/v2/plans/<plan_id>.evidence.json`.
///
/// `entry` is merged with a `recorded_at` timestamp. Returns the sidecar path
/// and the resulting total entry count for caller-facing reporting. Used by
/// both `record_evidence` (manual evidence) and the plan-runner internal
/// dispatch path (`plan_runner_dispatch` audit trail).
async fn append_plan_evidence_entry(
    state: &AppState,
    plan_id: uuid::Uuid,
    project_arg: Option<&str>,
    entry: Value,
) -> Result<(PathBuf, usize)> {
    let project_root = resolve_project_root(state, project_arg).await?;
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
// helpers
// ───────────────────────────────────────────────────────────────────────

fn parse_id_arg(args: &Value, key: &str) -> Result<uuid::Uuid> {
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

async fn resolve_project_root(
    state: &AppState,
    project_id: Option<&str>,
) -> Result<PathBuf> {
    if let Some(id) = project_id {
        if let Some(p) = state.project_registry.read().await.get(id) {
            return Ok(PathBuf::from(&p.path));
        }
        return Err(anyhow!(
            "project '{}' not registered; run mission_project(action=\"list\")",
            id
        ));
    }
    let cwd = std::env::current_dir().map_err(|e| anyhow!("cannot read CWD: {}", e))?;
    Ok(Path::new(&cwd).to_path_buf())
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

    #[test]
    fn bridge_response_includes_plan_runner_v0_fields() {
        let plan = fixture_plan("(plan)");
        let result = action_execute_bridge(&plan, "mission_execution", "fresh-code-alignment");
        assert!(result.is_error.is_none());
        let text = match result.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        let v: Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(v["execute_mode"], "bridge");
        assert_eq!(v["runner_status"], "bridge_only");
        assert_eq!(v["target_tool"], "mission_execution");
        assert_eq!(v["dispatch_strategy"], "fresh-code-alignment");
        assert_eq!(v["next_call"]["tool"], "mission_execution");
        assert_eq!(v["next_call"]["action"], "open");
    }

    #[test]
    fn build_internal_args_for_mission_execution_defaults() {
        let plan = fixture_plan("(plan)");
        let args = json!({});
        let inner = build_internal_dispatch_args(&args, &plan, "mission_execution")
            .expect("default args build");
        assert_eq!(inner["action"], "open");
        assert_eq!(inner["execution_id"], format!("plan-{}", plan.id));
        assert_eq!(inner["parent_design"], format!("plan/{}", plan.id));
        assert_eq!(inner["owner"], "plan-runner");
        assert!(inner["scope"]
            .as_str()
            .unwrap()
            .contains(&plan.board_task_id));
    }

    #[test]
    fn build_internal_args_for_task_delegate_derives_objective() {
        let plan = fixture_plan("(plan-draft :goal :align)\n");
        let args = json!({});
        let inner = build_internal_dispatch_args(&args, &plan, "mission_task_delegate")
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
    }

    #[test]
    fn build_internal_args_for_task_delegate_rejects_unknown_intent() {
        let plan = fixture_plan("(plan)");
        let args = json!({ "intent": "cosmic" });
        let err = build_internal_dispatch_args(&args, &plan, "mission_task_delegate")
            .expect_err("unknown intent should be rejected");
        assert_eq!(err.is_error, Some(true));
    }

    #[test]
    fn build_internal_args_for_flow_run_requires_flow_id() {
        let plan = fixture_plan("(plan)");
        let args = json!({});
        let err = build_internal_dispatch_args(&args, &plan, "mission_flow_run")
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
        let inner = build_internal_dispatch_args(&args, &plan, "mission_flow_run")
            .expect("flow_run with flow_id");
        assert_eq!(inner["action"], "run");
        assert_eq!(inner["flow_id"], "F-demo");
        assert_eq!(inner["params"]["k"], "v");
    }

    fn parse_payload(result: &ToolResult) -> Value {
        let text = match result.content.first() {
            Some(ToolContent::Text { text }) => text.clone(),
            _ => panic!("expected text content"),
        };
        serde_json::from_str(&text).expect("valid json")
    }

    #[test]
    fn success_response_clean_path_is_executing() {
        let plan = fixture_plan("(plan)");
        let result = build_internal_dispatch_success_response(
            &plan,
            "mission_execution",
            "fresh-code-alignment",
            json!({"ok": true}),
            Some("/tmp/sidecar.json".to_string()),
            None,
            None,
        );
        let v = parse_payload(&result);
        assert_eq!(v["status"], "executing");
        assert_eq!(v["runner_status"], "dispatched");
        assert_eq!(v["evidence_path"], "/tmp/sidecar.json");
        assert!(v.get("evidence_error").is_none());
        assert!(v.get("status_update_error").is_none());
        assert_eq!(v["target_tool"], "mission_execution");
        assert_eq!(v["dispatch_strategy"], "fresh-code-alignment");
        assert_eq!(v["inner_result"]["ok"], true);
    }

    #[test]
    fn success_response_evidence_failure_keeps_dispatched_but_exposes_error() {
        let plan = fixture_plan("(plan)");
        let result = build_internal_dispatch_success_response(
            &plan,
            "mission_task_delegate",
            "agent-team",
            json!({"task_id": "btk-9"}),
            None,
            Some("mkdir failed: read-only fs".to_string()),
            None,
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
        let result = build_internal_dispatch_success_response(
            &plan,
            "mission_execution",
            "resident-lisp",
            json!({"execution_id": "plan-x"}),
            Some("/tmp/sidecar.json".to_string()),
            None,
            Some("DB error: connection lost".to_string()),
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

    #[test]
    fn success_response_status_and_evidence_failure_combined() {
        let plan = fixture_plan("(plan)");
        let result = build_internal_dispatch_success_response(
            &plan,
            "mission_flow_run",
            "mixed",
            json!({"flow_id": "F-demo"}),
            None,
            Some("disk full".to_string()),
            Some("DB error: timeout".to_string()),
        );
        let v = parse_payload(&result);
        assert_eq!(v["status"], "dispatch_partial");
        assert_eq!(v["runner_status"], "status_update_failed");
        assert_eq!(v["evidence_error"], "disk full");
        assert_eq!(v["status_update_error"], "DB error: timeout");
        assert!(v["evidence_path"].is_null());
    }
}
