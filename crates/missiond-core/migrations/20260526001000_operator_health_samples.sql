-- MissionD operator health trends.
--
-- Lightweight Postgres-only samples for Board Operations Overview trend
-- projection. This is an operational read model, not a source of truth.

CREATE TABLE IF NOT EXISTS operator_health_samples (
  id BIGSERIAL PRIMARY KEY,
  sampled_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  worker_failed BIGINT NOT NULL DEFAULT 0,
  worker_stale BIGINT NOT NULL DEFAULT 0,
  event_dispatch_lag BIGINT NOT NULL DEFAULT 0,
  dlq_count BIGINT NOT NULL DEFAULT 0,
  evidence_missing BIGINT NOT NULL DEFAULT 0,
  pending_questions BIGINT NOT NULL DEFAULT 0,
  snapshot JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_operator_health_samples_sampled_at
  ON operator_health_samples(sampled_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_result_artifacts_completed_gate
  ON task_result_artifacts(task_id, created_at DESC)
  WHERE lower(result_status) IN ('completed', 'complete', 'verified', 'pass', 'passed');
