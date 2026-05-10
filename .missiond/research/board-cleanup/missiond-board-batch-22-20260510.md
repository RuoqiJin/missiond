# MissionD Board Cleanup Batch 22 - 2026-05-10

Dispatch task: `de137c56-981b-4379-9326-702d1dbc9789`.

Scope: read-only fact check of five MissionD BoardTasks, then write this cleanup artifact under `.missiond/research/board-cleanup/`. I did not update Board status, add Board notes, stage, or commit.

Method: for each task, first ask whether the original ask still holds today; then verify against Board details, SSOT Lisp, code, checker output, and runtime mechanics; finally give one executable cleanup verdict.

## Findings

| Task ID | Current Board Fact | Original Ask Today | Classification | Recommendation |
| --- | --- | --- | --- | --- |
| `28aa2935-f2c4-438c-8677-6e8fa08ced07` | `open`; no Board notes found | The narrow false-positive class is covered by worker/session exclusion, even though generic batch-whitelist math is not implemented | `close-covered` | Close the old row. Reopen only if user-session retrospective false positives need a generic batch-operation whitelist. |
| `e770e29b-25e8-4906-976d-c6d20d3cef3b` | `open`; no Board notes found | Covered operationally: deep analysis dispatches one session at a time and quota output leaves the conversation pending | `close-covered` | Close. Optional future rewrite only if a quota retry storm/cooldown issue is observed. |
| `4208d649-bd7d-44cd-9d64-ec0df5b9d66c` | `open`; no Board notes found | The old `briefing_worker` path is gone; the residual risk belongs to the current embedding-summary path | `rewrite-candidate` | Rewrite to target `embedding_worker` / conversation-summary denoising, not deleted `briefing_worker` behavior. |
| `9e71cf4b-ae65-4fe1-99c5-f0c652477267` | `open`; no Board notes found | Covered by Board-independent memory-slot stuck recovery; implementation uses kill/respawn rather than Ctrl+C | `close-covered` | Close. The current recovery is stronger than the requested interrupt rule for the cited memory-slot scenario. |
| `576b80c8-7c45-4f5b-8755-c55654505dfd` | `open`; no Board notes found | Phase 1 is implemented; the original row is an umbrella for several later ambitions | `rewrite-candidate` | Rewrite into a narrow residual if needed, such as validating taxonomy/UI/drift goals after Phase 1. |

## Evidence

### `28aa2935-f2c4-438c-8677-6e8fa08ced07` - close-covered

Original ask: add a batch-operation whitelist and worker-type filter so deep-analysis / memory worker sessions do not become RetroWorker high-severity false positives.

Evidence:
- Board detail via `mission_board_query`: title is `[RetroWorker] 添加批量操作白名单，消除 deep-analysis/memory worker 假阳性`; status is `open`; no notes returned.
- [retrospective.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/handlers/comm/retrospective.rs:166) still computes `waste_ratio` as repeat calls divided by total calls. There is no explicit successful-batch denominator exclusion in this analyzer.
- [conversation.rs](/Users/jinchen/Projects/missiond/crates/missiond-core/src/db/pg/conversation.rs:2627) selects retrospective candidates only where `c.status = 'completed'`, `c.conversation_type = 'user'`, and slot id is not `slot-memory%`, `slot-diagnosis%`, or `agent-%`.
- [conversation.rs](/Users/jinchen/Projects/missiond/crates/missiond-core/src/db/pg/conversation.rs:2669) applies the same worker-slot exclusion to retrospective backfill, both forced and non-forced.
- [retro_worker.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/workers/sonnet/retro_worker.rs:273) still has severity thresholds based on error rate, waste ratio, blind retry, and repeated tools, but no explicit "batch operation whitelist" branch.

Judgment: the exact whitelist math is absent, but the original incident class no longer reaches the automatic RetroWorker candidate/backfill path. That is enough to close the old deep-analysis/memory false-positive task as covered.

### `e770e29b-25e8-4906-976d-c6d20d3cef3b` - close-covered

Original ask: cap deep-analysis batches to three large sessions and requeue remaining sessions when quota is exhausted.

