//! Distillation sub-surface for mission_workflow.
//!
//! Owns the dry-run preview, Sonnet workflow-distiller adapter, evidence
//! sidecar gate, workflow_sexp validation, and auto-chain handoff. The parent
//! `workflow.rs` remains the action facade.

use anyhow::Result;
use missiond_core::types::{Plan, PlanStatus};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::handlers::knowledge::review_gate::{
    apply_compile_review_gates, parse_compile_review_gate, parse_review_gate_policy,
    review_gate_policy_was_explicit,
};
use crate::minimax_client::ChatMessage;
use crate::state::AppState;

use super::artifacts::{
    extract_workflow_file_args, maybe_write_workflow_artifact, render_workflow_artifact_sexp,
};
use super::auto_chain::maybe_apply_distill_chain_layers;
use super::auto_sonnet::{parse_auto_sonnet_policy, validate_auto_sonnet_args};
use super::{parse_id_arg, resolve_project_root_from_args};

const EVIDENCE_DIR: &str = ".missiond/v2/plans";
const SONNET_COMPILER_MODEL: &str = "claude-sonnet";
const DISTILLER_MAX_TOKENS: u32 = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DistillMode {
    DryRun,
    Sonnet,
}

pub(super) fn parse_distill_mode(raw: Option<&str>) -> Result<DistillMode, String> {
    match raw {
        None | Some("") | Some("dry_run") => Ok(DistillMode::DryRun),
        Some("sonnet") => Ok(DistillMode::Sonnet),
        Some(other) => Err(format!(
            "distill_mode must be one of [\"dry_run\", \"sonnet\"]; got `{}`",
            other
        )),
    }
}

pub(super) async fn action_distill(state: &AppState, args: &Value) -> Result<ToolResult> {
    let mode = match parse_distill_mode(args.get("distill_mode").and_then(|v| v.as_str())) {
        Ok(m) => m,
        Err(msg) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                msg,
            )))
        }
    };

    // wave-21 / task 07 — pre-flight strict-shape validation for the
    // `auto_sonnet` apply-gate v1 knobs. Runs BEFORE the plan lookup so
    // a typo (`auto_sonnet="true"`, `auto_sonnet_approved=1`) surfaces
    // loud as INVALID_PARAM, never silently demotes to default-off.
    if let Err(msg) = validate_auto_sonnet_args(args) {
        return Ok(ToolResult::structured_error(
            ToolError::new(error_codes::INVALID_PARAM, msg).with_suggestion(
                "auto_sonnet must be a boolean (true|false); auto_sonnet_approved must be a boolean (true|false)",
            ),
        ));
    }

    // wave-22 / task 06 — pre-flight strict closed-enum validation for
    // the `auto_sonnet_policy` knob v2. Runs BEFORE the plan lookup so
    // unknown values / non-string shapes surface loud as INVALID_PARAM
    // (a single typo cannot escalate the daemon — I2 carryover).
    if let Err(msg) = parse_auto_sonnet_policy(args) {
        return Ok(ToolResult::structured_error(
            ToolError::new(error_codes::INVALID_PARAM, msg).with_suggestion(
                "auto_sonnet_policy valid values: \"off\" (default) | \"safe_after_rules\" | \"dry_run\"",
            ),
        ));
    }

    let plan_id = parse_id_arg(args, "plan_id")?;
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let persist = args
        .get("persist")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let plan = state
        .store
        .plan_get(plan_id)
        .await
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;
    let plan = match plan {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("plan `{}` not found", plan_id),
            )))
        }
    };
    if plan.status != PlanStatus::Succeeded {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!(
                    "distill expects plan status=succeeded; got `{}`",
                    plan.status.as_str()
                ),
            )
            .with_suggestion("mark the plan as succeeded after evidence is recorded, then re-run"),
        ));
    }

    let inner = match mode {
        DistillMode::DryRun => action_distill_dry_run(state, args, &plan, name, persist).await?,
        DistillMode::Sonnet => action_distill_sonnet(state, args, &plan, name, persist).await?,
    };

    // wave-19 / task 09 :: opt-in auto-chain hook. Default `auto_chain=false`
    // keeps the response byte-identical with the wave-18 distill surface so
    // existing callers (including plan.rs's apply_distill_chain forwarder)
    // see no shape change. When enabled, we DERIVE a deterministic chain id
    // from the workflow context (project root + plan id + workflow
    // name/id + evidence sha256) and append exactly one chain-record entry
    // to the plan's evidence sidecar (append-only — no migration). NEVER
    // calls Sonnet implicitly: the auto-chain runs AFTER the distill mode
    // the caller already opted into, and only records what already exists.
    //
    // wave-20 / task 06 :: ORTHOGONAL auto-trigger v1 layer. When the caller
    // sets `auto_chain_trigger="auto_safe"` (default `"never"`) we evaluate a
    // deterministic safety-rule set FIRST and only fall through to the
    // wave-19 recorder when ALL rules pass. Rules + final trigger status
    // are surfaced verbatim (`auto_trigger.{trigger_status,
    // safety_rule_results, sidecar, chain_id?}`) so audit consumers can
    // distinguish "explicit opt-in" from "rule-driven opt-in" without
    // re-deriving the policy. Rule failures NEVER partially append; Sonnet
    // is NEVER invoked implicitly — the trigger only enables the existing
    // record-only auto-chain hook.
    Ok(maybe_apply_distill_chain_layers(state, args, &plan, name, inner).await)
}

