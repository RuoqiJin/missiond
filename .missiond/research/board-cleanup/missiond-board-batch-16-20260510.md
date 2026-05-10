# MissionD Board Cleanup Batch 16 - 2026-05-10

Scope: read-only review of 5 MissionD-related BoardTasks. The active task `74844610-6f2e-4a7a-b5a4-fa63ddb1ee1c` contained 8-char prefixes padded with zeros and is now `status=failed` with supervisor notes saying those padded IDs did not resolve in the TSV. Board search surfaced the re-dispatch `d952b946-86a2-4a46-8e4c-7e19fafe6f9a`, which carries the full UUIDs and the same output path. No historical Board task statuses were changed by me. Only this Markdown file under `.missiond/research/board-cleanup/` was written.

Heuristic applied: **first ask whether each original ask still holds today; then verify against SSOT/code/checker/runtime; finally give one actionable verdict**. Wide goals are not split into sub-tasks per dispatch instruction.

All 5 reviewed tasks are currently `status=open`.

## Findings

| Task ID | Title | Original Ask Still Valid? | Classification | Recommendation |
| --- | --- | --- | --- | --- |
| `22661290-d93e-4734-9a1f-8a95fcb9c946` | `[优化] Memory extraction pending 队列预过滤` | Partially — current `conversation_type='user'` gates cover ordinary worker/self-recursion, but low-density user sessions and historical misclassified rows remain a narrower concern | `rewrite-candidate` | Rewrite to the residual: audit/fix historical memory-slot self sessions and add low-knowledge-density suppression only if live metrics still show waste. |
| `233c10f9-729f-47c8-a1b7-685f46a5a3d0` | `Claude Max OAuth Token Relay: headless VPS 自动凭据同步` | No for the original HostVDS MissionD slot premise | `close-stale` | Close the old `slot-clean` / `slot-openclaw` relay task; future remote Claude auth should be a fresh secret-store-backed design, not raw credential sync. |
| `8eff0df1-16b4-4a07-b7a1-633171bc6942` | `GA self-hosted runner 健康检查：检测 stale busy 状态并自动重启` | Yes — but the implementation owner is outside this MissionD repo | `rewrite-candidate` | Rewrite under the correct owner (`xjp-mcp`, private-cloud cron, or runner skill); MissionD has no runner-health implementation surface today. |
| `b535c8f8-1e1a-42dc-be1a-57fc7629664f` | `KB search: architecture:module 结果污染凭据/运维类查询` | Yes — same root cause as `016ceb68` `excludeCategory` ask | `merge-into-existing-candidate` | Merge into `016ceb68-bd3c-414b-8a61-3f08ab0bc520` (Batch 10's `excludeCategory` thread); credential queries already get `Some("credential")` bias in the discovery path but free-form `mission_kb_search` still lacks a black-list. |
| `905e5a26-73e1-44dd-8098-4e41340276f8` | `[优化] Deep-analysis 管线效率：~75% 空分析轮次` | Partially — conversation-type filtering, checkpoint cursor, and minimum-message gates are wired; empty-result backoff/cooldown remains open | `merge-into-existing-candidate` | Merge into `2de8cbb2-...` (exponential backoff, Batch 10) and `dd65b5eb-...` (cooldown + memory_pending error, Batch 10); the broader "75% waste" framing duplicates those two follow-ups. |

## Evidence

### `22661290-d93e-4734-9a1f-8a95fcb9c946` — `rewrite-candidate`

Heuristic: original ask was three-fold (self-reference / deploy-monitor noise / instruction templates). The broad task is stale, but a narrower residual may still be worth implementing after live metrics confirm it.

- `conversation_type='user'` filter is the gate that already eliminates worker / subagent / system streams from the realtime pending pool:
  - `crates/missiond-core/src/db/pg/conversation.rs:1520–1564` `get_pending_realtime_messages_with_limit(...)` constrains the outer SELECT with `WHERE c.conversation_type = 'user'` (line ≈1542) plus `m.role IN ('user', 'assistant', 'tool_result')` for messages.
- Conversation classification is now a shared SSOT rather than ad hoc:
  - `crates/missiond-core/src/db/mod.rs:35–61` says slot-bound sessions derive as `worker` unless a slot category supplies a more specific value.
  - `crates/missiond-core/src/db/conversation_query.rs:34–50` routes provider-aware classification through `classify_conversation_type`.
  - `.missiond/v3/missiond-blueprint.lisp:1796–1801` requires provider-aware `conversation_type` classification and dry-run/report-first historical repair.
- The watermark `c.realtime_forwarded_at` advances per session, providing per-session de-bounce.
- What is not present in the SQL: an explicit `slot_id NOT LIKE 'slot-memory%'` / `c.id != current_memory_slot_session_id` guard. The two memory slots are declared in `crates/missiond-daemon/src/state.rs:26–27` and used as dispatch targets in `crates/missiond-daemon/src/engine/learning_engine/extraction.rs`; the pending SQL relies on correct classification.
- Memory-slot recursion guard upstream of pending: `extraction.rs:125–135` skips realtime extraction *while* a submit task is running on `slot-memory`, but does not exclude its conversations from the pool itself.
- The Board notes already record progress: a major KB consolidation (`b20cafc1` 2026-03-13) deleted 59 old entries / created 12 new; a follow-up checkpoint (`d4f41f24`) closed the "absolute timeout over liveness" thread.

Recommendation: rewrite. Do not keep the old broad "67%空转" task as-is. A useful replacement would be: "Audit live pending rows for memory-slot self sessions; if any remain after classification repair, add explicit slot/session exclusion and optional `low_knowledge_density` suppression for user sessions."

### `233c10f9-729f-47c8-a1b7-685f46a5a3d0` — `close-stale`

Heuristic: ask premise no longer matches deployment topology.

- The 2026-03-13 task was about HostVDS VPS slots (`slot-clean`, `slot-openclaw`). Later evidence makes that exact premise stale:
  - `.missiond/research/memory-review-v2/batches/memory-review-batch-0681.md:1147` records the user saying "不用管VDS 上的 missonD,不用".
  - `.missiond/research/memory-review-v2/collected-5f90876b-final.md:30822` records the same item as a scope clarification already covered by active memory.
  - `.missiond/research/memory-review-v2/collected-5f90876b-final.md:22734` notes the HostVDS truth is nuanced and belongs in HostVDS skill/registry SSOT, not a free-floating MissionD task.
- Code corroborates the move:
  - No matches for `mission_auth_relay`, `auth_relay`, `token.*relay`, or `credentials\.json.*sync` in `crates/` / `scripts/`.
  - There is no `mission_auth_relay` MCP tool surface.
- Current code detects auth failure but does not implement relay:
  - `crates/missiond-pty/src/session.rs:730–757` warns if local `~/.claude/.credentials.json` is absent/unreadable before spawn.
  - `crates/missiond-daemon/src/supervisor.rs:188–200` detects OAuth/auth error text.
  - `crates/missiond-daemon/src/engine/intent_engine/autopilot.rs:2842–2892` treats auth errors as task failure/retry and notes the Board; it does not push credentials.

Recommendation: close as stale. If a new remote Claude Code deployment needs auth automation, create a fresh design with explicit secret-store, rotation, and redaction requirements.

### `8eff0df1-16b4-4a07-b7a1-633171bc6942` — `rewrite-candidate`

Heuristic: ask still holds operationally, but the surface lives outside MissionD.

- Implementation is missing from MissionD runtime code:
  - Targeted search found no runner-health implementation under `crates/` or `scripts/` for `xjp_runner_health`, stale-busy detection, or automatic `actions-runner` restart.
- The task body itself proposes three implementations, all outside MissionD:
  1. AIOps additions — sits in `xjp-mcp` / `aiops` skill.
  2. private-cloud cron — sits under `private-cloud` skill / its own host config.
  3. New MCP tool `xjp_runner_health` — sits in `xjp-mcp`.
- Local memory-review artifacts show the operational knowledge exists as KB/memory text, not as MissionD code: e.g. `.missiond/research/memory-review/batches/memory-review-batch-0199.md:573` records `ga-self-hosted-runner-stale-busy-fix`.

Recommendation: rewrite to move ownership to the right Board (xjp-mcp or private-cloud). Per dispatch instruction, no sub-task spinning here; just classify and let the human reattach it. If it stays on the MissionD board, MissionD will neither implement nor close it.

### `b535c8f8-1e1a-42dc-be1a-57fc7629664f` — `merge-into-existing-candidate`

Heuristic: same root cause as a kept-open task in Batch 10; do not duplicate.

- Existing partial mitigation:
  - `crates/missiond-daemon/src/handlers/knowledge/kb/discovery.rs:25` already calls `kb_search(&format!("{} password", host), Some("credential"))` — when the daemon itself looks up host credentials it pins the category.
  - `crates/missiond-daemon/src/handlers/knowledge/kb/analyze.rs:63` has a `if e.category == "credential"` branch — credentials get special-cased downstream of search.
- Missing surface (the actual ask of this task):
  - `crates/missiond-daemon/src/handlers/knowledge/kb/args.rs:45–63` `KBSearchArgs` exposes `query, category, limit, offset, search_mode, project, include_archived, state_filter` — there is no `excludeCategory` blacklist.
  - `crates/missiond-daemon/src/handlers/knowledge/kb/query.rs:53–242` ranks FTS/vector results, applies review-state filtering, truncates `architecture:module` detail, and attaches snippets; it does not query-sensitively exclude `architecture:module` for credential/SSH searches.
  - BoardTask `016ceb68-bd3c-414b-8a61-3f08ab0bc520` explicitly tracks "`kb_search` 增加 category 排除过滤: 如 `excludeCategory=[\"architecture:module\"]`".
- Free-form `mission_kb_search` from an AI agent does not benefit from `discovery.rs`'s built-in `Some("credential")` bias.

Recommendation: merge into `016ceb68`. Its acceptance ("kb_search excludeCategory") is the smallest patch that fixes both this task and the original deep-analysis-waste-ratio context.

### `905e5a26-73e1-44dd-8098-4e41340276f8` — `merge-into-existing-candidate`

Heuristic: dedup mechanism is partly in code; remaining decisions are the same as two open tasks already kept.

- Already in code:
  - `crates/missiond-core/src/db/pg/conversation.rs:504–541` gates pending deep analysis to `conversation_type = 'user'` and active sessions with at least 100 new `user`/`assistant` messages after `deep_analyzed_message_id`.
  - `crates/missiond-daemon/src/engine/learning_engine/extraction.rs:512–519` computes checkpoint `since_id` from `deep_analyzed_message_id`.
  - `crates/missiond-daemon/src/engine/learning_engine/extraction.rs:528–535` marks completed sessions with fewer than 6 messages as analyzed and skips them.
  - `crates/missiond-daemon/src/engine/learning_engine/extraction.rs:690–699` advances checkpoint or marks full analysis complete after the slow lane returns.
- Not yet in code:
  - Empty-result backoff / cooldown — open in `2de8cbb2-8e7f-4951-af58-4e25fd7a5dcf` (Batch 10 `keep`).
  - `mission_memory_pending` error-typed response when nothing new — open in `dd65b5eb-bc88-4179-8942-ab134747e394` (Batch 10 `keep`).
- The task's own follow-up note (`484a541f`, 2026-03-14) explicitly broadens scope to "整个记忆提取管线", which is precisely the framing of the two kept tasks above plus `22661290` (also in this batch).

Recommendation: merge into `2de8cbb2` and `dd65b5eb`. Re-implementing under a third row would just duplicate the same two patches.

## Recommendations

- Rewrite `22661290-d93e-4734-9a1f-8a95fcb9c946` to the narrow residual around memory-slot self-session audit plus optional low-knowledge-density suppression.
- Close `233c10f9-729f-47c8-a1b7-685f46a5a3d0` as stale for the original HostVDS MissionD slot relay premise.
- Rewrite `8eff0df1-16b4-4a07-b7a1-633171bc6942` under the correct operational owner (`xjp-mcp`, private-cloud cron, or runner skill).
- Merge `b535c8f8-1e1a-42dc-be1a-57fc7629664f` into `016ceb68-bd3c-414b-8a61-3f08ab0bc520`.
- Merge `905e5a26-73e1-44dd-8098-4e41340276f8` into `2de8cbb2-8e7f-4951-af58-4e25fd7a5dcf` and `dd65b5eb-bc88-4179-8942-ab134747e394`.

## Notes

- Two of the five (`b535c8f8`, `905e5a26`) collapse into already-tracked threads; `22661290` should be rewritten to a narrow residual rather than left broad.
- `233c10f9` is a textbook example of a task whose physical premise has dissolved (VPS slot redeployment). Memory entries about deployment topology should auto-tag any task that references `HostVDS` as `subject-to-relocation`.
- `8eff0df1` is a misfile, not a stale task; the work is real but belongs to `xjp-mcp` / `private-cloud`. The cleanup process needs an explicit "wrong board" outcome, not a forced choice between close and keep.

## Verification

- Wrote only `.missiond/research/board-cleanup/missiond-board-batch-16-20260510.md` inside the declared `write_scope`.
- Did not call `mission_board_update` or `mission_board_note_add`; no historical Board task statuses changed.
- Source directories in `must_not_touch` (`crates/`, `packages/`, `scripts/`) were read only; no source edits, staging, or commit.
- Passing checkers used as corroboration: `check-v3-cli-conversation-ingestion-isomorphism.mjs`, `check-v3-conversation-ingestion-isomorphism.mjs`, `check-v3-ops-infra-isomorphism.mjs`, and `check-v3-pty-recognition-isomorphism.mjs`.
- Each reviewed task carries one allowed classification and concrete evidence from file paths/functions, checker results, Board details, Board notes, or cross-batch Board evidence.
