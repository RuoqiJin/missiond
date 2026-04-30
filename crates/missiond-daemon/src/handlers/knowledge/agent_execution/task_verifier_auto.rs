use missiond_mcp::tools::{ToolError, ToolResult};
use serde_json::{json, Value};
use std::path::Path;

use super::task_verifier_auto_artifacts::{
    read_report_artifact, read_shared_memory_artifact, read_task_contract_artifact,
    resolve_verifier_artifact_path,
};
use super::task_verifier_inputs::{
    read_report_summary, read_shared_memory_ledger, read_task_contract_id,
};

// ───────────────────────────────────────────────────────────────────────
// wave-22 / task 02 — auto task-run verifier (in-process, read-only)
// ───────────────────────────────────────────────────────────────────────
//
// Lifts the wave21-03 caller-supplied `verified=true` claim into a
// daemon-computed verdict. When `action_complete` sees all four of
// `task_contract_path`, `task_report_path`, `shared_memory_path`, and
// `commit_hash` the daemon runs the in-tree task-run verifier itself — no
// Node spawn, no shell, no mutating git, no process boundary at all.
#[allow(clippy::too_many_arguments)]
pub(super) fn auto_run_task_run_verifier(
    project_root: &Path,
    task_contract_path: &str,
    task_report_path: &str,
    shared_memory_path: &str,
    commit_hash: &str,
) -> std::result::Result<Value, ToolResult> {
    // (1) Resolve + load the task contract. Same path-resolution rule
    // as the wave19-08 / wave21-03 gates: relative anchors at the
    // project root, absolute flows verbatim. Reuses the workstation
    // pillar's projector so daemon + workstation share one schema.
    let contract_resolved = resolve_verifier_artifact_path(project_root, task_contract_path);
    // Recover the head id via the local mini-reader so we depend on the
    // same projector the wave21-03 gate uses (cross-check anchor).
    let contract_text = read_task_contract_artifact(&contract_resolved)?;
    let contract_id = read_task_contract_id(&contract_text).ok_or_else(|| {
        ToolResult::structured_error(ToolError::new(
            "TASK_CONTRACT_MALFORMED",
            format!(
                "task_contract_path `{}` is not a `(task <id> ...)` form",
                contract_resolved.display()
            ),
        ))
    })?;

    // (2) Resolve + load the report-contract. Mirrors the wave21-03
    // verified-gate's checks (schema, task_id, commit_hash) so the two
    // gates stay semantically aligned — only the trigger differs.
    let report_resolved = resolve_verifier_artifact_path(project_root, task_report_path);
    let report_text = read_report_artifact(&report_resolved)?;
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
    match report.schema.as_deref() {
        Some("missiond.report-contract.v1") => {}
        Some(other) => {
            return Err(ToolResult::structured_error(ToolError::new(
                "TASK_REPORT_MALFORMED",
                format!(
                    "task_report_path `{}` :schema must equal `missiond.report-contract.v1`, got `{}`",
                    report_resolved.display(),
                    other
                ),
            )));
        }
        None => {
            return Err(ToolResult::structured_error(ToolError::new(
                "TASK_REPORT_MALFORMED",
                format!(
                    "task_report_path `{}` has no `:schema` field",
                    report_resolved.display()
                ),
            )));
        }
    }
    match report.task_id.as_deref() {
        Some(id) if id == contract_id => {}
        Some(other) => {
            return Err(ToolResult::structured_error(
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
            ));
        }
        None => {
            return Err(ToolResult::structured_error(ToolError::new(
                "TASK_REPORT_MALFORMED",
                format!(
                    "task_report_path `{}` is missing required `:task_id` field",
                    report_resolved.display()
                ),
            )));
        }
    }
    // commit_hash overlap: full equality OR either side a prefix of the
    // other. Mirrors the wave21-03 short<->long sha tolerance so a
    // 7-char `git log %h` value still matches a 40-char `git rev-parse`.
    match report.commit_hash.as_deref() {
        Some(report_hash) => {
            let matches = report_hash == commit_hash
                || report_hash.starts_with(commit_hash)
                || commit_hash.starts_with(report_hash);
            if !matches {
                return Err(ToolResult::structured_error(
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
                ));
            }
        }
        None => {
            return Err(ToolResult::structured_error(ToolError::new(
                "TASK_REPORT_MALFORMED",
                format!(
                    "task_report_path `{}` is missing required `:commit_hash` field",
                    report_resolved.display()
                ),
            )));
        }
    }

    // (3) Resolve + load the shared-memory ledger. The script-side verifier
    // requires a `(completion :task <id> ...)` entry; the daemon mirrors that
    // rule using the in-tree sexp parser so the two produce identical verdicts.
    let memory_resolved = resolve_verifier_artifact_path(project_root, shared_memory_path);
    let memory_text = read_shared_memory_artifact(&memory_resolved)?;
    let ledger = match read_shared_memory_ledger(&memory_text) {
        Ok(l) => l,
        Err(e) => {
            return Err(ToolResult::structured_error(
                ToolError::new(
                    "SHARED_MEMORY_MALFORMED",
                    format!(
                        "shared_memory_path `{}` failed structural parse: {}",
                        memory_resolved.display(),
                        e
                    ),
                )
                .with_suggestion(
                    "run `node scripts/check-task-memory.mjs <path>` to see the exact schema error",
                ),
            ));
        }
    };
    if ledger.schema.as_deref() != Some("missiond.shared-memory.v1") {
        return Err(ToolResult::structured_error(ToolError::new(
            "SHARED_MEMORY_MALFORMED",
            format!(
                "shared_memory_path `{}` :schema must equal `missiond.shared-memory.v1`, got `{:?}`",
                memory_resolved.display(),
                ledger.schema,
            ),
        )));
    }
    let matched = ledger
        .completion_tasks
        .iter()
        .any(|task| task == &contract_id);
    if !matched {
        return Err(ToolResult::structured_error(
            ToolError::new(
                "SHARED_MEMORY_NO_COMPLETION_FOR_TASK",
                format!(
                    "shared_memory_path `{}` has no `(completion :task {} ...)` entry — the wave21-02 verifier requires the ledger to record the completion before the run can be ratified",
                    memory_resolved.display(),
                    contract_id
                ),
            )
            .with_suggestion(
                "append a `(completion :task ... :id ... :agent ... :seq ... :touched [...] :summary \"...\")` entry to the ledger before completing",
            ),
        ));
    }

    Ok(json!({
        "verifier_status": "passed",
        "task_id": contract_id,
        "task_contract_path": task_contract_path,
        "task_contract_resolved_path": contract_resolved.display().to_string(),
        "task_report_path": task_report_path,
        "task_report_resolved_path": report_resolved.display().to_string(),
        "shared_memory_path": shared_memory_path,
        "shared_memory_resolved_path": memory_resolved.display().to_string(),
        "commit_hash": commit_hash,
        "checks": [
            "task_contract_loadable",
            "task_report_loadable",
            "task_report_schema",
            "task_id_matches_contract",
            "commit_hash_matches_report",
            "shared_memory_loadable",
            "shared_memory_schema",
            "shared_memory_completion_for_task",
        ],
    }))
}
