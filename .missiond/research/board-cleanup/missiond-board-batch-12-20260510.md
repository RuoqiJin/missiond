# MissionD Board Cleanup Batch 12 - 2026-05-10

Scope: read-only review of 5 MissionD BoardTasks for dispatch `f31e061b-a648-4406-92e2-78e965998d8e`. I queried the target BoardTasks, their parents, current SSOT Lisp, current code evidence, selected runtime state, and isomorphism checkers. No Board statuses were changed; this report under `.missiond/research/board-cleanup/` is the only intended write.

All 5 reviewed tasks are currently `status=open`.

## Findings

| Task ID | Title | Classification | Recommendation |
| --- | --- | --- | --- |
| `0224127e-9940-47cf-a1a1-f1672175175a` | `#3 建立一个本代码库文档的 lisp 版` | `close-superseded` | Close as standalone; parent `2629bde5` explicitly merged #3 into the holographic code context engine and the Lisp/AST documentation surfaces now exist. |
| `c507283f-baf2-490d-a405-a2c59753f338` | `Jarvis Session Sticky + PTY 输出结构化` | `rewrite-candidate` | Rewrite into smaller residual tasks. PTY cleanup/structured SSE are partly covered, but Header/JWT session sticky and content-level micro-semantic tags are not covered as written. |
| `1e63d830-0c27-4bf9-956e-862828926e6e` | `#4 让 AI 教会我看 lisp 文档 + Opus 维护工位` | `rewrite-candidate` | Split the residual user-facing tutorial from the already-covered maintenance pipeline; do not keep the broad "Opus 工位" framing as-is. |
| `e8d85b2e-1e32-4270-b856-3bc0bd995d93` | `#7 全工位会话记录和事件 + GPT/Gemini 对话` | `close-covered` | Capture-side requirements are covered by event_log, conversation ingestion, Codex/Gemini reconcilers, and live conversation/timeline data. |
| `2eda2ce3-81fc-442a-8d1f-c655300e1434` | `Specify PTY Confirming resolver policy and send/confirm boundary` | `rewrite-candidate` | Rewrite from "proposal before implementation" into residual canonization/checker work: document the now-implemented boundary, decide the remaining `mission_pty_send` error/redirect behavior, and pin tests. |

## Evidence

### `0224127e-9940-47cf-a1a1-f1672175175a` - `close-superseded`

- Board parent `2629bde5-d535-4948-8faa-aed07d5b4ed3` says original research tasks `#1 #3 #4` were merged into the "全息代码上下文引擎", implemented through Language-native Stub plus architecture manual generation.
- `.missiond/intent.lisp:1-10` is a generated implementation-detail Lisp document; `.missiond/intent.lisp:66-107` defines the layered code documentation shape.
- `.missiond/intent-db-misc.lisp:20-43` documents `ast_nodes`, `stub_content`, calls, embeddings, and AST sync/search functions.
- `crates/missiond-core/src/ast/mod.rs:1-42` implements the code context engine: tree-sitter AST to language-native stubs, preserving signatures/docstrings/calls and eliding bodies.
- `crates/missiond-daemon/src/workers/local/ast_sync_worker.rs:1-8` names this as the incremental code indexing pipeline and part of P2 Holographic Context Engine; `:165-216` handles commit sync and topology updates.
- `crates/missiond-daemon/src/context/topology_map.rs:1-24` builds dynamic topology summaries after AST sync and stores module summaries under KB category `architecture:module` at `:68-86`.

Recommendation: close as superseded. The old task is too generic and its stated outcome now exists through a richer SSOT/code-context mechanism. Any new docs request should specify audience and missing surface.

### `c507283f-baf2-490d-a405-a2c59753f338` - `rewrite-candidate`

The task mixes three separate 2026-02-26 review suggestions.

