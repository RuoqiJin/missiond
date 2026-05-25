-- Structured orchestration metadata for interaction/work-order dispatch.
-- Human-readable task descriptions remain summaries; runtime gates and Exec
-- timelines should read this JSONB field first and fall back to legacy
-- description parsing only for old tasks.

ALTER TABLE board_tasks
    ADD COLUMN IF NOT EXISTS runtime_metadata JSONB NOT NULL DEFAULT '{}'::jsonb;

CREATE INDEX IF NOT EXISTS idx_board_tasks_runtime_metadata_gin
    ON board_tasks USING GIN (runtime_metadata);

CREATE INDEX IF NOT EXISTS idx_board_tasks_runtime_metadata_interaction_id
    ON board_tasks ((runtime_metadata->>'interaction_id'))
    WHERE runtime_metadata ? 'interaction_id';

CREATE INDEX IF NOT EXISTS idx_board_tasks_runtime_metadata_grounding_context_id
    ON board_tasks ((runtime_metadata->>'grounding_context_id'))
    WHERE runtime_metadata ? 'grounding_context_id';
