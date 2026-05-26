-- Codex replay control plane.
--
-- Stores MissionD-owned automation state for protocol-level Codex app-server
-- replay runs. Codex provider-local sqlite/jsonl files remain Codex-owned logs;
-- this table set is the PostgreSQL source of truth for the replay runner.

CREATE TABLE IF NOT EXISTS codex_replay_campaigns (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  project_root TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'running',
  current_phase TEXT NOT NULL DEFAULT 'queued',
  max_cycles INTEGER,
  interval_seconds INTEGER NOT NULL DEFAULT 0,
  completed_cycles INTEGER NOT NULL DEFAULT 0,
  last_run_id TEXT,
  last_error TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  completed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_codex_replay_campaigns_status
  ON codex_replay_campaigns(status, updated_at DESC);

CREATE TABLE IF NOT EXISTS codex_replay_runs (
  id TEXT PRIMARY KEY,
  campaign_id TEXT NOT NULL REFERENCES codex_replay_campaigns(id) ON DELETE CASCADE,
  cycle_no INTEGER NOT NULL,
  project_root TEXT NOT NULL,
  thread_id TEXT,
  review_turn_id TEXT,
  plan_turn_id TEXT,
  implement_turn_id TEXT,
  status TEXT NOT NULL DEFAULT 'queued',
  phase TEXT NOT NULL DEFAULT 'queued',
  model TEXT,
  reasoning_effort TEXT,
  plan_text TEXT,
  selected_options_json JSONB NOT NULL DEFAULT '[]'::jsonb,
  blocked_reason TEXT,
  last_error TEXT,
  final_message TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  started_at TIMESTAMPTZ,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  completed_at TIMESTAMPTZ,
  UNIQUE(campaign_id, cycle_no)
);

CREATE INDEX IF NOT EXISTS idx_codex_replay_runs_campaign
  ON codex_replay_runs(campaign_id, cycle_no DESC);

CREATE INDEX IF NOT EXISTS idx_codex_replay_runs_status
  ON codex_replay_runs(status, updated_at DESC);

CREATE TABLE IF NOT EXISTS codex_replay_events (
  id BIGSERIAL PRIMARY KEY,
  campaign_id TEXT NOT NULL REFERENCES codex_replay_campaigns(id) ON DELETE CASCADE,
  run_id TEXT REFERENCES codex_replay_runs(id) ON DELETE CASCADE,
  cycle_no INTEGER,
  phase TEXT NOT NULL,
  event_kind TEXT NOT NULL,
  message TEXT NOT NULL,
  payload JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_codex_replay_events_campaign
  ON codex_replay_events(campaign_id, id DESC);

CREATE INDEX IF NOT EXISTS idx_codex_replay_events_run
  ON codex_replay_events(run_id, id DESC);
