# wave30-03-atomic-lifecycle-event-log-v0 — Atomic lifecycle event log v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave30/wave30-03-atomic-lifecycle-event-log-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave30-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `45`
- heartbeat_minutes: `10`
- report_contract: `.missiond/tasks/wave30/reports/wave30-03-atomic-lifecycle-event-log-v0.report.lisp`
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave30/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave30/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave30/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave30/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Replace direct max-seq + Edit writes for shared lifecycle facts with an orchestrator-owned append helper and schema. The new event log should be the future source for claims, trace starts, worker commits, parent hotfixes, finalization, receipts, and completions while projecting back to current shared-memory/session-trace files during migration.

## Ownership

- `.missiond/tasks/schema/task-lifecycle-event-v1.lisp`
- `scripts/task-runner-append-event.mjs`
- `scripts/check-task-lifecycle-events.mjs`
- `scripts/project-task-lifecycle-ledger.mjs`
- `scripts/prepare-task-runner-wave.mjs`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/research/**`
- `.missiond/router/**`
- `.missiond/tasks/schema/task-contract-v1.lisp`
- `.missiond/tasks/schema/report-contract-v1.lisp`
- `.missiond/tasks/schema/task-runner-manifest-v1.lisp`
- `.missiond/tasks/schema/task-runner-manifest-v2.lisp`
- `.missiond/tasks/schema/verification-receipt-v1.lisp`
- `.missiond/tasks/wave28/**`
- `.missiond/tasks/wave29/**`
- `.missiond/tasks/wave30/wave30-*.lisp`
- `.missiond/tasks/wave30/manifest.lisp`
- `.missiond/tasks/wave30/dispatch-plan.lisp`
- `.missiond/claudecode/**`
- `scripts/task-runner-finalize-report.mjs`
- `scripts/task-runner-parent-hotfix.mjs`
- `scripts/check-staged-source-hygiene.mjs`
- `scripts/check-task-report.mjs`
- `scripts/check-verification-receipt.mjs`
- `scripts/verify-task-runner-batch.mjs`
- `scripts/plan-task-runner.mjs`
- `scripts/check-task-runner-manifest.mjs`
- `scripts/render-wave-briefs.mjs`

## Requirements

1. Add task-lifecycle-event-v1.lisp documenting event kinds for claim, trace_start, read, worker_commit, parent_hotfix, finalized_report, receipt, completion, and issue.
2. Create check-task-lifecycle-events.mjs with --dry-fixture and named exports. Validate repo-relative paths, unique ids, monotonic seq, known event kinds, commit hash format, and task id shape.
3. Create task-runner-append-event.mjs as the single append helper. It should avoid hand-edited max-seq races as much as possible in a file-based implementation and clearly document concurrency limits.
4. Create project-task-lifecycle-ledger.mjs to project event logs into current shared-memory/session-trace compatible facts during migration.
5. Update prepare-task-runner-wave.mjs to use the append/projection helpers for bootstrap events when possible, while preserving existing CLI behavior and dry-fixture output shape unless explicitly versioned.

## Acceptance Commands

```bash
node scripts/check-task-lifecycle-events.mjs --dry-fixture
node scripts/task-runner-append-event.mjs --dry-fixture
node scripts/project-task-lifecycle-ledger.mjs --dry-fixture
node scripts/prepare-task-runner-wave.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
perl -ne 'exit 1 if /\x00/' scripts/task-runner-append-event.mjs scripts/check-task-lifecycle-events.mjs scripts/project-task-lifecycle-ledger.mjs scripts/prepare-task-runner-wave.mjs
git diff --check -- .missiond/tasks/schema/task-lifecycle-event-v1.lisp scripts/task-runner-append-event.mjs scripts/check-task-lifecycle-events.mjs scripts/project-task-lifecycle-ledger.mjs scripts/prepare-task-runner-wave.mjs
```

## Shared Protocol

Read `.missiond/claudecode/wave30-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load the context atlas / pattern card before broad repository search; use their anchors to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add ".missiond/tasks/schema/task-lifecycle-event-v1.lisp" \
        "scripts/task-runner-append-event.mjs" \
        "scripts/check-task-lifecycle-events.mjs" \
        "scripts/project-task-lifecycle-ledger.mjs" \
        "scripts/prepare-task-runner-wave.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave30/wave30-03-atomic-lifecycle-event-log-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave30/wave30-03-atomic-lifecycle-event-log-v0.lisp \
  git commit -m "feat(tasks): add lifecycle event log"
node scripts/verify-task-contract.mjs .missiond/tasks/wave30/wave30-03-atomic-lifecycle-event-log-v0.lisp
```

## Report

- `Commit hash.`
- `Event schema and kinds.`
- `Append helper concurrency boundary.`
- `Projection behavior for shared-memory/session-trace.`
- `Acceptance command results.`

