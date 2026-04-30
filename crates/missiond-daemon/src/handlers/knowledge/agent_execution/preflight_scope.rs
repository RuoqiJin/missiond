use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};

use super::claim_lease::{parse_claims, scopes_overlap_pure};
use super::log_store::LogFile;
#[cfg(test)]
pub(super) use super::preflight_contract_scope::build_contract_scope_summary;
pub(super) use super::preflight_contract_scope::evaluate_task_contract_for_preflight;
use super::preflight_porcelain::PorcelainEntry;

/// Collect every claim scope on the companion log, regardless of
/// status. Mirrors `enforce_scoped_commit_completion` — both
/// active and released claims count for scope-overlap purposes
/// because `F-scoped-commit-handoff :: s7` legitimately commits inside
/// a just-released claim window.
pub(super) fn collect_all_claim_scopes(file: &LogFile) -> Vec<String> {
    parse_claims(file)
        .iter()
        .map(|c| c.scope.clone())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Restrict to the scope of a specific claim id when caller supplies
/// `claim_id`. Returns `Err` with a structured `NOT_FOUND` ToolResult
/// when the claim id does not match any record so the writer learns
/// the typo before running git.
pub(super) fn collect_specific_claim_scope(
    file: &LogFile,
    claim_id: &str,
) -> std::result::Result<Vec<String>, ToolResult> {
    let claims = parse_claims(file);
    let hit = claims.iter().find(|c| c.id == claim_id);
    match hit {
        Some(c) if !c.scope.is_empty() => Ok(vec![c.scope.clone()]),
        Some(_) => Err(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!("claim {} has no scope set", claim_id),
            )
            .with_suggestion("rerun with claim_id omitted to use the union of all claim scopes"),
        )),
        None => Err(ToolResult::structured_error(
            ToolError::new(
                error_codes::NOT_FOUND,
                format!("claim_id `{}` not found on companion log", claim_id),
            )
            .with_suggestion("call action=status to list active claim ids"),
        )),
    }
}

/// Pure preflight comparison: given porcelain entries + claim scopes +
/// an optional `expected_files` hint from the dispatch brief, return
/// the structured projection the action surfaces back to the caller.
///
/// Output shape (also wired into the response JSON):
///   - `changed_files`: every porcelain entry whose worktree slot is
///      non-clean (includes untracked).
///   - `staged_files`: every porcelain entry whose index slot is
///      non-clean (excludes untracked).
///   - `out_of_scope_files`: subset of (changed ∪ staged) that does
///      NOT overlap any claim scope.
///   - `expected_missing`: paths in `expected_files` that are NOT in
///      the changed/staged set. Helps the writer notice when a file the
///      brief expected to touch was forgotten.
///   - `expected_unexpected`: paths changed/staged that are NOT in
///      `expected_files`. Surfaced only when `expected_files` is supplied
///      so the writer can audit drift from the plan node's `paths`
///      hint without us hard-failing on it.
///   - `ok`: true iff `out_of_scope_files` is empty.
///   - `next_step`: human-readable hint mirroring the wave16-06
///      enforcement messages so the writer can act without re-reading
///      the contract.
pub(super) fn build_preflight_summary(
    entries: &[PorcelainEntry],
    claim_scopes: &[String],
    expected_files: Option<&[String]>,
) -> Value {
    let changed_files: Vec<String> = entries
        .iter()
        .filter(|e| e.is_changed())
        .map(|e| e.path.clone())
        .collect();
    let staged_files: Vec<String> = entries
        .iter()
        .filter(|e| e.is_staged())
        .map(|e| e.path.clone())
        .collect();

    // Union of changed + staged for scope check, dedup-preserving order.
    let mut union: Vec<String> = Vec::with_capacity(changed_files.len() + staged_files.len());
    for p in changed_files.iter().chain(staged_files.iter()) {
        if !union.contains(p) {
            union.push(p.clone());
        }
    }

    let out_of_scope_files: Vec<String> = if claim_scopes.is_empty() {
        // No claim → every touched file is out-of-scope by definition;
        // the writer must claim before committing.
        union.clone()
    } else {
        union
            .iter()
            .filter(|path| !claim_scopes.iter().any(|cs| scopes_overlap_pure(cs, path)))
            .cloned()
            .collect()
    };

    let mut summary = json!({
        "ok": out_of_scope_files.is_empty(),
        "changed_files": changed_files,
        "staged_files": staged_files,
        "out_of_scope_files": out_of_scope_files,
        "claim_scopes": claim_scopes,
    });

    if let Some(expected) = expected_files {
        let expected_missing: Vec<String> = expected
            .iter()
            .filter(|p| !changed_files.contains(p) && !staged_files.contains(p))
            .cloned()
            .collect();
        let expected_unexpected: Vec<String> = changed_files
            .iter()
            .chain(staged_files.iter())
            .filter(|p| !expected.contains(p))
            .cloned()
            .collect();
        // Dedup expected_unexpected while preserving insertion order so
        // the response is deterministic across porcelain orderings.
        let mut seen_un: Vec<String> = Vec::new();
        for p in expected_unexpected {
            if !seen_un.contains(&p) {
                seen_un.push(p);
            }
        }
        summary["expected_files"] = json!(expected);
        summary["expected_missing"] = json!(expected_missing);
        summary["expected_unexpected"] = json!(seen_un);
    }

    let next_step = if !out_of_scope_files.is_empty() {
        if claim_scopes.is_empty() {
            "open a claim covering the touched paths via `mission_execution(action=claim, scope=…)` before staging anything".to_string()
        } else {
            format!(
                "narrow staged set to claim scope, or open a new claim covering: {:?}",
                out_of_scope_files
            )
        }
    } else if staged_files.is_empty() && changed_files.is_empty() {
        "worktree clean — nothing to commit".to_string()
    } else if staged_files.is_empty() {
        "stage the in-scope edits with `git add <paths>` then re-run preflight before committing"
            .to_string()
    } else {
        "in-scope changes detected — run scoped `git commit`, then call `action=complete` with `enforce_scoped_commit=true`".to_string()
    };
    summary["next_step"] = json!(next_step);

    summary
}
