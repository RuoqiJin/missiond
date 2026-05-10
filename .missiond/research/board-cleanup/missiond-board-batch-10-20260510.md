# MissionD Board Cleanup Batch 10 - 2026-05-10

Scope: read-only review of 5 MissionD-related BoardTasks. The original dispatch (`c889ebfc-35f7-44e7-b5f1-8afe5364afac`) was claimed by a ClaudeCode worker but autopilot blocked its closure with `missing-output-contract-sections` because the worker only returned in-conversation text and did not write a durable artifact under `.missiond/research/board-cleanup/`. This file is the durable Batch 10 artifact, restated by Batch 11 dispatch (`3e04dd4c-2cac-4981-b327-33b9f3a07707`) from the same evidence collected during the original investigation. No Board task statuses were changed by me.

## Summary

| Task ID | Title | Classification | Recommendation |
| --- | --- | --- | --- |
| `d5c6ecba-71e8-4437-a1fc-96c9816a6c28` | `#5 会话 logs 升级为全 timeline 事件日志中心` | `close-superseded` | v1.3.0 SSOT cutover already promoted `event_log` to the timeline truth source; the original aspiration is satisfied. |
| `016ceb68-bd3c-414b-8a61-3f08ab0bc520` | `[待修复] MCP 工具效率优化: 切片提取+搜索过滤+批量写入` | `keep` | Split into 4 narrow children; only `conversation_get` is partially covered (tail+since_id), the other three asks are absent. |
| `1526d81c-fd79-451d-a2cf-60a853b08970` | `#6 元会话展示运行顺序和关联` | `rewrite-candidate` | Infrastructure exists (event_log + dispatch event preview + CognitiveTimeline.tsx), but the end-to-end UX (full prompt + fetched batch on the same timeline row) needs scoping. |
| `2de8cbb2-8e7f-4951-af58-4e25fd7a5dcf` | `[待修复] Memory scheduler 自适应节流 — 连续空结果指数退避` | `keep` | Not implemented; revisit after dependency `5ba75dc0` (Autopilot robustness) is resolved. |
| `dd65b5eb-bc88-4179-8942-ab134747e394` | `[待修复] extraction cooldown + memory_pending error 返回` | `keep` | Both fixes are still missing in code; ship as a small two-line patch. |

## Evidence

### `d5c6ecba-71e8-4437-a1fc-96c9816a6c28` — `close-superseded`

The v1.3.0 SSOT cutover already established `event_log` as the timeline truth source.

- Migrations:
  - `crates/missiond-core/migrations/20260419000000_event_log.sql` creates the `event_log` table.
  - `crates/missiond-core/migrations/20260420100000_event_log_fts.sql` adds the FTS index.
  - `crates/missiond-core/migrations/20260420200000_drop_system_timeline.sql` drops the legacy `system_timeline` table.
- Source notes confirm the cutover:
  - `crates/missiond-core/src/db/traits.rs` line 941–942: `v1.3.0 SSOT cutover: event_log is the timeline truth source (frozen lisp §4.6).`
  - `crates/missiond-core/src/db/pg/timeline.rs` line 3: `v1.3.0 SSOT cutover: event_log is the timeline SSOT (frozen lisp §4.6 ...)`.
  - `crates/missiond-core/src/db/pg/mod.rs` line 68: `system_timeline dropped in v1.3.0 SSOT cutover (event_log SSOT).`
- Recent commit `67895091 code: SSOT cutover — event_log 成 timeline 真理源, drop system_timeline` documents the architectural move.

Recommendation: close as superseded. The aspiration `会话 logs 升级为全 timeline 事件日志中心` is already realized by event_log + FTS + system_timeline removal.

### `016ceb68-bd3c-414b-8a61-3f08ab0bc520` — `keep`

Four sub-asks; only one is partially covered.

- `mission_conversation_get` slicing — partial.
  - `crates/missiond-daemon/src/handlers/comm/conversation/query.rs` line 154–197 defines `Args` with `tail`, `since_id`, plus include flags. There is no `startIndex/endIndex` or `offset+limit`. `tail` + `since_id` covers tail and incremental cursors, not arbitrary range slices.
- `mission_kb_search` `excludeCategory` — absent.
  - `crates/missiond-daemon/src/handlers/knowledge/kb/query.rs` line 53–66: `KBSearchArgs` exposes `query, category, limit, offset, search_mode, project, include_archived, state_filter`. Only positive `category` is supported.
- `kb_remember_batch` — absent.
  - No matches for `kb_remember_batch` in `crates/`. Only the single-entry `mission_kb_remember` exists.
