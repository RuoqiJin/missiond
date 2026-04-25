//! mission_workflow — manager surface for the workflow table.
//!
//! Lisp authority:
//!   - intent-memory.lisp :: module directive-layer :: plumbing workflow-templates
//!   - intent-memory.lisp :: module directive-layer :: file-first-artifacts :: workflow-artifact
//!   - intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s8 workflow-distillation
//!   - intent-flow.lisp :: F-directive-plan-workflow-compile :: workflow distill/match branch
//!   - intent-flow.lisp :: F-methodology-to-executable-compile (compile/run methodology)
//!   - intent-intent-layer.lisp :: section unified-entry-pipeline :: role workflow-distiller
//!   - intent-tools.lisp :: implemented-surface mission_workflow
//!
//! Action coverage:
//!   list                — full (workflow_list_top_n with explicit limit)
//!   get                 — full (workflow_get_by_name | workflow_get_by_id)
//!   match               — full (workflow_find_by_match)
//!   apply               — read-only: returns the matched template; never executes
//!   distill             — dual mode: `distill_mode="dry_run"` (default) preserves
//!                         legacy preview/draft behaviour; `distill_mode="sonnet"`
//!                         drives the workflow-distiller actor v0 (Sonnet over plan
//!                         + evidence sidecar → workflow_sexp + match_rules; persist
//!                         optional)
//!   record_execution    — full (workflow_record_execution)
//!   compile_methodology — dry-run: locates `.missiond/workflows/<name>.lisp`,
//!                         emits compile preview; YAML emitter actor pending
//!   run_methodology     — not_implemented: returns next-step pointer to
//!                         compile_methodology + mission_flow_run

use anyhow::{anyhow, Result};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::minimax_client::ChatMessage;
use crate::state::AppState;
use missiond_core::types::PlanStatus;

const DEFAULT_LIST_LIMIT: i64 = 50;
const MAX_LIST_LIMIT: i64 = 500;
const WORKFLOWS_DIR: &str = ".missiond/workflows";
const EVIDENCE_DIR: &str = ".missiond/v2/plans";
const SONNET_COMPILER_MODEL: &str = "claude-sonnet";
const DISTILLER_MAX_TOKENS: u32 = 2048;

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a.to_string(),
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "mission_workflow requires `action`",
                )
                .with_suggestion(
                    "actions: list|get|match|apply|distill|record_execution|compile_methodology|run_methodology",
                ),
            ))
        }
    };

    match action.as_str() {
        "list" => action_list(state, &args).await,
        "get" => action_get(state, &args).await,
        "match" => action_match(state, &args).await,
        "apply" => action_apply(state, &args).await,
        "distill" => action_distill(state, &args).await,
        "record_execution" => action_record_execution(state, &args).await,
        "compile_methodology" => action_compile_methodology(state, &args).await,
        "run_methodology" => action_run_methodology(state, &args).await,
        other => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("unknown mission_workflow action `{}`", other),
            )
            .with_suggestion(
                "valid: list|get|match|apply|distill|record_execution|compile_methodology|run_methodology",
            ),
        )),
    }
}

// ───────────────────────────────────────────────────────────────────────
// list / get / match — store-backed reads
// ───────────────────────────────────────────────────────────────────────

async fn action_list(state: &AppState, args: &Value) -> Result<ToolResult> {
    let limit = args
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT);
    let rows = state
        .store
        .workflow_list_top_n(limit)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&json!({
        "workflows": rows,
        "count": rows.len(),
        "limit": limit,
        "note": "ranked by executions desc, success_count desc, last_used_at desc",
    })))
}

