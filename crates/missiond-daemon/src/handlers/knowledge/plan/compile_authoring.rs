use super::*;
use missiond_core::types::DirectiveStatus;

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

pub(super) async fn action_compile(state: &AppState, args: &Value) -> Result<ToolResult> {
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

mod artifact;
mod validation;

pub(super) use artifact::{extract_plan_file_args, maybe_write_plan_artifact};
pub(super) use validation::{
    build_planner_system_prompt, build_planner_user_prompt, collect_string_list,
    validate_compiled_plan_sexp, SexpValidationError,
};
#[cfg(test)]
pub(super) use validation::{parens_balanced, strip_fenced_code_block, top_level_head};

pub(super) struct DryRunPlanSexpInput<'a> {
    pub(super) directive_id: Option<&'a str>,
    pub(super) board_task_id: Option<&'a str>,
    pub(super) target: &'static str,
    pub(super) dispatch_strategy: Option<&'static str>,
    pub(super) target_project: Option<&'a str>,
    pub(super) requested_cwd: Option<&'a str>,
    pub(super) flow_id: Option<&'a str>,
    pub(super) objective: &'a str,
    pub(super) acceptance: Vec<String>,
    pub(super) constraints: Vec<String>,
}

fn arg_nonblank_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

pub(super) fn resolve_dry_run_plan_target(
    args: &Value,
) -> std::result::Result<&'static str, ToolResult> {
    let flow_id_present = arg_nonblank_str(args, "flow_id").is_some();
    let Some(raw) = arg_nonblank_str(args, "target") else {
        return Ok("mission_task_delegate");
    };
    normalize_target(raw, flow_id_present).ok_or_else(|| {
        ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!("compile dry_run target `{}` cannot be rendered as an executable PLAN.lisp hint", raw),
            )
            .with_suggestion(
                "supported dry-run plan targets: mission_execution | mission_task_delegate | mission_flow_run with flow_id",
            ),
        )
    })
}

pub(super) fn resolve_dry_run_dispatch_strategy(args: &Value) -> Option<&'static str> {
    arg_nonblank_str(args, "dispatch_strategy")
        .and_then(canonicalize_strategy)
        .or_else(|| arg_nonblank_str(args, "parallelism").and_then(canonicalize_strategy))
}

pub(super) fn derive_dry_run_plan_objective(
    args: &Value,
    directive_sexp: Option<&str>,
    board_task_id: Option<&str>,
) -> String {
    if let Some(o) = arg_nonblank_str(args, "objective") {
        return truncate_chars(o, DERIVED_OBJECTIVE_MAX);
    }
    if let Some(sexp) = directive_sexp {
        for wanted in ["objective", "goal", "utterance", "summary"] {
            if let Some((_, value)) = scan_keyword_pairs(sexp)
                .into_iter()
                .find(|(key, value)| key.eq_ignore_ascii_case(wanted) && !value.trim().is_empty())
            {
                let value = value.trim().trim_start_matches(':');
                if !value.trim().is_empty() {
                    return truncate_chars(value.trim(), DERIVED_OBJECTIVE_MAX);
                }
            }
        }
    }
    if let Some(task_id) = board_task_id.map(str::trim).filter(|s| !s.is_empty()) {
        return format!(
            "Execute MissionD request plan anchored to board_task {}",
            task_id
        );
    }
    "Execute MissionD request plan".to_string()
}

fn push_lisp_string_field(out: &mut String, key: &str, value: &str) {
    out.push_str("  :");
    out.push_str(key);
    out.push_str(" \"");
    out.push_str(&lisp_escape_string(value));
    out.push_str("\"\n");
}

