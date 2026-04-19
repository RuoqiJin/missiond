-- Event Bus v2 · egress · event_subscriptions (cursor store)
--
-- Frozen lisp §4.3 cursor-store.schema. Schema is built in Phase 2 so that
-- Phase 4 (subscription API) can flush cursors immediately; no runtime code
-- reads/writes this table yet.
--
-- `subscription_name` is the PK — one consumer can hold multiple logical
-- subscriptions. `consumer_name` is informational for ops.

CREATE TABLE IF NOT EXISTS event_subscriptions (
    subscription_name  TEXT PRIMARY KEY,
    consumer_name      TEXT NOT NULL,
    domain             TEXT NOT NULL,
    last_acked_seq     BIGINT NOT NULL DEFAULT 0,
    last_seen_at       TIMESTAMPTZ,
    failure_policy     JSONB,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_event_subscriptions_domain
    ON event_subscriptions (domain);
