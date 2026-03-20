-- Gemini CLI session file watermarks for incremental ingestion.
-- Uses message count cursor (not byte offset) because Gemini CLI
-- rewrites the entire JSON file on each new message.
CREATE TABLE IF NOT EXISTS gemini_cli_watermarks (
    session_file TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    last_msg_count BIGINT NOT NULL DEFAULT 0,
    last_reconciled_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_gemini_watermarks_session
    ON gemini_cli_watermarks (session_id);
