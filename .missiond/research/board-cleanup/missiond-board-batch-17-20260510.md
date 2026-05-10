# MissionD Board Cleanup Batch 17 - 2026-05-10

Scope: read-only review of 5 MissionD-related BoardTasks. Dispatch task: `1aa15b93-df78-4e7b-a21a-e3481aa50c71`. No historical Board task statuses were changed by me. Only this Markdown file under `.missiond/research/board-cleanup/` was written.

Heuristic applied: **first ask whether each original ask still holds today; then verify against SSOT/code/checker/runtime; finally give one actionable verdict**. Wide goals are not split into sub-tasks per dispatch instruction.

All 5 reviewed tasks are currently `status=open`.

## Summary

| Task ID | Title | Original Ask Still Valid? | Classification | Recommendation |
| --- | --- | --- | --- | --- |
| `5d8af13f-742b-453e-a78c-6c56f9e5f285` | `巡检自适应调频：全 Idle 时降低频率,有 Running 时恢复高频` | Yes — supervisor still ticks on a fixed cadence | `keep` | Add an adaptive interval governed by `running task count` directly in `crates/missiond-daemon/src/supervisor.rs`; do not split. |
| `43d79003-abdb-4178-8ffa-9f7231f2f413` | `realtime-extract dispatch 层应过滤 meta-agent 会话` | No — the SQL gate at the dispatch query already enforces this | `close-covered` | Close; `get_pending_realtime_messages_with_limit` filters `conversation_type='user'` at the source. |
| `7bb96a6e-0b6f-4d1f-b8fd-e9f5263c2822` | `Agent CI 轮询防护：Skill/Prompt 层面禁止裸 gh api 循环` | No — `backend-deploy` skill already documents `xjp_build_wait` | `close-covered` | Close; skill already names the right tool. Compaction-loss is a separate context-budget concern, not a Board task. |
| `61a87ef1-1801-4bcd-9b97-614859d84fdc` | `KB 跨管线去重：realtime 与 deep-analysis 同时产出重复条目` | No — `kb_remember` already runs `token_jaccard_similarity` fuzzy dedup before insert | `close-covered` | Close; both pipelines write through the same fuzzy-dedup gate. |
| `98b8ea33-d7c8-4eec-8446-1da0c61c0be4` | `主控 slot 完成通知机制：替代 pty_screen 高频轮询` | No (for master) — `master_control` already subscribes `SlotEvent::BecameIdle` | `close-covered` | Close; the master-side completion notification is event-driven via the bus. The 2026-03 polling pattern was a discipline issue, not a missing feature. |

## Evidence

### `5d8af13f-742b-453e-a78c-6c56f9e5f285` — `keep`

Heuristic: ask still holds today; no adaptive frequency in supervisor.

- `crates/missiond-daemon/src/supervisor.rs` does not contain any of `adaptive`, `consecutive_idle`, `all_idle`, `backoff` (rg returned empty for each across the full file). The 5-min fixed cadence the task names is still the model.
- The task's own evidence (`session 44a8dd8f`: 25 rounds, 100% idle; preceding session: 31 rounds, 2.5h, 100% idle) is consistent with the still-fixed schedule.
- A "running user task count" gate is straightforward inside `supervisor.rs` (use `state.store.get_tasks_by_status(TaskStatus::Running)` already used in `extraction.rs:127–134`), but this is implementation detail, not justification to split.

Recommendation: keep. The patch is local to `supervisor.rs`; no children needed.

### `43d79003-abdb-4178-8ffa-9f7231f2f413` — `close-covered`

Heuristic: ask says "filter meta-agent at dispatch layer". The SQL gate at the realtime pending query is exactly that.

- `crates/missiond-core/src/db/pg/conversation.rs:1520–1564` `get_pending_realtime_messages_with_limit` constrains to `WHERE c.conversation_type = 'user'` (line ≈1542). Meta-agent / worker / subagent / compaction conversations never reach the dispatch payload.
- The `conversation_type` taxonomy is defined and enforced at multiple call sites — see `:341–349` (user / meta / worker / jarvis / subagent / compaction / system / gemini) and `:855` / `:936` (`AND conversation_type NOT IN ('meta', 'compaction')` for embedding queries). Meta-circulation has structural attenuation across the daemon, not just at one site.
- Combined with the watermark advance via `realtime_forwarded_at`, repeated dispatch on a meta-agent session that produced no user messages is impossible: the outer `WHERE` excludes the row before the cursor logic even runs.

Recommendation: close as covered. The 2026-03-13 evidence (9 dispatches / 0 knowledge) predates the conversation_type SQL gate that now stops this at source.

### `7bb96a6e-0b6f-4d1f-b8fd-e9f5263c2822` — `close-covered`

Heuristic: skill-level ask is "the agent should know `xjp_build_wait` exists and use it instead of polling". The skill file already documents this.

