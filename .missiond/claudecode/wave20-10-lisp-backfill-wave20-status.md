# wave20-10-lisp-backfill-wave20-status — Lisp backfill Wave 20 status

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave20/wave20-10-lisp-backfill-wave20-status.lisp`

## Machine Contract

- kind: `lisp-only`
- status: `ready`
- owner: `resident-lisp-architect`
- dispatch_strategy: `resident-lisp`
- depends_on: `wave20-01-task-scope-index-guard-v1`, `wave20-02-renderer-scoped-commit-guard-v2`, `wave20-03-execution-preflight-contract-scope-v1`, `wave20-04-machine-driven-dispatch-v0`, `wave20-05-unified-entry-machine-loop-smoke-v2`, `wave20-06-cross-plan-distill-auto-trigger-v1`, `wave20-07-llm-augmented-plan-inference-v0`, `wave20-08-review-auto-answer-policy-v0`, `wave20-09-execution-event-legacy-metadata-sweep`
- shared_memory: `.missiond/tasks/wave20/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave20/reports/wave20-10-lisp-backfill-wave20-status.report.lisp`

## Goal

Backfill MissionD v2 architecture Lisp after Wave 20, with special focus on machine-contract dispatch, scoped-index guardrails, and remaining autonomous-loop boundaries.

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
2. Backfill only committed facts; mark skipped/no-op tasks honestly.
3. Preserve all source-index and shard checker invariants.
4. Do not compress or split additional shards in this wave unless a checker demands it.
5. Keep frontend Lisp explicitly postponed.

## Acceptance Commands

```bash
node scripts/check-architecture-lisp.mjs --all-v2
node scripts/check-task-contract.mjs --all
git diff --check -- .missiond/v2/intent-machine-contract.lisp .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent-plan-dag.lisp .missiond/v2/intent-workstation-policy.lisp .missiond/v2/intent-execution-governance.lisp .missiond/v2/intent.lisp
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

Expected machine-readable report: `.missiond/tasks/wave20/reports/wave20-10-lisp-backfill-wave20-status.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave20/reports/wave20-10-lisp-backfill-wave20-status.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

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
git commit -m "docs(v2): backfill wave20 machine-dispatch status"
```

Scope check: `write-scope-only`.

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave20/wave20-10-lisp-backfill-wave20-status.lisp
```

## Report

- `Commit hash.`
- `Anchors updated.`
- `Remaining pending list.`
- `Acceptance command results.`

