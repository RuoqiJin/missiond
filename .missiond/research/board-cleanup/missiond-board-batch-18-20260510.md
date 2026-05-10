# MissionD Board Cleanup Batch 18 - 2026-05-10

Scope: read-only review for active BoardTask `9f4bc949-da5b-4103-937c-6fbb4143a2e1`.
Reviewed target tasks: `a861cebf-5268-4484-a41f-bfb3c9fcf72e`, `e22ce935-d0f6-4203-afd6-11789cef40d5`, `3b3788b7-6c9d-408f-b72b-751e342469fa`, `3350f86d-8f83-45ac-8041-8137e8d8fe42`, `b2e1f7d2-4504-490a-800a-c8f49336da4f`.

Method: ask whether the original ask still holds today, then verify against Board details, SSOT/code/checkers, and skill operational facts. No Board status/notes were changed.

## Findings

| Task ID | Original ask today | Classification | Recommendation |
| --- | --- | --- | --- |
| `a861cebf-5268-4484-a41f-bfb3c9fcf72e` | Partially valid, but too broad now. Current code already filters to `conversation_type='user'` and skips zero-user-message batches; the residual belongs to narrower memory-noise/self-recursion/backoff rows. | `merge-into-existing-candidate` | Merge into the existing memory-noise family, especially `2ba85d68-4a76-4c17-8bad-382a57b5250a` and `2de8cbb2-8e7f-4951-af58-4e25fd7a5dcf`; do not keep this broad duplicate as a separate implementation row. |
| `e22ce935-d0f6-4203-afd6-11789cef40d5` | Valid, but should be rewritten against the current conversation API/turn-skeleton architecture rather than the old raw `mode=summary` wording. | `rewrite-candidate` | Rewrite as a bounded `deep_summary`/turn-skeleton response for `mission_conversation_get`, including public tool schema and deep-analysis prompt updates. |
| `3b3788b7-6c9d-408f-b72b-751e342469fa` | Still valid exactly as written. `/v1/chat/completions` still falls back to `slot-jarvis` when `X-Slot-Id` is absent. | `keep` | Keep as a small code task: configurable default chat slot or model->slot mapping, preserving `X-Slot-Id` override. |
| `3350f86d-8f83-45ac-8041-8137e8d8fe42` | The old polling discipline issue is superseded by current deploy wait/watch workflow guidance and a completed Deploy Center tooling task. | `close-superseded` | Close as superseded by `xjp_build_wait`/`xjp_deploy_watch` workflow guidance plus done task `faebf9b9-d682-4871-b76a-2d2a9ec0e689`. A literal `xjp_build_watch` alias can be a doc touch, not a Board row. |
| `b2e1f7d2-4504-490a-800a-c8f49336da4f` | Valid residual, but duplicate of a more specific open row for cross-pipeline KB dedupe. Current code detects conflicts after creation; it does not pre-write merge/refuse duplicates. | `merge-into-existing-candidate` | Merge into `61a87ef1-1801-4bcd-9b97-614859d84fdc` (`KB 跨管线去重...`), which is the clearer owner for realtime/deep-analysis duplicate writes. |

## Evidence

### Board Facts

- `mission_board_query(action=get, ids=[...])` shows all five reviewed tasks are still `status=open` and have no notes.
- Related open Board rows:
  - `2ba85d68-4a76-4c17-8bad-382a57b5250a`: memory worker self-referential feedback loop, pending queue filtering.
  - `2de8cbb2-8e7f-4951-af58-4e25fd7a5dcf`: Memory scheduler adaptive throttling / consecutive empty-result backoff.
  - `61a87ef1-1801-4bcd-9b97-614859d84fdc`: cross-pipeline KB dedupe for realtime and deep-analysis.
- Related completed Board row:
  - `faebf9b9-d682-4871-b76a-2d2a9ec0e689`: Deploy Center tooling completion note says `build_wait` and `build_watch` behavior were fixed.
  - `d305b87a-2f1b-4c1c-9209-d4189695859e`: investigation concluded `ExtractionState` had no adaptive empty-result backoff and fixed 3s debounce was a root cause.

### Code / SSOT Evidence

