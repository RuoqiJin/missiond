use anyhow::{anyhow, Result};
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::claim_lease::{parse_claims, scopes_overlap_pure};
use super::log_surface::{
    parse_kv_pairs,
    sexp::{self, Node, NodeKind},
    LogFile,
};

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

/// wave-21 / task 03 — minimal report-contract reader.
///
/// Pulls just the keys the daemon-side cross-check needs (`:schema`,
/// `:task_id`, `:commit_hash`) out of a `(report <id> ...)` form using
/// the local sexp parser. No new dependency, no new lisp dialect — the
/// projector trusts the authoritative schema checker
/// (`scripts/check-task-report.mjs`) for shape policing and only echoes
/// the three fields the daemon needs for the wave21-03 verified-gate
/// cross-check.
pub(super) struct ReportSummary {
    pub(super) schema: Option<String>,
    pub(super) task_id: Option<String>,
    pub(super) commit_hash: Option<String>,
}

pub(super) fn read_report_summary(text: &str) -> Result<ReportSummary> {
    let nodes = sexp::parse(text)?;
    let top = nodes
        .first()
        .ok_or_else(|| anyhow!("report file is empty"))?;
    if top.head_atom() != Some("report") {
        return Err(anyhow!(
            "top-level form must be `(report <id> ...)`, got `{}`",
            top.head_atom().unwrap_or("<non-atom>")
        ));
    }
    // children = [Atom("report"), Atom(<id>), :keyword, value, :keyword, value, ...]
    let kids = top.children();
    let mut schema = None;
    let mut task_id = None;
    let mut commit_hash = None;
    let mut i = 2;
    while i + 1 < kids.len() {
        let key = match kids[i].as_atom() {
            Some(a) if a.starts_with(':') => &a[1..],
            _ => {
                i += 1;
                continue;
            }
        };
        let val = &kids[i + 1];
        let val_str = match &val.kind {
            sexp::NodeKind::Str(s) => Some(s.clone()),
            sexp::NodeKind::Atom(a) => Some(a.clone()),
            _ => None,
        };
        match key {
            "schema" => schema = val_str.filter(|s| !s.is_empty()),
            "task_id" => task_id = val_str.filter(|s| !s.is_empty()),
            "commit_hash" => commit_hash = val_str.filter(|s| !s.is_empty()),
            _ => {}
        }
        i += 2;
    }
    Ok(ReportSummary {
        schema,
        task_id,
        commit_hash,
    })
}

/// wave-21 / task 03 — pull the task-contract head id (the `<id>` in
/// `(task <id> ...)`) so the daemon-side cross-check can match it
/// against the report's `:task_id`. Returns `None` when the file is
/// shaped unexpectedly — caller treats that as advisory.
pub(super) fn read_task_contract_id(text: &str) -> Option<String> {
    let nodes = sexp::parse(text).ok()?;
    let top = nodes.first()?;
    if top.head_atom() != Some("task") {
        return None;
    }
    let kids = top.children();
    kids.get(1).and_then(|n| n.as_atom().map(|s| s.to_string()))
}