- Partial coverage: `crates/missiond-core/src/ws/server.rs:117-195` has `clean_jarvis_response`, stripping status/TUI markers and tool-result references. The same server emits structured SSE metadata/status/tool events around Jarvis work (`:1255-1284`, `:1348-1408`, `:1510-1585`, `:1926-2002`).
- Partial coverage: `crates/missiond-core/src/ws/jarvis_trace.rs:1-47` records structured Jarvis traces with slot id, state transitions, and router trace id. `crates/missiond-core/src/db/pg/observability.rs:1058-1100` creates/reuses Jarvis conversations and saves exchanges.
- Missing as written: `crates/missiond-core/src/ws/server.rs:1147-1158` chooses `X-Slot-Id` or default `slot-jarvis`; I found no Header/JWT-derived SessionID-to-same-PTY routing contract.
- Missing as written: current structured state is emitted as SSE events, not in-content micro-semantic tags such as `thinking/searching/coding` embedded in the SSE body.
- Runtime spot check: `mission_pty_status {"slotId":"slot-jarvis"}` returned `null`, and `mission_conversation_query` for `source=jarvis_ui` returned `[]`, so there is no current live Jarvis session proving sticky behavior.

Recommendation: rewrite into separate residual tasks: (1) Jarvis sticky routing contract (`conversation_id`/JWT/header to PTY slot, TTL, busy behavior), (2) sanitizer/structured output spec including ANSI/tool-call expectations, (3) decide whether micro-semantics should remain SSE events or be added to content.

### `1e63d830-0c27-4bf9-956e-862828926e6e` - `rewrite-candidate`

- Board parent `2629bde5-d535-4948-8faa-aed07d5b4ed3` says `#4` was merged with the same Lisp/manual research line as `#3`.
- Maintenance mechanisms exist:
  - `crates/missiond-daemon/src/engine/lisp_code_sync.rs:212-320` starts the Lisp-code sync service, subscribes to config changes, and sets up the watcher when enabled.
  - `crates/missiond-daemon/src/engine/lisp_code_sync.rs:916-930` writes Lisp-code-sync reports.
  - `crates/missiond-daemon/src/engine/nightly_evolution.rs:100-115` defines the scheduled evolution service, and `:340-376` includes Lisp density / checker drift / logic consistency findings.
- The "teach me to read Lisp docs" part is not covered by those maintenance services. I did not find a dedicated operator tutorial artifact in the checked docs/SSOT evidence.
- The "Opus maintenance workstation" portion is no longer a clean acceptance target because current maintenance is service/checker driven, not a dedicated manual workstation contract in this task.

Recommendation: rewrite. Keep one residual task for an operator-facing Lisp reading guide if still desired, and treat maintenance as covered by existing `lisp_code_sync`, `nightly_evolution`, AST sync, and checker/report surfaces.

### `e8d85b2e-1e32-4270-b856-3bc0bd995d93` - `close-covered`

- SSOT: `.missiond/v3/missiond-blueprint.lisp:1717-1803` defines conversation ingestion policy and canonical CLI ingestion paths for ClaudeCode, Gemini CLI, and Codex CLI.
- Timeline/event ingestion:
  - `crates/missiond-daemon/src/infra/message_handler.rs:84-170` ingests conversation messages with source, slot, jsonl path, and task id.
  - `crates/missiond-daemon/src/infra/message_handler.rs:471-594` emits message/tool events to the timeline/event log.
  - `crates/missiond-daemon/src/events_sync.rs:1-17` and `:683-706` sync raw JSONL into conversation messages/events.
- Gemini/Codex capture:
  - `crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs:1-10` scans Gemini CLI chat files and normalizes messages/tool content at `:240-360`.
  - `crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs:1-16` ingests Codex SQLite/session JSONL and writes conversations/tool calls; `:497-610` processes sessions and inserts messages/tool calls.
- UI/API surfaces exist: `crates/missiond-daemon/src/handlers/comm/question/llm_trace.rs:9-33` routes Gemini/Jarvis trace calls; `packages/board/src/app/api/system/llm-traces/route.ts:4-23` and `packages/board/src/app/api/system/gemini-content/route.ts:4-12` expose board API routes.
- Runtime spot checks:
  - `mission_timeline stats` returned `total_events=253946`, including `message::logged`, `system::tool_completed`, `llm::request_started`, and `llm::request_completed`.
  - `mission_conversation_query` returned live `gemini_cli` conversations with slot `slot-gemini-ultra` and JSONL paths under `.gemini/tmp/...`.
  - `mission_conversation_query` returned live `codex_cli` conversations, including `~/.codex/sessions/...` JSONL paths and active Codex chat rows.

Recommendation: close as covered. Any remaining UI polish for showing full prompt/fetched batch on one timeline row belongs to existing task `1526d81c-fd79-451d-a2cf-60a853b08970` from Batch 10, not this broad capture-side task.

