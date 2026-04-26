# wave20-00-archive-wave19-task-contracts — Archive Wave 19 task contracts and rendered briefs

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave20/wave20-00-archive-wave19-task-contracts.lisp`

## Machine Contract

- kind: `docs`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- shared_memory: `.missiond/tasks/wave20/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave20/reports/wave20-00-archive-wave19-task-contracts.report.lisp`

## Goal

Commit the Wave 19 Lisp task contracts and rendered ClaudeCode briefs that remained untracked after Wave 19.

## Ownership

- `.missiond/tasks/wave19/wave19-*.lisp`
- `.missiond/claudecode/wave19-*.md`

## Must Not Touch

- `crates/**`
- `scripts/**`
- `.missiond/v2/*.lisp`
- `.missiond/tasks/wave20/**`

## Requirements

1. Stage only Wave 19 task contracts and rendered Wave 19 briefs.
2. Do not stage Wave 20 task contracts.
3. Do not edit Wave 19 files unless git diff --check reports whitespace problems.
4. Before committing, inspect git diff --cached --name-only and confirm every staged path matches this task :write-scope.

## Acceptance Commands

```bash
node scripts/check-task-contract.mjs --all
git diff --check -- .missiond/tasks/wave19 .missiond/claudecode/wave19-*.md
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

Expected machine-readable report: `.missiond/tasks/wave20/reports/wave20-00-archive-wave19-task-contracts.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave20/reports/wave20-00-archive-wave19-task-contracts.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add ".missiond/tasks/wave19/wave19-*.lisp" \
        ".missiond/claudecode/wave19-*.md"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave20/wave20-00-archive-wave19-task-contracts.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave20/wave20-00-archive-wave19-task-contracts.lisp \
  git commit -m "chore(wave19): archive task contracts"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave20/wave20-00-archive-wave19-task-contracts.lisp
```

## Report

- `Commit hash.`
- `Number of Lisp contracts archived.`
- `Number of Markdown briefs archived.`
- `Staged-file scope check result.`

