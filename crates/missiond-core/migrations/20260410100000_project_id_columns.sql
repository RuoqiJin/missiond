-- Add project_id to conversations (nullable for historical data)
ALTER TABLE conversations ADD COLUMN IF NOT EXISTS project_id TEXT;
CREATE INDEX IF NOT EXISTS idx_conversations_project_id ON conversations(project_id);

-- Add project_id to knowledge (nullable: NULL = global knowledge)
ALTER TABLE knowledge ADD COLUMN IF NOT EXISTS project_id TEXT;
CREATE INDEX IF NOT EXISTS idx_knowledge_project_id ON knowledge(project_id);

-- board_tasks already has a project column, add project_id as normalized version
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name='board_tasks' AND column_name='project_id') THEN
        ALTER TABLE board_tasks ADD COLUMN project_id TEXT;
        CREATE INDEX idx_board_tasks_project_id ON board_tasks(project_id);
    END IF;
END $$;
