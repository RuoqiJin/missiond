# wave22-02-execution-auto-run-verifier-v2 — Execution auto task-run verifier v2

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave22/wave22-02-execution-auto-run-verifier-v2.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave21-03-execution-report-verifier-integration-v1`
- shared_memory: `.missiond/tasks/wave22/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave22/reports/wave22-02-execution-auto-run-verifier-v2.report.lisp`

## Goal

Remove the caller-supplied verified=true escape hatch by having mission_execution complete derive verification from task/report/memory/commit inputs when all paths are supplied.

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

1. When task_contract_path, task_report_path, shared_memory_path, and commit_hash are all supplied, daemon should run its in-tree read-only verifier and compute verified status itself.
2. Keep caller-supplied verified=true only as legacy compatibility, and downgrade it to legacy_verified_claim in the response when full inputs are absent.
3. Return verification_source, verifier_status, verifier_diagnostics, and verified_scope_summary.
4. Do not invoke Node, shell scripts, or mutating git commands from daemon.
5. Preserve legacy complete behavior when the new fields are absent and enforce_scoped_commit=false.

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

Coordination ledger: `.missiond/tasks/wave22/shared-memory.lisp` (schema `missiond.shared-memory.v1`).

- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.
- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.
- `:touched` paths in your entries must stay inside this task `:write-scope`.

Validate with:

```bash
node scripts/check-task-memory.mjs .missiond/tasks/wave22/shared-memory.lisp
```

## Report Contract

Expected machine-readable report: `.missiond/tasks/wave22/reports/wave22-02-execution-auto-run-verifier-v2.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave22/reports/wave22-02-execution-auto-run-verifier-v2.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs" \
        "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave22/wave22-02-execution-auto-run-verifier-v2.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave22/wave22-02-execution-auto-run-verifier-v2.lisp \
  git commit -m "feat(execution): auto-verify task run completion"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave22/wave22-02-execution-auto-run-verifier-v2.lisp
```

## Report

- `Commit hash.`
- `Verifier source states.`
- `Legacy verified=true behavior.`
- `Read-only proof.`
- `Acceptance command results.`

