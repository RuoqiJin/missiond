# wave33-01-autopilot-prompt-contract-v0 — Autopilot prompt/tool contract Lisp projection v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave33/wave33-01-autopilot-prompt-contract-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave33-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `35`
- heartbeat_minutes: `10`
- report_contract: `.missiond/tasks/wave33/reports/wave33-01-autopilot-prompt-contract-v0.report.lisp`
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave33/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave33/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave33/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave33/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Align Autopilot's ClaudeCode prompt construction with the V3 Lisp workstation-config contract so delegated coding tasks no longer show duplicated objective text or unconditional instructions to call board MCP tools that may not be attached to the slot.

## Ownership

- `crates/missiond-daemon/src/engine/intent_engine/autopilot.rs`
- `.missiond/v3/missiond-blueprint.lisp`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/compute/task_delegate.rs`
- `crates/missiond-daemon/src/handlers/compute/compute_slot.rs`
- `crates/missiond-daemon/src/handlers/compute/pty.rs`
- `crates/missiond-core/**`
- `crates/missiond-mcp/**`
- `scripts/**`
- `.missiond/v1/**`
- `.missiond/v2/**`
- `.missiond/research/**`
- `.missiond/tasks/schema/**`
- `.missiond/tasks/wave31/**`
- `.missiond/tasks/wave32/**`
- `.missiond/tasks/wave33/manifest.lisp`
- `.missiond/tasks/wave33/dispatch-plan.lisp`
- `.missiond/tasks/wave33/context-atlas.lisp`
- `.missiond/tasks/wave33/pattern-cards.lisp`
- `.missiond/tasks/wave33/wave33-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Add a compact prompt/tool contract under .missiond/v3/missiond-blueprint.lisp workstation-config. It should say: Board Task ID is always surfaced; worker self-close via board MCP tools is conditional on tool availability; if tools are unavailable, the worker should return a concise completion summary and Autopilot/orchestrator remains responsible for closing the task; prompt assembly must avoid duplicating title/objective text.
2. Update the workstation-config implementation-map note to state that Autopilot prompt assembly projects this V3 prompt/tool contract.
3. In autopilot.rs, extract the title/description/template prompt assembly into a pure helper. The helper must suppress duplicated objective text when description is exactly the title or starts with title followed by blank lines. Distinct title + description should still render both.
4. Replace the unconditional self-close wording that says the worker must call mission_board_update / mission_board_note_add. New wording must be conditional and explicit that returning a final summary is acceptable when board tools are absent.
5. Keep Decision Engine escalation guidance and ops-task guidance behaviorally intact, except for any helper extraction needed to make the code testable.
6. Add focused pure unit tests in autopilot.rs for duplicate objective suppression and conditional board-tool completion instructions. Do not construct AppState in tests.

## Acceptance Commands

```bash
cargo test -p missiond-daemon engine::intent_engine::autopilot::tests
cargo check -p missiond-daemon
node scripts/check-lisp-blueprint-compression.mjs
node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp
perl -ne 'exit 1 if /\x00/' crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp
git diff --check -- crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp
```

## Shared Protocol

Read `.missiond/claudecode/wave33-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load the context atlas / pattern card before broad repository search; use their anchors to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs" \
        ".missiond/v3/missiond-blueprint.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave33/wave33-01-autopilot-prompt-contract-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave33/wave33-01-autopilot-prompt-contract-v0.lisp \
  git commit -m "fix(autopilot): project prompt tool contract"
node scripts/verify-task-contract.mjs .missiond/tasks/wave33/wave33-01-autopilot-prompt-contract-v0.lisp
```

## Report

- `Commit hash.`
- `Prompt/tool contract added to V3 blueprint.`
- `Prompt helper behavior and test coverage.`
- `Exact replacement wording for board-tool self-close instructions.`
- `Acceptance command results.`

