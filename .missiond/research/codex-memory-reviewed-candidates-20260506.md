# Codex Memory Candidate Review

Generated: 2026-05-06

Source artifacts:
- `.missiond/research/codex-memory-candidates-20260506.md`
- `.missiond/research/codex-memory-gemini-3.1pro-summary-20260506.md`

## Current Counts

- Active KB rows: 1118
- Codex conversations tracked in MissionD: 249
- Codex conversations classified as `codex_chat`: 248
- Codex conversations with messages and turns: 209
- Codex turns rebuilt: 1542
- Codex message rows after duplicate cleanup: 13171
- Codex message rows with `message_uuid IS NULL`: 0

## High-Confidence Memory Candidates

These are stable enough to promote later into project constants, Universe constants, workflow rules, or active KB after the memory policy is ready.

- Auth production canonical domain and issuer: `https://auth.xiaojinpro.com`.
- Auth domain model requirement: multi-tenant, tenant applications/products, product users, and product user groups must stay distinct in SSOT and code.
- MissionD formatting policy: touched-file-only formatting; never run full recursive `cargo fmt` for scoped SSOT/code tasks.
- Gemini lane policy: Gemini is low-authority survey/summarization by default; it must not receive tasks requiring authoritative code edits, Board/KB mutation, or cross-project write access unless a separate scoped write smoke passes.
- ClaudeCode lane policy: ClaudeCode Opus is the main code worker; Sonnet is only for already-atomized narrow patches.
- Provider evidence authority: durable provider logs and MissionD lifecycle events are authoritative; PTY text is diagnostic only.
- Conversation ingestion requirement: Codex/Claude/Gemini logs must preserve provider, raw role, stable message UUID, task attribution, and turn boundaries.
- Swarm dispatch requirement: external-project tasks must carry target `cwd`/read scope/write scope structurally, not only in natural-language objective text.

## Infrastructure Issue Inventory

- `conversations.task_id` attribution is still an important audit surface: task-linked conversation lookup must be reliable for all providers and dynamic slots.
- Codex historical ingestion previously created duplicate null-UUID message rows; the runtime fix is in place, but history cleanup and vacuum were required.
- Context preloading must stay disabled or tightly scoped until KB cleanup is complete, otherwise worker prompts receive noisy stale memory.
- Nightly evolution must default to MissionD V3 SSOT-only review; KB, Board, event bus, and provider logs require explicit `memory-audit` mode.
- Disk pressure can block worker dispatch and builds; MissionD should own disk budget telemetry and cleanup policy for generated worktrees, releases, caches, and provider logs.
- GPG/signing prompts can block automated commits in worker sessions; commit workflow needs a noninteractive signing policy or explicit blocked-state handling.
- Stale slot claims and projection drift should become first-class diagnostics in `mission_slots` and convergence status.

## Do Not Promote

- Per-wave progress narration, transient Shard A/B/C status, and routine Running/Done lifecycle messages.
- Raw logs, repeated summaries, and provider UI text unless used as a fixture or evidence sidecar.
- Already repaired one-off ENOSPC/cache cleanup traces, except as evidence for a general disk-budget rule.
- Facts already represented in project SSOT Lisp; those should be marked `superseded-by-lisp` during KB cleanup rather than duplicated as active memory.

## User Decision Candidates

- Auth KB entry-level cleanup still needs a later user-facing decision surface if deletion is destructive. Safe default: mark `superseded-by-lisp` or archive first, do not hard-delete.
- `jarvis-mechanic` activation remains a design decision: keep registered but inactive until its boundary with MissionD nightly evolution is settled.

## Gemini Output Corrections

- Gemini listed the Auth `.com` vs `.top` domain as unresolved. It is no longer unresolved: `auth.xiaojinpro.com` is production canonical.
- Gemini suggested cross-repo domain replacement after a decision. That should not happen as a broad mechanical replacement. It must be driven by per-project SSOT/checker evidence.
