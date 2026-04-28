# wave28-06-task-runner-loop-smoke-v0 — Task runner loop smoke v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave28/wave28-06-task-runner-loop-smoke-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave28-shared-preamble.md`

## Task Contract

- kind: `smoke`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `full`
- dispatch_group: `D`
- estimated_minutes: `45`
- heartbeat_minutes: `10`
- depends_on: `wave28-02-task-runner-plan-cli-v0`, `wave28-03-wave-brief-batch-renderer-v0`, `wave28-04-mission-plan-task-runner-dry-run-surface-v0`, `wave28-05-task-runner-batch-verifier-v0`
- shared_memory: `.missiond/tasks/wave28/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave28/reports/wave28-06-task-runner-loop-smoke-v0.report.lisp`
- session_trace: `.missiond/tasks/wave28/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)

## Goal

Add a cross-layer smoke suite for task-contract runner v0. The smoke should prove manifest checker, runner-plan CLI, wave brief renderer, daemon dry-run surface, and batch verifier agree on the same productive-only wave semantics.

## Ownership

- `scripts/check-task-runner-manifest.mjs`
- `scripts/plan-task-runner.mjs`
- `scripts/render-wave-briefs.mjs`
- `scripts/verify-task-runner-batch.mjs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-daemon/src/handlers/knowledge/directive.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`
- `.missiond/v2/**`
- `.missiond/tasks/wave27/**`
- `.missiond/tasks/wave28/wave28-*.lisp`
- `.missiond/tasks/wave28/dispatch-plan.lisp`
- `.missiond/claudecode/**`
- `scripts/check-task-contract.mjs`
- `scripts/render-claudecode-task.mjs`
- `scripts/verify-task-run.mjs`

## Requirements

1. Add smoke fixtures or tests that pin the same manifest through checker -> plan CLI -> render-wave-briefs -> mission_plan dry-run -> batch verifier.
2. Pin productive-only behavior: archive/backfill/index are not worker nodes and are absent from thin brief generation and batch verification.
3. Pin verification-tier behavior: local tasks do not require full cargo; full tier appears only on final smoke/final nodes.
4. Pin heartbeat metadata propagation into thin brief or shared preamble guidance.
5. Pin no execution: task-runner dry-run does not spawn, does not call Node, does not mutate git, and returns applied=false.
6. Keep the smoke deterministic and no LLM/no network.

## Acceptance Commands

```bash
node scripts/check-task-runner-manifest.mjs --dry-fixture
node scripts/plan-task-runner.mjs --dry-fixture
node scripts/render-wave-briefs.mjs --dry-fixture
node scripts/verify-task-runner-batch.mjs --dry-fixture
cargo test -p missiond-daemon task_runner --lib
cargo test -p missiond-daemon --lib
cargo build --workspace
node scripts/check-task-contract.mjs --all
git diff --check -- scripts/check-task-runner-manifest.mjs scripts/plan-task-runner.mjs scripts/render-wave-briefs.mjs scripts/verify-task-runner-batch.mjs crates/missiond-daemon/src/handlers/knowledge/plan.rs
```

## Shared Protocol

Read `.missiond/claudecode/wave28-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add "scripts/check-task-runner-manifest.mjs" \
        "scripts/plan-task-runner.mjs" \
        "scripts/render-wave-briefs.mjs" \
        "scripts/verify-task-runner-batch.mjs" \
        "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave28/wave28-06-task-runner-loop-smoke-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave28/wave28-06-task-runner-loop-smoke-v0.lisp \
  git commit -m "test(tasks): smoke task runner loop"
node scripts/verify-task-contract.mjs .missiond/tasks/wave28/wave28-06-task-runner-loop-smoke-v0.lisp
```

## Report

- `Commit hash.`
- `Smoke layers pinned.`
- `Productive-only and no-execution proof.`
- `Acceptance command results.`

