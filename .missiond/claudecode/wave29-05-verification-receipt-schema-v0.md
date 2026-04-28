# wave29-05-verification-receipt-schema-v0 — Verification receipt schema v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave29/wave29-05-verification-receipt-schema-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave29-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `B`
- estimated_minutes: `40`
- heartbeat_minutes: `10`
- depends_on: `wave29-04-parent-hotfix-lineage-v1`
- shared_memory: `.missiond/tasks/wave29/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave29/reports/wave29-05-verification-receipt-schema-v0.report.lisp`
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

Introduce verification receipts so the orchestrator can reuse already-run smoke/full evidence across a wave instead of blindly repeating expensive checks. Receipts cache command evidence; they are never a substitute for source facts or commit verification.

## Ownership

- `.missiond/tasks/schema/verification-receipt-v1.lisp`
- `scripts/check-verification-receipt.mjs`
- `scripts/verify-task-runner-batch.mjs`

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
- `scripts/prepare-task-runner-wave.mjs`
- `scripts/render-wave-briefs.mjs`
- `scripts/plan-task-runner.mjs`

## Requirements

1. Define schema missiond.verification-receipt.v1 with wave, task_id, commit_hash, command, exit_code, started_at/finished_at or duration_ms, tier, and files/paths evidence.
2. Checker must validate command strings, positive/non-negative durations, exit_code integer, tier enum local|smoke|full, commit hash shape, repo-relative paths, duplicate receipt ids, and stale wave/task mismatch.
3. verify-task-runner-batch may load optional receipts and report receipt coverage, but must still verify task contract, report, memory completion, and git commit.
4. Receipt reuse rules must be conservative: wrong commit, wrong command, non-zero exit, or stale tier must not count as reusable evidence.
5. Checker supports --json, --stdin, --dry-fixture; no git mutation, no network, no LLM.
6. Fixtures must include valid smoke receipt, stale commit rejection, wrong tier rejection, non-zero exit rejection, duplicate id rejection, and batch verifier coverage.

## Acceptance Commands

```bash
node scripts/check-verification-receipt.mjs --dry-fixture
node scripts/verify-task-runner-batch.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
git diff --check -- .missiond/tasks/schema/verification-receipt-v1.lisp scripts/check-verification-receipt.mjs scripts/verify-task-runner-batch.mjs
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
git add ".missiond/tasks/schema/verification-receipt-v1.lisp" \
        "scripts/check-verification-receipt.mjs" \
        "scripts/verify-task-runner-batch.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave29/wave29-05-verification-receipt-schema-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave29/wave29-05-verification-receipt-schema-v0.lisp \
  git commit -m "feat(tasks): add verification receipt checks"
node scripts/verify-task-contract.mjs .missiond/tasks/wave29/wave29-05-verification-receipt-schema-v0.lisp
```

## Report

- `Commit hash.`
- `Receipt schema fields.`
- `Conservative reuse rules.`
- `Acceptance command results.`

