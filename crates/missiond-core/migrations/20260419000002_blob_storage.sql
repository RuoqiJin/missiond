-- Event Bus v2 · claim-check · blob_storage + dead_letter_queue
--
-- Frozen lisp §4.2.b claim-check.backends.blob-table and §4.3 FailurePolicy
-- SkipToDLQ. Both tables are Phase-2 infrastructure; they are referenced by
-- LogWriter / BlobStore (blob_storage) and the Phase-4 subscription runtime
-- (dead_letter_queue).

CREATE TABLE IF NOT EXISTS blob_storage (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    data        BYTEA NOT NULL,
    size        INTEGER NOT NULL,
    checksum    TEXT NOT NULL,  -- sha256 hex
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    ttl_expires TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_blob_ttl
    ON blob_storage (ttl_expires)
    WHERE ttl_expires IS NOT NULL;

CREATE TABLE IF NOT EXISTS dead_letter_queue (
    id                 BIGSERIAL PRIMARY KEY,
    subscription_name  TEXT NOT NULL,
    event_seq          BIGINT NOT NULL,
    failure_reason     TEXT NOT NULL,
    payload_snapshot   JSONB,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_dlq_subscription
    ON dead_letter_queue (subscription_name, created_at DESC);
