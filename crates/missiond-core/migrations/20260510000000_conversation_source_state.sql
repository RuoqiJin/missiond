-- Non-destructive provider source coverage overlay.
--
-- This table tracks whether a MissionD conversation is backed by a durable
-- provider source file/index, is a raw-only provider transcript, or is only a
-- diagnostic PTY placeholder. It is intentionally separate from conversations:
-- audits may reclassify source evidence without deleting provider rows.

CREATE TABLE IF NOT EXISTS conversation_source_state (
    conversation_id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    raw_path TEXT,
    raw_state TEXT NOT NULL,
    raw_first_seen_at TIMESTAMPTZ,
    raw_last_seen_at TIMESTAMPTZ,
    raw_line_count BIGINT,
    raw_message_line_count BIGINT,
    raw_hash TEXT,
    reason TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_conversation_source_state_source
    ON conversation_source_state(source);

CREATE INDEX IF NOT EXISTS idx_conversation_source_state_state
    ON conversation_source_state(raw_state);

CREATE INDEX IF NOT EXISTS idx_conversation_source_state_raw_path
    ON conversation_source_state(raw_path);
