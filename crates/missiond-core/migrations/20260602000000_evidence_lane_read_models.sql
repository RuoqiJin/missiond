-- Typed evidence lane read models.
--
-- These tables are additive projections over existing raw sources. They do not
-- delete, rewrite, or promote raw conversations, skill files, or KB rows.

CREATE TABLE IF NOT EXISTS evidence_items (
    id TEXT PRIMARY KEY,
    lane_id TEXT NOT NULL CHECK (
        lane_id IN (
            'runtime_truth',
            'project_ssot',
            'reviewed_kb',
            'active_board',
            'skill_evidence',
            'conversation_audit',
            'cold_archive',
            'support_refs'
        )
    ),
    source_type TEXT NOT NULL,
    source_id TEXT,
    source_ref TEXT,
    project_id TEXT,
    task_id TEXT,
    title TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL DEFAULT '',
    authority_class TEXT NOT NULL DEFAULT 'evidence-only',
    validity TEXT NOT NULL DEFAULT 'evidence_only',
    privacy_class TEXT NOT NULL DEFAULT 'internal',
    freshness TEXT NOT NULL DEFAULT 'unknown',
    score DOUBLE PRECISION,
    raw_policy TEXT NOT NULL DEFAULT 'summary-only',
    evidence_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    fts_doc TSVECTOR GENERATED ALWAYS AS (
        to_tsvector(
            'simple',
            coalesce(title, '') || ' ' ||
            coalesce(summary, '') || ' ' ||
            coalesce(source_type, '') || ' ' ||
            coalesce(source_ref, '') || ' ' ||
            coalesce(project_id, '') || ' ' ||
            coalesce(task_id, '')
        )
    ) STORED
);

CREATE INDEX IF NOT EXISTS idx_evidence_items_lane_project
    ON evidence_items(lane_id, project_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_evidence_items_source
    ON evidence_items(source_type, source_id);
CREATE INDEX IF NOT EXISTS idx_evidence_items_task
    ON evidence_items(task_id, updated_at DESC)
    WHERE task_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_evidence_items_fts
    ON evidence_items USING GIN(fts_doc);

CREATE TABLE IF NOT EXISTS context_gather_runs (
    id TEXT PRIMARY KEY,
    query TEXT NOT NULL DEFAULT '',
    project_id TEXT,
    task_id TEXT,
    source_profile TEXT NOT NULL,
    lane_counts JSONB NOT NULL DEFAULT '{}'::jsonb,
    metrics JSONB NOT NULL DEFAULT '{}'::jsonb,
    raw_sources_included BOOLEAN NOT NULL DEFAULT FALSE,
    credential_opt_in BOOLEAN NOT NULL DEFAULT FALSE,
    conversation_opt_in BOOLEAN NOT NULL DEFAULT FALSE,
    resolver_source TEXT,
    runtime_root_consistent BOOLEAN,
    artifact_hash TEXT,
    diagnostics JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_context_gather_runs_project_created
    ON context_gather_runs(project_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_context_gather_runs_task_created
    ON context_gather_runs(task_id, created_at DESC)
    WHERE task_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_context_gather_runs_profile_created
    ON context_gather_runs(source_profile, created_at DESC);

CREATE TABLE IF NOT EXISTS conversation_episodes (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    project_id TEXT,
    task_id TEXT,
    incident_id TEXT,
    conversation_type TEXT NOT NULL,
    time_range_start TIMESTAMPTZ,
    time_range_end TIMESTAMPTZ,
    topic TEXT NOT NULL DEFAULT '',
    outcome TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL DEFAULT '',
    duplicate_group_id TEXT,
    staleness TEXT NOT NULL DEFAULT 'unknown',
    review_state TEXT NOT NULL DEFAULT 'needs_review',
    derived_from_conversation BOOLEAN NOT NULL DEFAULT TRUE,
    evidence_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_conversation_episodes_project
    ON conversation_episodes(project_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_conversation_episodes_conversation
    ON conversation_episodes(conversation_id);

CREATE TABLE IF NOT EXISTS conversation_fact_extracts (
    id TEXT PRIMARY KEY,
    episode_id TEXT REFERENCES conversation_episodes(id) ON DELETE SET NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    project_id TEXT,
    fact_key TEXT NOT NULL,
    fact_summary TEXT NOT NULL,
    validity TEXT NOT NULL DEFAULT 'needs_review',
    staleness TEXT NOT NULL DEFAULT 'unknown',
    confidence DOUBLE PRECISION NOT NULL DEFAULT 0.5 CHECK (confidence >= 0.0 AND confidence <= 1.0),
    derived_from_conversation BOOLEAN NOT NULL DEFAULT TRUE,
    source_message_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(conversation_id, fact_key)
);

CREATE INDEX IF NOT EXISTS idx_conversation_fact_extracts_project
    ON conversation_fact_extracts(project_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_conversation_fact_extracts_validity
    ON conversation_fact_extracts(validity, staleness);

CREATE TABLE IF NOT EXISTS conversation_duplicate_groups (
    id TEXT PRIMARY KEY,
    project_id TEXT,
    task_id TEXT,
    canonical_episode_id TEXT REFERENCES conversation_episodes(id) ON DELETE SET NULL,
    duplicate_episode_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    rationale TEXT NOT NULL DEFAULT '',
    confidence DOUBLE PRECISION NOT NULL DEFAULT 0.5 CHECK (confidence >= 0.0 AND confidence <= 1.0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_conversation_duplicate_groups_project
    ON conversation_duplicate_groups(project_id, updated_at DESC);

CREATE TABLE IF NOT EXISTS skill_evidence_items (
    id TEXT PRIMARY KEY,
    skill TEXT NOT NULL,
    item_type TEXT NOT NULL CHECK (
        item_type IN (
            'metadata',
            'procedure',
            'operational_fact',
            'warning',
            'credential_ref'
        )
    ),
    project_id TEXT,
    service_id TEXT,
    domain TEXT,
    source_path TEXT NOT NULL,
    source_line INTEGER,
    title TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL DEFAULT '',
    validity TEXT NOT NULL DEFAULT 'evidence_only',
    confidence DOUBLE PRECISION NOT NULL DEFAULT 0.5 CHECK (confidence >= 0.0 AND confidence <= 1.0),
    secret_ref TEXT,
    credential_inline_risk BOOLEAN NOT NULL DEFAULT FALSE,
    evidence_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(skill, item_type, source_path, source_line, title)
);

CREATE INDEX IF NOT EXISTS idx_skill_evidence_items_skill
    ON skill_evidence_items(skill, item_type, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_skill_evidence_items_project
    ON skill_evidence_items(project_id, updated_at DESC)
    WHERE project_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_skill_evidence_items_validity
    ON skill_evidence_items(validity, confidence DESC);
