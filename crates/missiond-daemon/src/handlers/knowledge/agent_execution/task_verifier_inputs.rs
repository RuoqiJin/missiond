use anyhow::{anyhow, Result};

use super::lisp_syntax::{self as sexp, Node, NodeKind};

/// wave-21 / task 03 — minimal report-contract reader.
///
/// Pulls just the keys the daemon-side cross-check needs (`:schema`,
/// `:task_id`, `:commit_hash`) out of a `(report <id> ...)` form using
/// the local sexp parser. No new dependency, no new lisp dialect — the
/// projector trusts the authoritative schema checker
/// (`scripts/check-task-report.mjs`) for shape policing and only echoes
/// the three fields the daemon needs for the wave21-03 verified-gate
/// cross-check.
pub(super) struct ReportSummary {
    pub(super) schema: Option<String>,
    pub(super) task_id: Option<String>,
    pub(super) commit_hash: Option<String>,
}

pub(super) fn read_report_summary(text: &str) -> Result<ReportSummary> {
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
            sexp::NodeKind::Str(s) => Some(s.clone()),
            sexp::NodeKind::Atom(a) => Some(a.clone()),
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
pub(super) fn read_task_contract_id(text: &str) -> Option<String> {
    let nodes = sexp::parse(text).ok()?;
    let top = nodes.first()?;
    if top.head_atom() != Some("task") {
        return None;
    }
    let kids = top.children();
    kids.get(1).and_then(|n| n.as_atom().map(|s| s.to_string()))
}

/// wave-22 / task 02 — minimal shared-memory ledger projector.
///
/// Pulls just the `:schema` field and the list of `:task` ids that
/// appear inside `(completion ...)` children. Mirrors the wave21-02
/// `loadLedger` projection in `scripts/verify-task-run.mjs` so the
/// daemon-side auto-verifier hits the same rule:
/// `ledger.completions.some(c => c.task === contract.id)`.
pub(super) struct SharedMemorySummary {
    pub(super) schema: Option<String>,
    pub(super) completion_tasks: Vec<String>,
}

pub(super) fn read_shared_memory_ledger(text: &str) -> Result<SharedMemorySummary, anyhow::Error> {
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
pub(super) fn read_completion_task_id(node: &Node) -> Option<String> {
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
