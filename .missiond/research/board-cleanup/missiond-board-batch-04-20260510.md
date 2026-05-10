# MissionD Board Cleanup Batch 04

- generated_at: 2026-05-10
- scope: `project=missiond`, `status in (open, blocked)`, high-priority window offset 15
- mode: read-only investigation; no Board mutation

## 1. `45df56c0-c774-4344-a605-5ba497b2b8e6`

- title: `MissionD 远程调试体验优化（7项）`
- classification: `rewrite-candidate`
- current assessment: The exact HostVDS `cleancc` incident is historical, but the durable issue is still valid as a product gap: remote MissionD instances, sockets, WebSocket ports, and deployed Board frontends need one authority chain instead of ad hoc env variables.
- evidence:
  - `packages/board/src/lib/missiond.ts` still resolves IPC through `MISSION_IPC_SOCKET`.
  - `packages/board/next.config.mjs`, `packages/board/src/eventStream.ts`, and `packages/board/src/components/Terminal.tsx` still default WebSocket port to `9120`.
  - `crates/missiond-mcp/src/bin/mission-mcp.rs` retains legacy `MISSION_IPC_SOCKET` support.
  - HostVDS-specific `cleancc:9121` facts now appear mainly in research/memory-review artifacts, not active deployment authority.
- recommended rewrite:
  - `remote-missiond-instance-registry`: MissionD Universe/deploy-center should own remote instance identity, socket path, WS URL, Board frontend binding, and provenance.
  - old task can close after rewrite candidate is captured.

## 2. `133a89a9-e507-4157-906e-d26d0dcee1ff`

- title: `记忆提取管线 idle backoff：连续空批指数退避`
- classification: `merge-into-memory-extraction-anti-spin-candidate`
- current assessment: Partially superseded by current memory review workflows, but the realtime extraction surface still has only a `pending_served` latch and timeout/watermark handling; no explicit `consecutive_empty_count` exponential backoff was found.
- evidence:
  - `crates/missiond-daemon/src/handlers/knowledge/memory.rs` returns a textual de-bounce warning when `pending_served` is already true; it is not a hard counter/fuse.
  - `crates/missiond-daemon/src/engine/learning_engine/extraction.rs` claims/release extraction probes and filters sessions with no user messages.
  - `crates/missiond-core/src/db/pg/conversation.rs` now limits realtime candidates to `conversation_type='user'`.
  - `.missiond/v3/missiond-blueprint.lisp` has `memory-kb-policy` and `learning-engine-policy`, but no explicit idle-empty exponential backoff invariant.
- recommended rewrite:
  - merge with batch 05 memory tasks as `memory-extraction-anti-spin-policy`: empty-yield backoff, self-session exclusion, hard call fuse, and diagnostic event.

## 3. `83973bab-61bc-4272-bc75-ac8937b852bb`

- title: `[待修复] MCP Board 工具系统性 flailing — create/update 20步未恢复`
- classification: `close-covered/stale`
- current assessment: The note investigation proved the root cause was duplicate MCP registration and transient tool unavailability, not Board handler validation. Current MissionD has MCP reconnect detection and arrow-key reconnect logic.
- evidence:
  - Board note dated 2026-03-14 identifies `No such tool available: mcp__missiond__mission_board_create` after duplicate `mission`/`missiond` MCP registration.
  - `crates/missiond-pty/src/manager.rs` detects `No such tool available` / `tool_use_error`.
  - `crates/missiond-pty/src/session.rs` implements ClaudeCode MCP reconnect ritual and forbids numeric shortcuts.
  - `scripts/check-v3-pty-recognition-isomorphism.mjs` pins `mcp_reconnect_sequence`.
- close reason:
  - Covered by MCP recovery governance. Reopen only if a new duplicate MCP server source appears.

## 4. `3b217aee-0b47-4ac6-bc28-4c466793c9dd`

- title: `Memory工位 compact-resilient 角色锚定：allowed-tools 白名单 + compact后重注入 prompt`
- classification: `merge-into-role-stable-worker-lanes-candidate`
- current assessment: Compact/session lifecycle is now much better covered, but the old allowed-tools design is only partially represented. Current Gemini tool policy exists; ClaudeCode role/prompt reinjection is mostly handled by restart/slot lifecycle and task metadata, not a strict role lane.
- evidence:
  - `crates/missiond-daemon/src/supervisor.rs` marks stuck extraction failed, advances watermarks on kill/timeout, and respawns memory slots.
  - `crates/missiond-pty/src/session.rs` has provider tool policy plumbing, currently strongest for Gemini.
  - `.missiond/v3/missiond-blueprint.lisp` now has workstation/prompt/context-prefetch policies and execution-control-plane.
  - `crates/missiond-daemon/src/engine/shared_memory.rs` has worker-completion-settle/task-result artifacts, which supersede older memory-slot prompt discipline.
- recommended rewrite:
  - `role-stable-worker-lanes`: compact restart, role reminder/restart policy, per-lane tool policy where provider supports it, and output artifact contract.

## 5. `2340f854-0a49-4d8b-b632-2956fe8c9c13`

- title: `⏳ Gemini 3.1-pro-preview 可用性监测中`
- classification: `close-covered/stale`
- current assessment: This was a transient capacity monitor. Current workstation pool and router policy pin `gemini-3.1-pro-preview`, and recent slots run it.
- evidence:
  - `scripts/check-v3-workstation-config-isomorphism.mjs` pins `gemini-3.1-pro-preview`.
  - `scripts/check-v3-router-policy-isomorphism.mjs` pins the same model.
  - `crates/missiond-pty/src/session.rs` has tests/logic for Gemini model profile and tool policy.
  - Current frontend screenshot shows `slot-gemini-ultra` with `gemini-3.1-pro-preview`.
- close reason:
  - Historical availability watch. If provider health is needed, it should be a router/provider health surface, not an open Board task.

