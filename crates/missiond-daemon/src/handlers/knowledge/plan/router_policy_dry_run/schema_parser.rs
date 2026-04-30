use super::{readiness::BackendEntry, Clause, PolicyDoc, RuleDoc};

/// Allowed readiness status values mirrored from the wave26-01 schema.
/// Anything outside this set is treated as malformed (the wave26-01
/// checker rejects unknown values upstream; this is a defence-in-depth
/// re-check).
const READINESS_STATUSES: &[&str] = &[
    "current-default",
    "advisory-only",
    "runtime-ready",
    "unavailable",
];

/// Minimal Lisp parser for the wave26-01 registry. Reuses the existing
/// tokeniser + cursor; extracts ONLY `:id` `:readiness_status`
/// `:runtime_allowed` `:apply_blockers` per `(backend ...)` entry. Any
/// other key inside a backend entry is tolerated (gracefully ignored)
/// so the registry schema can grow without breaking the daemon.
pub(super) fn parse_backend_registry(input: &str) -> Result<Vec<BackendEntry>, String> {
    let tokens = tokenize(input)?;
    let mut cursor = TokenCursor::new(&tokens);
    let form = cursor
        .read_form()
        .ok_or_else(|| "no form found".to_string())?;
    if cursor.peek().is_some() {
        return Err("multiple top-level forms".to_string());
    }
    let list = match form {
        Sexp::List(items) => items,
        _ => return Err("expected (router-backend-registry ...) at top level".to_string()),
    };
    let mut iter = list.into_iter();
    let head = iter
        .next()
        .ok_or_else(|| "empty top-level list".to_string())?;
    match head {
        Sexp::Atom(s) if s == "router-backend-registry" => {}
        _ => return Err("expected (router-backend-registry ...) at top level".to_string()),
    }
    // Skip the registry id atom (next item).
    let _id = iter.next();
    let mut backends: Vec<BackendEntry> = Vec::new();
    let mut pending_keyword: Option<String> = None;
    for item in iter {
        if pending_keyword.take().is_some() {
            // Header keyword/value pair (`:schema`, `:version`,
            // `:description`) — value already consumed; skip.
            continue;
        }
        match &item {
            Sexp::Keyword(k) => pending_keyword = Some(k.clone()),
            Sexp::List(inner) => {
                if matches!(inner.first(), Some(Sexp::Atom(h)) if h == "backend") {
                    let entry = parse_backend_entry(inner)?;
                    backends.push(entry);
                }
                // Other top-level lists (none today) are tolerated.
            }
            _ => {}
        }
    }
    Ok(backends)
}

fn parse_backend_entry(items: &[Sexp]) -> Result<BackendEntry, String> {
    // items[0] is the `backend` atom.
    let mut id: Option<String> = None;
    let mut readiness_status: Option<String> = None;
    let mut runtime_allowed: Option<bool> = None;
    let mut apply_blockers: Option<Vec<String>> = None;
    let mut idx = 1usize;
    while idx < items.len() {
        let key = match &items[idx] {
            Sexp::Keyword(k) => k.clone(),
            _ => {
                idx += 1;
                continue;
            }
        };
        idx += 1;
        if idx >= items.len() {
            break;
        }
        let value = &items[idx];
        idx += 1;
        match key.as_str() {
            ":id" => id = Some(sexp_as_text(value)),
            ":readiness_status" => readiness_status = Some(sexp_as_text(value)),
            ":runtime_allowed" => runtime_allowed = Some(sexp_as_bool(value)),
            ":apply_blockers" => {
                let v = sexp_as_string_vec(value);
                apply_blockers = Some(v);
            }
            // Tolerated but not consumed: :substrate, :non-goals,
            // :notes, :owner, :adapter_path. Future schema growth
            // does not require a daemon update.
            _ => {}
        }
    }
    let id = id.ok_or_else(|| "backend entry missing :id".to_string())?;
    let readiness_status =
        readiness_status.ok_or_else(|| format!("backend `{}` missing :readiness_status", id))?;
    if !READINESS_STATUSES.iter().any(|s| *s == readiness_status) {
        return Err(format!(
            "backend `{}` :readiness_status `{}` is not in the wave26-01 enum",
            id, readiness_status
        ));
    }
    let runtime_allowed =
        runtime_allowed.ok_or_else(|| format!("backend `{}` missing :runtime_allowed", id))?;
    let apply_blockers = apply_blockers.unwrap_or_default();
    Ok(BackendEntry {
        id,
        readiness_status,
        runtime_allowed,
        apply_blockers,
    })
}

