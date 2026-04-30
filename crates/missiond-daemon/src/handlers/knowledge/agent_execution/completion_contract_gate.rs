use missiond_mcp::tools::{ToolError, ToolResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use super::claim_lease::{parse_claims, scopes_overlap};
use super::log_store::LogFile;

/// wave-19 / task 08 - contract-level completion gate.
///
/// Runs only when `action_complete` saw both `enforce_scoped_commit=true`
/// AND a non-empty `task_contract_path`. We:
///
///   1. Resolve the path against the project root (relative paths anchor
///      on the registered project, never the daemon's CWD).
///   2. Read the file off disk (read-only) and parse it through the
///      shared `workstation_dispatch::parse_task_contract` projector so
///      the daemon and the workstation pillar agree on the schema.
///   3. Require a non-empty `commit_hash` - by contract a successful
///      task-contract completion must point at a durable scoped commit;
///      anything else means the verifier could not have run.
///   4. For every entry in the contract's `:write-scope`, assert it is
///      covered by either an active/released claim scope (re-using the
///      same `scopes_overlap` rule as `enforce_scoped_commit_completion`)
///      or by a path the caller staged (so a contract that names a brand
///      new file is not rejected before its first claim lands).
///
/// Returns `Ok(validation_summary)` on success; the summary is echoed
/// back on the response under `task_contract_validation` so callers can
/// confirm which rules ran. Failure modes:
///
///   - `TASK_CONTRACT_REQUIRED` - file missing / unreadable.
///   - `TASK_CONTRACT_MALFORMED` - lex / schema-mismatch / shape error.
///   - `COMMIT_HASH_REQUIRED_FOR_CONTRACT` - `commit_hash` was absent or
///     blank; the writer must report the durable scoped commit.
///   - `CLAIM_SCOPE_MISSING` - at least one `:write-scope` entry is not
///     covered by any active/released claim AND was not staged.
///
/// Daemon never runs git or any verifier here - the writer agent runs
/// `node scripts/verify-task-contract.mjs` out-of-process and reports the
/// outcome via `verifier_status`. This gate only checks the daemon-owned
/// state (claim scopes, on-disk contract file) versus the caller's
/// reported metadata.
pub(super) fn enforce_task_contract_completion(
    file: &LogFile,
    project_root: &Path,
    task_contract_path: &str,
    commit_hash: Option<&str>,
    staged_files: Option<&[String]>,
) -> std::result::Result<Value, ToolResult> {
    // Relative paths anchor on the project root; absolute paths flow through
    // verbatim so an out-of-tree contract is still loadable.
    let raw = std::path::Path::new(task_contract_path);
    let resolved: PathBuf = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        project_root.join(raw)
    };

    let contract = match super::super::workstation_dispatch::load_task_contract(&resolved) {
        Ok(c) => c,
        Err(e) => {
            use super::super::workstation_dispatch::TaskContractParseError as Tce;
            let (code, message) = match &e {
                Tce::Io(detail) => (
                    "TASK_CONTRACT_REQUIRED",
                    format!(
                        "task_contract_path `{}` is not readable: {}",
                        resolved.display(),
                        detail
                    ),
                ),
                _ => (
                    "TASK_CONTRACT_MALFORMED",
                    format!(
                        "task_contract_path `{}` failed schema parse: {}",
                        resolved.display(),
                        e.reason()
                    ),
                ),
            };
            return Err(ToolResult::structured_error(
                ToolError::new(code, message).with_suggestion(
                    "ensure the path resolves under the project root and the file is a valid `missiond.task-contract.v1` Lisp form",
                ),
            ));
        }
    };

    let commit_present = commit_hash.map(|s| !s.trim().is_empty()).unwrap_or(false);
    if !commit_present {
        return Err(ToolResult::structured_error(
            ToolError::new(
                "COMMIT_HASH_REQUIRED_FOR_CONTRACT",
                format!(
                    "enforce_scoped_commit=true with task_contract_path=`{}` requires a non-empty commit_hash",
                    task_contract_path
                ),
            )
            .with_suggestion(
                "report the scoped commit hash so the verifier can correlate the report-contract to the durable commit",
            ),
        ));
    }

    let claim_scopes: Vec<String> = parse_claims(file)
        .iter()
        .map(|c| c.scope.clone())
        .filter(|s| !s.is_empty())
        .collect();
    let staged: &[String] = staged_files.unwrap_or(&[]);

    let mut uncovered: Vec<String> = Vec::new();
    for ws in &contract.write_scope {
        if ws.is_empty() {
            continue;
        }
        let in_claim = claim_scopes.iter().any(|cs| scopes_overlap(cs, ws));
        let in_staged = staged.iter().any(|sp| scopes_overlap(sp, ws));
        if !in_claim && !in_staged {
            uncovered.push(ws.clone());
        }
    }
    if !uncovered.is_empty() {
        return Err(ToolResult::structured_error(
            ToolError::new(
                "CLAIM_SCOPE_MISSING",
                format!(
                    "task_contract_path `{}` :write-scope has {} entry/entries with no covering claim or staged file: uncovered={:?}, claim_scopes={:?}, staged_files={:?}",
                    task_contract_path,
                    uncovered.len(),
                    uncovered,
                    claim_scopes,
                    staged,
                ),
            )
            .with_suggestion(
                "open a claim covering each missing :write-scope entry, or stage the corresponding files before completing",
            ),
        ));
    }

    Ok(json!({
        "task_contract_path": task_contract_path,
        "resolved_path": resolved.display().to_string(),
        "schema": contract.schema,
        "checked": [
            "commit_hash_present",
            "task_contract_loadable",
            "write_scope_covered",
        ],
        "write_scope_entries": contract.write_scope.len(),
        "claim_scopes": claim_scopes,
        "staged_files_checked": staged.len(),
    }))
}
