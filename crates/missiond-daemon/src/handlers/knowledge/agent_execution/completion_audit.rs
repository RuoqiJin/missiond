use anyhow::{anyhow, Result};
use chrono::Utc;
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::state::AppState;

use super::claim_lease::{
    find_claim_node, parse_claims, parse_iso, scopes_overlap, scopes_overlap_pure, ClaimRecord,
};
use super::log_surface::{
    append_session_trace_event, companion_path, emit_execution_event, insert_id_counters_block,
    lisp_quote_string, list_block_summaries, parse_kv_pairs, project_or_target_project,
    read_dispatch_metadata_from_log, read_log_file, require_str, resolve_project_root,
    resolve_session_trace_path, resolve_trace_task_id, scan_max_id,
    sexp::{self, Node, NodeKind},
    touch_last_updated, update_kv_in_node, write_log_file, Counter, LogFile, TraceEvent, TraceKind,
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
    if !enforce_scoped_commit {
        return Err(ToolResult::structured_error(
            ToolError::new(
                "VERIFIED_REQUIRES_ENFORCEMENT",
                "verified=true requires enforce_scoped_commit=true so the underlying scope + contract gates also run",
            )
            .with_suggestion(
                "set enforce_scoped_commit=true alongside verified=true, or omit verified for legacy completions",
            ),
        ));
    }
    let tcp = task_contract_path
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ToolResult::structured_error(
                ToolError::new(
                    "VERIFIED_REQUIRES_TASK_CONTRACT",
                    "verified=true requires a non-empty task_contract_path so the daemon-side cross-check can resolve the contract",
                )
                .with_suggestion(
                    "supply task_contract_path pointing at the task-contract v1 lisp file the dispatch brief used",
                ),
            )
        })?;
    let trp = task_report_path
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ToolResult::structured_error(
                ToolError::new(
                    "VERIFIED_REQUIRES_TASK_REPORT",
                    "verified=true requires a non-empty task_report_path so the daemon can read the report-contract off disk",
                )
                .with_suggestion(
                    "supply task_report_path pointing at the report-contract v1 lisp file the writer produced",
                ),
            )
        })?;
    let hash = commit_hash
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ToolResult::structured_error(
                ToolError::new(
                    "VERIFIED_REQUIRES_COMMIT_HASH",
                    "verified=true requires a non-empty commit_hash so the daemon can match it against the report's :commit_hash",
                )
                .with_suggestion(
                    "report the durable scoped commit hash, or omit verified for non-verified completions",
                ),
            )
        })?;

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

    if let Some(report_task_id) = report.task_id.as_deref() {
        if report_task_id != contract_id {
            return Err(ToolResult::structured_error(
                ToolError::new(
                    "TASK_REPORT_TASK_ID_MISMATCH",
                    format!(
                        "task_report :task_id `{}` does not match task contract head id `{}` (contract `{}`, report `{}`)",
                        report_task_id,
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
    } else {
        return Err(ToolResult::structured_error(ToolError::new(
            "TASK_REPORT_MALFORMED",
            format!(
                "task_report_path `{}` is missing required `:task_id` field",
                report_resolved.display()
            ),
        )));
    }

    if let Some(report_hash) = report.commit_hash.as_deref() {
        // Accept short<->long sha overlap: either side may be a prefix
        // of the other. Mirrors how `git log --format=%h` truncates
        // hashes to 7+ chars by default, while `git rev-parse HEAD`
        // returns the full 40-char form.
        let matches =
            report_hash == hash || report_hash.starts_with(hash) || hash.starts_with(report_hash);
        if !matches {
            return Err(ToolResult::structured_error(
                ToolError::new(
                    "TASK_REPORT_COMMIT_HASH_MISMATCH",
                    format!(
                        "task_report :commit_hash `{}` does not match completion commit_hash `{}` (report `{}`)",
                        report_hash,
                        hash,
                        report_resolved.display(),
                    ),
                )
                .with_suggestion(
                    "regenerate the report against the durable commit, or correct the completion commit_hash",
                ),
            ));
        }
    } else {
        return Err(ToolResult::structured_error(ToolError::new(
            "TASK_REPORT_MALFORMED",
            format!(
                "task_report_path `{}` is missing required `:commit_hash` field",
                report_resolved.display()
            ),
        )));
    }

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

pub(super) async fn action_preflight_commit(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };

    // Resolve project root through the registry — same gate every other
    // action uses. Refusing unresolved roots is part of the wave18-08
    // safety contract: we never run git outside an explicitly registered
    // project (or the active CWD when no project is supplied).
    let root = match resolve_project_root(state, project_or_target_project(args)).await {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("cannot resolve project root: {}", e),
                )
                .with_suggestion(
                    "register the project via `mission_project(action=add, …)` or call from inside the project worktree",
                ),
            ));
        }
    };

    // Optional `cwd` override — must stay inside the resolved project
    // root. We canonicalize both sides so symlinks / `..` traversals
    // can't escape the project boundary. If canonicalization fails we
    // refuse rather than silently fall back to root, matching the
    // fail-fast posture of the wave16-06 enforcement gate.
    let cwd_arg = args
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let inspect_dir = match cwd_arg {
        Some(cwd) => {
            let candidate = PathBuf::from(cwd);
            let abs = if candidate.is_absolute() {
                candidate
            } else {
                root.join(candidate)
            };
            let canon_root = root.canonicalize().unwrap_or_else(|_| root.clone());
            let canon_abs = match abs.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    return Ok(ToolResult::structured_error(ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!("cwd `{}` does not exist or is not accessible: {}", cwd, e),
                    )));
                }
            };
            if !canon_abs.starts_with(&canon_root) {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "cwd `{}` resolves outside the project root `{}`",
                            cwd,
                            root.display()
                        ),
                    )
                    .with_suggestion("supply a path inside the project, or omit `cwd`"),
                ));
            }
            canon_abs
        }
        None => root.clone(),
    };

    // Expected_files hint from the workstation brief. Trimmed and
    // empty-filtered through the same helper as `staged_files` so the
    // writer doesn't need to pre-clean its list.
    let expected_files = collect_string_list(args, "expected_files");

    // Companion log read — same path resolution as every other action.
    // We need the claims block for scope comparison; opening the file
    // also doubles as a "did the writer pass a real execution_id?"
    // gate, mirroring the rejection shape of action_status.
    let path = companion_path(&root, execution_id);
    let file = match read_log_file(&path) {
        Ok(f) => f,
        Err(e) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("companion log {} not readable: {}", path.display(), e),
                )
                .with_suggestion("confirm execution_id matches a previously opened companion log"),
            ));
        }
    };

    // Resolve which claim scope(s) we audit against. Default = union of
    // all claim scopes; explicit `claim_id` narrows to a single scope so
    // the writer can preflight against the exact claim it just acquired.
    let claim_scopes = if let Some(cid) = args
        .get("claim_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        match collect_specific_claim_scope(&file, cid) {
            Ok(scopes) => scopes,
            Err(err) => return Ok(err),
        }
    } else {
        collect_all_claim_scopes(&file)
    };

    // Read-only git status under the inspect_dir. The only mutating
    // codepath in this whole crate is `arch_maintenance_worker`, which
    // lives behind a feature flag the writer agent never reaches; this
    // action stays strictly to `git status --porcelain=v1`.
    let raw_status = match run_git_status(&inspect_dir) {
        Ok(s) => s,
        Err(err) => return Ok(err),
    };
    let entries = parse_porcelain_status(&raw_status);

    let mut summary = build_preflight_summary(&entries, &claim_scopes, expected_files.as_deref());

    // Echo the inputs so the writer agent can correlate the response
    // with the exact dispatch envelope it sent us. `cwd` is the
    // canonicalized form so any symlink / `..` resolution is visible.
    summary["execution_id"] = json!(execution_id);
    summary["cwd"] = json!(inspect_dir.to_string_lossy());
    summary["project_root"] = json!(root.to_string_lossy());
    if let Some(cid) = args.get("claim_id").and_then(|v| v.as_str()) {
        summary["claim_id"] = json!(cid);
    }
    // wave-20 / task 03 — when the caller threads `task_contract_path`
    // through preflight, daemon now loads it (read-only) and projects
    // staged/changed files against the contract's `:write-scope` +
    // `:must-not-touch` so the writer sees scope drift BEFORE running
    // `git commit`. Daemon never mutates the worktree here — load failures
    // surface as `task_contract_status="missing"` / `"malformed"` so the
    // writer can fix the path / file content without preflight hard-
    // rejecting (the post-commit gate at `action=complete` is the
    // authoritative enforcement).
    if let Some(tcp) = args
        .get("task_contract_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        summary["task_contract_path"] = json!(tcp);
        let staged: Vec<String> = summary
            .get("staged_files")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let changed: Vec<String> = summary
            .get("changed_files")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let (status, scope_summary, resolved_path, failure) =
            evaluate_task_contract_for_preflight(&root, tcp, &staged, &changed);
        summary["task_contract_status"] = json!(status);
        if let Some(rp) = resolved_path {
            summary["task_contract_resolved_path"] = json!(rp);
        }
        if let Some(scope) = scope_summary {
            // Promote the four contract-derived fields to the top level so
            // dashboards keying off `task_contract_status` can read the
            // drift signals without descending one more level. The full
            // projection (including write_scope / must_not_touch echo)
            // stays under `task_contract_scope` for inspectors that want
            // the raw inputs.
            for key in [
                "staged_out_of_scope",
                "staged_forbidden",
                "unstaged_in_scope",
            ] {
                if let Some(v) = scope.get(key) {
                    summary[key] = v.clone();
                }
            }
            // Override `next_step` with the contract-aware hint when the
            // contract added forbidden / out-of-scope drift the claim-only
            // check missed (forbidden patterns aren't a claim concept).
            // Otherwise prefer the existing claim-derived next_step.
            let has_contract_drift = scope
                .get("staged_forbidden")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false)
                || scope
                    .get("staged_out_of_scope")
                    .and_then(|v| v.as_array())
                    .map(|a| !a.is_empty())
                    .unwrap_or(false);
            if has_contract_drift {
                if let Some(ns) = scope.get("next_step") {
                    summary["next_step"] = ns.clone();
                }
                // Flip `ok=false` because contract-level drift is at least
                // as serious as claim-level drift; downstream consumers
                // already key off `ok` for go/no-go decisions.
                summary["ok"] = json!(false);
            }
            summary["task_contract_scope"] = scope;
        } else if let Some(msg) = failure {
            summary["task_contract_error"] = json!(msg);
        }
    }

    // wave-21 / task 03 — echo the task-run verifier hint paths when
    // the caller threads them through preflight. These are advisory
    // only (the daemon does not load the report at preflight time;
    // the wave21-03 verified-gate at `action=complete` is the
    // authoritative cross-check). Surfacing them here lets the writer
    // confirm the dispatch envelope matches what the script-side
    // verifier (`scripts/verify-task-run.mjs`) will load post-commit.
    if let Some(trp) = args
        .get("task_report_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        summary["task_report_path"] = json!(trp);
    }
    if let Some(smp) = args
        .get("shared_memory_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        summary["shared_memory_path"] = json!(smp);
    }

    // wave23-04 — opt-in session-trace append. Preflight is informational
    // (no commit happens here) so we record it as `observation` carrying
    // the staged + ok flag in the summary text. Best-effort: failures
    // surface as `trace_warning` without flipping the preflight verdict.
    if let Some(trace_path) = resolve_session_trace_path(args, &root) {
        match resolve_trace_task_id(args, &root, execution_id) {
            Some(task_id) => {
                let ok_flag = summary.get("ok").and_then(|v| v.as_bool()).unwrap_or(true);
                let staged_count = summary
                    .get("staged_files")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let changed_count = summary
                    .get("changed_files")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let ev = TraceEvent {
                    task: task_id,
                    backend: "claudecode".to_string(),
                    kind: TraceKind::Observation,
                    summary: format!(
                        "mission_execution(action=preflight_commit) execution_id={} ok={} staged={} changed={}",
                        execution_id, ok_flag, staged_count, changed_count
                    ),
                    agent: None,
                    files: None,
                    commit_hash: None,
                    report_path: None,
                };
                if let Err(w) = append_session_trace_event(&trace_path, &ev) {
                    summary["trace_warning"] = json!(w.to_string());
                }
            }
            None => {
                summary["trace_warning"] = json!(format!(
                    "session_trace_path supplied but execution_id `{}` is not a valid trace task id and no task_contract_path was provided",
                    execution_id
                ));
            }
        }
    }

    Ok(ToolResult::json_pretty(&summary))
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
