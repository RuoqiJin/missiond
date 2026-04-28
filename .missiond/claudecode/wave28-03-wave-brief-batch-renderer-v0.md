# wave28-03-wave-brief-batch-renderer-v0 — Wave brief batch renderer v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave28/wave28-03-wave-brief-batch-renderer-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave28-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `B`
- estimated_minutes: `25`
- heartbeat_minutes: `10`
- depends_on: `wave28-01-task-runner-manifest-schema-v0`
- shared_memory: `.missiond/tasks/wave28/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave28/reports/wave28-03-wave-brief-batch-renderer-v0.report.lisp`
- session_trace: `.missiond/tasks/wave28/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)

## Goal

Add a batch renderer that consumes a task-runner manifest and emits one shared preamble plus thin ClaudeCode briefs for productive worker tasks. This makes Wave28+ dispatch use the new thin-brief path without hand-rendering every task.

## Ownership

- `scripts/render-wave-briefs.mjs`
- `scripts/render-claudecode-task.mjs`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/router/**`
- `.missiond/tasks/schema/task-contract-v1.lisp`
- `.missiond/tasks/schema/task-runner-manifest-v1.lisp`
- `.missiond/tasks/wave27/**`
- `.missiond/tasks/wave28/wave28-*.lisp`
- `.missiond/tasks/wave28/dispatch-plan.lisp`
- `.missiond/claudecode/wave27-*.md`
- `scripts/check-task-runner-manifest.mjs`
- `scripts/plan-task-runner.mjs`
- `scripts/verify-task-runner-batch.mjs`

## Requirements

1. CLI: node scripts/render-wave-briefs.mjs --manifest <manifest.lisp> [--force] [--dry-fixture].
2. Generate .missiond/claudecode/<wave>-shared-preamble.md once, then render each productive node with render-claudecode-task.mjs --brief-mode thin --shared-preamble <path>.
3. Do not render archive/backfill/index pseudo-nodes as worker briefs; fail if the manifest tries to mark them as productive worker nodes.
4. Prefer importing renderer functions only if the existing renderer can expose them cleanly; otherwise invoke renderer logic in-process without shelling out.
5. Keep render-claudecode-task.mjs full-mode backward compatible; existing dry fixtures must remain green.
6. Fixtures must prove preamble exists, thin brief omits repeated Shared Memory / Report Contract / Session Trace / Router Policy sections, and output paths are deterministic.

## Acceptance Commands

```bash
node scripts/render-wave-briefs.mjs --dry-fixture
node scripts/render-claudecode-task.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
git diff --check -- scripts/render-wave-briefs.mjs scripts/render-claudecode-task.mjs
```

## Shared Protocol

Read `.missiond/claudecode/wave28-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add "scripts/render-wave-briefs.mjs" \
        "scripts/render-claudecode-task.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave28/wave28-03-wave-brief-batch-renderer-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave28/wave28-03-wave-brief-batch-renderer-v0.lisp \
  git commit -m "feat(tasks): render wave briefs from manifest"
node scripts/verify-task-contract.mjs .missiond/tasks/wave28/wave28-03-wave-brief-batch-renderer-v0.lisp
```

## Report

- `Commit hash.`
- `Generated output conventions.`
- `Backward-compat notes for full renderer.`
- `Acceptance command results.`

