# wave27-03-plan-router-dispatch-descriptor-surface-v0 — mission_plan router dispatch descriptor surface v0

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave27/wave27-03-plan-router-dispatch-descriptor-surface-v0.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave27-01-router-dispatch-descriptor-schema-v0`
- shared_memory: `.missiond/tasks/wave27/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave27/reports/wave27-03-plan-router-dispatch-descriptor-surface-v0.report.lisp`
- session_trace: `.missiond/tasks/wave27/session-trace.lisp`
- session_trace_writable: `true`

## Goal

Expose a router dispatch descriptor block from mission_plan execute dry-run mode using the existing Rust router recommendation/readiness path. This is a response surface only; dispatch must remain unchanged.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

## Must Not Touch

- `scripts/**`
- `.missiond/v2/**`
- `.missiond/router/**`
- `.missiond/tasks/schema/**`
- `.missiond/tasks/wave26/**`
- `.missiond/tasks/wave27/wave27-*.lisp`
- `.missiond/claudecode/**`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`

## Requirements

1. Add an optional mission_plan execute argument such as router_dispatch_descriptor=true; choose the smallest naming consistent with existing router_policy_* args and document it in the MCP schema.
2. Only honor the argument when router_policy_mode=dry_run. absent/off/default must preserve the old response shape and perform no additional file I/O.
3. Build the descriptor from the existing Rust dry-run recommendation + backend readiness fields; do not spawn Node and do not shell out.
4. Descriptor fields must match wave27-01 schema names where practical: recommended_backend, router_confidence, backend_readiness_status, backend_runtime_allowed, router_apply_eligible, router_apply_blockers, dry_run_only=true, runtime_replacement=false, no_execution=true.
5. If router_backend_registry_path is absent, descriptor should either be omitted or return a structured descriptor_status explaining registry_missing; do not fake readiness.
6. applied=false remains literal; dispatch_strategy / target_project / requested_cwd must remain unchanged.
7. Add tests proving off/default with descriptor flag does no I/O and remains byte-identical; dry_run with seed registry emits no_execution=true and router_apply_eligible=false for current-default.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-task-contract.mjs --all
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs
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

Expected machine-readable report: `.missiond/tasks/wave27/reports/wave27-03-plan-router-dispatch-descriptor-surface-v0.report.lisp` (schema `missiond.report-contract.v1`).

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
node scripts/check-task-report.mjs .missiond/tasks/wave27/reports/wave27-03-plan-router-dispatch-descriptor-surface-v0.report.lisp
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
node scripts/recommend-task-backend.mjs --task .missiond/tasks/wave27/wave27-03-plan-router-dispatch-descriptor-surface-v0.lisp --policy .missiond/router/router-policy-v1.lisp --backend-registry .missiond/router/router-backend-registry-v1.lisp --json
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
git add "crates/missiond-daemon/src/handlers/knowledge/plan.rs" \
        "crates/missiond-mcp/src/tools/knowledge/plan.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave27/wave27-03-plan-router-dispatch-descriptor-surface-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave27/wave27-03-plan-router-dispatch-descriptor-surface-v0.lisp \
  git commit -m "feat(plan): surface router dispatch descriptors"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `node scripts/install-missiond-hooks.mjs --install`, equivalent to `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave27/wave27-03-plan-router-dispatch-descriptor-surface-v0.lisp
```

## Report

- `Commit hash.`
- `Argument name and response fields.`
- `Byte-compat/off-mode tests.`
- `Acceptance command results.`

