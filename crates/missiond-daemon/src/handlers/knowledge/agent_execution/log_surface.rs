use crate::state::AppState;
use anyhow::{anyhow, Result};
use missiond_core::event::events::ExecutionEvent;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tracing::warn;

use super::completion_audit::read_task_contract_id;
use super::{lisp_quote_string, now_iso, parse_kv_pairs};

pub(super) mod sexp {
    use anyhow::{anyhow, Result};

    #[derive(Debug, Clone)]
    pub struct Node {
        pub kind: NodeKind,
        pub start: usize,
        pub end: usize,
    }

    #[derive(Debug, Clone)]
    pub enum NodeKind {
        List(Vec<Node>),
        Bracket(Vec<Node>),
        Str(String),
        Atom(String),
    }

    impl Node {
        pub fn head_atom(&self) -> Option<&str> {
            match &self.kind {
                NodeKind::List(children) | NodeKind::Bracket(children) => match children.first() {
                    Some(n) => match &n.kind {
                        NodeKind::Atom(s) => Some(s.as_str()),
                        _ => None,
                    },
                    None => None,
                },
                _ => None,
            }
        }

        pub fn children(&self) -> &[Node] {
            match &self.kind {
                NodeKind::List(c) | NodeKind::Bracket(c) => c.as_slice(),
                _ => &[],
            }
        }

        pub fn as_atom(&self) -> Option<&str> {
            match &self.kind {
                NodeKind::Atom(s) => Some(s.as_str()),
                _ => None,
            }
        }

        /// Render this node's literal source slice from the original text.
        pub fn slice<'a>(&self, src: &'a str) -> &'a str {
            &src[self.start..self.end]
        }
    }

    pub fn parse(src: &str) -> Result<Vec<Node>> {
        let mut p = Parser {
            src: src.as_bytes(),
            i: 0,
        };
        let mut out = Vec::new();
        loop {
            p.skip_ws_and_comments();
            if p.i >= p.src.len() {
                break;
            }
            out.push(p.read_form()?);
        }
        Ok(out)
    }

    struct Parser<'a> {
        src: &'a [u8],
        i: usize,
    }

    impl<'a> Parser<'a> {
        fn read_form(&mut self) -> Result<Node> {
            self.skip_ws_and_comments();
            if self.i >= self.src.len() {
                return Err(anyhow!("unexpected EOF"));
            }
            let c = self.src[self.i];
            match c {
                b'(' => self.read_list(b')'),
                b'[' => self.read_list(b']'),
                b'"' => self.read_string(),
                b')' | b']' => Err(anyhow!(
                    "unexpected closing delimiter '{}' at byte {}",
                    c as char,
                    self.i
                )),
                _ => self.read_atom(),
            }
        }

        fn read_list(&mut self, close: u8) -> Result<Node> {
            let start = self.i;
            self.i += 1;
            let mut children = Vec::new();
            loop {
                self.skip_ws_and_comments();
                if self.i >= self.src.len() {
                    return Err(anyhow!(
                        "unterminated list opened at byte {} (expected '{}')",
                        start,
                        close as char
                    ));
                }
                let c = self.src[self.i];
                if c == close {
                    self.i += 1;
                    let end = self.i;
                    let kind = if close == b')' {
                        NodeKind::List(children)
                    } else {
                        NodeKind::Bracket(children)
                    };
                    return Ok(Node { kind, start, end });
                }
                if c == b')' || c == b']' {
                    return Err(anyhow!(
                        "mismatched closing delimiter '{}' at byte {} (expected '{}')",
                        c as char,
                        self.i,
                        close as char
                    ));
                }
                children.push(self.read_form()?);
            }
        }

        fn read_string(&mut self) -> Result<Node> {
            let start = self.i;
            self.i += 1;
            let mut out = String::new();
            while self.i < self.src.len() {
                let c = self.src[self.i];
                if c == b'"' {
                    self.i += 1;
                    return Ok(Node {
                        kind: NodeKind::Str(out),
                        start,
                        end: self.i,
                    });
                }
                if c == b'\\' {
                    if self.i + 1 >= self.src.len() {
                        return Err(anyhow!("unterminated escape in string at byte {}", start));
                    }
                    let next = self.src[self.i + 1];
                    let mapped = match next {
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'\\' => '\\',
                        b'"' => '"',
                        other => other as char,
                    };
                    out.push(mapped);
                    self.i += 2;
                    continue;
                }
                out.push(c as char);
                self.i += 1;
            }
            Err(anyhow!("unterminated string starting at byte {}", start))
        }

        fn read_atom(&mut self) -> Result<Node> {
            let start = self.i;
            while self.i < self.src.len() {
                let c = self.src[self.i];
                if c.is_ascii_whitespace()
                    || c == b'('
                    || c == b')'
                    || c == b'['
                    || c == b']'
                    || c == b'"'
                    || c == b';'
                {
                    break;
                }
                self.i += 1;
            }
            if start == self.i {
                return Err(anyhow!("empty atom at byte {}", start));
            }
            let text = std::str::from_utf8(&self.src[start..self.i])
                .map_err(|e| anyhow!("non-utf8 atom at byte {}: {}", start, e))?
                .to_string();
            Ok(Node {
                kind: NodeKind::Atom(text),
                start,
                end: self.i,
            })
        }

        fn skip_ws_and_comments(&mut self) {
            loop {
                while self.i < self.src.len() && self.src[self.i].is_ascii_whitespace() {
                    self.i += 1;
                }
                if self.i < self.src.len() && self.src[self.i] == b';' {
                    while self.i < self.src.len() && self.src[self.i] != b'\n' {
                        self.i += 1;
                    }
                } else {
                    break;
                }
            }
        }
    }

    /// Verify the source has balanced delimiters and no unterminated string.
    /// Returns Ok(()) on success or the byte offset of the first error.
    pub fn check_balance(src: &str) -> Result<()> {
        let mut stack: Vec<(u8, usize)> = Vec::new();
        let bytes = src.as_bytes();
        let mut i = 0;
        let mut in_str = false;
        let mut esc = false;
        let mut comment = false;
        while i < bytes.len() {
            let c = bytes[i];
            if comment {
                if c == b'\n' {
                    comment = false;
                }
            } else if in_str {
                if esc {
                    esc = false;
                } else if c == b'\\' {
                    esc = true;
                } else if c == b'"' {
                    in_str = false;
                }
            } else {
                match c {
                    b';' => comment = true,
                    b'"' => in_str = true,
                    b'(' | b'[' => stack.push((c, i)),
                    b')' | b']' => {
                        let want = if c == b')' { b'(' } else { b'[' };
                        match stack.pop() {
                            Some((open, _)) if open == want => {}
                            Some((open, pos)) => {
                                return Err(anyhow!(
                                "mismatched delimiter at byte {}: '{}' closes '{}' opened at {}",
                                i,
                                c as char,
                                open as char,
                                pos
                            ))
                            }
                            None => {
                                return Err(anyhow!(
                                    "stray closing delimiter '{}' at byte {}",
                                    c as char,
                                    i
                                ))
                            }
                        }
                    }
                    _ => {}
                }
            }
            i += 1;
        }
        if in_str {
            return Err(anyhow!("unterminated string"));
        }
        if let Some((open, pos)) = stack.last() {
            return Err(anyhow!(
                "unterminated '{}' opened at byte {}",
                *open as char,
                pos
            ));
        }
        Ok(())
    }
}

