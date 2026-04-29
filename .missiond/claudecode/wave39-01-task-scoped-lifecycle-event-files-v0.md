# wave39-01-task-scoped-lifecycle-event-files-v0 — task-scoped lifecycle event files v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave39/wave39-01-task-scoped-lifecycle-event-files-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave39-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `55`
- heartbeat_minutes: `10`
- report_contract: `.missiond/tasks/wave39/reports/wave39-01-task-scoped-lifecycle-event-files-v0.report.lisp`
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave39/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave39/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave39/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave39/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Close the event-sourced lifecycle isomorphism gap in V3. The blueprint already declares lifecycle-event as one event per file, but the real task-scoped runner path still appends a compatibility task-lifecycle-events.lisp ledger and only request-local events use standalone .event.lisp files. Make task-scoped lifecycle event files a first-class append/read path while keeping the legacy ledger as a backward-compatible projection/input.

## Ownership

- `.missiond/v3/missiond-blueprint.lisp`
- `.missiond/tasks/schema/task-lifecycle-event-v1.lisp`
- `scripts/check-task-lifecycle-events.mjs`
- `scripts/task-runner-append-event.mjs`
- `scripts/task-runner-wave-state.mjs`
- `scripts/task-runner-next-action.mjs`
- `scripts/task-runner-dispatch.mjs`
- `scripts/task-runner-submit-dispatch.mjs`
- `scripts/check-v3-task-lifecycle-isomorphism.mjs`
- `scripts/verify-task-runner-batch.mjs`

## Must Not Touch

- `crates/**`
- `packages/**`
- `.missiond/v1/**`
- `.missiond/v2/**`
- `.missiond/research/**`
- `.missiond/tasks/wave30/**`
- `.missiond/tasks/wave31/**`
- `.missiond/tasks/wave32/**`
- `.missiond/tasks/wave33/**`
- `.missiond/tasks/wave34/**`
- `.missiond/tasks/wave35/**`
- `.missiond/tasks/wave36/**`
- `.missiond/tasks/wave37/**`
- `.missiond/tasks/wave38/**`
- `.missiond/tasks/wave39/manifest.lisp`
- `.missiond/tasks/wave39/context-atlas.lisp`
- `.missiond/tasks/wave39/pattern-cards.lisp`
- `.missiond/tasks/wave39/wave39-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Update .missiond/v3/missiond-blueprint.lisp first. The task-runner-cli implementation-map should no longer describe task-scoped lifecycle as only a compatibility ledger. It should state the primary task-scoped path is .missiond/tasks/<wave>/events/<seq>.event.lisp and that task-lifecycle-events.lisp remains a compatibility projection/input during migration.
2. Update .missiond/tasks/schema/task-lifecycle-event-v1.lisp to document two accepted shapes: legacy (task-lifecycle-event-log ... (lifecycle-event ...)) and standalone task-scoped event files. The standalone task-scoped event file should be a single lifecycle-event form carrying schema/wave identity or an equivalent explicit header so one file can be validated without reading the legacy ledger.
3. Extend scripts/check-task-lifecycle-events.mjs so it validates standalone task-scoped lifecycle event files in addition to legacy ledgers and request-local event files. It must reject malformed schema, missing wave/task/kind/seq, bad paths, bad commit hashes, and duplicate or non-increasing seq when a directory or multiple files are checked together.
4. Extend scripts/task-runner-append-event.mjs with a task-scoped event-file append mode, preferably --events-dir <dir>. It must allocate the next numeric file under lock, validate the standalone event bytes before atomic create/rename, and optionally continue to update --ledger when supplied for compatibility. Existing --ledger-only callers must remain backward-compatible.
5. Make scripts/task-runner-wave-state.mjs read conventional .missiond/tasks/<wave>/events/*.event.lisp when present, falling back to task-lifecycle-events.lisp. If both exist, merge idempotently or prefer event files without double-counting the same event id.
6. Pass task-scoped events-dir through the real dispatch path: task-runner-next-action.mjs / task-runner-dispatch.mjs / task-runner-submit-dispatch.mjs should be able to append dispatch events to event files, not only the legacy ledger. Preserve existing CLI defaults if needed, but expose and test the new path.
7. Update scripts/check-v3-task-lifecycle-isomorphism.mjs to pin the Lisp contract, schema/checker support, append-event --events-dir path, wave-state consumption, and dispatch/submit pass-through.
8. Add or update dry fixtures in check-task-lifecycle-events, task-runner-append-event, task-runner-wave-state, and verify-task-runner-batch so standalone task-scoped event files are exercised end to end.
9. Keep legacy task-lifecycle-events.lisp readers/writers working. Do not migrate historical waves or rewrite existing wave30-38 files in this task.

## Acceptance Commands

```bash
node scripts/check-task-lifecycle-events.mjs --dry-fixture
node scripts/task-runner-append-event.mjs --dry-fixture
node scripts/task-runner-wave-state.mjs --dry-fixture
node scripts/task-runner-dispatch.mjs --dry-fixture
node scripts/task-runner-submit-dispatch.mjs --dry-fixture
node scripts/verify-task-runner-batch.mjs --dry-fixture
node scripts/check-v3-task-lifecycle-isomorphism.mjs --dry-fixture
node scripts/check-v3-task-lifecycle-isomorphism.mjs
node scripts/check-lisp-blueprint-compression.mjs
node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp
perl -ne 'exit 1 if /\x00/' .missiond/v3/missiond-blueprint.lisp .missiond/tasks/schema/task-lifecycle-event-v1.lisp scripts/check-task-lifecycle-events.mjs scripts/task-runner-append-event.mjs scripts/task-runner-wave-state.mjs scripts/task-runner-next-action.mjs scripts/task-runner-dispatch.mjs scripts/task-runner-submit-dispatch.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/verify-task-runner-batch.mjs
git diff --check -- .missiond/v3/missiond-blueprint.lisp .missiond/tasks/schema/task-lifecycle-event-v1.lisp scripts/check-task-lifecycle-events.mjs scripts/task-runner-append-event.mjs scripts/task-runner-wave-state.mjs scripts/task-runner-next-action.mjs scripts/task-runner-dispatch.mjs scripts/task-runner-submit-dispatch.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/verify-task-runner-batch.mjs
```

## Shared Protocol

Read `.missiond/claudecode/wave39-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load the context atlas / pattern card before broad repository search; use their anchors to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add ".missiond/v3/missiond-blueprint.lisp" \
        ".missiond/tasks/schema/task-lifecycle-event-v1.lisp" \
        "scripts/check-task-lifecycle-events.mjs" \
        "scripts/task-runner-append-event.mjs" \
        "scripts/task-runner-wave-state.mjs" \
        "scripts/task-runner-next-action.mjs" \
        "scripts/task-runner-dispatch.mjs" \
        "scripts/task-runner-submit-dispatch.mjs" \
        "scripts/check-v3-task-lifecycle-isomorphism.mjs" \
        "scripts/verify-task-runner-batch.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave39/wave39-01-task-scoped-lifecycle-event-files-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave39/wave39-01-task-scoped-lifecycle-event-files-v0.lisp \
  git commit -m "feat(tasks): promote lifecycle event files"
node scripts/verify-task-contract.mjs .missiond/tasks/wave39/wave39-01-task-scoped-lifecycle-event-files-v0.lisp
```

## Report

- `Commit hash.`
- `Standalone task-scoped lifecycle event artifact shape.`
- `Append-event CLI and locking/atomic-write behavior.`
- `Wave-state/dispatch/batch verifier compatibility behavior.`
- `Legacy ledger backward-compat guarantees.`
- `Acceptance command results.`

