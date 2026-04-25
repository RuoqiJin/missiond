//! mission_execution — manager for the agent-execution-coordination protocol.
//!
//! Lisp authority:
//!   - intent-memory.lisp :: helper agent-execution-coordination v0.5.x (protocol)
//!   - intent-worker.lisp :: agent-execution-manager-interface (runtime mechanics)
//!   - intent-tools.lisp  :: future-surface mission_execution (MCP schema)
//!   - intent-flow.lisp   :: F-execution-log-governance (cross-pillar choreography)
//!
//! Companion logs live at `<project_root>/.missiond/v2/<execution_id>.lisp`.
//! This handler owns id-counters / claims-with-lease / deviations / decisions /
//! issues / completions / derived-indexes per the helper-recursive-contract.
//!
//! ExecutionEvent emission: each mutating action emits the matching variant
//! to the v2 event bus AFTER the durable companion log write succeeds. The
//! file remains the source of truth (per `planned-event-extensions ::
//! ExecutionEvent :: rationale`); the bus event is a non-authoritative live
//! projection for status dashboards and audit consumers. Publish failures
//! are logged but never abort the action — observability must never break
//! durable-write semantics.

use anyhow::{anyhow, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::warn;

use crate::state::AppState;

/// Forward an `ExecutionEvent` to the v2 bus and log (but never propagate)
/// publish failures. Companion-log writes are already durable on disk; the
/// bus event is a live projection.
async fn emit_execution_event(state: &AppState, ev: ExecutionEvent) {
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
fn build_opened_event(
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

const COMPANION_DIR: &str = ".missiond/v2";
const DEFAULT_LEASE_SECS: i64 = 1800;
const MAX_LEASE_SECS: i64 = 24 * 3600;

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
const DEFAULT_DISPATCH_STRATEGY: &str = "unknown";

/// Canonical scoped-commit handoff statuses surfaced by intent-memory.lisp ::
/// helper agent-execution-coordination :: shared-memory-slots :: completions
/// :commit-status-values "[not-required pending committed blocked skipped]".
/// Used both to validate `mission_execution(action=complete, commit_status=…)`
/// arguments and to drive the audit checks for the durability plane.
const VALID_COMMIT_STATUSES: &[&str] = &[
    "not-required",
    "pending",
    "committed",
    "blocked",
    "skipped",
];

/// Audit finding kinds emitted by the scoped-commit handoff checks. Kept as
/// `&'static str` constants so test assertions can pin the exact wire form
/// without spelling them out repeatedly. Names mirror the scoped-commit
/// contract terminology (intent-memory.lisp :: scoped-commit-contract +
/// intent-flow.lisp :: F-scoped-commit-handoff :: failure-modes).
const FINDING_COMMIT_STATUS_NO_HASH: &str = "commit-status-without-hash";
const FINDING_COMMIT_BLOCKED_NO_BLOCKER: &str = "commit-status-blocked-without-blocker";
const FINDING_SCOPED_COMMIT_VIOLATION: &str = "scoped-commit-violation";

/// Normalize an optional dispatch strategy string against the canonical set.
/// Unknown / empty values fall back to `DEFAULT_DISPATCH_STRATEGY` (`"unknown"`)
/// without erroring; we never hard-fail open() on a strategy mismatch because
/// upstream dispatchers may legitimately surface novel labels we then audit.
fn normalize_dispatch_strategy(raw: Option<&str>) -> &'static str {
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

/// Return the canonical form of a `commit_status` value if recognised.
/// Unlike `normalize_dispatch_strategy`, an unknown value yields `None` so
/// the caller can hard-fail with a structured `INVALID_PARAM`. Per
/// intent-memory.lisp :: completions :commit-status-values these are the
/// only legal labels; we refuse to silently coerce typos because audit
/// invariants downstream key off the exact string.
fn normalize_commit_status(raw: &str) -> Option<&'static str> {
    let v = raw.trim();
    if v.is_empty() {
        return None;
    }
    for &known in VALID_COMMIT_STATUSES {
        if known == v {
            return Some(known);
        }
    }
    None
}

/// Pull a `[string]` argument off `args[key]` and return it as a `Vec<String>`.
/// Returns `None` if the key is absent so callers can distinguish "field was
/// not supplied" from "field was supplied as empty list" — both shapes are
/// legal: a writer that ran no commit may legitimately report
/// `staged_files=[]` to record "nothing staged".
fn collect_string_list(args: &Value, key: &str) -> Option<Vec<String>> {
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
fn render_string_list(items: &[String]) -> String {
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
fn parse_string_list(slice: &str) -> Option<Vec<String>> {
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

pub(crate) async fn handle(state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a.to_string(),
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "mission_execution requires `action`",
                )
                .with_suggestion(
                    "actions: open|list|claim|heartbeat|release|deviate|decide|issue|complete|status|audit|repair",
                ),
            ))
        }
    };

    match action.as_str() {
        "open" => action_open(state, &args).await,
        "list" => action_list(state, &args).await,
        "claim" => action_claim(state, &args).await,
        "heartbeat" => action_heartbeat(state, &args).await,
        "release" => action_release(state, &args).await,
        "deviate" => action_deviate(state, &args).await,
        "decide" => action_decide(state, &args).await,
        "issue" => action_issue(state, &args).await,
        "complete" => action_complete(state, &args).await,
        "status" => action_status(state, &args).await,
        "audit" => action_audit(state, &args).await,
        "repair" => action_repair(state, &args).await,
        other => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("unknown mission_execution action `{}`", other),
            )
            .with_suggestion(
                "valid: open|list|claim|heartbeat|release|deviate|decide|issue|complete|status|audit|repair",
            ),
        )),
    }
}

// ───────────────────────────────────────────────────────────────────────
// path resolution
// ───────────────────────────────────────────────────────────────────────

async fn resolve_project_root(state: &AppState, project_id: Option<&str>) -> Result<PathBuf> {
    if let Some(id) = project_id {
        if let Some(p) = state.project_registry.read().await.get(id) {
            return Ok(PathBuf::from(&p.path));
        }
        // Fall through to error below if explicit id given but unknown.
        return Err(anyhow!(
            "project '{}' not registered; run mission_project(action=\"list\") to see available ids",
            id
        ));
    }
    let cwd = std::env::current_dir().map_err(|e| anyhow!("cannot read CWD: {}", e))?;
    Ok(cwd)
}

fn companion_path(root: &Path, execution_id: &str) -> PathBuf {
    let mut p = root.join(COMPANION_DIR);
    let mut name = execution_id.to_string();
    if !name.ends_with(".lisp") {
        name.push_str(".lisp");
    }
    p.push(name);
    p
}

/// Canonical `project` field accessor. Kept (currently only via the alias
/// resolver below) so future callers — or sibling handlers reaching in for the
/// strict canonical field — have one source of truth for the field name.
#[allow(dead_code)]
fn project_arg(args: &Value) -> Option<&str> {
    args.get("project").and_then(|v| v.as_str())
}