pub(super) fn render_dry_run_plan_sexp(input: DryRunPlanSexpInput<'_>) -> String {
    let mut out = String::from("(plan-draft\n");
    push_lisp_string_field(&mut out, "directive_id", input.directive_id.unwrap_or(""));
    push_lisp_string_field(&mut out, "board_task_id", input.board_task_id.unwrap_or(""));
    out.push_str("  :status :awaiting-compiler-actor\n");
    out.push_str("  :execution-readiness :dry-run-executable-scaffold\n");
    push_lisp_string_field(&mut out, "target", input.target);
    if let Some(strategy) = input.dispatch_strategy {
        push_lisp_string_field(&mut out, "dispatch-strategy", strategy);
    }
    if let Some(tp) = input
        .target_project
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        push_lisp_string_field(&mut out, "target-project", tp);
    }
    if let Some(cwd) = input.requested_cwd.map(str::trim).filter(|s| !s.is_empty()) {
        push_lisp_string_field(&mut out, "requested-cwd", cwd);
    }
    if let Some(flow_id) = input.flow_id.map(str::trim).filter(|s| !s.is_empty()) {
        push_lisp_string_field(&mut out, "flow-id", flow_id);
    }
    push_lisp_string_field(&mut out, "objective", input.objective);
    if !input.acceptance.is_empty() {
        out.push_str("  :acceptance ");
        out.push_str(&render_lisp_string_list(&input.acceptance));
        out.push('\n');
    }
    if !input.constraints.is_empty() {
        out.push_str("  :constraints ");
        out.push_str(&render_lisp_string_list(&input.constraints));
        out.push('\n');
    }
    out.push_str("  :nodes\n");
    out.push_str("    [(:id \"root\"\n");
    out.push_str("      :target \"");
    out.push_str(&lisp_escape_string(input.target));
    out.push_str("\"\n");
    if let Some(strategy) = input.dispatch_strategy {
        out.push_str("      :dispatch-strategy \"");
        out.push_str(&lisp_escape_string(strategy));
        out.push_str("\"\n");
    }
    out.push_str("      :objective \"");
    out.push_str(&lisp_escape_string(input.objective));
    out.push_str("\")]\n");
    out.push_str(")\n");
    out
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
            .with_suggestion(
                "plan-compiler runs against an approved directive bound to a board_task",
            ),
        ));
    }

    let directive_uuid = match directive_id {
        Some(s) => {
            Some(uuid::Uuid::parse_str(s).map_err(|e| anyhow!("directive_id not UUID: {}", e))?)
        }
        None => None,
    };
    let directive_version_arg = args
        .get("directive_version")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);
    let directive = match directive_uuid {
        Some(id) => resolve_directive(state, id, directive_version_arg)
            .await
            .ok(),
        None => None,
    };

    let dry_run_target = match resolve_dry_run_plan_target(args) {
        Ok(t) => t,
        Err(err_result) => return Ok(err_result),
    };
    let dry_run_dispatch_strategy = resolve_dry_run_dispatch_strategy(args);
    let dry_run_objective = derive_dry_run_plan_objective(
        args,
        directive.as_ref().map(|d| d.sexp_text.as_str()),
        board_task_id,
    );
    let dry_run_sexp = render_dry_run_plan_sexp(DryRunPlanSexpInput {
        directive_id,
        board_task_id,
        target: dry_run_target,
        dispatch_strategy: dry_run_dispatch_strategy,
        target_project: args
            .get("target_project")
            .and_then(|v| v.as_str())
            .or_else(|| args.get("project").and_then(|v| v.as_str())),
        requested_cwd: args
            .get("requested_cwd")
            .and_then(|v| v.as_str())
            .or_else(|| args.get("cwd").and_then(|v| v.as_str())),
        flow_id: arg_nonblank_str(args, "flow_id"),
        objective: &dry_run_objective,
        acceptance: collect_string_list(args.get("acceptance")),
        constraints: collect_string_list(args.get("constraints")),
    });
    let sexp_hash = sha256_hex(&dry_run_sexp);

    let mut payload = json!({
        "status": "dry_run",
        "compiler_mode": COMPILER_MODE_DRY_RUN,
        "actor_pending": "intent-layer :: plan-compiler (LLM)",
        "flow_ref": "F-intent-alignment-plan-execution-loop :: s4 plan-authoring",
        "directive_id": directive_id,
        "board_task_id": board_task_id,
        "target": dry_run_target,
        "dispatch_strategy": dry_run_dispatch_strategy.unwrap_or("unknown"),
        "objective": dry_run_objective,
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
    let persist = args
        .get("persist")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let allow_unapproved = args
        .get("allow_unapproved")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let directive_id_str = args.get("directive_id").and_then(|v| v.as_str());
    let directive_uuid = match directive_id_str {
        Some(s) => {
            Some(uuid::Uuid::parse_str(s).map_err(|e| anyhow!("directive_id not UUID: {}", e))?)
        }
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
