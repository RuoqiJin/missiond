use anyhow::{anyhow, Result};
use chrono::Utc;
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::state::AppState;

use super::claim_lease::{find_claim_node, parse_claims, parse_iso, scopes_overlap, ClaimRecord};
use super::log_surface::{
    allocate_id, append_session_trace_event, append_to_block, companion_path, emit_execution_event,
    insert_id_counters_block, lisp_quote_string, list_block_summaries, now_iso, parse_kv_pairs,
    project_or_target_project, read_dispatch_metadata_from_log, read_log_file, require_str,
    resolve_project_root, resolve_session_trace_path, resolve_trace_task_id,
    sanitize_trace_backend, scan_max_id, sexp, touch_last_updated, update_kv_in_node,
    write_log_file, Counter, LogFile, TraceEvent, TraceKind,
};
use super::task_verifier::auto_run_task_run_verifier;

/// Canonical scoped-commit handoff statuses surfaced by intent-memory.lisp ::
/// helper agent-execution-coordination :: shared-memory-slots :: completions
/// :commit-status-values "[not-required pending committed blocked skipped]".
/// Used both to validate `mission_execution(action=complete, commit_status=...)`
/// arguments and to drive the audit checks for the durability plane.
pub(super) const VALID_COMMIT_STATUSES: &[&str] =
    &["not-required", "pending", "committed", "blocked", "skipped"];

/// Audit finding kinds emitted by the scoped-commit handoff checks. Kept as
/// static constants so test assertions can pin the exact wire form without
/// spelling them out repeatedly.
pub(super) const FINDING_COMMIT_STATUS_NO_HASH: &str = "commit-status-without-hash";
pub(super) const FINDING_COMMIT_BLOCKED_NO_BLOCKER: &str = "commit-status-blocked-without-blocker";
pub(super) const FINDING_SCOPED_COMMIT_VIOLATION: &str = "scoped-commit-violation";

/// Canonical verifier-status values surfaced by wave19-02 / wave19-08 ::
/// task-contract completion metadata. The writer agent runs the verifier
/// out-of-process and reports the outcome verbatim.
pub(super) const VALID_VERIFIER_STATUSES: &[&str] = &["passed", "failed", "skipped", "unknown"];

/// Canonical task-run verifier-status values surfaced by wave21-03 ::
/// task-run verification metadata.
pub(super) const VALID_TASK_RUN_VERIFIER_STATUSES: &[&str] =
    &["passed", "failed", "skipped", "unknown"];

/// Return the canonical form of a `commit_status` value if recognised.
/// Unknown values return `None` so the caller can hard-fail with a structured
/// INVALID_PARAM before any companion-log mutation.
pub(super) fn normalize_commit_status(raw: &str) -> Option<&'static str> {
    normalize_known(raw, VALID_COMMIT_STATUSES)
}

/// Canonicalize or reject the wave19-08 task-contract verifier-status enum.
pub(super) fn normalize_verifier_status(raw: &str) -> Option<&'static str> {
    normalize_known(raw, VALID_VERIFIER_STATUSES)
}

/// Canonicalize or reject the wave21-03 task-run verifier-status enum.
pub(super) fn normalize_task_run_verifier_status(raw: &str) -> Option<&'static str> {
    normalize_known(raw, VALID_TASK_RUN_VERIFIER_STATUSES)
}

fn normalize_known(raw: &str, known: &'static [&'static str]) -> Option<&'static str> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    known.iter().copied().find(|candidate| *candidate == value)
}