/// Resolve the active project id from either the canonical `project` field or
/// the workstation-dispatch alias `target_project`. `project` always wins when
/// both are present so existing callers stay deterministic; the alias is the
/// surface intent-tools.lisp :: implemented-surface mission_execution exposes
/// for `:workstation-dispatch-record`.
fn project_or_target_project(args: &Value) -> Option<&str> {
    args.get("project")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("target_project").and_then(|v| v.as_str()))
}

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolResult> {
    args.get(key).and_then(|v| v.as_str()).ok_or_else(|| {
        ToolResult::structured_error(
            ToolError::new(
                error_codes::MISSING_PARAM,
                format!("missing required param `{}`", key),
            ),
        )
    })
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

// ───────────────────────────────────────────────────────────────────────
// minimal S-expression parser with byte spans
// ───────────────────────────────────────────────────────────────────────

mod sexp {
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
        let mut p = Parser { src: src.as_bytes(), i: 0 };
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

use sexp::{Node, NodeKind};

// ───────────────────────────────────────────────────────────────────────
// execution-log accessor — sits on top of the parsed tree
// ───────────────────────────────────────────────────────────────────────

/// View over a parsed execution-log file. Holds the source so byte spans stay
/// valid; keeps a flat list of top-level forms and the index of the
/// `execution-log` (or legacy `execution`) form.
struct LogFile {
    src: String,
    forms: Vec<Node>,
    root_idx: usize,
}

impl LogFile {
    fn parse(text: String) -> Result<Self> {
        let forms = sexp::parse(&text)?;
        let root_idx = forms
            .iter()
            .position(|n| {
                matches!(
                    n.head_atom(),
                    Some("execution-log") | Some("execution")
                )
            })
            .ok_or_else(|| {
                anyhow!(
                    "no (execution-log ...) or (execution ...) top-level form in companion log"
                )
            })?;
        Ok(Self {
            src: text,
            forms,
            root_idx,
        })
    }

    fn root(&self) -> &Node {
        &self.forms[self.root_idx]
    }

    fn root_children(&self) -> &[Node] {
        // Skip the head atom (e.g. `execution-log`).
        let kids = self.root().children();
        if kids.is_empty() {
            kids
        } else {
            &kids[1..]
        }
    }

    fn find_block(&self, name: &str) -> Option<&Node> {
        self.root_children()
            .iter()
            .find(|n| n.head_atom() == Some(name))
    }
}

/// Parse a `(:key value :key value ...)` style argument tail into a map of
/// keyword (without leading `:`) to the raw source slice covering the value.
fn parse_kv_pairs<'a>(src: &'a str, kids: &[Node]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut i = 0;
    while i < kids.len() {
        let tok = kids[i].as_atom().unwrap_or("");
        if tok.starts_with(':') && i + 1 < kids.len() {
            let key = tok.trim_start_matches(':').to_string();
            let val_node = &kids[i + 1];
            let val = match &val_node.kind {
                NodeKind::Str(s) => s.clone(),
                _ => val_node.slice(src).to_string(),
            };
            out.insert(key, val);
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

// ───────────────────────────────────────────────────────────────────────
// canonical writer — used by `open` and by `repair` to materialize blocks
// ───────────────────────────────────────────────────────────────────────

fn lisp_quote_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn render_canonical_template(
    execution_id: &str,
    parent_design: &str,
    scope: &str,
    owner: &str,
    dispatch_strategy: &str,
    target_project: Option<&str>,
    requested_cwd: Option<&str>,
) -> String {
    let now = now_iso();
    let id_q = lisp_quote_string(execution_id);
    let parent_q = lisp_quote_string(parent_design);
    let now_q = lisp_quote_string(&now);
    let owner_q = lisp_quote_string(owner);
    let scope_q = lisp_quote_string(scope);
    let dispatch_q = lisp_quote_string(dispatch_strategy);

    // Build the meta block incrementally so the optional dispatch slots can be
    // omitted cleanly when not supplied while keeping the closing paren
    // balanced. `:companion-of` was the historical terminal field — we preserve
    // its position and append the new dispatch metadata after it.
    let mut meta = String::new();
    meta.push_str("  (meta\n");
    meta.push_str(&format!("    :execution-id {}\n", id_q));
    meta.push_str(&format!("    :parent-design {}\n", parent_q));
    meta.push_str("    :status \"open\"\n");
    meta.push_str(&format!("    :opened-at {}\n", now_q));
    meta.push_str(&format!("    :last-updated-at {}\n", now_q));
    meta.push_str(&format!("    :owner {}\n", owner_q));
    meta.push_str(&format!("    :scope {}\n", scope_q));
    meta.push_str(&format!("    :companion-of {}\n", parent_q));
    meta.push_str(&format!("    :dispatch-strategy {}", dispatch_q));
    if let Some(tp) = target_project {
        meta.push('\n');
        meta.push_str(&format!("    :target-project {}", lisp_quote_string(tp)));
    }
    if let Some(cwd) = requested_cwd {
        meta.push('\n');
        meta.push_str(&format!("    :requested-cwd {}", lisp_quote_string(cwd)));
    }
    meta.push_str(")\n");

    format!(
        ";; ══════════════════════════════════════════════════════\n\
         ;; MissionD — Execution Companion Log\n\
         ;; Created via mission_execution(action=open) at {now}\n\
         ;; Protocol: agent-execution-coordination v0.5.x\n\
         ;; Parent:   {parent}\n\
         ;; ══════════════════════════════════════════════════════\n\
         \n\
         (execution-log\n\
         {meta}\n\
         \x20\x20(id-counters\n\
         \x20\x20\x20\x20:next-claim-id 1\n\
         \x20\x20\x20\x20:next-deviation-id 1\n\
         \x20\x20\x20\x20:next-decision-id 1\n\
         \x20\x20\x20\x20:next-issue-id 1\n\
         \x20\x20\x20\x20:next-completion-id 1)\n\
         \n\
         \x20\x20(phase-tracker\n\
         \x20\x20\x20\x20:current-phase nil\n\
         \x20\x20\x20\x20:phases ()\n\
         \x20\x20\x20\x20:stage-cursor 0\n\
         \x20\x20\x20\x20:checkpoints ())\n\
         \n\
         \x20\x20(claims)\n\
         \n\
         \x20\x20(deviations)\n\
         \n\
         \x20\x20(decisions)\n\
         \n\
         \x20\x20(issues)\n\
         \n\
         \x20\x20(completions)\n\
         \n\
         \x20\x20(derived-indexes\n\
         \x20\x20\x20\x20:active-claims ()\n\
         \x20\x20\x20\x20:open-issues ()\n\
         \x20\x20\x20\x20:unresolved-deviations ()\n\
         \x20\x20\x20\x20:latest-decisions ()\n\
         \x20\x20\x20\x20:completed-phases ()))\n",
        now = now,
        parent = parent_design,
        meta = meta,
    )
}

// ───────────────────────────────────────────────────────────────────────
// id allocation helpers — atomic via id-counters slot
// ───────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Counter {
    Claim,
    Deviation,
    Decision,
    Issue,
    Completion,
}

impl Counter {
    fn key(self) -> &'static str {
        match self {
            Counter::Claim => "next-claim-id",
            Counter::Deviation => "next-deviation-id",
            Counter::Decision => "next-decision-id",
            Counter::Issue => "next-issue-id",
            Counter::Completion => "next-completion-id",
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            Counter::Claim => "C",
            Counter::Deviation => "D",
            Counter::Decision => "DC",
            Counter::Issue => "I",
            Counter::Completion => "COMP",
        }
    }

    fn block_name(self) -> &'static str {
        match self {
            Counter::Claim => "claims",
            Counter::Deviation => "deviations",
            Counter::Decision => "decisions",
            Counter::Issue => "issues",
            Counter::Completion => "completions",
        }
    }
}

/// Find the byte position in `src` where `:key <value>` begins inside the
/// id-counters block. Returns (key_start, value_start, value_end).
fn locate_kv_value(src: &str, block: &Node, key: &str) -> Option<(usize, usize, usize)> {
    let kids = block.children();
    let mut i = 1; // skip head atom
    while i + 1 < kids.len() {
        if kids[i].as_atom().map(|a| a.trim_start_matches(':')) == Some(key) {
            return Some((kids[i].start, kids[i + 1].start, kids[i + 1].end));
        }
        i += 1;
    }
    let _ = src;
    None
}

/// Allocate the next ID for `counter`. Returns the formatted id string and
/// rewrites the source to bump the counter. If the id-counters block is
/// missing, falls back to scanning existing entries for max+1 and synthesizes
/// the counter via `repair`-style insertion before the first existing entry
/// block. (audit will surface this as a structural fix-up.)
fn allocate_id(file: &mut LogFile, counter: Counter) -> Result<String> {
    let counter_block = file.find_block("id-counters").cloned();

    if let Some(block) = counter_block {
        let (_, vstart, vend) =
            locate_kv_value(&file.src, &block, counter.key()).ok_or_else(|| {
                anyhow!(
                    "id-counters block missing `:{}` — run mission_execution(action=\"repair\")",
                    counter.key()
                )
            })?;
        let value_text = file.src[vstart..vend].trim();
        let n: u32 = value_text
            .parse()
            .map_err(|e| anyhow!("id-counters `:{}` not an integer: {} ({})", counter.key(), value_text, e))?;
        let id = format!("{}{:03}", counter.prefix(), n);
        let next = n + 1;
        let new_value = next.to_string();
        let mut new_src = String::with_capacity(file.src.len());
        new_src.push_str(&file.src[..vstart]);
        new_src.push_str(&new_value);
        new_src.push_str(&file.src[vend..]);
        file.src = new_src;
        // Re-parse so subsequent block lookups use refreshed spans.
        let forms = sexp::parse(&file.src)?;
        let root_idx = forms
            .iter()
            .position(|n| {
                matches!(
                    n.head_atom(),
                    Some("execution-log") | Some("execution")
                )
            })
            .ok_or_else(|| anyhow!("execution-log root vanished after counter bump"))?;
        file.forms = forms;
        file.root_idx = root_idx;
        return Ok(id);
    }

    // Fallback path: no id-counters block. Scan existing entries for the
    // largest numeric suffix matching the prefix, and synthesize next.
    // Mutating without an id-counters slot is allowed but flagged by audit.
    let max = scan_max_id(file, counter);
    let next = max + 1;
    Ok(format!("{}{:03}", counter.prefix(), next))
}

fn scan_max_id(file: &LogFile, counter: Counter) -> u32 {
    let block = match file.find_block(counter.block_name()) {
        Some(b) => b,
        None => return 0,
    };
    let prefix = counter.prefix();
    let mut max: u32 = 0;
    for child in block.children().iter().skip(1) {
        // Two flavors:
        //   (D001 ...)   — id is the head atom
        //   (deviation :id D001 ...) — id is after :id
        if let Some(head) = child.head_atom() {
            if let Some(rest) = head.strip_prefix(prefix) {
                if rest.chars().all(|c| c.is_ascii_digit()) && !rest.is_empty() {
                    if let Ok(n) = rest.parse::<u32>() {
                        max = max.max(n);
                        continue;
                    }
                }
            }
            // Look for `:id <ID>` inside.
            let kids = child.children();
            let mut i = 1;
            while i + 1 < kids.len() {
                if kids[i].as_atom() == Some(":id") {
                    let val = match &kids[i + 1].kind {
                        NodeKind::Str(s) => s.clone(),
                        NodeKind::Atom(s) => s.clone(),
                        _ => String::new(),
                    };
                    if let Some(rest) = val.strip_prefix(prefix) {
                        if let Ok(n) = rest.parse::<u32>() {
                            max = max.max(n);
                        }
                    }
                    break;
                }
                i += 1;
            }
        }
    }
    max
}

// ───────────────────────────────────────────────────────────────────────
// block append / read / write helpers
// ───────────────────────────────────────────────────────────────────────

/// Insert `entry_text` (already-rendered S-expr lines without leading newline)
/// into the block named `block_name` just before its closing paren. If the
/// block is missing it is synthesized at the end of the root form (audit will
/// flag the file but the append still succeeds — this matches the lisp's
/// "derived-indexes can rebuild" tolerance).
fn append_to_block(file: &mut LogFile, block_name: &str, entry_text: &str) -> Result<()> {
    if let Some(block) = file.find_block(block_name).cloned() {
        let close = block.end - 1;
        let body_is_empty = block_body_is_empty(&block, &file.src);
        let mut new_src = String::with_capacity(file.src.len() + entry_text.len() + 8);
        new_src.push_str(&file.src[..close]);
        if body_is_empty {
            new_src.push('\n');
        } else {
            new_src.push_str("\n");
        }
        new_src.push_str(entry_text);
        if !entry_text.ends_with('\n') {
            new_src.push('\n');
        }
        new_src.push_str("  ");
        new_src.push_str(&file.src[close..]);
        file.src = new_src;
    } else {
        // Synthesize block before the root form's closing paren.
        let root = file.root().clone();
        let close = root.end - 1;
        let synth = format!(
            "\n  ({block}\n{entry}\n  )\n",
            block = block_name,
            entry = entry_text,
        );
        let mut new_src = String::with_capacity(file.src.len() + synth.len());
        new_src.push_str(&file.src[..close]);
        new_src.push_str(&synth);
        new_src.push_str(&file.src[close..]);
        file.src = new_src;
    }
    let forms = sexp::parse(&file.src)?;
    let root_idx = forms
        .iter()
        .position(|n| matches!(n.head_atom(), Some("execution-log") | Some("execution")))
        .ok_or_else(|| anyhow!("execution-log root vanished after append"))?;
    file.forms = forms;
    file.root_idx = root_idx;
    Ok(())
}

fn block_body_is_empty(block: &Node, _src: &str) -> bool {
    block.children().len() <= 1
}

/// Update the meta block's :last-updated-at field to `now`. If the block is
/// absent or the field absent, this is a best-effort no-op.
fn touch_last_updated(file: &mut LogFile) -> Result<()> {
    let now = now_iso();
    let meta = match file.find_block("meta").cloned() {
        Some(m) => m,
        None => return Ok(()),
    };
    if let Some((_, vstart, vend)) = locate_kv_value(&file.src, &meta, "last-updated-at") {
        let new_value = lisp_quote_string(&now);
        let mut new_src = String::with_capacity(file.src.len() + new_value.len());
        new_src.push_str(&file.src[..vstart]);
        new_src.push_str(&new_value);
        new_src.push_str(&file.src[vend..]);
        file.src = new_src;
        let forms = sexp::parse(&file.src)?;
        let root_idx = forms
            .iter()
            .position(|n| matches!(n.head_atom(), Some("execution-log") | Some("execution")))
            .ok_or_else(|| anyhow!("execution-log root vanished after touch"))?;
        file.forms = forms;
        file.root_idx = root_idx;
    }
    Ok(())
}

fn write_log_file(path: &Path, file: &LogFile) -> Result<()> {
    sexp::check_balance(&file.src)
        .map_err(|e| anyhow!("refusing to write — paren balance broken: {}", e))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("lisp.tmp");
    std::fs::write(&tmp, file.src.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn read_log_file(path: &Path) -> Result<LogFile> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("cannot read {}: {}", path.display(), e))?;
    LogFile::parse(text)
}

// ───────────────────────────────────────────────────────────────────────
// claim helpers
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ClaimRecord {
    id: String,
    claimer: String,
    scope: String,
    phase: Option<String>,
    lease_expires_at: Option<String>,
    heartbeat_at: Option<String>,
    status: String,
}

fn parse_claims(file: &LogFile) -> Vec<ClaimRecord> {
    let block = match file.find_block("claims") {
        Some(b) => b,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for child in block.children().iter().skip(1) {
        let head = child.head_atom().unwrap_or("");
        let kvs = parse_kv_pairs(&file.src, child.children());
        // Two flavors: head is the id, or `:id <ID>` is inline.
        let id = if head.starts_with(['C', 'c']) && head.len() > 1 && head[1..].chars().all(|c| c.is_ascii_digit()) {
            head.to_string()
        } else if let Some(v) = kvs.get("id").or_else(|| kvs.get("claim-id")).cloned() {
            v.trim().to_string()
        } else {
            // Legacy unnumbered claim — keep but with synthetic id.
            format!("claim@{}", child.start)
        };
        let status = kvs
            .get("status")
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_else(|| {
                if kvs.get("released-at").is_some() {
                    "released".to_string()
                } else {
                    "active".to_string()
                }
            });
        out.push(ClaimRecord {
            id,
            claimer: kvs
                .get("claimer")
                .or_else(|| kvs.get("agent"))
                .cloned()
                .unwrap_or_default(),
            scope: kvs.get("scope").cloned().unwrap_or_default(),
            phase: kvs.get("phase").cloned(),
            lease_expires_at: kvs.get("lease-expires-at").cloned(),
            heartbeat_at: kvs.get("heartbeat-at").cloned(),
            status,
        });
    }
    out
}

fn parse_iso(s: &str) -> Option<DateTime<Utc>> {
    let t = s.trim().trim_matches('"');
    DateTime::parse_from_rfc3339(t).ok().map(|d| d.with_timezone(&Utc))
}

fn scopes_overlap(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.starts_with(b) || b.starts_with(a)
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
struct CompletionRecord {
    id: String,
    phase: String,
    agent: String,
    at: String,
    changed_files: Option<Vec<String>>,
    staged_files: Option<Vec<String>>,
    commit_hash: Option<String>,
    commit_status: Option<String>,
    commit_blocker: Option<String>,
}

fn parse_completions(file: &LogFile) -> Vec<CompletionRecord> {
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
        });
    }
    out
}

