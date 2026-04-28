# wave29-04-parent-hotfix-lineage-v1 — Parent hotfix lineage v1

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave29/wave29-04-parent-hotfix-lineage-v1.lisp`
> Shared preamble: `.missiond/claudecode/wave29-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `30`
- heartbeat_minutes: `10`
- shared_memory: `.missiond/tasks/wave29/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave29/reports/wave29-04-parent-hotfix-lineage-v1.report.lisp`
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

Harden the parent-hotfix commit lineage model introduced during wave29 prep. Parent one-line fixes should update the final report hash and record :agent_commit_hash plus :parent_patches, without amending worker commits and without breaking batch verification.

## Ownership

- `scripts/check-task-report.mjs`
- `scripts/verify-task-run.mjs`
- `scripts/verify-task-runner-batch.mjs`
- `.missiond/tasks/schema/report-contract-v1.lisp`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/router/**`
- `.missiond/tasks/schema/task-contract-v1.lisp`
- `.missiond/tasks/schema/task-runner-manifest-v1.lisp`
- `.missiond/tasks/schema/context-atlas-v1.lisp`
- `.missiond/tasks/schema/pattern-card-v1.lisp`
- `.missiond/tasks/wave28/**`
- `.missiond/tasks/wave29/wave29-*.lisp`
- `.missiond/tasks/wave29/manifest.lisp`
- `.missiond/tasks/wave29/dispatch-plan.lisp`
- `.missiond/claudecode/**`
- `scripts/check-context-atlas.mjs`
- `scripts/check-pattern-card.mjs`
- `scripts/check-verification-receipt.mjs`
- `scripts/prepare-task-runner-wave.mjs`
- `scripts/render-wave-briefs.mjs`
- `scripts/plan-task-runner.mjs`

## Requirements

1. Model the wave28-02 case explicitly in fixtures: worker commit 954116e followed by parent lint-cleanup commit 302330a, final report :commit_hash equal to final commit, and :agent_commit_hash equal to worker commit.
2. Report checker must reject parent patches with missing commit/kind/reason/files, absolute/traversal files, malformed hashes, and final/verified hash drift.
3. verify-task-run must expose lineage in JSON and verify against final/verified commit when provided, while preserving existing reports without lineage fields.
4. verify-task-runner-batch must accept memory completion summaries that mention either the final commit or the agent commit, but the verified result should point at the final/verified hash.
5. No git mutation. Verifier commands remain read-only.
6. Add fixtures without reducing existing wave23/wave28 fixture coverage.

## Acceptance Commands

```bash
node scripts/check-task-report.mjs --dry-fixture
node scripts/verify-task-run.mjs --dry-fixture
node scripts/verify-task-runner-batch.mjs --dry-fixture
node scripts/check-task-report.mjs --all
node scripts/check-task-contract.mjs --all
git diff --check -- scripts/check-task-report.mjs scripts/verify-task-run.mjs scripts/verify-task-runner-batch.mjs .missiond/tasks/schema/report-contract-v1.lisp
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
git add "scripts/check-task-report.mjs" \
        "scripts/verify-task-run.mjs" \
        "scripts/verify-task-runner-batch.mjs" \
        ".missiond/tasks/schema/report-contract-v1.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave29/wave29-04-parent-hotfix-lineage-v1.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave29/wave29-04-parent-hotfix-lineage-v1.lisp \
  git commit -m "feat(tasks): verify parent hotfix lineage"
node scripts/verify-task-contract.mjs .missiond/tasks/wave29/wave29-04-parent-hotfix-lineage-v1.lisp
```

## Report

- `Commit hash.`
- `Lineage fields and verifier behavior.`
- `Wave28-02 hotfix fixture behavior.`
- `Acceptance command results.`

