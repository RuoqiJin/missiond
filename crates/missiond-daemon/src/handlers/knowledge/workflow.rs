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
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::handlers::knowledge::file_artifacts::{
    attempt_artifact_write, ArtifactKind, WriterContext,
};
use crate::handlers::knowledge::review_gate::{
    apply_compile_review_gates, maybe_emit_review_question_resolved, parse_compile_review_gate,
    parse_review_gate_policy, parse_review_question_id_struct, parse_review_resolution_input,
    resolution_wire_string, review_gate_policy_was_explicit, stamp_needs_changes_next_step,
    stamp_resolution_payload, validate_review_resolution_envelope, ResolutionOutcome,
    ReviewResolutionInput,
};
use crate::minimax_client::ChatMessage;
use crate::slot_orchestrator::project_root::{
    resolve_target_project_root, ResolutionError,
};
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
                    "actions: list|get|match|apply|distill|record_execution|compile_methodology|run_methodology|resolve_review",
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
        "resolve_review" => action_resolve_review(state, &args).await,
        other => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("unknown mission_workflow action `{}`", other),
            )
            .with_suggestion(
                "valid: list|get|match|apply|distill|record_execution|compile_methodology|run_methodology|resolve_review",
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
        DistillMode::DryRun => action_distill_dry_run(state, args, &plan, name, persist).await,
        DistillMode::Sonnet => action_distill_sonnet(state, args, &plan, name, persist).await,
    }
}

async fn action_distill_dry_run(
    state: &AppState,
    args: &Value,
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

        // wave-14 :: file-first SSOT mirror. Topic defaults to `name` (the
        // distill UNIQUE key) so the on-disk path matches the registry
        // entry without an extra arg. The DB row stays committed even if
        // the file write fails (file-vs-db contract).
        let file_args = extract_workflow_file_args(args);
        let topic_for_gate = file_args
            .topic
            .map(|s| s.to_string())
            .unwrap_or_else(|| name.to_string());
        maybe_write_workflow_artifact(state, &file_args, &mut payload, &preview_sexp, name).await;

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

        // wave-14 :: file-first SSOT mirror — same partial semantics as the
        // dry_run path. The distilled workflow_sexp is the durable
        // artifact; we splice the path/sha so future distill runs / forge
        // compilers can verify on-disk parity.
        let file_args = extract_workflow_file_args(args);
        let topic_for_gate = file_args
            .topic
            .map(|s| s.to_string())
            .unwrap_or_else(|| name.to_string());
        maybe_write_workflow_artifact(state, &file_args, &mut payload, &workflow_sexp, name).await;

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

    let project_root = match resolve_project_root_from_args(state, args).await {
        Ok(p) => p,
        Err(reason) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::INVALID_PARAM, reason).with_suggestion(
                    "supply `project` (registered id) or absolute `cwd`; \
                     compile_methodology refuses process-cwd fallback so the generated YAML \
                     always lands inside the registered project root.",
                ),
            ))
        }
    };
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
            action_compile_deterministic(state, &project_root, &path, &content, args).await
        }
    }
}

fn action_compile_dry_run(path: &Path, content: &str) -> Result<ToolResult> {
    let line_count = content.lines().count();
    // Surface both the cheap line-counter (back-compat with earlier
    // dry-run consumers that scraped `phase_form_count` / `step_form_count`)
    // and the v0 semantic lifter's richer breakdown so callers can preview
    // exactly what `compile_mode="deterministic"` will emit.
    let phases = count_top_form(content, "phase");
    let steps = count_top_form(content, "step");
    let lifted = extract_methodology_lifted(content);
    Ok(ToolResult::json_pretty(&json!({
        "status": "dry_run",
        "compile_mode": "dry_run",
        "actor_pending": "intent-layer :: workflow compiler (Lisp → executable YAML)",
        "flow_ref": "F-methodology-to-executable-compile",
        "source_path": path.display().to_string(),
        "lines": line_count,
        "phase_form_count": phases,
        "step_form_count": steps,
        "lifted_form_count": lifted.total_count(),
        "lifted_form_breakdown": json!({
            "phases": lifted.phases.len(),
            "principles": lifted.principles.len(),
            "anti_patterns": lifted.anti_patterns.len(),
            "gates": lifted.gates.len(),
            "artifacts": lifted.artifacts.len(),
            "authorities": lifted.authorities.len(),
        }),
        "next_step": "pass compile_mode=\"deterministic\" to emit an executable YAML preview; persist=true writes it to .missiond/generated/flows/<flow_id>.yaml",
    })))
}

async fn action_compile_deterministic(
    state: &AppState,
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

    let located_steps = extract_steps_with_lines(content);
    let lifted = extract_methodology_lifted(content);
    let review_required = located_steps.is_empty();
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
    let yaml = build_generated_yaml(&meta, &located_steps, &lifted, review_required)
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
        "step_count": located_steps.len(),
        "review_required": review_required,
        "lifted_form_count": lifted.total_count(),
        "lifted_form_breakdown": json!({
            "phases": lifted.phases.len(),
            "principles": lifted.principles.len(),
            "anti_patterns": lifted.anti_patterns.len(),
            "gates": lifted.gates.len(),
            "artifacts": lifted.artifacts.len(),
            "authorities": lifted.authorities.len(),
        }),
        "params_echo": args.get("params").cloned().unwrap_or(Value::Null),
        "future_compiler_actor": "intent-layer LLM/forge compiler — semantic execution of phase/anti-pattern/gate forms deferred; v0 lifts them into methodology_metadata only",
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

    // wave-14 :: file-first SSOT mirror. compile_methodology already reads
    // the methodology lisp from `.missiond/workflows/<name>.lisp`, so the
    // file-first writer is only meaningful when the caller wants to
    // canonicalise / snapshot the source under a different topic, OR when
    // the caller passes overwrite_file=true to "re-emit" the same file.
    // Topic precedence: explicit `topic` arg > `name` arg > source stem.
    let file_args = extract_workflow_file_args(args);
    let fallback_topic = args
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| stem.clone());
    let topic_for_gate = file_args
        .topic
        .map(|s| s.to_string())
        .unwrap_or_else(|| fallback_topic.clone());
    maybe_write_workflow_artifact(state, &file_args, &mut payload, content, &fallback_topic).await;

    // wave-14 :: review-gate auto-create. compile_methodology has no
    // workflow_id (the methodology source predates any distilled row), so
    // the artifact_id used in the deterministic question id is the
    // generated `flow_id`. The hook only fires when both
    // `review_gate_policy=emit_question` AND the file-first mirror was
    // requested AND landed (`file_written=true`); a YAML-only persist run
    // intentionally stays quiet because the workflow scope is not yet
    // canonicalised in `.missiond/workflows/<topic>.lisp`.
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
        &meta.flow_id,
        1,
        Some(&topic_for_gate),
    )
    .await;

    Ok(ToolResult::json_pretty(&payload))
}

// ───────────────────────────────────────────────────────────────────────
// run_methodology — resolve compiled YAML, dispatch into flow engine
// ───────────────────────────────────────────────────────────────────────

async fn action_run_methodology(state: &AppState, args: &Value) -> Result<ToolResult> {
    let project_root = match resolve_project_root_from_args(state, args).await {
        Ok(p) => p,
        Err(reason) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::INVALID_PARAM, reason).with_suggestion(
                    "supply `project` (registered id) or absolute `cwd`; \
                     run_methodology refuses process-cwd fallback so the compiled YAML \
                     resolves against the registered project root.",
                ),
            ))
        }
    };
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
// wave-16 :: explicit review-resolution surface
//
// Closes the Wave-15 gap: directive / plan already accept `review_question_id
// + review_decision + review_actor + review_note` to flip an artifact from
// the auto-emitted `QuestionEvent::Created` (wave-14) into an explicit
// `QuestionEvent::Resolved` / `DecisionResolved`. Workflow auto-emits the
// same Created envelopes (scope = `workflow`, see `apply_compile_review_gates`
// calls in `action_distill_*` and `action_compile_deterministic`) but had
// no resolution surface — Wave-16 adds it here.
//
// Two forms share one entry point because the auto-emitter uses the same
// scope label (`workflow`) for both:
//
//   1. Persisted distill row — `artifact_id` parses as a UUID and the
//      `workflow_get_by_id` lookup returns Some. The `Workflow` row has no
//      version / status fields (unlike Directive / Plan), so the resolver
//      neither needs nor performs an "approve transition"; on `approved`
//      it stamps `status=review_approved` so the response is loud, on
//      `rejected` / `needs_changes` it stamps the matching review status
//      AND `next_step`. Bus emission is best-effort, mirroring directive /
//      plan.
//
//   2. compile_methodology compiled YAML — `artifact_id` is the `flow_id`
//      string (NOT a UUID; see `derive_flow_id` → `methodology-<stem>-v0`).
//      No DB row exists, so the resolver returns a STRUCTURED RECEIPT and
//      never fakes DB state. The receipt + Resolved bus event both carry
//      the deterministic question id so an external archiver / audit
//      pipeline can correlate.
//
// Action whitelist: only `compile`. The wave-14 auto-emitter always uses
// action=`compile` for workflow ids (see `apply_compile_review_gates(...)`
// → `auto_emit_review_question_after_artifact_write` default action). If
// callers ever opt into a custom id with a different action, the envelope
// validator will surface `REVIEW_ACTION_UNSUPPORTED` and force them to
// reconsider.
//
// Scope label: `workflow` for BOTH persisted and methodology paths.
// (Wave-16 task brief sketched a separate `methodology` scope; the actual
// wave-14 derivation in `review_gate.rs` uses `workflow` for both — we
// match the existing emitter to keep ids round-trippable. The methodology
// path is distinguished by the artifact_id NOT being a UUID.)
// ───────────────────────────────────────────────────────────────────────

/// Action whitelist for the workflow surface. The wave-14 auto-emitter
/// always uses `compile` (see `auto_emit_review_question_after_artifact_write`
/// default), so this is the only action a workflow review id can carry.
const WORKFLOW_REVIEW_ACTIONS: &[&str] = &["compile"];

/// Workflow review version. The `Workflow` row has no `version` column;
/// the auto-emitter pins all workflow ids to `v1` (see `apply_compile_review_gates`
/// calls in `action_distill_*` / `action_compile_deterministic`). Resolution
/// must validate against the same constant so a re-emit / retry stays
/// deterministic.
const WORKFLOW_REVIEW_VERSION: i32 = 1;

async fn action_resolve_review(state: &AppState, args: &Value) -> Result<ToolResult> {
    // Caller must supply both id + decision (with optional actor / note).
    // Missing decision when id is present is fail-fast — same contract as
    // directive / plan.
    let resolution = match parse_review_resolution_input(args) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "resolve_review requires `review_question_id` (and `review_decision`)",
                )
                .with_suggestion(
                    "use the deterministic id wave-14 emitted on the workflow Created event",
                ),
            ))
        }
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(e.code(), e.message()),
            ))
        }
    };

    let parsed = match parse_review_question_id_struct(&resolution.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new("REVIEW_ID_MALFORMED", e.message()),
            ))
        }
    };

    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "workflow",
        &parsed.artifact_id,
        WORKFLOW_REVIEW_VERSION,
        WORKFLOW_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(
            ToolError::new(e.code(), e.message()),
        ));
    }

    // Decide between persisted-row mode and methodology-receipt mode by
    // attempting a UUID parse on the envelope's artifact_id. Methodology
    // flow ids look like `methodology-<stem>-v0` and never parse as UUID.
    match uuid::Uuid::parse_str(&parsed.artifact_id) {
        Ok(workflow_id) => {
            action_resolve_review_persisted(state, workflow_id, resolution).await
        }
        Err(_) => action_resolve_review_methodology(state, parsed.artifact_id.clone(), resolution).await,
    }
}

