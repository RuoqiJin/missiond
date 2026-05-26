-- Add workflow_run trend counters to the operator health sample read model.

ALTER TABLE operator_health_samples
  ADD COLUMN IF NOT EXISTS workflow_blocked BIGINT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS workflow_failed BIGINT NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS workflow_stale BIGINT NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_operator_health_samples_workflow
  ON operator_health_samples(sampled_at DESC, workflow_blocked, workflow_failed, workflow_stale);