// ───────────────────────────────────────────────────────────────────────
// wave-22 / task 02 — auto task-run verifier (in-process, read-only)
// ───────────────────────────────────────────────────────────────────────
//
// Lifts the wave21-03 caller-supplied `verified=true` claim into a
// daemon-computed verdict. When `action_complete` sees all four of
// `task_contract_path`, `task_report_path`, `shared_memory_path`, and
// `commit_hash` the daemon runs the in-tree task-run verifier itself —
// no Node spawn, no shell, no mutating git, no process boundary at all.
// The script-side `scripts/verify-task-run.mjs` (wave21-02) remains the
// out-of-process truth; this in-process projection delivers the same
// verdict during the action-complete window so callers stop relying on
// the caller-supplied `verified` flag as an escape hatch.
//
// Three fail-fast checks fold together:
//   1. task contract loadable + commit_hash present (re-uses the
//      wave19-08 helper internals so a future schema update tracks).
//   2. report cross-check: schema = `missiond.report-contract.v1`,
//      `:task_id` matches the contract head id, `:commit_hash` matches
//      the supplied hash (full string equality OR prefix overlap, same
//      rule as the wave21-03 gate so the two stay byte-identical).
//   3. shared-memory ledger: schema = `missiond.shared-memory.v1` AND
//      contains a `(completion :task <contract-id> ...)` entry — the
//      wave21-02 verifier's "ledger references the task" rule rendered
//      in pure Rust against the same on-disk file.
//
// Failures surface deterministic structured codes so dashboards can
// route on them without re-parsing prose:
//
//   * `TASK_REPORT_REQUIRED` / `TASK_REPORT_MALFORMED` /
//     `TASK_REPORT_TASK_ID_MISMATCH` / `TASK_REPORT_COMMIT_HASH_MISMATCH`
//     — re-used from the wave21-03 vocabulary so consumers see one
//     vocabulary across both gates.
//   * `TASK_CONTRACT_REQUIRED` / `TASK_CONTRACT_MALFORMED` — re-used
//     from the wave19-08 vocabulary for the same reason.
//   * `SHARED_MEMORY_REQUIRED` — `shared_memory_path` does not resolve
//     to a readable file under the project root.
//   * `SHARED_MEMORY_MALFORMED` — file parses but `:schema` is missing
//     / wrong, or there is no `(shared-memory ...)` form.
//   * `SHARED_MEMORY_NO_COMPLETION_FOR_TASK` — file is well-formed but
//     contains no `(completion :task <id> ...)` entry for the contract
//     head id.
//
// Returns the structured `verified_scope_summary` payload on success;
// `action_complete` folds it into the response under the same key.
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
    let contract_raw = std::path::Path::new(task_contract_path);
    let contract_resolved: PathBuf = if contract_raw.is_absolute() {
        contract_raw.to_path_buf()
    } else {
        project_root.join(contract_raw)
    };
    // The loaded contract value itself is unused — `read_task_contract_id`
    // below re-parses the head id from raw text — but the load call is
    // intentional: it surfaces TASK_CONTRACT_REQUIRED / TASK_CONTRACT_MALFORMED
    // before the cheaper text-side projector runs, keeping the wave22-02
    // auto-verifier's error vocabulary aligned with the wave19-08 verifier.
    let _contract = match super::super::workstation_dispatch::load_task_contract(&contract_resolved)
    {
        Ok(c) => c,
        Err(e) => {
            use super::super::workstation_dispatch::TaskContractParseError as Tce;
            let (code, message) = match &e {
                Tce::Io(detail) => (
                    "TASK_CONTRACT_REQUIRED",
                    format!(
                        "task_contract_path `{}` is not readable: {}",
                        contract_resolved.display(),
                        detail
                    ),
                ),
                _ => (
                    "TASK_CONTRACT_MALFORMED",
                    format!(
                        "task_contract_path `{}` failed schema parse: {}",
                        contract_resolved.display(),
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
    // Recover the head id via the local mini-reader so we depend on the
    // same projector the wave21-03 gate uses (cross-check anchor).
    let contract_text = match std::fs::read_to_string(&contract_resolved) {
        Ok(s) => s,
        Err(e) => {
            return Err(ToolResult::structured_error(ToolError::new(
                "TASK_CONTRACT_REQUIRED",
                format!(
                    "task_contract_path `{}` became unreadable mid-verification: {}",
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

    // (2) Resolve + load the report-contract. Mirrors the wave21-03
    // verified-gate's checks (schema, task_id, commit_hash) so the two
    // gates stay semantically aligned — only the trigger differs.
    let report_raw = std::path::Path::new(task_report_path);
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
    match report.schema.as_deref() {
        Some("missiond.report-contract.v1") => {}
        Some(other) => {
            return Err(ToolResult::structured_error(
                ToolError::new(
                    "TASK_REPORT_MALFORMED",
                    format!(
                        "task_report_path `{}` :schema must equal `missiond.report-contract.v1`, got `{}`",
                        report_resolved.display(),
                        other
                    ),
                ),
            ));
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

    // (3) Resolve + load the shared-memory ledger. The script-side
    // verifier requires a `(completion :task <id> ...)` entry; the
    // daemon mirrors that rule using the in-tree sexp parser so the two
    // produce identical verdicts on the same files.
    let memory_raw = std::path::Path::new(shared_memory_path);
    let memory_resolved: PathBuf = if memory_raw.is_absolute() {
        memory_raw.to_path_buf()
    } else {
        project_root.join(memory_raw)
    };
    let memory_text = match std::fs::read_to_string(&memory_resolved) {
        Ok(s) => s,
        Err(e) => {
            return Err(ToolResult::structured_error(
                ToolError::new(
                    "SHARED_MEMORY_REQUIRED",
                    format!(
                        "shared_memory_path `{}` is not readable: {}",
                        memory_resolved.display(),
                        e
                    ),
                )
                .with_suggestion(
                    "ensure the path resolves under the project root and the wave shared-memory ledger exists",
                ),
            ));
        }
    };
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
        return Err(ToolResult::structured_error(
            ToolError::new(
                "SHARED_MEMORY_MALFORMED",
                format!(
                    "shared_memory_path `{}` :schema must equal `missiond.shared-memory.v1`, got `{:?}`",
                    memory_resolved.display(),
                    ledger.schema,
                ),
            ),
        ));
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

/// wave-22 / task 02 — minimal shared-memory ledger projector.
///
/// Pulls just the `:schema` field and the list of `:task` ids that
/// appear inside `(completion ...)` children. Mirrors the wave21-02
/// `loadLedger` projection in `scripts/verify-task-run.mjs` so the
/// daemon-side auto-verifier hits the same rule:
/// `ledger.completions.some(c => c.task === contract.id)`.
pub(super) struct SharedMemorySummary {
    pub(super) schema: Option<String>,
    pub(super) completion_tasks: Vec<String>,
}

pub(super) fn read_shared_memory_ledger(text: &str) -> Result<SharedMemorySummary, anyhow::Error> {
    let nodes = sexp::parse(text)?;
    let top = nodes
        .iter()
        .find(|n| n.head_atom() == Some("shared-memory"))
        .ok_or_else(|| anyhow!("no `(shared-memory ...)` form found"))?;
    let kids = top.children();
    let mut schema: Option<String> = None;
    // children layout mirrors `(shared-memory <wave> :keyword value ... (claim ...) (completion ...))`
    // We walk the children once: bare keyword/value pairs feed the
    // metadata bag; nested lists matching `(completion :task <id> ...)`
    // feed the completion tasks list.
    let mut completion_tasks: Vec<String> = Vec::new();
    let mut i = 2; // skip head atom + wave id
    while i < kids.len() {
        let node = &kids[i];
        match &node.kind {
            NodeKind::Atom(a) if a.starts_with(':') => {
                if i + 1 < kids.len() {
                    let key = &a[1..];
                    let val = &kids[i + 1];
                    let val_str = match &val.kind {
                        NodeKind::Str(s) => Some(s.clone()),
                        NodeKind::Atom(s) => Some(s.clone()),
                        _ => None,
                    };
                    if key == "schema" {
                        schema = val_str.filter(|s| !s.is_empty());
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            NodeKind::List(_) | NodeKind::Bracket(_) => {
                if node.head_atom() == Some("completion") {
                    let task_id = read_completion_task_id(node);
                    if let Some(id) = task_id {
                        completion_tasks.push(id);
                    }
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    Ok(SharedMemorySummary {
        schema,
        completion_tasks,
    })
}

/// Pull the `:task` keyword value out of a `(completion :id ... :task <id> ...)` form.
/// Returns `None` when the entry has no `:task` slot — the auto-verifier
/// silently ignores such entries because the wave21-02 script-side
/// verifier uses the same "must have :task" rule when matching.
pub(super) fn read_completion_task_id(node: &Node) -> Option<String> {
    let kids = node.children();
    let mut i = 1; // skip head atom `completion`
    while i + 1 < kids.len() {
        if let Some(atom) = kids[i].as_atom() {
            if atom == ":task" {
                let val = &kids[i + 1];
                return match &val.kind {
                    NodeKind::Str(s) => Some(s.clone()),
                    NodeKind::Atom(s) => Some(s.clone()),
                    _ => None,
                };
            }
        }
        i += 2;
    }
    None
}

// ───────────────────────────────────────────────────────────────────────
// action: preflight_commit — read-only worktree audit before scoped commit
//
// Wave 18 / Task 08. The daemon may inspect git status / diff but MUST
// NEVER stage/commit/reset/checkout. The writer agent is the only actor
// that mutates the worktree; we just project worktree state vs the
// active+released claim scopes so the writer can see scope drift before
// running its scoped commit.
//
// Pairs with `enforce_scoped_commit_completion` (wave16-06) which is the
// post-commit gate; preflight catches the same violations one step
// earlier so the writer doesn't have to roll back a bad stage.
//
// Wave 20 / Task 03 augmentation: when the caller threads
// `task_contract_path` through the preflight call, daemon also loads the
// task-contract v1 (read-only) and projects the staged set against the
// contract's `:write-scope` / `:must-not-touch` patterns. Two new
// top-level fields (`staged_out_of_scope`, `staged_forbidden`) plus
// `unstaged_in_scope` and a `task_contract_status` label surface so the
// writer learns about contract-level drift one hop earlier than the
// post-commit `task-scope-guard.mjs`. Daemon still runs no mutating git
// command — `evaluate_task_contract_for_preflight` is pure file IO + a
// glob projection.
// ───────────────────────────────────────────────────────────────────────

/// Single-file entry from `git status --porcelain=v1`. The first byte is
/// the index (staged) status, the second is the worktree status; we
/// surface both so the caller can tell "staged but reverted in worktree"
/// from "edited but not staged".
///
/// We deliberately keep the struct minimal and plain — no path
/// canonicalization here, since rename pairs / quoted paths would require
/// shelling out to `git diff` per entry. The audit needs file paths
/// relative to the project root, which porcelain v1 already provides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PorcelainEntry {
    /// Index/staged status byte (`'M'`, `'A'`, `'D'`, `'R'`, `'?'`, ` `, …).
    pub(super) index_status: char,
    /// Worktree status byte (same alphabet as `index_status`).
    pub(super) worktree_status: char,
    /// Path as reported by porcelain (rename right-hand side when applicable).
    pub(super) path: String,
}

impl PorcelainEntry {
    /// True when the index slot reflects a tracked staged change
    /// (anything but ` ` / `?` / `!`). Untracked / ignored files never
    /// count as staged because porcelain marks them with `?` / `!`.
    pub(super) fn is_staged(&self) -> bool {
        !matches!(self.index_status, ' ' | '?' | '!')
    }

    /// True when the worktree slot reflects an unstaged change OR the
    /// file is untracked — both shapes carry "would be touched by an
    /// over-broad `git add .`". Ignored files (`!`) stay out so the
    /// preflight doesn't flag `.gitignore`d build artefacts.
    pub(super) fn is_changed(&self) -> bool {
        match (self.index_status, self.worktree_status) {
            ('!', _) | (_, '!') => false,
            _ => self.index_status != ' ' || self.worktree_status != ' ',
        }
    }
}

/// Parse the textual output of `git status --porcelain=v1`. Returns an
/// owned `Vec<PorcelainEntry>` so the caller is free of any borrow on
/// the source string.
///
/// Rules:
///   - skip blank lines.
///   - rename entries (`R` / `C` in the index slot) carry the rename
///     pair on a single line as `RENAMED -> ORIG`; we record the
///     right-hand side which is the post-rename path, matching what the
///     scoped-commit audit cares about.
///   - quoted paths (porcelain c-style escapes when the path contains
///     special bytes) are forwarded verbatim with the surrounding
///     quotes — this preserves round-trip fidelity even though
///     scope-overlap matching against quoted paths will fail-by-design;
///     the violator surfaces in `out_of_scope_files` so the writer can
///     widen the claim or rename the file.
///
/// We keep this parser deliberately tiny and pure: no panics, no
/// allocations beyond the obvious `String` per path, no calls into the
/// process. That means the fail-fast contract from the task brief — the
/// daemon never spawns a mutating git command — sits one level up
/// (`run_git_status`).
pub(super) fn parse_porcelain_status(text: &str) -> Vec<PorcelainEntry> {
    let mut out = Vec::new();
    for raw in text.lines() {
        if raw.is_empty() {
            continue;
        }
        let bytes = raw.as_bytes();
        if bytes.len() < 4 {
            // Defensive: malformed line, skip silently. Porcelain v1
            // always emits at least `XY <path>` (4+ chars).
            continue;
        }
        let index_status = bytes[0] as char;
        let worktree_status = bytes[1] as char;
        let rest = &raw[3..];
        // Rename / copy pairs separate `OLD -> NEW`; we pin the new
        // path because that is what lives on disk after `git add`.
        let path = if (index_status == 'R' || index_status == 'C') && rest.contains(" -> ") {
            // unwrap is safe because contains() returned true.
            rest.split(" -> ").nth(1).unwrap().to_string()
        } else {
            rest.to_string()
        };
        out.push(PorcelainEntry {
            index_status,
            worktree_status,
            path,
        });
    }
    out
}

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

/// wave-20 / task 03 — repo-relative path-vs-pattern matcher used by the
/// task-contract scope projection in preflight. Mirrors the JS helper in
/// `scripts/lib/missiond_lisp.mjs::pathMatchesPattern` so daemon-side
/// preflight, the post-commit guard (`scripts/task-scope-guard.mjs`), and
/// the verifier (`scripts/verify-task-contract.mjs`) all key off the same
/// glob semantics. The contract is intentionally narrow:
///
///   * Patterns and paths are normalised by stripping `\\` → `/`,
///     leading `./`, and leading `/` so the comparison is repo-relative.
///   * A pattern with no glob meta-characters matches either the exact
///     path OR any file under that path when the pattern names a
///     directory prefix (e.g. `crates/` or `crates` matches
///     `crates/foo/bar.rs`).
///   * `*` matches any sequence of characters except `/`.
///   * `**` matches any sequence including `/` (folder hops).
///   * `?` matches a single character except `/`.
///   * Other regex meta-characters are escaped — the matcher is glob-only,
///     never a full regex evaluator.
///
/// Daemon-only fail-fast posture: an empty pattern OR an empty path never
/// matches. Empty inputs are a contract bug upstream; we surface them as
/// "no match" so the caller sees the path land in `staged_out_of_scope`
/// rather than silently coercing them through.
pub(super) fn pattern_matches_path(file_path: &str, pattern: &str) -> bool {
    if file_path.is_empty() || pattern.is_empty() {
        return false;
    }
    let norm_path = normalize_repo_relative(file_path);
    let pat = normalize_repo_relative(pattern);
    if !pat.contains('*') && !pat.contains('?') {
        if norm_path == pat {
            return true;
        }
        let prefix = if pat.ends_with('/') {
            pat.clone()
        } else {
            format!("{}/", pat)
        };
        return norm_path.starts_with(&prefix);
    }
    glob_to_regex(&pat).is_match(&norm_path)
}

/// Normalise a path or pattern to a repo-relative form: backslash → slash,
/// strip a single leading `./`, and any leading `/` so absolute-style
/// patterns (rare in our contracts) still match repo-relative entries.
fn normalize_repo_relative(input: &str) -> String {
    let mut s = input.replace('\\', "/");
    if let Some(stripped) = s.strip_prefix("./") {
        s = stripped.to_string();
    }
    while let Some(stripped) = s.strip_prefix('/') {
        s = stripped.to_string();
    }
    s
}

/// Compile a glob pattern into a regex anchored on both ends. Mirrors the
/// JS `globToRegExp` in `scripts/lib/missiond_lisp.mjs` so the JS guard
/// and the daemon-side preflight stay in lock-step.
fn glob_to_regex(pattern: &str) -> regex::Regex {
    let mut out = String::with_capacity(pattern.len() + 4);
    out.push('^');
    let bytes: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == '*' {
            if i + 1 < bytes.len() && bytes[i + 1] == '*' {
                out.push_str(".*");
                i += 2;
                // mirror the JS swallow: a following `/` is consumed by `.*`
            } else {
                out.push_str("[^/]*");
                i += 1;
            }
        } else if c == '?' {
            out.push_str("[^/]");
            i += 1;
        } else if matches!(
            c,
            '.' | '+' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\'
        ) {
            out.push('\\');
            out.push(c);
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out.push('$');
    // Pattern is glob-derived so cannot fail; build a permissive fallback
    // (matches nothing) to preserve fail-fast posture without panicking on
    // pathological contract input.
    regex::Regex::new(&out).unwrap_or_else(|_| regex::Regex::new("$.^").unwrap())
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

/// Run `git status --porcelain=v1` under `root` (read-only). Returns the
/// raw stdout text on success, or a structured `ToolResult` error when
/// git is unavailable or refuses to operate on the path.
///
/// Safety: the only git subcommand spawned by this module is `status`
/// + `--porcelain=v1`. There is **no** `git add / commit / reset /
/// checkout` codepath in this file — grep for `Command.*git.*(add|
/// commit|reset|checkout)` over `agent_execution.rs` returns zero hits
/// (verified at PR time).
pub(super) fn run_git_status(root: &Path) -> std::result::Result<String, ToolResult> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(root)
        .output()
        .map_err(|e| {
            ToolResult::structured_error(
                ToolError::new(
                    error_codes::EXTERNAL_ERROR,
                    format!(
                        "failed to spawn `git status` under {}: {}",
                        root.display(),
                        e
                    ),
                )
                .with_suggestion("ensure git is installed and the project root is a worktree"),
            )
        })?;
    if !output.status.success() {
        return Err(ToolResult::structured_error(
            ToolError::new(
                error_codes::EXTERNAL_ERROR,
                format!(
                    "`git status` exited non-zero under {}: {}",
                    root.display(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            )
            .with_suggestion("verify the project root is a git worktree (no `--git-dir` override)"),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
