# wave37-01-request-verification-receipt-v0 — request-local verification receipt projection v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave37/wave37-01-request-verification-receipt-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave37-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `45`
- heartbeat_minutes: `10`
- shared_memory: `.missiond/tasks/wave37/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave37/reports/wave37-01-request-verification-receipt-v0.report.lisp`
- session_trace: `.missiond/tasks/wave37/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave37/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave37/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave37/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave37/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Close the next task-runner Lisp-isomorphism gap: verification receipts should have a request-local writer/projection under .missiond/requests/<request_id>/receipts/<receipt_id>.lisp, while the legacy task-scoped receipt set remains a compatibility input. Keep receipt reuse advisory and keep existing batch-verifier behavior backward-compatible.

## Ownership

- `scripts/check-verification-receipt.mjs`
- `scripts/check-v3-task-lifecycle-isomorphism.mjs`
- `scripts/verify-task-runner-batch.mjs`
- `.missiond/v3/missiond-blueprint.lisp`

## Must Not Touch

- `crates/**`
- `packages/**`
- `.missiond/v1/**`
- `.missiond/v2/**`
- `.missiond/research/**`
- `.missiond/tasks/schema/**`
- `.missiond/tasks/wave29/**`
- `.missiond/tasks/wave30/**`
- `.missiond/tasks/wave31/**`
- `.missiond/tasks/wave32/**`
- `.missiond/tasks/wave33/**`
- `.missiond/tasks/wave34/**`
- `.missiond/tasks/wave35/**`
- `.missiond/tasks/wave36/**`
- `.missiond/tasks/wave37/manifest.lisp`
- `.missiond/tasks/wave37/context-atlas.lisp`
- `.missiond/tasks/wave37/pattern-cards.lisp`
- `.missiond/tasks/wave37/wave37-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Update .missiond/v3/missiond-blueprint.lisp first. The task-runner surface should state that verification receipts can be projected to .missiond/requests/<request_id>/receipts/<receipt_id>.lisp, while legacy receipt-set files remain compatibility inputs.
2. Add a deterministic request-local receipt writer/projection. It may live in check-verification-receipt.mjs as exported helpers or in a narrowly named task-runner receipt helper, but it must render a single verification-receipt Lisp artifact with schema missiond.verification-receipt.v1.
3. The writer must validate generated receipt bytes with the existing receipt validator before rename/create. Reject absolute paths, .. traversal, malformed request ids, malformed receipt ids, and invalid receipt objects.
4. Use atomic writes and avoid overwriting unrelated receipt files. If overwrite is supported, it must be explicit; default behavior should be create-only or deterministic safe replace of the same generated artifact.
5. Keep existing verification receipt checking and verify-task-runner-batch --receipts behavior backward-compatible. Existing dry fixtures should still pass without requiring request-local args.
6. Extend check-v3-task-lifecycle-isomorphism.mjs so the V3 Lisp/code contract pins the request-local receipt writer path and helper names.
7. Add at least one fixture that writes a request-local receipt under a temp .missiond/requests/<request_id>/receipts directory and then validates it through check-verification-receipt.mjs.
8. Optionally update verify-task-runner-batch.mjs only for a cross-layer smoke fixture; do not change its default JSON shape when --receipts is omitted.

## Acceptance Commands

```bash
node scripts/check-verification-receipt.mjs --dry-fixture
node scripts/check-v3-task-lifecycle-isomorphism.mjs --dry-fixture
node scripts/check-v3-task-lifecycle-isomorphism.mjs
node scripts/verify-task-runner-batch.mjs --dry-fixture
node scripts/check-lisp-blueprint-compression.mjs
node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp
perl -ne 'exit 1 if /\x00/' scripts/check-verification-receipt.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/verify-task-runner-batch.mjs .missiond/v3/missiond-blueprint.lisp
git diff --check -- scripts/check-verification-receipt.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/verify-task-runner-batch.mjs .missiond/v3/missiond-blueprint.lisp
```

## Shared Protocol

Read `.missiond/claudecode/wave37-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load the context atlas / pattern card before broad repository search; use their anchors to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add "scripts/check-verification-receipt.mjs" \
        "scripts/check-v3-task-lifecycle-isomorphism.mjs" \
        "scripts/verify-task-runner-batch.mjs" \
        ".missiond/v3/missiond-blueprint.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave37/wave37-01-request-verification-receipt-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave37/wave37-01-request-verification-receipt-v0.lisp \
  git commit -m "feat(tasks): project request verification receipts"
node scripts/verify-task-contract.mjs .missiond/tasks/wave37/wave37-01-request-verification-receipt-v0.lisp
```

## Report

- `Commit hash.`
- `Request-local verification-receipt artifact shape.`
- `Writer/projection helper or CLI entrypoint.`
- `Backward-compat behavior for existing receipt-set inputs.`
- `Acceptance command results.`

