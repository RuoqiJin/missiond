use missiond_mcp::tools::{ToolError, ToolResult};
use serde_json::{json, Value};

pub(super) use super::completion_contract_gate::enforce_task_contract_completion;
pub(super) use super::completion_handoff_audit::audit_scoped_commit_handoff;
pub(super) use super::completion_id_audit::check_id_monotonic;

use super::claim_lease::{parse_claims, scopes_overlap};
use super::log_store::LogFile;

/// Apply the wave16-06 fail-fast scoped-commit handoff checks against a
/// pending `action_complete` payload. Mirrors the audit-only failure
/// modes from `audit_scoped_commit_handoff` — same `scopes_overlap`
/// helper, same union of active+released claim scopes — but instead of
/// pushing audit findings the violations short-circuit completion with
/// a structured `ToolResult` error.
///
/// Returns `Ok(validation_summary)` when every gate passes; the summary
/// is echoed back on the response under `scoped_commit_validation` so
/// callers can confirm which rules ran.
///
/// Failure modes (all wired to the wave16-06 task contract):
/// 1. `COMMIT_HASH_REQUIRED` — `commit_status="committed"` without a
///    `commit_hash`. Mirrors the audit `commit-status-without-hash`
///    finding (intent-memory.lisp :: scoped-commit-contract :inv-7).
/// 2. `COMMIT_BLOCKER_REQUIRED` — `commit_status="blocked"` without a
///    `commit_blocker`. Mirrors `commit-status-blocked-without-blocker`.
/// 3. `CLAIM_SCOPE_REQUIRED` — caller reported `staged_files` but the
///    file has no claims at all. Distinct error code so callers can
///    tell "claim missing" from "scope drift" — both surface as
///    `scoped-commit-violation` in the audit-only path.
/// 4. `SCOPED_COMMIT_VIOLATION` — at least one staged path escapes the
///    union of every recorded claim scope. Direct parallel of the
///    audit `scoped-commit-violation` finding.
///
/// We deliberately do not run git inside the daemon. The caller is the
/// writer agent; the daemon validates the metadata it reports.
pub(super) fn enforce_scoped_commit_completion(
    file: &LogFile,
    staged_files: Option<&[String]>,
    commit_hash: Option<&str>,
    commit_status: Option<&str>,
    commit_blocker: Option<&str>,
) -> std::result::Result<Value, ToolResult> {
    if commit_status == Some("committed") && commit_hash.map(|s| s.is_empty()).unwrap_or(true) {
        return Err(ToolResult::structured_error(
            ToolError::new(
                "COMMIT_HASH_REQUIRED",
                "enforce_scoped_commit=true requires a non-empty commit_hash when commit_status=\"committed\"",
            )
            .with_suggestion(
                "report the scoped commit hash, or set commit_status to `blocked`/`pending`/`skipped`/`not-required`",
            ),
        ));
    }

    if commit_status == Some("blocked") && commit_blocker.map(|s| s.is_empty()).unwrap_or(true) {
        return Err(ToolResult::structured_error(
            ToolError::new(
                "COMMIT_BLOCKER_REQUIRED",
                "enforce_scoped_commit=true requires a non-empty commit_blocker when commit_status=\"blocked\"",
            )
            .with_suggestion(
                "describe why the scoped commit could not land so the next agent can resume per scoped-commit-contract :recovery-rule",
            ),
        ));
    }

    let staged_non_empty: &[String] = match staged_files {
        Some(list) if !list.is_empty() => list,
        // Empty / absent staged_files: nothing to validate against
        // claims — the completion may legitimately be read-only.
        _ => {
            return Ok(json!({
                "checked": ["commit_hash", "commit_blocker"],
                "staged_files_checked": 0,
                "claim_scopes": Vec::<String>::new(),
            }));
        }
    };

    let claims = parse_claims(file);
    let claim_scopes: Vec<String> = claims
        .iter()
        .map(|c| c.scope.clone())
        .filter(|s| !s.is_empty())
        .collect();

    if claim_scopes.is_empty() {
        return Err(ToolResult::structured_error(
            ToolError::new(
                "CLAIM_SCOPE_REQUIRED",
                format!(
                    "enforce_scoped_commit=true requires at least one claim scope on the companion log when staged_files is non-empty (got {} staged path(s))",
                    staged_non_empty.len()
                ),
            )
            .with_suggestion(
                "issue a `mission_execution(action=claim, scope=…)` covering the staged paths before completing, or stage no files",
            ),
        ));
    }

    // Reuse `scopes_overlap` so coordinator + auditor + enforcement all
    // agree on what "inside scope" means (same prefix-match rule).
    let mut violators: Vec<String> = Vec::new();
    for path in staged_non_empty {
        let in_scope = claim_scopes.iter().any(|cs| scopes_overlap(cs, path));
        if !in_scope {
            violators.push(path.clone());
        }
    }
    if !violators.is_empty() {
        // ToolError has no structured details slot today; bake the
        // offending paths + the claim scopes into the reason string so
        // the writer agent can correct without a second roundtrip.
        return Err(ToolResult::structured_error(
            ToolError::new(
                "SCOPED_COMMIT_VIOLATION",
                format!(
                    "enforce_scoped_commit=true rejected {} staged path(s) that escape every recorded claim scope: violators={:?}, claim_scopes={:?}",
                    violators.len(),
                    violators,
                    claim_scopes,
                ),
            )
            .with_suggestion(
                "narrow the staged set to the active claim scope, or open a new claim covering the escaped paths",
            ),
        ));
    }

    Ok(json!({
        "checked": ["commit_hash", "commit_blocker", "scoped_commit_violation"],
        "staged_files_checked": staged_non_empty.len(),
        "claim_scopes": claim_scopes,
    }))
}
