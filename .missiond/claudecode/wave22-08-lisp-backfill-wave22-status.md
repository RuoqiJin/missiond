# wave22-08-lisp-backfill-wave22-status — Lisp backfill Wave 22 status

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave22/wave22-08-lisp-backfill-wave22-status.lisp`

## Machine Contract

- kind: `lisp-only`
- status: `ready`
- owner: `resident-lisp-architect`
- dispatch_strategy: `resident-lisp`
- depends_on: `wave22-01-hooks-default-on-doctor-v2`, `wave22-02-execution-auto-run-verifier-v2`, `wave22-03-review-llm-approve-apply-gate-v1`, `wave22-04-persisted-plan-inference-apply-v2`, `wave22-05-autonomous-workstation-true-spawn-v1`, `wave22-06-distill-chain-policy-auto-sonnet-v2`, `wave22-07-autonomous-loop-apply-smoke-v4`
- shared_memory: `.missiond/tasks/wave22/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave22/reports/wave22-08-lisp-backfill-wave22-status.report.lisp`

## Goal

Backfill MissionD v2 Lisp for Wave 22, marking explicit apply gates, auto verification, autonomous spawn gate, and remaining pending boundaries accurately.

## Ownership

- `.missiond/v2/intent-machine-contract.lisp`
- `.missiond/v2/intent-pillar-source-index.lisp`
- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-intent-layer.lisp`
- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent-plan-dag.lisp`
- `.missiond/v2/intent-workstation-policy.lisp`
- `.missiond/v2/intent-execution-governance.lisp`
- `.missiond/v2/intent.lisp`

## Must Not Touch

- `crates/**`
- `scripts/**`
- `.missiond/tasks/**`
- `.missiond/claudecode/**`

## Requirements

1. Use the resident Lisp architect session if available.
2. Backfill only committed facts; any skipped/no-op task must stay marked pending or no-op.
3. Distinguish policy-gated automatic action from unconditional autonomy.
4. Preserve source-index and shard checker invariants.
5. Keep frontend Lisp explicitly postponed.

## Acceptance Commands

```bash
node scripts/check-architecture-lisp.mjs --all-v2
node scripts/check-task-contract.mjs --all
git diff --check -- .missiond/v2/intent-machine-contract.lisp .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent-plan-dag.lisp .missiond/v2/intent-workstation-policy.lisp .missiond/v2/intent-execution-governance.lisp .missiond/v2/intent.lisp
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

Expected machine-readable report: `.missiond/tasks/wave22/reports/wave22-08-lisp-backfill-wave22-status.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave22/reports/wave22-08-lisp-backfill-wave22-status.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add ".missiond/v2/intent-machine-contract.lisp" \
        ".missiond/v2/intent-pillar-source-index.lisp" \
        ".missiond/v2/intent-flow.lisp" \
        ".missiond/v2/intent-intent-layer.lisp" \
        ".missiond/v2/intent-tools.lisp" \
        ".missiond/v2/intent-plan-dag.lisp" \
        ".missiond/v2/intent-workstation-policy.lisp" \
        ".missiond/v2/intent-execution-governance.lisp" \
        ".missiond/v2/intent.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave22/wave22-08-lisp-backfill-wave22-status.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave22/wave22-08-lisp-backfill-wave22-status.lisp \
  git commit -m "docs(v2): backfill wave22 apply-gate status"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave22/wave22-08-lisp-backfill-wave22-status.lisp
```

## Report

- `Commit hash.`
- `Anchors updated.`
- `Automatic vs policy-gated distinction.`
- `Remaining pending list.`
- `Acceptance command results.`

