# MissionD Board Cleanup Batch 13 - 2026-05-10

Scope: read-only review of 5 MissionD-related BoardTasks. Dispatch task: `faf80aa7-a0e2-4c71-8c8d-8bf10761ade3`. No historical Board task statuses were changed by me. Only this Markdown file under `.missiond/research/board-cleanup/` was written.

All 5 reviewed tasks are currently `status=open`.

## Summary

| Task ID | Title | Classification | Recommendation |
| --- | --- | --- | --- |
| `5d6b5705-74c0-4367-8ff4-f9c8d9cef7c0` | `Add nightly/checker coverage for worker-scheduling evidence gaps` | `keep` | Checkers exist but are not yet wired into nightly_evolution; depends on three sibling tasks that are still `keep`. |
| `2c171d4e-18b2-44cb-b36d-45d900a8480a` | `#8 KB 记忆后清理原始会话（30 天保留）` | `keep` | Event-bus retention exists, gemini_log cleanup exists, but the conversation-table-level 30-day prune does not. |
| `481e4ab7-a2e8-40ec-ad60-7dfb89da74b0` | `Jarvis 通道计费模型设计` | `close-stale` | 2026-02-26 deferred-from-day-one task; no matching code or design doc has landed and the architecture has moved to direct slot accounting. |
| `8573a84b-210b-49bf-897a-825d7fe4634e` | `Phase 5: KB 分类涌现 — 打破硬编码枚举` | `keep` (partial) | The "硬编码枚举" never existed (`category` is already free-form `String`), but the auto-emergence / clustering pipeline is not implemented. |
| `2bf31fe5-5532-4b22-9b7f-6e5a7d14c3ac` | `#14 任务步骤翻阅系统（卷轴式 UI）` | `rewrite-candidate` | Backend foundation (`step_narrator`) was deleted in v0.4.23 Phase 6; the UI feature cannot ship as written. |

## Evidence

### `5d6b5705-74c0-4367-8ff4-f9c8d9cef7c0` — `keep`

The checker scripts exist, but the nightly loop does not invoke them yet, and the upstream contract tasks this depends on are still open.

- Checker scripts present:
  - `scripts/check-v3-conversation-ingestion-isomorphism.mjs`
  - `scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs`
- Nightly evolution wiring (`crates/missiond-daemon/src/engine/nightly_evolution.rs`):
  - Line 101 `start_nightly_evolution_service` and line 232 `run_nightly_evolution_once` exist.
  - Schedule defaults to OFF (line 105–108: `nightly-evolution schedule disabled by default; use mission_nightly_evolution or set env=true to run periodically`).
  - The only checkers explicitly invoked are `node scripts/check-v3-final-convergence.mjs --json --static-only` (lines 341, 396) and `scripts/check-v3-code-isomorphism-complete.mjs` (line 352). The two ingestion-isomorphism checkers above are not directly invoked here.
- Task `dependsOn`: `b716398b`, `d47d7800`, `2eda2ce3`. Of these, `b716398b` and `2eda2ce3` are still `keep` per Batch 9 / Batch 12 reviews.

Recommendation: keep. Move only after the contract tasks settle, then either invoke the two ingestion-isomorphism checkers from `nightly_evolution` or merge them into the aggregate `check-v3-final-convergence.mjs` set.

### `2c171d4e-18b2-44cb-b36d-45d900a8480a` — `keep`

The 30-day retention idea is partly present in adjacent surfaces but not on the conversation table itself.

- Adjacent retention code that already exists:
  - `crates/missiond-daemon/src/bus/retention_cron.rs:1–37`: daily retention + orphan-subscription cleanup; the lifecycle module evicts entries with `last_seen_at` > 30 days.
  - `crates/missiond-core/src/db/pg/observability.rs:311 gemini_log_cleanup(retention_days: i64)` — bound deletion based on a configurable retention window for Gemini logs.
  - `crates/missiond-daemon/src/handlers/comm/capability_usage/runtime.rs:152` and 170 use `Duration::days(30)` as a window for usage queries (read, not delete).
- What is missing for the task:
  - No code path deletes rows from `conversations` or `conversation_messages` after 30 days.
  - `crates/missiond-daemon/src/handlers/comm/codex_ops.rs:73` explicitly notes "No coarse prune by conversation timestamps — they don't update reliably on…", confirming the team consciously skipped a naive timestamp-based prune.
- The task body is a one-liner aspiration without an explicit safety contract (e.g., "only after KB extraction has consumed the conversation; only after embedding distillation").

Recommendation: keep. Before implementation, add an acceptance line: "delete `conversations` rows older than 30 days only when (a) `analyzed_at IS NOT NULL` (Layer 1 distillation done) and (b) embedding row(s) are committed; otherwise skip and emit an `IncidentEvent`."

### `481e4ab7-a2e8-40ec-ad60-7dfb89da74b0` — `close-stale`

The 2026-02-26 Gemini 3.1 Pro recommendation never landed and the original framing is outdated.