Evidence:
- Board detail via `mission_board_query`: title is `Deep-analysis worker: 批次限 3 个大会话 + quota 耗尽重入队`; status is `open`; no notes returned.
- [extraction.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/engine/learning_engine/extraction.rs:512) loops over pending conversations but dispatches one conversation, sets `dispatched = true`, and breaks at line 732. Current behavior is stricter than "batch <= 3".
- [conversation.rs](/Users/jinchen/Projects/missiond/crates/missiond-core/src/db/pg/conversation.rs:504) keeps pending deep-analysis eligibility in SQL via `analysis_retries < max`, missing/outdated `analyzed_at`, and bounded active-session probes. There is no destructive pop from a queue before dispatch.
- [supervisor.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/supervisor.rs:204) detects quota/usage exhaustion from PTY response tails, including `out of extra usage`, `usage limit exceeded`, `quota exceeded`, 429, and related patterns.
- [extraction.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/engine/learning_engine/extraction.rs:662) handles auth/quota results by failing the slot task and clearing extraction state, but it does not call `mark_analysis_complete`. The current conversation therefore remains eligible for a later pending query, and other pending sessions were never dequeued.

Judgment: the historic "large batch drains several sessions, quota cuts off the tail, and work is lost" shape is covered. The residual concern is not this task's ask; it would be a narrower cooldown/retry-accounting task if repeated quota wakeups become a real runtime problem.

### `4208d649-bd7d-44cd-9d64-ec0df5b9d66c` - rewrite-candidate

Original ask: prevent `llmSummary` from capturing system-injection text in quota-exhaustion sessions, originally framed around briefing-summary behavior.

Evidence:
- Board detail via `mission_board_query`: title is `[Bug] llmSummary 在 quota-exhaustion 会话中捕获系统注入文本`; status is `open`; no notes returned.
- [mod.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/workers/sonnet/mod.rs:5) states that `briefing_worker` was removed in the v1.3.0 SSOT cutover because update semantics were incompatible with append-only `event_log`.
- [main.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/main.rs:1277) repeats that `briefing_worker` was deleted; message previews now come from `payload_inline`, while semantic briefing is deferred.
- [embedding_worker.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs:440) is the current summary path. It uses raw message content for topic extraction and stores the combined topic summary in `llm_summary` at line 476.
- [embedding_worker.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs:646) also updates `llm_summary` by concatenating per-turn topics. This path has no explicit quota sentinel or system-injection denoising branch.
- [conversation_query.rs](/Users/jinchen/Projects/missiond/crates/missiond-core/src/db/conversation_query.rs:116) includes `[Matched Skills` as a worker-prompt prefix for historical conversation-type audit, but that is classification evidence, not `llm_summary` denoising.
- [observability.rs](/Users/jinchen/Projects/missiond/crates/missiond-core/src/db/pg/observability.rs:812) backfills conversations missing summaries when `llm_summary IS NULL OR llm_summary = '[timeout]'`; there is no quota-exhausted sentinel handling.

Judgment: the original implementation target is stale, but the underlying hygiene concern may still be valid in the new embedding-summary pipeline. Rewrite the row around the current owner and current sentinel behavior.

### `9e71cf4b-ae65-4fe1-99c5-f0c652477267` - close-covered

Original ask: when a memory slot is stuck in `Thinking` with a frozen PTY screen, recover even if no Board task is active.

Evidence:
- Board detail via `mission_board_query`: title is `巡检：Thinking+冻结PTY 应触发自动 Ctrl+C 恢复（无 Board task 场景）`; status is `open`; no notes returned.
- [autopilot.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/engine/intent_engine/autopilot.rs:1756) calls `check_slot_stuck` for `MEMORY_SLOT_ID` and `MEMORY_SLOW_SLOT_ID` during normal autopilot ticks. This path is not gated on an active BoardTask.
- [supervisor.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/supervisor.rs:374) defines a 10-minute stuck threshold and [supervisor.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/supervisor.rs:381) detects non-idle memory slots stuck too long.
- [supervisor.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/supervisor.rs:431) checks JSONL activity before acting, so long-running real work is not killed just because the TUI says `Thinking`.
- [supervisor.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/supervisor.rs:472) intentionally kills and respawns instead of sending Ctrl+C, with the comment that Ctrl+C often does not recover.
- [supervisor.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/supervisor.rs:483) requeues running submit tasks, releases Board claims, marks stuck deep analysis failed if applicable, and resets extraction state.
- [anomaly.rs](/Users/jinchen/Projects/missiond/crates/missiond-pty/src/anomaly.rs:224) can emit a `state_stuck` anomaly for `Thinking` / `ToolRunning`, but the recovery evidence above is the load-bearing mechanism.

