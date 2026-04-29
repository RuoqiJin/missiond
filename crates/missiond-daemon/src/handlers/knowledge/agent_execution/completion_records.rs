use serde_json::{json, Value};
use std::collections::HashMap;

use super::log_store::{lisp_quote_string, parse_kv_pairs, sexp, LogFile};

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