async fn action_get(state: &AppState, args: &Value) -> Result<ToolResult> {
    let row = if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
        state
            .store
            .workflow_get_by_name(name)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
    } else if let Some(raw) = args.get("workflow_id").and_then(|v| v.as_str()) {
        let id = uuid::Uuid::parse_str(raw)
            .map_err(|e| anyhow!("workflow_id not UUID: {}", e))?;
        state
            .store
            .workflow_get_by_id(id)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
    } else {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::MISSING_PARAM,
                "get requires `name` or `workflow_id`",
            ),
        ));
    };
    match row {
        Some(w) => Ok(ToolResult::json_pretty(&w)),
        None => Ok(ToolResult::structured_error(
            ToolError::new(error_codes::NOT_FOUND, "workflow not found")
                .with_suggestion("use action=list or action=match"),
        )),
    }
}

async fn action_match(state: &AppState, args: &Value) -> Result<ToolResult> {
    let utterance = match args.get("utterance").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "match requires `utterance` (or `query`)",
                ),
            ))
        }
    };
    let rows = state
        .store
        .workflow_find_by_match(utterance)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&json!({
        "query": utterance,
        "matches": rows,
        "count": rows.len(),
        "note": "current matcher is substring over match_rules JSONB text; refine by parsing keys after actor lands",
    })))
}

// ───────────────────────────────────────────────────────────────────────
// apply — read-only candidate, no execution
// ───────────────────────────────────────────────────────────────────────

async fn action_apply(state: &AppState, args: &Value) -> Result<ToolResult> {
    let row = if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
        state
            .store
            .workflow_get_by_name(name)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
    } else if let Some(raw) = args.get("workflow_id").and_then(|v| v.as_str()) {
        let id = uuid::Uuid::parse_str(raw)
            .map_err(|e| anyhow!("workflow_id not UUID: {}", e))?;
        state
            .store
            .workflow_get_by_id(id)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
    } else {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::MISSING_PARAM,
                "apply requires `name` or `workflow_id`",
            ),
        ));
    };
    match row {
        Some(w) => Ok(ToolResult::json_pretty(&json!({
            "status": "candidate_returned",
            "workflow": w,
            "note": "apply returns the template. Execution requires action=run_methodology or mission_flow_run on a compiled YAML.",
        }))),
        None => Ok(ToolResult::structured_error(
            ToolError::new(error_codes::NOT_FOUND, "workflow not found"),
        )),
    }
}

// ───────────────────────────────────────────────────────────────────────
// distill — dry_run preview vs sonnet workflow-distiller actor v0
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DistillMode {
    DryRun,
    Sonnet,
}

fn parse_distill_mode(raw: Option<&str>) -> Result<DistillMode, String> {
    match raw {
        None | Some("") | Some("dry_run") => Ok(DistillMode::DryRun),
        Some("sonnet") => Ok(DistillMode::Sonnet),
        Some(other) => Err(format!(
            "distill_mode must be one of [\"dry_run\", \"sonnet\"]; got `{}`",
            other
        )),
    }
}

async fn action_distill(state: &AppState, args: &Value) -> Result<ToolResult> {
    let mode = match parse_distill_mode(args.get("distill_mode").and_then(|v| v.as_str())) {
        Ok(m) => m,
        Err(msg) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::INVALID_PARAM, msg),
            ))
        }
    };

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
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let plan = match plan {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::NOT_FOUND, format!("plan `{}` not found", plan_id)),
            ))
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

    match mode {
        DistillMode::DryRun => action_distill_dry_run(state, &plan, name, persist).await,
        DistillMode::Sonnet => action_distill_sonnet(state, args, &plan, name, persist).await,
    }
}

async fn action_distill_dry_run(
    state: &AppState,
    plan: &missiond_core::types::Plan,
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
            .map_err(|e| anyhow!("DB error: {}", e))?;
        payload["persisted"] = json!(true);
        payload["workflow_id"] = json!(id);
    } else {
        payload["persisted"] = json!(false);
    }
    Ok(ToolResult::json_pretty(&payload))
}

