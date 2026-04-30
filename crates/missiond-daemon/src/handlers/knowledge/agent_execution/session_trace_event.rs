use super::lisp_syntax as sexp;
use super::log_store::lisp_quote_string;

pub(super) const TRACE_ID_RE: &str = r"^[a-z0-9][a-z0-9._-]*$";

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
/// `(trace-event ...)` Lisp form.
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
/// response as `trace_warning` so the writer agent can correlate the failure
/// with its dispatch envelope.
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
/// daemon emits round-trips through `scripts/check-session-trace.mjs`.
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
/// characters.
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
    }
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

/// Render a `(trace-event ...)` form body. Uses bare atoms for schema ids and
/// quoted strings for free-form fields.
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
/// `(trace-event ...)` child of the `(session-trace ...)` root.
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
