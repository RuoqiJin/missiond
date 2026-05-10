# MissionD Board Cleanup Batch 14 - 2026-05-10

Scope: read-only review of 5 MissionD-related BoardTasks. Dispatch task: `59ebb5c2-8d19-4a5c-b025-d8a24f395804`. No historical Board task statuses were changed by me. Only this Markdown file under `.missiond/research/board-cleanup/` was written.

All 5 reviewed tasks are currently `status=open`.

## Findings

| Task ID | Title | Classification | Recommendation |
| --- | --- | --- | --- |
| `c1c17796-0ee9-4615-9b97-da548643fad3` | `#15 会话分类和标签体系（需人工投入）` | `close-covered` | Software side fully wired (label MCP + EAV + `message_labels` + `conversation_type`); the human-in-the-loop labelling effort is operational, not a code task. |
| `5c6a29df-7144-4a37-b83c-5f76b42d9570` | `P3: MiniMax 任务拆分 — submit task 队列化批量操作` | `close-stale` | MiniMax is scoped by SSOT to direct background/legacy lanes, while submit-task queueing is now generic; the original "23-step prompt MiniMax can't finish" framing no longer drives the queue. |
| `6def7646-1cc8-4092-9b51-efa0c61761f4` | `router_chat 支持发送大文件/附件给 Gemini` | `close-covered` | `mission_router_chat` already accepts a `files: array` parameter, reads files server-side, and routes binary uploads through the Gemini File API. |
| `9dabe4a3-2e93-4878-8eb6-d2326d0df24f` | `P2: Flow Execute 阶段改用 idle 心跳超时替代硬编码 30min` | `rewrite-candidate` | The newer `mission_execution`/lease substrate has heartbeat semantics, but current Flow v2 `SlotTask` / `ParallelSlotTasks` still dispatch fire-and-forget and do not implement idle/result reflow. Rewrite around that exact residual. |
| `fe506dd5-0f99-40a9-89e0-0fd4d5d69600` | `调研：为 MissionD 引入货真价实的多源搜索能力` | `rewrite-candidate` | Research and user decisions are captured in Board notes, but the task is too broad for implementation as-is and none of the chosen providers (Grok / Tavily / 博查 / Agent Reach) are wired into `crates/`. |

## Evidence

### `c1c17796-0ee9-4615-9b97-da548643fad3` — `close-covered`

The labelling infrastructure the task wants is already in production code; what remains is operator effort.

- MCP entry points are wired:
  - `crates/missiond-daemon/src/handlers/comm/conversation/router.rs:20–21` maps `set_label` / `delete_label` actions to `mission_conversation_set_label` / `mission_conversation_delete_label`.
  - Handler implementations: `crates/missiond-daemon/src/handlers/comm/conversation/query.rs:833 (mission_conversation_set_label)`, `:853 (conversation_label_set)`, `:877 (conversation_label_delete)`.
- DB primitives:
  - `crates/missiond-core/src/db/traits.rs:1169` comment "conversation labels (EAV, same pattern as message_labels)".
  - Trait methods at `:1170 conversation_label_set`, `:1180 conversation_label_delete`, `:1184 conversation_label_get`, `:1188 conversation_label_get_batch`.
  - `set_conversation_type` at `:165` for the coarse `conversation_type` field; `Conversation.conversation_type` itself is in `crates/missiond-core/src/db/shared.rs:55`.
- Label tables already exist:
  - `crates/missiond-core/migrations/20260318000000_init.sql:755–764` creates `message_labels` and its indexes.
  - `crates/missiond-core/migrations/20260322300000_conversation_labels.sql:1–12` creates `conversation_labels` and its indexes.
- The task body reads "需要我投入更多时间去一条一条地看,一条一条地告诉 claudecode 应该如何对会话进行分类和贴标签". This is a manual review/process commitment, not a missing software surface.

Recommendation: close as covered. If the user wants automation that *suggests* labels, that should be a new, scoped task (input: `(session_id, snippets)`; output: candidate labels + confidence; gate: human approval before persistence).

### `5c6a29df-7144-4a37-b83c-5f76b42d9570` — `close-stale`

The 2026-02-27 premise no longer matches the operational model.

- Original premise: "MiniMax M2.5 执行大 prompt（23 步操作）时只完成部分步骤就切走".
- Current MiniMax surface is now scoped:
  - `crates/missiond-daemon/src/context/v3_blueprint_runtime.rs:24 DEFAULT_MINIMAX_MODEL = "MiniMax-M2.5-highspeed"`.
  - Blueprint policy `.missiond/v3/missiond-blueprint.lisp:1217–1226` `(minimax-runtime-policy ...)` self-describes as "Lisp-owned defaults for direct MiniMax HTTP gateway calls used by background briefing, translation, and legacy minimax lanes." — no general 23-step PTY orchestration.
  - `crates/missiond-daemon/src/supervisor.rs:432` mentions MiniMax only in a Thinking-state idle-watch comment.
