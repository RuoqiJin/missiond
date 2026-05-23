-- Durable commit envelope for artifact/DB/event coupling.
--
-- The artifact file remains the file-first SSOT, but multi-step writers now
-- leave a DB recovery record before/while linking the file, mirrored row, and
-- event projection. operation_key is the idempotency boundary.

CREATE TABLE IF NOT EXISTS artifact_commit_outbox (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  operation_key TEXT NOT NULL UNIQUE,
  surface TEXT NOT NULL,
  request_id TEXT,
  project_id TEXT,
  artifact_kind TEXT NOT NULL,
  artifact_path TEXT NOT NULL,
  artifact_sha256 TEXT,
  db_table TEXT,
  db_row_id TEXT,
  event_id TEXT,
  event_seq BIGINT,
  status TEXT NOT NULL DEFAULT 'pending',
  attempt_count INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  payload JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  completed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_artifact_commit_outbox_status_updated
  ON artifact_commit_outbox(status, updated_at);

CREATE INDEX IF NOT EXISTS idx_artifact_commit_outbox_request
  ON artifact_commit_outbox(request_id, event_seq)
  WHERE request_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_artifact_commit_outbox_project
  ON artifact_commit_outbox(project_id, updated_at DESC)
  WHERE project_id IS NOT NULL;
