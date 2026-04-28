# wave36-01-mission-request-review-response-v0 — mission_request review response v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave36/wave36-01-mission-request-review-response-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave36-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `60`
- heartbeat_minutes: `10`
- shared_memory: `.missiond/tasks/wave36/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave36/reports/wave36-01-mission-request-review-response-v0.report.lisp`
- session_trace: `.missiond/tasks/wave36/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave36/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave36/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave36/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave36/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Close the next gap in the user-facing unified entry loop: after mission_request returns a review_packet, callers should be able to send an explicit review response back to mission_request instead of knowing the internal mission_directive / mission_plan surfaces. Implement a narrow v0 adapter for approve/reject/question decisions while preserving the existing directive/plan gates and avoiding autonomous execution.

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
- `packages/**`
- `.missiond/v1/**`
- `.missiond/v2/**`
- `.missiond/research/**`
- `.missiond/tasks/schema/**`
- `.missiond/tasks/wave31/**`
- `.missiond/tasks/wave32/**`
- `.missiond/tasks/wave33/**`
- `.missiond/tasks/wave34/**`
- `.missiond/tasks/wave35/**`
- `.missiond/tasks/wave36/manifest.lisp`
- `.missiond/tasks/wave36/context-atlas.lisp`
- `.missiond/tasks/wave36/pattern-cards.lisp`
- `.missiond/tasks/wave36/wave36-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Update .missiond/v3/missiond-blueprint.lisp first. Extend the unified-entry / mission_request contract with a review-response adapter: callers may answer a review_packet with approve_intent, reject_intent, ask_question, approve_plan, reject_plan, or execute_plan only through mission_request.
2. Add `respond` to mission_request actions. Inputs should include request_id, response (or decision), optional note, optional board_task_id, optional execute, and the same project/cwd/target_project root-resolution fields as status. Preserve existing start/advance/status behavior and response shape.
3. For approve_intent, require a persisted directive id/ref from the latest pipeline artifacts or an explicit approved_directive_id/directive_id argument. The adapter may call the existing mission_directive approve surface and then the existing unified-entry advance path to produce plan.lisp. If the needed ref is missing, return a structured blocked response with next_action instead of guessing.
4. For approve_plan / execute_plan, require a persisted plan id/ref or explicit approved_plan_id/plan_id. approve_plan should not execute by default; execute_plan requires execute=true (or response=execute_plan) and must still route through the existing mission_plan execute path. Never spawn work directly.
5. For reject_intent, reject_plan, and ask_question, do not mutate directive/plan approval state. Return a structured recorded/blocked response and append a request-local review event Lisp file under .missiond/requests/<id>/events so the user decision is auditable.
6. Persist minimal request-local response events for every respond call using atomic file writes and monotonically increasing local event sequence. Keep the event shape Lisp-first and documented in the V3 blueprint.
7. Keep review_packet derivation pure. After respond, return the latest review_packet plus respond_result, inner approval/advance payloads when invoked, and clear next_action text.
8. Update the MCP schema/description to document action=respond and response/decision fields. Preserve additionalProperties=true.
9. Add focused unit tests for pure response parsing, event sequencing/path choice, blocked missing-ref responses, and no-execute-by-default behavior. Avoid AppState-heavy tests where pure helpers are enough.

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

Read `.missiond/claudecode/wave36-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
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
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave36/wave36-01-mission-request-review-response-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave36/wave36-01-mission-request-review-response-v0.lisp \
  git commit -m "feat(request): accept mission request review responses"
node scripts/verify-task-contract.mjs .missiond/tasks/wave36/wave36-01-mission-request-review-response-v0.lisp
```

## Report

- `Commit hash.`
- `V3 review-response contract added.`
- `respond action inputs and response shape.`
- `Which paths still require persisted directive/plan refs.`
- `Why execution remains explicitly gated.`
- `Acceptance command results.`