/// Build the dashboard-friendly `durability` projection over a slice of
/// `CompletionRecord`s. The shape stays stable across legacy + new
/// companion logs: when no completion carries scoped-commit metadata the
/// summary still surfaces zero counts plus `latest_commit_status: null`
/// so consumers do not need to special-case "old log".
fn summarize_durability(records: &[CompletionRecord]) -> Value {
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
    let latest_status = records
        .iter()
        .rev()
        .find_map(|r| r.commit_status.clone());
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
// action: open
// ───────────────────────────────────────────────────────────────────────

async fn action_open(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let parent_design = match require_str(args, "parent_design") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let scope = match require_str(args, "scope") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let owner = args
        .get("owner")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let dispatch_strategy = normalize_dispatch_strategy(
        args.get("dispatch_strategy").and_then(|v| v.as_str()),
    );
    let target_project = args
        .get("target_project")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let requested_cwd = args
        .get("requested_cwd")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);

    if path.exists() {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "EXECUTION_EXISTS",
                format!("companion log already exists at {}", path.display()),
            )
            .with_suggestion("use action=status to inspect, or pick a different execution_id"),
        ));
    }

    let body = render_canonical_template(
        execution_id,
        parent_design,
        scope,
        owner,
        dispatch_strategy,
        target_project,
        requested_cwd,
    );
    sexp::check_balance(&body).map_err(|e| anyhow!("template paren balance broken: {}", e))?;
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(&path, body.as_bytes())?;

    // intent-worker.lisp :: claudecode-workstation-orchestration ::
    // execution-strategy-record asks for dispatch metadata to be surfaced on
    // the live ExecutionEvent::Opened projection alongside the durable
    // companion-log meta block. The companion log remains the source of
    // truth (per planned-event-extensions :: ExecutionEvent :: rationale);
    // these optional fields are skipped on serialize when absent so legacy
    // Opened consumers stay byte-identical.
    let event = build_opened_event(
        execution_id,
        parent_design,
        scope,
        owner,
        path.display().to_string(),
        dispatch_strategy,
        target_project,
        requested_cwd,
    );
    emit_execution_event(state, event).await;

    let mut response = json!({
        "status": "opened",
        "execution_id": execution_id,
        "path": path.display().to_string(),
        "parent_design": parent_design,
        "scope": scope,
        "owner": owner,
        "dispatch_strategy": dispatch_strategy,
    });
    if let Some(tp) = target_project {
        response["target_project"] = json!(tp);
    }
    if let Some(cwd) = requested_cwd {
        response["requested_cwd"] = json!(cwd);
    }
    Ok(ToolResult::json_pretty(&response))
}

// ───────────────────────────────────────────────────────────────────────
// action: list
// ───────────────────────────────────────────────────────────────────────