async fn action_distill_sonnet(
    state: &AppState,
    args: &Value,
    plan: &missiond_core::types::Plan,
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

    let project_root =
        resolve_project_root(state, args.get("project").and_then(|v| v.as_str())).await?;
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
                .with_suggestion("set ANTHROPIC_API_KEY / xjp-router credentials and restart daemon"),
            ))
        }
    };

    let prompt = build_distiller_prompt(plan, name, &match_hint, &evidence_value);
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: prompt,
    }];
    let raw_response = match sonnet
        .call_briefing(messages, Some(DISTILLER_MAX_TOKENS), Some(plan.id.to_string()))
        .await
    {
        Ok(s) => s,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::EXTERNAL_ERROR,
                    format!("Sonnet distiller call failed: {}", e),
                ),
            ))
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
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::EXTERNAL_ERROR,
                    "distiller response missing required string field `workflow_sexp`",
                ),
            ))
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
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::EXTERNAL_ERROR,
                    "distiller response missing required object field `match_rules`",
                ),
            ))
        }
    };
    if !match_rules.is_object() {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::EXTERNAL_ERROR,
                "distiller `match_rules` must be a JSON object",
            ),
        ));
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
            .map_err(|e| anyhow!("DB error: {}", e))?;
        payload["persisted"] = json!(true);
        payload["workflow_id"] = json!(id);
    } else {
        payload["persisted"] = json!(false);
    }
    Ok(ToolResult::json_pretty(&payload))
}

// ───────────────────────────────────────────────────────────────────────
// record_execution — full
// ───────────────────────────────────────────────────────────────────────

async fn action_record_execution(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "workflow_id")?;
    let success = args
        .get("success")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| anyhow!("`success` required (boolean)"))?;
    let cost_usd = args.get("cost_usd").and_then(|v| v.as_f64());
    state
        .store
        .workflow_record_execution(id, success, cost_usd)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&json!({
        "status": "recorded",
        "workflow_id": id,
        "success": success,
        "cost_usd": cost_usd,
    })))
}

// ───────────────────────────────────────────────────────────────────────
// compile_methodology — dry-run preview from .missiond/workflows/<name>.lisp
// ───────────────────────────────────────────────────────────────────────

async fn action_compile_methodology(state: &AppState, args: &Value) -> Result<ToolResult> {
    let project_root = resolve_project_root(state, args.get("project").and_then(|v| v.as_str())).await?;
    let workflows_dir = project_root.join(WORKFLOWS_DIR);

    let path: PathBuf = if let Some(p) = args.get("workflow_path").and_then(|v| v.as_str()) {
        let candidate = PathBuf::from(p);
        if candidate.is_absolute() {
            candidate
        } else {
            project_root.join(candidate)
        }
    } else if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
        let mut p = workflows_dir.join(name);
        if p.extension().is_none() {
            p.set_extension("lisp");
        }
        p
    } else {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::MISSING_PARAM,
                "compile_methodology requires `workflow_path` or `name`",
            ),
        ));
    };

    if !path.exists() {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::NOT_FOUND,
                format!("methodology lisp not found: {}", path.display()),
            )
            .with_suggestion(format!(
                "place it under {} and retry",
                workflows_dir.display()
            )),
        ));
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| anyhow!("read {}: {}", path.display(), e))?;
    let line_count = content.lines().count();
    let phases = count_top_form(&content, "phase");
    let steps = count_top_form(&content, "step");

    Ok(ToolResult::json_pretty(&json!({
        "status": "dry_run",
        "actor_pending": "intent-layer :: workflow compiler (Lisp → executable YAML)",
        "flow_ref": "F-methodology-to-executable-compile",
        "source_path": path.display().to_string(),
        "lines": line_count,
        "phase_form_count": phases,
        "step_form_count": steps,
        "next_step": "compiler emits $MISSIOND_HOME/flows/<name>.yaml; today execute via mission_flow_run on a hand-authored YAML",
    })))
}

// ───────────────────────────────────────────────────────────────────────
// run_methodology — not implemented; explicit pointer
// ───────────────────────────────────────────────────────────────────────

