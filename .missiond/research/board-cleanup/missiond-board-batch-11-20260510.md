# MissionD Board Cleanup Batch 11 - 2026-05-10

Scope: read-only review of 5 MissionD-related BoardTasks already auto-closed by `codex-board-cleanup` on `2026-05-10T08:30:19+00:00`. Two are `Backfill Lisp/checker` rows; three are `架构漂移` strategy-drift advisories. Goal: sample-verify whether the cleanup wave's claim ("covered by SSOT/checker/workflow or superseded by current V3/OCaml/shared-memory architecture") holds against current code, and recommend whether to leave them closed.

This batch was dispatched as `3e04dd4c-2cac-4981-b327-33b9f3a07707`. No Board task statuses were changed by me.

## Summary

| Task ID | Title | Pre-existing Status | Classification | Recommendation |
| --- | --- | --- | --- | --- |
| `65ef7afe-4572-40b2-a3c8-4dd6811e65aa` | `Backfill Lisp/checker for code-first change` (codex_ingestion_worker.rs / db/pg/message.rs) | `skipped` | `close-stale` | Leave closed; drift gate green and the changed files are stable. |
| `b5b65989-6949-4a88-83bc-c52e451cfbed` | `Backfill Lisp/checker for commit 4ab7994` (`mission_execution` 12-action manager) | `skipped` | `close-covered` | Leave closed; `mission_execution` MCP tool and `agent_execution` handler are wired and current spec is even broader (13 actions). |
| `4e0f4400-880a-4768-bcd1-72605ba1ecf5` | `架构漂移: Preferences / Hot Topics 上下文注入载体转移` | `skipped` | `close-superseded` | Leave closed; the new injection carrier (`claude_md_sync` managed block) is the de facto SSOT and is visible in this session. |
| `8aacc032-6b35-47ea-8f24-15f5a5b79136` | `架构漂移: 记忆注入三层架构 (MCP 静态摘要 + Tool 描述 + mission_context_build)` | `skipped` | `close-superseded` | Leave closed; the three layers exist as documented. |
| `3df06926-da77-4a4f-8df1-025ea1a74f27` | `架构漂移: Engine 架构收敛 8→2 (autopilot/flow/memory → intent_engine + learning_engine)` | `skipped` | `close-superseded` | Leave closed; current `crates/missiond-daemon/src/engine/` has exactly the two declared engines plus the necessary cross-cutting helpers. |

## Evidence

### `65ef7afe-4572-40b2-a3c8-4dd6811e65aa` — `close-stale`

- Pre-existing close note: `board-cleanup-85c511908244856610` from `codex-board-cleanup` at `2026-05-10T08:30:19+00:00`, reason `current drift gate clean`.
- Original drift advisory listed two changed files. Both still exist and are not part of any open drift report:
  - `crates/missiond-core/src/db/pg/message.rs`
  - `crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs` (this is the same file Batch 10 already verified for Codex `task_id` stamping at lines 780–808).
- The drift gate scripts that detect this class of issue are present and functional: `scripts/check-v3-final-convergence.mjs`, plus the conversation-ingestion isomorphism checkers (`scripts/check-v3-conversation-ingestion-isomorphism.mjs`, `scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs`).

Recommendation: leave as `skipped`. If this drift recurs in the future, a new `Backfill Lisp/checker` row will be auto-created with a fresh `dedupeKey`.

### `b5b65989-6949-4a88-83bc-c52e451cfbed` — `close-covered`

- Pre-existing close note: `board-cleanup-7929c8372fb8ccda93`.
- Original commit `4ab7994` introduced `mission_execution`. The current code has the matching surface area:
  - MCP tool defined in `crates/missiond-mcp/src/tools/knowledge/agent_execution.rs` (single `ToolDefinition::new("mission_execution", ...)` at line ~414–415).
  - The tool description names 13 actions: `open / list / claim / heartbeat / release / deviate / decide / issue / complete / status / audit / repair / preflight_commit` — a superset of the original commit's 12 actions (the original advisory).
  - Handler module exists at `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`.
- The current implementation is referenced by `intent-tools.lisp :: implemented-surface mission_execution :: :workstation-dispatch-record`, indicating Lisp surface coverage as well.

Recommendation: leave as `skipped`. The original code-first commit has been fully integrated into the Lisp surface.

