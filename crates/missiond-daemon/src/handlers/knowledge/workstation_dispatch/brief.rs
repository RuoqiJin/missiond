use std::path::Path;

use missiond_core::types::Plan;

use super::super::plan::AGENT_TEAM_OBJECTIVE_HINT;
use super::{WorkstationDispatchHints, COMMIT_POLICY_SCOPED};
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

pub(crate) fn workstation_execution_id(plan: &Plan) -> String {
    format!("plan-{}", plan.id)
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
///
/// wave-23 / task 05 — added `session_trace_path` parameter (kept as a
/// separate parameter rather than a field on `WorkstationDispatchHints`
/// so out-of-scope callers in `plan_dag.rs` / `unified_entry.rs` keep
/// working without struct-literal modifications). When `Some`, the
/// brief gains a `## Session trace` block before the parallelism hint.
pub(crate) fn build_task_brief_with_source(
    plan: &Plan,
    hints: &WorkstationDispatchHints,
    dispatch_strategy: &str,
    contract_source: Option<&Path>,
) -> String {
    build_task_brief_with_source_and_trace(plan, hints, dispatch_strategy, contract_source, None)
}

/// wave-23 / task 05 — variant of `build_task_brief_with_source` that
/// also accepts an optional session-trace ledger path. The path is
/// surfaced as a `## Session trace` block in the brief so the worker
/// knows which ledger to record `mission_execution(action=*)` events
/// against. `None` preserves the wave-15..22 byte shape exactly.
pub(crate) fn build_task_brief_with_source_and_trace(
    plan: &Plan,
    hints: &WorkstationDispatchHints,
    dispatch_strategy: &str,
    contract_source: Option<&Path>,
    session_trace_path: Option<&str>,
) -> String {
    let mut out = String::new();

    // Header pins plan + board_task so the delegated agent always knows
    // which row it is acting on.
    out.push_str(&format!("# Plan {} — workstation task brief\n", plan.id));
    out.push_str(&format!("Board task: {}\n\n", plan.board_task_id));
    let execution_id = workstation_execution_id(plan);
    out.push_str(&format!("Execution log: `{}`\n\n", execution_id));

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
    if let Some(scope) = hints
        .scope
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
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
    out.push_str("- do not stage or commit outside the owned files declared above\n");
    out.push_str("- code tasks: produce a single scoped commit naming the owned files\n");
    // Hidden worktree mutations have repeatedly clobbered other workers' in-flight
    // changes when a delegated worker tried to "reset" before staging. The brief MUST
    // forbid them on a single visible line unless the task contract explicitly owns
    // the operation, so a worker that hits a dirty worktree stops and asks rather
    // than silently rewinding shared state.
    out.push_str(
        "- forbidden git state mutations: do NOT run `git stash`, `git reset`, `git checkout`, or `git restore` unless the task contract explicitly owns that operation — if the worktree looks dirty, stop and add a BoardTask note instead of mutating it\n",
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
    out.push_str(&format!(
        "- on completion call `mission_execution(action=complete, execution_id=\"{}\")` with `enforce_scoped_commit=true`\n",
        execution_id
    ));
    out.push_str(
        "- the dispatcher pre-opened this MissionD audit log; completion may append to it even for read-only briefs\n",
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

    // 7.5 wave-23 / task 05 — Session trace. Optional pointer at the
    //     wave-23 / task 04 ledger. Surfaced before the parallelism hint
    //     so the worker sees the bookkeeping target close to the
    //     completion-handoff section that pins how it should report back.
    //     When absent, this section is omitted entirely so legacy briefs
    //     stay byte-identical.
    if let Some(stp) = session_trace_path
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        out.push_str("## Session trace\n");
        out.push_str(&format!("- ledger path: `{}`\n", stp));
        out.push_str(
            "- forward this path verbatim as `session_trace_path` when calling \
             `mission_execution(action=open|preflight_commit|complete)`\n",
        );
        out.push_str(
            "- the daemon (wave-23 / task 04) appends a `(trace-event ...)` form per call \
             — best-effort, never blocks the primary action result\n",
        );
        out.push('\n');
    }

    // 8. Agent-team hint (exactly once, literal Chinese).
    if dispatch_strategy == "agent-team" {
        out.push_str("## Parallelism hint\n");
        out.push_str(AGENT_TEAM_OBJECTIVE_HINT);
        out.push('\n');
    }

    out
}
