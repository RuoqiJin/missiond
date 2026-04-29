# wave45-01-request-execute-dry-run-smoke-v0 — v3 request execute dry-run smoke v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave45/wave45-01-request-execute-dry-run-smoke-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave45-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `smoke`
- dispatch_group: `A`
- estimated_minutes: `60`
- heartbeat_minutes: `10`
- shared_memory: `.missiond/tasks/wave45/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave45/reports/wave45-01-request-execute-dry-run-smoke-v0.report.lisp`
- session_trace: `.missiond/tasks/wave45/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave45/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave45/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave45/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave45/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

After wave43/44 proved live mission_request through approve_plan and request-local artifacts, make the final execute_plan transition observable in the same smoke checker without consuming a workstation slot: an explicit --execute-dry-run mode must drive start -> approve_intent -> approve_plan -> execute_plan(dry_run=true), then assert request-local plan.lisp and review events move the packet to execute_requested.

## Ownership

- `.missiond/v3/missiond-blueprint.lisp`
- `scripts/check-v3-request-flow-smoke.mjs`
- `crates/missiond-daemon/src/handlers/knowledge/request.rs`
- `crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
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
- `.missiond/tasks/wave42/**`
- `.missiond/tasks/wave43/**`
- `.missiond/tasks/wave44/**`
- `.missiond/tasks/wave45/manifest.lisp`
- `.missiond/tasks/wave45/context-atlas.lisp`
- `.missiond/tasks/wave45/pattern-cards.lisp`
- `.missiond/tasks/wave45/wave45-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Update the V3 blueprint first: document that mission_request has an explicit live execute dry-run audit mode for checker/smoke use. Default mission_request smoke still stops at awaiting_execution; only --execute-dry-run may call execute_plan, and it must pass dry_run=true.
2. Extend scripts/check-v3-request-flow-smoke.mjs with a new opt-in flag, preferred name --execute-dry-run. Do not repurpose the default live smoke. Keep --confirm-execute reserved or backwards-compatible; the new flag is the one acceptance should use.
3. In --execute-dry-run mode, after approve_plan succeeds, call mission_request respond with response=execute_plan, execute=true, dry_run=true, cwd=<repo>, target=mission_task_delegate, and the same smoke objective.
4. Assert the execute_plan response is dispatched, inner_action is unified_entry::plan_execute, respond_result.execute is true, review_packet.state is execute_requested, allowed_responses is observe-only, and a request-local execute_plan review event is appended.
5. Assert the inner execution payload proves no workstation slot was consumed. Current live probe returns status=bridge_ready and runner_status=bridge_only; if the newer code returns dry_run/dry_run_no_dispatch instead, accept that equivalent no-dispatch proof. Do not require a real worker dispatch.
6. Keep wave44 guarantees: default --live-ipc omits write_file, compat_write_audit still asserts no .missiond/alignment/<request_id> and no .missiond/plans/* compat leak. --cleanup must remove only the request-local directory.
7. Only touch Rust/MCP files if the live execute dry-run exposes a real code/schema bug. If JS checker + blueprint are enough, leave Rust/MCP unchanged and state that in the report.
8. Preserve daemon-free behavior for default and --dry-fixture; the aggregate v3 gate must still run without live IPC and without executing.

## Acceptance Commands

```bash
node scripts/check-v3-request-flow-smoke.mjs --dry-fixture
node scripts/check-v3-request-flow-smoke.mjs
node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave45-request-execute-dry-run-v0 --cleanup
node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave45-request-execute-dry-run-v0-exec --cleanup --execute-dry-run
node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave45-request-execute-dry-run-v0-json --cleanup --execute-dry-run --json
node scripts/check-v3-code-isomorphism-complete.mjs
node scripts/check-lisp-blueprint-compression.mjs
node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp
cargo test -p missiond-daemon handlers::knowledge::request::tests
cargo test -p missiond-mcp test_directive_plan_workflow_surfaces_registered
perl -ne 'exit 1 if /\x00/' scripts/check-v3-request-flow-smoke.mjs .missiond/v3/missiond-blueprint.lisp crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/request.rs
git diff --check -- scripts/check-v3-request-flow-smoke.mjs .missiond/v3/missiond-blueprint.lisp crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/request.rs
```

## Shared Protocol

Read `.missiond/claudecode/wave45-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load the context atlas / pattern card before broad repository search; use their anchors to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add ".missiond/v3/missiond-blueprint.lisp" \
        "scripts/check-v3-request-flow-smoke.mjs" \
        "crates/missiond-daemon/src/handlers/knowledge/request.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/plan.rs" \
        "crates/missiond-mcp/src/tools/knowledge/request.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave45/wave45-01-request-execute-dry-run-smoke-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave45/wave45-01-request-execute-dry-run-smoke-v0.lisp \
  git commit -m "feat(v3): add request execute dry-run smoke"
node scripts/verify-task-contract.mjs .missiond/tasks/wave45/wave45-01-request-execute-dry-run-smoke-v0.lisp
```

## Report

- `Commit hash.`
- `Whether Rust/MCP changed, or why blueprint + checker were sufficient.`
- `Default live IPC behavior versus --execute-dry-run behavior.`
- `The exact no-dispatch proof observed in pipeline_result (bridge_only/bridge_ready or dry_run_no_dispatch).`
- `Whether any request-local or compatibility artifacts were left behind after --cleanup.`
- `Acceptance command results.`

