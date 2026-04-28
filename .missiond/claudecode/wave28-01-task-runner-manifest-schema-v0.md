# wave28-01-task-runner-manifest-schema-v0 — Task runner manifest schema v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave28/wave28-01-task-runner-manifest-schema-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave28-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `30`
- heartbeat_minutes: `10`
- shared_memory: `.missiond/tasks/wave28/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave28/reports/wave28-01-task-runner-manifest-schema-v0.report.lisp`
- session_trace: `.missiond/tasks/wave28/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)

## Goal

Introduce a machine-checkable task-runner manifest schema and checker. The manifest describes a batch of task contracts, dependency groups, write-scope overlap policy, verification tiers, and shared preamble path. It is orchestration metadata for MissionD/Codex, not a worker task and not a runtime backend switch.

## Ownership

- `.missiond/tasks/schema/task-runner-manifest-v1.lisp`
- `scripts/check-task-runner-manifest.mjs`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/router/**`
- `.missiond/tasks/wave27/**`
- `.missiond/tasks/wave28/wave28-*.lisp`
- `.missiond/tasks/wave28/dispatch-plan.lisp`
- `.missiond/claudecode/**`
- `scripts/render-claudecode-task.mjs`
- `scripts/check-task-contract.mjs`
- `scripts/plan-task-runner.mjs`
- `scripts/render-wave-briefs.mjs`
- `scripts/verify-task-runner-batch.mjs`

## Requirements

1. Define schema missiond.task-runner-manifest.v1 in .missiond/tasks/schema/task-runner-manifest-v1.lisp.
2. Manifest must record wave id, brief_mode, shared_preamble_path, productive_only boolean, nodes, dependencies, verification_tier, dispatch_group, estimated_minutes, heartbeat_minutes, and write_scope.
3. Checker must validate task ids, dependency references, duplicate nodes, enum values, positive integer minutes, repo-relative paths, and no worker-task entries for archive/backfill/index categories.
4. Checker must detect write-scope overlap among nodes in the same dispatch group and emit a warning or error according to manifest policy; default policy should reject same-file overlaps for productive tasks.
5. Checker must support --json, --stdin, and --dry-fixture, use scripts/lib/missiond_lisp.mjs, and never shell out / call git / call network / call LLM.
6. Fixtures must include valid productive-only manifest, duplicate node rejection, missing dependency rejection, invalid verification tier rejection, overlap rejection, positive heartbeat validation, archive/backfill/index worker rejection, and missing shared preamble warning.

## Acceptance Commands

```bash
node scripts/check-task-runner-manifest.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
git diff --check -- .missiond/tasks/schema/task-runner-manifest-v1.lisp scripts/check-task-runner-manifest.mjs
```

## Shared Protocol

Read `.missiond/claudecode/wave28-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add ".missiond/tasks/schema/task-runner-manifest-v1.lisp" \
        "scripts/check-task-runner-manifest.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave28/wave28-01-task-runner-manifest-schema-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave28/wave28-01-task-runner-manifest-schema-v0.lisp \
  git commit -m "feat(tasks): add task runner manifest schema"
node scripts/verify-task-contract.mjs .missiond/tasks/wave28/wave28-01-task-runner-manifest-schema-v0.lisp
```

## Report

- `Commit hash.`
- `Schema fields and fixture categories.`
- `Overlap/default policy behavior.`
- `Acceptance command results.`

