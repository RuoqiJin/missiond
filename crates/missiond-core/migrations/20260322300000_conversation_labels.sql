-- Conversation-level EAV labels (same pattern as message_labels)
-- Used for starring, categorization, and future extensible tagging.
CREATE TABLE IF NOT EXISTS conversation_labels (
    session_id TEXT NOT NULL,
    label TEXT NOT NULL,
    value TEXT,
    source TEXT NOT NULL DEFAULT 'user',
    created_at TEXT NOT NULL DEFAULT to_char(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
    PRIMARY KEY (session_id, label)
);
CREATE INDEX IF NOT EXISTS idx_conv_label ON conversation_labels(label, value);
CREATE INDEX IF NOT EXISTS idx_conv_label_sid ON conversation_labels(session_id);
