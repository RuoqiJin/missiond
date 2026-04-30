use serde_json::Value;
use std::path::{Path, PathBuf};

use super::lisp_syntax as sexp;
use super::log_store::now_iso;
pub(super) use super::session_trace_event::{
    is_valid_trace_id, render_trace_event, sanitize_trace_backend, scan_max_trace_seq, TraceEvent,
    TraceKind, TraceWarning,
};
use super::task_verifier_inputs::read_task_contract_id;

// ───────────────────────────────────────────────────────────────────────
// wave23-04: session-trace append (opt-in, best-effort).
//
// `mission_execution` callers can opt into structured factual telemetry by
// supplying `session_trace_path`. When present, the daemon appends a
// `(trace-event ...)` form to the named file directly via Rust I/O — no
// Node spawn, no shell. Failures surface as a `trace_warning` field on the
// action's response; the primary action result is never hidden behind a
// trace error (per task contract requirement 3).
//
// Output passes `scripts/check-session-trace.mjs` validation:
//   * required fields :id :seq :at :task :backend :kind :summary
//   * :seq strictly monotonic (read existing max, append max+1)
//   * :id stable + unique within file (`<task>-<kind>-<seq>`)
//   * timestamps are ISO-8601 with timezone (now_iso() emits `Z`)
//   * :task / :backend match `^[a-z0-9][a-z0-9._-]*$`
//   * optional :files / :report_path / :command paths repo-relative
// ───────────────────────────────────────────────────────────────────────

/// Append a single trace event to `path`. Best-effort: any failure
/// returns `Err(TraceWarning)` and the caller MUST surface the warning
/// without aborting the primary action result.
pub(super) fn append_session_trace_event(
    path: &Path,
    ev: &TraceEvent,
) -> std::result::Result<(), TraceWarning> {
    if !is_valid_trace_id(&ev.task) {
        return Err(TraceWarning::InvalidTaskId(ev.task.clone()));
    }
    if !is_valid_trace_id(&ev.backend) {
        return Err(TraceWarning::InvalidBackend(ev.backend.clone()));
    }
    if !path.exists() {
        return Err(TraceWarning::MissingFile(path.display().to_string()));
    }
    let src = std::fs::read_to_string(path).map_err(|e| TraceWarning::Io(e.to_string()))?;
    let forms = sexp::parse(&src).map_err(|e| TraceWarning::Malformed(e.to_string()))?;
    let trace_form = forms
        .iter()
        .find(|n| n.head_atom() == Some("session-trace"))
        .ok_or_else(|| {
            TraceWarning::Malformed("no (session-trace ...) top-level form".to_string())
        })?;
    let seq = scan_max_trace_seq(&forms) + 1;
    let at = now_iso();
    let entry = render_trace_event(seq, &at, ev);
    // The closing `)` of the (session-trace ...) form sits at byte
    // `trace_form.end - 1`. We splice the new entry in just before it so
    // the file remains a single well-formed top-level form.
    let close_byte = trace_form
        .end
        .checked_sub(1)
        .ok_or_else(|| TraceWarning::Malformed("session-trace form has zero length".to_string()))?;
    if close_byte > src.len() {
        return Err(TraceWarning::Malformed(
            "session-trace form end byte out of range".to_string(),
        ));
    }
    let (head, tail) = src.split_at(close_byte);
    let mut new_body = String::with_capacity(src.len() + entry.len() + 1);
    new_body.push_str(head);
    // Trim trailing whitespace before the close so the appended entry sits
    // at a consistent indent (one entry per line block, mirrors the seed
    // file shape).
    let trimmed = new_body
        .trim_end_matches(|c: char| c == ' ' || c == '\t')
        .to_string();
    new_body = trimmed;
    new_body.push_str(&entry);
    new_body.push('\n');
    new_body.push_str(tail);
    // Validate balance + reparse before writing — we never want to leave a
    // trace file in a broken state.
    sexp::check_balance(&new_body)
        .map_err(|e| TraceWarning::Malformed(format!("appended event broke balance: {}", e)))?;
    std::fs::write(path, new_body.as_bytes()).map_err(|e| TraceWarning::Io(e.to_string()))?;
    Ok(())
}

/// Resolve the optional `session_trace_path` argument to an absolute path
/// under the project root. Returns `None` when the argument is absent or
/// blank — the caller treats that as "trace integration disabled".
pub(super) fn resolve_session_trace_path(args: &Value, root: &Path) -> Option<PathBuf> {
    let raw = args
        .get("session_trace_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())?;
    let candidate = PathBuf::from(raw);
    Some(if candidate.is_absolute() {
        candidate
    } else {
        root.join(candidate)
    })
}

/// Prefer the task-contract id parsed from `task_contract_path` when the
/// caller threads it through; otherwise fall back to `execution_id` if it
/// matches the trace id regex. Returns `None` when neither yields a valid
/// id — the caller surfaces the warning.
pub(super) fn resolve_trace_task_id(args: &Value, root: &Path, fallback: &str) -> Option<String> {
    if let Some(tcp) = args
        .get("task_contract_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        let abs = if Path::new(tcp).is_absolute() {
            PathBuf::from(tcp)
        } else {
            root.join(tcp)
        };
        if let Ok(text) = std::fs::read_to_string(&abs) {
            if let Some(id) = read_task_contract_id(&text) {
                if is_valid_trace_id(&id) {
                    return Some(id);
                }
            }
        }
    }
    if is_valid_trace_id(fallback) {
        Some(fallback.to_string())
    } else {
        None
    }
}
