# MissionD Board Cleanup Batch 08 - 2026-05-10

Scope: read-only review of 5 MissionD-related BoardTasks. This batch used a ClaudeCode worker (`f0fd7868-96ac-4aab-928b-270b3b634b2e`) for fact checking; I cross-checked its durable final and accepted the worker evidence where it matched local evidence. No Board state was changed.

## Summary

| Task ID | Title | Classification | Recommendation |
| --- | --- | --- | --- |
| `c364ab71-d0ff-45c5-9173-045623b73530` | 会话日志存储三层抽象重构 (CQRS + Event Sourcing) | `rewrite-candidate` | Rewrite around the current PostgreSQL conversation/query architecture; do not continue from the obsolete `MessageFeed::builder()` premise. |
| `f533e3df-7dbe-4f87-b6a2-5f5246ebcd12` | Unified Control Tree - 统一工位控制架构 | `close-covered` | Close with evidence summary. |
| `67709041-4458-4ac8-987c-8fc4acdb950a` | `[BUG] mission_kb_remember` INT8 vs INT4 | `keep` | Keep open; bug appears live. Refine the description from `kb_entries` to `knowledge/access_count`. |
| `d1686dc7-0090-4b92-bcf6-06b2af0d9f42` | ControlTree `slot_role` 维度闭环 | `close-covered` | Close with slot-role gate evidence. |
| `61a1994d-f03c-4207-8aa2-b86f0c8d569a` | unknown | `stale-reference` | Task ID was not found; verify the source list before action. |

## Evidence

### `c364ab71-d0ff-45c5-9173-045623b73530`

The task is partly covered but stale in its current wording.

- Schema pieces exist in `crates/missiond-core/migrations/20260318000000_init.sql`:
  - `conversation_messages.raw_role/content_types/has_image/has_tool_use/token_count`
  - `consumer_watermarks`
  - `message_labels`
- The current ingestion path has layer-like functions in `crates/missiond-daemon/src/infra/message_handler.rs`:
  - `handle_new_messages`
  - `ingest`
  - `classify`
  - `emit`
- Batch labels are written through `crates/missiond-core/src/db/pg/conversation.rs` via `insert_message_labels_batch`.
- The key abstraction named in the task, `MessageFeed::builder()`, does not exist in the current codebase. The worker found that `crates/missiond-core/src/db/message_feed.rs` and `db/watermark.rs` were introduced in an earlier stage and later deleted by the SQLite cleanup commit (`56b6ce80`).
- Current `consumer_watermarks` usage is limited to trait/PG infra and is not a real downstream consumer migration.

Recommendation: rewrite this as a current PostgreSQL conversation-source-state/query-governance task. The old task should not be continued as if `MessageFeed::builder()` still exists.

### `f533e3df-7dbe-4f87-b6a2-5f5246ebcd12`

Covered.

- `crates/missiond-daemon/src/control_tree.rs` defines `ControlTree`, `ControlManager`, JSON persistence, and effective pause checks.
- `crates/missiond-daemon/src/workers/registry.rs` uses dependency-aware `WorkerContext` and `is_effectively_paused`.
- `crates/missiond-daemon/src/handlers/compute/worker.rs` exposes control for global/provider/domain/worker/slot_role/project.
- `crates/missiond-daemon/src/bus/control_gate_adapter.rs` and `crates/missiond-daemon/src/bus/bootstrap.rs` wire ControlTree into the EventBus gate path.
- `crates/missiond-daemon/src/main.rs` hydrates legacy global/LLM gates into the ControlTree-backed model.

Recommendation: close as covered.

### `67709041-4458-4ac8-987c-8fc4acdb950a`

Keep. The worker found the reported type mismatch still appears real.

- The task text names `kb_entries`, but the active table is `knowledge`.
- `crates/missiond-core/migrations/20260318000000_init.sql` defines `knowledge.access_count INTEGER DEFAULT 0`, which is Postgres `INT4`.
- `crates/missiond-core/src/db/pg/knowledge.rs` decodes `access_count` through `i64` in `row_to_knowledge_entry` / `KBRow`.
- `crates/missiond-core/src/types/knowledge.rs` and generated types also expose `access_count: i64`.

Recommendation: do not close. Rewrite the task as a narrow bugfix: align `knowledge.access_count` Rust/Postgres typing, either by casting `access_count::BIGINT` in SELECTs or changing the Rust field path to `i32` where appropriate.

### `d1686dc7-0090-4b92-bcf6-06b2af0d9f42`

Covered.

- `crates/missiond-daemon/src/slot_orchestrator/agent.rs` checks `is_slot_role_paused`.
- `crates/missiond-daemon/src/engine/intent_engine/autopilot.rs` checks slot role before dispatch.
- `crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs` refuses PTY spawn when role is paused.
- `crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs` refuses memory slot spawn when role is paused.
- `crates/missiond-daemon/src/handlers/compute/worker.rs` actively kills matching PTYs when a slot role is paused.
- `crates/missiond-daemon/src/state.rs` documents that legacy `memory_paused` state was removed and ControlTree is the source of truth.

Recommendation: close as covered, or merge into the parent ControlTree closure note.

### `61a1994d-f03c-4207-8aa2-b86f0c8d569a`

`mission_board_query(action=get)` returned `Task not found`.

Recommendation: classify as stale source-reference in this cleanup batch; no Board action possible without a valid task id.

## Process Notes

- ClaudeCode worker conversation `3518fa23-3a49-4918-b606-865853ccc776` completed as a worker conversation with `endedAt`.
- Worker did not mutate Board state or files.
- This batch revealed a useful cleanup rule: do not auto-close bug reports just because nearby infrastructure has evolved; if a worker proves the code-level mismatch is still present, keep and narrow the task.
