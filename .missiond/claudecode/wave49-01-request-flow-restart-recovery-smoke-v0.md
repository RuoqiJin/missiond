# wave49-01-request-flow-restart-recovery-smoke-v0 — implement request-flow restart recovery smoke

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave49/wave49-01-request-flow-restart-recovery-smoke-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave49-shared-preamble.md`

## Task Contract

- kind: `test-fix`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `45`
- heartbeat_minutes: `10`
- report_contract: `.missiond/tasks/wave49/reports/wave49-01-request-flow-restart-recovery-smoke-v0.report.lisp`
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave49/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave49/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave49/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave49/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Implement the accepted wave48 recovery-smoke shard in scripts/check-v3-request-flow-smoke.mjs. Add an opt-in --restart-during-dispatch mode that is valid only with --live-ipc --execute-real-dispatch, plus dry-fixture coverage that proves the parser/planner refuses unsafe combinations and preserves existing default behavior. Do not run a live daemon restart unless the parent explicitly asks after review.

## Ownership

- `scripts/check-v3-request-flow-smoke.mjs`
- `.missiond/tasks/wave49/shared-memory.lisp`
- `.missiond/tasks/wave49/session-trace.lisp`
- `.missiond/tasks/wave49/reports/wave49-01-request-flow-restart-recovery-smoke-v0.report.lisp`

## Must Not Touch

- `crates/**`
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
- `.missiond/tasks/wave48/**`
- `.missiond/tasks/wave49/manifest.lisp`
- `.missiond/tasks/wave49/wave49-*.lisp`
- `.missiond/tasks/wave49/context-atlas.lisp`
- `.missiond/tasks/wave49/pattern-cards.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Read the shared preamble, this task contract, context atlas, pattern cards, and wave48 context-pack integration-plan first.
2. Preserve default behavior: without --live-ipc the smoke stays read-only; with --live-ipc but without --execute-real-dispatch it still stops before dispatch.
3. Add a CLI flag --restart-during-dispatch that errors unless both --live-ipc and --execute-real-dispatch are present.
4. Implement the restart-recovery smoke as an explicit opt-in path. It should be structured so parent/Codex can review the steps before running it against a live daemon.
5. Dry-fixture coverage must include safe default behavior, invalid flag combinations, and a planned restart-recovery step sequence.
6. Keep the implementation localized to scripts/check-v3-request-flow-smoke.mjs; do not edit Rust, V3 blueprint/checkers, package files, or wave48 artifacts.
7. Write the task report and commit only the declared write scope.

## Acceptance Commands

```bash
node scripts/check-v3-request-flow-smoke.mjs --dry-fixture
node scripts/check-v3-request-flow-smoke.mjs
node scripts/check-v3-code-isomorphism-complete.mjs
node scripts/check-task-report.mjs .missiond/tasks/wave49/reports/wave49-01-request-flow-restart-recovery-smoke-v0.report.lisp
git diff --check -- scripts/check-v3-request-flow-smoke.mjs .missiond/tasks/wave49/reports/wave49-01-request-flow-restart-recovery-smoke-v0.report.lisp
```

## Shared Protocol

Read `.missiond/claudecode/wave49-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load the context atlas / pattern card before broad repository search; use their anchors to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add "scripts/check-v3-request-flow-smoke.mjs" \
        ".missiond/tasks/wave49/shared-memory.lisp" \
        ".missiond/tasks/wave49/session-trace.lisp" \
        ".missiond/tasks/wave49/reports/wave49-01-request-flow-restart-recovery-smoke-v0.report.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave49/wave49-01-request-flow-restart-recovery-smoke-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave49/wave49-01-request-flow-restart-recovery-smoke-v0.lisp \
  git commit -m "test(v3): add restart recovery dispatch smoke"
node scripts/verify-task-contract.mjs .missiond/tasks/wave49/wave49-01-request-flow-restart-recovery-smoke-v0.lisp
```

## Report

- `Commit hash.`
- `Exactly what --restart-during-dispatch does and what remains parent-run/live-only.`
- `Dry-fixture cases added.`
- `Backward compatibility evidence for default and --live-ipc non-dispatch modes.`
- `Acceptance command results.`