/// Pull a `[string]` argument off `args[key]` and return it as a `Vec<String>`.
/// Returns `None` if the key is absent so callers can distinguish "field was
/// not supplied" from "field was supplied as empty list" — both shapes are
/// legal: a writer that ran no commit may legitimately report
/// `staged_files=[]` to record "nothing staged".
pub(super) fn collect_string_list(args: &Value, key: &str) -> Option<Vec<String>> {
    let arr = args.get(key)?.as_array()?;
    let out: Vec<String> = arr
        .iter()
        .filter_map(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Some(out)
}

/// Render a string list as a Lisp expression `("a" "b" ...)`, or `()` when
/// empty. Empty lists still emit the empty-list literal so audit can tell the
/// caller deliberately recorded "no files" — distinct from the field being
/// absent altogether.
pub(super) fn render_string_list(items: &[String]) -> String {
    if items.is_empty() {
        return "()".to_string();
    }
    let parts: Vec<String> = items.iter().map(|s| lisp_quote_string(s)).collect();
    format!("({})", parts.join(" "))
}

/// Parse a Lisp list literal `("a" "b" ...)` slice back into `Vec<String>`.
/// Tolerates whitespace/newlines and unquoted atoms (legacy hand-edited
/// files); caller passes the raw source slice covering the value.
/// Returns `None` if the slice does not parse as a list — caller decides
/// whether to treat that as audit-worthy or as a no-op.
pub(super) fn parse_string_list(slice: &str) -> Option<Vec<String>> {
    let trimmed = slice.trim();
    if !trimmed.starts_with('(') {
        return None;
    }
    let nodes = sexp::parse(trimmed).ok()?;
    let outer = nodes.first()?;
    let mut out = Vec::new();
    for child in outer.children() {
        match &child.kind {
            sexp::NodeKind::Str(s) => out.push(s.clone()),
            sexp::NodeKind::Atom(a) => out.push(a.clone()),
            _ => {}
        }
    }
    Some(out)
}

// ───────────────────────────────────────────────────────────────────────
// completion record + durability projection
// ───────────────────────────────────────────────────────────────────────

/// View of a single `(COMPxxx ...)` entry inside the `completions` block,
/// including the optional scoped-commit handoff fields per intent-memory.lisp
/// :: helper agent-execution-coordination :: shared-memory-slots ::
/// completions. All durability fields are `Option`/`Option<Vec<_>>` so legacy
/// completions (no scoped-commit metadata) round-trip cleanly: missing keys
/// stay `None` and consumers — status, list, audit — make the same backward
/// compatibility decisions in one place.
#[derive(Debug, Clone)]
pub(super) struct CompletionRecord {
    pub(super) id: String,
    pub(super) phase: String,
    pub(super) agent: String,
    pub(super) at: String,
    pub(super) changed_files: Option<Vec<String>>,
    pub(super) staged_files: Option<Vec<String>>,
    pub(super) commit_hash: Option<String>,
    pub(super) commit_status: Option<String>,
    pub(super) commit_blocker: Option<String>,
    // ── wave-19 / task 08 — task-contract completion metadata ──
    // Recorded verbatim from the matching `mission_execution(action=complete)`
    // invocation when the dispatch flowed through a task-contract v1 +
    // report-contract v1 pair (wave19-02 / wave19-03). Daemon never
    // parses `task_report_path` itself; `verifier_status` is the
    // authoritative outcome signal and is reported by the writer.
    pub(super) task_contract_path: Option<String>,
    pub(super) task_report_path: Option<String>,
    pub(super) verifier_status: Option<String>,
    pub(super) verifier_notes: Option<String>,
    // ── wave-21 / task 03 — task-run verifier completion metadata ──
    // Records the outcome of an end-to-end task-run verifier (e.g.
    // `node scripts/verify-task-run.mjs` from wave21-02) which folds
    // the contract, report, shared-memory completion, and commit scope
    // into one proof. `verified=true` lights up the daemon-side
    // read-only re-check that loads the report file off disk and
    // cross-validates `:task_id` + `:commit_hash` against the supplied
    // metadata. `task_run_verifier_status` lives next to the wave19-08
    // `verifier_status` slot — the two enums share a vocabulary but
    // describe orthogonal verifiers, so we persist them independently.
    pub(super) task_run_verifier_status: Option<String>,
    pub(super) shared_memory_path: Option<String>,
    pub(super) verifier_diagnostics: Option<String>,
    pub(super) verified: Option<bool>,
}

pub(super) fn parse_completions(file: &LogFile) -> Vec<CompletionRecord> {
    let block = match file.find_block("completions") {
        Some(b) => b,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for child in block.children().iter().skip(1) {
        let head = child.head_atom().unwrap_or("").to_string();
        let kvs = parse_kv_pairs(&file.src, child.children());
        // `parse_kv_pairs` returns the value's verbatim source slice (the
        // outer quotes survive for strings, parentheses survive for lists).
        // We trim the wrapping quote characters here so per-field consumers
        // can compare canonical content directly.
        let unwrap_str = |raw: &str| raw.trim().trim_matches('"').to_string();
        // For `:changed-files (...)` and `:staged-files (...)` the slice is a
        // Lisp list literal; reuse the sexp parser to recover the entries.
        let unwrap_list = |raw: &str| -> Option<Vec<String>> {
            let trimmed = raw.trim();
            if !trimmed.starts_with('(') {
                return None;
            }
            parse_string_list(trimmed)
        };

        let id = if head.starts_with("COMP")
            && head.len() > 4
            && head[4..].chars().all(|c| c.is_ascii_digit())
        {
            head.clone()
        } else if let Some(v) = kvs.get("id").or_else(|| kvs.get("completion-id")) {
            unwrap_str(v)
        } else {
            format!("completion@{}", child.start)
        };

        let changed_files = kvs
            .get("changed-files")
            .or_else(|| kvs.get("changed_files"))
            .and_then(|raw| unwrap_list(raw));
        let staged_files = kvs
            .get("staged-files")
            .or_else(|| kvs.get("staged_files"))
            .and_then(|raw| unwrap_list(raw));
        let commit_hash = kvs
            .get("commit-hash")
            .or_else(|| kvs.get("commit_hash"))
            .map(|raw| unwrap_str(raw))
            .filter(|s| !s.is_empty());
        let commit_status = kvs
            .get("commit-status")
            .or_else(|| kvs.get("commit_status"))
            .map(|raw| unwrap_str(raw))
            .filter(|s| !s.is_empty());
        let commit_blocker = kvs
            .get("commit-blocker")
            .or_else(|| kvs.get("commit_blocker"))
            .map(|raw| unwrap_str(raw))
            .filter(|s| !s.is_empty());

        // wave-19 / task 08 — task-contract completion metadata. Empty
        // strings collapse to `None` so audit / status do not surface
        // whitespace as a meaningful value (mirrors `commit_hash` /
        // `commit_blocker`). `verifier-status` is normalised against the
        // canonical enum at write-time, but we still tolerate legacy /
        // hand-edited values here so a malformed file remains parseable.
        let task_contract_path = kvs
            .get("task-contract-path")
            .or_else(|| kvs.get("task_contract_path"))
            .map(|raw| unwrap_str(raw))
            .filter(|s| !s.is_empty());
        let task_report_path = kvs
            .get("task-report-path")
            .or_else(|| kvs.get("task_report_path"))
            .map(|raw| unwrap_str(raw))
            .filter(|s| !s.is_empty());
        let verifier_status = kvs
            .get("verifier-status")
            .or_else(|| kvs.get("verifier_status"))
            .map(|raw| unwrap_str(raw))
            .filter(|s| !s.is_empty());
        let verifier_notes = kvs
            .get("verifier-notes")
            .or_else(|| kvs.get("verifier_notes"))
            .map(|raw| unwrap_str(raw))
            .filter(|s| !s.is_empty());

        // wave-21 / task 03 — task-run verifier metadata. Same
        // empty-collapse + legacy-tolerant rules as the wave19-08
        // fields above. `verified` parses both `true`/`false` atoms
        // (the canonical write form) so a round-trip through this
        // reader recovers the boolean without quoted-string handling.
        let task_run_verifier_status = kvs
            .get("task-run-verifier-status")
            .or_else(|| kvs.get("task_run_verifier_status"))
            .map(|raw| unwrap_str(raw))
            .filter(|s| !s.is_empty());
        let shared_memory_path = kvs
            .get("shared-memory-path")
            .or_else(|| kvs.get("shared_memory_path"))
            .map(|raw| unwrap_str(raw))
            .filter(|s| !s.is_empty());
        let verifier_diagnostics = kvs
            .get("verifier-diagnostics")
            .or_else(|| kvs.get("verifier_diagnostics"))
            .map(|raw| unwrap_str(raw))
            .filter(|s| !s.is_empty());
        let verified = kvs
            .get("verified")
            .map(|raw| unwrap_str(raw))
            .filter(|s| !s.is_empty())
            .and_then(|s| match s.as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            });

        out.push(CompletionRecord {
            id,
            phase: kvs.get("phase").map(|s| unwrap_str(s)).unwrap_or_default(),
            agent: kvs.get("agent").map(|s| unwrap_str(s)).unwrap_or_default(),
            at: kvs.get("at").map(|s| unwrap_str(s)).unwrap_or_default(),
            changed_files,
            staged_files,
            commit_hash,
            commit_status,
            commit_blocker,
            task_contract_path,
            task_report_path,
            verifier_status,
            verifier_notes,
            task_run_verifier_status,
            shared_memory_path,
            verifier_diagnostics,
            verified,
        });
    }
    out
}

/// Build the dashboard-friendly `durability` projection over a slice of
/// `CompletionRecord`s. The shape stays stable across legacy + new
/// companion logs: when no completion carries scoped-commit metadata the
/// summary still surfaces zero counts plus `latest_commit_status: null`
/// so consumers do not need to special-case "old log".
pub(super) fn summarize_durability(records: &[CompletionRecord]) -> Value {
    let total = records.len();
    let mut by_status: HashMap<&str, u32> = HashMap::new();
    let mut without_status = 0u32;
    let mut with_hash = 0u32;
    let mut blocked_with_blocker = 0u32;
    let mut blocked_without_blocker = 0u32;
    for r in records {
        match r.commit_status.as_deref() {
            Some(s) => {
                *by_status.entry(canonical_status_str(s)).or_insert(0) += 1;
                if s == "blocked" {
                    if r.commit_blocker.is_some() {
                        blocked_with_blocker += 1;
                    } else {
                        blocked_without_blocker += 1;
                    }
                }
            }
            None => without_status += 1,
        }
        if r.commit_hash.is_some() {
            with_hash += 1;
        }
    }
    let mut by_status_json = serde_json::Map::new();
    for &status in VALID_COMMIT_STATUSES {
        by_status_json.insert(
            status.to_string(),
            json!(*by_status.get(status).unwrap_or(&0)),
        );
    }
    let unknown_count = *by_status.get("unknown").unwrap_or(&0);
    if unknown_count > 0 {
        by_status_json.insert("unknown".to_string(), json!(unknown_count));
    }
    let latest_status = records.iter().rev().find_map(|r| r.commit_status.clone());
    let latest_hash = records.iter().rev().find_map(|r| r.commit_hash.clone());
    json!({
        "completion_count": total,
        "without_commit_status": without_status,
        "with_commit_hash": with_hash,
        "blocked_with_blocker": blocked_with_blocker,
        "blocked_without_blocker": blocked_without_blocker,
        "by_commit_status": Value::Object(by_status_json),
        "latest_commit_status": latest_status,
        "latest_commit_hash": latest_hash,
    })
}

/// Map a raw status string back to one of `VALID_COMMIT_STATUSES`. Returns
/// `"unknown"` for anything else so we never silently drop weird tokens out
/// of the rollup. Audit still emits a finding via the strict normalize path
/// at write-time, but the dashboard shape stays predictable.
fn canonical_status_str(raw: &str) -> &'static str {
    match raw.trim() {
        "not-required" => "not-required",
        "pending" => "pending",
        "committed" => "committed",
        "blocked" => "blocked",
        "skipped" => "skipped",
        _ => "unknown",
    }
}

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

