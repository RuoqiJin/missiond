-- MissionD kernel reverse convergence follow-up.
-- missiond-allow-destructive-migration: replaces CHECK constraints with wider
-- compatible constraints and recreates a projection view; no data is dropped.
--
-- This closes the first implementation mismatch from the aggressive slimming
-- plan: capability subjects include system/operator/daemon, Board runtime
-- control contracts move to task_contracts, and task_result_artifacts records
-- the attempt/subject/grant that produced the canonical completion fact.

ALTER TABLE capability_grants
  DROP CONSTRAINT IF EXISTS capability_grants_subject_kind_check;

ALTER TABLE capability_grants
  ADD CONSTRAINT capability_grants_subject_kind_check
  CHECK (subject_kind IN ('worker', 'conversation', 'task', 'system', 'operator', 'daemon'));

CREATE TABLE IF NOT EXISTS task_contracts (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL UNIQUE,
  project_id TEXT,
  task_contract_id TEXT NOT NULL,
  dispatch_metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
  read_scope JSONB NOT NULL DEFAULT '[]'::jsonb,
  write_scope JSONB NOT NULL DEFAULT '[]'::jsonb,
  must_not_touch JSONB NOT NULL DEFAULT '[]'::jsonb,
  capability_grant_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
  sandbox_profile TEXT,
  completion_materialization_policy TEXT,
  grounding_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
  context_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_task_contracts_project
  ON task_contracts(project_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_contracts_contract_id
  ON task_contracts(task_contract_id);

ALTER TABLE work_leases
  ADD COLUMN IF NOT EXISTS heartbeat_at TIMESTAMPTZ;

ALTER TABLE task_result_artifacts
  ADD COLUMN IF NOT EXISTS job_id TEXT,
  ADD COLUMN IF NOT EXISTS attempt_id TEXT,
  ADD COLUMN IF NOT EXISTS producer_subject_kind TEXT,
  ADD COLUMN IF NOT EXISTS producer_subject_id TEXT,
  ADD COLUMN IF NOT EXISTS capability_grant_id TEXT;

CREATE INDEX IF NOT EXISTS idx_task_result_artifacts_attempt
  ON task_result_artifacts(task_id, attempt_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_result_artifacts_capability_grant
  ON task_result_artifacts(capability_grant_id)
  WHERE capability_grant_id IS NOT NULL;

DROP VIEW IF EXISTS completion_artifacts;

CREATE VIEW completion_artifacts AS
SELECT
  id,
  artifact_hash,
  project_id,
  task_id,
  slot_id,
  conversation_id,
  provider,
  result_status,
  summary,
  job_id,
  attempt_id,
  producer_subject_kind,
  producer_subject_id,
  capability_grant_id,
  created_at
FROM task_result_artifacts;
