-- Phase 4: conversation_turns — structured Turn boundaries from flat message stream.
-- A Turn = one user instruction + all Claude Code actions to complete it.
-- Populated by TaggerChunkerWorker (S3 of Cognitive Pipeline).

CREATE TABLE IF NOT EXISTS conversation_turns (
    id BIGSERIAL PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    turn_idx INTEGER NOT NULL,
    start_message_id BIGINT NOT NULL REFERENCES conversation_messages(id),
    end_message_id BIGINT NOT NULL REFERENCES conversation_messages(id),

    -- Pre-extracted summary fields (pure rules, no LLM)
    user_content TEXT,                      -- User message text (truncated to 2000 chars)
    tool_names TEXT,                        -- CSV: "Bash,Read,Edit"
    tool_call_count INTEGER DEFAULT 0,      -- Number of tool_use messages
    message_count INTEGER DEFAULT 0,        -- Total messages in Turn
    has_code_change BOOLEAN DEFAULT FALSE,  -- Contains Write/Edit tool calls
    has_mcp_call BOOLEAN DEFAULT FALSE,     -- Contains mcp__ tool calls

    started_at TEXT,                        -- First message timestamp
    ended_at TEXT,                          -- Last message timestamp

    -- Downstream fields (filled by S4 Embedder / Intent Analyst)
    topic TEXT,
    intent_group_id BIGINT,

    UNIQUE (session_id, turn_idx)
);

CREATE INDEX IF NOT EXISTS idx_ct_session ON conversation_turns(session_id);
CREATE INDEX IF NOT EXISTS idx_ct_session_idx ON conversation_turns(session_id, turn_idx);
CREATE INDEX IF NOT EXISTS idx_ct_start_msg ON conversation_turns(start_message_id);
CREATE INDEX IF NOT EXISTS idx_ct_end_msg ON conversation_turns(end_message_id);
