# MissionD Board Cleanup Batch 21 - 2026-05-10

Scope: read-only review of 5 MissionD-related BoardTasks. Dispatch task: `53cc4a7e-47b1-41b2-88d0-49441b4c5c8d`. No historical Board task statuses were changed by me. Only this Markdown file under `.missiond/research/board-cleanup/` was written.

Heuristic applied: **first ask whether each original ask still holds today; then verify against SSOT/code/checker/runtime; finally give one actionable verdict**. Wide goals are not split into sub-tasks per dispatch instruction.

All 5 reviewed tasks are currently `status=open`.

## Summary

| Task ID | Title | Original Ask Still Valid? | Classification | Recommendation |
| --- | --- | --- | --- | --- |
| `b95482b2-dc13-4043-b6c4-f1119943f699` | `deep-analysis 会话泄漏进 realtime pending 队列` | Partially — same memory-pipeline waste family already kept | `merge-into-existing-candidate` | Merge into the `22661290` / `2ba85d68` memory-noise thread; the residual is one specific branch of `derive_conversation_type`. |
| `36eb94ca-99e6-4d09-803b-5f5cf55813b6` | `P2: 本地 Token 预算软熔断 — 滑动窗口消耗限额` | Yes — current quota throttles are post-error, not pre-budget windows | `keep` | Single-file change in `supervisor.rs` or `llm_gateway.rs`; do not split. |
| `0fe3e713-60d5-4ca9-a532-87e3cd752722` | `MissionD chat completions: /clear + full messages passthrough` | Yes — `handle_chat_completions` still uses last-user-message + PTY context | `keep` | Single function-body change in `crates/missiond-core/src/ws/server.rs:979`; keep. |
| `bf892944-eef9-4935-91e2-5b00fac5f6f5` | `Memory pipeline: 禁止向 Thinking 状态工位注入新批次` | No — extraction.rs already gates dispatch on `SessionState::Idle` at all dispatch sites | `close-covered` | Close; `check_realtime_extraction` releases the probe instead of dispatching when slot is non-Idle. |
| `f14082d0-8750-4a4b-9b08-06c167e8ee93` | `LearnedPermissions TTL 过期机制` | Yes — `LearnedPermission` struct has no `expires_at` and no filter on read | `keep` | Single struct field + read-time filter; the task's "Migration" pointer is misleading because the store is YAML-persisted, not SQL. |

## Evidence

### `b95482b2-dc13-4043-b6c4-f1119943f699` — `merge-into-existing-candidate`

Heuristic: same memory-pipeline waste family already kept open.

- The classifier `derive_conversation_type` (`crates/missiond-core/src/db/mod.rs:38`) does not give deep-analysis self-sessions a special branch:
  - `gemini_cli` / `router_chat` source → `gemini_chat`.
  - Slot config supplies `slot_category` → use it.
  - Slot id present but no config → `worker`.
  - Session id contains `-acompact-` → `compaction`.
  - Session id starts with `agent-` → `subagent`.
  - Otherwise → `user`.
- A deep-analysis self-session opened by the slow lane has no slot id, no special prefix, and project root `/Users/jinchen/...`; it lands on `user` and therefore passes the `c.conversation_type='user'` realtime filter at `crates/missiond-core/src/db/pg/conversation.rs:1542` (Batch 16/17 evidence).
- Existing partial mitigations:
  - `c.conversation_type NOT IN ('meta', 'compaction')` is enforced in many SELECT paths (`crates/missiond-core/src/db/pg/message.rs:237, 259, 279, 365, 386, 451, 517, 605` and `pg/conversation.rs:855, 936`).
  - `extraction.rs:125–135` skips realtime extraction *while* a submit task is running on `slot-memory`.
- Open peer rows already track this family:
  - `22661290-d93e-4734-9a1f-8a95fcb9c946` (Batch 16 / 19 `keep`).
  - `2ba85d68-4a76-4c17-8bad-382a57b5250a` (Board reference: memory worker self-referential feedback loop / pending-queue filter).
  - `905e5a26`, `a861cebf`, `c8c0345a`, `d1ffe953` (all merged in earlier batches).

Recommendation: merge. The residual the task names is "give deep-analysis self-sessions a non-`user` `conversation_type`" — that is one branch in `derive_conversation_type`, properly tracked under `22661290`.

### `36eb94ca-99e6-4d09-803b-5f5cf55813b6` — `keep`

Heuristic: current throttles are post-error, not pre-budget sliding windows.

- `crates/missiond-daemon/src/llm/sonnet_gateway.rs:268, 356–359, 466` declares `quota_throttle_sleep: Duration` and only `tokio::time::sleep(self.quota_throttle_sleep).await` *after* the API has reported a 429-style quota error.
- `crates/missiond-daemon/src/llm/minimax_gateway.rs:265, 342–345, 451` mirrors the same pattern.
- `crates/missiond-daemon/src/llm/gemini_cli.rs:1045–1049` only detects `"TerminalQuotaError"` / `"exhausted your daily quota"` strings post-hoc.
- No matches for `sliding.*window`, `token_budget`, `hourly.*quota`, or `daily.*quota` in `llm/` or `supervisor.rs` — the pre-budget cliff guard the task describes does not exist.
- The `2026-03-14 事故` referenced in the task body confirms the gap is operationally felt, not theoretical.