Judgment: the exact "same screen x 3 -> mission_pty_interrupt" rule is not implemented, but the original no-Board-task recovery failure is covered by a stronger memory-slot reset path. Close as covered.

### `576b80c8-7c45-4f5b-8755-c55654505dfd` - rewrite-candidate

Original ask: full historical conversation scan to learn user operation habits, with phased scanner, dynamic injection, map-reduce/drift, and UI/correction channels.

Evidence:
- Board detail via `mission_board_query`: title is `历史对话全量扫描 — 学习用户操作习惯`; status is `open`; no notes returned.
- [.missiond/v3/missiond-blueprint.lisp](/Users/jinchen/Projects/missiond/.missiond/v3/missiond-blueprint.lisp:1684) defines `learning-engine-policy`, including `:habit-scan-timeout-secs 600`, `:habit-scan-interval-secs 14400`, and `:habit-scan-batch-size 5`.
- [.missiond/intent-db-conv.lisp](/Users/jinchen/Projects/missiond/.missiond/intent-db-conv.lisp:44) declares `habit_scanned_at`; [.missiond/intent-db-conv.lisp](/Users/jinchen/Projects/missiond/.missiond/intent-db-conv.lisp:99) declares `mark_habit_scanned`; [.missiond/intent-db-conv.lisp](/Users/jinchen/Projects/missiond/.missiond/intent-db-conv.lisp:164) declares unscanned conversation queries.
- [historical_scanner.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs:20) implements `check_historical_scan` with cadence, kill-switch, urgent-work, unscanned-count, and slow-slot-idle gates.
- [historical_scanner.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs:87) fetches a configured batch of unscanned conversations, sends them to the slow memory slot, and [historical_scanner.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs:158) marks sessions scanned after completion.
- [prompts.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/llm/prompts.rs:125) defines the habit extraction dimensions: workflow, style, technical, and correction, with `user_quote` in the detail payload.
- [maintenance.rs](/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/handlers/comm/conversation/maintenance.rs:196) exposes `mission_habit_scan`; `action=trigger` resets cadence so scan runs on the next learning tick.
- Checker output: `node scripts/check-v3-memory-kb-isomorphism.mjs` returned `v3 memory-kb Lisp/code isomorphism check OK`.
- Checker output: `node scripts/check-v3-conversation-ingestion-isomorphism.mjs` returned `v3 conversation-ingestion Lisp/code isomorphism check OK`.

Judgment: the core historical habit scanner exists and is SSOT-owned. The original task still bundles later dynamic-injection, map-reduce/drift, and UI ambitions, so it should be rewritten rather than kept as one wide row.

## Recommendations

1. Close `28aa2935`, `e770e29b`, and `9e71cf4b` as covered by current code paths.
2. Rewrite `4208d649` around the current `embedding_worker` / summary backfill pipeline if polluted `llm_summary` is still observed after the `briefing_worker` removal.
3. Rewrite `576b80c8` as a narrow residual acceptance item for habit-scan taxonomy/UI/drift needs, because Phase 1 scanning is already implemented and checker-backed.
4. Do not split this review task into child BoardTasks; no new Board mutations were needed to produce the cleanup artifact.

## Verification

- Read BoardTask details for the active dispatch and all five target tasks via `mission_board_query`.
- Read the active context pack at `.missiond/v3/runtime/master-control/context-packs/de137c56-981b-4379-9326-702d1dbc9789.lisp`.
- Ran `node scripts/check-v3-memory-kb-isomorphism.mjs`: `v3 memory-kb Lisp/code isomorphism check OK`.
- Ran `node scripts/check-v3-conversation-ingestion-isomorphism.mjs`: `v3 conversation-ingestion Lisp/code isomorphism check OK`.
- Wrote only this report file inside the declared write scope.
- Did not call Board mutation tools, did not edit source files, did not stage, and did not commit.
