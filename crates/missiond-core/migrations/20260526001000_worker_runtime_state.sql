-- MissionD high-ROI operator health substrate.
--
-- Stores the latest runtime snapshot for each daemon worker. This is a
-- Postgres-only operational read model; it is not a task source of truth.

CREATE TABLE IF NOT EXISTS worker_runtime_state (
  worker_name TEXT PRIMARY KEY,
  lifecycle TEXT NOT NULL DEFAULT 'idle',
  current_task_id TEXT,
  current_slot_id TEXT,
  last_heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  last_progress_at TIMESTAMPTZ,
  last_error TEXT,
  lease_expires_at TIMESTAMPTZ,
  tasks_processed BIGINT NOT NULL DEFAULT 0,
  tasks_failed BIGINT NOT NULL DEFAULT 0,
  current JSONB NOT NULL DEFAULT '{}'::jsonb,
  stale BOOLEAN NOT NULL DEFAULT false,
  stale_reason TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_worker_runtime_state_lifecycle
  ON worker_runtime_state(lifecycle, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_worker_runtime_state_stale
  ON worker_runtime_state(stale, updated_at DESC)
  WHERE stale = true;
