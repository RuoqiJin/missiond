use super::MANIFEST_SCHEMA;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Read manifest path arg + load + parse. Always returns a structured
/// `RunnerInputs`; failure modes degrade through `manifest_status`.
pub(super) fn load_runner_inputs(args: &Value) -> RunnerInputs {
    let manifest_path = arg_string(args, "task_runner_manifest_path");
    let Some(path_str) = manifest_path else {
        return RunnerInputs {
            manifest_path: None,
            manifest_status: ManifestStatus::Missing,
            warning: Some(
                "task_runner_mode=dry_run requires task_runner_manifest_path".to_string(),
            ),
            manifest: None,
        };
    };
    let resolved = resolve_manifest_path(&path_str);
    let raw = match std::fs::read_to_string(&resolved) {
        Ok(s) => s,
        Err(e) => {
            let warning = format!("manifest read failed: {}", e);
            let status = if e.kind() == std::io::ErrorKind::NotFound {
                ManifestStatus::Missing
            } else {
                ManifestStatus::Unreadable
            };
            return RunnerInputs {
                manifest_path: Some(path_str),
                manifest_status: status,
                warning: Some(warning),
                manifest: None,
            };
        }
    };
    match parse_manifest(&raw) {
        Ok(m) => RunnerInputs {
            manifest_path: Some(path_str),
            manifest_status: ManifestStatus::Used,
            warning: None,
            manifest: Some(m),
        },
        Err(msg) => RunnerInputs {
            manifest_path: Some(path_str),
            manifest_status: ManifestStatus::Malformed,
            warning: Some(format!("manifest parse failed: {}", msg)),
            manifest: None,
        },
    }
}

/// Resolve a manifest path verbatim (mirrors `resolve_policy_path`
/// in `router_policy_dry_run`: absolute paths stay absolute;
/// relative paths are passed verbatim and resolved against the
/// daemon's CWD). Free of repo-root detection logic so tests can
/// pass tmp paths.
fn resolve_manifest_path(input: &str) -> PathBuf {
    let p = Path::new(input);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        PathBuf::from(input)
    }
}

fn arg_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Manifest read / parse status surfaced on the response block.
/// Mirrors the wave26-03 `BackendRegistryInfo` enum-as-status pattern
/// and is encoded as a string in the `manifest_status` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ManifestStatus {
    Used,
    Missing,
    Unreadable,
    Malformed,
}

impl ManifestStatus {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            ManifestStatus::Used => "used",
            ManifestStatus::Missing => "missing",
            ManifestStatus::Unreadable => "unreadable",
            ManifestStatus::Malformed => "malformed",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct RunnerInputs {
    pub(super) manifest_path: Option<String>,
    pub(super) manifest_status: ManifestStatus,
    pub(super) warning: Option<String>,
    pub(super) manifest: Option<Manifest>,
}

/// Minimal Rust subset of the wave28-01 manifest. Only the fields
/// needed for the wave28-04 projection are extracted; unknown keys
/// are tolerated so future schema growth does not require a daemon
/// update.
#[derive(Debug, Clone)]
pub(super) struct Manifest {
    pub(super) wave: String,
    pub(super) productive_only: bool,
    pub(super) overlap_policy: String,
    pub(super) nodes: Vec<ManifestNode>,
}

#[derive(Debug, Clone)]
pub(super) struct ManifestNode {
    pub(super) task_id: String,
    pub(super) depends_on: Vec<String>,
    pub(super) dispatch_group: String,
    pub(super) verification_tier: String,
    pub(super) estimated_minutes: u64,
    pub(super) write_scope: Vec<String>,
}

/// Manifest parser. Returns a structured `Manifest` or a string
/// describing the failure. Reuses the wave24-04 in-module S-expression
/// tokeniser by re-implementing the minimal subset needed here (so
/// this module stays self-contained and the cross-module surface
/// stays small). The parser deliberately validates ONLY the bits
/// required to project the response — full schema validation lives
/// in the wave28-01 Node checker.
fn parse_manifest(input: &str) -> Result<Manifest, String> {
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
        _ => return Err("expected (task-runner-manifest ...) at top level".to_string()),
    };
    let mut iter = list.into_iter();
    let head = iter
        .next()
        .ok_or_else(|| "empty top-level list".to_string())?;
    match head {
        Sexp::Atom(s) if s == "task-runner-manifest" => {}
        _ => return Err("expected (task-runner-manifest ...) at top level".to_string()),
    }
    // Skip the manifest id atom (next item).
    let _id = iter.next();

    let mut schema: Option<String> = None;
    let mut wave: Option<String> = None;
    let mut productive_only: Option<bool> = None;
    let mut overlap_policy: Option<String> = None;
    let mut nodes: Vec<ManifestNode> = Vec::new();
    let mut pending_keyword: Option<String> = None;

