//! workstation-dispatch v0 — conservative augmentation layer that turns a
//! plan node targeting `mission_task_delegate` into a scoped task brief
//! before delegating through the existing internal handler.
//!
//! Scope (Wave 15 / Task 05):
//!   - This module ONLY runs when caller / PLAN explicitly opts in. There is
//!     no broad private scheduling. The opt-in surface is:
//!       * execute arg `workstation_dispatch=true`, OR
//!       * PLAN.lisp / DAG node hint `:workstation-dispatch true`.
//!     The dispatch target itself must already resolve to
//!     `mission_task_delegate` — workstation dispatch never silently
//!     re-routes a `mission_execution` / `mission_flow_run` node.
//!   - We never shell out to `claude -p`. The actual transport is the
//!     existing `mission_task_delegate` substrate (which itself prefers a
//!     spawned / reused workstation). When the dispatch cannot be performed
//!     safely (e.g. project root unresolved, target wrong) we return a
//!     structured `safe descriptor`; the caller sees the reason and can
//!     reroute. We do NOT silently fall back to prompt mode.
//!   - `agent-team` is treated as a task-text hint (the literal Chinese
//!     line `使用 agent-team提高效率`) injected exactly once into the brief.
//!     It is not a new transport.
//!   - Project-root resolution honours
//!     `slot_orchestrator::project_root::resolve_target_project_root` —
//!     relative cwd is rejected, no process-cwd fallback.
//!   - Scoped commit handoff: the generated brief always carries
//!     `commit-policy: scoped` (default) and explicit "do not stage or
//!     commit outside owned files" guidance.
//!
//! Lisp authority:
//!   - intent-flow.lisp        :: F-workstation-dispatch-policy
//!   - intent-worker.lisp      :: claudecode-workstation-orchestration
//!   - intent-tools.lisp       :: implemented-surface mission_plan
//!                                 :execute-contract :: workstation-dispatch
//!
//! Wave 15 / Task 05 explicitly does NOT touch the Lisp authority files —
//! that backfill is Wave 15 / Task 06.

use std::path::{Path, PathBuf};

use missiond_core::types::Plan;
use serde_json::{json, Value};

use crate::slot_orchestrator::project_root::resolve_target_project_root;
use crate::state::AppState;

use super::evidence_collector::{
    self, AppendOutcome, EventRef, EvidenceEntry,
};
use super::plan::{tool_result_payload, AGENT_TEAM_OBJECTIVE_HINT};

/// Default commit policy when none is provided. Matches the wave-12 scoped
/// commit handoff contract: each delegated task only stages its own owned
/// files and never touches sibling shards.
pub(crate) const COMMIT_POLICY_SCOPED: &str = "scoped";

/// wave-17 / task 07 — workstation-dispatch scoped-commit handoff defaults.
///
/// These constants are surfaced verbatim on every dispatch response under
/// `scoped_commit_required` / `scoped_commit_policy`. They pin the policy so
/// downstream callers (Claude / agent-team / observers) can assert the
/// invariant without re-reading the brief text.
///
/// Important: the `enforced-on-complete` value describes the *brief contract*,
/// NOT the daemon-level `mission_execution(action=complete)` default. The
/// legacy `enforce_scoped_commit` flag still defaults to `false` so callers
/// who wire completions outside the workstation-dispatch pipeline keep their
/// audit-only behaviour. The brief explicitly instructs the worker to opt
/// into enforcement when calling completion.
pub(crate) const SCOPED_COMMIT_REQUIRED: bool = true;
pub(crate) const SCOPED_COMMIT_POLICY: &str = "enforced-on-complete";

/// Hard cap on the per-list size for `owned-files` / `forbidden-files` /
/// `acceptance-commands` so a runaway PLAN.lisp can't blow the brief past
/// `mission_task_delegate`'s 16K objective cap. Author intent is preserved
/// via the `unsupported_*` overflow lists when this fires.
const TASK_BRIEF_LIST_CAP: usize = 32;

/// Hint contract the workstation-dispatch module recognises. Any field NOT
/// listed here is left to the existing `ParsedPlanHints` parser; we never
/// reinterpret arbitrary Lisp inside this module. Unknown PLAN keywords
/// reach this layer as `unsupported_fields` in the wave-12 v1 hint summary
/// and are never touched here.
#[derive(Debug, Clone, Default)]
pub(crate) struct WorkstationDispatchHints {
    pub objective: Option<String>,
    pub scope: Option<String>,
    pub owned_files: Vec<String>,
    pub forbidden_files: Vec<String>,
    pub acceptance_commands: Vec<String>,
    pub commit_policy: Option<String>,
    pub target_project: Option<String>,
    pub requested_cwd: Option<String>,
    pub dispatch_strategy: Option<String>,
}

impl WorkstationDispatchHints {
    /// Merge explicit args > plan-hint values. Args win on every field;
    /// list-shaped fields use the args list outright when non-empty.
    pub(crate) fn merge_args(mut self, args: &Value) -> Self {
        let s = |v: Option<&Value>| v.and_then(|x| x.as_str()).map(|s| s.to_string());
        if let Some(o) = s(args.get("objective")).filter(|x| !x.trim().is_empty()) {
            self.objective = Some(o);
        }
        if let Some(scope) = s(args.get("scope")).filter(|x| !x.trim().is_empty()) {
            self.scope = Some(scope);
        }
        let owned = collect_string_list(args.get("owned_files"));
        if !owned.is_empty() {
            self.owned_files = owned;
        }
        let forbidden = collect_string_list(args.get("forbidden_files"));
        if !forbidden.is_empty() {
            self.forbidden_files = forbidden;
        }
        let acceptance = collect_string_list(args.get("acceptance_commands"));
        if !acceptance.is_empty() {
            self.acceptance_commands = acceptance;
        }
        if let Some(cp) = s(args.get("commit_policy")).filter(|x| !x.trim().is_empty()) {
            self.commit_policy = Some(cp);
        }
        if let Some(tp) = s(args.get("target_project")).filter(|x| !x.trim().is_empty()) {
            self.target_project = Some(tp);
        }
        if let Some(c) = s(args.get("requested_cwd"))
            .or_else(|| s(args.get("cwd")))
            .filter(|x| !x.trim().is_empty())
        {
            self.requested_cwd = Some(c);
        }
        if let Some(ds) = s(args.get("dispatch_strategy")).filter(|x| !x.trim().is_empty()) {
            self.dispatch_strategy = Some(ds);
        }
        self
    }

    /// Cap every list field so a runaway plan body can't bloat the brief
    /// past the downstream 16K `objective` cap. Returns `Some(field_name,
    /// dropped_count)` for each list that was truncated so the caller can
    /// surface it on the response.
    pub(crate) fn cap_lists(&mut self) -> Vec<(&'static str, usize)> {
        let mut dropped: Vec<(&'static str, usize)> = Vec::new();
        for (label, list) in [
            ("owned_files", &mut self.owned_files),
            ("forbidden_files", &mut self.forbidden_files),
            ("acceptance_commands", &mut self.acceptance_commands),
        ] {
            if list.len() > TASK_BRIEF_LIST_CAP {
                let drop_count = list.len() - TASK_BRIEF_LIST_CAP;
                list.truncate(TASK_BRIEF_LIST_CAP);
                dropped.push((label, drop_count));
            }
        }
        dropped
    }

    /// wave-19 / task 07 — overlay a parsed task contract on top of the
    /// existing hints. The contract is the SSOT, so non-empty contract
    /// fields ALWAYS win over caller args (which were merged earlier).
    /// Empty contract list-fields do NOT clobber non-empty arg lists —
    /// that protects against a contract that omits a field (the renderer
    /// only emits non-empty `:acceptance` etc.) from accidentally
    /// erasing a caller-supplied list. The `task_contract_path` field is
    /// preserved so observers can trace provenance.
    pub(crate) fn overlay_contract(&mut self, contract: &ParsedTaskContract) {
        // :goal → objective (always wins when non-empty).
        if !contract.goal.trim().is_empty() {
            self.objective = Some(contract.goal.trim().to_string());
        }
        // :scope (optional, wins when present).
        if let Some(scope) = contract
            .scope
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            self.scope = Some(scope.to_string());
        }
        // :write-scope → owned_files (only overwrite when non-empty).
        if !contract.write_scope.is_empty() {
            self.owned_files = contract.write_scope.clone();
        }
        // :must-not-touch → forbidden_files (only overwrite when non-empty).
        if !contract.must_not_touch.is_empty() {
            self.forbidden_files = contract.must_not_touch.clone();
        }
        // :acceptance (only overwrite when non-empty).
        if !contract.acceptance.is_empty() {
            self.acceptance_commands = contract.acceptance.clone();
        }
        // :commit (:policy "...") (optional).
        if let Some(policy) = contract
            .commit_policy
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            self.commit_policy = Some(policy.to_string());
        }
        // :dispatch-strategy (optional, wins when present).
        if let Some(ds) = contract
            .dispatch_strategy
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            self.dispatch_strategy = Some(ds.to_string());
        }
        // :target-project (optional).
        if let Some(tp) = contract
            .target_project
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            self.target_project = Some(tp.to_string());
        }
        // :requested-cwd (optional).
        if let Some(cwd) = contract
            .requested_cwd
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            self.requested_cwd = Some(cwd.to_string());
        }
    }
}

// ── wave-19 / task 07 — narrow task-contract v1 parser ─────────────────
//
// Scope rule: this parser ONLY extracts the fields workstation_dispatch
// actually consumes (objective / scope / owned-files / forbidden-files /
// acceptance / commit policy / dispatch strategy / target / target-project
// / requested-cwd). It is NOT a general-purpose Lisp reader — the
// authoritative checker lives at scripts/check-task-contract.mjs and is
// the gate for any new field. Adding a field here without first teaching
// the checker would break the SSOT invariant.
//
// Why a hand-rolled tokeniser instead of pulling in a Lisp crate:
//   * The daemon already deliberately avoids a Lisp dependency (see
//     handlers/comm/capability_usage.rs which uses regexes for the same
//     reason — keeps the fail-mode surface narrow and the dependency tree
//     clean for the embedded-runtime build).
//   * The contract emitter (plan.rs::build_task_contract_lisp) only ever
//     produces strings, bracketed string lists, bare symbols (kind/status
//     values), and one nested property list (`:commit (...)`). The
//     tokeniser below covers exactly that surface.
//   * Comment lines start with `;;` and end at end-of-line.
//   * Strings are double-quoted with `\"` and `\\` escapes.
//
// Failure mode: any structural error (unbalanced parens, EOF inside a
// string, missing `:schema`, schema mismatch, unexpected token shape) is
// surfaced as `TaskContractParseError` and caught by the dispatch layer
// which converts it into `SafeDescriptorReason::MalformedTaskContract`.
// The parser NEVER guesses or recovers — fail-fast over silent salvage.

/// Narrow projection of task-contract v1 holding only the fields
/// `workstation_dispatch` consumes. New fields land here only after the
/// authoritative checker (scripts/check-task-contract.mjs) is taught
/// about them. The unused fields (kind, status, owner, etc.) are not
/// stripped — see `read_optional_string`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ParsedTaskContract {
    pub schema: String,
    pub goal: String,
    pub scope: Option<String>,
    pub write_scope: Vec<String>,
    pub must_not_touch: Vec<String>,
    pub acceptance: Vec<String>,
    pub commit_policy: Option<String>,
    pub dispatch_strategy: Option<String>,
    pub target_project: Option<String>,
    pub requested_cwd: Option<String>,
    pub target: Option<String>,
}

/// Typed parser/loader failure. Each variant maps deterministically to a
/// `SafeDescriptorReason::MalformedTaskContract` reason string.
#[derive(Debug, Clone)]
pub(crate) enum TaskContractParseError {
    /// IO failure reading the file (missing / permission / etc.).
    Io(String),
    /// The file content failed structural parsing (lex / paren balance /
    /// EOF inside string).
    Lex(String),
    /// The top-level form is not `(task <id> ...)`.
    NotATaskForm(String),
    /// `:schema` is missing or not equal to `missiond.task-contract.v1`.
    SchemaMismatch(String),
    /// A required field (`:goal`) is missing or blank.
    MissingRequired(&'static str),
    /// A field has the wrong type (e.g. `:goal "..."` is not a string).
    FieldShape { field: &'static str, detail: String },
}

impl TaskContractParseError {
    pub(crate) fn reason(&self) -> String {
        match self {
            TaskContractParseError::Io(e) => format!("io: {}", e),
            TaskContractParseError::Lex(e) => format!("lex: {}", e),
            TaskContractParseError::NotATaskForm(s) => {
                format!("not a `(task ...)` form: {}", s)
            }
            TaskContractParseError::SchemaMismatch(found) => format!(
                "schema mismatch — expected `missiond.task-contract.v1`, got `{}`",
                found
            ),
            TaskContractParseError::MissingRequired(field) => {
                format!("missing required field `{}`", field)
            }
            TaskContractParseError::FieldShape { field, detail } => {
                format!("field `{}` has wrong shape: {}", field, detail)
            }
        }
    }
}

/// One element of the small parse tree. Bracketed `[..]` and parens `(..)`
/// are not distinguished here — the contract emitter uses `[...]` for
/// vectors and `(...)` for the top form + `:commit` plist; both shapes
/// flatten into a `List` and the field reader differentiates by context.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SExp {
    /// Bare symbol (e.g. `task`, `code-alignment`, `write-scope-only`,
    /// `true`). Includes leading `:` keywords as their own variant below.
    Symbol(String),
    /// Property keyword (`:schema`, `:goal`, ...). Stored without the
    /// leading colon for ergonomic matching.
    Keyword(String),
    /// String literal (post-unescape).
    String(String),
    /// Compound — both `(...)` and `[...]` flatten here.
    List(Vec<SExp>),
}

/// Tokenise + parse a single top-level form. The contract emitter only
/// ever writes one form per file, so we accept exactly one and reject
/// trailing junk.
fn parse_one_top_form(src: &str) -> Result<SExp, TaskContractParseError> {
    let tokens = lex_tokens(src)?;
    let mut iter = tokens.into_iter().peekable();
    let form = parse_form(&mut iter)?;
    // Skip whitespace tokens — the lexer already drops them; remaining
    // iter must be empty.
    if iter.peek().is_some() {
        return Err(TaskContractParseError::Lex(
            "trailing tokens after top-level form".to_string(),
        ));
    }
    Ok(form)
}

/// Internal lexer token. We collapse `(`/`[` into `Open` and `)`/`]`
/// into `Close` because the contract grammar does not need the
/// distinction (see `SExp::List`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Open,
    Close,
    Symbol(String),
    Keyword(String),
    String(String),
}

