# wave24-03-router-recommendation-cli-v0 — Router recommendation CLI v0

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave24/wave24-03-router-recommendation-cli-v0.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave24-01-router-policy-schema-v1`, `wave24-02-trace-corpus-index-v0`
- shared_memory: `.missiond/tasks/wave24/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave24/reports/wave24-03-router-recommendation-cli-v0.report.lisp`
- session_trace: `.missiond/tasks/wave24/session-trace.lisp`
- session_trace_writable: `true`

## Goal

Add a read-only CLI that consumes a task contract, router policy, and trace corpus index to emit an explainable backend recommendation without changing runtime dispatch.

## Ownership

- `scripts/recommend-task-backend.mjs`
- `scripts/check-router-policy.mjs`
- `scripts/build-session-trace-index.mjs`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/tasks/schema/*.lisp`
- `.missiond/tasks/wave23/**`
- `.missiond/tasks/wave24/wave24-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. CLI shape: node scripts/recommend-task-backend.mjs --task <task.lisp> --policy <router-policy.lisp> [--trace-index <json>] [--json] [--dry-fixture].
2. Recommendation output must include backend, confidence, matched_rules, rejected_rules, non_goals, and dry_run_only=true.
3. Use deterministic inputs only; do not call LLMs, ClaudeCode, git, or shell commands.
4. If evidence is insufficient, recommend claudecode with confidence low and reason insufficient_trace_history.
5. Do not mutate task contracts, router policy, trace index, or runtime dispatch.

## Acceptance Commands

```bash
node scripts/recommend-task-backend.mjs --dry-fixture
node scripts/recommend-task-backend.mjs --task .missiond/tasks/wave24/wave24-01-router-policy-schema-v1.lisp --policy .missiond/router/router-policy-v1.lisp --json
node scripts/check-router-policy.mjs .missiond/router/router-policy-v1.lisp
node scripts/check-task-contract.mjs --all
git diff --check -- scripts/recommend-task-backend.mjs scripts/check-router-policy.mjs scripts/build-session-trace-index.mjs
```

## Shared Memory

Coordination ledger: `.missiond/tasks/wave24/shared-memory.lisp` (schema `missiond.shared-memory.v1`).

- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.
- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.
- `:touched` paths in your entries must stay inside this task `:write-scope`.

Validate with:

```bash
node scripts/check-task-memory.mjs .missiond/tasks/wave24/shared-memory.lisp
```

## Report Contract

Expected machine-readable report: `.missiond/tasks/wave24/reports/wave24-03-router-recommendation-cli-v0.report.lisp` (schema `missiond.report-contract.v1`).

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
node scripts/check-task-report.mjs .missiond/tasks/wave24/reports/wave24-03-router-recommendation-cli-v0.report.lisp
```

## Session Trace

Factual telemetry ledger: `.missiond/tasks/wave24/session-trace.lisp` (schema `missiond.session-trace.v1`).

- This file is the single source of truth for what happened: dispatch / start / read / edit / command / test / commit / complete / failure / retry / observation events.
- Worker prose explanations belong in the report contract's `:time_sinks` / `:major_decisions` / `:unexpected_work` / `:blockers` / `:trace_refs` fields, not here.
- This task is `:session-trace-writable true`: you MAY append `(trace-event ...)` entries to the ledger as factual coordination output, in addition to your declared `:write-scope`. Entries must follow the schema (required `:id` `:seq` `:at` `:task` `:backend` `:kind` `:summary`).
- Treat the trace ledger as an append-only journal: never edit prior events; record corrections as new events that reference the prior `:id` via `:trace_refs`.

Validate the ledger after any change with:

```bash
node scripts/check-session-trace.mjs .missiond/tasks/wave24/session-trace.lisp
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
git add "scripts/recommend-task-backend.mjs" \
        "scripts/check-router-policy.mjs" \
        "scripts/build-session-trace-index.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave24/wave24-03-router-recommendation-cli-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave24/wave24-03-router-recommendation-cli-v0.lisp \
  git commit -m "feat(tasks): recommend backend from trace policy"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `node scripts/install-missiond-hooks.mjs --install`, equivalent to `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave24/wave24-03-router-recommendation-cli-v0.lisp
```

## Report

- `Commit hash.`
- `Recommendation JSON shape.`
- `Dry-run/non-mutating guarantees.`
- `Acceptance command results.`

