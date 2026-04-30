use anyhow::{anyhow, Result};
use chrono::{SecondsFormat, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

pub(super) use super::log_paths::{
    companion_path, project_or_target_project, require_str, resolve_project_root, COMPANION_DIR,
};
pub(super) use super::log_template::render_canonical_template;

use super::lisp_syntax::{self as sexp, Node, NodeKind};

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

/// Update or insert `:key value` inside the given node. The node must be a
/// list; insertion happens just before the closing paren.
pub(super) fn update_kv_in_node(
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

pub(super) fn list_block_summaries<F>(file: &LogFile, name: &str, mut f: F) -> Vec<Value>
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

pub(super) fn json_strip_quotes(map: HashMap<String, String>) -> Value {
    let mut obj = serde_json::Map::new();
    for (k, v) in map {
        let trimmed = v.trim();
        let unquoted = trimmed.trim_matches('"');
        obj.insert(k, Value::String(unquoted.to_string()));
    }
    Value::Object(obj)
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