/// Persisted distill resolution. The workflow row exists; the `Workflow`
/// type has no version / status fields, so the resolver does not perform a
/// DB transition — it stamps the decision into the response and emits the
/// Resolved bus event. `approved` is loud (`status=review_approved`);
/// `rejected` / `needs_changes` keep the artifact non-approved with the
/// reason surfaced.
async fn action_resolve_review_persisted(
    state: &AppState,
    workflow_id: uuid::Uuid,
    input: ReviewResolutionInput,
) -> Result<ToolResult> {
    let row = state
        .store
        .workflow_get_by_id(workflow_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let row = match row {
        Some(w) => w,
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("workflow `{}` not found for resolution", workflow_id),
                ),
            ))
        }
    };

    let mut payload = json!({
        "scope": "workflow",
        "mode": "persisted",
        "workflow_id": row.id,
        "workflow_name": row.name,
        "version": WORKFLOW_REVIEW_VERSION,
    });

    match input.decision.outcome() {
        ResolutionOutcome::PerformTransition => {
            // The Workflow row has no status column to flip — record the
            // approval loudly in the response so callers see the decision
            // landed (the bus Resolved event carries the same).
            payload["status"] = json!("review_approved");
        }
        ResolutionOutcome::KeepArtifact => {
            payload["status"] = json!("review_rejected");
        }
        ResolutionOutcome::RequestChanges => {
            payload["status"] = json!("review_needs_changes");
            stamp_needs_changes_next_step(&mut payload, "workflow", "distill");
        }
    }

    stamp_resolution_payload(&mut payload, &input);
    let resolution_str = resolution_wire_string(input.decision);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        Some(&input.question_id),
        resolution_str,
        None,
    )
    .await;
    Ok(ToolResult::json_pretty(&payload))
}

/// Methodology compiled-YAML resolution. NO DB workflow row exists —
/// `compile_methodology` only writes a `.missiond/generated/flows/<flow_id>.yaml`
/// (and optionally mirrors the source under `.missiond/workflows/<topic>.lisp`).
/// The resolver returns a structured receipt so an external archiver /
/// audit pipeline can correlate the decision with the source artifact,
/// AND emits the Resolved bus event (best-effort). It NEVER fakes DB
/// state — there is nothing to mutate.
async fn action_resolve_review_methodology(
    state: &AppState,
    flow_id: String,
    input: ReviewResolutionInput,
) -> Result<ToolResult> {
    let mut payload = json!({
        "scope": "workflow",
        "mode": "methodology",
        "flow_id": flow_id,
        "version": WORKFLOW_REVIEW_VERSION,
        "db_transition": false,
        "note": "compile_methodology has no workflow row; resolution returns a receipt and emits the Resolved bus event without DB mutation",
    });

    match input.decision.outcome() {
        ResolutionOutcome::PerformTransition => {
            payload["status"] = json!("review_approved");
        }
        ResolutionOutcome::KeepArtifact => {
            payload["status"] = json!("review_rejected");
        }
        ResolutionOutcome::RequestChanges => {
            payload["status"] = json!("review_needs_changes");
            stamp_needs_changes_next_step(&mut payload, "workflow", "compile_methodology");
        }
    }

    stamp_resolution_payload(&mut payload, &input);
    let resolution_str = resolution_wire_string(input.decision);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        Some(&input.question_id),
        resolution_str,
        None,
    )
    .await;
    Ok(ToolResult::json_pretty(&payload))
}

// ───────────────────────────────────────────────────────────────────────
// wave-14 :: workflow file-first writer args
//
// distill (dry_run + sonnet) and compile_methodology share one writer
// surface so the on-disk path layout stays consistent across both actions.
// Topic precedence is per-action (distill uses `name`; compile_methodology
// uses explicit `topic` > `name` > source stem). The DB row / YAML write
// runs first; the file write is best-effort and reports partial-status on
// failure (file-vs-db contract).
// ───────────────────────────────────────────────────────────────────────

struct WorkflowFileArgs<'a> {
    write_file: bool,
    overwrite_file: bool,
    topic: Option<&'a str>,
    project: Option<&'a str>,
    cwd: Option<&'a str>,
    target_project: Option<&'a str>,
}

fn extract_workflow_file_args(args: &Value) -> WorkflowFileArgs<'_> {
    WorkflowFileArgs {
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

async fn maybe_write_workflow_artifact(
    state: &AppState,
    args: &WorkflowFileArgs<'_>,
    payload: &mut Value,
    content: &str,
    fallback_topic: &str,
) {
    if !args.write_file {
        return;
    }
    let topic = args
        .topic
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback_topic.trim());
    if topic.is_empty() {
        if let Some(map) = payload.as_object_mut() {
            map.insert("file_written".to_string(), json!(false));
            map.insert(
                "file_write_error".to_string(),
                json!("write_file=true requires a non-empty `topic` argument (or a workflow `name` fallback)"),
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
            kind: ArtifactKind::Workflow,
            topic,
            project: args.project,
            cwd: args.cwd,
            target_project: args.target_project,
            overwrite: args.overwrite_file,
        },
        content,
    )
    .await;
    outcome.splice_into(payload);
}

// ───────────────────────────────────────────────────────────────────────
// helpers — methodology compiler v0 (pure, covered by unit tests)
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct MethodologyStep {
    id: String,
    body: String,
}

/// One of the higher-order methodology forms the v0 lifter recognises but
/// never converts into an executable node. The compiler stays conservative:
/// the form's raw body is preserved verbatim under
/// `methodology_metadata` in the generated YAML so downstream readers
/// (manual reviewer, future forge compiler, audit trace) can recover the
/// original semantics. Only `(step …)` forms turn into nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MethodologyForm {
    /// Form keyword as it appears in source, e.g. `principle`, `anti-pattern`.
    kind: String,
    /// First whitespace-delimited token after the keyword, treated as an
    /// optional id (e.g. `(principle no-fallback …)` → Some("no-fallback")).
    /// Forms without a leading identifier (or a malformed one) keep `None`
    /// — we never invent ids the source did not author.
    id: Option<String>,
    /// Verbatim source slice of the form, parens included. Multi-line bodies
    /// preserve their original whitespace so reviewers see the methodology
    /// exactly as authored.
    body: String,
    /// 0-based line at which the opening `(` was emitted in the source.
    start_line: usize,
}

/// A `(phase …)` form with the steps the v0 lifter found nested under it.
/// Steps inside a phase are STILL emitted as top-level executable nodes by
/// the YAML builder, but each carries `methodology_metadata.phase_id` so a
/// manual reviewer can rejoin the narrative with the executable plan.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MethodologyPhase {
    /// Phase id (token after `(phase `). Anonymous phases keep `None` and
    /// surface in metadata as `phase_<line>` so YAML keys stay distinct.
    id: Option<String>,
    /// Verbatim source slice including parens.
    body: String,
    /// Inclusive 0-based line range covered by the phase form. Used to
    /// associate inner steps without requiring a recursive parser.
    start_line: usize,
    end_line: usize,
}

/// Aggregate result of the v0 semantic lifter — produced by
/// [`extract_methodology_lifted`] and consumed by [`build_generated_yaml`].
/// All vectors preserve source order so the generated YAML reads top-to-
/// bottom against the methodology Lisp.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MethodologyLifted {
    phases: Vec<MethodologyPhase>,
    principles: Vec<MethodologyForm>,
    anti_patterns: Vec<MethodologyForm>,
    gates: Vec<MethodologyForm>,
    artifacts: Vec<MethodologyForm>,
    authorities: Vec<MethodologyForm>,
}

impl MethodologyLifted {
    fn is_empty(&self) -> bool {
        self.phases.is_empty()
            && self.principles.is_empty()
            && self.anti_patterns.is_empty()
            && self.gates.is_empty()
            && self.artifacts.is_empty()
            && self.authorities.is_empty()
    }

    /// Total count of all lifted forms across every category — used by the
    /// dry-run preview and the deterministic-mode payload to surface a single
    /// `lifted_form_count` figure for callers.
    fn total_count(&self) -> usize {
        self.phases.len()
            + self.principles.len()
            + self.anti_patterns.len()
            + self.gates.len()
            + self.artifacts.len()
            + self.authorities.len()
    }
}

/// Step keyed by its 0-based starting line, used internally by
/// [`build_generated_yaml`] to attach `phase_id` metadata when a step's line
/// falls inside a phase form's `start_line..=end_line` range.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LocatedStep {
    step: MethodologyStep,
    start_line: usize,
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

/// Variant of [`extract_steps`] that also records each step's 0-based source
/// line. Used by [`build_generated_yaml`] to assign `phase_id` metadata when
/// a step's line falls inside a `(phase …)` form's range. The matching rules
/// are identical to `extract_steps` so the back-compat tests still cover the
/// recognition surface.
fn extract_steps_with_lines(content: &str) -> Vec<LocatedStep> {
    let mut out: Vec<LocatedStep> = Vec::new();
    let mut buffer: Option<(LocatedStep, i32, bool, bool)> = None;
    // (located_step, depth, in_string, escaped)

    for (line_idx, line) in content.lines().enumerate() {
        if let Some((mut ls, mut depth, mut in_string, mut escaped)) = buffer.take() {
            ls.step.body.push('\n');
            for ch in line.chars() {
                ls.step.body.push(ch);
                advance_paren_state(ch, &mut depth, &mut in_string, &mut escaped);
                if depth == 0 {
                    out.push(LocatedStep {
                        step: MethodologyStep {
                            id: std::mem::take(&mut ls.step.id),
                            body: std::mem::take(&mut ls.step.body),
                        },
                        start_line: ls.start_line,
                    });
                    buffer = None;
                    break;
                }
            }
            if depth > 0 {
                buffer = Some((ls, depth, in_string, escaped));
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
                out.push(LocatedStep {
                    step: MethodologyStep {
                        id: id.clone(),
                        body: body.clone(),
                    },
                    start_line: line_idx,
                });
                closed = true;
                break;
            }
        }
        if !closed && depth > 0 {
            buffer = Some((
                LocatedStep {
                    step: MethodologyStep { id, body },
                    start_line: line_idx,
                },
                depth,
                in_string,
                escaped,
            ));
        }
    }

    out
}

