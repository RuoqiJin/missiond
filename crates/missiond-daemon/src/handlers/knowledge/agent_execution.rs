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
        let mut row = json!({
            "execution_id": name,
            "path": path.display().to_string(),
            "parent_design": parent.trim_matches('"'),
            "status": status.trim_matches('"'),
            "scope": scope.trim_matches('"'),
            "active_claims": active,
            "claim_count": claims.len(),
            "dispatch_strategy": dispatch,
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

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;
    let id = allocate_id(&mut file, Counter::Completion)?;
    let date = now_iso();
    let entry = format!(
        "    ({id}\n      :phase {phase}\n      :agent {agent}\n      :summary {summary}\n      :deliverables {deliverables}\n      :verification {verification}\n      :at {date})",
        id = id,
        phase = lisp_quote_string(phase),
        agent = lisp_quote_string(agent),
        summary = lisp_quote_string(summary),
        deliverables = lisp_quote_string(deliverables),
        verification = lisp_quote_string(verification),
        date = lisp_quote_string(&date),
    );
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

    Ok(ToolResult::json_pretty(&json!({
        "status": "recorded",
        "completion_id": id,
        "phase": phase,
        "agent": agent,
        "at": date,
    })))
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

    let completed_phases = list_block_summaries(&file, "completions", |kvs, head| {
        Some(json!({
            "id": head.to_string(),
            "phase": kvs.get("phase").cloned().unwrap_or_default(),
            "agent": kvs.get("agent").cloned().unwrap_or_default(),
            "at": kvs.get("at").cloned().unwrap_or_default(),
        }))
    });

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
}
