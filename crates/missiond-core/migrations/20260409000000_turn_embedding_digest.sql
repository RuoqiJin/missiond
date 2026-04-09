-- Add embedding digest fields to conversation_turns.
-- files_read/files_changed: short file names extracted from tool calls (rule-based).
-- outcome: last assistant non-tool text, used as embedding signal.

ALTER TABLE conversation_turns ADD COLUMN IF NOT EXISTS files_read TEXT;
ALTER TABLE conversation_turns ADD COLUMN IF NOT EXISTS files_changed TEXT;
ALTER TABLE conversation_turns ADD COLUMN IF NOT EXISTS outcome TEXT;
