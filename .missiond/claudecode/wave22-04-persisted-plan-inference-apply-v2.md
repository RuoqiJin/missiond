# wave22-04-persisted-plan-inference-apply-v2 — Persisted PLAN inference apply v2

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave22/wave22-04-persisted-plan-inference-apply-v2.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave21-05-plan-inference-apply-gate-v1`
- shared_memory: `.missiond/tasks/wave22/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave22/reports/wave22-04-persisted-plan-inference-apply-v2.report.lisp`

## Goal

Allow inferred PLAN fields to persist into plan.sexp_text only through an explicit versioned apply path with preview, audit sidecar, and rollback metadata.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Add explicit persist_inference=true gate; default remains preview/apply-in-memory only.
2. Require apply_inferred_fields=true, caller_approved=true, and a matching proposal_hash.
3. Before mutation, return or record original_sexp_hash and resulting_sexp_hash.
4. Persist only by creating a new plan version or existing project pattern equivalent; do not overwrite without version/audit.
5. Append typed evidence entry with applied_fields, skipped_fields, proposal_hash, original hash, resulting hash, and rollback pointer.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs crates/missiond-mcp/src/tools/knowledge/plan.rs
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

Expected machine-readable report: `.missiond/tasks/wave22/reports/wave22-04-persisted-plan-inference-apply-v2.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave22/reports/wave22-04-persisted-plan-inference-apply-v2.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/plan.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs" \
        "crates/missiond-mcp/src/tools/knowledge/plan.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave22/wave22-04-persisted-plan-inference-apply-v2.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave22/wave22-04-persisted-plan-inference-apply-v2.lisp \
  git commit -m "feat(plan): persist inferred PLAN fields with audit"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave22/wave22-04-persisted-plan-inference-apply-v2.lisp
```

## Report

- `Commit hash.`
- `Persist gate fields.`
- `Version/audit behavior.`
- `Rollback metadata.`
- `Acceptance command results.`

