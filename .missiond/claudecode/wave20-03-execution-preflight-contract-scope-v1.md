# wave20-03-execution-preflight-contract-scope-v1 — Execution preflight task-contract scope v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave20/wave20-03-execution-preflight-contract-scope-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave20-01-task-scope-index-guard-v1`
- shared_memory: `.missiond/tasks/wave20/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave20/reports/wave20-03-execution-preflight-contract-scope-v1.report.lisp`

## Goal

Teach mission_execution preflight_commit to use task_contract_path as the scope source, so daemon-side read-only preflight catches index pollution before completion.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-mcp/src/tools/knowledge/agent_execution.rs`

## Must Not Touch

- `crates/missiond-core/src/event/events/execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `scripts/**`
- `.missiond/v2/*.lisp`

## Requirements

1. Add optional task_contract_path handling to action=preflight_commit.
2. Read only task-contract v1 fields needed for scope: :write-scope, :must-not-touch, :commit.
3. Compare git status / staged files against contract scope; do not run mutating git commands.
4. Return structured fields: task_contract_status, staged_out_of_scope, staged_forbidden, unstaged_in_scope, next_step.
5. Preserve legacy preflight behavior when task_contract_path is absent.

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

Coordination ledger: `.missiond/tasks/wave20/shared-memory.lisp` (schema `missiond.shared-memory.v1`).

- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.
- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.
- `:touched` paths in your entries must stay inside this task `:write-scope`.

Validate with:

```bash
node scripts/check-task-memory.mjs .missiond/tasks/wave20/shared-memory.lisp
```

## Report Contract

Expected machine-readable report: `.missiond/tasks/wave20/reports/wave20-03-execution-preflight-contract-scope-v1.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave20/reports/wave20-03-execution-preflight-contract-scope-v1.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs" \
        "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
git commit -m "feat(execution): preflight task contract scope"
```

Scope check: `write-scope-only`.

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave20/wave20-03-execution-preflight-contract-scope-v1.lisp
```

## Report

- `Commit hash.`
- `New preflight fields.`
- `Read-only git command proof.`
- `Legacy compatibility notes.`
- `Acceptance command results.`

