# MissionD Board Cleanup Batch 15 - 2026-05-10

Scope: read-only review of 5 MissionD-related BoardTasks. Dispatch task: `1f6f1600-44cb-4b2e-ba8b-0de14fd0972c`. No historical Board task statuses were changed by me. Only this Markdown file under `.missiond/research/board-cleanup/` was written.

Heuristic applied: **first ask whether each original ask still holds today; then verify against SSOT/code/checker/runtime; finally give one actionable verdict**. Wide goals are not split into sub-tasks per dispatch instruction.

All 5 reviewed tasks are currently `status=open` at the parent level. Several children are already `skipped` from a previous cleanup wave.

## Summary

| Task ID | Title | Original Ask Still Valid? | Classification | Recommendation |
| --- | --- | --- | --- | --- |
| `789ef24c-b9a0-4d76-ba73-735094a4a8fa` | `Board 事件总线集成 — 独立 Board 泳道 + 细粒度事件类型` | Backend ask is satisfied; UI swimlane remains aspirational | `keep` | Leave open as a frontend swimlane task only; backend variants and emit sites are already in code. |
| `2629bde5-d535-4948-8faa-aed07d5b4ed3` | `研究：代码文档与手册系统` | No — explicitly self-merged | `close-superseded` | Parent epic; the description itself says merged into the holographic code context engine. |
| `d08f930a-ecf5-4fda-9589-9e15928312de` | `研究：代码 Embedding 与预取加速` | No — explicitly self-merged | `close-superseded` | Parent epic; both children already `skipped`; embedding strategy and #9 prefetch decisions are recorded in the description itself. |
| `05bc86a2-af20-413b-a659-86301a52ab19` | `研究：PTY 状态识别系统` | No — capability already in production | `close-superseded` | Parent epic; all four children already `skipped`; `crates/missiond-pty/` ships the recognition module. |
| `3f44bb8e-ab36-4144-a7b8-23c532a7a276` | `Context Prefetch v2 观测期 — 收集数据校准阈值` | No — observation due date 2026-03-20 elapsed without a tuning report | `close-stale` | Threshold (`MIN_COSINE_SIM = 0.35`) is unchanged in code, no P2 cross-encoder landed, and architecture has moved on; if a tuning report is still wanted, file it as a fresh, scoped task. |

## Evidence

### `789ef24c-b9a0-4d76-ba73-735094a4a8fa` — `keep`

Heuristic: the 2026-03-07 ask had two halves — Rust event surface and frontend swimlane. The backend half is done; the frontend half is not.

- BoardEvent variants exist with the exact shapes the task specified:
  - `crates/missiond-core/src/event/events/board.rs:3–5` module comment lists "v1 variants: `BoardTaskCreated`, `BoardTaskStatusChanged`, `BoardTaskNoteAdded`, `BoardTaskClaimed`, `BoardTaskDeleted`, `BoardTaskUpdated`."
  - Round-trip test cases at `:74–100` cover `BoardEvent::TaskCreated`, `StatusChanged` (with `old_status` + `new_status`), `NoteAdded` (with `note_id` + `content_preview`), `Claimed` (`task_id` + `slot_id`), `Deleted` (`task_id` + `title`), `Updated` (`task_id` + `status`).
- Emit sites are wired across every Board mutation handler:
  - `crates/missiond-daemon/src/handlers/knowledge/board/create.rs:54` `publish_board_created(state, &task)`.
  - `crates/missiond-daemon/src/handlers/knowledge/board/note.rs:78` publishes `BoardEvent::NoteAdded`.
  - `crates/missiond-daemon/src/handlers/knowledge/board/update.rs:73–75 / :129 / :199–201` route through `publish_board_status_changed` (carrying old + new status) or `publish_board_update` as fallback.
  - `crates/missiond-daemon/src/handlers/knowledge/board/claim.rs:31–32` publishes `Claimed`.
  - `crates/missiond-daemon/src/handlers/knowledge/board/delete.rs:29–30` publishes `Deleted`.
  - `crates/missiond-daemon/src/handlers/knowledge/board/events.rs:4–13` defines the publish helpers.
- Frontend swimlane is missing:
  - `rg 'swimlane|board[_-]lane' packages/board/src` returned zero matches.
  - The task's referenced design doc `docs/designs/board-timeline-swimlane.md` does not exist (`ls` did not surface it; only `context-prefetch-pipeline.md` and `context-prefetch-v2.md` were present).

Recommendation: keep. The single remaining piece is the frontend swimlane render + colour/icon config. Per dispatch instruction, do not spin off a sub-task; leave this row open and let the next frontend pass close it against the existing event types.

### `2629bde5-d535-4948-8faa-aed07d5b4ed3` — `close-superseded`

Heuristic: this is the textbook "already-merged" epic. The body itself records the merge.

- Description: "已合并入全息代码上下文引擎. 原始研究任务 #1 #3 #4 的文档/lisp/手册需求通过 Language-native Stub + 架构手册混合生成实现."
- Children's status (already classified by earlier batches):
  - `32025290-...` `#1` — Batch 9 / Batch 12 verdict `rewrite-candidate`.
  - `0224127e-...` `#3` — Batch 12 verdict `close-superseded` (the Lisp doc tree exists at 403 files under `.missiond/`).
  - `1e63d830-...` `#4` — Batch 12 verdict `rewrite-candidate`.
- The parent itself adds nothing the children don't already track; keeping it open creates a permanently-half-done umbrella row.

Recommendation: close as superseded at the parent level. The two child rows still flagged `rewrite-candidate` carry whatever residual work remains.

### `d08f930a-ecf5-4fda-9589-9e15928312de` — `close-superseded`

Heuristic: same self-merged pattern; both children are already `skipped`.

