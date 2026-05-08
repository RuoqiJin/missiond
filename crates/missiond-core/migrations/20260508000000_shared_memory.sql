-- Durable coordination substrate for MissionD multi-agent execution.
--
-- EventBus remains the wakeup/observation mechanism. These tables are the
-- durable shared memory layer used by workers to exchange artifacts, claim
-- write scopes, and resume cursors without racing on Lisp ledger files.

CREATE TABLE IF NOT EXISTS shared_events (
  id TEXT PRIMARY KEY,
  stream_id TEXT NOT NULL,
  seq BIGSERIAL NOT NULL UNIQUE,
  project_id TEXT,
  task_id TEXT,
  agent_id TEXT,
  event_kind TEXT NOT NULL,
  payload JSONB NOT NULL DEFAULT '{}'::jsonb,
  idempotency_key TEXT,
  correlation_id TEXT,
  parent_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_shared_events_idempotency
  ON shared_events(stream_id, idempotency_key)
  WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_shared_events_stream_seq
  ON shared_events(stream_id, seq);

CREATE INDEX IF NOT EXISTS idx_shared_events_project_task
  ON shared_events(project_id, task_id);

CREATE TABLE IF NOT EXISTS shared_artifacts (
  hash TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  project_id TEXT,
  task_id TEXT,
  media_type TEXT NOT NULL DEFAULT 'application/json',
  bytes BYTEA NOT NULL,
  size_bytes BIGINT NOT NULL,
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_shared_artifacts_project_task
  ON shared_artifacts(project_id, task_id);

CREATE TABLE IF NOT EXISTS shared_claims (
  id TEXT PRIMARY KEY,
  project_id TEXT,
  task_id TEXT,
  owner_id TEXT NOT NULL,
  scope_kind TEXT NOT NULL,
  scope_key TEXT NOT NULL,
  status TEXT NOT NULL,
  acquired_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  lease_expires_at TIMESTAMPTZ NOT NULL,
  released_at TIMESTAMPTZ,
  heartbeat_at TIMESTAMPTZ,
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_shared_claims_active_scope
  ON shared_claims(scope_kind, scope_key, lease_expires_at)
  WHERE status = 'active';

CREATE INDEX IF NOT EXISTS idx_shared_claims_project_task
  ON shared_claims(project_id, task_id);

CREATE TABLE IF NOT EXISTS agent_cursors (
  agent_id TEXT NOT NULL,
  stream_id TEXT NOT NULL,
  last_seq BIGINT NOT NULL DEFAULT 0,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY(agent_id, stream_id)
);
