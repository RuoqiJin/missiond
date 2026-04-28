# wave34-01-autopilot-complete-close-v0 — Autopilot delegated-task completion ownership v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave34/wave34-01-autopilot-complete-close-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave34-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `40`
- heartbeat_minutes: `10`
- shared_memory: `.missiond/tasks/wave34/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave34/reports/wave34-01-autopilot-complete-close-v0.report.lisp`
- session_trace: `.missiond/tasks/wave34/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave34/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave34/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave34/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave34/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Fix the delegated BoardTask execution ownership gap observed in wave33: dynamic slot provisioning must not send a fire-and-forget task objective before Autopilot sends the real BoardTask prompt, and Autopilot must remain the close owner after pty.send returns Complete unless the worker already self-closed or blocked the task.

## Ownership

- `crates/missiond-daemon/src/handlers/compute/task_delegate.rs`
- `crates/missiond-daemon/src/handlers/compute/compute_slot.rs`
- `crates/missiond-daemon/src/engine/intent_engine/autopilot.rs`
- `.missiond/v3/missiond-blueprint.lisp`

## Must Not Touch

- `crates/missiond-daemon/src/slot_orchestrator/spawner.rs`
- `crates/missiond-daemon/src/slot_dispatch.rs`
- `crates/missiond-daemon/src/handlers/compute/pty.rs`
- `crates/missiond-core/**`
- `crates/missiond-mcp/**`
- `crates/missiond-pty/**`
- `scripts/**`
- `.missiond/v1/**`
- `.missiond/v2/**`
- `.missiond/research/**`
- `.missiond/tasks/schema/**`
- `.missiond/tasks/wave31/**`
- `.missiond/tasks/wave32/**`
- `.missiond/tasks/wave33/**`
- `.missiond/tasks/wave34/manifest.lisp`
- `.missiond/tasks/wave34/dispatch-plan.lisp`
- `.missiond/tasks/wave34/context-atlas.lisp`
- `.missiond/tasks/wave34/pattern-cards.lisp`
- `.missiond/tasks/wave34/wave34-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Add a compact execution-ownership rule under .missiond/v3/missiond-blueprint.lisp workstation-config. It must state: for delegated BoardTask execution, Autopilot is the task prompt owner; compute_slot/spawner may provision and warm a slot but must not send the task objective as a fire-and-forget execution prompt; Autopilot is the close owner unless board MCP tools are attached and the worker self-closes or the task becomes blocked by a question.
2. Update the workstation-config implementation-map note so it names the Rust projection points in task_delegate, compute_slot, and autopilot.
3. In compute_slot.rs, add an explicit create-time option or small helper for initial prompt ownership. Direct mission_compute_slot create should remain compatible by default, but task_delegate auto-provisioning must be able to suppress PTYSpawnOptions.initial_prompt.
4. In task_delegate.rs, pass the new option when auto-provisioning a slot for a BoardTask so the dynamic slot starts idle and the queued BoardTask is the only task prompt Autopilot sends.
5. In autopilot.rs, close the release-before-send race around the per-slot dispatch guard. The guard is per-slot, so it may be held until state.pty.send returns; keep existing behavior that preserves Done self-close and Blocked question states.
6. Keep wave32 timeout-budget projection and wave33 prompt/tool wording intact.
7. Add focused tests for the new compute_slot/task_delegate initial-prompt option or helper, and for any Autopilot guard helper if extracted. Avoid constructing AppState in tests.

## Acceptance Commands

```bash
cargo test -p missiond-daemon engine::intent_engine::autopilot::tests
cargo test -p missiond-daemon handlers::compute::compute_slot::tests
cargo check -p missiond-daemon
node scripts/check-lisp-blueprint-compression.mjs
node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp
perl -ne 'exit 1 if /\x00/' crates/missiond-daemon/src/handlers/compute/task_delegate.rs crates/missiond-daemon/src/handlers/compute/compute_slot.rs crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp
git diff --check -- crates/missiond-daemon/src/handlers/compute/task_delegate.rs crates/missiond-daemon/src/handlers/compute/compute_slot.rs crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp
```

## Shared Protocol

Read `.missiond/claudecode/wave34-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load the context atlas / pattern card before broad repository search; use their anchors to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add "crates/missiond-daemon/src/handlers/compute/task_delegate.rs" \
        "crates/missiond-daemon/src/handlers/compute/compute_slot.rs" \
        "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs" \
        ".missiond/v3/missiond-blueprint.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave34/wave34-01-autopilot-complete-close-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave34/wave34-01-autopilot-complete-close-v0.lisp \
  git commit -m "fix(autopilot): close delegated task completion loop"
node scripts/verify-task-contract.mjs .missiond/tasks/wave34/wave34-01-autopilot-complete-close-v0.lisp
```

## Report

- `Commit hash.`
- `V3 execution-ownership rule added.`
- `How task_delegate suppresses compute_slot initial_prompt for delegated BoardTasks.`
- `How Autopilot keeps prompt/close ownership and preserves self-close or blocked states.`
- `Acceptance command results.`

