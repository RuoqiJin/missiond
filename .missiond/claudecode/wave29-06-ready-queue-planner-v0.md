# wave29-06-ready-queue-planner-v0 — Ready queue planner v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave29/wave29-06-ready-queue-planner-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave29-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `B`
- estimated_minutes: `45`
- heartbeat_minutes: `10`
- depends_on: `wave29-01-context-atlas-schema-v0`, `wave29-02-pattern-card-schema-v0`
- shared_memory: `.missiond/tasks/wave29/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave29/reports/wave29-06-ready-queue-planner-v0.report.lisp`
- session_trace: `.missiond/tasks/wave29/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave29/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave29/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave29/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave29/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Evolve the read-only plan CLI from strict group-barrier batches to an additive ready-queue/phase-barrier planner. The new view should show which tasks can start as soon as dependency edges are satisfied, while preserving the current group-barrier output for backward compatibility.

## Ownership

- `scripts/plan-task-runner.mjs`
- `scripts/check-task-runner-manifest.mjs`
- `.missiond/tasks/schema/task-runner-manifest-v1.lisp`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/router/**`
- `.missiond/tasks/schema/task-contract-v1.lisp`
- `.missiond/tasks/schema/report-contract-v1.lisp`
- `.missiond/tasks/schema/context-atlas-v1.lisp`
- `.missiond/tasks/schema/pattern-card-v1.lisp`
- `.missiond/tasks/wave28/**`
- `.missiond/tasks/wave29/wave29-*.lisp`
- `.missiond/tasks/wave29/manifest.lisp`
- `.missiond/tasks/wave29/dispatch-plan.lisp`
- `.missiond/claudecode/**`
- `scripts/check-context-atlas.mjs`
- `scripts/check-pattern-card.mjs`
- `scripts/check-task-report.mjs`
- `scripts/verify-task-run.mjs`
- `scripts/verify-task-runner-batch.mjs`
- `scripts/prepare-task-runner-wave.mjs`
- `scripts/render-wave-briefs.mjs`

## Requirements

1. Keep existing group-barrier batches byte-compatible by default, or gate new output behind an explicit --schedule ready-queue flag. Do not break wave28 dry fixtures.
2. Add ready-queue output that releases a node when all dependency edges are satisfied, independent of unrelated long-running peers in the same dispatch group.
3. Priority should be deterministic and useful: critical-path or estimated-minutes first, then task id as tie breaker. Output should expose idle-window savings versus group-barrier where computable.
4. Preserve overlap safety: nodes with same dispatch_group write-scope overlap cannot be in the same ready window under reject policy.
5. Update task-runner-manifest schema docs/checker fixtures only as needed for additive planner metadata; do not change existing required node fields.
6. No dispatch, no spawn, no git mutation, no network, no LLM. Planner remains read-only.
7. Ensure source contains no raw NUL bytes so rg/grep keep treating it as text.

## Acceptance Commands

```bash
node scripts/plan-task-runner.mjs --dry-fixture
node scripts/check-task-runner-manifest.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
perl -ne 'exit 1 if /\x00/' scripts/plan-task-runner.mjs
git diff --check -- scripts/plan-task-runner.mjs scripts/check-task-runner-manifest.mjs .missiond/tasks/schema/task-runner-manifest-v1.lisp
```

## Shared Protocol

Read `.missiond/claudecode/wave29-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load the context atlas / pattern card before broad repository search; use their anchors to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add "scripts/plan-task-runner.mjs" \
        "scripts/check-task-runner-manifest.mjs" \
        ".missiond/tasks/schema/task-runner-manifest-v1.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave29/wave29-06-ready-queue-planner-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave29/wave29-06-ready-queue-planner-v0.lisp \
  git commit -m "feat(tasks): plan runner ready queue"
node scripts/verify-task-contract.mjs .missiond/tasks/wave29/wave29-06-ready-queue-planner-v0.lisp
```

## Report

- `Commit hash.`
- `Ready-queue output fields.`
- `Backward compatibility strategy.`
- `Acceptance command results.`