async fn action_distill_dry_run(
    state: &AppState,
    args: &Value,
    plan: &Plan,
    name: &str,
    persist: bool,
) -> Result<ToolResult> {
    let preview_sexp = format!(
        "(workflow-draft\n  :name {:?}\n  :learned_from-plan {:?}\n  :status :awaiting-distiller-actor)\n",
        name, plan.id
    );
    let mut payload = json!({
        "status": "dry_run",
        "distill_mode": "dry_run",
        "actor_pending": "intent-layer :: workflow-distiller (LLM)",
        "flow_ref": "F-intent-alignment-plan-execution-loop :: s8 workflow-distillation (dry_run preview) / F-directive-plan-workflow-compile :: workflow distill branch",
        "plan_id": plan.id,
        "name_hint": name,
        "compiled_sexp_preview": preview_sexp,
        "next_step": "pass distill_mode=\"sonnet\" to invoke the workflow-distiller actor; persist=true stores a draft template either way",
    });
    if persist {
        if name.is_empty() {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::MISSING_PARAM, "persist=true requires `name`")
                    .with_suggestion("workflow.name has UNIQUE constraint"),
            ));
        }
        let id = state
            .store
            .workflow_insert(name, &preview_sexp, &json!({}), Some(plan.id))
            .await
            .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;
        payload["persisted"] = json!(true);
        payload["workflow_id"] = json!(id);

        // wave-14/35 :: file-first SSOT mirror. Topic defaults to `name`
        // (the distill UNIQUE key) so the on-disk path matches the registry
        // entry without an extra arg. The file content is the enriched V3
        // workflow artifact, not a bare preview body. The DB row stays
        // committed even if the file write fails (file-vs-db contract).
        let file_args = extract_workflow_file_args(args);
        let topic_for_gate = file_args
            .topic
            .map(|s| s.to_string())
            .unwrap_or_else(|| name.to_string());
        let artifact_sexp = render_workflow_artifact_sexp(
            &id.to_string(),
            &[plan.id.to_string()],
            &json!({}),
            "draft",
            &preview_sexp,
        );
        maybe_write_workflow_artifact(state, &file_args, &mut payload, &artifact_sexp, name).await;

        // wave-14 :: review-gate auto-create. Default policy = Manual; the
        // workflow distill draft is rare enough that explicit-emit usually
        // wins, but `emit_question` lets a methodology pipeline opt in.
        let policy = parse_review_gate_policy(args);
        let policy_explicit = review_gate_policy_was_explicit(args);
        let legacy = parse_compile_review_gate(args);
        apply_compile_review_gates(
            &mut payload,
            &state.bus,
            policy,
            policy_explicit,
            &legacy,
            "workflow",
            &id.to_string(),
            1,
            Some(&topic_for_gate),
        )
        .await;
    } else {
        payload["persisted"] = json!(false);
    }
    Ok(ToolResult::json_pretty(&payload))
}

