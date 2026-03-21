-- ============================================================
-- Backfill orphaned subagent parent links (Phase 2 of Cognitive Pipeline S2)
-- Design: docs/designs/conversation-organizer.md
-- Safe to re-run (idempotent: only updates NULL/broken fields)
-- ============================================================

-- Phase 1: Fix parent_session_id for orphaned subagents
-- Path pattern: .../PARENT_SESSION_ID/subagents/agent-xxx.jsonl
-- Extract the path component two levels above the filename.
UPDATE conversations
SET parent_session_id = (regexp_match(jsonl_path, '/([^/]+)/subagents/[^/]+\.jsonl$'))[1]
WHERE conversation_type = 'subagent'
  AND parent_session_id IS NULL
  AND jsonl_path IS NOT NULL
  AND jsonl_path ~ '/[^/]+/subagents/[^/]+\.jsonl$';

-- Phase 2: Fix project field for subagents
-- Subagents whose project is NULL or incorrectly set to a "subagents" path
-- should inherit the parent conversation's project.
UPDATE conversations c
SET project = p.project
FROM conversations p
WHERE c.parent_session_id = p.id
  AND c.parent_session_id IS NOT NULL
  AND p.project IS NOT NULL
  AND (c.project IS NULL OR c.project LIKE '%/subagents' OR c.project LIKE '%/subagents/');
