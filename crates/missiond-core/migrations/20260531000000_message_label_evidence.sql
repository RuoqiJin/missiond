-- Message label evidence ledger.
--
-- `message_labels` remains the compatibility projection consumed by existing
-- read paths. This table stores deterministic rule evidence with provenance so
-- multiple labelers/rules do not overwrite each other.

CREATE TABLE IF NOT EXISTS message_label_evidence (
    message_id BIGINT NOT NULL REFERENCES conversation_messages(id) ON DELETE CASCADE,
    label TEXT NOT NULL,
    value TEXT NOT NULL,
    source TEXT NOT NULL,
    rule_id TEXT NOT NULL,
    rule_version TEXT NOT NULL,
    confidence DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    priority INTEGER NOT NULL DEFAULT 100,
    reason TEXT,
    evidence JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TEXT NOT NULL DEFAULT (NOW() AT TIME ZONE 'UTC')::TEXT,
    updated_at TEXT NOT NULL DEFAULT (NOW() AT TIME ZONE 'UTC')::TEXT,
    PRIMARY KEY (message_id, label, value, source, rule_id, rule_version),
    CHECK (confidence >= 0.0 AND confidence <= 1.0)
);

CREATE INDEX IF NOT EXISTS idx_message_label_evidence_message
    ON message_label_evidence(message_id);

CREATE INDEX IF NOT EXISTS idx_message_label_evidence_label_value
    ON message_label_evidence(label, value);

CREATE INDEX IF NOT EXISTS idx_message_label_evidence_source_rule
    ON message_label_evidence(source, rule_id, rule_version);
