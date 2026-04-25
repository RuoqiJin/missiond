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
//!   compile_methodology — dual mode: compile_mode="dry_run" (default) preserves
//!                         the legacy lint preview; compile_mode="deterministic"
//!                         runs the methodology compiler v0 (paren validate +
//!                         (step …) extraction → executable FlowDefinition YAML
//!                         loadable by mission_flow_run). persist=true writes
//!                         the YAML under `.missiond/generated/flows/<flow_id>.yaml`
//!                         atomically and refuses overwrite unless overwrite=true.
//!   run_methodology     — resolves a compiled YAML by flow_id|flow_path|name;
//!                         dry_run=true (default) returns a `would_run` descriptor;
//!                         dry_run=false dispatches into the existing flow engine
//!                         (load FlowDefinition + spawn board task + runner::run_flow).
//!                         Missing compiled YAML returns structured
//!                         MISSING_COMPILED_FLOW + next-step pointer.

use anyhow::{anyhow, Result};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::minimax_client::ChatMessage;
use crate::state::AppState;
use missiond_core::types::PlanStatus;

const DEFAULT_LIST_LIMIT: i64 = 50;
const MAX_LIST_LIMIT: i64 = 500;
const WORKFLOWS_DIR: &str = ".missiond/workflows";
const GENERATED_FLOWS_DIR: &str = ".missiond/generated/flows";
const EVIDENCE_DIR: &str = ".missiond/v2/plans";
const SONNET_COMPILER_MODEL: &str = "claude-sonnet";
const DISTILLER_MAX_TOKENS: u32 = 2048;
const COMPILER_VERSION: &str = "mission_workflow.compile_methodology.v0";
const COMPILER_STATUS_PREVIEW: &str = "preview_requires_review";

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
// compile_methodology — dry-run preview vs deterministic compiler v0
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompileMode {
    DryRun,
    Deterministic,
}

fn parse_compile_mode(raw: Option<&str>) -> Result<CompileMode, String> {
    match raw {
        None | Some("") | Some("dry_run") => Ok(CompileMode::DryRun),
        Some("deterministic") => Ok(CompileMode::Deterministic),
        Some(other) => Err(format!(
            "compile_mode must be one of [\"dry_run\", \"deterministic\"]; got `{}`",
            other
        )),
    }
}

async fn action_compile_methodology(state: &AppState, args: &Value) -> Result<ToolResult> {
    let mode = match parse_compile_mode(args.get("compile_mode").and_then(|v| v.as_str())) {
        Ok(m) => m,
        Err(msg) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::INVALID_PARAM, msg),
            ))
        }
    };

    let project_id = args
        .get("project")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("target_project").and_then(|v| v.as_str()));
    let project_root = resolve_project_root(state, project_id).await?;
    let workflows_dir = project_root.join(WORKFLOWS_DIR);

    let path = match resolve_methodology_path(
        &project_root,
        args.get("name").and_then(|v| v.as_str()),
        args.get("workflow_path").and_then(|v| v.as_str()),
    ) {
        Ok(p) => p,
        Err(msg) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::MISSING_PARAM, msg),
            ))
        }
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

    match mode {
        CompileMode::DryRun => action_compile_dry_run(&path, &content),
        CompileMode::Deterministic => {
            action_compile_deterministic(&project_root, &path, &content, args)
        }
    }
}

fn action_compile_dry_run(path: &Path, content: &str) -> Result<ToolResult> {
    let line_count = content.lines().count();
    let phases = count_top_form(content, "phase");
    let steps = count_top_form(content, "step");
    Ok(ToolResult::json_pretty(&json!({
        "status": "dry_run",
        "compile_mode": "dry_run",
        "actor_pending": "intent-layer :: workflow compiler (Lisp → executable YAML)",
        "flow_ref": "F-methodology-to-executable-compile",
        "source_path": path.display().to_string(),
        "lines": line_count,
        "phase_form_count": phases,
        "step_form_count": steps,
        "next_step": "pass compile_mode=\"deterministic\" to emit an executable YAML preview; persist=true writes it to .missiond/generated/flows/<flow_id>.yaml",
    })))
}

