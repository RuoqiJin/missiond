-- Event Bus v2 · SSOT cutover · event_log FTS index
--
-- v1.3.0 §4.6 event_log read-ui-projection — mission_timeline 搜索经 event_log.payload_inline
-- Frozen lisp declares event_log as timeline SSOT.

CREATE INDEX IF NOT EXISTS idx_event_log_payload_fts
    ON event_log
    USING GIN (to_tsvector('simple', coalesce(payload_inline::text, '')));