async fn action_run_methodology(_state: &AppState, args: &Value) -> Result<ToolResult> {
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let workflow_path = args.get("workflow_path").and_then(|v| v.as_str()).unwrap_or("");
    Ok(ToolResult::json_pretty(&json!({
        "status": "not_implemented",
        "actor_pending": "intent-layer :: workflow compiler + worker flow loader",
        "flow_ref": "F-methodology-to-executable-compile :: s5/s6",
        "name": name,
        "workflow_path": workflow_path,
        "next_step": [
            "1) action=compile_methodology to verify source_hash + lint",
            "2) hand-author or generate <name>.yaml under $MISSIOND_HOME/flows",
            "3) call mission_flow_run(action=run, flow_id=<name>, params=...)"
        ],
    })))
}

// ───────────────────────────────────────────────────────────────────────
// helpers — shared
// ───────────────────────────────────────────────────────────────────────

fn parse_id_arg(args: &Value, key: &str) -> Result<uuid::Uuid> {
    let raw = args
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("`{}` required", key))?;
    uuid::Uuid::parse_str(raw).map_err(|e| anyhow!("`{}` is not a UUID: {}", key, e))
}

fn count_top_form(content: &str, name: &str) -> usize {
    let pat = format!("({}", name);
    content
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with(&pat)
                && t.as_bytes()
                    .get(pat.len())
                    .map(|b| b.is_ascii_whitespace() || *b == b')')
                    .unwrap_or(false)
        })
        .count()
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

// ───────────────────────────────────────────────────────────────────────
// helpers — distiller pure functions (covered by tests)
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum EvidenceOutcome {
    Missing,
    ParseFailed { error: String },
    Present { value: Value, entry_count: usize },
}

fn evidence_sidecar_path(project_root: &Path, plan_id: uuid::Uuid) -> PathBuf {
    project_root
        .join(EVIDENCE_DIR)
        .join(format!("{}.evidence.json", plan_id))
}

fn read_evidence_sidecar(path: &Path) -> EvidenceOutcome {
    if !path.exists() {
        return EvidenceOutcome::Missing;
    }
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return EvidenceOutcome::ParseFailed { error: e.to_string() },
    };
    let value: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return EvidenceOutcome::ParseFailed { error: e.to_string() },
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
fn evidence_gate(
    present: bool,
    entry_count: usize,
    min_evidence: usize,
    allow_missing: bool,
) -> Option<String> {
    if allow_missing {
        return None;
    }
    if !present {
        return Some(
            "evidence sidecar not found and allow_missing_evidence=false".to_string(),
        );
    }
    if entry_count < min_evidence {
        return Some(format!(
            "evidence sidecar has {} entries, requires at least {}",
            entry_count, min_evidence
        ));
    }
    None
}

