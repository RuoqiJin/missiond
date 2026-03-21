-- Phase 6: Intent Analyst — deep intent analysis from conversation turns
CREATE TABLE IF NOT EXISTS user_intents (
    id BIGSERIAL PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    turn_range_start INTEGER NOT NULL,     -- start turn_idx (inclusive)
    turn_range_end INTEGER NOT NULL,       -- end turn_idx (inclusive)
    intent_type TEXT NOT NULL,             -- 'normal_progress' | 'stuck_retry' | 'architecture_explore' | 'refactor_shift' | 'scope_creep'
    confidence REAL NOT NULL DEFAULT 0.5,  -- 0.0-1.0
    summary TEXT,                          -- LLM-generated intent summary
    context_json TEXT,                     -- optional structured context (JSON)
    related_goal_id TEXT,                  -- FK to board_tasks goal
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (session_id, turn_range_start)
);

CREATE INDEX IF NOT EXISTS idx_ui_session ON user_intents(session_id);
CREATE INDEX IF NOT EXISTS idx_ui_type ON user_intents(intent_type);
CREATE INDEX IF NOT EXISTS idx_ui_created ON user_intents(created_at);