pub(super) async fn action_complete(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let phase = match require_str(args, "phase") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let agent = match require_str(args, "agent_name") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let summary = match require_str(args, "summary") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let deliverables = args
        .get("deliverables")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let verification = args
        .get("verification")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // ── scoped-commit handoff fields (intent-memory.lisp :: helper
    // agent-execution-coordination :: shared-memory-slots :: completions —
    // :fields "... changed_files / staged_files / commit_hash / commit_status").
    // All five are optional so legacy callers that omit them still write a
    // backward-compatible completion entry; only the keys actually supplied
    // are emitted into the Lisp slot. `commit_status` is normalized against
    // the canonical enum from the protocol's :commit-status-values.
    let changed_files = collect_string_list(args, "changed_files");
    let staged_files = collect_string_list(args, "staged_files");
    let commit_hash = args
        .get("commit_hash")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let commit_status_raw = args
        .get("commit_status")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let commit_status = match commit_status_raw {
        Some(s) => match normalize_commit_status(s) {
            Some(canonical) => Some(canonical.to_string()),
            None => {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "commit_status must be one of {:?}, got `{}`",
                            VALID_COMMIT_STATUSES, s
                        ),
                    )
                    .with_suggestion("see intent-memory.lisp :: completions :commit-status-values"),
                ));
            }
        },
        None => None,
    };
    let commit_blocker = args
        .get("commit_blocker")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // ── wave-19 / task 08 — task-contract completion metadata.
    //
    // All four fields are optional and recorded verbatim into the
    // companion log when supplied. `verifier_status` is normalized
    // against the canonical enum so audit / dashboard consumers can key
    // off the exact string; unknown labels reject with `INVALID_PARAM`
    // BEFORE any file mutation. `task_contract_path` doubles as the
    // trigger for the contract-level enforcement gate further below
    // when paired with `enforce_scoped_commit=true`.
    let task_contract_path = args
        .get("task_contract_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let task_report_path = args
        .get("task_report_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let verifier_status_raw = args
        .get("verifier_status")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let verifier_status = match verifier_status_raw {
        Some(s) => match normalize_verifier_status(s) {
            Some(canonical) => Some(canonical.to_string()),
            None => {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "verifier_status must be one of {:?}, got `{}`",
                            VALID_VERIFIER_STATUSES, s
                        ),
                    )
                    .with_suggestion(
                        "see wave19-08 :: verifier-status enum (passed|failed|skipped|unknown)",
                    ),
                ));
            }
        },
        None => None,
    };
    let verifier_notes = args
        .get("verifier_notes")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // ── wave-21 / task 03 — task-run verifier completion metadata.
    //
    // `task_run_verifier_status` / `shared_memory_path` /
    // `verifier_diagnostics` / `verified` mirror the wave19-08 fields
    // but capture the END-TO-END verifier outcome (task contract +
    // report + shared-memory completion + commit scope all proven in
    // one pass — see wave21-02 :: scripts/verify-task-run.mjs). All
    // four are optional and recorded verbatim into the companion log;
    // `task_run_verifier_status` rejects unknown labels at parse time
    // so audit / dashboard consumers can key off the canonical enum.
    let task_run_verifier_status_raw = args
        .get("task_run_verifier_status")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let task_run_verifier_status = match task_run_verifier_status_raw {
        Some(s) => match normalize_task_run_verifier_status(s) {
            Some(canonical) => Some(canonical.to_string()),
            None => {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "task_run_verifier_status must be one of {:?}, got `{}`",
                            VALID_TASK_RUN_VERIFIER_STATUSES, s
                        ),
                    )
                    .with_suggestion(
                        "see wave21-03 :: task-run-verifier-status enum (passed|failed|skipped|unknown)",
                    ),
                ));
            }
        },
        None => None,
    };
    let shared_memory_path = args
        .get("shared_memory_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let verifier_diagnostics = args
        .get("verifier_diagnostics")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    // `verified` is a tri-state at parse time: absent → None (legacy
    // shape, no extra gate), false → Some(false) (caller explicitly
    // recorded a non-verified completion), true → Some(true) (gate
    // runs). We persist the explicit `false` so audit can tell "writer
    // intentionally skipped verification" from "writer omitted the
    // field because they're a legacy caller".
    let verified_flag = args.get("verified").and_then(|v| v.as_bool());

    // ── Optional fail-fast enforcement (wave16-06).
    //
    // `enforce_scoped_commit=true` flips the existing audit-only handoff
    // checks into hard rejects at completion-time. Default `false` keeps
    // legacy callers byte-identical: they still get the audit-only path
    // wired through `mission_execution(action=audit)` later. We resolve
    // the flag here so the validation step (run BEFORE id allocation)
    // sees the caller's intent without paying the read cost twice.
    let enforce_scoped_commit = args
        .get("enforce_scoped_commit")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;

    // Run the enforcement gate BEFORE `allocate_id` mutates the
    // id-counters block — a rejected completion must not bump the
    // counter or otherwise change the durable file.
    let scoped_commit_validation = if enforce_scoped_commit {
        match enforce_scoped_commit_completion(
            &file,
            staged_files.as_deref(),
            commit_hash.as_deref(),
            commit_status.as_deref(),
            commit_blocker.as_deref(),
        ) {
            Ok(v) => Some(v),
            Err(err) => return Ok(err),
        }
    } else {
        None
    };

    // wave-19 / task 08 — contract-level enforcement gate. Runs only
    // when the caller paired `enforce_scoped_commit=true` with a
    // `task_contract_path`; otherwise the contract metadata is recorded
    // verbatim with no additional checks (legacy / opt-out behaviour).
    // Daemon never shells out — we read the file off disk and use the
    // workstation_dispatch parser to project the narrow view we need.
    let task_contract_validation = if enforce_scoped_commit && task_contract_path.is_some() {
        let path_arg = task_contract_path.as_deref().unwrap();
        match enforce_task_contract_completion(
            &file,
            &root,
            path_arg,
            commit_hash.as_deref(),
            staged_files.as_deref(),
        ) {
            Ok(v) => Some(v),
            Err(err) => return Ok(err),
        }
    } else {
        None
    };

    // wave-22 / task 02 — auto task-run verifier dispatch.
    //
    // The wave21-03 caller-supplied `verified=true` escape hatch is now
    // a legacy-compat fallback. The new contract: when the writer hands
    // every path the daemon needs for an end-to-end proof
    // (`task_contract_path`, `task_report_path`, `shared_memory_path`,
    // `commit_hash`) the daemon runs the in-tree task-run verifier
    // ITSELF and computes the verified status from the on-disk inputs
    // — no Node spawn, no shell, no mutating git, no caller assertion
    // accepted at face value. The wave21-02 script-side verifier
    // remains the out-of-process truth; this in-process projection just
    // closes the action-complete window so dashboards stop relying on
    // a writer-asserted boolean.
    //
    // Three-state `verification_source` summarises what happened:
    //   * `daemon-auto-verifier` — all four paths present, daemon ran
    //     the in-tree verifier and produced the verdict in
    //     `verifier_status` / `verified_scope_summary`.
    //   * `legacy-caller-claim` — caller passed `verified=true` but at
    //     least one of the four paths is absent. We honour the legacy
    //     posture (no hard reject), record the claim into the companion
    //     log verbatim, and surface `verifier_status="unknown"` plus a
    //     diagnostic explaining which path was missing so reviewers can
    //     migrate the caller off the escape hatch.
    //   * `none` — no auto-verifier run AND no legacy claim; absent in
    //     the response so legacy completions stay byte-identical.
    //
    // Backward compat: the wave21-03 helper `enforce_verified_completion`
    // is preserved verbatim and still callable from tests, but
    // `action_complete` no longer routes through it — the v2 dispatch
    // either runs the auto-verifier or downgrades the legacy claim.
    let auto_verifier_inputs_present = task_contract_path.is_some()
        && task_report_path.is_some()
        && shared_memory_path.is_some()
        && commit_hash.is_some();

    let mut verification_source: Option<&'static str> = None;
    let mut auto_verifier_summary: Option<Value> = None;
    let mut auto_verifier_status: Option<&'static str> = None;
    let mut auto_verifier_diagnostics: Option<String> = None;

    if auto_verifier_inputs_present {
        // unwraps are safe — we just checked all four are Some.
        let tcp = task_contract_path.as_deref().unwrap();
        let trp = task_report_path.as_deref().unwrap();
        let smp = shared_memory_path.as_deref().unwrap();
        let hash = commit_hash.as_deref().unwrap();
        match auto_run_task_run_verifier(&root, tcp, trp, smp, hash) {
            Ok(summary) => {
                auto_verifier_status = Some("passed");
                auto_verifier_summary = Some(summary);
                verification_source = Some("daemon-auto-verifier");
            }
            Err(err) => return Ok(err),
        }
    } else if verified_flag == Some(true) {
        // Legacy caller-supplied claim. Record it but flag in the
        // diagnostic which path was missing so the writer agent can
        // upgrade the next dispatch.
        let mut missing: Vec<&'static str> = Vec::new();
        if task_contract_path.is_none() {
            missing.push("task_contract_path");
        }
        if task_report_path.is_none() {
            missing.push("task_report_path");
        }
        if shared_memory_path.is_none() {
            missing.push("shared_memory_path");
        }
        if commit_hash.is_none() {
            missing.push("commit_hash");
        }
        verification_source = Some("legacy-caller-claim");
        auto_verifier_status = Some("unknown");
        auto_verifier_diagnostics = Some(format!(
            "verified=true accepted as legacy_verified_claim because the daemon-side auto-verifier requires all four of [task_contract_path, task_report_path, shared_memory_path, commit_hash]; missing: {:?}. Migrate the dispatch envelope to supply every path so the daemon can compute the verdict itself (wave22-02).",
            missing,
        ));
    }
    // Tri-state placeholder kept in sync with the wave21-03 response
    // shape: when the auto-verifier ran the response surfaces the
    // structured summary; when only the legacy claim was made it stays
    // None and the diagnostic prose above carries the explanation.
    let verified_validation: Option<Value> = auto_verifier_summary.clone();

    let id = allocate_id(&mut file, Counter::Completion)?;
    let date = now_iso();

    // Build the completion entry incrementally so the durability handoff
    // fields are appended only when supplied. The legacy 6-field shape stays
    // byte-identical when no scoped-commit metadata is provided; new callers
    // simply tack additional `:key value` pairs onto the same form.
    let mut entry = format!(
        "    ({id}\n      :phase {phase}\n      :agent {agent}\n      :summary {summary}\n      :deliverables {deliverables}\n      :verification {verification}\n      :at {date}",
        id = id,
        phase = lisp_quote_string(phase),
        agent = lisp_quote_string(agent),
        summary = lisp_quote_string(summary),
        deliverables = lisp_quote_string(deliverables),
        verification = lisp_quote_string(verification),
        date = lisp_quote_string(&date),
    );
    if let Some(ref list) = changed_files {
        entry.push_str(&format!(
            "\n      :changed-files {}",
            render_string_list(list)
        ));
    }
    if let Some(ref list) = staged_files {
        entry.push_str(&format!(
            "\n      :staged-files {}",
            render_string_list(list)
        ));
    }
    if let Some(ref hash) = commit_hash {
        entry.push_str(&format!("\n      :commit-hash {}", lisp_quote_string(hash)));
    }
    if let Some(ref status_val) = commit_status {
        entry.push_str(&format!(
            "\n      :commit-status {}",
            lisp_quote_string(status_val)
        ));
    }
    if let Some(ref blocker) = commit_blocker {
        entry.push_str(&format!(
            "\n      :commit-blocker {}",
            lisp_quote_string(blocker)
        ));
    }
    // wave-19 / task 08 — task-contract metadata. Each field skips when
    // absent so legacy callers that never set them keep the byte-identical
    // 6-field shape (or 11-field shape with scoped-commit fields).
    if let Some(ref tcp) = task_contract_path {
        entry.push_str(&format!(
            "\n      :task-contract-path {}",
            lisp_quote_string(tcp)
        ));
    }
    if let Some(ref trp) = task_report_path {
        entry.push_str(&format!(
            "\n      :task-report-path {}",
            lisp_quote_string(trp)
        ));
    }
    if let Some(ref vs) = verifier_status {
        entry.push_str(&format!(
            "\n      :verifier-status {}",
            lisp_quote_string(vs)
        ));
    }
    if let Some(ref vn) = verifier_notes {
        entry.push_str(&format!(
            "\n      :verifier-notes {}",
            lisp_quote_string(vn)
        ));
    }
    // wave-21 / task 03 — task-run verifier metadata. Each field skips
    // when absent so legacy callers (and wave19-08 callers that never
    // touched the wave21 slots) keep their byte-identical companion log
    // shape. `verified` is written as a bare `true`/`false` atom so a
    // round-trip through `parse_completions` recovers the boolean
    // without quoted-string handling.
    if let Some(ref trvs) = task_run_verifier_status {
        entry.push_str(&format!(
            "\n      :task-run-verifier-status {}",
            lisp_quote_string(trvs)
        ));
    }
    if let Some(ref smp) = shared_memory_path {
        entry.push_str(&format!(
            "\n      :shared-memory-path {}",
            lisp_quote_string(smp)
        ));
    }
    if let Some(ref vd) = verifier_diagnostics {
        entry.push_str(&format!(
            "\n      :verifier-diagnostics {}",
            lisp_quote_string(vd)
        ));
    }
    if let Some(v) = verified_flag {
        entry.push_str(&format!("\n      :verified {}", v));
    }
    entry.push(')');

    append_to_block(&mut file, "completions", &entry)?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    // Same dispatch-metadata projection rationale as `action_claim` —
    // surface the trio from the companion-log meta block so completion
    // consumers can route on workstation-dispatch context without reading
    // the on-disk file. Absent / legacy meta cleanly skip-serializes
    // (see ExecutionEvent::Completed doc comment).
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Completed {
            execution_id: execution_id.to_string(),
            completion_id: id.clone(),
            phase: phase.to_string(),
            agent: agent.to_string(),
            at: date.clone(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    let mut response = json!({
        "status": "recorded",
        "completion_id": id,
        "phase": phase,
        "agent": agent,
        "at": date,
        // Always surfaced so callers can detect at a glance which mode
        // the completion went through. `false` here means audit-only
        // (legacy / opt-out) — `true` means the durability invariants
        // were validated at write-time and the validation summary is
        // included below.
        "scoped_commit_enforced": enforce_scoped_commit,
    });
    if let Some(list) = changed_files {
        response["changed_files"] = json!(list);
    }
    if let Some(list) = staged_files {
        response["staged_files"] = json!(list);
    }
    if let Some(hash) = commit_hash {
        response["commit_hash"] = json!(hash);
    }
    if let Some(status_val) = commit_status {
        response["commit_status"] = json!(status_val);
    }
    if let Some(blocker) = commit_blocker {
        response["commit_blocker"] = json!(blocker);
    }
    if let Some(v) = scoped_commit_validation {
        response["scoped_commit_validation"] = v;
    }
    // wave-19 / task 08 — surface contract metadata + the contract-level
    // validation summary (when the gate ran). Skip-serialize semantics
    // mirror the scoped-commit fields above so the response stays
    // byte-identical for legacy callers that omit every wave19 field.
    if let Some(tcp) = task_contract_path {
        response["task_contract_path"] = json!(tcp);
    }
    if let Some(trp) = task_report_path {
        response["task_report_path"] = json!(trp);
    }
    // The wave19-08 caller-supplied `verifier_status` slot is preserved
    // verbatim when the wave22-02 auto-verifier did NOT run; otherwise
    // the daemon-computed status (set further below) wins so the
    // response surface advertises a single authoritative verdict.
    if let Some(ref vs) = verifier_status {
        response["verifier_status"] = json!(vs);
    }
    if let Some(vn) = verifier_notes {
        response["verifier_notes"] = json!(vn);
    }
    if let Some(v) = task_contract_validation {
        response["task_contract_validation"] = v;
    }
    // wave-21 / task 03 — surface task-run verifier metadata + the
    // verified-gate validation summary. Same skip-serialize semantics
    // as the wave19-08 fields above so legacy callers stay byte-
    // identical when they omit every wave21 field.
    if let Some(trvs) = task_run_verifier_status {
        response["task_run_verifier_status"] = json!(trvs);
    }
    if let Some(smp) = shared_memory_path {
        response["shared_memory_path"] = json!(smp);
    }
    // The wave21-03 caller-supplied `verifier_diagnostics` slot is
    // preserved verbatim when the wave22-02 auto-verifier did NOT run;
    // otherwise the daemon-computed diagnostic (set further below)
    // wins so reviewers see one diagnostic per response.
    if let Some(ref vd) = verifier_diagnostics {
        response["verifier_diagnostics"] = json!(vd);
    }
    if let Some(v) = verified_flag {
        response["verified"] = json!(v);
    }

    // ── wave-22 / task 02 — auto task-run verifier surface ────────────
    //
    // `verification_source` flags how the verdict was reached:
    //   * `daemon-auto-verifier` — daemon ran the in-tree verifier; the
    //     daemon-computed `verifier_status="passed"` overrides any
    //     caller-supplied wave19-08 / wave21-03 status. The structured
    //     `verified_scope_summary` records every cross-checked rule.
    //   * `legacy-caller-claim` — caller passed `verified=true` but at
    //     least one path was missing; daemon-computed status is
    //     `"unknown"` and `verifier_diagnostics` carries the migration
    //     prose pointing at the missing path(s).
    //
    // Absent `verification_source` (legacy callers) keeps the response
    // shape byte-identical to the wave21-03 surface.
    if let Some(src) = verification_source {
        response["verification_source"] = json!(src);
    }
    if let Some(status) = auto_verifier_status {
        // Daemon-computed verdict wins over the caller-supplied
        // wave19-08 / wave21-03 statuses. Reviewers can still see the
        // caller-supplied values inside `task_run_verifier_status` /
        // the companion log.
        response["verifier_status"] = json!(status);
    }
    if let Some(diag) = auto_verifier_diagnostics {
        response["verifier_diagnostics"] = json!(diag);
    }
    if let Some(scope_summary) = verified_validation {
        // wave-22 contract: the summary is exposed as
        // `verified_scope_summary`. We keep the wave21-03 shape under
        // the legacy `verified_validation` key too so existing
        // dashboards keep parsing while consumers migrate.
        response["verified_scope_summary"] = scope_summary.clone();
        response["verified_validation"] = scope_summary;
    }

    // wave23-04 — opt-in session-trace append. Records `complete` or
    // `failure` depending on the verifier verdict resolved above. The
    // entry mirrors the durable companion-log completion: it carries the
    // commit hash, report path, and changed-file list so future
    // analyzers can correlate completions with their durable artifacts
    // without re-reading the .missiond/v2/<exec>.lisp companion.
    if let Some(trace_path) = resolve_session_trace_path(args, &root) {
        match resolve_trace_task_id(args, &root, execution_id) {
            Some(task_id) => {
                // Failure when caller-supplied OR daemon-computed verifier
                // status resolved to "failed". Otherwise treat the
                // completion as a success-shaped event.
                let final_verifier_status = response
                    .get("verifier_status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let kind = match final_verifier_status.as_deref() {
                    Some("failed") => TraceKind::Failure,
                    _ => TraceKind::Complete,
                };
                let backend = sanitize_trace_backend(agent);
                // Re-read the commit / report / file metadata from args
                // since the local bindings above were consumed by the
                // response builder.
                let commit_hash_for_trace = args
                    .get("commit_hash")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    // checker requires `[0-9a-f]{4,64}` — drop anything
                    // shorter / non-hex so we don't fail validation.
                    .filter(|s| {
                        s.len() >= 4 && s.len() <= 64 && s.chars().all(|c| c.is_ascii_hexdigit())
                    });
                let report_path_for_trace = args
                    .get("task_report_path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    // checker rejects absolute report paths.
                    .filter(|s| !Path::new(s).is_absolute());
                let files_for_trace = collect_string_list(args, "changed_files")
                    .or_else(|| collect_string_list(args, "staged_files"))
                    .map(|v| {
                        v.into_iter()
                            // strip absolute paths — checker rejects them
                            .filter(|p| !Path::new(p).is_absolute())
                            .collect::<Vec<_>>()
                    })
                    .filter(|v: &Vec<String>| !v.is_empty());
                let ev = TraceEvent {
                    task: task_id,
                    backend,
                    kind,
                    summary: format!(
                        "mission_execution(action=complete) phase={} agent={} completion_id={}",
                        phase, agent, id
                    ),
                    agent: None,
                    files: files_for_trace,
                    commit_hash: commit_hash_for_trace,
                    report_path: report_path_for_trace,
                };
                if let Err(w) = append_session_trace_event(&trace_path, &ev) {
                    response["trace_warning"] = json!(w.to_string());
                }
            }
            None => {
                response["trace_warning"] = json!(format!(
                    "session_trace_path supplied but execution_id `{}` is not a valid trace task id and no task_contract_path was provided",
                    execution_id
                ));
            }
        }
    }

    Ok(ToolResult::json_pretty(&response))
}

// ───────────────────────────────────────────────────────────────────────
// action: audit — paren balance + ID monotonic + claim overlap + stale +
//                 completion coverage + open-issue owners
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_audit(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let raw = std::fs::read_to_string(&path)?;
    let mut findings: Vec<Value> = Vec::new();

    if let Err(e) = sexp::check_balance(&raw) {
        findings.push(json!({
            "severity": "error",
            "kind": "paren-imbalance",
            "detail": e.to_string(),
        }));
    }

    let file = match LogFile::parse(raw) {
        Ok(f) => f,
        Err(e) => {
            findings.push(json!({
                "severity": "error",
                "kind": "parse-failed",
                "detail": e.to_string(),
            }));
            return Ok(ToolResult::json_pretty(&json!({
                "execution_id": execution_id,
                "path": path.display().to_string(),
                "ok": false,
                "findings": findings,
            })));
        }
    };

    if file.find_block("id-counters").is_none() {
        findings.push(json!({
            "severity": "warn",
            "kind": "missing-id-counters",
            "detail": "id-counters block absent; mutating actions fall back to scan-max — run action=repair to materialize",
        }));
    }

    for counter in [
        Counter::Claim,
        Counter::Deviation,
        Counter::Decision,
        Counter::Issue,
        Counter::Completion,
    ] {
        check_id_monotonic(&file, counter, &mut findings);
    }

    let claims = parse_claims(&file);
    let now = Utc::now();
    for c in &claims {
        if c.status != "active" {
            continue;
        }
        if let Some(exp) = c.lease_expires_at.as_deref().and_then(parse_iso) {
            if exp < now {
                findings.push(json!({
                    "severity": "warn",
                    "kind": "stale-claim",
                    "claim_id": c.id,
                    "claimer": c.claimer,
                    "lease_expires_at": c.lease_expires_at,
                    "detail": "lease expired with no release/heartbeat",
                }));
            }
        }
    }

    // Active claim overlaps.
    for (i, a) in claims.iter().enumerate() {
        if a.status != "active" {
            continue;
        }
        for b in claims.iter().skip(i + 1) {
            if b.status != "active" {
                continue;
            }
            if scopes_overlap(&a.scope, &b.scope) {
                findings.push(json!({
                    "severity": "error",
                    "kind": "claim-overlap",
                    "left": a.id,
                    "right": b.id,
                    "scope_left": a.scope,
                    "scope_right": b.scope,
                }));
            }
        }
    }

    // Open-issue owners.
    let issues_block = file.find_block("issues");
    if let Some(block) = issues_block {
        for child in block.children().iter().skip(1) {
            let kvs = parse_kv_pairs(&file.src, child.children());
            let status = kvs
                .get("status")
                .map(|s| s.trim_matches('"').to_string())
                .unwrap_or_else(|| "open".to_string());
            if status == "resolved" || status == "closed" {
                continue;
            }
            let owner = kvs
                .get("owner")
                .map(|s| s.trim_matches('"').to_string())
                .unwrap_or_default();
            if owner.is_empty() {
                let head = child.head_atom().unwrap_or("?");
                findings.push(json!({
                    "severity": "warn",
                    "kind": "open-issue-no-owner",
                    "issue_id": head,
                }));
            }
        }
    }

    // Completion coverage: each phase referenced by a completion should have
    // a phase entry. We just check the inverse — phases marked completed in
    // phase-tracker should have at least one COMP entry referencing them.
    let phase_tracker = file
        .find_block("phase-tracker")
        .map(|m| parse_kv_pairs(&file.src, m.children()))
        .unwrap_or_default();
    if let Some(current) = phase_tracker.get("current-phase") {
        if current.trim().trim_matches('"') != "nil" && !current.trim().is_empty() {
            let comps = list_block_summaries(&file, "completions", |kvs, head| {
                Some(json!({
                    "id": head,
                    "phase": kvs.get("phase").cloned().unwrap_or_default(),
                }))
            });
            let _ = comps; // informational only — no failing assertion yet
        }
    }

    // ── scoped-commit handoff durability checks ──────────────────────
    // intent-memory.lisp :: helper agent-execution-coordination ::
    // scoped-commit-contract + invariants :inv-7 — every completion that
    // claims to have committed must carry a real commit_hash, every
    // blocked completion must explain itself, and staged_files must stay
    // inside the claim scope (the active or most-recently-released claim
    // owned by the same agent). These are read-only audit findings — the
    // daemon never executes git itself; the writer agent is responsible
    // for the actual commit. See task-file
    // wave12-01-mission-execution-scoped-commit-handoff.md.
    audit_scoped_commit_handoff(&file, &claims, &mut findings);

    let ok = findings.iter().all(|f| {
        f.get("severity")
            .and_then(|v| v.as_str())
            .map(|s| s != "error")
            .unwrap_or(true)
    });

    let error_count = findings
        .iter()
        .filter(|f| f.get("severity").and_then(|v| v.as_str()) == Some("error"))
        .count() as u32;
    // Wave 20 / Task 09 — surface the workstation-dispatch trio on the
    // audit + stale-claim events. Audit is read-only so we don't write
    // back to the file; the meta block we observe is whatever the latest
    // writer left there.
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Audited {
            execution_id: execution_id.to_string(),
            ok,
            findings_count: findings.len() as u32,
            error_count,
            dispatch_strategy: meta.dispatch_strategy.clone(),
            target_project: meta.target_project.clone(),
            requested_cwd: meta.requested_cwd.clone(),
        },
    )
    .await;
    for f in &findings {
        if f.get("kind").and_then(|v| v.as_str()) != Some("stale-claim") {
            continue;
        }
        let claim_id = f
            .get("claim_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let claimer = f
            .get("claimer")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let lease_expires_at = f
            .get("lease_expires_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        emit_execution_event(
            state,
            ExecutionEvent::StaleClaim {
                execution_id: execution_id.to_string(),
                claim_id,
                claimer,
                lease_expires_at,
                dispatch_strategy: meta.dispatch_strategy.clone(),
                target_project: meta.target_project.clone(),
                requested_cwd: meta.requested_cwd.clone(),
            },
        )
        .await;
    }

    Ok(ToolResult::json_pretty(&json!({
        "execution_id": execution_id,
        "path": path.display().to_string(),
        "ok": ok,
        "findings": findings,
    })))
}

// ───────────────────────────────────────────────────────────────────────
// action: repair — dry-run by default; structural fixes only
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_repair(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let mode = args
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("dry_run");
    if mode != "dry_run" && mode != "apply" {
        return Ok(ToolResult::structured_error(ToolError::new(
            error_codes::INVALID_PARAM,
            format!("repair mode must be `dry_run` or `apply`, got `{}`", mode),
        )));
    }

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let raw = std::fs::read_to_string(&path)?;
    let mut file = LogFile::parse(raw)?;

    let mut actions: Vec<Value> = Vec::new();

    // 1) Synthesize missing id-counters with values derived from scan_max_id.
    if file.find_block("id-counters").is_none() {
        let claim_n = scan_max_id(&file, Counter::Claim) + 1;
        let dev_n = scan_max_id(&file, Counter::Deviation) + 1;
        let dec_n = scan_max_id(&file, Counter::Decision) + 1;
        let issue_n = scan_max_id(&file, Counter::Issue) + 1;
        let comp_n = scan_max_id(&file, Counter::Completion) + 1;
        actions.push(json!({
            "kind": "synthesize-id-counters",
            "next_claim_id": claim_n,
            "next_deviation_id": dev_n,
            "next_decision_id": dec_n,
            "next_issue_id": issue_n,
            "next_completion_id": comp_n,
        }));
        if mode == "apply" {
            insert_id_counters_block(&mut file, claim_n, dev_n, dec_n, issue_n, comp_n)?;
        }
    }

    // 2) Mark stale claims (lease expired, no release).
    let claims = parse_claims(&file);
    let now = Utc::now();
    let mut stale_ids = Vec::new();
    for c in &claims {
        if c.status != "active" {
            continue;
        }
        if let Some(exp) = c.lease_expires_at.as_deref().and_then(parse_iso) {
            if exp < now {
                stale_ids.push(c.id.clone());
            }
        }
    }
    for id in &stale_ids {
        actions.push(json!({
            "kind": "mark-stale-claim",
            "claim_id": id,
        }));
        if mode == "apply" {
            if let Some(node) = find_claim_node(&file, id).cloned() {
                update_kv_in_node(&mut file, &node, "status", &lisp_quote_string("stale"))?;
            }
        }
    }

    // 3) Rebuild derived-indexes if it exists; otherwise leave alone (the
    //    block is cache, not truth — status action recomputes anyway).
    if file.find_block("derived-indexes").is_some() {
        actions.push(json!({
            "kind": "rebuild-derived-indexes",
            "note": "regenerating from durable slots"
        }));
        if mode == "apply" {
            rebuild_derived_indexes(&mut file)?;
        }
    }

    if mode == "apply" && !actions.is_empty() {
        touch_last_updated(&mut file)?;
        write_log_file(&path, &file)?;
    }

    // Wave 20 / Task 09 — surface the workstation-dispatch trio on
    // repair events. The same `file` handle is current after any
    // apply-mode mutations above, so the meta block we observe is the
    // post-write authoritative state.
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Repaired {
            execution_id: execution_id.to_string(),
            applied: mode == "apply",
            action_count: actions.len() as u32,
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "execution_id": execution_id,
        "path": path.display().to_string(),
        "mode": mode,
        "actions": actions,
        "applied": mode == "apply",
    })))
}

