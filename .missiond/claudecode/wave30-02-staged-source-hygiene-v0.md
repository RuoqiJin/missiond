# wave30-02-staged-source-hygiene-v0 — Staged source hygiene v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave30/wave30-02-staged-source-hygiene-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave30-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `35`
- heartbeat_minutes: `10`
- report_contract: `.missiond/tasks/wave30/reports/wave30-02-staged-source-hygiene-v0.report.lisp`
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave30/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave30/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave30/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave30/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Promote the Wave29 NUL-byte/diff-check lessons into a reusable staged source hygiene preflight that MissionD can run before final report projection and commit handoff.

## Ownership

- `scripts/check-staged-source-hygiene.mjs`
- `scripts/check-missiond-hooks.mjs`
- `scripts/install-missiond-hooks.mjs`
- `.githooks/pre-commit`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/research/**`
- `.missiond/router/**`
- `.missiond/tasks/schema/**`
- `.missiond/tasks/wave28/**`
- `.missiond/tasks/wave29/**`
- `.missiond/tasks/wave30/wave30-*.lisp`
- `.missiond/tasks/wave30/manifest.lisp`
- `.missiond/tasks/wave30/dispatch-plan.lisp`
- `.missiond/claudecode/**`
- `scripts/task-runner-finalize-report.mjs`
- `scripts/task-runner-parent-hotfix.mjs`
- `scripts/task-runner-append-event.mjs`
- `scripts/check-task-lifecycle-events.mjs`
- `scripts/project-task-lifecycle-ledger.mjs`
- `scripts/check-task-report.mjs`
- `scripts/check-verification-receipt.mjs`
- `scripts/verify-task-runner-batch.mjs`
- `scripts/plan-task-runner.mjs`
- `scripts/check-task-runner-manifest.mjs`
- `scripts/render-wave-briefs.mjs`
- `scripts/prepare-task-runner-wave.mjs`

## Requirements

1. Create check-staged-source-hygiene.mjs with named exports and --dry-fixture. It should check staged or supplied files for raw NUL bytes, diff whitespace errors, and task-scope guard readiness.
2. Default operation must be read-only diagnostics. If hook integration is added, keep repo-local opt-in behavior and do not silently install global hooks.
3. Integrate the new checker into .githooks/pre-commit only behind existing MISSIOND_TASK_CONTRACT/repo-local guard semantics.
4. Update hook doctor output so it can report whether staged-source hygiene is available, without requiring git config mutation.
5. Fixture NUL detection using temp files or escaped byte writes inside the fixture; do not leave raw NUL bytes in repository source.

## Acceptance Commands

```bash
node scripts/check-staged-source-hygiene.mjs --dry-fixture
node scripts/check-missiond-hooks.mjs --dry-fixture
node scripts/install-missiond-hooks.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
perl -ne 'exit 1 if /\x00/' scripts/check-staged-source-hygiene.mjs scripts/check-missiond-hooks.mjs scripts/install-missiond-hooks.mjs .githooks/pre-commit
git diff --check -- scripts/check-staged-source-hygiene.mjs scripts/check-missiond-hooks.mjs scripts/install-missiond-hooks.mjs .githooks/pre-commit
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
git add "scripts/check-staged-source-hygiene.mjs" \
        "scripts/check-missiond-hooks.mjs" \
        "scripts/install-missiond-hooks.mjs" \
        ".githooks/pre-commit"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave30/wave30-02-staged-source-hygiene-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave30/wave30-02-staged-source-hygiene-v0.lisp \
  git commit -m "feat(tasks): check staged source hygiene"
node scripts/verify-task-contract.mjs .missiond/tasks/wave30/wave30-02-staged-source-hygiene-v0.lisp
```

## Report

- `Commit hash.`
- `Source hygiene checks implemented.`
- `Hook integration and mutation boundary.`
- `NUL byte fixture result.`
- `Acceptance command results.`