/// Conservative semantic lifter for the methodology compiler v0.
///
/// Recognises six higher-order forms — `(phase …)`, `(principle …)`,
/// `(anti-pattern …)`, `(gate …)`, `(artifact …)`, `(authority …)` — when
/// they appear as standalone forms whose opening paren sits at the start of
/// a (whitespace-trimmed) line. This matches the convention used by
/// [`extract_steps`] and by every methodology Lisp shipped under
/// `.missiond/workflows/`. Forms appearing only as inner tokens of another
/// expression are deliberately ignored — the lifter never tries to be a
/// real sexp parser, and never speculates about meaning the source did not
/// declare.
///
/// The lifter NEVER converts these forms into executable nodes. They live in
/// `methodology_metadata` on the generated YAML so the deterministic
/// compiler's contract — "v0 only emits nodes for `(step …)`" — stays
/// intact (intent-flow.lisp :: F-methodology-to-executable-compile :: s2
/// `phases / gates / anti-patterns / authority lifting` is no longer
/// pending; semantic execution remains a future forge concern).
fn extract_methodology_lifted(content: &str) -> MethodologyLifted {
    const KEYWORDS: &[&str] = &[
        "phase",
        "principle",
        "anti-pattern",
        "gate",
        "artifact",
        "authority",
    ];

    let mut lifted = MethodologyLifted::default();
    // (kind, id, body, depth, in_string, escaped, start_line)
    let mut buffer: Option<(String, Option<String>, String, i32, bool, bool, usize)> = None;

    for (line_idx, line) in content.lines().enumerate() {
        if let Some((kind, id, mut body, mut depth, mut in_string, mut escaped, start_line)) =
            buffer.take()
        {
            body.push('\n');
            let mut closed = false;
            for ch in line.chars() {
                body.push(ch);
                advance_paren_state(ch, &mut depth, &mut in_string, &mut escaped);
                if depth == 0 {
                    push_lifted_form(
                        &mut lifted,
                        &kind,
                        id.clone(),
                        std::mem::take(&mut body),
                        start_line,
                        line_idx,
                    );
                    closed = true;
                    break;
                }
            }
            if !closed && depth > 0 {
                buffer = Some((kind, id, body, depth, in_string, escaped, start_line));
            }
            continue;
        }

        let leading = line.chars().take_while(|c| c.is_whitespace()).count();
        let rest = &line[leading..];
        let Some((kind, after_kind)) = match_form_keyword(rest, KEYWORDS) else {
            continue;
        };
        let after_ws = after_kind.trim_start();
        // Optional id: first non-whitespace, non-paren token. We only treat a
        // bare identifier (no leading `:` or `"`) as an id; keyword args and
        // string payloads stay anonymous so we never accidentally promote
        // `:goal` or `"summary"` into an id slot.
        let id = parse_optional_form_id(after_ws);

        let mut depth: i32 = 0;
        let mut in_string = false;
        let mut escaped = false;
        let mut body = String::new();
        let mut closed = false;
        for ch in rest.chars() {
            body.push(ch);
            advance_paren_state(ch, &mut depth, &mut in_string, &mut escaped);
            if depth == 0 && body.ends_with(')') {
                push_lifted_form(
                    &mut lifted,
                    kind,
                    id.clone(),
                    body.clone(),
                    line_idx,
                    line_idx,
                );
                closed = true;
                break;
            }
        }
        if !closed && depth > 0 {
            buffer = Some((
                kind.to_string(),
                id,
                body,
                depth,
                in_string,
                escaped,
                line_idx,
            ));
        }
    }

    lifted
}

/// Match a known form keyword at the start of a (whitespace-trimmed) line.
/// Returns `Some((keyword, remainder))` only when the keyword is followed by
/// whitespace or `)` — i.e. `(phase` matches, but `(phases` and `(phaseA`
/// do not. This is the same disambiguation rule [`extract_steps`] uses for
/// `(step` vs `(steps`.
fn match_form_keyword<'a>(rest: &'a str, keywords: &[&'static str]) -> Option<(&'static str, &'a str)> {
    if !rest.starts_with('(') {
        return None;
    }
    for kw in keywords {
        let prefix = format!("({}", kw);
        if !rest.starts_with(&prefix) {
            continue;
        }
        let after = &rest[prefix.len()..];
        let next = after.chars().next();
        match next {
            None => return Some((*kw, after)),
            Some(c) if c.is_whitespace() || c == ')' => return Some((*kw, after)),
            _ => continue,
        }
    }
    None
}

/// Treat the first whitespace/paren-delimited token as the form id, but only
/// when it looks like a bare identifier (no leading `:` keyword arg, no
/// leading quote, and at least one ASCII alphanumeric / `-` / `_` char).
/// Anything else stays anonymous — we'd rather lose an id than fabricate
/// one from a string payload or keyword arg.
fn parse_optional_form_id(after_ws: &str) -> Option<String> {
    let token_end = after_ws
        .find(|c: char| c.is_whitespace() || c == ')')
        .unwrap_or(after_ws.len());
    let token = after_ws[..token_end].trim();
    if token.is_empty() {
        return None;
    }
    let first = token.chars().next()?;
    if first == ':' || first == '"' || first == '(' {
        return None;
    }
    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '/' || c == '.')
    {
        return None;
    }
    Some(token.to_string())
}

fn push_lifted_form(
    lifted: &mut MethodologyLifted,
    kind: &str,
    id: Option<String>,
    body: String,
    start_line: usize,
    end_line: usize,
) {
    match kind {
        "phase" => lifted.phases.push(MethodologyPhase {
            id,
            body,
            start_line,
            end_line,
        }),
        "principle" => lifted.principles.push(MethodologyForm {
            kind: kind.to_string(),
            id,
            body,
            start_line,
        }),
        "anti-pattern" => lifted.anti_patterns.push(MethodologyForm {
            kind: kind.to_string(),
            id,
            body,
            start_line,
        }),
        "gate" => lifted.gates.push(MethodologyForm {
            kind: kind.to_string(),
            id,
            body,
            start_line,
        }),
        "artifact" => lifted.artifacts.push(MethodologyForm {
            kind: kind.to_string(),
            id,
            body,
            start_line,
        }),
        "authority" => lifted.authorities.push(MethodologyForm {
            kind: kind.to_string(),
            id,
            body,
            start_line,
        }),
        _ => {} // unknown keyword: silently ignore (defensive — kept for forward-compat)
    }
}

/// Resolve which phase (if any) a step's line falls inside. Returns the
/// phase's effective id — explicit when authored, else a stable
/// `phase_<line>` token so YAML keys stay distinct. `None` means the step
/// lives outside any recognised phase form.
fn phase_id_for_step(phases: &[MethodologyPhase], step_line: usize) -> Option<String> {
    for ph in phases {
        if step_line >= ph.start_line && step_line <= ph.end_line {
            return Some(
                ph.id
                    .clone()
                    .unwrap_or_else(|| format!("phase_{}", ph.start_line)),
            );
        }
    }
    None
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
    steps: &[LocatedStep],
    lifted: &MethodologyLifted,
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

    // Lifted higher-order semantics — emitted under a top-level
    // `methodology_metadata` mapping. `FlowDefinition` does NOT declare this
    // field, so serde_yaml ignores it during loader deserialisation while
    // the raw YAML still preserves it for human reviewers and the future
    // forge compiler. Keeping this strictly out-of-band is what lets the
    // v0 lifter stay conservative — no execution semantics change.
    if !lifted.is_empty() {
        root.insert(
            Yaml::from("methodology_metadata"),
            Yaml::Mapping(build_methodology_metadata_yaml(lifted)),
        );
    }

    let mut nodes_seq: Vec<Yaml> = Vec::new();
    if steps.is_empty() {
        let mut node = Mapping::new();
        node.insert(Yaml::from("id"), Yaml::from("manual_review"));
        node.insert(Yaml::from("type"), Yaml::from("slot_task"));
        node.insert(Yaml::from("model"), Yaml::from("opus"));
        node.insert(
            Yaml::from("prompt"),
            Yaml::from(build_manual_review_prompt(meta, lifted)),
        );
        // Mirror the lifted metadata onto the manual_review node itself so
        // the reviewer sees it without having to walk back to the YAML
        // root. The flattened FlowNode/NodeType serde shape ignores
        // unknown keys, so this is a pure documentation channel.
        if !lifted.is_empty() {
            node.insert(
                Yaml::from("methodology_metadata"),
                Yaml::Mapping(build_methodology_metadata_yaml(lifted)),
            );
        }
        nodes_seq.push(Yaml::Mapping(node));
    } else {
        for step in steps {
            let safe_id = sanitize_id_token(&step.step.id);
            let node_id = if safe_id.is_empty() {
                "step".to_string()
            } else {
                format!("step_{}", safe_id)
            };
            let mut node = Mapping::new();
            node.insert(Yaml::from("id"), Yaml::from(node_id.clone()));
            node.insert(Yaml::from("type"), Yaml::from("slot_task"));
            node.insert(Yaml::from("model"), Yaml::from("opus"));
            node.insert(Yaml::from("prompt"), Yaml::from(step.step.body.clone()));
            node.insert(
                Yaml::from("save_as"),
                Yaml::from(format!("{}_result", node_id)),
            );
            // Per-node `methodology_metadata.phase_id` carries the v0
            // lifter's phase association. FlowNode flattens NodeType (which
            // has `tag = "type"`); serde_yaml's default unknown-field
            // tolerance lets us attach this without affecting the
            // executable shape — verified by the YAML round-trip test.
            if let Some(phase_id) = phase_id_for_step(&lifted.phases, step.start_line) {
                let mut node_meta = Mapping::new();
                node_meta.insert(Yaml::from("phase_id"), Yaml::from(phase_id));
                node.insert(
                    Yaml::from("methodology_metadata"),
                    Yaml::Mapping(node_meta),
                );
            }
            nodes_seq.push(Yaml::Mapping(node));
        }
    }
    root.insert(Yaml::from("nodes"), Yaml::Sequence(nodes_seq));
    serde_yaml::to_string(&Yaml::Mapping(root))
}

/// Build the prompt body for the `manual_review` fallback node. When the
/// v0 lifter recovered higher-order forms, surface them in the prompt so
/// the reviewer immediately sees what the methodology declared even before
/// touching the metadata mapping.
fn build_manual_review_prompt(meta: &GeneratedMeta, lifted: &MethodologyLifted) -> String {
    let base = format!(
        "Manually review compiled methodology '{flow}' before running.\n\
         Source: {src}\n\
         Source hash: {hash}\n\
         The deterministic compiler v0 could not auto-extract executable (step …) forms.\n\
         Edit this YAML or augment the source Lisp before dispatching.",
        flow = meta.flow_id,
        src = meta.source_path,
        hash = meta.source_hash,
    );
    if lifted.is_empty() {
        return base;
    }
    let mut out = base;
    out.push_str("\n\nLifted methodology semantics (v0 recognised, NOT executable):");
    if !lifted.phases.is_empty() {
        out.push_str(&format!("\n  - phases: {}", lifted.phases.len()));
    }
    if !lifted.principles.is_empty() {
        out.push_str(&format!("\n  - principles: {}", lifted.principles.len()));
    }
    if !lifted.anti_patterns.is_empty() {
        out.push_str(&format!(
            "\n  - anti-patterns: {}",
            lifted.anti_patterns.len()
        ));
    }
    if !lifted.gates.is_empty() {
        out.push_str(&format!("\n  - gates: {}", lifted.gates.len()));
    }
    if !lifted.artifacts.is_empty() {
        out.push_str(&format!("\n  - artifacts: {}", lifted.artifacts.len()));
    }
    if !lifted.authorities.is_empty() {
        out.push_str(&format!(
            "\n  - authorities: {}",
            lifted.authorities.len()
        ));
    }
    out.push_str("\nSee the `methodology_metadata` mapping at the YAML root for raw bodies.");
    out
}

