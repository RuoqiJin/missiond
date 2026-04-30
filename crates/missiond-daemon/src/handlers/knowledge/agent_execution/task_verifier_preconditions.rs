use missiond_mcp::tools::{ToolError, ToolResult};

#[allow(dead_code)]
pub(super) struct VerifiedCompletionInputs<'a> {
    pub(super) task_contract_path: &'a str,
    pub(super) task_report_path: &'a str,
    pub(super) commit_hash: &'a str,
}

#[allow(dead_code)]
pub(super) fn require_verified_completion_inputs<'a>(
    enforce_scoped_commit: bool,
    task_contract_path: Option<&'a str>,
    task_report_path: Option<&'a str>,
    commit_hash: Option<&'a str>,
) -> Result<VerifiedCompletionInputs<'a>, ToolResult> {
    if !enforce_scoped_commit {
        return Err(ToolResult::structured_error(
            ToolError::new(
                "VERIFIED_REQUIRES_ENFORCEMENT",
                "verified=true requires enforce_scoped_commit=true so the underlying scope + contract gates also run",
            )
            .with_suggestion(
                "set enforce_scoped_commit=true alongside verified=true, or omit verified for legacy completions",
            ),
        ));
    }
    let task_contract_path = task_contract_path
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ToolResult::structured_error(
                ToolError::new(
                    "VERIFIED_REQUIRES_TASK_CONTRACT",
                    "verified=true requires a non-empty task_contract_path so the daemon-side cross-check can resolve the contract",
                )
                .with_suggestion(
                    "supply task_contract_path pointing at the task-contract v1 lisp file the dispatch brief used",
                ),
            )
        })?;
    let task_report_path = task_report_path
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ToolResult::structured_error(
                ToolError::new(
                    "VERIFIED_REQUIRES_TASK_REPORT",
                    "verified=true requires a non-empty task_report_path so the daemon can read the report-contract off disk",
                )
                .with_suggestion(
                    "supply task_report_path pointing at the report-contract v1 lisp file the writer produced",
                ),
            )
        })?;
    let commit_hash = commit_hash
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ToolResult::structured_error(
                ToolError::new(
                    "VERIFIED_REQUIRES_COMMIT_HASH",
                    "verified=true requires a non-empty commit_hash so the daemon can match it against the report's :commit_hash",
                )
                .with_suggestion(
                    "report the durable scoped commit hash, or omit verified for non-verified completions",
                ),
            )
        })?;

    Ok(VerifiedCompletionInputs {
        task_contract_path,
        task_report_path,
        commit_hash,
    })
}
