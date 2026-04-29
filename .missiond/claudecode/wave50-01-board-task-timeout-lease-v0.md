# wave50-01-board-task-timeout-lease-v0 — derive BoardTask claim lease from timeout_secs

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave50/wave50-01-board-task-timeout-lease-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave50-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `45`
- heartbeat_minutes: `10`
- shared_memory: `.missiond/tasks/wave50/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave50/reports/wave50-01-board-task-timeout-lease-v0.report.lisp`
- session_trace: `.missiond/tasks/wave50/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave50/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave50/pattern-cards.lisp`
- context_pack: `.missiond/tasks/wave50/context-pack.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave50/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave50/pattern-cards.lisp`.
- Read context-pack integration-plan before implementation: `.missiond/tasks/wave50/context-pack.lisp`.
- Treat accepted shards and mapped dispatch groups as the shard authority; do not re-derive architecture from older observations.
- Use declared context anchors and shard boundaries before falling back to broad scans.

## Goal

Make Autopilot BoardTask claim leases derive from BoardTask.timeout_secs instead of the current fixed 20-minute lease. Keep pty.send budget, watchdog threshold, and claim lease aligned through V3 Lisp/code isomorphism.

## Ownership

- `crates/missiond-daemon/src/engine/intent_engine/autopilot.rs`
- `.missiond/v3/missiond-blueprint.lisp`
- `scripts/check-v3-workstation-config-isomorphism.mjs`
- `.missiond/tasks/wave50/shared-memory.lisp`
- `.missiond/tasks/wave50/session-trace.lisp`
- `.missiond/tasks/wave50/reports/wave50-01-board-task-timeout-lease-v0.report.lisp`

## Must Not Touch

- `packages/**`
- `.missiond/v1/**`
- `.missiond/v2/**`
- `.missiond/tasks/wave48/**`
- `.missiond/tasks/wave49/**`
- `.missiond/tasks/wave50/manifest.lisp`
- `.missiond/tasks/wave50/wave50-*.lisp`
- `.missiond/tasks/wave50/context-atlas.lisp`
- `.missiond/tasks/wave50/pattern-cards.lisp`
- `.missiond/tasks/wave50/context-pack.lisp`
- `.missiond/claudecode/**`
- `scripts/check-context-pack.mjs`
- `scripts/context-pack-append.mjs`
- `scripts/context-pack-compile-shards.mjs`
- `scripts/check-v3-context-pack-isomorphism.mjs`

## Requirements

1. Read the shared preamble, this task contract, context atlas, pattern cards, and the wave50 context-pack integration-plan before broad scans.
2. Use scripts/context-pack-compile-shards.mjs .missiond/tasks/wave50/context-pack.lisp to confirm this is the accepted mapped shard.
3. Replace the fixed TimeDelta::minutes(20) BoardTask claim lease in dispatch_board_tasks with a timeout-derived helper.
4. Prefer deriving the lease from idle_watchdog_threshold_secs(timeout_secs), so explicit 3300s tasks receive a 3420s lease.
5. Add pure helper tests near existing pty_timeout / idle_watchdog tests.
6. Update .missiond/v3/missiond-blueprint.lisp and scripts/check-v3-workstation-config-isomorphism.mjs so the invariant is pinned.
7. Write the task report and commit only the declared write scope.

## Acceptance Commands

```bash
node scripts/check-v3-workstation-config-isomorphism.mjs --dry-fixture
node scripts/check-v3-workstation-config-isomorphism.mjs
node scripts/check-v3-code-isomorphism-complete.mjs
cargo check -p missiond-daemon
cargo test -p missiond-daemon engine::intent_engine::autopilot::tests -- --nocapture
node scripts/check-task-report.mjs .missiond/tasks/wave50/reports/wave50-01-board-task-timeout-lease-v0.report.lisp
git diff --check -- crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp scripts/check-v3-workstation-config-isomorphism.mjs .missiond/tasks/wave50/reports/wave50-01-board-task-timeout-lease-v0.report.lisp
```

## Shared Protocol

Read `.missiond/claudecode/wave50-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load declared context surfaces before broad repository search; use atlas/card anchors and the latest context-pack integration-plan to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs" \
        ".missiond/v3/missiond-blueprint.lisp" \
        "scripts/check-v3-workstation-config-isomorphism.mjs" \
        ".missiond/tasks/wave50/shared-memory.lisp" \
        ".missiond/tasks/wave50/session-trace.lisp" \
        ".missiond/tasks/wave50/reports/wave50-01-board-task-timeout-lease-v0.report.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave50/wave50-01-board-task-timeout-lease-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave50/wave50-01-board-task-timeout-lease-v0.lisp \
  git commit -m "fix(autopilot): derive board task lease from timeout"
node scripts/verify-task-contract.mjs .missiond/tasks/wave50/wave50-01-board-task-timeout-lease-v0.lisp
```

## Report

- `Commit hash.`
- `Helper name and exact timeout/lease semantics.`
- `Which V3 blueprint/checker invariant was updated.`
- `Acceptance command results.`