/// Coerce a `Sexp::List` of strings/atoms into a `Vec<String>`. Used
/// for `:apply_blockers` (the wave26-01 schema requires a vector of
/// strings; an empty vector is `[]`).
fn sexp_as_string_vec(value: &Sexp) -> Vec<String> {
    match value {
        Sexp::List(items) => items
            .iter()
            .map(|i| sexp_as_text(i))
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

// ---- minimal Lisp parser for the wave24-01 router-policy schema ---

/// Parse a router-policy v1 Lisp file. Returns a structured `PolicyDoc`
/// or a human-readable error message. The parser is purpose-built for
/// this schema and does NOT attempt to be a general Lisp reader: it
/// handles atoms, strings, lists, brackets-as-lists, and line comments.
/// The wave24-01 checker already rejects malformed policies upstream;
/// this parser is conservative and surfaces unknown shapes as errors.
pub(super) fn parse_router_policy(input: &str) -> Result<PolicyDoc, String> {
    let tokens = tokenize(input)?;
    let mut cursor = TokenCursor::new(&tokens);
    // Top-level form must be `(router-policy <id> ...)`.
    let form = cursor
        .read_form()
        .ok_or_else(|| "no form found".to_string())?;
    if cursor.peek().is_some() {
        // We tolerate trailing whitespace / comments (already stripped
        // by tokenize) but not multiple top-level forms.
        return Err("multiple top-level forms".to_string());
    }
    let list = match form {
        Sexp::List(items) => items,
        _ => return Err("expected (router-policy ...) at top level".to_string()),
    };
    let mut iter = list.into_iter();
    let head = iter
        .next()
        .ok_or_else(|| "empty top-level list".to_string())?;
    match head {
        Sexp::Atom(s) if s == "router-policy" => {}
        _ => return Err("expected (router-policy ...) at top level".to_string()),
    }
    // Skip the policy id atom (next item).
    let _id = iter.next();
    // Walk the remaining items: keyword/value pairs OR (rule ...) lists.
    let mut dry_run_only: Option<bool> = None;
    let mut runtime_replacement: Option<bool> = None;
    let mut rules: Vec<RuleDoc> = Vec::new();
    let mut pending_keyword: Option<String> = None;
    for item in iter {
        if let Some(key) = pending_keyword.take() {
            let value = item;
            match key.as_str() {
                ":dry-run-only" => dry_run_only = Some(sexp_as_bool(&value)),
                ":runtime-replacement" => runtime_replacement = Some(sexp_as_bool(&value)),
                // Other keys (`:schema`, `:version`, `:description`)
                // are tolerated but not consumed — wave24-01 checker
                // owns header validation.
                _ => {}
            }
            continue;
        }
        match &item {
            Sexp::Keyword(k) => pending_keyword = Some(k.clone()),
            Sexp::List(inner) => {
                if matches!(inner.first(), Some(Sexp::Atom(h)) if h == "rule") {
                    let rule = parse_rule(inner)?;
                    rules.push(rule);
                }
            }
            _ => {}
        }
    }
    // Sort rules by priority ascending (matches wave24-03 selection order).
    rules.sort_by_key(|r| r.priority);
    Ok(PolicyDoc {
        dry_run_only: dry_run_only.unwrap_or(false),
        runtime_replacement: runtime_replacement.unwrap_or(false),
        rules,
    })
}

fn parse_rule(items: &[Sexp]) -> Result<RuleDoc, String> {
    // items[0] is the `rule` atom.
    let mut id: Option<String> = None;
    let mut priority: Option<u32> = None;
    let mut when_clause: Option<Clause> = None;
    let mut backend: Option<String> = None;
    let mut reasoning: Option<String> = None;
    let mut idx = 1usize;
    while idx < items.len() {
        let key = match &items[idx] {
            Sexp::Keyword(k) => k.clone(),
            _ => {
                idx += 1;
                continue;
            }
        };
        idx += 1;
        if idx >= items.len() {
            break;
        }
        let value = &items[idx];
        idx += 1;
        match key.as_str() {
            ":id" => id = Some(sexp_as_text(value)),
            ":priority" => {
                let raw = sexp_as_text(value);
                priority = raw.parse::<u32>().ok();
            }
            ":when" => {
                if let Sexp::List(children) = value {
                    when_clause = Some(parse_when_list(children)?);
                }
            }
            ":recommend" => {
                if let Sexp::List(children) = value {
                    let mut bk: Option<String> = None;
                    let mut rs: Option<String> = None;
                    let mut j = 0usize;
                    while j < children.len() {
                        if let Sexp::Keyword(k) = &children[j] {
                            if j + 1 < children.len() {
                                let v = &children[j + 1];
                                match k.as_str() {
                                    ":backend" => bk = Some(sexp_as_text(v)),
                                    ":reasoning" => rs = Some(sexp_as_text(v)),
                                    _ => {}
                                }
                            }
                            j += 2;
                        } else {
                            j += 1;
                        }
                    }
                    backend = bk;
                    reasoning = rs;
                }
            }
            // `:non-goals`, `:notes` are tolerated but not consumed.
            _ => {}
        }
    }
    Ok(RuleDoc {
        id: id.ok_or_else(|| "rule missing :id".to_string())?,
        priority: priority.ok_or_else(|| "rule missing :priority".to_string())?,
        when: when_clause.ok_or_else(|| "rule missing :when".to_string())?,
        backend: backend.ok_or_else(|| "rule missing :recommend :backend".to_string())?,
        reasoning: reasoning.unwrap_or_default(),
    })
}

fn parse_when_list(children: &[Sexp]) -> Result<Clause, String> {
    // The top-level `:when` is implicit-`all` over its direct children.
    let mut clauses: Vec<Clause> = Vec::new();
    for child in children {
        if let Sexp::List(inner) = child {
            if let Some(c) = parse_clause(inner)? {
                clauses.push(c);
            }
        }
    }
    if clauses.len() == 1 {
        Ok(clauses.into_iter().next().unwrap())
    } else {
        Ok(Clause::All(clauses))
    }
}

fn parse_clause(items: &[Sexp]) -> Result<Option<Clause>, String> {
    let head_atom = match items.first() {
        Some(Sexp::Atom(s)) => s.clone(),
        _ => return Ok(None),
    };
    match head_atom.as_str() {
        "kind" => Ok(Some(Clause::Kind(arg_value(items)))),
        "dispatch_strategy" | "dispatch-strategy" => {
            Ok(Some(Clause::DispatchStrategy(arg_value(items))))
        }
        "owner" => Ok(Some(Clause::Owner(arg_value(items)))),
        "status" => Ok(Some(Clause::Status(arg_value(items)))),
        "path-glob" => Ok(Some(Clause::PathGlob(arg_value(items)))),
        "any" => {
            let mut children = Vec::new();
            for it in &items[1..] {
                if let Sexp::List(inner) = it {
                    if let Some(c) = parse_clause(inner)? {
                        children.push(c);
                    }
                }
            }
            Ok(Some(Clause::Any(children)))
        }
        "all" => {
            let mut children = Vec::new();
            for it in &items[1..] {
                if let Sexp::List(inner) = it {
                    if let Some(c) = parse_clause(inner)? {
                        children.push(c);
                    }
                }
            }
            Ok(Some(Clause::All(children)))
        }
        // Unknown predicate head — fail closed to mirror wave24-03.
        _ => Err(format!("unknown predicate head `{}`", head_atom)),
    }
}

fn arg_value(items: &[Sexp]) -> String {
    items.get(1).map(|v| sexp_as_text(v)).unwrap_or_default()
}

fn sexp_as_text(value: &Sexp) -> String {
    match value {
        Sexp::Atom(s) | Sexp::Str(s) | Sexp::Keyword(s) => s.clone(),
        Sexp::List(_) => String::new(),
    }
}

fn sexp_as_bool(value: &Sexp) -> bool {
    match value {
        Sexp::Atom(s) => s == "true",
        Sexp::Str(s) => s == "true",
        _ => false,
    }
}

// ---- tiny tokenizer / cursor ------------------------------------

#[derive(Debug, Clone)]
pub(super) enum Sexp {
    Atom(String),
    Str(String),
    Keyword(String),
    List(Vec<Sexp>),
}

#[derive(Debug, Clone)]
enum Token {
    LParen,
    RParen,
    LBracket,
    RBracket,
    Atom(String),
    Str(String),
    Keyword(String),
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == ';' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '(' {
            out.push(Token::LParen);
            i += 1;
            continue;
        }
        if c == ')' {
            out.push(Token::RParen);
            i += 1;
            continue;
        }
        if c == '[' {
            out.push(Token::LBracket);
            i += 1;
            continue;
        }
        if c == ']' {
            out.push(Token::RBracket);
            i += 1;
            continue;
        }
        if c == '"' {
            let mut s = String::new();
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                if ch == '\\' {
                    i += 1;
                    if i < chars.len() {
                        s.push(chars[i]);
                        i += 1;
                    }
                    continue;
                }
                if ch == '"' {
                    i += 1;
                    break;
                }
                s.push(ch);
                i += 1;
            }
            out.push(Token::Str(s));
            continue;
        }
        if c == ':' {
            let mut s = String::from(":");
            i += 1;
            while i < chars.len() && !is_atom_terminator(chars[i]) {
                s.push(chars[i]);
                i += 1;
            }
            out.push(Token::Keyword(s));
            continue;
        }
        // Atom.
        let mut s = String::new();
        while i < chars.len() && !is_atom_terminator(chars[i]) {
            s.push(chars[i]);
            i += 1;
        }
        if !s.is_empty() {
            out.push(Token::Atom(s));
        }
    }
    Ok(out)
}

