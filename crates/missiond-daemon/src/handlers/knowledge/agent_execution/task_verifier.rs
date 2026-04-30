use missiond_mcp::tools::{ToolError, ToolResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use super::task_verifier_inputs::{read_report_summary, read_task_contract_id};
use super::task_verifier_preconditions::require_verified_completion_inputs;
use super::task_verifier_report::verify_report_against_contract;

/// wave-21 / task 03 — verified-completion gate.
///
/// Runs only when `action_complete` saw `verified=true`. Enforces the
/// caller-asserted "task-run verifier passed end-to-end" claim with the
/// cross-checks the daemon can perform purely from local files:
///
///   1. Pre-conditions — `verified=true` is meaningless without
///      `enforce_scoped_commit=true`, a `task_contract_path`, a
///      `task_report_path`, and a `commit_hash`. Missing any of those
///      rejects with a structured `VERIFIED_REQUIRES_*` code BEFORE any
///      file mutation, mirroring the wave19-08 fail-fast posture.
///   2. Read-only file parses — load the report off disk (resolved
///      against the project root), confirm `:schema =
///      missiond.report-contract.v1`, confirm `:task_id` matches the
///      head id of the task contract, confirm the report's
///      `:commit_hash` matches the supplied `commit_hash`.
///
/// Daemon never spawns Node here — this is purely caller-supplied
/// metadata + read-only file inspection. The script-side
/// `scripts/verify-task-run.mjs` (wave21-02) is the authoritative
/// out-of-process verifier; this gate is the durable record that the
/// caller asserted it passed and that the assertion still survives a
/// daemon-side cross-check from the same files.
#[allow(clippy::too_many_arguments)]
pub(super) fn enforce_verified_completion(
    project_root: &Path,
    enforce_scoped_commit: bool,
    task_contract_path: Option<&str>,
    task_report_path: Option<&str>,
    commit_hash: Option<&str>,
) -> std::result::Result<Value, ToolResult> {
    let inputs = require_verified_completion_inputs(
        enforce_scoped_commit,
        task_contract_path,
        task_report_path,
        commit_hash,
    )?;
    let tcp = inputs.task_contract_path;
    let trp = inputs.task_report_path;
    let hash = inputs.commit_hash;

    // Resolve the report path (relative anchors at the project root,
    // absolute paths flow through verbatim — same semantics as the
    // wave19-08 contract gate).
    let report_raw = std::path::Path::new(trp);
    let report_resolved: PathBuf = if report_raw.is_absolute() {
        report_raw.to_path_buf()
    } else {
        project_root.join(report_raw)
    };
    let report_text = match std::fs::read_to_string(&report_resolved) {
        Ok(s) => s,
        Err(e) => {
            return Err(ToolResult::structured_error(
                ToolError::new(
                    "TASK_REPORT_REQUIRED",
                    format!(
                        "task_report_path `{}` is not readable: {}",
                        report_resolved.display(),
                        e
                    ),
                )
                .with_suggestion(
                    "ensure the path resolves under the project root and the writer wrote the report-contract v1 file",
                ),
            ));
        }
    };
    let report = match read_report_summary(&report_text) {
        Ok(r) => r,
        Err(e) => {
            return Err(ToolResult::structured_error(
                ToolError::new(
                    "TASK_REPORT_MALFORMED",
                    format!(
                        "task_report_path `{}` failed structural parse: {}",
                        report_resolved.display(),
                        e
                    ),
                )
                .with_suggestion(
                    "run `node scripts/check-task-report.mjs <path>` to see the exact schema error",
                ),
            ));
        }
    };

    // Load the contract to recover the head id for the cross-check.
    // Failures here re-use the wave19-08 error codes so callers see a
    // single vocabulary across the two gates.
    let contract_raw = std::path::Path::new(tcp);
    let contract_resolved: PathBuf = if contract_raw.is_absolute() {
        contract_raw.to_path_buf()
    } else {
        project_root.join(contract_raw)
    };
    let contract_text = match std::fs::read_to_string(&contract_resolved) {
        Ok(s) => s,
        Err(e) => {
            return Err(ToolResult::structured_error(ToolError::new(
                "TASK_CONTRACT_REQUIRED",
                format!(
                    "task_contract_path `{}` is not readable: {}",
                    contract_resolved.display(),
                    e
                ),
            )));
        }
    };
    let contract_id = read_task_contract_id(&contract_text).ok_or_else(|| {
        ToolResult::structured_error(ToolError::new(
            "TASK_CONTRACT_MALFORMED",
            format!(
                "task_contract_path `{}` is not a `(task <id> ...)` form",
                contract_resolved.display()
            ),
        ))
    })?;

    verify_report_against_contract(
        &report,
        &report_resolved,
        &contract_resolved,
        &contract_id,
        hash,
    )?;

    Ok(json!({
        "task_report_path": trp,
        "task_report_resolved_path": report_resolved.display().to_string(),
        "task_contract_path": tcp,
        "task_contract_resolved_path": contract_resolved.display().to_string(),
        "task_id": contract_id,
        "checked": [
            "preconditions_present",
            "task_report_loadable",
            "task_report_schema",
            "task_id_matches_contract",
            "commit_hash_matches_report",
        ],
    }))
}
