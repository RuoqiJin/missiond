use super::*;

/// Cap derived objective at a manager-friendly length so we never push huge
/// sexp blobs into mission_task_delegate (which has its own 16K cap, but the
/// derived summary should be a *summary*, not the whole DAG).
pub(super) const DERIVED_OBJECTIVE_MAX: usize = 240;

/// Valid intents accepted by mission_task_delegate (kept in sync with that
/// handler's whitelist; we surface a structured error if caller picks something
/// else, instead of letting it through to be rejected downstream).
const VALID_DELEGATE_INTENTS: &[&str] = &["code", "ops", "research", "general"];

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
pub(in crate::handlers::knowledge) fn build_internal_dispatch_args(
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
                .or_else(|| plan.source_directive_id.map(|d| format!("directive/{}", d)))
                .unwrap_or_else(|| format!("plan/{}", plan.id));
            let scope = args
                .get("scope")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("plan {} (board_task {})", plan.id, plan.board_task_id));
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
            if dispatch_strategy == "agent-team" && !objective.contains(AGENT_TEAM_OBJECTIVE_HINT) {
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
            // Dedup linkage (BoardTask 31a99a30): the rendered-internal path
            // is the legacy resident/plan dispatch into mission_task_delegate.
            // Forward parent / source ids and write_scope / must_not_touch /
            // task_class so the dedup guard in mission_task_delegate refuses
            // a second concurrent code worker against the same plan when
            // their write_scope overlaps. Caller args win over plan hints —
            // mirrors the precedence the rest of this branch already uses.
            let parent_explicit = args
                .get("parent_board_task_id")
                .or_else(|| args.get("parentBoardTaskId"))
                .or_else(|| args.get("parent_id"))
                .or_else(|| args.get("parentId"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| plan.board_task_id.clone());
            let source_explicit = args
                .get("source_board_task_id")
                .or_else(|| args.get("sourceBoardTaskId"))
                .or_else(|| args.get("source_id"))
                .or_else(|| args.get("sourceId"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| plan.board_task_id.clone());
            inner["parent_board_task_id"] = json!(parent_explicit);
            inner["source_board_task_id"] = json!(source_explicit);
            // write_scope / must_not_touch precedence: explicit args > plan
            // hint owned/forbidden files. We only inject when at least one
            // source is non-empty so read-only delegations stay free of the
            // dedup guard (which short-circuits on empty write_scope).
            let arg_write_scope = collect_string_list_arg(
                args,
                &["write_scope", "writeScope", "owned_files", "ownedFiles"],
            );
            let effective_write_scope = if !arg_write_scope.is_empty() {
                arg_write_scope
            } else {
                split_lisp_string_list(hints.owned_files_raw.as_deref())
            };
            if !effective_write_scope.is_empty() {
                inner["write_scope"] = json!(effective_write_scope);
                // task_class default = "code" for code-intent dispatches
                // with declared write_scope. Lets the dedup guard short
                // circuit on context-pack / research delegations even
                // when the caller forwards the same intent string.
                if intent == "code" && args.get("task_class").is_none() {
                    inner["task_class"] = json!("code");
                }
            }
            let arg_must_not_touch = collect_string_list_arg(
                args,
                &[
                    "must_not_touch",
                    "mustNotTouch",
                    "forbidden_files",
                    "forbiddenFiles",
                ],
            );
            let effective_must_not_touch = if !arg_must_not_touch.is_empty() {
                arg_must_not_touch
            } else {
                split_lisp_string_list(hints.forbidden_files_raw.as_deref())
            };
            if !effective_must_not_touch.is_empty() {
                inner["must_not_touch"] = json!(effective_must_not_touch);
            }
            if let Some(tc) = args.get("task_class").and_then(|v| v.as_str()) {
                if !tc.trim().is_empty() {
                    inner["task_class"] = json!(tc);
                }
            }
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
pub(super) fn derive_objective_from_plan(plan: &Plan, max_chars: usize) -> String {
    let summary = plan
        .sexp_text
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("plan execution");
    let summary = truncate_chars(summary, max_chars);
    format!("Plan {}: {}", plan.id, summary)
}

pub(super) fn truncate_chars(s: &str, max_chars: usize) -> String {
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
pub(in crate::handlers::knowledge) fn tool_result_payload(result: &ToolResult) -> Value {
    match result.content.first() {
        Some(ToolContent::Text { text }) => {
            serde_json::from_str::<Value>(text).unwrap_or_else(|_| Value::String(text.clone()))
        }
        None => Value::Null,
    }
}

/// Pull a string-list argument from the caller-supplied JSON, accepting any
/// of `keys` as the source. Empty / whitespace entries drop out so the
/// downstream `mission_task_delegate` dedup guard sees a clean scope set.
pub(super) fn collect_string_list_arg(args: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| args.get(*key))
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::trim))
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}