### `2eda2ce3-81fc-442a-8d1f-c655300e1434` - `rewrite-candidate`

- SSOT contract exists:
  - `.missiond/v3/missiond-blueprint.lisp:1840-1844` says explicit `Confirming` preserves blocked state, Codex approval menus are explicit blocked signatures, `mission_pty_confirm` must use keyboard navigation rather than numeric shortcuts, and bare approval words must not trigger blocked.
  - `.missiond/intent-mcp-defs.lisp:377-408` defines `mission_pty_send` and `mission_pty_confirm`; this generated doc is slightly older/looser for `confirm` than the current MCP schema.
- MCP schema and runtime boundary exist:
  - `crates/missiond-mcp/src/tools/compute/pty.rs:25-38` defines `mission_pty_send`.
  - `crates/missiond-mcp/src/tools/compute/pty.rs:67-81` defines `mission_pty_confirm` as semantic choice and says raw text belongs in `mission_pty_send`.
  - `crates/missiond-daemon/src/handlers/compute/pty.rs:224-268` dispatches `mission_pty_send`; both blocking and fire-and-forget delegate to PTY session send paths.
  - `crates/missiond-daemon/src/handlers/compute/pty.rs:369-415` dispatches `mission_pty_confirm`; `:577-610` rejects arbitrary raw text and maps only semantic confirmations.
- PTY state behavior is implemented:
  - `crates/missiond-pty/src/session.rs:1614-1619` rejects fire-and-forget sends unless the session is `Idle`.
  - `crates/missiond-pty/src/session.rs:1677-1681` rejects blocking sends unless the session is `Idle`.
  - `crates/missiond-pty/src/session.rs:1845-1858` confirms only when the state is `Confirming` or the screen is a blocked confirmation.
  - `crates/missiond-pty/src/manager.rs:691-720` forwards fire-and-forget sends through the checked session path, not raw `write`.
- Residual gap: the task asks for a policy/proposal artifact with non-goals, user-decision points, and concrete implementation/test tasks. I found the code boundary and SSOT invariants, but not a single proposal artifact canonizing the policy. Also, the send path currently returns generic `Cannot send message in state: Confirming` rather than a policy-specific redirect/error such as "use `mission_pty_confirm`".
- Runtime spot check: `mission_pty_status {"slotId":"slot-codex-master-control"}` showed a running/tool state, not a live `Confirming` case, so there is no current incident proving the resolver behavior end-to-end.

Recommendation: rewrite. The replacement should be a narrow closeout task: canonize current Confirming policy in SSOT/design doc, decide whether `mission_pty_send` should produce a structured Confirming redirect error, and add checker/test coverage around send/confirm boundary drift.

## Recommendations

- Close `0224127e-9940-47cf-a1a1-f1672175175a` as `close-superseded`.
- Close `e8d85b2e-1e32-4270-b856-3bc0bd995d93` as `close-covered`.
- Replace `c507283f-baf2-490d-a405-a2c59753f338` with smaller Jarvis sticky / sanitizer / SSE semantics tasks if those are still desired.
- Replace `1e63d830-0c27-4bf9-956e-862828926e6e` with an operator-facing Lisp reading guide task only; maintenance is already handled elsewhere.
- Replace `2eda2ce3-81fc-442a-8d1f-c655300e1434` with a policy-canonization and checker task anchored to the current implementation.

## Verification

- Read-only Board access used: `mission_board_query get` for dispatch `f31e061b-a648-4406-92e2-78e965998d8e`, the five target IDs, and relevant parents. I did not call `mission_board_update` or `mission_board_note_add`.
- Checker results:
  - `node scripts/check-v3-pty-recognition-isomorphism.mjs` -> OK
  - `node scripts/check-v3-compute-primitives-isomorphism.mjs` -> OK
  - `node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs` -> OK
  - `node scripts/check-v3-conversation-ingestion-isomorphism.mjs` -> OK
  - `node scripts/check-architecture-lisp.mjs --no-structure .missiond/intent.lisp .missiond/intent-db-misc.lisp .missiond/v3/missiond-blueprint.lisp` -> OK
- Files written: only `.missiond/research/board-cleanup/missiond-board-batch-12-20260510.md`.
- No source files under `crates/`, `packages/`, or `scripts/` were edited. No staging or commit was performed.
