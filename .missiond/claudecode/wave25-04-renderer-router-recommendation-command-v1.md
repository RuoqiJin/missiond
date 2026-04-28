# wave25-04-renderer-router-recommendation-command-v1 — Renderer router recommendation command v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave25/wave25-04-renderer-router-recommendation-command-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave25-01-router-policy-corpus-evaluator-v0`, `wave25-02-report-router-recommendation-fields-v0`
- shared_memory: `.missiond/tasks/wave25/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave25/reports/wave25-04-renderer-router-recommendation-command-v1.report.lisp`
- session_trace: `.missiond/tasks/wave25/session-trace.lisp`
- session_trace_writable: `true`

## Goal

Make rendered task briefs show exact read-only router recommendation commands and report-field expectations, while keeping the recommendation advisory and non-load-bearing.

## Ownership

- `scripts/render-claudecode-task.mjs`
- `.missiond/tasks/schema/task-contract-v1.lisp`

## Must Not Touch

- `crates/**`
- `scripts/check-task-report.mjs`
- `scripts/recommend-task-backend.mjs`
- `scripts/evaluate-router-policy-corpus.mjs`
- `.missiond/tasks/schema/report-contract-v1.lisp`
- `.missiond/v2/**`
- `.missiond/tasks/wave24/**`
- `.missiond/tasks/wave25/wave25-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Extend the existing Router Policy (advisory) section with exact read-only commands: check-router-policy and recommend-task-backend for the current task.
2. If a report_contract path is rendered, mention that reports may include the optional router recommendation fields from wave25-02.
3. Do not make the recommendation a dispatch instruction; keep literal advisory and dry-run only text.
4. Do not shell out from the renderer; commands are text only.
5. Preserve Wave23 Session Trace section and Wave24 Router Policy section ordering and backward compatibility.

## Acceptance Commands

```bash
node scripts/render-claudecode-task.mjs --stdout .missiond/tasks/wave25/wave25-01-router-policy-corpus-evaluator-v0.lisp > /tmp/wave25-render-check.md
rg -n "Router Policy|advisory|dry-run only|recommend-task-backend|check-router-policy" /tmp/wave25-render-check.md
node scripts/check-task-contract.mjs --all
node scripts/check-task-report.mjs --dry-fixture
git diff --check -- scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp
```

## Shared Memory

Coordination ledger: `.missiond/tasks/wave25/shared-memory.lisp` (schema `missiond.shared-memory.v1`).

- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.
- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.
- `:touched` paths in your entries must stay inside this task `:write-scope`.

Validate with:

```bash
node scripts/check-task-memory.mjs .missiond/tasks/wave25/shared-memory.lisp
```

## Report Contract

Expected machine-readable report: `.missiond/tasks/wave25/reports/wave25-04-renderer-router-recommendation-command-v1.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.
- Optional worker-explanation fields (prose only — facts live in `session-trace.lisp`):
  - `:time_sinks` — vector of strings or `(:label <s> [:duration_ms <int>] [:notes <s>])` entries.
  - `:major_decisions` — vector of strings or `(:decision <s> [:rationale <s>] [:trace_ref <s>])` entries.
  - `:unexpected_work` — vector of strings or `(:summary <s> [:trace_ref <s>])` entries.
  - `:blockers` — vector of strings or `(:summary <s> [:resolved <bool>] [:trace_ref <s>])` entries.
  - `:trace_refs` — vector of session-trace event ids or repo-relative paths linking back to factual telemetry.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave25/reports/wave25-04-renderer-router-recommendation-command-v1.report.lisp
```

## Session Trace

Factual telemetry ledger: `.missiond/tasks/wave25/session-trace.lisp` (schema `missiond.session-trace.v1`).

- This file is the single source of truth for what happened: dispatch / start / read / edit / command / test / commit / complete / failure / retry / observation events.
- Worker prose explanations belong in the report contract's `:time_sinks` / `:major_decisions` / `:unexpected_work` / `:blockers` / `:trace_refs` fields, not here.
- This task is `:session-trace-writable true`: you MAY append `(trace-event ...)` entries to the ledger as factual coordination output, in addition to your declared `:write-scope`. Entries must follow the schema (required `:id` `:seq` `:at` `:task` `:backend` `:kind` `:summary`).
- Treat the trace ledger as an append-only journal: never edit prior events; record corrections as new events that reference the prior `:id` via `:trace_refs`.

Validate the ledger after any change with:

```bash
node scripts/check-session-trace.mjs .missiond/tasks/wave25/session-trace.lisp
```

## Router Policy (advisory)

Dry-run router-policy ledger: `.missiond/router/router-policy-v1.lisp` (schema `missiond.router-policy.v1`).

- This section is **advisory** and **dry-run only**. The policy file is informational; it captures backend recommendations distilled from prior session-trace observations, but **runtime dispatch is unchanged** — ClaudeCode remains the live backend for this task.
- The brief surfaces the policy path so human readers and ClaudeCode workers can consult the recommendations; it does not instruct the worker to switch backend, alter the dispatch strategy, or run the recommendation CLI.
- Source: explicit `:router-policy-path` on the task contract.

Inspect the policy with the read-only checker (the renderer itself does not execute the policy or shell out to the recommendation CLI):

```bash
node scripts/check-router-policy.mjs .missiond/router/router-policy-v1.lisp
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
        ".missiond/tasks/schema/task-contract-v1.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave25/wave25-04-renderer-router-recommendation-command-v1.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave25/wave25-04-renderer-router-recommendation-command-v1.lisp \
  git commit -m "feat(tasks): render router recommendation commands"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `node scripts/install-missiond-hooks.mjs --install`, equivalent to `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave25/wave25-04-renderer-router-recommendation-command-v1.lisp
```

## Report

- `Commit hash.`
- `Rendered section wording and command examples.`
- `Backward-compat proof.`
- `Acceptance command results.`

