# wave28-02-task-runner-plan-cli-v0 — Task runner plan CLI v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave28/wave28-02-task-runner-plan-cli-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave28-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `B`
- estimated_minutes: `35`
- heartbeat_minutes: `10`
- depends_on: `wave28-01-task-runner-manifest-schema-v0`
- shared_memory: `.missiond/tasks/wave28/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave28/reports/wave28-02-task-runner-plan-cli-v0.report.lisp`
- session_trace: `.missiond/tasks/wave28/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)

## Goal

Add a read-only CLI that consumes a task-runner manifest and task contracts, then emits a deterministic runner plan: topological groups, critical-path estimate, write-scope overlap diagnostics, and verification-tier summary. This is the first dry-run substrate for a future MissionD task-contract runner.

## Ownership

- `scripts/plan-task-runner.mjs`

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
- `scripts/render-claudecode-task.mjs`
- `scripts/render-wave-briefs.mjs`
- `scripts/verify-task-runner-batch.mjs`

## Requirements

1. CLI: node scripts/plan-task-runner.mjs --manifest <manifest.lisp> [--json|--lisp] [--dry-fixture].
2. Import the manifest checker or share a small parser helper from wave28-01; do not duplicate inconsistent validation rules.
3. Output must include schema, manifest_path, wave, productive_only, batches, critical_path_minutes, total_estimated_minutes, max_parallel_width, overlap_diagnostics, and verification_tier_counts.
4. Topological sort must be deterministic and fail on dependency cycles with a structured error.
5. No dispatch, no spawn, no git mutation, no network, no LLM. This CLI plans only.
6. Fixtures must cover valid manifest, cycle rejection, missing task contract rejection, overlap warning/error, and deterministic ordering.

## Acceptance Commands

```bash
node scripts/plan-task-runner.mjs --dry-fixture
node scripts/check-task-runner-manifest.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
git diff --check -- scripts/plan-task-runner.mjs
```

## Shared Protocol

Read `.missiond/claudecode/wave28-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add "scripts/plan-task-runner.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave28/wave28-02-task-runner-plan-cli-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave28/wave28-02-task-runner-plan-cli-v0.lisp \
  git commit -m "feat(tasks): add task runner plan CLI"
node scripts/verify-task-contract.mjs .missiond/tasks/wave28/wave28-02-task-runner-plan-cli-v0.lisp
```

## Report

- `Commit hash.`
- `Output schema fields.`
- `Cycle/overlap behavior.`
- `Acceptance command results.`