- Memory realtime filtering already exists but is partial:
  - `crates/missiond-core/src/db/pg/conversation.rs:1520-1544` uses a bounded lateral query, selects only `m.role IN ('user', 'assistant', 'tool_result')`, and restricts outer rows to `c.conversation_type = 'user'`.
  - `crates/missiond-daemon/src/engine/learning_engine/extraction.rs:178-214` skips pending batches with zero user messages and advances their realtime watermark.
  - `crates/missiond-daemon/src/state.rs:43-65` has no `consecutive_empty_count` or backoff field.
  - `crates/missiond-daemon/src/bus/v2_subscribers.rs:522-540` still triggers realtime extraction through a fixed 3-second debounce.
  - `.missiond/v3/missiond-blueprint.lisp:1709-1711` pins bounded pending SQL and learning-engine policy ownership, not task-specific natural-language filters.

- `conversation_get` summary mode is not implemented as requested:
  - `crates/missiond-daemon/src/handlers/comm/conversation/query.rs:154-188` accepts `tail`, `since_id`, `include_raw`, `include_labels`, `include_user_index`, and `include_turns`; there is no `mode`.
  - `crates/missiond-mcp/src/tools/comm/conversation.rs:16-24` public schema exposes `tail`, `sinceId`, and `includeRaw`, but not `mode` or `includeTurns`.
  - `crates/missiond-daemon/src/engine/learning_engine/extraction.rs:547-568` still instructs the slow-lane agent to call `mission_conversation_get(...)` for checkpoint/full analysis.

- Chat completions default slot hardcode is still present:
  - `crates/missiond-core/src/ws/server.rs:1147-1158` uses `X-Slot-Id` if present, otherwise `unwrap_or_else(|| "slot-jarvis".to_string())`.
  - `rg -n "default_chat_slot" crates packages scripts .missiond/v3` returned no matches.
  - `crates/missiond-core/src/types/slot.rs:198-207` still contains historical compatibility logic around `slot-jarvis` category detection, but that is not a configurable chat-completions fallback.

- Deploy polling guidance has moved on:
  - `/Users/jinchen/.claude/skills/backend-deploy/SKILL.md:123-126` still has a one-shot GA status check with `xjp_github_workflow_status`.
  - `/Users/jinchen/.claude/skills/backend-deploy/SKILL.md:360-381` defines the actual deploy workflow as trigger CI -> `xjp_build_wait` -> `xjp_deploy_watch`.
  - `/Users/jinchen/.claude/skills/xjp-mcp/SKILL.md:47-54` marks `xjp_deploy_watch` as the primary current monitoring tool.

- KB dedupe exists as post-write conflict detection and periodic cleanup, not as the pre-write cross-pipeline gate the task asked for:
  - `crates/missiond-daemon/src/handlers/knowledge/kb/remember.rs:27-35` writes/updates the KB entry and enqueues embedding processing before duplicate/conflict handling.
  - `crates/missiond-daemon/src/handlers/knowledge/kb/remember.rs:89-93` calls `detect_kb_conflicts` only when the result action is `created`.
  - `crates/missiond-daemon/src/handlers/knowledge/kb/conflicts.rs:3-10` defines semantic conflict detection with `CONFLICT_SIM_THRESHOLD = 0.82`.
  - `crates/missiond-core/src/db/pg/knowledge.rs:1216-1240` implements `kb_find_duplicates()` for later duplicate discovery using Jaccard threshold `0.6`.
  - `crates/missiond-daemon/src/engine/learning_engine/extraction.rs:790-803` runs periodic KB consolidation through `mission_kb_analyze(mode="consolidation_plan", ...)`, which is after-the-fact maintenance.

## Recommendations

1. Close/merge candidates without implementation: `a861cebf` -> memory-noise/backoff/self-recursion rows; `b2e1f7d2` -> `61a87ef1`.
2. Rewrite `e22ce935` before implementation so acceptance matches today's architecture: public schema support, handler support, turn-skeleton or bounded summary payload, and deep-analysis prompt usage.
3. Keep `3b3788b7` as a narrow code fix. The current code evidence is exact and still live.
4. Close `3350f86d` as superseded. Current deploy workflow guidance already uses wait/watch; retaining an old Board row for the exact string `xjp_build_watch` would add noise.

## Verification

- Ran `node scripts/check-v3-memory-kb-isomorphism.mjs` -> `v3 memory-kb Lisp/code isomorphism check OK`.
- Ran `node scripts/check-v3-conversation-ingestion-isomorphism.mjs` -> `v3 conversation-ingestion Lisp/code isomorphism check OK`.
- Ran `node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs` -> `v3 CLI conversation ingestion isomorphism check OK`.
- Wrote only this report under `.missiond/research/board-cleanup/`.
- Did not call `mission_board_update`, `mission_board_note_add`, staging, or commit commands.
