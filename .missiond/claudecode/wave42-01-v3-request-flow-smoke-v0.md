# wave42-01-v3-request-flow-smoke-v0 — v3 request flow smoke v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave42/wave42-01-v3-request-flow-smoke-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave42-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `smoke`
- dispatch_group: `A`
- estimated_minutes: `50`
- heartbeat_minutes: `10`
- report_contract: `.missiond/tasks/wave42/reports/wave42-01-v3-request-flow-smoke-v0.report.lisp`
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave42/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave42/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave42/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave42/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Add a V3 request-flow smoke checker that proves the user-facing MissionD path the user actually cares about: a request produces request-local Lisp review artifacts, approve_intent leads to plan.lisp, approve_plan reaches awaiting_execution, and execute_plan remains an explicit execution gate. This should graduate the post-wave41 state from per-surface code-isomorphism to an executable cross-surface flow pin.

## Ownership

- `.missiond/v3/missiond-blueprint.lisp`
- `scripts/check-v3-request-flow-smoke.mjs`
- `scripts/check-v3-code-isomorphism-complete.mjs`
- `scripts/check-lisp-blueprint-compression.mjs`
- `crates/missiond-daemon/src/handlers/knowledge/request.rs`
- `crates/missiond-mcp/src/tools/knowledge/request.rs`

## Must Not Touch

- `packages/**`
- `.missiond/v1/**`
- `.missiond/v2/**`
- `.missiond/research/**`
- `.missiond/router/**`
- `.missiond/tasks/wave28/**`
- `.missiond/tasks/wave29/**`
- `.missiond/tasks/wave30/**`
- `.missiond/tasks/wave31/**`
- `.missiond/tasks/wave32/**`
- `.missiond/tasks/wave33/**`
- `.missiond/tasks/wave34/**`
- `.missiond/tasks/wave35/**`
- `.missiond/tasks/wave36/**`
- `.missiond/tasks/wave37/**`
- `.missiond/tasks/wave38/**`
- `.missiond/tasks/wave39/**`
- `.missiond/tasks/wave40/**`
- `.missiond/tasks/wave41/**`
- `.missiond/tasks/wave42/manifest.lisp`
- `.missiond/tasks/wave42/context-atlas.lisp`
- `.missiond/tasks/wave42/pattern-cards.lisp`
- `.missiond/tasks/wave42/wave42-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Start from .missiond/v3/missiond-blueprint.lisp. Treat the V3 unified-entry/review-packet/review-response clauses as the source of truth. If you find drift, update the Lisp contract first and then the checker/code to match.
2. Add scripts/check-v3-request-flow-smoke.mjs. It must be deterministic, read-only by default, support --json and --dry-fixture, and validate the cross-surface user flow from request-local Lisp artifacts rather than only string needles. Use the shared Lisp parser where practical.
3. Dry fixtures must cover at least: request-only received/default packet; intent-alignment.lisp present with :directive_id + :version -> awaiting_intent_approval; plan.lisp present before approval -> awaiting_plan_approval; plan.lisp with :plan_id/:version/:board_task_id plus an approve_plan review event -> awaiting_execution with execute_plan allowed; execute_plan event -> execute_requested/observe; malformed/missing persisted refs produce a failure diagnostic.
4. Default acceptance must not dispatch a real workstation task. If you add an optional --live-ipc mode, keep it opt-in and stop before real execution unless a second explicit flag is supplied.
5. Add the new smoke checker to the V3 compression-contract :checks list and to scripts/check-v3-code-isomorphism-complete.mjs as a cross-surface check. The aggregate gate must still run every existing per-surface checker.
6. Update scripts/check-lisp-blueprint-compression.mjs narrowly if it pins the compression-contract command set.
7. Only edit crates/missiond-daemon/src/handlers/knowledge/request.rs or crates/missiond-mcp/src/tools/knowledge/request.rs if the new smoke exposes real drift. Keep fixes surgical and backed by the existing request.rs tests.

## Acceptance Commands

```bash
node scripts/check-v3-request-flow-smoke.mjs --dry-fixture
node scripts/check-v3-request-flow-smoke.mjs
node scripts/check-v3-code-isomorphism-complete.mjs
node scripts/check-v3-request-lisp-isomorphism.mjs
node scripts/check-lisp-blueprint-compression.mjs
node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp
cargo test -p missiond-daemon handlers::knowledge::request::tests
cargo test -p missiond-mcp test_directive_plan_workflow_surfaces_registered
perl -ne 'exit 1 if /\x00/' .missiond/v3/missiond-blueprint.lisp scripts/check-v3-request-flow-smoke.mjs scripts/check-v3-code-isomorphism-complete.mjs scripts/check-lisp-blueprint-compression.mjs crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs
git diff --check -- .missiond/v3/missiond-blueprint.lisp scripts/check-v3-request-flow-smoke.mjs scripts/check-v3-code-isomorphism-complete.mjs scripts/check-lisp-blueprint-compression.mjs crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs
```

## Shared Protocol

Read `.missiond/claudecode/wave42-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load the context atlas / pattern card before broad repository search; use their anchors to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add ".missiond/v3/missiond-blueprint.lisp" \
        "scripts/check-v3-request-flow-smoke.mjs" \
        "scripts/check-v3-code-isomorphism-complete.mjs" \
        "scripts/check-lisp-blueprint-compression.mjs" \
        "crates/missiond-daemon/src/handlers/knowledge/request.rs" \
        "crates/missiond-mcp/src/tools/knowledge/request.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave42/wave42-01-v3-request-flow-smoke-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave42/wave42-01-v3-request-flow-smoke-v0.lisp \
  git commit -m "feat(v3): add request-flow smoke gate"
node scripts/verify-task-contract.mjs .missiond/tasks/wave42/wave42-01-v3-request-flow-smoke-v0.lisp
```

## Report

- `Commit hash.`
- `What request-flow states the new checker pins.`
- `Whether any Lisp/code drift was found and how it was resolved.`
- `How the checker avoids real workstation dispatch by default.`
- `Acceptance command results.`

