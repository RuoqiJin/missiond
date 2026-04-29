# wave52-01-contract-artifact-validation-v0 — validate touched Lisp artifacts during task contract verification

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave52/wave52-01-contract-artifact-validation-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave52-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `55`
- heartbeat_minutes: `10`
- shared_memory: `.missiond/tasks/wave52/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave52/reports/wave52-01-contract-artifact-validation-v0.report.lisp`
- session_trace: `.missiond/tasks/wave52/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave52/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave52/pattern-cards.lisp`
- context_pack: `.missiond/tasks/wave52/context-pack.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave52/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave52/pattern-cards.lisp`.
- Read context-pack integration-plan before implementation: `.missiond/tasks/wave52/context-pack.lisp`.
- Treat accepted shards and mapped dispatch groups as the shard authority; do not re-derive architecture from older observations.
- Use declared context anchors and shard boundaries before falling back to broad scans.

## Goal

Upgrade scripts/verify-task-contract.mjs so commit verification also validates known Lisp artifacts touched by the verified commit. The wave51 worker commit 7f462d17 must now fail because it contains an invalid session-trace :kind acceptance.

## Ownership

- `scripts/verify-task-contract.mjs`
- `.missiond/v3/missiond-blueprint.lisp`
- `scripts/check-v3-task-lifecycle-isomorphism.mjs`
- `.missiond/tasks/wave52/shared-memory.lisp`
- `.missiond/tasks/wave52/session-trace.lisp`
- `.missiond/tasks/wave52/reports/wave52-01-contract-artifact-validation-v0.report.lisp`

## Must Not Touch

- `packages/**`
- `crates/**`
- `.missiond/v1/**`
- `.missiond/v2/**`
- `.missiond/tasks/wave48/**`
- `.missiond/tasks/wave49/**`
- `.missiond/tasks/wave50/**`
- `.missiond/tasks/wave51/**`
- `.missiond/tasks/wave52/manifest.lisp`
- `.missiond/tasks/wave52/wave52-*.lisp`
- `.missiond/tasks/wave52/context-atlas.lisp`
- `.missiond/tasks/wave52/pattern-cards.lisp`
- `.missiond/tasks/wave52/context-pack.lisp`
- `.missiond/claudecode/**`
- `scripts/check-session-trace.mjs`
- `scripts/check-task-memory.mjs`
- `scripts/check-task-report.mjs`
- `scripts/check-task-lifecycle-events.mjs`
- `scripts/task-scope-guard.mjs`
- `scripts/verify-task-run.mjs`
- `scripts/verify-task-runner-batch.mjs`

## Requirements

1. Read the shared preamble, this task contract, context atlas, pattern cards, and the wave52 context-pack integration-plan before broad scans.
2. Use scripts/context-pack-compile-shards.mjs .missiond/tasks/wave52/context-pack.lisp to confirm this is the accepted mapped shard.
3. Extend scripts/verify-task-contract.mjs so real commit verification detects known Lisp artifacts touched by the resolved commit and validates them with the existing artifact checkers.
4. Known artifact paths: .missiond/tasks/<wave>/session-trace.lisp -> check-session-trace, shared-memory.lisp -> check-task-memory, task-lifecycle-events.lisp and events/*.event.lisp -> check-task-lifecycle-events, reports/*.report.lisp -> check-task-report.
5. Validate artifact bytes from the resolved commit, not the current working tree, so --commit=<worker-hash> remains correct after later parent commits.
6. Preserve the existing pure verifyContract(contract, commitInfo) API for importers; add artifact validation around the CLI path or through a clearly separated helper so verify-task-run and batch imports do not gain hidden disk side effects.
7. Add dry fixtures or focused regression guards proving artifact validation planning and the invalid session-trace case are covered.
8. Add a live regression command that expects node scripts/verify-task-contract.mjs --commit=7f462d17 .missiond/tasks/wave51/wave51-01-autopilot-concurrent-slot-dispatch-v0.lisp to fail on the invalid session-trace artifact, not on commit message or scope.
9. Update .missiond/v3/missiond-blueprint.lisp and scripts/check-v3-task-lifecycle-isomorphism.mjs so this V3 task-runner-cli invariant is pinned.
10. Write the task report and commit only the declared write scope.

## Acceptance Commands

```bash
node scripts/verify-task-contract.mjs --dry-fixture
node scripts/check-v3-task-lifecycle-isomorphism.mjs --dry-fixture
node scripts/check-v3-task-lifecycle-isomorphism.mjs
node scripts/check-v3-code-isomorphism-complete.mjs
if node scripts/verify-task-contract.mjs --commit=7f462d17 .missiond/tasks/wave51/wave51-01-autopilot-concurrent-slot-dispatch-v0.lisp >/tmp/wave52-invalid-trace.out 2>&1; then cat /tmp/wave52-invalid-trace.out; exit 1; else rg "session-trace|acceptance|artifact" /tmp/wave52-invalid-trace.out; fi
node scripts/check-task-report.mjs .missiond/tasks/wave52/reports/wave52-01-contract-artifact-validation-v0.report.lisp
git diff --check -- scripts/verify-task-contract.mjs .missiond/v3/missiond-blueprint.lisp scripts/check-v3-task-lifecycle-isomorphism.mjs .missiond/tasks/wave52/reports/wave52-01-contract-artifact-validation-v0.report.lisp
```

## Shared Protocol

Read `.missiond/claudecode/wave52-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load declared context surfaces before broad repository search; use atlas/card anchors and the latest context-pack integration-plan to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add "scripts/verify-task-contract.mjs" \
        ".missiond/v3/missiond-blueprint.lisp" \
        "scripts/check-v3-task-lifecycle-isomorphism.mjs" \
        ".missiond/tasks/wave52/shared-memory.lisp" \
        ".missiond/tasks/wave52/session-trace.lisp" \
        ".missiond/tasks/wave52/reports/wave52-01-contract-artifact-validation-v0.report.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave52/wave52-01-contract-artifact-validation-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave52/wave52-01-contract-artifact-validation-v0.lisp \
  git commit -m "fix(tasks): validate lisp artifacts during contract verify"
node scripts/verify-task-contract.mjs .missiond/tasks/wave52/wave52-01-contract-artifact-validation-v0.lisp
```

## Report

- `Commit hash.`
- `Artifact detection rules added to verify-task-contract.`
- `How commit-specific artifact bytes are validated.`
- `Evidence that wave51 commit 7f462d17 now fails for invalid session-trace.`
- `Acceptance command results.`