- Submit-task queue itself exists today and is generic:
  - `.missiond/intent-mcp-defs.lisp:294–320` defines `mission_task_submit`, `mission_task_query`, and `mission_task_cancel`.
  - `crates/missiond-daemon/src/handlers/compute/task.rs:43–80` dispatches those tools; `:83–108` stores a submitted task and attempts immediate dispatch.
  - `crates/missiond-daemon/src/handlers/compute/task.rs:123–187` sends to idle matching slots; `:191–335` auto-spawns eligible slots or leaves the task queued with a "No idle slot found" hint.
  - `crates/missiond-daemon/src/bus/v2_subscribers.rs:215–233` dispatches queued submit tasks on `TaskEvent::Created` / `Completed`.
- Task `dependsOn` is "P1（autopilot 让路）+ P5（submit task 跨 restart 持久化）"; the reusable submit queue exists, so the MiniMax-specific "队列补 MiniMax 能力短板" framing is the stale part.

Recommendation: close as stale. If the underlying need (worker pool that retries multi-step prompts) returns for any other model, write a model-agnostic task instead of resurrecting the MiniMax-specific framing.

### `6def7646-1cc8-4092-9b51-efa0c61761f4` — `close-covered`

All three acceptance criteria from the 2026-02-28 task description are met today.

- MCP tool surface — `crates/missiond-mcp/src/tools/comm/router_chat.rs:23`:
  - `"files": {"type": "array", "description": "本地文件路径列表(≤1MB 文本, ≤10MB 二进制)"}`
  - `"max_tokens": {"type": "integer", "default": 16384}` (line 21) covers "max_tokens 自动适配长回复".
- Daemon-side read + dispatch:
  - `crates/missiond-daemon/src/handlers/comm/router_chat/chat.rs:46–55` detects `has_files` and adapts behaviour.
  - `:210–217` "Process file attachments: read files and append to last user message" — this is exactly the file_path/content_ref behaviour the task proposed.
  - `:213` notes "Binary files (images/video/PDF): uploaded via Gemini File API if API key available."
  - `:244` includes a security denylist that rejects sensitive matches with a structured `<file>` denial line, addressing "100K+ markdown" without leaking secrets.
Recommendation: close as covered. The original task's three "可能方案" (file_path single, file_paths array, content_ref) all converged onto the array form already in use.

### `9dabe4a3-2e93-4878-8eb6-d2326d0df24f` — `rewrite-candidate`

The original concern has been partially superseded by newer execution lease/heartbeat machinery, but the specific Flow v2 surface named by the task is not covered.

- SSOT still describes Flow v2 slot execution as fire-and-forget:
  - `.missiond/intent-pillar-engines.lisp:24–37` defines `SlotTask` with `timeout_secs` default `3600` and `ParallelSlotTasks` with `timeout_secs` default `1800`, marked as POC fire-and-forget.
  - `.missiond/intent-pillar-engines.lisp:70–83` says both `SlotTask` and `ParallelSlotTasks` use `state.pty.send_fire_and_forget`; result reflow is explicitly Phase 2.
- Implementation confirms the timeout is not an idle heartbeat gate:
  - `crates/missiond-daemon/src/engine/flow/handlers.rs:55–65` accepts `timeout_secs` for `ParallelSlotTasks` but assigns it to `_` with a comment that real result reflow is Phase 2.
  - `crates/missiond-daemon/src/engine/flow/handlers.rs:100–140` dispatches `SlotTask` via `send_fire_and_forget` and immediately returns a dispatch receipt string containing the timeout value.
  - `crates/missiond-daemon/src/engine/flow/handlers.rs:145–150` documents `ParallelSlotTasks` as POC fire-and-forget; `:151–263` fan out dispatches and return receipts without idle/result wait.
  - `crates/missiond-daemon/src/engine/flow/runner.rs:74–116` retries failed node-handler results only; it does not observe PTY idle heartbeat.
- Related modern substrate exists, but is not the same surface:
  - `.missiond/v3/missiond-blueprint.lisp:671–679` projects smart watchdog / claim lease policy for autopilot and PTY send.
  - `mission_execution` checker passed and that subsystem exposes heartbeat/lease semantics, but current Flow v2 handlers do not delegate through that result model.

Recommendation: rewrite, not close. The residual task should be precise: "Flow v2 `SlotTask` / `ParallelSlotTasks` should either (a) wire result reflow plus idle-heartbeat timeout, or (b) migrate execution to the `mission_execution` / delegated-task substrate and retire the unused `timeout_secs` semantics."

### `fe506dd5-0f99-40a9-89e0-0fd4d5d69600` — `rewrite-candidate`

The investigation half is done; the implementation half is untouched, and the open task should be split before execution.

- Seven durable Board notes record the deliberation: ChatGPT 5.2 Thinking, Gemini 3.1 Pro, Claude Opus 4.6 Research, and user decisions/feedback. The last user decision (`9d0cd2a1`, 2026-03-03) consolidates the strategy:
  - Layer 1 (AI-level search): **Grok**.
  - Layer 2 (platform-managed sources): **Tavily** + **博查 (Bocha)**.
  - Layer 3 (toolset to evaluate): **Agent Reach** (`https://github.com/Panniantong/agent-reach`).