pub(super) async fn action_distill_sonnet(
    state: &AppState,
    args: &Value,
    plan: &Plan,
    name: &str,
    persist: bool,
) -> Result<ToolResult> {
    let allow_missing_evidence = args
        .get("allow_missing_evidence")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let min_evidence = args
        .get("min_evidence")
        .and_then(|v| v.as_i64())
        .map(|n| n.max(0) as usize)
        .unwrap_or(1);
    let match_hint = collect_match_hint(args.get("match_hint"));

    if persist && name.is_empty() {
        return Ok(ToolResult::structured_error(
            ToolError::new(error_codes::MISSING_PARAM, "persist=true requires `name`")
                .with_suggestion("workflow.name has UNIQUE constraint"),
        ));
    }

    let project_root = match resolve_project_root_from_args(state, args).await {
        Ok(p) => p,
        Err(reason) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::INVALID_PARAM, reason).with_suggestion(
                    "supply `project` (registered id) or absolute `cwd` so the distiller \
                     can locate the evidence sidecar; relative cwd is refused.",
                ),
            ))
        }
    };
    let evidence_path = evidence_sidecar_path(&project_root, plan.id);

    let evidence_outcome = read_evidence_sidecar(&evidence_path);
    let (evidence_value, evidence_entry_count) = match &evidence_outcome {
        EvidenceOutcome::Missing => {
            if let Some(msg) = evidence_gate(false, 0, min_evidence, allow_missing_evidence) {
                return Ok(ToolResult::structured_error(
                    ToolError::new(error_codes::NOT_FOUND, msg).with_suggestion(format!(
                        "missing sidecar: {} — record evidence via mission_plan(action=record_evidence) or pass allow_missing_evidence=true",
                        evidence_path.display()
                    )),
                ));
            }
            (Value::Null, 0usize)
        }
        EvidenceOutcome::ParseFailed { error } => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!(
                        "evidence sidecar at {} is not valid JSON: {}",
                        evidence_path.display(),
                        error
                    ),
                )
                .with_suggestion("inspect/repair the .evidence.json file before retrying"),
            ));
        }
        EvidenceOutcome::Present { value, entry_count } => {
            if let Some(msg) =
                evidence_gate(true, *entry_count, min_evidence, allow_missing_evidence)
            {
                return Ok(ToolResult::structured_error(
                    ToolError::new(error_codes::INVALID_PARAM, msg).with_suggestion(format!(
                        "sidecar {} has {} entries; require >= {} (or pass allow_missing_evidence=true)",
                        evidence_path.display(),
                        entry_count,
                        min_evidence
                    )),
                ));
            }
            (value.clone(), *entry_count)
        }
    };

    let sonnet = match state.sonnet.as_ref() {
        Some(s) => s,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::EXTERNAL_ERROR,
                    "Sonnet gateway not available; cannot run distiller actor",
                )
                .with_suggestion(
                    "set ANTHROPIC_API_KEY / xjp-router credentials and restart daemon",
                ),
            ))
        }
    };

    let prompt = build_distiller_prompt(plan, name, &match_hint, &evidence_value);
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: prompt,
    }];
    let raw_response = match sonnet
        .call_briefing(
            messages,
            Some(DISTILLER_MAX_TOKENS),
            Some(plan.id.to_string()),
        )
        .await
    {
        Ok(s) => s,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::EXTERNAL_ERROR,
                format!("Sonnet distiller call failed: {}", e),
            )))
        }
    };

    let json_slice = extract_json_payload(&raw_response);
    let parsed: Value = match serde_json::from_str(json_slice) {
        Ok(v) => v,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::EXTERNAL_ERROR,
                    format!("distiller response is not valid JSON: {}", e),
                )
                .with_suggestion(
                    "model returned non-JSON; tighten prompt or rerun. Raw response retained in daemon logs.",
                ),
            ))
        }
    };

    let workflow_sexp = match parsed.get("workflow_sexp").and_then(|v| v.as_str()) {
        Some(s) => s.trim().to_string(),
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::EXTERNAL_ERROR,
                "distiller response missing required string field `workflow_sexp`",
            )))
        }
    };
    if let Err(msg) = validate_workflow_sexp(&workflow_sexp) {
        return Ok(ToolResult::structured_error(
            ToolError::new(error_codes::EXTERNAL_ERROR, msg)
                .with_suggestion("rerun distiller; ensure model emits balanced sexp"),
        ));
    }

    let mut match_rules = match parsed.get("match_rules").cloned() {
        Some(v) => v,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::EXTERNAL_ERROR,
                "distiller response missing required object field `match_rules`",
            )))
        }
    };
    if !match_rules.is_object() {
        return Ok(ToolResult::structured_error(ToolError::new(
            error_codes::EXTERNAL_ERROR,
            "distiller `match_rules` must be a JSON object",
        )));
    }
    if let Some(protected) = args.get("protected").and_then(|v| v.as_bool()) {
        if let Some(obj) = match_rules.as_object_mut() {
            obj.insert("protected".to_string(), json!(protected));
        }
    }

    let mut warnings: Vec<String> = Vec::new();
    if !name.is_empty() && !name_referenced(name, &workflow_sexp, &match_rules) {
        warnings.push(format!(
            "name `{}` not found in workflow_sexp or match_rules — review before persisting",
            name
        ));
    }

    let summary = parsed.get("summary").and_then(|v| v.as_str()).unwrap_or("");
    let reusability_score = parsed.get("reusability_score").and_then(|v| v.as_f64());

    let mut payload = json!({
        "status": "distilled",
        "distill_mode": "sonnet",
        "compiler_model": SONNET_COMPILER_MODEL,
        "flow_ref": "F-intent-alignment-plan-execution-loop :: s8 workflow-distillation (sonnet actor v0) / F-directive-plan-workflow-compile :: workflow distill branch",
        "plan_id": plan.id,
        "name_hint": name,
        "evidence_path": evidence_path.display().to_string(),
        "evidence_entry_count": evidence_entry_count,
        "workflow_sexp": workflow_sexp,
        "match_rules": match_rules,
        "summary": summary,
        "reusability_score": reusability_score,
        "review_required": true,
    });
    if !warnings.is_empty() {
        payload["warnings"] = json!(warnings);
    }

    if persist {
        let id = state
            .store
            .workflow_insert(name, &workflow_sexp, &match_rules, Some(plan.id))
            .await
            .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;
        payload["persisted"] = json!(true);
        payload["workflow_id"] = json!(id);

        // wave-14/35 :: file-first SSOT mirror — same partial semantics as
        // the dry_run path. The distilled workflow_sexp is wrapped in the
        // enriched V3 workflow artifact so the on-disk Lisp carries the row
        // ref, source plan, match rules, extracted steps, and status.
        let file_args = extract_workflow_file_args(args);
        let topic_for_gate = file_args
            .topic
            .map(|s| s.to_string())
            .unwrap_or_else(|| name.to_string());
        let artifact_sexp = render_workflow_artifact_sexp(
            &id.to_string(),
            &[plan.id.to_string()],
            &match_rules,
            "distilled",
            &workflow_sexp,
        );
        maybe_write_workflow_artifact(state, &file_args, &mut payload, &artifact_sexp, name).await;

        // wave-14 :: review-gate auto-create. Same policy semantics as the
        // dry_run branch above.
        let policy = parse_review_gate_policy(args);
        let policy_explicit = review_gate_policy_was_explicit(args);
        let legacy = parse_compile_review_gate(args);
        apply_compile_review_gates(
            &mut payload,
            &state.bus,
            policy,
            policy_explicit,
            &legacy,
            "workflow",
            &id.to_string(),
            1,
            Some(&topic_for_gate),
        )
        .await;
    } else {
        payload["persisted"] = json!(false);
    }
    Ok(ToolResult::json_pretty(&payload))
}

