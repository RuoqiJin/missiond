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

#[cfg(test)]
use crate::handlers::knowledge::review_gate::ReviewDecision;
use crate::handlers::knowledge::review_gate::{
    apply_compile_review_gates, parse_compile_review_gate, parse_review_gate_policy,
    review_gate_policy_was_explicit,
};
use crate::minimax_client::ChatMessage;
use crate::slot_orchestrator::project_root::{resolve_target_project_root, ResolutionError};
use crate::state::AppState;
use missiond_core::types::PlanStatus;

mod artifacts;
mod methodology;
mod review_resolution;

use artifacts::{
    build_methodology_match_rules, extract_workflow_file_args, maybe_write_workflow_artifact,
    render_workflow_artifact_sexp,
};
use methodology::{
    atomic_write, build_generated_yaml, derive_flow_id, extract_methodology_lifted,
    extract_steps_with_lines, generated_yaml_path, resolve_compiled_flow, resolve_methodology_path,
    source_hash, source_path_for_yaml, validate_methodology_source, CompiledFlowError,
    GeneratedMeta,
};
#[cfg(test)]
use methodology::{
    build_manual_review_prompt, extract_steps, match_form_keyword, parse_optional_form_id,
    phase_id_for_step, sanitize_id_token, unique_generated_yaml_temp_path, LocatedStep,
    MethodologyForm, MethodologyLifted, MethodologyPhase, MethodologyStep,
};
use review_resolution::action_resolve_review;
pub(crate) use review_resolution::{handle_review_resolved_event, WorkflowSubscriberOutcome};
#[cfg(test)]
use review_resolution::{WORKFLOW_REVIEW_ACTIONS, WORKFLOW_REVIEW_VERSION};

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
        let id = uuid::Uuid::parse_str(raw).map_err(|e| anyhow!("workflow_id not UUID: {}", e))?;
        state
            .store
            .workflow_get_by_id(id)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
    } else {
        return Ok(ToolResult::structured_error(ToolError::new(
            error_codes::MISSING_PARAM,
            "get requires `name` or `workflow_id`",
        )));
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
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::MISSING_PARAM,
                "match requires `utterance` (or `query`)",
            )))
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
        let id = uuid::Uuid::parse_str(raw).map_err(|e| anyhow!("workflow_id not UUID: {}", e))?;
        state
            .store
            .workflow_get_by_id(id)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
    } else {
        return Ok(ToolResult::structured_error(ToolError::new(
            error_codes::MISSING_PARAM,
            "apply requires `name` or `workflow_id`",
        )));
    };
    match row {
        Some(w) => Ok(ToolResult::json_pretty(&json!({
            "status": "candidate_returned",
            "workflow": w,
            "note": "apply returns the template. Execution requires action=run_methodology or mission_flow_run on a compiled YAML.",
        }))),
        None => Ok(ToolResult::structured_error(ToolError::new(
            error_codes::NOT_FOUND,
            "workflow not found",
        ))),
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
        .map_err(|e| anyhow!("DB error: {}", e))?;
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
            .map_err(|e| anyhow!("DB error: {}", e))?;
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

// ───────────────────────────────────────────────────────────────────────
// wave-19 / task 09 :: cross-plan distill auto-chain
//
// `apply_distill_chain` in plan.rs already records cross-plan distill chains
// when callers opt in via `distill_chain_id / distill_chain_mode /
// distill_chain_name`. Wave-19 adds an opt-in counterpart on the workflow
// surface itself so a direct `mission_workflow(action=distill, …)` caller
// can record a chain entry WITHOUT supplying an explicit `chain_id`. The
// id is derived deterministically from the workflow context the caller
// already has in scope:
//
//     project_root + plan_id + (workflow_name | workflow_id) + evidence_sha256
//
// Hard constraints (mirror the wave-19 / task 09 brief):
//   - DEFAULT `auto_chain=false`. Existing callers see byte-identical
//     response shapes; only opt-in payloads carry the `auto_chain` block.
//   - NEVER calls Sonnet implicitly. The auto-chain hook runs AFTER the
//     caller-chosen distill mode (dry_run or sonnet) and only RECORDS the
//     chain entry — it never re-invokes the distiller.
//   - Sidecar is append-only (uses `evidence_collector::append`); no
//     migration, no overwrite, no schema change.
//   - Failures (sidecar write / project resolution) collapse to a partial
//     `auto_chain` block carrying `evidence_error` so the original distill
//     payload stays durable.
// ───────────────────────────────────────────────────────────────────────

/// Source tag stamped on the auto-chain evidence row so consumers can
/// distinguish the wave-19 workflow-side recorder from the wave-18
/// plan_dag-side recorder (`source::PLAN_DAG_NODE_DISPATCH`).
const AUTO_CHAIN_EVIDENCE_SOURCE: &str = "workflow_distill_auto_chain";

/// Evidence `kind` tag — same wire form as plan.rs's `CHAIN_RECORD_KIND`
/// so a single audit query (`kind="distill_chain_record"`) sees BOTH the
/// wave-18 plan_dag-driven entries AND the wave-19 workflow-driven entries.
const AUTO_CHAIN_EVIDENCE_KIND: &str = "distill_chain_record";

/// Status surfaced under `auto_chain.status` on the response — kept as
/// constants so audit / test consumers can pin the wire form.
const AUTO_CHAIN_STATUS_RECORDED: &str = "recorded";
const AUTO_CHAIN_STATUS_RECORD_FAILED: &str = "record_failed";
const AUTO_CHAIN_STATUS_RESOLVE_FAILED: &str = "resolve_failed";

/// Deterministic id source label that mirrors plan.rs's
/// `chain_id_source` taxonomy (`explicit_arg` / `derived_from_plan_id`).
const AUTO_CHAIN_ID_SOURCE_DERIVED: &str = "derived_from_workflow_context";

/// Returns true when the caller opted into the auto-chain hook. Strict
/// boolean check on `auto_chain` so a missing / non-bool field collapses
/// to false (byte-compat with pre-wave-19 callers).
fn auto_chain_requested(args: &Value) -> bool {
    args.get("auto_chain")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Optional caller-supplied chain name (free-form, e.g.
/// `"wave19-finalize-loop"`). Echoed into the chain block + evidence row
/// so dashboards can group runs without parsing the deterministic id.
fn parse_auto_chain_name(args: &Value) -> Option<String> {
    args.get("auto_chain_name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Derive the deterministic auto-chain id from the workflow context.
///
/// Inputs (in canonical concatenation order):
///
///   1. `project_root`        — registry-resolved, absolute. Anchors the
///                              chain to the project so two projects
///                              cannot collide on the same plan_id.
///   2. `plan_id`             — the source plan UUID.
///   3. `workflow_anchor`     — the persisted `workflow_id` UUID when the
///                              distill row landed; else the workflow
///                              `name` arg; else the literal string
///                              `<unnamed>`. Provides a stable anchor
///                              across re-runs that produce the same
///                              workflow row / name.
///   4. `evidence_sha256`     — sha256 of the on-disk evidence sidecar
///                              when present; the literal string
///                              `<no-evidence>` otherwise. Lets a fresh
///                              evidence record (e.g. additional rows
///                              appended between distill runs) bucket
///                              into a NEW chain id without forcing the
///                              caller to roll an explicit value.
///
/// The four components are joined with a single `\u{1f}` (US — unit
/// separator) so no caller-supplied substring can collide with the
/// delimiter. The sha256 of the resulting blob is hex-encoded and
/// prefixed with `chain:auto:wf-` to mirror plan.rs's `chain:auto:plan-…`
/// shape; the `wf-` namespace prefix lets audit queries pivot on the
/// recorder origin without re-deriving it from the evidence row.
fn derive_auto_chain_id(
    project_root: &Path,
    plan_id: uuid::Uuid,
    workflow_anchor: &str,
    evidence_sha256: &str,
) -> String {
    let project_canonical = project_root.display().to_string();
    let blob = format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}",
        project_canonical, plan_id, workflow_anchor, evidence_sha256
    );
    let mut hasher = Sha256::new();
    hasher.update(blob.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{:02x}", byte));
    }
    format!("chain:auto:wf-{}", hex)
}

/// Compute the sha256 hex of an on-disk evidence sidecar. Returns
/// `Some(hex)` when the file exists AND the bytes hash cleanly; `None`
/// when the sidecar is absent or unreadable. The auto-chain id derivation
/// substitutes the literal `<no-evidence>` for the `None` case so
/// missing-evidence runs still hash to a stable id.
fn compute_evidence_sha256(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return None,
    };
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{:02x}", byte));
    }
    Some(hex)
}

/// Pick the workflow anchor for chain-id derivation. Persisted distill
/// rows expose a fresh UUID via `payload["workflow_id"]`; the dry-run /
/// non-persist path falls back to the caller-supplied `name` (which is
/// the workflow UNIQUE key) or the literal `<unnamed>` so the hash stays
/// well-defined.
fn pick_workflow_anchor(workflow_id: Option<&str>, name: &str) -> String {
    if let Some(id) = workflow_id.map(str::trim).filter(|s| !s.is_empty()) {
        return id.to_string();
    }
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "<unnamed>".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Splice an `auto_chain` block onto the distill response payload. The
/// block always carries `requested` / `status` / (when applicable)
/// `chain_id` / `chain_id_source` / `chain_id_inputs`. Optional fields
/// (`chain_name`, `evidence_path`, `evidence_error`) are only added when
/// present. Mirrors `plan.rs::attach_distill_chain_to_payload`'s
/// "always-stable shape" contract.
fn attach_auto_chain_to_payload(payload: &mut Value, block: Value) {
    if let Some(obj) = payload.as_object_mut() {
        // Top-level shortcuts so callers can pivot without descending into
        // the block — mirrors `distill_chain_status` on plan.rs.
        if let Some(status) = block.get("status").and_then(|v| v.as_str()) {
            obj.insert("auto_chain_status".to_string(), json!(status));
        }
        if let Some(id) = block.get("chain_id").and_then(|v| v.as_str()) {
            obj.insert("auto_chain_id".to_string(), json!(id));
        }
        obj.insert("auto_chain".to_string(), block);
    }
}

/// Compose the auto-chain block surfaced under `payload.auto_chain`.
fn build_auto_chain_block(
    requested: bool,
    status: &str,
    chain_id: Option<&str>,
    chain_name: Option<&str>,
    chain_id_inputs: Option<Value>,
    evidence_path: Option<&str>,
    evidence_error: Option<&str>,
) -> Value {
    let mut block = json!({
        "requested": requested,
        "status": status,
        "chain_id_source": AUTO_CHAIN_ID_SOURCE_DERIVED,
    });
    if let Some(id) = chain_id {
        block["chain_id"] = json!(id);
    }
    if let Some(name) = chain_name {
        block["chain_name"] = json!(name);
    }
    if let Some(inputs) = chain_id_inputs {
        block["chain_id_inputs"] = inputs;
    }
    if let Some(p) = evidence_path {
        block["evidence_path"] = json!(p);
    }
    if let Some(e) = evidence_error {
        block["evidence_error"] = json!(e);
    }
    block
}

/// Post-distill hook. Returns the original `inner` ToolResult unchanged
/// when `auto_chain=false` (byte-compat); otherwise re-projects the inner
/// payload, computes the deterministic chain id, appends a chain-record
/// evidence row, and splices an `auto_chain` block onto the response.
///
/// Failure surface (per CLAUDE.md fail-fast policy):
///   - Project root resolution failure → `auto_chain.status="resolve_failed"`
///     (the original distill payload is preserved; no chain id is derived
///     because the project root anchors the hash).
///   - Sidecar append failure          → `auto_chain.status="record_failed"`
///     with `evidence_error` set; the chain id IS still derived and
///     surfaced so callers can retry / re-record manually.
///   - Inner result is a structured error → skip auto-chain entirely
///     (matches plan.rs's `apply_distill_chain` policy: never overwrite
///     an error envelope with chain side-effects).
async fn maybe_apply_auto_chain(
    state: &AppState,
    args: &Value,
    plan: &missiond_core::types::Plan,
    name: &str,
    inner: ToolResult,
) -> ToolResult {
    if !auto_chain_requested(args) {
        return inner;
    }
    if inner.is_error.unwrap_or(false) {
        return inner;
    }

    // Re-project the inner payload so we can splice the chain block
    // alongside the existing distill fields without rebuilding the whole
    // response shape. We borrow plan.rs's `tool_result_payload` helper
    // (`pub(super)`) so the projection rule stays consistent across both
    // chain recorders. Distill always emits a JSON text part; the helper
    // collapses anything else to `Value::Null` / `Value::String`, in
    // which case we return the inner result untouched (no silent payload
    // synthesis).
    let mut payload = super::plan::tool_result_payload(&inner);
    if !payload.is_object() {
        return inner;
    }

    let chain_name = parse_auto_chain_name(args);

    // Project resolution: we need the canonical absolute root to hash
    // into the chain id AND to anchor the sidecar append.
    let project_root = match resolve_project_root_from_args(state, args).await {
        Ok(p) => p,
        Err(reason) => {
            let block = build_auto_chain_block(
                true,
                AUTO_CHAIN_STATUS_RESOLVE_FAILED,
                None,
                chain_name.as_deref(),
                None,
                None,
                Some(&reason),
            );
            attach_auto_chain_to_payload(&mut payload, block);
            return ToolResult::json_pretty(&payload);
        }
    };

    // Pull the persisted workflow_id from the distill payload when
    // available (persist=true path); fall back to the caller-supplied
    // `name` so dry-run / non-persist runs still produce a stable
    // anchor.
    let workflow_id_str = payload
        .get("workflow_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let workflow_anchor = pick_workflow_anchor(workflow_id_str.as_deref(), name);

    // Hash the on-disk evidence sidecar (when present). Missing sidecar
    // collapses to the literal `<no-evidence>` placeholder so the chain
    // id stays well-defined for callers that distil without an evidence
    // gate (e.g. `allow_missing_evidence=true`).
    let evidence_path = evidence_sidecar_path(&project_root, plan.id);
    let evidence_sha = compute_evidence_sha256(&evidence_path);
    let evidence_sha_for_id = evidence_sha
        .clone()
        .unwrap_or_else(|| "<no-evidence>".to_string());

    let chain_id = derive_auto_chain_id(
        &project_root,
        plan.id,
        &workflow_anchor,
        &evidence_sha_for_id,
    );

    // Inputs surfaced verbatim on the response so audit consumers can
    // re-derive the chain id without scraping the daemon's source. Names
    // mirror the function signature so the mapping is obvious.
    let chain_id_inputs = json!({
        "project_root": project_root.display().to_string(),
        "plan_id": plan.id.to_string(),
        "workflow_anchor": workflow_anchor,
        "evidence_sha256": evidence_sha_for_id,
    });

    // Append exactly ONE chain-record evidence row. Schema mirrors
    // plan.rs's wave-18 chain-record entry so a single audit query
    // (`kind="distill_chain_record"`) sees both recorders' rows; the
    // `source` tag distinguishes which surface produced it.
    let mut entry = super::evidence_collector::EvidenceEntry::new(
        AUTO_CHAIN_EVIDENCE_SOURCE,
        AUTO_CHAIN_EVIDENCE_KIND,
    )
    .with_state_transition("workflow_distill_auto_chain_appended")
    .with_extra("event_kind", json!("workflow_distill_auto_chain"))
    .with_extra("plan_id", json!(plan.id))
    .with_extra("plan_version", json!(plan.version))
    .with_extra("chain_id", json!(chain_id))
    .with_extra("chain_id_source", json!(AUTO_CHAIN_ID_SOURCE_DERIVED))
    .with_extra("chain_id_inputs", chain_id_inputs.clone())
    .with_extra("workflow_anchor", json!(workflow_anchor))
    .with_extra("workflow_name_arg", json!(name))
    .with_extra(
        "distill_mode",
        payload
            .get("distill_mode")
            .cloned()
            .unwrap_or_else(|| Value::String("dry_run".to_string())),
    );
    if let Some(label) = chain_name.as_deref() {
        entry = entry.with_extra("chain_name", json!(label));
    }
    if let Some(id) = workflow_id_str.as_deref() {
        entry = entry.with_extra("workflow_id", json!(id));
    }
    if let Some(sha) = evidence_sha.as_deref() {
        entry = entry.with_extra("evidence_sha256", json!(sha));
    }

    let project_arg = args.get("project").and_then(|v| v.as_str());
    let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg = args.get("target_project").and_then(|v| v.as_str());

    let outcome = super::evidence_collector::append(
        state,
        plan.id,
        project_arg,
        cwd_arg,
        target_project_arg,
        entry,
    )
    .await;

    let (evidence_path_str, evidence_error) = match outcome {
        super::evidence_collector::AppendOutcome::Written { path, .. } => {
            (Some(path.display().to_string()), None)
        }
        super::evidence_collector::AppendOutcome::Failed { error } => {
            tracing::warn!(
                plan_id = %plan.id,
                chain_id = %chain_id,
                error = %error,
                "workflow auto_chain: evidence sidecar append failed"
            );
            (None, Some(error))
        }
    };

    let status = if evidence_error.is_some() {
        AUTO_CHAIN_STATUS_RECORD_FAILED
    } else {
        AUTO_CHAIN_STATUS_RECORDED
    };

    let block = build_auto_chain_block(
        true,
        status,
        Some(&chain_id),
        chain_name.as_deref(),
        Some(chain_id_inputs),
        evidence_path_str.as_deref(),
        evidence_error.as_deref(),
    );
    attach_auto_chain_to_payload(&mut payload, block);
    ToolResult::json_pretty(&payload)
}

// ───────────────────────────────────────────────────────────────────────
// wave-20 / task 06 :: cross-plan distill auto-trigger v1
//
// Layered ON TOP of the wave-19 auto-chain recorder. Default trigger mode
// is `"never"` so existing callers (including the wave-19 `auto_chain=true`
// opt-in path) see byte-identical responses. When the caller passes
// `auto_chain_trigger="auto_safe"` the daemon evaluates a deterministic
// safety-rule set; only if ALL rules pass does it fall through to the
// wave-19 recorder. Rule failures surface a non-recording `skipped` block
// so audit consumers can replay the exact rule outcomes.
//
// Non-negotiables (mirror the wave-20 / task 06 brief):
//   - DEFAULT `auto_chain_trigger="never"` (legacy behaviour preserved).
//   - ONLY deterministic rules; NEVER calls Sonnet implicitly. Sonnet is
//     reachable solely via the existing `distill_mode="sonnet"` arg, which
//     is upstream of this trigger.
//   - Rule failure → `trigger_status="skipped_rules_failed"` + the full
//     rule-result list. NEVER partially appends a chain entry.
//   - Rule pass → behaves as if `auto_chain=true`: same evidence row, same
//     `auto_chain` block, same top-level `auto_chain_status` /
//     `auto_chain_id` shortcuts. We additionally splice an
//     `auto_trigger` block carrying `trigger_status`, `chain_id`,
//     `safety_rule_results`, and `sidecar` for symmetry with the
//     skipped path.
//   - Inner distill error → `trigger_status="skipped_inner_error"`; the
//     inner ToolResult is returned unmutated so error envelopes stay
//     loud (matches the wave-19 / plan.rs `apply_distill_chain` policy).
// ───────────────────────────────────────────────────────────────────────

/// Caller-facing trigger modes. `Never` (default) preserves wave-19
/// behaviour byte-for-byte. `AutoSafe` runs the deterministic safety
/// rules and triggers the wave-19 recorder iff all rules pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoChainTrigger {
    Never,
    AutoSafe,
}

impl AutoChainTrigger {
    fn as_wire_str(self) -> &'static str {
        match self {
            AutoChainTrigger::Never => "never",
            AutoChainTrigger::AutoSafe => "auto_safe",
        }
    }
}

/// Status surfaced under `auto_trigger.trigger_status`. Audit consumers
/// pin these strings — never rename in-place.
const AUTO_TRIGGER_STATUS_DISABLED: &str = "skipped_disabled";
const AUTO_TRIGGER_STATUS_INNER_ERROR: &str = "skipped_inner_error";
const AUTO_TRIGGER_STATUS_RULES_FAILED: &str = "skipped_rules_failed";
const AUTO_TRIGGER_STATUS_TRIGGERED: &str = "triggered";
const AUTO_TRIGGER_STATUS_TRIGGERED_RECORD_FAILED: &str = "triggered_record_failed";
const AUTO_TRIGGER_STATUS_TRIGGERED_RESOLVE_FAILED: &str = "triggered_resolve_failed";

/// Deterministic safety-rule identifiers. Each maps to a single
/// boolean check evaluated by `evaluate_auto_trigger_safety_rules`.
const SAFETY_RULE_INNER_DISTILL_OK: &str = "inner_distill_succeeded";
const SAFETY_RULE_DISTILL_MODE_RECORDED: &str = "distill_mode_recorded";
const SAFETY_RULE_PROJECT_ROOT_RESOLVED: &str = "project_root_resolved";
const SAFETY_RULE_EVIDENCE_PRESENT: &str = "evidence_sidecar_present";
const SAFETY_RULE_EVIDENCE_MIN_ENTRIES: &str = "evidence_min_entries";
const SAFETY_RULE_NOT_ALREADY_CHAINED: &str = "chain_id_not_already_recorded";

/// Default minimum sidecar-entry count required by
/// `SAFETY_RULE_EVIDENCE_MIN_ENTRIES`. Mirrors the existing
/// `min_evidence` default on the sonnet distill gate so the trigger's
/// notion of "real evidence" matches the upstream.
const AUTO_TRIGGER_DEFAULT_MIN_EVIDENCE: usize = 1;

/// Parse the caller-supplied trigger mode. Missing / blank / null →
/// `Never` (default-off). Unknown values are rejected loudly so a typo
/// can't silently disable the trigger.
fn parse_auto_chain_trigger(raw: Option<&str>) -> Result<AutoChainTrigger, String> {
    match raw.map(str::trim) {
        None | Some("") | Some("never") => Ok(AutoChainTrigger::Never),
        Some("auto_safe") => Ok(AutoChainTrigger::AutoSafe),
        Some(other) => Err(format!(
            "auto_chain_trigger must be one of [\"never\", \"auto_safe\"]; got `{}`",
            other
        )),
    }
}

/// Pure outcome of a single safety-rule evaluation. `detail` is omitted
/// from the response when `passed=true` to keep the payload small.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SafetyRuleResult {
    rule_id: &'static str,
    passed: bool,
    detail: Option<String>,
}