async fn action_list(state: &AppState, args: &Value) -> Result<ToolResult> {
    let parent_filter = args.get("parent_design").and_then(|v| v.as_str());
    let status_filter = args.get("status").and_then(|v| v.as_str());
    let scope_prefix = args.get("scope_prefix").and_then(|v| v.as_str());
    let limit = args
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(50)
        .clamp(1, 500) as usize;

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let dir = root.join(COMPANION_DIR);
    let mut summaries: Vec<Value> = Vec::new();
    if !dir.exists() {
        return Ok(ToolResult::json_pretty(&json!({
            "executions": [],
            "hint": format!("no {} directory under {}", COMPANION_DIR, root.display()),
        })));
    }

    for entry in std::fs::read_dir(&dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("lisp") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let file = match read_log_file(&path) {
            Ok(f) => f,
            Err(_) => continue, // skip non-execution lisps
        };
        let meta = match file.find_block("meta") {
            Some(m) => parse_kv_pairs(&file.src, m.children()),
            None => HashMap::new(),
        };
        let parent = meta
            .get("parent-design")
            .or_else(|| meta.get("parent_design"))
            .or_else(|| meta.get("parent"))
            .cloned()
            .unwrap_or_default();
        let status = meta
            .get("status")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        let scope = meta.get("scope").cloned().unwrap_or_default();
        // Workstation-dispatch metadata; legacy logs may omit it. Empty
        // string preserves a stable column shape for dashboards while
        // signalling "no record" cheaply.
        let dispatch = meta
            .get("dispatch-strategy")
            .map(|s| s.trim().trim_matches('"').to_string())
            .unwrap_or_default();
        let target_project = meta
            .get("target-project")
            .map(|s| s.trim().trim_matches('"').to_string())
            .filter(|s| !s.is_empty());

        if let Some(pf) = parent_filter {
            if !parent.contains(pf) {
                continue;
            }
        }
        if let Some(sf) = status_filter {
            if !status.contains(sf) {
                continue;
            }
        }
        if let Some(sp) = scope_prefix {
            if !scope.starts_with(sp) {
                continue;
            }
        }

        let claims = parse_claims(&file);
        let active = claims.iter().filter(|c| c.status == "active").count();
        // Surface a thin durability snapshot per execution so dashboards can
        // tell at a glance whether scoped commits are flowing. Full per-row
        // details still live behind `mission_execution(action=status)` —
        // here we only carry counts + the latest commit_status to keep the
        // list payload small (intent-memory.lisp :: helper agent-execution-
        // coordination :: scoped-commit-contract :: invariants :inv-7).
        let completions = parse_completions(&file);
        let durability = summarize_durability(&completions);
        let mut row = json!({
            "execution_id": name,
            "path": path.display().to_string(),
            "parent_design": parent.trim_matches('"'),
            "status": status.trim_matches('"'),
            "scope": scope.trim_matches('"'),
            "active_claims": active,
            "claim_count": claims.len(),
            "dispatch_strategy": dispatch,
            "durability": durability,
        });
        if let Some(tp) = target_project {
            row["target_project"] = json!(tp);
        }
        summaries.push(row);
        if summaries.len() >= limit {
            break;
        }
    }

    summaries.sort_by(|a, b| {
        a["execution_id"]
            .as_str()
            .unwrap_or("")
            .cmp(b["execution_id"].as_str().unwrap_or(""))
    });

    Ok(ToolResult::json_pretty(&json!({
        "executions": summaries,
        "count": summaries.len(),
    })))
}

// ───────────────────────────────────────────────────────────────────────
// action: claim / heartbeat / release
// ───────────────────────────────────────────────────────────────────────

async fn action_claim(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let claimer = match require_str(args, "claimer_name") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let scope = match require_str(args, "scope") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let phase = args.get("phase").and_then(|v| v.as_str()).unwrap_or("");
    let lease_secs = args
        .get("lease_secs")
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_LEASE_SECS)
        .clamp(60, MAX_LEASE_SECS);

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;

    // Conflict check: any active claim with overlapping scope.
    let now = Utc::now();
    let claims = parse_claims(&file);
    for c in &claims {
        if c.status != "active" {
            continue;
        }
        // Treat lease-expired claims as soft-released for conflict purposes
        // (still surfaced in audit as stale).
        if let Some(exp) = c.lease_expires_at.as_deref().and_then(parse_iso) {
            if exp < now {
                continue;
            }
        }
        if scopes_overlap(&c.scope, scope) {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    "CLAIM_CONFLICT",
                    format!(
                        "scope `{}` overlaps active claim {} held by `{}` over `{}`",
                        scope, c.id, c.claimer, c.scope
                    ),
                )
                .with_suggestion(
                    "wait for release/heartbeat expiry, narrow scope, or contact the claimer",
                ),
            ));
        }
    }

    let claim_id = allocate_id(&mut file, Counter::Claim)?;
    let acquired = now_iso();
    let expires = (now + chrono::Duration::seconds(lease_secs))
        .to_rfc3339_opts(SecondsFormat::Secs, true);
    let entry = format!(
        "    ({id}\n      :claimer {claimer}\n      :scope {scope}\n      :phase {phase}\n      :acquired-at {acquired}\n      :lease-expires-at {expires}\n      :heartbeat-at {acquired}\n      :status \"active\")",
        id = claim_id,
        claimer = lisp_quote_string(claimer),
        scope = lisp_quote_string(scope),
        phase = lisp_quote_string(phase),
        acquired = lisp_quote_string(&acquired),
        expires = lisp_quote_string(&expires),
    );
    append_to_block(&mut file, "claims", &entry)?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    emit_execution_event(
        state,
        ExecutionEvent::Claimed {
            execution_id: execution_id.to_string(),
            claim_id: claim_id.clone(),
            claimer: claimer.to_string(),
            scope: scope.to_string(),
            phase: phase.to_string(),
            lease_expires_at: expires.clone(),
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "claimed",
        "claim_id": claim_id,
        "claimer": claimer,
        "scope": scope,
        "phase": phase,
        "acquired_at": acquired,
        "lease_expires_at": expires,
    })))
}

async fn action_heartbeat(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let claim_id = match require_str(args, "claim_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let claimer = match require_str(args, "claimer_name") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let lease_secs = args
        .get("lease_secs")
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_LEASE_SECS)
        .clamp(60, MAX_LEASE_SECS);

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;

    let claim_node = match find_claim_node(&file, claim_id) {
        Some(n) => n.clone(),
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::NOT_FOUND, format!("claim {} not found", claim_id))
                    .with_suggestion("use action=status to list active claims"),
            ))
        }
    };

    let kvs = parse_kv_pairs(&file.src, claim_node.children());
    let owner = kvs
        .get("claimer")
        .or_else(|| kvs.get("agent"))
        .cloned()
        .unwrap_or_default();
    if owner.trim_matches('"') != claimer {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "CLAIM_WRONG_OWNER",
                format!("claim {} owned by `{}`, not `{}`", claim_id, owner, claimer),
            )
            .with_suggestion("use the original claimer_name or run action=audit"),
        ));
    }

    let now = Utc::now();
    let now_s = now.to_rfc3339_opts(SecondsFormat::Secs, true);
    let expires = (now + chrono::Duration::seconds(lease_secs))
        .to_rfc3339_opts(SecondsFormat::Secs, true);

    update_kv_in_node(&mut file, &claim_node, "heartbeat-at", &lisp_quote_string(&now_s))?;
    let claim_node2 = find_claim_node(&file, claim_id)
        .cloned()
        .ok_or_else(|| anyhow!("claim node vanished after heartbeat update"))?;
    update_kv_in_node(
        &mut file,
        &claim_node2,
        "lease-expires-at",
        &lisp_quote_string(&expires),
    )?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    emit_execution_event(
        state,
        ExecutionEvent::Heartbeat {
            execution_id: execution_id.to_string(),
            claim_id: claim_id.to_string(),
            claimer: claimer.to_string(),
            heartbeat_at: now_s.clone(),
            lease_expires_at: expires.clone(),
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "heartbeat",
        "claim_id": claim_id,
        "heartbeat_at": now_s,
        "lease_expires_at": expires,
    })))
}

async fn action_release(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let claim_id = match require_str(args, "claim_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let claimer = match require_str(args, "claimer_name") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let summary = args.get("summary").and_then(|v| v.as_str()).unwrap_or("");

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;

    let claim_node = match find_claim_node(&file, claim_id) {
        Some(n) => n.clone(),
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::NOT_FOUND, format!("claim {} not found", claim_id))
                    .with_suggestion("use action=status to list active claims"),
            ))
        }
    };

    let kvs = parse_kv_pairs(&file.src, claim_node.children());
    let owner = kvs
        .get("claimer")
        .or_else(|| kvs.get("agent"))
        .cloned()
        .unwrap_or_default();
    if owner.trim_matches('"') != claimer {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "CLAIM_WRONG_OWNER",
                format!("claim {} owned by `{}`, not `{}`", claim_id, owner, claimer),
            )
            .with_suggestion("use the original claimer_name or run action=audit"),
        ));
    }

    let now = now_iso();
    update_kv_in_node(&mut file, &claim_node, "released-at", &lisp_quote_string(&now))?;
    let claim_node2 = find_claim_node(&file, claim_id)
        .cloned()
        .ok_or_else(|| anyhow!("claim node vanished after release update"))?;
    update_kv_in_node(&mut file, &claim_node2, "status", &lisp_quote_string("released"))?;
    if !summary.is_empty() {
        let claim_node3 = find_claim_node(&file, claim_id)
            .cloned()
            .ok_or_else(|| anyhow!("claim node vanished after status update"))?;
        update_kv_in_node(&mut file, &claim_node3, "summary", &lisp_quote_string(summary))?;
    }
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    emit_execution_event(
        state,
        ExecutionEvent::Released {
            execution_id: execution_id.to_string(),
            claim_id: claim_id.to_string(),
            claimer: claimer.to_string(),
            released_at: now.clone(),
            summary: if summary.is_empty() {
                None
            } else {
                Some(summary.to_string())
            },
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "released",
        "claim_id": claim_id,
        "released_at": now,
        "summary": summary,
    })))
}

fn find_claim_node<'a>(file: &'a LogFile, claim_id: &str) -> Option<&'a Node> {
    let block = file.find_block("claims")?;
    for child in block.children().iter().skip(1) {
        if child.head_atom() == Some(claim_id) {
            return Some(child);
        }
        let kvs = parse_kv_pairs(&file.src, child.children());
        if let Some(id) = kvs.get("id").or_else(|| kvs.get("claim-id")) {
            if id.trim().trim_matches('"') == claim_id {
                return Some(child);
            }
        }
    }
    None
}

/// Update or insert `:key value` inside the given node. The node must be a
/// list; insertion happens just before the closing paren.
fn update_kv_in_node(file: &mut LogFile, node: &Node, key: &str, new_value_lit: &str) -> Result<()> {
    if let Some((kstart, vstart, vend)) = locate_kv_value(&file.src, node, key) {
        let _ = kstart;
        let mut new_src = String::with_capacity(file.src.len());
        new_src.push_str(&file.src[..vstart]);
        new_src.push_str(new_value_lit);
        new_src.push_str(&file.src[vend..]);
        file.src = new_src;
    } else {
        let close = node.end - 1;
        let insertion = format!("\n      :{} {}", key, new_value_lit);
        let mut new_src = String::with_capacity(file.src.len() + insertion.len());
        new_src.push_str(&file.src[..close]);
        new_src.push_str(&insertion);
        new_src.push_str(&file.src[close..]);
        file.src = new_src;
    }
    let forms = sexp::parse(&file.src)?;
    let root_idx = forms
        .iter()
        .position(|n| matches!(n.head_atom(), Some("execution-log") | Some("execution")))
        .ok_or_else(|| anyhow!("execution-log root vanished after kv update"))?;
    file.forms = forms;
    file.root_idx = root_idx;
    Ok(())
}

