//! mission_directive — manager surface for the directive table.
//!
//! Lisp authority:
//!   - intent-flow.lisp :: F-intent-alignment-plan-execution-loop ::
//!       s2 intent-alignment-authoring + s3 alignment-review-gate
//!   - intent-intent-layer.lisp :: section unified-entry-pipeline ::
//!       role alignment-author (mode-A direct-llm / mode-B resident slot)
//!   - intent-memory.lisp :: module directive-layer ::
//!       file-first-artifacts :: intent-alignment-artifact
//!   - intent-tools.lisp :: implemented-surface mission_directive
//!
//! Action coverage:
//!   compile        — directive-compiler actor v0:
//!                      compiler_mode="dry_run" (default) → no LLM, preview shape
//!                      compiler_mode="sonnet"            → SonnetGateway interactive call,
//!                                                          validates lisp shape, draft persist
//!                    persist=true writes DirectiveStatus::Draft (review remains human gate)
//!   list           — full (DirectiveLayerStore::directive_list_recent)
//!   get            — full (directive_get / version_chain head)
//!   approve        — full (directive_approve)
//!   archive        — full (directive_update_status → archived)
//!   version_chain  — full (directive_get_version_chain)

use anyhow::{anyhow, Result};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::str::FromStr;

use crate::context::v3_blueprint_runtime::RouterRuntimeConfig;
use crate::state::AppState;
use missiond_core::types::DirectiveStatus;

const DEFAULT_LIST_LIMIT: i64 = 50;
const MAX_LIST_LIMIT: i64 = 500;

pub(super) fn load_sonnet_compiler_model() -> Result<String> {
    RouterRuntimeConfig::load_for_current_dir()
        .map(|config| config.queued_sonnet_model)
        .map_err(|err| anyhow!("V3_BLUEPRINT_CONFIG_ERROR: {}", err))
}

mod compile_authoring;
use compile_authoring::action_compile;
mod approval_review;
#[cfg(test)]
use approval_review::DIRECTIVE_REVIEW_ACTIONS;
use approval_review::{action_approve, action_archive};
pub(crate) use approval_review::{handle_review_resolved_event, DirectiveSubscriberOutcome};
#[cfg(test)]
use compile_authoring::{
    build_compiler_system_prompt, build_references_json, collect_string_list,
    enrich_persisted_directive_sexp, extract_directive_file_args, parens_balanced,
    strip_fenced_code_block, top_level_head, validate_compiled_sexp, ALLOWED_SEXP_HEADS,
};

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a.to_string(),
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "mission_directive requires `action`",
                )
                .with_suggestion("actions: compile|list|get|approve|archive|version_chain"),
            ));
        }
    };

    match action.as_str() {
        "compile" => action_compile(state, &args).await,
        "list" => action_list(state, &args).await,
        "get" => action_get(state, &args).await,
        "approve" => action_approve(state, &args).await,
        "archive" => action_archive(state, &args).await,
        "version_chain" => action_version_chain(state, &args).await,
        other => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("unknown mission_directive action `{}`", other),
            )
            .with_suggestion("valid: compile|list|get|approve|archive|version_chain"),
        )),
    }
}

//   compiler_mode = "dry_run" (default): no LLM, returns preview shape
//   compiler_mode = "sonnet"           : SonnetGateway interactive call
// ───────────────────────────────────────────────────────────────────────

// ───────────────────────────────────────────────────────────────────────
// list / get / version_chain — store-backed reads
// ───────────────────────────────────────────────────────────────────────

async fn action_list(state: &AppState, args: &Value) -> Result<ToolResult> {
    let status = args
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| DirectiveStatus::from_str(s).map_err(|e| anyhow!(e)))
        .transpose()?;
    let limit = args
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT);

    let rows = state
        .store
        .directive_list_recent(status, limit)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    Ok(ToolResult::json_pretty(&json!({
        "directives": rows,
        "count": rows.len(),
        "filter": { "status": status.map(|s| s.as_str().to_string()) },
        "limit": limit,
    })))
}

async fn action_get(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "directive_id")?;
    let version = args
        .get("version")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);

    // No version → return the head (latest) of the chain.
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
    match resolved {
        Some(d) => Ok(ToolResult::json_pretty(&d)),
        None => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::NOT_FOUND,
                format!("directive `{}` not found", id),
            )
            .with_suggestion("use action=list to enumerate"),
        )),
    }
}

async fn action_version_chain(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "directive_id")?;
    let chain = state
        .store
        .directive_get_version_chain(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&json!({
        "directive_id": id,
        "chain": chain,
        "versions": chain.len(),
    })))
}

// ───────────────────────────────────────────────────────────────────────
// approve / archive — control actions
// ───────────────────────────────────────────────────────────────────────

// approve / archive review transitions live in directive/approval_review.rs.

// ───────────────────────────────────────────────────────────────────────
// arg helpers
// ───────────────────────────────────────────────────────────────────────

fn parse_id_arg(args: &Value, key: &str) -> Result<uuid::Uuid> {
    let raw = args
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("`{}` required", key))?;
    uuid::Uuid::parse_str(raw).map_err(|e| anyhow!("`{}` is not a UUID: {}", key, e))
}

fn require_i32(args: &Value, key: &str) -> Result<i32> {
    args.get(key)
        .and_then(|v| v.as_i64())
        .map(|v| v as i32)
        .ok_or_else(|| anyhow!("`{}` required (integer)", key))
}

// ───────────────────────────────────────────────────────────────────────
// tests — pure functions only (no LLM, no DB)
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
