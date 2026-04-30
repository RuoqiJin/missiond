use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use super::claim_lease::{parse_claims, scopes_overlap_pure};
use super::log_store::LogFile;
use super::preflight_patterns::pattern_matches_path;
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

/// wave-20 / task 03 — pure projection of staged + changed files against a
/// task-contract v1's `:write-scope` and `:must-not-touch` patterns.
///
/// Shape (folded into the preflight response under `task_contract_scope`):
///   - `staged_out_of_scope`: staged paths that match no `:write-scope`
///      entry (and are not on `:must-not-touch`). Authoritative drift
///      signal; populates the new top-level `staged_out_of_scope` field.
///   - `staged_forbidden`: staged paths that match at least one
///      `:must-not-touch` pattern. Always considered out-of-scope.
///   - `unstaged_in_scope`: changed-but-not-staged paths that DO overlap
///      `:write-scope`. Surfaces "you edited it but forgot to stage it"
///      so the writer knows what to add.
///   - `next_step`: terse hint mirroring the wave16-06 enforcement
///      prose so a single screen tells the writer what to fix.
///   - `task_contract_status` is set by the caller (`loaded` / `missing` /
///      `malformed`) and merged on top of this projection.
///
/// Empty `write_scope` is treated as "contract declared no scope" — every
/// staged path then becomes out-of-scope, matching the verifier's
/// fail-fast posture (`scripts/verify-task-contract.mjs` rejects when
/// `:write-scope` is missing).
pub(super) fn build_contract_scope_summary(
    staged_files: &[String],
    changed_files: &[String],
    write_scope: &[String],
    must_not_touch: &[String],
) -> Value {
    let staged_forbidden: Vec<String> = staged_files
        .iter()
        .filter(|p| {
            must_not_touch
                .iter()
                .any(|pat| pattern_matches_path(p, pat))
        })
        .cloned()
        .collect();
    let staged_out_of_scope: Vec<String> = staged_files
        .iter()
        .filter(|p| !write_scope.iter().any(|pat| pattern_matches_path(p, pat)))
        .cloned()
        .collect();
    // `unstaged_in_scope` only counts paths that are changed but NOT
    // staged AND fall inside :write-scope. Lets the writer notice "edit
    // forgotten in `git add`" without flagging legitimate background
    // edits outside scope.
    let unstaged_in_scope: Vec<String> = changed_files
        .iter()
        .filter(|p| !staged_files.contains(p))
        .filter(|p| write_scope.iter().any(|pat| pattern_matches_path(p, pat)))
        .cloned()
        .collect();

    let next_step = if !staged_forbidden.is_empty() {
        format!(
            "unstage paths matching :must-not-touch before committing: {:?}",
            staged_forbidden
        )
    } else if !staged_out_of_scope.is_empty() {
        format!(
            "unstage paths outside :write-scope before committing: {:?}",
            staged_out_of_scope
        )
    } else if !unstaged_in_scope.is_empty() {
        format!(
            "stage the in-scope edits before committing: {:?}",
            unstaged_in_scope
        )
    } else if staged_files.is_empty() {
        "no staged files in scope yet — `git add` your write-scope edits".to_string()
    } else {
        "staged set respects :write-scope and :must-not-touch — proceed with scoped `git commit`"
            .to_string()
    };

    json!({
        "staged_out_of_scope": staged_out_of_scope,
        "staged_forbidden": staged_forbidden,
        "unstaged_in_scope": unstaged_in_scope,
        "write_scope": write_scope,
        "must_not_touch": must_not_touch,
        "next_step": next_step,
    })
}

/// wave-20 / task 03 — read-only contract loader for preflight. Resolves
/// relative paths against the project root, loads via the shared
/// workstation-dispatch projector, and returns the projection summary +
/// `task_contract_status` label. Failures map to `missing` (IO) /
/// `malformed` (parse) so preflight stays informational instead of
/// rejecting — the post-commit gate is the authoritative enforcement.
///
/// Returns `(status, optional_summary, optional_resolved_path,
/// optional_failure_message)`. Caller folds the tuple into the response.
pub(super) fn evaluate_task_contract_for_preflight(
    project_root: &Path,
    task_contract_path: &str,
    staged_files: &[String],
    changed_files: &[String],
) -> (&'static str, Option<Value>, Option<String>, Option<String>) {
    let raw = std::path::Path::new(task_contract_path);
    let resolved: PathBuf = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        project_root.join(raw)
    };
    let resolved_str = resolved.display().to_string();
    match super::super::workstation_dispatch::load_task_contract(&resolved) {
        Ok(contract) => {
            let summary = build_contract_scope_summary(
                staged_files,
                changed_files,
                &contract.write_scope,
                &contract.must_not_touch,
            );
            ("loaded", Some(summary), Some(resolved_str), None)
        }
        Err(err) => {
            use super::super::workstation_dispatch::TaskContractParseError as Tce;
            let (status, msg) = match &err {
                Tce::Io(detail) => (
                    "missing",
                    format!(
                        "task_contract_path `{}` is not readable: {}",
                        resolved.display(),
                        detail
                    ),
                ),
                _ => (
                    "malformed",
                    format!(
                        "task_contract_path `{}` failed schema parse: {}",
                        resolved.display(),
                        err.reason()
                    ),
                ),
            };
            (status, None, Some(resolved_str), Some(msg))
        }
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