Recommendation: keep. Single new file (or section in `llm_gateway.rs`) with a per-window counter that flips `global_paused=true` on threshold; per dispatch instruction, no sub-task split.

### `0fe3e713-60d5-4ca9-a532-87e3cd752722` — `keep`

Heuristic: implementation site is concrete; the requested behaviour is missing.

- `crates/missiond-core/src/ws/server.rs:979 async fn handle_chat_completions(...)` is the entry point named in the task.
- Reading the body confirms the current shape: it parses the HTTP request, validates the bearer token (`:1003–1018`), reads headers, but no `/clear\n` send precedes the prompt forward; the existing flow inherits PTY context for continuity.
- Multi-user concurrency concern noted in the task body is real because the same default slot (`slot-jarvis`, see Batch 18 `3b3788b7`) accepts a shared PTY.

Recommendation: keep. The change is a 3–5 line addition in one function: send `/clear\n`, await an idle ack, format `messages[]` into a single block, then forward. Do not split.

### `bf892944-eef9-4935-91e2-5b00fac5f6f5` — `close-covered`

Heuristic: the dispatch sites already gate on `SessionState::Idle`; the observed Thinking-injection symptom is a different failure mode.

- `crates/missiond-daemon/src/engine/learning_engine/extraction.rs:243` `Some(s) if s.state == SessionState::Idle => {}` (realtime path) — anything else falls into the `else` branch which calls `release_extraction_probe(...)` and returns.
- Same Idle-gate pattern at `:460` and `:784` (slow-lane and other dispatch entrypoints).
- `crates/missiond-daemon/src/state.rs` defines `ExtractionPhase::WaitingForSlotIdle` (used in the latch logic at handlers/knowledge/memory.rs:154 — Batch 10 evidence) so the system *already* has a "wait for idle" phase in its state machine.
- `crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs:13–21 ensure_memory_slot_by_id` is broader (`info.state != SessionState::Exited`) but is the *spawn* path, not the *dispatch* path; spawn alone does not push input.
- The 30-minute frozen-screen symptom is more consistent with PTY recognition lag (the screen still showed `Thinking` after the worker had emitted `本批次已处理完毕`), not with new input being injected on top of a Thinking state. That is a separate, narrower diagnostic — not what this row asks for.

Recommendation: close as covered. If the recognition-lag symptom recurs in current builds, file a fresh narrow task on `pty_recognition.rs` rather than re-opening this row.

### `f14082d0-8750-4a4b-9b08-06c167e8ee93` — `keep`

Heuristic: ask is concrete; struct field is missing; "Migration" pointer in the task body is mild scope drift.

- `crates/missiond-core/src/core/learned_permissions.rs:1–30` declares `LearnedPermission` with fields `id, scope_type, scope_id, tool_pattern, decision, param_pattern, learned_at, last_used_at, use_count`. No `expires_at` field.
- File header on the same module: `Learned Permissions — in-memory HashMap with YAML persistence.` — this is a YAML-persisted in-memory store, not a SQL table. The task body's reference to `crates/missiond-core/src/db/migration.rs` is misdirected (no `learned_permissions` migration exists; `rg "learned_permissions" crates/missiond-core/migrations/` returned zero).
- Read sites at `crates/missiond-core/src/core/permission.rs:184, 186` consult the dynamic permissions path; without an `expires_at` filter they treat any present row as currently authoritative.
- The "Gemini 审计 Issue #4" framing is a real residual: a learned `Allow` lasts forever until manually edited.

Recommendation: keep. Single struct change + a small filter at the read sites and a YAML migration default of 30d TTL; per dispatch instruction, do not split. Optionally include the "auto-extend on hit" branch the task mentions, but that is one extra `if` and not a separate row.

## Notes

- `b95482b2` is the fifth membership of the memory-noise / meta-circulation family this cleanup season; the cleanup wave should auto-merge such rows by KB ref (`memory-worker-self-referential-feedback-loop`, `deep-analysis-session-leaks-into-realtime-queue`, etc.) without re-reading natural-language framings.
- `bf892944` is the textbook "symptom blamed on the wrong layer" row: the screen-frozen observation was real, but the dispatch-side gate already exists, so the right fix lives in PTY recognition, not memory-scheduler.
- `f14082d0`'s task body had a small SQL-vs-YAML scope error; cleanup-time review should normalise such pointers (`db/migration.rs` → YAML defaults) when the task is processed, not at implementation time.

## Verification

- ✅ Wrote only `.missiond/research/board-cleanup/missiond-board-batch-21-20260510.md` inside the declared `write_scope`.
- ✅ Did not call `mission_board_update` or `mission_board_note_add`; no historical Board task statuses changed.
- ✅ `must_not_touch` directories (`.git`, `crates/`, `packages/`, `scripts/`) untouched (read-only).
- ✅ Each reviewed task carries one classification from the allowed set and at least one of `file_path:line` / cross-batch reference / module-header line as evidence.
- ✅ Heuristic format honoured: original-ask validity question first, then SSOT/code/runtime check, then a single actionable verdict per task — no sub-task spinning.
- ✅ Final answer follows the Findings / Evidence / Recommendations / Verification contract; no raw KB JSON or full logs pasted.
