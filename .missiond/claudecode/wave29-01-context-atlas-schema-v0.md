# wave29-01-context-atlas-schema-v0 — Context atlas schema v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave29/wave29-01-context-atlas-schema-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave29-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `35`
- heartbeat_minutes: `10`
- shared_memory: `.missiond/tasks/wave29/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave29/reports/wave29-01-context-atlas-schema-v0.report.lisp`
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

Introduce a durable context-atlas schema and checker so future waves can give workers precise file anchors, grep keywords, and read-order guidance before broad repository search. The dispatch-time wave29 atlas is guidance; this task makes the format machine-checkable for future waves.

## Ownership

- `.missiond/tasks/schema/context-atlas-v1.lisp`
- `scripts/check-context-atlas.mjs`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/router/**`
- `.missiond/tasks/wave28/**`
- `.missiond/tasks/wave29/wave29-*.lisp`
- `.missiond/tasks/wave29/manifest.lisp`
- `.missiond/tasks/wave29/dispatch-plan.lisp`
- `.missiond/tasks/wave29/pattern-cards.lisp`
- `.missiond/claudecode/**`
- `scripts/check-pattern-card.mjs`
- `scripts/prepare-task-runner-wave.mjs`
- `scripts/check-task-report.mjs`
- `scripts/verify-task-run.mjs`
- `scripts/verify-task-runner-batch.mjs`
- `scripts/plan-task-runner.mjs`

## Requirements

1. Define schema missiond.context-atlas.v1 in .missiond/tasks/schema/context-atlas-v1.lisp.
2. Atlas records id, schema, purpose, read_order, file entries, task_focus entries, grep anchors, and avoid notes. It is navigation metadata only; it must not be treated as a behavioral contract.
3. Checker must validate repo-relative paths, non-empty anchors, duplicate file entries, duplicate task_focus entries, path traversal rejection, and malformed read_order values.
4. Checker must support --json, --stdin, and --dry-fixture, use scripts/lib/missiond_lisp.mjs, and never shell out / call git / call network / call LLM.
5. Fixtures must include a valid atlas, duplicate file rejection, duplicate task_focus rejection, absolute path rejection, traversal rejection, empty grep anchor rejection, and multiple atlas forms.

## Acceptance Commands

```bash
node scripts/check-context-atlas.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
git diff --check -- .missiond/tasks/schema/context-atlas-v1.lisp scripts/check-context-atlas.mjs
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
git add ".missiond/tasks/schema/context-atlas-v1.lisp" \
        "scripts/check-context-atlas.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave29/wave29-01-context-atlas-schema-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave29/wave29-01-context-atlas-schema-v0.lisp \
  git commit -m "feat(tasks): add context atlas schema"
node scripts/verify-task-contract.mjs .missiond/tasks/wave29/wave29-01-context-atlas-schema-v0.lisp
```

## Report

- `Commit hash.`
- `Schema head and required fields.`
- `Fixture categories.`
- `Acceptance command results.`

