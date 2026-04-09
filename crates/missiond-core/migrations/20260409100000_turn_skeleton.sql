-- Add skeleton column for on-demand message retrieval.
-- JSON index of tool calls with message IDs, enabling Claude Code
-- to selectively pull specific messages instead of loading entire turns.

ALTER TABLE conversation_turns ADD COLUMN IF NOT EXISTS skeleton TEXT;
