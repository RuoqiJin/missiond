use anyhow::{anyhow, Result};

use super::lisp_syntax::{self as sexp, Node};
use super::log_store::{now_iso, LogFile};

// ───────────────────────────────────────────────────────────────────────
// canonical writer helpers — used by durable companion-log mutations
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
    refresh_root(file, "kv update")
}

/// Insert `entry_text` (already-rendered S-expr lines without leading newline)
/// into the block named `block_name` just before its closing paren. If the
/// block is missing it is synthesized at the end of the root form (audit will
/// flag the file but the append still succeeds).
pub(super) fn append_to_block(
    file: &mut LogFile,
    block_name: &str,
    entry_text: &str,
) -> Result<()> {
    if let Some(block) = file.find_block(block_name).cloned() {
        let close = block.end - 1;
        let body_is_empty = block_body_is_empty(&block);
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
    refresh_root(file, "append")
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
        refresh_root(file, "touch")?;
    }
    Ok(())
}

fn block_body_is_empty(block: &Node) -> bool {
    block.children().len() <= 1
}

fn refresh_root(file: &mut LogFile, action: &str) -> Result<()> {
    let forms = sexp::parse(&file.src)?;
    let root_idx = forms
        .iter()
        .position(|n| matches!(n.head_atom(), Some("execution-log") | Some("execution")))
        .ok_or_else(|| anyhow!("execution-log root vanished after {}", action))?;
    file.forms = forms;
    file.root_idx = root_idx;
    Ok(())
}