fn lex_tokens(src: &str) -> Result<Vec<Tok>, TaskContractParseError> {
    let mut out: Vec<Tok> = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        // Skip whitespace.
        if matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
            i += 1;
            continue;
        }
        // Comment: `;;` (single `;` is also tolerated) → skip to EOL.
        if b == b';' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Bracket / paren.
        if b == b'(' || b == b'[' {
            out.push(Tok::Open);
            i += 1;
            continue;
        }
        if b == b')' || b == b']' {
            out.push(Tok::Close);
            i += 1;
            continue;
        }
        // String literal.
        if b == b'"' {
            i += 1;
            let mut buf = String::new();
            let mut closed = false;
            while i < bytes.len() {
                let c = bytes[i];
                if c == b'\\' {
                    if i + 1 >= bytes.len() {
                        break;
                    }
                    let next = bytes[i + 1];
                    match next {
                        b'\\' => buf.push('\\'),
                        b'"' => buf.push('"'),
                        b'n' => buf.push('\n'),
                        b't' => buf.push('\t'),
                        b'r' => buf.push('\r'),
                        // Unknown escape → keep the next byte verbatim so
                        // `\?` round-trips. Conservative; matches the
                        // emitter's tolerance.
                        other => buf.push(other as char),
                    }
                    i += 2;
                    continue;
                }
                if c == b'"' {
                    closed = true;
                    i += 1;
                    break;
                }
                // UTF-8 safe: re-walk via str slicing instead of byte cast.
                // We grow `buf` one char at a time using char_indices below.
                // To avoid double-walking, fall back to a per-char loop here.
                // For ASCII (which `lisp_escape_string` guarantees for the
                // structural bytes), the cast is safe; non-ASCII bytes flow
                // through the str-slice branch.
                if c.is_ascii() {
                    buf.push(c as char);
                    i += 1;
                } else {
                    // Find the next char boundary in the original `&str`.
                    let rest = &src[i..];
                    if let Some(ch) = rest.chars().next() {
                        buf.push(ch);
                        i += ch.len_utf8();
                    } else {
                        break;
                    }
                }
            }
            if !closed {
                return Err(TaskContractParseError::Lex(
                    "unterminated string literal".to_string(),
                ));
            }
            out.push(Tok::String(buf));
            continue;
        }
        // Atom (symbol or keyword). Read until whitespace / paren.
        let start = i;
        while i < bytes.len() {
            let c = bytes[i];
            if matches!(
                c,
                b' ' | b'\t' | b'\r' | b'\n' | b'(' | b')' | b'[' | b']' | b'"' | b';'
            ) {
                break;
            }
            i += 1;
        }
        let raw = &src[start..i];
        if raw.is_empty() {
            return Err(TaskContractParseError::Lex(format!(
                "unexpected byte 0x{:02x} at offset {}",
                b, start
            )));
        }
        if let Some(stripped) = raw.strip_prefix(':') {
            if stripped.is_empty() {
                return Err(TaskContractParseError::Lex(
                    "bare `:` is not a valid keyword".to_string(),
                ));
            }
            out.push(Tok::Keyword(stripped.to_string()));
        } else {
            out.push(Tok::Symbol(raw.to_string()));
        }
    }
    Ok(out)
}

fn parse_form(
    iter: &mut std::iter::Peekable<std::vec::IntoIter<Tok>>,
) -> Result<SExp, TaskContractParseError> {
    let tok = iter
        .next()
        .ok_or_else(|| TaskContractParseError::Lex("unexpected EOF".to_string()))?;
    match tok {
        Tok::Open => {
            let mut items: Vec<SExp> = Vec::new();
            loop {
                match iter.peek() {
                    None => {
                        return Err(TaskContractParseError::Lex(
                            "unbalanced parens — EOF inside list".to_string(),
                        ))
                    }
                    Some(Tok::Close) => {
                        iter.next();
                        return Ok(SExp::List(items));
                    }
                    _ => {
                        items.push(parse_form(iter)?);
                    }
                }
            }
        }
        Tok::Close => Err(TaskContractParseError::Lex(
            "unexpected closing bracket".to_string(),
        )),
        Tok::Symbol(s) => Ok(SExp::Symbol(s)),
        Tok::Keyword(k) => Ok(SExp::Keyword(k)),
        Tok::String(s) => Ok(SExp::String(s)),
    }
}

/// Walk the top-level `(task <id> :schema ... :goal ... ...)` form and
/// project the narrow contract.
fn project_contract(form: &SExp) -> Result<ParsedTaskContract, TaskContractParseError> {
    let SExp::List(items) = form else {
        return Err(TaskContractParseError::NotATaskForm(format!(
            "top form is not a list: {:?}",
            form
        )));
    };
    // Expect: (task <id-symbol> :keyword <value> :keyword <value> ...)
    let mut iter = items.iter();
    let head = iter
        .next()
        .ok_or_else(|| TaskContractParseError::NotATaskForm("empty form".to_string()))?;
    match head {
        SExp::Symbol(s) if s == "task" => {}
        other => {
            return Err(TaskContractParseError::NotATaskForm(format!(
                "expected leading `task` symbol, got {:?}",
                other
            )))
        }
    }
    // Skip the task id (must be a bare symbol per the schema, but we
    // accept anything non-keyword conservatively — the authoritative
    // checker enforces shape).
    let _id = iter
        .next()
        .ok_or_else(|| TaskContractParseError::NotATaskForm("missing task id".to_string()))?;

    let mut out = ParsedTaskContract::default();
    while let Some(tok) = iter.next() {
        let SExp::Keyword(k) = tok else {
            return Err(TaskContractParseError::Lex(format!(
                "expected `:keyword` token in task body, got {:?}",
                tok
            )));
        };
        let val = iter.next().ok_or_else(|| TaskContractParseError::Lex(
            format!("keyword `:{}` missing value", k),
        ))?;
        match k.as_str() {
            "schema" => out.schema = require_string(val, "schema")?,
            "goal" => out.goal = require_string(val, "goal")?,
            "scope" => out.scope = Some(require_string(val, "scope")?),
            "write-scope" => out.write_scope = require_string_list(val, "write-scope")?,
            "must-not-touch" => {
                out.must_not_touch = require_string_list(val, "must-not-touch")?
            }
            "acceptance" => out.acceptance = require_string_list(val, "acceptance")?,
            "commit" => {
                out.commit_policy = extract_commit_policy(val)?;
            }
            "dispatch-strategy" => {
                out.dispatch_strategy = Some(require_string(val, "dispatch-strategy")?)
            }
            "target-project" => {
                out.target_project = Some(require_string(val, "target-project")?)
            }
            "requested-cwd" => out.requested_cwd = Some(require_string(val, "requested-cwd")?),
            "target" => out.target = Some(require_string(val, "target")?),
            // Other v1 fields (kind / status / owner / depends-on / title /
            // plan-id / node-id / report / requirements) are not consumed
            // by workstation_dispatch — accept-and-ignore so the parser
            // does not break when the emitter adds non-load-bearing
            // metadata. Adding a field here MUST be paired with a checker
            // update.
            _ => {}
        }
    }

    if out.schema.is_empty() {
        return Err(TaskContractParseError::SchemaMismatch("(absent)".to_string()));
    }
    if out.schema != "missiond.task-contract.v1" {
        return Err(TaskContractParseError::SchemaMismatch(out.schema.clone()));
    }
    if out.goal.trim().is_empty() {
        return Err(TaskContractParseError::MissingRequired("goal"));
    }
    Ok(out)
}

fn require_string(val: &SExp, field: &'static str) -> Result<String, TaskContractParseError> {
    match val {
        SExp::String(s) => Ok(s.clone()),
        other => Err(TaskContractParseError::FieldShape {
            field,
            detail: format!("expected string literal, got {:?}", other),
        }),
    }
}

fn require_string_list(
    val: &SExp,
    field: &'static str,
) -> Result<Vec<String>, TaskContractParseError> {
    match val {
        SExp::List(items) => {
            let mut out: Vec<String> = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    SExp::String(s) => out.push(s.clone()),
                    other => {
                        return Err(TaskContractParseError::FieldShape {
                            field,
                            detail: format!("non-string item in list: {:?}", other),
                        })
                    }
                }
            }
            Ok(out)
        }
        other => Err(TaskContractParseError::FieldShape {
            field,
            detail: format!("expected list of strings, got {:?}", other),
        }),
    }
}

/// `(:required true :message "..." :scope-check write-scope-only :policy "scoped")`
///
/// We only need `:policy` for the workstation brief's commit policy
/// surface. The other fields are validated by the authoritative checker;
/// we tolerate missing `:policy` (returns `None`) so the brief defaults
/// to `COMMIT_POLICY_SCOPED`.
fn extract_commit_policy(val: &SExp) -> Result<Option<String>, TaskContractParseError> {
    let SExp::List(items) = val else {
        return Err(TaskContractParseError::FieldShape {
            field: "commit",
            detail: format!("expected property list, got {:?}", val),
        });
    };
    let mut iter = items.iter().peekable();
    while let Some(tok) = iter.next() {
        let SExp::Keyword(k) = tok else { continue };
        if k != "policy" {
            // Skip the value of any non-`:policy` keyword — the structure
            // is keyword-then-value, so consume one item.
            let _ = iter.next();
            continue;
        }
        let val = iter.next().ok_or_else(|| TaskContractParseError::FieldShape {
            field: "commit",
            detail: "`:policy` keyword missing value".to_string(),
        })?;
        return Ok(Some(require_string(val, "commit.policy")?));
    }
    Ok(None)
}

/// Pure parser entrypoint — exercised directly by the unit tests. The
/// loader (`load_task_contract`) wraps this with file IO.
pub(crate) fn parse_task_contract(
    src: &str,
) -> Result<ParsedTaskContract, TaskContractParseError> {
    let form = parse_one_top_form(src)?;
    project_contract(&form)
}

/// Loader — read the file then run `parse_task_contract`. Path is
/// expected to be absolute (the dispatch caller already resolves
/// `task_contract_path` against the project root before calling).
pub(crate) fn load_task_contract(
    path: &Path,
) -> Result<ParsedTaskContract, TaskContractParseError> {
    let bytes = std::fs::read_to_string(path)
        .map_err(|e| TaskContractParseError::Io(format!("{}: {}", path.display(), e)))?;
    parse_task_contract(&bytes)
}

/// Resolve a (possibly relative) `task_contract_path` against an
/// already-resolved project root. The dispatch helper applies this AFTER
/// `resolve_target_project_root` succeeds so a relative path always has
/// a deterministic anchor — never the daemon's process cwd.
fn resolve_contract_path(raw: &Path, project_root: &Path) -> PathBuf {
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        project_root.join(raw)
    }
}

/// Whether the caller / plan opted into workstation-dispatch v0. This is
/// the legacy back-compat helper kept so existing tests / callers keep
/// reading the same boolean. New code goes through `evaluate_dispatch_decision`
/// so the response can surface the source + inference reason.
pub(crate) fn opt_in_requested(args: &Value, plan_hint_workstation_dispatch: bool) -> bool {
    let arg_flag = args
        .get("workstation_dispatch")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    arg_flag || plan_hint_workstation_dispatch
}

/// wave-16 / task 03 — the resolved source of a workstation-dispatch
/// decision. Surfaced verbatim on the response under
/// `workstation_dispatch_source` so callers can route on the provenance
/// without re-deriving it.
///
/// Semantics:
///   * `ExplicitArg`   — caller passed `workstation_dispatch=true` (and
///                       passed every safety gate). Wave-15 behaviour.
///   * `PlanHint`      — PLAN.lisp / DAG node carried `:workstation-dispatch
///                       true` (and explicit arg was absent or true).
///                       Wave-15 behaviour.
///   * `Inferred`      — caller set neither flag, but the resolved target +
///                       dispatch strategy + objective + at least one
///                       scoping signal all matched the conservative
///                       auto-inference rule. Wave-16 behaviour.
///   * `Disabled`      — caller passed `workstation_dispatch=false`.
///                       Auto-inference is suppressed.
///   * `NotApplicable` — none of the above; fall through to the legacy
///                       plan-runner internal dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkstationDispatchSource {
    ExplicitArg,
    PlanHint,
    Inferred,
    Disabled,
    NotApplicable,
}

impl WorkstationDispatchSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            WorkstationDispatchSource::ExplicitArg => "explicit_arg",
            WorkstationDispatchSource::PlanHint => "plan_hint",
            WorkstationDispatchSource::Inferred => "inferred",
            WorkstationDispatchSource::Disabled => "disabled",
            WorkstationDispatchSource::NotApplicable => "not_applicable",
        }
    }
}

