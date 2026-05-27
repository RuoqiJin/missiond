-- MissionD control-plane hard cutover follow-up.
--
-- worktree_manifests stores attempt pre/post filesystem snapshots for
-- post-run write_scope verification. model_route_outcomes closes the router
-- learning loop without making provider prose or static defaults the only
-- routing signal.

CREATE TABLE IF NOT EXISTS worktree_manifests (
  id TEXT PRIMARY KEY,
  job_id TEXT REFERENCES jobs(id) ON DELETE SET NULL,
  attempt_id TEXT REFERENCES job_attempts(id) ON DELETE SET NULL,
  task_id TEXT,
  project_id TEXT,
  project_root TEXT,
  phase TEXT NOT NULL CHECK (phase IN ('pre', 'post')),
  manifest JSONB NOT NULL DEFAULT '{}'::jsonb,
  changed_paths JSONB NOT NULL DEFAULT '[]'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_worktree_manifests_attempt_phase
  ON worktree_manifests(attempt_id, phase, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_worktree_manifests_task_phase
  ON worktree_manifests(task_id, phase, created_at DESC);

CREATE TABLE IF NOT EXISTS model_route_outcomes (
  id TEXT PRIMARY KEY,
  request_id TEXT,
  task_id TEXT,
  project_id TEXT,
  task_class TEXT,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  route TEXT,
  decision JSONB NOT NULL DEFAULT '{}'::jsonb,
  outcome JSONB NOT NULL DEFAULT '{}'::jsonb,
  latency_ms BIGINT,
  prompt_tokens BIGINT,
  completion_tokens BIGINT,
  total_tokens BIGINT,
  cost_usd NUMERIC(18,8),
  artifact_hash TEXT,
  job_state TEXT,
  status TEXT NOT NULL DEFAULT 'recorded'
    CHECK (status IN ('recorded', 'succeeded', 'failed', 'blocked')),
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_model_route_outcomes_model
  ON model_route_outcomes(provider, model, task_class, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_model_route_outcomes_task
  ON model_route_outcomes(task_id, created_at DESC);
