# wave21-00-archive-wave20-task-artifacts — Archive Wave 20 task artifacts

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave21/wave21-00-archive-wave20-task-artifacts.lisp`

## Machine Contract

- kind: `docs`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- shared_memory: `.missiond/tasks/wave21/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave21/reports/wave21-00-archive-wave20-task-artifacts.report.lisp`

## Goal

Commit the Wave 20 task contracts, rendered briefs, reports, and shared-memory ledger left untracked after Wave 20.

## Ownership

- `.missiond/tasks/wave20/**`
- `.missiond/claudecode/wave20-*.md`

## Must Not Touch

- `crates/**`
- `scripts/**`
- `.missiond/v2/*.lisp`
- `.missiond/tasks/wave21/**`

## Requirements

1. Stage only Wave 20 artifacts.
2. Do not stage Wave 21 task contracts or briefs.
3. Do not edit Wave 20 files unless git diff --check reports whitespace problems.
4. Before committing, run git diff --cached --name-only and confirm every path is inside this task :write-scope.

## Acceptance Commands

```bash
node scripts/check-task-contract.mjs --all
node scripts/check-task-memory.mjs .missiond/tasks/wave20/shared-memory.lisp
git diff --check -- .missiond/tasks/wave20 .missiond/claudecode/wave20-*.md
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

Expected machine-readable report: `.missiond/tasks/wave21/reports/wave21-00-archive-wave20-task-artifacts.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave21/reports/wave21-00-archive-wave20-task-artifacts.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add ".missiond/tasks/wave20/**" \
        ".missiond/claudecode/wave20-*.md"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave21/wave21-00-archive-wave20-task-artifacts.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave21/wave21-00-archive-wave20-task-artifacts.lisp \
  git commit -m "chore(wave20): archive task artifacts"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave21/wave21-00-archive-wave20-task-artifacts.lisp
```

## Report

- `Commit hash.`
- `Number of task contracts archived.`
- `Number of rendered briefs archived.`
- `Number of reports archived.`
- `Shared-memory entry count.`