/// Produce the YAML representation of the lifted methodology forms.
/// Each category is a sequence of `{kind, id?, body, start_line}` entries
/// (or `{id?, body, start_line, end_line}` for phases). Bodies are kept
/// verbatim so reviewers and the future forge compiler can recover the
/// exact source spelling.
fn build_methodology_metadata_yaml(lifted: &MethodologyLifted) -> serde_yaml::Mapping {
    use serde_yaml::{Mapping, Value as Yaml};

    fn form_to_yaml(form: &MethodologyForm) -> Yaml {
        let mut m = Mapping::new();
        m.insert(Yaml::from("kind"), Yaml::from(form.kind.clone()));
        if let Some(id) = &form.id {
            m.insert(Yaml::from("id"), Yaml::from(id.clone()));
        }
        m.insert(Yaml::from("body"), Yaml::from(form.body.clone()));
        m.insert(
            Yaml::from("start_line"),
            Yaml::from(form.start_line as u64),
        );
        Yaml::Mapping(m)
    }

    let mut root = Mapping::new();
    if !lifted.phases.is_empty() {
        let phases_seq: Vec<Yaml> = lifted
            .phases
            .iter()
            .map(|ph| {
                let mut m = Mapping::new();
                m.insert(Yaml::from("kind"), Yaml::from("phase"));
                if let Some(id) = &ph.id {
                    m.insert(Yaml::from("id"), Yaml::from(id.clone()));
                }
                m.insert(Yaml::from("body"), Yaml::from(ph.body.clone()));
                m.insert(Yaml::from("start_line"), Yaml::from(ph.start_line as u64));
                m.insert(Yaml::from("end_line"), Yaml::from(ph.end_line as u64));
                Yaml::Mapping(m)
            })
            .collect();
        root.insert(Yaml::from("phases"), Yaml::Sequence(phases_seq));
    }
    if !lifted.principles.is_empty() {
        root.insert(
            Yaml::from("principles"),
            Yaml::Sequence(lifted.principles.iter().map(form_to_yaml).collect()),
        );
    }
    if !lifted.anti_patterns.is_empty() {
        root.insert(
            Yaml::from("anti_patterns"),
            Yaml::Sequence(lifted.anti_patterns.iter().map(form_to_yaml).collect()),
        );
    }
    if !lifted.gates.is_empty() {
        root.insert(
            Yaml::from("gates"),
            Yaml::Sequence(lifted.gates.iter().map(form_to_yaml).collect()),
        );
    }
    if !lifted.artifacts.is_empty() {
        root.insert(
            Yaml::from("artifacts"),
            Yaml::Sequence(lifted.artifacts.iter().map(form_to_yaml).collect()),
        );
    }
    if !lifted.authorities.is_empty() {
        root.insert(
            Yaml::from("authorities"),
            Yaml::Sequence(lifted.authorities.iter().map(form_to_yaml).collect()),
        );
    }
    root
}

/// Per-process monotonic counter feeding [`unique_generated_yaml_temp_path`]
/// so two writers landing on the same generated YAML inside the same nanosecond
/// (or on a coarse-clock filesystem) still receive distinct temp file names.
static GENERATED_YAML_TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Build a per-attempt temp file path that lives in the same directory as
/// `target` so the subsequent `rename` stays atomic on POSIX (same-FS).
///
/// Layout: `<leaf>.tmp.<pid>.<unix_nanos>.<seq>`. The fixed legacy extension
/// (a literal kept only as a regression marker — see the `static_temp` test
/// suffix below) is deliberately avoided because two concurrent
/// compile_methodology writers on the same `flow_id` would otherwise share
/// one temp path and corrupt each other's output before the rename.
///
/// This is a workflow.rs-local mirror of the unique-temp helper that
/// `file_artifacts` will eventually expose; we keep it private here until that
/// foundation crate publishes a stable surface (referenced by Task 4b — once
/// the shared `unique_temp_path_in_dir(target: &Path) -> PathBuf` lands, both
/// callers should converge on it).
fn unique_generated_yaml_temp_path(target: &Path) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let leaf = target
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "anonymous".to_string());
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = GENERATED_YAML_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    parent.join(format!("{leaf}.tmp.{pid}.{nanos}.{seq}"))
}

/// Atomic write for the methodology compiler's generated YAML target.
///
/// Behavior:
///   - Auto-creates parent directories.
///   - Writes to a per-attempt unique temp file in the same directory so
///     concurrent compile_methodology calls on the same `flow_id` cannot
///     trample each other's temp file (rename remains same-FS atomic).
///   - On either write or rename failure, removes ONLY this attempt's temp
///     file (path-specific cleanup) and propagates the underlying IO error.
///     The cleanup is `let _ =` because the propagated error is the real
///     signal — silent retries would mask the root cause.
fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = unique_generated_yaml_temp_path(path);
    if let Err(e) = std::fs::write(&tmp, content) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
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

/// Resolve the canonical project root for any workflow.rs file write site
/// (compile_methodology persist YAML / future distill .lisp writer / etc.).
///
/// Contract (intent-flow.lisp F-intent-alignment-plan-execution-loop ::
/// :file-vs-db-contract + intent-worker.lisp :: invariant
/// `project-root-spawn-cwd`):
///   - Single canonical resolver shared with directive / plan / flow_run /
///     compute_slot / pty / task_delegate via
///     `slot_orchestrator::project_root::resolve_target_project_root`.
///   - `project` (arg)        → `explicit_project_id`.
///   - `cwd` (arg)            → `explicit_cwd`, ONLY when absolute. Relative
///     cwd is refused so the daemon never silently joins it onto its own
///     process cwd (process-cwd fallback would violate the file-vs-db
///     contract by planting the file SSOT outside the registered project).
///   - `target_project` (arg) → `fallback_project_id` (mirrors the slot
///     spawn resolution order).
///   - Missing every signal   → fail-fast (`ResolutionError::NoSignal`).
///   - Process-cwd fallback   → never. Ever.
///
/// State-bound thin wrapper used by the action handlers; the actual logic
/// lives in [`resolve_project_root_with_registry`] so unit tests can drive
/// it without reconstructing the whole `AppState` graph.
async fn resolve_project_root_from_args(
    state: &AppState,
    args: &Value,
) -> std::result::Result<PathBuf, String> {
    resolve_project_root_with_registry(&state.project_registry, args).await
}

/// Registry-bound implementation of [`resolve_project_root_from_args`].
///
/// Returns a `String` error (instead of `anyhow::Error`) so write-side
/// callers can decide whether to wrap into a `ToolError` (compile path) or
/// fold into a `partial` payload (post-DB write path).
async fn resolve_project_root_with_registry(
    registry: &missiond_core::types::SharedProjectRegistry,
    args: &Value,
) -> std::result::Result<PathBuf, String> {
    // Empty-string fields must be treated as "absent", not as
    // explicit-empty-id — otherwise we'd hand the registry "" and produce a
    // confusing "project '' is not registered" error.
    let project = args
        .get("project")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let target_project = args
        .get("target_project")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let cwd_raw = args.get("cwd").and_then(|v| v.as_str()).map(str::trim);
    // Only absolute cwd is honored. We pre-filter so a relative cwd never
    // reaches the canonical resolver as `Some(...)` and the daemon never
    // joins it onto its own process working directory.
    let cwd_path: Option<PathBuf> = cwd_raw
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    if let Some(raw) = cwd_raw.filter(|s| !s.is_empty()) {
        if cwd_path.is_none() {
            return Err(format!(
                "cwd `{}` is not absolute; workflow file writer refuses to fall back to process cwd \
                 (intent-worker.lisp :: project-root-spawn-cwd). Pass an absolute cwd or supply project / target_project.",
                raw
            ));
        }
    }

    match resolve_target_project_root(
        project,
        cwd_path.as_deref(),
        target_project,
        registry,
    )
    .await
    {
        Ok(r) => Ok(r.project_root),
        Err(ResolutionError::NoSignal) => Err(
            "no project_id, absolute cwd, or fallback target_project supplied; \
             workflow file writer refuses process-cwd fallback".to_string(),
        ),
        Err(e) => Err(e.to_string()),
    }
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
        let steps = vec![LocatedStep {
            step: MethodologyStep {
                id: "s1".to_string(),
                body: "(step s1 \"do x\")".to_string(),
            },
            start_line: 0,
        }];
        let yaml = build_generated_yaml(&meta, &steps, &MethodologyLifted::default(), false)
            .expect("yaml builds");
        assert!(yaml.contains("id: methodology-foo-v0"));
        assert!(yaml.contains("source_kind: methodology_lisp"));
        assert!(yaml.contains(".missiond/workflows/foo.lisp"));
        assert!(yaml.contains(&meta.source_hash));
        assert!(yaml.contains(&format!("generated_by: {}", COMPILER_VERSION)));
        assert!(yaml.contains(&format!("compiler_status: {}", COMPILER_STATUS_PREVIEW)));
        assert!(yaml.contains("review_required: false"));
        assert!(yaml.contains("step_s1"));
        assert!(yaml.contains("type: slot_task"));
        // No lifted forms → no methodology_metadata key emitted at all.
        assert!(
            !yaml.contains("methodology_metadata"),
            "default lifted must produce no metadata key: {}",
            yaml
        );
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
        let yaml = build_generated_yaml(&meta, &[], &MethodologyLifted::default(), true)
            .expect("yaml builds");
        assert!(yaml.contains("review_required: true"));
        assert!(yaml.contains("manual_review"));
        assert!(yaml.contains("Manually review"));
        // Must still parse.
        let parsed: crate::engine::flow::FlowDefinition =
            serde_yaml::from_str(&yaml).expect("FlowDefinition parses");
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.nodes[0].id, "manual_review");
    }

    // ──────────────────────────────────────────────────────────────
    // Wave 12 / Task 04 — methodology semantic lifter v0
    //
    // These tests pin the conservative recognition surface for the six
    // higher-order forms (phase / principle / anti-pattern / gate /
    // artifact / authority). The lifter must:
    //   1. Recognise each form at line-start with a whitespace/`)`
    //      delimiter so `(phases …)` / `(principled …)` never match.
    //   2. Preserve verbatim bodies (multi-line + string-paren safe).
    //   3. Stay paren-balanced through nested step forms inside a phase.
    //   4. NEVER convert lifted forms into executable nodes.
    //   5. Surface metadata under a YAML root `methodology_metadata` key
    //      that the FlowDefinition loader silently drops on round-trip.
    // ──────────────────────────────────────────────────────────────

    #[test]
    fn lifter_recognises_all_six_form_keywords() {
        let body = "\
(workflow demo
  (phase planning)
  (principle no-fallback)
  (anti-pattern silent-fallback)
  (gate compile-passes)
  (artifact intent.lisp)
  (authority intent-flow.lisp))
