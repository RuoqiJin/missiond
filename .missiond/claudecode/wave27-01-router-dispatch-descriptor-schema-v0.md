# wave27-01-router-dispatch-descriptor-schema-v0 — Router dispatch descriptor schema v0

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave27/wave27-01-router-dispatch-descriptor-schema-v0.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave27-00-archive-wave26-artifacts`
- shared_memory: `.missiond/tasks/wave27/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave27/reports/wave27-01-router-dispatch-descriptor-schema-v0.report.lisp`
- session_trace: `.missiond/tasks/wave27/session-trace.lisp`
- session_trace_writable: `true`

## Goal

Introduce a machine-checkable router dispatch descriptor schema and checker. The descriptor is a handoff contract derived from router recommendation + backend readiness; it is not a runtime backend switch and must carry no-execution invariants explicitly.

## Ownership

- `.missiond/tasks/schema/router-dispatch-descriptor-v1.lisp`
- `scripts/check-router-dispatch-descriptor.mjs`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/router/**`
- `.missiond/tasks/wave26/**`
- `.missiond/tasks/wave27/wave27-*.lisp`
- `.missiond/claudecode/**`
- `scripts/recommend-task-backend.mjs`
- `scripts/evaluate-router-policy-corpus.mjs`
- `scripts/check-task-report.mjs`
- `scripts/render-claudecode-task.mjs`

## Requirements

1. Define schema missiond.router-dispatch-descriptor.v1 with a top-level descriptor form, not a policy form.
2. Required fields must include task_id, recommended_backend, router_confidence, backend_readiness_status, backend_runtime_allowed, router_apply_eligible, router_apply_blockers, dry_run_only, runtime_replacement, no_execution, source_recommendation_schema, source_policy_path, and source_backend_registry_path.
3. dry_run_only must be literal true; runtime_replacement must be literal false; no_execution must be literal true. Reject strings like "true" and "false".
4. router_apply_eligible=true is allowed only when backend_readiness_status is runtime-ready, backend_runtime_allowed is true, router_confidence is high, and router_apply_blockers is empty.
5. current-default must be accepted as a readiness status but rejected for router_apply_eligible=true.
6. Checker must support --json, --stdin, and --dry-fixture, use the shared missiond_lisp parser, and never shell out / call git / call network / call LLM.
7. Add fixtures for valid eligible runtime-ready, valid current-default blocked, string bool rejected, missing no_execution rejected, runtime_replacement=true rejected, current-default eligible rejected, unknown backend rejected, absolute path rejected, and malformed blocker vector rejected.

## Acceptance Commands

```bash
node scripts/check-router-dispatch-descriptor.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
git diff --check -- .missiond/tasks/schema/router-dispatch-descriptor-v1.lisp scripts/check-router-dispatch-descriptor.mjs
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

Expected machine-readable report: `.missiond/tasks/wave27/reports/wave27-01-router-dispatch-descriptor-schema-v0.report.lisp` (schema `missiond.report-contract.v1`).

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
node scripts/check-task-report.mjs .missiond/tasks/wave27/reports/wave27-01-router-dispatch-descriptor-schema-v0.report.lisp
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
node scripts/recommend-task-backend.mjs --task .missiond/tasks/wave27/wave27-01-router-dispatch-descriptor-schema-v0.lisp --policy .missiond/router/router-policy-v1.lisp --backend-registry .missiond/router/router-backend-registry-v1.lisp --json
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
git add ".missiond/tasks/schema/router-dispatch-descriptor-v1.lisp" \
        "scripts/check-router-dispatch-descriptor.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave27/wave27-01-router-dispatch-descriptor-schema-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave27/wave27-01-router-dispatch-descriptor-schema-v0.lisp \
  git commit -m "feat(router): add dispatch descriptor schema"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `node scripts/install-missiond-hooks.mjs --install`, equivalent to `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave27/wave27-01-router-dispatch-descriptor-schema-v0.lisp
```

## Report

- `Commit hash.`
- `Descriptor schema required fields.`
- `Fixture count and rejection categories.`
- `Acceptance command results.`

