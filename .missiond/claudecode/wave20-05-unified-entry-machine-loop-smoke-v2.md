# wave20-05-unified-entry-machine-loop-smoke-v2 — Unified entry machine loop smoke v2

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave20/wave20-05-unified-entry-machine-loop-smoke-v2.lisp`

## Machine Contract

- kind: `smoke`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave20-04-machine-driven-dispatch-v0`
- shared_memory: `.missiond/tasks/wave20/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave20/reports/wave20-05-unified-entry-machine-loop-smoke-v2.report.lisp`

## Goal

Add a deterministic smoke test proving unified_entry can drive directive/plan/task-contract/workstation handoff without relying on Markdown parsing.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Create a no-LLM, no-spawn smoke path that uses fixture task.lisp data and asserts machine contract fields survive every handoff.
2. Assert Markdown/rendered brief is not required in the machine-mode smoke.
3. Assert malformed task contract returns a structured error rather than prompt fallback.
4. Keep the test deterministic and local.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests
cargo test -p missiond-daemon
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs
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

Expected machine-readable report: `.missiond/tasks/wave20/reports/wave20-05-unified-entry-machine-loop-smoke-v2.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave20/reports/wave20-05-unified-entry-machine-loop-smoke-v2.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/plan.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
git commit -m "test(intent): cover machine contract dispatch loop"
```

Scope check: `write-scope-only`.

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave20/wave20-05-unified-entry-machine-loop-smoke-v2.lisp
```

## Report

- `Commit hash.`
- `Smoke stages covered.`
- `Proof Markdown is non-load-bearing.`
- `Acceptance command results.`

