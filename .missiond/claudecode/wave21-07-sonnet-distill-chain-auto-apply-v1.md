# wave21-07-sonnet-distill-chain-auto-apply-v1 — Sonnet distill chain auto-apply v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave21/wave21-07-sonnet-distill-chain-auto-apply-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave20-06-cross-plan-distill-auto-trigger-v1`
- shared_memory: `.missiond/tasks/wave21/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave21/reports/wave21-07-sonnet-distill-chain-auto-apply-v1.report.lisp`

## Goal

Allow cross-plan distill chains to invoke Sonnet automatically only when all deterministic safety rules pass and the caller explicitly enables auto_sonnet.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/workflow.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Add explicit auto_sonnet=true gate; default keeps Sonnet explicit/manual.
2. Invoke Sonnet only after all deterministic safety rules pass.
3. If any rule fails, return skipped and do not call Sonnet.
4. If Sonnet returns invalid output, record structured failure and preserve existing plan/workflow state.
5. Return auto_sonnet_status, safety_rule_results, model_call_status, sidecar path, and review_required.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::workflow::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/workflow.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/workflow.rs
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

Expected machine-readable report: `.missiond/tasks/wave21/reports/wave21-07-sonnet-distill-chain-auto-apply-v1.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave21/reports/wave21-07-sonnet-distill-chain-auto-apply-v1.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/workflow.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/plan.rs" \
        "crates/missiond-mcp/src/tools/knowledge/workflow.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave21/wave21-07-sonnet-distill-chain-auto-apply-v1.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave21/wave21-07-sonnet-distill-chain-auto-apply-v1.lisp \
  git commit -m "feat(workflow): gate Sonnet distill chain auto-apply"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave21/wave21-07-sonnet-distill-chain-auto-apply-v1.lisp
```

## Report

- `Commit hash.`
- `auto_sonnet gate.`
- `Safety rules.`
- `State-preservation behavior.`
- `Acceptance command results.`

