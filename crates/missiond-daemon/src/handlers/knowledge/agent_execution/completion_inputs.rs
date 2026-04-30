use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::Value;

use super::completion_fields::{
    collect_string_list, normalize_commit_status, normalize_task_run_verifier_status,
    normalize_verifier_status, VALID_COMMIT_STATUSES, VALID_TASK_RUN_VERIFIER_STATUSES,
    VALID_VERIFIER_STATUSES,
};
use super::log_store::require_str;

pub(super) struct CompletionRequest<'a> {
    pub(super) execution_id: &'a str,
    pub(super) phase: &'a str,
    pub(super) agent: &'a str,
    pub(super) summary: &'a str,
    pub(super) deliverables: &'a str,
    pub(super) verification: &'a str,
    pub(super) changed_files: Option<Vec<String>>,
    pub(super) staged_files: Option<Vec<String>>,
    pub(super) commit_hash: Option<String>,
    pub(super) commit_status: Option<String>,
    pub(super) commit_blocker: Option<String>,
    pub(super) task_contract_path: Option<String>,
    pub(super) task_report_path: Option<String>,
    pub(super) verifier_status: Option<String>,
    pub(super) verifier_notes: Option<String>,
    pub(super) task_run_verifier_status: Option<String>,
    pub(super) shared_memory_path: Option<String>,
    pub(super) verifier_diagnostics: Option<String>,
    pub(super) verified_flag: Option<bool>,
    pub(super) enforce_scoped_commit: bool,
}

pub(super) fn parse_completion_request<'a>(
    args: &'a Value,
) -> Result<CompletionRequest<'a>, ToolResult> {
    let execution_id = require_str(args, "execution_id")?;
    let phase = require_str(args, "phase")?;
    let agent = require_str(args, "agent_name")?;
    let summary = require_str(args, "summary")?;
    let deliverables = args
        .get("deliverables")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let verification = args
        .get("verification")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let changed_files = collect_string_list(args, "changed_files");
    let staged_files = collect_string_list(args, "staged_files");
    let commit_hash = args
        .get("commit_hash")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let commit_status = parse_commit_status(args)?;
    let commit_blocker = args
        .get("commit_blocker")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let task_contract_path = args
        .get("task_contract_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let task_report_path = args
        .get("task_report_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let verifier_status = parse_verifier_status(args)?;
    let verifier_notes = args
        .get("verifier_notes")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let task_run_verifier_status = parse_task_run_verifier_status(args)?;
    let shared_memory_path = args
        .get("shared_memory_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let verifier_diagnostics = args
        .get("verifier_diagnostics")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let verified_flag = args.get("verified").and_then(|v| v.as_bool());
    let enforce_scoped_commit = args
        .get("enforce_scoped_commit")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok(CompletionRequest {
        execution_id,
        phase,
        agent,
        summary,
        deliverables,
        verification,
        changed_files,
        staged_files,
        commit_hash,
        commit_status,
        commit_blocker,
        task_contract_path,
        task_report_path,
        verifier_status,
        verifier_notes,
        task_run_verifier_status,
        shared_memory_path,
        verifier_diagnostics,
        verified_flag,
        enforce_scoped_commit,
    })
}

fn parse_commit_status(args: &Value) -> Result<Option<String>, ToolResult> {
    let Some(s) = trimmed_string_arg(args, "commit_status") else {
        return Ok(None);
    };
    match normalize_commit_status(s) {
        Some(canonical) => Ok(Some(canonical.to_string())),
        None => Err(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!(
                    "commit_status must be one of {:?}, got `{}`",
                    VALID_COMMIT_STATUSES, s
                ),
            )
            .with_suggestion("see intent-memory.lisp :: completions :commit-status-values"),
        )),
    }
}

fn parse_verifier_status(args: &Value) -> Result<Option<String>, ToolResult> {
    let Some(s) = trimmed_string_arg(args, "verifier_status") else {
        return Ok(None);
    };
    match normalize_verifier_status(s) {
        Some(canonical) => Ok(Some(canonical.to_string())),
        None => Err(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!(
                    "verifier_status must be one of {:?}, got `{}`",
                    VALID_VERIFIER_STATUSES, s
                ),
            )
            .with_suggestion(
                "see wave19-08 :: verifier-status enum (passed|failed|skipped|unknown)",
            ),
        )),
    }
}

fn parse_task_run_verifier_status(args: &Value) -> Result<Option<String>, ToolResult> {
    let Some(s) = trimmed_string_arg(args, "task_run_verifier_status") else {
        return Ok(None);
    };
    match normalize_task_run_verifier_status(s) {
        Some(canonical) => Ok(Some(canonical.to_string())),
        None => Err(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!(
                    "task_run_verifier_status must be one of {:?}, got `{}`",
                    VALID_TASK_RUN_VERIFIER_STATUSES, s
                ),
            )
            .with_suggestion(
                "see wave21-03 :: task-run-verifier-status enum (passed|failed|skipped|unknown)",
            ),
        )),
    }
}

fn trimmed_string_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
}
