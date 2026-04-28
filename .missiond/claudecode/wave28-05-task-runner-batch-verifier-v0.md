# wave28-05-task-runner-batch-verifier-v0 — Task runner batch verifier v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave28/wave28-05-task-runner-batch-verifier-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave28-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `C`
- estimated_minutes: `35`
- heartbeat_minutes: `10`
- depends_on: `wave28-01-task-runner-manifest-schema-v0`, `wave28-02-task-runner-plan-cli-v0`
- shared_memory: `.missiond/tasks/wave28/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave28/reports/wave28-05-task-runner-batch-verifier-v0.report.lisp`
- session_trace: `.missiond/tasks/wave28/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)

## Goal

Add a batch verifier that checks a task-runner manifest after worker commits: every productive node has a report, shared-memory completion, commit hash, and contract verification result. This prepares MissionD to close a full wave without parsing prose.

## Ownership

- `scripts/verify-task-runner-batch.mjs`
- `scripts/verify-task-run.mjs`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/router/**`
- `.missiond/tasks/schema/task-contract-v1.lisp`
- `.missiond/tasks/schema/task-runner-manifest-v1.lisp`
- `.missiond/tasks/wave27/**`
- `.missiond/tasks/wave28/wave28-*.lisp`
- `.missiond/tasks/wave28/dispatch-plan.lisp`
- `.missiond/claudecode/**`
- `scripts/check-task-runner-manifest.mjs`
- `scripts/plan-task-runner.mjs`
- `scripts/render-wave-briefs.mjs`
- `scripts/render-claudecode-task.mjs`

## Requirements

1. CLI: node scripts/verify-task-runner-batch.mjs --manifest <manifest.lisp> [--json] [--dry-fixture].
2. For each productive node, locate task contract, expected report, shared memory completion, and commit hash; reuse existing verify-task-run/checker helpers where possible.
3. Do not require archive/backfill/index pseudo-nodes. They are orchestrator-owned and outside worker completion accounting.
4. Output must include schema, manifest_path, wave, total_nodes, verified_nodes, missing_reports, missing_memory_completions, failed_contract_verifications, and aggregate_status.
5. Read-only only: no git mutation, no shell beyond existing read-only git helpers if imported from verifier, no network, no LLM.
6. Fixtures must cover all-green, missing report, missing memory completion, commit mismatch, and non-productive pseudo-node skipped.

## Acceptance Commands

```bash
node scripts/verify-task-runner-batch.mjs --dry-fixture
node scripts/verify-task-run.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
git diff --check -- scripts/verify-task-runner-batch.mjs scripts/verify-task-run.mjs
```

## Shared Protocol

Read `.missiond/claudecode/wave28-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add "scripts/verify-task-runner-batch.mjs" \
        "scripts/verify-task-run.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave28/wave28-05-task-runner-batch-verifier-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave28/wave28-05-task-runner-batch-verifier-v0.lisp \
  git commit -m "feat(tasks): verify task runner batches"
node scripts/verify-task-contract.mjs .missiond/tasks/wave28/wave28-05-task-runner-batch-verifier-v0.lisp
```

## Report

- `Commit hash.`
- `Verifier output schema.`
- `Skipped pseudo-node behavior.`
- `Acceptance command results.`

