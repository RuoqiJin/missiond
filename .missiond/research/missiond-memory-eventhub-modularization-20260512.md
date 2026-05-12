# MissionD Memory Provider + EventHub Modularization Decision

Date: 2026-05-12

This document pins the current architecture goal so future Codex/MissionD
context compaction can resume without drifting.

## Decision

MissionD Core should stop being the long-term owner of private memory data and
cross-service durable event storage.

MissionD remains the local orchestrator:

- Board, Decision Inbox, workstation slots, PTY diagnostics, workflow runner.
- Local EventBus for low-latency agent/Board/slot/workflow wakeups.
- Compatibility MCP facades for existing memory and event tools.
- Context-pack construction and scope/policy enforcement.

Memory becomes a pluggable provider contract:

- `null-memory`: open-source/default mode, no private data.
- `local-postgres-memory`: current MissionD database compatibility provider.
- `xjp-memory`: private multi-universe/multi-tenant provider with conversation
  archive, active memory, review overlay, skill evidence, FTS, embedding, rerank,
  export, and purge.

Cross-service durable events move toward `xjp-eventhub`:

- MissionD local EventBus remains local-first and offline-capable.
- Selected local events are written to an outbound spool and relayed to
  `xjp-eventhub` when configured.
- deploy-center/auth/router/timeline events use `missiond.event-envelope.v1`.
- EventHub provides durable streams, cursors, subscriptions, waits, dead-letter,
  and replay for cross-service orchestration.

## Why

- Open-source MissionD must not carry private KB, conversations, skill evidence,
  or business data.
- Multi-tenant and multi-universe use requires strict memory scope isolation.
- EventBus is becoming a cross-service backbone, but local agent control must not
  depend on a cloud service being online.
- MissionD should be small enough to remain a reliable local operator surface.

## SSOT Changes

The V3 blueprint now includes:

- `memory-provider-contract`
- `eventhub-service-contract`
- `memory-access-plane`
- `eventhub-extraction-plane`
- `service-extraction-boundary` implementation surface

The active workflow is:

- `.missiond/workflows/missiond-module-extraction.lisp`

The checker pin is:

- `node scripts/check-v3-service-extraction-isomorphism.mjs`

## Migration Rule

Do not delete current MissionD KB/conversation/EventBus code in the first pass.
Wrap it as compatibility adapters:

- current KB/conversation tables become `local-postgres-memory`
- current local EventBus remains local control bus
- current webhook/eventbridge remains MissionD adapter to future `xjp-eventhub`

Only after parity checks and dual-read/double-write reports pass should code be
physically removed from MissionD Core.

