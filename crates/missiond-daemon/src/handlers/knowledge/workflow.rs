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
use crate::slot_orchestrator::project_root::{resolve_target_project_root, ResolutionError};
use crate::state::AppState;

mod artifacts;
mod auto_chain;
mod auto_sonnet;
mod compile_methodology;
mod distill;
mod methodology;
mod project_root;
mod review_resolution;
mod run_methodology;
mod store_actions;

use artifacts::{
    build_methodology_match_rules, extract_workflow_file_args, maybe_write_workflow_artifact,
    render_workflow_artifact_sexp,
};
#[cfg(test)]
use auto_chain::*;
#[cfg(test)]
use auto_sonnet::*;
use compile_methodology::action_compile_methodology;
#[cfg(test)]
use compile_methodology::{count_top_form, parse_compile_mode, CompileMode};
use distill::action_distill;
#[cfg(test)]
use distill::*;
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
use project_root::resolve_project_root_from_args;
#[cfg(test)]
use project_root::resolve_project_root_with_registry;
use review_resolution::action_resolve_review;
pub(crate) use review_resolution::{handle_review_resolved_event, WorkflowSubscriberOutcome};
#[cfg(test)]
use review_resolution::{WORKFLOW_REVIEW_ACTIONS, WORKFLOW_REVIEW_VERSION};
use run_methodology::action_run_methodology;
use store_actions::parse_id_arg;
use store_actions::{action_apply, action_get, action_list, action_match, action_record_execution};

const DEFAULT_LIST_LIMIT: i64 = 50;
const MAX_LIST_LIMIT: i64 = 500;
const WORKFLOWS_DIR: &str = ".missiond/workflows";
const GENERATED_FLOWS_DIR: &str = ".missiond/generated/flows";
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
// helpers — shared
// ───────────────────────────────────────────────────────────────────────

// ───────────────────────────────────────────────────────────────────────
// tests
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