- Board note evidence:
  - `a9a15147-1cd1-4061-8b58-003b14194e52`: ChatGPT 5.2 Thinking recommends platform-controlled multi-source orchestration and a `search(query, locale, intent, freshness, must_search, providers, filters) -> SearchBundle` style interface.
  - `480e3ca5-5b5d-45c4-b925-72a6ad99b7c8`: user says Grok search has the best current experience and should be the fastest Phase 1.
  - `52f1f91b-b329-43c4-894a-9ce66b3d1bee`: Gemini 3.1 Pro recommends an independently controllable multi-source search component.
  - `0fffb4eb-4ec5-471c-8e51-2f6a6aa0b0ba`: user says Tavily is worth trying and WeChat search can follow after Baidu.
  - `5eb1ddfc-96fd-4ac6-877e-34db2af35658`: Claude Opus 4.6 Research recommends hybrid SearXNG + commercial APIs with Brave/Bocha-style options, RRF, and dedup.
  - `3b8fcd3c-3557-40d2-b6a9-2f3b6629ffbf`: user says Bocha is usable and the WeChat ecosystem can wait for further investigation.
  - `9d0cd2a1-2d14-4f1a-a081-821b78896256`: final strategy: Grok, Tavily + 博查, then Agent Reach evaluation.
- Code state:
  - `mission_board_query search "Grok Tavily 博查 search MissionD"` returned `total=0`, so no implementation child task was found through Board search.
  - `rg -n "mission_search|web_search|tavily|Tavily|bocha|Bocha|博查|grok|Grok|agent-reach|Agent Reach|SearchBundle|SearchHit|SearchProvider|xAI|xai" crates packages scripts docs .missiond` found only AST/code-search usage and Board cleanup/research text, not a provider implementation under `crates/`.
  - Existing `router_chat` has a `search` boolean that delegates to provider-native Google search tooling, but that is not the deterministic multi-source orchestration selected in the Board notes.
- The original task is positioned as 调研（investigation）with explicit deliverables "如需自建：架构设计文档 → docs/designs/; 最终方案选定后建子任务实施". The first deliverable (research) is satisfied by the Board-note thread; the second (design doc) and third (implementation children) are missing.

Recommendation: rewrite this broad research task into concrete children instead of keeping it as-is:
1. Add `docs/designs/multi-source-search.md` distilling the Board-note thread into one design (Grok as Phase 1; Tavily + 博查 + RRF + dedup as Phase 2; Agent Reach as evaluation track).
2. Implement a single MCP search entry point and the Phase 1 Grok adapter.
3. Add provider adapters for Tavily and 博查, with deterministic source bundles, freshness controls, dedup, and citation-preserving results.

## Recommendations

- Close `c1c17796-0ee9-4615-9b97-da548643fad3` as `close-covered`.
- Close `5c6a29df-7144-4a37-b83c-5f76b42d9570` as `close-stale`.
- Close `6def7646-1cc8-4092-9b51-efa0c61761f4` as `close-covered`.
- Rewrite `9dabe4a3-2e93-4878-8eb6-d2326d0df24f` around the remaining Flow v2 idle/result-reflow gap.
- Rewrite `fe506dd5-0f99-40a9-89e0-0fd4d5d69600` into design + Grok Phase 1 + Tavily/Bocha provider implementation children.

## Notes

- Two of the five (`c1c17796`, `6def7646`) are clean closeouts: software shipped, the only remaining work is using it. `5c6a29df` is also closeout-ready, but as stale model-specific debt rather than covered current work.
- `5c6a29df` is the kind of model-specific debt that should auto-tag stale once a memory entry like `multi-model-strategy-...` lands in `MEMORY.md` — the cleanup detector could match these even without re-reviewing.
- `9dabe4a3` should not be closed under the newer `mission_execution` heartbeat evidence alone; Flow v2 still has its own fire-and-forget slot node semantics.
- `fe506dd5`'s Board notes are unusually rich for a Board task and should not be lost when the task is rewritten; the new design doc must reference the original note IDs (`a9a15147`, `480e3ca5`, `52f1f91b`, `0fffb4eb`, `5eb1ddfc`, `3b8fcd3c`, `9d0cd2a1`) so the trail stays auditable.

## Verification

- ✅ Wrote only `.missiond/research/board-cleanup/missiond-board-batch-14-20260510.md` inside the declared `write_scope`.
- ✅ Did not call `mission_board_update` or `mission_board_note_add`; no historical Board task statuses changed.
- ✅ Source directories in `must_not_touch` (`crates/`, `packages/`, `scripts/`) were read only; no source edits, staging, or commit.
- ✅ Passing checkers used as corroboration: `check-v3-cli-conversation-ingestion-isomorphism.mjs`, `check-v3-conversation-ingestion-isomorphism.mjs`, `check-v3-compute-primitives-isomorphism.mjs`, `check-v3-router-policy-isomorphism.mjs`, `check-v3-pillar-flow-schema.mjs`, `check-v3-mission-execution-isomorphism.mjs`, and `check-router-policy.mjs .missiond/router/router-policy-v1.lisp`.
- ✅ Each reviewed task carries one allowed classification and concrete evidence from file paths/functions, checker results, Board details, or Board notes.