#[derive(Debug)]
pub(super) enum EvidenceOutcome {
    Missing,
    ParseFailed { error: String },
    Present { value: Value, entry_count: usize },
}

pub(super) fn evidence_sidecar_path(project_root: &Path, plan_id: uuid::Uuid) -> PathBuf {
    project_root
        .join(EVIDENCE_DIR)
        .join(format!("{}.evidence.json", plan_id))
}

pub(super) fn read_evidence_sidecar(path: &Path) -> EvidenceOutcome {
    if !path.exists() {
        return EvidenceOutcome::Missing;
    }
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return EvidenceOutcome::ParseFailed {
                error: e.to_string(),
            }
        }
    };
    let value: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            return EvidenceOutcome::ParseFailed {
                error: e.to_string(),
            }
        }
    };
    let entry_count = value
        .get("entries")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    EvidenceOutcome::Present { value, entry_count }
}

/// Decide whether to fail the distill request based on sidecar presence and
/// entry count. Returns `Some(reason)` when the gate should reject; `None`
/// means continue. Pure-fn for unit testing.
pub(super) fn evidence_gate(
    present: bool,
    entry_count: usize,
    min_evidence: usize,
    allow_missing: bool,
) -> Option<String> {
    if allow_missing {
        return None;
    }
    if !present {
        return Some("evidence sidecar not found and allow_missing_evidence=false".to_string());
    }
    if entry_count < min_evidence {
        return Some(format!(
            "evidence sidecar has {} entries, requires at least {}",
            entry_count, min_evidence
        ));
    }
    None
}