fn rebuild_derived_indexes(file: &mut LogFile) -> Result<()> {
    let claims = parse_claims(file);
    let now = Utc::now();
    let active_ids: Vec<String> = claims
        .iter()
        .filter(|c| {
            c.status == "active"
                && c.lease_expires_at
                    .as_deref()
                    .and_then(parse_iso)
                    .map(|exp| exp >= now)
                    .unwrap_or(true)
        })
        .map(|c| c.id.clone())
        .collect();

    let open_issue_ids = list_block_summaries(file, "issues", |kvs, head| {
        let status = kvs
            .get("status")
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_else(|| "open".to_string());
        if status == "resolved" || status == "closed" {
            None
        } else {
            Some(Value::String(head.to_string()))
        }
    });

    let unresolved_dev_ids = list_block_summaries(file, "deviations", |kvs, head| {
        let status = kvs
            .get("status")
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_else(|| "open".to_string());
        if status == "resolved" || status == "closed" {
            None
        } else {
            Some(Value::String(head.to_string()))
        }
    });

    let latest_decisions = list_block_summaries(file, "decisions", |_kvs, head| {
        Some(Value::String(head.to_string()))
    });
    let completed_phases = list_block_summaries(file, "completions", |kvs, _head| {
        Some(Value::String(
            kvs.get("phase")
                .map(|s| s.trim_matches('"').to_string())
                .unwrap_or_default(),
        ))
    });

    let render_list = |items: &[Value]| -> String {
        let parts: Vec<String> = items
            .iter()
            .filter_map(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(lisp_quote_string)
            .collect();
        if parts.is_empty() {
            "()".to_string()
        } else {
            format!("({})", parts.join(" "))
        }
    };

    let block = match file.find_block("derived-indexes").cloned() {
        Some(b) => b,
        None => return Ok(()),
    };
    let active_lit = render_list(
        &active_ids
            .iter()
            .map(|s| Value::String(s.clone()))
            .collect::<Vec<_>>(),
    );
    let issues_lit = render_list(&open_issue_ids);
    let dev_lit = render_list(&unresolved_dev_ids);
    let dec_lit = render_list(&latest_decisions);
    let phases_lit = render_list(&completed_phases);

    update_kv_in_node(file, &block, "active-claims", &active_lit)?;
    let block2 = file
        .find_block("derived-indexes")
        .cloned()
        .ok_or_else(|| anyhow!("derived-indexes vanished"))?;
    update_kv_in_node(file, &block2, "open-issues", &issues_lit)?;
    let block3 = file
        .find_block("derived-indexes")
        .cloned()
        .ok_or_else(|| anyhow!("derived-indexes vanished"))?;
    update_kv_in_node(file, &block3, "unresolved-deviations", &dev_lit)?;
    let block4 = file
        .find_block("derived-indexes")
        .cloned()
        .ok_or_else(|| anyhow!("derived-indexes vanished"))?;
    update_kv_in_node(file, &block4, "latest-decisions", &dec_lit)?;
    let block5 = file
        .find_block("derived-indexes")
        .cloned()
        .ok_or_else(|| anyhow!("derived-indexes vanished"))?;
    update_kv_in_node(file, &block5, "completed-phases", &phases_lit)?;
    Ok(())
}
