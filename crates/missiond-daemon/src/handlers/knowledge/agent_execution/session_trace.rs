use serde_json::Value;
use std::path::{Path, PathBuf};

use super::log_store::{lisp_quote_string, now_iso, sexp};
use super::task_verifier::read_task_contract_id;

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

const TRACE_ID_RE: &str = r"^[a-z0-9][a-z0-9._-]*$";

/// Trace event kinds the daemon emits. Mirrors the `event-kinds` enum in
/// `.missiond/tasks/schema/session-trace-v1.lisp` and the JS-side
/// `KIND_VALUES` set in `scripts/check-session-trace.mjs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TraceKind {
    Dispatch,
    Observation,
    Complete,
    Failure,
}

impl TraceKind {
    fn as_str(self) -> &'static str {
        match self {
            TraceKind::Dispatch => "dispatch",
            TraceKind::Observation => "observation",
            TraceKind::Complete => "complete",
            TraceKind::Failure => "failure",
        }
    }
}

/// Structured event the daemon constructs before formatting it as a
/// `(trace-event ...)` Lisp form. Required fields stay non-optional so the
/// type system enforces the schema's required set; optional fields are
/// `Option<String>` and skip-emit when `None`.
#[derive(Debug, Clone)]
pub(super) struct TraceEvent {
    pub(super) task: String,
    pub(super) backend: String,
    pub(super) kind: TraceKind,
    pub(super) summary: String,
    pub(super) agent: Option<String>,
    pub(super) files: Option<Vec<String>>,
    pub(super) commit_hash: Option<String>,
    pub(super) report_path: Option<String>,
}

/// Why a trace append could not happen. Surfaced verbatim on the action
/// response as `trace_warning` so the writer agent can correlate the
/// failure with its dispatch envelope. `Display` produces the user-facing
/// string; the variant is preserved internally for tests / future
/// structured logging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TraceWarning {
    MissingFile(String),
    Io(String),
    Malformed(String),
    InvalidTaskId(String),
    InvalidBackend(String),
}

impl std::fmt::Display for TraceWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TraceWarning::MissingFile(p) => {
                write!(f, "session_trace_path `{}` does not exist", p)
            }
            TraceWarning::Io(msg) => write!(f, "session_trace_path I/O error: {}", msg),
            TraceWarning::Malformed(msg) => {
                write!(f, "session_trace_path malformed: {}", msg)
            }
            TraceWarning::InvalidTaskId(s) => write!(
                f,
                "session_trace task id `{}` does not match {}; cannot append",
                s, TRACE_ID_RE
            ),
            TraceWarning::InvalidBackend(s) => write!(
                f,
                "session_trace backend `{}` does not match {}; cannot append",
                s, TRACE_ID_RE
            ),
        }
    }
}

/// `^[a-z0-9][a-z0-9._-]*$` — same as the JS-side `ID_RE` so an event the
/// daemon emits round-trips through `scripts/check-session-trace.mjs`
/// without diagnostics.
pub(super) fn is_valid_trace_id(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' || c == '-')
}

/// Slugify a free-form backend / agent name into something matching
/// `TRACE_ID_RE`. Falls back to `"claudecode"` when the input has no usable
/// characters — the daemon is the executor by default.
pub(super) fn sanitize_trace_backend(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "claudecode".to_string();
    }
    let mut out = String::with_capacity(trimmed.len());
    for c in trimmed.chars() {
        if c.is_ascii_uppercase() {
            out.push(c.to_ascii_lowercase());
        } else if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' || c == '-' {
            out.push(c);
        } else if c == ' ' || c == '/' || c == ':' {
            out.push('-');
        }
        // other characters dropped silently
    }
    // First char must be alnum
    while let Some(first) = out.chars().next() {
        if first.is_ascii_lowercase() || first.is_ascii_digit() {
            break;
        }
        out.remove(0);
    }
    if out.is_empty() {
        "claudecode".to_string()
    } else {
        out
    }
}

/// Render a `(trace-event ...)` form body. Uses bare atoms for `:id` /
/// `:task` / `:backend` / `:kind` / `:seq` (matches the seed file shape +
/// the JS checker's `nodeText` semantics) and quoted strings for free-form
/// fields like `:summary`. `:files` / `:report_path` / `:commit_hash` skip
/// when absent.
pub(super) fn render_trace_event(seq: u64, at: &str, ev: &TraceEvent) -> String {
    let id = format!("{}-{}-{}", ev.task, ev.kind.as_str(), seq);
    let mut out = String::new();
    out.push_str("\n  (trace-event\n");
    out.push_str(&format!("    :id {}\n", id));
    out.push_str(&format!("    :seq {}\n", seq));
    out.push_str(&format!("    :at {}\n", lisp_quote_string(at)));
    out.push_str(&format!("    :task {}\n", ev.task));
    out.push_str(&format!("    :backend {}\n", ev.backend));
    out.push_str(&format!("    :kind {}\n", ev.kind.as_str()));
    out.push_str(&format!("    :summary {}", lisp_quote_string(&ev.summary)));
    if let Some(ref agent) = ev.agent {
        out.push_str(&format!("\n    :agent {}", agent));
    }
    if let Some(ref files) = ev.files {
        let rendered = files
            .iter()
            .map(|p| lisp_quote_string(p))
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&format!("\n    :files [{}]", rendered));
    }
    if let Some(ref hash) = ev.commit_hash {
        out.push_str(&format!("\n    :commit_hash {}", lisp_quote_string(hash)));
    }
    if let Some(ref rp) = ev.report_path {
        out.push_str(&format!("\n    :report_path {}", lisp_quote_string(rp)));
    }
    out.push(')');
    out
}

/// Scan the parsed trace forms for the maximum `:seq` across every
/// `(trace-event ...)` child of the `(session-trace ...)` root. Returns 0
/// when the file has no events yet — the first append picks `seq=1`.
pub(super) fn scan_max_trace_seq(forms: &[sexp::Node]) -> u64 {
    let Some(trace_form) = forms
        .iter()
        .find(|n| n.head_atom() == Some("session-trace"))
    else {
        return 0;
    };
    let mut max = 0u64;
    for child in trace_form.children() {
        if child.head_atom() != Some("trace-event") {
            continue;
        }
        // `:seq <int>` — find the keyword and read the next sibling atom.
        let kids = child.children();
        let mut i = 0;
        while i + 1 < kids.len() {
            if let Some(atom) = kids[i].as_atom() {
                if atom == ":seq" {
                    if let Some(val) = kids[i + 1].as_atom() {
                        if let Ok(n) = val.parse::<u64>() {
                            if n > max {
                                max = n;
                            }
                        }
                    }
                }
            }
            i += 1;
        }
    }
    max
}

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