- Description: "已合并入全息代码上下文引擎. 代码 Embedding 策略决策：不 embed 裸代码,embed tree-sitter 提取的自然语言描述. 代码预取 (#9) 通过 P3 Prefetch Injection 实现."
- Children:
  - `a62cad62-...` `#2` — `status=skipped`, board-cleanup note `board-cleanup-910a605eef4c54e05c` (2026-05-10) `roadmap superseded by V3/OCaml/shared-memory`.
  - `3a9d2eb9-...` `#9` — `status=skipped`, board-cleanup note `board-cleanup-0c0d815ef9acb68db7` (same wave).
- The parent description itself records the embed strategy and the #9 → P3 Prefetch mapping, so there is nothing to chase.

Recommendation: close as superseded. The cleanup wave that closed both children should also have closed the parent.

### `05bc86a2-af20-413b-a659-86301a52ab19` — `close-superseded`

Heuristic: capability already shipped; all four children already closed.

- Description: "PTY 识别 Gemini CLI / Codex CLI 状态,抽象为可复用模块,支持集成测试和多产品扩展". Today this maps directly to `crates/missiond-pty/src/pty_recognition.rs` (used in earlier batches as the canonical PTY state-recognition module — see Batch 9 / Batch 12 entries on `2eda2ce3` / `c507283f` for `:533`, `:823–828`, etc.) plus the multi-CLI design doc `docs/designs/pty-multi-cli-state-detection.md`.
- Children:
  - `118e145c-...` `#10` PTY识别 Gemini/Codex — `skipped` (`board-cleanup-0ec948142aad7407e4`).
  - `184dbccc-...` `#11` 抽象为可复用模块 — `skipped` (`board-cleanup-88b4c959e16227c1a7`).
  - `6a9960a5-...` `#12` 集成测试定期检查 — `skipped` (`board-cleanup-744ba0435a81fe0d86`).
  - `6c89e5da-...` `#13` GPT5.4 识图辅助 — `skipped` (`board-cleanup-a4147675acc1349f07`).
- All four cleanup notes carry the same justification "roadmap superseded by V3/OCaml/shared-memory".

Recommendation: close as superseded at the parent level for the same reason its children were closed.

### `3f44bb8e-ab36-4144-a7b8-23c532a7a276` — `close-stale`

Heuristic: this is a self-timing-out observational task. The observation window has long elapsed without producing the report it asked for, and the system has moved on.

- Due date `2026-03-20`; today is `2026-05-10` — the observation window expired about seven weeks ago. The task's own completion condition ("收集 1 周数据，输出调优报告") was never met: no tuning report exists under `docs/designs/` or `.missiond/research/`.
- Indicator wiring is still in place (so any new observation could resume), but no decision was committed:
  - `crates/missiond-daemon/src/context/context_pipeline.rs:49` `const MIN_COSINE_SIM: f32 = 0.35;` — unchanged from the value the task wanted to validate.
  - Stats counters: `:268 prefetch_router_latency.record(elapsed_us)`, `:479 prefetch_total.fetch_add`, `:491 / :614 prefetch_bypass.fetch_add`, `:850–860 / :986–996 cosine cutoff branch`, `:853 / :989 prefetch_cosine_filtered.fetch_add`.
  - Stats surface: `crates/missiond-daemon/src/infra/daemon_stats.rs:103 prefetch_cosine_filtered: AtomicU64`, `:226 "cosine_filtered": Self::l(&self.prefetch_cosine_filtered)` exposed to JSON.
  - `mission_health` MCP entry: `crates/missiond-daemon/src/handlers/sysinfra/infra.rs:57` and `:misc.rs:86`.
- Pending decisions in the task body have not been actioned anywhere in code:
  - `MIN_COSINE_SIM` still 0.35.
  - No P2 Cross-Encoder rerank stage in `context_pipeline.rs`.
  - No `INTENT_MODEL` Sonnet downgrade flag found.
- Architectural drift: the project memory and prior batches (especially Batch 11 / Batch 14) record the move toward V3 SSOT + OCaml + shared-memory; the v2 Prefetch observation framing pre-dates that move.

Recommendation: close as stale. The instrumentation stays in code (no harm); if the user still wants threshold tuning, file a new, scoped task that names the exact decision (e.g., "either lower `MIN_COSINE_SIM` to 0.30 or close it out") rather than reopening an observation window from March.

## Notes

- The three `研究` parent epics (`2629bde5`, `d08f930a`, `05bc86a2`) all show the same anti-pattern: parent stays open while children are closed, holding the cleanup score artificially low. The next cleanup wave should sweep parents whose all-children are `skipped` together.
- Per dispatch heuristic, no new sub-tasks were created for `789ef24c` even though the residual swimlane work is well-defined; the existing row is enough to track it.
- `3f44bb8e` is the cleanest example of a "completion-by-time-elapsed" task that should auto-stale once `dueDate` is more than `2 * (planned observation window)` past.

## Verification

- ✅ Wrote only `.missiond/research/board-cleanup/missiond-board-batch-15-20260510.md` inside the declared `write_scope`.
- ✅ Did not call `mission_board_update` or `mission_board_note_add`; no historical Board task statuses changed.
- ✅ `must_not_touch` directories (`.git`, `crates/`, `packages/`, `scripts/`) untouched (read-only).
- ✅ Each reviewed task carries one classification from the allowed set and at least one of `file_path:line` / Board note id / cross-batch fact as evidence.
- ✅ Heuristic format honoured: original-ask validity question first, then SSOT/code/runtime check, then a single actionable verdict per task — no sub-task spinning.
- ✅ Final answer follows the Findings / Evidence / Recommendations / Verification contract; no raw KB JSON or full logs pasted.