impl SafetyRuleResult {
    fn pass(rule_id: &'static str) -> Self {
        Self {
            rule_id,
            passed: true,
            detail: None,
        }
    }

    fn fail(rule_id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            rule_id,
            passed: false,
            detail: Some(detail.into()),
        }
    }

    fn to_value(&self) -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert("rule_id".to_string(), json!(self.rule_id));
        obj.insert("passed".to_string(), json!(self.passed));
        if let Some(d) = &self.detail {
            obj.insert("detail".to_string(), json!(d));
        }
        Value::Object(obj)
    }
}

/// Render the full rule list as a JSON array suitable for the
/// response payload.
fn render_safety_rule_results(rules: &[SafetyRuleResult]) -> Value {
    Value::Array(rules.iter().map(SafetyRuleResult::to_value).collect())
}

/// Pure check: does the inner ToolResult carry an error envelope?
/// Wave-19's `maybe_apply_auto_chain` skips chain side-effects on
/// errors, so the trigger must surface the same skip — but with a
/// distinct `trigger_status` so audit consumers can tell a rule
/// failure apart from an upstream distill failure.
fn inner_result_is_error(inner: &ToolResult) -> bool {
    inner.is_error.unwrap_or(false)
}

/// Pure check: does the inner payload carry a `distill_mode` field?
/// The wave-19 dry_run / sonnet branches both stamp this; an inner
/// payload missing it indicates the trigger is being asked to chain
/// a non-distill response (e.g. someone re-shaped the inner result
/// without preserving the wire contract). Refuse loud rather than
/// guess.
fn inner_payload_has_distill_mode(payload: &Value) -> bool {
    payload
        .get("distill_mode")
        .and_then(|v| v.as_str())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

/// Pure check: does the existing evidence sidecar already contain a
/// `distill_chain_record` entry whose `chain_id` matches the candidate?
/// The check is deterministic over the on-disk sidecar so two
/// concurrent triggers with the same canonical inputs both refuse to
/// double-record (the second one's rule simply fails — no DB
/// transaction needed).
///
/// Only the chain_id is compared; sources (wave-18 plan_dag vs wave-19
/// workflow) intentionally share the kind tag so audit queries see
/// both, and for dedup purposes we treat any prior `chain_id` collision
/// as "already chained".
fn chain_id_already_in_sidecar(sidecar_value: &Value, candidate_chain_id: &str) -> bool {
    let entries = match sidecar_value.get("entries").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return false,
    };
    for entry in entries {
        let kind = entry.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        if kind != AUTO_CHAIN_EVIDENCE_KIND {
            continue;
        }
        let chain_id = entry
            .get("extra")
            .and_then(|e| e.get("chain_id"))
            .and_then(|v| v.as_str())
            .or_else(|| entry.get("chain_id").and_then(|v| v.as_str()))
            .unwrap_or("");
        if chain_id == candidate_chain_id {
            return true;
        }
    }
    false
}

