-- Structured catalog for cold V3 runtime files.
--
-- The files under .missiond/v3/runtime/** remain diagnostic caches on disk.
-- This table gives the control plane a queryable index and retention boundary
-- without turning cold runtime files back into authoring SSOT.

CREATE TABLE IF NOT EXISTS runtime_artifacts (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  hash TEXT NOT NULL,
  path TEXT NOT NULL,
  kind TEXT NOT NULL,
  source_surface TEXT,
  project_id TEXT,
  task_id TEXT,
  media_type TEXT NOT NULL DEFAULT 'application/octet-stream',
  size_bytes BIGINT NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'active',
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  indexed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  expires_at TIMESTAMPTZ,
  UNIQUE(path, hash)
);

CREATE INDEX IF NOT EXISTS idx_runtime_artifacts_project_task
  ON runtime_artifacts(project_id, task_id, indexed_at DESC);

CREATE INDEX IF NOT EXISTS idx_runtime_artifacts_kind_status
  ON runtime_artifacts(kind, status, indexed_at DESC);

CREATE INDEX IF NOT EXISTS idx_runtime_artifacts_expires
  ON runtime_artifacts(expires_at)
  WHERE expires_at IS NOT NULL AND status = 'active';
