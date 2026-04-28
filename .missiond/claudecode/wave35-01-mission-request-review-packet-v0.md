# wave35-01-mission-request-review-packet-v0 — mission_request review packet v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave35/wave35-01-mission-request-review-packet-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave35-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `45`
- heartbeat_minutes: `10`
- shared_memory: `.missiond/tasks/wave35/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave35/reports/wave35-01-mission-request-review-packet-v0.report.lisp`
- session_trace: `.missiond/tasks/wave35/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave35/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave35/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave35/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave35/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Project the V3 unified-entry review contract into mission_request responses: when request-local intent-alignment.lisp or plan.lisp exists, mission_request must return a compact review packet that tells the caller what artifact should be shown to the human, what approval state it represents, and which next action is expected. This is an interface contract only; do not auto-approve or auto-dispatch work.

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
- `.missiond/tasks/wave35/manifest.lisp`
- `.missiond/tasks/wave35/context-atlas.lisp`
- `.missiond/tasks/wave35/pattern-cards.lisp`
- `.missiond/tasks/wave35/wave35-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Update .missiond/v3/missiond-blueprint.lisp first. Add a compact review-packet contract under mission_request / unified-entry stating that human_interactive mode surfaces intent-alignment for intent approval, then plan.lisp for plan approval; trusted_agent may still fold intent into plan only through existing policy gates.
2. Add a `review_packet` object to mission_request start/advance/status responses when request paths are known. It should be deterministic, compact, and safe to show in a UI or CLI without re-reading files. Suggested fields: state, artifact_kind, artifact_path, artifact_exists, artifact_preview, prompt, allowed_responses, next_action, execute_allowed.
3. Derive review_packet from request-local artifact existence and the latest projection result. If intent-alignment.lisp exists and plan.lisp does not, state should be awaiting_intent_approval. If plan.lisp exists, state should be awaiting_plan_approval unless execution has already been explicitly requested. If neither exists, state should remain received or intent_drafting depending on available local facts.
4. Do not implement automatic approval, automatic execution, DB migrations, or workstation dispatch. This wave is only the review surface projection for the unified entry contract.
5. Use safe byte truncation for artifact_preview so Chinese text never panics on UTF-8 boundaries.
6. Update the MCP schema/description to document review_packet. Preserve additionalProperties=true and existing action names.
7. Add focused unit tests in request.rs for pure review_packet derivation and UTF-8-safe preview. Avoid AppState construction.

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

Read `.missiond/claudecode/wave35-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
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
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave35/wave35-01-mission-request-review-packet-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave35/wave35-01-mission-request-review-packet-v0.lisp \
  git commit -m "feat(request): surface mission request review packet"
node scripts/verify-task-contract.mjs .missiond/tasks/wave35/wave35-01-mission-request-review-packet-v0.lisp
```

## Report

- `Commit hash.`
- `V3 review-packet contract added.`
- `Response fields added and state derivation rules.`
- `Why no auto-approval / auto-dispatch was added.`
- `Acceptance command results.`

