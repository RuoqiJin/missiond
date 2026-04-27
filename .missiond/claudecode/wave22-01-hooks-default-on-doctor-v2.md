# wave22-01-hooks-default-on-doctor-v2 — Hooks default-on doctor v2

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave22/wave22-01-hooks-default-on-doctor-v2.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave22-00-archive-wave21-task-artifacts`
- shared_memory: `.missiond/tasks/wave22/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave22/reports/wave22-01-hooks-default-on-doctor-v2.report.lisp`

## Goal

Make repo-local hook installation a first-class default preflight expectation for MissionD task workflows, without silently changing global user config.

## Ownership

- `scripts/install-missiond-hooks.mjs`
- `scripts/check-missiond-hooks.mjs`
- `scripts/render-claudecode-task.mjs`
- `.missiond/tasks/schema/task-contract-v1.lisp`

## Must Not Touch

- `crates/**`
- `.githooks/pre-commit`
- `.missiond/v2/*.lisp`
- `.missiond/tasks/wave21/**`

## Requirements

1. Add a default-on doctor status: task briefs should treat unset/wrong core.hooksPath as a preflight problem with a concrete install command.
2. Do not mutate git config from the renderer or doctor; only install-missiond-hooks --install may run git config --local core.hooksPath .githooks.
3. Render hook doctor commands in commit-required task briefs before staged guard commands.
4. Add dry-fixture coverage for installed, unset, wrong path, and missing hook file states.
5. Keep all hook config repo-local; do not write global git config.

## Acceptance Commands

```bash
node scripts/install-missiond-hooks.mjs --dry-fixture
node scripts/check-missiond-hooks.mjs --json
node scripts/check-task-contract.mjs --all
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- scripts/install-missiond-hooks.mjs scripts/check-missiond-hooks.mjs scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp
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

Expected machine-readable report: `.missiond/tasks/wave22/reports/wave22-01-hooks-default-on-doctor-v2.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave22/reports/wave22-01-hooks-default-on-doctor-v2.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add "scripts/install-missiond-hooks.mjs" \
        "scripts/check-missiond-hooks.mjs" \
        "scripts/render-claudecode-task.mjs" \
        ".missiond/tasks/schema/task-contract-v1.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave22/wave22-01-hooks-default-on-doctor-v2.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave22/wave22-01-hooks-default-on-doctor-v2.lisp \
  git commit -m "feat(tasks): surface hooksPath as default preflight"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave22/wave22-01-hooks-default-on-doctor-v2.lisp
```

## Report

- `Commit hash.`
- `Doctor status model.`
- `Rendered brief changes.`
- `Mutating command boundary.`
- `Acceptance command results.`

