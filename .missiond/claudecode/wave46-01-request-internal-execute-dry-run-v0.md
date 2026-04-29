# wave46-01-request-internal-execute-dry-run-v0 — v3 request internal execute dry-run v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave46/wave46-01-request-internal-execute-dry-run-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave46-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `smoke`
- dispatch_group: `A`
- estimated_minutes: `60`
- heartbeat_minutes: `10`
- shared_memory: `.missiond/tasks/wave46/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave46/reports/wave46-01-request-internal-execute-dry-run-v0.report.lisp`
- session_trace: `.missiond/tasks/wave46/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave46/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave46/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave46/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave46/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Wave45 proved mission_request can drive execute_plan without consuming a workstation slot, but the observed no-dispatch proof was bridge mode (status=bridge_ready / runner_status=bridge_only). Tighten the Lisp/code isomorphism: --execute-dry-run must explicitly pass execute_mode=internal and dispatch_strategy=agent-team so the live smoke reaches mission_plan's internal workstation-dispatch dry-run substrate and proves status=dry_run plus workstation_dispatch_status=dry_run_no_dispatch.

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
- `.missiond/tasks/wave45/**`
- `.missiond/tasks/wave46/manifest.lisp`
- `.missiond/tasks/wave46/context-atlas.lisp`
- `.missiond/tasks/wave46/pattern-cards.lisp`
- `.missiond/tasks/wave46/wave46-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Update the V3 blueprint first. Refine (execute-dry-run-smoke ...) so its audit path explicitly declares execute_mode=internal, dispatch_strategy=agent-team, dry_run=true, target=mission_task_delegate, and a workstation-dispatch dry-run no-dispatch proof.
2. Extend scripts/check-v3-request-flow-smoke.mjs so --execute-dry-run sends execute_mode='internal' and dispatch_strategy='agent-team' on the execute_plan respond call. Do not alter default --live-ipc behavior; it must still stop at awaiting_execution and must not call execute_plan.
3. In --execute-dry-run mode, assert the existing review-level invariants from wave45 still hold: respond outcome=dispatched, inner_action=unified_entry::plan_execute, respond_result.execute=true, review_packet.state=execute_requested, allowed_responses=[observe], and a request-local execute_plan event is appended.
4. Add internal-path assertions: pipeline_result.execute_mode='internal', status='dry_run', runner_status='workstation_dispatch_v0', workstation_dispatch_status='dry_run_no_dispatch', target_tool='mission_task_delegate', dispatch_strategy='agent-team', and task_brief_preview is present. This proves MissionD reached the workstation dispatch substrate but did not spawn a slot.
5. Keep wave44 request-local cleanup guarantees: default flow omits compat_write_file, compat_write_audit remains clean, and --cleanup removes only .missiond/requests/<request_id>/.
6. Only touch Rust/MCP if the live internal dry-run reveals a real forwarding/schema bug. Parent probe before dispatch already observed the current daemon returns status=dry_run, runner_status=workstation_dispatch_v0, workstation_dispatch_status=dry_run_no_dispatch when execute_mode=internal + dry_run=true + dispatch_strategy=agent-team.
7. Preserve daemon-free behavior for default and --dry-fixture; the aggregate v3 gate must still run without live IPC and without executing.

## Acceptance Commands

```bash
node scripts/check-v3-request-flow-smoke.mjs --dry-fixture
node scripts/check-v3-request-flow-smoke.mjs
node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave46-request-internal-dry-run-v0 --cleanup
node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave46-request-internal-dry-run-v0-exec --cleanup --execute-dry-run
node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave46-request-internal-dry-run-v0-json --cleanup --execute-dry-run --json
node scripts/check-v3-code-isomorphism-complete.mjs
node scripts/check-lisp-blueprint-compression.mjs
node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp
cargo test -p missiond-daemon handlers::knowledge::request::tests
cargo test -p missiond-mcp test_directive_plan_workflow_surfaces_registered
perl -ne 'exit 1 if /\x00/' scripts/check-v3-request-flow-smoke.mjs .missiond/v3/missiond-blueprint.lisp crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/request.rs
git diff --check -- scripts/check-v3-request-flow-smoke.mjs .missiond/v3/missiond-blueprint.lisp crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/request.rs
```

## Shared Protocol

Read `.missiond/claudecode/wave46-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
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
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave46/wave46-01-request-internal-execute-dry-run-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave46/wave46-01-request-internal-execute-dry-run-v0.lisp \
  git commit -m "feat(v3): require internal execute dry-run smoke"
node scripts/verify-task-contract.mjs .missiond/tasks/wave46/wave46-01-request-internal-execute-dry-run-v0.lisp
```

## Report

- `Commit hash.`
- `Whether Rust/MCP changed, or why blueprint + checker were sufficient.`
- `Default live IPC behavior versus --execute-dry-run behavior.`
- `The exact internal no-dispatch proof observed in pipeline_result.`
- `Whether a workstation slot or BoardTask was consumed by the smoke; expected answer is no slot consumed, only DB audit rows and request-local files.`
- `Whether any request-local or compatibility artifacts were left behind after --cleanup.`
- `Acceptance command results.`

