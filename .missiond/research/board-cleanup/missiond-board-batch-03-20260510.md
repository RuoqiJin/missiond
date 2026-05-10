# MissionD Board Cleanup Batch 03

Scope: five remaining `project=missiond` Board tasks, ordered by priority and creation time after the first ten already reviewed. This is an investigation artifact only; it does not mutate Board state.

## Batch Items

### 1. `e6ef4524-21f5-4095-8cf2-70448bd843ac`

Title: `研究：全事件日志中心 & Timeline 升级`

Classification: `close-covered/superseded`

Evidence:
- `crates/missiond-core/migrations/20260419000000_event_log.sql`
- `crates/missiond-core/migrations/20260420200000_drop_system_timeline.sql`
- `crates/missiond-core/src/db/pg/timeline.rs`
- `crates/missiond-core/src/event/log/`
- `crates/missiond-core/src/event/pipeline/`
- `crates/missiond-daemon/src/events_sync.rs`
- `crates/missiond-daemon/src/handlers/comm/timeline.rs`
- `crates/missiond-mcp/src/tools/comm/timeline.rs`
- `packages/board/src/components/timeline/`
- `.missiond/frontend/board-blueprint.lisp`

Rationale: the old research target has been superseded by the current EventBus/EventBridge/event_log timeline architecture. The legacy `system_timeline` path has already been dropped and Timeline now projects from event log surfaces.

### 2. `c9859af3-e851-40bc-a9aa-c87506769683`

Title: `Timeline 选择系统统一重构：Scope + Focus 架构`

Classification: `close-covered`

Evidence:
- `packages/board/src/components/timeline/types.ts` defines `SelectionScope`, `SelectionState`, `focusedSeq`, `contextSeqs`.
- `packages/board/src/components/timeline/stores/timelineStore.ts` owns selection state.
- `packages/board/src/components/timeline/CognitiveTimeline.tsx` wires session/slot/trace/global selection and `focusedSeq`.
- `packages/board/src/components/timeline/UnifiedDetailPanel.tsx` renders the unified detail surface.
- `packages/board/src/components/timeline/helpers.ts` implements `getDotStatus` / capsule selection state.

Rationale: the code now directly implements the requested `scope + focus` model.

### 3. `6ad3b851-a55a-4d7b-a514-cbf927acd3bc`

Title: `统一 CLI 抽象层：Claude/Gemini/Codex 引擎识别与管控`

Classification: `close-covered`

Evidence:
- `crates/missiond-core/src/types/gen_types.rs` and `crates/missiond-core/src/types/slot.rs` define `CliEngine`.
- `crates/missiond-pty/src/pty_recognition.rs` has provider-aware recognition for ClaudeCode, Gemini, and Codex.
- `crates/missiond-pty/src/session.rs` routes provider-specific parsers and confirmation handling.
- `crates/missiond-daemon/src/slot_orchestrator/` owns provider slot spawn/control paths.
- `scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs` pins canonical `claude_code`, `gemini_cli`, and `codex_cli` ingestion.

Rationale: the design has moved beyond the original CLI abstraction task into provider-aware PTY/log/slot/runtime management.

### 4. `a37f5ddc-2fe2-4453-a775-059acfda0eb7`

Title: `系统化 MissionD 知识库 — 四层知识网络 + 自动收敛`

Classification: `close-superseded-by-current-KB-governance`

Evidence:
- `docs/designs/kb-layered-knowledge-network.md` records the original design and phase completion.
- `crates/missiond-core/migrations/20260508001000_knowledge_review_state.sql`
- `.missiond/workflows/conversation-memory-distillation.lisp`
- `.missiond/workflows/memory-review-batch-runner.lisp`
- `scripts/backfill-memory-review-results.mjs`
- `scripts/run-memory-review-supervisor.mjs`

Rationale: the original four-layer KB design is historical. Current direction is SSOT-first active-memory governance with review overlay, explicit archival states, and batch review workflow.

### 5. `82578ecc-b988-4c4c-bf79-e4a2d4c28813`

Title: `[P2] feat: Submit 任务完成通知提交者验收`

Classification: `rewrite-candidate`

Evidence:
- Legacy hook design exists in `docs/designs/task-completion-notification.md`.
- `mission_task_ack` still exists in `crates/missiond-daemon/src/handlers/compute/task.rs`.
- `mission_inbox` exists in `crates/missiond-daemon/src/handlers/compute/slot.rs` and MCP compute tools.
- New execution-control-plane artifacts exist in `crates/missiond-core/migrations/20260509000000_execution_control_plane.sql`.
- Current V3 surfaces include `task-result-artifact` and `worker-completion-settle`.

Rationale: the need is still valid, but the old `UserPromptSubmit -> mission_task_ack` design should not be the primary architecture. The current architecture should project `task-result-artifact -> inbox/Board/Jarvis notification -> submitter acknowledgement`.

Suggested rewrite candidate:
- `task completion notification v2: task-result artifact + inbox projection + submitter acknowledgement`

