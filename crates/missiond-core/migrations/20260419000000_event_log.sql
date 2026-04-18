-- Event Bus v2 · storage layer · event_log
--
-- Frozen lisp .missiond/v2/intent-event-bus.lisp §4.2.b event-log.schema.
-- Appended-in-order, single writer, BIGSERIAL as the global seq authority.
-- Coexists with the legacy `system_timeline` table until Phase 8 cuts it over.

CREATE TABLE IF NOT EXISTS event_log (
    seq             BIGSERIAL PRIMARY KEY,
    domain          TEXT NOT NULL,
    kind            TEXT NOT NULL,
    payload_inline  JSONB,
    payload_ref     TEXT,
    producer_id     TEXT NOT NULL,
    dedupe_key      UUID,
    causation_depth SMALLINT NOT NULL DEFAULT 0,
    trace_id        UUID,
    span_id         UUID,
    parent_span_id  UUID,
    ts              TIMESTAMPTZ NOT NULL DEFAULT now(),
    ephemeral       BOOLEAN NOT NULL DEFAULT false
);

-- Per-domain catch-up scan — subscriber pull path after cursor bootstrap.
CREATE INDEX IF NOT EXISTS idx_event_log_domain_seq ON event_log (domain, seq);

-- Producer retry protection. Partial index so rows without dedupe_key skip it.
CREATE UNIQUE INDEX IF NOT EXISTS uq_event_log_dedupe
    ON event_log (producer_id, dedupe_key)
    WHERE dedupe_key IS NOT NULL;

-- Fast TTL cleanup for ephemeral rows.
CREATE INDEX IF NOT EXISTS idx_event_log_ephemeral_ts
    ON event_log (ts)
    WHERE ephemeral = true;
