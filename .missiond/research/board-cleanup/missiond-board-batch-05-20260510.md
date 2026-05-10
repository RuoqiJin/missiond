# MissionD Board Cleanup Batch 05

- generated_at: 2026-05-10
- scope: `project=missiond`, `status in (open, blocked)`, high-priority window offset 20
- mode: read-only investigation; no Board mutation

## Batch conclusion

All five tasks describe the same historical memory-worker failure cluster. They should not remain as five open tasks. Merge into two rewrite candidates:

1. `memory-extraction-anti-spin-policy`
   - empty-yield exponential backoff
   - hard call fuse for repeated `mission_memory_pending`
   - self-session / worker-output exclusion
   - diagnostic event when the fuse trips

2. `role-stable-worker-lanes`
   - compact-aware lane restart/reinjection
   - lane tool-policy where provider supports it
   - durable task binding and artifact output

The old tasks can close after those two rewrite candidates are captured.

## 1. `97595bc3-162e-4c94-b60d-91f2ec66c9b3`

- title: `🐛 slot-memory 记忆提取循环 bug`
- classification: `merge`
- current status: The original loop path is mostly mitigated: realtime extraction now filters to `conversation_type='user'`, advances watermarks on completion/error/timeout, and memory review has moved to artifact workflows. Still useful as evidence for the anti-spin policy.
- evidence:
  - `crates/missiond-core/src/db/pg/conversation.rs` realtime pending query uses `WHERE c.conversation_type = 'user'`.
  - `crates/missiond-daemon/src/engine/learning_engine/extraction.rs` filters sessions with no user messages and advances watermarks on send-complete or send error.
  - `crates/missiond-daemon/src/handlers/knowledge/memory.rs` still only has a textual repeated-call warning.

## 2. `e7d4e337-bf5c-444b-b302-897159519784`

- title: `Memory Worker Idle Backoff 未实现：30/31 轮空转浪费 Opus token`
- classification: `merge`
- current status: The exact exponential backoff is still not present as a first-class invariant/code path. Keep only as evidence for `memory-extraction-anti-spin-policy`.
- evidence:
  - `rg consecutive_empty` found no runtime counter.
  - `memory-kb-policy` has extraction budgets and review overlay, not empty-yield backoff.
  - `learning-engine-policy` has cadences/timeouts, not per-session empty result backoff.

## 3. `43292726-45f0-40bd-9e02-a1a3ae383146`

- title: `[待修复] mission_memory_pending 增加服务端硬熔断防止轮询死循环`
- classification: `merge`
- current status: The task is still directionally correct: current `pending_served` returns text, not `isError` or a hard stop. It should be implemented as a runtime guard in the anti-spin candidate rather than kept as a standalone old task.
- evidence:
  - `crates/missiond-daemon/src/handlers/knowledge/memory.rs` repeated pending call returns `ToolResult::text(...)`.
  - No per-extraction-cycle counter or hard error threshold was found.

## 4. `2ba85d68-4a76-4c17-8bad-382a57b5250a`

- title: `[待修复] memory worker自引用反馈循环 — pending队列过滤worker自身会话`
- classification: `merge/mostly-covered`
- current status: The main SQL path now excludes `conversation_type='worker'` by selecting only user conversations, so the old self-reference path is mostly covered. The policy still belongs in the anti-spin candidate for regression tests.
- evidence:
  - `count_pending_realtime` and `get_pending_realtime_messages_with_limit` both use `c.conversation_type = 'user'`.
  - Current memory-review worker reports repeatedly confirm old `mission_memory_pending` prompts leaked into exported user utterances, so importer/exporter filters still need regression coverage.

## 5. `271ed1e6-48ef-42a7-a259-2e1058a07e15`

- title: `Bug: 上下文压缩导致专用工位角色丢失 (role drift)`
- classification: `merge`
- current status: Current runtime has better compaction detection and slot restart behavior, but the broader “role-stable lane” guarantee should remain as a rewrite candidate because provider compaction can still erase role context unless MissionD treats worker lanes as restartable, scoped, artifact-producing executions.
- evidence:
  - `crates/missiond-daemon/src/supervisor.rs` includes compact/stuck reset logic and marks lost tasks failed.
  - `crates/missiond-pty/src/session.rs` supports provider tool policy plumbing.
  - `shared_memory` and `memory-review-batch-runner` now define task-result artifacts and worker-completion-settle.

