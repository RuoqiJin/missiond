//! mission_execution — manager for the agent-execution-coordination protocol.
//!
//! Lisp authority:
//!   - intent-memory.lisp :: helper agent-execution-coordination v0.5.x (protocol)
//!   - intent-worker.lisp :: agent-execution-manager-interface (runtime mechanics)
//!   - intent-tools.lisp  :: future-surface mission_execution (MCP schema)
//!   - intent-flow.lisp   :: F-execution-log-governance (cross-pillar choreography)
//!
//! Companion logs live at `<project_root>/.missiond/v2/<execution_id>.lisp`.
//! This handler owns id-counters / claims-with-lease / deviations / decisions /
//! issues / completions / derived-indexes per the helper-recursive-contract.
//!
//! ExecutionEvent emission: each mutating action emits the matching variant
//! to the v2 event bus AFTER the durable companion log write succeeds. The
//! file remains the source of truth (per `planned-event-extensions ::
//! ExecutionEvent :: rationale`); the bus event is a non-authoritative live
//! projection for status dashboards and audit consumers. Publish failures
//! are logged but never abort the action — observability must never break
//! durable-write semantics.

use anyhow::Result;
#[cfg(test)]
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
#[cfg(test)]
use serde_json::json;
use serde_json::Value;
#[cfg(test)]
use std::path::{Path, PathBuf};

use crate::state::AppState;

mod claim_heartbeat;
mod claim_lease;
mod claim_records;
mod claim_release;
mod completion_audit;
mod completion_audit_findings;
mod completion_contract_gate;
mod completion_durability;
mod completion_entry;
mod completion_fields;
mod completion_gates;
mod completion_handoff_audit;
mod completion_id_audit;
mod completion_indexes;
mod completion_inputs;
mod completion_maintenance;
mod completion_records;
mod completion_repair;
mod completion_response;
mod completion_trace;
mod completion_verification;
mod lisp_syntax;
mod lisp_syntax_balance;
mod lisp_syntax_node;
mod log_counters;
mod log_decision;
mod log_deviation;
mod log_dispatch;
mod log_governance;
mod log_issue;
mod log_list;
mod log_mutation;
mod log_open;
mod log_paths;
mod log_status;
mod log_store;
mod log_surface;
mod log_template;
mod preflight;
mod preflight_contract;
mod preflight_contract_scope;
mod preflight_cwd;
mod preflight_patterns;
mod preflight_porcelain;
mod preflight_scope;
mod preflight_trace;
mod session_trace;
mod session_trace_event;
mod task_verifier;
mod task_verifier_auto;
mod task_verifier_auto_artifacts;
mod task_verifier_inputs;
mod task_verifier_preconditions;
mod task_verifier_report;

#[cfg(test)]
use self::claim_lease::parse_claims;
pub(super) use self::claim_lease::scopes_overlap_pure;
use self::claim_lease::{action_claim, action_heartbeat, action_release};
use self::completion_audit::action_complete;
#[cfg(test)]
use self::completion_durability::summarize_durability;
#[cfg(test)]
use self::completion_fields::{
    collect_string_list, normalize_commit_status, normalize_task_run_verifier_status,
    normalize_verifier_status, parse_string_list, render_string_list,
    FINDING_COMMIT_BLOCKED_NO_BLOCKER, FINDING_COMMIT_STATUS_NO_HASH,
    FINDING_SCOPED_COMMIT_VIOLATION, VALID_COMMIT_STATUSES, VALID_TASK_RUN_VERIFIER_STATUSES,
    VALID_VERIFIER_STATUSES,
};
#[cfg(test)]
use self::completion_gates::{
    audit_scoped_commit_handoff, enforce_scoped_commit_completion, enforce_task_contract_completion,
};
use self::completion_maintenance::{action_audit, action_repair};
#[cfg(test)]
use self::completion_records::{parse_completions, CompletionRecord};
#[cfg(test)]
use self::lisp_syntax as sexp;
#[cfg(test)]
use self::log_counters::{allocate_id, scan_max_id, Counter};
#[cfg(test)]
use self::log_dispatch::{
    build_opened_event, normalize_dispatch_strategy, read_dispatch_metadata_from_log, DispatchMeta,
    DEFAULT_DISPATCH_STRATEGY,
};
use self::log_governance::{action_decide, action_deviate, action_issue};
use self::log_status::action_status;
#[cfg(test)]
use self::log_store::{
    append_to_block, lisp_quote_string, now_iso, parse_kv_pairs, project_or_target_project,
    render_canonical_template, LogFile,
};
use self::log_surface::{action_list, action_open};
use self::preflight::action_preflight_commit;
#[cfg(test)]
use self::preflight_patterns::pattern_matches_path;
#[cfg(test)]
use self::preflight_porcelain::{parse_porcelain_status, PorcelainEntry};
#[cfg(test)]
use self::preflight_scope::{
    build_contract_scope_summary, build_preflight_summary, collect_all_claim_scopes,
    collect_specific_claim_scope, evaluate_task_contract_for_preflight,
};
#[cfg(test)]
use self::session_trace::{
    append_session_trace_event, is_valid_trace_id, render_trace_event, resolve_session_trace_path,
    resolve_trace_task_id, sanitize_trace_backend, scan_max_trace_seq, TraceEvent, TraceKind,
    TraceWarning,
};
#[cfg(test)]
use self::task_verifier::enforce_verified_completion;
#[cfg(test)]
use self::task_verifier_auto::auto_run_task_run_verifier;
#[cfg(test)]
use self::task_verifier_inputs::{
    read_report_summary, read_shared_memory_ledger, read_task_contract_id,
};

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a.to_string(),
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "mission_execution requires `action`",
                )
                .with_suggestion(
                    "actions: open|list|claim|heartbeat|release|deviate|decide|issue|complete|status|audit|repair",
                ),
            ))
        }
    };

    match action.as_str() {
        "open" => action_open(state, &args).await,
        "list" => action_list(state, &args).await,
        "claim" => action_claim(state, &args).await,
        "heartbeat" => action_heartbeat(state, &args).await,
        "release" => action_release(state, &args).await,
        "deviate" => action_deviate(state, &args).await,
        "decide" => action_decide(state, &args).await,
        "issue" => action_issue(state, &args).await,
        "complete" => action_complete(state, &args).await,
        "status" => action_status(state, &args).await,
        "audit" => action_audit(state, &args).await,
        "repair" => action_repair(state, &args).await,
        "preflight_commit" => action_preflight_commit(state, &args).await,
        other => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("unknown mission_execution action `{}`", other),
            )
            .with_suggestion(
                "valid: open|list|claim|heartbeat|release|deviate|decide|issue|complete|status|audit|repair|preflight_commit",
            ),
        )),
    }
}

// ───────────────────────────────────────────────────────────────────────
// tests — exercise the parser, ID allocation, and round-trip on a
// freshly-opened canonical file
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
