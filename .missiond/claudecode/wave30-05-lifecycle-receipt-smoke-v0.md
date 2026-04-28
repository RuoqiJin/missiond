# wave30-05-lifecycle-receipt-smoke-v0 — Lifecycle receipt smoke v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave30/wave30-05-lifecycle-receipt-smoke-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave30-shared-preamble.md`

## Task Contract

- kind: `smoke`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `smoke`
- dispatch_group: `C`
- estimated_minutes: `45`
- heartbeat_minutes: `10`
- depends_on: `wave30-01-parent-hotfix-finalizer-v0`, `wave30-02-staged-source-hygiene-v0`, `wave30-03-atomic-lifecycle-event-log-v0`, `wave30-04-manifest-hard-soft-deps-v2`
- report_contract: `.missiond/tasks/wave30/reports/wave30-05-lifecycle-receipt-smoke-v0.report.lisp`
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave30/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave30/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave30/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave30/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Add the cross-layer regression that proves Wave30 is one lifecycle architecture, not five isolated helpers: staged source hygiene passes, lifecycle events append/project, parent hotfix finalization updates report lineage, receipts bind final commit/files/tier, ready-queue ignores soft references, and batch verification accepts the finalized truth.

## Ownership

- `scripts/task-runner-finalize-report.mjs`
- `scripts/task-runner-parent-hotfix.mjs`
- `scripts/task-runner-append-event.mjs`
- `scripts/check-task-lifecycle-events.mjs`
- `scripts/check-staged-source-hygiene.mjs`
- `scripts/check-verification-receipt.mjs`
- `scripts/verify-task-runner-batch.mjs`
- `scripts/plan-task-runner.mjs`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/research/**`
- `.missiond/router/**`
- `.missiond/tasks/schema/**`
- `.missiond/tasks/wave28/**`
- `.missiond/tasks/wave29/**`
- `.missiond/tasks/wave30/wave30-*.lisp`
- `.missiond/tasks/wave30/manifest.lisp`
- `.missiond/tasks/wave30/dispatch-plan.lisp`
- `.missiond/claudecode/**`
- `scripts/check-task-report.mjs`
- `scripts/check-task-runner-manifest.mjs`
- `scripts/render-wave-briefs.mjs`
- `scripts/prepare-task-runner-wave.mjs`
- `scripts/project-task-lifecycle-ledger.mjs`

## Requirements

1. Add layer-local smoke fixtures to the owning scripts instead of a single opaque shell test. Each failure should identify the nearest broken layer.
2. Add one synthetic Wave30 fixture that starts from a worker draft report, appends a parent hotfix event, projects a finalized report, validates staged source hygiene, validates a receipt for the final commit/files/tier, and passes batch verification.
3. Assert that worker draft commit remains visible as agent_commit_hash while commit_hash/final_commit_hash/verified_commit_hash point at the finalized commit.
4. Assert ready-queue output does not wait on soft_refs. The fixture should include at least one hard dependency and one unrelated soft reference.
5. Audit all touched scripts for raw NUL bytes before commit.

## Acceptance Commands

```bash
node scripts/task-runner-finalize-report.mjs --dry-fixture
node scripts/task-runner-parent-hotfix.mjs --dry-fixture
node scripts/task-runner-append-event.mjs --dry-fixture
node scripts/check-task-lifecycle-events.mjs --dry-fixture
node scripts/check-staged-source-hygiene.mjs --dry-fixture
node scripts/check-verification-receipt.mjs --dry-fixture
node scripts/verify-task-runner-batch.mjs --dry-fixture
node scripts/plan-task-runner.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
perl -ne 'exit 1 if /\x00/' scripts/task-runner-finalize-report.mjs scripts/task-runner-parent-hotfix.mjs scripts/task-runner-append-event.mjs scripts/check-task-lifecycle-events.mjs scripts/check-staged-source-hygiene.mjs scripts/check-verification-receipt.mjs scripts/verify-task-runner-batch.mjs scripts/plan-task-runner.mjs
git diff --check -- scripts/task-runner-finalize-report.mjs scripts/task-runner-parent-hotfix.mjs scripts/task-runner-append-event.mjs scripts/check-task-lifecycle-events.mjs scripts/check-staged-source-hygiene.mjs scripts/check-verification-receipt.mjs scripts/verify-task-runner-batch.mjs scripts/plan-task-runner.mjs
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
git add "scripts/task-runner-finalize-report.mjs" \
        "scripts/task-runner-parent-hotfix.mjs" \
        "scripts/task-runner-append-event.mjs" \
        "scripts/check-task-lifecycle-events.mjs" \
        "scripts/check-staged-source-hygiene.mjs" \
        "scripts/check-verification-receipt.mjs" \
        "scripts/verify-task-runner-batch.mjs" \
        "scripts/plan-task-runner.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave30/wave30-05-lifecycle-receipt-smoke-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave30/wave30-05-lifecycle-receipt-smoke-v0.lisp \
  git commit -m "test(tasks): smoke lifecycle finalization"
node scripts/verify-task-contract.mjs .missiond/tasks/wave30/wave30-05-lifecycle-receipt-smoke-v0.lisp
```

## Report

- `Commit hash.`
- `Synthetic lifecycle fixture shape.`
- `Layer-local fixture increments.`
- `Receipt/finalized report/ready-queue invariants.`
- `Acceptance command results.`

