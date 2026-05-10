# MissionD Board Cleanup Batch 23 - 2026-05-10

Scope: read-only review of 5 MissionD-related BoardTasks. Dispatch task: `b445fa10-ec17-4dd7-b55e-6dc195774bbf`. No historical Board task statuses were changed by me. Only this Markdown file under `.missiond/research/board-cleanup/` was written.

Heuristic applied: **first ask whether each original ask still holds today; then verify against SSOT/code/checker/runtime; finally give one actionable verdict**. Wide goals are not split into sub-tasks per dispatch instruction.

All 5 reviewed tasks are currently `status=open`. Four are Phase 6 hardening rows from `docs/designs/phase6-hardening.md`; this batch is unusually clean — three of the four P6 items have shipped exactly as the design doc asked.

## Summary

| Task ID | Title | Original Ask Still Valid? | Classification | Recommendation |
| --- | --- | --- | --- | --- |
| `3ffd8b31-31cd-4ba4-b485-3c24319be1f7` | `Jarvis 异步任务派发给不存在的工位 slot-worker-1` | No — the `slot-worker-1` hardcode at the cited line is gone | `close-covered` | Close; the specific hardcode named in the bug body no longer exists. (`slot-jarvis` default is a different row, tracked under Batch 18 `3b3788b7`.) |
| `804b3abf-8257-4869-90c6-0a41a38fd8c0` | `[P6.2] Intent escape — 意图白名单校验` | No — strict whitelist + fail-fast is in place | `close-covered` | Close; `task_delegate.rs` has `Phase 6.2: Strict intent whitelist — fail-fast on unknown intent` with `VALID_INTENTS.contains` and a structured `INVALID_PARAM` rejection branch. |
| `6bb2b210-24d9-4133-99a6-0448c7442fde` | `[P6.4] Daemon restart reconciliation — 启动时 Board 任务恢复` | No — startup reconciliation is wired with threshold 0 | `close-covered` | Close; `main.rs:412–413` calls `store.recover_stale_running_tasks(0).await` and logs `"Startup: recovered stale running board tasks"`. |
| `746adc3a-f231-4933-bb2e-e152ffa03ed0` | `[P6.5] Slot ID namespace guard — 静态/动态 ID 冲突防护` | No — startup-time prefix validation is in place | `close-covered` | Close; `main.rs:358–362` validates `slot.config.id.starts_with("slot-dyn-")` and rejects with the exact wording the design doc proposed. |
| `8d77260d-6c5d-4b9e-87b1-966c17999ac8` | `[P6.6] Description Token 限制 — BoardTask 描述字段兜底校验` | Yes — no `MAX_DESCRIPTION_LEN` / description size cap is wired | `keep` | Single function-body addition in `crates/missiond-core/src/db/pg/board.rs` `create_board_task` ingress; per dispatch instruction, do not split. |

## Evidence

### `3ffd8b31-31cd-4ba4-b485-3c24319be1f7` — `close-covered`

Heuristic: the specific hardcode the bug names no longer exists.

- The bug body cites `crates/missiond-core/src/ws/server.rs:1014` `assignee: Some("slot-worker-1".to_string())`.
- `rg "slot-worker-1" crates/missiond-core/src/ws/server.rs` returned zero matches today; line 1014 in current `server.rs` is part of the `handle_chat_completions` body (Batch 18/21 evidence on `:979` start, `:1147–1158` `slot-jarvis` default).
- A separate hardcode (`unwrap_or_else(|| "slot-jarvis".to_string())` at `:1158`) is tracked by Batch 18 `3b3788b7-6c9d-408f-b72b-751e342469fa` and is a different ask (default chat-completions slot).

Recommendation: close as covered. If asynchronous Jarvis dispatch is missing in any current path, that is a new bug, not this row.

### `804b3abf-8257-4869-90c6-0a41a38fd8c0` — `close-covered`

Heuristic: design doc's #6.2 has shipped, with the exact framing.

- `crates/missiond-daemon/src/handlers/compute/task_delegate.rs` (around lines 40–80) carries the explicit comment `// Phase 6.2: Strict intent whitelist — fail-fast on unknown intent`.
- Implementation matches the design doc:
  - `let intent = match args.get("intent").and_then(|v| v.as_str()) { Some(i) if VALID_INTENTS.contains(&i) => i, Some(i) => return Ok(ToolResult::structured_error(ToolError::new(error_codes::INVALID_PARAM, &format!("Invalid intent '{}'. Valid: {:?}", i, VALID_INTENTS)))), None => "general", };`