- The task body itself defers explicitly: "优先级：端到端跑通后再处理".
- No matching code surface in `crates/`:
  - No matches for `jarvis.*credit`, `jarvis.*billing`, `jarvis.*token`, or `token_multiplier`.
  - The only references to `jarvis-missiond` model framing are in `docs/audit/session-1-implementation-raw.md` (the original advisory text) and an unrelated OAuth failure message in `crates/missiond-daemon/src/engine/intent_engine/autopilot.rs:2886`.
- Architectural drift since 2026-02:
  - The project standardised on direct slot orchestration with per-slot model accounting (see `mission_slots`, slot-memory / slot-claude-code, etc.) instead of fronting Claude Code with a `jarvis-missiond` proxy model.
  - Project memory `multi-model-strategy-opus-gemini-minimax` and `policy-router-chat-never-specify-model` further pin model choice to slot/router policies, not a single Jarvis billing channel.

Recommendation: close as stale. If Jarvis-style billing returns later, write a fresh task anchored to the current slot/router accounting model rather than the 2026-02 PTY-token-multiplier framing.

### `8573a84b-210b-49bf-897a-825d7fe4634e` — `keep` (partial)

The premise "打破硬编码枚举" is already inaccurate, but the auto-emergence pipeline is genuinely missing.

- `crates/missiond-core/src/types/knowledge.rs:10` declares `pub category: String`. There is no Rust enum or DB constraint enumerating categories — `kb_remember` just persists the input string, e.g.:
  - `crates/missiond-daemon/src/handlers/knowledge/kb/remember.rs` line ~84: `categories: vec![input.category.clone()]`.
- Migration `crates/missiond-core/migrations/20260318000000_init.sql:160` declares `category TEXT NOT NULL` (no `CHECK` enum).
- What the task asked for that is *missing*:
  - L1 free-form tag emission: already on (today's `category` is already free-form).
  - L3 cluster / topic-modeling / proposal flow: no matches for `dynamic.*category`, `emerging.*category`, `topic_modeling`, `cluster.*kb`, or `tag.*emergence` in `crates/missiond-daemon/src/handlers/knowledge/` or `engine/learning_engine/`.
- Parent `bc4dabec` was reviewed in Batch 7; this Phase 5 child is the last open phase under that parent.

Recommendation: keep, but rewrite the description so it does not claim a non-existent enum constraint. Acceptance candidates: implement an L3 clustering job that reads `knowledge` rows by category prefix, runs simple co-occurrence / embedding clustering, and writes a `category-proposal` row to KB; promotion stays manual.

### `2bf31fe5-5532-4b22-9b7f-6e5a7d14c3ac` — `rewrite-candidate`

The UI cannot ship in its current shape because its backend foundation was deleted.

- `step_narrator` worker (which abstracted "每一步动作" into short phrases) was removed:
  - `crates/missiond-daemon/src/workers/codex/mod.rs:4`: `step_narrator removed v0.4.23 Phase 6 (message_narrations + narration_cursors dropped).`
  - `crates/missiond-daemon/src/main.rs:1280`: comment confirming the deletion.
  - `crates/missiond-core/src/db/traits.rs:445–446`: `narration (removed v0.4.23 Phase 6): tables message_narrations + narration_cursors dropped; step_narrator worker deleted together.`
  - Migration `crates/missiond-core/migrations/20260421000000_drop_deprecated_tables.sql` lines 4–5 list both tables as `pending-drop-v0.4.12`; lines 16–17 actually drop them.
- Frontend panels exist (`packages/board/src/components/timeline/CognitiveTimeline.tsx`, `UnifiedDetailPanel.tsx`, `JarvisChat.tsx`, `Conversations.tsx`), but they cannot fill a scroll-style step list without an upstream "abstract this step into a phrase" stream.

Recommendation: rewrite. Either (a) re-introduce a step-summarisation worker (e.g., re-instantiate `step_narrator` against today's `event_log` SSOT) and then build the scroll UI on top, or (b) re-scope the UI to render existing structured slot events (`SlotEvent::TaskDispatched`, `SlotEvent::BecameIdle`, etc.) as scroll items without LLM-summarised phrases.

## Notes

- Two of the five (`481e4ab7`, `2bf31fe5`) are obsolete because of architectural deletions (Jarvis proxy model, `step_narrator` worker). These are the kind of items the close-candidates wave should flag automatically next time the drift detector runs.
- `5d6b5705`'s upstream chain (`b716398b`, `d47d7800`, `2eda2ce3`) is the same chain audited in Batch 9 / Batch 12; closing this task without its upstream peers will leave the parent epic `546e47b6` half-finished.

## Verification

- ✅ Wrote only `.missiond/research/board-cleanup/missiond-board-batch-13-20260510.md` inside the declared `write_scope`.
- ✅ Did not call `mission_board_update` or `mission_board_note_add`; no historical Board task statuses changed.
- ✅ `must_not_touch` directories (`.git`, `crates/`, `packages/`, `scripts/`) untouched (all reads only).
- ✅ Each reviewed task carries one classification from the allowed set and at least one of `file_path:line` / migration / commit / Board fact as evidence.
- ✅ Final answer follows the Findings / Evidence / Recommendations / Verification contract; no raw KB JSON or full logs pasted.
