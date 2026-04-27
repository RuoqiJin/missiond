# wave22-07-autonomous-loop-apply-smoke-v4 — Autonomous loop apply smoke v4

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave22/wave22-07-autonomous-loop-apply-smoke-v4.lisp`

## Machine Contract

- kind: `smoke`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave22-02-execution-auto-run-verifier-v2`, `wave22-03-review-llm-approve-apply-gate-v1`, `wave22-04-persisted-plan-inference-apply-v2`, `wave22-05-autonomous-workstation-true-spawn-v1`
- shared_memory: `.missiond/tasks/wave22/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave22/reports/wave22-07-autonomous-loop-apply-smoke-v4.report.lisp`

## Goal

Add a deterministic smoke test that exercises the explicit apply gates for review, plan inference, workstation spawn, and execution verification without real LLM/spawn/git mutation.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/review_gate.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-core/src/event/events/execution.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Use fixture proposal hashes and fixture task/report/memory data; no real LLM, no real spawn, no mutating git.
2. Assert each apply gate rejects missing proposal_hash and accepts the valid fixture path.
3. Assert Markdown remains non-load-bearing.
4. Assert failed verification blocks completion when enforce_scoped_commit=true.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests
cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests
cargo test -p missiond-daemon
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs crates/missiond-daemon/src/handlers/knowledge/review_gate.rs
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

Expected machine-readable report: `.missiond/tasks/wave22/reports/wave22-07-autonomous-loop-apply-smoke-v4.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave22/reports/wave22-07-autonomous-loop-apply-smoke-v4.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/plan.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave22/wave22-07-autonomous-loop-apply-smoke-v4.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave22/wave22-07-autonomous-loop-apply-smoke-v4.lisp \
  git commit -m "test(intent): cover autonomous apply gates"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave22/wave22-07-autonomous-loop-apply-smoke-v4.lisp
```

## Report

- `Commit hash.`
- `Apply gates covered.`
- `No real side-effect proof.`
- `Acceptance command results.`

