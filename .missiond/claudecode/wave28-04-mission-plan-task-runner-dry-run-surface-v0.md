# wave28-04-mission-plan-task-runner-dry-run-surface-v0 — mission_plan task-runner dry-run surface v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave28/wave28-04-mission-plan-task-runner-dry-run-surface-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave28-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `smoke`
- dispatch_group: `C`
- estimated_minutes: `60`
- heartbeat_minutes: `10`
- depends_on: `wave28-01-task-runner-manifest-schema-v0`, `wave28-02-task-runner-plan-cli-v0`
- shared_memory: `.missiond/tasks/wave28/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave28/reports/wave28-04-mission-plan-task-runner-dry-run-surface-v0.report.lisp`
- session_trace: `.missiond/tasks/wave28/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)

## Goal

Expose a daemon/MCP dry-run surface for task-runner manifests through mission_plan execute. The surface should read a manifest and return deterministic runner-plan facts, but it must not spawn workers, mutate git, or execute task contracts.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-daemon/src/handlers/knowledge/directive.rs`
- `.missiond/v2/**`
- `.missiond/tasks/**`
- `.missiond/claudecode/**`
- `scripts/**`

## Requirements

1. Add optional mission_plan execute args task_runner_manifest_path and task_runner_mode, where only absent/off/dry_run are accepted in v0; apply/auto/unknown must reject before plan lookup.
2. Dry-run response must surface manifest_status, wave, productive_only, batches, critical_path_minutes, total_estimated_minutes, verification_tier_counts, overlap_diagnostics, and applied=false literal.
3. Do not spawn, do not call Node, do not call shell, do not mutate git, do not dispatch mission_task_delegate. Implement a small in-Rust reader/projector or a narrow parser sufficient for manifest facts.
4. Absent/off mode must be byte-identical to the current baseline and must not read task_runner_manifest_path even if supplied.
5. Malformed/missing manifest should be non-fatal in dry_run and return manifest_status plus warning fields, not panic.
6. MCP schema description must state dry-run only and no execution.

## Acceptance Commands

```bash
cargo test -p missiond-daemon task_runner --lib
cargo test -p missiond-daemon --lib
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-task-contract.mjs --all
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs
```

## Shared Protocol

Read `.missiond/claudecode/wave28-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/plan.rs" \
        "crates/missiond-mcp/src/tools/knowledge/plan.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave28/wave28-04-mission-plan-task-runner-dry-run-surface-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave28/wave28-04-mission-plan-task-runner-dry-run-surface-v0.lisp \
  git commit -m "feat(plan): surface task runner manifest dry-run"
node scripts/verify-task-contract.mjs .missiond/tasks/wave28/wave28-04-mission-plan-task-runner-dry-run-surface-v0.lisp
```

## Report

- `Commit hash.`
- `New args and response fields.`
- `Byte-compat/no-I/O proof for off/default.`
- `Acceptance command results.`