pub(super) fn collect_match_hint(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}

/// Strip a fenced code block (``` or ```json) from the start/end of an LLM
/// response, returning the inner JSON-bearing slice. Pure-fn for testing.
pub(super) fn extract_json_payload(content: &str) -> &str {
    let trimmed = content.trim();
    if !trimmed.starts_with("```") {
        return trimmed;
    }
    let after_open = match trimmed.find('\n') {
        Some(idx) => &trimmed[idx + 1..],
        None => return trimmed,
    };
    match after_open.rfind("```") {
        Some(close_idx) => after_open[..close_idx].trim(),
        None => after_open.trim(),
    }
}

/// Validate the LLM-emitted workflow_sexp string. Returns Err with reason on
/// any failure; Ok(()) means the sexp is structurally usable.
pub(super) fn validate_workflow_sexp(s: &str) -> Result<(), String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err("workflow_sexp is empty".to_string());
    }
    if !trimmed.starts_with('(') {
        return Err("workflow_sexp must start with `(`".to_string());
    }
    if !paren_balanced_ignoring_strings(trimmed) {
        return Err("workflow_sexp parens are unbalanced".to_string());
    }
    Ok(())
}

/// Check that `(` and `)` balance, treating characters inside double-quoted
/// strings (with `\\` escape) as opaque. Returns false on under-flow or
/// non-zero terminal depth. Pure-fn for testing.
pub(super) fn paren_balanced_ignoring_strings(s: &str) -> bool {
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escaped = false;
    for ch in s.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
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

pub(super) fn name_referenced(name: &str, sexp: &str, match_rules: &Value) -> bool {
    if name.is_empty() {
        return true;
    }
    if sexp.contains(name) {
        return true;
    }
    match_rules.to_string().contains(name)
}

pub(super) fn build_distiller_prompt(
    plan: &Plan,
    name_hint: &str,
    match_hint: &str,
    evidence: &Value,
) -> String {
    let evidence_blob = if evidence.is_null() {
        "<none — caller passed allow_missing_evidence=true>".to_string()
    } else {
        serde_json::to_string_pretty(evidence).unwrap_or_else(|_| "<unserializable>".to_string())
    };
    let name_line = if name_hint.is_empty() {
        "<none>".to_string()
    } else {
        name_hint.to_string()
    };
    let match_hint_line = if match_hint.is_empty() {
        "<none>".to_string()
    } else {
        match_hint.to_string()
    };
    format!(
        "你是 MissionD workflow-distiller actor (intent-flow.lisp F-intent-alignment-plan-execution-loop :: s8 workflow-distillation).\n\
任务: 从一次 succeeded plan 及其 evidence sidecar 蒸馏出可复用的 workflow 模板.\n\
\n\
输入:\n\
- plan_id: {plan_id}\n\
- board_task_id: {board_task_id}\n\
- plan.sexp_text:\n```lisp\n{plan_sexp}\n```\n\
- name hint: {name_line}\n\
- match hint: {match_hint_line}\n\
- evidence sidecar (JSON):\n```json\n{evidence}\n```\n\
\n\
要求:\n\
1. 只输出严格 JSON, 不要 markdown 代码围栏, 不要其他文字.\n\
2. workflow_sexp: 把 plan 中 task-specific 常量替换为占位符 (例如 :target-file ?path), 保留可复用骨架, 括号平衡.\n\
3. match_rules: 必须是 JSON object, 推荐键: tokens / intents / tools / flows.\n\
4. summary: 一句话描述何时复用此 workflow.\n\
5. reusability_score: 0.0-1.0 浮点, 反映可复用度.\n\
\n\
输出 JSON 形如:\n\
{{\n  \"workflow_sexp\": \"(workflow ...)\",\n  \"match_rules\": {{\"tokens\": [], \"intents\": [], \"tools\": [], \"flows\": []}},\n  \"summary\": \"...\",\n  \"reusability_score\": 0.0\n}}\n",
        plan_id = plan.id,
        board_task_id = plan.board_task_id,
        plan_sexp = plan.sexp_text,
        name_line = name_line,
        match_hint_line = match_hint_line,
        evidence = evidence_blob,
    )
}
