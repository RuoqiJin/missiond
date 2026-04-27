# wave21-08-machine-contract-autonomous-loop-smoke-v3 — Machine contract autonomous loop smoke v3

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave21/wave21-08-machine-contract-autonomous-loop-smoke-v3.lisp`

## Machine Contract

- kind: `smoke`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave21-03-execution-report-verifier-integration-v1`, `wave21-04-autonomous-workstation-llm-proposal-v0`, `wave21-05-plan-inference-apply-gate-v1`
- shared_memory: `.missiond/tasks/wave21/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave21/reports/wave21-08-machine-contract-autonomous-loop-smoke-v3.report.lisp`

## Goal

Add an end-to-end deterministic smoke that exercises task.lisp dispatch, scoped preflight, report contract, shared-memory completion, and run verifier without relying on Markdown.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-daemon/src/handlers/knowledge/review_gate.rs`
- `crates/missiond-core/src/event/events/execution.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Use fixture task.lisp/report/shared-memory data; no real LLM, no real spawn, no mutating git.
2. Assert Markdown path is absent or metadata-only.
3. Assert the run verifier fields would pass with the fixture commit metadata.
4. Assert malformed task/report/memory data produce structured failures.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests
cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests
cargo test -p missiond-daemon
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs
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

Expected machine-readable report: `.missiond/tasks/wave21/reports/wave21-08-machine-contract-autonomous-loop-smoke-v3.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave21/reports/wave21-08-machine-contract-autonomous-loop-smoke-v3.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/plan.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave21/wave21-08-machine-contract-autonomous-loop-smoke-v3.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave21/wave21-08-machine-contract-autonomous-loop-smoke-v3.lisp \
  git commit -m "test(intent): cover autonomous machine-contract loop"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave21/wave21-08-machine-contract-autonomous-loop-smoke-v3.lisp
```

## Report

- `Commit hash.`
- `Smoke stages covered.`
- `Proof Markdown is non-load-bearing.`
- `Acceptance command results.`