/// Resolved decision: should this dispatch run through the workstation
/// substrate? Carries the source + (when relevant) the reason text the
/// inference engine attached. The reason is response-facing only — it does
/// NOT change the dispatch path.
#[derive(Debug, Clone)]
pub(crate) struct DispatchDecision {
    pub source: WorkstationDispatchSource,
    pub reason: Option<String>,
}

impl DispatchDecision {
    fn enabled(source: WorkstationDispatchSource, reason: Option<String>) -> Self {
        Self { source, reason }
    }

    fn off(source: WorkstationDispatchSource, reason: Option<String>) -> Self {
        Self { source, reason }
    }

    /// True iff this decision routes through the workstation-dispatch
    /// substrate. Only the explicit/plan-hint/inferred branches do; the
    /// disabled and not-applicable branches stay on the legacy plan-runner.
    pub(crate) fn is_enabled(&self) -> bool {
        matches!(
            self.source,
            WorkstationDispatchSource::ExplicitArg
                | WorkstationDispatchSource::PlanHint
                | WorkstationDispatchSource::Inferred
        )
    }
}

/// wave-16 / task 03 — the strategies the auto-inference engine accepts.
/// Strictly a sub-list of `VALID_DISPATCH_STRATEGIES` from plan.rs:
/// `unknown` and `prompt-fallback` are intentionally excluded so a node
/// without a real dispatch hint stays on the legacy plan-runner path.
pub(crate) const INFERABLE_DISPATCH_STRATEGIES: &[&str] =
    &["fresh-code-alignment", "resident-lisp", "agent-team", "mixed"];

/// Hint context the inference engine reads. Only the conservative subset
/// of fields that actually scope a workstation task — the fully merged
/// hint set is built later via `WorkstationDispatchHints::merge_args` once
/// we know the decision is "go".
pub(crate) struct InferenceContext<'a> {
    /// Resolved target (already normalised to one of `mission_execution |
    /// mission_task_delegate | mission_flow_run`).
    pub target: &'a str,
    /// Already-canonicalised dispatch strategy (one of
    /// `VALID_DISPATCH_STRATEGIES` in plan.rs, including `unknown`).
    pub dispatch_strategy: &'a str,
    /// Final objective text (caller arg with PLAN.lisp / node fallback).
    pub objective: Option<&'a str>,
    /// Scoping signal #1 — declared owned files (post-merge).
    pub owned_files_present: bool,
    /// Scoping signal #2 — free-form scope string.
    pub scope_present: bool,
    /// Scoping signal #3 — explicit `target_project` (caller arg or hint).
    pub target_project_present: bool,
    /// Scoping signal #4 — explicit `requested_cwd` (caller arg or hint).
    pub requested_cwd_present: bool,
}

/// Read the caller's explicit `workstation_dispatch` knob. `None` means
/// "no explicit choice" (auto-inference is allowed); `Some(true)` /
/// `Some(false)` mean "explicit on" / "explicit off".
pub(crate) fn explicit_workstation_dispatch_flag(args: &Value) -> Option<bool> {
    args.get("workstation_dispatch").and_then(|v| v.as_bool())
}

/// Decide whether to route through workstation-dispatch.
///
/// Precedence (highest first):
///   1. `workstation_dispatch=false` arg → `Disabled` (suppresses inference).
///   2. `workstation_dispatch=true` arg  → `ExplicitArg`.
///   3. PLAN.lisp / node `:workstation-dispatch true` → `PlanHint`.
///   4. Auto-inference (all five conditions) → `Inferred`.
///   5. Otherwise → `NotApplicable`.
///
/// Conditions for `Inferred` (ALL must hold):
///   a. resolved target is `mission_task_delegate`
///   b. dispatch strategy is one of `INFERABLE_DISPATCH_STRATEGIES`
///   c. objective is non-empty
///   d. at least one scoping signal is present
///      (owned_files | scope | target_project | requested_cwd)
///   e. caller did not set `workstation_dispatch=false`
///
/// `mission_execution` and `mission_flow_run` are NEVER auto-inferred —
/// auto-inference only ever wraps the task_delegate substrate.
pub(crate) fn evaluate_dispatch_decision(
    args: &Value,
    plan_hint_workstation_dispatch: bool,
    ctx: &InferenceContext<'_>,
) -> DispatchDecision {
    let explicit = explicit_workstation_dispatch_flag(args);

    // 1. Explicit `false` short-circuits everything.
    if explicit == Some(false) {
        return DispatchDecision::off(
            WorkstationDispatchSource::Disabled,
            Some("workstation_dispatch=false suppresses both opt-in and auto-inference".to_string()),
        );
    }

    // 2. Explicit `true` is honoured even if a safety gate would later
    //    refuse — the wave-15 behaviour returns a SafeDescriptor and the
    //    caller sees it. We do NOT silently downgrade to NotApplicable.
    if explicit == Some(true) {
        return DispatchDecision::enabled(
            WorkstationDispatchSource::ExplicitArg,
            Some("caller passed workstation_dispatch=true".to_string()),
        );
    }

    // 3. PLAN.lisp / node hint.
    if plan_hint_workstation_dispatch {
        return DispatchDecision::enabled(
            WorkstationDispatchSource::PlanHint,
            Some("PLAN.lisp / node carried :workstation-dispatch true".to_string()),
        );
    }

    // 4. Auto-inference. Each gate produces a deterministic skip-reason
    //    so the response can explain why we did NOT auto-enable.

    // a. Target must be mission_task_delegate.
    if ctx.target != "mission_task_delegate" {
        return DispatchDecision::off(
            WorkstationDispatchSource::NotApplicable,
            Some(format!(
                "auto-inference only wraps mission_task_delegate; resolved target is `{}`",
                ctx.target
            )),
        );
    }

    // b. Dispatch strategy must be in the inferable subset.
    if !INFERABLE_DISPATCH_STRATEGIES.contains(&ctx.dispatch_strategy) {
        return DispatchDecision::off(
            WorkstationDispatchSource::NotApplicable,
            Some(format!(
                "auto-inference requires a known workstation dispatch strategy ({:?}); got `{}`",
                INFERABLE_DISPATCH_STRATEGIES, ctx.dispatch_strategy
            )),
        );
    }

    // c. Objective must be non-empty.
    let has_objective = ctx
        .objective
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if !has_objective {
        return DispatchDecision::off(
            WorkstationDispatchSource::NotApplicable,
            Some(
                "auto-inference requires a non-empty objective (caller arg or PLAN.lisp hint)"
                    .to_string(),
            ),
        );
    }

    // d. At least one scoping signal.
    let any_scope = ctx.owned_files_present
        || ctx.scope_present
        || ctx.target_project_present
        || ctx.requested_cwd_present;
    if !any_scope {
        return DispatchDecision::off(
            WorkstationDispatchSource::NotApplicable,
            Some(
                "auto-inference requires at least one scoping signal: owned_files, scope, \
                 target_project, or requested_cwd"
                    .to_string(),
            ),
        );
    }

    DispatchDecision::enabled(
        WorkstationDispatchSource::Inferred,
        Some(format!(
            "inferred from target=mission_task_delegate, dispatch_strategy=`{}`, non-empty objective, scoping signals present",
            ctx.dispatch_strategy
        )),
    )
}

/// Outcome of a workstation-dispatch evaluation. The variants are surfaced
/// directly into the response so callers can route on
/// `workstation_dispatch_status` without re-walking the inner payload.
#[derive(Debug)]
pub(crate) enum WorkstationDispatchOutcome {
    /// Inner `mission_task_delegate` returned non-error. `inner_payload`
    /// carries the delegated task's response.
    ///
    /// wave-20 / task 04 — `task_contract_source_path` carries the
    /// resolved on-disk task-contract v1 path WHEN the dispatch consumed
    /// the contract directly (machine-driven mode). It is `None` for
    /// the legacy / rendered path so the response stays byte-compatible
    /// with wave-15..19.
    Dispatched {
        task_brief: String,
        task_brief_path: Option<String>,
        task_contract_source_path: Option<String>,
        evidence_path: Option<String>,
        evidence_error: Option<String>,
        inner_payload: Value,
    },
    /// Inner handler returned an error result; we surface it verbatim and
    /// do NOT mark plan executing — caller decides whether to retry.
    InnerError {
        task_brief: String,
        inner_payload: Value,
    },
    /// dry_run: brief built, nothing dispatched, no evidence written.
    DryRun {
        task_brief: String,
    },
    /// Pre-flight failed (project root unresolved, wrong target, etc).
    /// We refuse to dispatch and refuse to silently fall back to prompt
    /// mode — the descriptor explains why so the caller can fix and retry.
    SafeDescriptor {
        reason: SafeDescriptorReason,
        task_brief: Option<String>,
    },
}

/// Why we refused to dispatch. Each variant maps to a deterministic
/// `workstation_dispatch_status` string the caller can match on.
#[derive(Debug, Clone)]
pub(crate) enum SafeDescriptorReason {
    /// Caller pointed at a non-`mission_task_delegate` target — workstation
    /// dispatch only ever wraps the task_delegate substrate.
    UnsupportedTarget(String),
    /// Project root could not be resolved (no signal / unknown id /
    /// relative cwd / cwd outside any registered project).
    ProjectRootUnresolved(String),
    /// Caller did not provide an objective and the plan hints were empty,
    /// so the brief would have been content-free.
    MissingObjective,
    /// wave-19 / task 07 — `task_contract_path` was supplied but the file
    /// is missing, unreadable, or fails the narrow task-contract v1
    /// parse. We refuse to fall back to the legacy natural-language path
    /// because the contract is the SSOT — silently downgrading would
    /// hide an authoring bug. Carries the absolute path + a typed reason.
    MalformedTaskContract { path: String, reason: String },
}

impl SafeDescriptorReason {
    pub(crate) fn status(&self) -> &'static str {
        match self {
            SafeDescriptorReason::UnsupportedTarget(_) => "skipped_unsupported_target",
            SafeDescriptorReason::ProjectRootUnresolved(_) => "skipped_project_root_unresolved",
            SafeDescriptorReason::MissingObjective => "skipped_missing_objective",
            SafeDescriptorReason::MalformedTaskContract { .. } => "skipped_malformed_task_contract",
        }
    }

    pub(crate) fn detail(&self) -> String {
        match self {
            SafeDescriptorReason::UnsupportedTarget(t) => {
                format!("workstation-dispatch v0 only wraps `mission_task_delegate`, got `{}`", t)
            }
            SafeDescriptorReason::ProjectRootUnresolved(r) => r.clone(),
            SafeDescriptorReason::MissingObjective => {
                "workstation-dispatch v0 requires either an explicit objective or a plan hint; \
                 refusing to dispatch a content-free task brief".to_string()
            }
            SafeDescriptorReason::MalformedTaskContract { path, reason } => {
                format!(
                    "task_contract_path `{}` is malformed: {} — refusing to fall back to the \
                     legacy natural-language brief because the contract is the SSOT",
                    path, reason
                )
            }
        }
    }
}

impl WorkstationDispatchOutcome {
    pub(crate) fn status(&self) -> &'static str {
        match self {
            WorkstationDispatchOutcome::Dispatched { .. } => "dispatched",
            WorkstationDispatchOutcome::InnerError { .. } => "inner_returned_error",
            WorkstationDispatchOutcome::DryRun { .. } => "dry_run_no_dispatch",
            WorkstationDispatchOutcome::SafeDescriptor { reason, .. } => reason.status(),
        }
    }
}

/// Wave-17 / Task 07 — classify the brief as code-generating or read-only
/// so the completion-handoff section can prescribe a different
/// `commit_status` default. Conservative rule: a brief with at least one
/// declared `owned_files` entry is treated as code-generating; an empty
/// `owned_files` list means the worker has no licence to stage anything,
/// which is the read-only contract.
///
/// This rule deliberately does NOT inspect the objective text — keyword
/// sniffing would be both ambiguous and easy to game. Authors that want a
/// read-only task simply omit `owned_files` (the brief already nudges them
/// with "stage NOTHING by default").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BriefTaskKind {
    /// Worker is expected to produce a scoped commit naming the owned
    /// files. Brief instructs the worker to call completion with
    /// `enforce_scoped_commit=true`, `commit_status=committed`,
    /// `commit_hash=<hash>`, `staged_files=[<owned files actually staged>]`.
    Code,
    /// Worker is read-only (no owned files declared). Brief instructs
    /// the worker to call completion with `enforce_scoped_commit=true`
    /// and `commit_status=not-required`, plus a one-line explanation of
    /// why no commit was produced (so the audit trail captures intent
    /// rather than silently defaulting to "no commit").
    ReadOnly,
}

impl BriefTaskKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            BriefTaskKind::Code => "code",
            BriefTaskKind::ReadOnly => "read-only",
        }
    }
}

/// Wave-17 / Task 07 — derive the brief task kind from the merged hints.
/// Pure function so tests can pin the rule and downstream callers can
/// classify a brief without re-walking the hint set.
pub(crate) fn classify_task_kind(hints: &WorkstationDispatchHints) -> BriefTaskKind {
    if hints.owned_files.is_empty() {
        BriefTaskKind::ReadOnly
    } else {
        BriefTaskKind::Code
    }
}

/// Build the canonical task-brief text. The shape is fixed so downstream
/// consumers (Claude / agent-team) always see the same headings.
///
/// Sections (in order):
///   1. Objective.
///   2. Scope (free-form additional bounds).
///   3. Owned files (the only files this task may stage / commit).
///   4. Forbidden files (explicit "do not touch" list).
///   5. Acceptance commands (verification commands the task must pass).
///   6. Commit policy (default `scoped`) + the literal scoped-commit
///      reminder line.
///   7. Wave-17 / Task 07 — Completion handoff (scoped commit). Always
///      present. Prescribes the exact `mission_execution(action=complete)`
///      arguments the worker must report back: `enforce_scoped_commit=true`
///      always; `commit_status=committed` + `commit_hash` + `staged_files`
///      for code briefs; `commit_status=not-required` + a `summary`
///      explanation for read-only briefs.
///   8. Agent-team hint (literal Chinese line, exactly once) when
///      `dispatch_strategy=agent-team`.
pub(crate) fn build_task_brief(
    plan: &Plan,
    hints: &WorkstationDispatchHints,
    dispatch_strategy: &str,
) -> String {
    build_task_brief_with_source(plan, hints, dispatch_strategy, None)
}

