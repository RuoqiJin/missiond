# wave29-03-runner-wave-prep-v0 — Runner wave prep v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave29/wave29-03-runner-wave-prep-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave29-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `B`
- estimated_minutes: `35`
- heartbeat_minutes: `10`
- depends_on: `wave29-01-context-atlas-schema-v0`, `wave29-02-pattern-card-schema-v0`
- shared_memory: `.missiond/tasks/wave29/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave29/reports/wave29-03-runner-wave-prep-v0.report.lisp`
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

Add a read-only-plus-file-generation preparation CLI for future task-runner waves: validate a manifest, render thin briefs, prepare report skeleton paths, and emit bootstrap shared-memory/session-trace entries including an auditable shared-preamble-read expectation. This reduces per-agent report/setup work without creating archive/backfill/index workers.

## Ownership

- `scripts/prepare-task-runner-wave.mjs`
- `scripts/render-wave-briefs.mjs`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/router/**`
- `.missiond/tasks/schema/task-contract-v1.lisp`
- `.missiond/tasks/schema/context-atlas-v1.lisp`
- `.missiond/tasks/schema/pattern-card-v1.lisp`
- `.missiond/tasks/wave28/**`
- `.missiond/tasks/wave29/wave29-*.lisp`
- `.missiond/tasks/wave29/manifest.lisp`
- `.missiond/tasks/wave29/dispatch-plan.lisp`
- `.missiond/tasks/wave29/context-atlas.lisp`
- `.missiond/tasks/wave29/pattern-cards.lisp`
- `.missiond/claudecode/**`
- `.missiond/patterns/**`
- `scripts/check-context-atlas.mjs`
- `scripts/check-pattern-card.mjs`
- `scripts/check-task-report.mjs`
- `scripts/verify-task-run.mjs`
- `scripts/verify-task-runner-batch.mjs`
- `scripts/plan-task-runner.mjs`

## Requirements

1. CLI: node scripts/prepare-task-runner-wave.mjs --manifest <manifest.lisp> [--out-dir <repo>] [--dry-run] [--force] [--json] [--dry-fixture].
2. Reuse render-wave-briefs internals through named exports instead of shelling out; if needed, export renderManifest from scripts/render-wave-briefs.mjs while preserving CLI behavior.
3. Prepare reports directory and optional report skeletons in a deterministic form, but do not stage or commit generated wave artifacts from fixtures.
4. Emit or print bootstrap shared-memory/session-trace entries that include a preamble-read trace expectation for trace-writable tasks.
5. Respect productive-only: archive/backfill/index/lisp-backfill pseudo-nodes remain orchestrator-owned and must not receive worker briefs or report skeletons.
6. Fixtures must use temporary repos only and cover dry-run no-write, force overwrite, report skeleton generation, preamble-read trace event, pseudo-node rejection, and deterministic output.

## Acceptance Commands

```bash
node scripts/prepare-task-runner-wave.mjs --dry-fixture
node scripts/render-wave-briefs.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
git diff --check -- scripts/prepare-task-runner-wave.mjs scripts/render-wave-briefs.mjs
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
git add "scripts/prepare-task-runner-wave.mjs" \
        "scripts/render-wave-briefs.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave29/wave29-03-runner-wave-prep-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave29/wave29-03-runner-wave-prep-v0.lisp \
  git commit -m "feat(tasks): prepare task runner waves"
node scripts/verify-task-contract.mjs .missiond/tasks/wave29/wave29-03-runner-wave-prep-v0.lisp
```

## Report

- `Commit hash.`
- `Prepared artifact types.`
- `How preamble-read trace evidence is represented.`
- `Acceptance command results.`

