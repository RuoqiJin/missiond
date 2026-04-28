# wave25-07-parallel-dispatch-index — Wave 25 parallel dispatch index

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave25/wave25-07-parallel-dispatch-index.lisp`

## Machine Contract

- kind: `coordination`
- status: `ready`
- owner: `codex-orchestrator`
- dispatch_strategy: `manual`
- shared_memory: `.missiond/tasks/wave25/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave25/reports/wave25-07-parallel-dispatch-index.report.lisp`
- session_trace: `.missiond/tasks/wave25/session-trace.lisp`
- session_trace_writable: `false`

## Goal

Human/Codex dispatch index for Wave 25. This file is not meant for a ClaudeCode worker commit by itself.

## Ownership

- `.missiond/tasks/wave25/wave25-*.lisp`
- `.missiond/claudecode/wave25-*.md`

## Must Not Touch

- `crates/**`
- `scripts/**`
- `.missiond/v2/**`
- `.missiond/tasks/wave24/**`

## Requirements

1. Group A: 00 archive must run first.
2. Group B after 00: 01 evaluator, 02 report fields, and 03 plan trace-index confidence can run in parallel (disjoint write scopes).
3. Group C: 04 renderer depends on 01+02.
4. Group D: 05 smoke depends on 01+02+03+04.
5. Group E: 06 Lisp backfill is Codex-owned after all committed code tasks.
6. Wave25 remains dry-run/advisory only; no runtime backend replacement task is included.

## Acceptance Commands

```bash
node scripts/check-task-contract.mjs --all
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

Expected machine-readable report: `.missiond/tasks/wave25/reports/wave25-07-parallel-dispatch-index.report.lisp` (schema `missiond.report-contract.v1`).

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
node scripts/check-task-report.mjs .missiond/tasks/wave25/reports/wave25-07-parallel-dispatch-index.report.lisp
```

## Session Trace

Factual telemetry ledger: `.missiond/tasks/wave25/session-trace.lisp` (schema `missiond.session-trace.v1`).

- This file is the single source of truth for what happened: dispatch / start / read / edit / command / test / commit / complete / failure / retry / observation events.
- Worker prose explanations belong in the report contract's `:time_sinks` / `:major_decisions` / `:unexpected_work` / `:blockers` / `:trace_refs` fields, not here.
- This task is **not** `:session-trace-writable` (default). You MUST NOT write to `session-trace.lisp` — read it for context only. Telemetry for this task is recorded by MissionD or by tasks explicitly opted in via `:session-trace-writable true`.

Validate the ledger after any change with:

```bash
node scripts/check-session-trace.mjs .missiond/tasks/wave25/session-trace.lisp
```

## Router Policy (advisory)

Dry-run router-policy ledger: `.missiond/router/router-policy-v1.lisp` (schema `missiond.router-policy.v1`).

- This section is **advisory** and **dry-run only**. The policy file is informational; it captures backend recommendations distilled from prior session-trace observations, but **runtime dispatch is unchanged** — ClaudeCode remains the live backend for this task.
- The brief surfaces the policy path so human readers and ClaudeCode workers can consult the recommendations; it does not instruct the worker to switch backend, alter the dispatch strategy, or run the recommendation CLI.
- Source: auto-detected default seed (no `:router-policy-path` on the task contract).

Inspect the policy with the read-only checker (the renderer itself does not execute the policy or shell out to the recommendation CLI):

```bash
node scripts/check-router-policy.mjs .missiond/router/router-policy-v1.lisp
```

## Commit

No commit required by contract.

## Report

- `No report required; this is a coordination index.`

