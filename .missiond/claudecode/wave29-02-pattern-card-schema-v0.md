# wave29-02-pattern-card-schema-v0 — Pattern card schema v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave29/wave29-02-pattern-card-schema-v0.lisp`
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
- report_contract: `.missiond/tasks/wave29/reports/wave29-02-pattern-card-schema-v0.report.lisp`
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

Introduce a durable pattern-card schema/checker and seed reusable cards for schema checkers, read-only Node CLIs, report lineage, cross-layer smoke, and large-file navigation. Pattern cards are implementation recipes, not extra task scope.

## Ownership

- `.missiond/tasks/schema/pattern-card-v1.lisp`
- `scripts/check-pattern-card.mjs`
- `.missiond/patterns/schema-checker.pattern.lisp`
- `.missiond/patterns/node-cli-readonly.pattern.lisp`
- `.missiond/patterns/report-lineage.pattern.lisp`
- `.missiond/patterns/cross-layer-smoke.pattern.lisp`
- `.missiond/patterns/large-file-navigation.pattern.lisp`

## Must Not Touch

- `crates/**`
- `.missiond/v2/**`
- `.missiond/router/**`
- `.missiond/tasks/wave28/**`
- `.missiond/tasks/wave29/wave29-*.lisp`
- `.missiond/tasks/wave29/manifest.lisp`
- `.missiond/tasks/wave29/dispatch-plan.lisp`
- `.missiond/tasks/wave29/context-atlas.lisp`
- `.missiond/claudecode/**`
- `scripts/check-context-atlas.mjs`
- `scripts/prepare-task-runner-wave.mjs`
- `scripts/check-task-report.mjs`
- `scripts/verify-task-run.mjs`
- `scripts/verify-task-runner-batch.mjs`
- `scripts/plan-task-runner.mjs`

## Requirements

1. Define schema missiond.pattern-card.v1 in .missiond/tasks/schema/pattern-card-v1.lisp.
2. Checker must validate card id, schema, use_for task ids, recipe vectors, known_good paths, and optional anti-pattern notes.
3. Create five seed pattern cards under .missiond/patterns: schema-checker, node-cli-readonly, report-lineage, cross-layer-smoke, large-file-navigation.
4. Pattern cards must stay repo-relative and read-only guidance. They must not grant scope beyond the task contract.
5. Checker must support --json, --stdin, and --dry-fixture and reuse scripts/lib/missiond_lisp.mjs.
6. Fixtures must include valid multi-card input, duplicate card id rejection, empty recipe rejection, bad path rejection, and invalid use_for id rejection.

## Acceptance Commands

```bash
node scripts/check-pattern-card.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
git diff --check -- .missiond/tasks/schema/pattern-card-v1.lisp scripts/check-pattern-card.mjs .missiond/patterns/schema-checker.pattern.lisp .missiond/patterns/node-cli-readonly.pattern.lisp .missiond/patterns/report-lineage.pattern.lisp .missiond/patterns/cross-layer-smoke.pattern.lisp .missiond/patterns/large-file-navigation.pattern.lisp
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
git add ".missiond/tasks/schema/pattern-card-v1.lisp" \
        "scripts/check-pattern-card.mjs" \
        ".missiond/patterns/schema-checker.pattern.lisp" \
        ".missiond/patterns/node-cli-readonly.pattern.lisp" \
        ".missiond/patterns/report-lineage.pattern.lisp" \
        ".missiond/patterns/cross-layer-smoke.pattern.lisp" \
        ".missiond/patterns/large-file-navigation.pattern.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave29/wave29-02-pattern-card-schema-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave29/wave29-02-pattern-card-schema-v0.lisp \
  git commit -m "feat(tasks): add pattern card schema"
node scripts/verify-task-contract.mjs .missiond/tasks/wave29/wave29-02-pattern-card-schema-v0.lisp
```

## Report

- `Commit hash.`
- `Seed cards created.`
- `Fixture categories.`
- `Acceptance command results.`