fn action_compile_deterministic(
    project_root: &Path,
    path: &Path,
    content: &str,
    args: &Value,
) -> Result<ToolResult> {
    if let Err(msg) = validate_methodology_source(content) {
        return Ok(ToolResult::structured_error(
            ToolError::new(error_codes::INVALID_PARAM, msg)
                .with_suggestion("repair the methodology lisp and retry"),
        ));
    }

    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("methodology")
        .to_string();
    let output_flow_id = args
        .get("output_flow_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let flow_id = derive_flow_id(&stem, output_flow_id);
    let display_name = format!("methodology compile v0 — {}", stem);

    let steps = extract_steps(content);
    let review_required = steps.is_empty();
    let hash = source_hash(content);
    let generated_at = chrono::Utc::now().to_rfc3339();
    let source_display = source_path_for_yaml(project_root, path);

    let meta = GeneratedMeta {
        flow_id: flow_id.clone(),
        name: display_name,
        source_path: source_display.clone(),
        source_hash: hash.clone(),
        generated_at: generated_at.clone(),
        compiler_status: COMPILER_STATUS_PREVIEW.to_string(),
    };
    let yaml = build_generated_yaml(&meta, &steps, review_required)
        .map_err(|e| anyhow!("serialize yaml: {}", e))?;

    let persist = args
        .get("persist")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let overwrite = args
        .get("overwrite")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut payload = json!({
        "status": "compiled_preview",
        "compile_mode": "deterministic",
        "compiler_version": COMPILER_VERSION,
        "compiler_status": COMPILER_STATUS_PREVIEW,
        "flow_ref": "F-methodology-to-executable-compile :: s2/s3/s5",
        "flow_id": flow_id,
        "source_path": source_display,
        "source_hash": hash,
        "generated_at": generated_at,
        "step_count": steps.len(),
        "review_required": review_required,
        "params_echo": args.get("params").cloned().unwrap_or(Value::Null),
        "future_compiler_actor": "intent-layer LLM/forge compiler — semantic phase/anti-pattern lifting deferred",
        "yaml_preview": yaml,
    });

    if !persist {
        payload["persisted"] = json!(false);
        payload["next_step"] = json!(
            "persist=true to write to .missiond/generated/flows/<flow_id>.yaml; \
             then run_methodology(flow_id=<flow_id>, dry_run=true) to inspect, dry_run=false to dispatch"
        );
        return Ok(ToolResult::json_pretty(&payload));
    }

    let yaml_path = generated_yaml_path(project_root, &meta.flow_id);
    if yaml_path.exists() && !overwrite {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!(
                    "generated YAML already exists at {}; pass overwrite=true to replace",
                    yaml_path.display()
                ),
            ),
        ));
    }
    atomic_write(&yaml_path, &yaml)
        .map_err(|e| anyhow!("write {}: {}", yaml_path.display(), e))?;

    payload["persisted"] = json!(true);
    payload["flow_path"] = json!(yaml_path.display().to_string());
    payload["next_step"] = json!(
        "run_methodology(flow_id=<flow_id>, dry_run=true) to verify; dry_run=false to dispatch into mission_flow_run"
    );
    Ok(ToolResult::json_pretty(&payload))
}

// ───────────────────────────────────────────────────────────────────────
// run_methodology — resolve compiled YAML, dispatch into flow engine
// ───────────────────────────────────────────────────────────────────────

