use anyhow::{anyhow, Result};

use super::lisp_syntax::{self as sexp, NodeKind};
use super::log_store::{locate_kv_value, LogFile};

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

pub(super) fn insert_id_counters_block(
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
