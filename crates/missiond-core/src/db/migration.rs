//! Database schema initialization and migrations.

use rusqlite::params;
use super::MissionDB;
use super::error::DbResult;
use super::extract_parent_session_id;
use std::sync::atomic::Ordering;

const SCHEMA: &str = r#"
-- Tasks table
CREATE TABLE IF NOT EXISTS tasks (
  id TEXT PRIMARY KEY,
  role TEXT NOT NULL,
  prompt TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'queued',
  slot_id TEXT,
  session_id TEXT,
  result TEXT,
  error TEXT,
  created_at INTEGER NOT NULL,
  started_at INTEGER,
  finished_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_role ON tasks(role);
CREATE INDEX IF NOT EXISTS idx_tasks_created ON tasks(created_at);

-- Inbox table
CREATE TABLE IF NOT EXISTS inbox (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  from_role TEXT NOT NULL,
  content TEXT NOT NULL,
  read INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_inbox_read ON inbox(read);
CREATE INDEX IF NOT EXISTS idx_inbox_created ON inbox(created_at);

-- Events log table
CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id TEXT NOT NULL,
  type TEXT NOT NULL,
  data TEXT,
  timestamp INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_task ON events(task_id);
CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);

-- Session cache table
CREATE TABLE IF NOT EXISTS slot_sessions (
  slot_id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

-- Board tasks (personal task board)
CREATE TABLE IF NOT EXISTS board_tasks (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'open',
  priority TEXT NOT NULL DEFAULT 'medium',
  category TEXT NOT NULL DEFAULT 'other',
  project TEXT,
  server TEXT,
  due_date TEXT,
  parent_id TEXT,
  assignee TEXT,
  auto_execute INTEGER NOT NULL DEFAULT 0,
  prompt_template TEXT,
  hidden INTEGER NOT NULL DEFAULT 0,
  retry_count INTEGER NOT NULL DEFAULT 0,
  max_retries INTEGER NOT NULL DEFAULT 2,
  order_idx INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  claim_executor_id TEXT,
  claim_executor_type TEXT,
  claimed_at TEXT,
  lease_expires_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_board_tasks_status ON board_tasks(status);
CREATE INDEX IF NOT EXISTS idx_board_tasks_parent ON board_tasks(parent_id);
-- idx_board_tasks_claim created in migration (column may not exist yet on existing DBs)

-- Board task notes (progress tracking)
CREATE TABLE IF NOT EXISTS board_task_notes (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  content TEXT NOT NULL,
  note_type TEXT NOT NULL DEFAULT 'note',
  author TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (task_id) REFERENCES board_tasks(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_board_task_notes_task ON board_task_notes(task_id);

-- Agent questions (pending decisions for user/master)
CREATE TABLE IF NOT EXISTS agent_questions (
    id TEXT PRIMARY KEY,
    task_id TEXT,
    slot_id TEXT,
    session_id TEXT,
    question TEXT NOT NULL,
    context TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    answer TEXT,
    target TEXT NOT NULL DEFAULT 'user',
    options TEXT,
    decision_type TEXT NOT NULL DEFAULT 'implementation',
    retry_count INTEGER NOT NULL DEFAULT 0,
    routing_trace TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_questions_status ON agent_questions(status);
CREATE INDEX IF NOT EXISTS idx_agent_questions_target ON agent_questions(target, status);

-- Knowledge base (Jarvis Memory)
CREATE TABLE IF NOT EXISTS knowledge (
    id TEXT PRIMARY KEY,
    category TEXT NOT NULL,
    key TEXT NOT NULL,
    summary TEXT NOT NULL,
    detail TEXT,
    source TEXT DEFAULT 'conversation',
    confidence REAL DEFAULT 1.0,
    access_count INTEGER DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_accessed_at TEXT,
    linked_task_id TEXT,
    embedding BLOB,
    embedding_provider TEXT,
    UNIQUE(category, key)
);
CREATE INDEX IF NOT EXISTS idx_knowledge_category ON knowledge(category);

-- Knowledge credentials
CREATE TABLE IF NOT EXISTS credentials (
    id TEXT PRIMARY KEY,
    knowledge_id TEXT REFERENCES knowledge(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    value_encrypted TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Conversation sessions
CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    project TEXT,
    slot_id TEXT,
    source TEXT NOT NULL DEFAULT 'claude_cli',
    model TEXT,
    git_branch TEXT,
    jsonl_path TEXT,
    message_count INTEGER DEFAULT 0,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    status TEXT DEFAULT 'active',
    analyzed_at TEXT,
    updated_at TEXT,
    memory_forwarded_at TEXT,
    user_voice_forwarded_at TEXT,
    realtime_forwarded_at TEXT,
    analysis_version INTEGER DEFAULT 0,
    analysis_retries INTEGER DEFAULT 0,
    parent_session_id TEXT,
    deep_analyzed_message_id INTEGER DEFAULT 0,
    task_id TEXT,
    chat_type TEXT DEFAULT 'pty',
    conversation_type TEXT NOT NULL DEFAULT 'user',
    llm_summary TEXT,
    embedding_provider TEXT,
    embedding BLOB,
    session_timeline TEXT,
    timeline_built_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_conv_status ON conversations(status);

-- Conversation messages
CREATE TABLE IF NOT EXISTS conversation_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    raw_content TEXT,
    message_uuid TEXT,
    parent_uuid TEXT,
    model TEXT,
    timestamp TEXT NOT NULL,
    metadata TEXT,
    FOREIGN KEY (session_id) REFERENCES conversations(id)
);
CREATE INDEX IF NOT EXISTS idx_conv_msg_session ON conversation_messages(session_id);
CREATE INDEX IF NOT EXISTS idx_conv_msg_timestamp ON conversation_messages(timestamp);

-- Auto-update message_count and updated_at on conversations when messages are inserted
CREATE TRIGGER IF NOT EXISTS trg_msg_count_insert
AFTER INSERT ON conversation_messages
BEGIN
    UPDATE conversations
    SET message_count = message_count + 1,
        updated_at = NEW.timestamp
    WHERE id = NEW.session_id;
END;

-- Gemini File API upload cache (avoid re-uploading unchanged files)
CREATE TABLE IF NOT EXISTS gemini_file_uploads (
    file_hash TEXT PRIMARY KEY,
    file_uri TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    expires_at INTEGER NOT NULL
);
"#;

impl MissionDB {
    /// Initialize the database schema and run migrations.
    pub(super) fn init(&self) -> DbResult<()> {
        {
            let conn = self.conn();
            conn.execute_batch(SCHEMA)?;
        }
        self.migrate()?;
        self.check_fts_integrity()?;
        Ok(())
    }

    /// Rebuild FTS5 index on startup to ensure consistency.
    /// integrity-check only validates structure, not data consistency after concurrent writes.
    fn check_fts_integrity(&self) -> DbResult<()> {
        let conn = self.conn();
        let has_fts: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='knowledge_fts'",
            [],
            |row| row.get(0),
        ).unwrap_or(false);
        if !has_fts {
            return Ok(());
        }
        conn.execute_batch(
            "INSERT INTO knowledge_fts(knowledge_fts) VALUES('rebuild')"
        )?;
        self.fts_dirty.store(false, Ordering::Relaxed);
        tracing::info!("FTS5 index rebuilt on startup");
        Ok(())
    }

    /// Rebuild FTS5 index if dirty flag is set (called from autopilot_tick).
    /// Returns true if rebuild was performed.
    pub fn kb_rebuild_fts_if_dirty(&self) -> DbResult<bool> {
        if !self.fts_dirty.swap(false, Ordering::Relaxed) {
            return Ok(false);
        }
        let conn = self.conn();
        conn.execute_batch("INSERT INTO knowledge_fts(knowledge_fts) VALUES('rebuild')")?;
        tracing::info!("FTS5 index rebuilt (dirty flag)");
        Ok(true)
    }

    /// Run schema migrations for existing databases
    fn migrate(&self) -> DbResult<()> {
        let conn = self.conn();

        // Phase D: Add autopilot columns to board_tasks
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(board_tasks)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();

        if !columns.iter().any(|c| c == "assignee") {
            conn.execute_batch(
                "ALTER TABLE board_tasks ADD COLUMN assignee TEXT;
                 ALTER TABLE board_tasks ADD COLUMN auto_execute INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE board_tasks ADD COLUMN prompt_template TEXT;"
            )?;
        }

        if !columns.iter().any(|c| c == "hidden") {
            conn.execute_batch(
                "ALTER TABLE board_tasks ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;"
            )?;
        }

        if !columns.iter().any(|c| c == "retry_count") {
            conn.execute_batch(
                "ALTER TABLE board_tasks ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE board_tasks ADD COLUMN max_retries INTEGER NOT NULL DEFAULT 2;"
            )?;
        }

        // Flow Engine: add flow_phase, flow_context, flow_template columns
        if !columns.iter().any(|c| c == "flow_phase") {
            conn.execute_batch(
                "ALTER TABLE board_tasks ADD COLUMN flow_phase TEXT;
                 ALTER TABLE board_tasks ADD COLUMN flow_context TEXT;
                 ALTER TABLE board_tasks ADD COLUMN flow_template TEXT;"
            )?;
        }

        // Phase T: Task Claim — add claim fields for conflict prevention
        if !columns.iter().any(|c| c == "claim_executor_id") {
            conn.execute_batch(
                "ALTER TABLE board_tasks ADD COLUMN claim_executor_id TEXT;
                 ALTER TABLE board_tasks ADD COLUMN claim_executor_type TEXT;
                 ALTER TABLE board_tasks ADD COLUMN claimed_at TEXT;"
            )?;
            // Index for fast lookup by executor (zombie cleanup)
            conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_board_tasks_claim ON board_tasks(claim_executor_id);"
            )?;
        }

        // Pipeline DAG: add depends_on column
        if !columns.iter().any(|c| c == "depends_on") {
            conn.execute_batch(
                "ALTER TABLE board_tasks ADD COLUMN depends_on TEXT NOT NULL DEFAULT '[]';"
            )?;
        }

        if !columns.iter().any(|c| c == "lease_expires_at") {
            conn.execute_batch(
                "ALTER TABLE board_tasks ADD COLUMN lease_expires_at TEXT;"
            )?;
        }

        // AIOps alert aggregation: dedupe_key for state-based dedup
        if !columns.iter().any(|c| c == "dedupe_key") {
            conn.execute_batch(
                "ALTER TABLE board_tasks ADD COLUMN dedupe_key TEXT;
                 CREATE INDEX IF NOT EXISTS idx_board_tasks_dedupe ON board_tasks(dedupe_key, status);"
            )?;
        }

        // Knowledge Base: create FTS index if knowledge table exists but FTS doesn't
        let has_knowledge: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='knowledge'",
            [],
            |row| row.get(0),
        )?;
        if has_knowledge {
            let has_fts: bool = conn.query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='knowledge_fts'",
                [],
                |row| row.get(0),
            )?;
            if !has_fts {
                conn.execute_batch(
                    "CREATE VIRTUAL TABLE knowledge_fts USING fts5(
                        key, summary, detail, category,
                        content='knowledge', content_rowid='rowid'
                    );
                    -- Populate FTS from existing data
                    INSERT INTO knowledge_fts(rowid, key, summary, detail, category)
                        SELECT rowid, key, summary, COALESCE(detail, ''), category FROM knowledge;"
                )?;
            }
        }

        // KB type field: rule/fact/goal/state — inferred from category prefix
        if has_knowledge {
            let kb_columns: Vec<String> = conn
                .prepare("PRAGMA table_info(knowledge)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .collect();
            if !kb_columns.iter().any(|c| c == "kb_type") {
                conn.execute_batch("ALTER TABLE knowledge ADD COLUMN kb_type TEXT DEFAULT 'fact';")?;
                // Backfill existing entries based on category prefix
                conn.execute_batch(
                    "UPDATE knowledge SET kb_type = 'rule' WHERE category LIKE 'policy%' OR category LIKE 'preference%' OR category = 'system_rule' OR category = 'decision';
                     UPDATE knowledge SET kb_type = 'goal' WHERE category LIKE 'feature%' OR category LIKE 'project%' OR category = 'design_spec';
                     UPDATE knowledge SET kb_type = 'state' WHERE category LIKE 'memory:ops%' OR category LIKE 'memory:debug%' OR category LIKE 'memory:bugfix%' OR category = 'ops';"
                )?;
            }
            // Working Memory scope: NULL=global (default), task_id=scratchpad for that task
            if !kb_columns.iter().any(|c| c == "scope_task_id") {
                conn.execute_batch(
                    "ALTER TABLE knowledge ADD COLUMN scope_task_id TEXT;
                     CREATE INDEX IF NOT EXISTS idx_kb_scope ON knowledge(scope_task_id);"
                )?;
            }
        }

        // Add memory_forwarded_at to conversations if missing
        let conv_columns: Vec<String> = conn
            .prepare("PRAGMA table_info(conversations)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();
        if !conv_columns.iter().any(|c| c == "memory_forwarded_at") {
            conn.execute_batch(
                "ALTER TABLE conversations ADD COLUMN memory_forwarded_at TEXT;"
            )?;
        }
        if !conv_columns.iter().any(|c| c == "user_voice_forwarded_at") {
            conn.execute_batch(
                "ALTER TABLE conversations ADD COLUMN user_voice_forwarded_at TEXT;"
            )?;
        }
        // Unified realtime watermark (replaces memory_forwarded_at + user_voice_forwarded_at)
        if !conv_columns.iter().any(|c| c == "realtime_forwarded_at") {
            conn.execute_batch(
                "ALTER TABLE conversations ADD COLUMN realtime_forwarded_at TEXT;"
            )?;
            // Migrate: set realtime watermark to max of old watermarks
            conn.execute_batch(
                "UPDATE conversations SET realtime_forwarded_at = MAX(
                    COALESCE(memory_forwarded_at, ''),
                    COALESCE(user_voice_forwarded_at, '')
                ) WHERE memory_forwarded_at IS NOT NULL OR user_voice_forwarded_at IS NOT NULL;"
            )?;
        }

        // Performance indexes for conversation messages
        conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_conv_msg_uuid ON conversation_messages(message_uuid);
             CREATE INDEX IF NOT EXISTS idx_conv_memory_pending ON conversations(slot_id, memory_forwarded_at);
             CREATE INDEX IF NOT EXISTS idx_conv_realtime_pending ON conversations(slot_id, realtime_forwarded_at);"
        )?;

        // Deep analysis: migrate from message_pipeline_state to conversation-level watermark.
        // Add analysis_version and analysis_retries columns if missing.
        let has_analysis_version: bool = conn
            .prepare("SELECT analysis_version FROM conversations LIMIT 0")
            .is_ok();
        if !has_analysis_version {
            conn.execute_batch(
                "ALTER TABLE conversations ADD COLUMN analysis_version INTEGER DEFAULT 0;
                 ALTER TABLE conversations ADD COLUMN analysis_retries INTEGER DEFAULT 0;"
            )?;
            // Backfill: mark all completed user conversations as analyzed (v1) to prevent
            // startup thundering herd. Realtime pipeline already extracted message-level knowledge.
            let backfilled: usize = conn.execute(
                "UPDATE conversations
                 SET analyzed_at = COALESCE(analyzed_at, ended_at),
                     analysis_version = 1
                 WHERE status = 'completed'
                   AND conversation_type = 'user'
                   AND analyzed_at IS NULL",
                [],
            )?;
            if backfilled > 0 {
                tracing::info!(backfilled, "Backfilled analysis_version for historical conversations");
            }
        }

        // Drop legacy message_pipeline_state table (no longer used by any pipeline:
        // realtime uses realtime_forwarded_at watermark, deep_analysis uses analyzed_at + analysis_version)
        conn.execute_batch("DROP TABLE IF EXISTS message_pipeline_state;")?;

        // Add parent_session_id for subagent → parent conversation linking
        let has_parent_session_id: bool = conn
            .prepare("SELECT parent_session_id FROM conversations LIMIT 0")
            .is_ok();
        if !has_parent_session_id {
            conn.execute_batch(
                "ALTER TABLE conversations ADD COLUMN parent_session_id TEXT;"
            )?;
            // Backfill: extract parent session ID from jsonl_path for existing subagent sessions
            // Path pattern: .../PARENT_SESSION_ID/subagents/agent-xxx.jsonl
            let mut stmt = conn.prepare(
                "SELECT id, jsonl_path FROM conversations WHERE id LIKE 'agent-%' AND jsonl_path IS NOT NULL"
            )?;
            let rows: Vec<(String, String)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            for (id, path) in &rows {
                if let Some(parent_id) = extract_parent_session_id(path) {
                    conn.execute(
                        "UPDATE conversations SET parent_session_id = ?1 WHERE id = ?2",
                        rusqlite::params![parent_id, id],
                    )?;
                }
            }
            if !rows.is_empty() {
                tracing::info!(count = rows.len(), "Backfilled parent_session_id for subagent conversations");
            }
        }

        // Add deep_analyzed_message_id for incremental checkpoint watermark
        let has_deep_analyzed_message_id: bool = conn
            .prepare("SELECT deep_analyzed_message_id FROM conversations LIMIT 0")
            .is_ok();
        if !has_deep_analyzed_message_id {
            conn.execute_batch(
                "ALTER TABLE conversations ADD COLUMN deep_analyzed_message_id INTEGER DEFAULT 0;"
            )?;
        }

        // Add task_id for grouping compacted sessions under the same logical task
        let has_task_id: bool = conn
            .prepare("SELECT task_id FROM conversations LIMIT 0")
            .is_ok();
        if !has_task_id {
            conn.execute_batch(
                "ALTER TABLE conversations ADD COLUMN task_id TEXT;
                 CREATE INDEX IF NOT EXISTS idx_conv_task_id ON conversations(task_id);"
            )?;
        }

        // Add chat_type for distinguishing PTY vs router_chat conversations
        let has_chat_type: bool = conn
            .prepare("SELECT chat_type FROM conversations LIMIT 0")
            .is_ok();
        if !has_chat_type {
            conn.execute_batch(
                "ALTER TABLE conversations ADD COLUMN chat_type TEXT DEFAULT 'pty';"
            )?;
        }

        // Add conversation_type: 'user' | 'meta' | 'worker' | 'subagent'
        let has_conv_type: bool = conn
            .prepare("SELECT conversation_type FROM conversations LIMIT 0")
            .is_ok();
        if !has_conv_type {
            conn.execute_batch(
                "ALTER TABLE conversations ADD COLUMN conversation_type TEXT NOT NULL DEFAULT 'user';"
            )?;
            // Backfill from slot_id and id patterns
            conn.execute_batch(
                "UPDATE conversations SET conversation_type = 'meta' WHERE slot_id IN ('slot-memory', 'slot-memory-slow');
                 UPDATE conversations SET conversation_type = 'worker' WHERE slot_id IS NOT NULL AND conversation_type = 'user' AND slot_id NOT IN ('slot-memory', 'slot-memory-slow');
                 UPDATE conversations SET conversation_type = 'subagent' WHERE id LIKE 'agent-%';"
            )?;
            conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_conv_type ON conversations(conversation_type);"
            )?;
        }

        // Backfill: reclassify compaction fragments from 'subagent' to 'compaction'
        // (agent-acompact-* sessions were previously misclassified by derive_conversation_type)
        let compaction_fixed: usize = conn.execute(
            "UPDATE conversations SET conversation_type = 'compaction'
             WHERE id LIKE '%acompact%' AND conversation_type != 'compaction'",
            [],
        )?;
        if compaction_fixed > 0 {
            tracing::info!(count = compaction_fixed, "Reclassified compaction fragments from subagent");
        }

        // Add updated_at for tracking last message write time (compaction detection)
        if !conv_columns.iter().any(|c| c == "updated_at") {
            conn.execute_batch(
                "ALTER TABLE conversations ADD COLUMN updated_at TEXT;"
            )?;
            // Backfill: set updated_at = started_at for existing rows
            conn.execute_batch(
                "UPDATE conversations SET updated_at = started_at WHERE updated_at IS NULL;"
            )?;
        }

        // Slot task history table
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS slot_tasks (
                id TEXT PRIMARY KEY,
                slot_id TEXT NOT NULL,
                task_type TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                prompt_summary TEXT,
                source_sessions TEXT,
                output_count INTEGER DEFAULT 0,
                created_at TEXT NOT NULL,
                started_at TEXT,
                completed_at TEXT,
                duration_ms INTEGER,
                error TEXT,
                conversation_id TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_slot_tasks_slot ON slot_tasks(slot_id);
            CREATE INDEX IF NOT EXISTS idx_slot_tasks_type ON slot_tasks(task_type);
            CREATE INDEX IF NOT EXISTS idx_slot_tasks_created ON slot_tasks(created_at);"
        )?;

        // Backfill: fix tool_result messages incorrectly stored as role='user'.
        // In Claude Code JSONL, tool_result has type="user" + role="user" but
        // content blocks are all type="tool_result". Detect via raw_content.
        let fixed = conn.execute(
            "UPDATE conversation_messages SET role = 'tool_result'
             WHERE role = 'user' AND raw_content LIKE '%\"type\":\"tool_result\"%'",
            [],
        )?;
        if fixed > 0 {
            tracing::info!(count = fixed, "Backfilled tool_result role for misclassified messages");
        }

        // Backfill: slot session "user" messages → "system" (daemon-sent prompts, not the human)
        let fixed_system = conn.execute(
            "UPDATE conversation_messages SET role = 'system'
             WHERE role = 'user'
               AND session_id IN (SELECT id FROM conversations WHERE slot_id IS NOT NULL)",
            [],
        )?;
        if fixed_system > 0 {
            tracing::info!(count = fixed_system, "Backfilled system role for slot session messages");
        }

        // Add linked_task_id to knowledge table for Board-aware consolidation
        let has_linked_task_id: bool = conn
            .prepare("SELECT linked_task_id FROM knowledge LIMIT 0")
            .is_ok();
        if !has_linked_task_id {
            conn.execute_batch(
                "ALTER TABLE knowledge ADD COLUMN linked_task_id TEXT;"
            )?;
        }

        // Add embedding column to knowledge table for semantic vector search
        let has_embedding: bool = conn
            .prepare("SELECT embedding FROM knowledge LIMIT 0")
            .is_ok();
        if !has_embedding {
            conn.execute_batch(
                "ALTER TABLE knowledge ADD COLUMN embedding BLOB;"
            )?;
            tracing::info!("Migration: added embedding column to knowledge table");
        }

        // Add llm_summary, embedding_provider, embedding to conversations for semantic search
        if !conv_columns.iter().any(|c| c == "llm_summary") {
            conn.execute_batch(
                "ALTER TABLE conversations ADD COLUMN llm_summary TEXT;
                 ALTER TABLE conversations ADD COLUMN embedding_provider TEXT;
                 ALTER TABLE conversations ADD COLUMN embedding BLOB;"
            )?;
            tracing::info!("Migration: added llm_summary/embedding_provider/embedding to conversations");
        }

        // Conversation messages FTS5 index for full-text search
        let has_conv_msg_fts: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='conversation_msg_fts'",
            [],
            |row| row.get(0),
        )?;
        if !has_conv_msg_fts {
            conn.execute_batch(
                "CREATE VIRTUAL TABLE conversation_msg_fts USING fts5(
                    content,
                    content='conversation_messages', content_rowid='id',
                    tokenize='unicode61'
                );
                INSERT INTO conversation_msg_fts(rowid, content)
                    SELECT id, content FROM conversation_messages;
                CREATE TRIGGER IF NOT EXISTS trg_conv_msg_fts_insert
                AFTER INSERT ON conversation_messages
                BEGIN
                    INSERT INTO conversation_msg_fts(rowid, content) VALUES (NEW.id, NEW.content);
                END;
                CREATE TRIGGER IF NOT EXISTS trg_conv_msg_fts_delete
                AFTER DELETE ON conversation_messages
                BEGIN
                    INSERT INTO conversation_msg_fts(conversation_msg_fts, rowid, content)
                        VALUES('delete', OLD.id, OLD.content);
                END;"
            )?;
            tracing::info!("Migration: created conversation_msg_fts with existing data backfill");
        }

        // Router chat archive — soft-delete target for cleared/deleted messages
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS router_chat_archive (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                original_id INTEGER NOT NULL,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                archived_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                archive_reason TEXT NOT NULL DEFAULT 'clear'
            );
            CREATE INDEX IF NOT EXISTS idx_rca_session ON router_chat_archive(session_id);
            CREATE INDEX IF NOT EXISTS idx_rca_archived ON router_chat_archive(archived_at);"
        )?;

        // Token usage ledger — append-only event stream for cost analysis
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS token_usage_ledger (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                slot_id TEXT,
                slot_task_id TEXT,
                conversation_id TEXT NOT NULL,
                model TEXT,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_token_ledger_conv ON token_usage_ledger(conversation_id);
            CREATE INDEX IF NOT EXISTS idx_token_ledger_slot ON token_usage_ledger(slot_id);
            CREATE INDEX IF NOT EXISTS idx_token_ledger_created ON token_usage_ledger(created_at);"
        )?;

        // Skill self-management tables (CQRS: DB as SoT, SKILL.md as materialized view)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS skill_topics (
                topic TEXT PRIMARY KEY,
                description TEXT,
                aka TEXT,
                allowed_tools TEXT,
                file_path TEXT NOT NULL,
                hit_count INTEGER DEFAULT 0,
                last_hit_at TEXT,
                fragment_count INTEGER DEFAULT 0,
                total_lines INTEGER DEFAULT 0,
                checksum TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS skill_blocks (
                id TEXT PRIMARY KEY,
                topic TEXT NOT NULL,
                block_type TEXT NOT NULL,
                title TEXT,
                content TEXT NOT NULL,
                sort_order INTEGER DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_skill_blocks_topic ON skill_blocks(topic);
            CREATE INDEX IF NOT EXISTS idx_skill_blocks_status ON skill_blocks(status);"
        )?;

        // Skill FTS5 index for full-text search
        let has_skill_fts: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='skill_fts'",
            [],
            |row| row.get(0),
        )?;
        if !has_skill_fts {
            conn.execute_batch(
                "CREATE VIRTUAL TABLE skill_fts USING fts5(
                    topic, title, content,
                    content='skill_blocks', content_rowid='rowid'
                );"
            )?;
        }

        // Phase 2: Add requires_json column to skill_topics for dependency declarations
        let skill_columns: Vec<String> = conn
            .prepare("PRAGMA table_info(skill_topics)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();
        if !skill_columns.iter().any(|c| c == "requires_json") {
            conn.execute_batch(
                "ALTER TABLE skill_topics ADD COLUMN requires_json TEXT;"
            )?;
        }
        // Phase 3: Add actions_json column for executable skill action declarations
        if !skill_columns.iter().any(|c| c == "actions_json") {
            conn.execute_batch(
                "ALTER TABLE skill_topics ADD COLUMN actions_json TEXT;"
            )?;
        }

        // Phase 3: Skill workflow execution log
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS skill_executions (
                id TEXT PRIMARY KEY,
                skill_topic TEXT NOT NULL,
                action_id TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'running',
                steps_total INTEGER NOT NULL,
                steps_completed INTEGER DEFAULT 0,
                context_json TEXT,
                error TEXT,
                triggered_by TEXT DEFAULT 'manual',
                created_at TEXT NOT NULL,
                completed_at TEXT
            )"
        )?;

        // Phase 4: Add context_hooks_json column to skill_topics
        if !skill_columns.iter().any(|c| c == "context_hooks_json") {
            conn.execute_batch(
                "ALTER TABLE skill_topics ADD COLUMN context_hooks_json TEXT;"
            )?;
        }

        // Phase 4: Add duration_ms to skill_executions
        {
            let exec_columns: Vec<String> = conn
                .prepare("PRAGMA table_info(skill_executions)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .collect();
            if !exec_columns.iter().any(|c| c == "duration_ms") {
                conn.execute_batch(
                    "ALTER TABLE skill_executions ADD COLUMN duration_ms INTEGER;"
                )?;
            }
        }

        // Phase 4: Skill version snapshots (for rollback)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS skill_versions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                topic TEXT NOT NULL,
                content TEXT NOT NULL,
                checksum TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_skill_ver_topic ON skill_versions(topic);"
        )?;

        // KB operation queue — persists kb_analyze consolidation plans for cross-session execution
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS kb_operation_queue (
                id TEXT PRIMARY KEY,
                plan_id TEXT NOT NULL,
                task_id TEXT,
                operation TEXT NOT NULL,
                target_keys TEXT NOT NULL,
                rationale TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                priority INTEGER NOT NULL DEFAULT 0,
                result TEXT,
                created_at TEXT NOT NULL,
                executed_at TEXT,
                error TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_kb_op_status ON kb_operation_queue(status);
            CREATE INDEX IF NOT EXISTS idx_kb_op_plan ON kb_operation_queue(plan_id);"
        )?;

        // Add notified_at to tasks table for hook-based completion notification
        let has_notified_at: bool = conn
            .prepare("SELECT notified_at FROM tasks LIMIT 0")
            .is_ok();
        if !has_notified_at {
            conn.execute_batch(
                "ALTER TABLE tasks ADD COLUMN notified_at INTEGER;"
            )?;
        }

        // Conversation events — non-dialog system events from JSONL
        // (turn_duration, compact_boundary, hook_progress, queue_operation, file_history_snapshot)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS conversation_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                content TEXT,
                raw_data TEXT,
                timestamp TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES conversations(id)
            );
            CREATE INDEX IF NOT EXISTS idx_conv_event_session ON conversation_events(session_id);
            CREATE INDEX IF NOT EXISTS idx_conv_event_type ON conversation_events(event_type);"
        )?;

        // Conversation tool calls — structured extraction of tool_use/tool_result pairs
        // for audit trail (Summary-to-Drilldown architecture)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS conversation_tool_calls (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                message_id INTEGER,
                tool_name TEXT NOT NULL,
                input_summary TEXT,
                raw_input TEXT,
                output_summary TEXT,
                raw_output TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                duration_ms INTEGER,
                timestamp TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES conversations(id)
            );
            CREATE INDEX IF NOT EXISTS idx_tc_session ON conversation_tool_calls(session_id);
            CREATE INDEX IF NOT EXISTS idx_tc_name ON conversation_tool_calls(tool_name);
            CREATE INDEX IF NOT EXISTS idx_tc_status ON conversation_tool_calls(status);"
        )?;

        // Decision Engine: add target/options/decision_type/retry_count to agent_questions
        let aq_columns: Vec<String> = conn
            .prepare("PRAGMA table_info(agent_questions)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();
        if !aq_columns.iter().any(|c| c == "target") {
            conn.execute_batch(
                "ALTER TABLE agent_questions ADD COLUMN target TEXT NOT NULL DEFAULT 'user';
                 ALTER TABLE agent_questions ADD COLUMN options TEXT;
                 ALTER TABLE agent_questions ADD COLUMN decision_type TEXT NOT NULL DEFAULT 'implementation';
                 ALTER TABLE agent_questions ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0;
                 CREATE INDEX IF NOT EXISTS idx_agent_questions_target ON agent_questions(target, status);"
            )?;
        }

        // Decision Engine Phase 2: add routing_trace to agent_questions
        if !aq_columns.iter().any(|c| c == "routing_trace") {
            conn.execute_batch(
                "ALTER TABLE agent_questions ADD COLUMN routing_trace TEXT;"
            )?;
        }

        // AIOps: incidents table for proactive monitoring
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS incidents (
                id TEXT PRIMARY KEY,
                severity TEXT NOT NULL,
                source TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                server_id TEXT,
                raw_payload TEXT,
                board_task_id TEXT,
                dedupe_key TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_incidents_created ON incidents(created_at);
            CREATE INDEX IF NOT EXISTS idx_incidents_dedupe ON incidents(dedupe_key, created_at);"
        )?;

        // Prompt snapshots for Skill auto-verification replay
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS prompt_snapshots (
                task_id TEXT PRIMARY KEY,
                prompt TEXT NOT NULL,
                cited_kb_ids TEXT NOT NULL DEFAULT '[]',
                category TEXT NOT NULL DEFAULT 'other',
                task_outcome TEXT,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_prompt_snap_cat ON prompt_snapshots(category);"
        )?;

        // Unified Embedding upgrade: add embedding_provider to KB + clear stale 512d vectors
        {
            let kb_columns: Vec<String> = conn
                .prepare("PRAGMA table_info(knowledge)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .collect();
            if !kb_columns.iter().any(|c| c == "embedding_provider") {
                conn.execute_batch(
                    "ALTER TABLE knowledge ADD COLUMN embedding_provider TEXT;
                     UPDATE knowledge SET embedding = NULL;"
                )?;
                tracing::info!("Migration: KB embedding_provider column added, stale 512d vectors cleared");
            }
        }

        // Unified Embedding upgrade: add embedding + embedding_provider to skill_topics
        {
            let st_columns: Vec<String> = conn
                .prepare("PRAGMA table_info(skill_topics)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .collect();
            if !st_columns.iter().any(|c| c == "embedding") {
                conn.execute_batch(
                    "ALTER TABLE skill_topics ADD COLUMN embedding BLOB;
                     ALTER TABLE skill_topics ADD COLUMN embedding_provider TEXT;"
                )?;
                tracing::info!("Migration: skill_topics embedding columns added");
            }
        }

        // Session Timeline: add session_timeline + timeline_built_at for compaction fragment merging
        if !conv_columns.iter().any(|c| c == "session_timeline") {
            conn.execute_batch(
                "ALTER TABLE conversations ADD COLUMN session_timeline TEXT;
                 ALTER TABLE conversations ADD COLUMN timeline_built_at TEXT;"
            )?;
            tracing::info!("Migration: added session_timeline + timeline_built_at to conversations");
        }

        // Daemon state KV: persist timestamps/flags that must survive daemon restarts
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS daemon_state (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );"
        )?;

        // Phase 6: System Timeline — persistent event log with global monotonic seq
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS system_timeline (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                trace_id TEXT,
                span_id TEXT,
                parent_span_id TEXT,
                event_type TEXT NOT NULL,
                summary TEXT,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_tl_created ON system_timeline(created_at);
            CREATE INDEX IF NOT EXISTS idx_tl_type ON system_timeline(event_type);
            CREATE INDEX IF NOT EXISTS idx_tl_trace ON system_timeline(trace_id);
            CREATE INDEX IF NOT EXISTS idx_tl_parent ON system_timeline(parent_span_id);"
        )?;

        // Message translations — side table for async translated content (e.g. thinking → Chinese)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS message_translations (
                message_id INTEGER PRIMARY KEY,
                translation TEXT NOT NULL,
                source_lang TEXT NOT NULL DEFAULT 'en',
                target_lang TEXT NOT NULL DEFAULT 'zh',
                model TEXT,
                duration_ms INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );"
        )?;

        // Gemini request log: persistent instrumentation for all LLM calls
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS gemini_requests (
                id TEXT PRIMARY KEY,
                caller TEXT NOT NULL,
                session_id TEXT,
                api_mode TEXT NOT NULL,
                model TEXT NOT NULL,
                prompt_chars INTEGER,
                response_chars INTEGER,
                queue_wait_ms INTEGER,
                duration_ms INTEGER,
                retry_count INTEGER DEFAULT 0,
                status TEXT NOT NULL,
                error_msg TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_gemini_req_created ON gemini_requests(created_at);
            CREATE INDEX IF NOT EXISTS idx_gemini_req_caller ON gemini_requests(caller);
            CREATE INDEX IF NOT EXISTS idx_gemini_req_session ON gemini_requests(session_id);"
        )?;

        // ── Conversation Topic Vectors (multi-topic MaxSim search) ──
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS conversation_topic_vectors (
                session_id TEXT NOT NULL,
                chunk_idx INTEGER NOT NULL,
                topic TEXT NOT NULL,
                embedding BLOB NOT NULL,
                embedding_provider TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (session_id, chunk_idx)
            );
            CREATE INDEX IF NOT EXISTS idx_ctv_provider ON conversation_topic_vectors(embedding_provider);"
        )?;

        // Gemini requests: add prompt_text/response_text for full content storage
        {
            let gr_columns: Vec<String> = conn
                .prepare("PRAGMA table_info(gemini_requests)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .collect();
            if !gr_columns.iter().any(|c| c == "prompt_text") {
                conn.execute_batch(
                    "ALTER TABLE gemini_requests ADD COLUMN prompt_text TEXT;
                     ALTER TABLE gemini_requests ADD COLUMN response_text TEXT;"
                )?;
                tracing::info!("Migration: added prompt_text/response_text to gemini_requests");
            }
        }

        // ── Image Descriptions (Vision Worker cache) ──
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS image_descriptions (
                image_hash TEXT PRIMARY KEY,
                media_type TEXT NOT NULL,
                description TEXT NOT NULL,
                char_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );"
        )?;

        // ── Holographic Context Engine: AST code structure nodes ──
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ast_nodes (
                id TEXT PRIMARY KEY,
                repo TEXT NOT NULL,
                file_path TEXT NOT NULL,
                node_type TEXT NOT NULL,
                name TEXT NOT NULL,
                signature TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                is_exported INTEGER DEFAULT 0,
                docstring TEXT,
                stub_content TEXT NOT NULL,
                calls TEXT,
                embedding BLOB,
                embedding_provider TEXT,
                updated_at TEXT DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_ast_repo_file ON ast_nodes(repo, file_path);
            CREATE INDEX IF NOT EXISTS idx_ast_name ON ast_nodes(name);
            CREATE INDEX IF NOT EXISTS idx_ast_node_type ON ast_nodes(node_type);
            CREATE INDEX IF NOT EXISTS idx_ast_exported ON ast_nodes(is_exported);
            CREATE INDEX IF NOT EXISTS idx_ast_provider ON ast_nodes(embedding_provider);"
        )?;

        // AST nodes FTS5 for full-text search
        let has_ast_fts: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='ast_nodes_fts'",
            [],
            |row| row.get(0),
        )?;
        if !has_ast_fts {
            conn.execute_batch(
                "CREATE VIRTUAL TABLE ast_nodes_fts USING fts5(
                    name, signature, docstring, stub_content,
                    content='ast_nodes', content_rowid='rowid'
                );"
            )?;
        }

        // AST file metadata — incremental sync cursor
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ast_file_meta (
                repo TEXT NOT NULL,
                file_path TEXT NOT NULL,
                commit_hash TEXT NOT NULL,
                node_count INTEGER DEFAULT 0,
                updated_at TEXT DEFAULT (datetime('now')),
                PRIMARY KEY (repo, file_path)
            );"
        )?;

        // ── Holographic Beacon: feature-level code annotations (P4) ──
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS beacons (
                id TEXT PRIMARY KEY,
                name TEXT UNIQUE NOT NULL,
                description TEXT,
                created_at TEXT DEFAULT (datetime('now')),
                updated_at TEXT DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS beacon_nodes (
                beacon_id TEXT NOT NULL,
                repo TEXT NOT NULL,
                file_path TEXT NOT NULL,
                symbol_name TEXT NOT NULL,
                annotation TEXT,
                PRIMARY KEY (beacon_id, repo, file_path, symbol_name),
                FOREIGN KEY (beacon_id) REFERENCES beacons(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_bn_symbol ON beacon_nodes(symbol_name);
            CREATE INDEX IF NOT EXISTS idx_bn_beacon ON beacon_nodes(beacon_id);"
        )?;

        // Message narrations — GPT-5.4 generated step-by-step explanations
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS message_narrations (
                message_id INTEGER PRIMARY KEY,
                session_id TEXT NOT NULL,
                step_title TEXT NOT NULL,
                step_intent TEXT NOT NULL,
                step_result TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_narrations_session ON message_narrations(session_id);"
        )?;

        // Narration cursor — tracks batch progress per session (断点续传)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS narration_cursors (
                session_id TEXT PRIMARY KEY,
                last_processed_id INTEGER NOT NULL DEFAULT 0,
                batch_index INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'idle',
                retry_count INTEGER NOT NULL DEFAULT 0,
                total_messages INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );"
        )?;

        // ── Retrospective Results — session performance analysis ──
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS retrospective_results (
                session_id TEXT PRIMARY KEY,
                trigger_reason TEXT NOT NULL,
                quick_stats TEXT NOT NULL,
                full_analysis TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_retro_created ON retrospective_results(created_at);"
        )?;

        // ── Add tool_name column to conversation_messages ──
        // Stores comma-separated tool names extracted from raw_content for searchability.
        let has_tool_name: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('conversation_messages') WHERE name = 'tool_name'",
            [],
            |row| row.get(0),
        )?;
        if !has_tool_name {
            conn.execute_batch("ALTER TABLE conversation_messages ADD COLUMN tool_name TEXT")?;

            // Backfill: extract tool names from raw_content JSON
            let mut stmt = conn.prepare(
                "SELECT id, raw_content FROM conversation_messages WHERE raw_content IS NOT NULL"
            )?;
            let rows: Vec<(i64, String)> = stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?.filter_map(|r| r.ok()).collect();

            let mut update_stmt = conn.prepare(
                "UPDATE conversation_messages SET tool_name = ?1 WHERE id = ?2"
            )?;
            let mut backfilled = 0usize;
            for (id, raw) in &rows {
                if let Some(tool_names) = extract_tool_names_from_raw(raw) {
                    update_stmt.execute(rusqlite::params![tool_names, id])?;
                    backfilled += 1;
                }
            }
            if backfilled > 0 {
                tracing::info!(backfilled, "Migration: backfilled tool_name for conversation_messages");
            }

            // Index for tool_name filtering
            conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_conv_msg_tool_name ON conversation_messages(tool_name)"
            )?;
            tracing::info!("Migration: added tool_name column to conversation_messages");
        }

        // ── Timeline FTS5 — full-text search for system_timeline ──
        let has_timeline_fts: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='system_timeline_fts'",
            [],
            |row| row.get(0),
        )?;
        if !has_timeline_fts {
            conn.execute_batch(
                "CREATE VIRTUAL TABLE system_timeline_fts USING fts5(
                    summary, payload,
                    event_type UNINDEXED,
                    content='system_timeline', content_rowid='seq',
                    tokenize='unicode61'
                );

                -- Populate from existing data
                INSERT INTO system_timeline_fts(rowid, summary, payload, event_type)
                    SELECT seq, COALESCE(summary, ''), payload, event_type FROM system_timeline;

                -- Auto-sync triggers
                CREATE TRIGGER IF NOT EXISTS trg_timeline_fts_insert
                AFTER INSERT ON system_timeline BEGIN
                    INSERT INTO system_timeline_fts(rowid, summary, payload, event_type)
                    VALUES (NEW.seq, COALESCE(NEW.summary, ''), NEW.payload, NEW.event_type);
                END;

                CREATE TRIGGER IF NOT EXISTS trg_timeline_fts_delete
                AFTER DELETE ON system_timeline BEGIN
                    INSERT INTO system_timeline_fts(system_timeline_fts, rowid, summary, payload, event_type)
                    VALUES ('delete', OLD.seq, OLD.summary, OLD.payload, OLD.event_type);
                END;

                CREATE TRIGGER IF NOT EXISTS trg_timeline_fts_update
                AFTER UPDATE ON system_timeline BEGIN
                    INSERT INTO system_timeline_fts(system_timeline_fts, rowid, summary, payload, event_type)
                    VALUES ('delete', OLD.seq, OLD.summary, OLD.payload, OLD.event_type);
                    INSERT INTO system_timeline_fts(rowid, summary, payload, event_type)
                    VALUES (NEW.seq, COALESCE(NEW.summary, ''), NEW.payload, NEW.event_type);
                END;"
            )?;
            tracing::info!("Migration: created system_timeline_fts with auto-sync triggers");
        }

        // Phase E: beacon harvest_count (self-evolution: skill synthesis frequency tracking)
        {
            let cols: Vec<String> = conn
                .prepare("PRAGMA table_info(beacons)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .collect();
            if !cols.iter().any(|c| c == "harvest_count") {
                conn.execute_batch(
                    "ALTER TABLE beacons ADD COLUMN harvest_count INTEGER DEFAULT 0;"
                )?;
                tracing::info!("Migration: added harvest_count to beacons");
            }
        }

        // Phase E: exit_code on conversations (self-evolution: ground truth capture)
        {
            let cols: Vec<String> = conn
                .prepare("PRAGMA table_info(conversations)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .collect();
            if !cols.iter().any(|c| c == "exit_code") {
                conn.execute_batch(
                    "ALTER TABLE conversations ADD COLUMN exit_code INTEGER;"
                )?;
                tracing::info!("Migration: added exit_code to conversations");
            }
        }

        // Historical habit scan watermark — tracks which conversations have been
        // scanned for user operation habits (workflow patterns, style, corrections).
        {
            let cols: Vec<String> = conn
                .prepare("PRAGMA table_info(conversations)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .collect();
            if !cols.iter().any(|c| c == "habit_scanned_at") {
                conn.execute_batch(
                    "ALTER TABLE conversations ADD COLUMN habit_scanned_at TEXT;"
                )?;
                tracing::info!("Migration: added habit_scanned_at to conversations");
            }
        }

        // Knowledge Graph: directed edges between KB entries for multi-hop reasoning
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS knowledge_edges (
                source_id TEXT NOT NULL,
                target_id TEXT NOT NULL,
                relation_type TEXT NOT NULL,
                weight REAL NOT NULL DEFAULT 1.0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (source_id, target_id, relation_type)
            );
            CREATE INDEX IF NOT EXISTS idx_kb_edge_source ON knowledge_edges(source_id);
            CREATE INDEX IF NOT EXISTS idx_kb_edge_target ON knowledge_edges(target_id);"
        )?;

        // KB co-access log: tracks which entries are retrieved together
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS kb_access_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kb_id TEXT NOT NULL,
                co_accessed_ids TEXT NOT NULL,
                context_source TEXT NOT NULL DEFAULT 'prefetch',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_kb_access_log_kb ON kb_access_log(kb_id);
            CREATE INDEX IF NOT EXISTS idx_kb_access_log_created ON kb_access_log(created_at);"
        )?;

        // Dynamic slots: Jarvis-created ephemeral compute slots with TTL
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS dynamic_slots (
                id TEXT PRIMARY KEY,
                parent_slot_id TEXT NOT NULL,
                template TEXT NOT NULL,
                objective TEXT,
                config TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                termination_reason TEXT,
                created_at TEXT NOT NULL,
                terminated_at TEXT,
                ttl_seconds INTEGER NOT NULL DEFAULT 14400,
                expires_at TEXT NOT NULL,
                extend_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_dynamic_slots_active ON dynamic_slots(status, expires_at);"
        )?;

        // Phase 2: KB-AST Graph Unification — bipartite links between KB entries and AST code nodes
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS kb_ast_links (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kb_id TEXT NOT NULL,
                ast_node_id TEXT,
                symbol_name TEXT NOT NULL,
                file_path TEXT,
                relation TEXT NOT NULL DEFAULT 'related_to',
                confidence REAL DEFAULT 0.8,
                created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_kb_ast_kb ON kb_ast_links(kb_id);
            CREATE INDEX IF NOT EXISTS idx_kb_ast_symbol ON kb_ast_links(symbol_name);
            CREATE INDEX IF NOT EXISTS idx_kb_ast_file ON kb_ast_links(file_path);
            CREATE INDEX IF NOT EXISTS idx_kb_ast_node ON kb_ast_links(ast_node_id);"
        )?;

        // Utility Score: Darwin GC — add utility_score to knowledge table
        {
            let kb_columns: Vec<String> = conn
                .prepare("PRAGMA table_info(knowledge)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .collect();
            if !kb_columns.iter().any(|c| c == "utility_score") {
                conn.execute_batch(
                    "ALTER TABLE knowledge ADD COLUMN utility_score REAL NOT NULL DEFAULT 0.5;"
                )?;
                // Migrate existing data: access_count → initial utility_score
                conn.execute_batch(
                    "UPDATE knowledge SET utility_score = MIN(1.0, 0.5 + (access_count * 0.05))
                     WHERE access_count > 0;
                     UPDATE knowledge SET utility_score = MAX(utility_score, 0.7)
                     WHERE category IN ('preference', 'policy:decision', 'memory:architecture', 'memory:decision');
                     CREATE INDEX IF NOT EXISTS idx_kb_utility ON knowledge(utility_score);"
                )?;
                tracing::info!("Migration: knowledge.utility_score column added, existing data migrated");
            }
        }

        // Phase 5.3: board_tasks — timeout_secs + context_intent
        {
            let bt_columns: Vec<String> = conn
                .prepare("PRAGMA table_info(board_tasks)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .collect();
            if !bt_columns.iter().any(|c| c == "timeout_secs") {
                conn.execute_batch(
                    "ALTER TABLE board_tasks ADD COLUMN timeout_secs INTEGER;
                     ALTER TABLE board_tasks ADD COLUMN context_intent TEXT;"
                )?;
                tracing::info!("Migration: board_tasks.timeout_secs + context_intent columns added");
            }
        }

        // Phase 6: TaskId integrity — backfill short parent_ids → full UUIDs.
        // Short IDs (< 36 chars) stored as parent_id are resolved to the full UUID
        // of the matching board_task. Orphans (no match) are NULLed out.
        {
            let mut stmt = conn.prepare(
                "SELECT id, parent_id FROM board_tasks WHERE parent_id IS NOT NULL AND LENGTH(parent_id) < 36"
            )?;
            let short_parents: Vec<(String, String)> = stmt
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            if !short_parents.is_empty() {
                let mut fixed = 0usize;
                let mut orphaned = 0usize;
                for (task_id, short_pid) in &short_parents {
                    // Prefix match: find unique full UUID matching the short ID
                    let prefix = format!("{}%", short_pid);
                    let mut match_stmt = conn.prepare("SELECT id FROM board_tasks WHERE id LIKE ?1")?;
                    let matches: Vec<String> = match_stmt
                        .query_map(rusqlite::params![prefix], |row| row.get(0))?
                        .filter_map(|r| r.ok())
                        .collect();
                    if matches.len() == 1 {
                        conn.execute(
                            "UPDATE board_tasks SET parent_id = ?1 WHERE id = ?2",
                            rusqlite::params![matches[0], task_id],
                        )?;
                        fixed += 1;
                    } else {
                        // No unique match → orphan, NULL out to prevent FK violation
                        conn.execute(
                            "UPDATE board_tasks SET parent_id = NULL WHERE id = ?1",
                            rusqlite::params![task_id],
                        )?;
                        orphaned += 1;
                    }
                }
                tracing::info!(fixed, orphaned, "Migration: resolved short parent_ids in board_tasks");
            }
        }

        // ── Backfill progress tracking tables ──
        {
            let has_table: bool = conn.query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='backfill_progress'",
                [], |row| row.get(0),
            )?;
            if !has_table {
                conn.execute_batch("
                    CREATE TABLE backfill_progress (
                        phase TEXT PRIMARY KEY,
                        status TEXT NOT NULL DEFAULT 'pending',
                        last_cursor INTEGER DEFAULT 0,
                        total_estimated INTEGER DEFAULT 0,
                        processed INTEGER DEFAULT 0,
                        failed INTEGER DEFAULT 0,
                        started_at TEXT,
                        completed_at TEXT,
                        updated_at TEXT DEFAULT (datetime('now'))
                    );
                    CREATE TABLE backfill_failures (
                        session_id TEXT NOT NULL,
                        phase TEXT NOT NULL,
                        retry_count INTEGER DEFAULT 1,
                        last_error TEXT,
                        updated_at TEXT DEFAULT (datetime('now')),
                        PRIMARY KEY (session_id, phase)
                    );
                ")?;
                tracing::info!("Migration: created backfill_progress and backfill_failures tables");
            }
        }

        // Phase 4b: kb_access_log — add session_id for precise session-level feedback
        {
            let al_columns: Vec<String> = conn
                .prepare("PRAGMA table_info(kb_access_log)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .collect();
            if !al_columns.iter().any(|c| c == "session_id") {
                conn.execute_batch(
                    "ALTER TABLE kb_access_log ADD COLUMN session_id TEXT;
                     CREATE INDEX IF NOT EXISTS idx_kb_access_log_session ON kb_access_log(session_id);"
                )?;
                tracing::info!("Migration: kb_access_log.session_id column added");
            }
        }

        // Phase 4c: knowledge — add needs_re_extraction flag for weekly LLM reflection
        {
            let kb_columns: Vec<String> = conn
                .prepare("PRAGMA table_info(knowledge)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .collect();
            if !kb_columns.iter().any(|c| c == "needs_re_extraction") {
                conn.execute_batch(
                    "ALTER TABLE knowledge ADD COLUMN needs_re_extraction INTEGER NOT NULL DEFAULT 0;"
                )?;
                tracing::info!("Migration: knowledge.needs_re_extraction column added");
            }
        }

        // ── P0: Three-Layer Conversation Log Refactor ──
        // consumer_watermarks: decouple per-consumer cursors from conversations table (OCP)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS consumer_watermarks (
                consumer_name TEXT NOT NULL,
                session_id    TEXT NOT NULL,
                last_processed_msg_id INTEGER,
                last_processed_time   TEXT,
                extra                 TEXT,
                PRIMARY KEY (consumer_name, session_id)
            );
            CREATE INDEX IF NOT EXISTS idx_cw_consumer ON consumer_watermarks(consumer_name);"
        )?;

        // message_labels: EAV model for per-message classification tags
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS message_labels (
                message_id INTEGER NOT NULL,
                label      TEXT NOT NULL,
                value      TEXT,
                source     TEXT NOT NULL DEFAULT 'rule',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (message_id, label)
            );
            CREATE INDEX IF NOT EXISTS idx_msg_label ON message_labels(label, value);
            CREATE INDEX IF NOT EXISTS idx_msg_label_mid ON message_labels(message_id);"
        )?;

        // Extend conversation_messages with storage layer metadata
        {
            let msg_columns: Vec<String> = conn
                .prepare("PRAGMA table_info(conversation_messages)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .collect();
            if !msg_columns.iter().any(|c| c == "raw_role") {
                conn.execute_batch(
                    "ALTER TABLE conversation_messages ADD COLUMN raw_role TEXT;
                     ALTER TABLE conversation_messages ADD COLUMN content_types TEXT;
                     ALTER TABLE conversation_messages ADD COLUMN has_image INTEGER DEFAULT 0;
                     ALTER TABLE conversation_messages ADD COLUMN has_tool_use INTEGER DEFAULT 0;
                     ALTER TABLE conversation_messages ADD COLUMN has_tool_result INTEGER DEFAULT 0;
                     ALTER TABLE conversation_messages ADD COLUMN token_count INTEGER;"
                )?;
                tracing::info!("Migration: conversation_messages storage-layer columns added (raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count)");
            }
        }

        // NOTE: Watermark migration from conversations → consumer_watermarks is deferred to P3.
        // Migrating now would snapshot stale data; consumers still write to conversations table.
        // When each consumer is migrated to use consumer_watermarks, it should do a fresh sync.

        // Event dedup: add event_uuid column + UNIQUE index to conversation_events.
        // Previously events had NO dedup mechanism and were duplicated on every daemon restart.
        {
            let ev_columns: Vec<String> = conn
                .prepare("PRAGMA table_info(conversation_events)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .collect();

            if !ev_columns.iter().any(|c| c == "event_uuid") {
                // 1. Add event_uuid column
                conn.execute_batch(
                    "ALTER TABLE conversation_events ADD COLUMN event_uuid TEXT;"
                )?;

                // 2. Backfill event_uuid from raw_data JSON for existing rows (before dedup)
                let mut stmt = conn.prepare(
                    "SELECT id, raw_data FROM conversation_events WHERE event_uuid IS NULL AND raw_data IS NOT NULL"
                )?;
                let rows: Vec<(i64, String)> = stmt.query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?.filter_map(|r| r.ok()).collect();
                drop(stmt);

                let mut filled = 0usize;
                for (id, raw) in &rows {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) {
                        // progress/system events have top-level "uuid"
                        // file-history-snapshot has "messageId"
                        let uuid = val.get("uuid").and_then(|v| v.as_str())
                            .or_else(|| val.get("messageId").and_then(|v| v.as_str()));
                        if let Some(u) = uuid {
                            conn.execute(
                                "UPDATE conversation_events SET event_uuid = ?1 WHERE id = ?2",
                                params![u, id],
                            )?;
                            filled += 1;
                        }
                    }
                }

                // 3. Deduplicate: keep min(id) per (session_id, event_uuid) for rows with UUID,
                //    and per (session_id, event_type, content, timestamp) for rows without.
                let deleted_uuid: usize = conn.execute(
                    "DELETE FROM conversation_events WHERE event_uuid IS NOT NULL AND id NOT IN (
                        SELECT MIN(id) FROM conversation_events
                        WHERE event_uuid IS NOT NULL
                        GROUP BY session_id, event_uuid
                    )",
                    [],
                )?;
                let deleted_no_uuid: usize = conn.execute(
                    "DELETE FROM conversation_events WHERE event_uuid IS NULL AND id NOT IN (
                        SELECT MIN(id) FROM conversation_events
                        WHERE event_uuid IS NULL
                        GROUP BY session_id, event_type, COALESCE(content, ''), timestamp
                    )",
                    [],
                )?;
                let deleted = deleted_uuid + deleted_no_uuid;

                // 4. Create UNIQUE index on (session_id, event_uuid) — same UUID can appear
                //    in different sessions (e.g., agent sub-sessions sharing parent hook events)
                conn.execute_batch(
                    "CREATE UNIQUE INDEX IF NOT EXISTS idx_conv_event_uuid
                     ON conversation_events(session_id, event_uuid) WHERE event_uuid IS NOT NULL;"
                )?;

                tracing::info!(
                    deleted_duplicates = deleted,
                    uuid_backfilled = filled,
                    "Migration: conversation_events dedup — added event_uuid column, purged duplicates, created UNIQUE index"
                );
            }
        }

        Ok(())
    }
}

/// Extract tool names from raw_content JSON (content blocks array).
/// Returns comma-separated tool names or None if no tool_use blocks found.
fn extract_tool_names_from_raw(raw_content: &str) -> Option<String> {
    let content: serde_json::Value = serde_json::from_str(raw_content).ok()?;
    let names: Vec<String> = content.as_array()?
        .iter()
        .filter_map(|block| {
            if block.get("type")?.as_str()? == "tool_use" {
                block.get("name")?.as_str().map(String::from)
            } else {
                None
            }
        })
        .collect();
    if names.is_empty() { None } else { Some(names.join(",")) }
}
