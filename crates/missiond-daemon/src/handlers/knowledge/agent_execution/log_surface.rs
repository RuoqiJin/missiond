use crate::state::AppState;
use anyhow::{anyhow, Result};
use chrono::{SecondsFormat, Utc};
use missiond_core::event::events::ExecutionEvent;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::warn;

use super::completion_audit::read_task_contract_id;
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

use self::sexp::{Node, NodeKind};

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

pub(super) fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

// ───────────────────────────────────────────────────────────────────────
// minimal S-expression parser with byte spans
// ───────────────────────────────────────────────────────────────────────

/// Parse a `(:key value :key value ...)` style argument tail into a map of
/// keyword (without leading `:`) to the raw source slice covering the value.
pub(super) fn parse_kv_pairs<'a>(src: &'a str, kids: &[Node]) -> HashMap<String, String> {
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

pub(super) fn lisp_quote_string(s: &str) -> String {
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

pub(super) fn render_canonical_template(
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
pub(super) enum Counter {
    Claim,
    Deviation,
    Decision,
    Issue,
    Completion,
}

impl Counter {
    pub(super) fn key(self) -> &'static str {
        match self {
            Counter::Claim => "next-claim-id",
            Counter::Deviation => "next-deviation-id",
            Counter::Decision => "next-decision-id",
            Counter::Issue => "next-issue-id",
            Counter::Completion => "next-completion-id",
        }
    }

    pub(super) fn prefix(self) -> &'static str {
        match self {
            Counter::Claim => "C",
            Counter::Deviation => "D",
            Counter::Decision => "DC",
            Counter::Issue => "I",
            Counter::Completion => "COMP",
        }
    }

    pub(super) fn block_name(self) -> &'static str {
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
pub(super) fn locate_kv_value(src: &str, block: &Node, key: &str) -> Option<(usize, usize, usize)> {
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
pub(super) fn allocate_id(file: &mut LogFile, counter: Counter) -> Result<String> {
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

pub(super) fn scan_max_id(file: &LogFile, counter: Counter) -> u32 {
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
pub(super) fn append_to_block(
    file: &mut LogFile,
    block_name: &str,
    entry_text: &str,
) -> Result<()> {
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
pub(super) fn touch_last_updated(file: &mut LogFile) -> Result<()> {
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

pub(super) fn write_log_file(path: &Path, file: &LogFile) -> Result<()> {
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

pub(super) fn read_log_file(path: &Path) -> Result<LogFile> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("cannot read {}: {}", path.display(), e))?;
    LogFile::parse(text)
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
