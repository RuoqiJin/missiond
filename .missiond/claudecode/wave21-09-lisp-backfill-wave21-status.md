# wave21-09-lisp-backfill-wave21-status — Lisp backfill Wave 21 status

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave21/wave21-09-lisp-backfill-wave21-status.lisp`

## Machine Contract

- kind: `lisp-only`
- status: `ready`
- owner: `resident-lisp-architect`
- dispatch_strategy: `resident-lisp`
- depends_on: `wave21-01-hooks-path-installer-v1`, `wave21-02-run-verifier-v1`, `wave21-03-execution-report-verifier-integration-v1`, `wave21-04-autonomous-workstation-llm-proposal-v0`, `wave21-05-plan-inference-apply-gate-v1`, `wave21-06-llm-auto-approve-proposal-v0`, `wave21-07-sonnet-distill-chain-auto-apply-v1`, `wave21-08-machine-contract-autonomous-loop-smoke-v3`
- shared_memory: `.missiond/tasks/wave21/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave21/reports/wave21-09-lisp-backfill-wave21-status.report.lisp`

## Goal

Backfill MissionD v2 Lisp for Wave 21, marking default-on hook guardrails, task-run verification, and LLM proposal/apply gates accurately.

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
2. Backfill only committed facts; proposal-only tasks must not be marked as automatic execution.
3. Preserve all source-index and shard checker invariants.
4. Keep frontend Lisp explicitly postponed unless the user starts a frontend wave.

## Acceptance Commands

```bash
node scripts/check-architecture-lisp.mjs --all-v2
node scripts/check-task-contract.mjs --all
git diff --check -- .missiond/v2/intent-machine-contract.lisp .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent-plan-dag.lisp .missiond/v2/intent-workstation-policy.lisp .missiond/v2/intent-execution-governance.lisp .missiond/v2/intent.lisp
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

Expected machine-readable report: `.missiond/tasks/wave21/reports/wave21-09-lisp-backfill-wave21-status.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave21/reports/wave21-09-lisp-backfill-wave21-status.report.lisp
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
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave21/wave21-09-lisp-backfill-wave21-status.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave21/wave21-09-lisp-backfill-wave21-status.lisp \
  git commit -m "docs(v2): backfill wave21 autonomous-loop status"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave21/wave21-09-lisp-backfill-wave21-status.lisp
```

## Report

- `Commit hash.`
- `Anchors updated.`
- `Proposal-only vs applied status distinctions.`
- `Remaining pending list.`
- `Acceptance command results.`

