# wave21-02-run-verifier-v1 — Task run verifier v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave21/wave21-02-run-verifier-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `agent-team`
- depends_on: `wave21-00-archive-wave20-task-artifacts`
- shared_memory: `.missiond/tasks/wave21/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave21/reports/wave21-02-run-verifier-v1.report.lisp`

## Dispatch Note

使用 agent-team提高效率

## Goal

Add one verifier that ties together task.lisp, report.lisp, shared-memory completion, and git commit scope into a single post-run proof.

## Ownership

- `scripts/verify-task-run.mjs`
- `scripts/verify-task-contract.mjs`
- `scripts/check-task-report.mjs`
- `scripts/check-task-memory.mjs`
- `scripts/lib/missiond_lisp.mjs`
- `.missiond/tasks/schema/report-contract-v1.lisp`
- `.missiond/tasks/schema/shared-memory-v1.lisp`

## Must Not Touch

- `crates/**`
- `.missiond/v2/*.lisp`
- `.githooks/pre-commit`
- `.missiond/tasks/wave20/**`

## Requirements

1. Use agent-team if useful: 使用 agent-team提高效率.
2. Add scripts/verify-task-run.mjs with --task <task.lisp>, --report <report.lisp>, --memory <shared-memory.lisp>, --commit <hash>, --json, and --dry-fixture.
3. Verify: task contract passes, report references the same task_id, report commit_hash equals commit, commit scope passes verify-task-contract, and shared-memory has a completion entry for the task.
4. Keep the verifier read-only: no git add, commit, reset, checkout, stash, push, merge, or rebase.
5. If shared-memory is absent, fail with a structured diagnostic unless --allow-missing-memory is explicitly provided.

## Acceptance Commands

```bash
node scripts/verify-task-run.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
node scripts/check-task-report.mjs --dry-fixture
node scripts/check-task-memory.mjs --dry-fixture
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- scripts/verify-task-run.mjs scripts/verify-task-contract.mjs scripts/check-task-report.mjs scripts/check-task-memory.mjs scripts/lib/missiond_lisp.mjs .missiond/tasks/schema/report-contract-v1.lisp .missiond/tasks/schema/shared-memory-v1.lisp
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

Expected machine-readable report: `.missiond/tasks/wave21/reports/wave21-02-run-verifier-v1.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave21/reports/wave21-02-run-verifier-v1.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add "scripts/verify-task-run.mjs" \
        "scripts/verify-task-contract.mjs" \
        "scripts/check-task-report.mjs" \
        "scripts/check-task-memory.mjs" \
        "scripts/lib/missiond_lisp.mjs" \
        ".missiond/tasks/schema/report-contract-v1.lisp" \
        ".missiond/tasks/schema/shared-memory-v1.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave21/wave21-02-run-verifier-v1.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave21/wave21-02-run-verifier-v1.lisp \
  git commit -m "feat(tasks): verify complete task runs"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave21/wave21-02-run-verifier-v1.lisp
```

## Report

- `Commit hash.`
- `Run verifier CLI synopsis.`
- `Dry-fixture coverage.`
- `Read-only git proof.`
- `Acceptance command results.`

