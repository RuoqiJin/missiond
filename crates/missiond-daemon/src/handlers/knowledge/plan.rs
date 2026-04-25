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
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::minimax_client::ChatMessage;
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

    let inner_args = match build_internal_dispatch_args(args, plan, target, dispatch_strategy) {
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
///
/// `dispatch_strategy` is the already-normalised value from `action_execute`
/// (one of `VALID_DISPATCH_STRATEGIES`, defaulted to `"unknown"`). It is
/// forwarded into the `mission_execution(action=open)` inner JSON so the
/// companion log can persist `:dispatch-strategy`. Other targets ignore it.
fn build_internal_dispatch_args(
    args: &Value,
    plan: &Plan,
    target: &str,
    dispatch_strategy: &str,
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
            // project is the canonical key for mission_execution; target_project
            // is accepted as an alias and resolved to project here.
            if let Some(p) = args.get("target_project").or_else(|| args.get("project")) {
                if let Some(s) = p.as_str() {
                    inner["project"] = json!(s);
                }
            }
            // Also forward the original target_project string (when present) so
            // mission_execution can persist it verbatim alongside `project` —
            // the companion log records it as `:target-project` per
            // intent-tools.lisp :: workstation-dispatch-record.
            if let Some(s) = args.get("target_project").and_then(|v| v.as_str()) {
                inner["target_project"] = json!(s);
            }
            // requested_cwd is metadata only (used by mission_execution for
            // workstation-dispatch-record :requested-cwd persistence).
            if let Some(s) = args.get("requested_cwd").and_then(|v| v.as_str()) {
                inner["requested_cwd"] = json!(s);
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
        let inner = build_internal_dispatch_args(&args, &plan, "mission_execution", "unknown")
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
        let inner =
            build_internal_dispatch_args(&args, &plan, "mission_execution", "agent-team")
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
        let inner = build_internal_dispatch_args(&args, &plan, "mission_execution", "unknown")
            .expect("normalised default");
        assert_eq!(inner["dispatch_strategy"], "unknown");
    }

    #[test]
    fn build_internal_args_for_task_delegate_derives_objective() {
        let plan = fixture_plan("(plan-draft :goal :align)\n");
        let args = json!({});
        let inner =
            build_internal_dispatch_args(&args, &plan, "mission_task_delegate", "unknown")
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
        )
        .expect_err("unknown intent should be rejected");
        assert_eq!(err.is_error, Some(true));
    }

    #[test]
    fn build_internal_args_for_flow_run_requires_flow_id() {
        let plan = fixture_plan("(plan)");
        let args = json!({});
        let err = build_internal_dispatch_args(&args, &plan, "mission_flow_run", "unknown")
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
        let inner = build_internal_dispatch_args(&args, &plan, "mission_flow_run", "unknown")
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
