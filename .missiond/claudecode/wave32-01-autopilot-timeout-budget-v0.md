# wave32-01-autopilot-timeout-budget-v0 — Autopilot PTY timeout budget alignment v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave32/wave32-01-autopilot-timeout-budget-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave32-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `35`
- heartbeat_minutes: `10`
- shared_memory: `.missiond/tasks/wave32/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave32/reports/wave32-01-autopilot-timeout-budget-v0.report.lisp`
- session_trace: `.missiond/tasks/wave32/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave32/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave32/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave32/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave32/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Fix the wave31 stability issue where Autopilot sent a ClaudeCode task with a fixed 10 minute pty.send timeout even though mission_task_delegate had already stored a longer task timeout_secs on the board task.

## Ownership

- `crates/missiond-daemon/src/engine/intent_engine/autopilot.rs`
- `.missiond/v3/missiond-blueprint.lisp`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/compute/task_delegate.rs`
- `crates/missiond-core/src/types/board.rs`
- `crates/missiond-core/src/db/pg/board.rs`
- `crates/missiond-pty/**`
- `crates/missiond-mcp/**`
- `scripts/**`
- `.missiond/v1/**`
- `.missiond/v2/**`
- `.missiond/research/**`
- `.missiond/tasks/schema/**`
- `.missiond/tasks/wave31/**`
- `.missiond/tasks/wave32/manifest.lisp`
- `.missiond/tasks/wave32/dispatch-plan.lisp`
- `.missiond/tasks/wave32/context-atlas.lisp`
- `.missiond/tasks/wave32/pattern-cards.lisp`
- `.missiond/tasks/wave32/wave32-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Replace Autopilot's fixed `let timeout_ms = 600_000` PTY send budget with a helper derived from BoardTask.timeout_secs. If timeout_secs is absent or invalid, use the existing task_delegate default of 1800 seconds. Clamp to a sane 60..7200 second range before converting to milliseconds.
2. Update the smart watchdog that currently unclaims idle running tasks after claimed_age > 120s. It must wait until the task timeout plus a small grace window has elapsed before treating an idle slot as orphaned. This prevents long-running Opus coding tasks from being re-dispatched while their original pty.send is still within the declared task timeout.
3. Keep the no-PTY-session branch recoverable without waiting for the full timeout, because a missing session is different from an idle session that may still be returning a result.
4. Improve watchdog note/log wording so it says the task exceeded its configured timeout/grace, not only that daemon restart may have lost send().
5. Add focused pure unit tests in autopilot.rs for timeout derivation and watchdog threshold behavior. Do not construct AppState in tests.
6. Update .missiond/v3/missiond-blueprint.lisp workstation-config invariants or implementation-map note to record that Autopilot wait budget and watchdog recovery are Lisp/task-timeout projected policy, not hardcoded runtime constants.

## Acceptance Commands

```bash
cargo test -p missiond-daemon engine::intent_engine::autopilot::tests
cargo check -p missiond-daemon
node scripts/check-lisp-blueprint-compression.mjs
node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp
perl -ne 'exit 1 if /\x00/' crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp
git diff --check -- crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp
```

## Shared Protocol

Read `.missiond/claudecode/wave32-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load the context atlas / pattern card before broad repository search; use their anchors to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs" \
        ".missiond/v3/missiond-blueprint.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave32/wave32-01-autopilot-timeout-budget-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave32/wave32-01-autopilot-timeout-budget-v0.lisp \
  git commit -m "fix(autopilot): honor task timeout budget"
node scripts/verify-task-contract.mjs .missiond/tasks/wave32/wave32-01-autopilot-timeout-budget-v0.lisp
```

## Report

- `Commit hash.`
- `Timeout derivation policy.`
- `Watchdog recovery threshold policy.`
- `Whether blueprint invariant/note changed.`
- `Acceptance command results.`