- Silent default for unknown intent → fail-fast `INVALID_PARAM` error; default for missing intent is `"general"` (no longer the silent "coder" the original task warned about).
- `docs/designs/phase6-hardening.md` exists, confirming the design is the source of the row.

Recommendation: close as covered.

### `6bb2b210-24d9-4133-99a6-0448c7442fde` — `close-covered`

Heuristic: design doc's #6.4 has shipped at startup with the requested zero threshold.

- `crates/missiond-daemon/src/main.rs:412–413`:
  - `match store.recover_stale_running_tasks(0).await {`
  - `Ok(n) if n > 0 => info!(count = n, "Startup: recovered stale running board tasks"),`
- The threshold-0 argument matches the design doc's "with 0 threshold" requirement (no 120s watchdog wait at recovery time).
- `:443` comment "have been GC'd, any BoardTask whose assignee still names a `slot-dyn-*` ... this complements it by clearing the …" confirms the startup path is multi-stage: stale-running-tasks recovery + slot-dyn-* assignee cleanup.

Recommendation: close as covered.

### `746adc3a-f231-4933-bb2e-e152ffa03ed0` — `close-covered`

Heuristic: design doc's #6.5 has shipped with the same wording.

- `crates/missiond-daemon/src/main.rs:358–362`:
  - `// Phase 6.5: Validate static slot IDs don't use reserved 'slot-dyn-' prefix`
  - `if slot.config.id.starts_with("slot-dyn-") { ... "Static slot '{}' uses reserved 'slot-dyn-' prefix. ..." }`
- The dynamic-slot side has its own helper: `crates/missiond-daemon/src/engine/intent_engine/autopilot.rs:1672 slot_id.starts_with("slot-dyn-")` plus `is_dynamic_slot_id` test cases at `:6373, :6457`.
- `crates/missiond-daemon/src/context/slot_env.rs:606–608` test asserts `MISSIOND_SLOT_ID = "slot-dyn-test"` for the dynamic path, confirming the prefix is treated as a reserved namespace at runtime.
- The "UNIQUE constraint on dynamic slots table" half of the design doc's recommendation is partially redundant given the startup-time guard prevents the conflict at the source.

Recommendation: close as covered.

### `8d77260d-6c5d-4b9e-87b1-966c17999ac8` — `keep`

Heuristic: design doc's #6.6 is genuinely missing.

- `rg "MAX_DESCRIPTION_LEN|description.*length.*limit|description.*50_?000|description.*truncate"` across `crates/missiond-core/src/db/pg/board.rs` and `crates/missiond-daemon/src/handlers/knowledge/board/` returned zero matches.
- Conversely, `crates/missiond-daemon/src/handlers/knowledge/board/note.rs:3 COMPACT_NOTE_RESPONSE_THRESHOLD_BYTES = 16_000` (Batch 9 evidence) shows the project does pay attention to bounded responses on the note side; the same hygiene was never applied to the BoardTask description ingress.
- The accompanying #6.3 referenced in the task body (build_context throttle) is not in scope here, but the two-line defence the task body proposes still requires this row.

Recommendation: keep. Single-file addition in `crates/missiond-core/src/db/pg/board.rs` `create_board_task` (and its `update` companion if `description` can be patched): truncate at 50 KB with a logged warning. Per dispatch instruction, no sub-task split.

## Notes

- Three of the four P6 hardening rows in this batch (`804b3abf`, `6bb2b210`, `746adc3a`) shipped exactly as the design doc described and were never re-checked against current code; this is the same pattern Batch 17 / 20 saw for "[优化]" rows. The cleanup wave should auto-detect P6.x rows by matching the comment string `// Phase 6.x:` in code against open Board rows mentioning the same identifier.
- `3ffd8b31` is a "spec drifted away from the bug" example — the cited line moved, the hardcode disappeared, but no one updated the row.
- `8d77260d` is the lone real residual; the `note_add` 16 KB threshold is a good shape to copy.

## Verification

- ✅ Wrote only `.missiond/research/board-cleanup/missiond-board-batch-23-20260510.md` inside the declared `write_scope`.
- ✅ Did not call `mission_board_update` or `mission_board_note_add`; no historical Board task statuses changed.
- ✅ `must_not_touch` directories (`.git`, `crates/`, `packages/`, `scripts/`) untouched (read-only).
- ✅ Each reviewed task carries one classification from the allowed set and at least one of `file_path:line` / design-doc reference / cross-batch fact as evidence.
- ✅ Heuristic format honoured: original-ask validity question first, then SSOT/code/runtime check, then a single actionable verdict per task — no sub-task spinning.
- ✅ Final answer follows the Findings / Evidence / Recommendations / Verification contract; no raw KB JSON or full logs pasted.