// ───────────────────────────────────────────────────────────────────────
// action: deviate / decide / issue / complete
// ───────────────────────────────────────────────────────────────────────

async fn action_deviate(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let lisp_said = match require_str(args, "lisp_said") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let actually_found = match require_str(args, "actually_found") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let reason = match require_str(args, "reason") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let approved_by = args.get("approved_by").and_then(|v| v.as_str()).unwrap_or("auto");
    let phase = args.get("phase").and_then(|v| v.as_str()).unwrap_or("");

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;
    let id = allocate_id(&mut file, Counter::Deviation)?;
    let date = now_iso();
    let entry = format!(
        "    ({id}\n      :phase {phase}\n      :date {date}\n      :lisp-said {lisp_said}\n      :actually-found {actually_found}\n      :reason {reason}\n      :approved-by {approved_by}\n      :status \"open\")",
        id = id,
        phase = lisp_quote_string(phase),
        date = lisp_quote_string(&date),
        lisp_said = lisp_quote_string(lisp_said),
        actually_found = lisp_quote_string(actually_found),
        reason = lisp_quote_string(reason),
        approved_by = lisp_quote_string(approved_by),
    );
    append_to_block(&mut file, "deviations", &entry)?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    emit_execution_event(
        state,
        ExecutionEvent::DeviationRecorded {
            execution_id: execution_id.to_string(),
            deviation_id: id.clone(),
            phase: phase.to_string(),
            approved_by: approved_by.to_string(),
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "recorded",
        "deviation_id": id,
        "phase": phase,
        "approved_by": approved_by,
    })))
}

async fn action_decide(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let context = match require_str(args, "context") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let chosen = match require_str(args, "chosen") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let rationale = match require_str(args, "rationale") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let decided_by = match require_str(args, "decided_by") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let options = args.get("options").and_then(|v| v.as_str()).unwrap_or("");

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;
    let id = allocate_id(&mut file, Counter::Decision)?;
    let date = now_iso();
    let entry = format!(
        "    ({id}\n      :context {context}\n      :options {options}\n      :chosen {chosen}\n      :rationale {rationale}\n      :decided-by {decided_by}\n      :at {date})",
        id = id,
        context = lisp_quote_string(context),
        options = lisp_quote_string(options),
        chosen = lisp_quote_string(chosen),
        rationale = lisp_quote_string(rationale),
        decided_by = lisp_quote_string(decided_by),
        date = lisp_quote_string(&date),
    );
    append_to_block(&mut file, "decisions", &entry)?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    emit_execution_event(
        state,
        ExecutionEvent::DecisionRecorded {
            execution_id: execution_id.to_string(),
            decision_id: id.clone(),
            decided_by: decided_by.to_string(),
            at: date.clone(),
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "recorded",
        "decision_id": id,
        "decided_by": decided_by,
        "at": date,
    })))
}

async fn action_issue(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let severity = args
        .get("severity")
        .and_then(|v| v.as_str())
        .unwrap_or("medium");
    let desc = match require_str(args, "desc") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let resolution_path = args
        .get("resolution_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let owner = args.get("owner").and_then(|v| v.as_str()).unwrap_or("");

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;
    let id = allocate_id(&mut file, Counter::Issue)?;
    let date = now_iso();
    let entry = format!(
        "    ({id}\n      :severity {severity}\n      :desc {desc}\n      :resolution-path {res}\n      :owner {owner}\n      :at {date}\n      :status \"open\")",
        id = id,
        severity = lisp_quote_string(severity),
        desc = lisp_quote_string(desc),
        res = lisp_quote_string(resolution_path),
        owner = lisp_quote_string(owner),
        date = lisp_quote_string(&date),
    );
    append_to_block(&mut file, "issues", &entry)?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    emit_execution_event(
        state,
        ExecutionEvent::IssueRecorded {
            execution_id: execution_id.to_string(),
            issue_id: id.clone(),
            severity: severity.to_string(),
            owner: owner.to_string(),
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "recorded",
        "issue_id": id,
        "severity": severity,
        "owner": owner,
    })))
}