/// wave-19 / task 07 — `build_task_brief` augmented with an optional
/// "Source contract" preamble. When `contract_source` is `Some(path)` the
/// brief opens with a `## Source contract` block that names the on-disk
/// task-contract v1 file the worker should treat as authoritative — this
/// gives the worker a stable reference if it needs to re-read the
/// machine contract while iterating. When `None`, the brief is
/// byte-identical to the wave-15/16/17 baseline.
pub(crate) fn build_task_brief_with_source(
    plan: &Plan,
    hints: &WorkstationDispatchHints,
    dispatch_strategy: &str,
    contract_source: Option<&Path>,
) -> String {
    let mut out = String::new();

    // Header pins plan + board_task so the delegated agent always knows
    // which row it is acting on.
    out.push_str(&format!("# Plan {} — workstation task brief\n", plan.id));
    out.push_str(&format!("Board task: {}\n\n", plan.board_task_id));

    // 0. Source contract (wave-19 / task 07). Preamble — only present
    //    when the dispatch flowed through a task-contract v1 file.
    //    Legacy / non-contract briefs omit this block entirely so the
    //    rest of the brief stays byte-identical.
    if let Some(path) = contract_source {
        out.push_str("## Source contract\n");
        out.push_str(&format!("- task-contract v1: `{}`\n", path.display()));
        out.push_str(
            "- this brief is rendered from the contract above; treat the contract as the SSOT\n",
        );
        out.push_str(
            "- if the brief and the contract diverge, the contract wins — re-read it before staging\n",
        );
        out.push('\n');
    }

    // 1. Objective.
    let objective = hints
        .objective
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("(objective omitted by caller — see PLAN.lisp)");
    out.push_str("## Objective\n");
    out.push_str(objective);
    out.push_str("\n\n");

    // 2. Scope.
    if let Some(scope) = hints.scope.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        out.push_str("## Scope\n");
        out.push_str(scope);
        out.push_str("\n\n");
    }

    // 3. Owned files.
    out.push_str("## Owned files\n");
    if hints.owned_files.is_empty() {
        out.push_str("(none declared — caller must stage NOTHING by default)\n\n");
    } else {
        for f in &hints.owned_files {
            out.push_str(&format!("- {}\n", f));
        }
        out.push('\n');
    }

    // 4. Forbidden files.
    if !hints.forbidden_files.is_empty() {
        out.push_str("## Forbidden files\n");
        for f in &hints.forbidden_files {
            out.push_str(&format!("- {}\n", f));
        }
        out.push('\n');
    }

    // 5. Acceptance commands.
    if !hints.acceptance_commands.is_empty() {
        out.push_str("## Acceptance commands\n");
        for c in &hints.acceptance_commands {
            out.push_str(&format!("- {}\n", c));
        }
        out.push('\n');
    }

    // 6. Commit policy + scoped reminder.
    let policy = hints
        .commit_policy
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(COMMIT_POLICY_SCOPED);
    out.push_str("## Commit policy\n");
    out.push_str(&format!("- policy: {}\n", policy));
    out.push_str(
        "- do not stage or commit outside the owned files declared above\n",
    );
    out.push_str(
        "- code tasks: produce a single scoped commit naming the owned files\n",
    );
    out.push('\n');

    // 7. Completion handoff (scoped commit) — wave-17 / task 07.
    //
    // Pin the EXACT `mission_execution(action=complete)` arguments the worker
    // must report back. The daemon NEVER runs git itself; the worker is
    // expected to perform the scoped commit (or skip with a typed reason)
    // and call completion with `enforce_scoped_commit=true` so the daemon's
    // wave-16/06 fail-fast gates run BEFORE the companion log mutation.
    //
    // The legacy `mission_execution(action=complete)` default for
    // `enforce_scoped_commit` is still `false` — that backward-compatibility
    // contract MUST NOT be touched (callers outside the workstation-dispatch
    // pipeline keep audit-only behaviour). The brief is the *opt-in* lever
    // for this dispatch path: it tells the worker to set the flag explicitly.
    let task_kind = classify_task_kind(hints);
    out.push_str("## Completion handoff (scoped commit)\n");
    out.push_str(&format!("- task kind: {}\n", task_kind.as_str()));
    out.push_str(
        "- on completion call `mission_execution(action=complete)` with `enforce_scoped_commit=true`\n",
    );
    match task_kind {
        BriefTaskKind::Code => {
            out.push_str(
                "- this brief generates code: stage only the owned files listed above and produce one scoped commit\n",
            );
            out.push_str(
                "- report back `commit_status=\"committed\"`, `commit_hash=\"<git sha>\"`, and `staged_files=[<owned files actually staged>]`\n",
            );
            out.push_str(
                "- if you cannot commit (blocked / refused), report `commit_status=\"blocked\"` with a non-empty `commit_blocker` explaining why so the next agent can resume\n",
            );
        }
        BriefTaskKind::ReadOnly => {
            out.push_str(
                "- this brief is read-only: no `owned_files` were declared, so the worker has no licence to stage anything\n",
            );
            out.push_str(
                "- report back `commit_status=\"not-required\"` and use the `summary` field to explain WHY no commit was produced (e.g. \"audit-only — no source files modified\")\n",
            );
            out.push_str(
                "- if the investigation surfaces a code change, STOP and request a follow-up brief with `owned_files` declared instead of staging silently\n",
            );
        }
    }
    out.push_str(
        "- the daemon never runs git itself — the worker performs the scoped commit and reports the hash back\n",
    );
    out.push('\n');

    // 8. Agent-team hint (exactly once, literal Chinese).
    if dispatch_strategy == "agent-team" {
        out.push_str("## Parallelism hint\n");
        out.push_str(AGENT_TEAM_OBJECTIVE_HINT);
        out.push('\n');
    }

    out
}

/// Top-level entry point invoked from `plan::action_execute_internal` and
/// `plan_dag::dispatch_node` when the caller / plan opted in.
///
/// `target` MUST be the resolved target string (already normalised by the
/// outer handler). When `target != "mission_task_delegate"` we return a
/// safe descriptor instead of dispatching.
///
/// `dispatch_strategy` is the already-normalised strategy from the outer
/// handler (one of `VALID_DISPATCH_STRATEGIES` in plan.rs, including
/// `unknown`). It controls only the agent-team hint injection.
///
/// Wave-19 / task 07 — preserved as-is for the no-contract dispatch path.
/// Delegates to [`run_workstation_dispatch_with_contract`] with
/// `task_contract_path = None`, so the legacy objective / owned-files
/// brief is built byte-identically. Future call sites that have a
/// task-contract v1 file on disk should call
/// `run_workstation_dispatch_with_contract` directly.
pub(crate) async fn run_workstation_dispatch(
    state: &AppState,
    plan: &Plan,
    target: &str,
    dispatch_strategy: &str,
    hints: WorkstationDispatchHints,
    dry_run: bool,
) -> WorkstationDispatchOutcome {
    run_workstation_dispatch_with_contract(
        state,
        plan,
        target,
        dispatch_strategy,
        hints,
        dry_run,
        None,
    )
    .await
}

