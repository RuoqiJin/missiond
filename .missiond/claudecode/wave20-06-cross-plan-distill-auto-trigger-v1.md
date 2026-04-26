# wave20-06-cross-plan-distill-auto-trigger-v1 — Cross-plan distill auto-trigger v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave20/wave20-06-cross-plan-distill-auto-trigger-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- shared_memory: `.missiond/tasks/wave20/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave20/reports/wave20-06-cross-plan-distill-auto-trigger-v1.report.lisp`

## Goal

Trigger cross-plan distill chain recording automatically when deterministic safety rules pass, while keeping Sonnet calls explicit.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-mcp/src/tools/knowledge/workflow.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Add opt-in auto_trigger mode for cross-plan distill chain sidecar recording.
2. Use deterministic safety rules only; do not call Sonnet unless the existing explicit sonnet mode is requested.
3. Return trigger_status, chain_id, safety_rule_results, and sidecar path.
4. If any safety rule fails, return skipped with rule evidence; do not partially append.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::workflow::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/workflow.rs crates/missiond-mcp/src/tools/knowledge/workflow.rs
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

Expected machine-readable report: `.missiond/tasks/wave20/reports/wave20-06-cross-plan-distill-auto-trigger-v1.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave20/reports/wave20-06-cross-plan-distill-auto-trigger-v1.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/workflow.rs" \
        "crates/missiond-mcp/src/tools/knowledge/workflow.rs"
git commit -m "feat(workflow): auto-trigger safe distill chains"
```

Scope check: `write-scope-only`.

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave20/wave20-06-cross-plan-distill-auto-trigger-v1.lisp
```

## Report

- `Commit hash.`
- `Safety rules.`
- `Response fields.`
- `Sonnet non-implicit proof.`
- `Acceptance command results.`

