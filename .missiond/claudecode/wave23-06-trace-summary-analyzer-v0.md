# wave23-06-trace-summary-analyzer-v0 — Session trace summary analyzer v0

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave23/wave23-06-trace-summary-analyzer-v0.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave23-01-session-trace-schema-v0`
- shared_memory: `.missiond/tasks/wave23/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave23/reports/wave23-06-trace-summary-analyzer-v0.report.lisp`

## Goal

Add a small read-only analyzer that summarizes session-trace facts into counts and bottleneck hints, without attempting router replacement yet.

## Ownership

- `scripts/analyze-session-trace.mjs`
- `scripts/check-session-trace.mjs`
- `.missiond/tasks/schema/session-trace-v1.lisp`

## Must Not Touch

- `crates/**`
- `.missiond/v2/*.lisp`
- `.missiond/tasks/wave22/**`
- `scripts/render-claudecode-task.mjs`

## Requirements

1. Add scripts/analyze-session-trace.mjs with --json, --dry-fixture, and one or more trace file inputs.
2. Summarize by task and backend: event counts, command/test counts, failure/retry counts, total duration_ms when present, files touched, commit count.
3. Emit conservative bottleneck hints only from observed facts; do not infer hidden ClaudeCode reasoning.
4. Do not implement router policy or model replacement in this task.

## Acceptance Commands

```bash
node scripts/analyze-session-trace.mjs --dry-fixture
node scripts/check-session-trace.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
git diff --check -- scripts/analyze-session-trace.mjs scripts/check-session-trace.mjs .missiond/tasks/schema/session-trace-v1.lisp
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

Expected machine-readable report: `.missiond/tasks/wave23/reports/wave23-06-trace-summary-analyzer-v0.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave23/reports/wave23-06-trace-summary-analyzer-v0.report.lisp
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
git add "scripts/analyze-session-trace.mjs" \
        "scripts/check-session-trace.mjs" \
        ".missiond/tasks/schema/session-trace-v1.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave23/wave23-06-trace-summary-analyzer-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave23/wave23-06-trace-summary-analyzer-v0.lisp \
  git commit -m "feat(tasks): summarize session traces"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `node scripts/install-missiond-hooks.mjs --install`, equivalent to `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave23/wave23-06-trace-summary-analyzer-v0.lisp
```

## Report

- `Commit hash.`
- `Analyzer output fields.`
- `Bottleneck hint rules.`
- `Non-goals.`
- `Acceptance command results.`