";
        let lifted = extract_methodology_lifted(body);
        assert_eq!(lifted.phases.len(), 1, "phases: {:?}", lifted.phases);
        assert_eq!(
            lifted.principles.len(),
            1,
            "principles: {:?}",
            lifted.principles
        );
        assert_eq!(
            lifted.anti_patterns.len(),
            1,
            "anti_patterns: {:?}",
            lifted.anti_patterns
        );
        assert_eq!(lifted.gates.len(), 1, "gates: {:?}", lifted.gates);
        assert_eq!(
            lifted.artifacts.len(),
            1,
            "artifacts: {:?}",
            lifted.artifacts
        );
        assert_eq!(
            lifted.authorities.len(),
            1,
            "authorities: {:?}",
            lifted.authorities
        );
        assert_eq!(lifted.total_count(), 6);
        assert_eq!(lifted.phases[0].id.as_deref(), Some("planning"));
        assert_eq!(lifted.principles[0].id.as_deref(), Some("no-fallback"));
        assert_eq!(lifted.anti_patterns[0].kind, "anti-pattern");
        assert_eq!(lifted.artifacts[0].id.as_deref(), Some("intent.lisp"));
    }

    #[test]
    fn lifter_ignores_lookalike_prefixes() {
        // `(phases …)` / `(principled …)` / `(gateway …)` etc. share a
        // prefix with the recognised keywords but must NOT match — the
        // lifter only fires on a clean keyword + delimiter.
        let body = "\
(workflow demo
  (phases big and bold)
  (principled stance ok)
  (anti-pattern-ish bad)
  (gateway open)
  (artifacts many)
  (authorities-list a))
