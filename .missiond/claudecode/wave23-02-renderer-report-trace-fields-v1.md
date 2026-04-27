# wave23-02-renderer-report-trace-fields-v1 — Renderer and report trace fields v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave23/wave23-02-renderer-report-trace-fields-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave23-01-session-trace-schema-v0`
- shared_memory: `.missiond/tasks/wave23/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave23/reports/wave23-02-renderer-report-trace-fields-v1.report.lisp`

## Goal

Surface the session-trace path in rendered task briefs and add worker explanation fields to report-contract without making report prose the factual source.

## Ownership

- `scripts/render-claudecode-task.mjs`
- `scripts/check-task-report.mjs`
- `.missiond/tasks/schema/task-contract-v1.lisp`
- `.missiond/tasks/schema/report-contract-v1.lisp`
- `.missiond/tasks/wave23/wave23-01-session-trace-schema-v0.lisp`
- `.missiond/claudecode/wave23-01-session-trace-schema-v0.md`

## Must Not Touch

- `crates/**`
- `.missiond/v2/*.lisp`
- `scripts/check-session-trace.mjs`
- `.missiond/tasks/wave22/**`

## Requirements

1. Render session_trace path when .missiond/tasks/<wave>/session-trace.lisp exists.
2. In rendered briefs, instruct workers to append claim/completion to shared-memory and append factual trace entries only when their task contract allows the trace ledger as shared coordination output.
3. Extend report-contract optional fields for worker explanations: :time_sinks, :major_decisions, :unexpected_work, :blockers, :trace_refs.
4. Report checker should validate these fields structurally but not treat them as facts.
5. Re-render wave23-01 as a golden example.

## Acceptance Commands

```bash
node scripts/check-task-report.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
node scripts/render-claudecode-task.mjs --force .missiond/tasks/wave23/wave23-01-session-trace-schema-v0.lisp
git diff --check -- scripts/render-claudecode-task.mjs scripts/check-task-report.mjs .missiond/tasks/schema/task-contract-v1.lisp .missiond/tasks/schema/report-contract-v1.lisp .missiond/tasks/wave23/wave23-01-session-trace-schema-v0.lisp .missiond/claudecode/wave23-01-session-trace-schema-v0.md
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

Expected machine-readable report: `.missiond/tasks/wave23/reports/wave23-02-renderer-report-trace-fields-v1.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave23/reports/wave23-02-renderer-report-trace-fields-v1.report.lisp
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
git add "scripts/render-claudecode-task.mjs" \
        "scripts/check-task-report.mjs" \
        ".missiond/tasks/schema/task-contract-v1.lisp" \
        ".missiond/tasks/schema/report-contract-v1.lisp" \
        ".missiond/tasks/wave23/wave23-01-session-trace-schema-v0.lisp" \
        ".missiond/claudecode/wave23-01-session-trace-schema-v0.md"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave23/wave23-02-renderer-report-trace-fields-v1.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave23/wave23-02-renderer-report-trace-fields-v1.lisp \
  git commit -m "feat(tasks): surface session trace in briefs and reports"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `node scripts/install-missiond-hooks.mjs --install`, equivalent to `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave23/wave23-02-renderer-report-trace-fields-v1.lisp
```

## Report

- `Commit hash.`
- `Rendered sections added.`
- `Report explanation fields.`
- `Acceptance command results.`

