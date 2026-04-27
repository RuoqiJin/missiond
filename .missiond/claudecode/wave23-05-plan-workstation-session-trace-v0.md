# wave23-05-plan-workstation-session-trace-v0 — Plan/workstation session-trace propagation v0

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave23/wave23-05-plan-workstation-session-trace-v0.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `agent-team`
- depends_on: `wave23-04-execution-session-trace-integration-v0`
- shared_memory: `.missiond/tasks/wave23/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave23/reports/wave23-05-plan-workstation-session-trace-v0.report.lisp`

## Dispatch Note

使用 agent-team提高效率

## Goal

Propagate session_trace_path through mission_plan and workstation_dispatch so generated task contracts, dispatch descriptors, and completion paths share one factual trace ledger.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Use agent-team if useful: 使用 agent-team提高效率.
2. Add optional session_trace_path forwarding in mission_plan execute paths.
3. When emitting task contracts, include trace path metadata or response fields so downstream completion can append to the same trace.
4. workstation_dispatch should include session_trace_path in descriptors/briefs when provided.
5. Do not make trace required; preserve legacy behavior when absent.
6. Malformed trace path should fail before dispatch when caller explicitly requires trace, otherwise return warning.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon handlers::knowledge::workstation_dispatch::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-mcp/src/tools/knowledge/plan.rs
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

Expected machine-readable report: `.missiond/tasks/wave23/reports/wave23-05-plan-workstation-session-trace-v0.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave23/reports/wave23-05-plan-workstation-session-trace-v0.report.lisp
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
git add "crates/missiond-daemon/src/handlers/knowledge/plan.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs" \
        "crates/missiond-mcp/src/tools/knowledge/plan.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave23/wave23-05-plan-workstation-session-trace-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave23/wave23-05-plan-workstation-session-trace-v0.lisp \
  git commit -m "feat(plan): propagate session trace through dispatch"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `node scripts/install-missiond-hooks.mjs --install`, equivalent to `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave23/wave23-05-plan-workstation-session-trace-v0.lisp
```

## Report

- `Commit hash.`
- `Forwarded arguments/fields.`
- `Trace-required behavior.`
- `Compatibility notes.`
- `Acceptance command results.`

