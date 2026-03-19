-- Daily reconciliation watermarks: track per-file reconcile progress
-- Design: docs/designs/reconcile-worker.md

CREATE TABLE IF NOT EXISTS reconcile_watermarks (
    jsonl_path TEXT PRIMARY KEY,
    last_reconciled_size BIGINT NOT NULL DEFAULT 0,
    last_reconciled_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
