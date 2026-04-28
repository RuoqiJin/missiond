# wave30-01-parent-hotfix-finalizer-v0 — Parent hotfix finalizer v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave30/wave30-01-parent-hotfix-finalizer-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave30-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `B`
- estimated_minutes: `50`
- heartbeat_minutes: `10`
- depends_on: `wave30-02-staged-source-hygiene-v0`, `wave30-03-atomic-lifecycle-event-log-v0`
- report_contract: `.missiond/tasks/wave30/reports/wave30-01-parent-hotfix-finalizer-v0.report.lisp`
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave30/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave30/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave30/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave30/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Close the Wave29-03 lineage drift class by making parent hotfix finalization orchestrator-owned. A worker may produce a draft report and worker commit; if the parent applies a post-worker hotfix, the runner must append/consume the parent patch fact and project a finalized report whose commit_hash, final_commit_hash, verified_commit_hash, and parent_patches agree.

## Ownership

- `scripts/task-runner-finalize-report.mjs`
- `scripts/task-runner-parent-hotfix.mjs`
- `scripts/check-task-report.mjs`
- `scripts/verify-task-runner-batch.mjs`
- `.missiond/tasks/schema/report-contract-v1.lisp`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/research/**`
- `.missiond/router/**`
- `.missiond/tasks/schema/task-contract-v1.lisp`
- `.missiond/tasks/schema/task-runner-manifest-v1.lisp`
- `.missiond/tasks/schema/task-runner-manifest-v2.lisp`
- `.missiond/tasks/schema/task-lifecycle-event-v1.lisp`
- `.missiond/tasks/schema/verification-receipt-v1.lisp`
- `.missiond/tasks/wave28/**`
- `.missiond/tasks/wave29/**`
- `.missiond/tasks/wave30/wave30-*.lisp`
- `.missiond/tasks/wave30/manifest.lisp`
- `.missiond/tasks/wave30/dispatch-plan.lisp`
- `.missiond/claudecode/**`
- `scripts/task-runner-append-event.mjs`
- `scripts/check-task-lifecycle-events.mjs`
- `scripts/project-task-lifecycle-ledger.mjs`
- `scripts/check-staged-source-hygiene.mjs`
- `scripts/check-verification-receipt.mjs`
- `scripts/plan-task-runner.mjs`
- `scripts/check-task-runner-manifest.mjs`
- `scripts/render-wave-briefs.mjs`
- `scripts/prepare-task-runner-wave.mjs`

## Requirements

1. Create task-runner-finalize-report.mjs with named exports and --dry-fixture. It should accept a worker draft report plus finalization facts and emit a deterministic finalized report object/string without mutating git.
2. Create task-runner-parent-hotfix.mjs with a dry-run/default read-only planning mode plus an explicit write mode if file mutation is needed; it must document that parent hotfix commits are appended as lineage facts, not worker commit amendments.
3. Update check-task-report.mjs/report-contract docs so parent_patches tail commit, final_commit_hash, verified_commit_hash, and commit_hash drift rules are explicit and fixture-pinned.
4. Update verify-task-runner-batch.mjs so finalized reports are the completion truth and worker draft hashes can still match lineage roles.
5. Dogfood the Wave29-03 drift shape as a fixture: worker commit d36de80, parent hotfix d842b1d, finalized report commit_hash d842b1d, agent_commit_hash d36de80.
6. No spawn, no LLM, no network. Any git inspection must be read-only and optional; default fixtures must run in temp dirs.

## Acceptance Commands

```bash
node scripts/task-runner-finalize-report.mjs --dry-fixture
node scripts/task-runner-parent-hotfix.mjs --dry-fixture
node scripts/check-task-report.mjs --dry-fixture
node scripts/verify-task-runner-batch.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
perl -ne 'exit 1 if /\x00/' scripts/task-runner-finalize-report.mjs scripts/task-runner-parent-hotfix.mjs scripts/check-task-report.mjs scripts/verify-task-runner-batch.mjs
git diff --check -- scripts/task-runner-finalize-report.mjs scripts/task-runner-parent-hotfix.mjs scripts/check-task-report.mjs scripts/verify-task-runner-batch.mjs .missiond/tasks/schema/report-contract-v1.lisp
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
        "scripts/check-task-report.mjs" \
        "scripts/verify-task-runner-batch.mjs" \
        ".missiond/tasks/schema/report-contract-v1.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave30/wave30-01-parent-hotfix-finalizer-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave30/wave30-01-parent-hotfix-finalizer-v0.lisp \
  git commit -m "feat(tasks): finalize parent hotfix lineage"
node scripts/verify-task-contract.mjs .missiond/tasks/wave30/wave30-01-parent-hotfix-finalizer-v0.lisp
```

## Report

- `Commit hash.`
- `Finalizer CLI input/output contract.`
- `Parent hotfix helper behavior and explicit mutation boundary.`
- `Wave29-03 drift fixture result.`
- `Acceptance command results.`