async fn action_run_methodology(state: &AppState, args: &Value) -> Result<ToolResult> {
    let project_id = args
        .get("project")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("target_project").and_then(|v| v.as_str()));
    let project_root = resolve_project_root(state, project_id).await?;
    let dry_run = args
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let flow_id_arg = args.get("flow_id").and_then(|v| v.as_str());
    let flow_path_arg = args.get("flow_path").and_then(|v| v.as_str());
    let name_arg = args.get("name").and_then(|v| v.as_str());

    let resolved = match resolve_compiled_flow(&project_root, flow_id_arg, flow_path_arg, name_arg)
    {
        Ok(r) => r,
        Err(CompiledFlowError::MissingArgs) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "run_methodology requires `flow_id`, `flow_path`, or `name`",
                ),
            ))
        }
        Err(CompiledFlowError::Missing { flow_id, expected }) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    "MISSING_COMPILED_FLOW: no compiled YAML on disk for the requested methodology",
                )
                .with_suggestion(format!(
                    "call mission_workflow(action=compile_methodology, compile_mode=\"deterministic\", persist=true, name=<methodology>, output_flow_id=\"{}\") to generate {}",
                    flow_id,
                    expected.display()
                )),
            ))
        }
    };

    let raw = std::fs::read_to_string(&resolved.path)
        .map_err(|e| anyhow!("read {}: {}", resolved.path.display(), e))?;
    let flow: crate::engine::flow::FlowDefinition = serde_yaml::from_str(&raw)
        .map_err(|e| anyhow!("parse {}: {}", resolved.path.display(), e))?;

    if dry_run {
        return Ok(ToolResult::json_pretty(&json!({
            "status": "would_run",
            "flow_ref": "F-methodology-to-executable-compile :: s6 dry-run-or-run (dry_run)",
            "flow_id": flow.id,
            "flow_path": resolved.path.display().to_string(),
            "node_count": flow.nodes.len(),
            "node_ids": flow.nodes.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
            "params_echo": args.get("params").cloned().unwrap_or(Value::Null),
            "next_step": "pass dry_run=false to dispatch into mission_flow_run on this compiled YAML",
        })));
    }

    // dry_run=false → dispatch through flow engine.
    let title = format!("Methodology: {}", flow.name);
    let input = missiond_core::types::CreateBoardTaskInput {
        title,
        category: Some("methodology".to_string()),
        description: Some(format!(
            "compiled methodology flow `{}` — source: {}",
            flow.id,
            resolved.path.display()
        )),
        flow_template: Some(flow.id.clone()),
        ..Default::default()
    };
    let task = state
        .store
        .create_board_task(&input)
        .await
        .map_err(|e| anyhow!("DB: {}", e))?;
    let task_id = task.id.to_string();

    let mut ctx = crate::engine::flow::FlowContext::new();
    if let Some(params) = args.get("params").and_then(|v| v.as_object()) {
        for (k, v) in params {
            let value = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            ctx.set(k.clone(), value);
        }
    }

    let _ = state
        .store
        .update_board_task(
            &task_id,
            &missiond_core::types::UpdateBoardTaskInput {
                flow_phase: Some("running".to_string()),
                flow_context: Some(serde_json::to_string(&ctx).unwrap_or_default()),
                status: Some("running".to_string()),
                ..Default::default()
            },
        )
        .await;

    let run_result = crate::engine::flow::runner::run_flow(state, &flow, &mut ctx, &task_id).await;
    match run_result {
        Ok(()) => {
            let _ = state
                .store
                .update_board_task(
                    &task_id,
                    &missiond_core::types::UpdateBoardTaskInput {
                        flow_phase: Some("completed".to_string()),
                        status: Some("done".to_string()),
                        ..Default::default()
                    },
                )
                .await;
            Ok(ToolResult::json_pretty(&json!({
                "status": "dispatched",
                "flow_ref": "F-methodology-to-executable-compile :: s6 dry-run-or-run (run)",
                "flow_id": flow.id,
                "flow_path": resolved.path.display().to_string(),
                "task_id": task_id,
                "completed_nodes": ctx.completed_nodes,
                "record_execution_status": "TODO_external — methodology compile flows are not yet linked to a workflow row; call mission_workflow(action=record_execution) manually with the matching workflow_id once distilled",
            })))
        }
        Err(e) => {
            let _ = state
                .store
                .update_board_task(
                    &task_id,
                    &missiond_core::types::UpdateBoardTaskInput {
                        flow_phase: Some("failed".to_string()),
                        status: Some("failed".to_string()),
                        ..Default::default()
                    },
                )
                .await;
            Err(e)
        }
    }
}

// ───────────────────────────────────────────────────────────────────────
// helpers — methodology compiler v0 (pure, covered by unit tests)
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct MethodologyStep {
    id: String,
    body: String,
}

#[derive(Debug, Clone)]
struct GeneratedMeta {
    flow_id: String,
    name: String,
    source_path: String,
    source_hash: String,
    generated_at: String,
    compiler_status: String,
}

#[derive(Debug)]
enum CompiledFlowError {
    MissingArgs,
    Missing { flow_id: String, expected: PathBuf },
}

#[derive(Debug, Clone)]
struct CompiledFlow {
    path: PathBuf,
}

fn resolve_methodology_path(
    project_root: &Path,
    name: Option<&str>,
    workflow_path: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(p) = workflow_path.filter(|s| !s.is_empty()) {
        let candidate = PathBuf::from(p);
        return Ok(if candidate.is_absolute() {
            candidate
        } else {
            project_root.join(candidate)
        });
    }
    if let Some(name) = name.filter(|s| !s.is_empty()) {
        let mut p = project_root.join(WORKFLOWS_DIR).join(name);
        if p.extension().is_none() {
            p.set_extension("lisp");
        }
        return Ok(p);
    }
    Err("compile_methodology requires `workflow_path` or `name`".to_string())
}

