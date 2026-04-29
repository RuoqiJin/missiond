# wave48-01-context-autopilot-restart-recovery-v0 — context-pack investigation: autopilot dynamic slot restart recovery

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave48/wave48-01-context-autopilot-restart-recovery-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave48-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `35`
- heartbeat_minutes: `10`
- shared_memory: `.missiond/tasks/wave48/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave48/reports/wave48-01-context-autopilot-restart-recovery-v0.report.lisp`
- session_trace: `.missiond/tasks/wave48/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave48/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave48/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave48/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave48/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Read-only investigation for the next implementation shard: analyze why a delegated BoardTask assigned to a dynamic slot can fail after daemon restart, then append concrete observations and at least one shard-proposal to .missiond/tasks/wave48/context-pack.lisp using scripts/context-pack-append.mjs. Do not edit Rust/JS implementation files in this task.

## Ownership

- `.missiond/tasks/wave48/context-pack.lisp`
- `.missiond/tasks/wave48/shared-memory.lisp`
- `.missiond/tasks/wave48/session-trace.lisp`
- `.missiond/tasks/wave48/reports/wave48-01-context-autopilot-restart-recovery-v0.report.lisp`

## Must Not Touch

- `crates/**`
- `scripts/**`
- `packages/**`
- `.missiond/v1/**`
- `.missiond/v2/**`
- `.missiond/v3/**`
- `.missiond/research/**`
- `.missiond/tasks/wave28/**`
- `.missiond/tasks/wave29/**`
- `.missiond/tasks/wave30/**`
- `.missiond/tasks/wave31/**`
- `.missiond/tasks/wave32/**`
- `.missiond/tasks/wave33/**`
- `.missiond/tasks/wave34/**`
- `.missiond/tasks/wave35/**`
- `.missiond/tasks/wave36/**`
- `.missiond/tasks/wave37/**`
- `.missiond/tasks/wave38/**`
- `.missiond/tasks/wave39/**`
- `.missiond/tasks/wave40/**`
- `.missiond/tasks/wave41/**`
- `.missiond/tasks/wave42/**`
- `.missiond/tasks/wave43/**`
- `.missiond/tasks/wave44/**`
- `.missiond/tasks/wave45/**`
- `.missiond/tasks/wave46/**`
- `.missiond/tasks/wave47/**`
- `.missiond/tasks/wave48/manifest.lisp`
- `.missiond/tasks/wave48/wave48-*.lisp`
- `.missiond/tasks/wave48/context-atlas.lisp`
- `.missiond/tasks/wave48/pattern-cards.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Read the shared preamble, this task contract, manifest, context atlas, and pattern cards before broad search.
2. Inspect only: crates/missiond-daemon/src/engine/intent_engine/autopilot.rs, crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs, crates/missiond-daemon/src/handlers/compute/task_delegate.rs, crates/missiond-daemon/src/handlers/compute/compute_slot.rs, and the V3 workstation-config/context-pack blueprint sections.
3. Use scripts/context-pack-append.mjs to append at least two observations to .missiond/tasks/wave48/context-pack.lisp. Each observation must cite concrete files and explain the restart failure path.
4. Use scripts/context-pack-append.mjs to append at least one shard-proposal. The proposal must identify a single code owner, a bounded write-scope, must-not-touch scope, and acceptance commands for implementing dynamic slot restart recovery.
5. Do not edit Rust, JS, package, or blueprint files in this task. This is an investigation/proposal shard only.
6. Write the task report and commit only the declared write scope.

## Acceptance Commands

```bash
node scripts/check-context-pack.mjs .missiond/tasks/wave48/context-pack.lisp
node scripts/check-task-report.mjs .missiond/tasks/wave48/reports/wave48-01-context-autopilot-restart-recovery-v0.report.lisp
git diff --check -- .missiond/tasks/wave48/context-pack.lisp .missiond/tasks/wave48/reports/wave48-01-context-autopilot-restart-recovery-v0.report.lisp
```

## Shared Protocol

Read `.missiond/claudecode/wave48-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load the context atlas / pattern card before broad repository search; use their anchors to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add ".missiond/tasks/wave48/context-pack.lisp" \
        ".missiond/tasks/wave48/shared-memory.lisp" \
        ".missiond/tasks/wave48/session-trace.lisp" \
        ".missiond/tasks/wave48/reports/wave48-01-context-autopilot-restart-recovery-v0.report.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave48/wave48-01-context-autopilot-restart-recovery-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave48/wave48-01-context-autopilot-restart-recovery-v0.lisp \
  git commit -m "chore(tasks): record wave48 autopilot recovery context"
node scripts/verify-task-contract.mjs .missiond/tasks/wave48/wave48-01-context-autopilot-restart-recovery-v0.lisp
```

## Report

- `Commit hash.`
- `Which dynamic-slot restart failure path was identified.`
- `Context-pack entries appended, including shard-proposal id.`
- `Recommended implementation write-scope and acceptance commands.`
- `Acceptance command results.`

