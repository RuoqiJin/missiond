# Codex history management audit 2026-05-10

## Current data

MissionD database:

- `codex_cli` conversations: 249
- real Codex sqlite-backed conversations: 248
- PTY placeholder conversation: 1 (`pty-slot-codex-master-control`)
- `codex_cli` messages in MissionD: 13,171
- Codex message UUID duplicates: 0
- Codex null `message_uuid`: 0
- imported roles: `assistant=11,837`, `user=1,334`

Codex local sources:

- `~/.codex/state_5.sqlite` threads: 248
- archived sqlite threads: 1
- `~/.codex/sessions/**/*.jsonl`: 308 files
- `~/.codex/archived_sessions/*.jsonl`: 1 file
- session JSONL files not referenced by `state_5.sqlite`: 61
- missing sqlite-referenced rollout files: 0

## Findings

### 1. The current audit is too narrow

`scripts/audit-codex-history-ingestion.mjs` reports OK because it compares only:

`~/.codex/state_5.sqlite threads -> MissionD conversations`.

That path is currently healthy: all 248 sqlite threads are present in MissionD, archive state matches, and message UUIDs are deterministic.

However, the file system has 61 additional Codex rollout JSONL files under `~/.codex/sessions/**` that are not referenced by `state_5.sqlite` and are not imported into MissionD.

Those 61 files are not empty:

- total size: about 65 MB
- total JSONL lines: 17,032
- `event_msg.user_message`: 507
- `event_msg.agent_message`: 1,328
- `response_item.function_call`: 2,736
- `response_item.function_call_output`: 2,736

This means MissionD is currently missing real Codex conversation history when Codex writes a rollout file but does not register it in `state_5.sqlite`.

### 2. `conversation_source_state` is not populated for Codex

`conversation_source_state` has `0` Codex rows.

That table is populated by the ClaudeCode normalization script, but the Codex ingestion worker does not write source-state rows. As a result MissionD cannot answer:

- which Codex raw files are current
- which raw files are missing from DB
- which DB rows point to stale/missing raw files
- which raw files are raw-only or not indexed by sqlite

### 3. Codex conversation `status=active` is not meaningful

MissionD maps Codex `threads.archived = false` to `status=active`.

Current DB state:

- `active`: 248 conversations
- `archived`: 1 conversation

That does not mean 248 Codex sessions are actually running. It only means they are not archived in Codex sqlite. This makes Logs/System views noisy and weakens orchestration logic that treats `active` as live work.

The right split should be closer to:

- source archive state: `active|archived` from Codex metadata
- runtime state: `running|idle|completed|stale|unknown` from durable final event, file mtime, PTY slot, or task binding

### 4. Large Codex rollout files are truncated by the ingestion safety cap

`CodexIngestionWorker` has `MAX_LINES_PER_THREAD = 50_000`.

The current resident Codex rollout is about 3.8 GB:

`~/.codex/sessions/2026/04/25/rollout-2026-04-25T13-21-52-019dc316-5055-7bd3-932e-4d0504d27431.jsonl`

Its persisted Codex line watermark is exactly `50,000`, which means the current worker will not parse beyond the safety cap in one pass. This protects memory, but it silently makes long-running resident Codex history incomplete.

The cap needs to become streaming/paginated ingestion with cursor checkpoints, not a hard stop.

### 5. PTY placeholder is mixed into conversation storage

MissionD has one `codex_cli` conversation row:

`pty-slot-codex-master-control`

It has `message_count=0`, `status=active`, and no raw JSONL path. This is useful as slot runtime state, but it is not a durable conversation. It should be surfaced as slot/PTY diagnostic state, not counted as imported Codex history.

### 6. Codex `history.jsonl` is not managed as a first-class source

`~/.codex/history.jsonl` has about 790 prompt-history rows. This is prompt-only, not full assistant history, but it is still useful for true-user utterance recovery when no full rollout exists.

MissionD currently has strong ClaudeCode `history_jsonl` handling, but no equivalent Codex prompt-history source-state.

## What is working

- SQLite-backed Codex threads are all imported.
- Duplicate Codex message UUID groups are zero.
- Null Codex message UUID rows are zero.
- Raw role is preserved for imported Codex messages.
- Archived sqlite thread state is synchronized.

## Required fixes

1. Extend Codex ingestion to scan `~/.codex/sessions/**/*.jsonl` and `~/.codex/archived_sessions/**/*.jsonl` directly, using `session_meta.payload.id` as the conversation id when sqlite does not reference the file.
2. Write `conversation_source_state` for Codex, with states such as `current`, `raw-only-uningested`, `sqlite-missing`, `missing-stale`, `path-mismatch`, and `pty-placeholder`.
3. Split Codex source archive state from runtime conversation status. Do not use `archived=false` as `status=active`.
4. Replace `MAX_LINES_PER_THREAD` hard stop with paginated streaming ingestion and durable line cursors.
5. Treat `pty-slot-*` rows as runtime diagnostics rather than historical conversations.
6. Add Codex prompt-history source-state for `~/.codex/history.jsonl`, but keep it separate from full conversation rollouts.
7. Upgrade `scripts/audit-codex-history-ingestion.mjs` so it fails when filesystem rollout files are not represented by sqlite or MissionD source-state.

## Suggested owner surfaces

- V3 Lisp: `cli-conversation-ingestion`, `codex-history-source-state`, `resident-master-control`
- Rust: `crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs`
- Scripts/checkers: `scripts/audit-codex-history-ingestion.mjs`, `scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs`
- Frontend: Logs/System should display source-state coverage and distinguish PTY placeholder from durable conversation.
