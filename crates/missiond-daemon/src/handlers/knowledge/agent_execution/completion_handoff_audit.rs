use serde_json::{json, Value};

use super::claim_lease::{scopes_overlap, ClaimRecord};
use super::completion_records::{
    parse_completions, FINDING_COMMIT_BLOCKED_NO_BLOCKER, FINDING_COMMIT_STATUS_NO_HASH,
    FINDING_SCOPED_COMMIT_VIOLATION,
};
use super::log_store::LogFile;

/// Run the scoped-commit handoff checks against every completion in the file.
/// Three failure modes from intent-memory.lisp :: scoped-commit-contract +
/// intent-flow.lisp :: F-scoped-commit-handoff :: failure-modes:
///
/// 1. `commit-status-without-hash` - `commit_status=committed` but no
///    `commit_hash`. The completion claims durability without the artifact.
/// 2. `commit-status-blocked-without-blocker` - `commit_status=blocked` but
///    no `commit_blocker`. The next agent has no recovery context.
/// 3. `scoped-commit-violation` - a `staged_files` entry escapes the union
///    of every claim scope on the file (active + released). We use the
///    union because a completion can post-date a release: the writer
///    legitimately stages files inside their just-released claim. Audit
///    only fails when no claim - past or present - covers a staged file.
///
/// All three are `error`-severity to match the existing audit invariants
/// (duplicate-id / claim-overlap), so the audit `ok=false` flips and
/// downstream consumers can gate on the same boolean.
pub(super) fn audit_scoped_commit_handoff(
    file: &LogFile,
    claims: &[ClaimRecord],
    findings: &mut Vec<Value>,
) {
    let completions = parse_completions(file);
    if completions.is_empty() {
        return;
    }
    // Collect every claim scope ever recorded - even released ones - so a
    // completion that stages files in a just-released claim is not flagged.
    // Empty scopes are skipped (legacy claims sometimes omit `:scope`).
    let claim_scopes: Vec<&str> = claims
        .iter()
        .map(|c| c.scope.as_str())
        .filter(|s| !s.is_empty())
        .collect();

    for c in &completions {
        if let Some(status_val) = c.commit_status.as_deref() {
            if status_val == "committed" && c.commit_hash.is_none() {
                findings.push(json!({
                    "severity": "error",
                    "kind": FINDING_COMMIT_STATUS_NO_HASH,
                    "completion_id": c.id,
                    "phase": c.phase,
                    "agent": c.agent,
                    "detail": "commit_status=committed but no commit_hash recorded - durability gap per scoped-commit-contract :inv-7",
                }));
            }
            if status_val == "blocked" && c.commit_blocker.is_none() {
                findings.push(json!({
                    "severity": "error",
                    "kind": FINDING_COMMIT_BLOCKED_NO_BLOCKER,
                    "completion_id": c.id,
                    "phase": c.phase,
                    "agent": c.agent,
                    "detail": "commit_status=blocked but no commit_blocker recorded - recovery-rule violation per scoped-commit-contract",
                }));
            }
        }
        if let Some(staged) = c.staged_files.as_ref() {
            if staged.is_empty() {
                continue;
            }
            if claim_scopes.is_empty() {
                // Files staged with no claim ever recorded: every entry is
                // a violation. Reuse the same finding kind so audit
                // consumers branch on `kind` rather than count claim
                // history.
                findings.push(json!({
                    "severity": "error",
                    "kind": FINDING_SCOPED_COMMIT_VIOLATION,
                    "completion_id": c.id,
                    "phase": c.phase,
                    "staged_files": staged,
                    "detail": "staged_files recorded but no claims exist on this companion log - scope-rule violation per scoped-commit-contract",
                }));
                continue;
            }
            // A file is in-scope when at least one claim's scope is a prefix
            // (or exact match). `scopes_overlap` already encodes the
            // bidirectional prefix relationship the contract uses for
            // claim conflict detection; we reuse it here so coordinator and
            // auditor agree on what "inside scope" means.
            let mut violators = Vec::new();
            for path in staged {
                let in_scope = claim_scopes.iter().any(|cs| scopes_overlap(cs, path));
                if !in_scope {
                    violators.push(path.clone());
                }
            }
            if !violators.is_empty() {
                findings.push(json!({
                    "severity": "error",
                    "kind": FINDING_SCOPED_COMMIT_VIOLATION,
                    "completion_id": c.id,
                    "phase": c.phase,
                    "agent": c.agent,
                    "staged_files": violators,
                    "claim_scopes": claim_scopes,
                    "detail": "staged_files include paths outside every recorded claim scope - scope-rule violation per scoped-commit-contract",
                }));
            }
        }
    }
}
