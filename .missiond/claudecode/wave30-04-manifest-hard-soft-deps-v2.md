# wave30-04-manifest-hard-soft-deps-v2 — Manifest hard/soft deps v2

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave30/wave30-04-manifest-hard-soft-deps-v2.lisp`
> Shared preamble: `.missiond/claudecode/wave30-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `B`
- estimated_minutes: `45`
- heartbeat_minutes: `10`
- depends_on: `wave30-02-staged-source-hygiene-v0`
- report_contract: `.missiond/tasks/wave30/reports/wave30-04-manifest-hard-soft-deps-v2.report.lisp`
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave30/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave30/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave30/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave30/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Make ready-queue scheduling precise by separating hard dependencies that block dispatch from soft references that only enrich briefs/context. Preserve task-runner-manifest v1 compatibility while adding a v2/additive path for hard_deps, soft_refs, ready-queue release facts, and optional lifecycle lease/event-log references.

## Ownership

- `.missiond/tasks/schema/task-runner-manifest-v2.lisp`
- `scripts/check-task-runner-manifest.mjs`
- `scripts/plan-task-runner.mjs`
- `scripts/render-wave-briefs.mjs`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/research/**`
- `.missiond/router/**`
- `.missiond/tasks/schema/task-contract-v1.lisp`
- `.missiond/tasks/schema/report-contract-v1.lisp`
- `.missiond/tasks/schema/task-lifecycle-event-v1.lisp`
- `.missiond/tasks/schema/verification-receipt-v1.lisp`
- `.missiond/tasks/wave28/**`
- `.missiond/tasks/wave29/**`
- `.missiond/tasks/wave30/wave30-*.lisp`
- `.missiond/tasks/wave30/manifest.lisp`
- `.missiond/tasks/wave30/dispatch-plan.lisp`
- `.missiond/claudecode/**`
- `scripts/task-runner-finalize-report.mjs`
- `scripts/task-runner-parent-hotfix.mjs`
- `scripts/task-runner-append-event.mjs`
- `scripts/check-task-lifecycle-events.mjs`
- `scripts/project-task-lifecycle-ledger.mjs`
- `scripts/check-staged-source-hygiene.mjs`
- `scripts/check-task-report.mjs`
- `scripts/check-verification-receipt.mjs`
- `scripts/verify-task-runner-batch.mjs`
- `scripts/prepare-task-runner-wave.mjs`

## Requirements

1. Add task-runner-manifest-v2.lisp or an explicitly additive v1-compatible schema note that distinguishes :hard_deps from :soft_refs. Existing :depends_on must keep v1 behavior.
2. Update check-task-runner-manifest.mjs to validate v2/additive hard/soft references without breaking all existing v1 fixtures.
3. Update plan-task-runner.mjs ready-queue mode so only hard dependencies block dispatch. Soft references may be reported as context but must not affect barrier_finish_at/ready time.
4. Update render-wave-briefs.mjs so soft references render as context guidance, not as dependencies or blockers.
5. Add a fixture matching the Wave29-03 observation: a task that only hard-depends on the manifest/atlas source must not wait for unrelated soft references.

## Acceptance Commands

```bash
node scripts/check-task-runner-manifest.mjs --dry-fixture
node scripts/plan-task-runner.mjs --dry-fixture
node scripts/render-wave-briefs.mjs --dry-fixture
node scripts/check-task-runner-manifest.mjs .missiond/tasks/wave30/manifest.lisp
node scripts/check-task-contract.mjs --all
perl -ne 'exit 1 if /\x00/' scripts/check-task-runner-manifest.mjs scripts/plan-task-runner.mjs scripts/render-wave-briefs.mjs
git diff --check -- .missiond/tasks/schema/task-runner-manifest-v2.lisp scripts/check-task-runner-manifest.mjs scripts/plan-task-runner.mjs scripts/render-wave-briefs.mjs
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
git add ".missiond/tasks/schema/task-runner-manifest-v2.lisp" \
        "scripts/check-task-runner-manifest.mjs" \
        "scripts/plan-task-runner.mjs" \
        "scripts/render-wave-briefs.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave30/wave30-04-manifest-hard-soft-deps-v2.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave30/wave30-04-manifest-hard-soft-deps-v2.lisp \
  git commit -m "feat(tasks): split hard and soft runner deps"
node scripts/verify-task-contract.mjs .missiond/tasks/wave30/wave30-04-manifest-hard-soft-deps-v2.lisp
```

## Report

- `Commit hash.`
- `Manifest v2/additive compatibility strategy.`
- `Ready-queue hard-vs-soft dependency fixture.`
- `Renderer soft-reference output.`
- `Acceptance command results.`

