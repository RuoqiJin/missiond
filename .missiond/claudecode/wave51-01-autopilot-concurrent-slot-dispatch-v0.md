# wave51-01-autopilot-concurrent-slot-dispatch-v0 — start Autopilot pty sends concurrently across slots

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave51/wave51-01-autopilot-concurrent-slot-dispatch-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave51-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `55`
- heartbeat_minutes: `10`
- report_contract: `.missiond/tasks/wave51/reports/wave51-01-autopilot-concurrent-slot-dispatch-v0.report.lisp`
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave51/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave51/pattern-cards.lisp`
- context_pack: `.missiond/tasks/wave51/context-pack.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave51/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave51/pattern-cards.lisp`.
- Read context-pack integration-plan before implementation: `.missiond/tasks/wave51/context-pack.lisp`.
- Treat accepted shards and mapped dispatch groups as the shard authority; do not re-derive architecture from older observations.
- Use declared context anchors and shard boundaries before falling back to broad scans.

## Goal

Make Autopilot BoardTask dispatch start pty.send work concurrently across different slots in the same dispatch tick. Preserve same-slot exclusion by holding the per-slot dispatch guard across each send.

## Ownership

- `crates/missiond-daemon/src/engine/intent_engine/autopilot.rs`
- `.missiond/v3/missiond-blueprint.lisp`
- `scripts/check-v3-workstation-config-isomorphism.mjs`
- `.missiond/tasks/wave51/shared-memory.lisp`
- `.missiond/tasks/wave51/session-trace.lisp`
- `.missiond/tasks/wave51/reports/wave51-01-autopilot-concurrent-slot-dispatch-v0.report.lisp`

## Must Not Touch

- `packages/**`
- `.missiond/v1/**`
- `.missiond/v2/**`
- `.missiond/tasks/wave48/**`
- `.missiond/tasks/wave49/**`
- `.missiond/tasks/wave50/**`
- `.missiond/tasks/wave51/manifest.lisp`
- `.missiond/tasks/wave51/wave51-*.lisp`
- `.missiond/tasks/wave51/context-atlas.lisp`
- `.missiond/tasks/wave51/pattern-cards.lisp`
- `.missiond/tasks/wave51/context-pack.lisp`
- `.missiond/claudecode/**`
- `scripts/check-context-pack.mjs`
- `scripts/context-pack-append.mjs`
- `scripts/context-pack-compile-shards.mjs`
- `scripts/check-v3-context-pack-isomorphism.mjs`

## Requirements

1. Read the shared preamble, this task contract, context atlas, pattern cards, and the wave51 context-pack integration-plan before broad scans.
2. Use scripts/context-pack-compile-shards.mjs .missiond/tasks/wave51/context-pack.lisp to confirm this is the accepted mapped shard.
3. Fix dispatch_board_tasks so it does not await one slot's state.pty.send before starting sends for other ready tasks assigned to other slots in the same tick.
4. Preserve the per-slot dispatch guard across each individual state.pty.send call; same-slot work must remain exclusive.
5. Keep the existing close-owner behavior, auth/quota/failure paths, KB confidence feedback, deploy post-mortem trigger, prompt snapshot, and dispatch event semantics unless there is a compile-driven reason to factor them.
6. Add a focused regression guard near autopilot tests, preferably source/pure-level if a full AppState integration test would be too heavy.
7. Update .missiond/v3/missiond-blueprint.lisp and scripts/check-v3-workstation-config-isomorphism.mjs so the invariant is pinned.
8. Write the task report and commit only the declared write scope.

## Acceptance Commands

```bash
node scripts/check-v3-workstation-config-isomorphism.mjs --dry-fixture
node scripts/check-v3-workstation-config-isomorphism.mjs
node scripts/check-v3-code-isomorphism-complete.mjs
cargo check -p missiond-daemon
cargo test -p missiond-daemon engine::intent_engine::autopilot::tests -- --nocapture
node scripts/check-task-report.mjs .missiond/tasks/wave51/reports/wave51-01-autopilot-concurrent-slot-dispatch-v0.report.lisp
git diff --check -- crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp scripts/check-v3-workstation-config-isomorphism.mjs .missiond/tasks/wave51/reports/wave51-01-autopilot-concurrent-slot-dispatch-v0.report.lisp
```

## Shared Protocol

Read `.missiond/claudecode/wave51-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
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
        ".missiond/tasks/wave51/shared-memory.lisp" \
        ".missiond/tasks/wave51/session-trace.lisp" \
        ".missiond/tasks/wave51/reports/wave51-01-autopilot-concurrent-slot-dispatch-v0.report.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave51/wave51-01-autopilot-concurrent-slot-dispatch-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave51/wave51-01-autopilot-concurrent-slot-dispatch-v0.lisp \
  git commit -m "fix(autopilot): dispatch board tasks concurrently across slots"
node scripts/verify-task-contract.mjs .missiond/tasks/wave51/wave51-01-autopilot-concurrent-slot-dispatch-v0.lisp
```

## Report

- `Commit hash.`
- `Exact concurrency structure used and how the per-slot guard is held.`
- `Which V3 blueprint/checker invariant was updated.`
- `Acceptance command results.`

