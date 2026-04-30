use missiond_mcp::tools::{ToolError, ToolResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use super::claim_lease::{parse_claims, scopes_overlap, ClaimRecord};
use super::completion_records::{
    parse_completions, FINDING_COMMIT_BLOCKED_NO_BLOCKER, FINDING_COMMIT_STATUS_NO_HASH,
    FINDING_SCOPED_COMMIT_VIOLATION,
};
use super::log_counters::Counter;
use super::log_store::{parse_kv_pairs, LogFile};

// ───────────────────────────────────────────────────────────────────────
// completion audit/enforcement gates
// ───────────────────────────────────────────────────────────────────────

pub(super) fn check_id_monotonic(file: &LogFile, counter: Counter, findings: &mut Vec<Value>) {
    let block = match file.find_block(counter.block_name()) {
        Some(b) => b,
        None => return,
    };
    let prefix = counter.prefix();
    let mut seen: Vec<u32> = Vec::new();
    let mut duplicates: Vec<String> = Vec::new();
    for child in block.children().iter().skip(1) {
        let head = child.head_atom().unwrap_or("");
        let id_str = if let Some(rest) = head.strip_prefix(prefix) {
            if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                Some(head.to_string())
            } else {
                None
            }
        } else {
            let kvs = parse_kv_pairs(&file.src, child.children());
            kvs.get("id")
                .map(|s| s.trim_matches('"').to_string())
                .filter(|s| s.starts_with(prefix))
        };
        if let Some(idtxt) = id_str {
            let num: u32 = idtxt.trim_start_matches(prefix).parse().unwrap_or(0);
            if seen.contains(&num) {
                duplicates.push(idtxt);
            } else {
                seen.push(num);
            }
        }
    }
    if !duplicates.is_empty() {
        findings.push(json!({
            "severity": "error",
            "kind": "duplicate-id",
            "block": counter.block_name(),
            "ids": duplicates,
        }));
    }
}

/// Run the scoped-commit handoff checks against every completion in the file.
/// Three failure modes from intent-memory.lisp :: scoped-commit-contract +
/// intent-flow.lisp :: F-scoped-commit-handoff :: failure-modes:
///
/// 1. `commit-status-without-hash` — `commit_status=committed` but no
///    `commit_hash`. The completion claims durability without the artifact.
/// 2. `commit-status-blocked-without-blocker` — `commit_status=blocked` but
///    no `commit_blocker`. The next agent has no recovery context.
/// 3. `scoped-commit-violation` — a `staged_files` entry escapes the union
///    of every claim scope on the file (active + released). We use the
///    union because a completion can post-date a release: the writer
///    legitimately stages files inside their just-released claim. Audit
///    only fails when no claim — past or present — covers a staged file.
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
    // Collect every claim scope ever recorded — even released ones — so a
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
                    "detail": "commit_status=committed but no commit_hash recorded — durability gap per scoped-commit-contract :inv-7",
                }));
            }
            if status_val == "blocked" && c.commit_blocker.is_none() {
                findings.push(json!({
                    "severity": "error",
                    "kind": FINDING_COMMIT_BLOCKED_NO_BLOCKER,
                    "completion_id": c.id,
                    "phase": c.phase,
                    "agent": c.agent,
                    "detail": "commit_status=blocked but no commit_blocker recorded — recovery-rule violation per scoped-commit-contract",
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
                    "detail": "staged_files recorded but no claims exist on this companion log — scope-rule violation per scoped-commit-contract",
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
                    "detail": "staged_files include paths outside every recorded claim scope — scope-rule violation per scoped-commit-contract",
                }));
            }
        }
    }
}

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

/// wave-19 / task 08 — contract-level completion gate.
///
/// Runs only when `action_complete` saw both `enforce_scoped_commit=true`
/// AND a non-empty `task_contract_path`. We:
///
///   1. Resolve the path against the project root (relative paths anchor
///      on the registered project, never the daemon's CWD).
///   2. Read the file off disk (read-only) and parse it through the
///      shared `workstation_dispatch::parse_task_contract` projector so
///      the daemon and the workstation pillar agree on the schema.
///   3. Require a non-empty `commit_hash` — by contract a successful
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
///   - `TASK_CONTRACT_REQUIRED` — file missing / unreadable.
///   - `TASK_CONTRACT_MALFORMED` — lex / schema-mismatch / shape error.
///   - `COMMIT_HASH_REQUIRED_FOR_CONTRACT` — `commit_hash` was absent or
///     blank; the writer must report the durable scoped commit.
///   - `CLAIM_SCOPE_MISSING` — at least one `:write-scope` entry is not
///     covered by any active/released claim AND was not staged.
///
/// Daemon never runs git or any verifier here — the writer agent runs
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
    // (1) Resolve. Relative paths anchor on the project root; absolute
    // paths flow through verbatim so an out-of-tree contract (rare) is
    // still loadable. We deliberately do NOT canonicalize here — the
    // caller's path string is echoed back into the validation summary
    // so dashboards correlate the response to the dispatch envelope.
    let raw = std::path::Path::new(task_contract_path);
    let resolved: PathBuf = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        project_root.join(raw)
    };

    // (2) Load + parse. Shared projector; daemon + workstation pillar
    // agree on schema. Errors map deterministically to the two
    // `TASK_CONTRACT_*` codes so callers can branch on file-vs-content.
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

    // (3) commit_hash gate. The contract pins a writer's durable
    // commit; a missing hash means we cannot tie the report back to a
    // git ref the verifier could have inspected.
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

    // (4) Claim-scope coverage. Every `:write-scope` entry must overlap
    // an active/released claim OR a staged_files path. We re-use the
    // same overlap rule as the audit + scoped-commit gates so the three
    // checkpoints stay semantically aligned.
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