### `4e0f4400-880a-4768-bcd1-72605ba1ecf5` — `close-superseded`

- Pre-existing close note: `board-cleanup-06580c2caa21144db5`.
- The drift advisory called out that Preferences / Hot Topics are now injected via a different carrier than the historical MCP-server initialize path. The new carrier is the CLAUDE.md managed block, written by the daemon:
  - `crates/missiond-daemon/src/context/claude_md_sync.rs` defines `MANAGED_START = "<!-- missiond:managed:start -->"` and `MANAGED_END = "<!-- missiond:managed:end -->"` (lines 5–6) and writes a `# MissionD Managed` section (line 81).
  - Module is wired from `crates/missiond-daemon/src/main.rs` (line 30) and from the autopilot at `crates/missiond-daemon/src/engine/intent_engine/autopilot.rs` (line 7) via `sync_claude_md`.
  - The user's `~/.claude/CLAUDE.md` for this project (visible in this session's system context) contains `## Preferences` and `## Hot Topics` inside the managed block — direct runtime evidence the carrier is in use.

Recommendation: leave as `skipped`. The drift is now the canonical SSOT for these injections.

### `8aacc032-6b35-47ea-8f24-15f5a5b79136` — `close-superseded`

- Pre-existing close note: `board-cleanup-792ff723aa5aaf311c`.
- The advisory described a three-layer memory injection: MCP static summary + Tool descriptions + `mission_context_build` / CLAUDE.md dynamic injection. Each layer is observable today:
  1. MCP static summary — built into MCP server description; tool descriptions in `crates/missiond-mcp/src/tools/...`.
  2. Tool description guidance — `crates/missiond-mcp/src/tools/knowledge/agent_execution.rs` (and others) carry directive prose inside their `ToolDefinition::new(...)` strings.
  3. `mission_context_build` — handler entry at `crates/missiond-daemon/src/handlers/knowledge/skill.rs` line 29 (`"mission_context_build" => context::handle_build(state, args).await`); claude_md sync is the persistent companion (`claude_md_sync.rs`).

Recommendation: leave as `skipped`. The architecture moved to where the advisory pointed; nothing to backfill in the manifest beyond what the SSOT already records.

### `3df06926-da77-4a4f-8df1-025ea1a74f27` — `close-superseded`

- Pre-existing close note: `board-cleanup-fb9c497781a7b471fc`.
- The advisory called for collapsing 8 scattered engine modules into 2. Current state of `crates/missiond-daemon/src/engine/`:
  - Two engine packages: `intent_engine/` (contains `autopilot.rs`, `flow_engine.rs`, `gen_engine.rs`, `memory_scheduler.rs`, `workflow_executor.rs`) and `learning_engine/` (contains `extraction.rs`, `decision_engine.rs`, `decision_harvest.rs`, `historical_scanner.rs`, `idle_explorer.rs`, `intent_analyst.rs`, `timeline_analyst.rs`, `gen_engine.rs`).
  - Cross-cutting helpers as siblings, not engines: `commit_convergence.rs`, `lisp_code_sync.rs`, `master_control.rs`, `nightly_evolution.rs`, `shared_memory.rs`, plus a `flow/` directory.
- Autopilot, flow, memory, and learning concerns are all reachable from one of the two engines, matching the advisory's target shape.

Recommendation: leave as `skipped`. The 8→2 collapse is observable on disk.

## Notes

- All 5 rows are already `status=skipped`. This batch did not change anything; it only audited the cleanup wave's claims.
- For all `架构漂移` rows, the underlying reason is identical: the daemon code already moved to the architecture pointed at by the drift detector, so the advisory's only remaining ask ("update YAML manifest after verification") is now the responsibility of the V3 SSOT / OCaml universe checker, not a discrete BoardTask.
- The `Backfill Lisp/checker` rows are auto-spawned by the drift detector with a `dedupeKey`. If similar drift reappears later, a fresh row will be created — leaving these closed does not lose anything.

## Follow-Up (out of scope for this batch)

- Batch 10 was originally dispatched as `c889ebfc-35f7-44e7-b5f1-8afe5364afac`; the durable artifact for that batch was missing on disk and has been written by this dispatch as `missiond-board-batch-10-20260510.md` from the prior in-conversation evidence. The `c889ebfc` row is still `running` from the autopilot's perspective; closing or releasing it is the autopilot's responsibility, not part of this batch's read-only acceptance.
