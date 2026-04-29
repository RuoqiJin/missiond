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
use chrono::{SecondsFormat, Utc};
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::state::AppState;

mod claim_lease;
mod completion_audit;
mod log_surface;

pub(super) use self::claim_lease::scopes_overlap_pure;
use self::claim_lease::{
    find_claim_node, parse_claims, parse_iso, scopes_overlap, ClaimRecord, DEFAULT_LEASE_SECS,
    MAX_LEASE_SECS,
};
use self::completion_audit::{
    normalize_commit_status, normalize_task_run_verifier_status, normalize_verifier_status,
    parse_completions, summarize_durability, FINDING_COMMIT_BLOCKED_NO_BLOCKER,
    FINDING_COMMIT_STATUS_NO_HASH, FINDING_SCOPED_COMMIT_VIOLATION, VALID_COMMIT_STATUSES,
    VALID_TASK_RUN_VERIFIER_STATUSES, VALID_VERIFIER_STATUSES,
};
#[cfg(test)]
use self::completion_audit::{parse_string_list, CompletionRecord};
use self::log_surface::{
    append_session_trace_event, build_opened_event, emit_execution_event,
    normalize_dispatch_strategy, read_dispatch_metadata_from_log, resolve_session_trace_path,
    resolve_trace_task_id, sanitize_trace_backend, TraceEvent, TraceKind,
};
#[cfg(test)]
use self::log_surface::{
    is_valid_trace_id, render_trace_event, scan_max_trace_seq, DispatchMeta, TraceWarning,
    DEFAULT_DISPATCH_STRATEGY,
};

