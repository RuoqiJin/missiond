-- Non-destructive KB review overlay.
--
-- The knowledge table remains the immutable source record. Review state only
-- controls retrieval priority and human/operator triage decisions.

CREATE TABLE IF NOT EXISTS knowledge_review_state (
    id TEXT PRIMARY KEY,
    knowledge_id TEXT NOT NULL REFERENCES knowledge(id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (
        state IN (
            'active',
            'superseded-by-lisp',
            'superseded-by-code',
            'historical-evidence',
            'duplicate',
            'wrong-or-stale',
            'delete-candidate',
            'needs-human'
        )
    ),
    batch_id TEXT NOT NULL,
    reviewer TEXT NOT NULL,
    rationale TEXT NOT NULL,
    evidence_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    superseded_by TEXT,
    confidence DOUBLE PRECISION NOT NULL DEFAULT 0.8 CHECK (confidence >= 0.0 AND confidence <= 1.0),
    reviewed_at TEXT NOT NULL,
    applied_at TEXT,
    is_current BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_knowledge_review_state_current
    ON knowledge_review_state (knowledge_id)
    WHERE is_current;

CREATE INDEX IF NOT EXISTS idx_knowledge_review_state_state
    ON knowledge_review_state (state)
    WHERE is_current;

CREATE INDEX IF NOT EXISTS idx_knowledge_review_state_batch
    ON knowledge_review_state (batch_id);