/// Bundle of inputs + final pass/fail outcome for the deterministic
/// safety-rule evaluator. Public-shape members are private so the
/// evaluator is the single source of truth for rule wiring.
struct SafetyRuleContext<'a> {
    inner: &'a ToolResult,
    inner_payload: &'a Value,
    project_root: Option<&'a Path>,
    project_resolve_error: Option<&'a str>,
    evidence_outcome: &'a EvidenceOutcome,
    candidate_chain_id: Option<&'a str>,
    min_evidence: usize,
}

/// Pure evaluator. Returns the rule list in a fixed order so audit
/// consumers can pin the indices. `all_passed` is `true` iff every
/// rule's `passed=true`.
fn evaluate_auto_trigger_safety_rules(
    ctx: &SafetyRuleContext<'_>,
) -> (Vec<SafetyRuleResult>, bool) {
    let mut rules: Vec<SafetyRuleResult> = Vec::new();

    // R1: inner distill must have produced a non-error envelope. Without
    // this rule the trigger could append chain rows for failed distills.
    if inner_result_is_error(ctx.inner) {
        rules.push(SafetyRuleResult::fail(
            SAFETY_RULE_INNER_DISTILL_OK,
            "inner distill returned a structured error envelope",
        ));
    } else {
        rules.push(SafetyRuleResult::pass(SAFETY_RULE_INNER_DISTILL_OK));
    }

    // R2: inner payload must carry a `distill_mode` so we know the inner
    // call really ran the distiller (dry_run or sonnet). A missing field
    // points at upstream contract drift — refuse loud instead of guessing.
    if inner_payload_has_distill_mode(ctx.inner_payload) {
        rules.push(SafetyRuleResult::pass(SAFETY_RULE_DISTILL_MODE_RECORDED));
    } else {
        rules.push(SafetyRuleResult::fail(
            SAFETY_RULE_DISTILL_MODE_RECORDED,
            "inner distill payload missing `distill_mode`",
        ));
    }

    // R3: project root must resolve. The wave-19 recorder anchors the
    // chain id on the canonical project root, so an unresolved root
    // makes the deterministic id meaningless.
    if ctx.project_root.is_some() {
        rules.push(SafetyRuleResult::pass(SAFETY_RULE_PROJECT_ROOT_RESOLVED));
    } else {
        let detail = ctx
            .project_resolve_error
            .map(|s| s.to_string())
            .unwrap_or_else(|| "project root resolution failed".to_string());
        rules.push(SafetyRuleResult::fail(
            SAFETY_RULE_PROJECT_ROOT_RESOLVED,
            detail,
        ));
    }

    // R4: evidence sidecar must exist and parse cleanly. Wave-19's
    // recorder will append even when the sidecar is missing (using the
    // `<no-evidence>` placeholder), but the v1 trigger refuses to do
    // that automatically — auto-mode demands the caller has already
    // recorded evidence.
    let (sidecar_present, sidecar_entry_count) = match ctx.evidence_outcome {
        EvidenceOutcome::Present { entry_count, .. } => (true, *entry_count),
        _ => (false, 0usize),
    };
    if sidecar_present {
        rules.push(SafetyRuleResult::pass(SAFETY_RULE_EVIDENCE_PRESENT));
    } else {
        let detail = match ctx.evidence_outcome {
            EvidenceOutcome::Missing => "evidence sidecar not found".to_string(),
            EvidenceOutcome::ParseFailed { error } => {
                format!("evidence sidecar parse failed: {}", error)
            }
            EvidenceOutcome::Present { .. } => unreachable!(),
        };
        rules.push(SafetyRuleResult::fail(SAFETY_RULE_EVIDENCE_PRESENT, detail));
    }

    // R5: sidecar must carry at least `min_evidence` entries. Mirrors
    // the upstream sonnet distill gate's `min_evidence` default; the
    // trigger never overrides it.
    if sidecar_present && sidecar_entry_count >= ctx.min_evidence {
        rules.push(SafetyRuleResult::pass(SAFETY_RULE_EVIDENCE_MIN_ENTRIES));
    } else if sidecar_present {
        rules.push(SafetyRuleResult::fail(
            SAFETY_RULE_EVIDENCE_MIN_ENTRIES,
            format!(
                "sidecar has {} entries; require >= {}",
                sidecar_entry_count, ctx.min_evidence
            ),
        ));
    } else {
        // R5 cannot evaluate independently when the sidecar is missing;
        // surface the dependency explicitly rather than silently
        // skipping.
        rules.push(SafetyRuleResult::fail(
            SAFETY_RULE_EVIDENCE_MIN_ENTRIES,
            "sidecar missing — entry count cannot be verified",
        ));
    }

    // R6: candidate chain id must NOT already exist in the sidecar.
    // Without this dedup the trigger could append the same chain_id
    // twice on rapid successive calls. We only evaluate when the
    // sidecar parsed AND a candidate id was derived.
    match (ctx.candidate_chain_id, ctx.evidence_outcome) {
        (Some(id), EvidenceOutcome::Present { value, .. }) => {
            if chain_id_already_in_sidecar(value, id) {
                rules.push(SafetyRuleResult::fail(
                    SAFETY_RULE_NOT_ALREADY_CHAINED,
                    format!("chain_id `{}` already recorded in sidecar", id),
                ));
            } else {
                rules.push(SafetyRuleResult::pass(SAFETY_RULE_NOT_ALREADY_CHAINED));
            }
        }
        (None, _) => {
            // Without a candidate chain id we cannot evaluate the
            // dedup rule — fail loud so the caller knows the trigger
            // is incomplete.
            rules.push(SafetyRuleResult::fail(
                SAFETY_RULE_NOT_ALREADY_CHAINED,
                "candidate chain_id not derived (upstream gate failed)",
            ));
        }
        (Some(_), _) => {
            // Sidecar absent / unreadable was already flagged by R4;
            // surface the dependency rather than silently passing.
            rules.push(SafetyRuleResult::fail(
                SAFETY_RULE_NOT_ALREADY_CHAINED,
                "sidecar unavailable — dedup cannot be verified",
            ));
        }
    }

    let all_passed = rules.iter().all(|r| r.passed);
    (rules, all_passed)
}

