-- MissionD control-plane kernel hard cutover.
--
-- These tables make the runtime authority explicit: jobs/state, attempts,
-- work leases, capability grants, capability audit, review gates, and Board
-- projection rows. BoardTask/PTY/provider text remain projections or
-- observations; terminal control decisions must reference these typed facts.

CREATE TABLE IF NOT EXISTS capability_grants (
  id TEXT PRIMARY KEY,
  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('worker', 'conversation', 'task')),
  subject_id TEXT NOT NULL,
  operation TEXT NOT NULL CHECK (operation IN ('read', 'write', 'claim', 'settle', 'delegate', 'deploy', 'network', 'spawn')),
  scope_kind TEXT NOT NULL CHECK (scope_kind IN ('project', 'path', 'task', 'shared-memory', 'deploy-target', 'network-target')),
  scope_key TEXT NOT NULL,
  project_id TEXT,
  task_id TEXT,
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'revoked', 'expired', 'consumed')),
  issuer TEXT NOT NULL DEFAULT 'missiond',
  evidence_requirement TEXT,
  expires_at TIMESTAMPTZ,
  consumed_at TIMESTAMPTZ,
  revoked_at TIMESTAMPTZ,
  details JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_capability_grants_subject
  ON capability_grants(subject_kind, subject_id, status, expires_at);

CREATE INDEX IF NOT EXISTS idx_capability_grants_task_operation
  ON capability_grants(task_id, operation, status);

CREATE INDEX IF NOT EXISTS idx_capability_grants_scope
  ON capability_grants(scope_kind, scope_key, status);

CREATE TABLE IF NOT EXISTS capability_audit_events (
  id TEXT PRIMARY KEY,
  grant_id TEXT,
  subject_kind TEXT,
  subject_id TEXT,
  operation TEXT NOT NULL,
  scope_kind TEXT NOT NULL,
  scope_key TEXT NOT NULL,
  decision TEXT NOT NULL CHECK (decision IN ('allowed', 'denied')),
  code TEXT,
  reason TEXT,
  details JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_capability_audit_subject
  ON capability_audit_events(subject_kind, subject_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_capability_audit_decision
  ON capability_audit_events(decision, code, created_at DESC);

CREATE TABLE IF NOT EXISTS jobs (
  id TEXT PRIMARY KEY,
  project_id TEXT,
  task_id TEXT UNIQUE,
  state TEXT NOT NULL CHECK (state IN ('created', 'claimed', 'running', 'blocked', 'failed', 'completed', 'skipped')),
  source_kind TEXT NOT NULL DEFAULT 'board_task',
  source_id TEXT,
  current_attempt_id TEXT,
  artifact_hash TEXT,
  runtime_metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_jobs_project_state
  ON jobs(project_id, state, updated_at DESC);

CREATE TABLE IF NOT EXISTS job_attempts (
  id TEXT PRIMARY KEY,
  job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
  worker_id TEXT,
  conversation_id TEXT,
  state TEXT NOT NULL CHECK (state IN ('created', 'started', 'blocked', 'failed', 'completed', 'skipped')),
  started_at TIMESTAMPTZ,
  finished_at TIMESTAMPTZ,
  artifact_hash TEXT,
  details JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_job_attempts_job
  ON job_attempts(job_id, created_at DESC);

CREATE TABLE IF NOT EXISTS work_leases (
  id TEXT PRIMARY KEY,
  project_id TEXT,
  task_id TEXT,
  holder_id TEXT NOT NULL,
  holder_kind TEXT NOT NULL DEFAULT 'worker',
  scope_kind TEXT NOT NULL,
  scope_key TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'released', 'expired')),
  acquired_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  lease_expires_at TIMESTAMPTZ NOT NULL,
  released_at TIMESTAMPTZ,
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb
);

UPDATE work_leases
SET status = 'expired'
WHERE status = 'active'
  AND lease_expires_at < now();

WITH ranked AS (
  SELECT id,
         row_number() OVER (
           PARTITION BY scope_kind, scope_key
           ORDER BY acquired_at DESC, id DESC
         ) AS rn
  FROM work_leases
  WHERE status = 'active'
)
UPDATE work_leases
SET status = 'expired'
WHERE id IN (SELECT id FROM ranked WHERE rn > 1);

CREATE UNIQUE INDEX IF NOT EXISTS uq_work_leases_active_scope
  ON work_leases(scope_kind, scope_key)
  WHERE status = 'active';

CREATE INDEX IF NOT EXISTS idx_work_leases_task
  ON work_leases(task_id, status, lease_expires_at);

CREATE TABLE IF NOT EXISTS review_gates (
  id TEXT PRIMARY KEY,
  job_id TEXT REFERENCES jobs(id) ON DELETE CASCADE,
  task_id TEXT,
  gate_kind TEXT NOT NULL,
  state TEXT NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'approved', 'rejected', 'superseded')),
  evidence_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
  decision JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  resolved_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_review_gates_task
  ON review_gates(task_id, state, created_at DESC);

CREATE TABLE IF NOT EXISTS board_task_views (
  task_id TEXT PRIMARY KEY,
  job_id TEXT,
  projected_status TEXT NOT NULL,
  artifact_hash TEXT,
  projection JSONB NOT NULL DEFAULT '{}'::jsonb,
  projected_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_board_task_views_status
  ON board_task_views(projected_status, projected_at DESC);

-- Shared-memory claims already use an advisory transaction lock in code. This
-- partial unique index adds a database backstop after stale claims are expired
-- inside the same transaction.
UPDATE shared_claims
SET status = 'expired'
WHERE status = 'active'
  AND lease_expires_at < now();

WITH ranked AS (
  SELECT id,
         row_number() OVER (
           PARTITION BY scope_kind, scope_key
           ORDER BY acquired_at DESC, id DESC
         ) AS rn
  FROM shared_claims
  WHERE status = 'active'
)
UPDATE shared_claims
SET status = 'expired'
WHERE id IN (SELECT id FROM ranked WHERE rn > 1);

CREATE UNIQUE INDEX IF NOT EXISTS uq_shared_claims_active_scope
  ON shared_claims(scope_kind, scope_key)
  WHERE status = 'active';

-- Canonical completion artifact projection. Keep the existing storage table as
-- the write authority; this view gives the new kernel a stable name.
CREATE OR REPLACE VIEW completion_artifacts AS
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
  created_at
FROM task_result_artifacts;
