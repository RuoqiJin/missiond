# wave23-04-execution-session-trace-integration-v0 — Execution session-trace integration v0

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave23/wave23-04-execution-session-trace-integration-v0.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `agent-team`
- depends_on: `wave23-01-session-trace-schema-v0`
- shared_memory: `.missiond/tasks/wave23/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave23/reports/wave23-04-execution-session-trace-integration-v0.report.lisp`

## Dispatch Note

使用 agent-team提高效率

## Goal

Add opt-in session-trace append support to mission_execution so dispatch/complete/preflight facts can be recorded by MissionD without relying on worker prose.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-mcp/src/tools/knowledge/agent_execution.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `scripts/**`
- `.missiond/v2/*.lisp`

## Requirements

1. Use agent-team if useful: 使用 agent-team提高效率.
2. Add optional session_trace_path argument to relevant mission_execution actions.
3. Append trace events for open/preflight_commit/complete when session_trace_path is supplied.
4. Trace append must be append-only, structured, and best-effort: report trace_warning on failure but do not hide core action result.
5. Do not spawn Node or shell scripts from daemon.
6. Preserve legacy behavior when session_trace_path is absent.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs crates/missiond-mcp/src/tools/knowledge/agent_execution.rs
```

## Shared Memory

Coordination ledger: `.missiond/tasks/wave23/shared-memory.lisp` (schema `missiond.shared-memory.v1`).

- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.
- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.
- `:touched` paths in your entries must stay inside this task `:write-scope`.

Validate with:

```bash
node scripts/check-task-memory.mjs .missiond/tasks/wave23/shared-memory.lisp
```

## Report Contract

Expected machine-readable report: `.missiond/tasks/wave23/reports/wave23-04-execution-session-trace-integration-v0.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave23/reports/wave23-04-execution-session-trace-integration-v0.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Preflight: confirm the repo-local `core.hooksPath` doctor is green so the shared `.githooks/pre-commit` hook also enforces the staged guard. Drift here is a preflight problem, not a hard error — the doctor is read-only; only `--install` mutates git config.

```bash
node scripts/check-missiond-hooks.mjs --json   # read-only doctor; reports preflight-drift on unset/wrong path
node scripts/install-missiond-hooks.mjs --install   # only run when the doctor reports drift; writes --local config only
```

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs" \
        "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave23/wave23-04-execution-session-trace-integration-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave23/wave23-04-execution-session-trace-integration-v0.lisp \
  git commit -m "feat(execution): append session trace events"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `node scripts/install-missiond-hooks.mjs --install`, equivalent to `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave23/wave23-04-execution-session-trace-integration-v0.lisp
```

## Report

- `Commit hash.`
- `Actions that append trace.`
- `Failure semantics.`
- `Compatibility notes.`
- `Acceptance command results.`

