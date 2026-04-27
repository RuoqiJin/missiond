# wave21-01-hooks-path-installer-v1 — MissionD hooksPath installer v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave21/wave21-01-hooks-path-installer-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave21-00-archive-wave20-task-artifacts`
- shared_memory: `.missiond/tasks/wave21/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave21/reports/wave21-01-hooks-path-installer-v1.report.lisp`

## Goal

Move scoped task guardrails from optional knowledge to a repo-local, explicit installer/doctor flow for core.hooksPath .githooks.

## Ownership

- `scripts/install-missiond-hooks.mjs`
- `scripts/check-missiond-hooks.mjs`
- `.githooks/pre-commit`
- `.missiond/tasks/schema/task-contract-v1.lisp`

## Must Not Touch

- `crates/**`
- `.missiond/v2/*.lisp`
- `.missiond/tasks/wave20/**`
- `.missiond/claudecode/wave20-*.md`

## Requirements

1. Add scripts/install-missiond-hooks.mjs with --check, --install, --json, and --dry-fixture.
2. In --check mode, read git config --get core.hooksPath and report whether it equals .githooks.
3. In --install mode, run only git config core.hooksPath .githooks; do not mutate anything else.
4. Add scripts/check-missiond-hooks.mjs as a read-only doctor alias if that keeps CLI cleaner.
5. Ensure .githooks/pre-commit remains env-gated by MISSIOND_TASK_CONTRACT and delegates to task-scope-guard.
6. Do not enable hooks globally; repo-local only.

## Acceptance Commands

```bash
node scripts/install-missiond-hooks.mjs --dry-fixture
node scripts/install-missiond-hooks.mjs --check --json
node scripts/check-task-contract.mjs --all
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- scripts/install-missiond-hooks.mjs scripts/check-missiond-hooks.mjs .githooks/pre-commit .missiond/tasks/schema/task-contract-v1.lisp
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

Expected machine-readable report: `.missiond/tasks/wave21/reports/wave21-01-hooks-path-installer-v1.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave21/reports/wave21-01-hooks-path-installer-v1.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add "scripts/install-missiond-hooks.mjs" \
        "scripts/check-missiond-hooks.mjs" \
        ".githooks/pre-commit" \
        ".missiond/tasks/schema/task-contract-v1.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave21/wave21-01-hooks-path-installer-v1.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave21/wave21-01-hooks-path-installer-v1.lisp \
  git commit -m "feat(tasks): add MissionD hooksPath installer"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave21/wave21-01-hooks-path-installer-v1.lisp
```

## Report

- `Commit hash.`
- `Installer CLI synopsis.`
- `Mutating command proof.`
- `Hook doctor output.`
- `Acceptance command results.`