fn is_atom_terminator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '"' | ';')
}

struct TokenCursor<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> TokenCursor<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, pos: 0 }
    }
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }
    fn read_form(&mut self) -> Option<Sexp> {
        let tok = self.tokens.get(self.pos)?;
        match tok {
            Token::LParen => {
                self.pos += 1;
                let mut items = Vec::new();
                while let Some(t) = self.tokens.get(self.pos) {
                    if matches!(t, Token::RParen) {
                        self.pos += 1;
                        return Some(Sexp::List(items));
                    }
                    if let Some(form) = self.read_form() {
                        items.push(form);
                    } else {
                        break;
                    }
                }
                Some(Sexp::List(items))
            }
            Token::LBracket => {
                self.pos += 1;
                let mut items = Vec::new();
                while let Some(t) = self.tokens.get(self.pos) {
                    if matches!(t, Token::RBracket) {
                        self.pos += 1;
                        return Some(Sexp::List(items));
                    }
                    if let Some(form) = self.read_form() {
                        items.push(form);
                    } else {
                        break;
                    }
                }
                Some(Sexp::List(items))
            }
            Token::RParen | Token::RBracket => None,
            Token::Atom(s) => {
                self.pos += 1;
                Some(Sexp::Atom(s.clone()))
            }
            Token::Str(s) => {
                self.pos += 1;
                Some(Sexp::Str(s.clone()))
            }
            Token::Keyword(s) => {
                self.pos += 1;
                Some(Sexp::Keyword(s.clone()))
            }
        }
    }
}
