# wave20-09-execution-event-legacy-metadata-sweep — ExecutionEvent legacy metadata sweep

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave20/wave20-09-execution-event-legacy-metadata-sweep.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- shared_memory: `.missiond/tasks/wave20/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave20/reports/wave20-09-execution-event-legacy-metadata-sweep.report.lisp`

## Goal

Audit and fill any remaining older ExecutionEvent variants that still lack optional dispatch metadata after Wave 19, without breaking serde backward compatibility.

## Ownership

- `crates/missiond-core/src/event/events/execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Inventory ExecutionEvent variants and identify which already carry dispatch_strategy/target_project/requested_cwd.
2. For any remaining lifecycle variant that should carry the triplet, add optional serde-default fields with skip_serializing_if.
3. Add old-JSON and new-JSON round-trip tests.
4. If no variant remains, make a no-op commit only if a test/doc assertion is added inside the write scope; otherwise report NO-OP.

## Acceptance Commands

```bash
cargo test -p missiond-core --lib
cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests
cargo test -p missiond-daemon
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-core/src/event/events/execution.rs crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs
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

Expected machine-readable report: `.missiond/tasks/wave20/reports/wave20-09-execution-event-legacy-metadata-sweep.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave20/reports/wave20-09-execution-event-legacy-metadata-sweep.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "crates/missiond-core/src/event/events/execution.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
git commit -m "feat(execution): complete dispatch metadata event coverage"
```

Scope check: `write-scope-only`.

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave20/wave20-09-execution-event-legacy-metadata-sweep.lisp
```

## Report

- `Commit hash or NO-OP reason.`
- `Variant inventory.`
- `Serde compatibility proof.`
- `Acceptance command results.`