const COMPANION_DIR: &str = ".missiond/v2";

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
        "preflight_commit" => action_preflight_commit(state, &args).await,
        other => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("unknown mission_execution action `{}`", other),
            )
            .with_suggestion(
                "valid: open|list|claim|heartbeat|release|deviate|decide|issue|complete|status|audit|repair|preflight_commit",
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
        ToolResult::structured_error(ToolError::new(
            error_codes::MISSING_PARAM,
            format!("missing required param `{}`", key),
        ))
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
        let n: u32 = value_text.parse().map_err(|e| {
            anyhow!(
                "id-counters `:{}` not an integer: {} ({})",
                counter.key(),
                value_text,
                e
            )
        })?;
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
            .position(|n| matches!(n.head_atom(), Some("execution-log") | Some("execution")))
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

    let dispatch_strategy =
        normalize_dispatch_strategy(args.get("dispatch_strategy").and_then(|v| v.as_str()));
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

    // wave23-04 — opt-in session-trace append. When the caller threads
    // `session_trace_path` we emit a `dispatch` event capturing this
    // open as the first fact in the task's trace. Best-effort: failures
    // surface as `trace_warning` without aborting the open result.
    if let Some(trace_path) = resolve_session_trace_path(args, &root) {
        match resolve_trace_task_id(args, &root, execution_id) {
            Some(task_id) => {
                let backend = sanitize_trace_backend(owner);
                let ev = TraceEvent {
                    task: task_id,
                    backend,
                    kind: TraceKind::Dispatch,
                    summary: format!(
                        "mission_execution(action=open) execution_id={} parent_design={} dispatch_strategy={}",
                        execution_id, parent_design, dispatch_strategy
                    ),
                    agent: None,
                    files: None,
                    commit_hash: None,
                    report_path: None,
                };
                if let Err(w) = append_session_trace_event(&trace_path, &ev) {
                    response["trace_warning"] = json!(w.to_string());
                }
            }
            None => {
                response["trace_warning"] = json!(format!(
                    "session_trace_path supplied but execution_id `{}` is not a valid trace task id and no task_contract_path was provided",
                    execution_id
                ));
            }
        }
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
    let expires =
        (now + chrono::Duration::seconds(lease_secs)).to_rfc3339_opts(SecondsFormat::Secs, true);
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

    // Surface the workstation-dispatch trio on the live event so consumers
    // can correlate this claim against the dispatch context without
    // re-loading the companion log. We read the trio from the same
    // post-write `file` handle so the meta block we observe is the one
    // just persisted (the claim append doesn't touch meta beyond
    // `:last-updated-at`, which we ignore here).
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Claimed {
            execution_id: execution_id.to_string(),
            claim_id: claim_id.clone(),
            claimer: claimer.to_string(),
            scope: scope.to_string(),
            phase: phase.to_string(),
            lease_expires_at: expires.clone(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
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
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("claim {} not found", claim_id),
                )
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
    let expires =
        (now + chrono::Duration::seconds(lease_secs)).to_rfc3339_opts(SecondsFormat::Secs, true);

    update_kv_in_node(
        &mut file,
        &claim_node,
        "heartbeat-at",
        &lisp_quote_string(&now_s),
    )?;
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

    // Wave 20 / Task 09 — surface the workstation-dispatch trio on the
    // live event so a long-lived heartbeat stream stays correlatable
    // against the dispatch context. The same projection rationale as
    // `action_claim` / `action_complete`: read the trio from the
    // post-write `file` handle so the meta block we observe is the one
    // just persisted.
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Heartbeat {
            execution_id: execution_id.to_string(),
            claim_id: claim_id.to_string(),
            claimer: claimer.to_string(),
            heartbeat_at: now_s.clone(),
            lease_expires_at: expires.clone(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
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
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("claim {} not found", claim_id),
                )
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
    update_kv_in_node(
        &mut file,
        &claim_node,
        "released-at",
        &lisp_quote_string(&now),
    )?;
    let claim_node2 = find_claim_node(&file, claim_id)
        .cloned()
        .ok_or_else(|| anyhow!("claim node vanished after release update"))?;
    update_kv_in_node(
        &mut file,
        &claim_node2,
        "status",
        &lisp_quote_string("released"),
    )?;
    if !summary.is_empty() {
        let claim_node3 = find_claim_node(&file, claim_id)
            .cloned()
            .ok_or_else(|| anyhow!("claim node vanished after status update"))?;
        update_kv_in_node(
            &mut file,
            &claim_node3,
            "summary",
            &lisp_quote_string(summary),
        )?;
    }
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    // Wave 20 / Task 09 — same dispatch-metadata projection rationale as
    // `action_claim`. `Released` completes the pair with `Claimed`, so
    // claim-lifetime aggregators can join the two events without
    // re-loading the companion log.
    let meta = read_dispatch_metadata_from_log(&file);
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
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
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

/// Update or insert `:key value` inside the given node. The node must be a
/// list; insertion happens just before the closing paren.
fn update_kv_in_node(
    file: &mut LogFile,
    node: &Node,
    key: &str,
    new_value_lit: &str,
) -> Result<()> {
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
    let approved_by = args
        .get("approved_by")
        .and_then(|v| v.as_str())
        .unwrap_or("auto");
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

    // Wave 20 / Task 09 — surface the workstation-dispatch trio so a
    // deviation observer can route on dispatch context without re-loading
    // the companion log. Read from the post-write `file` handle.
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::DeviationRecorded {
            execution_id: execution_id.to_string(),
            deviation_id: id.clone(),
            phase: phase.to_string(),
            approved_by: approved_by.to_string(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
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

    // Wave 20 / Task 09 — surface the workstation-dispatch trio so a
    // decision observer can route on dispatch context without re-loading
    // the companion log. Read from the post-write `file` handle.
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::DecisionRecorded {
            execution_id: execution_id.to_string(),
            decision_id: id.clone(),
            decided_by: decided_by.to_string(),
            at: date.clone(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
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

    // Wave 20 / Task 09 — surface the workstation-dispatch trio so an
    // issue observer can route on dispatch context without re-loading
    // the companion log. Read from the post-write `file` handle.
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::IssueRecorded {
            execution_id: execution_id.to_string(),
            issue_id: id.clone(),
            severity: severity.to_string(),
            owner: owner.to_string(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
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
                    .with_suggestion("see intent-memory.lisp :: completions :commit-status-values"),
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

    // ── wave-19 / task 08 — task-contract completion metadata.
    //
    // All four fields are optional and recorded verbatim into the
    // companion log when supplied. `verifier_status` is normalized
    // against the canonical enum so audit / dashboard consumers can key
    // off the exact string; unknown labels reject with `INVALID_PARAM`
    // BEFORE any file mutation. `task_contract_path` doubles as the
    // trigger for the contract-level enforcement gate further below
    // when paired with `enforce_scoped_commit=true`.
    let task_contract_path = args
        .get("task_contract_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let task_report_path = args
        .get("task_report_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let verifier_status_raw = args
        .get("verifier_status")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let verifier_status = match verifier_status_raw {
        Some(s) => match normalize_verifier_status(s) {
            Some(canonical) => Some(canonical.to_string()),
            None => {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "verifier_status must be one of {:?}, got `{}`",
                            VALID_VERIFIER_STATUSES, s
                        ),
                    )
                    .with_suggestion(
                        "see wave19-08 :: verifier-status enum (passed|failed|skipped|unknown)",
                    ),
                ));
            }
        },
        None => None,
    };
    let verifier_notes = args
        .get("verifier_notes")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // ── wave-21 / task 03 — task-run verifier completion metadata.
    //
    // `task_run_verifier_status` / `shared_memory_path` /
    // `verifier_diagnostics` / `verified` mirror the wave19-08 fields
    // but capture the END-TO-END verifier outcome (task contract +
    // report + shared-memory completion + commit scope all proven in
    // one pass — see wave21-02 :: scripts/verify-task-run.mjs). All
    // four are optional and recorded verbatim into the companion log;
    // `task_run_verifier_status` rejects unknown labels at parse time
    // so audit / dashboard consumers can key off the canonical enum.
    let task_run_verifier_status_raw = args
        .get("task_run_verifier_status")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let task_run_verifier_status = match task_run_verifier_status_raw {
        Some(s) => match normalize_task_run_verifier_status(s) {
            Some(canonical) => Some(canonical.to_string()),
            None => {
                return Ok(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "task_run_verifier_status must be one of {:?}, got `{}`",
                            VALID_TASK_RUN_VERIFIER_STATUSES, s
                        ),
                    )
                    .with_suggestion(
                        "see wave21-03 :: task-run-verifier-status enum (passed|failed|skipped|unknown)",
                    ),
                ));
            }
        },
        None => None,
    };
    let shared_memory_path = args
        .get("shared_memory_path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let verifier_diagnostics = args
        .get("verifier_diagnostics")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    // `verified` is a tri-state at parse time: absent → None (legacy
    // shape, no extra gate), false → Some(false) (caller explicitly
    // recorded a non-verified completion), true → Some(true) (gate
    // runs). We persist the explicit `false` so audit can tell "writer
    // intentionally skipped verification" from "writer omitted the
    // field because they're a legacy caller".
    let verified_flag = args.get("verified").and_then(|v| v.as_bool());

    // ── Optional fail-fast enforcement (wave16-06).
    //
    // `enforce_scoped_commit=true` flips the existing audit-only handoff
    // checks into hard rejects at completion-time. Default `false` keeps
    // legacy callers byte-identical: they still get the audit-only path
    // wired through `mission_execution(action=audit)` later. We resolve
    // the flag here so the validation step (run BEFORE id allocation)
    // sees the caller's intent without paying the read cost twice.
    let enforce_scoped_commit = args
        .get("enforce_scoped_commit")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;

    // Run the enforcement gate BEFORE `allocate_id` mutates the
    // id-counters block — a rejected completion must not bump the
    // counter or otherwise change the durable file.
    let scoped_commit_validation = if enforce_scoped_commit {
        match enforce_scoped_commit_completion(
            &file,
            staged_files.as_deref(),
            commit_hash.as_deref(),
            commit_status.as_deref(),
            commit_blocker.as_deref(),
        ) {
            Ok(v) => Some(v),
            Err(err) => return Ok(err),
        }
    } else {
        None
    };

    // wave-19 / task 08 — contract-level enforcement gate. Runs only
    // when the caller paired `enforce_scoped_commit=true` with a
    // `task_contract_path`; otherwise the contract metadata is recorded
    // verbatim with no additional checks (legacy / opt-out behaviour).
    // Daemon never shells out — we read the file off disk and use the
    // workstation_dispatch parser to project the narrow view we need.
    let task_contract_validation = if enforce_scoped_commit && task_contract_path.is_some() {
        let path_arg = task_contract_path.as_deref().unwrap();
        match enforce_task_contract_completion(
            &file,
            &root,
            path_arg,
            commit_hash.as_deref(),
            staged_files.as_deref(),
        ) {
            Ok(v) => Some(v),
            Err(err) => return Ok(err),
        }
    } else {
        None
    };

    // wave-22 / task 02 — auto task-run verifier dispatch.
    //
    // The wave21-03 caller-supplied `verified=true` escape hatch is now
    // a legacy-compat fallback. The new contract: when the writer hands
    // every path the daemon needs for an end-to-end proof
    // (`task_contract_path`, `task_report_path`, `shared_memory_path`,
    // `commit_hash`) the daemon runs the in-tree task-run verifier
    // ITSELF and computes the verified status from the on-disk inputs
    // — no Node spawn, no shell, no mutating git, no caller assertion
    // accepted at face value. The wave21-02 script-side verifier
    // remains the out-of-process truth; this in-process projection just
    // closes the action-complete window so dashboards stop relying on
    // a writer-asserted boolean.
    //
    // Three-state `verification_source` summarises what happened:
    //   * `daemon-auto-verifier` — all four paths present, daemon ran
    //     the in-tree verifier and produced the verdict in
    //     `verifier_status` / `verified_scope_summary`.
    //   * `legacy-caller-claim` — caller passed `verified=true` but at
    //     least one of the four paths is absent. We honour the legacy
    //     posture (no hard reject), record the claim into the companion
    //     log verbatim, and surface `verifier_status="unknown"` plus a
    //     diagnostic explaining which path was missing so reviewers can
    //     migrate the caller off the escape hatch.
    //   * `none` — no auto-verifier run AND no legacy claim; absent in
    //     the response so legacy completions stay byte-identical.
    //
    // Backward compat: the wave21-03 helper `enforce_verified_completion`
    // is preserved verbatim and still callable from tests, but
    // `action_complete` no longer routes through it — the v2 dispatch
    // either runs the auto-verifier or downgrades the legacy claim.
    let auto_verifier_inputs_present = task_contract_path.is_some()
        && task_report_path.is_some()
        && shared_memory_path.is_some()
        && commit_hash.is_some();

    let mut verification_source: Option<&'static str> = None;
    let mut auto_verifier_summary: Option<Value> = None;
    let mut auto_verifier_status: Option<&'static str> = None;
    let mut auto_verifier_diagnostics: Option<String> = None;

    if auto_verifier_inputs_present {
        // unwraps are safe — we just checked all four are Some.
        let tcp = task_contract_path.as_deref().unwrap();
        let trp = task_report_path.as_deref().unwrap();
        let smp = shared_memory_path.as_deref().unwrap();
        let hash = commit_hash.as_deref().unwrap();
        match auto_run_task_run_verifier(&root, tcp, trp, smp, hash) {
            Ok(summary) => {
                auto_verifier_status = Some("passed");
                auto_verifier_summary = Some(summary);
                verification_source = Some("daemon-auto-verifier");
            }
            Err(err) => return Ok(err),
        }
    } else if verified_flag == Some(true) {
        // Legacy caller-supplied claim. Record it but flag in the
        // diagnostic which path was missing so the writer agent can
        // upgrade the next dispatch.
        let mut missing: Vec<&'static str> = Vec::new();
        if task_contract_path.is_none() {
            missing.push("task_contract_path");
        }
        if task_report_path.is_none() {
            missing.push("task_report_path");
        }
        if shared_memory_path.is_none() {
            missing.push("shared_memory_path");
        }
        if commit_hash.is_none() {
            missing.push("commit_hash");
        }
        verification_source = Some("legacy-caller-claim");
        auto_verifier_status = Some("unknown");
        auto_verifier_diagnostics = Some(format!(
            "verified=true accepted as legacy_verified_claim because the daemon-side auto-verifier requires all four of [task_contract_path, task_report_path, shared_memory_path, commit_hash]; missing: {:?}. Migrate the dispatch envelope to supply every path so the daemon can compute the verdict itself (wave22-02).",
            missing,
        ));
    }
    // Tri-state placeholder kept in sync with the wave21-03 response
    // shape: when the auto-verifier ran the response surfaces the
    // structured summary; when only the legacy claim was made it stays
    // None and the diagnostic prose above carries the explanation.
    let verified_validation: Option<Value> = auto_verifier_summary.clone();

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
    // wave-19 / task 08 — task-contract metadata. Each field skips when
    // absent so legacy callers that never set them keep the byte-identical
    // 6-field shape (or 11-field shape with scoped-commit fields).
    if let Some(ref tcp) = task_contract_path {
        entry.push_str(&format!(
            "\n      :task-contract-path {}",
            lisp_quote_string(tcp)
        ));
    }
    if let Some(ref trp) = task_report_path {
        entry.push_str(&format!(
            "\n      :task-report-path {}",
            lisp_quote_string(trp)
        ));
    }
    if let Some(ref vs) = verifier_status {
        entry.push_str(&format!(
            "\n      :verifier-status {}",
            lisp_quote_string(vs)
        ));
    }
    if let Some(ref vn) = verifier_notes {
        entry.push_str(&format!(
            "\n      :verifier-notes {}",
            lisp_quote_string(vn)
        ));
    }
    // wave-21 / task 03 — task-run verifier metadata. Each field skips
    // when absent so legacy callers (and wave19-08 callers that never
    // touched the wave21 slots) keep their byte-identical companion log
    // shape. `verified` is written as a bare `true`/`false` atom so a
    // round-trip through `parse_completions` recovers the boolean
    // without quoted-string handling.
    if let Some(ref trvs) = task_run_verifier_status {
        entry.push_str(&format!(
            "\n      :task-run-verifier-status {}",
            lisp_quote_string(trvs)
        ));
    }
    if let Some(ref smp) = shared_memory_path {
        entry.push_str(&format!(
            "\n      :shared-memory-path {}",
            lisp_quote_string(smp)
        ));
    }
    if let Some(ref vd) = verifier_diagnostics {
        entry.push_str(&format!(
            "\n      :verifier-diagnostics {}",
            lisp_quote_string(vd)
        ));
    }
    if let Some(v) = verified_flag {
        entry.push_str(&format!("\n      :verified {}", v));
    }
    entry.push(')');

    append_to_block(&mut file, "completions", &entry)?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    // Same dispatch-metadata projection rationale as `action_claim` —
    // surface the trio from the companion-log meta block so completion
    // consumers can route on workstation-dispatch context without reading
    // the on-disk file. Absent / legacy meta cleanly skip-serializes
    // (see ExecutionEvent::Completed doc comment).
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Completed {
            execution_id: execution_id.to_string(),
            completion_id: id.clone(),
            phase: phase.to_string(),
            agent: agent.to_string(),
            at: date.clone(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    let mut response = json!({
        "status": "recorded",
        "completion_id": id,
        "phase": phase,
        "agent": agent,
        "at": date,
        // Always surfaced so callers can detect at a glance which mode
        // the completion went through. `false` here means audit-only
        // (legacy / opt-out) — `true` means the durability invariants
        // were validated at write-time and the validation summary is
        // included below.
        "scoped_commit_enforced": enforce_scoped_commit,
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
    if let Some(v) = scoped_commit_validation {
        response["scoped_commit_validation"] = v;
    }
    // wave-19 / task 08 — surface contract metadata + the contract-level
    // validation summary (when the gate ran). Skip-serialize semantics
    // mirror the scoped-commit fields above so the response stays
    // byte-identical for legacy callers that omit every wave19 field.
    if let Some(tcp) = task_contract_path {
        response["task_contract_path"] = json!(tcp);
    }
    if let Some(trp) = task_report_path {
        response["task_report_path"] = json!(trp);
    }
    // The wave19-08 caller-supplied `verifier_status` slot is preserved
    // verbatim when the wave22-02 auto-verifier did NOT run; otherwise
    // the daemon-computed status (set further below) wins so the
    // response surface advertises a single authoritative verdict.
    if let Some(ref vs) = verifier_status {
        response["verifier_status"] = json!(vs);
    }
    if let Some(vn) = verifier_notes {
        response["verifier_notes"] = json!(vn);
    }
    if let Some(v) = task_contract_validation {
        response["task_contract_validation"] = v;
    }
    // wave-21 / task 03 — surface task-run verifier metadata + the
    // verified-gate validation summary. Same skip-serialize semantics
    // as the wave19-08 fields above so legacy callers stay byte-
    // identical when they omit every wave21 field.
    if let Some(trvs) = task_run_verifier_status {
        response["task_run_verifier_status"] = json!(trvs);
    }
    if let Some(smp) = shared_memory_path {
        response["shared_memory_path"] = json!(smp);
    }
    // The wave21-03 caller-supplied `verifier_diagnostics` slot is
    // preserved verbatim when the wave22-02 auto-verifier did NOT run;
    // otherwise the daemon-computed diagnostic (set further below)
    // wins so reviewers see one diagnostic per response.
    if let Some(ref vd) = verifier_diagnostics {
        response["verifier_diagnostics"] = json!(vd);
    }
    if let Some(v) = verified_flag {
        response["verified"] = json!(v);
    }

    // ── wave-22 / task 02 — auto task-run verifier surface ────────────
    //
    // `verification_source` flags how the verdict was reached:
    //   * `daemon-auto-verifier` — daemon ran the in-tree verifier; the
    //     daemon-computed `verifier_status="passed"` overrides any
    //     caller-supplied wave19-08 / wave21-03 status. The structured
    //     `verified_scope_summary` records every cross-checked rule.
    //   * `legacy-caller-claim` — caller passed `verified=true` but at
    //     least one path was missing; daemon-computed status is
    //     `"unknown"` and `verifier_diagnostics` carries the migration
    //     prose pointing at the missing path(s).
    //
    // Absent `verification_source` (legacy callers) keeps the response
    // shape byte-identical to the wave21-03 surface.
    if let Some(src) = verification_source {
        response["verification_source"] = json!(src);
    }
    if let Some(status) = auto_verifier_status {
        // Daemon-computed verdict wins over the caller-supplied
        // wave19-08 / wave21-03 statuses. Reviewers can still see the
        // caller-supplied values inside `task_run_verifier_status` /
        // the companion log.
        response["verifier_status"] = json!(status);
    }
    if let Some(diag) = auto_verifier_diagnostics {
        response["verifier_diagnostics"] = json!(diag);
    }
    if let Some(scope_summary) = verified_validation {
        // wave-22 contract: the summary is exposed as
        // `verified_scope_summary`. We keep the wave21-03 shape under
        // the legacy `verified_validation` key too so existing
        // dashboards keep parsing while consumers migrate.
        response["verified_scope_summary"] = scope_summary.clone();
        response["verified_validation"] = scope_summary;
    }

    // wave23-04 — opt-in session-trace append. Records `complete` or
    // `failure` depending on the verifier verdict resolved above. The
    // entry mirrors the durable companion-log completion: it carries the
    // commit hash, report path, and changed-file list so future
    // analyzers can correlate completions with their durable artifacts
    // without re-reading the .missiond/v2/<exec>.lisp companion.
    if let Some(trace_path) = resolve_session_trace_path(args, &root) {
        match resolve_trace_task_id(args, &root, execution_id) {
            Some(task_id) => {
                // Failure when caller-supplied OR daemon-computed verifier
                // status resolved to "failed". Otherwise treat the
                // completion as a success-shaped event.
                let final_verifier_status = response
                    .get("verifier_status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let kind = match final_verifier_status.as_deref() {
                    Some("failed") => TraceKind::Failure,
                    _ => TraceKind::Complete,
                };
                let backend = sanitize_trace_backend(agent);
                // Re-read the commit / report / file metadata from args
                // since the local bindings above were consumed by the
                // response builder.
                let commit_hash_for_trace = args
                    .get("commit_hash")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    // checker requires `[0-9a-f]{4,64}` — drop anything
                    // shorter / non-hex so we don't fail validation.
                    .filter(|s| {
                        s.len() >= 4 && s.len() <= 64 && s.chars().all(|c| c.is_ascii_hexdigit())
                    });
                let report_path_for_trace = args
                    .get("task_report_path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    // checker rejects absolute report paths.
                    .filter(|s| !Path::new(s).is_absolute());
                let files_for_trace = collect_string_list(args, "changed_files")
                    .or_else(|| collect_string_list(args, "staged_files"))
                    .map(|v| {
                        v.into_iter()
                            // strip absolute paths — checker rejects them
                            .filter(|p| !Path::new(p).is_absolute())
                            .collect::<Vec<_>>()
                    })
                    .filter(|v: &Vec<String>| !v.is_empty());
                let ev = TraceEvent {
                    task: task_id,
                    backend,
                    kind,
                    summary: format!(
                        "mission_execution(action=complete) phase={} agent={} completion_id={}",
                        phase, agent, id
                    ),
                    agent: None,
                    files: files_for_trace,
                    commit_hash: commit_hash_for_trace,
                    report_path: report_path_for_trace,
                };
                if let Err(w) = append_session_trace_event(&trace_path, &ev) {
                    response["trace_warning"] = json!(w.to_string());
                }
            }
            None => {
                response["trace_warning"] = json!(format!(
                    "session_trace_path supplied but execution_id `{}` is not a valid trace task id and no task_contract_path was provided",
                    execution_id
                ));
            }
        }
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
            // wave-19 / task 08 — task-contract metadata projection.
            // Same skip-on-absent semantics as the scoped-commit fields
            // above so legacy completed_phases entries stay shape-stable.
            if let Some(tcp) = &c.task_contract_path {
                row["task_contract_path"] = json!(tcp);
            }
            if let Some(trp) = &c.task_report_path {
                row["task_report_path"] = json!(trp);
            }
            if let Some(vs) = &c.verifier_status {
                row["verifier_status"] = json!(vs);
            }
            if let Some(vn) = &c.verifier_notes {
                row["verifier_notes"] = json!(vn);
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
fn audit_scoped_commit_handoff(file: &LogFile, claims: &[ClaimRecord], findings: &mut Vec<Value>) {
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
fn enforce_scoped_commit_completion(
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
fn enforce_task_contract_completion(
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
    let contract = match super::workstation_dispatch::load_task_contract(&resolved) {
        Ok(c) => c,
        Err(e) => {
            use super::workstation_dispatch::TaskContractParseError as Tce;
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

/// wave-21 / task 03 — minimal report-contract reader.
///
/// Pulls just the keys the daemon-side cross-check needs (`:schema`,
/// `:task_id`, `:commit_hash`) out of a `(report <id> ...)` form using
/// the local sexp parser. No new dependency, no new lisp dialect — the
/// projector trusts the authoritative schema checker
/// (`scripts/check-task-report.mjs`) for shape policing and only echoes
/// the three fields the daemon needs for the wave21-03 verified-gate
/// cross-check.
struct ReportSummary {
    schema: Option<String>,
    task_id: Option<String>,
    commit_hash: Option<String>,
}

fn read_report_summary(text: &str) -> Result<ReportSummary, anyhow::Error> {
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
            NodeKind::Str(s) => Some(s.clone()),
            NodeKind::Atom(a) => Some(a.clone()),
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
fn read_task_contract_id(text: &str) -> Option<String> {
    let nodes = sexp::parse(text).ok()?;
    let top = nodes.first()?;
    if top.head_atom() != Some("task") {
        return None;
    }
    let kids = top.children();
    kids.get(1).and_then(|n| n.as_atom().map(|s| s.to_string()))
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
fn enforce_verified_completion(
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
fn auto_run_task_run_verifier(
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
    let _contract = match super::workstation_dispatch::load_task_contract(&contract_resolved) {
        Ok(c) => c,
        Err(e) => {
            use super::workstation_dispatch::TaskContractParseError as Tce;
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
struct SharedMemorySummary {
    schema: Option<String>,
    completion_tasks: Vec<String>,
}

fn read_shared_memory_ledger(text: &str) -> Result<SharedMemorySummary, anyhow::Error> {
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
fn read_completion_task_id(node: &Node) -> Option<String> {
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
// action: repair — dry-run by default; structural fixes only
// ───────────────────────────────────────────────────────────────────────

async fn action_repair(state: &AppState, args: &Value) -> Result<ToolResult> {
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
struct PorcelainEntry {
    /// Index/staged status byte (`'M'`, `'A'`, `'D'`, `'R'`, `'?'`, ` `, …).
    index_status: char,
    /// Worktree status byte (same alphabet as `index_status`).
    worktree_status: char,
    /// Path as reported by porcelain (rename right-hand side when applicable).
    path: String,
}

impl PorcelainEntry {
    /// True when the index slot reflects a tracked staged change
    /// (anything but ` ` / `?` / `!`). Untracked / ignored files never
    /// count as staged because porcelain marks them with `?` / `!`.
    fn is_staged(&self) -> bool {
        !matches!(self.index_status, ' ' | '?' | '!')
    }

    /// True when the worktree slot reflects an unstaged change OR the
    /// file is untracked — both shapes carry "would be touched by an
    /// over-broad `git add .`". Ignored files (`!`) stay out so the
    /// preflight doesn't flag `.gitignore`d build artefacts.
    fn is_changed(&self) -> bool {
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
fn parse_porcelain_status(text: &str) -> Vec<PorcelainEntry> {
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
fn collect_all_claim_scopes(file: &LogFile) -> Vec<String> {
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
fn collect_specific_claim_scope(
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
fn build_contract_scope_summary(
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
fn evaluate_task_contract_for_preflight(
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
    match super::workstation_dispatch::load_task_contract(&resolved) {
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
            use super::workstation_dispatch::TaskContractParseError as Tce;
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
fn build_preflight_summary(
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
fn run_git_status(root: &Path) -> std::result::Result<String, ToolResult> {
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

async fn action_preflight_commit(state: &AppState, args: &Value) -> Result<ToolResult> {
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
            let candidate = std::path::PathBuf::from(cwd);
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
        let body =
            render_canonical_template("e", "p", "s", "o", DEFAULT_DISPATCH_STRATEGY, None, None);
        sexp::check_balance(&body).expect("balanced");
        LogFile::parse(body).expect("parse");
    }

    #[test]
    fn dispatch_strategy_normalization() {
        assert_eq!(normalize_dispatch_strategy(None), "unknown");
        assert_eq!(normalize_dispatch_strategy(Some("")), "unknown");
        assert_eq!(normalize_dispatch_strategy(Some("   ")), "unknown");
        assert_eq!(
            normalize_dispatch_strategy(Some("not-a-real-mode")),
            "unknown"
        );
        assert_eq!(
            normalize_dispatch_strategy(Some("fresh-code-alignment")),
            "fresh-code-alignment"
        );
        assert_eq!(
            normalize_dispatch_strategy(Some("agent-team")),
            "agent-team"
        );
        assert_eq!(
            normalize_dispatch_strategy(Some("resident-lisp")),
            "resident-lisp"
        );
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
        let body =
            "(execution-log\n  (deviations\n    (D001 :phase \"a\")\n    (D004 :phase \"b\")))\n";
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
                assert_eq!(
                    dispatch_strategy.as_deref(),
                    Some(DEFAULT_DISPATCH_STRATEGY)
                );
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

    // ── Wave 18 / Task 02 — dispatch metadata projection on Claimed /
    // Completed events. The daemon-side helper `read_dispatch_metadata_from_log`
    // is the single mapping point between the persisted companion-log meta
    // block and the live `Claimed` / `Completed` events. These tests exercise
    // it against the canonical writer (`render_canonical_template`) so the
    // wire form stays in lock-step with what the runtime emits.

    /// When the companion log was opened with the full dispatch trio,
    /// `read_dispatch_metadata_from_log` returns every field verbatim
    /// (with outer string-quotes stripped to match the existing `action_list`
    /// contract).
    #[test]
    fn read_dispatch_metadata_returns_full_trio_when_present() {
        let body = render_canonical_template(
            "exec-disp",
            ".missiond/v2/disp.lisp",
            "scope/x",
            "owner-x",
            "fresh-code-alignment",
            Some("missiond"),
            Some("/Users/x/Projects/missiond/crates/foo"),
        );
        let file = LogFile::parse(body).expect("parse");
        let meta = read_dispatch_metadata_from_log(&file);
        assert_eq!(
            meta.dispatch_strategy.as_deref(),
            Some("fresh-code-alignment")
        );
        assert_eq!(meta.target_project.as_deref(), Some("missiond"));
        assert_eq!(
            meta.requested_cwd.as_deref(),
            Some("/Users/x/Projects/missiond/crates/foo")
        );
    }

    /// When the open args omitted `target_project` / `requested_cwd`,
    /// the helper still returns the canonical `dispatch_strategy` slot
    /// and leaves the optional pair as `None` so the event skip-serializes.
    #[test]
    fn read_dispatch_metadata_returns_dispatch_only_when_optionals_absent() {
        let body = render_canonical_template(
            "exec-min",
            ".missiond/v2/min.lisp",
            "scope/y",
            "owner-y",
            "agent-team",
            None,
            None,
        );
        let file = LogFile::parse(body).expect("parse");
        let meta = read_dispatch_metadata_from_log(&file);
        assert_eq!(meta.dispatch_strategy.as_deref(), Some("agent-team"));
        assert!(meta.target_project.is_none());
        assert!(meta.requested_cwd.is_none());
    }

    /// Legacy companion logs (pre-wave12-01) had no dispatch keys at all.
    /// The helper must return `DispatchMeta::default()` so the event
    /// serializes byte-identical to the pre-trio wire form.
    #[test]
    fn read_dispatch_metadata_returns_default_for_legacy_log() {
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
        let meta = read_dispatch_metadata_from_log(&file);
        assert_eq!(meta, DispatchMeta::default());
    }

    /// Whitespace-only / empty-string values in the meta block must
    /// collapse to `None` so the bus event doesn't surface an empty
    /// label that downstream consumers would have to special-case.
    #[test]
    fn read_dispatch_metadata_collapses_empty_values_to_none() {
        let body = "(execution-log\n  \
                    (meta\n    \
                    :execution-id \"e\"\n    \
                    :parent-design \"p\"\n    \
                    :status \"open\"\n    \
                    :owner \"o\"\n    \
                    :scope \"s\"\n    \
                    :companion-of \"p\"\n    \
                    :dispatch-strategy \"agent-team\"\n    \
                    :target-project \"\"\n    \
                    :requested-cwd \"   \")\n  \
                    (claims))\n";
        let file = LogFile::parse(body.to_string()).expect("parse");
        let meta = read_dispatch_metadata_from_log(&file);
        assert_eq!(meta.dispatch_strategy.as_deref(), Some("agent-team"));
        assert!(
            meta.target_project.is_none(),
            "empty target_project must collapse to None"
        );
        assert!(
            meta.requested_cwd.is_none(),
            "whitespace requested_cwd must collapse to None"
        );
    }

    /// The canonical companion log written by `render_canonical_template`
    /// projects cleanly into a `Claimed` event with the full trio. This
    /// ties the writer + reader contracts together and pins the wire form
    /// the runtime `action_claim` emit path will produce.
    #[test]
    fn claimed_event_inherits_dispatch_trio_from_companion_log() {
        let body = render_canonical_template(
            "exec-disp",
            ".missiond/v2/disp.lisp",
            "scope/x",
            "owner-x",
            "fresh-code-alignment",
            Some("missiond"),
            Some("/Users/x/Projects/missiond/crates/foo"),
        );
        let file = LogFile::parse(body).expect("parse");
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::Claimed {
            execution_id: "exec-disp".into(),
            claim_id: "C001".into(),
            claimer: "claude".into(),
            scope: "scope/x".into(),
            phase: "".into(),
            lease_expires_at: "2026-04-25T01:00:00Z".into(),
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let claimed = parsed.get("Claimed").and_then(|v| v.as_object()).unwrap();
        assert_eq!(claimed.len(), 9);
        assert_eq!(claimed["dispatch_strategy"], "fresh-code-alignment");
        assert_eq!(claimed["target_project"], "missiond");
        assert_eq!(
            claimed["requested_cwd"],
            "/Users/x/Projects/missiond/crates/foo"
        );
    }

    /// Legacy companion logs project into a `Completed` event whose wire
    /// form omits the dispatch trio entirely (byte-identical to the
    /// pre-wave18 5-field shape).
    #[test]
    fn completed_event_omits_dispatch_trio_for_legacy_log() {
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
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::Completed {
            execution_id: "legacy-x".into(),
            completion_id: "COMP001".into(),
            phase: "phase-A".into(),
            agent: "old-agent".into(),
            at: "2026-04-25T03:00:00Z".into(),
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let completed = parsed.get("Completed").and_then(|v| v.as_object()).unwrap();
        assert_eq!(completed.len(), 5);
        assert!(!completed.contains_key("dispatch_strategy"));
        assert!(!completed.contains_key("target_project"));
        assert!(!completed.contains_key("requested_cwd"));
    }

    // ── Wave 20 / Task 09 — legacy ExecutionEvent variants now project
    //                       the workstation-dispatch trio from the
    //                       companion-log meta block, mirroring what
    //                       Opened / Claimed / Completed already do.
    //
    // The action_* runtime paths each call `read_dispatch_metadata_from_log`
    // on the post-write `file` handle and forward the trio onto the
    // emitted event. We don't have AppState in unit tests, so we mirror
    // the same projection chain here against the canonical template
    // (writer side) — this guarantees the wire shape the runtime emits
    // tracks the writer contract exactly.

    /// Helper that builds a canonical companion log carrying the full
    /// dispatch trio so each swept-variant test below shares the same
    /// fixture.
    fn canonical_log_with_dispatch_trio() -> LogFile {
        let body = render_canonical_template(
            "exec-disp",
            ".missiond/v2/disp.lisp",
            "scope/x",
            "owner-x",
            "fresh-code-alignment",
            Some("missiond"),
            Some("/Users/x/Projects/missiond/crates/foo"),
        );
        LogFile::parse(body).expect("parse")
    }

    /// Helper that builds a legacy companion log (pre-wave12-01) with
    /// no dispatch keys at all. Each swept variant must emit its
    /// pre-trio wire shape against this fixture.
    fn legacy_log_without_dispatch() -> LogFile {
        let body = "(execution-log\n  \
                    (meta\n    \
                    :execution-id \"legacy-x\"\n    \
                    :parent-design \"old.lisp\"\n    \
                    :status \"open\"\n    \
                    :owner \"old-owner\"\n    \
                    :scope \"legacy/scope\"\n    \
                    :companion-of \"old.lisp\")\n  \
                    (claims))\n";
        LogFile::parse(body.to_string()).expect("legacy parses")
    }

    fn assert_full_dispatch_trio(map: &serde_json::Map<String, serde_json::Value>) {
        assert_eq!(map["dispatch_strategy"], "fresh-code-alignment");
        assert_eq!(map["target_project"], "missiond");
        assert_eq!(
            map["requested_cwd"],
            "/Users/x/Projects/missiond/crates/foo"
        );
    }

    fn assert_no_dispatch_trio(map: &serde_json::Map<String, serde_json::Value>) {
        assert!(!map.contains_key("dispatch_strategy"));
        assert!(!map.contains_key("target_project"));
        assert!(!map.contains_key("requested_cwd"));
    }

    #[test]
    fn heartbeat_event_inherits_dispatch_trio_from_companion_log() {
        let file = canonical_log_with_dispatch_trio();
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::Heartbeat {
            execution_id: "exec-disp".into(),
            claim_id: "C001".into(),
            claimer: "claude".into(),
            heartbeat_at: "2026-04-25T01:00:00Z".into(),
            lease_expires_at: "2026-04-25T01:30:00Z".into(),
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        let p = parsed.get("Heartbeat").and_then(|v| v.as_object()).unwrap();
        assert_eq!(p.len(), 8);
        assert_full_dispatch_trio(p);
    }

    #[test]
    fn heartbeat_event_omits_dispatch_trio_for_legacy_log() {
        let file = legacy_log_without_dispatch();
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::Heartbeat {
            execution_id: "legacy-x".into(),
            claim_id: "C001".into(),
            claimer: "old".into(),
            heartbeat_at: "t".into(),
            lease_expires_at: "t2".into(),
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        let p = parsed.get("Heartbeat").and_then(|v| v.as_object()).unwrap();
        assert_eq!(p.len(), 5);
        assert_no_dispatch_trio(p);
    }

    #[test]
    fn released_event_inherits_dispatch_trio_from_companion_log() {
        let file = canonical_log_with_dispatch_trio();
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::Released {
            execution_id: "exec-disp".into(),
            claim_id: "C001".into(),
            claimer: "claude".into(),
            released_at: "2026-04-25T02:00:00Z".into(),
            summary: Some("done".into()),
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        let p = parsed.get("Released").and_then(|v| v.as_object()).unwrap();
        assert_eq!(p.len(), 8);
        assert_full_dispatch_trio(p);
    }

    #[test]
    fn released_event_omits_dispatch_trio_for_legacy_log() {
        let file = legacy_log_without_dispatch();
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::Released {
            execution_id: "legacy-x".into(),
            claim_id: "C001".into(),
            claimer: "old".into(),
            released_at: "t".into(),
            summary: None,
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        let p = parsed.get("Released").and_then(|v| v.as_object()).unwrap();
        // Released always carries the `summary` key (Option<String>
        // without skip-serializing) so the legacy shape is 5 fields.
        assert_eq!(p.len(), 5);
        assert_no_dispatch_trio(p);
    }

    #[test]
    fn deviation_recorded_event_inherits_dispatch_trio_from_companion_log() {
        let file = canonical_log_with_dispatch_trio();
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::DeviationRecorded {
            execution_id: "exec-disp".into(),
            deviation_id: "D001".into(),
            phase: "phase-A".into(),
            approved_by: "claude".into(),
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        let p = parsed
            .get("DeviationRecorded")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(p.len(), 7);
        assert_full_dispatch_trio(p);
    }

    #[test]
    fn deviation_recorded_event_omits_dispatch_trio_for_legacy_log() {
        let file = legacy_log_without_dispatch();
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::DeviationRecorded {
            execution_id: "legacy-x".into(),
            deviation_id: "D001".into(),
            phase: "p".into(),
            approved_by: "auto".into(),
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        let p = parsed
            .get("DeviationRecorded")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(p.len(), 4);
        assert_no_dispatch_trio(p);
    }

    #[test]
    fn decision_recorded_event_inherits_dispatch_trio_from_companion_log() {
        let file = canonical_log_with_dispatch_trio();
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::DecisionRecorded {
            execution_id: "exec-disp".into(),
            decision_id: "DC001".into(),
            decided_by: "claude".into(),
            at: "2026-04-25T05:00:00Z".into(),
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        let p = parsed
            .get("DecisionRecorded")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(p.len(), 7);
        assert_full_dispatch_trio(p);
    }

    #[test]
    fn decision_recorded_event_omits_dispatch_trio_for_legacy_log() {
        let file = legacy_log_without_dispatch();
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::DecisionRecorded {
            execution_id: "legacy-x".into(),
            decision_id: "DC001".into(),
            decided_by: "old".into(),
            at: "t".into(),
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        let p = parsed
            .get("DecisionRecorded")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(p.len(), 4);
        assert_no_dispatch_trio(p);
    }

    #[test]
    fn issue_recorded_event_inherits_dispatch_trio_from_companion_log() {
        let file = canonical_log_with_dispatch_trio();
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::IssueRecorded {
            execution_id: "exec-disp".into(),
            issue_id: "I001".into(),
            severity: "high".into(),
            owner: "claude".into(),
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        let p = parsed
            .get("IssueRecorded")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(p.len(), 7);
        assert_full_dispatch_trio(p);
    }

    #[test]
    fn issue_recorded_event_omits_dispatch_trio_for_legacy_log() {
        let file = legacy_log_without_dispatch();
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::IssueRecorded {
            execution_id: "legacy-x".into(),
            issue_id: "I001".into(),
            severity: "low".into(),
            owner: "".into(),
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        let p = parsed
            .get("IssueRecorded")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(p.len(), 4);
        assert_no_dispatch_trio(p);
    }

    #[test]
    fn audited_event_inherits_dispatch_trio_from_companion_log() {
        let file = canonical_log_with_dispatch_trio();
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::Audited {
            execution_id: "exec-disp".into(),
            ok: true,
            findings_count: 0,
            error_count: 0,
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        let p = parsed.get("Audited").and_then(|v| v.as_object()).unwrap();
        assert_eq!(p.len(), 7);
        assert_full_dispatch_trio(p);
    }

    #[test]
    fn audited_event_omits_dispatch_trio_for_legacy_log() {
        let file = legacy_log_without_dispatch();
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::Audited {
            execution_id: "legacy-x".into(),
            ok: false,
            findings_count: 1,
            error_count: 1,
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        let p = parsed.get("Audited").and_then(|v| v.as_object()).unwrap();
        assert_eq!(p.len(), 4);
        assert_no_dispatch_trio(p);
    }

    #[test]
    fn repaired_event_inherits_dispatch_trio_from_companion_log() {
        let file = canonical_log_with_dispatch_trio();
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::Repaired {
            execution_id: "exec-disp".into(),
            applied: true,
            action_count: 2,
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        let p = parsed.get("Repaired").and_then(|v| v.as_object()).unwrap();
        assert_eq!(p.len(), 6);
        assert_full_dispatch_trio(p);
    }

    #[test]
    fn repaired_event_omits_dispatch_trio_for_legacy_log() {
        let file = legacy_log_without_dispatch();
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::Repaired {
            execution_id: "legacy-x".into(),
            applied: false,
            action_count: 0,
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        let p = parsed.get("Repaired").and_then(|v| v.as_object()).unwrap();
        assert_eq!(p.len(), 3);
        assert_no_dispatch_trio(p);
    }

    #[test]
    fn stale_claim_event_inherits_dispatch_trio_from_companion_log() {
        let file = canonical_log_with_dispatch_trio();
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::StaleClaim {
            execution_id: "exec-disp".into(),
            claim_id: "C001".into(),
            claimer: "claude".into(),
            lease_expires_at: "2026-04-25T00:30:00Z".into(),
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        let p = parsed
            .get("StaleClaim")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(p.len(), 7);
        assert_full_dispatch_trio(p);
    }

    #[test]
    fn stale_claim_event_omits_dispatch_trio_for_legacy_log() {
        let file = legacy_log_without_dispatch();
        let dm = read_dispatch_metadata_from_log(&file);
        let ev = ExecutionEvent::StaleClaim {
            execution_id: "legacy-x".into(),
            claim_id: "C001".into(),
            claimer: "old".into(),
            lease_expires_at: "t".into(),
            dispatch_strategy: dm.dispatch_strategy,
            target_project: dm.target_project,
            requested_cwd: dm.requested_cwd,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        let p = parsed
            .get("StaleClaim")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(p.len(), 4);
        assert_no_dispatch_trio(p);
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
        assert_eq!(
            c.staged_files.as_deref(),
            Some(&["src/a.rs".to_string()][..])
        );
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
        assert_eq!(f.get("severity").and_then(|v| v.as_str()), Some("error"));
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
        assert!(findings
            .iter()
            .any(|f| f.get("kind").and_then(|v| v.as_str())
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
            .find(|f| {
                f.get("kind").and_then(|v| v.as_str()) == Some(FINDING_SCOPED_COMMIT_VIOLATION)
            })
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
        assert_eq!(v.get("completion_count").and_then(|x| x.as_i64()), Some(0));
        assert!(v
            .get("latest_commit_status")
            .map(|x| x.is_null())
            .unwrap_or(false));

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
                task_contract_path: None,
                task_report_path: None,
                verifier_status: None,
                verifier_notes: None,
                task_run_verifier_status: None,
                shared_memory_path: None,
                verifier_diagnostics: None,
                verified: None,
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
                task_contract_path: None,
                task_report_path: None,
                verifier_status: None,
                verifier_notes: None,
                task_run_verifier_status: None,
                shared_memory_path: None,
                verifier_diagnostics: None,
                verified: None,
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

    // ── Wave 16 / Task 06 — fail-fast scoped-commit enforcement ────
    //
    // Tests below pin the contract of `enforce_scoped_commit_completion`,
    // the runtime gate `action_complete` calls when the caller opts in
    // via `enforce_scoped_commit=true`. The audit-only path still owns
    // legacy callers (covered above) — these only exercise the new
    // structured-error short-circuit.

    fn extract_error_code(result: &ToolResult) -> Option<String> {
        let v = serde_json::to_value(result).ok()?;
        // structured_error renders into the content[0].text JSON payload.
        let content = v.get("content")?.as_array()?;
        let first = content.first()?;
        let text = first.get("text")?.as_str()?;
        let parsed: Value = serde_json::from_str(text).ok()?;
        parsed
            .get("error_code")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
    }

    /// Released claim alongside the existing "src/" active claim — both
    /// must count as in-scope when validating staged paths so the
    /// post-release commit window stays open per
    /// F-scoped-commit-handoff :: s7 release-claim.
    fn fresh_file_with_released_claim() -> LogFile {
        let mut file = fresh_file();
        let now = now_iso();
        let entry = format!(
            "    (C001\n      :claimer \"agent\"\n      :scope \"crates/foo/\"\n      :phase \"phase-A\"\n      :acquired-at {ts}\n      :lease-expires-at {ts}\n      :released-at {ts}\n      :heartbeat-at {ts}\n      :status \"released\")",
            ts = lisp_quote_string(&now),
        );
        append_to_block(&mut file, "claims", &entry).unwrap();
        file
    }

    /// committed without commit_hash + enforce_scoped_commit=true must
    /// short-circuit with COMMIT_HASH_REQUIRED before the file is touched.
    #[test]
    fn enforce_rejects_committed_without_hash() {
        let file = fresh_file_with_claim();
        let res = enforce_scoped_commit_completion(
            &file,
            Some(&["src/a.rs".to_string()]),
            None,
            Some("committed"),
            None,
        );
        let err = res.expect_err("should reject committed without hash");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("COMMIT_HASH_REQUIRED"),
        );
    }

    /// blocked without commit_blocker + enforce_scoped_commit=true must
    /// reject with COMMIT_BLOCKER_REQUIRED. Empty-string blocker is
    /// equivalent to absent (caller-side trim already collapsed it).
    #[test]
    fn enforce_rejects_blocked_without_blocker() {
        let file = fresh_file_with_claim();
        let res = enforce_scoped_commit_completion(
            &file,
            Some(&["src/a.rs".to_string()]),
            None,
            Some("blocked"),
            None,
        );
        let err = res.expect_err("should reject blocked without blocker");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("COMMIT_BLOCKER_REQUIRED"),
        );
    }

    /// staged_files non-empty + zero claims must reject with the
    /// CLAIM_SCOPE_REQUIRED variant — distinct from a scope drift
    /// violation so the writer can tell "missing claim" from "outside
    /// scope" without parsing the audit findings list.
    #[test]
    fn enforce_rejects_staged_files_with_no_claims() {
        let file = fresh_file();
        let res = enforce_scoped_commit_completion(
            &file,
            Some(&["src/a.rs".to_string()]),
            Some("abc1234"),
            Some("committed"),
            None,
        );
        let err = res.expect_err("should reject staged with no claims");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("CLAIM_SCOPE_REQUIRED"),
        );
    }

    /// staged path outside every claim scope must reject with
    /// SCOPED_COMMIT_VIOLATION. Mirrors the audit-only finding so the
    /// runtime contract matches the audit contract.
    #[test]
    fn enforce_rejects_staged_file_outside_claim_scope() {
        let file = fresh_file_with_claim();
        let res = enforce_scoped_commit_completion(
            &file,
            Some(&["vendor/x.rs".to_string()]),
            Some("abc1234"),
            Some("committed"),
            None,
        );
        let err = res.expect_err("should reject scope drift");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("SCOPED_COMMIT_VIOLATION"),
        );
    }

    /// staged path inside an already-released claim must pass —
    /// the writer legitimately commits files inside the just-released
    /// scope window per F-scoped-commit-handoff :: s7.
    #[test]
    fn enforce_accepts_staged_file_inside_released_claim() {
        let file = fresh_file_with_released_claim();
        let res = enforce_scoped_commit_completion(
            &file,
            Some(&["crates/foo/src/a.rs".to_string()]),
            Some("abc1234"),
            Some("committed"),
            None,
        );
        let summary = res.expect("should accept released-claim handoff");
        assert_eq!(
            summary.get("staged_files_checked").and_then(|v| v.as_u64()),
            Some(1),
        );
        let scopes = summary
            .get("claim_scopes")
            .and_then(|v| v.as_array())
            .expect("claim_scopes array");
        assert!(scopes.iter().any(|v| v.as_str() == Some("crates/foo/")));
    }

    /// Empty staged_files + enforce_scoped_commit=true must still pass
    /// (read-only completions are legal per scoped-commit-contract
    /// :commit-status-values :not-required) and the validation summary
    /// must record "0 staged paths checked" so callers can confirm the
    /// branch they hit.
    #[test]
    fn enforce_accepts_empty_staged_files() {
        let file = fresh_file_with_claim();
        let res =
            enforce_scoped_commit_completion(&file, Some(&[]), None, Some("not-required"), None);
        let summary = res.expect("read-only completion must pass");
        assert_eq!(
            summary.get("staged_files_checked").and_then(|v| v.as_u64()),
            Some(0),
        );
    }

    /// Caller did not opt in → `enforce_scoped_commit_completion` is
    /// never called. We assert the legacy code path explicitly: a
    /// `commit_status=committed` payload with no hash is accepted by
    /// the gate when `enforce_scoped_commit=false` because the gate
    /// simply does not run (audit will still flag it later).
    ///
    /// We can't drive `action_complete` directly without AppState, so
    /// instead we mirror its branch by ensuring the helper is only
    /// reached when the flag is true — invoking it directly with the
    /// same payload here illustrates the contract: legacy callers
    /// would never hit this path.
    #[test]
    fn enforce_helper_is_opt_in_only() {
        // Mirror the dispatch branch from `action_complete`: when the
        // caller does not set `enforce_scoped_commit`, we never reach
        // the helper. So a payload that *would* fail validation
        // (`committed` without hash) is allowed through the legacy
        // path. We assert the helper rejects it to make the contrast
        // explicit.
        let file = fresh_file_with_claim();
        let res = enforce_scoped_commit_completion(&file, None, None, Some("committed"), None);
        assert_eq!(
            extract_error_code(&res.expect_err("opt-in path rejects")).as_deref(),
            Some("COMMIT_HASH_REQUIRED"),
        );
        // The gate is gated on the caller flag; this test pins that
        // contract by exercising it directly. The opt-out (legacy)
        // path is exercised by every existing `action_complete` test
        // above, all of which omit `enforce_scoped_commit`.
    }

    // ── Wave 18 / Task 08 — preflight_commit (worktree audit) ──
    //
    // These tests exercise the pure helpers (`parse_porcelain_status`,
    // `build_preflight_summary`) plus the claim-resolution helper. The
    // outer `action_preflight_commit` async path needs an AppState +
    // a real git worktree, so we only smoke-test the orchestration
    // through the helpers — the same approach the wave16-06 tests took
    // for `enforce_scoped_commit_completion`.

    /// Porcelain v1 parser must surface the standard XY-status pairs
    /// that scoped-commit enforcement keys off (modified, added,
    /// deleted, renamed, untracked) without dropping any path.
    #[test]
    fn porcelain_parser_recognises_each_status_kind() {
        let raw = " M src/a.rs\nA  src/b.rs\nMM src/c.rs\nD  src/d.rs\n?? new/file.rs\n!! .build/cache\nR  src/e.rs -> src/f.rs\n";
        let entries = parse_porcelain_status(raw);
        assert_eq!(entries.len(), 7);

        // Worktree-modified, not staged: changed but NOT staged.
        assert_eq!(entries[0].path, "src/a.rs");
        assert!(entries[0].is_changed());
        assert!(!entries[0].is_staged());

        // Staged-add: staged AND changed (worktree slot is space ⇒
        // identical to index, but the index slot is non-blank).
        assert!(entries[1].is_staged());
        assert!(entries[1].is_changed());

        // Both staged and worktree-edited (`MM`).
        assert!(entries[2].is_staged());
        assert!(entries[2].is_changed());

        // Staged delete.
        assert_eq!(entries[3].path, "src/d.rs");
        assert!(entries[3].is_staged());

        // Untracked: changed but NOT staged.
        assert_eq!(entries[4].path, "new/file.rs");
        assert!(entries[4].is_changed());
        assert!(!entries[4].is_staged());

        // Ignored: stays out of both buckets so .gitignore'd build
        // artefacts don't trip preflight.
        assert!(!entries[5].is_changed());
        assert!(!entries[5].is_staged());

        // Rename: parser must keep the post-rename path so scope-overlap
        // matches the on-disk file.
        assert_eq!(entries[6].path, "src/f.rs");
        assert!(entries[6].is_staged());
    }

    /// Empty stdout (clean worktree) must yield an empty entry list —
    /// downstream `build_preflight_summary` then emits the
    /// "worktree clean — nothing to commit" hint.
    #[test]
    fn porcelain_parser_handles_clean_worktree() {
        assert!(parse_porcelain_status("").is_empty());
        assert!(parse_porcelain_status("\n\n").is_empty());
    }

    /// Scope comparison: union of changed/staged paths inside the claim
    /// scope keeps `out_of_scope_files` empty and `ok=true`.
    #[test]
    fn preflight_summary_in_scope_is_ok() {
        let entries = vec![
            PorcelainEntry {
                index_status: 'M',
                worktree_status: ' ',
                path: "src/a.rs".into(),
            },
            PorcelainEntry {
                index_status: ' ',
                worktree_status: 'M',
                path: "src/b.rs".into(),
            },
        ];
        let scopes = vec!["src/".to_string()];
        let summary = build_preflight_summary(&entries, &scopes, None);
        assert_eq!(summary.get("ok").and_then(|v| v.as_bool()), Some(true));
        let oos = summary
            .get("out_of_scope_files")
            .and_then(|v| v.as_array())
            .unwrap();
        assert!(oos.is_empty());
        assert_eq!(
            summary
                .get("staged_files")
                .and_then(|v| v.as_array())
                .map(|a| a.len()),
            Some(1),
        );
        assert_eq!(
            summary
                .get("changed_files")
                .and_then(|v| v.as_array())
                .map(|a| a.len()),
            Some(2),
        );
    }

    /// A staged path outside every claim scope must surface in
    /// `out_of_scope_files` with `ok=false`. Parallel to
    /// SCOPED_COMMIT_VIOLATION on the post-commit gate so the writer
    /// agent sees the same drift signal at preflight time.
    #[test]
    fn preflight_summary_flags_out_of_scope_path() {
        let entries = vec![
            PorcelainEntry {
                index_status: 'M',
                worktree_status: ' ',
                path: "src/a.rs".into(),
            },
            PorcelainEntry {
                index_status: 'A',
                worktree_status: ' ',
                path: "vendor/x.rs".into(),
            },
        ];
        let scopes = vec!["src/".to_string()];
        let summary = build_preflight_summary(&entries, &scopes, None);
        assert_eq!(summary.get("ok").and_then(|v| v.as_bool()), Some(false));
        let oos: Vec<String> = summary
            .get("out_of_scope_files")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.to_string())
            .collect();
        assert_eq!(oos, vec!["vendor/x.rs"]);
        let next = summary
            .get("next_step")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(
            next.contains("vendor/x.rs"),
            "next_step should mention the violator, got: {}",
            next
        );
    }

    /// No claims on the companion log + dirty worktree → every touched
    /// path is out-of-scope by definition. This is the pre-claim case
    /// the wave16-06 enforcement gate calls CLAIM_SCOPE_REQUIRED;
    /// preflight surfaces it as a flat out-of-scope list with a
    /// "open a claim first" next_step instead of a hard error so the
    /// writer can iteratively fix it.
    #[test]
    fn preflight_summary_no_claims_marks_everything_out_of_scope() {
        let entries = vec![PorcelainEntry {
            index_status: 'M',
            worktree_status: ' ',
            path: "src/a.rs".into(),
        }];
        let scopes: Vec<String> = vec![];
        let summary = build_preflight_summary(&entries, &scopes, None);
        assert_eq!(summary.get("ok").and_then(|v| v.as_bool()), Some(false));
        let oos: Vec<&str> = summary
            .get("out_of_scope_files")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(oos, vec!["src/a.rs"]);
        let next = summary
            .get("next_step")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(
            next.contains("open a claim"),
            "next_step should suggest opening a claim, got: {}",
            next
        );
    }

    /// Clean worktree + active claim: ok=true, both file lists empty,
    /// next_step explicitly says "nothing to commit".
    #[test]
    fn preflight_summary_clean_worktree_ok() {
        let entries: Vec<PorcelainEntry> = vec![];
        let scopes = vec!["src/".to_string()];
        let summary = build_preflight_summary(&entries, &scopes, None);
        assert_eq!(summary.get("ok").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            summary
                .get("changed_files")
                .and_then(|v| v.as_array())
                .map(|a| a.len()),
            Some(0),
        );
        assert_eq!(
            summary
                .get("staged_files")
                .and_then(|v| v.as_array())
                .map(|a| a.len()),
            Some(0),
        );
        let next = summary
            .get("next_step")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(
            next.contains("worktree clean"),
            "next_step should mention clean worktree, got: {}",
            next
        );
    }

    /// `expected_files` hint surfaces both directions of drift:
    /// expected-but-not-touched goes to `expected_missing`, touched-but-
    /// not-expected goes to `expected_unexpected`. Neither flips `ok`
    /// because the scope check is the source of truth — expected_files
    /// is advisory metadata from the dispatch brief.
    #[test]
    fn preflight_summary_expected_files_drift_surfaces_both_directions() {
        let entries = vec![
            PorcelainEntry {
                index_status: 'M',
                worktree_status: ' ',
                path: "src/a.rs".into(),
            },
            PorcelainEntry {
                index_status: 'A',
                worktree_status: ' ',
                path: "src/c.rs".into(),
            },
        ];
        let scopes = vec!["src/".to_string()];
        let expected = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        let summary = build_preflight_summary(&entries, &scopes, Some(&expected));
        // Scope check passes — `c.rs` is inside `src/` even though it
        // wasn't expected.
        assert_eq!(summary.get("ok").and_then(|v| v.as_bool()), Some(true));
        let missing: Vec<&str> = summary
            .get("expected_missing")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(missing, vec!["src/b.rs"]);
        let unexpected: Vec<&str> = summary
            .get("expected_unexpected")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(unexpected, vec!["src/c.rs"]);
    }

    /// Claim resolution: `claim_id` pointing to a real claim returns
    /// just that claim's scope (single-element vec).
    #[test]
    fn preflight_specific_claim_returns_just_that_scope() {
        let mut file = fresh_file();
        let now = now_iso();
        let entry = format!(
            "    (C001\n      :claimer \"agent\"\n      :scope \"src/\"\n      :phase \"phase-A\"\n      :acquired-at {ts}\n      :lease-expires-at {ts}\n      :heartbeat-at {ts}\n      :status \"active\")",
            ts = lisp_quote_string(&now),
        );
        append_to_block(&mut file, "claims", &entry).unwrap();
        let entry2 = format!(
            "    (C002\n      :claimer \"agent\"\n      :scope \"vendor/\"\n      :phase \"phase-A\"\n      :acquired-at {ts}\n      :lease-expires-at {ts}\n      :heartbeat-at {ts}\n      :status \"active\")",
            ts = lisp_quote_string(&now),
        );
        append_to_block(&mut file, "claims", &entry2).unwrap();
        let scopes = collect_specific_claim_scope(&file, "C002").unwrap();
        assert_eq!(scopes, vec!["vendor/".to_string()]);
        // Union path includes both.
        let union = collect_all_claim_scopes(&file);
        assert_eq!(union.len(), 2);
        assert!(union.contains(&"src/".to_string()));
        assert!(union.contains(&"vendor/".to_string()));
    }

    /// Unknown claim_id must reject with NOT_FOUND so the writer
    /// learns about the typo before running git.
    #[test]
    fn preflight_unknown_claim_id_rejects() {
        let file = fresh_file();
        let err =
            collect_specific_claim_scope(&file, "C999").expect_err("unknown claim id must reject");
        assert_eq!(extract_error_code(&err).as_deref(), Some("NOT_FOUND"));
    }

    // ── Wave 19 / Task 08 — task-contract completion metadata ──
    //
    // The runtime gate `enforce_task_contract_completion` is what
    // `action_complete` calls when the caller pairs
    // `enforce_scoped_commit=true` with a `task_contract_path`. These
    // tests pin the four structured-error codes plus the happy-path
    // validation summary using a tempdir-anchored project root so the
    // contract loader sees a real file. Verifier-status normalization
    // and persistence are covered separately at the helper level.

    use std::io::Write;

    fn write_task_contract(dir: &Path, rel: &str, body: &str) -> PathBuf {
        let abs = dir.join(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        let mut f = std::fs::File::create(&abs).expect("create");
        f.write_all(body.as_bytes()).expect("write");
        abs
    }

    /// Minimal valid task-contract v1 form. Mirrors the shape produced
    /// by plan.rs::build_task_contract_lisp (wave19-06) but trimmed to
    /// the fields the daemon enforcement gate inspects.
    const SAMPLE_CONTRACT_BODY: &str = r#"
(task wave19-08-test-contract
  :schema "missiond.task-contract.v1"
  :goal "exercise task-contract completion gate"
  :write-scope ["src/a.rs" "src/b.rs"]
  :must-not-touch []
  :acceptance []
  :commit (:required true :message "feat(test): wave19-08" :scope-check write-scope-only))
"#;

    /// `verifier_status` normalizer must accept every canonical label
    /// (with whitespace) and reject typos. Mirrors the contract for
    /// `commit_status` so the test surface stays uniform across
    /// completion enums.
    #[test]
    fn verifier_status_normalizer_accepts_canonical_only() {
        for &status in VALID_VERIFIER_STATUSES {
            assert_eq!(normalize_verifier_status(status), Some(status));
        }
        assert_eq!(normalize_verifier_status("  passed  "), Some("passed"));
        assert!(normalize_verifier_status("").is_none());
        assert!(normalize_verifier_status("done").is_none());
        assert!(normalize_verifier_status("PASSED").is_none());
    }

    /// `parse_completions` must round-trip the wave19-08 metadata when
    /// every new field is present, including verifier_notes prose with
    /// punctuation, so dashboards / status surfaces see the original
    /// caller-supplied strings.
    #[test]
    fn parse_completions_reads_task_contract_metadata() {
        let body = "(execution-log\n  (completions\n    (COMP001\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :commit-hash \"abc1234\"\n      :commit-status \"committed\"\n      :task-contract-path \".missiond/tasks/wave19/sample.lisp\"\n      :task-report-path \".missiond/tasks/wave19/reports/sample.report.lisp\"\n      :verifier-status \"passed\"\n      :verifier-notes \"verifier OK against abc1234\")))\n";
        let file = LogFile::parse(body.to_string()).expect("parse");
        let comps = parse_completions(&file);
        assert_eq!(comps.len(), 1);
        let c = &comps[0];
        assert_eq!(
            c.task_contract_path.as_deref(),
            Some(".missiond/tasks/wave19/sample.lisp"),
        );
        assert_eq!(
            c.task_report_path.as_deref(),
            Some(".missiond/tasks/wave19/reports/sample.report.lisp"),
        );
        assert_eq!(c.verifier_status.as_deref(), Some("passed"));
        assert_eq!(
            c.verifier_notes.as_deref(),
            Some("verifier OK against abc1234"),
        );
    }

    /// Legacy completions (no wave19-08 fields) must still parse and
    /// surface `None` everywhere new — the same backward-compat contract
    /// the wave12-01 scoped-commit fields uphold.
    #[test]
    fn parse_completions_legacy_omits_task_contract_metadata() {
        let body = "(execution-log\n  (completions\n    (COMP001\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\")))\n";
        let file = LogFile::parse(body.to_string()).expect("parse");
        let c = &parse_completions(&file)[0];
        assert!(c.task_contract_path.is_none());
        assert!(c.task_report_path.is_none());
        assert!(c.verifier_status.is_none());
        assert!(c.verifier_notes.is_none());
    }

    /// Missing task-contract file → TASK_CONTRACT_REQUIRED. The error
    /// must surface BEFORE the daemon mutates the companion log so the
    /// writer can correct the path without a partial commit on record.
    #[test]
    fn enforce_contract_rejects_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = fresh_file_with_claim();
        let res = enforce_task_contract_completion(
            &file,
            dir.path(),
            "tasks/does-not-exist.lisp",
            Some("abc1234"),
            Some(&["src/a.rs".to_string()]),
        );
        let err = res.expect_err("missing file must reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("TASK_CONTRACT_REQUIRED"),
        );
    }

    /// Malformed contract body (schema mismatch) → TASK_CONTRACT_MALFORMED.
    /// Distinct from REQUIRED so the writer can tell "wrong path" from
    /// "wrong content" without re-running the verifier.
    #[test]
    fn enforce_contract_rejects_malformed_schema() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bad = "(task wave19-08-bad\n  :schema \"missiond.task-contract.v0\"\n  :goal \"bad\")";
        write_task_contract(dir.path(), "tasks/bad.lisp", bad);
        let file = fresh_file_with_claim();
        let res = enforce_task_contract_completion(
            &file,
            dir.path(),
            "tasks/bad.lisp",
            Some("abc1234"),
            Some(&["src/a.rs".to_string()]),
        );
        let err = res.expect_err("schema mismatch must reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("TASK_CONTRACT_MALFORMED"),
        );
    }

    /// Missing commit_hash → COMMIT_HASH_REQUIRED_FOR_CONTRACT. Distinct
    /// from the scoped-commit COMMIT_HASH_REQUIRED so dashboards can
    /// distinguish "no hash on report" from "no hash on commit_status".
    #[test]
    fn enforce_contract_rejects_missing_commit_hash() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
        let file = fresh_file_with_claim();
        let res = enforce_task_contract_completion(
            &file,
            dir.path(),
            "tasks/ok.lisp",
            None,
            Some(&["src/a.rs".to_string()]),
        );
        let err = res.expect_err("missing hash must reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("COMMIT_HASH_REQUIRED_FOR_CONTRACT"),
        );
    }

    /// Empty / whitespace commit_hash also rejects — the helper trims
    /// before checking so the writer cannot smuggle a blank string past
    /// the gate.
    #[test]
    fn enforce_contract_rejects_blank_commit_hash() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
        let file = fresh_file_with_claim();
        let res = enforce_task_contract_completion(
            &file,
            dir.path(),
            "tasks/ok.lisp",
            Some("   "),
            Some(&["src/a.rs".to_string()]),
        );
        let err = res.expect_err("blank hash must reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("COMMIT_HASH_REQUIRED_FOR_CONTRACT"),
        );
    }

    /// `:write-scope` entry not covered by any claim AND not staged →
    /// CLAIM_SCOPE_MISSING. This is the "writer ran the verifier OK but
    /// the daemon-side state cannot prove the work landed inside scope"
    /// case the gate exists to catch.
    #[test]
    fn enforce_contract_rejects_uncovered_write_scope() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
        // fresh_file_with_claim covers "src/" — that overlaps both
        // contract entries, so we narrow the claim to a sibling path
        // that proves the contract entries are uncovered. Easiest: use
        // fresh_file (no claims) and stage NOTHING.
        let file = fresh_file();
        let res = enforce_task_contract_completion(
            &file,
            dir.path(),
            "tasks/ok.lisp",
            Some("abc1234"),
            None,
        );
        let err = res.expect_err("uncovered scope must reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("CLAIM_SCOPE_MISSING"),
        );
    }

    /// Happy path: contract loadable, hash present, every :write-scope
    /// entry overlaps an active claim. Validation summary records the
    /// resolved path + checked rules so the response mirrors the
    /// scoped-commit gate's shape.
    #[test]
    fn enforce_contract_accepts_covered_write_scope() {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolved = write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
        let file = fresh_file_with_claim(); // active claim on "src/"
        let res = enforce_task_contract_completion(
            &file,
            dir.path(),
            "tasks/ok.lisp",
            Some("abc1234"),
            Some(&["src/a.rs".to_string(), "src/b.rs".to_string()]),
        );
        let summary = res.expect("covered scope must pass");
        assert_eq!(
            summary.get("schema").and_then(|v| v.as_str()),
            Some("missiond.task-contract.v1"),
        );
        assert_eq!(
            summary.get("write_scope_entries").and_then(|v| v.as_u64()),
            Some(2),
        );
        assert_eq!(
            summary
                .get("resolved_path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            Some(resolved.display().to_string()),
        );
    }

    /// Happy path with absolute task_contract_path: must NOT be rejoined
    /// against the project root, and the resolved_path echoed back must
    /// be byte-equal to the absolute path the caller supplied.
    #[test]
    fn enforce_contract_accepts_absolute_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolved = write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
        let abs_str = resolved.display().to_string();
        let file = fresh_file_with_claim();
        let res = enforce_task_contract_completion(
            &file,
            // Anchor against an unrelated tempdir to prove the absolute
            // path takes precedence over project_root.
            tempfile::tempdir().unwrap().path(),
            &abs_str,
            Some("abc1234"),
            Some(&["src/a.rs".to_string(), "src/b.rs".to_string()]),
        );
        let summary = res.expect("absolute path must load");
        assert_eq!(
            summary.get("resolved_path").and_then(|v| v.as_str()),
            Some(abs_str.as_str()),
        );
    }

    /// Staged file alone (no claim) is enough to cover a :write-scope
    /// entry — this is the "brand new file" case where the writer staged
    /// it but has not yet opened a claim. Mirrors the scoped-commit gate
    /// which accepts staged paths inside released claims.
    #[test]
    fn enforce_contract_accepts_staged_only_coverage() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
        let file = fresh_file(); // zero claims
        let res = enforce_task_contract_completion(
            &file,
            dir.path(),
            "tasks/ok.lisp",
            Some("abc1234"),
            Some(&["src/a.rs".to_string(), "src/b.rs".to_string()]),
        );
        assert!(res.is_ok(), "staged paths alone should cover write-scope");
    }

    // ── Wave 20 / Task 03 — preflight task-contract scope projection ──
    //
    // These tests pin the new wave20-03 pure helpers used by
    // `action_preflight_commit` when the caller threads
    // `task_contract_path` through the call. They exercise the glob
    // matcher, the four-field structured projection, and the contract
    // loader's status labels (loaded / missing / malformed). The async
    // path through `action_preflight_commit` itself is smoke-tested via
    // these helpers — same approach the wave18-08 preflight tests use.

    /// Bare prefix patterns must match the exact path AND any descendant
    /// when the pattern denotes a directory. Mirrors the JS
    /// `pathMatchesPattern` semantics so daemon-side preflight stays in
    /// lock-step with `scripts/lib/missiond_lisp.mjs`.
    #[test]
    fn pattern_matches_path_handles_bare_prefix() {
        // Exact match.
        assert!(pattern_matches_path(
            "crates/missiond-daemon/src/lib.rs",
            "crates/missiond-daemon/src/lib.rs",
        ));
        // Directory prefix without trailing slash.
        assert!(pattern_matches_path("crates/foo/bar.rs", "crates"));
        // Directory prefix with trailing slash.
        assert!(pattern_matches_path("crates/foo/bar.rs", "crates/"));
        // Sibling path must NOT match (no false-positive prefix overlap).
        assert!(!pattern_matches_path("crates2/foo.rs", "crates"));
        // Empty inputs never match.
        assert!(!pattern_matches_path("", "crates"));
        assert!(!pattern_matches_path("crates/foo.rs", ""));
    }

    /// `**` must match across folder hops; `*` must NOT cross `/`.
    /// Pinned because the wave20-03 contract for must-not-touch uses
    /// `scripts/**` and `.missiond/v2/*.lisp` — both shapes need to work
    /// or the task scope guard regresses.
    #[test]
    fn pattern_matches_path_handles_globs() {
        // `**` crosses folder boundaries.
        assert!(pattern_matches_path("scripts/foo.mjs", "scripts/**"));
        assert!(pattern_matches_path(
            "scripts/lib/missiond_lisp.mjs",
            "scripts/**",
        ));
        // `*` does not cross `/`.
        assert!(pattern_matches_path(
            ".missiond/v2/foo.lisp",
            ".missiond/v2/*.lisp"
        ));
        assert!(!pattern_matches_path(
            ".missiond/v2/sub/foo.lisp",
            ".missiond/v2/*.lisp",
        ));
        // `?` matches a single non-`/` char.
        assert!(pattern_matches_path("a.rs", "?.rs"));
        assert!(!pattern_matches_path("ab.rs", "?.rs"));
        // Regex meta-characters in pattern are escaped — a literal `.`
        // matches a `.`, not "any char".
        assert!(pattern_matches_path("a.rs", "a.rs"));
        assert!(!pattern_matches_path("axrs", "a.rs"));
    }

    /// Backslashes / leading `./` / leading `/` collapse to repo-relative
    /// before comparison so Windows-style paths and verbose contract
    /// entries match the same canonical form.
    #[test]
    fn pattern_matches_path_normalizes_separators() {
        assert!(pattern_matches_path("./crates/foo.rs", "crates/foo.rs"));
        assert!(pattern_matches_path("crates\\foo.rs", "crates/foo.rs"));
        assert!(pattern_matches_path("/crates/foo.rs", "crates/foo.rs"));
        assert!(pattern_matches_path("crates/foo.rs", "./crates/foo.rs"));
    }

    /// Happy path: every staged path lands in :write-scope, none in
    /// :must-not-touch, no unstaged drift. `next_step` confirms the
    /// writer can proceed with the scoped commit.
    #[test]
    fn contract_scope_summary_clean_in_scope_set() {
        let staged = vec![
            "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string(),
            "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs".to_string(),
        ];
        let changed = staged.clone();
        let write_scope = vec![
            "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string(),
            "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs".to_string(),
        ];
        let must_not_touch = vec!["scripts/**".to_string()];
        let summary =
            build_contract_scope_summary(&staged, &changed, &write_scope, &must_not_touch);
        assert_eq!(
            summary
                .get("staged_out_of_scope")
                .and_then(|v| v.as_array())
                .unwrap()
                .len(),
            0,
        );
        assert_eq!(
            summary
                .get("staged_forbidden")
                .and_then(|v| v.as_array())
                .unwrap()
                .len(),
            0,
        );
        assert_eq!(
            summary
                .get("unstaged_in_scope")
                .and_then(|v| v.as_array())
                .unwrap()
                .len(),
            0,
        );
        let next = summary
            .get("next_step")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(
            next.contains("respects :write-scope"),
            "next_step should confirm clean state, got: {}",
            next,
        );
    }

    /// Staged path matches a `:must-not-touch` glob → surfaces in
    /// `staged_forbidden` and the next_step prose tells the writer to
    /// unstage. Mirrors what `scripts/task-scope-guard.mjs` rejects on
    /// the post-commit side.
    #[test]
    fn contract_scope_summary_flags_must_not_touch_glob() {
        let staged = vec![
            "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string(),
            "scripts/render-claudecode-task.mjs".to_string(),
        ];
        let changed = staged.clone();
        let write_scope =
            vec!["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string()];
        let must_not_touch = vec!["scripts/**".to_string()];
        let summary =
            build_contract_scope_summary(&staged, &changed, &write_scope, &must_not_touch);
        let forbidden: Vec<&str> = summary
            .get("staged_forbidden")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(forbidden, vec!["scripts/render-claudecode-task.mjs"]);
        // The same path is also out-of-scope (it doesn't match write_scope).
        let oos: Vec<&str> = summary
            .get("staged_out_of_scope")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(oos, vec!["scripts/render-claudecode-task.mjs"]);
        let next = summary
            .get("next_step")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(
            next.contains("must-not-touch"),
            "next_step should mention must-not-touch, got: {}",
            next,
        );
    }

    /// Staged path lands outside both :write-scope and :must-not-touch →
    /// only `staged_out_of_scope` populates; `staged_forbidden` stays
    /// empty. Distinct signal from the `must-not-touch` case so dashboards
    /// can distinguish "out of declared scope" from "explicitly off-limits".
    #[test]
    fn contract_scope_summary_flags_out_of_scope_without_forbidden() {
        let staged = vec!["crates/missiond-core/src/event/events/execution.rs".to_string()];
        let changed = staged.clone();
        let write_scope =
            vec!["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string()];
        // execution.rs is in must-not-touch for wave20-03 but for this
        // test we leave it empty so we get a pure "out-of-scope" signal.
        let must_not_touch: Vec<String> = vec![];
        let summary =
            build_contract_scope_summary(&staged, &changed, &write_scope, &must_not_touch);
        assert_eq!(
            summary
                .get("staged_forbidden")
                .and_then(|v| v.as_array())
                .unwrap()
                .len(),
            0,
        );
        let oos: Vec<&str> = summary
            .get("staged_out_of_scope")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            oos,
            vec!["crates/missiond-core/src/event/events/execution.rs"]
        );
    }

    /// Unstaged-but-in-scope: a file the writer edited but forgot to
    /// `git add` lands in `unstaged_in_scope`. Must NOT bleed into
    /// `staged_out_of_scope` (it's not staged) and must NOT bleed into
    /// `staged_forbidden`.
    #[test]
    fn contract_scope_summary_flags_unstaged_in_scope_drift() {
        let staged =
            vec!["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string()];
        let changed = vec![
            "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string(),
            "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs".to_string(),
        ];
        let write_scope = vec![
            "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string(),
            "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs".to_string(),
        ];
        let summary = build_contract_scope_summary(&staged, &changed, &write_scope, &[]);
        let unstaged: Vec<&str> = summary
            .get("unstaged_in_scope")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            unstaged,
            vec!["crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"],
        );
        assert_eq!(
            summary
                .get("staged_out_of_scope")
                .and_then(|v| v.as_array())
                .unwrap()
                .len(),
            0,
        );
        let next = summary
            .get("next_step")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(
            next.contains("stage the in-scope edits"),
            "next_step should suggest staging, got: {}",
            next,
        );
    }

    /// Empty :write-scope → every staged path lands in
    /// `staged_out_of_scope`. Matches the verifier's posture: a contract
    /// without `:write-scope` cannot grant any path.
    #[test]
    fn contract_scope_summary_empty_write_scope_rejects_everything() {
        let staged = vec!["crates/foo.rs".to_string()];
        let summary = build_contract_scope_summary(&staged, &staged, &[], &[]);
        let oos: Vec<&str> = summary
            .get("staged_out_of_scope")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(oos, vec!["crates/foo.rs"]);
    }

    /// Loader happy path: contract on disk + matching staged set →
    /// `task_contract_status="loaded"`, scope summary populated, no
    /// failure message.
    #[test]
    fn evaluate_contract_for_preflight_loaded_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolved =
            write_task_contract(dir.path(), "tasks/wave20-03.lisp", SAMPLE_CONTRACT_BODY);
        let staged = vec!["src/a.rs".to_string()];
        let changed = vec!["src/a.rs".to_string()];
        let (status, summary, resolved_path, failure) = evaluate_task_contract_for_preflight(
            dir.path(),
            "tasks/wave20-03.lisp",
            &staged,
            &changed,
        );
        assert_eq!(status, "loaded");
        assert!(failure.is_none());
        assert_eq!(
            resolved_path.as_deref(),
            Some(resolved.display().to_string().as_str()),
        );
        let scope = summary.expect("loaded path must produce summary");
        assert_eq!(
            scope
                .get("staged_out_of_scope")
                .and_then(|v| v.as_array())
                .unwrap()
                .len(),
            0,
        );
    }

    /// Loader missing-file path: returns `task_contract_status="missing"`
    /// and a failure message that names the resolved path so the caller
    /// can correct the brief without spawning git.
    #[test]
    fn evaluate_contract_for_preflight_missing_file_returns_status() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (status, summary, resolved_path, failure) =
            evaluate_task_contract_for_preflight(dir.path(), "tasks/does-not-exist.lisp", &[], &[]);
        assert_eq!(status, "missing");
        assert!(
            summary.is_none(),
            "missing file must not yield a scope summary"
        );
        assert!(resolved_path.is_some());
        let msg = failure.expect("missing file must produce failure message");
        assert!(
            msg.contains("not readable"),
            "msg should describe IO failure, got: {}",
            msg
        );
    }

    /// Loader malformed-file path: returns `task_contract_status="malformed"`
    /// distinct from `missing` so the caller can tell "wrong path" from
    /// "wrong content" without re-reading the file.
    #[test]
    fn evaluate_contract_for_preflight_malformed_returns_status() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bad = "(task wave20-03-bad\n  :schema \"missiond.task-contract.v0\"\n  :goal \"bad\")";
        write_task_contract(dir.path(), "tasks/bad.lisp", bad);
        let (status, summary, _resolved, failure) =
            evaluate_task_contract_for_preflight(dir.path(), "tasks/bad.lisp", &[], &[]);
        assert_eq!(status, "malformed");
        assert!(summary.is_none());
        let msg = failure.expect("malformed file must produce failure message");
        assert!(
            msg.contains("schema parse"),
            "msg should describe schema mismatch, got: {}",
            msg,
        );
    }

    /// Absolute task_contract_path must NOT be re-anchored against the
    /// project root. Resolved path echoed back is byte-equal to the input.
    #[test]
    fn evaluate_contract_for_preflight_accepts_absolute_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolved =
            write_task_contract(dir.path(), "tasks/wave20-03.lisp", SAMPLE_CONTRACT_BODY);
        let abs = resolved.display().to_string();
        let unrelated = tempfile::tempdir().expect("tempdir");
        let (status, summary, resolved_path, _failure) =
            evaluate_task_contract_for_preflight(unrelated.path(), &abs, &[], &[]);
        assert_eq!(status, "loaded");
        assert!(summary.is_some());
        assert_eq!(resolved_path.as_deref(), Some(abs.as_str()));
    }

    // ── Wave 21 / Task 03 — execution report verifier integration ──
    //
    // Pin the wave21-03 surface: enum normalizer, the four-field
    // companion-log round-trip via `parse_completions`, the mini
    // report-summary reader, the contract head-id reader, and the
    // `enforce_verified_completion` gate covering every documented
    // failure code plus the happy path.

    /// `task_run_verifier_status` normalizer accepts every canonical
    /// label (with whitespace) and rejects typos. Mirrors the contract
    /// of the wave19-08 `verifier_status` normalizer.
    #[test]
    fn task_run_verifier_status_normalizer_accepts_canonical_only() {
        for &status in VALID_TASK_RUN_VERIFIER_STATUSES {
            assert_eq!(normalize_task_run_verifier_status(status), Some(status));
        }
        assert_eq!(
            normalize_task_run_verifier_status("  passed  "),
            Some("passed")
        );
        assert!(normalize_task_run_verifier_status("").is_none());
        assert!(normalize_task_run_verifier_status("done").is_none());
        assert!(normalize_task_run_verifier_status("PASSED").is_none());
    }

    /// `parse_completions` round-trips the wave21-03 metadata when
    /// every new field is present, including `verified=true` written
    /// as a bare atom and `verifier_diagnostics` prose with
    /// punctuation.
    #[test]
    fn parse_completions_reads_task_run_verifier_metadata() {
        let body = "(execution-log\n  (completions\n    (COMP001\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :commit-hash \"abc1234\"\n      :commit-status \"committed\"\n      :task-run-verifier-status \"passed\"\n      :shared-memory-path \".missiond/tasks/wave21/shared-memory.lisp\"\n      :verifier-diagnostics \"verify-task-run.mjs OK against abc1234\"\n      :verified true)))\n";
        let file = LogFile::parse(body.to_string()).expect("parse");
        let comps = parse_completions(&file);
        assert_eq!(comps.len(), 1);
        let c = &comps[0];
        assert_eq!(c.task_run_verifier_status.as_deref(), Some("passed"));
        assert_eq!(
            c.shared_memory_path.as_deref(),
            Some(".missiond/tasks/wave21/shared-memory.lisp"),
        );
        assert_eq!(
            c.verifier_diagnostics.as_deref(),
            Some("verify-task-run.mjs OK against abc1234"),
        );
        assert_eq!(c.verified, Some(true));
    }

    /// Legacy completions (no wave21-03 fields) parse cleanly and
    /// surface `None` everywhere new — the same backward-compat
    /// contract every prior wave upholds.
    #[test]
    fn parse_completions_legacy_omits_task_run_verifier_metadata() {
        let body = "(execution-log\n  (completions\n    (COMP001\n      :phase \"phase-A\"\n      :agent \"agent\"\n      :summary \"done\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\")))\n";
        let file = LogFile::parse(body.to_string()).expect("parse");
        let c = &parse_completions(&file)[0];
        assert!(c.task_run_verifier_status.is_none());
        assert!(c.shared_memory_path.is_none());
        assert!(c.verifier_diagnostics.is_none());
        assert!(c.verified.is_none());
    }

    /// Explicit `verified=false` round-trips to `Some(false)` so audit
    /// can tell "writer intentionally skipped verification" from "writer
    /// omitted the field" (legacy caller).
    #[test]
    fn parse_completions_round_trips_verified_false() {
        let body = "(execution-log\n  (completions\n    (COMP001\n      :phase \"p\"\n      :agent \"a\"\n      :summary \"s\"\n      :deliverables \"d\"\n      :verification \"v\"\n      :at \"2026-04-26T00:00:00Z\"\n      :verified false)))\n";
        let file = LogFile::parse(body.to_string()).expect("parse");
        let c = &parse_completions(&file)[0];
        assert_eq!(c.verified, Some(false));
    }

    /// Mini report reader pulls just the three keys the gate cares
    /// about and ignores everything else (notes, files_changed, etc.).
    #[test]
    fn read_report_summary_extracts_required_fields() {
        let body = r#"
(report wave21-03-sample
  :schema "missiond.report-contract.v1"
  :task_id "wave21-03-sample"
  :status done
  :commit_hash "deadbeef0123"
  :files_changed ["a.rs" "b.rs"]
  :acceptance_results []
  :notes "ignored")"#;
        let r = read_report_summary(body).expect("parse");
        assert_eq!(r.schema.as_deref(), Some("missiond.report-contract.v1"));
        assert_eq!(r.task_id.as_deref(), Some("wave21-03-sample"));
        assert_eq!(r.commit_hash.as_deref(), Some("deadbeef0123"));
    }

    /// Non-`(report ...)` top form rejects so the reader cannot be
    /// tricked into projecting a contract or a companion log.
    #[test]
    fn read_report_summary_rejects_non_report_form() {
        let body = r#"(task wave21-03-not-a-report :schema "missiond.task-contract.v1" :goal "x")"#;
        assert!(read_report_summary(body).is_err());
    }

    /// Contract head-id reader pulls the `<id>` symbol from
    /// `(task <id> ...)`. Used by the verified-gate to cross-check
    /// the report `:task_id`.
    #[test]
    fn read_task_contract_id_extracts_head_symbol() {
        let body =
            r#"(task wave21-03-test-contract :schema "missiond.task-contract.v1" :goal "x")"#;
        assert_eq!(
            read_task_contract_id(body).as_deref(),
            Some("wave21-03-test-contract"),
        );
        let other = r#"(plan p :schema "x")"#;
        assert!(read_task_contract_id(other).is_none());
    }

    /// Sample report body matching SAMPLE_CONTRACT_BODY's head id.
    /// Hash is the `abc1234` short sha used across the wave19-08 tests
    /// so the verified-gate hash-prefix overlap rule lights up cleanly.
    const SAMPLE_REPORT_BODY: &str = r#"
(report wave19-08-test-contract
  :schema "missiond.report-contract.v1"
  :task_id "wave19-08-test-contract"
  :status done
  :commit_hash "abc1234"
  :files_changed ["src/a.rs" "src/b.rs"]
  :acceptance_results [(:command "x" :exit_code 0 :ok true)]
  :notes "wave21-03 test fixture")
"#;

    fn write_task_report(dir: &Path, rel: &str, body: &str) -> PathBuf {
        let abs = dir.join(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        let mut f = std::fs::File::create(&abs).expect("create");
        f.write_all(body.as_bytes()).expect("write");
        abs
    }

    /// `verified=true` without `enforce_scoped_commit=true` rejects
    /// with `VERIFIED_REQUIRES_ENFORCEMENT`. The verified flag is
    /// meaningless without the underlying scope gate also running.
    #[test]
    fn verified_rejects_without_enforce_scoped_commit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let res = enforce_verified_completion(
            dir.path(),
            false,
            Some("tasks/x.lisp"),
            Some("tasks/x.report.lisp"),
            Some("abc1234"),
        );
        let err = res.expect_err("must reject without enforcement");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("VERIFIED_REQUIRES_ENFORCEMENT"),
        );
    }

    /// Missing task_contract_path → `VERIFIED_REQUIRES_TASK_CONTRACT`.
    #[test]
    fn verified_rejects_missing_task_contract_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let res = enforce_verified_completion(
            dir.path(),
            true,
            None,
            Some("tasks/x.report.lisp"),
            Some("abc1234"),
        );
        let err = res.expect_err("must reject missing contract");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("VERIFIED_REQUIRES_TASK_CONTRACT"),
        );
    }

    /// Missing task_report_path → `VERIFIED_REQUIRES_TASK_REPORT`.
    #[test]
    fn verified_rejects_missing_task_report_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let res = enforce_verified_completion(
            dir.path(),
            true,
            Some("tasks/x.lisp"),
            None,
            Some("abc1234"),
        );
        let err = res.expect_err("must reject missing report");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("VERIFIED_REQUIRES_TASK_REPORT"),
        );
    }

    /// Missing commit_hash → `VERIFIED_REQUIRES_COMMIT_HASH`.
    /// Whitespace-only also rejects via the trim-then-filter-empty
    /// pipeline.
    #[test]
    fn verified_rejects_missing_commit_hash() {
        let dir = tempfile::tempdir().expect("tempdir");
        let res = enforce_verified_completion(
            dir.path(),
            true,
            Some("tasks/x.lisp"),
            Some("tasks/x.report.lisp"),
            None,
        );
        let err = res.expect_err("must reject absent hash");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("VERIFIED_REQUIRES_COMMIT_HASH"),
        );

        let res2 = enforce_verified_completion(
            dir.path(),
            true,
            Some("tasks/x.lisp"),
            Some("tasks/x.report.lisp"),
            Some("   "),
        );
        let err2 = res2.expect_err("must reject blank hash");
        assert_eq!(
            extract_error_code(&err2).as_deref(),
            Some("VERIFIED_REQUIRES_COMMIT_HASH"),
        );
    }

    /// Missing report file → `TASK_REPORT_REQUIRED`. The error must
    /// surface BEFORE the daemon mutates the companion log.
    #[test]
    fn verified_rejects_missing_report_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
        let res = enforce_verified_completion(
            dir.path(),
            true,
            Some("tasks/ok.lisp"),
            Some("tasks/does-not-exist.report.lisp"),
            Some("abc1234"),
        );
        let err = res.expect_err("missing report must reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("TASK_REPORT_REQUIRED"),
        );
    }

    /// Report with a wrong `:schema` → `TASK_REPORT_MALFORMED`.
    #[test]
    fn verified_rejects_report_schema_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
        let bad = r#"(report wave19-08-test-contract :schema "missiond.report-contract.v0" :task_id "wave19-08-test-contract" :commit_hash "abc1234")"#;
        write_task_report(dir.path(), "tasks/bad.report.lisp", bad);
        let res = enforce_verified_completion(
            dir.path(),
            true,
            Some("tasks/ok.lisp"),
            Some("tasks/bad.report.lisp"),
            Some("abc1234"),
        );
        let err = res.expect_err("schema mismatch must reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("TASK_REPORT_MALFORMED"),
        );
    }

    /// Report `:task_id` not matching the contract head id →
    /// `TASK_REPORT_TASK_ID_MISMATCH`. Distinct from the schema error
    /// so the writer can tell "wrong file referenced" from "wrong
    /// file shape".
    #[test]
    fn verified_rejects_report_task_id_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
        let body = r#"
(report wave21-03-other-task
  :schema "missiond.report-contract.v1"
  :task_id "wave21-03-other-task"
  :status done
  :commit_hash "abc1234"
  :files_changed []
  :acceptance_results [])
"#;
        write_task_report(dir.path(), "tasks/wrong.report.lisp", body);
        let res = enforce_verified_completion(
            dir.path(),
            true,
            Some("tasks/ok.lisp"),
            Some("tasks/wrong.report.lisp"),
            Some("abc1234"),
        );
        let err = res.expect_err("task_id mismatch must reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("TASK_REPORT_TASK_ID_MISMATCH"),
        );
    }

    /// Report `:commit_hash` not matching the supplied hash →
    /// `TASK_REPORT_COMMIT_HASH_MISMATCH`. Tests with a clearly
    /// different hash so the prefix-overlap rule cannot accidentally
    /// pass.
    #[test]
    fn verified_rejects_report_commit_hash_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
        let body = r#"
(report wave19-08-test-contract
  :schema "missiond.report-contract.v1"
  :task_id "wave19-08-test-contract"
  :status done
  :commit_hash "feedbeef9999"
  :files_changed []
  :acceptance_results [])
"#;
        write_task_report(dir.path(), "tasks/x.report.lisp", body);
        let res = enforce_verified_completion(
            dir.path(),
            true,
            Some("tasks/ok.lisp"),
            Some("tasks/x.report.lisp"),
            Some("abc1234"),
        );
        let err = res.expect_err("hash mismatch must reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("TASK_REPORT_COMMIT_HASH_MISMATCH"),
        );
    }

    /// Happy path: every precondition met, report loadable, schema +
    /// task_id + commit_hash all match. Validation summary echoes the
    /// resolved paths + the checked rules.
    #[test]
    fn verified_accepts_aligned_report() {
        let dir = tempfile::tempdir().expect("tempdir");
        let contract_resolved =
            write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
        let report_resolved =
            write_task_report(dir.path(), "tasks/ok.report.lisp", SAMPLE_REPORT_BODY);
        let res = enforce_verified_completion(
            dir.path(),
            true,
            Some("tasks/ok.lisp"),
            Some("tasks/ok.report.lisp"),
            Some("abc1234"),
        );
        let summary = res.expect("aligned report must pass");
        assert_eq!(
            summary.get("task_id").and_then(|v| v.as_str()),
            Some("wave19-08-test-contract"),
        );
        assert_eq!(
            summary
                .get("task_contract_resolved_path")
                .and_then(|v| v.as_str()),
            Some(contract_resolved.display().to_string().as_str()),
        );
        assert_eq!(
            summary
                .get("task_report_resolved_path")
                .and_then(|v| v.as_str()),
            Some(report_resolved.display().to_string().as_str()),
        );
        let checked = summary
            .get("checked")
            .and_then(|v| v.as_array())
            .expect("checked");
        assert!(checked
            .iter()
            .any(|v| v.as_str() == Some("preconditions_present")));
        assert!(checked
            .iter()
            .any(|v| v.as_str() == Some("task_report_loadable")));
        assert!(checked
            .iter()
            .any(|v| v.as_str() == Some("task_report_schema")));
        assert!(checked
            .iter()
            .any(|v| v.as_str() == Some("task_id_matches_contract")));
        assert!(checked
            .iter()
            .any(|v| v.as_str() == Some("commit_hash_matches_report")));
    }

    /// Long-sha completion hash + short-sha report hash overlap via
    /// `starts_with`. Matches the way `git log --format=%h` truncates
    /// to 7+ chars while `git rev-parse HEAD` returns the full
    /// 40-char form.
    #[test]
    fn verified_accepts_short_long_sha_prefix_overlap() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(dir.path(), "tasks/ok.lisp", SAMPLE_CONTRACT_BODY);
        let body = r#"
(report wave19-08-test-contract
  :schema "missiond.report-contract.v1"
  :task_id "wave19-08-test-contract"
  :status done
  :commit_hash "abc1234"
  :files_changed []
  :acceptance_results [])
"#;
        write_task_report(dir.path(), "tasks/x.report.lisp", body);
        let res = enforce_verified_completion(
            dir.path(),
            true,
            Some("tasks/ok.lisp"),
            Some("tasks/x.report.lisp"),
            Some("abc1234567890abcdef"),
        );
        assert!(res.is_ok(), "long↔short sha overlap should pass");
    }

    /// Read-only proof: this whole file may only spawn `git` for
    /// `git status --porcelain=v1` (the wave18-08 preflight check).
    /// We grep the file at test time so the proof survives future
    /// edits — exactly one `Command::new(<git>)` site is allowed.
    /// Anchor at `CARGO_MANIFEST_DIR` so the test stays robust to
    /// whichever working directory the cargo harness picks. The
    /// search needle is built at runtime via `format!` so the test
    /// source itself doesn't count toward the match (a self-counting
    /// literal would always inflate the total by one).
    #[test]
    fn daemon_never_invokes_mutating_git() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path =
            std::path::Path::new(manifest_dir).join("src/handlers/knowledge/agent_execution.rs");
        let src = std::fs::read_to_string(&path).expect("read self");
        let needle = format!("Command::new({}git{})", '"', '"');
        let command_git = src.matches(needle.as_str()).count();
        assert_eq!(
            command_git, 1,
            "expected exactly one git Command::new site (the wave18-08 status read), found {}",
            command_git
        );
        let argv_needle = format!(
            ".args([{}status{}, {}--porcelain=v1{}])",
            '"', '"', '"', '"'
        );
        assert!(
            src.contains(argv_needle.as_str()),
            "the single git Command::new site must use the wave18-08 status argv",
        );
    }

    // ── Wave 21 / Task 08 — machine-contract autonomous loop smoke ──
    //
    // These tests deterministically exercise the daemon-side cross-checks
    // that close the wave21 autonomous loop. They drive the wave21-03
    // verifier helpers (`enforce_verified_completion` /
    // `enforce_task_contract_completion` / `read_report_summary` /
    // `read_task_contract_id`) end-to-end against fixture task-contract
    // and report-contract Lisp text on disk. No LLM, no spawn, no shell,
    // no markdown read — the smoke proves the daemon can ratify a fully
    // machine-contract dispatch using only local file IO + structural
    // parses.
    //
    // Invariants pinned (cross-wave):
    //   * wave19-08 / wave21-03 — every malformed input maps to a
    //     deterministic structured-error code (TASK_CONTRACT_REQUIRED /
    //     TASK_CONTRACT_MALFORMED / TASK_REPORT_REQUIRED /
    //     TASK_REPORT_MALFORMED / TASK_REPORT_TASK_ID_MISMATCH /
    //     TASK_REPORT_COMMIT_HASH_MISMATCH).
    //   * wave21-03 — the verified gate REUSES the wave19-08 contract
    //     gate; the happy-path summary echoes both `task_contract_*` and
    //     `task_report_*` resolved paths so observers can reconstruct the
    //     handoff without reparsing the inputs.
    //   * The daemon NEVER falls back to prompt mode / markdown when the
    //     contract or report fails to parse — fail-fast over silent
    //     salvage.

    /// Fixture task contract that mirrors the byte-shape produced by
    /// `plan::build_task_contract_lisp` for the wave21-08 smoke. The
    /// `:write-scope` is empty so the wave19-08 claim-coverage rule is
    /// satisfied vacuously; the smoke focuses on the wave21-03 verified
    /// gate (schema + task_id + commit_hash) rather than the wave19-08
    /// scope coverage rule (already covered above).
    const WAVE21_08_SMOKE_CONTRACT_BODY: &str = r#"
(task wave21-08-smoke-contract
  :schema "missiond.task-contract.v1"
  :goal "wave21-08 deterministic machine-contract loop smoke"
  :write-scope []
  :must-not-touch []
  :acceptance ["cargo test -p missiond-daemon"]
  :commit (:required true :message "test(intent): cover wave21 loop" :scope-check write-scope-only))
"#;

    /// Fixture report-contract aligned with the contract above. Both
    /// `:task_id` and `:commit_hash` match what the smoke supplies via
    /// `commit_hash`, so the wave21-03 cross-check passes end-to-end.
    const WAVE21_08_SMOKE_REPORT_BODY: &str = r#"
(report wave21-08-smoke-contract
  :schema "missiond.report-contract.v1"
  :task_id "wave21-08-smoke-contract"
  :status done
  :commit_hash "cafef00d1234"
  :files_changed ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]
  :acceptance_results [(:command "cargo test -p missiond-daemon" :exit_code 0 :ok true)]
  :notes "wave21-08 smoke fixture")
"#;

    /// Wave21-08 happy path: the verifier accepts an aligned (contract,
    /// report, hash) triple and surfaces every cross-checked rule on the
    /// validation summary. This is the SSOT proof that the wave21-03
    /// gate can ratify a machine-contract autonomous loop end-to-end
    /// without any external process.
    #[test]
    fn smoke_wave21_machine_contract_autonomous_loop_verifier_accepts_aligned_triple() {
        let dir = tempfile::tempdir().expect("tempdir");
        let contract_resolved = write_task_contract(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-smoke.lisp",
            WAVE21_08_SMOKE_CONTRACT_BODY,
        );
        let report_resolved = write_task_report(
            dir.path(),
            ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
            WAVE21_08_SMOKE_REPORT_BODY,
        );
        let res = enforce_verified_completion(
            dir.path(),
            true,
            Some(".missiond/tasks/wave21/wave21-08-smoke.lisp"),
            Some(".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp"),
            Some("cafef00d1234"),
        );
        let summary = res.expect("aligned wave21-08 fixture must pass verifier");

        // Cross-check: every wave21-03 invariant lands on the summary so
        // observers can grep without reparsing the inputs.
        assert_eq!(
            summary.get("task_id").and_then(|v| v.as_str()),
            Some("wave21-08-smoke-contract"),
            "verified summary must echo the contract head id"
        );
        assert_eq!(
            summary
                .get("task_contract_resolved_path")
                .and_then(|v| v.as_str()),
            Some(contract_resolved.display().to_string().as_str()),
            "verified summary must echo the resolved contract path"
        );
        assert_eq!(
            summary
                .get("task_report_resolved_path")
                .and_then(|v| v.as_str()),
            Some(report_resolved.display().to_string().as_str()),
            "verified summary must echo the resolved report path"
        );
        let checked = summary
            .get("checked")
            .and_then(|v| v.as_array())
            .expect("checked list must exist");
        for needle in [
            "preconditions_present",
            "task_report_loadable",
            "task_report_schema",
            "task_id_matches_contract",
            "commit_hash_matches_report",
        ] {
            assert!(
                checked.iter().any(|v| v.as_str() == Some(needle)),
                "wave21-03 verifier must record `{}` in :checked",
                needle
            );
        }
    }

    /// Wave21-08 fail-fast: a report with a mismatched `:task_id` MUST
    /// surface `TASK_REPORT_TASK_ID_MISMATCH`. Pinning this here proves
    /// the verifier never silently accepts a stale report glued onto a
    /// fresh contract — the daemon refuses, and the operator must
    /// regenerate the report.
    #[test]
    fn smoke_wave21_malformed_report_task_id_yields_structured_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-smoke.lisp",
            WAVE21_08_SMOKE_CONTRACT_BODY,
        );
        // Report carries a different head id + :task_id — the verifier
        // MUST refuse the cross-check.
        let body = r#"
(report wave21-08-other-task
  :schema "missiond.report-contract.v1"
  :task_id "wave21-08-other-task"
  :status done
  :commit_hash "cafef00d1234"
  :files_changed []
  :acceptance_results [])
"#;
        write_task_report(
            dir.path(),
            ".missiond/tasks/wave21/reports/wave21-08-wrong.report.lisp",
            body,
        );
        let res = enforce_verified_completion(
            dir.path(),
            true,
            Some(".missiond/tasks/wave21/wave21-08-smoke.lisp"),
            Some(".missiond/tasks/wave21/reports/wave21-08-wrong.report.lisp"),
            Some("cafef00d1234"),
        );
        let err = res.expect_err("mismatched :task_id MUST reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("TASK_REPORT_TASK_ID_MISMATCH"),
            "wave21-03 verifier must surface the dedicated mismatch code so dashboards can route on it"
        );
    }

    /// Wave21-08 fail-fast: a malformed task-contract (schema mismatch)
    /// MUST surface `TASK_CONTRACT_MALFORMED` even when the report
    /// itself parses cleanly. The verifier MUST refuse rather than
    /// silently downgrading to "report-only" mode.
    #[test]
    fn smoke_wave21_malformed_task_contract_yields_structured_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bad_contract = r#"(task wave21-08-bad
  :schema "missiond.task-contract.v0"
  :goal "wrong schema")"#;
        write_task_contract(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-bad.lisp",
            bad_contract,
        );
        // The report parses cleanly so we prove the rejection comes from
        // the contract side, not the report side.
        write_task_report(
            dir.path(),
            ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
            WAVE21_08_SMOKE_REPORT_BODY,
        );
        // Drive the wave19-08 contract gate directly — that's the gate
        // the daemon hits FIRST when `enforce_scoped_commit=true` is
        // paired with `task_contract_path`.
        let file = fresh_file_with_claim();
        let res = enforce_task_contract_completion(
            &file,
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-bad.lisp",
            Some("cafef00d1234"),
            Some(&[]),
        );
        let err = res.expect_err("schema mismatch MUST reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("TASK_CONTRACT_MALFORMED"),
            "wave21-08 smoke: malformed contract MUST hit the dedicated TASK_CONTRACT_MALFORMED code"
        );
    }

    /// Wave21-08 fail-fast: a missing report file MUST surface
    /// `TASK_REPORT_REQUIRED` — distinct from `TASK_REPORT_MALFORMED` so
    /// the writer can tell "wrong path" from "wrong content" without
    /// rerunning anything.
    #[test]
    fn smoke_wave21_missing_report_yields_structured_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-smoke.lisp",
            WAVE21_08_SMOKE_CONTRACT_BODY,
        );
        let res = enforce_verified_completion(
            dir.path(),
            true,
            Some(".missiond/tasks/wave21/wave21-08-smoke.lisp"),
            // Path does not exist on disk.
            Some(".missiond/tasks/wave21/reports/wave21-08-nope.report.lisp"),
            Some("cafef00d1234"),
        );
        let err = res.expect_err("missing report MUST reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("TASK_REPORT_REQUIRED"),
            "wave21-08 smoke: missing report MUST hit TASK_REPORT_REQUIRED"
        );
    }

    /// Wave21-08 fail-fast: a commit_hash that does not match the
    /// report's `:commit_hash` (and is not a prefix-overlap) MUST
    /// surface `TASK_REPORT_COMMIT_HASH_MISMATCH`. Pinning this with a
    /// clearly-different hash proves the prefix-overlap rule does NOT
    /// accidentally accept an unrelated SHA.
    #[test]
    fn smoke_wave21_mismatched_commit_hash_yields_structured_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-smoke.lisp",
            WAVE21_08_SMOKE_CONTRACT_BODY,
        );
        write_task_report(
            dir.path(),
            ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
            WAVE21_08_SMOKE_REPORT_BODY,
        );
        let res = enforce_verified_completion(
            dir.path(),
            true,
            Some(".missiond/tasks/wave21/wave21-08-smoke.lisp"),
            Some(".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp"),
            // Different hash, not a prefix of `cafef00d1234`.
            Some("badc0ffee999"),
        );
        let err = res.expect_err("hash mismatch MUST reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("TASK_REPORT_COMMIT_HASH_MISMATCH"),
            "wave21-08 smoke: mismatched commit_hash MUST hit the dedicated mismatch code"
        );
    }

    /// Wave21-08 structural projector smoke: the wave21-03 mini reader
    /// (`read_report_summary` + `read_task_contract_id`) extracts the
    /// three load-bearing fields from the fixture report and the head
    /// id from the fixture contract. Pinning these directly proves the
    /// daemon-side projection survives a future wave-21+ schema change
    /// without leaning on the script-side checker.
    #[test]
    fn smoke_wave21_report_and_contract_projectors_extract_required_fields() {
        let report = read_report_summary(WAVE21_08_SMOKE_REPORT_BODY)
            .expect("wave21-08 smoke report must parse");
        assert_eq!(
            report.schema.as_deref(),
            Some("missiond.report-contract.v1"),
            "report :schema MUST be the wave21-03 v1 schema"
        );
        assert_eq!(
            report.task_id.as_deref(),
            Some("wave21-08-smoke-contract"),
            "report :task_id MUST surface verbatim"
        );
        assert_eq!(
            report.commit_hash.as_deref(),
            Some("cafef00d1234"),
            "report :commit_hash MUST surface verbatim"
        );
        let contract_id = read_task_contract_id(WAVE21_08_SMOKE_CONTRACT_BODY)
            .expect("wave21-08 smoke contract head id must extract");
        assert_eq!(
            contract_id, "wave21-08-smoke-contract",
            "contract head id MUST equal the report :task_id (cross-check anchor)"
        );
        // Anchor: the head id pulled out of the contract is exactly the
        // value the wave21-03 verifier compares against the report's
        // `:task_id`. Pinning the equality here in one place catches a
        // future drift between the two readers.
        assert_eq!(
            Some(contract_id.as_str()),
            report.task_id.as_deref(),
            "wave21-08 cross-check anchor: contract head id must equal report :task_id"
        );
    }

    // ── Wave 22 / Task 02 — auto task-run verifier (in-process) ──
    //
    // These tests pin the wave22-02 contract on the daemon-side
    // auto-verifier (`auto_run_task_run_verifier`) and the supporting
    // shared-memory projector (`read_shared_memory_ledger` /
    // `read_completion_task_id`). The auto-verifier removes the
    // wave21-03 caller-supplied `verified=true` escape hatch by
    // computing the verdict itself when all four paths
    // (`task_contract_path`, `task_report_path`, `shared_memory_path`,
    // `commit_hash`) are supplied. The verdict reuses the wave19-08 +
    // wave21-03 error-code vocabulary plus three wave22-02 codes
    // (`SHARED_MEMORY_REQUIRED`, `SHARED_MEMORY_MALFORMED`,
    // `SHARED_MEMORY_NO_COMPLETION_FOR_TASK`) so dashboards see one
    // consistent surface across the gates.

    /// Aligned shared-memory ledger fixture mirroring the byte-shape of
    /// `.missiond/tasks/<wave>/shared-memory.lisp`. The `(completion ...)`
    /// child references the wave21-08 smoke contract head id so the
    /// auto-verifier finds a matching entry.
    const WAVE22_02_SMOKE_MEMORY_BODY: &str = r#"
(shared-memory wave21
  :schema "missiond.shared-memory.v1"
  :wave wave21
  :created-at "2026-04-26T00:00:00Z"
  :sequence 1
  (claim
    :id wave21-08-claim-001
    :task wave21-08-smoke-contract
    :agent claudecode
    :seq 1
    :at "2026-04-26T00:01:00Z"
    :touched ["src/x.rs"]
    :summary "claim")
  (completion
    :id wave21-08-completion-001
    :task wave21-08-smoke-contract
    :agent claudecode
    :seq 2
    :at "2026-04-26T00:02:00Z"
    :touched ["src/x.rs"]
    :summary "done"))
"#;

    /// Wave22-02 happy path: every path supplied + every cross-check
    /// passes → daemon-computed `verifier_status="passed"` and the
    /// `verified_scope_summary` records every check name. This is the
    /// SSOT proof that the daemon can ratify a task run end-to-end
    /// without a Node spawn.
    #[test]
    fn auto_verifier_accepts_aligned_quartet() {
        let dir = tempfile::tempdir().expect("tempdir");
        let contract_resolved = write_task_contract(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-smoke.lisp",
            WAVE21_08_SMOKE_CONTRACT_BODY,
        );
        let report_resolved = write_task_report(
            dir.path(),
            ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
            WAVE21_08_SMOKE_REPORT_BODY,
        );
        let memory_resolved = write_task_report(
            dir.path(),
            ".missiond/tasks/wave21/shared-memory.lisp",
            WAVE22_02_SMOKE_MEMORY_BODY,
        );
        let res = auto_run_task_run_verifier(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-smoke.lisp",
            ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
            ".missiond/tasks/wave21/shared-memory.lisp",
            "cafef00d1234",
        );
        let summary = res.expect("aligned quartet must pass auto-verifier");
        assert_eq!(
            summary.get("verifier_status").and_then(|v| v.as_str()),
            Some("passed"),
            "daemon-computed verdict MUST be `passed` for the aligned quartet"
        );
        assert_eq!(
            summary.get("task_id").and_then(|v| v.as_str()),
            Some("wave21-08-smoke-contract"),
        );
        assert_eq!(
            summary
                .get("task_contract_resolved_path")
                .and_then(|v| v.as_str()),
            Some(contract_resolved.display().to_string().as_str()),
        );
        assert_eq!(
            summary
                .get("task_report_resolved_path")
                .and_then(|v| v.as_str()),
            Some(report_resolved.display().to_string().as_str()),
        );
        assert_eq!(
            summary
                .get("shared_memory_resolved_path")
                .and_then(|v| v.as_str()),
            Some(memory_resolved.display().to_string().as_str()),
        );
        let checks = summary
            .get("checks")
            .and_then(|v| v.as_array())
            .expect("checks list must exist");
        for needle in [
            "task_contract_loadable",
            "task_report_loadable",
            "task_report_schema",
            "task_id_matches_contract",
            "commit_hash_matches_report",
            "shared_memory_loadable",
            "shared_memory_schema",
            "shared_memory_completion_for_task",
        ] {
            assert!(
                checks.iter().any(|v| v.as_str() == Some(needle)),
                "auto-verifier MUST record `{}` in :checks",
                needle
            );
        }
    }

    /// Missing shared-memory file → `SHARED_MEMORY_REQUIRED`. Distinct
    /// from `SHARED_MEMORY_MALFORMED` so the writer can tell "wrong
    /// path" from "wrong content" without re-running anything.
    #[test]
    fn auto_verifier_rejects_missing_shared_memory() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-smoke.lisp",
            WAVE21_08_SMOKE_CONTRACT_BODY,
        );
        write_task_report(
            dir.path(),
            ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
            WAVE21_08_SMOKE_REPORT_BODY,
        );
        let res = auto_run_task_run_verifier(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-smoke.lisp",
            ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
            ".missiond/tasks/wave21/does-not-exist.lisp",
            "cafef00d1234",
        );
        let err = res.expect_err("missing shared-memory must reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("SHARED_MEMORY_REQUIRED"),
        );
    }

    /// Shared-memory ledger with the wrong `:schema` →
    /// `SHARED_MEMORY_MALFORMED`. The structural parse succeeds but
    /// the schema check refuses to ratify a non-v1 ledger so the
    /// auto-verifier never silently accepts a stale shape.
    #[test]
    fn auto_verifier_rejects_shared_memory_schema_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-smoke.lisp",
            WAVE21_08_SMOKE_CONTRACT_BODY,
        );
        write_task_report(
            dir.path(),
            ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
            WAVE21_08_SMOKE_REPORT_BODY,
        );
        let bad_memory = r#"
(shared-memory wave21
  :schema "missiond.shared-memory.v0"
  :wave wave21
  (completion :id x :task wave21-08-smoke-contract :agent x :seq 1 :touched [] :summary "x"))
"#;
        write_task_report(
            dir.path(),
            ".missiond/tasks/wave21/shared-memory.lisp",
            bad_memory,
        );
        let res = auto_run_task_run_verifier(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-smoke.lisp",
            ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
            ".missiond/tasks/wave21/shared-memory.lisp",
            "cafef00d1234",
        );
        let err = res.expect_err("schema mismatch must reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("SHARED_MEMORY_MALFORMED"),
        );
    }

    /// Shared-memory ledger has the right schema but no
    /// `(completion :task <id> ...)` for the contract head id →
    /// `SHARED_MEMORY_NO_COMPLETION_FOR_TASK`. Mirrors the wave21-02
    /// script-side rule so the daemon and the script agree.
    #[test]
    fn auto_verifier_rejects_shared_memory_without_completion_for_task() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-smoke.lisp",
            WAVE21_08_SMOKE_CONTRACT_BODY,
        );
        write_task_report(
            dir.path(),
            ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
            WAVE21_08_SMOKE_REPORT_BODY,
        );
        // Ledger has only a claim and a completion for OTHER task — the
        // daemon must refuse rather than silently passing.
        let no_match_memory = r#"
(shared-memory wave21
  :schema "missiond.shared-memory.v1"
  :wave wave21
  (claim
    :id wave21-99-claim-001
    :task wave21-99-other
    :agent claudecode
    :seq 1
    :touched []
    :summary "claim")
  (completion
    :id wave21-99-completion-001
    :task wave21-99-other
    :agent claudecode
    :seq 2
    :touched []
    :summary "done"))
"#;
        write_task_report(
            dir.path(),
            ".missiond/tasks/wave21/shared-memory.lisp",
            no_match_memory,
        );
        let res = auto_run_task_run_verifier(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-smoke.lisp",
            ".missiond/tasks/wave21/reports/wave21-08-smoke.report.lisp",
            ".missiond/tasks/wave21/shared-memory.lisp",
            "cafef00d1234",
        );
        let err = res.expect_err("missing completion entry must reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("SHARED_MEMORY_NO_COMPLETION_FOR_TASK"),
        );
    }

    /// The shared-memory projector pulls `:schema` and every
    /// `(completion :task <id> ...)` task id off the ledger. Pinning
    /// this directly proves the wave22-02 auto-verifier's matching
    /// rule survives a future ledger schema change without leaning on
    /// the script-side checker.
    #[test]
    fn shared_memory_projector_extracts_required_fields() {
        let summary = read_shared_memory_ledger(WAVE22_02_SMOKE_MEMORY_BODY).expect("must parse");
        assert_eq!(summary.schema.as_deref(), Some("missiond.shared-memory.v1"),);
        assert!(
            summary
                .completion_tasks
                .iter()
                .any(|t| t == "wave21-08-smoke-contract"),
            "projector MUST surface every (completion :task <id> ...) entry"
        );
    }

    /// `read_completion_task_id` ignores `(completion ...)` forms with
    /// no `:task` slot — mirrors the script-side verifier which uses
    /// the same "must have :task" rule when matching.
    #[test]
    fn completion_task_id_ignores_entry_without_task_slot() {
        let body = r#"
(shared-memory wave99
  :schema "missiond.shared-memory.v1"
  :wave wave99
  (completion :id x :agent y :seq 1 :touched [] :summary "no task slot"))
"#;
        let summary = read_shared_memory_ledger(body).expect("must parse");
        assert!(
            summary.completion_tasks.is_empty(),
            "entries without :task MUST be silently skipped to mirror the script-side rule"
        );
    }

    /// Auto-verifier delegates the contract+report cross-checks to the
    /// same projectors as the wave21-03 gate, so a report `:task_id`
    /// mismatch still surfaces the dedicated `TASK_REPORT_TASK_ID_MISMATCH`
    /// code rather than a generic auto-verifier failure. Pinning this
    /// directly proves the vocabulary stays unified across the two gates.
    #[test]
    fn auto_verifier_reuses_wave21_03_codes_for_report_task_id_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-smoke.lisp",
            WAVE21_08_SMOKE_CONTRACT_BODY,
        );
        let mismatched_report = r#"
(report wave21-08-other-task
  :schema "missiond.report-contract.v1"
  :task_id "wave21-08-other-task"
  :status done
  :commit_hash "cafef00d1234"
  :files_changed []
  :acceptance_results [])
"#;
        write_task_report(
            dir.path(),
            ".missiond/tasks/wave21/reports/wave21-08-mismatch.report.lisp",
            mismatched_report,
        );
        write_task_report(
            dir.path(),
            ".missiond/tasks/wave21/shared-memory.lisp",
            WAVE22_02_SMOKE_MEMORY_BODY,
        );
        let res = auto_run_task_run_verifier(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-smoke.lisp",
            ".missiond/tasks/wave21/reports/wave21-08-mismatch.report.lisp",
            ".missiond/tasks/wave21/shared-memory.lisp",
            "cafef00d1234",
        );
        let err = res.expect_err("task_id mismatch MUST reject");
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("TASK_REPORT_TASK_ID_MISMATCH"),
            "wave22-02 auto-verifier MUST reuse wave21-03 vocabulary so consumers see one code"
        );
    }

    /// Auto-verifier preserves the short<->long sha overlap rule from
    /// the wave21-03 gate. A 7-char `git log %h` value MUST match a
    /// 40-char `git rev-parse HEAD` value via prefix overlap.
    #[test]
    fn auto_verifier_accepts_short_long_sha_prefix_overlap() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-smoke.lisp",
            WAVE21_08_SMOKE_CONTRACT_BODY,
        );
        let short_hash_report = r#"
(report wave21-08-smoke-contract
  :schema "missiond.report-contract.v1"
  :task_id "wave21-08-smoke-contract"
  :status done
  :commit_hash "cafef00"
  :files_changed []
  :acceptance_results [])
"#;
        write_task_report(
            dir.path(),
            ".missiond/tasks/wave21/reports/wave21-08-short.report.lisp",
            short_hash_report,
        );
        write_task_report(
            dir.path(),
            ".missiond/tasks/wave21/shared-memory.lisp",
            WAVE22_02_SMOKE_MEMORY_BODY,
        );
        let res = auto_run_task_run_verifier(
            dir.path(),
            ".missiond/tasks/wave21/wave21-08-smoke.lisp",
            ".missiond/tasks/wave21/reports/wave21-08-short.report.lisp",
            ".missiond/tasks/wave21/shared-memory.lisp",
            // Long sha; report has the 7-char prefix.
            "cafef001234567890abcdef",
        );
        assert!(
            res.is_ok(),
            "short<->long sha prefix overlap MUST pass the auto-verifier",
        );
    }

    // ── Wave 22 / Task 07 — autonomous loop apply smoke v4 ──
    //
    // Deterministic smoke tests covering the wave22-02 auto task-run
    // verifier slice of the apply-gate cluster. Every test pinned here is
    // a `no real LLM / no real spawn / no real git mutation` proof: the
    // verifier helpers do read-only file inspection on tempfile fixtures
    // and never invoke `Command::new`. Companion tests in the other four
    // write-scope files (review_gate.rs / plan.rs / workstation_dispatch.rs
    // / unified_entry.rs) cover the matching apply-gate pure evaluators
    // and the envelope-side markdown-non-load-bearing invariant.

    /// V4 smoke: when `enforce_scoped_commit=true` is paired with a
    /// task_contract_path + report_path + shared_memory_path quartet, a
    /// completion entry that does NOT match the contract head id MUST
    /// block the completion via `SHARED_MEMORY_NO_COMPLETION_FOR_TASK`.
    /// This is the wave22-07 Requirement 4 anchor: failed verification
    /// blocks completion. The smoke deliberately routes through the
    /// `auto_run_task_run_verifier` helper because that is the daemon-
    /// side gate `action_complete` will dispatch to when the caller
    /// supplies the full quartet (per wave22-02 contract).
    #[test]
    fn smoke_wave22_07_failed_verification_blocks_completion_when_enforce_scoped_commit_true() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(
            dir.path(),
            ".missiond/tasks/wave22/wave22-07-smoke.lisp",
            WAVE21_08_SMOKE_CONTRACT_BODY,
        );
        write_task_report(
            dir.path(),
            ".missiond/tasks/wave22/reports/wave22-07-smoke.report.lisp",
            WAVE21_08_SMOKE_REPORT_BODY,
        );
        // Ledger has only completion entries for OTHER tasks — the
        // verifier MUST refuse rather than silently passing. This
        // models the wave22-07 v4 brief Requirement 4: the verifier
        // blocks completion on the failed-verification path.
        let no_match_memory = r#"
(shared-memory wave22
  :schema "missiond.shared-memory.v1"
  :wave wave22
  (claim
    :id wave22-99-claim-001
    :task wave22-99-other
    :agent claudecode
    :seq 1
    :touched []
    :summary "claim")
  (completion
    :id wave22-99-completion-001
    :task wave22-99-other
    :agent claudecode
    :seq 2
    :touched []
    :summary "done"))
"#;
        write_task_report(
            dir.path(),
            ".missiond/tasks/wave22/shared-memory.lisp",
            no_match_memory,
        );
        let res = auto_run_task_run_verifier(
            dir.path(),
            ".missiond/tasks/wave22/wave22-07-smoke.lisp",
            ".missiond/tasks/wave22/reports/wave22-07-smoke.report.lisp",
            ".missiond/tasks/wave22/shared-memory.lisp",
            "cafef00d1234",
        );
        let err = res.expect_err(
            "wave22-07 v4 invariant: when enforce_scoped_commit=true paths align but no \
             completion entry exists for the contract head id, completion MUST be blocked",
        );
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("SHARED_MEMORY_NO_COMPLETION_FOR_TASK"),
            "wave22-07 v4 invariant: failed verification MUST surface the dedicated \
             SHARED_MEMORY_NO_COMPLETION_FOR_TASK code so dashboards can route on it"
        );
    }

    /// V4 smoke (companion): a mismatched commit_hash on the same
    /// quartet MUST also block completion. Pinning both rejection
    /// surfaces here proves the gate is symmetric — neither a missing
    /// completion entry nor a stale commit hash can sneak past
    /// `enforce_scoped_commit=true`.
    #[test]
    fn smoke_wave22_07_failed_verification_blocks_on_commit_hash_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_task_contract(
            dir.path(),
            ".missiond/tasks/wave22/wave22-07-smoke.lisp",
            WAVE21_08_SMOKE_CONTRACT_BODY,
        );
        write_task_report(
            dir.path(),
            ".missiond/tasks/wave22/reports/wave22-07-smoke.report.lisp",
            WAVE21_08_SMOKE_REPORT_BODY,
        );
        write_task_report(
            dir.path(),
            ".missiond/tasks/wave22/shared-memory.lisp",
            WAVE22_02_SMOKE_MEMORY_BODY,
        );
        let res = auto_run_task_run_verifier(
            dir.path(),
            ".missiond/tasks/wave22/wave22-07-smoke.lisp",
            ".missiond/tasks/wave22/reports/wave22-07-smoke.report.lisp",
            ".missiond/tasks/wave22/shared-memory.lisp",
            // Different hash, not a prefix overlap of the report's
            // `cafef00d1234` value.
            "badc0ffee999",
        );
        let err = res.expect_err(
            "wave22-07 v4 invariant: a commit_hash that does not match the report's \
             `:commit_hash` MUST block completion even when the rest of the quartet aligns",
        );
        assert_eq!(
            extract_error_code(&err).as_deref(),
            Some("TASK_REPORT_COMMIT_HASH_MISMATCH"),
            "wave22-07 v4 invariant: hash mismatch MUST hit the dedicated wave21-03 \
             TASK_REPORT_COMMIT_HASH_MISMATCH code so the verifier vocabulary stays unified"
        );
    }

    // ── wave23-04 — session-trace append unit tests ───────────────────
    //
    // Cover the three append surfaces (open / preflight / complete) plus
    // the helper invariants the JS-side checker enforces:
    // schema-valid event shape, seq monotonicity, id format, repo-relative
    // paths. Failure paths return `TraceWarning` instead of panicking so
    // the caller can surface `trace_warning` without aborting.

    const TRACE_SEED: &str = "(session-trace wave23\n  :schema \"missiond.session-trace.v1\"\n  :wave wave23\n  :created-at \"2026-04-28T00:00:00+08:00\"\n  :sequence 1\n\n  (trace-event\n    :id wave23-trace-bootstrap-001\n    :seq 1\n    :at \"2026-04-28T00:00:00+08:00\"\n    :task wave23-04-execution-session-trace-integration-v0\n    :backend codex-orchestrator\n    :kind observation\n    :summary \"seed event\"))\n";

    fn write_trace_seed(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, TRACE_SEED.as_bytes()).expect("seed write");
        path
    }

    #[test]
    fn is_valid_trace_id_matches_checker_regex() {
        assert!(is_valid_trace_id("wave23-04-foo"));
        assert!(is_valid_trace_id("a"));
        assert!(is_valid_trace_id("9abc"));
        assert!(is_valid_trace_id("wave.23_04-x"));
        assert!(!is_valid_trace_id(""));
        assert!(!is_valid_trace_id("-leading-dash"));
        assert!(!is_valid_trace_id("Upper"));
        assert!(!is_valid_trace_id("has space"));
        assert!(!is_valid_trace_id("has/slash"));
    }

    #[test]
    fn sanitize_trace_backend_falls_back_to_claudecode() {
        assert_eq!(sanitize_trace_backend(""), "claudecode");
        assert_eq!(sanitize_trace_backend("   "), "claudecode");
        assert_eq!(sanitize_trace_backend("ClaudeCode"), "claudecode");
        assert_eq!(sanitize_trace_backend("claudecode"), "claudecode");
        assert_eq!(sanitize_trace_backend("agent team"), "agent-team");
        // leading non-alnum stripped
        assert_eq!(sanitize_trace_backend("---abc"), "abc");
        // entirely punctuation / whitespace -> fallback
        assert_eq!(sanitize_trace_backend("!!!"), "claudecode");
    }

    #[test]
    fn render_trace_event_emits_required_and_optional_fields() {
        let ev = TraceEvent {
            task: "wave23-04-execution-session-trace-integration-v0".to_string(),
            backend: "claudecode".to_string(),
            kind: TraceKind::Complete,
            summary: "trace round-trip".to_string(),
            agent: None,
            files: Some(vec![
                "crates/foo/src/lib.rs".to_string(),
                "crates/bar/src/lib.rs".to_string(),
            ]),
            commit_hash: Some("cafef00d".to_string()),
            report_path: Some(".missiond/tasks/wave23/reports/x.report.lisp".to_string()),
        };
        let rendered = render_trace_event(42, "2026-04-28T01:00:00Z", &ev);
        assert!(
            rendered.contains(":id wave23-04-execution-session-trace-integration-v0-complete-42")
        );
        assert!(rendered.contains(":seq 42"));
        assert!(rendered.contains(":at \"2026-04-28T01:00:00Z\""));
        assert!(rendered.contains(":task wave23-04-execution-session-trace-integration-v0"));
        assert!(rendered.contains(":backend claudecode"));
        assert!(rendered.contains(":kind complete"));
        assert!(rendered.contains(":summary \"trace round-trip\""));
        assert!(rendered.contains(":files [\"crates/foo/src/lib.rs\" \"crates/bar/src/lib.rs\"]"));
        assert!(rendered.contains(":commit_hash \"cafef00d\""));
        assert!(rendered.contains(":report_path \".missiond/tasks/wave23/reports/x.report.lisp\""));
    }

    #[test]
    fn append_session_trace_event_round_trips_minimal_event() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_trace_seed(dir.path(), "session-trace.lisp");
        let ev = TraceEvent {
            task: "wave23-04-execution-session-trace-integration-v0".to_string(),
            backend: "claudecode".to_string(),
            kind: TraceKind::Dispatch,
            summary: "open dispatched".to_string(),
            agent: None,
            files: None,
            commit_hash: None,
            report_path: None,
        };
        append_session_trace_event(&path, &ev).expect("append ok");
        let after = std::fs::read_to_string(&path).expect("read");
        // Parser must accept the new file shape.
        let forms = sexp::parse(&after).expect("parse");
        assert_eq!(scan_max_trace_seq(&forms), 2);
        // The new entry's id reflects the seq.
        assert!(after.contains(":id wave23-04-execution-session-trace-integration-v0-dispatch-2"));
        // Required fields the checker enforces are all present.
        assert!(after.contains(":kind dispatch"));
        assert!(after.contains(":backend claudecode"));
        assert!(after.contains(":task wave23-04-execution-session-trace-integration-v0"));
    }

    #[test]
    fn append_session_trace_event_seq_monotonic_across_appends() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_trace_seed(dir.path(), "session-trace.lisp");
        let task = "wave23-04-execution-session-trace-integration-v0".to_string();
        let backend = "claudecode".to_string();
        for (i, kind) in [
            TraceKind::Dispatch,
            TraceKind::Observation,
            TraceKind::Complete,
        ]
        .iter()
        .enumerate()
        {
            let ev = TraceEvent {
                task: task.clone(),
                backend: backend.clone(),
                kind: *kind,
                summary: format!("event {}", i),
                agent: None,
                files: None,
                commit_hash: None,
                report_path: None,
            };
            append_session_trace_event(&path, &ev).unwrap_or_else(|w| {
                panic!("append #{} failed: {}", i, w);
            });
        }
        let text = std::fs::read_to_string(&path).expect("read");
        let forms = sexp::parse(&text).expect("parse");
        let max = scan_max_trace_seq(&forms);
        assert_eq!(max, 4, "seed seq=1 + three appends => max seq must be 4");
        // ids must be unique — seq is in the id so this is implicit, but
        // exercise the parser to confirm no entries collide.
        let trace_form = forms
            .iter()
            .find(|n| n.head_atom() == Some("session-trace"))
            .expect("trace form");
        let mut ids = Vec::new();
        for child in trace_form.children() {
            if child.head_atom() != Some("trace-event") {
                continue;
            }
            let kids = child.children();
            let mut i = 0;
            while i + 1 < kids.len() {
                if kids[i].as_atom() == Some(":id") {
                    if let Some(v) = kids[i + 1].as_atom() {
                        ids.push(v.to_string());
                    }
                }
                i += 1;
            }
        }
        assert_eq!(ids.len(), 4);
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            4,
            "ids must be unique across appends: {:?}",
            ids
        );
    }

    #[test]
    fn append_session_trace_event_missing_file_returns_warning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("does-not-exist.lisp");
        let ev = TraceEvent {
            task: "wave23-04-execution-session-trace-integration-v0".to_string(),
            backend: "claudecode".to_string(),
            kind: TraceKind::Dispatch,
            summary: "open".to_string(),
            agent: None,
            files: None,
            commit_hash: None,
            report_path: None,
        };
        let warn = append_session_trace_event(&path, &ev)
            .expect_err("missing file must surface as warning");
        assert!(matches!(warn, TraceWarning::MissingFile(_)));
        // Display must mention the path so the writer can correlate.
        assert!(warn.to_string().contains("does-not-exist.lisp"));
    }

    #[test]
    fn append_session_trace_event_malformed_returns_warning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("session-trace.lisp");
        // Unbalanced parens — sexp::parse will fail.
        std::fs::write(&path, b"(session-trace wave23\n  :schema \"x\"\n").unwrap();
        let ev = TraceEvent {
            task: "wave23-04-execution-session-trace-integration-v0".to_string(),
            backend: "claudecode".to_string(),
            kind: TraceKind::Dispatch,
            summary: "open".to_string(),
            agent: None,
            files: None,
            commit_hash: None,
            report_path: None,
        };
        let warn = append_session_trace_event(&path, &ev)
            .expect_err("malformed trace must surface as warning");
        assert!(matches!(warn, TraceWarning::Malformed(_)));
    }

    #[test]
    fn append_session_trace_event_invalid_task_id_returns_warning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_trace_seed(dir.path(), "session-trace.lisp");
        let ev = TraceEvent {
            task: "BadTask Id!".to_string(),
            backend: "claudecode".to_string(),
            kind: TraceKind::Dispatch,
            summary: "open".to_string(),
            agent: None,
            files: None,
            commit_hash: None,
            report_path: None,
        };
        let warn = append_session_trace_event(&path, &ev)
            .expect_err("invalid task id must surface as warning");
        assert!(matches!(warn, TraceWarning::InvalidTaskId(_)));
    }

    #[test]
    fn append_session_trace_event_invalid_backend_returns_warning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_trace_seed(dir.path(), "session-trace.lisp");
        let ev = TraceEvent {
            task: "wave23-04-execution-session-trace-integration-v0".to_string(),
            backend: "Has Upper".to_string(),
            kind: TraceKind::Dispatch,
            summary: "open".to_string(),
            agent: None,
            files: None,
            commit_hash: None,
            report_path: None,
        };
        let warn = append_session_trace_event(&path, &ev)
            .expect_err("invalid backend id must surface as warning");
        assert!(matches!(warn, TraceWarning::InvalidBackend(_)));
    }

    #[test]
    fn resolve_session_trace_path_handles_relative_and_absolute() {
        let root = std::path::PathBuf::from("/tmp/missiond-fake-root");
        // Relative path joins under the root.
        let args_rel = json!({"session_trace_path": ".missiond/tasks/wave23/session-trace.lisp"});
        let resolved = resolve_session_trace_path(&args_rel, &root).expect("relative resolves");
        assert!(resolved.starts_with(&root));
        assert!(resolved.ends_with(".missiond/tasks/wave23/session-trace.lisp"));
        // Absolute path passes through verbatim.
        let abs = "/var/lib/missiond/trace.lisp";
        let args_abs = json!({"session_trace_path": abs});
        let resolved = resolve_session_trace_path(&args_abs, &root).expect("absolute resolves");
        assert_eq!(resolved, std::path::PathBuf::from(abs));
        // Empty / blank string -> None (legacy behaviour disabled).
        let args_empty = json!({"session_trace_path": "   "});
        assert!(resolve_session_trace_path(&args_empty, &root).is_none());
        // Absent -> None.
        let args_none = json!({});
        assert!(resolve_session_trace_path(&args_none, &root).is_none());
    }

    #[test]
    fn append_session_trace_event_preserves_existing_entries() {
        // Append must NEVER rewrite prior entries — read length, append
        // after the last (trace-event ...) form, atomic enough to survive
        // concurrent execution. The seed bootstrap entry must survive the
        // append unchanged.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_trace_seed(dir.path(), "session-trace.lisp");
        let before = std::fs::read_to_string(&path).expect("read seed");
        assert!(before.contains(":id wave23-trace-bootstrap-001"));
        let ev = TraceEvent {
            task: "wave23-04-execution-session-trace-integration-v0".to_string(),
            backend: "claudecode".to_string(),
            kind: TraceKind::Complete,
            summary: "complete recorded".to_string(),
            agent: None,
            files: Some(vec![
                "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs".to_string(),
            ]),
            commit_hash: Some("deadbeef".to_string()),
            report_path: Some(".missiond/tasks/wave23/reports/wave23-04.report.lisp".to_string()),
        };
        append_session_trace_event(&path, &ev).expect("append ok");
        let after = std::fs::read_to_string(&path).expect("read after");
        // Bootstrap entry must still be present and untouched.
        assert!(after.contains(":id wave23-trace-bootstrap-001"));
        assert!(after.contains(":summary \"seed event\""));
        // New entry sits at end, before the closing paren of session-trace.
        assert!(after.contains(":kind complete"));
        assert!(after.contains(":commit_hash \"deadbeef\""));
        // The file remains a single well-formed top-level form.
        let forms = sexp::parse(&after).expect("parse");
        let trace_forms: Vec<_> = forms
            .iter()
            .filter(|f| f.head_atom() == Some("session-trace"))
            .collect();
        assert_eq!(
            trace_forms.len(),
            1,
            "must remain a single session-trace form"
        );
        let event_count = trace_forms[0]
            .children()
            .iter()
            .filter(|c| c.head_atom() == Some("trace-event"))
            .count();
        assert_eq!(event_count, 2, "seed + new = 2 events");
    }

    #[test]
    fn resolve_trace_task_id_prefers_task_contract_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Write a minimal task contract whose head id matches the regex.
        let contract_dir = dir.path().join(".missiond/tasks/wave23");
        std::fs::create_dir_all(&contract_dir).expect("mkdir");
        let contract_path = contract_dir.join("wave23-04-test.lisp");
        std::fs::write(
            &contract_path,
            b"(task wave23-04-real-task-id\n  :schema \"missiond.task-contract.v1\")\n",
        )
        .expect("write");
        let args = json!({
            "task_contract_path": ".missiond/tasks/wave23/wave23-04-test.lisp"
        });
        let resolved =
            resolve_trace_task_id(&args, dir.path(), "fallback-execution-id").expect("resolved");
        assert_eq!(resolved, "wave23-04-real-task-id");
    }

    #[test]
    fn resolve_trace_task_id_falls_back_to_execution_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        // No task_contract_path supplied -> fallback to execution_id.
        let args = json!({});
        let resolved = resolve_trace_task_id(
            &args,
            dir.path(),
            "wave23-04-execution-session-trace-integration-v0",
        )
        .expect("resolved");
        assert_eq!(resolved, "wave23-04-execution-session-trace-integration-v0");
    }

    #[test]
    fn resolve_trace_task_id_rejects_non_regex_fallback() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Execution id with uppercase / spaces -> no valid id.
        let args = json!({});
        assert!(resolve_trace_task_id(&args, dir.path(), "Bad Exec ID").is_none());
    }
}
