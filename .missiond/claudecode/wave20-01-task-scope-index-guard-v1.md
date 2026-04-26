# wave20-01-task-scope-index-guard-v1 — Task scope index guard v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave20/wave20-01-task-scope-index-guard-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `agent-team`
- depends_on: `wave20-00-archive-wave19-task-contracts`
- shared_memory: `.missiond/tasks/wave20/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave20/reports/wave20-01-task-scope-index-guard-v1.report.lisp`

## Dispatch Note

使用 agent-team提高效率

## Goal

Prevent the Wave 19 git-index pollution failure by adding a task-contract aware guard for staged files and an optional pre-commit hook entry point.

## Ownership

- `scripts/task-scope-guard.mjs`
- `.githooks/pre-commit`
- `.missiond/tasks/schema/task-contract-v1.lisp`

## Must Not Touch

- `crates/**`
- `.missiond/v2/*.lisp`
- `.missiond/tasks/wave19/**`
- `.missiond/claudecode/wave19-*.md`

## Requirements

1. Use agent-team if useful: 使用 agent-team提高效率.
2. Add scripts/task-scope-guard.mjs with --task <task.lisp>, --mode staged|commit, --json, and --dry-fixture.
3. In staged mode, read git diff --cached --name-only and fail if any staged path is outside :write-scope or inside :must-not-touch.
4. In commit mode, accept --commit <hash> and delegate or share logic with verify-task-contract where practical.
5. Add .githooks/pre-commit that runs the guard only when MISSIOND_TASK_CONTRACT is set; without that env var it should exit 0.
6. The guard must be read-only: no git add, commit, reset, checkout, stash, push, merge, or rebase.

## Acceptance Commands

```bash
node scripts/task-scope-guard.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- scripts/task-scope-guard.mjs .githooks/pre-commit .missiond/tasks/schema/task-contract-v1.lisp
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

Expected machine-readable report: `.missiond/tasks/wave20/reports/wave20-01-task-scope-index-guard-v1.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave20/reports/wave20-01-task-scope-index-guard-v1.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "scripts/task-scope-guard.mjs" \
        ".githooks/pre-commit" \
        ".missiond/tasks/schema/task-contract-v1.lisp"
git commit -m "feat(tasks): guard staged files by task scope"
```

Scope check: `write-scope-only`.

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave20/wave20-01-task-scope-index-guard-v1.lisp
```

## Report

- `Commit hash.`
- `Guard CLI synopsis.`
- `Hook activation rule.`
- `Dry-fixture coverage.`
- `Acceptance command results.`