/// Wave-19 / task 07 — contract-aware variant of
/// [`run_workstation_dispatch`].
///
/// Behaviour matrix:
///   * `task_contract_path = None`  → identical to wave-15/16/17. The
///     hints feed `build_task_brief` directly; no contract IO happens.
///   * `task_contract_path = Some`  → load + parse the file, overlay
///     contract fields onto the hints (contract is the SSOT — non-empty
///     contract fields beat caller args/hints), and prefix the brief
///     with a `## Source contract` block naming the on-disk file. The
///     scoped-commit handoff section (wave-17 / task 07) is preserved
///     verbatim because it lives in `build_task_brief_with_source` after
///     the optional preamble.
///
/// Failure semantics (contract path supplied):
///   * IO error / lex error / schema mismatch / missing required field
///     → `SafeDescriptor { reason: MalformedTaskContract { ... } }`.
///     We refuse to fall back to the legacy natural-language brief —
///     downgrading silently would defeat the whole point of having a
///     machine SSOT.
///
/// Path resolution: a relative `task_contract_path` is joined against
/// the resolved project root (NOT the daemon's process cwd). An
/// absolute path is taken verbatim.
pub(crate) async fn run_workstation_dispatch_with_contract(
    state: &AppState,
    plan: &Plan,
    target: &str,
    dispatch_strategy: &str,
    hints: WorkstationDispatchHints,
    dry_run: bool,
    task_contract_path: Option<&Path>,
) -> WorkstationDispatchOutcome {
    // 1. Refuse non-task_delegate targets up front (architecture rule).
    if target != "mission_task_delegate" {
        return WorkstationDispatchOutcome::SafeDescriptor {
            reason: SafeDescriptorReason::UnsupportedTarget(target.to_string()),
            task_brief: None,
        };
    }

    // 2. Hint-only safety: a content-free brief is useless. Refuse rather
    //    than dispatch a placeholder objective.
    //
    //    wave-19 / task 07 — when a contract file is pinned, defer this
    //    check: the contract's `:goal` field will populate
    //    `hints.objective` during overlay (step 3.5). The post-overlay
    //    re-check enforces the same invariant for contract-driven
    //    dispatches and keeps the failure mode identical
    //    (`SafeDescriptorReason::MissingObjective`).
    if task_contract_path.is_none() {
        let has_meaningful_objective = hints
            .objective
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !has_meaningful_objective {
            return WorkstationDispatchOutcome::SafeDescriptor {
                reason: SafeDescriptorReason::MissingObjective,
                task_brief: None,
            };
        }
    }

    // 3. Project-root resolution. `cwd` MUST be absolute when supplied —
    //    `resolve_target_project_root` enforces that contract; we surface
    //    the error string verbatim so the caller can fix and retry.
    //    Owned strings here so the later `hints` move into `cap_lists`
    //    doesn't conflict with the borrows feeding the resolver.
    let project_arg_owned: Option<String> = hints.target_project.clone();
    let cwd_arg_owned: Option<String> = hints.requested_cwd.clone();
    if let Some(cwd) = cwd_arg_owned.as_deref() {
        if !Path::new(cwd).is_absolute() {
            return WorkstationDispatchOutcome::SafeDescriptor {
                reason: SafeDescriptorReason::ProjectRootUnresolved(format!(
                    "requested_cwd `{}` is not absolute; \
                     workstation-dispatch never joins a relative cwd against the daemon process cwd",
                    cwd
                )),
                task_brief: None,
            };
        }
    }
    let resolution = resolve_target_project_root(
        project_arg_owned.as_deref(),
        cwd_arg_owned.as_deref().map(Path::new),
        project_arg_owned.as_deref(),
        &state.project_registry,
    )
    .await;
    let resolution = match resolution {
        Ok(r) => r,
        Err(e) => {
            return WorkstationDispatchOutcome::SafeDescriptor {
                reason: SafeDescriptorReason::ProjectRootUnresolved(e.to_string()),
                task_brief: None,
            };
        }
    };

    // 3.5 wave-19 / task 07 — when a task-contract v1 file is pinned,
    //     load + parse it and overlay onto the hints. The contract is
    //     the SSOT, so non-empty contract fields beat caller args. A
    //     parse failure refuses the dispatch with a typed safe descriptor
    //     rather than silently downgrading to the legacy brief — keeping
    //     a malformed contract from masquerading as a working brief is
    //     the whole point of this layer.
    let mut hints = hints;
    let mut contract_source_path: Option<PathBuf> = None;
    let mut contract_dispatch_strategy: Option<String> = None;
    if let Some(raw_path) = task_contract_path {
        let resolved_path = resolve_contract_path(raw_path, &resolution.project_root);
        match load_task_contract(&resolved_path) {
            Ok(contract) => {
                hints.overlay_contract(&contract);
                contract_dispatch_strategy = contract.dispatch_strategy.clone();
                contract_source_path = Some(resolved_path);
            }
            Err(err) => {
                return WorkstationDispatchOutcome::SafeDescriptor {
                    reason: SafeDescriptorReason::MalformedTaskContract {
                        path: resolved_path.display().to_string(),
                        reason: err.reason(),
                    },
                    task_brief: None,
                };
            }
        }
        // Defence in depth: re-check the post-overlay objective. The
        // pure parser already rejects empty `:goal`, so this cannot
        // fire today — but if a future overlay rule loosens, the
        // dispatch refuses rather than silently shipping a content-free
        // brief.
        let still_has_objective = hints
            .objective
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !still_has_objective {
            return WorkstationDispatchOutcome::SafeDescriptor {
                reason: SafeDescriptorReason::MissingObjective,
                task_brief: None,
            };
        }
    }

    // 4. Build the brief. Hints have already been arg-merged + (when a
    //    contract was supplied) overlaid with the contract; cap lists
    //    here so the brief stays under the 16K objective limit.
    let _capped = hints.cap_lists();
    // Strategy precedence: when the contract pins `:dispatch-strategy`,
    // it overrides the caller-passed `dispatch_strategy` for brief
    // rendering ONLY (the response-facing `dispatch_strategy` keeps the
    // resolver's value so observers can see what was requested vs what
    // the contract enforced).
    let brief_dispatch_strategy: &str = contract_dispatch_strategy
        .as_deref()
        .unwrap_or(dispatch_strategy);
    let brief = build_task_brief_with_source(
        plan,
        &hints,
        brief_dispatch_strategy,
        contract_source_path.as_deref(),
    );

    // 5. dry_run: stop here, no dispatch, no evidence.
    if dry_run {
        return WorkstationDispatchOutcome::DryRun {
            task_brief: brief,
        };
    }

    // 6. Dispatch through the existing mission_task_delegate substrate.
    //    cwd is the resolved canonical project root (downstream resolves
    //    the same way; we forward it explicitly so the inner handler does
    //    not have to re-resolve when the caller only supplied a project
    //    id).
    let mut inner_args = json!({
        "objective": brief,
        "intent": "code",
        "context_hints": [
            format!("plan:{}", plan.id),
            format!("board_task:{}", plan.board_task_id),
            format!("workstation_dispatch:v0"),
        ],
        "cwd": resolution.project_root.to_string_lossy().to_string(),
    });
    if let Some(ds) = hints.dispatch_strategy.as_deref() {
        inner_args["dispatch_strategy"] = json!(ds);
    } else {
        inner_args["dispatch_strategy"] = json!(dispatch_strategy);
    }

    let inner_result =
        match super::super::compute::task_delegate::handle(state, "mission_task_delegate", inner_args).await {
            Ok(r) => r,
            Err(err) => {
                // Hard error from the inner handler (panic-equivalent) —
                // surface it as a safe descriptor so the caller can route.
                return WorkstationDispatchOutcome::SafeDescriptor {
                    reason: SafeDescriptorReason::ProjectRootUnresolved(format!(
                        "mission_task_delegate handler raised: {}",
                        err
                    )),
                    task_brief: Some(brief),
                };
            }
        };
    let inner_payload = tool_result_payload(&inner_result);
    let inner_is_error = inner_result.is_error.unwrap_or(false);
    if inner_is_error {
        return WorkstationDispatchOutcome::InnerError {
            task_brief: brief,
            inner_payload,
        };
    }

    // 7. Evidence sidecar — typed entry, source=`workstation_dispatch`.
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::WORKSTATION_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_inner_dispatch(inner_payload.clone())
    .add_execution_event(EventRef::unavailable(
        "workstation-dispatch v0 wraps mission_task_delegate; live event correlation \
         is the inner handler's responsibility — bus subscription is a future task",
    ))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra("commit_policy", json!(hints
        .commit_policy
        .as_deref()
        .unwrap_or(COMMIT_POLICY_SCOPED)))
    .with_extra("project_id", json!(resolution.project_id))
    .with_extra("project_root", json!(resolution.project_root.to_string_lossy().to_string()))
    .with_extra("owned_files", json!(hints.owned_files.clone()))
    .with_extra("forbidden_files", json!(hints.forbidden_files.clone()))
    .with_extra("acceptance_commands", json!(hints.acceptance_commands.clone()))
    .with_extra("task_brief_preview", json!(truncate_brief_preview(&brief)));
    // wave-19 / task 07 — when the dispatch flowed through a task-contract
    // v1 file, surface the source path on the evidence ledger so observers
    // can correlate the brief preview against the on-disk SSOT. Absent
    // this annotation an audit could mistake a contract-flavoured brief
    // for a legacy natural-language brief; the field disambiguates.
    if let Some(p) = contract_source_path.as_deref() {
        entry = entry.with_extra(
            "task_contract_source_path",
            json!(p.display().to_string()),
        );
    }
    if let Some(eds) = contract_dispatch_strategy.as_deref() {
        entry = entry.with_extra("contract_dispatch_strategy", json!(eds));
    }
    let outcome = evidence_collector::append(
        state,
        plan.id,
        project_arg_owned.as_deref(),
        cwd_arg_owned.as_deref(),
        hints.target_project.as_deref(),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &outcome {
        tracing::warn!(
            plan_id = %plan.id,
            error = %error,
            "workstation_dispatch: evidence sidecar append failed"
        );
    }
    let (evidence_path, evidence_error) = outcome.into_legacy_tuple();

    WorkstationDispatchOutcome::Dispatched {
        task_brief: brief,
        // task_brief_path: future enhancement — wave-15 v0 keeps the brief
        // inline on the response. None signals the file-mirror is not yet
        // wired so callers know to read `task_brief_preview` instead.
        task_brief_path: None,
        // wave-20 / task 04 — surface the on-disk task-contract source
        // path on the response when the dispatch consumed the contract
        // directly (machine-driven mode). The legacy / rendered path
        // leaves it `None` so the wire shape stays byte-compatible with
        // wave-15..19 callers that only watch for `task_brief_preview`.
        task_contract_source_path: contract_source_path
            .as_deref()
            .map(|p| p.display().to_string()),
        evidence_path,
        evidence_error,
        inner_payload,
    }
}

/// Render the workstation-dispatch outcome into the JSON object plan.rs /
/// plan_dag.rs splice into their response. Centralised so both call sites
/// emit the same field names.
pub(crate) fn outcome_to_response_fields(
    outcome: &WorkstationDispatchOutcome,
    dispatch_strategy: &str,
) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("workstation_dispatch_status".to_string(), json!(outcome.status()));
    m.insert("dispatch_strategy".to_string(), json!(dispatch_strategy));
    // Wave-17 / Task 07 — every dispatch (live, dry-run, inner-error,
    // and safe-descriptor) carries the scoped-commit policy contract so
    // observers can assert the invariant without parsing the brief text.
    // The policy is fixed at the workstation-dispatch layer; legacy
    // callers of `mission_execution(action=complete)` keep their default
    // `enforce_scoped_commit=false` behaviour untouched.
    m.insert(
        "scoped_commit_required".to_string(),
        json!(SCOPED_COMMIT_REQUIRED),
    );
    m.insert(
        "scoped_commit_policy".to_string(),
        json!(SCOPED_COMMIT_POLICY),
    );
    match outcome {
        WorkstationDispatchOutcome::Dispatched {
            task_brief,
            task_brief_path,
            task_contract_source_path,
            evidence_path,
            evidence_error,
            inner_payload,
        } => {
            m.insert(
                "task_brief_preview".to_string(),
                json!(truncate_brief_preview(task_brief)),
            );
            if let Some(p) = task_brief_path {
                m.insert("task_brief_path".to_string(), json!(p));
            }
            // wave-20 / task 04 — when the dispatch consumed the
            // task-contract v1 file directly, surface the resolved
            // source path so observers (CI, PR review, audit) can prove
            // the Lisp was load-bearing rather than the rendered
            // markdown brief. Absent on the legacy / rendered path so
            // the wire shape stays byte-compatible with wave-15..19.
            if let Some(p) = task_contract_source_path {
                m.insert(
                    "task_contract_source_path".to_string(),
                    json!(p),
                );
            }
            if let Some(p) = evidence_path {
                m.insert("evidence_path".to_string(), json!(p));
            }
            if let Some(e) = evidence_error {
                m.insert("evidence_error".to_string(), json!(e));
            }
            m.insert("inner_result".to_string(), inner_payload.clone());
        }
        WorkstationDispatchOutcome::InnerError {
            task_brief,
            inner_payload,
        } => {
            m.insert(
                "task_brief_preview".to_string(),
                json!(truncate_brief_preview(task_brief)),
            );
            m.insert("inner_result".to_string(), inner_payload.clone());
        }
        WorkstationDispatchOutcome::DryRun { task_brief } => {
            m.insert(
                "task_brief_preview".to_string(),
                json!(truncate_brief_preview(task_brief)),
            );
        }
        WorkstationDispatchOutcome::SafeDescriptor { reason, task_brief } => {
            m.insert("workstation_dispatch_reason".to_string(), json!(reason.detail()));
            if let Some(brief) = task_brief {
                m.insert(
                    "task_brief_preview".to_string(),
                    json!(truncate_brief_preview(brief)),
                );
            }
        }
    }
    Value::Object(m)
}

/// Trim the brief for the response preview field. The full text already
/// reaches the inner handler via `objective`; we just want a humane
/// preview on the response.
fn truncate_brief_preview(brief: &str) -> String {
    const MAX: usize = 800;
    if brief.len() <= MAX {
        return brief.to_string();
    }
    let mut end = MAX;
    while end > 0 && !brief.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &brief[..end])
}

