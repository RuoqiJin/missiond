# wave29-07-runner-efficiency-smoke-v1 — Runner efficiency smoke v1

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave29/wave29-07-runner-efficiency-smoke-v1.lisp`
> Shared preamble: `.missiond/claudecode/wave29-shared-preamble.md`

## Task Contract

- kind: `smoke`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `smoke`
- dispatch_group: `C`
- estimated_minutes: `45`
- heartbeat_minutes: `10`
- depends_on: `wave29-03-runner-wave-prep-v0`, `wave29-04-parent-hotfix-lineage-v1`, `wave29-05-verification-receipt-schema-v0`, `wave29-06-ready-queue-planner-v0`
- shared_memory: `.missiond/tasks/wave29/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave29/reports/wave29-07-runner-efficiency-smoke-v1.report.lisp`
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

Add a cross-layer smoke suite for runner-efficiency v1. It should prove context atlas, pattern cards, wave preparation, parent-hotfix lineage, verification receipts, ready-queue planning, and batch verification agree on a single productive-only synthetic wave.

## Ownership

- `scripts/check-context-atlas.mjs`
- `scripts/check-pattern-card.mjs`
- `scripts/prepare-task-runner-wave.mjs`
- `scripts/check-task-report.mjs`
- `scripts/check-verification-receipt.mjs`
- `scripts/plan-task-runner.mjs`
- `scripts/render-wave-briefs.mjs`
- `scripts/verify-task-runner-batch.mjs`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/router/**`
- `.missiond/tasks/schema/task-contract-v1.lisp`
- `.missiond/tasks/wave28/**`
- `.missiond/tasks/wave29/wave29-*.lisp`
- `.missiond/tasks/wave29/manifest.lisp`
- `.missiond/tasks/wave29/dispatch-plan.lisp`
- `.missiond/claudecode/**`
- `scripts/verify-task-run.mjs`

## Requirements

1. Use one synthetic productive-only wave that includes context_atlas_path, pattern_card_path, parent-hotfix lineage, verification receipts, local/smoke tiers, heartbeat metadata, and a DAG where ready-queue scheduling saves time versus group barrier.
2. Pin layer-local failures near their owners: atlas checker, pattern checker, prep CLI, report checker, receipt checker, planner, renderer, and batch verifier should each have at least one wave29-07 fixture or assertion.
3. Prove shared preamble usage is auditable: generated trace/skeleton guidance includes a preamble-read event for trace-writable tasks.
4. Prove parent-hotfix lineage: final commit hash is authoritative, agent commit hash is preserved, and parent_patches files are repo-relative.
5. Prove receipt reuse is conservative: wrong commit/tier/command does not count as reusable evidence.
6. Prove no cargo is required for this Node/Lisp-only smoke. Do not touch crates/**.

## Acceptance Commands

```bash
node scripts/check-context-atlas.mjs --dry-fixture
node scripts/check-pattern-card.mjs --dry-fixture
node scripts/prepare-task-runner-wave.mjs --dry-fixture
node scripts/check-task-report.mjs --dry-fixture
node scripts/check-verification-receipt.mjs --dry-fixture
node scripts/plan-task-runner.mjs --dry-fixture
node scripts/render-wave-briefs.mjs --dry-fixture
node scripts/verify-task-runner-batch.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
git diff --check -- scripts/check-context-atlas.mjs scripts/check-pattern-card.mjs scripts/prepare-task-runner-wave.mjs scripts/check-task-report.mjs scripts/check-verification-receipt.mjs scripts/plan-task-runner.mjs scripts/render-wave-briefs.mjs scripts/verify-task-runner-batch.mjs
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
git add "scripts/check-context-atlas.mjs" \
        "scripts/check-pattern-card.mjs" \
        "scripts/prepare-task-runner-wave.mjs" \
        "scripts/check-task-report.mjs" \
        "scripts/check-verification-receipt.mjs" \
        "scripts/plan-task-runner.mjs" \
        "scripts/render-wave-briefs.mjs" \
        "scripts/verify-task-runner-batch.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave29/wave29-07-runner-efficiency-smoke-v1.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave29/wave29-07-runner-efficiency-smoke-v1.lisp \
  git commit -m "test(tasks): smoke runner efficiency loop"
node scripts/verify-task-contract.mjs .missiond/tasks/wave29/wave29-07-runner-efficiency-smoke-v1.lisp
```

## Report

- `Commit hash.`
- `Smoke layers pinned.`
- `Ready-queue savings and receipt reuse proofs.`
- `Acceptance command results.`

