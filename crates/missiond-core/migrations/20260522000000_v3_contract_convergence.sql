-- MissionD V3 SSOT contract convergence.
--
-- plan.contract_json stores the typed plan projection emitted by missiond-lispc
-- (missiond.plan-contract.v1). lisp_code_sync_jobs makes Lisp/code
-- reconciliation durable instead of doing compile/check work directly inside
-- the EventBus subscriber.

ALTER TABLE plan
  ADD COLUMN IF NOT EXISTS contract_json JSONB NOT NULL DEFAULT '{}'::jsonb;

CREATE TABLE IF NOT EXISTS lisp_code_sync_jobs (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id       TEXT NOT NULL,
    root_path        TEXT NOT NULL,
    changed_path     TEXT NOT NULL,
    content_hash     TEXT NOT NULL,
    event_kind       TEXT NOT NULL,
    status           TEXT NOT NULL DEFAULT 'queued',
    attempts         INT NOT NULL DEFAULT 0,
    next_run_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    lease_owner      TEXT,
    lease_expires_at TIMESTAMPTZ,
    checker_ok       BOOLEAN,
    checker_command  TEXT,
    checker_tail     TEXT,
    sync_task_id     TEXT REFERENCES board_tasks(id) ON DELETE SET NULL,
    dedupe_key       TEXT NOT NULL,
    storm_circuit    BOOLEAN NOT NULL DEFAULT false,
    last_error       TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (dedupe_key)
);

CREATE INDEX IF NOT EXISTS idx_lisp_code_sync_jobs_due
  ON lisp_code_sync_jobs (status, next_run_at);

CREATE INDEX IF NOT EXISTS idx_lisp_code_sync_jobs_project_status
  ON lisp_code_sync_jobs (project_id, status, next_run_at);

CREATE INDEX IF NOT EXISTS idx_lisp_code_sync_jobs_lease
  ON lisp_code_sync_jobs (lease_expires_at)
  WHERE lease_expires_at IS NOT NULL;
