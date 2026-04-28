# wave31-01-mission-request-local-projections-v0 — mission_request request-local projections v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave31/wave31-01-mission-request-local-projections-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave31-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `45`
- heartbeat_minutes: `10`
- report_contract: `.missiond/tasks/wave31/reports/wave31-01-mission-request-local-projections-v0.report.lisp`
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave31/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave31/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave31/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave31/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Advance mission_request from request.lisp + initial event + compatibility pipeline into request-local Lisp projections, so a single request directory can hold request.lisp, intent-alignment.lisp, plan.lisp, events, receipts, and reports.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/request.rs`
- `crates/missiond-mcp/src/tools/knowledge/request.rs`
- `.missiond/v3/missiond-blueprint.lisp`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/directive.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs`
- `crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs`
- `crates/missiond-daemon/src/handlers/mod.rs`
- `crates/missiond-mcp/src/tools/mod.rs`
- `scripts/**`
- `.missiond/v1/**`
- `.missiond/v2/**`
- `.missiond/research/**`
- `.missiond/tasks/schema/**`
- `.missiond/tasks/wave30/**`
- `.missiond/tasks/wave31/manifest.lisp`
- `.missiond/tasks/wave31/dispatch-plan.lisp`
- `.missiond/tasks/wave31/context-atlas.lisp`
- `.missiond/tasks/wave31/pattern-cards.lisp`
- `.missiond/tasks/wave31/wave31-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Keep mission_request(action=start) conservative: it may write request-local projections, but must not auto-approve intent or plan and must not dispatch workstation work directly.
2. After the existing unified_entry call returns, project stable inner compile payloads into request-local files. Directive compile stages write .missiond/requests/<request_id>/intent-alignment.lisp from compiled_sexp or compiled_sexp_preview. Plan compile stages write .missiond/requests/<request_id>/plan.lisp from compiled_sexp or compiled_sexp_preview.
3. Use the existing request_paths + atomic_write_artifact flow. Respect overwrite_file for these projections. If projection is not possible because the pipeline stage is execute/error/missing sexp, return a clear projection status in the mission_request wrapper instead of failing the whole call.
4. mission_request(action=status) should expose request-local artifact paths and existence booleans for request, intent_alignment, plan, events_dir, receipts_dir, and reports_dir. It should still read request.lisp as before.
5. Update the MCP tool schema/description only if new response fields or args need to be documented; keep additionalProperties=true for compatibility.
6. Update .missiond/v3/missiond-blueprint.lisp implementation-map note/status if the code alignment moves beyond the current partial v0.
7. Add focused unit tests in request.rs for pure extraction/projection helpers, including directive preview projection, plan preview projection, no-sexpr no-op status, and status artifact existence shape if practical without constructing AppState.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::request::tests
cargo test -p missiond-mcp test_directive_plan_workflow_surfaces_registered
cargo check -p missiond-daemon
cargo check -p missiond-mcp
node scripts/check-lisp-blueprint-compression.mjs
node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp
perl -ne 'exit 1 if /\x00/' crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs .missiond/v3/missiond-blueprint.lisp
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs .missiond/v3/missiond-blueprint.lisp
```

## Shared Protocol

Read `.missiond/claudecode/wave31-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load the context atlas / pattern card before broad repository search; use their anchors to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/request.rs" \
        "crates/missiond-mcp/src/tools/knowledge/request.rs" \
        ".missiond/v3/missiond-blueprint.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave31/wave31-01-mission-request-local-projections-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave31/wave31-01-mission-request-local-projections-v0.lisp \
  git commit -m "feat(architecture): project request-local lisp artifacts"
node scripts/verify-task-contract.mjs .missiond/tasks/wave31/wave31-01-mission-request-local-projections-v0.lisp
```

## Report

- `Commit hash.`
- `Projection behavior by pipeline stage.`
- `Response/status fields added.`
- `Whether blueprint status/note changed.`
- `Acceptance command results.`

