use serde_json::Value;

use super::lisp_syntax as sexp;
use super::log_store::lisp_quote_string;

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
/// not supplied" from "field was supplied as empty list" - both shapes are
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
/// caller deliberately recorded "no files" - distinct from the field being
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
/// Returns `None` if the slice does not parse as a list - caller decides
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
