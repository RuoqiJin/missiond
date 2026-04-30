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
mod auto_chain;
mod auto_sonnet;
mod methodology;
mod review_resolution;

use artifacts::{
    build_methodology_match_rules, extract_workflow_file_args, maybe_write_workflow_artifact,
    render_workflow_artifact_sexp,
};
use auto_chain::maybe_apply_distill_chain_layers;
#[cfg(test)]
use auto_chain::*;
#[cfg(test)]
use auto_sonnet::*;
use auto_sonnet::{parse_auto_sonnet_policy, validate_auto_sonnet_args};
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
