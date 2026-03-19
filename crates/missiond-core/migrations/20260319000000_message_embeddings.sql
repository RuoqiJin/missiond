-- Message-level embeddings: independent vector table + skip tracking
-- Design: docs/designs/message-level-embedding.md

-- Independent table: read/write isolation from conversation_messages,
-- avoids Buffer Cache pollution from 1KB halfvec mixed into high-frequency table.
CREATE TABLE IF NOT EXISTS message_embeddings (
    message_id BIGINT PRIMARY KEY REFERENCES conversation_messages(id) ON DELETE CASCADE,
    session_id TEXT NOT NULL,
    embedding_vec halfvec(512) NOT NULL,
    model_version TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Session-internal search: B-Tree on session_id → exact cosine (few thousand msgs = few ms)
CREATE INDEX IF NOT EXISTS idx_me_session ON message_embeddings(session_id);

-- Global HNSW index deferred to backfill completion (Phase 2c).
-- Building HNSW after bulk insert is 3-5x faster than incremental.
-- Will be: CREATE INDEX CONCURRENTLY idx_me_hnsw ON message_embeddings
--          USING hnsw(embedding_vec halfvec_cosine_ops) WITH (m = 24, ef_construction = 128);

-- Skip tracking: distinguish "filtered out" from "not yet processed"
CREATE TABLE IF NOT EXISTS message_embedding_skips (
    message_id BIGINT PRIMARY KEY REFERENCES conversation_messages(id) ON DELETE CASCADE,
    skip_reason TEXT NOT NULL
);