fn collect_match_hint(value: Option<&Value>) -> String {
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
fn extract_json_payload(content: &str) -> &str {
    let trimmed = content.trim();
    if !trimmed.starts_with("```") {
        return trimmed;
    }
    let after_open = match trimmed.find('\n') {
        Some(idx) => &trimmed[idx + 1..],
        None => return trimmed, // single-line fence, give up
    };
    match after_open.rfind("```") {
        Some(close_idx) => after_open[..close_idx].trim(),
        None => after_open.trim(),
    }
}

/// Validate the LLM-emitted workflow_sexp string. Returns Err with reason on
/// any failure; Ok(()) means the sexp is structurally usable.
fn validate_workflow_sexp(s: &str) -> Result<(), String> {
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
fn paren_balanced_ignoring_strings(s: &str) -> bool {
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

fn name_referenced(name: &str, sexp: &str, match_rules: &Value) -> bool {
    if name.is_empty() {
        return true;
    }
    if sexp.contains(name) {
        return true;
    }
    match_rules.to_string().contains(name)
}

fn build_distiller_prompt(
    plan: &missiond_core::types::Plan,
    name_hint: &str,
    match_hint: &str,
    evidence: &Value,
) -> String {
    let evidence_blob = if evidence.is_null() {
        "<none — caller passed allow_missing_evidence=true>".to_string()
    } else {
        serde_json::to_string_pretty(evidence)
            .unwrap_or_else(|_| "<unserializable>".to_string())
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

// ───────────────────────────────────────────────────────────────────────
// tests
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_top_form_matches_phase_and_step() {
        // count_top_form scans for forms at the start of trimmed lines —
        // matches the typical methodology Lisp layout (one form per line).
        let body = "\
(workflow demo
  (phase a
    (step s1)
    (step s2))
  (phase b
    (step s3)))
";
        assert_eq!(count_top_form(body, "phase"), 2);
        assert_eq!(count_top_form(body, "step"), 3);
        assert_eq!(count_top_form(body, "absent"), 0);
    }

    #[test]
    fn parse_id_arg_rejects_non_uuid() {
        let args = serde_json::json!({"plan_id": "not-a-uuid"});
        assert!(parse_id_arg(&args, "plan_id").is_err());
    }

    #[test]
    fn parse_id_arg_accepts_uuid() {
        let id = uuid::Uuid::new_v4();
        let args = serde_json::json!({"plan_id": id.to_string()});
        assert_eq!(parse_id_arg(&args, "plan_id").unwrap(), id);
    }

    #[test]
    fn parse_distill_mode_default_and_explicit() {
        // Backwards-compat: missing or empty → dry_run keeps legacy callers working.
        assert_eq!(parse_distill_mode(None), Ok(DistillMode::DryRun));
        assert_eq!(parse_distill_mode(Some("")), Ok(DistillMode::DryRun));
        assert_eq!(parse_distill_mode(Some("dry_run")), Ok(DistillMode::DryRun));
        assert_eq!(parse_distill_mode(Some("sonnet")), Ok(DistillMode::Sonnet));
        assert!(parse_distill_mode(Some("nope")).is_err());
    }

    #[test]
    fn extract_json_payload_passes_through_plain() {
        let raw = "{\"workflow_sexp\":\"(workflow x)\"}";
        assert_eq!(extract_json_payload(raw), raw);
    }

    #[test]
    fn extract_json_payload_strips_fenced_block() {
        let raw = "```json\n{\"a\":1}\n```";
        assert_eq!(extract_json_payload(raw), "{\"a\":1}");
        let raw2 = "```\n{\"b\":2}\n```";
        assert_eq!(extract_json_payload(raw2), "{\"b\":2}");
    }

    #[test]
    fn extract_json_payload_strips_fence_without_close() {
        // Some models forget the closing fence; we still surface the inner content.
        let raw = "```json\n{\"a\":1}";
        assert_eq!(extract_json_payload(raw), "{\"a\":1}");
    }

    #[test]
    fn distiller_response_parse_pass_and_fail() {
        let good = "{\"workflow_sexp\":\"(workflow demo)\",\"match_rules\":{\"tokens\":[\"demo\"]}}";
        let v: serde_json::Value = serde_json::from_str(extract_json_payload(good))
            .expect("good JSON parses");
        assert_eq!(
            v.get("workflow_sexp").and_then(|x| x.as_str()),
            Some("(workflow demo)")
        );
        assert!(v.get("match_rules").map(|m| m.is_object()).unwrap_or(false));

        let bad = "not a json blob";
        assert!(serde_json::from_str::<serde_json::Value>(extract_json_payload(bad)).is_err());
    }

    #[test]
    fn paren_balance_basic() {
        // Empty input is vacuously balanced — `validate_workflow_sexp` is the
        // gate that rejects empty / non-`(`-prefixed strings.
        assert!(paren_balanced_ignoring_strings(""));
        assert!(paren_balanced_ignoring_strings("()"));
        assert!(paren_balanced_ignoring_strings("(a (b c) (d (e)))"));
        assert!(!paren_balanced_ignoring_strings("(a (b)"));
        assert!(!paren_balanced_ignoring_strings(")("));
    }

    #[test]
    fn paren_balance_ignores_string_payload() {
        // The closing paren in the literal is inside a string and must be ignored.
        assert!(paren_balanced_ignoring_strings("(workflow :note \"closes ) here\")"));
        // Escaped quote inside string should not flip the in-string flag.
        assert!(paren_balanced_ignoring_strings(
            "(a \"esc \\\" still in str ) \" b)"
        ));
        // Unterminated string is invalid.
        assert!(!paren_balanced_ignoring_strings("(a \"unterminated"));
    }

    #[test]
    fn validate_workflow_sexp_rejects_empty_and_unbalanced() {
        assert!(validate_workflow_sexp("").is_err());
        assert!(validate_workflow_sexp("   ").is_err());
        assert!(validate_workflow_sexp("not-sexp").is_err());
        assert!(validate_workflow_sexp("(open").is_err());
        assert!(validate_workflow_sexp("(workflow demo)").is_ok());
    }

    #[test]
    fn match_rules_must_be_object() {
        let parsed: serde_json::Value =
            serde_json::from_str("{\"match_rules\":[\"oops\"]}").unwrap();
        assert!(!parsed.get("match_rules").map(|v| v.is_object()).unwrap_or(false));
        let parsed_ok: serde_json::Value =
            serde_json::from_str("{\"match_rules\":{\"tokens\":[]}}").unwrap();
        assert!(parsed_ok
            .get("match_rules")
            .map(|v| v.is_object())
            .unwrap_or(false));
    }

    #[test]
    fn evidence_gate_allows_missing_when_flag_set() {
        assert_eq!(evidence_gate(false, 0, 1, true), None);
        assert_eq!(evidence_gate(true, 0, 1, true), None);
        assert_eq!(evidence_gate(true, 5, 1, true), None);
    }

    #[test]
    fn evidence_gate_rejects_missing_or_short() {
        assert!(evidence_gate(false, 0, 1, false).is_some());
        assert!(evidence_gate(true, 0, 1, false).is_some());
        assert!(evidence_gate(true, 1, 2, false).is_some());
    }

    #[test]
    fn evidence_gate_passes_when_enough_entries() {
        assert_eq!(evidence_gate(true, 1, 1, false), None);
        assert_eq!(evidence_gate(true, 5, 3, false), None);
    }

    #[test]
    fn collect_match_hint_string_array_or_none() {
        assert_eq!(collect_match_hint(None), "");
        assert_eq!(collect_match_hint(Some(&serde_json::json!(""))), "");
        assert_eq!(collect_match_hint(Some(&serde_json::json!("alpha"))), "alpha");
        assert_eq!(
            collect_match_hint(Some(&serde_json::json!(["alpha", "beta", "", "gamma"]))),
            "alpha, beta, gamma"
        );
        // Non-string array elements are dropped.
        assert_eq!(
            collect_match_hint(Some(&serde_json::json!(["alpha", 42, "gamma"]))),
            "alpha, gamma"
        );
    }

    #[test]
    fn name_referenced_checks_sexp_and_rules() {
        let rules = serde_json::json!({"tokens": ["demo"]});
        assert!(name_referenced("", "(workflow x)", &rules));
        assert!(name_referenced("demo", "(workflow demo)", &serde_json::json!({})));
        assert!(name_referenced("demo", "(workflow x)", &rules));
        assert!(!name_referenced("absent", "(workflow x)", &serde_json::json!({})));
    }

    #[test]
    fn evidence_sidecar_path_is_under_v2_plans() {
        let id = uuid::Uuid::nil();
        let path = evidence_sidecar_path(Path::new("/tmp/proj"), id);
        let s = path.display().to_string();
        assert!(s.ends_with(&format!(".missiond/v2/plans/{}.evidence.json", id)));
    }
}