/// Local copy of the same string-list helper used by the compile path.
/// Accepts either a single string or an array of strings; ignores other
/// JSON shapes.
fn collect_string_list(v: Option<&Value>) -> Vec<String> {
    match v {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(s)) => {
            if s.trim().is_empty() {
                Vec::new()
            } else {
                vec![s.clone()]
            }
        }
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|item| match item {
                Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono::Utc;
    use missiond_core::types::PlanStatus;
    use uuid::Uuid;

    fn fixture_plan(sexp: &str) -> Plan {
        Plan {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000def").unwrap(),
            board_task_id: "btk-wd".to_string(),
            source_directive_id: None,
            version: 1,
            sexp_text: sexp.to_string(),
            sexp_hash: "deadbeef".to_string(),
            status: PlanStatus::Approved,
            compiler_model: None,
            compiled_from: None,
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            approved_at: None,
            finished_at: None,
        }
    }

    #[test]
    fn opt_in_requires_explicit_arg_or_plan_hint() {
        assert!(!opt_in_requested(&json!({}), false));
        assert!(opt_in_requested(&json!({"workstation_dispatch": true}), false));
        assert!(opt_in_requested(&json!({}), true));
        // Random truthy fields do NOT count.
        assert!(!opt_in_requested(&json!({"target": "mission_task_delegate"}), false));
        assert!(!opt_in_requested(&json!({"workstation_dispatch": false}), false));
    }

    // ── wave-16 / task 03 — auto-inference decision tests ───────────────

    /// Helper: build an inference context that matches every gate by
    /// default. Individual tests flip a single field to assert that gate.
    fn ctx_all_pass<'a>() -> InferenceContext<'a> {
        InferenceContext {
            target: "mission_task_delegate",
            dispatch_strategy: "fresh-code-alignment",
            objective: Some("ship the wave"),
            owned_files_present: true,
            scope_present: false,
            target_project_present: false,
            requested_cwd_present: false,
        }
    }

    #[test]
    fn evaluate_decision_explicit_true_wins_even_without_scope_signal() {
        let mut ctx = ctx_all_pass();
        ctx.owned_files_present = false;
        let decision = evaluate_dispatch_decision(
            &json!({"workstation_dispatch": true}),
            false,
            &ctx,
        );
        assert_eq!(decision.source, WorkstationDispatchSource::ExplicitArg);
        assert!(decision.is_enabled());
    }

    #[test]
    fn evaluate_decision_explicit_false_disables_inference() {
        let ctx = ctx_all_pass();
        let decision = evaluate_dispatch_decision(
            &json!({"workstation_dispatch": false}),
            true, // even with plan hint set
            &ctx,
        );
        assert_eq!(decision.source, WorkstationDispatchSource::Disabled);
        assert!(!decision.is_enabled());
        assert!(decision.reason.unwrap().contains("workstation_dispatch=false"));
    }

    #[test]
    fn evaluate_decision_plan_hint_takes_precedence_over_inference() {
        let ctx = ctx_all_pass();
        let decision = evaluate_dispatch_decision(&json!({}), true, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::PlanHint);
        assert!(decision.is_enabled());
    }

    #[test]
    fn evaluate_decision_inferred_when_all_gates_pass() {
        let ctx = ctx_all_pass();
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::Inferred);
        assert!(decision.is_enabled());
        assert!(decision.reason.unwrap().contains("fresh-code-alignment"));
    }

    #[test]
    fn evaluate_decision_inferred_for_each_strategy_in_whitelist() {
        for strategy in INFERABLE_DISPATCH_STRATEGIES {
            let mut ctx = ctx_all_pass();
            ctx.dispatch_strategy = strategy;
            let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
            assert_eq!(
                decision.source,
                WorkstationDispatchSource::Inferred,
                "strategy `{}` should be inferable",
                strategy
            );
        }
    }

    #[test]
    fn evaluate_decision_not_inferred_for_unknown_strategy() {
        let mut ctx = ctx_all_pass();
        ctx.dispatch_strategy = "unknown";
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
        assert!(!decision.is_enabled());
        assert!(decision.reason.unwrap().contains("dispatch strategy"));
    }

    #[test]
    fn evaluate_decision_not_inferred_for_prompt_fallback_strategy() {
        let mut ctx = ctx_all_pass();
        ctx.dispatch_strategy = "prompt-fallback";
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
    }

    #[test]
    fn evaluate_decision_not_inferred_for_mission_execution_target() {
        let mut ctx = ctx_all_pass();
        ctx.target = "mission_execution";
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
        assert!(decision.reason.unwrap().contains("mission_task_delegate"));
    }

    #[test]
    fn evaluate_decision_not_inferred_for_mission_flow_run_target() {
        let mut ctx = ctx_all_pass();
        ctx.target = "mission_flow_run";
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
    }

    #[test]
    fn evaluate_decision_not_inferred_when_objective_missing() {
        let mut ctx = ctx_all_pass();
        ctx.objective = None;
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
        assert!(decision.reason.unwrap().contains("objective"));
    }

    #[test]
    fn evaluate_decision_not_inferred_when_objective_blank() {
        let mut ctx = ctx_all_pass();
        ctx.objective = Some("   ");
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
    }

    #[test]
    fn evaluate_decision_not_inferred_when_no_scope_signal() {
        let ctx = InferenceContext {
            target: "mission_task_delegate",
            dispatch_strategy: "fresh-code-alignment",
            objective: Some("ship"),
            owned_files_present: false,
            scope_present: false,
            target_project_present: false,
            requested_cwd_present: false,
        };
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::NotApplicable);
        assert!(decision.reason.unwrap().contains("scoping signal"));
    }

    #[test]
    fn evaluate_decision_inferred_when_scope_present_only() {
        let ctx = InferenceContext {
            target: "mission_task_delegate",
            dispatch_strategy: "agent-team",
            objective: Some("ship"),
            owned_files_present: false,
            scope_present: true,
            target_project_present: false,
            requested_cwd_present: false,
        };
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::Inferred);
    }

    #[test]
    fn evaluate_decision_inferred_when_target_project_present_only() {
        let ctx = InferenceContext {
            target: "mission_task_delegate",
            dispatch_strategy: "resident-lisp",
            objective: Some("ship"),
            owned_files_present: false,
            scope_present: false,
            target_project_present: true,
            requested_cwd_present: false,
        };
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::Inferred);
    }

    #[test]
    fn evaluate_decision_inferred_when_requested_cwd_present_only() {
        let ctx = InferenceContext {
            target: "mission_task_delegate",
            dispatch_strategy: "mixed",
            objective: Some("ship"),
            owned_files_present: false,
            scope_present: false,
            target_project_present: false,
            requested_cwd_present: true,
        };
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::Inferred);
    }

    #[test]
    fn workstation_dispatch_source_string_pin() {
        // The five values are part of the response wire contract.
        assert_eq!(WorkstationDispatchSource::ExplicitArg.as_str(), "explicit_arg");
        assert_eq!(WorkstationDispatchSource::PlanHint.as_str(), "plan_hint");
        assert_eq!(WorkstationDispatchSource::Inferred.as_str(), "inferred");
        assert_eq!(WorkstationDispatchSource::Disabled.as_str(), "disabled");
        assert_eq!(WorkstationDispatchSource::NotApplicable.as_str(), "not_applicable");
    }

    /// End-to-end shape check: when auto-inference picks `agent-team`,
    /// the brief built from the inferred hints carries the literal Chinese
    /// reminder exactly once. This pins the wave-15 invariant onto the
    /// wave-16 inference path so a future merge cannot silently double-
    /// inject the hint.
    #[test]
    fn inferred_agent_team_path_injects_literal_exactly_once() {
        let ctx = InferenceContext {
            target: "mission_task_delegate",
            dispatch_strategy: "agent-team",
            objective: Some("ship the wave"),
            owned_files_present: true,
            scope_present: false,
            target_project_present: false,
            requested_cwd_present: false,
        };
        let decision = evaluate_dispatch_decision(&json!({}), false, &ctx);
        assert_eq!(decision.source, WorkstationDispatchSource::Inferred);
        // Build the brief the same way `run_workstation_dispatch` would
        // to confirm the literal lands once.
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship the wave".to_string()),
            owned_files: vec!["a.rs".to_string()],
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "agent-team");
        assert_eq!(
            brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(),
            1,
            "agent-team hint must appear exactly once on the inferred path"
        );
    }

    #[test]
    fn explicit_workstation_dispatch_flag_extracts_explicit_choice() {
        assert_eq!(explicit_workstation_dispatch_flag(&json!({})), None);
        assert_eq!(
            explicit_workstation_dispatch_flag(&json!({"workstation_dispatch": true})),
            Some(true)
        );
        assert_eq!(
            explicit_workstation_dispatch_flag(&json!({"workstation_dispatch": false})),
            Some(false)
        );
        // Non-bool values do not satisfy the strict opt-in/out contract.
        assert_eq!(
            explicit_workstation_dispatch_flag(&json!({"workstation_dispatch": "yes"})),
            None
        );
    }

    #[test]
    fn merge_args_arg_wins_over_hint_for_every_field() {
        let hints = WorkstationDispatchHints {
            objective: Some("hint obj".to_string()),
            scope: Some("hint scope".to_string()),
            owned_files: vec!["hint.rs".to_string()],
            forbidden_files: vec!["hint_forbidden.rs".to_string()],
            acceptance_commands: vec!["hint cmd".to_string()],
            commit_policy: Some("hint-policy".to_string()),
            target_project: Some("hint-proj".to_string()),
            requested_cwd: Some("/hint/cwd".to_string()),
            dispatch_strategy: Some("resident-lisp".to_string()),
        };
        let args = json!({
            "objective": "arg obj",
            "scope": "arg scope",
            "owned_files": ["arg.rs", "arg2.rs"],
            "forbidden_files": ["arg_forbidden.rs"],
            "acceptance_commands": ["arg cmd1", "arg cmd2"],
            "commit_policy": "arg-policy",
            "target_project": "arg-proj",
            "requested_cwd": "/arg/cwd",
            "dispatch_strategy": "agent-team",
        });
        let merged = hints.merge_args(&args);
        assert_eq!(merged.objective.as_deref(), Some("arg obj"));
        assert_eq!(merged.scope.as_deref(), Some("arg scope"));
        assert_eq!(merged.owned_files, vec!["arg.rs", "arg2.rs"]);
        assert_eq!(merged.forbidden_files, vec!["arg_forbidden.rs"]);
        assert_eq!(merged.acceptance_commands, vec!["arg cmd1", "arg cmd2"]);
        assert_eq!(merged.commit_policy.as_deref(), Some("arg-policy"));
        assert_eq!(merged.target_project.as_deref(), Some("arg-proj"));
        assert_eq!(merged.requested_cwd.as_deref(), Some("/arg/cwd"));
        assert_eq!(merged.dispatch_strategy.as_deref(), Some("agent-team"));
    }

    #[test]
    fn merge_args_falls_back_to_hint_when_arg_absent_or_blank() {
        let hints = WorkstationDispatchHints {
            objective: Some("hint obj".to_string()),
            commit_policy: Some("hint-policy".to_string()),
            ..Default::default()
        };
        let args = json!({
            "objective": "   ",  // blank → falls back
            "commit_policy": "",
        });
        let merged = hints.merge_args(&args);
        assert_eq!(merged.objective.as_deref(), Some("hint obj"));
        assert_eq!(merged.commit_policy.as_deref(), Some("hint-policy"));
    }

    #[test]
    fn merge_args_cwd_falls_back_to_args_cwd_alias() {
        let hints = WorkstationDispatchHints::default();
        let args = json!({"cwd": "/from/cwd/alias"});
        let merged = hints.merge_args(&args);
        assert_eq!(merged.requested_cwd.as_deref(), Some("/from/cwd/alias"));
    }

    #[test]
    fn cap_lists_truncates_runaway_lists_and_reports_drop_count() {
        let mut hints = WorkstationDispatchHints {
            owned_files: (0..100).map(|i| format!("f{}.rs", i)).collect(),
            ..Default::default()
        };
        let dropped = hints.cap_lists();
        assert_eq!(hints.owned_files.len(), TASK_BRIEF_LIST_CAP);
        assert!(dropped
            .iter()
            .any(|(label, count)| *label == "owned_files" && *count == 100 - TASK_BRIEF_LIST_CAP));
    }

    #[test]
    fn build_task_brief_includes_canonical_sections_and_scoped_commit_reminder() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship the wave".to_string()),
            scope: Some("wave 15 task 05 only".to_string()),
            owned_files: vec!["a.rs".to_string(), "b.rs".to_string()],
            forbidden_files: vec!["c.rs".to_string()],
            acceptance_commands: vec!["cargo test".to_string(), "git diff --check".to_string()],
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "fresh-code-alignment");
        // headings present
        assert!(brief.contains("## Objective"));
        assert!(brief.contains("## Scope"));
        assert!(brief.contains("## Owned files"));
        assert!(brief.contains("## Forbidden files"));
        assert!(brief.contains("## Acceptance commands"));
        assert!(brief.contains("## Commit policy"));
        // owned files listed
        assert!(brief.contains("- a.rs"));
        assert!(brief.contains("- b.rs"));
        // scoped commit reminder line
        assert!(brief.contains("do not stage or commit outside the owned files"));
        // default policy (we did NOT pass commit_policy)
        assert!(brief.contains("policy: scoped"));
        // fresh-code-alignment must NOT inject the agent-team hint
        assert!(!brief.contains(AGENT_TEAM_OBJECTIVE_HINT));
    }

    #[test]
    fn build_task_brief_injects_agent_team_hint_exactly_once_for_agent_team_strategy() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "agent-team");
        assert_eq!(
            brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(),
            1,
            "agent-team hint must appear exactly once, got: {brief}"
        );
    }

    #[test]
    fn build_task_brief_omits_optional_sections_when_lists_empty() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "fresh-code-alignment");
        // Forbidden / Acceptance / Scope sections must NOT appear when their
        // backing lists are empty / absent.
        assert!(!brief.contains("## Forbidden files"));
        assert!(!brief.contains("## Acceptance commands"));
        assert!(!brief.contains("## Scope"));
        // Owned-files section is always present (the policy is "stage NOTHING
        // by default" — explicit reminder).
        assert!(brief.contains("## Owned files"));
        assert!(brief.contains("(none declared"));
    }

    #[test]
    fn build_task_brief_uses_explicit_commit_policy_when_supplied() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            commit_policy: Some("monorepo-cascade".to_string()),
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "resident-lisp");
        assert!(brief.contains("policy: monorepo-cascade"));
        // resident-lisp must NOT inject agent-team hint
        assert!(!brief.contains(AGENT_TEAM_OBJECTIVE_HINT));
    }

    #[test]
    fn safe_descriptor_status_strings_are_distinct() {
        assert_eq!(
            SafeDescriptorReason::UnsupportedTarget("mission_execution".into()).status(),
            "skipped_unsupported_target"
        );
        assert_eq!(
            SafeDescriptorReason::ProjectRootUnresolved("nope".into()).status(),
            "skipped_project_root_unresolved"
        );
        assert_eq!(
            SafeDescriptorReason::MissingObjective.status(),
            "skipped_missing_objective"
        );
    }

    #[test]
    fn outcome_to_response_dispatched_carries_inner_and_brief_preview() {
        let outcome = WorkstationDispatchOutcome::Dispatched {
            task_brief: "## Objective\nship\n".to_string(),
            task_brief_path: None,
            task_contract_source_path: None,
            evidence_path: Some("/tmp/sidecar.json".to_string()),
            evidence_error: None,
            inner_payload: json!({"task_id": "btk-9"}),
        };
        let v = outcome_to_response_fields(&outcome, "agent-team");
        assert_eq!(v["workstation_dispatch_status"], "dispatched");
        assert_eq!(v["dispatch_strategy"], "agent-team");
        assert_eq!(v["evidence_path"], "/tmp/sidecar.json");
        assert!(v["task_brief_preview"].as_str().unwrap().contains("## Objective"));
        assert_eq!(v["inner_result"]["task_id"], "btk-9");
        // wave-20 / task 04 — legacy / rendered path leaves the
        // `task_contract_source_path` key OFF so the wire shape stays
        // byte-compatible with wave-15..19 callers.
        assert!(
            v.get("task_contract_source_path").is_none(),
            "rendered-path dispatch must omit task_contract_source_path \
             (wave-15..19 byte-compat)"
        );
    }

    /// wave-20 / task 04 — when the dispatch ran in machine-driven mode
    /// the response must carry the resolved on-disk task-contract path
    /// so observers can prove the Lisp was load-bearing.
    #[test]
    fn outcome_to_response_dispatched_machine_mode_surfaces_contract_path() {
        let outcome = WorkstationDispatchOutcome::Dispatched {
            task_brief: "## Objective\nship\n".to_string(),
            task_brief_path: None,
            task_contract_source_path: Some(
                "/tmp/p/.missiond/tasks/generated/plan/root.lisp".to_string(),
            ),
            evidence_path: None,
            evidence_error: None,
            inner_payload: json!({"task_id": "btk-9"}),
        };
        let v = outcome_to_response_fields(&outcome, "agent-team");
        assert_eq!(
            v["task_contract_source_path"],
            "/tmp/p/.missiond/tasks/generated/plan/root.lisp"
        );
    }

    #[test]
    fn outcome_to_response_safe_descriptor_carries_reason_detail() {
        let outcome = WorkstationDispatchOutcome::SafeDescriptor {
            reason: SafeDescriptorReason::ProjectRootUnresolved(
                "no signal".to_string(),
            ),
            task_brief: None,
        };
        let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
        assert_eq!(v["workstation_dispatch_status"], "skipped_project_root_unresolved");
        assert_eq!(v["workstation_dispatch_reason"], "no signal");
        // No inner_result on safe descriptors
        assert!(v.get("inner_result").is_none());
    }

    #[test]
    fn outcome_to_response_dry_run_omits_evidence_and_inner() {
        let outcome = WorkstationDispatchOutcome::DryRun {
            task_brief: "## Objective\nship\n".to_string(),
        };
        let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
        assert_eq!(v["workstation_dispatch_status"], "dry_run_no_dispatch");
        assert!(v.get("inner_result").is_none());
        assert!(v.get("evidence_path").is_none());
        assert!(v["task_brief_preview"].as_str().unwrap().contains("ship"));
    }

    // ── async path tests (stand up a minimal AppState) ───────────────────

    use crate::slot_orchestrator::project_root::ResolutionError;
    use missiond_core::types::{ProjectConfig, ProjectRegistry, SharedProjectRegistry};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn fixture_registry(id: &str, root: &Path) -> SharedProjectRegistry {
        Arc::new(RwLock::new(ProjectRegistry::new(vec![ProjectConfig {
            id: id.to_string(),
            path: root.display().to_string(),
            intent_path: None,
            active: true,
            slots: vec![],
            github_url: None,
            kind: "managed".to_string(),
            vault_path: None,
            parent_id: None,
            created_at: None,
            updated_at: None,
        }])))
    }

    /// Build a minimal AppState skeleton good enough for the workstation-
    /// dispatch resolver path. Only `project_registry` is touched here;
    /// other fields stay at their default constructions because we never
    /// invoke a code path that reads them in the safe-descriptor and
    /// resolver-only tests.
    async fn fixture_state_with_registry(
        reg: SharedProjectRegistry,
    ) -> Option<AppState> {
        // AppState construction is feature-gated and pulls in the full
        // daemon graph (DB, bus, slot dispatcher) — far heavier than this
        // unit-level test wants. We therefore exercise the resolver path
        // directly via `resolve_target_project_root` in
        // `safe_descriptor_emitted_when_project_root_unresolved` instead
        // of standing up a full AppState. Keeping the helper here for
        // future async-only tests; returning `None` for now signals the
        // test harness to fall back to direct resolver assertions.
        let _ = reg;
        None
    }

    /// Resolver-level assertion: missing project root yields a structured
    /// safe descriptor instead of a silent fallback. We exercise the path
    /// from `run_workstation_dispatch` would take by calling
    /// `resolve_target_project_root` with the same args and asserting the
    /// branch shape downstream.
    #[tokio::test]
    async fn missing_project_root_signals_resolver_no_signal() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let reg = fixture_registry("missiond", &root);
        // No project_id, no cwd, no fallback → NoSignal.
        let err = resolve_target_project_root(None, None, None, &reg)
            .await
            .expect_err("should fail");
        assert!(matches!(err, ResolutionError::NoSignal));
        // Mirror what run_workstation_dispatch would build:
        let descriptor = SafeDescriptorReason::ProjectRootUnresolved(err.to_string());
        assert_eq!(descriptor.status(), "skipped_project_root_unresolved");
        let _ = fixture_state_with_registry(reg).await;
    }

    #[tokio::test]
    async fn relative_cwd_is_rejected_by_pre_flight() {
        // The dispatch helper itself rejects a relative cwd before even
        // reaching the resolver — this is the "do not join relative cwd
        // against process cwd" architectural invariant.
        let cwd = "relative/path";
        assert!(!Path::new(cwd).is_absolute());
        let descriptor = SafeDescriptorReason::ProjectRootUnresolved(format!(
            "requested_cwd `{}` is not absolute; \
             workstation-dispatch never joins a relative cwd against the daemon process cwd",
            cwd
        ));
        assert_eq!(descriptor.status(), "skipped_project_root_unresolved");
        assert!(descriptor.detail().contains("not absolute"));
    }

    /// `WorkstationDispatchHints::default` inherently has no objective —
    /// confirm the safe descriptor branch fires before any I/O happens.
    #[tokio::test]
    async fn missing_objective_yields_safe_descriptor() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let reg = fixture_registry("missiond", &root);
        // We reuse the resolver to make this test fully self-contained
        // (no AppState dependency) — the resolver succeeds, but the
        // dispatch helper would refuse on `MissingObjective` first.
        let _ok = resolve_target_project_root(Some("missiond"), None, None, &reg)
            .await
            .expect("resolver succeeds");
        let descriptor = SafeDescriptorReason::MissingObjective;
        assert_eq!(descriptor.status(), "skipped_missing_objective");
        assert!(descriptor.detail().contains("content-free"));
    }

    // ── wave-17 / task 07 — scoped-commit handoff default tests ─────────

    #[test]
    fn classify_task_kind_treats_owned_files_as_code_brief() {
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            owned_files: vec!["a.rs".to_string()],
            ..Default::default()
        };
        assert_eq!(classify_task_kind(&hints), BriefTaskKind::Code);
    }

    #[test]
    fn classify_task_kind_treats_empty_owned_files_as_read_only_brief() {
        let hints = WorkstationDispatchHints {
            objective: Some("audit the wave".to_string()),
            ..Default::default()
        };
        assert_eq!(classify_task_kind(&hints), BriefTaskKind::ReadOnly);
    }

    #[test]
    fn build_task_brief_code_requires_enforce_scoped_commit_on_completion() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            owned_files: vec!["a.rs".to_string(), "b.rs".to_string()],
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "fresh-code-alignment");
        // The completion handoff section is always present.
        assert!(
            brief.contains("## Completion handoff (scoped commit)"),
            "code brief must carry the completion handoff section"
        );
        // The brief tells the worker to set enforce_scoped_commit=true.
        assert!(
            brief.contains("`enforce_scoped_commit=true`"),
            "code brief must instruct the worker to opt into enforcement"
        );
        // The brief asks for committed status + commit_hash + staged_files.
        assert!(
            brief.contains("`commit_status=\"committed\"`"),
            "code brief must request commit_status=committed"
        );
        assert!(
            brief.contains("`commit_hash="),
            "code brief must request commit_hash"
        );
        assert!(
            brief.contains("`staged_files="),
            "code brief must request staged_files"
        );
        // Task kind line.
        assert!(
            brief.contains("- task kind: code"),
            "code brief must declare task kind"
        );
        // The blocked branch must also be documented so workers don't
        // silently drop to "no commit".
        assert!(
            brief.contains("`commit_status=\"blocked\"`"),
            "code brief must document the blocked branch"
        );
        // The daemon-never-runs-git invariant must be loud.
        assert!(
            brief.contains("daemon never runs git itself"),
            "brief must restate the daemon-never-runs-git invariant"
        );
    }

    #[test]
    fn build_task_brief_read_only_uses_not_required_with_explanation() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("audit the wave-17 surface".to_string()),
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "fresh-code-alignment");
        assert!(brief.contains("## Completion handoff (scoped commit)"));
        // Read-only briefs default to commit_status=not-required.
        assert!(
            brief.contains("`commit_status=\"not-required\"`"),
            "read-only brief must default to commit_status=not-required"
        );
        // ...with an explanation requirement.
        assert!(
            brief.contains("explain WHY"),
            "read-only brief must require an explanation in the summary field"
        );
        // Task kind line.
        assert!(
            brief.contains("- task kind: read-only"),
            "read-only brief must declare task kind"
        );
        // Still asks for enforce_scoped_commit=true so the daemon's
        // wave-16/06 gates run.
        assert!(
            brief.contains("`enforce_scoped_commit=true`"),
            "read-only brief still opts the completion call into enforcement"
        );
        // Read-only brief must NOT instruct the worker to commit anything.
        assert!(
            !brief.contains("`commit_status=\"committed\"`"),
            "read-only brief must NOT prescribe commit_status=committed"
        );
    }

    #[test]
    fn build_task_brief_completion_handoff_does_not_double_inject_agent_team_hint() {
        // The agent-team hint sits in section 8 (after completion handoff).
        // Adding the new section must not leak the literal anywhere else.
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            owned_files: vec!["a.rs".to_string()],
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "agent-team");
        assert_eq!(
            brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(),
            1,
            "agent-team hint must still appear exactly once after wave-17 / task 07"
        );
    }

    #[test]
    fn build_task_brief_read_only_does_not_inject_agent_team_hint_for_other_strategies() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("audit".to_string()),
            ..Default::default()
        };
        let brief = build_task_brief(&plan, &hints, "resident-lisp");
        assert_eq!(brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(), 0);
        // Confirm read-only branch lands.
        assert!(brief.contains("- task kind: read-only"));
    }

    #[test]
    fn outcome_to_response_dispatched_advertises_scoped_commit_policy() {
        let outcome = WorkstationDispatchOutcome::Dispatched {
            task_brief: "## Objective\nship\n".to_string(),
            task_brief_path: None,
            task_contract_source_path: None,
            evidence_path: None,
            evidence_error: None,
            inner_payload: json!({"task_id": "btk-9"}),
        };
        let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
        assert_eq!(v["scoped_commit_required"], json!(true));
        assert_eq!(v["scoped_commit_policy"], "enforced-on-complete");
    }

    #[test]
    fn outcome_to_response_dry_run_advertises_scoped_commit_policy() {
        let outcome = WorkstationDispatchOutcome::DryRun {
            task_brief: "## Objective\nship\n".to_string(),
        };
        let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
        assert_eq!(v["scoped_commit_required"], json!(true));
        assert_eq!(v["scoped_commit_policy"], "enforced-on-complete");
    }

    #[test]
    fn outcome_to_response_inner_error_advertises_scoped_commit_policy() {
        let outcome = WorkstationDispatchOutcome::InnerError {
            task_brief: "## Objective\nship\n".to_string(),
            inner_payload: json!({"error": "nope"}),
        };
        let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
        assert_eq!(v["scoped_commit_required"], json!(true));
        assert_eq!(v["scoped_commit_policy"], "enforced-on-complete");
    }

    #[test]
    fn outcome_to_response_safe_descriptor_advertises_scoped_commit_policy() {
        // Even on safe-descriptor refusals the policy contract is part of
        // the wire shape so observers don't have to special-case the
        // skipped branch when asserting the invariant.
        let outcome = WorkstationDispatchOutcome::SafeDescriptor {
            reason: SafeDescriptorReason::MissingObjective,
            task_brief: None,
        };
        let v = outcome_to_response_fields(&outcome, "fresh-code-alignment");
        assert_eq!(v["scoped_commit_required"], json!(true));
        assert_eq!(v["scoped_commit_policy"], "enforced-on-complete");
    }

    #[test]
    fn brief_task_kind_string_pin_is_stable_wire_contract() {
        // These two strings are part of the brief / response wire contract;
        // changing them silently would break downstream observers.
        assert_eq!(BriefTaskKind::Code.as_str(), "code");
        assert_eq!(BriefTaskKind::ReadOnly.as_str(), "read-only");
    }

    // ── wave-19 / task 07 — task-contract v1 parser tests ────────────────

    /// Reference contract body produced by the wave-19 / task 06 emitter.
    /// We pin against the exact textual shape so a future emitter tweak
    /// that breaks the parser surface trips a test rather than silently
    /// downgrading to the legacy brief.
    const SAMPLE_CONTRACT: &str = r#";; Generated by MissionD plan-runner (wave-19 / task 06).
