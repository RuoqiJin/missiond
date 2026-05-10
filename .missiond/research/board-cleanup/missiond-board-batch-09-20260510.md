# MissionD Board Cleanup Batch 09 - 2026-05-10

Scope: read-only review of 5 MissionD-related BoardTasks. This batch used a ClaudeCode worker (`4e655f29-3190-47ac-812d-4ce963298004`) for fact checking; no Board task statuses were changed by me.

This batch also exposed two MissionD infrastructure issues:

- A first delegation with `engine_hint=claude-code` and `intent=research` reported `routes_to_gemini_researcher=true`, but the actual task was claimed by `slot-claude-code-default`. The dispatch response and real slot ownership can disagree.
- A second explicit ClaudeCode duplicate task (`48c06971-1ae6-4a4b-b320-6c23fcac2413`) was created while the first one was already claimed. Autopilot displaced/released the first claim, but the duplicate row remains open. This is a duplicate-review-task cleanup problem, not part of the BoardTask classifications below.

## Summary

| Task ID | Title | Classification | Recommendation |
| --- | --- | --- | --- |
| `db309142-a4fa-4c9e-9310-dd83ec146237` | `[待修复] board_note_add 大文本提交 unknown error` | `close-covered` | Close; handler now returns structured errors and compact receipts for large notes. |
| `661db93e-95d4-4ca2-aaf4-790a5f9bdeac` | Backfill devtool universe checker static gates | `keep` | Still actionable; remove/replace `--dry-run` use for neural-codegen / semantic-terminal checks. |
| `546e47b6-f536-4780-bca2-6572b8033421` | Backfill worker-scheduling attribution and PTY confirming gaps | `keep` | Parent epic is partially covered; keep until child gaps are closed or split. |
| `b716398b-ddb7-4dd2-a318-421b6dd68ddf` | Backfill conversations.task_id attribution for worker conversations | `keep` | Narrow to Gemini reconcile task_id stamping and `list(taskId=...)` query behavior. |
| `32025290-463e-4967-98eb-98f1224b9c88` | `#1 需要一个工位建立本程序的文档和手册` | `rewrite-candidate` | Rewrite into a scoped manual/runbook task, or close as superseded by current docs/SSOT. |

## Evidence

### `db309142-a4fa-4c9e-9310-dd83ec146237`

Covered by current Board note handler.

- `crates/missiond-daemon/src/handlers/knowledge/board/note.rs` validates malformed args through `invalid_board_args`.
- Empty `taskId`, empty `content`, and invalid `noteType` return structured `INVALID_PARAM` errors with suggestions.
- Store failures are routed through `board_store_error`.
- `note_add_response` compacts large note responses once they exceed `COMPACT_NOTE_RESPONSE_THRESHOLD_BYTES = 16_000`.
- Unit test `large_note_response_is_compact_receipt` verifies the compact response path.

Recommendation: close as covered.

### `661db93e-95d4-4ca2-aaf4-790a5f9bdeac`

Still actionable.

- `scripts/check-project-ssot-universe.mjs` still declares:
  - `neural-codegen` as `bash .missiond/check.sh --dry-run`
  - `semantic-terminal` as `bash .missiond/check.sh --dry-run`
- The file comments still describe those projects as dry-run-only static checks.
- The expected target from the task is stronger:
  - `neural-codegen`: `bash .missiond/check.sh`
  - `semantic-terminal`: `bash .missiond/check.sh --skip-rust --skip-node`

Recommendation: keep as a narrow patch. This is not a Board cleanup close candidate.

### `546e47b6-f536-4780-bca2-6572b8033421`

Partially covered, but not clean enough to close.

- Conversation task attribution is present in many paths:
  - `crates/missiond-daemon/src/engine/intent_engine/autopilot.rs`
  - `crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs`
  - `crates/missiond-daemon/src/workers/local/reconcile_worker.rs`
  - `crates/missiond-daemon/src/workers/local/conversation_logger.rs`
  - `crates/missiond-daemon/src/engine/learning_engine/extraction.rs`
  - `crates/missiond-daemon/src/context/slot_env.rs`
- Codex slot/thread attribution has code in `crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs`.
- PTY Confirming exists as a state in `crates/missiond-pty/src/session.rs`, recognition, manager, and MCP boundary text for `mission_pty_confirm`.
- Remaining gaps:
  - Gemini reconcile still creates conversations with `task_id: None`.
  - PTY Confirming policy is distributed across enum/state/MCP text rather than a single policy surface.
  - Checker coverage exists but is not a clean closeout for every child.

Recommendation: keep the parent epic or split it into smaller current tasks before closing.

### `b716398b-ddb7-4dd2-a318-421b6dd68ddf`

Keep, but narrow.

- Positive evidence:
  - `crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs` uses the running slot task to stamp Codex conversations.
  - `crates/missiond-daemon/src/workers/local/conversation_logger.rs` carries compaction task IDs.
  - `crates/missiond-daemon/src/engine/intent_engine/autopilot.rs` and `flow_engine.rs` bind conversation to task.
  - `crates/missiond-daemon/src/workers/local/reconcile_worker.rs` backfills task ID.
  - `crates/missiond-daemon/src/engine/learning_engine/extraction.rs` and `context/slot_env.rs` provide more task-id binding paths.
- Remaining gap:
  - `crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs` still hardcodes `task_id: None`.
  - Historical evidence also suggests `mission_conversation_query(list, taskId=...)` and direct conversation lookup can disagree; the query path should be verified after the Gemini fix.

Recommendation: keep and rewrite acceptance to two points:

1. Stamp `task_id` in the Gemini reconcile path.
2. Verify/fix `mission_conversation_query(action=list, taskId=...)` against the direct `get_conversations_by_task_id` path.

### `32025290-463e-4967-98eb-98f1224b9c88`

Needs rewrite or closure as superseded.

- The task body is effectively only its title and has no acceptance, audience, scope, or owner.
- Existing documentation/SSOT artifacts already exist:
  - `docs/MODULE_CATALOG.md`
  - `docs/CORE_FLOWS.md`
  - `docs/architectures/`
  - `docs/designs/`
  - `.missiond/intent-*.lisp`
  - `.missiond/v3/missiond-blueprint.lisp`

Recommendation: either close as superseded by current docs/SSOT, or rewrite as a concrete manual task with a defined audience such as operator runbook, developer onboarding, or user-facing manual.

## Follow-Up Infrastructure Issues

1. `mission_task_delegate` should honor `engine_hint=claude-code` in its response and actual route, or explicitly report a reroute reason that matches the actual claim.
2. Duplicate read-only review tasks should dedupe by `source target task ids + cleanup batch`, not only by natural-language objective.
3. A worker conversation can be reused across multiple BoardTasks; task attribution changed from `f0fd...` to `4e65...` inside the same ClaudeCode session. This may be acceptable for a persistent PTY, but result artifacts must be per BoardTask, not per session.