/// Build the `auto_trigger` block surfaced under the response payload.
/// The block always carries `requested`, `mode`, `trigger_status`, and
/// `safety_rule_results` so audit consumers see a stable shape.
/// `chain_id` and `sidecar` are surfaced when known.
fn build_auto_trigger_block(
    requested: bool,
    mode: AutoChainTrigger,
    trigger_status: &str,
    safety_rule_results: Value,
    chain_id: Option<&str>,
    sidecar: Option<&str>,
) -> Value {
    let mut block = json!({
        "requested": requested,
        "mode": mode.as_wire_str(),
        "trigger_status": trigger_status,
        "safety_rule_results": safety_rule_results,
    });
    if let Some(id) = chain_id {
        block["chain_id"] = json!(id);
    }
    if let Some(p) = sidecar {
        block["sidecar"] = json!(p);
    }
    block
}

/// Splice the `auto_trigger` block + top-level shortcut onto the
/// response payload. Mirrors `attach_auto_chain_to_payload`'s
/// always-stable shape contract.
fn attach_auto_trigger_to_payload(payload: &mut Value, block: Value) {
    if let Some(obj) = payload.as_object_mut() {
        if let Some(status) = block.get("trigger_status").and_then(|v| v.as_str()) {
            obj.insert("auto_trigger_status".to_string(), json!(status));
        }
        if let Some(id) = block.get("chain_id").and_then(|v| v.as_str()) {
            obj.insert("auto_trigger_chain_id".to_string(), json!(id));
        }
        obj.insert("auto_trigger".to_string(), block);
    }
}

