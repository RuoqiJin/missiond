use anyhow::{anyhow, Result};
use missiond_mcp::tools::{ToolError, ToolResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use super::log_surface::sexp::{self, Node, NodeKind};

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
pub(super) fn auto_run_task_run_verifier(
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
    let _contract = match super::super::workstation_dispatch::load_task_contract(&contract_resolved)
    {
        Ok(c) => c,
        Err(e) => {
            use super::super::workstation_dispatch::TaskContractParseError as Tce;
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
pub(super) fn enforce_verified_completion(
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
