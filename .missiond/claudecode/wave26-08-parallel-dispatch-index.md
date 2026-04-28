# wave26-08-parallel-dispatch-index — Wave 26 parallel dispatch index

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave26/wave26-08-parallel-dispatch-index.lisp`

## Machine Contract

- kind: `coordination`
- status: `ready`
- owner: `codex-orchestrator`
- dispatch_strategy: `manual`
- shared_memory: `.missiond/tasks/wave26/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave26/reports/wave26-08-parallel-dispatch-index.report.lisp`
- session_trace: `.missiond/tasks/wave26/session-trace.lisp`
- session_trace_writable: `false`

## Goal

Human/Codex dispatch index for Wave 26. This file is not meant for a ClaudeCode worker commit by itself.

## Ownership

- `.missiond/tasks/wave26/wave26-*.lisp`
- `.missiond/claudecode/wave26-*.md`

## Must Not Touch

- `crates/**`
- `scripts/**`
- `.missiond/v2/**`
- `.missiond/tasks/wave25/**`

## Requirements

1. Group A: 00 archive must run first.
2. Group B after 00: 01 backend registry is the foundation and must run before readiness consumers.
3. Group C after 01: 02 Node recommendation readiness and 03 Rust mission_plan readiness can run in parallel (disjoint write scopes).
4. Group D after 02: 04 report readiness fields.
5. Group E after 01+02+04: 05 renderer readiness context.
6. Group F after 02+03+04+05: 06 smoke.
7. Group G: 07 Lisp backfill is Codex-owned after all committed code tasks.
8. Wave26 remains dry-run/advisory/readiness-only; no runtime backend replacement task is included.

## Acceptance Commands

```bash
node scripts/check-task-contract.mjs --all
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

Expected machine-readable report: `.missiond/tasks/wave26/reports/wave26-08-parallel-dispatch-index.report.lisp` (schema `missiond.report-contract.v1`).

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
node scripts/check-task-report.mjs .missiond/tasks/wave26/reports/wave26-08-parallel-dispatch-index.report.lisp
```

## Session Trace

Factual telemetry ledger: `.missiond/tasks/wave26/session-trace.lisp` (schema `missiond.session-trace.v1`).

- This file is the single source of truth for what happened: dispatch / start / read / edit / command / test / commit / complete / failure / retry / observation events.
- Worker prose explanations belong in the report contract's `:time_sinks` / `:major_decisions` / `:unexpected_work` / `:blockers` / `:trace_refs` fields, not here.
- This task is **not** `:session-trace-writable` (default). You MUST NOT write to `session-trace.lisp` — read it for context only. Telemetry for this task is recorded by MissionD or by tasks explicitly opted in via `:session-trace-writable true`.

Validate the ledger after any change with:

```bash
node scripts/check-session-trace.mjs .missiond/tasks/wave26/session-trace.lisp
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

You **may** also inspect the dry-run recommendation for THIS task by running the recommendation CLI directly. The renderer never executes it — the command below is rendered text only and stays **advisory** and **dry-run only**; the recommendation does not change dispatch and you MUST NOT switch backend on the strength of its output:

```bash
node scripts/recommend-task-backend.mjs --task .missiond/tasks/wave26/wave26-08-parallel-dispatch-index.lisp --policy .missiond/router/router-policy-v1.lisp --json
```

## Commit

No commit required by contract.

## Report

- `No report required; this is a coordination index.`