/// Top-level orchestrator. Decides whether the wave-19 explicit
/// `auto_chain=true` path runs, the wave-20 auto-trigger evaluates,
/// or the inner ToolResult is returned unmutated (default).
///
/// Order of operations:
///   1. Parse the trigger mode. A malformed mode short-circuits to a
///      structured error envelope so callers see the typo loud.
///   2. If trigger=Never AND `auto_chain=false` → return inner unchanged
///      (legacy fast path; zero overhead).
///   3. If trigger=Never AND `auto_chain=true` → delegate to wave-19
///      `maybe_apply_auto_chain` (existing behaviour preserved).
///   4. If trigger=AutoSafe → evaluate safety rules; on pass, route
///      through the wave-19 recorder AND splice the trigger block; on
///      fail, splice a `skipped_rules_failed` block WITHOUT calling
///      the recorder.
async fn maybe_apply_distill_chain_layers(
    state: &AppState,
    args: &Value,
    plan: &missiond_core::types::Plan,
    name: &str,
    inner: ToolResult,
) -> ToolResult {
    let trigger_mode =
        match parse_auto_chain_trigger(args.get("auto_chain_trigger").and_then(|v| v.as_str())) {
            Ok(m) => m,
            Err(msg) => {
                return ToolResult::structured_error(
                    ToolError::new(error_codes::INVALID_PARAM, msg).with_suggestion(
                        "auto_chain_trigger valid values: \"never\" (default) | \"auto_safe\"",
                    ),
                );
            }
        };

    let explicit_auto_chain = auto_chain_requested(args);
    let explicit_auto_sonnet = auto_sonnet_requested(args);
    // wave-22 / task 06 — closed-enum policy parser. Validation already
    // ran inside `action_distill`, so any non-Ok here is defensive only;
    // we collapse to Off on the unlikely fail-path so the chain still
    // runs.
    let auto_sonnet_policy = parse_auto_sonnet_policy(args).unwrap_or(AutoSonnetPolicy::Off);
    let policy_active = auto_sonnet_policy.is_active();

    // Fast path: nothing to do — return inner unchanged.
    //
    // wave-21 / task 07: when `auto_sonnet=true` is opted in WITHOUT the
    // wave-20 trigger AND without the wave-19 explicit chain, the gate
    // refuses the auto-apply (I3 — rules never ran) but still surfaces
    // a `skipped_no_trigger` block so the caller sees the missed
    // pre-condition.
    //
    // wave-22 / task 06: same pre-condition for the policy path —
    // `auto_sonnet_policy=safe_after_rules|dry_run` without the wave-20
    // trigger surfaces `skipped_no_trigger` on the policy block (I3).
    if trigger_mode == AutoChainTrigger::Never && !explicit_auto_chain {
        let mut result = inner;
        if explicit_auto_sonnet {
            result = maybe_apply_auto_sonnet_no_trigger(state, args, plan, name, result).await;
        }
        if policy_active {
            result = maybe_apply_auto_sonnet_policy(
                state,
                args,
                plan,
                name,
                result,
                auto_sonnet_policy,
                AutoSonnetTriggerContext {
                    trigger_mode,
                    rules_passed: false,
                    rules_value: Value::Array(Vec::new()),
                    sidecar: None,
                },
            )
            .await;
        }
        return result;
    }

    // Explicit wave-19 opt-in (no wave-20 trigger): preserve byte-compat
    // by delegating directly to the existing recorder. Wave-21 / task 07:
    // if `auto_sonnet=true` accompanies the wave-19 path, the gate
    // refuses (no trigger ran the safety rules) but layers a
    // `skipped_no_trigger` block on top. Wave-22 / task 06 mirrors the
    // refusal on the policy path.
    if trigger_mode == AutoChainTrigger::Never && explicit_auto_chain {
        let mut recorded = maybe_apply_auto_chain(state, args, plan, name, inner).await;
        if explicit_auto_sonnet {
            recorded = maybe_apply_auto_sonnet_no_trigger(state, args, plan, name, recorded).await;
        }
        if policy_active {
            recorded = maybe_apply_auto_sonnet_policy(
                state,
                args,
                plan,
                name,
                recorded,
                auto_sonnet_policy,
                AutoSonnetTriggerContext {
                    trigger_mode,
                    rules_passed: false,
                    rules_value: Value::Array(Vec::new()),
                    sidecar: None,
                },
            )
            .await;
        }
        return recorded;
    }

    // Wave-20 auto-trigger path. Inner errors short-circuit with a
    // dedicated status (so audit can tell rule failure apart from
    // upstream distill failure).
    if inner_result_is_error(&inner) {
        // Inner is an error envelope; we do not mutate it. We surface a
        // synthesised `auto_trigger` summary on a SECOND ToolResult would
        // mask the upstream error, so we return inner verbatim — the
        // caller already sees the failure loud.
        return inner;
    }

    // Re-project inner payload so we can splice. Anything non-object
    // returns inner unchanged (no silent payload synthesis — same rule
    // as wave-19).
    let mut payload = super::plan::tool_result_payload(&inner);
    if !payload.is_object() {
        return inner;
    }

    // Resolve project root + load evidence sidecar ONCE for both rule
    // evaluation and downstream chain-id derivation.
    let project_root_outcome = resolve_project_root_from_args(state, args).await;
    let (project_root_opt, project_resolve_error) = match &project_root_outcome {
        Ok(p) => (Some(p.clone()), None),
        Err(e) => (None, Some(e.clone())),
    };

    let evidence_path_opt = project_root_opt
        .as_ref()
        .map(|root| evidence_sidecar_path(root, plan.id));
    let evidence_outcome = match &evidence_path_opt {
        Some(path) => read_evidence_sidecar(path),
        None => EvidenceOutcome::Missing,
    };

    // Derive the candidate chain id (used by the dedup rule + by the
    // recorder downstream). When the project root failed to resolve we
    // skip derivation — R3 will fail and the trigger short-circuits
    // before we ever need the id.
    let workflow_id_str = payload
        .get("workflow_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let workflow_anchor = pick_workflow_anchor(workflow_id_str.as_deref(), name);
    let candidate_chain_id = project_root_opt.as_ref().map(|root| {
        let evidence_sha = evidence_path_opt
            .as_ref()
            .and_then(|p| compute_evidence_sha256(p));
        let evidence_sha_for_id = evidence_sha.unwrap_or_else(|| "<no-evidence>".to_string());
        derive_auto_chain_id(root, plan.id, &workflow_anchor, &evidence_sha_for_id)
    });

    let min_evidence = args
        .get("auto_trigger_min_evidence")
        .and_then(|v| v.as_i64())
        .map(|n| n.max(0) as usize)
        .unwrap_or(AUTO_TRIGGER_DEFAULT_MIN_EVIDENCE);

    let ctx = SafetyRuleContext {
        inner: &inner,
        inner_payload: &payload,
        project_root: project_root_opt.as_deref(),
        project_resolve_error: project_resolve_error.as_deref(),
        evidence_outcome: &evidence_outcome,
        candidate_chain_id: candidate_chain_id.as_deref(),
        min_evidence,
    };

    let (rules, all_passed) = evaluate_auto_trigger_safety_rules(&ctx);
    let rules_value = render_safety_rule_results(&rules);
    let sidecar_str = evidence_path_opt.as_ref().map(|p| p.display().to_string());

    if !all_passed {
        // Rule failure path: build a `skipped_rules_failed` block, splice
        // onto the payload (no chain_id), and return without recording.
        let block = build_auto_trigger_block(
            true,
            trigger_mode,
            AUTO_TRIGGER_STATUS_RULES_FAILED,
            rules_value.clone(),
            None,
            sidecar_str.as_deref(),
        );
        attach_auto_trigger_to_payload(&mut payload, block);

        // wave-21 / task 07: even on rules failure, if `auto_sonnet=true`
        // was opted in we surface a `skipped_rules_failed` auto-sonnet
        // block (mirrors the trigger's status) so the caller sees the
        // pre-condition that blocked Sonnet. wave-22 / task 06 layers
        // the policy block on top with the same `skipped_rules_failed`
        // status — I3 carryover proof.
        let mut result = ToolResult::json_pretty(&payload);
        if explicit_auto_sonnet {
            let ctx = AutoSonnetTriggerContext {
                trigger_mode,
                rules_passed: false,
                rules_value: rules_value.clone(),
                sidecar: sidecar_str.as_deref(),
            };
            result = maybe_apply_auto_sonnet(state, args, plan, name, result, ctx).await;
        }
        if policy_active {
            let ctx = AutoSonnetTriggerContext {
                trigger_mode,
                rules_passed: false,
                rules_value,
                sidecar: sidecar_str.as_deref(),
            };
            result = maybe_apply_auto_sonnet_policy(
                state,
                args,
                plan,
                name,
                result,
                auto_sonnet_policy,
                ctx,
            )
            .await;
        }
        return result;
    }

    // Rules passed: route the inner result through the wave-19 recorder
    // EXACTLY as if the caller had passed `auto_chain=true`. We
    // synthesise an args view with `auto_chain=true` flipped on so the
    // existing recorder sees an opt-in caller; downstream code then
    // attaches the wave-19 `auto_chain` block + shortcuts.
    let mut auto_args = args.clone();
    if let Some(obj) = auto_args.as_object_mut() {
        obj.insert("auto_chain".to_string(), json!(true));
    }
    let recorded = maybe_apply_auto_chain(state, &auto_args, plan, name, inner).await;

    // Re-project the recorded result so we can append the trigger block
    // alongside the wave-19 fields. If the recorder collapsed back to a
    // non-object (impossible in practice — `tool_result_payload` keeps
    // objects), we surrender and return its result verbatim.
    let mut recorded_payload = super::plan::tool_result_payload(&recorded);
    if !recorded_payload.is_object() {
        return recorded;
    }

    let recorded_chain_id = recorded_payload
        .get("auto_chain_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| candidate_chain_id.clone());
    let recorded_status = recorded_payload
        .get("auto_chain_status")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| AUTO_CHAIN_STATUS_RECORDED.to_string());

    let trigger_status = match recorded_status.as_str() {
        AUTO_CHAIN_STATUS_RECORDED => AUTO_TRIGGER_STATUS_TRIGGERED,
        AUTO_CHAIN_STATUS_RECORD_FAILED => AUTO_TRIGGER_STATUS_TRIGGERED_RECORD_FAILED,
        AUTO_CHAIN_STATUS_RESOLVE_FAILED => AUTO_TRIGGER_STATUS_TRIGGERED_RESOLVE_FAILED,
        // Defensive default: an unknown wave-19 status means the
        // recorder shape changed without us — fail loud rather than
        // guess. Surface the literal string verbatim.
        other => {
            tracing::warn!(
                wave19_status = other,
                "auto_trigger: unexpected wave-19 auto_chain status; surfacing verbatim"
            );
            return ToolResult::json_pretty(&recorded_payload);
        }
    };

    let block = build_auto_trigger_block(
        true,
        trigger_mode,
        trigger_status,
        rules_value.clone(),
        recorded_chain_id.as_deref(),
        sidecar_str.as_deref(),
    );
    attach_auto_trigger_to_payload(&mut recorded_payload, block);

    // wave-21 / task 07 — auto-sonnet apply-gate v1. Only reachable from
    // the wave-20 `auto_chain_trigger="auto_safe"` path AFTER all 6
    // safety rules already passed (we are inside the rules-passed
    // branch). The gate then layers an EXPLICIT caller-approval check
    // (`auto_sonnet=true` AND `auto_sonnet_approved=true`) and refuses
    // to auto-invoke Sonnet when the caller's `distill_mode` was
    // already `sonnet` (no double call). When all gates pass, Sonnet
    // is invoked and the inner `dry_run` payload is replaced with the
    // sonnet payload; on Sonnet failure (model error / invalid output)
    // the existing payload is preserved verbatim.
    //
    // wave-22 / task 06 — POLICY auto-sonnet apply-gate v2. Layered
    // AFTER the wave-21/07 layer so a caller can opt into either
    // surface (or both, in which case the policy block records the v2
    // verdict alongside the legacy v1 block — I7 additive).
    let recorded_inner = ToolResult::json_pretty(&recorded_payload);
    let mut after_legacy = maybe_apply_auto_sonnet(
        state,
        args,
        plan,
        name,
        recorded_inner,
        AutoSonnetTriggerContext {
            trigger_mode,
            rules_passed: true,
            rules_value: rules_value.clone(),
            sidecar: sidecar_str.as_deref(),
        },
    )
    .await;
    if policy_active {
        after_legacy = maybe_apply_auto_sonnet_policy(
            state,
            args,
            plan,
            name,
            after_legacy,
            auto_sonnet_policy,
            AutoSonnetTriggerContext {
                trigger_mode,
                rules_passed: true,
                rules_value,
                sidecar: sidecar_str.as_deref(),
            },
        )
        .await;
    }
    after_legacy
}

// ───────────────────────────────────────────────────────────────────────
// wave-21 / task 07 :: sonnet distill chain auto-apply v1
//
// Layered ON TOP of the wave-20 `auto_chain_trigger="auto_safe"` path.
// Only when ALL six wave-20 deterministic safety rules pass AND the
// caller explicitly opts in via `auto_sonnet=true` AND attests
// `auto_sonnet_approved=true` does the daemon promote the inner distill
// from `dry_run` to `sonnet` automatically.
//
// Conservative invariants (mirror wave-21 / task 04..06 patterns):
//   I1: DEFAULT `auto_sonnet=false`. Existing callers see byte-identical
//       wave-20 responses (no `auto_sonnet` block).
//   I2: NEVER auto-invoke Sonnet without BOTH `auto_sonnet=true` AND
//       explicit `auto_sonnet_approved=true`. The latter is a separate
//       caller-attestation flag — a single typo cannot escalate the
//       daemon into invoking Sonnet.
//   I3: NEVER auto-invoke Sonnet unless ALL six wave-20 safety rules
//       passed. The auto-sonnet gate REUSES the same rule outcomes the
//       trigger evaluated; it does not relax them.
//   I4: NEVER auto-invoke Sonnet when the caller's `distill_mode` was
//       already `sonnet` — the call already chose its mode; we refuse
//       to re-call.
//   I5: On Sonnet failure (model error / invalid output) PRESERVE the
//       existing inner payload. The auto-sonnet block surfaces
//       `model_call_status="failed"` / `"invalid_output"` and
//       `applied=false`; the dry_run distill artifact stays durable.
//   I6: `review_required=true` PINNED on every successful auto-sonnet
//       outcome in v1 — the auto-applied sonnet output is always
//       receipt-only; no DB transition flips, the operator still
//       reviews the distilled workflow before promoting.
//   I7: The wave-20 `auto_trigger.trigger_status="triggered"` and
//       wave-19 `auto_chain.status="recorded"` blocks remain
//       UNCHANGED. The auto-sonnet block is purely additive.
// ───────────────────────────────────────────────────────────────────────

/// Status surfaced under `auto_sonnet.status`. Audit consumers pin these
/// strings — never rename in-place.
const AUTO_SONNET_STATUS_NOT_REQUESTED: &str = "not_requested";
const AUTO_SONNET_STATUS_DISABLED: &str = "disabled";
const AUTO_SONNET_STATUS_SKIPPED_NO_TRIGGER: &str = "skipped_no_trigger";
const AUTO_SONNET_STATUS_SKIPPED_RULES_FAILED: &str = "skipped_rules_failed";
const AUTO_SONNET_STATUS_SKIPPED_NOT_APPROVED: &str = "skipped_caller_approval_missing";
const AUTO_SONNET_STATUS_SKIPPED_ALREADY_SONNET: &str = "skipped_already_sonnet";
const AUTO_SONNET_STATUS_SKIPPED_INNER_ERROR: &str = "skipped_inner_error";
const AUTO_SONNET_STATUS_APPLIED_SONNET: &str = "applied_sonnet";

/// Status surfaced under `auto_sonnet.model_call_status`. Pinned so the
/// audit shape stays stable across runs.
const AUTO_SONNET_MODEL_NOT_INVOKED: &str = "not_invoked";
const AUTO_SONNET_MODEL_INVOKED: &str = "invoked";
const AUTO_SONNET_MODEL_FAILED: &str = "failed";
const AUTO_SONNET_MODEL_INVALID_OUTPUT: &str = "invalid_output";

/// Strict pre-flight shape validator for the wave-21 / task 07 knobs.
/// Both `auto_sonnet` and `auto_sonnet_approved` MUST be booleans when
/// supplied — string `"true"` is rejected with INVALID_PARAM (mirrors
/// wave-21 / task 05's `validate_apply_gate_args`).
fn validate_auto_sonnet_args(args: &Value) -> std::result::Result<(), String> {
    if let Some(v) = args.get("auto_sonnet") {
        if !v.is_boolean() {
            return Err(format!(
                "auto_sonnet must be a boolean (true|false); got {}",
                shape_label(v)
            ));
        }
    }
    if let Some(v) = args.get("auto_sonnet_approved") {
        if !v.is_boolean() {
            return Err(format!(
                "auto_sonnet_approved must be a boolean (true|false); got {}",
                shape_label(v)
            ));
        }
    }
    Ok(())
}

/// Tiny helper to render the JSON shape label for INVALID_PARAM
/// diagnostics. Keeps the validator messages stable across upgrades.
fn shape_label(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Pure check: caller opted into auto-sonnet via `auto_sonnet=true`.
/// Defaults to false on missing / non-bool / null shapes (validator
/// already rejected non-bool above; this guard preserves byte-compat
/// for callers that omit the key).
fn auto_sonnet_requested(args: &Value) -> bool {
    args.get("auto_sonnet")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Pure check: caller attested `auto_sonnet_approved=true`. Same
/// strict-bool semantics as `auto_sonnet_requested`. Both flags must
/// flip true independently — a single typo cannot escalate the daemon.
fn auto_sonnet_caller_approved(args: &Value) -> bool {
    args.get("auto_sonnet_approved")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Pure check: caller's original `distill_mode` was already `sonnet`.
/// The auto-apply gate refuses to double-invoke Sonnet on the same
/// call (it would land a duplicate sidecar entry + waste tokens).
fn caller_already_chose_sonnet(args: &Value) -> bool {
    matches!(
        args.get("distill_mode").and_then(|v| v.as_str()),
        Some("sonnet")
    )
}

/// Context passed into `maybe_apply_auto_sonnet` from the wave-20
/// trigger orchestrator. The trigger has already paid for project
/// resolution + sidecar load + rule evaluation — we forward the
/// outcomes verbatim so the auto-sonnet gate doesn't re-do the work.
struct AutoSonnetTriggerContext<'a> {
    trigger_mode: AutoChainTrigger,
    rules_passed: bool,
    rules_value: Value,
    sidecar: Option<&'a str>,
}

/// Build the `auto_sonnet` block surfaced under the response payload.
/// Always carries `requested`, `status`, `applied`, `review_required`,
/// `model_call_status`, and `safety_rule_results`. Optional fields
/// (`model_call_error`, `sidecar`, `chain_id`, `caller_approval`,
/// `caller_distill_mode`) are surfaced when known.
#[allow(clippy::too_many_arguments)]
fn build_auto_sonnet_block(
    requested: bool,
    status: &str,
    applied: bool,
    review_required: bool,
    model_call_status: &str,
    safety_rule_results: Value,
    caller_approval: bool,
    caller_distill_mode: Option<&str>,
    model_call_error: Option<&str>,
    sidecar: Option<&str>,
    chain_id: Option<&str>,
) -> Value {
    let mut block = json!({
        "requested": requested,
        "status": status,
        "applied": applied,
        "review_required": review_required,
        "model_call_status": model_call_status,
        "safety_rule_results": safety_rule_results,
        "caller_approval": caller_approval,
    });
    if let Some(m) = caller_distill_mode {
        block["caller_distill_mode"] = json!(m);
    }
    if let Some(e) = model_call_error {
        block["model_call_error"] = json!(e);
    }
    if let Some(p) = sidecar {
        block["sidecar"] = json!(p);
    }
    if let Some(id) = chain_id {
        block["chain_id"] = json!(id);
    }
    block
}

/// Splice the `auto_sonnet` block + top-level shortcut onto the
/// response payload. Mirrors the wave-19 / wave-20 attach helpers.
fn attach_auto_sonnet_to_payload(payload: &mut Value, block: Value) {
    if let Some(obj) = payload.as_object_mut() {
        if let Some(status) = block.get("status").and_then(|v| v.as_str()) {
            obj.insert("auto_sonnet_status".to_string(), json!(status));
        }
        obj.insert("auto_sonnet".to_string(), block);
    }
}

/// Top-level orchestrator for the wave-21 / task 07 auto-apply gate.
///
/// Order of operations:
///   1. If `auto_sonnet` was not requested → return inner unchanged
///      (default-off byte-compat path).
///   2. If inner payload is a structured error → splice
///      `skipped_inner_error` block; do NOT call Sonnet.
///   3. If wave-20 trigger mode is `Never` → splice `skipped_no_trigger`
///      block; the gate refuses to operate without the deterministic
///      trigger context.
///   4. If wave-20 rules failed → splice `skipped_rules_failed` block;
///      the gate REUSES the trigger's rule outcomes — it does not
///      relax them.
///   5. If `auto_sonnet_approved` was not set → splice
///      `skipped_caller_approval_missing` block.
///   6. If caller's `distill_mode` was already `sonnet` → splice
///      `skipped_already_sonnet` block.
///   7. All gates pass → invoke `mission_workflow(action=distill,
///      distill_mode="sonnet", …)` internally. On success, replace the
///      inner payload with the sonnet payload + splice the
///      `auto_sonnet` block. On model failure / invalid output,
///      PRESERVE the inner payload + splice the failure block.
async fn maybe_apply_auto_sonnet<'a>(
    state: &AppState,
    args: &Value,
    plan: &missiond_core::types::Plan,
    name: &str,
    inner: ToolResult,
    ctx: AutoSonnetTriggerContext<'a>,
) -> ToolResult {
    if !auto_sonnet_requested(args) {
        // I1: default-off byte-compat — return inner verbatim.
        return inner;
    }

    // From here on the caller opted in; surface a block on every branch.
    let inner_is_error = inner_result_is_error(&inner);
    let mut payload = super::plan::tool_result_payload(&inner);
    if !payload.is_object() {
        // Non-object payload (defensive — wave-19/20 always emit
        // objects). Surrender without splicing.
        return inner;
    }

    let caller_approval = auto_sonnet_caller_approved(args);
    let caller_already_sonnet = caller_already_chose_sonnet(args);
    let caller_mode = args
        .get("distill_mode")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    let chain_id = payload
        .get("auto_chain_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // I5 short-circuit: inner error envelope. We never call Sonnet on
    // top of an error and we never overwrite the error envelope.
    if inner_is_error {
        let block = build_auto_sonnet_block(
            true,
            AUTO_SONNET_STATUS_SKIPPED_INNER_ERROR,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_approval,
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // Trigger context check. We only operate inside the wave-20
    // auto_safe path so the deterministic safety rules already ran.
    if ctx.trigger_mode == AutoChainTrigger::Never {
        let block = build_auto_sonnet_block(
            true,
            AUTO_SONNET_STATUS_SKIPPED_NO_TRIGGER,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_approval,
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // I3: rules MUST have all passed. The auto-sonnet gate REUSES the
    // trigger's rule outcomes; it does not re-evaluate or relax them.
    if !ctx.rules_passed {
        let block = build_auto_sonnet_block(
            true,
            AUTO_SONNET_STATUS_SKIPPED_RULES_FAILED,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_approval,
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // I2: caller approval MUST be explicit. A single typo on
    // `auto_sonnet_approved` cannot escalate the daemon into invoking
    // Sonnet — it stays at `disabled` even when `auto_sonnet=true`.
    if !caller_approval {
        let block = build_auto_sonnet_block(
            true,
            AUTO_SONNET_STATUS_SKIPPED_NOT_APPROVED,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_approval,
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // I4: caller's `distill_mode` must NOT already be sonnet. We refuse
    // to double-invoke (would land a duplicate sidecar entry + burn
    // tokens). The caller's existing sonnet payload is already in the
    // response; we just surface a `skipped_already_sonnet` status.
    if caller_already_sonnet {
        let block = build_auto_sonnet_block(
            true,
            AUTO_SONNET_STATUS_SKIPPED_ALREADY_SONNET,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_approval,
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // All gates passed — invoke the sonnet distiller internally. We
    // synthesise an args view with `distill_mode="sonnet"` flipped on,
    // and `auto_sonnet=false` to prevent infinite recursion. The
    // wave-19 `auto_chain` + wave-20 `auto_trigger` blocks already
    // landed on the parent payload — we strip them from the synthesised
    // args (`auto_chain_trigger=never`, `auto_chain=false`) so the
    // sonnet sub-call only runs the distiller and does NOT re-record a
    // chain entry (the parent already did).
    let mut sonnet_args = args.clone();
    if let Some(obj) = sonnet_args.as_object_mut() {
        obj.insert("distill_mode".to_string(), json!("sonnet"));
        obj.insert("auto_sonnet".to_string(), json!(false));
        obj.insert("auto_sonnet_approved".to_string(), json!(false));
        obj.insert("auto_chain".to_string(), json!(false));
        obj.insert("auto_chain_trigger".to_string(), json!("never"));
    }

    // Re-run the inner sonnet path directly. We bypass the top-level
    // `action_distill` dispatcher because we already validated args and
    // already ran the auto-chain layer; calling `action_distill_sonnet`
    // gives us the bare sonnet payload without re-entering the chain
    // orchestrator.
    let persist = args
        .get("persist")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let sonnet_outcome = action_distill_sonnet(state, &sonnet_args, plan, name, persist).await;

    let sonnet_result = match sonnet_outcome {
        Ok(tr) => tr,
        Err(e) => {
            // Handler-level error → preserve inner payload, surface
            // `model_call_status="failed"` with the error text. The
            // dry_run / record-only distill artifact stays durable
            // (I5).
            let block = build_auto_sonnet_block(
                true,
                AUTO_SONNET_STATUS_APPLIED_SONNET,
                false,
                true,
                AUTO_SONNET_MODEL_FAILED,
                ctx.rules_value.clone(),
                caller_approval,
                caller_mode,
                Some(&format!("sonnet handler error: {}", e)),
                ctx.sidecar,
                chain_id.as_deref(),
            );
            attach_auto_sonnet_to_payload(&mut payload, block);
            return ToolResult::json_pretty(&payload);
        }
    };

    // I5: structured error envelope from the sonnet path → preserve
    // inner payload; surface `model_call_status="invalid_output"` (the
    // gateway returned, but the distiller refused — model returned bad
    // JSON / missing fields / unbalanced sexp).
    if sonnet_result.is_error.unwrap_or(false) {
        let err_payload = super::plan::tool_result_payload(&sonnet_result);
        let err_text = err_payload
            .get("error")
            .and_then(|e| e.get("message").or_else(|| e.get("code")))
            .and_then(|v| v.as_str())
            .unwrap_or("sonnet returned a structured error envelope")
            .to_string();
        let block = build_auto_sonnet_block(
            true,
            AUTO_SONNET_STATUS_APPLIED_SONNET,
            false,
            true,
            AUTO_SONNET_MODEL_INVALID_OUTPUT,
            ctx.rules_value.clone(),
            caller_approval,
            caller_mode,
            Some(&err_text),
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // Sonnet succeeded — promote the sonnet payload as the new outer
    // payload (it carries `distill_mode="sonnet"`, `compiler_model`,
    // `workflow_sexp`, `match_rules`, etc.). We carry forward the
    // wave-19 / wave-20 blocks from the original inner payload so the
    // chain recorder + trigger receipts remain visible.
    let mut sonnet_payload = super::plan::tool_result_payload(&sonnet_result);
    if !sonnet_payload.is_object() {
        // Defensive — sonnet path always emits an object; preserve
        // inner if it didn't.
        let block = build_auto_sonnet_block(
            true,
            AUTO_SONNET_STATUS_APPLIED_SONNET,
            false,
            true,
            AUTO_SONNET_MODEL_INVALID_OUTPUT,
            ctx.rules_value.clone(),
            caller_approval,
            caller_mode,
            Some("sonnet payload was not a JSON object"),
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // Carry forward wave-19 + wave-20 receipt fields from the original
    // dry_run payload so observers do not lose the chain recording.
    if let Some(obj) = sonnet_payload.as_object_mut() {
        for key in [
            "auto_chain",
            "auto_chain_status",
            "auto_chain_id",
            "auto_trigger",
            "auto_trigger_status",
            "auto_trigger_chain_id",
        ] {
            if let Some(v) = payload.get(key).cloned() {
                obj.entry(key.to_string()).or_insert(v);
            }
        }
    }

    // I6: review_required PINNED true on the auto-sonnet block; the
    // sonnet payload itself already carries `review_required: true`
    // for callers grepping that field directly.
    let block = build_auto_sonnet_block(
        true,
        AUTO_SONNET_STATUS_APPLIED_SONNET,
        true,
        true,
        AUTO_SONNET_MODEL_INVOKED,
        ctx.rules_value.clone(),
        caller_approval,
        caller_mode,
        None,
        ctx.sidecar,
        chain_id.as_deref(),
    );
    attach_auto_sonnet_to_payload(&mut sonnet_payload, block);
    ToolResult::json_pretty(&sonnet_payload)
}

/// Wave-21 / task 07 helper: the wave-19 explicit `auto_chain=true`
/// path (no wave-20 trigger) does NOT pre-evaluate the safety rules —
/// it just records. When the caller flips `auto_sonnet=true` on that
/// path the gate must still refuse (I3) because the deterministic rule
/// set was never run. We surface a `skipped_no_trigger` status so the
/// caller sees the missed pre-condition explicitly.
///
/// This wrapper is invoked from the wave-19 explicit path inside
/// `maybe_apply_distill_chain_layers` (see the early-return branch
/// for `trigger=Never AND auto_chain=true`).
async fn maybe_apply_auto_sonnet_no_trigger(
    state: &AppState,
    args: &Value,
    plan: &missiond_core::types::Plan,
    name: &str,
    inner: ToolResult,
) -> ToolResult {
    if !auto_sonnet_requested(args) {
        return inner;
    }
    let ctx = AutoSonnetTriggerContext {
        trigger_mode: AutoChainTrigger::Never,
        rules_passed: false,
        rules_value: Value::Array(Vec::new()),
        sidecar: None,
    };
    maybe_apply_auto_sonnet(state, args, plan, name, inner, ctx).await
}

// ───────────────────────────────────────────────────────────────────────
// wave-22 / task 06 :: distill chain POLICY auto-Sonnet v2
//
// Replaces the wave-21 / task 07 dual opt-in (`auto_sonnet=true` AND
// `auto_sonnet_approved=true`) with a single explicit policy gate
// `auto_sonnet_policy ∈ {off, safe_after_rules, dry_run}`. The legacy
// dual opt-in path stays available for byte-shape back-compat (off →
// wave-21/07 unchanged); supplying any non-off policy promotes the
// daemon to the policy path which surfaces a `auto_sonnet_policy`
// block instead of (or in addition to, when both opt-ins land on the
// same call) the legacy `auto_sonnet` block.
//
// All seven wave-21/07 invariants STAY pinned on the policy path:
//   I1: DEFAULT `auto_sonnet_policy=off`. No `auto_sonnet_policy`
//       block emitted; existing callers see byte-identical
//       wave-21/07 + wave-20 responses.
//   I2: closed-enum strict-shape parser; string typo / unknown values
//       fail-fast as INVALID_PARAM. A single typo cannot escalate the
//       daemon — a malformed policy is rejected at action entry, a
//       missing policy stays `off`. The policy itself is the single
//       opt-in; supplying `safe_after_rules` IS the explicit operator
//       attestation (it is documented as the "auto-promote inner
//       distill from dry_run to sonnet" choice).
//   I3: `safe_after_rules` REUSES the wave-20 trigger's rule outcomes
//       verbatim — never relaxes them. The trigger MUST be
//       `auto_safe` AND ALL six deterministic rules MUST have passed
//       before the policy fires. Any rule failure surfaces
//       `skipped_rules_failed` with the full safety_rule_results.
//   I4: caller's `distill_mode=sonnet` still rejected (no double
//       Sonnet call). Surfaces `skipped_already_sonnet`.
//   I5: on Sonnet failure (model error / invalid output) PRESERVE
//       the existing inner payload. Policy block surfaces
//       `model_call_status="failed"|"invalid_output"` and
//       `applied=false`; the dry_run distill artifact stays durable.
//   I6: `review_required=true` PINNED on every successful
//       `safe_after_rules_applied` outcome — the auto-applied sonnet
//       output is always receipt-only; no DB transition flips, the
//       operator still reviews the distilled workflow.
//   I7: wave-19 `auto_chain` + wave-20 `auto_trigger` blocks remain
//       UNCHANGED. The `auto_sonnet_policy` block is purely additive.
//
// Block shape (always carried when policy != off):
//   {requested, policy, policy_status, applied, review_required,
//    model_call_status, safety_rule_results, sidecar?, chain_id?,
//    model_call_error?, caller_distill_mode?}
//   plus top-level shortcut `auto_sonnet_policy_status`.
// ───────────────────────────────────────────────────────────────────────

/// Closed-enum policy value surfaced in `auto_sonnet_policy.policy`.
/// Wire strings are pinned by `auto_sonnet_policy_value_strings_pin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoSonnetPolicy {
    Off,
    SafeAfterRules,
    DryRun,
}

const AUTO_SONNET_POLICY_OFF_STR: &str = "off";
const AUTO_SONNET_POLICY_SAFE_AFTER_RULES_STR: &str = "safe_after_rules";
const AUTO_SONNET_POLICY_DRY_RUN_STR: &str = "dry_run";

impl AutoSonnetPolicy {
    fn as_wire(self) -> &'static str {
        match self {
            AutoSonnetPolicy::Off => AUTO_SONNET_POLICY_OFF_STR,
            AutoSonnetPolicy::SafeAfterRules => AUTO_SONNET_POLICY_SAFE_AFTER_RULES_STR,
            AutoSonnetPolicy::DryRun => AUTO_SONNET_POLICY_DRY_RUN_STR,
        }
    }

    fn is_active(self) -> bool {
        !matches!(self, AutoSonnetPolicy::Off)
    }
}

/// `auto_sonnet_policy.policy_status` taxonomy. Pinned by
/// `auto_sonnet_policy_status_constants_pin_the_wire_form`.
const AUTO_SONNET_POLICY_STATUS_NOT_REQUESTED: &str = "not_requested";
const AUTO_SONNET_POLICY_STATUS_OFF: &str = "off";
const AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED: &str = "safe_after_rules_applied";
const AUTO_SONNET_POLICY_STATUS_SAFE_DRY_RUN: &str = "safe_after_rules_dry_run";
const AUTO_SONNET_POLICY_STATUS_SKIPPED_NO_TRIGGER: &str = "skipped_no_trigger";
const AUTO_SONNET_POLICY_STATUS_SKIPPED_RULES_FAILED: &str = "skipped_rules_failed";
const AUTO_SONNET_POLICY_STATUS_SKIPPED_ALREADY_SONNET: &str = "skipped_already_sonnet";
const AUTO_SONNET_POLICY_STATUS_SKIPPED_INNER_ERROR: &str = "skipped_inner_error";

/// Strict closed-enum parser for the wave-22 / task 06 policy knob.
/// Returns Ok(Off) for missing / null / empty string (back-compat with
/// wave-21/07 callers). Rejects unknown values + non-string shapes
/// loudly so a typo (`"safe-after-rules"`, `"safeAfterRules"`, `1`)
/// surfaces as INVALID_PARAM at action entry — a single typo can NEVER
/// silently escalate the daemon (I2 carryover from wave-21/07).
fn parse_auto_sonnet_policy(args: &Value) -> std::result::Result<AutoSonnetPolicy, String> {
    let raw = match args.get("auto_sonnet_policy") {
        None => return Ok(AutoSonnetPolicy::Off),
        Some(Value::Null) => return Ok(AutoSonnetPolicy::Off),
        Some(v) => v,
    };
    let s = match raw.as_str() {
        Some(s) => s,
        None => {
            return Err(format!(
                "auto_sonnet_policy must be a string (one of [\"off\",\"safe_after_rules\",\"dry_run\"]); got {}",
                shape_label(raw)
            ));
        }
    };
    match s {
        "" | AUTO_SONNET_POLICY_OFF_STR => Ok(AutoSonnetPolicy::Off),
        AUTO_SONNET_POLICY_SAFE_AFTER_RULES_STR => Ok(AutoSonnetPolicy::SafeAfterRules),
        AUTO_SONNET_POLICY_DRY_RUN_STR => Ok(AutoSonnetPolicy::DryRun),
        other => Err(format!(
            "auto_sonnet_policy must be one of [\"off\",\"safe_after_rules\",\"dry_run\"]; got `{}`",
            other
        )),
    }
}

/// Build the `auto_sonnet_policy` block. Mirrors `build_auto_sonnet_block`
/// but anchored on the policy enum so the wire surface stays decoupled
/// from the wave-21/07 dual opt-in vocabulary.
#[allow(clippy::too_many_arguments)]
fn build_auto_sonnet_policy_block(
    requested: bool,
    policy: AutoSonnetPolicy,
    policy_status: &str,
    applied: bool,
    review_required: bool,
    model_call_status: &str,
    safety_rule_results: Value,
    caller_distill_mode: Option<&str>,
    model_call_error: Option<&str>,
    sidecar: Option<&str>,
    chain_id: Option<&str>,
) -> Value {
    let mut block = json!({
        "requested": requested,
        "policy": policy.as_wire(),
        "policy_status": policy_status,
        "applied": applied,
        "review_required": review_required,
        "model_call_status": model_call_status,
        "safety_rule_results": safety_rule_results,
    });
    if let Some(m) = caller_distill_mode {
        block["caller_distill_mode"] = json!(m);
    }
    if let Some(e) = model_call_error {
        block["model_call_error"] = json!(e);
    }
    if let Some(p) = sidecar {
        block["sidecar"] = json!(p);
    }
    if let Some(id) = chain_id {
        block["chain_id"] = json!(id);
    }
    block
}

/// Splice the `auto_sonnet_policy` block + top-level shortcut onto the
/// response payload. Mirrors the wave-19 / wave-20 / wave-21 attach
/// helpers — preserves any pre-existing payload fields.
fn attach_auto_sonnet_policy_to_payload(payload: &mut Value, block: Value) {
    if let Some(obj) = payload.as_object_mut() {
        if let Some(status) = block.get("policy_status").and_then(|v| v.as_str()) {
            obj.insert("auto_sonnet_policy_status".to_string(), json!(status));
        }
        obj.insert("auto_sonnet_policy".to_string(), block);
    }
}

/// Top-level orchestrator for the wave-22 / task 06 policy gate.
///
/// Order of operations (executed AFTER the wave-21/07 layer, so the
/// inner payload may already carry `auto_sonnet*` keys from the legacy
/// dual opt-in path — those are preserved verbatim):
///   0. Policy=Off → return inner unchanged (default-off byte-compat
///      with wave-21/07).
///   1. Inner payload is a structured error → splice
///      `skipped_inner_error` block; do NOT call Sonnet.
///   2. Wave-20 trigger is `Never` → splice `skipped_no_trigger`
///      block; the policy refuses to operate without the deterministic
///      trigger context (I3 — rules never ran).
///   3. Wave-20 rules failed → splice `skipped_rules_failed` block
///      (REUSES the trigger's rule outcomes verbatim — I3).
///   4. Caller's `distill_mode` was already `sonnet` → splice
///      `skipped_already_sonnet` block (I4 — no double-call).
///   5. Policy=DryRun → splice `safe_after_rules_dry_run` block; do
///      NOT call Sonnet. Used for testing the gate path without
///      burning model tokens.
///   6. Policy=SafeAfterRules + all gates pass → invoke
///      `mission_workflow(action=distill, distill_mode="sonnet", …)`
///      internally. On success, replace the inner payload with the
///      sonnet payload + splice the policy block. On model failure /
///      invalid output, PRESERVE the inner payload + splice the
///      failure block (I5).
async fn maybe_apply_auto_sonnet_policy<'a>(
    state: &AppState,
    args: &Value,
    plan: &missiond_core::types::Plan,
    name: &str,
    inner: ToolResult,
    policy: AutoSonnetPolicy,
    ctx: AutoSonnetTriggerContext<'a>,
) -> ToolResult {
    if !policy.is_active() {
        return inner;
    }

    let inner_is_error = inner_result_is_error(&inner);
    let mut payload = super::plan::tool_result_payload(&inner);
    if !payload.is_object() {
        // Defensive — wave-19/20/21 always emit objects.
        return inner;
    }

    let caller_already_sonnet = caller_already_chose_sonnet(args);
    let caller_mode = args
        .get("distill_mode")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    let chain_id = payload
        .get("auto_chain_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // I5 short-circuit: inner error envelope. Never call Sonnet on top
    // of an error and never overwrite the error envelope.
    if inner_is_error {
        let block = build_auto_sonnet_policy_block(
            true,
            policy,
            AUTO_SONNET_POLICY_STATUS_SKIPPED_INNER_ERROR,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_policy_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // I3 pre-condition: trigger context required. Without the wave-20
    // auto_safe trigger the deterministic rules never ran, so the
    // policy refuses to operate.
    if ctx.trigger_mode == AutoChainTrigger::Never {
        let block = build_auto_sonnet_policy_block(
            true,
            policy,
            AUTO_SONNET_POLICY_STATUS_SKIPPED_NO_TRIGGER,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_policy_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // I3: ALL six wave-20 safety rules MUST have passed. Reuses the
    // trigger's outcomes verbatim — never re-evaluates or relaxes.
    if !ctx.rules_passed {
        let block = build_auto_sonnet_policy_block(
            true,
            policy,
            AUTO_SONNET_POLICY_STATUS_SKIPPED_RULES_FAILED,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_policy_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // I4: caller's distill_mode must NOT already be sonnet.
    if caller_already_sonnet {
        let block = build_auto_sonnet_policy_block(
            true,
            policy,
            AUTO_SONNET_POLICY_STATUS_SKIPPED_ALREADY_SONNET,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_policy_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // Policy=DryRun: surface the gate verdict but do NOT invoke
    // Sonnet. Used for testing the policy path end-to-end without
    // burning model tokens. `applied=false`, `model_call_status=
    // not_invoked`, `review_required=true` PINNED.
    if matches!(policy, AutoSonnetPolicy::DryRun) {
        let block = build_auto_sonnet_policy_block(
            true,
            policy,
            AUTO_SONNET_POLICY_STATUS_SAFE_DRY_RUN,
            false,
            true,
            AUTO_SONNET_MODEL_NOT_INVOKED,
            ctx.rules_value.clone(),
            caller_mode,
            None,
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_policy_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // Policy=SafeAfterRules + all gates passed → invoke the sonnet
    // distiller internally. Synthesise an args view with
    // `distill_mode="sonnet"` flipped on, `auto_sonnet_policy="off"`
    // (anti-recursion) AND clear the wave-21/07 dual opt-in flags +
    // wave-19/20 chain knobs so the sonnet sub-call only runs the
    // distiller and does NOT re-record a chain entry (the parent
    // already did via the wave-19 + wave-20 path).
    let mut sonnet_args = args.clone();
    if let Some(obj) = sonnet_args.as_object_mut() {
        obj.insert("distill_mode".to_string(), json!("sonnet"));
        obj.insert("auto_sonnet_policy".to_string(), json!("off"));
        obj.insert("auto_sonnet".to_string(), json!(false));
        obj.insert("auto_sonnet_approved".to_string(), json!(false));
        obj.insert("auto_chain".to_string(), json!(false));
        obj.insert("auto_chain_trigger".to_string(), json!("never"));
    }

    let persist = args
        .get("persist")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let sonnet_outcome = action_distill_sonnet(state, &sonnet_args, plan, name, persist).await;

    let sonnet_result = match sonnet_outcome {
        Ok(tr) => tr,
        Err(e) => {
            // I5: handler-level error → preserve inner payload, surface
            // `model_call_status="failed"`. The dry_run / record-only
            // distill artifact stays durable.
            let block = build_auto_sonnet_policy_block(
                true,
                policy,
                AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED,
                false,
                true,
                AUTO_SONNET_MODEL_FAILED,
                ctx.rules_value.clone(),
                caller_mode,
                Some(&format!("sonnet handler error: {}", e)),
                ctx.sidecar,
                chain_id.as_deref(),
            );
            attach_auto_sonnet_policy_to_payload(&mut payload, block);
            return ToolResult::json_pretty(&payload);
        }
    };

    // I5: structured error envelope from the sonnet path → preserve
    // inner payload; surface `model_call_status="invalid_output"`.
    if sonnet_result.is_error.unwrap_or(false) {
        let err_payload = super::plan::tool_result_payload(&sonnet_result);
        let err_text = err_payload
            .get("error")
            .and_then(|e| e.get("message").or_else(|| e.get("code")))
            .and_then(|v| v.as_str())
            .unwrap_or("sonnet returned a structured error envelope")
            .to_string();
        let block = build_auto_sonnet_policy_block(
            true,
            policy,
            AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED,
            false,
            true,
            AUTO_SONNET_MODEL_INVALID_OUTPUT,
            ctx.rules_value.clone(),
            caller_mode,
            Some(&err_text),
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_policy_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // Sonnet succeeded — promote the sonnet payload as the new outer
    // payload. Carry forward the wave-19/20/21 receipt blocks from the
    // original inner payload so chain recorder + trigger receipts +
    // legacy auto_sonnet block (if any) remain visible.
    let mut sonnet_payload = super::plan::tool_result_payload(&sonnet_result);
    if !sonnet_payload.is_object() {
        let block = build_auto_sonnet_policy_block(
            true,
            policy,
            AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED,
            false,
            true,
            AUTO_SONNET_MODEL_INVALID_OUTPUT,
            ctx.rules_value.clone(),
            caller_mode,
            Some("sonnet payload was not a JSON object"),
            ctx.sidecar,
            chain_id.as_deref(),
        );
        attach_auto_sonnet_policy_to_payload(&mut payload, block);
        return ToolResult::json_pretty(&payload);
    }

    // I7: carry forward wave-19 + wave-20 + wave-21/07 receipt fields
    // from the original dry_run payload so observers do not lose the
    // chain recording / trigger receipts / legacy auto_sonnet block.
    if let Some(obj) = sonnet_payload.as_object_mut() {
        for key in [
            "auto_chain",
            "auto_chain_status",
            "auto_chain_id",
            "auto_trigger",
            "auto_trigger_status",
            "auto_trigger_chain_id",
            "auto_sonnet",
            "auto_sonnet_status",
        ] {
            if let Some(v) = payload.get(key).cloned() {
                obj.entry(key.to_string()).or_insert(v);
            }
        }
    }

    // I6: review_required PINNED true on every successful auto-sonnet
    // policy outcome.
    let block = build_auto_sonnet_policy_block(
        true,
        policy,
        AUTO_SONNET_POLICY_STATUS_SAFE_APPLIED,
        true,
        true,
        AUTO_SONNET_MODEL_INVOKED,
        ctx.rules_value.clone(),
        caller_mode,
        None,
        ctx.sidecar,
        chain_id.as_deref(),
    );
    attach_auto_sonnet_policy_to_payload(&mut sonnet_payload, block);
    ToolResult::json_pretty(&sonnet_payload)
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
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                msg,
            )))
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
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::MISSING_PARAM,
                msg,
            )))
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

    let content =
        std::fs::read_to_string(&path).map_err(|e| anyhow!("read {}: {}", path.display(), e))?;

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
        return Ok(ToolResult::structured_error(ToolError::new(
            error_codes::INVALID_PARAM,
            format!(
                "generated YAML already exists at {}; pass overwrite=true to replace",
                yaml_path.display()
            ),
        )));
    }
    atomic_write(&yaml_path, &yaml).map_err(|e| anyhow!("write {}: {}", yaml_path.display(), e))?;

    payload["persisted"] = json!(true);
    payload["flow_path"] = json!(yaml_path.display().to_string());
    payload["next_step"] = json!(
        "run_methodology(flow_id=<flow_id>, dry_run=true) to verify; dry_run=false to dispatch into mission_flow_run"
    );

    // wave-14/38 :: file-first SSOT mirror. compile_methodology already reads
    // the methodology lisp from `.missiond/workflows/<name>.lisp`, so the
    // file-first writer is only meaningful when the caller wants to
    // canonicalise / snapshot the source under a different topic, OR when
    // the caller passes overwrite_file=true to "re-emit" the same file.
    // Topic precedence: explicit `topic` arg > `name` arg > source stem.
    //
    // wave38-01 :: project the methodology compile as the same enriched V3
    // workflow artifact shape distill writes (render_workflow_artifact_sexp).
    // The methodology branch never produces a Workflow DB row, so :workflow_id
    // is stamped with the generated `flow_id` (deterministic, derived from
    // stem + output_flow_id) instead of a UUID; :source_plans stays empty;
    // :match_rules carries source_kind/compiler/compiler_version/source_hash/
    // flow_id/source_path/generated_at so reviewers can correlate the .lisp
    // artifact with the generated YAML; :steps re-runs the same step
    // extractor distill uses; :status is compiled (or compiled_review_required
    // when the methodology has no executable steps); :body is the methodology
    // Lisp body verbatim. Reviewers therefore see the V3 contract artifact,
    // not a raw source mirror. No DB migration is introduced.
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
    let methodology_status = if review_required {
        "compiled_review_required"
    } else {
        "compiled"
    };
    let methodology_match_rules = build_methodology_match_rules(&meta);
    let methodology_artifact_sexp = render_workflow_artifact_sexp(
        &meta.flow_id,
        &[],
        &methodology_match_rules,
        methodology_status,
        content,
    );
    maybe_write_workflow_artifact(
        state,
        &file_args,
        &mut payload,
        &methodology_artifact_sexp,
        &fallback_topic,
    )
    .await;

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

    match resolve_target_project_root(project, cwd_path.as_deref(), target_project, registry).await
    {
        Ok(r) => Ok(r.project_root),
        Err(ResolutionError::NoSignal) => Err(
            "no project_id, absolute cwd, or fallback target_project supplied; \
             workflow file writer refuses process-cwd fallback"
                .to_string(),
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

// ───────────────────────────────────────────────────────────────────────
// tests
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
