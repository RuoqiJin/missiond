# wave27-05-renderer-router-dispatch-descriptor-context-v0 — Renderer router dispatch descriptor context v0

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave27/wave27-05-renderer-router-dispatch-descriptor-context-v0.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave27-01-router-dispatch-descriptor-schema-v0`, `wave27-02-router-dispatch-descriptor-cli-v0`, `wave27-04-report-router-dispatch-descriptor-fields-v0`
- shared_memory: `.missiond/tasks/wave27/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave27/reports/wave27-05-renderer-router-dispatch-descriptor-context-v0.report.lisp`
- session_trace: `.missiond/tasks/wave27/session-trace.lisp`
- session_trace_writable: `true`

## Goal

Teach the task renderer to surface router dispatch descriptor commands and report-field guidance while preserving the advisory/no-execution boundary.

## Ownership

- `scripts/render-claudecode-task.mjs`
- `.missiond/tasks/schema/task-contract-v1.lisp`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/router/**`
- `.missiond/tasks/wave27/wave27-*.lisp`
- `.missiond/claudecode/**`
- `scripts/check-router-dispatch-descriptor.mjs`
- `scripts/build-router-dispatch-descriptor.mjs`
- `scripts/check-task-report.mjs`
- `scripts/recommend-task-backend.mjs`
- `scripts/evaluate-router-policy-corpus.mjs`

## Requirements

1. Add optional task field :router-dispatch-descriptor-path / :router_dispatch_descriptor_path if useful; otherwise document why descriptor is generated on demand.
2. Render a read-only command using scripts/build-router-dispatch-descriptor.mjs and pipe/check instructions using scripts/check-router-dispatch-descriptor.mjs.
3. The rendered section must include literal phrases: advisory, dry-run only, no execution, and MUST NOT switch backend.
4. Report Contract section must mention the wave27-04 descriptor report fields with MAY language.
5. Renderer must not shell out and must preserve the wave26 Router Policy section ordering unless tests prove an intentional minimal insertion.
6. Add renderer dry-fixture coverage for descriptor literals and static audit for shell/child_process/git/network patterns.

## Acceptance Commands

```bash
node scripts/render-claudecode-task.mjs --dry-fixture
node scripts/render-claudecode-task.mjs --stdout .missiond/tasks/wave27/wave27-02-router-dispatch-descriptor-cli-v0.lisp > /tmp/wave27-router-dispatch-descriptor.md
rg 'Router Policy|dispatch descriptor|no execution|MUST NOT switch backend|build-router-dispatch-descriptor' /tmp/wave27-router-dispatch-descriptor.md
node scripts/check-task-contract.mjs --all
git diff --check -- scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp
```

## Shared Memory

Coordination ledger: `.missiond/tasks/wave27/shared-memory.lisp` (schema `missiond.shared-memory.v1`).

- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.
- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.
- `:touched` paths in your entries must stay inside this task `:write-scope`.

Validate with:

```bash
node scripts/check-task-memory.mjs .missiond/tasks/wave27/shared-memory.lisp
```

## Report Contract

Expected machine-readable report: `.missiond/tasks/wave27/reports/wave27-05-renderer-router-dispatch-descriptor-context-v0.report.lisp` (schema `missiond.report-contract.v1`).

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
- Optional router-readiness fields (wave26-04 — populate ONLY when you observe a backend readiness registry; readiness is **advisory** and **dry-run only**, and you MUST NOT switch backend based on these values):
  - `:router_backend_readiness_status` — string enum: `current-default | advisory-only | runtime-ready | unavailable | unknown`.
  - `:router_backend_runtime_allowed` — literal `true` or `false` (atom, never a string).
  - `:router_apply_eligible` — literal `true` or `false` (atom, never a string; current-default alone is NEVER sufficient — explicit runtime-ready opt-in required upstream).
  - `:router_apply_blockers` — vector of non-empty strings (no property-list entries).
  - `:router_backend_registry_path` — repo-relative path to the backend readiness registry consulted.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave27/reports/wave27-05-renderer-router-dispatch-descriptor-context-v0.report.lisp
```

## Session Trace

Factual telemetry ledger: `.missiond/tasks/wave27/session-trace.lisp` (schema `missiond.session-trace.v1`).

- This file is the single source of truth for what happened: dispatch / start / read / edit / command / test / commit / complete / failure / retry / observation events.
- Worker prose explanations belong in the report contract's `:time_sinks` / `:major_decisions` / `:unexpected_work` / `:blockers` / `:trace_refs` fields, not here.
- This task is `:session-trace-writable true`: you MAY append `(trace-event ...)` entries to the ledger as factual coordination output, in addition to your declared `:write-scope`. Entries must follow the schema (required `:id` `:seq` `:at` `:task` `:backend` `:kind` `:summary`).
- Treat the trace ledger as an append-only journal: never edit prior events; record corrections as new events that reference the prior `:id` via `:trace_refs`.

Validate the ledger after any change with:

```bash
node scripts/check-session-trace.mjs .missiond/tasks/wave27/session-trace.lisp
```

## Router Policy (advisory)

Dry-run router-policy ledger: `.missiond/router/router-policy-v1.lisp` (schema `missiond.router-policy.v1`).
Backend readiness registry: `.missiond/router/router-backend-registry-v1.lisp` (schema `missiond.router-backend-registry.v1`).

- This section is **advisory** and **dry-run only**. The policy file is informational; it captures backend recommendations distilled from prior session-trace observations, but **runtime dispatch is unchanged** — ClaudeCode remains the live backend for this task.
- The brief surfaces the policy path so human readers and ClaudeCode workers can consult the recommendations; it does not instruct the worker to switch backend, alter the dispatch strategy, or run the recommendation CLI.
- **You MUST NOT switch backend** based on anything rendered in this section. The recommendation and readiness fields below are observational signals only — runtime dispatch never changes as a side-effect of reading this brief, and apply-eligibility is recorded for telemetry, not for worker action.
- Policy source: explicit `:router-policy-path` on the task contract.
- Registry source: explicit `:router-backend-registry-path` on the task contract.

Inspect the policy with the read-only checker (the renderer itself does not execute the policy or shell out to the recommendation CLI):

```bash
node scripts/check-router-policy.mjs .missiond/router/router-policy-v1.lisp
node scripts/check-router-backend-registry.mjs .missiond/router/router-backend-registry-v1.lisp
```

You **may** also inspect the dry-run recommendation for THIS task by running the recommendation CLI directly. The renderer never executes it — the command below is rendered text only and stays **advisory** and **dry-run only**; the recommendation does not change dispatch and you MUST NOT switch backend on the strength of its output:

```bash
node scripts/recommend-task-backend.mjs --task .missiond/tasks/wave27/wave27-05-renderer-router-dispatch-descriptor-context-v0.lisp --policy .missiond/router/router-policy-v1.lisp --backend-registry .missiond/router/router-backend-registry-v1.lisp --json
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
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave27/wave27-05-renderer-router-dispatch-descriptor-context-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave27/wave27-05-renderer-router-dispatch-descriptor-context-v0.lisp \
  git commit -m "feat(tasks): render router dispatch descriptor context"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `node scripts/install-missiond-hooks.mjs --install`, equivalent to `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave27/wave27-05-renderer-router-dispatch-descriptor-context-v0.lisp
```

## Report

- `Commit hash.`
- `Rendered command shape.`
- `Renderer invariants.`
- `Acceptance command results.`