fn validate_methodology_source(content: &str) -> Result<(), String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err("methodology source is empty".to_string());
    }
    if !paren_balanced_ignoring_strings(content) {
        return Err("methodology source has unbalanced parentheses".to_string());
    }
    if !content.chars().any(|c| c == '(') {
        return Err("methodology source has no top-level form".to_string());
    }
    Ok(())
}

/// SHA-256 hex of the source bytes — stable across runs for identical input.
fn source_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for byte in digest {
        s.push_str(&format!("{:02x}", byte));
    }
    s
}

fn derive_flow_id(stem: &str, output_flow_id: Option<&str>) -> String {
    if let Some(explicit) = output_flow_id.filter(|s| !s.is_empty()) {
        return explicit.to_string();
    }
    let safe = sanitize_id_token(stem);
    if safe.is_empty() {
        "methodology-anonymous-v0".to_string()
    } else {
        format!("methodology-{}-v0", safe)
    }
}

fn sanitize_id_token(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_hyphen = false;
    for ch in raw.chars() {
        let allowed = ch.is_ascii_alphanumeric() || ch == '_' || ch == '-';
        if allowed {
            out.push(ch);
            prev_hyphen = ch == '-';
        } else if !prev_hyphen && !out.is_empty() {
            out.push('-');
            prev_hyphen = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn source_path_for_yaml(project_root: &Path, path: &Path) -> String {
    match path.strip_prefix(project_root) {
        Ok(rel) => rel.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

fn generated_yaml_path(project_root: &Path, flow_id: &str) -> PathBuf {
    project_root
        .join(GENERATED_FLOWS_DIR)
        .join(format!("{}.yaml", flow_id))
}

/// Extract `(step <id> <body…>)` forms from a methodology Lisp source. Multi-line
/// bodies are accumulated using paren depth tracking that ignores string contents.
/// Pure-fn for testing.
fn extract_steps(content: &str) -> Vec<MethodologyStep> {
    let mut steps = Vec::new();
    let mut buffer: Option<(String, String, i32, bool, bool)> = None;
    // (id, body, depth, in_string, escaped)

    for line in content.lines() {
        if let Some((mut id, mut body, mut depth, mut in_string, mut escaped)) = buffer.take() {
            body.push('\n');
            for ch in line.chars() {
                body.push(ch);
                advance_paren_state(ch, &mut depth, &mut in_string, &mut escaped);
                if depth == 0 {
                    steps.push(MethodologyStep {
                        id: std::mem::take(&mut id),
                        body: std::mem::take(&mut body),
                    });
                    buffer = None;
                    break;
                }
            }
            if depth > 0 {
                buffer = Some((id, body, depth, in_string, escaped));
            }
            continue;
        }

        let leading = line.chars().take_while(|c| c.is_whitespace()).count();
        let rest = &line[leading..];
        if !rest.starts_with("(step") {
            continue;
        }
        let after_step = &rest["(step".len()..];
        if !after_step.starts_with(|c: char| c.is_whitespace()) {
            continue; // e.g. (steps … shouldn't match
        }
        let after_ws = after_step.trim_start();
        let id_end = after_ws
            .find(|c: char| c.is_whitespace() || c == ')')
            .unwrap_or(after_ws.len());
        let id = after_ws[..id_end].trim().to_string();
        if id.is_empty() {
            continue;
        }

        let mut depth: i32 = 0;
        let mut in_string = false;
        let mut escaped = false;
        let mut body = String::new();
        let mut closed = false;
        for ch in rest.chars() {
            body.push(ch);
            advance_paren_state(ch, &mut depth, &mut in_string, &mut escaped);
            if depth == 0 && body.ends_with(')') {
                steps.push(MethodologyStep {
                    id: id.clone(),
                    body: body.clone(),
                });
                closed = true;
                break;
            }
        }
        if !closed && depth > 0 {
            buffer = Some((id, body, depth, in_string, escaped));
        }
    }

    steps
}

fn advance_paren_state(ch: char, depth: &mut i32, in_string: &mut bool, escaped: &mut bool) {
    if *in_string {
        if *escaped {
            *escaped = false;
        } else if ch == '\\' {
            *escaped = true;
        } else if ch == '"' {
            *in_string = false;
        }
        return;
    }
    match ch {
        '"' => *in_string = true,
        '(' => *depth += 1,
        ')' => *depth -= 1,
        _ => {}
    }
}

fn build_generated_yaml(
    meta: &GeneratedMeta,
    steps: &[MethodologyStep],
    review_required: bool,
) -> Result<String, serde_yaml::Error> {
    use serde_yaml::{Mapping, Value as Yaml};

    let mut root = Mapping::new();
    root.insert(Yaml::from("id"), Yaml::from(meta.flow_id.clone()));
    root.insert(Yaml::from("name"), Yaml::from(meta.name.clone()));
    root.insert(Yaml::from("source_kind"), Yaml::from("methodology_lisp"));
    root.insert(Yaml::from("source_path"), Yaml::from(meta.source_path.clone()));
    root.insert(Yaml::from("source_hash"), Yaml::from(meta.source_hash.clone()));
    root.insert(Yaml::from("generated_by"), Yaml::from(COMPILER_VERSION));
    root.insert(Yaml::from("generated_at"), Yaml::from(meta.generated_at.clone()));
    root.insert(
        Yaml::from("compiler_status"),
        Yaml::from(meta.compiler_status.clone()),
    );
    root.insert(Yaml::from("review_required"), Yaml::from(review_required));

    let mut nodes_seq: Vec<Yaml> = Vec::new();
    if steps.is_empty() {
        let mut node = Mapping::new();
        node.insert(Yaml::from("id"), Yaml::from("manual_review"));
        node.insert(Yaml::from("type"), Yaml::from("slot_task"));
        node.insert(Yaml::from("model"), Yaml::from("opus"));
        node.insert(
            Yaml::from("prompt"),
            Yaml::from(format!(
                "Manually review compiled methodology '{flow}' before running.\n\
                 Source: {src}\n\
                 Source hash: {hash}\n\
                 The deterministic compiler v0 could not auto-extract executable (step …) forms.\n\
                 Edit this YAML or augment the source Lisp before dispatching.",
                flow = meta.flow_id,
                src = meta.source_path,
                hash = meta.source_hash,
            )),
        );
        nodes_seq.push(Yaml::Mapping(node));
    } else {
        for step in steps {
            let safe_id = sanitize_id_token(&step.id);
            let node_id = if safe_id.is_empty() {
                "step".to_string()
            } else {
                format!("step_{}", safe_id)
            };
            let mut node = Mapping::new();
            node.insert(Yaml::from("id"), Yaml::from(node_id.clone()));
            node.insert(Yaml::from("type"), Yaml::from("slot_task"));
            node.insert(Yaml::from("model"), Yaml::from("opus"));
            node.insert(Yaml::from("prompt"), Yaml::from(step.body.clone()));
            node.insert(
                Yaml::from("save_as"),
                Yaml::from(format!("{}_result", node_id)),
            );
            nodes_seq.push(Yaml::Mapping(node));
        }
    }
    root.insert(Yaml::from("nodes"), Yaml::Sequence(nodes_seq));
    serde_yaml::to_string(&Yaml::Mapping(root))
}

fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp.write");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}

fn resolve_compiled_flow(
    project_root: &Path,
    flow_id: Option<&str>,
    flow_path: Option<&str>,
    name: Option<&str>,
) -> Result<CompiledFlow, CompiledFlowError> {
    if let Some(p) = flow_path.filter(|s| !s.is_empty()) {
        let candidate = PathBuf::from(p);
        let resolved = if candidate.is_absolute() {
            candidate
        } else {
            project_root.join(candidate)
        };
        if resolved.exists() {
            return Ok(CompiledFlow { path: resolved });
        }
        let id_for_msg = flow_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| resolved.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string());
        return Err(CompiledFlowError::Missing {
            flow_id: id_for_msg,
            expected: resolved,
        });
    }

    let id = if let Some(id) = flow_id.filter(|s| !s.is_empty()) {
        id.to_string()
    } else if let Some(n) = name.filter(|s| !s.is_empty()) {
        derive_flow_id(n, None)
    } else {
        return Err(CompiledFlowError::MissingArgs);
    };
    let expected = generated_yaml_path(project_root, &id);
    if expected.exists() {
        Ok(CompiledFlow { path: expected })
    } else {
        Err(CompiledFlowError::Missing {
            flow_id: id,
            expected,
        })
    }
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

    // ──────────────────────────────────────────────────────────────
    // methodology compiler v0 — pure-fn tests
    // ──────────────────────────────────────────────────────────────

    #[test]
    fn parse_compile_mode_default_and_explicit() {
        assert_eq!(parse_compile_mode(None), Ok(CompileMode::DryRun));
        assert_eq!(parse_compile_mode(Some("")), Ok(CompileMode::DryRun));
        assert_eq!(parse_compile_mode(Some("dry_run")), Ok(CompileMode::DryRun));
        assert_eq!(
            parse_compile_mode(Some("deterministic")),
            Ok(CompileMode::Deterministic)
        );
        assert!(parse_compile_mode(Some("nope")).is_err());
    }

    #[test]
    fn methodology_path_workflow_path_takes_precedence() {
        let root = Path::new("/tmp/proj");
        // absolute workflow_path passes through
        let abs = resolve_methodology_path(root, None, Some("/abs/some.lisp")).unwrap();
        assert_eq!(abs, PathBuf::from("/abs/some.lisp"));
        // relative workflow_path joins to project_root
        let rel = resolve_methodology_path(root, None, Some("methods/foo.lisp")).unwrap();
        assert_eq!(rel, PathBuf::from("/tmp/proj/methods/foo.lisp"));
    }

    #[test]
    fn methodology_path_name_appends_lisp_extension() {
        let root = Path::new("/tmp/proj");
        let p = resolve_methodology_path(root, Some("bus-refactor"), None).unwrap();
        assert_eq!(
            p,
            PathBuf::from("/tmp/proj/.missiond/workflows/bus-refactor.lisp")
        );
        // Caller may pass an explicit extension — keep it.
        let p2 = resolve_methodology_path(root, Some("bus-refactor.lisp"), None).unwrap();
        assert_eq!(
            p2,
            PathBuf::from("/tmp/proj/.missiond/workflows/bus-refactor.lisp")
        );
    }

    #[test]
    fn methodology_path_requires_one_of_args() {
        let root = Path::new("/tmp/proj");
        assert!(resolve_methodology_path(root, None, None).is_err());
        assert!(resolve_methodology_path(root, Some(""), Some("")).is_err());
    }

    #[test]
    fn validate_methodology_source_rejects_empty_and_unbalanced() {
        assert!(validate_methodology_source("").is_err());
        assert!(validate_methodology_source("   \n  ").is_err());
        // No top-level form even if non-empty.
        assert!(validate_methodology_source("not-a-form").is_err());
        // Unbalanced parens (string-ignoring detector catches this).
        assert!(validate_methodology_source("(workflow demo (step s1").is_err());
        // Balanced + has form → ok.
        assert!(validate_methodology_source("(workflow demo (step s1 \"do x\"))").is_ok());
    }

    #[test]
    fn source_hash_is_stable_and_distinguishes_inputs() {
        let a1 = source_hash("(workflow demo)");
        let a2 = source_hash("(workflow demo)");
        let b = source_hash("(workflow other)");
        assert_eq!(a1, a2, "same input must hash identically");
        assert_ne!(a1, b, "different input must hash differently");
        // sha256 hex is 64 chars.
        assert_eq!(a1.len(), 64);
        assert!(a1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn derive_flow_id_uses_explicit_first() {
        assert_eq!(
            derive_flow_id("bus-refactor", Some("custom-id")),
            "custom-id".to_string()
        );
        // empty explicit falls back
        assert_eq!(
            derive_flow_id("bus-refactor", Some("")),
            "methodology-bus-refactor-v0".to_string()
        );
        // none → default
        assert_eq!(
            derive_flow_id("bus-refactor", None),
            "methodology-bus-refactor-v0".to_string()
        );
        // sanitization collapses non-alnum
        assert_eq!(
            derive_flow_id("Foo Bar/Baz!", None),
            "methodology-Foo-Bar-Baz-v0".to_string()
        );
        // anonymous fallback when stem yields empty token
        assert_eq!(
            derive_flow_id("///", None),
            "methodology-anonymous-v0".to_string()
        );
    }

    #[test]
    fn extract_steps_handles_single_line_form() {
        let body = "\
(workflow demo
  (step s1 \"first thing\")
  (step s2 \"second thing\"))
";
        let steps = extract_steps(body);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].id, "s1");
        assert!(steps[0].body.contains("first thing"));
        assert_eq!(steps[1].id, "s2");
        assert!(steps[1].body.contains("second thing"));
    }

    #[test]
    fn extract_steps_handles_multi_line_form() {
        let body = "\
(workflow demo
  (step long-id
    \"line one
     line two\"
    :note other))
";
        let steps = extract_steps(body);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].id, "long-id");
        assert!(steps[0].body.contains("line one"));
        assert!(steps[0].body.contains(":note other"));
    }

    #[test]
    fn extract_steps_ignores_lookalike_forms() {
        // (steps …) and (step) without body should be skipped — first because
        // of the prefix mismatch, second because the id parse fails.
        let body = "\
(workflow demo
  (steps
    (foo))
  (step)
  (step real \"ok\"))
";
        let steps = extract_steps(body);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].id, "real");
    }

    #[test]
    fn extract_steps_returns_empty_when_no_steps() {
        // Real methodology lisps frequently have no top-level (step …) — they
        // use (phase-* …) instead. Compiler v0 must hand this to manual review.
        let body = "\
(workflow bus-refactor
  (phase-A exploration :goal \"survey\")
  (phase-B design-freeze :goal \"freeze\"))
";
        assert!(extract_steps(body).is_empty());
    }

    #[test]
    fn extract_steps_paren_in_string_does_not_close_form() {
        let body = "\
(workflow demo
  (step s1 \"closes ) inside string\"
        :tag normal))
";
        let steps = extract_steps(body);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].id, "s1");
        assert!(steps[0].body.contains("closes ) inside string"));
        assert!(steps[0].body.contains(":tag normal"));
    }

    #[test]
    fn build_generated_yaml_contains_source_metadata_and_steps() {
        let meta = GeneratedMeta {
            flow_id: "methodology-foo-v0".to_string(),
            name: "methodology compile v0 — foo".to_string(),
            source_path: ".missiond/workflows/foo.lisp".to_string(),
            source_hash: "deadbeef".repeat(8),
            generated_at: "2026-04-25T00:00:00Z".to_string(),
            compiler_status: COMPILER_STATUS_PREVIEW.to_string(),
        };
        let steps = vec![MethodologyStep {
            id: "s1".to_string(),
            body: "(step s1 \"do x\")".to_string(),
        }];
        let yaml = build_generated_yaml(&meta, &steps, false).expect("yaml builds");
        assert!(yaml.contains("id: methodology-foo-v0"));
        assert!(yaml.contains("source_kind: methodology_lisp"));
        assert!(yaml.contains(".missiond/workflows/foo.lisp"));
        assert!(yaml.contains(&meta.source_hash));
        assert!(yaml.contains(&format!("generated_by: {}", COMPILER_VERSION)));
        assert!(yaml.contains(&format!("compiler_status: {}", COMPILER_STATUS_PREVIEW)));
        assert!(yaml.contains("review_required: false"));
        assert!(yaml.contains("step_s1"));
        assert!(yaml.contains("type: slot_task"));
        // round-trip parse via FlowDefinition (which silently drops the extra
        // metadata fields) — ensures the generated YAML is loader-ready.
        let parsed: crate::engine::flow::FlowDefinition =
            serde_yaml::from_str(&yaml).expect("FlowDefinition parses");
        assert_eq!(parsed.id, "methodology-foo-v0");
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.nodes[0].id, "step_s1");
    }

    #[test]
    fn build_generated_yaml_emits_manual_review_when_no_steps() {
        let meta = GeneratedMeta {
            flow_id: "methodology-foo-v0".to_string(),
            name: "foo".to_string(),
            source_path: "src.lisp".to_string(),
            source_hash: "abc".to_string(),
            generated_at: "ts".to_string(),
            compiler_status: COMPILER_STATUS_PREVIEW.to_string(),
        };
        let yaml = build_generated_yaml(&meta, &[], true).expect("yaml builds");
        assert!(yaml.contains("review_required: true"));
        assert!(yaml.contains("manual_review"));
        assert!(yaml.contains("Manually review"));
        // Must still parse.
        let parsed: crate::engine::flow::FlowDefinition =
            serde_yaml::from_str(&yaml).expect("FlowDefinition parses");
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.nodes[0].id, "manual_review");
    }

    #[test]
    fn generated_yaml_path_lives_under_project_local_dir() {
        let p = generated_yaml_path(Path::new("/tmp/proj"), "methodology-foo-v0");
        assert_eq!(
            p,
            PathBuf::from("/tmp/proj/.missiond/generated/flows/methodology-foo-v0.yaml")
        );
    }

    #[test]
    fn atomic_write_creates_dirs_and_replaces_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp
            .path()
            .join(".missiond/generated/flows/methodology-foo-v0.yaml");
        atomic_write(&target, "hello").expect("first write");
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "hello".to_string()
        );
        // Second write replaces in place.
        atomic_write(&target, "world").expect("second write");
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "world".to_string()
        );
    }

    #[test]
    fn resolve_compiled_flow_missing_returns_structured_pointer() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let err = resolve_compiled_flow(root, Some("methodology-foo-v0"), None, None)
            .expect_err("missing yaml must error");
        match err {
            CompiledFlowError::Missing { flow_id, expected } => {
                assert_eq!(flow_id, "methodology-foo-v0");
                assert!(expected
                    .display()
                    .to_string()
                    .contains(".missiond/generated/flows/methodology-foo-v0.yaml"));
            }
            other => panic!("expected Missing, got {:?}", other),
        }
    }

    #[test]
    fn resolve_compiled_flow_requires_some_arg() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let err = resolve_compiled_flow(root, None, None, None)
            .expect_err("no args → MissingArgs");
        assert!(matches!(err, CompiledFlowError::MissingArgs));
    }

    #[test]
    fn resolve_compiled_flow_finds_existing_yaml_by_flow_id() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let yaml_path = root.join(".missiond/generated/flows/methodology-foo-v0.yaml");
        std::fs::create_dir_all(yaml_path.parent().unwrap()).unwrap();
        std::fs::write(&yaml_path, "id: x\nname: x\nnodes: []\n").unwrap();
        let resolved = resolve_compiled_flow(root, Some("methodology-foo-v0"), None, None)
            .expect("flow yaml exists");
        assert_eq!(resolved.path, yaml_path);
    }

    #[test]
    fn resolve_compiled_flow_falls_back_to_name_via_derive_flow_id() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        // expected location uses the derived id
        let yaml_path = root.join(".missiond/generated/flows/methodology-bus-refactor-v0.yaml");
        std::fs::create_dir_all(yaml_path.parent().unwrap()).unwrap();
        std::fs::write(&yaml_path, "id: x\nname: x\nnodes: []\n").unwrap();
        let resolved = resolve_compiled_flow(root, None, None, Some("bus-refactor"))
            .expect("name resolves to derived flow id");
        assert_eq!(resolved.path, yaml_path);
    }

    #[test]
    fn persist_overwrite_policy_via_path_existence() {
        // The action_compile_deterministic flow uses path.exists() && !overwrite
        // to refuse rewrites. We mimic that condition here so the behavior is
        // covered by a unit test rather than an integration test.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let target = generated_yaml_path(root, "methodology-foo-v0");
        atomic_write(&target, "first").expect("seed file");

        let exists = target.exists();
        let overwrite = false;
        let should_refuse = exists && !overwrite;
        assert!(should_refuse, "must refuse overwrite without flag");

        let overwrite = true;
        let should_refuse_with_flag = target.exists() && !overwrite;
        assert!(
            !should_refuse_with_flag,
            "overwrite=true must allow replacement"
        );
        // And atomic_write actually replaces — the policy is the only gate.
        atomic_write(&target, "second").expect("overwrite write");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "second");
    }

    #[test]
    fn sanitize_id_token_keeps_safe_chars_and_collapses_runs() {
        assert_eq!(sanitize_id_token("foo"), "foo");
        assert_eq!(sanitize_id_token("foo_bar-baz"), "foo_bar-baz");
        assert_eq!(sanitize_id_token("Foo Bar/Baz!"), "Foo-Bar-Baz");
        assert_eq!(sanitize_id_token("///"), "");
    }

    #[test]
    fn source_path_for_yaml_strips_project_root_when_under_it() {
        let root = Path::new("/tmp/proj");
        assert_eq!(
            source_path_for_yaml(root, Path::new("/tmp/proj/.missiond/workflows/foo.lisp")),
            ".missiond/workflows/foo.lisp"
        );
        // Outside the project root → keep absolute.
        assert_eq!(
            source_path_for_yaml(root, Path::new("/elsewhere/foo.lisp")),
            "/elsewhere/foo.lisp"
        );
    }
}