use self::sexp::Node;

// ───────────────────────────────────────────────────────────────────────
// execution-log accessor — sits on top of the parsed tree
// ───────────────────────────────────────────────────────────────────────

/// View over a parsed execution-log file. Holds the source so byte spans stay
/// valid; keeps a flat list of top-level forms and the index of the
/// `execution-log` (or legacy `execution`) form.
pub(super) struct LogFile {
    pub(super) src: String,
    pub(super) forms: Vec<Node>,
    pub(super) root_idx: usize,
}

impl LogFile {
    pub(super) fn parse(text: String) -> Result<Self> {
        let forms = sexp::parse(&text)?;
        let root_idx = forms
            .iter()
            .position(|n| matches!(n.head_atom(), Some("execution-log") | Some("execution")))
            .ok_or_else(|| {
                anyhow!("no (execution-log ...) or (execution ...) top-level form in companion log")
            })?;
        Ok(Self {
            src: text,
            forms,
            root_idx,
        })
    }

    pub(super) fn root(&self) -> &Node {
        &self.forms[self.root_idx]
    }

    pub(super) fn root_children(&self) -> &[Node] {
        // Skip the head atom (e.g. `execution-log`).
        let kids = self.root().children();
        if kids.is_empty() {
            kids
        } else {
            &kids[1..]
        }
    }

    pub(super) fn find_block(&self, name: &str) -> Option<&Node> {
        self.root_children()
            .iter()
            .find(|n| n.head_atom() == Some(name))
    }
}

/// Canonical workstation-dispatch strategies surfaced by intent-tools.lisp ::
/// implemented-surface mission_execution :: :workstation-dispatch-record. Kept
/// in sync with `plan.rs::VALID_DISPATCH_STRATEGIES`; unknown / empty inputs
/// normalize to `DEFAULT_DISPATCH_STRATEGY` so legacy callers keep working.
const VALID_DISPATCH_STRATEGIES: &[&str] = &[
    "resident-lisp",
    "fresh-code-alignment",
    "agent-team",
    "mixed",
    "prompt-fallback",
    "unknown",
];
pub(super) const DEFAULT_DISPATCH_STRATEGY: &str = "unknown";

