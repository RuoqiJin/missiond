use missiond_mcp::tools::{ToolError, ToolResult};
use std::path::Path;

use super::task_verifier_inputs::ReportSummary;

pub(super) fn verify_report_against_contract(
    report: &ReportSummary,
    report_resolved: &Path,
    contract_resolved: &Path,
    contract_id: &str,
    commit_hash: &str,
) -> std::result::Result<(), ToolResult> {
    require_report_schema(report, report_resolved)?;
    require_report_task_id(report, report_resolved, contract_resolved, contract_id)?;
    require_report_commit_hash(report, report_resolved, commit_hash)?;
    Ok(())
}

fn require_report_schema(
    report: &ReportSummary,
    report_resolved: &Path,
) -> std::result::Result<(), ToolResult> {
    match report.schema.as_deref() {
        Some("missiond.report-contract.v1") => Ok(()),
        Some(other) => Err(ToolResult::structured_error(ToolError::new(
            "TASK_REPORT_MALFORMED",
            format!(
                "task_report_path `{}` :schema must equal `missiond.report-contract.v1`, got `{}`",
                report_resolved.display(),
                other
            ),
        ))),
        None => Err(ToolResult::structured_error(ToolError::new(
            "TASK_REPORT_MALFORMED",
            format!(
                "task_report_path `{}` has no `:schema` field",
                report_resolved.display()
            ),
        ))),
    }
}

fn require_report_task_id(
    report: &ReportSummary,
    report_resolved: &Path,
    contract_resolved: &Path,
    contract_id: &str,
) -> std::result::Result<(), ToolResult> {
    match report.task_id.as_deref() {
        Some(id) if id == contract_id => Ok(()),
        Some(other) => Err(ToolResult::structured_error(
            ToolError::new(
                "TASK_REPORT_TASK_ID_MISMATCH",
                format!(
                    "task_report :task_id `{}` does not match task contract head id `{}` (contract `{}`, report `{}`)",
                    other,
                    contract_id,
                    contract_resolved.display(),
                    report_resolved.display(),
                ),
            )
            .with_suggestion(
                "regenerate the report against the matching contract, or fix the report :task_id field",
            ),
        )),
        None => Err(ToolResult::structured_error(ToolError::new(
            "TASK_REPORT_MALFORMED",
            format!(
                "task_report_path `{}` is missing required `:task_id` field",
                report_resolved.display()
            ),
        ))),
    }
}

fn require_report_commit_hash(
    report: &ReportSummary,
    report_resolved: &Path,
    commit_hash: &str,
) -> std::result::Result<(), ToolResult> {
    match report.commit_hash.as_deref() {
        Some(report_hash) => {
            let matches = report_hash == commit_hash
                || report_hash.starts_with(commit_hash)
                || commit_hash.starts_with(report_hash);
            if matches {
                Ok(())
            } else {
                Err(ToolResult::structured_error(
                    ToolError::new(
                        "TASK_REPORT_COMMIT_HASH_MISMATCH",
                        format!(
                            "task_report :commit_hash `{}` does not match completion commit_hash `{}` (report `{}`)",
                            report_hash,
                            commit_hash,
                            report_resolved.display(),
                        ),
                    )
                    .with_suggestion(
                        "regenerate the report against the durable commit, or correct the completion commit_hash",
                    ),
                ))
            }
        }
        None => Err(ToolResult::structured_error(ToolError::new(
            "TASK_REPORT_MALFORMED",
            format!(
                "task_report_path `{}` is missing required `:commit_hash` field",
                report_resolved.display()
            ),
        ))),
    }
}
