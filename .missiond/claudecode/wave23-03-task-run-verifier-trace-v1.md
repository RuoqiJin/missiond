# wave23-03-task-run-verifier-trace-v1 — Task run verifier trace integration v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave23/wave23-03-task-run-verifier-trace-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave23-01-session-trace-schema-v0`
- shared_memory: `.missiond/tasks/wave23/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave23/reports/wave23-03-task-run-verifier-trace-v1.report.lisp`

## Goal

Teach the task-run verifier to include session-trace checks so completed tasks have contract/report/memory/commit/trace proof in one read-only verifier.

## Ownership

- `scripts/verify-task-run.mjs`
- `scripts/check-session-trace.mjs`
- `scripts/check-task-memory.mjs`
- `scripts/check-task-report.mjs`
- `scripts/lib/missiond_lisp.mjs`

## Must Not Touch

- `crates/**`
- `.missiond/v2/*.lisp`
- `.missiond/tasks/schema/*.lisp`
- `.missiond/tasks/wave22/**`

## Requirements

1. Add --trace <session-trace.lisp> to scripts/verify-task-run.mjs.
2. Verify the trace contains at least one completion event for the task when --trace is supplied.
3. Verify commit_hash in trace completion matches report commit_hash when both are present.
4. Preserve existing behavior when --trace is absent unless --require-trace is provided.
5. Add --dry-fixture coverage for pass, missing completion, mismatched commit, malformed trace, and absent trace allowed.

## Acceptance Commands

```bash
node scripts/verify-task-run.mjs --dry-fixture
node scripts/check-session-trace.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
git diff --check -- scripts/verify-task-run.mjs scripts/check-session-trace.mjs scripts/check-task-memory.mjs scripts/check-task-report.mjs scripts/lib/missiond_lisp.mjs
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

Expected machine-readable report: `.missiond/tasks/wave23/reports/wave23-03-task-run-verifier-trace-v1.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave23/reports/wave23-03-task-run-verifier-trace-v1.report.lisp
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
git add "scripts/verify-task-run.mjs" \
        "scripts/check-session-trace.mjs" \
        "scripts/check-task-memory.mjs" \
        "scripts/check-task-report.mjs" \
        "scripts/lib/missiond_lisp.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave23/wave23-03-task-run-verifier-trace-v1.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave23/wave23-03-task-run-verifier-trace-v1.lisp \
  git commit -m "feat(tasks): verify task runs with session trace"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `node scripts/install-missiond-hooks.mjs --install`, equivalent to `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave23/wave23-03-task-run-verifier-trace-v1.lisp
```

## Report

- `Commit hash.`
- `New verifier flags.`
- `Trace checks.`
- `Acceptance command results.`

