# wave20-02-renderer-scoped-commit-guard-v2 — Renderer scoped commit guard v2

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave20/wave20-02-renderer-scoped-commit-guard-v2.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave20-01-task-scope-index-guard-v1`
- shared_memory: `.missiond/tasks/wave20/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave20/reports/wave20-02-renderer-scoped-commit-guard-v2.report.lisp`

## Goal

Make every rendered ClaudeCode brief show the scoped staging and pre-commit guard sequence so agents do not accidentally commit someone else's staged files.

## Ownership

- `scripts/render-claudecode-task.mjs`
- `.missiond/tasks/schema/task-contract-v1.lisp`
- `.missiond/tasks/wave20/wave20-00-archive-wave19-task-contracts.lisp`
- `.missiond/claudecode/wave20-00-archive-wave19-task-contracts.md`

## Must Not Touch

- `crates/**`
- `.missiond/v2/*.lisp`
- `scripts/task-scope-guard.mjs`
- `.githooks/pre-commit`

## Requirements

1. Render a pre-commit scoped-index check before git commit whenever :commit :required true.
2. Render MISSIOND_TASK_CONTRACT=<task.lisp> as the hook activation environment.
3. Keep the existing verify-task-contract post-commit command.
4. Re-render wave20-00 as the golden example.

## Acceptance Commands

```bash
node scripts/check-task-contract.mjs --all
node scripts/render-claudecode-task.mjs --force .missiond/tasks/wave20/wave20-00-archive-wave19-task-contracts.lisp
git diff --check -- scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp .missiond/tasks/wave20/wave20-00-archive-wave19-task-contracts.lisp .missiond/claudecode/wave20-00-archive-wave19-task-contracts.md
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

Expected machine-readable report: `.missiond/tasks/wave20/reports/wave20-02-renderer-scoped-commit-guard-v2.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave20/reports/wave20-02-renderer-scoped-commit-guard-v2.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "scripts/render-claudecode-task.mjs" \
        ".missiond/tasks/schema/task-contract-v1.lisp" \
        ".missiond/tasks/wave20/wave20-00-archive-wave19-task-contracts.lisp" \
        ".missiond/claudecode/wave20-00-archive-wave19-task-contracts.md"
git commit -m "feat(tasks): render scoped commit guard steps"
```

Scope check: `write-scope-only`.

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave20/wave20-02-renderer-scoped-commit-guard-v2.lisp
```

## Report

- `Commit hash.`
- `Rendered commit-section changes.`
- `Golden brief path.`
- `Acceptance command results.`