async fn action_complete(state: &AppState, args: &Value) -> Result<ToolResult> {
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
                    .with_suggestion(
                        "see intent-memory.lisp :: completions :commit-status-values",
                    ),
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

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;
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
    entry.push(')');

    append_to_block(&mut file, "completions", &entry)?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    emit_execution_event(
        state,
        ExecutionEvent::Completed {
            execution_id: execution_id.to_string(),
            completion_id: id.clone(),
            phase: phase.to_string(),
            agent: agent.to_string(),
            at: date.clone(),
        },
    )
    .await;

    let mut response = json!({
        "status": "recorded",
        "completion_id": id,
        "phase": phase,
        "agent": agent,
        "at": date,
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
    Ok(ToolResult::json_pretty(&response))
}

// ───────────────────────────────────────────────────────────────────────
// action: status — meta + active claims + open issues + unresolved deviations
// ───────────────────────────────────────────────────────────────────────

async fn action_status(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let file = read_log_file(&path)?;

    let meta = file
        .find_block("meta")
        .map(|m| parse_kv_pairs(&file.src, m.children()))
        .unwrap_or_default();
    let counters = file
        .find_block("id-counters")
        .map(|m| parse_kv_pairs(&file.src, m.children()))
        .unwrap_or_default();
    let phase_tracker = file
        .find_block("phase-tracker")
        .map(|m| parse_kv_pairs(&file.src, m.children()))
        .unwrap_or_default();

    let claims = parse_claims(&file);
    let now = Utc::now();
    let active_claims: Vec<Value> = claims
        .iter()
        .filter(|c| c.status == "active")
        .map(|c| {
            let stale = c
                .lease_expires_at
                .as_deref()
                .and_then(parse_iso)
                .map(|exp| exp < now)
                .unwrap_or(false);
            json!({
                "id": c.id,
                "claimer": c.claimer,
                "scope": c.scope,
                "phase": c.phase,
                "lease_expires_at": c.lease_expires_at,
                "heartbeat_at": c.heartbeat_at,
                "stale": stale,
            })
        })
        .collect();

    let unresolved = list_block_summaries(&file, "deviations", |kvs, head| {
        let status = kvs
            .get("status")
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_else(|| "open".to_string());
        if status == "resolved" || status == "closed" {
            None
        } else {
            Some(json!({
                "id": head.to_string(),
                "phase": kvs.get("phase").cloned().unwrap_or_default(),
                "lisp_said": kvs.get("lisp-said").or_else(|| kvs.get("lisp_said")).cloned().unwrap_or_default(),
                "actually_did": kvs.get("actually-found").or_else(|| kvs.get("actually-did")).cloned().unwrap_or_default(),
                "approved_by": kvs.get("approved-by").or_else(|| kvs.get("approved_by")).cloned().unwrap_or_default(),
                "status": status,
            }))
        }
    });

    let open_issues = list_block_summaries(&file, "issues", |kvs, head| {
        let status = kvs
            .get("status")
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_else(|| "open".to_string());
        if status == "resolved" || status == "closed" {
            None
        } else {
            Some(json!({
                "id": head.to_string(),
                "severity": kvs.get("severity").cloned().unwrap_or_default(),
                "desc": kvs.get("desc").cloned().unwrap_or_default(),
                "owner": kvs.get("owner").cloned().unwrap_or_default(),
                "status": status,
            }))
        }
    });

    let latest_decisions = list_block_summaries(&file, "decisions", |kvs, head| {
        Some(json!({
            "id": head.to_string(),
            "context": kvs.get("context").cloned().unwrap_or_default(),
            "chosen": kvs.get("chosen").cloned().unwrap_or_default(),
            "decided_by": kvs.get("decided-by").or_else(|| kvs.get("decided_by")).cloned().unwrap_or_default(),
            "at": kvs.get("at").cloned().unwrap_or_default(),
        }))
    });

    // ── completion durability projection ───────────────────────────
    // intent-memory.lisp :: helper agent-execution-coordination :: completions
    // gained `changed_files / staged_files / commit_hash / commit_status /
    // commit_blocker` for the scoped-commit handoff. Surface them in
    // `completed_phases` (legacy keys preserved) and roll them up into a
    // dedicated `durability` block so dashboards can show "still pending /
    // blocked / fully durable" without re-parsing the companion log.
    let completion_records = parse_completions(&file);
    let completed_phases: Vec<Value> = completion_records
        .iter()
        .map(|c| {
            let mut row = json!({
                "id": c.id,
                "phase": c.phase,
                "agent": c.agent,
                "at": c.at,
            });
            if let Some(list) = &c.changed_files {
                row["changed_files"] = json!(list);
            }
            if let Some(list) = &c.staged_files {
                row["staged_files"] = json!(list);
            }
            if let Some(hash) = &c.commit_hash {
                row["commit_hash"] = json!(hash);
            }
            if let Some(status_val) = &c.commit_status {
                row["commit_status"] = json!(status_val);
            }
            if let Some(blocker) = &c.commit_blocker {
                row["commit_blocker"] = json!(blocker);
            }
            row
        })
        .collect();

    let durability = summarize_durability(&completion_records);

    Ok(ToolResult::json_pretty(&json!({
        "execution_id": execution_id,
        "path": path.display().to_string(),
        "meta": json_strip_quotes(meta),
        "id_counters": json_strip_quotes(counters),
        "phase_tracker": json_strip_quotes(phase_tracker),
        "active_claims": active_claims,
        "unresolved_deviations": unresolved,
        "open_issues": open_issues,
        "latest_decisions": latest_decisions,
        "completed_phases": completed_phases,
        "durability": durability,
    })))
}

fn list_block_summaries<F>(file: &LogFile, name: &str, mut f: F) -> Vec<Value>
where
    F: FnMut(&HashMap<String, String>, &str) -> Option<Value>,
{
    let block = match file.find_block(name) {
        Some(b) => b,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for child in block.children().iter().skip(1) {
        let head = child.head_atom().unwrap_or("");
        let kvs = parse_kv_pairs(&file.src, child.children());
        if let Some(v) = f(&kvs, head) {
            out.push(v);
        }
    }
    out
}

fn json_strip_quotes(map: HashMap<String, String>) -> Value {
    let mut obj = serde_json::Map::new();
    for (k, v) in map {
        let trimmed = v.trim();
        let unquoted = trimmed.trim_matches('"');
        obj.insert(k, Value::String(unquoted.to_string()));
    }
    Value::Object(obj)
}

// ───────────────────────────────────────────────────────────────────────
// action: audit — paren balance + ID monotonic + claim overlap + stale +
//                 completion coverage + open-issue owners
// ───────────────────────────────────────────────────────────────────────

async fn action_audit(state: &AppState, args: &Value) -> Result<ToolResult> {
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
    emit_execution_event(
        state,
        ExecutionEvent::Audited {
            execution_id: execution_id.to_string(),
            ok,
            findings_count: findings.len() as u32,
            error_count,
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

fn check_id_monotonic(file: &LogFile, counter: Counter, findings: &mut Vec<Value>) {
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
            let num: u32 = idtxt
                .trim_start_matches(prefix)
                .parse()
                .unwrap_or(0);
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
fn audit_scoped_commit_handoff(
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

// ───────────────────────────────────────────────────────────────────────
// action: repair — dry-run by default; structural fixes only
// ───────────────────────────────────────────────────────────────────────

async fn action_repair(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let mode = args.get("mode").and_then(|v| v.as_str()).unwrap_or("dry_run");
    if mode != "dry_run" && mode != "apply" {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!("repair mode must be `dry_run` or `apply`, got `{}`", mode),
            ),
        ));
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

    emit_execution_event(
        state,
        ExecutionEvent::Repaired {
            execution_id: execution_id.to_string(),
            applied: mode == "apply",
            action_count: actions.len() as u32,
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

fn insert_id_counters_block(
    file: &mut LogFile,
    claim_n: u32,
    dev_n: u32,
    dec_n: u32,
    issue_n: u32,
    comp_n: u32,
) -> Result<()> {
    // Insert just after the meta block if present; else at the start of the
    // root form's body.
    let insertion = format!(
        "\n  (id-counters\n    :next-claim-id {claim_n}\n    :next-deviation-id {dev_n}\n    :next-decision-id {dec_n}\n    :next-issue-id {issue_n}\n    :next-completion-id {comp_n})\n",
        claim_n = claim_n,
        dev_n = dev_n,
        dec_n = dec_n,
        issue_n = issue_n,
        comp_n = comp_n,
    );
    let pos = if let Some(meta) = file.find_block("meta") {
        meta.end
    } else {
        // After the head atom of the root form.
        let root = file.root();
        let kids = root.children();
        if let Some(first) = kids.first() {
            first.end
        } else {
            root.end - 1
        }
    };
    let mut new_src = String::with_capacity(file.src.len() + insertion.len());
    new_src.push_str(&file.src[..pos]);
    new_src.push_str(&insertion);
    new_src.push_str(&file.src[pos..]);
    file.src = new_src;
    let forms = sexp::parse(&file.src)?;
    let root_idx = forms
        .iter()
        .position(|n| matches!(n.head_atom(), Some("execution-log") | Some("execution")))
        .ok_or_else(|| anyhow!("execution-log root vanished after id-counters insert"))?;
    file.forms = forms;
    file.root_idx = root_idx;
    Ok(())
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

// ───────────────────────────────────────────────────────────────────────
// tests — exercise the parser, ID allocation, and round-trip on a
// freshly-opened canonical file
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_file() -> LogFile {
        let body = render_canonical_template(
            "test-exec",
            ".missiond/v2/test.lisp",
            "test scope",
            "tester",
            DEFAULT_DISPATCH_STRATEGY,
            None,
            None,
        );
        LogFile::parse(body).expect("template must parse")
    }

    #[test]
    fn template_parses_and_balances() {
        let body = render_canonical_template(
            "e",
            "p",
            "s",
            "o",
            DEFAULT_DISPATCH_STRATEGY,
            None,
            None,
        );
        sexp::check_balance(&body).expect("balanced");
        LogFile::parse(body).expect("parse");
    }

    #[test]
    fn dispatch_strategy_normalization() {
        assert_eq!(normalize_dispatch_strategy(None), "unknown");
        assert_eq!(normalize_dispatch_strategy(Some("")), "unknown");
        assert_eq!(normalize_dispatch_strategy(Some("   ")), "unknown");
        assert_eq!(normalize_dispatch_strategy(Some("not-a-real-mode")), "unknown");
        assert_eq!(
            normalize_dispatch_strategy(Some("fresh-code-alignment")),
            "fresh-code-alignment"
        );
        assert_eq!(normalize_dispatch_strategy(Some("agent-team")), "agent-team");
        assert_eq!(normalize_dispatch_strategy(Some("resident-lisp")), "resident-lisp");
    }

    #[test]
    fn template_writes_dispatch_metadata() {
        let body = render_canonical_template(
            "exec-disp",
            ".missiond/v2/disp.lisp",
            "scope/x",
            "owner-x",
            "fresh-code-alignment",
            Some("missiond"),
            Some("/Users/x/Projects/missiond/crates/foo"),
        );
        sexp::check_balance(&body).expect("balanced");
        let file = LogFile::parse(body).expect("parse");
        let meta = parse_kv_pairs(
            &file.src,
            file.find_block("meta").expect("meta block").children(),
        );
        assert_eq!(
            meta.get("dispatch-strategy").map(|s| s.as_str()),
            Some("fresh-code-alignment")
        );
        assert_eq!(
            meta.get("target-project").map(|s| s.as_str()),
            Some("missiond")
        );
        assert_eq!(
            meta.get("requested-cwd").map(|s| s.as_str()),
            Some("/Users/x/Projects/missiond/crates/foo")
        );
    }

    #[test]
    fn template_omits_optional_dispatch_fields() {
        let body = render_canonical_template(
            "exec-min",
            ".missiond/v2/min.lisp",
            "scope/y",
            "owner-y",
            "agent-team",
            None,
            None,
        );
        sexp::check_balance(&body).expect("balanced");
        let file = LogFile::parse(body).expect("parse");
        let meta = parse_kv_pairs(
            &file.src,
            file.find_block("meta").expect("meta block").children(),
        );
        assert_eq!(
            meta.get("dispatch-strategy").map(|s| s.as_str()),
            Some("agent-team")
        );
        assert!(meta.get("target-project").is_none());
        assert!(meta.get("requested-cwd").is_none());
    }

    #[test]
    fn legacy_template_without_dispatch_still_parses() {
        // Hand-written legacy meta: no dispatch-strategy key, mirrors files
        // produced by the previous handler version. Must round-trip cleanly.
        let body = "(execution-log\n  \
                    (meta\n    \
                    :execution-id \"legacy-x\"\n    \
                    :parent-design \"old.lisp\"\n    \
                    :status \"open\"\n    \
                    :owner \"old-owner\"\n    \
                    :scope \"legacy/scope\"\n    \
                    :companion-of \"old.lisp\")\n  \
                    (claims))\n";
        let file = LogFile::parse(body.to_string()).expect("legacy parses");
        let meta = parse_kv_pairs(
            &file.src,
            file.find_block("meta").expect("meta block").children(),
        );
        assert!(meta.get("dispatch-strategy").is_none());
        assert!(meta.get("target-project").is_none());
        // sanity: existing fields still readable. parse_kv_pairs returns the
        // raw source slice when the value is a quoted string atom, so the
        // outer quotes survive — downstream consumers strip them via
        // `trim_matches('"')`, which is the contract we mirror here.
        assert_eq!(
            meta.get("scope").map(|s| s.trim_matches('"').to_string()),
            Some("legacy/scope".to_string())
        );
    }

    #[test]
    fn project_or_target_project_prefers_canonical() {
        let args = json!({
            "project": "primary",
            "target_project": "alias",
        });
        assert_eq!(project_or_target_project(&args), Some("primary"));

        let alias_only = json!({"target_project": "alias-only"});
        assert_eq!(project_or_target_project(&alias_only), Some("alias-only"));

        let neither = json!({});
        assert_eq!(project_or_target_project(&neither), None);
    }

    #[test]
    fn id_counter_allocation_and_bump() {
        let mut file = fresh_file();
        let id1 = allocate_id(&mut file, Counter::Deviation).unwrap();
        let id2 = allocate_id(&mut file, Counter::Deviation).unwrap();
        assert_eq!(id1, "D001");
        assert_eq!(id2, "D002");
        let counters = file.find_block("id-counters").unwrap();
        let kvs = parse_kv_pairs(&file.src, counters.children());
        assert_eq!(kvs.get("next-deviation-id").unwrap().trim(), "3");
    }

    #[test]
    fn append_to_empty_block_keeps_balance() {
        let mut file = fresh_file();
        let id = allocate_id(&mut file, Counter::Issue).unwrap();
        let entry = format!(
            "    ({id}\n      :severity \"low\"\n      :desc \"smoke\"\n      :status \"open\")",
            id = id
        );
        append_to_block(&mut file, "issues", &entry).unwrap();
        sexp::check_balance(&file.src).expect("still balanced");
        let issues = file.find_block("issues").unwrap();
        assert_eq!(issues.children().len(), 2);
    }

    #[test]
    fn scan_max_id_handles_legacy_format() {
        let body = "(execution-log\n  (deviations\n    (D001 :phase \"a\")\n    (D004 :phase \"b\")))\n";
        let file = LogFile::parse(body.to_string()).unwrap();
        assert_eq!(scan_max_id(&file, Counter::Deviation), 4);
    }

    #[test]
    fn parses_existing_pilot_file_shape() {
        // Quick smoke on the legacy `(execution name ...)` shape.
        let body = "(execution worker-pillar\n  (meta :execution_id \"x\")\n  (claims))\n";
        let file = LogFile::parse(body.to_string()).unwrap();
        assert!(file.find_block("meta").is_some());
        assert!(file.find_block("claims").is_some());
    }

    /// `build_opened_event` is the single mapping point between
    /// `action_open` arguments and the live `ExecutionEvent::Opened`
    /// projection. When all dispatch metadata is present, every slot
    /// must round-trip into the event verbatim.
    #[test]
    fn build_opened_event_carries_all_dispatch_metadata() {
        let ev = build_opened_event(
            "exec-evt",
            ".missiond/v2/parent.lisp",
            "scope/x",
            "claude",
            "/abs/path/exec-evt.lisp".into(),
            "fresh-code-alignment",
            Some("missiond"),
            Some("/Users/x/Projects/missiond/crates/foo"),
        );
        match ev {
            ExecutionEvent::Opened {
                execution_id,
                parent_design,
                scope,
                owner,
                path,
                dispatch_strategy,
                target_project,
                requested_cwd,
            } => {
                assert_eq!(execution_id, "exec-evt");
                assert_eq!(parent_design, ".missiond/v2/parent.lisp");
                assert_eq!(scope, "scope/x");
                assert_eq!(owner, "claude");
                assert_eq!(path, "/abs/path/exec-evt.lisp");
                assert_eq!(dispatch_strategy.as_deref(), Some("fresh-code-alignment"));
                assert_eq!(target_project.as_deref(), Some("missiond"));
                assert_eq!(
                    requested_cwd.as_deref(),
                    Some("/Users/x/Projects/missiond/crates/foo"),
                );
            }
            _ => panic!("expected Opened"),
        }
    }

    /// When the open args omit `target_project` / `requested_cwd`, the
    /// event keeps them as `None` (so they skip-serialize) while still
    /// surfacing the canonical `dispatch_strategy` string. This mirrors
    /// the runtime path through `action_open` for callers that only
    /// provide the strategy.
    #[test]
    fn build_opened_event_omits_optional_metadata_when_absent() {
        let ev = build_opened_event(
            "exec-min",
            "p.lisp",
            "scope/y",
            "owner-y",
            "/abs/exec-min.lisp".into(),
            DEFAULT_DISPATCH_STRATEGY,
            None,
            None,
        );
        match &ev {
            ExecutionEvent::Opened {
                dispatch_strategy,
                target_project,
                requested_cwd,
                ..
            } => {
                assert_eq!(dispatch_strategy.as_deref(), Some(DEFAULT_DISPATCH_STRATEGY));
                assert!(target_project.is_none());
                assert!(requested_cwd.is_none());
            }
            _ => panic!("expected Opened"),
        }
        let json = serde_json::to_string(&ev).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let opened = parsed.get("Opened").and_then(|v| v.as_object()).unwrap();
        assert!(opened.contains_key("dispatch_strategy"));
        assert!(!opened.contains_key("target_project"));
        assert!(!opened.contains_key("requested_cwd"));
    }

    // ── Wave 12 / Task 01 — scoped-commit handoff durability plane ──
    //
    // Tests below pin the shape of the new completion fields and the
    // audit findings against intent-memory.lisp :: scoped-commit-contract
    // and intent-flow.lisp :: F-scoped-commit-handoff. They exercise
    // pure helpers (no AppState / no project root) so the daemon-wide
    // `cargo test -p missiond-daemon` still PASSes when sibling agents
    // are mid-edit on plan.rs / workflow.rs / etc.

    fn fresh_file_with_claim() -> LogFile {
        let mut file = fresh_file();
        // Hand-append a single active claim covering "src/" so the
        // staged-file scope check has something to validate against.
        // We bypass `action_claim` because it lives behind AppState.
        let now = now_iso();
        let entry = format!(
            "    (C001\n      :claimer \"agent\"\n      :scope \"src/\"\n      :phase \"phase-A\"\n      :acquired-at {ts}\n      :lease-expires-at {ts}\n      :heartbeat-at {ts}\n      :status \"active\")",
            ts = lisp_quote_string(&now),
        );
        append_to_block(&mut file, "claims", &entry).unwrap();
        file
    }

    /// Validate the canonical commit-status normalizer rejects unknown
    /// labels but lets every value from intent-memory.lisp ::
    /// :commit-status-values through unchanged.
    #[test]
    fn commit_status_normalizer_accepts_canonical_only() {
        for &status in VALID_COMMIT_STATUSES {
            assert_eq!(normalize_commit_status(status), Some(status));
        }
        assert_eq!(normalize_commit_status("  pending  "), Some("pending"));
        assert!(normalize_commit_status("").is_none());
        assert!(normalize_commit_status("done").is_none());
        assert!(normalize_commit_status("COMMITTED").is_none());
    }

    /// Empty list arguments must be preserved as `Some(vec![])` so a
    /// completion can record "intentionally staged nothing"; absent keys
    /// stay `None` so the legacy 6-field shape remains byte-identical.
    #[test]
    fn collect_string_list_distinguishes_absent_from_empty() {
        let none_args = json!({});
        assert!(collect_string_list(&none_args, "changed_files").is_none());

        let empty_args = json!({"changed_files": []});
        assert_eq!(
            collect_string_list(&empty_args, "changed_files"),
            Some(vec![])
        );

        let with_paths = json!({
            "changed_files": ["src/a.rs", "  src/b.rs  ", "", "src/c.rs"],
        });
        assert_eq!(
            collect_string_list(&with_paths, "changed_files"),
            Some(vec![
                "src/a.rs".to_string(),
                "src/b.rs".to_string(),
                "src/c.rs".to_string(),
            ])
        );
    }

    /// `render_string_list` round-trips through `parse_string_list` so
    /// audit/status reads of the companion log return the exact list the
    /// writer recorded — including the empty-list literal.
    #[test]
    fn string_list_round_trip() {
        let empty = render_string_list(&[]);
        assert_eq!(empty, "()");
        assert_eq!(parse_string_list(&empty), Some(Vec::<String>::new()));

        let items = vec!["src/a.rs".to_string(), "tests/b.rs".to_string()];
        let rendered = render_string_list(&items);
        let parsed = parse_string_list(&rendered).expect("must parse");
        assert_eq!(parsed, items);

        // Quotes inside paths survive the lisp_quote_string escape cycle.
        let quoted = vec!["src/a\"b.rs".to_string()];
        let rendered = render_string_list(&quoted);
        assert_eq!(parse_string_list(&rendered), Some(quoted));
    }

    /// Legacy completions (no scoped-commit metadata) must still parse
    /// and yield `None` everywhere on the new fields. This is the
    /// backward-compat contract from the task file: "legacy execution
    /// 文件缺字段必须继续 parse".
    #[test]
    fn parse_completions_handles_legacy_shape() {
        let body = "(execution-log\n  (completions\n    (COMP001 :phase \"a\" :agent \"x\" :summary \"s\" :deliverables \"d\" :verification \"v\" :at \"2026-04-26T00:00:00Z\")))\n";
        let file = LogFile::parse(body.to_string()).expect("legacy parses");
        let comps = parse_completions(&file);
        assert_eq!(comps.len(), 1);
        let c = &comps[0];
        assert_eq!(c.id, "COMP001");
        assert_eq!(c.phase, "a");
        assert_eq!(c.agent, "x");
        assert!(c.changed_files.is_none());
        assert!(c.staged_files.is_none());
        assert!(c.commit_hash.is_none());
        assert!(c.commit_status.is_none());
        assert!(c.commit_blocker.is_none());
    }

    /// A completion that carries every scoped-commit field must be
    /// readable round-trip from the durable file. We assemble the
    /// completion entry by hand (mirroring what `action_complete`
    /// writes) so the parser is exercised against the on-disk shape.
    #[test]
    fn parse_completions_reads_scoped_commit_fields() {
        let body = "(execution-log\n  (completions\n    (COMP001\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :changed-files (\"src/a.rs\" \"src/b.rs\")\n      :staged-files (\"src/a.rs\")\n      :commit-hash \"abc1234\"\n      :commit-status \"committed\"\n      :commit-blocker \"\")))\n";
        let file = LogFile::parse(body.to_string()).expect("parse");
        let comps = parse_completions(&file);
        assert_eq!(comps.len(), 1);
        let c = &comps[0];
        assert_eq!(
            c.changed_files.as_deref(),
            Some(&["src/a.rs".to_string(), "src/b.rs".to_string()][..])
        );
        assert_eq!(c.staged_files.as_deref(), Some(&["src/a.rs".to_string()][..]));
        assert_eq!(c.commit_hash.as_deref(), Some("abc1234"));
        assert_eq!(c.commit_status.as_deref(), Some("committed"));
        // Empty blocker collapses to `None` so audit does not key off
        // whitespace.
        assert!(c.commit_blocker.is_none());
    }

    /// `action_complete` is gated behind AppState, so we directly drive
    /// the lower-level write helpers it now wraps. The test asserts
    /// that each scoped-commit field round-trips into the companion log
    /// when supplied, and that omitting them keeps the legacy entry
    /// shape intact.
    #[test]
    fn complete_writes_each_commit_status_value() {
        for &status in &["not-required", "pending", "committed", "blocked", "skipped"] {
            let mut file = fresh_file_with_claim();
            let id = allocate_id(&mut file, Counter::Completion).unwrap();
            let mut entry = format!(
                "    ({id}\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :changed-files {changed}\n      :staged-files {staged}",
                id = id,
                changed = render_string_list(&["src/a.rs".to_string()]),
                staged = render_string_list(&["src/a.rs".to_string()]),
            );
            entry.push_str(&format!(
                "\n      :commit-status {}",
                lisp_quote_string(status)
            ));
            if status == "committed" {
                entry.push_str("\n      :commit-hash \"abc1234\"");
            }
            if status == "blocked" {
                entry.push_str("\n      :commit-blocker \"index conflict\"");
            }
            entry.push(')');
            append_to_block(&mut file, "completions", &entry).unwrap();
            sexp::check_balance(&file.src).expect("balanced");
            let comps = parse_completions(&file);
            let c = comps.last().unwrap();
            assert_eq!(c.commit_status.as_deref(), Some(status));
            if status == "committed" {
                assert_eq!(c.commit_hash.as_deref(), Some("abc1234"));
            } else {
                assert!(c.commit_hash.is_none());
            }
            if status == "blocked" {
                assert_eq!(c.commit_blocker.as_deref(), Some("index conflict"));
            } else {
                assert!(c.commit_blocker.is_none());
            }
        }
    }

    /// Audit must flag a completion whose commit_status="committed" lacks
    /// a commit_hash — the durability gap that scoped-commit-contract
    /// :inv-7 explicitly rejects.
    #[test]
    fn audit_flags_committed_without_hash() {
        let mut file = fresh_file_with_claim();
        let id = allocate_id(&mut file, Counter::Completion).unwrap();
        let entry = format!(
            "    ({id}\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :commit-status \"committed\")",
            id = id,
        );
        append_to_block(&mut file, "completions", &entry).unwrap();

        let claims = parse_claims(&file);
        let mut findings = Vec::new();
        audit_scoped_commit_handoff(&file, &claims, &mut findings);
        let kinds: Vec<&str> = findings
            .iter()
            .filter_map(|f| f.get("kind").and_then(|v| v.as_str()))
            .collect();
        assert!(
            kinds.contains(&FINDING_COMMIT_STATUS_NO_HASH),
            "expected {} in {:?}",
            FINDING_COMMIT_STATUS_NO_HASH,
            kinds
        );
        // Severity must be "error" so audit `ok` flips, mirroring the
        // existing duplicate-id / claim-overlap invariants.
        let f = findings
            .iter()
            .find(|f| f.get("kind").and_then(|v| v.as_str()) == Some(FINDING_COMMIT_STATUS_NO_HASH))
            .unwrap();
        assert_eq!(
            f.get("severity").and_then(|v| v.as_str()),
            Some("error")
        );
    }

    /// Audit must flag a completion whose commit_status="blocked" lacks a
    /// commit_blocker — the next agent has no recovery context per the
    /// scoped-commit-contract :recovery-rule.
    #[test]
    fn audit_flags_blocked_without_blocker() {
        let mut file = fresh_file_with_claim();
        let id = allocate_id(&mut file, Counter::Completion).unwrap();
        let entry = format!(
            "    ({id}\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :commit-status \"blocked\")",
            id = id,
        );
        append_to_block(&mut file, "completions", &entry).unwrap();

        let claims = parse_claims(&file);
        let mut findings = Vec::new();
        audit_scoped_commit_handoff(&file, &claims, &mut findings);
        assert!(findings.iter().any(|f| f.get("kind").and_then(|v| v.as_str())
            == Some(FINDING_COMMIT_BLOCKED_NO_BLOCKER)));
    }

    /// Audit must flag staged_files paths that escape every recorded
    /// claim scope. The active claim covers "src/"; staging
    /// "vendor/x.rs" is outside scope and must surface as
    /// scoped-commit-violation per scoped-commit-contract :scope-rule.
    #[test]
    fn audit_flags_scoped_commit_violation() {
        let mut file = fresh_file_with_claim();
        let id = allocate_id(&mut file, Counter::Completion).unwrap();
        let entry = format!(
            "    ({id}\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :changed-files {changed}\n      :staged-files {staged}\n      :commit-status \"committed\"\n      :commit-hash \"abc1234\")",
            id = id,
            changed = render_string_list(&["src/a.rs".to_string(), "vendor/x.rs".to_string()]),
            staged = render_string_list(&["src/a.rs".to_string(), "vendor/x.rs".to_string()]),
        );
        append_to_block(&mut file, "completions", &entry).unwrap();

        let claims = parse_claims(&file);
        let mut findings = Vec::new();
        audit_scoped_commit_handoff(&file, &claims, &mut findings);
        let violation = findings
            .iter()
            .find(|f| f.get("kind").and_then(|v| v.as_str()) == Some(FINDING_SCOPED_COMMIT_VIOLATION))
            .expect("scoped-commit-violation finding required");
        let staged = violation
            .get("staged_files")
            .and_then(|v| v.as_array())
            .unwrap();
        let staged_strs: Vec<&str> = staged.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(staged_strs, vec!["vendor/x.rs"]);
        assert_eq!(
            violation.get("severity").and_then(|v| v.as_str()),
            Some("error")
        );
    }

    /// Completions whose staged_files stay inside an existing claim
    /// scope must NOT trip the violation check, even when the claim is
    /// already released — that is the legitimate handoff path from
    /// F-scoped-commit-handoff :: s7 release-claim.
    #[test]
    fn audit_passes_scoped_commit_inside_released_claim() {
        let mut file = fresh_file();
        // Released claim covering "crates/foo/" — staging files inside
        // this scope must remain valid even after release.
        let now = now_iso();
        let claim = format!(
            "    (C001\n      :claimer \"agent\"\n      :scope \"crates/foo/\"\n      :phase \"phase-A\"\n      :acquired-at {ts}\n      :lease-expires-at {ts}\n      :released-at {ts}\n      :heartbeat-at {ts}\n      :status \"released\")",
            ts = lisp_quote_string(&now),
        );
        append_to_block(&mut file, "claims", &claim).unwrap();
        let id = allocate_id(&mut file, Counter::Completion).unwrap();
        let entry = format!(
            "    ({id}\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :changed-files {changed}\n      :staged-files {staged}\n      :commit-status \"committed\"\n      :commit-hash \"abc1234\")",
            id = id,
            changed = render_string_list(&["crates/foo/src/a.rs".to_string()]),
            staged = render_string_list(&["crates/foo/src/a.rs".to_string()]),
        );
        append_to_block(&mut file, "completions", &entry).unwrap();

        let claims = parse_claims(&file);
        let mut findings = Vec::new();
        audit_scoped_commit_handoff(&file, &claims, &mut findings);
        let kinds: Vec<&str> = findings
            .iter()
            .filter_map(|f| f.get("kind").and_then(|v| v.as_str()))
            .collect();
        assert!(
            !kinds.contains(&FINDING_SCOPED_COMMIT_VIOLATION),
            "no violation expected, got {:?}",
            kinds
        );
        assert!(
            !kinds.contains(&FINDING_COMMIT_STATUS_NO_HASH),
            "no missing-hash expected, got {:?}",
            kinds
        );
    }

    /// `summarize_durability` rolls up an empty completions list to
    /// zero counts + null latest fields, so list/status payloads stay
    /// shape-stable across legacy companion logs.
    #[test]
    fn summarize_durability_handles_empty_and_mixed() {
        let v = summarize_durability(&[]);
        assert_eq!(
            v.get("completion_count").and_then(|x| x.as_i64()),
            Some(0)
        );
        assert!(v.get("latest_commit_status").map(|x| x.is_null()).unwrap_or(false));

        let records = vec![
            CompletionRecord {
                id: "COMP001".into(),
                phase: "p".into(),
                agent: "a".into(),
                at: "2026-04-26T00:00:00Z".into(),
                changed_files: None,
                staged_files: None,
                commit_hash: Some("abc".into()),
                commit_status: Some("committed".into()),
                commit_blocker: None,
            },
            CompletionRecord {
                id: "COMP002".into(),
                phase: "p".into(),
                agent: "a".into(),
                at: "2026-04-26T00:01:00Z".into(),
                changed_files: None,
                staged_files: None,
                commit_hash: None,
                commit_status: Some("blocked".into()),
                commit_blocker: Some("conflict".into()),
            },
        ];
        let v = summarize_durability(&records);
        assert_eq!(v.get("completion_count").and_then(|x| x.as_i64()), Some(2));
        assert_eq!(v.get("with_commit_hash").and_then(|x| x.as_i64()), Some(1));
        assert_eq!(
            v.get("blocked_with_blocker").and_then(|x| x.as_i64()),
            Some(1)
        );
        assert_eq!(
            v.get("latest_commit_status").and_then(|x| x.as_str()),
            Some("blocked")
        );
        let by = v
            .get("by_commit_status")
            .and_then(|x| x.as_object())
            .unwrap();
        assert_eq!(by.get("committed").and_then(|x| x.as_i64()), Some(1));
        assert_eq!(by.get("blocked").and_then(|x| x.as_i64()), Some(1));
        assert_eq!(by.get("pending").and_then(|x| x.as_i64()), Some(0));
    }
}