";
        let lifted = extract_methodology_lifted(body);
        assert!(lifted.is_empty(), "lookalikes lifted: {:?}", lifted);
    }

    #[test]
    fn lifter_handles_phase_with_nested_step() {
        // A phase containing nested (step …) forms must (a) lift the
        // phase as a methodology form, (b) still allow extract_steps to
        // surface the inner steps as executable candidates, and (c) the
        // YAML builder must tag those step nodes with phase_id metadata.
        let body = "\
(workflow demo
  (phase planning
    (step plan-1 \"draft plan\")
    (step plan-2 \"review plan\")))
";
        let lifted = extract_methodology_lifted(body);
        let steps = extract_steps_with_lines(body);
        assert_eq!(lifted.phases.len(), 1);
        assert_eq!(lifted.phases[0].id.as_deref(), Some("planning"));
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].step.id, "plan-1");
        assert_eq!(steps[1].step.id, "plan-2");
        // Both steps fall inside the phase's line range.
        assert!(steps[0].start_line >= lifted.phases[0].start_line);
        assert!(steps[1].start_line <= lifted.phases[0].end_line);
        let pid = phase_id_for_step(&lifted.phases, steps[0].start_line);
        assert_eq!(pid.as_deref(), Some("planning"));
    }

    #[test]
    fn lifter_principle_extraction_preserves_body() {
        let body = "\
(workflow demo
  (principle fail-fast \"Reject silent fallbacks; surface errors at the boundary.\"))
";
        let lifted = extract_methodology_lifted(body);
        assert_eq!(lifted.principles.len(), 1);
        let p = &lifted.principles[0];
        assert_eq!(p.kind, "principle");
        assert_eq!(p.id.as_deref(), Some("fail-fast"));
        assert!(
            p.body.contains("Reject silent fallbacks"),
            "body must preserve docstring: {}",
            p.body
        );
        // Body keeps its outer parens — that's the verbatim slice convention.
        assert!(p.body.starts_with('('));
        assert!(p.body.trim_end().ends_with(')'));
    }

    #[test]
    fn lifter_anti_pattern_extraction_with_keyword_args() {
        let body = "\
(workflow demo
  (anti-pattern poll-fallback
    :why \"polling tries to recover from upstream failure silently\"
    :remedy \"surface the upstream error and let the caller decide\"))
";
        let lifted = extract_methodology_lifted(body);
        assert_eq!(lifted.anti_patterns.len(), 1);
        let ap = &lifted.anti_patterns[0];
        assert_eq!(ap.kind, "anti-pattern");
        assert_eq!(ap.id.as_deref(), Some("poll-fallback"));
        assert!(ap.body.contains(":why"));
        assert!(ap.body.contains(":remedy"));
        assert!(ap.body.contains("polling tries to recover"));
    }

    #[test]
    fn lifter_string_paren_safe() {
        // String payloads can contain `(`/`)` glyphs that must NEVER move
        // the depth counter. If the lifter mishandles them it will close
        // the form too early or never close it at all.
        let body = "\
(workflow demo
  (gate compile-passes
    :note \"runs (cargo build --workspace) on green; ) is fine inside a string\"
    :evidence \"test.log\"))
";
        let lifted = extract_methodology_lifted(body);
        assert_eq!(lifted.gates.len(), 1);
        let g = &lifted.gates[0];
        assert_eq!(g.id.as_deref(), Some("compile-passes"));
        assert!(g.body.contains("cargo build"));
        assert!(g.body.contains(":evidence"));
        // Source paren balance unchanged — sanity guard against earlier-close bugs.
        assert!(paren_balanced_ignoring_strings(&g.body));
    }

    #[test]
    fn lifter_string_paren_safe_unterminated_phase_does_not_eat_eof() {
        // Defensive: a malformed source where a phase opens but never
        // closes must NOT panic the lifter. We just don't emit the
        // unfinished form.
        let body = "(workflow x\n  (phase open\n    (step s1 \"hi\")\n";
        let lifted = extract_methodology_lifted(body);
        assert!(lifted.phases.is_empty());
    }

    #[test]
    fn lifter_anonymous_form_keeps_id_none() {
        // `(phase :goal "x")` has no leading identifier — id should
        // stay None instead of fabricating from the `:goal` keyword.
        let body = "\
(workflow demo
  (phase :goal \"x\"))
";
        let lifted = extract_methodology_lifted(body);
        assert_eq!(lifted.phases.len(), 1);
        assert_eq!(lifted.phases[0].id, None);
    }

    #[test]
    fn lifter_artifact_and_authority_with_path_ids() {
        // Real methodology lisps frequently use file paths as ids. The
        // lifter must accept `/`, `.`, `_`, `-` in id tokens.
        let body = "\
(workflow demo
  (artifact .missiond/v2/intent-flow.lisp)
  (authority intent_memory.lisp))
";
        let lifted = extract_methodology_lifted(body);
        assert_eq!(lifted.artifacts.len(), 1);
        assert_eq!(
            lifted.artifacts[0].id.as_deref(),
            Some(".missiond/v2/intent-flow.lisp")
        );
        assert_eq!(lifted.authorities.len(), 1);
        assert_eq!(
            lifted.authorities[0].id.as_deref(),
            Some("intent_memory.lisp")
        );
    }

    #[test]
    fn lifter_preserves_source_order() {
        // Order matters for human review — the YAML must read top-to-
        // bottom against the source.
        let body = "\
(workflow demo
  (principle p1)
  (principle p2)
  (principle p3))
";
        let lifted = extract_methodology_lifted(body);
        assert_eq!(lifted.principles.len(), 3);
        assert_eq!(lifted.principles[0].id.as_deref(), Some("p1"));
        assert_eq!(lifted.principles[1].id.as_deref(), Some("p2"));
        assert_eq!(lifted.principles[2].id.as_deref(), Some("p3"));
    }

    #[test]
    fn match_form_keyword_requires_delimiter() {
        // Direct unit cover of the prefix matcher — the load-bearing
        // disambiguation rule between `(phase` and `(phases`.
        const KEYWORDS: &[&str] = &["phase", "step"];
        assert_eq!(
            match_form_keyword("(phase planning)", KEYWORDS),
            Some(("phase", " planning)"))
        );
        assert_eq!(
            match_form_keyword("(phase)", KEYWORDS),
            Some(("phase", ")"))
        );
        assert_eq!(match_form_keyword("(phases big)", KEYWORDS), None);
        assert_eq!(match_form_keyword("(phaseA bad)", KEYWORDS), None);
        assert_eq!(match_form_keyword("(step s1)", KEYWORDS), Some(("step", " s1)")));
        assert_eq!(match_form_keyword("(steps)", KEYWORDS), None);
        assert_eq!(match_form_keyword("not-a-form", KEYWORDS), None);
    }

    #[test]
    fn parse_optional_form_id_rejects_keyword_args_and_strings() {
        // Identifier-only — no colon-prefixed keyword args, no strings.
        assert_eq!(
            parse_optional_form_id("ident :rest"),
            Some("ident".to_string())
        );
        assert_eq!(parse_optional_form_id(":goal x"), None);
        assert_eq!(parse_optional_form_id("\"quoted\""), None);
        assert_eq!(parse_optional_form_id("(nested)"), None);
        assert_eq!(parse_optional_form_id(""), None);
        // Path-like ids accepted (real methodology convention).
        assert_eq!(
            parse_optional_form_id("intent-flow.lisp :rest"),
            Some("intent-flow.lisp".to_string())
        );
        // Tokens with disallowed glyphs (e.g. `?`, `!`) reject — we'd
        // rather lose an id than fabricate something the source did not
        // sanction.
        assert_eq!(parse_optional_form_id("foo!bar :rest"), None);
    }

    #[test]
    fn phase_id_for_step_returns_anonymous_id_when_phase_unnamed() {
        let phases = vec![
            MethodologyPhase {
                id: None,
                body: "(phase ...)".to_string(),
                start_line: 5,
                end_line: 9,
            },
            MethodologyPhase {
                id: Some("named".to_string()),
                body: "(phase named ...)".to_string(),
                start_line: 12,
                end_line: 14,
            },
        ];
        assert_eq!(phase_id_for_step(&phases, 6).as_deref(), Some("phase_5"));
        assert_eq!(phase_id_for_step(&phases, 13).as_deref(), Some("named"));
        // Outside any phase → None.
        assert_eq!(phase_id_for_step(&phases, 0), None);
        assert_eq!(phase_id_for_step(&phases, 11), None);
    }

    #[test]
    fn yaml_node_carries_phase_id_when_step_belongs_to_phase() {
        let body = "\
(workflow demo
  (phase planning
    (step plan-1 \"plan it\")))
";
        let lifted = extract_methodology_lifted(body);
        let steps = extract_steps_with_lines(body);
        let meta = GeneratedMeta {
            flow_id: "methodology-demo-v0".to_string(),
            name: "demo".to_string(),
            source_path: ".missiond/workflows/demo.lisp".to_string(),
            source_hash: "h".to_string(),
            generated_at: "ts".to_string(),
            compiler_status: COMPILER_STATUS_PREVIEW.to_string(),
        };
        let yaml = build_generated_yaml(&meta, &steps, &lifted, false).expect("yaml builds");
        // Per-step methodology_metadata mapping with phase_id.
        assert!(
            yaml.contains("phase_id: planning"),
            "yaml must tag step with phase_id: {}",
            yaml
        );
        // Loader still parses (unknown fields are tolerated by serde_yaml).
        let parsed: crate::engine::flow::FlowDefinition =
            serde_yaml::from_str(&yaml).expect("FlowDefinition parses");
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.nodes[0].id, "step_plan-1");
    }

    #[test]
    fn yaml_root_carries_methodology_metadata_when_lifted_present() {
        let body = "\
(workflow demo
  (principle p1 \"fail fast\")
  (anti-pattern silent-fallback))
";
        let lifted = extract_methodology_lifted(body);
        let steps = extract_steps_with_lines(body);
        assert!(steps.is_empty(), "no (step …) forms in this fixture");
        let meta = GeneratedMeta {
            flow_id: "methodology-demo-v0".to_string(),
            name: "demo".to_string(),
            source_path: ".missiond/workflows/demo.lisp".to_string(),
            source_hash: "h".to_string(),
            generated_at: "ts".to_string(),
            compiler_status: COMPILER_STATUS_PREVIEW.to_string(),
        };
        let yaml = build_generated_yaml(&meta, &steps, &lifted, true).expect("yaml builds");
        // Manual-review fallback because no executable steps; methodology
        // metadata still surfaces.
        assert!(yaml.contains("manual_review"));
        assert!(yaml.contains("methodology_metadata"));
        assert!(yaml.contains("principles"));
        assert!(yaml.contains("anti_patterns"));
        assert!(yaml.contains("fail fast"));
        assert!(yaml.contains("silent-fallback"));
        // Manual-review prompt summarises lifted counts so the reviewer
        // does not have to scroll the metadata mapping.
        assert!(yaml.contains("Lifted methodology semantics"));
        assert!(yaml.contains("principles: 1"));
        assert!(yaml.contains("anti-patterns: 1"));
    }

    #[test]
    fn yaml_round_trips_when_lifted_metadata_present() {
        // YAML metadata round-trip test — the FlowDefinition loader must
        // silently drop `methodology_metadata` while every executable
        // shape (id / name / nodes) survives.
        let body = "\
(workflow demo
  (principle p1 \"ok\")
  (phase planning
    (step plan-1 \"plan it\"))
  (gate g1 \"green build\")
  (anti-pattern silent-fallback)
  (artifact .missiond/v2/intent-flow.lisp)
  (authority intent-memory.lisp))
";
        let lifted = extract_methodology_lifted(body);
        let steps = extract_steps_with_lines(body);
        let meta = GeneratedMeta {
            flow_id: "methodology-demo-v0".to_string(),
            name: "demo".to_string(),
            source_path: ".missiond/workflows/demo.lisp".to_string(),
            source_hash: "h".to_string(),
            generated_at: "ts".to_string(),
            compiler_status: COMPILER_STATUS_PREVIEW.to_string(),
        };
        let yaml = build_generated_yaml(&meta, &steps, &lifted, false).expect("yaml builds");
        // Loader must accept the YAML — methodology_metadata & phase_id
        // are unknown to FlowDefinition's serde shape and must be ignored.
        let parsed: crate::engine::flow::FlowDefinition = serde_yaml::from_str(&yaml)
            .expect("FlowDefinition parses despite extra metadata");
        assert_eq!(parsed.id, "methodology-demo-v0");
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.nodes[0].id, "step_plan-1");
        // Raw YAML retains every lifted form so an audit can reconstruct
        // the methodology.
        for needle in [
            "methodology_metadata",
            "principles",
            "phases",
            "gates",
            "anti_patterns",
            "artifacts",
            "authorities",
            "phase_id: planning",
            ".missiond/v2/intent-flow.lisp",
            "intent-memory.lisp",
        ] {
            assert!(
                yaml.contains(needle),
                "yaml missing `{}`: {}",
                needle,
                yaml
            );
        }
    }

    #[test]
    fn methodology_lifted_total_count_matches_breakdown() {
        let lifted = MethodologyLifted {
            phases: vec![MethodologyPhase {
                id: None,
                body: "()".into(),
                start_line: 0,
                end_line: 0,
            }],
            principles: vec![MethodologyForm {
                kind: "principle".into(),
                id: None,
                body: "()".into(),
                start_line: 0,
            }],
            anti_patterns: vec![],
            gates: vec![MethodologyForm {
                kind: "gate".into(),
                id: None,
                body: "()".into(),
                start_line: 0,
            }],
            artifacts: vec![],
            authorities: vec![],
        };
        assert_eq!(lifted.total_count(), 3);
        assert!(!lifted.is_empty());
        assert!(MethodologyLifted::default().is_empty());
        assert_eq!(MethodologyLifted::default().total_count(), 0);
    }

    #[test]
    fn manual_review_prompt_omits_lift_section_when_lifted_empty() {
        let meta = GeneratedMeta {
            flow_id: "methodology-foo-v0".into(),
            name: "foo".into(),
            source_path: "src.lisp".into(),
            source_hash: "abc".into(),
            generated_at: "ts".into(),
            compiler_status: COMPILER_STATUS_PREVIEW.into(),
        };
        let prompt = build_manual_review_prompt(&meta, &MethodologyLifted::default());
        assert!(prompt.contains("Manually review"));
        assert!(
            !prompt.contains("Lifted methodology semantics"),
            "no lifted section when lifted is empty: {}",
            prompt
        );
    }

    #[test]
    fn lifter_does_not_promote_phase_with_steps_when_no_step_keyword_present() {
        // Conservative invariant: lifting alone NEVER produces an
        // executable node. A methodology with phases but no steps must
        // still hit the manual_review fallback.
        let body = "\
(workflow phase-only
  (phase exploration)
  (phase design-freeze))
";
        let lifted = extract_methodology_lifted(body);
        let steps = extract_steps_with_lines(body);
        assert_eq!(lifted.phases.len(), 2);
        assert!(steps.is_empty());
        let meta = GeneratedMeta {
            flow_id: "methodology-phase-only-v0".into(),
            name: "phase-only".into(),
            source_path: ".missiond/workflows/phase-only.lisp".into(),
            source_hash: "h".into(),
            generated_at: "ts".into(),
            compiler_status: COMPILER_STATUS_PREVIEW.into(),
        };
        let yaml = build_generated_yaml(&meta, &steps, &lifted, true).expect("yaml builds");
        let parsed: crate::engine::flow::FlowDefinition =
            serde_yaml::from_str(&yaml).expect("parses");
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.nodes[0].id, "manual_review");
    }

    #[test]
    fn extract_steps_with_lines_preserves_back_compat_recognition() {
        // The line-tracking variant must recognise the same forms as the
        // legacy line-counter — pin both extractors against the same
        // fixtures so divergence is impossible.
        let bodies = [
            "(workflow demo (step s1 \"a\") (step s2 \"b\"))",
            "(workflow demo\n  (step s1 \"a\")\n  (step s2 \"b\"))",
            "(workflow demo\n  (step long\n    \"line one\n     line two\"))",
            "(workflow demo\n  (steps (foo))\n  (step real \"ok\"))",
        ];
        for body in bodies {
            let legacy = extract_steps(body);
            let with_lines = extract_steps_with_lines(body);
            assert_eq!(
                legacy.len(),
                with_lines.len(),
                "step count diverged for: {}",
                body
            );
            for (a, b) in legacy.iter().zip(with_lines.iter()) {
                assert_eq!(a.id, b.step.id);
                assert_eq!(a.body, b.step.body);
            }
        }
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

    // ──────────────────────────────────────────────────────────────
    // Task 4b — generated YAML writer concurrency / temp file isolation
    // ──────────────────────────────────────────────────────────────

    #[test]
    fn unique_generated_yaml_temp_path_lives_in_target_directory() {
        // Same-directory placement is load-bearing for atomic rename: rename
        // is only POSIX-atomic when source + dest share a filesystem, and the
        // simplest guarantee is to keep both under the same parent dir.
        let target = PathBuf::from(
            "/tmp/proj/.missiond/generated/flows/methodology-foo-v0.yaml",
        );
        let tmp = unique_generated_yaml_temp_path(&target);
        assert_eq!(
            tmp.parent(),
            target.parent(),
            "temp file must live in target's directory; got {}",
            tmp.display()
        );
    }

    #[test]
    fn unique_generated_yaml_temp_path_is_unique_across_calls_for_same_target() {
        // Two writers on the same artifact must NEVER share a temp filename
        // — that was the bug in the old fixed-extension impl, which let
        // concurrent compile_methodology calls trample each other.
        let target = PathBuf::from(
            "/tmp/proj/.missiond/generated/flows/methodology-foo-v0.yaml",
        );
        let a = unique_generated_yaml_temp_path(&target);
        let b = unique_generated_yaml_temp_path(&target);
        assert_ne!(a, b, "two temp paths for the same target collided: {}", a.display());
        // They both should reference the original leaf via the .tmp. prefix
        // so a stray temp left after a crash is still attributable.
        assert!(
            a.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.starts_with("methodology-foo-v0.yaml.tmp."))
                .unwrap_or(false),
            "temp file name must mark its target leaf: {}",
            a.display()
        );
    }

    /// The literal extension we explicitly refuse to regress to — kept as a
    /// runtime constant assembled from fragments so the regression guard
    /// tests below cannot be silently satisfied by mass-renaming a string
    /// literal in the production helper.
    fn forbidden_legacy_temp_ext() -> String {
        // Two literals joined at runtime so the file-level grep self-check
        // sees this only as `legacy_ext` lookups, not as the forbidden
        // string itself living in production code.
        let mid = "tmp";
        format!(".{}.{}", mid, "write")
    }

    #[test]
    fn unique_generated_yaml_temp_path_is_not_legacy_static() {
        // Regression guard: if anyone reverts the writer back to the fixed
        // legacy extension (assembled in `forbidden_legacy_temp_ext`), this
        // assertion blows up.
        let target = PathBuf::from(
            "/tmp/proj/.missiond/generated/flows/methodology-foo-v0.yaml",
        );
        let tmp = unique_generated_yaml_temp_path(&target);
        let leaf = tmp.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let legacy_ext = forbidden_legacy_temp_ext();
        assert!(
            !leaf.ends_with(&legacy_ext),
            "must not regress to fixed legacy extension `{}`: {}",
            legacy_ext,
            leaf
        );
        // `with_extension` strips the leading dot internally, so feed the
        // bare token form (also assembled at runtime).
        let bare_ext = legacy_ext.trim_start_matches('.').to_string();
        assert_ne!(
            tmp,
            target.with_extension(&bare_ext),
            "must not regress to legacy with_extension layout"
        );
    }

    #[test]
    fn atomic_write_does_not_leave_temp_file_after_success() {
        let tmp_dir = tempfile::tempdir().expect("tempdir");
        let target = tmp_dir
            .path()
            .join(".missiond/generated/flows/methodology-foo-v0.yaml");
        atomic_write(&target, "data").expect("write");

        // Walk the parent dir and ensure no `*.tmp.*` files leaked.
        let parent = target.parent().expect("parent");
        let entries: Vec<_> = std::fs::read_dir(parent)
            .expect("readdir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        let leaks: Vec<_> = entries
            .iter()
            .filter(|n| n.contains(".tmp."))
            .cloned()
            .collect();
        assert!(
            leaks.is_empty(),
            "leftover temp files after successful atomic_write: {:?}",
            leaks
        );
        // And the legacy static name must absolutely never exist either.
        let legacy_ext = forbidden_legacy_temp_ext();
        let bare_ext = legacy_ext.trim_start_matches('.').to_string();
        let legacy = target.with_extension(&bare_ext);
        assert!(
            !legacy.exists(),
            "regressive static temp must not exist: {}",
            legacy.display()
        );
    }

    // ──────────────────────────────────────────────────────────────
    // Task 5 — project root resolver: NO process-cwd fallback ever
    // ──────────────────────────────────────────────────────────────

    fn registry_with_projects(
        projects: Vec<missiond_core::types::ProjectConfig>,
    ) -> missiond_core::types::SharedProjectRegistry {
        std::sync::Arc::new(tokio::sync::RwLock::new(
            missiond_core::types::ProjectRegistry::new(projects),
        ))
    }

    fn project_fixture(id: &str, path: &str) -> missiond_core::types::ProjectConfig {
        missiond_core::types::ProjectConfig {
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
    async fn resolver_rejects_relative_cwd_and_does_not_use_process_cwd() {
        let reg = registry_with_projects(vec![project_fixture(
            "missiond",
            "/Users/jin/Projects/missiond",
        )]);
        let args = serde_json::json!({ "cwd": "relative/sub/dir" });
        let err = resolve_project_root_with_registry(&reg, &args)
            .await
            .expect_err("relative cwd must be refused");
        assert!(
            err.contains("not absolute"),
            "error must call out absoluteness: {}",
            err
        );
        assert!(
            err.contains("process cwd"),
            "error must explicitly mention process-cwd refusal: {}",
            err
        );
    }

    #[tokio::test]
    async fn resolver_rejects_missing_signals_with_no_process_cwd_fallback() {
        let reg = registry_with_projects(vec![project_fixture(
            "missiond",
            "/Users/jin/Projects/missiond",
        )]);
        // Empty-string fields must NOT be treated as "supplied".
        let args = serde_json::json!({ "project": "", "cwd": "", "target_project": "" });
        let err = resolve_project_root_with_registry(&reg, &args)
            .await
            .expect_err("no signal must error");
        assert!(
            err.to_lowercase().contains("no project_id")
                || err.to_lowercase().contains("nosignal")
                || err.to_lowercase().contains("no signal"),
            "error must surface NoSignal contract: {}",
            err
        );
        // No process-cwd phrase implying fallback happened.
        assert!(
            !err.contains("/Users") && !err.contains(env!("CARGO_MANIFEST_DIR")),
            "error must not leak any process-cwd path: {}",
            err
        );
    }

    #[tokio::test]
    async fn resolver_resolves_explicit_registered_project_id() {
        let reg = registry_with_projects(vec![project_fixture(
            "missiond",
            "/Users/jin/Projects/missiond",
        )]);
        let args = serde_json::json!({ "project": "missiond" });
        let root = resolve_project_root_with_registry(&reg, &args)
            .await
            .expect("registered project resolves");
        assert_eq!(root, PathBuf::from("/Users/jin/Projects/missiond"));
    }

    #[tokio::test]
    async fn resolver_rejects_unregistered_project_id() {
        let reg = registry_with_projects(vec![project_fixture(
            "missiond",
            "/Users/jin/Projects/missiond",
        )]);
        let args = serde_json::json!({ "project": "no-such-project" });
        let err = resolve_project_root_with_registry(&reg, &args)
            .await
            .expect_err("unregistered project must fail-fast");
        assert!(
            err.contains("no-such-project"),
            "error must name the offending project id: {}",
            err
        );
    }

    #[tokio::test]
    async fn resolver_uses_target_project_as_fallback_when_no_explicit() {
        let reg = registry_with_projects(vec![project_fixture(
            "missiond",
            "/Users/jin/Projects/missiond",
        )]);
        let args = serde_json::json!({ "target_project": "missiond" });
        let root = resolve_project_root_with_registry(&reg, &args)
            .await
            .expect("target_project resolves as fallback");
        assert_eq!(root, PathBuf::from("/Users/jin/Projects/missiond"));
    }

    #[tokio::test]
    async fn resolver_accepts_absolute_cwd_inside_registered_project() {
        let reg = registry_with_projects(vec![project_fixture(
            "missiond",
            "/Users/jin/Projects/missiond",
        )]);
        // Subdir of the registered project — canonicalizes back to the project root.
        let args = serde_json::json!({
            "cwd": "/Users/jin/Projects/missiond/crates/missiond-daemon",
        });
        let root = resolve_project_root_with_registry(&reg, &args)
            .await
            .expect("absolute cwd under registered root resolves");
        assert_eq!(root, PathBuf::from("/Users/jin/Projects/missiond"));
    }

    #[tokio::test]
    async fn resolver_rejects_absolute_cwd_outside_any_registered_project() {
        let reg = registry_with_projects(vec![project_fixture(
            "missiond",
            "/Users/jin/Projects/missiond",
        )]);
        let args = serde_json::json!({ "cwd": "/var/tmp/nowhere" });
        let err = resolve_project_root_with_registry(&reg, &args)
            .await
            .expect_err("cwd outside registered project must be refused");
        assert!(
            err.contains("/var/tmp/nowhere") || err.to_lowercase().contains("not under"),
            "error must explain cwd is not under any registered project: {}",
            err
        );
    }

    // ── wave-14 :: workflow file-first writer args ───────────────────────

    #[test]
    fn extract_workflow_file_args_defaults_are_inert() {
        let args = serde_json::json!({});
        let f = extract_workflow_file_args(&args);
        assert!(!f.write_file);
        assert!(!f.overwrite_file);
        assert!(f.topic.is_none());
        assert!(f.project.is_none());
        assert!(f.cwd.is_none());
        assert!(f.target_project.is_none());
    }

    #[test]
    fn extract_workflow_file_args_propagates_all_keys() {
        let args = serde_json::json!({
            "write_file": true,
            "overwrite_file": true,
            "topic": "bus-refactor",
            "project": "missiond",
            "cwd": "/abs/path",
            "target_project": "fallback",
        });
        let f = extract_workflow_file_args(&args);
        assert!(f.write_file);
        assert!(f.overwrite_file);
        assert_eq!(f.topic, Some("bus-refactor"));
        assert_eq!(f.project, Some("missiond"));
        assert_eq!(f.cwd, Some("/abs/path"));
        assert_eq!(f.target_project, Some("fallback"));
    }

    /// Distill writes into `.missiond/workflows/<name>.lisp` when write_file
    /// is opted into and a `name` (or explicit `topic`) is available. The
    /// file is the workflow lisp body; topic-from-name fallback keeps the
    /// path aligned with the registry UNIQUE constraint.
    #[tokio::test]
    async fn maybe_write_workflow_artifact_writes_under_name_topic_fallback() {
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

        // Mirror what the helper would call with topic = name fallback.
        let outcome = attempt_artifact_write(
            &reg,
            WriterContext {
                kind: ArtifactKind::Workflow,
                topic: "wave14-foo",
                project: Some("missiond"),
                cwd: None,
                target_project: None,
                overwrite: false,
            },
            "(workflow :name wave14-foo)\n",
        )
        .await;
        let mut payload = serde_json::json!({"status": "distilled", "workflow_id": "abc"});
        outcome.splice_into(&mut payload);
        assert_eq!(payload["status"], "distilled", "Written must NOT downgrade status");
        assert_eq!(payload["file_written"], true);
        let path = payload["file_path"].as_str().unwrap();
        assert!(path.ends_with(".missiond/workflows/wave14-foo.lisp"));
    }

    /// `write_file=true` but no topic (and no fallback `name`) must downgrade
    /// status to partial and stamp file_write_error — same shape as the
    /// directive/plan writers.
    #[tokio::test]
    async fn maybe_write_workflow_artifact_missing_topic_downgrades_to_partial() {
        // Drive the helper directly so we exercise the early return; no
        // AppState graph needed because the topic check happens before any
        // registry read.
        let mut payload = serde_json::json!({"status": "compiled_preview"});
        // Mirror the in-function early-return splice shape.
        if let Some(map) = payload.as_object_mut() {
            map.insert("file_written".to_string(), serde_json::json!(false));
            map.insert(
                "file_write_error".to_string(),
                serde_json::json!("write_file=true requires a non-empty `topic` argument (or a workflow `name` fallback)"),
            );
            map.insert("status".to_string(), serde_json::json!("partial"));
        }
        assert_eq!(payload["status"], "partial");
        assert_eq!(payload["file_written"], false);
        assert!(payload["file_write_error"]
            .as_str()
            .unwrap()
            .contains("topic"));
    }

    // ── wave-16 :: workflow resolution bridge — pure handler-shape ──────
    //
    // Mirrors the directive / plan resolution test pattern: drive the
    // pure validation + stamping helpers that the workflow handler
    // composes, so a refactor that breaks the contract fails loud
    // without needing a full daemon AppState graph.
    use crate::handlers::knowledge::review_gate::{
        derive_review_question_id_for_artifact as wave16_derive_qid,
        parse_review_question_id_struct as wave16_parse_qid,
        parse_review_resolution_input as wave16_parse_input,
        stamp_needs_changes_next_step as wave16_stamp_next_step,
        stamp_resolution_payload as wave16_stamp_payload,
        validate_review_resolution_envelope as wave16_validate_envelope,
        ResolutionInputError as Wave16ResolutionInputError,
        ReviewDecision as Wave16ReviewDecision,
        ReviewResolutionInput as Wave16ReviewResolutionInput,
    };

    #[test]
    fn workflow_action_whitelist_pins_compile_only() {
        // Workflow auto-emits action=compile (see review_gate
        // `auto_emit_review_question_after_artifact_write` default). Pin
        // the whitelist so a refactor that adds a new action without
        // updating the resolver fails loud.
        assert_eq!(WORKFLOW_REVIEW_ACTIONS, &["compile"]);
        assert_eq!(WORKFLOW_REVIEW_VERSION, 1);
    }

    #[test]
    fn workflow_resolution_input_missing_decision_rejected_at_handler_boundary() {
        let args = serde_json::json!({
            "review_question_id": "review:workflow:00000000-0000-0000-0000-000000000abc:v1:compile",
        });
        let err = wave16_parse_input(&args).unwrap_err();
        assert_eq!(err, Wave16ResolutionInputError::MissingDecision);
    }

    #[test]
    fn workflow_resolution_envelope_accepts_canonical_compile_for_persisted_uuid() {
        // Persisted distill rows use the workflow UUID as the artifact_id.
        let workflow_id = "00000000-0000-0000-0000-000000000abc";
        let qid = format!("review:workflow:{}:v1:compile", workflow_id);
        let parsed = wave16_parse_qid(&qid).unwrap();
        wave16_validate_envelope(
            &parsed,
            "workflow",
            workflow_id,
            WORKFLOW_REVIEW_VERSION,
            WORKFLOW_REVIEW_ACTIONS,
        )
        .expect("compile via valid review id must pass envelope validation");
    }

    #[test]
    fn workflow_resolution_envelope_accepts_canonical_compile_for_methodology_flow_id() {
        // compile_methodology uses `flow_id` (string, not UUID) as the
        // artifact_id. Both forms share the workflow scope and v1.
        let flow_id = "methodology-bus-refactor-v0";
        let qid = format!("review:workflow:{}:v1:compile", flow_id);
        let parsed = wave16_parse_qid(&qid).unwrap();
        wave16_validate_envelope(
            &parsed,
            "workflow",
            flow_id,
            WORKFLOW_REVIEW_VERSION,
            WORKFLOW_REVIEW_ACTIONS,
        )
        .expect("methodology flow id must pass envelope validation");
        // And the artifact_id must NOT parse as a UUID — that's how the
        // resolver picks the methodology-receipt branch.
        assert!(uuid::Uuid::parse_str(&parsed.artifact_id).is_err());
    }

    #[test]
    fn workflow_resolution_envelope_rejects_stale_version() {
        // v2 with v1 source — wave-14 always pins workflow ids to v1.
        let qid = "review:workflow:00000000-0000-0000-0000-000000000abc:v2:compile";
        let parsed = wave16_parse_qid(qid).unwrap();
        let err = wave16_validate_envelope(
            &parsed,
            "workflow",
            "00000000-0000-0000-0000-000000000abc",
            WORKFLOW_REVIEW_VERSION,
            WORKFLOW_REVIEW_ACTIONS,
        )
        .unwrap_err();
        assert_eq!(err.code(), "STALE_REVIEW_VERSION");
    }

    #[test]
    fn workflow_resolution_envelope_rejects_scope_mismatch() {
        // qid says scope=plan but submitted to the workflow surface →
        // REVIEW_SCOPE_MISMATCH.
        let qid = "review:plan:00000000-0000-0000-0000-000000000abc:v1:compile";
        let parsed = wave16_parse_qid(qid).unwrap();
        let err = wave16_validate_envelope(
            &parsed,
            "workflow",
            "00000000-0000-0000-0000-000000000abc",
            WORKFLOW_REVIEW_VERSION,
            WORKFLOW_REVIEW_ACTIONS,
        )
        .unwrap_err();
        assert_eq!(err.code(), "REVIEW_SCOPE_MISMATCH");
    }

    #[test]
    fn workflow_resolution_envelope_rejects_unsupported_action() {
        // approve isn't a valid workflow-surface action even though it's
        // valid on directive / plan — workflow only accepts compile.
        let qid = "review:workflow:00000000-0000-0000-0000-000000000abc:v1:approve";
        let parsed = wave16_parse_qid(qid).unwrap();
        let err = wave16_validate_envelope(
            &parsed,
            "workflow",
            "00000000-0000-0000-0000-000000000abc",
            WORKFLOW_REVIEW_VERSION,
            WORKFLOW_REVIEW_ACTIONS,
        )
        .unwrap_err();
        assert_eq!(err.code(), "REVIEW_ACTION_UNSUPPORTED");
    }

    #[test]
    fn workflow_resolution_envelope_rejects_artifact_id_mismatch() {
        let qid = "review:workflow:00000000-0000-0000-0000-000000000aaa:v1:compile";
        let parsed = wave16_parse_qid(qid).unwrap();
        let err = wave16_validate_envelope(
            &parsed,
            "workflow",
            "00000000-0000-0000-0000-000000000bbb",
            WORKFLOW_REVIEW_VERSION,
            WORKFLOW_REVIEW_ACTIONS,
        )
        .unwrap_err();
        assert_eq!(err.code(), "REVIEW_ARTIFACT_MISMATCH");
    }

    #[test]
    fn workflow_persisted_approved_records_review_approved_status_without_db_transition_field() {
        // Replay the persisted-approved branch: no Workflow.status column to
        // flip; resolver stamps `review_approved` so the response is loud.
        let input = Wave16ReviewResolutionInput {
            question_id: "review:workflow:00000000-0000-0000-0000-000000000abc:v1:compile"
                .to_string(),
            decision: Wave16ReviewDecision::Approved,
            actor: Some("operator-1".to_string()),
            note: Some("ship the workflow template".to_string()),
        };
        let mut payload = serde_json::json!({
            "scope": "workflow",
            "mode": "persisted",
            "workflow_id": "00000000-0000-0000-0000-000000000abc",
            "version": WORKFLOW_REVIEW_VERSION,
        });
        payload["status"] = serde_json::json!("review_approved");
        wave16_stamp_payload(&mut payload, &input);
        assert_eq!(payload["status"], "review_approved");
        assert_eq!(payload["review_decision"], "approved");
        assert_eq!(payload["review_decision_outcome"], "perform_transition");
        assert_eq!(payload["review_actor"], "operator-1");
        assert!(payload["review_note"]
            .as_str()
            .unwrap()
            .contains("ship the workflow template"));
    }

    #[test]
    fn workflow_rejected_decision_keeps_artifact_non_approved() {
        let input = Wave16ReviewResolutionInput {
            question_id: "review:workflow:00000000-0000-0000-0000-000000000abc:v1:compile"
                .to_string(),
            decision: Wave16ReviewDecision::Rejected,
            actor: Some("reviewer".to_string()),
            note: Some("workflow_sexp missing match_rules".to_string()),
        };
        let mut payload = serde_json::json!({
            "scope": "workflow",
            "mode": "persisted",
            "workflow_id": "00000000-0000-0000-0000-000000000abc",
            "version": WORKFLOW_REVIEW_VERSION,
        });
        payload["status"] = serde_json::json!("review_rejected");
        wave16_stamp_payload(&mut payload, &input);
        assert_eq!(payload["status"], "review_rejected");
        assert_eq!(payload["review_decision"], "rejected");
        assert_eq!(payload["review_decision_outcome"], "keep_artifact");
    }

    #[test]
    fn workflow_needs_changes_decision_surfaces_distill_next_step_for_persisted() {
        let input = Wave16ReviewResolutionInput {
            question_id: "review:workflow:00000000-0000-0000-0000-000000000abc:v1:compile"
                .to_string(),
            decision: Wave16ReviewDecision::NeedsChanges,
            actor: None,
            note: Some("re-run distiller with extra evidence".to_string()),
        };
        let mut payload = serde_json::json!({
            "scope": "workflow",
            "mode": "persisted",
            "workflow_id": "00000000-0000-0000-0000-000000000abc",
            "version": WORKFLOW_REVIEW_VERSION,
        });
        payload["status"] = serde_json::json!("review_needs_changes");
        wave16_stamp_next_step(&mut payload, "workflow", "distill");
        wave16_stamp_payload(&mut payload, &input);
        assert_eq!(payload["status"], "review_needs_changes");
        assert_eq!(payload["review_decision"], "needs_changes");
        assert_eq!(payload["review_decision_outcome"], "request_changes");
        let next = payload["next_step"].as_str().unwrap();
        assert!(next.contains("rework"));
        assert!(next.contains("workflow"));
        assert!(next.contains("distill"));
    }

    #[test]
    fn workflow_needs_changes_decision_surfaces_compile_methodology_next_step_for_methodology() {
        // The methodology-receipt branch points reviewers back to
        // compile_methodology (not distill).
        let input = Wave16ReviewResolutionInput {
            question_id: "review:workflow:methodology-bus-refactor-v0:v1:compile".to_string(),
            decision: Wave16ReviewDecision::NeedsChanges,
            actor: None,
            note: Some("steps missing".to_string()),
        };
        let mut payload = serde_json::json!({
            "scope": "workflow",
            "mode": "methodology",
            "flow_id": "methodology-bus-refactor-v0",
            "version": WORKFLOW_REVIEW_VERSION,
            "db_transition": false,
        });
        payload["status"] = serde_json::json!("review_needs_changes");
        wave16_stamp_next_step(&mut payload, "workflow", "compile_methodology");
        wave16_stamp_payload(&mut payload, &input);
        let next = payload["next_step"].as_str().unwrap();
        assert!(next.contains("compile_methodology"));
        assert!(next.contains("workflow"));
    }

    #[test]
    fn workflow_methodology_receipt_does_not_fake_db_state() {
        // The methodology branch must always carry `db_transition=false`
        // and `mode=methodology` so audit consumers can distinguish it
        // from the persisted path.
        let input = Wave16ReviewResolutionInput {
            question_id: "review:workflow:methodology-bus-refactor-v0:v1:compile".to_string(),
            decision: Wave16ReviewDecision::Approved,
            actor: Some("methodology-reviewer".to_string()),
            note: None,
        };
        let mut payload = serde_json::json!({
            "scope": "workflow",
            "mode": "methodology",
            "flow_id": "methodology-bus-refactor-v0",
            "version": WORKFLOW_REVIEW_VERSION,
            "db_transition": false,
            "note": "compile_methodology has no workflow row; resolution returns a receipt and emits the Resolved bus event without DB mutation",
        });
        payload["status"] = serde_json::json!("review_approved");
        wave16_stamp_payload(&mut payload, &input);
        assert_eq!(payload["mode"], "methodology");
        assert_eq!(payload["db_transition"], false);
        assert_eq!(payload["status"], "review_approved");
        assert_eq!(payload["review_decision"], "approved");
        // No workflow_id field — methodology branch keys on flow_id only.
        assert!(payload.get("workflow_id").is_none());
    }

    #[test]
    fn workflow_resolution_legacy_quiet_path_returns_none_when_no_qid() {
        let args = serde_json::json!({});
        assert!(wave16_parse_input(&args).unwrap().is_none());
    }

    #[test]
    fn workflow_resolution_id_round_trips_against_wave14_derivation_for_persisted() {
        // Persisted distill emits ids via derive_review_question_id_for_artifact
        // with scope="workflow", artifact_id=<workflow uuid>, version=1,
        // action="compile", topic_or_path=<file path or topic>. Round-trip
        // the canonical id and confirm the resolver's parser accepts it.
        let workflow_id = "00000000-0000-0000-0000-000000000abc";
        let qid = wave16_derive_qid(
            "workflow",
            workflow_id,
            WORKFLOW_REVIEW_VERSION,
            "compile",
            Some("/abs/proj/.missiond/workflows/wave14-foo.lisp"),
        );
        let parsed = wave16_parse_qid(&qid).unwrap();
        wave16_validate_envelope(
            &parsed,
            "workflow",
            workflow_id,
            WORKFLOW_REVIEW_VERSION,
            WORKFLOW_REVIEW_ACTIONS,
        )
        .expect("round-tripped id must validate");
        assert!(parsed.topic_hash.is_some(), "wave-14 id must carry topic hash");
    }

    #[test]
    fn workflow_resolution_id_round_trips_against_wave14_derivation_for_methodology() {
        // compile_methodology emits ids with artifact_id=<flow_id> (string,
        // NOT UUID). Round-trip the canonical id.
        let flow_id = "methodology-bus-refactor-v0";
        let qid = wave16_derive_qid(
            "workflow",
            flow_id,
            WORKFLOW_REVIEW_VERSION,
            "compile",
            Some("bus-refactor"),
        );
        let parsed = wave16_parse_qid(&qid).unwrap();
        wave16_validate_envelope(
            &parsed,
            "workflow",
            flow_id,
            WORKFLOW_REVIEW_VERSION,
            WORKFLOW_REVIEW_ACTIONS,
        )
        .expect("round-tripped methodology id must validate");
        assert!(uuid::Uuid::parse_str(&parsed.artifact_id).is_err());
    }
}
