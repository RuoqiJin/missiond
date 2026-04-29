use std::path::{Path, PathBuf};

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
    /// wave-23 / task 05 — optional session-trace ledger path. Mirrors
    /// the `:session-trace-path` field emitted by
    /// `plan::build_task_contract_lisp` so the dispatch overlay can
    /// re-derive the path when only the contract is supplied (caller
    /// dropped the explicit arg).
    pub session_trace_path: Option<String>,
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
        let val = iter.next().ok_or_else(|| {
            TaskContractParseError::Lex(format!("keyword `:{}` missing value", k))
        })?;
        match k.as_str() {
            "schema" => out.schema = require_string(val, "schema")?,
            "goal" => out.goal = require_string(val, "goal")?,
            "scope" => out.scope = Some(require_string(val, "scope")?),
            "write-scope" => out.write_scope = require_string_list(val, "write-scope")?,
            "must-not-touch" => out.must_not_touch = require_string_list(val, "must-not-touch")?,
            "acceptance" => out.acceptance = require_string_list(val, "acceptance")?,
            "commit" => {
                out.commit_policy = extract_commit_policy(val)?;
            }
            "dispatch-strategy" => {
                out.dispatch_strategy = Some(require_string(val, "dispatch-strategy")?)
            }
            "target-project" => out.target_project = Some(require_string(val, "target-project")?),
            "requested-cwd" => out.requested_cwd = Some(require_string(val, "requested-cwd")?),
            "target" => out.target = Some(require_string(val, "target")?),
            // wave-23 / task 05 — accept-and-store the optional session-trace
            // ledger path so a contract-driven dispatch (machine mode) can
            // re-derive the path even when the caller dropped the explicit
            // arg. The kebab-case `:session-trace-path` keyword matches the
            // emitter (`plan::build_task_contract_lisp`).
            "session-trace-path" => {
                out.session_trace_path = Some(require_string(val, "session-trace-path")?)
            }
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
        return Err(TaskContractParseError::SchemaMismatch(
            "(absent)".to_string(),
        ));
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
        let val = iter
            .next()
            .ok_or_else(|| TaskContractParseError::FieldShape {
                field: "commit",
                detail: "`:policy` keyword missing value".to_string(),
            })?;
        return Ok(Some(require_string(val, "commit.policy")?));
    }
    Ok(None)
}

/// Pure parser entrypoint — exercised directly by the unit tests. The
/// loader (`load_task_contract`) wraps this with file IO.
pub(crate) fn parse_task_contract(src: &str) -> Result<ParsedTaskContract, TaskContractParseError> {
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
pub(super) fn resolve_contract_path(raw: &Path, project_root: &Path) -> PathBuf {
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        project_root.join(raw)
    }
}

/// wave-22 / task 05 — public re-export of `resolve_contract_path` so
/// the auto-spawn gate caller (plan.rs) can pre-resolve a relative
/// `task_contract_path` against the daemon's process cwd / configured
/// project anchor BEFORE the substrate runs. The substrate path itself
/// still re-resolves via `resolve_target_project_root` — this helper
/// just lets the gate evaluator load + parse the contract early so
/// `:write-scope` / `:must-not-touch` checks fire BEFORE any spawn.
pub(crate) fn resolve_contract_path_public(raw: &Path, project_root: &Path) -> PathBuf {
    resolve_contract_path(raw, project_root)
}
