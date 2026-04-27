# wave23-08-lisp-backfill-wave23-status — Lisp backfill Wave 23 status

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave23/wave23-08-lisp-backfill-wave23-status.lisp`

## Machine Contract

- kind: `lisp-only`
- status: `ready`
- owner: `codex-architect`
- dispatch_strategy: `manual`
- depends_on: `wave23-01-session-trace-schema-v0`, `wave23-02-renderer-report-trace-fields-v1`, `wave23-03-task-run-verifier-trace-v1`, `wave23-04-execution-session-trace-integration-v0`, `wave23-05-plan-workstation-session-trace-v0`, `wave23-06-trace-summary-analyzer-v0`, `wave23-07-router-policy-draft-from-trace-v0`
- shared_memory: `.missiond/tasks/wave23/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave23/reports/wave23-08-lisp-backfill-wave23-status.report.lisp`

## Goal

Codex-owned architecture/status task: backfill MissionD v2 Lisp for Wave 23, marking session-trace collection and trace-derived router policy accurately while keeping router replacement pending.

## Ownership

- `.missiond/v2/intent-machine-contract.lisp`
- `.missiond/v2/intent-pillar-source-index.lisp`
- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-intent-layer.lisp`
- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent-workstation-policy.lisp`
- `.missiond/v2/intent-execution-governance.lisp`
- `.missiond/v2/intent.lisp`

## Must Not Touch

- `crates/**`
- `scripts/**`
- `.missiond/tasks/**`
- `.missiond/claudecode/**`

## Requirements

1. Do not delegate this architecture Lisp task to ClaudeCode; Codex owns the backfill.
2. Backfill only committed Wave23 facts.
3. Mark session-trace schema/checker/integration status separately from future trace analyzer/router replacement.
4. Keep frontend Lisp explicitly postponed.

## Acceptance Commands

```bash
node scripts/check-architecture-lisp.mjs --all-v2
node scripts/check-task-contract.mjs --all
git diff --check -- .missiond/v2/intent-machine-contract.lisp .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent-workstation-policy.lisp .missiond/v2/intent-execution-governance.lisp .missiond/v2/intent.lisp
```

## Shared Memory

Coordination ledger: `.missiond/tasks/wave23/shared-memory.lisp` (schema `missiond.shared-memory.v1`).

- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.
- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.
- `:touched` paths in your entries must stay inside this task `:write-scope`.

Validate with:

```bash
node scripts/check-task-memory.mjs .missiond/tasks/wave23/shared-memory.lisp
```

## Report Contract

Expected machine-readable report: `.missiond/tasks/wave23/reports/wave23-08-lisp-backfill-wave23-status.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave23/reports/wave23-08-lisp-backfill-wave23-status.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Preflight: confirm the repo-local `core.hooksPath` doctor is green so the shared `.githooks/pre-commit` hook also enforces the staged guard. Drift here is a preflight problem, not a hard error — the doctor is read-only; only `--install` mutates git config.

```bash
node scripts/check-missiond-hooks.mjs --json   # read-only doctor; reports preflight-drift on unset/wrong path
node scripts/install-missiond-hooks.mjs --install   # only run when the doctor reports drift; writes --local config only
```

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add ".missiond/v2/intent-machine-contract.lisp" \
        ".missiond/v2/intent-pillar-source-index.lisp" \
        ".missiond/v2/intent-flow.lisp" \
        ".missiond/v2/intent-intent-layer.lisp" \
        ".missiond/v2/intent-tools.lisp" \
        ".missiond/v2/intent-workstation-policy.lisp" \
        ".missiond/v2/intent-execution-governance.lisp" \
        ".missiond/v2/intent.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave23/wave23-08-lisp-backfill-wave23-status.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave23/wave23-08-lisp-backfill-wave23-status.lisp \
  git commit -m "docs(v2): backfill wave23 session-trace status"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `node scripts/install-missiond-hooks.mjs --install`, equivalent to `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave23/wave23-08-lisp-backfill-wave23-status.lisp
```

## Report

- `Commit hash.`
- `Anchors updated.`
- `Trace collection vs router replacement distinction.`
- `Remaining pending list.`
- `Acceptance command results.`