    for item in iter {
        if let Some(key) = pending_keyword.take() {
            match key.as_str() {
                ":schema" => schema = Some(sexp_as_text(&item)),
                ":wave" => wave = Some(sexp_as_text(&item)),
                ":productive_only" => productive_only = Some(sexp_as_bool(&item)),
                ":overlap_policy" => overlap_policy = Some(sexp_as_text(&item)),
                // Tolerated header keys: :brief_mode, :shared_preamble_path,
                // :description, :generated_at, :generator. Future schema
                // growth does not require a daemon update.
                _ => {}
            }
            continue;
        }
        match &item {
            Sexp::Keyword(k) => pending_keyword = Some(k.clone()),
            Sexp::List(inner) => {
                if matches!(inner.first(), Some(Sexp::Atom(h)) if h == "node") {
                    let entry = parse_node_entry(inner)?;
                    nodes.push(entry);
                }
                // Other top-level lists are tolerated.
            }
            _ => {}
        }
    }

    let schema = schema.ok_or_else(|| "missing :schema header field".to_string())?;
    if schema != MANIFEST_SCHEMA {
        return Err(format!(
            "header :schema `{}` does not match {}",
            schema, MANIFEST_SCHEMA
        ));
    }
    let wave = wave.ok_or_else(|| "missing :wave header field".to_string())?;
    let productive_only =
        productive_only.ok_or_else(|| "missing :productive_only header field".to_string())?;
    // Default per wave28-01 schema: reject. Unknown values are coerced
    // to reject so a typo is loud (severity=error).
    let overlap_policy = overlap_policy
        .map(|s| {
            if s == "warn" || s == "reject" {
                s
            } else {
                "reject".to_string()
            }
        })
        .unwrap_or_else(|| "reject".to_string());

    Ok(Manifest {
        wave,
        productive_only,
        overlap_policy,
        nodes,
    })
}

fn parse_node_entry(items: &[Sexp]) -> Result<ManifestNode, String> {
    // items[0] is the `node` atom.
    let mut task_id: Option<String> = None;
    let mut depends_on: Option<Vec<String>> = None;
    let mut dispatch_group: Option<String> = None;
    let mut verification_tier: Option<String> = None;
    let mut estimated_minutes: Option<u64> = None;
    let mut write_scope: Option<Vec<String>> = None;
    let mut idx = 1usize;
    // Optional first positional after `node` may be the task id atom
    // (mirrors the wave28-01 schema: many manifests use `(node <id> :prop ...)`
    // even though :task_id is the canonical key). Tolerate both shapes.
    if let Some(item) = items.get(idx) {
        if !matches!(item, Sexp::Keyword(_)) {
            let candidate = sexp_as_text(item);
            if !candidate.is_empty() {
                task_id = Some(candidate);
                idx += 1;
            }
        }
    }
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
            ":task_id" => task_id = Some(sexp_as_text(value)),
            ":depends_on" => depends_on = Some(sexp_as_string_vec(value)),
            ":dispatch_group" => dispatch_group = Some(sexp_as_text(value)),
            ":verification_tier" => verification_tier = Some(sexp_as_text(value)),
            ":estimated_minutes" => estimated_minutes = sexp_as_positive_u64(value),
            ":write_scope" => write_scope = Some(sexp_as_string_vec(value)),
            // Tolerated optional fields: :heartbeat_minutes, :notes, :owner, :kind.
            _ => {}
        }
    }
    let task_id = task_id.ok_or_else(|| "node missing :task_id".to_string())?;
    if task_id.is_empty() {
        return Err("node has empty :task_id".to_string());
    }
    let depends_on = depends_on.unwrap_or_default();
    let dispatch_group =
        dispatch_group.ok_or_else(|| format!("node `{}` missing :dispatch_group", task_id))?;
    if dispatch_group.is_empty() {
        return Err(format!("node `{}` has empty :dispatch_group", task_id));
    }
    let verification_tier = verification_tier
        .ok_or_else(|| format!("node `{}` missing :verification_tier", task_id))?;
    let estimated_minutes = estimated_minutes
        .ok_or_else(|| format!("node `{}` missing or invalid :estimated_minutes", task_id))?;
    let write_scope = write_scope.unwrap_or_default();
    Ok(ManifestNode {
        task_id,
        depends_on,
        dispatch_group,
        verification_tier,
        estimated_minutes,
        write_scope,
    })
}

fn sexp_as_text(value: &Sexp) -> String {
    match value {
        Sexp::Atom(s) => s.clone(),
        Sexp::Str(s) => s.clone(),
        Sexp::Keyword(s) => s.clone(),
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

fn sexp_as_positive_u64(value: &Sexp) -> Option<u64> {
    let raw = match value {
        Sexp::Atom(s) => s.clone(),
        Sexp::Str(s) => s.clone(),
        _ => return None,
    };
    raw.parse::<u64>().ok().filter(|n| *n > 0)
}

fn sexp_as_string_vec(value: &Sexp) -> Vec<String> {
    match value {
        Sexp::List(items) => items
            .iter()
            .map(sexp_as_text)
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

// ---- tiny tokeniser / cursor (self-contained copy) ----------------
// Mirrors the wave24-04 in-module S-expression tokeniser. Re-implemented
// here so this module stays self-contained; the parsers in the two
// modules accept the same surface but extract different schemas.

#[derive(Debug, Clone)]
enum Sexp {
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
