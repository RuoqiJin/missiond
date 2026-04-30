use serde_json::{json, Value};
use std::collections::HashMap;

use super::completion_fields::{parse_string_list, VALID_COMMIT_STATUSES};
use super::log_store::{parse_kv_pairs, LogFile};

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