/// Normalize an optional dispatch strategy string against the canonical set.
/// Unknown / empty values fall back to `DEFAULT_DISPATCH_STRATEGY` (`"unknown"`)
/// without erroring; we never hard-fail open() on a strategy mismatch because
/// upstream dispatchers may legitimately surface novel labels we then audit.
pub(super) fn normalize_dispatch_strategy(raw: Option<&str>) -> &'static str {
    let v = raw.unwrap_or("").trim();
    if v.is_empty() {
        return DEFAULT_DISPATCH_STRATEGY;
    }
    for &known in VALID_DISPATCH_STRATEGIES {
        if known == v {
            return known;
        }
    }
    DEFAULT_DISPATCH_STRATEGY
}

/// Forward an `ExecutionEvent` to the v2 bus and log (but never propagate)
/// publish failures. Companion-log writes are already durable on disk; the
/// bus event is a live projection.
pub(super) async fn emit_execution_event(state: &AppState, ev: ExecutionEvent) {
    if let Err(e) = state.bus.publish_execution(ev).await {
        warn!(error = %e, "failed to publish ExecutionEvent (companion log already durable)");
    }
}

/// Build an `ExecutionEvent::Opened` payload from the inputs `action_open`
/// has already validated and normalized. Centralizing the construction
/// keeps the dispatch-metadata mapping (intent-worker.lisp ::
/// claudecode-workstation-orchestration :: execution-strategy-record)
/// in one testable place — the runtime caller and the unit tests stay in
/// lock-step on which open args land in which event slot.
///
/// `dispatch_strategy` always resolves to a canonical string via
/// `normalize_dispatch_strategy`. We surface it on the event verbatim so
/// downstream auditors observe the same label that lives in the companion
/// log meta block. `target_project` / `requested_cwd` are forwarded only
/// when the open args carry them — `Option::is_none` skip-serialize keeps
/// the wire form byte-identical to the legacy 5-field shape otherwise.
pub(super) fn build_opened_event(
    execution_id: &str,
    parent_design: &str,
    scope: &str,
    owner: &str,
    path: String,
    dispatch_strategy: &str,
    target_project: Option<&str>,
    requested_cwd: Option<&str>,
) -> ExecutionEvent {
    ExecutionEvent::Opened {
        execution_id: execution_id.to_string(),
        parent_design: parent_design.to_string(),
        scope: scope.to_string(),
        owner: owner.to_string(),
        path,
        dispatch_strategy: Some(dispatch_strategy.to_string()),
        target_project: target_project.map(|s| s.to_string()),
        requested_cwd: requested_cwd.map(|s| s.to_string()),
    }
}

/// Single tuple of the workstation-dispatch trio surfaced on every
/// `ExecutionEvent` variant that carries dispatch context. Sourced from the
/// companion-log meta block so consumers don't have to re-load the file to
/// correlate the event against its dispatch strategy / target project /
/// requested cwd. All three fields are `None` when the meta block omits the
/// corresponding `:key`, which lets the legacy companion logs (pre-wave12-01)
/// emit cleanly with the default skip-serialize wire form.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct DispatchMeta {
    pub(super) dispatch_strategy: Option<String>,
    pub(super) target_project: Option<String>,
    pub(super) requested_cwd: Option<String>,
}

/// Read the workstation-dispatch trio (`:dispatch-strategy` /
/// `:target-project` / `:requested-cwd`) from the companion-log meta block.
///
/// Mirrors the parsing path used by `action_list` so the live event stream
/// and the dashboard list view see identical strings. Quoted-string atoms
/// have their outer quotes stripped via `trim_matches('"')` to match the
/// downstream contract; whitespace-only values collapse to `None` so a
/// caller that wrote `:target-project ""` doesn't surface a confusing empty
/// label on the bus.
///
/// Returns `DispatchMeta::default()` when the file has no meta block — the
/// caller emits the event without metadata in that case, matching what
/// legacy producers serialized before the trio was added.
pub(super) fn read_dispatch_metadata_from_log(file: &LogFile) -> DispatchMeta {
    let Some(block) = file.find_block("meta") else {
        return DispatchMeta::default();
    };
    let meta = parse_kv_pairs(&file.src, block.children());
    let read = |key: &str| -> Option<String> {
        meta.get(key)
            .map(|s| s.trim().trim_matches('"').to_string())
            .filter(|s| !s.is_empty())
    };
    DispatchMeta {
        dispatch_strategy: read("dispatch-strategy"),
        target_project: read("target-project"),
        requested_cwd: read("requested-cwd"),
    }
}

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
