# MissionD Board Cleanup Batch 07

- generated_at: 2026-05-10
- scope: `project=missiond`, `status in (open, blocked)`, high-priority window offset 30
- mode: read-only investigation; no Board mutation

## Batch conclusion

Four of the five tasks can be closed or merged. One old architecture umbrella should be superseded by the current SSOT + memory review workflow, with only a narrower taxonomy/graph follow-up kept if still valuable.

Recommended actions:

1. Close JarvisChat persistence, task delegate atomic reservation, and KB injection bloat as covered.
2. Merge the memory latch/replay issue into the current memory anti-spin / task-result artifact work.
3. Close or supersede the old “self-evolving knowledge graph” parent; rewrite only the remaining taxonomy work after active-memory cleanup.

## 1. `be0ad9a7-a9a9-46b9-b060-c1a358f5fbfd`

- title: `memory slot 上下文压缩后丢失批次内容...`
- classification: `merge-into-existing-candidate`
- merge targets: `memory-extraction-anti-spin-policy`, `task-result-artifact`
- current status: The historical “pending latch says fetched but batch is lost” problem has been mitigated by watermarks and artifact workflows, but the reliable answer is to require task-result artifacts and an ack/replay boundary for long batch runners.
- evidence:
  - `crates/missiond-daemon/src/engine/learning_engine/extraction.rs` advances watermarks after send-complete, send-error, or timeout so a stale memory lane does not spin forever.
  - `.missiond/workflows/conversation-memory-distillation.lisp` and the current memory review process use batch artifacts rather than relying on a single transient prompt.
  - The running board cleanup has repeatedly surfaced the same lesson: final output must be stored as a durable artifact before BoardTask close.

## 2. `37498eca-63de-4683-88e8-922e8d6c7efe`

- title: `JarvisChat 历史记录不持久化 - 每次打开页面为空`
- classification: `close-covered`
- current status: Covered. Jarvis chat now has durable conversation APIs, sidebar/history loading, and store-backed save/get paths.
- evidence:
  - `packages/board/src/components/JarvisChat.tsx` contains conversation sidebar/history state and loads conversations from API.
  - `packages/board/src/app/api/jarvis/conversations/route.ts` lists conversations through `mission_conversation_list` with source `jarvis_ui`.
  - `crates/missiond-core/src/db/pg/observability.rs` provides `jarvis_get_or_create`, `jarvis_save_exchange`, and `find_latest_jarvis_conversation`.
  - `crates/missiond-core/src/ws/server.rs` calls the Jarvis conversation store path for websocket chat handling.

## 3. `bc4dabec-b896-4120-bc10-679437687930`

- title: `记忆系统架构重构 — 自进化 Jarvis 知识图谱`
- classification: `close-superseded`
- optional rewrite: `memory-taxonomy-v2-after-active-memory-cleanup`
- current status: The original umbrella has been superseded by SSOT-governed memory workflows, review overlay, active/archive policy, conversation ingestion repair, and batch review. Dynamic taxonomy/graph emergence can be revisited later as a smaller post-cleanup design item.
- evidence:
  - `.missiond/workflows/conversation-memory-distillation.lisp` defines manual calibration, review overlay, and no-direct-delete rules.
  - `mission_kb_review` exists as a non-destructive overlay MCP surface.
  - The database now has memory review and graph support such as `knowledge_review_state`, `knowledge_edges`, access scoring, and re-extraction markers.
  - Current user direction is to reduce active memory to a small trusted subset before any graph/taxonomy automation.

## 4. `25e79c0e-8e02-4737-b74f-60aec3e1563f`

- title: `[P6.1] TOCTOU race — task_delegate Slot 原子预留`
- classification: `close-covered`
- current status: Covered by current task delegate slot reservation and shared claim discipline.
- evidence:
  - `crates/missiond-daemon/src/handlers/compute/task_delegate.rs` includes the Phase 6.1 path `find_and_reserve_slot`.
  - That path uses `state.slot_dispatch.try_acquire_guard(&slot.config.id)` before dispatch, making check + reserve atomic at the slot-dispatch layer.
  - The same task delegate path now also uses accepted-shard/write-scope validation and duplicate worker guards.

## 5. `d6c9ce3a-101d-4427-944d-e5ebd77d3f08`

- title: `[P6.3] KB injection bloat — 上下文注入限流`
- classification: `close-covered`
- current status: Covered. The current delegation path does not prefetch noisy KB by default, and the remaining legacy context builder is bounded.
- evidence:
  - `crates/missiond-daemon/src/handlers/compute/task_delegate.rs` defines `MAX_ENTRY_CHARS = 500` and `MAX_CONTEXT_CHARS = 2000` for the legacy context builder.
  - Task delegation metadata now expects explicit `read_scope` / `context_pack_path`; KB/Skill preloading is not the default route.
  - `.missiond/v3/missiond-blueprint.lisp` pins context prefetch as opt-in and says noisy KB/history stores must not be loaded by default.
