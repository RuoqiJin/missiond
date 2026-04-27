# wave21-03-execution-report-verifier-integration-v1 — Execution report verifier integration v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave21/wave21-03-execution-report-verifier-integration-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave21-02-run-verifier-v1`
- shared_memory: `.missiond/tasks/wave21/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave21/reports/wave21-03-execution-report-verifier-integration-v1.report.lisp`

## Goal

Expose task-run verification status through mission_execution complete/preflight metadata without making the daemon perform mutating git operations.

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

1. Add optional fields for task_run_verifier_status, task_report_path, shared_memory_path, and verifier_diagnostics where appropriate.
2. If enforce_scoped_commit=true and task_contract_path is present, allow caller to supply verified=true only when task_report_path and commit_hash are also present.
3. Daemon may perform read-only file parsing if existing helpers are local; otherwise record caller-supplied verifier status and fail fast on missing critical fields.
4. Preserve legacy complete/preflight behavior when new fields are absent.

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

Coordination ledger: `.missiond/tasks/wave21/shared-memory.lisp` (schema `missiond.shared-memory.v1`).

- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.
- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.
- `:touched` paths in your entries must stay inside this task `:write-scope`.

Validate with:

```bash
node scripts/check-task-memory.mjs .missiond/tasks/wave21/shared-memory.lisp
```

## Report Contract

Expected machine-readable report: `.missiond/tasks/wave21/reports/wave21-03-execution-report-verifier-integration-v1.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave21/reports/wave21-03-execution-report-verifier-integration-v1.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs" \
        "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave21/wave21-03-execution-report-verifier-integration-v1.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave21/wave21-03-execution-report-verifier-integration-v1.lisp \
  git commit -m "feat(execution): record task run verification status"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave21/wave21-03-execution-report-verifier-integration-v1.lisp
```

## Report

- `Commit hash.`
- `New fields.`
- `Enforcement conditions.`
- `Legacy compatibility notes.`
- `Acceptance command results.`