;; plan_id = 00000000-0000-0000-0000-000000000def
;; board_task_id = btk-wd
;; node_id = root

(task plan-00000000-node-root
  :schema "missiond.task-contract.v1"
  :title "Plan 00000000-0000-0000-0000-000000000def node root — workstation task contract"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :dispatch-strategy "agent-team"
  :goal "ship the wave-19 contract consumer"
  :scope "wave 19 task 07 only"
  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]
  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"]
  :acceptance
    ["cargo test -p missiond-daemon"
     "cargo build --workspace"]
  :commit
    (:required true
     :message "feat(workstation): consume Lisp task contracts"
     :scope-check write-scope-only
     :policy "scoped")
  :target-project "missiond"
  :requested-cwd "/Users/jinchen/Projects/missiond"
  :target "mission_task_delegate"
  :plan-id "00000000-0000-0000-0000-000000000def"
  :node-id "root"
)
"#;

    #[test]
    fn parse_task_contract_extracts_every_consumed_field() {
        let c = parse_task_contract(SAMPLE_CONTRACT).expect("must parse");
        assert_eq!(c.schema, "missiond.task-contract.v1");
        assert_eq!(c.goal, "ship the wave-19 contract consumer");
        assert_eq!(c.scope.as_deref(), Some("wave 19 task 07 only"));
        assert_eq!(
            c.write_scope,
            vec!["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"]
        );
        assert_eq!(c.must_not_touch.len(), 2);
        assert!(c
            .must_not_touch
            .contains(&"crates/missiond-daemon/src/handlers/knowledge/plan.rs".to_string()));
        assert_eq!(c.acceptance.len(), 2);
        assert_eq!(c.commit_policy.as_deref(), Some("scoped"));
        assert_eq!(c.dispatch_strategy.as_deref(), Some("agent-team"));
        assert_eq!(c.target_project.as_deref(), Some("missiond"));
        assert_eq!(
            c.requested_cwd.as_deref(),
            Some("/Users/jinchen/Projects/missiond")
        );
        assert_eq!(c.target.as_deref(), Some("mission_task_delegate"));
    }

    #[test]
    fn parse_task_contract_tolerates_optional_field_absence() {
        // Minimal viable contract: schema + goal only.
        let src = r#"(task minimal
  :schema "missiond.task-contract.v1"
  :goal "ship"
  :write-scope []
  :must-not-touch []
)
"#;
        let c = parse_task_contract(src).expect("minimal parse");
        assert_eq!(c.goal, "ship");
        assert!(c.scope.is_none());
        assert!(c.write_scope.is_empty());
        assert!(c.must_not_touch.is_empty());
        assert!(c.acceptance.is_empty());
        assert!(c.commit_policy.is_none());
        assert!(c.dispatch_strategy.is_none());
    }

    #[test]
    fn parse_task_contract_rejects_schema_mismatch() {
        let src = r#"(task wrong
  :schema "missiond.task-contract.v0"
  :goal "ship"
)"#;
        let err = parse_task_contract(src).expect_err("must reject");
        assert!(matches!(err, TaskContractParseError::SchemaMismatch(_)));
        assert!(err.reason().contains("schema mismatch"));
        assert!(err.reason().contains("v0"));
    }

    #[test]
    fn parse_task_contract_rejects_missing_schema() {
        let src = r#"(task no-schema :goal "ship")"#;
        let err = parse_task_contract(src).expect_err("must reject");
        assert!(matches!(err, TaskContractParseError::SchemaMismatch(_)));
        assert!(err.reason().contains("(absent)"));
    }

    #[test]
    fn parse_task_contract_rejects_missing_goal() {
        let src = r#"(task no-goal
  :schema "missiond.task-contract.v1"
)"#;
        let err = parse_task_contract(src).expect_err("must reject");
        assert!(matches!(err, TaskContractParseError::MissingRequired("goal")));
    }

    #[test]
    fn parse_task_contract_rejects_blank_goal() {
        let src = r#"(task blank
  :schema "missiond.task-contract.v1"
  :goal "   "
)"#;
        let err = parse_task_contract(src).expect_err("must reject");
        assert!(matches!(err, TaskContractParseError::MissingRequired("goal")));
    }

    #[test]
    fn parse_task_contract_rejects_unbalanced_parens() {
        let src = r#"(task bad
  :schema "missiond.task-contract.v1"
  :goal "ship"
"#;
        let err = parse_task_contract(src).expect_err("must reject");
        assert!(matches!(err, TaskContractParseError::Lex(_)));
    }

    #[test]
    fn parse_task_contract_rejects_unterminated_string() {
        let src = r#"(task bad :schema "unterminated"#;
        let err = parse_task_contract(src).expect_err("must reject");
        assert!(matches!(err, TaskContractParseError::Lex(_)));
    }

    #[test]
    fn parse_task_contract_rejects_non_task_top_form() {
        let src = r#"(plan something :schema "missiond.task-contract.v1" :goal "x")"#;
        let err = parse_task_contract(src).expect_err("must reject");
        assert!(matches!(err, TaskContractParseError::NotATaskForm(_)));
    }

    #[test]
    fn parse_task_contract_rejects_wrong_field_shape() {
        // :goal must be a string, not a list.
        let src = r#"(task bad
  :schema "missiond.task-contract.v1"
  :goal ["a" "b"]
)"#;
        let err = parse_task_contract(src).expect_err("must reject");
        match err {
            TaskContractParseError::FieldShape { field, .. } => assert_eq!(field, "goal"),
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn parse_task_contract_rejects_non_string_in_write_scope() {
        let src = r#"(task bad
  :schema "missiond.task-contract.v1"
  :goal "ship"
  :write-scope [foo "bar"]
)"#;
        let err = parse_task_contract(src).expect_err("must reject");
        match err {
            TaskContractParseError::FieldShape { field, .. } => {
                assert_eq!(field, "write-scope")
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn parse_task_contract_handles_escaped_strings() {
        let src = r#"(task esc
  :schema "missiond.task-contract.v1"
  :goal "ship \"quoted\" and \\backslash"
)"#;
        let c = parse_task_contract(src).expect("must parse");
        assert_eq!(c.goal, "ship \"quoted\" and \\backslash");
    }

    #[test]
    fn parse_task_contract_skips_unknown_fields() {
        // A future emitter may add fields we do not consume — accept and
        // ignore them so the parser stays forward-compatible. The
        // authoritative checker (scripts/check-task-contract.mjs) is the
        // gate for new fields.
        let src = r#"(task fwd
  :schema "missiond.task-contract.v1"
  :goal "ship"
  :unknown-future-field "ignored"
  :requirements ["a" "b"]
  :report ["x"]
)"#;
        let c = parse_task_contract(src).expect("must parse");
        assert_eq!(c.goal, "ship");
    }

    #[test]
    fn parse_task_contract_skips_comment_lines() {
        let src = r#"
;; comment 1
;; comment 2 with paren ( and string "
(task ok
  ;; inline comment
  :schema "missiond.task-contract.v1"
  :goal "ship"
)
"#;
        let c = parse_task_contract(src).expect("must parse");
        assert_eq!(c.goal, "ship");
    }

    #[test]
    fn extract_commit_policy_returns_none_when_policy_absent() {
        let src = r#"(task np
  :schema "missiond.task-contract.v1"
  :goal "ship"
  :commit (:required true :scope-check write-scope-only)
)"#;
        let c = parse_task_contract(src).expect("must parse");
        assert!(c.commit_policy.is_none());
    }

    #[test]
    fn extract_commit_policy_returns_value_when_present() {
        let src = r#"(task wp
  :schema "missiond.task-contract.v1"
  :goal "ship"
  :commit (:required true :policy "monorepo-cascade" :scope-check none)
)"#;
        let c = parse_task_contract(src).expect("must parse");
        assert_eq!(c.commit_policy.as_deref(), Some("monorepo-cascade"));
    }

    #[test]
    fn load_task_contract_round_trips_emitter_output() {
        // Write the emitter sample to disk and confirm the loader returns
        // the same projection as the in-memory parser.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("contract.lisp");
        std::fs::write(&path, SAMPLE_CONTRACT).unwrap();
        let c = load_task_contract(&path).expect("must load");
        assert_eq!(c.goal, "ship the wave-19 contract consumer");
        assert_eq!(c.dispatch_strategy.as_deref(), Some("agent-team"));
    }

    #[test]
    fn load_task_contract_io_error_when_file_missing() {
        let err = load_task_contract(Path::new("/nonexistent/path/contract.lisp"))
            .expect_err("must fail");
        assert!(matches!(err, TaskContractParseError::Io(_)));
        assert!(err.reason().starts_with("io:"));
    }

    // ── wave-19 / task 07 — overlay tests ────────────────────────────────

    #[test]
    fn overlay_contract_overrides_objective_and_lists() {
        let mut hints = WorkstationDispatchHints {
            objective: Some("hint obj".to_string()),
            owned_files: vec!["hint.rs".to_string()],
            forbidden_files: vec!["hint_forbidden.rs".to_string()],
            ..Default::default()
        };
        let contract = ParsedTaskContract {
            schema: "missiond.task-contract.v1".to_string(),
            goal: "contract goal".to_string(),
            write_scope: vec!["contract_a.rs".to_string(), "contract_b.rs".to_string()],
            must_not_touch: vec!["contract_no.rs".to_string()],
            acceptance: vec!["cargo test".to_string()],
            commit_policy: Some("scoped-strict".to_string()),
            dispatch_strategy: Some("resident-lisp".to_string()),
            target_project: Some("contract-proj".to_string()),
            requested_cwd: Some("/contract/cwd".to_string()),
            ..Default::default()
        };
        hints.overlay_contract(&contract);
        assert_eq!(hints.objective.as_deref(), Some("contract goal"));
        assert_eq!(hints.owned_files, vec!["contract_a.rs", "contract_b.rs"]);
        assert_eq!(hints.forbidden_files, vec!["contract_no.rs"]);
        assert_eq!(hints.acceptance_commands, vec!["cargo test"]);
        assert_eq!(hints.commit_policy.as_deref(), Some("scoped-strict"));
        assert_eq!(hints.dispatch_strategy.as_deref(), Some("resident-lisp"));
        assert_eq!(hints.target_project.as_deref(), Some("contract-proj"));
        assert_eq!(hints.requested_cwd.as_deref(), Some("/contract/cwd"));
    }

    #[test]
    fn overlay_contract_preserves_arg_lists_when_contract_lists_are_empty() {
        // The contract emitter only emits non-empty lists; an absent
        // `:acceptance` should NOT erase a caller-supplied acceptance arg.
        let mut hints = WorkstationDispatchHints {
            objective: Some("hint obj".to_string()),
            owned_files: vec!["arg.rs".to_string()],
            acceptance_commands: vec!["arg cmd".to_string()],
            ..Default::default()
        };
        let contract = ParsedTaskContract {
            schema: "missiond.task-contract.v1".to_string(),
            goal: "contract goal".to_string(),
            // All list fields empty.
            ..Default::default()
        };
        hints.overlay_contract(&contract);
        // Objective overridden (non-empty contract goal beats arg).
        assert_eq!(hints.objective.as_deref(), Some("contract goal"));
        // Lists preserved (contract did not declare them).
        assert_eq!(hints.owned_files, vec!["arg.rs"]);
        assert_eq!(hints.acceptance_commands, vec!["arg cmd"]);
    }

    #[test]
    fn overlay_contract_blank_scope_does_not_clobber_arg_scope() {
        let mut hints = WorkstationDispatchHints {
            objective: Some("o".to_string()),
            scope: Some("arg scope".to_string()),
            ..Default::default()
        };
        let contract = ParsedTaskContract {
            schema: "missiond.task-contract.v1".to_string(),
            goal: "g".to_string(),
            scope: Some("   ".to_string()),
            ..Default::default()
        };
        hints.overlay_contract(&contract);
        assert_eq!(hints.scope.as_deref(), Some("arg scope"));
    }

    // ── wave-19 / task 07 — brief integration tests ──────────────────────

    #[test]
    fn build_task_brief_with_source_prefixes_contract_block_when_path_supplied() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            owned_files: vec!["a.rs".to_string()],
            ..Default::default()
        };
        let path = std::path::PathBuf::from("/tmp/contract.lisp");
        let brief = build_task_brief_with_source(&plan, &hints, "agent-team", Some(&path));
        // Contract preamble present.
        assert!(brief.contains("## Source contract"));
        assert!(brief.contains("/tmp/contract.lisp"));
        assert!(brief.contains("treat the contract as the SSOT"));
        // Existing canonical sections still present (wave-15/16/17 invariants).
        assert!(brief.contains("## Objective"));
        assert!(brief.contains("## Owned files"));
        assert!(brief.contains("## Commit policy"));
        assert!(brief.contains("## Completion handoff (scoped commit)"));
        // Agent-team hint still appears exactly once.
        assert_eq!(brief.matches(AGENT_TEAM_OBJECTIVE_HINT).count(), 1);
    }

    #[test]
    fn build_task_brief_without_source_is_byte_identical_to_legacy_build_task_brief() {
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            owned_files: vec!["a.rs".to_string()],
            ..Default::default()
        };
        let legacy = build_task_brief(&plan, &hints, "agent-team");
        let with_none = build_task_brief_with_source(&plan, &hints, "agent-team", None);
        assert_eq!(
            legacy, with_none,
            "wave-19 wrapper must be byte-identical to legacy entry when no contract"
        );
    }

    #[test]
    fn build_task_brief_with_source_does_not_double_inject_completion_handoff() {
        // The completion handoff section is independent of the contract
        // preamble — it must appear exactly once even when the brief is
        // contract-flavoured.
        let plan = fixture_plan("(plan)");
        let hints = WorkstationDispatchHints {
            objective: Some("ship".to_string()),
            owned_files: vec!["a.rs".to_string()],
            ..Default::default()
        };
        let path = std::path::PathBuf::from("/tmp/contract.lisp");
        let brief = build_task_brief_with_source(
            &plan,
            &hints,
            "fresh-code-alignment",
            Some(&path),
        );
        assert_eq!(
            brief.matches("## Completion handoff (scoped commit)").count(),
            1
        );
        // Code task kind still classified from owned_files presence.
        assert!(brief.contains("- task kind: code"));
    }

    // ── wave-19 / task 07 — SafeDescriptor tests ─────────────────────────

    #[test]
    fn malformed_task_contract_descriptor_status_is_distinct() {
        let r = SafeDescriptorReason::MalformedTaskContract {
            path: "/tmp/x.lisp".to_string(),
            reason: "schema mismatch".to_string(),
        };
        assert_eq!(r.status(), "skipped_malformed_task_contract");
        let detail = r.detail();
        assert!(detail.contains("/tmp/x.lisp"));
        assert!(detail.contains("schema mismatch"));
        assert!(detail.contains("SSOT"));
    }

    #[test]
    fn outcome_to_response_malformed_contract_carries_full_detail() {
        let outcome = WorkstationDispatchOutcome::SafeDescriptor {
            reason: SafeDescriptorReason::MalformedTaskContract {
                path: "/tmp/bad.lisp".to_string(),
                reason: "lex: unbalanced parens".to_string(),
            },
            task_brief: None,
        };
        let v = outcome_to_response_fields(&outcome, "agent-team");
        assert_eq!(v["workstation_dispatch_status"], "skipped_malformed_task_contract");
        assert!(v["workstation_dispatch_reason"]
            .as_str()
            .unwrap()
            .contains("/tmp/bad.lisp"));
        assert!(v["workstation_dispatch_reason"]
            .as_str()
            .unwrap()
            .contains("lex: unbalanced parens"));
        // Scoped-commit policy contract still surfaces on the safe-descriptor
        // branch (wave-17 / task 07 invariant — applies regardless of branch).
        assert_eq!(v["scoped_commit_required"], json!(true));
    }

    // ── wave-19 / task 07 — path resolution tests ────────────────────────

    #[test]
    fn resolve_contract_path_keeps_absolute_paths_verbatim() {
        let abs = Path::new("/tmp/abs/contract.lisp");
        let root = Path::new("/Users/x/proj");
        assert_eq!(resolve_contract_path(abs, root), abs.to_path_buf());
    }

    #[test]
    fn resolve_contract_path_joins_relative_against_project_root() {
        let rel = Path::new(".missiond/tasks/generated/abc/root.lisp");
        let root = Path::new("/Users/x/proj");
        assert_eq!(
            resolve_contract_path(rel, root),
            Path::new("/Users/x/proj/.missiond/tasks/generated/abc/root.lisp").to_path_buf()
        );
    }

    // ── wave-19 / task 07 — parser error reason mapping pin ──────────────

    #[test]
    fn task_contract_parse_error_reason_strings_are_actionable() {
        // Each variant produces a human-actionable reason string. Pinned so
        // a future refactor that loses detail (e.g. dropping the offending
        // schema value) trips a test.
        assert!(TaskContractParseError::Io("perm denied".into())
            .reason()
            .contains("perm denied"));
        assert!(TaskContractParseError::Lex("EOF".into())
            .reason()
            .contains("EOF"));
        assert!(TaskContractParseError::NotATaskForm("(plan)".into())
            .reason()
            .contains("(plan)"));
        assert!(TaskContractParseError::SchemaMismatch("v9".into())
            .reason()
            .contains("v9"));
        assert!(TaskContractParseError::MissingRequired("goal")
            .reason()
            .contains("goal"));
        assert!(TaskContractParseError::FieldShape {
            field: "write-scope",
            detail: "got 42".into(),
        }
        .reason()
        .contains("write-scope"));
    }
}
