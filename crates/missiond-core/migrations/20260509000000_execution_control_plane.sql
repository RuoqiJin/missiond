-- Execution control plane runtime tables.
--
-- These make long-running workflow batches and worker final results first-class
-- durable objects. Board notes and PTY text remain projections/diagnostics.

CREATE TABLE IF NOT EXISTS workflow_runs (
  id TEXT PRIMARY KEY,
  workflow_id TEXT,
  workflow_path TEXT,
  project_id TEXT,
  parent_task_id TEXT,
  status TEXT NOT NULL DEFAULT 'running',
  cursor JSONB NOT NULL DEFAULT '{}'::jsonb,
  checkpoint JSONB NOT NULL DEFAULT '{}'::jsonb,
  max_inflight INTEGER NOT NULL DEFAULT 1,
  active_task_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
  artifact_hashes JSONB NOT NULL DEFAULT '[]'::jsonb,
  started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  finished_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_workflow_runs_project_status
  ON workflow_runs(project_id, status);

CREATE INDEX IF NOT EXISTS idx_workflow_runs_parent_task
  ON workflow_runs(parent_task_id);

CREATE TABLE IF NOT EXISTS task_result_artifacts (
  id TEXT PRIMARY KEY,
  artifact_hash TEXT NOT NULL REFERENCES shared_artifacts(hash) ON DELETE RESTRICT,
  project_id TEXT,
  task_id TEXT NOT NULL,
  slot_id TEXT,
  conversation_id TEXT,
  provider TEXT,
  result_status TEXT NOT NULL DEFAULT 'completed',
  summary TEXT NOT NULL DEFAULT '',
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE(task_id, artifact_hash)
);

CREATE INDEX IF NOT EXISTS idx_task_result_artifacts_task
  ON task_result_artifacts(task_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_result_artifacts_conversation
  ON task_result_artifacts(conversation_id);
