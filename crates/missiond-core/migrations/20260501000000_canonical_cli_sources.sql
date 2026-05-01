-- Canonicalize CLI conversation sources.
--
-- The initial schema used the historical `claude_cli` default. Runtime code now
-- treats `claude_code`, `gemini_cli`, and `codex_cli` as the only canonical CLI
-- source names; legacy values remain read aliases at ingestion boundaries only.

ALTER TABLE conversations
  ALTER COLUMN source SET DEFAULT 'claude_code';

UPDATE conversations
SET source = 'claude_code'
WHERE source IN ('claude_cli', 'pty_jsonl');

UPDATE conversations
SET source = 'gemini_cli'
WHERE source = 'pty'
  AND conversation_type = 'gemini_chat';