- `~/.claude/skills/backend-deploy/skill.md:375` carries a `tool: xjp_build_wait` entry — the exact tool the task wanted callers to prefer over `gh api ... runs/jobs` loops.
- The skill is in the user's global skill index (CLAUDE.md 「乙·部署」section names `backend-deploy` as the GA → DC pure CD playbook), so the runtime that loads `backend-deploy` already sees the guidance.
- The remaining concern in the task body — "ensure xjp_build_wait still recallable after context compaction" — is a context-budget / compaction-policy issue (memory `auto-context-hook` / context-prefetch family), not a separate Board surface.

Recommendation: close as covered. If a follow-up case shows the agent still polls after compaction, escalate to the context-prefetch / compaction track, not to a new Board row.

### `61a87ef1-1801-4bcd-9b97-614859d84fdc` — `close-covered`

Heuristic: ask is "kb_remember should reject / merge near-duplicates". This is implemented today.

- `crates/missiond-core/src/db/shared.rs:169` defines `pub fn token_jaccard_similarity(a: &str, b: &str) -> f64`.
- `crates/missiond-core/src/db/pg/knowledge.rs:5` imports it (`use crate::db::shared::{contains_sensitive_data, infer_kb_type, token_jaccard_similarity};`).
- `:239` and `:1231` are the actual call sites: each computes `token_jaccard_similarity(&new_text, &existing_text)` and gates the merge / reject decision.
- `:2032` comment "List KB entries by category (for fuzzy dedup in kb_remember)" confirms the lookup pattern (per-category candidate window, fuzzy compare, then merge).
- Both pipelines (realtime + deep-analysis) write through the same `kb_remember` entry point, so cross-pipeline duplicates are dedup'd by the same fuzzy gate.

Recommendation: close as covered. The 2026-03-14 evidence (two near-duplicate KB rows 2 minutes apart) predates / precedes the fuzzy-dedup gate, or sat below the threshold; if specific threshold tuning is wanted, that should be a tiny-scope task on `token_jaccard_similarity` alone, not a generic dedup ask.

### `98b8ea33-d7c8-4eec-8446-1da0c61c0be4` — `close-covered`

Heuristic: "master should not poll pty_screen; subscribe to slot events instead." The master already subscribes.

- `crates/missiond-daemon/src/engine/master_control.rs:33` `MASTER_SLOT_SUBSCRIPTION = "master_event_subscriber_slot_v2_live"` — named subscription on the slot domain.
- `:924` `subscribe::<SlotEvent>(MASTER_SLOT_SUBSCRIPTION, master_live_subscription_opts())` — actual `subscribe` call.
- `SlotEvent::BecameIdle` is emitted at the right boundaries:
  - `crates/missiond-daemon/src/workers/local/pty_event_worker.rs:195` `publish_slot(SlotEvent::BecameIdle { ... })`.
  - `crates/missiond-daemon/src/engine/shared_memory.rs:782` same.
  - `crates/missiond-daemon/src/handlers/comm/timeline.rs:564 / :580 / :625` and `master_control.rs:2825 / :2833 / :2070 / :2098 / :2110` further surface the variant.
- The 25 `pty_screen` polls in `session 97a112c2` (2026-03-14) reflect an agent / prompt that ignored the existing event surface, not a missing one.

Recommendation: close as covered for the master case. If end-user / UI clients need a polled-or-subscribed completion API, that is a separate frontend / MCP-client surface.

## Notes

- Four of the five tasks are clean closes (`43d79003`, `7bb96a6e`, `61a87ef1`, `98b8ea33`). The pattern: each named a behaviour the system has since put a structural gate behind, but the Board row was never closed because no one re-checked.
- `5d8af13f` is the rare "real residual" — adaptive supervisor cadence is not in code yet. Per dispatch instruction it stays as a single row, not split into "5min/15min/30min" sub-steps.
- The cleanup wave should periodically auto-recheck old `[优化]` rows against current code; many of them sit open simply because the prose was never re-read against the SQL / event surface.

## Verification

- ✅ Wrote only `.missiond/research/board-cleanup/missiond-board-batch-17-20260510.md` inside the declared `write_scope`.
- ✅ Did not call `mission_board_update` or `mission_board_note_add`; no historical Board task statuses changed.
- ✅ `must_not_touch` directories (`.git`, `crates/`, `packages/`, `scripts/`) untouched (read-only).
- ✅ Each reviewed task carries one classification from the allowed set and at least one of `file_path:line` / Board note id / skill-file reference / cross-batch fact as evidence.
- ✅ Heuristic format honoured: original-ask validity question first, then SSOT/code/runtime check, then a single actionable verdict per task — no sub-task spinning.
- ✅ Final answer follows the Findings / Evidence / Recommendations / Verification contract; no raw KB JSON or full logs pasted.
