# wave26-00-archive-wave25-artifacts — Archive Wave 25 artifacts

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave26/wave26-00-archive-wave25-artifacts.lisp`

## Machine Contract

- kind: `archive`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave25-06-lisp-backfill-router-measurement-status`
- shared_memory: `.missiond/tasks/wave26/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave26/reports/wave26-00-archive-wave25-artifacts.report.lisp`
- session_trace: `.missiond/tasks/wave26/session-trace.lisp`
- session_trace_writable: `true`

## Goal

Archive all untracked Wave 25 task contracts, rendered ClaudeCode briefs, reports, shared-memory ledger, and session-trace ledger after Wave 25 code and Lisp commits are complete.

## Ownership

- `.missiond/tasks/wave25/**`
- `.missiond/claudecode/wave25-*.md`

## Must Not Touch

- `crates/**`
- `scripts/**`
- `.missiond/v2/**`
- `.missiond/tasks/wave26/wave26-*.lisp`
- `.missiond/claudecode/wave26-*.md`

## Requirements

1. Stage only Wave 25 artifacts currently untracked.
2. Do not modify artifact contents while archiving; commit verbatim.
3. Include task contracts, rendered briefs, reports, shared-memory.lisp, and session-trace.lisp.
4. It is acceptable that Wave25 Codex-owned Lisp backfill and coordination-index tasks have no report; call that out explicitly if true.
5. Do not stage Wave 26 task files or briefs.

## Acceptance Commands

```bash
node scripts/check-task-contract.mjs --all
node scripts/check-task-memory.mjs .missiond/tasks/wave25/shared-memory.lisp
node scripts/check-session-trace.mjs .missiond/tasks/wave25/session-trace.lisp
git diff --cached --name-only
git diff --check -- .missiond/tasks/wave25 .missiond/claudecode
```

## Shared Memory

Coordination ledger: `.missiond/tasks/wave26/shared-memory.lisp` (schema `missiond.shared-memory.v1`).

- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.
- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.
- `:touched` paths in your entries must stay inside this task `:write-scope`.

Validate with:

```bash
node scripts/check-task-memory.mjs .missiond/tasks/wave26/shared-memory.lisp
```

## Report Contract

Expected machine-readable report: `.missiond/tasks/wave26/reports/wave26-00-archive-wave25-artifacts.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.
- Optional worker-explanation fields (prose only — facts live in `session-trace.lisp`):
  - `:time_sinks` — vector of strings or `(:label <s> [:duration_ms <int>] [:notes <s>])` entries.
  - `:major_decisions` — vector of strings or `(:decision <s> [:rationale <s>] [:trace_ref <s>])` entries.
  - `:unexpected_work` — vector of strings or `(:summary <s> [:trace_ref <s>])` entries.
  - `:blockers` — vector of strings or `(:summary <s> [:resolved <bool>] [:trace_ref <s>])` entries.
  - `:trace_refs` — vector of session-trace event ids or repo-relative paths linking back to factual telemetry.
- Optional router-recommendation fields (wave25-02 — populate ONLY when you observe a dry-run recommendation; the recommendation is **advisory** and **dry-run only**, never authoritative for dispatch):
  - `:recommended_backend` — string enum: `claudecode | missiond-llm-router | deterministic-checker | patch-worker | verifier-worker`.
  - `:router_confidence` — string enum: `high | medium | low`.
  - `:router_policy_path` — repo-relative path to the policy consulted.
  - `:router_dry_run_only` — literal `true` (cross-wave invariant).
  - `:router_applied` — literal `false` (cross-wave invariant — runtime replacement is rejected).
  - `:router_reasons` — vector of non-empty strings.
  - `:router_trace_index_path` — repo-relative path to the trace index that scored confidence (when used).

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave26/reports/wave26-00-archive-wave25-artifacts.report.lisp
```

## Session Trace

Factual telemetry ledger: `.missiond/tasks/wave26/session-trace.lisp` (schema `missiond.session-trace.v1`).

- This file is the single source of truth for what happened: dispatch / start / read / edit / command / test / commit / complete / failure / retry / observation events.
- Worker prose explanations belong in the report contract's `:time_sinks` / `:major_decisions` / `:unexpected_work` / `:blockers` / `:trace_refs` fields, not here.
- This task is `:session-trace-writable true`: you MAY append `(trace-event ...)` entries to the ledger as factual coordination output, in addition to your declared `:write-scope`. Entries must follow the schema (required `:id` `:seq` `:at` `:task` `:backend` `:kind` `:summary`).
- Treat the trace ledger as an append-only journal: never edit prior events; record corrections as new events that reference the prior `:id` via `:trace_refs`.

Validate the ledger after any change with:

```bash
node scripts/check-session-trace.mjs .missiond/tasks/wave26/session-trace.lisp
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

You **may** also inspect the dry-run recommendation for THIS task by running the recommendation CLI directly. The renderer never executes it — the command below is rendered text only and stays **advisory** and **dry-run only**; the recommendation does not change dispatch and you MUST NOT switch backend on the strength of its output:

```bash
node scripts/recommend-task-backend.mjs --task .missiond/tasks/wave26/wave26-00-archive-wave25-artifacts.lisp --policy .missiond/router/router-policy-v1.lisp --json
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
git add ".missiond/tasks/wave25/**" \
        ".missiond/claudecode/wave25-*.md"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave26/wave26-00-archive-wave25-artifacts.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave26/wave26-00-archive-wave25-artifacts.lisp \
  git commit -m "chore(wave25): archive router measurement artifacts"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `node scripts/install-missiond-hooks.mjs --install`, equivalent to `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave26/wave26-00-archive-wave25-artifacts.lisp
```

## Report

- `Commit hash.`
- `Archived file count and groups.`
- `Any intentionally missing report artifact.`
- `Acceptance command results.`