- Deep-analysis saturation detector — absent.
  - No matches for `saturation`, `consecutive_empty`, or `empty_run` in `crates/missiond-daemon/src/engine/learning_engine/`.

Recommendation: keep, and split into four small children (range slicing, excludeCategory, kb_remember_batch, saturation detector) so each can be shipped and closed independently.

### `1526d81c-fd79-451d-a2cf-60a853b08970` — `rewrite-candidate`

Infrastructure exists, but the user-visible feature is not end-to-end.

- Ordering / correlation are covered by `event_log` (see Batch 10 task d5c6ecba evidence). `crates/missiond-daemon/src/handlers/comm/capability_usage/runtime.rs` line 289 cross-joins board task creation events into the same source.
- Prompt sent to memory worker — partial.
  - `crates/missiond-daemon/src/engine/learning_engine/extraction.rs` line 47 emits a SlotTaskDispatched event via `emit_dispatch_event(bus, slot_id, purpose, prompt)`.
  - Lines 48–51 truncate the stored payload to a 200-char `preview`, so the full prompt is not directly recoverable from `event_log`.
- MCP-fetched batch content — not surfaced as a timeline event.
  - `crates/missiond-daemon/src/handlers/knowledge/memory.rs` lines 58–158: `mission_memory_pending` returns the batch via `ToolResult::text(...)`, but no separate timeline event with a shared batch id is emitted.
- Frontend — partial.
  - `packages/board/src/components/timeline/CognitiveTimeline.tsx` and `packages/board/src/components/timeline/ui/TimelineHeader.tsx` exist. `packages/board/src/components/EngineDashboard.tsx` lines 387, 587 surface extraction/submit/decision counters but not the prompt or fetched batch inline.

Recommendation: rewrite. Concrete acceptance:

1. Persist the full memory worker prompt in `event_log` (or via a blob storage reference) when emitting the dispatch event.
2. Emit a `MemoryBatchFetched` event from `mission_memory_pending` with a shared `batch_id` that matches the dispatch event.
3. Extend CognitiveTimeline with an expandable detail panel that shows `prompt` + `fetched_batch` side-by-side per memory cycle.

### `2de8cbb2-8e7f-4951-af58-4e25fd7a5dcf` — `keep`

Self-adaptive throttling for memory scheduler is not implemented.

- No matches for `consecutive_empty`, `empty_count`, `exponential.*backoff`, or `cooldown_until` in `crates/missiond-daemon/src/state.rs`, `crates/missiond-daemon/src/engine/learning_engine/extraction.rs`, or `crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs`.
- `crates/missiond-daemon/src/state.rs` lines 43–58 define `ExtractionState` with `phase`, `pending_served`, etc., but no `consecutive_empty_count` or `cooldown_until`.
- `crates/missiond-daemon/src/engine/learning_engine/extraction.rs` line 116 (`check_realtime_extraction`) gates only on `check_extraction_gate` + `try_claim_extraction_probe` — there is no empty-result backoff branch.
- The task itself notes a dependency on `d305b87a-...` (investigation report) and on `5ba75dc0` (Autopilot robustness fix) deciding whether this throttle is still required.

Recommendation: keep. Re-evaluate after the dependency tasks land. If the root cause is fixed by the Autopilot work, this becomes `close-superseded`; otherwise implement the documented `3s → 6s → 12s → 30s → 60s` ladder.

### `dd65b5eb-bc88-4179-8942-ab134747e394` — `keep`

Both fixes from the investigation report are still absent.

- Fix 1 — Extraction completion cooldown via `last_completion_at: i64` on `ExtractionState`.
  - No matches for `last_completion_at` in `crates/`. The struct in `crates/missiond-daemon/src/state.rs` lines 43–58 does not contain that field.
  - `crates/missiond-daemon/src/engine/learning_engine/extraction.rs` `check_realtime_extraction` has no 30s-since-completion guard branch.
- Fix 2 — `mission_memory_pending` `pending_served` latch should return `ToolResult::error(...)` instead of `ToolResult::text(...)`.
  - `crates/missiond-daemon/src/handlers/knowledge/memory.rs` line 58 still returns `ToolResult::text(...)` when the latch is set. The wording does warn the agent, but the `text` response type does not register as an error to the calling model.

Recommendation: keep. Both edits are minimal and independent; both should ship together as a small patch.

## Notes

This artifact is a faithful restatement of the previous in-conversation review for the same five task IDs. No additional code changes were made; no historical Board task status was changed. The only mutation produced by Batch 11 dispatch is this Markdown file under `.missiond/research/board-cleanup/`.
