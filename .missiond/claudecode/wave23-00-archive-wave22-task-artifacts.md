# wave23-00-archive-wave22-task-artifacts — Archive Wave 22 task artifacts

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave23/wave23-00-archive-wave22-task-artifacts.lisp`

## Machine Contract

- kind: `docs`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- shared_memory: `.missiond/tasks/wave23/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave23/reports/wave23-00-archive-wave22-task-artifacts.report.lisp`

## Goal

Commit the Wave 22 task contracts, rendered briefs, reports, and shared-memory ledger left untracked after Wave 22.

## Ownership

- `.missiond/tasks/wave22/**`
- `.missiond/claudecode/wave22-*.md`

## Must Not Touch

- `crates/**`
- `scripts/**`
- `.missiond/v2/*.lisp`
- `.missiond/tasks/wave23/**`

## Requirements

1. Stage only Wave 22 artifacts.
2. Do not stage Wave 23 task contracts, trace, shared memory, or briefs.
3. Do not edit Wave 22 files unless git diff --check reports whitespace problems.
4. Before committing, run git diff --cached --name-only and confirm every path is inside this task :write-scope.

## Acceptance Commands

```bash
node scripts/check-task-contract.mjs --all
node scripts/check-task-memory.mjs .missiond/tasks/wave22/shared-memory.lisp
git diff --check -- .missiond/tasks/wave22 .missiond/claudecode/wave22-*.md
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

Expected machine-readable report: `.missiond/tasks/wave23/reports/wave23-00-archive-wave22-task-artifacts.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave23/reports/wave23-00-archive-wave22-task-artifacts.report.lisp
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
git add ".missiond/tasks/wave22/**" \
        ".missiond/claudecode/wave22-*.md"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave23/wave23-00-archive-wave22-task-artifacts.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave23/wave23-00-archive-wave22-task-artifacts.lisp \
  git commit -m "chore(wave22): archive task artifacts"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `node scripts/install-missiond-hooks.mjs --install`, equivalent to `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave23/wave23-00-archive-wave22-task-artifacts.lisp
```

## Report

- `Commit hash.`
- `Number of Wave 22 task contracts archived.`
- `Number of rendered briefs archived.`
- `Number of reports archived.`
- `Shared-memory entry count.`

