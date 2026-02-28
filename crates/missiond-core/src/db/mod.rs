//! SQLite database operations for missiond
//!
//! Mirrors the TypeScript implementation in packages/missiond/src/db/index.ts

use rusqlite::{params, Connection, OptionalExtension, Result as SqliteResult};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use std::collections::HashSet;

use crate::types::{
    AddBoardTaskNoteInput, AgentQuestion, AgentQuestionStatus, BoardNoteType, BoardTask,
    BoardTaskNote, BoardTaskStatus, BoardTaskWithNotes, Conversation, ConversationMessage,
    CreateAgentQuestionInput, CreateBoardTaskInput, EventType, InboxMessage, KBOperation,
    KBOperationRow, KBRememberInput, KBRememberResult, KnowledgeEntry, SkillBlock,
    SkillSearchResult, SkillTopic, SlotTask, Task, TaskEvent, TaskStatus, TaskUpdate,
    ToolCallRecord, UpdateBoardTaskInput,
};

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
  claimed_at TEXT
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

-- Agent questions (pending decisions for user)
CREATE TABLE IF NOT EXISTS agent_questions (
    id TEXT PRIMARY KEY,
    task_id TEXT,
    slot_id TEXT,
    session_id TEXT,
    question TEXT NOT NULL,
    context TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    answer TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_questions_status ON agent_questions(status);

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
    analyzed_at TEXT
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
"#;

/// Extract parent session ID from a subagent's jsonl_path.
/// Path pattern: .../PARENT_SESSION_ID/subagents/agent-xxx.jsonl
pub fn extract_parent_session_id(jsonl_path: &str) -> Option<String> {
    let path = std::path::Path::new(jsonl_path);
    let parent = path.parent()?; // .../PARENT_SESSION_ID/subagents/
    if parent.file_name()?.to_str()? != "subagents" {
        return None;
    }
    let grandparent = parent.parent()?; // .../PARENT_SESSION_ID/
    let session_id = grandparent.file_name()?.to_str()?.to_string();
    // Sanity: parent session ID should look like a UUID
    if session_id.contains('-') && session_id.len() > 8 {
        Some(session_id)
    } else {
        None
    }
}

/// Derive conversation_type from slot_id and session_id.
/// "meta" = memory slots, "subagent" = agent-* IDs, "worker" = other slots, "user" = direct CLI.
pub fn derive_conversation_type(slot_id: Option<&str>, session_id: &str) -> String {
    match slot_id {
        Some("slot-memory" | "slot-memory-slow") => "meta".to_string(),
        Some(_) => "worker".to_string(),
        None if session_id.starts_with("agent-") => "subagent".to_string(),
        None => "user".to_string(),
    }
}

/// SQLite database operations class
pub struct MissionDB {
    conn: std::sync::Mutex<Connection>,
    /// Read-only connection for queries — avoids blocking on write Mutex (WAL concurrent reads)
    read_conn: std::sync::Mutex<Connection>,
    /// Dirty flag: set after kb_forget, cleared after FTS rebuild
    fts_dirty: AtomicBool,
}

impl MissionDB {
    /// Create a new database connection
    pub fn new<P: AsRef<Path>>(db_path: P) -> SqliteResult<Self> {
        let conn = Connection::open(&db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // Separate read-only connection for queries — WAL allows concurrent reads during writes
        let read_conn = Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        read_conn.busy_timeout(std::time::Duration::from_secs(2))?;
        let db = Self {
            conn: std::sync::Mutex::new(conn),
            read_conn: std::sync::Mutex::new(read_conn),
            fts_dirty: AtomicBool::new(false),
        };
        db.init()?;
        Ok(db)
    }

    /// Alias for new - opens a database file
    pub fn open<P: AsRef<Path>>(db_path: P) -> SqliteResult<Self> {
        Self::new(db_path)
    }

    /// Close the database connections
    pub fn close(self) {
        drop(self.read_conn);
        drop(self.conn);
    }

    /// Create an in-memory database (for testing)
    pub fn in_memory() -> SqliteResult<Self> {
        let conn = Connection::open_in_memory()?;
        let read_conn = Connection::open_in_memory()?;
        let db = Self {
            conn: std::sync::Mutex::new(conn),
            read_conn: std::sync::Mutex::new(read_conn),
            fts_dirty: AtomicBool::new(false),
        };
        db.init()?;
        Ok(db)
    }

    /// Get a lock on the write database connection
    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("MissionDB mutex poisoned")
    }

    /// Get a lock on the read-only database connection (non-blocking during writes in WAL mode)
    fn read_conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.read_conn.lock().expect("MissionDB read_conn mutex poisoned")
    }

    fn init(&self) -> SqliteResult<()> {
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
    fn check_fts_integrity(&self) -> SqliteResult<()> {
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
    pub fn kb_rebuild_fts_if_dirty(&self) -> SqliteResult<bool> {
        if !self.fts_dirty.swap(false, Ordering::Relaxed) {
            return Ok(false);
        }
        let conn = self.conn();
        conn.execute_batch("INSERT INTO knowledge_fts(knowledge_fts) VALUES('rebuild')")?;
        tracing::info!("FTS5 index rebuilt (dirty flag)");
        Ok(true)
    }

    /// Run schema migrations for existing databases
    fn migrate(&self) -> SqliteResult<()> {
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

        Ok(())
    }

    // ============ Tasks ============

    /// Insert a new task
    pub fn insert_task(&self, task: &Task) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO tasks (id, role, prompt, status, slot_id, session_id, result, error, created_at, started_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                task.id,
                task.role,
                task.prompt,
                task.status.as_str(),
                task.slot_id,
                task.session_id,
                task.result,
                task.error,
                task.created_at,
                task.started_at,
                task.finished_at,
            ],
        )?;
        Ok(())
    }

    /// Update a task by ID
    pub fn update_task(&self, id: &str, update: &TaskUpdate) -> SqliteResult<()> {
        let mut fields = Vec::new();
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(status) = &update.status {
            fields.push("status = ?");
            values.push(Box::new(status.as_str().to_string()));
        }
        if let Some(slot_id) = &update.slot_id {
            fields.push("slot_id = ?");
            values.push(Box::new(slot_id.clone()));
        }
        if let Some(session_id) = &update.session_id {
            fields.push("session_id = ?");
            values.push(Box::new(session_id.clone()));
        }
        if let Some(result) = &update.result {
            fields.push("result = ?");
            values.push(Box::new(result.clone()));
        }
        if let Some(error) = &update.error {
            fields.push("error = ?");
            values.push(Box::new(error.clone()));
        }
        if let Some(started_at) = &update.started_at {
            fields.push("started_at = ?");
            values.push(Box::new(*started_at));
        }
        if let Some(finished_at) = &update.finished_at {
            fields.push("finished_at = ?");
            values.push(Box::new(*finished_at));
        }

        if fields.is_empty() {
            return Ok(());
        }

        let sql = format!("UPDATE tasks SET {} WHERE id = ?", fields.join(", "));
        values.push(Box::new(id.to_string()));

        let params: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();
        let conn = self.conn();
        conn.execute(&sql, params.as_slice())?;
        Ok(())
    }

    /// Get a task by ID
    pub fn get_task(&self, id: &str) -> SqliteResult<Option<Task>> {
        let conn = self.conn();
        // Support short ID prefix matching (like git short hashes)
        if id.len() < 36 {
            let prefix = format!("{}%", id);
            let mut stmt = conn.prepare("SELECT * FROM tasks WHERE id LIKE ? ORDER BY created_at DESC LIMIT 2")?;
            let tasks: Vec<Task> = stmt
                .query_map(params![prefix], |row| Self::row_to_task(row))?
                .filter_map(|r| r.ok())
                .collect();
            match tasks.len() {
                1 => Ok(Some(tasks.into_iter().next().unwrap())),
                _ => Ok(None), // 0 = not found, 2+ = ambiguous
            }
        } else {
            let mut stmt = conn.prepare("SELECT * FROM tasks WHERE id = ?")?;
            let mut rows = stmt.query(params![id])?;
            if let Some(row) = rows.next()? {
                Ok(Some(Self::row_to_task(row)?))
            } else {
                Ok(None)
            }
        }
    }

    /// Get all tasks by status
    pub fn get_tasks_by_status(&self, status: TaskStatus) -> SqliteResult<Vec<Task>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT * FROM tasks WHERE status = ? ORDER BY created_at ASC")?;
        let rows = stmt.query_map(params![status.as_str()], |row| Self::row_to_task(row))?;

        let mut tasks = Vec::new();
        for task in rows {
            tasks.push(task?);
        }
        Ok(tasks)
    }

    /// Get queued tasks by role
    pub fn get_queued_tasks_by_role(&self, role: &str) -> SqliteResult<Vec<Task>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM tasks WHERE status = 'queued' AND role = ? ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![role], |row| Self::row_to_task(row))?;

        let mut tasks = Vec::new();
        for task in rows {
            tasks.push(task?);
        }
        Ok(tasks)
    }

    /// Requeue running tasks assigned to a slot (e.g. after slot restart).
    /// Resets status to Queued and clears slot_id/started_at so they get re-dispatched.
    pub fn requeue_running_tasks_for_slot(&self, slot_id: &str) -> SqliteResult<usize> {
        let conn = self.conn();
        let n = conn.execute(
            "UPDATE tasks SET status = 'queued', slot_id = NULL, started_at = NULL WHERE status = 'running' AND slot_id = ?",
            params![slot_id],
        )?;
        Ok(n)
    }

    /// Get all tasks (for listing)
    pub fn get_all_tasks(&self, limit: i64) -> SqliteResult<Vec<Task>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT * FROM tasks ORDER BY created_at DESC LIMIT ?")?;
        let rows = stmt.query_map(params![limit], |row| Self::row_to_task(row))?;

        let mut tasks = Vec::new();
        for task in rows {
            tasks.push(task?);
        }
        Ok(tasks)
    }

    /// Get completed/failed tasks since a given timestamp (per-session watermark model).
    /// Each caller tracks its own watermark — no global consume, no cross-session interference.
    pub fn ack_completed_tasks(&self, since: Option<i64>) -> SqliteResult<Vec<Task>> {
        let conn = self.read_conn();
        if let Some(since_ts) = since {
            let mut stmt = conn.prepare(
                "SELECT * FROM tasks WHERE status IN ('done', 'failed') AND finished_at > ?1 ORDER BY finished_at ASC"
            )?;
            let tasks = stmt.query_map(params![since_ts], |row| Self::row_to_task(row))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(tasks)
        } else {
            // No watermark: return all completed tasks from last 1 hour (millis)
            let cutoff = chrono::Utc::now().timestamp_millis() - 3_600_000;
            let mut stmt = conn.prepare(
                "SELECT * FROM tasks WHERE status IN ('done', 'failed') AND finished_at > ?1 ORDER BY finished_at ASC"
            )?;
            let tasks = stmt.query_map(params![cutoff], |row| Self::row_to_task(row))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(tasks)
        }
    }

    fn row_to_task(row: &rusqlite::Row) -> SqliteResult<Task> {
        let status_str: String = row.get("status")?;
        let status = TaskStatus::from_str(&status_str).unwrap_or(TaskStatus::Queued);

        Ok(Task {
            id: row.get("id")?,
            role: row.get("role")?,
            prompt: row.get("prompt")?,
            status,
            slot_id: row.get("slot_id")?,
            session_id: row.get("session_id")?,
            result: row.get("result")?,
            error: row.get("error")?,
            created_at: row.get("created_at")?,
            started_at: row.get("started_at")?,
            finished_at: row.get("finished_at")?,
        })
    }

    // ============ Inbox ============

    /// Insert an inbox message
    pub fn insert_inbox_message(&self, msg: &InboxMessage) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO inbox (id, task_id, from_role, content, read, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                msg.id,
                msg.task_id,
                msg.from_role,
                msg.content,
                if msg.read { 1 } else { 0 },
                msg.created_at,
            ],
        )?;
        Ok(())
    }

    /// Get inbox messages
    pub fn get_inbox_messages(&self, unread_only: bool, limit: i64) -> SqliteResult<Vec<InboxMessage>> {
        let sql = if unread_only {
            "SELECT * FROM inbox WHERE read = 0 ORDER BY created_at DESC LIMIT ?"
        } else {
            "SELECT * FROM inbox ORDER BY created_at DESC LIMIT ?"
        };

        let conn = self.conn();
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![limit], |row| Self::row_to_inbox_message(row))?;

        let mut messages = Vec::new();
        for msg in rows {
            messages.push(msg?);
        }
        Ok(messages)
    }

    /// Mark an inbox message as read
    pub fn mark_inbox_read(&self, id: &str) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute("UPDATE inbox SET read = 1 WHERE id = ?", params![id])?;
        Ok(())
    }

    fn row_to_inbox_message(row: &rusqlite::Row) -> SqliteResult<InboxMessage> {
        let read: i32 = row.get("read")?;
        Ok(InboxMessage {
            id: row.get("id")?,
            task_id: row.get("task_id")?,
            from_role: row.get("from_role")?,
            content: row.get("content")?,
            read: read == 1,
            created_at: row.get("created_at")?,
        })
    }

    // ============ Events ============

    /// Insert an event (returns the new event ID)
    pub fn insert_event(
        &self,
        task_id: &str,
        event_type: EventType,
        data: Option<&serde_json::Value>,
        timestamp: i64,
    ) -> SqliteResult<i64> {
        let data_str = data.map(|d| serde_json::to_string(d).unwrap_or_default());

        let conn = self.conn();
        conn.execute(
            "INSERT INTO events (task_id, type, data, timestamp)
             VALUES (?1, ?2, ?3, ?4)",
            params![task_id, event_type.as_str(), data_str, timestamp],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// Get events by task ID
    pub fn get_events_by_task(&self, task_id: &str) -> SqliteResult<Vec<TaskEvent>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT * FROM events WHERE task_id = ? ORDER BY id ASC")?;
        let rows = stmt.query_map(params![task_id], |row| Self::row_to_event(row))?;

        let mut events = Vec::new();
        for event in rows {
            events.push(event?);
        }
        Ok(events)
    }

    fn row_to_event(row: &rusqlite::Row) -> SqliteResult<TaskEvent> {
        let type_str: String = row.get("type")?;
        let event_type = EventType::from_str(&type_str).unwrap_or(EventType::TaskCreated);
        let data_str: Option<String> = row.get("data")?;
        let data = data_str.and_then(|s| serde_json::from_str(&s).ok());

        Ok(TaskEvent {
            id: row.get("id")?,
            task_id: row.get("task_id")?,
            event_type,
            data,
            timestamp: row.get("timestamp")?,
        })
    }

    // ============ Slot Sessions ============

    /// Get session ID for a slot
    pub fn get_slot_session(&self, slot_id: &str) -> SqliteResult<Option<String>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT session_id FROM slot_sessions WHERE slot_id = ?")?;
        let mut rows = stmt.query(params![slot_id])?;

        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// Set session ID for a slot (upsert)
    pub fn set_slot_session(&self, slot_id: &str, session_id: &str) -> SqliteResult<()> {
        let now = chrono::Utc::now().timestamp_millis();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO slot_sessions (slot_id, session_id, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(slot_id) DO UPDATE SET session_id = ?2, updated_at = ?3",
            params![slot_id, session_id, now],
        )?;
        Ok(())
    }

    /// Delete a slot session
    pub fn delete_slot_session(&self, slot_id: &str) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM slot_sessions WHERE slot_id = ?", params![slot_id])?;
        Ok(())
    }

    /// Alias for delete_slot_session
    pub fn clear_slot_session(&self, slot_id: &str) {
        let _ = self.delete_slot_session(slot_id);
    }

    /// Get all slot sessions
    pub fn get_all_slot_sessions(&self) -> SqliteResult<Vec<(String, String)>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT slot_id, session_id FROM slot_sessions")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut sessions = Vec::new();
        for session in rows {
            sessions.push(session?);
        }
        Ok(sessions)
    }

    /// Get slot_id for a given session_id (reverse lookup)
    pub fn get_slot_for_session(&self, session_id: &str) -> SqliteResult<Option<String>> {
        let conn = self.conn();
        conn.query_row(
            "SELECT slot_id FROM slot_sessions WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        ).optional()
    }

    // ============ Board Tasks ============

    /// Insert a new board task
    pub fn insert_board_task(&self, task: &BoardTask) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO board_tasks (id, title, description, status, priority, category, project, server, due_date, parent_id, assignee, auto_execute, prompt_template, hidden, retry_count, max_retries, order_idx, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                task.id,
                task.title,
                task.description,
                task.status.as_str(),
                task.priority,
                task.category,
                task.project,
                task.server,
                task.due_date,
                task.parent_id,
                task.assignee,
                task.auto_execute as i32,
                task.prompt_template,
                task.hidden as i32,
                task.retry_count,
                task.max_retries,
                task.order_idx,
                task.created_at,
                task.updated_at,
            ],
        )?;
        Ok(())
    }

    /// Create a board task from input
    pub fn create_board_task(&self, input: &CreateBoardTaskInput) -> SqliteResult<BoardTask> {
        let now = chrono::Utc::now().to_rfc3339();
        let id = uuid::Uuid::new_v4().to_string();

        // Get max order for siblings
        let conn = self.conn();
        let max_order: i64 = if let Some(ref pid) = input.parent_id {
            conn.query_row(
                "SELECT COALESCE(MAX(order_idx), -1) FROM board_tasks WHERE parent_id = ?1",
                params![pid],
                |row| row.get(0),
            )
            .unwrap_or(-1)
        } else {
            conn.query_row(
                "SELECT COALESCE(MAX(order_idx), -1) FROM board_tasks WHERE parent_id IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap_or(-1)
        };
        drop(conn);

        let task = BoardTask {
            id,
            title: input.title.clone(),
            description: input.description.clone().unwrap_or_default(),
            status: BoardTaskStatus::Open,
            priority: input.priority.clone().unwrap_or_else(|| "medium".to_string()),
            category: input.category.clone().unwrap_or_else(|| "other".to_string()),
            project: input.project.clone(),
            server: input.server.clone(),
            due_date: input.due_date.clone(),
            parent_id: input.parent_id.clone(),
            assignee: input.assignee.clone(),
            auto_execute: input.auto_execute.unwrap_or(false),
            prompt_template: input.prompt_template.clone(),
            hidden: input.hidden.unwrap_or(false),
            retry_count: 0,
            max_retries: 2,
            order_idx: max_order + 1,
            created_at: now.clone(),
            updated_at: now,
            claim_executor_id: None,
            claim_executor_type: None,
            claimed_at: None,
        };

        self.insert_board_task(&task)?;
        Ok(task)
    }

    /// Get a board task by ID
    /// Resolve a (possibly short) board task ID to a full UUID.
    /// Supports exact match and unique prefix match (>= 6 chars).
    pub fn resolve_board_task_id(&self, id: &str) -> SqliteResult<Option<String>> {
        let conn = self.read_conn();
        // Exact match
        let exact: Option<String> = conn.query_row(
            "SELECT id FROM board_tasks WHERE id = ?1",
            params![id],
            |row| row.get(0),
        ).optional()?;
        if exact.is_some() {
            return Ok(exact);
        }
        // Prefix match for short IDs
        if id.len() >= 6 && id.len() < 36 {
            let prefix = format!("{}%", id);
            let mut stmt = conn.prepare("SELECT id FROM board_tasks WHERE id LIKE ?1")?;
            let matches: Vec<String> = stmt
                .query_map(params![prefix], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            if matches.len() == 1 {
                return Ok(Some(matches.into_iter().next().unwrap()));
            }
        }
        Ok(None)
    }

    pub fn get_board_task(&self, id: &str) -> SqliteResult<Option<BoardTask>> {
        let full_id = match self.resolve_board_task_id(id)? {
            Some(fid) => fid,
            None => return Ok(None),
        };
        let conn = self.read_conn();
        let mut stmt = conn
            .prepare("SELECT * FROM board_tasks WHERE id = ?")?;
        let mut rows = stmt.query(params![full_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_board_task(row)?))
        } else {
            Ok(None)
        }
    }

    /// List all board tasks (optionally filtered by status, hidden excluded by default)
    pub fn list_board_tasks(&self, status: Option<&str>, include_hidden: bool) -> SqliteResult<Vec<BoardTask>> {
        let conn = self.read_conn();
        let mut tasks = Vec::new();
        let hidden_clause = if include_hidden { "" } else { " AND hidden = 0" };

        if let Some(s) = status {
            let sql = format!("SELECT * FROM board_tasks WHERE status = ?1{} ORDER BY order_idx ASC", hidden_clause);
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![s], |row| Self::row_to_board_task(row))?;
            for task in rows {
                tasks.push(task?);
            }
        } else {
            let sql = if include_hidden {
                "SELECT * FROM board_tasks ORDER BY order_idx ASC".to_string()
            } else {
                "SELECT * FROM board_tasks WHERE hidden = 0 ORDER BY order_idx ASC".to_string()
            };
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map([], |row| Self::row_to_board_task(row))?;
            for task in rows {
                tasks.push(task?);
            }
        }

        Ok(tasks)
    }

    /// Update a board task
    pub fn update_board_task(&self, id: &str, update: &UpdateBoardTaskInput) -> SqliteResult<Option<BoardTask>> {
        let id = match self.resolve_board_task_id(id)? {
            Some(fid) => fid,
            None => return Ok(None),
        };
        let id = id.as_str();
        let now = chrono::Utc::now().to_rfc3339();
        let mut fields = vec!["updated_at = ?".to_string()];
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];

        macro_rules! push_field {
            ($name:ident, $col:expr) => {
                if let Some(ref v) = update.$name {
                    fields.push(format!("{} = ?", $col));
                    values.push(Box::new(v.clone()));
                }
            };
        }

        push_field!(title, "title");
        push_field!(description, "description");
        push_field!(status, "status");
        push_field!(priority, "priority");
        push_field!(category, "category");
        push_field!(project, "project");
        push_field!(server, "server");
        push_field!(due_date, "due_date");
        push_field!(parent_id, "parent_id");
        push_field!(assignee, "assignee");
        push_field!(prompt_template, "prompt_template");

        // Explicit claim update (for manual claim via MCP tool)
        if let Some(ref eid) = update.claim_executor_id {
            fields.push("claim_executor_id = ?".to_string());
            values.push(Box::new(eid.clone()));
        }
        if let Some(ref etype) = update.claim_executor_type {
            fields.push("claim_executor_type = ?".to_string());
            values.push(Box::new(etype.clone()));
        }

        // Auto-clear claim when status changes away from 'running'
        if let Some(ref status_str) = update.status {
            if status_str != "running" {
                // Clear claim fields when transitioning to any non-running status
                if update.claim_executor_id.is_none() {
                    fields.push("claim_executor_id = NULL".to_string());
                    fields.push("claim_executor_type = NULL".to_string());
                    fields.push("claimed_at = NULL".to_string());
                }
            }
        }

        if let Some(auto_exec) = update.auto_execute {
            fields.push("auto_execute = ?".to_string());
            values.push(Box::new(auto_exec as i32));
        }

        if let Some(hidden) = update.hidden {
            fields.push("hidden = ?".to_string());
            values.push(Box::new(hidden as i32));
        }

        if let Some(idx) = update.order_idx {
            fields.push("order_idx = ?".to_string());
            values.push(Box::new(idx));
        }

        let sql = format!(
            "UPDATE board_tasks SET {} WHERE id = ?",
            fields.join(", ")
        );
        values.push(Box::new(id.to_string()));

        let params: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();
        {
            let conn = self.conn();
            conn.execute(&sql, params.as_slice())?;
        }

        self.get_board_task(id)
    }

    /// Delete a board task and all descendants
    pub fn delete_board_task(&self, id: &str) -> SqliteResult<i64> {
        let id = match self.resolve_board_task_id(id)? {
            Some(fid) => fid,
            None => return Ok(0),
        };
        let conn = self.conn();
        // Collect all descendant IDs recursively
        let mut to_delete = vec![id.to_string()];
        let mut i = 0;
        while i < to_delete.len() {
            let current = to_delete[i].clone();
            let mut stmt = conn
                .prepare("SELECT id FROM board_tasks WHERE parent_id = ?")?;
            let children: Vec<String> = stmt
                .query_map(params![current], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            to_delete.extend(children);
            i += 1;
        }

        let mut deleted = 0i64;
        for tid in &to_delete {
            conn.execute("DELETE FROM board_task_notes WHERE task_id = ?", params![tid])?;
            let r = conn
                .execute("DELETE FROM board_tasks WHERE id = ?", params![tid])?;
            deleted += r as i64;
        }
        Ok(deleted)
    }

    /// Toggle a board task status (open <-> done)
    pub fn toggle_board_task(&self, id: &str) -> SqliteResult<Option<BoardTask>> {
        if let Some(task) = self.get_board_task(id)? {
            let new_status = match task.status {
                BoardTaskStatus::Done => "open",
                _ => "done",
            };
            let update = UpdateBoardTaskInput {
                status: Some(new_status.to_string()),
                ..Default::default()
            };
            self.update_board_task(id, &update)
        } else {
            Ok(None)
        }
    }

    /// Atomically claim a board task (CAS: only succeeds if open + unclaimed).
    /// Returns Ok(Some(task)) on success, Ok(None) if task not found or already claimed.
    pub fn claim_board_task(
        &self,
        id: &str,
        executor_id: &str,
        executor_type: &str,
    ) -> SqliteResult<Option<BoardTask>> {
        let full_id = match self.resolve_board_task_id(id)? {
            Some(fid) => fid,
            None => return Ok(None),
        };
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        let rows = conn.execute(
            "UPDATE board_tasks
             SET claim_executor_id = ?1, claim_executor_type = ?2, claimed_at = ?3,
                 status = 'running', updated_at = ?3
             WHERE id = ?4 AND status = 'open' AND claim_executor_id IS NULL",
            params![executor_id, executor_type, now, full_id],
        )?;
        drop(conn);
        if rows == 0 {
            return Ok(None); // Already claimed or not open
        }
        self.get_board_task(&full_id)
    }

    /// Release all claims held by a specific executor (zombie cleanup on disconnect/exit).
    /// Resets claimed tasks back to 'open'.
    pub fn release_board_claims_by_executor(&self, executor_id: &str) -> SqliteResult<usize> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        let count = conn.execute(
            "UPDATE board_tasks
             SET claim_executor_id = NULL, claim_executor_type = NULL, claimed_at = NULL,
                 status = 'open', updated_at = ?1
             WHERE claim_executor_id = ?2 AND status = 'running'",
            params![now, executor_id],
        )?;
        if count > 0 {
            tracing::info!(count, executor_id, "Released board claims for disconnected executor");
        }
        Ok(count)
    }

    /// Recover stale running tasks: if a task has been 'running' for > N minutes,
    /// reset it to 'open' (daemon may have restarted mid-execution).
    pub fn recover_stale_running_tasks(&self, stale_minutes: i64) -> SqliteResult<usize> {
        let conn = self.conn();
        let count = conn.execute(
            "UPDATE board_tasks SET status = 'open', claim_executor_id = NULL,
                 claim_executor_type = NULL, claimed_at = NULL, updated_at = ?1
             WHERE status = 'running'
               AND julianday('now') - julianday(updated_at) > ?2 / 1440.0",
            params![chrono::Utc::now().to_rfc3339(), stale_minutes as f64],
        )?;
        if count > 0 {
            tracing::warn!(count, stale_minutes, "Recovered stale running tasks (claims cleared)");
        }
        Ok(count)
    }

    /// List board tasks eligible for autopilot execution
    /// (auto_execute=true, status=open, due_date <= now, has assignee)
    pub fn list_autopilot_tasks(&self) -> SqliteResult<Vec<BoardTask>> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM board_tasks
             WHERE auto_execute = 1
               AND status = 'open'
               AND assignee IS NOT NULL
               AND (due_date IS NULL OR due_date <= ?1)
             ORDER BY order_idx ASC"
        )?;
        let rows = stmt.query_map(params![now], |row| Self::row_to_board_task(row))?;
        let mut tasks = Vec::new();
        for task in rows {
            tasks.push(task?);
        }
        Ok(tasks)
    }

    /// Board summary: status counts + pending questions + recent activity
    pub fn board_summary(&self, since: Option<&str>) -> SqliteResult<serde_json::Value> {
        let conn = self.conn();
        let since_clause = since.unwrap_or("2000-01-01T00:00:00Z");

        // Status counts (since timestamp for completed/failed, all-time for open/running/blocked)
        let open: i64 = conn.query_row("SELECT COUNT(*) FROM board_tasks WHERE status = 'open'", [], |r| r.get(0))?;
        let running: i64 = conn.query_row("SELECT COUNT(*) FROM board_tasks WHERE status = 'running'", [], |r| r.get(0))?;
        let blocked: i64 = conn.query_row("SELECT COUNT(*) FROM board_tasks WHERE status = 'blocked'", [], |r| r.get(0))?;
        let completed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM board_tasks WHERE status = 'done' AND updated_at >= ?1",
            params![since_clause], |r| r.get(0),
        )?;
        let failed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM board_tasks WHERE status = 'failed' AND updated_at >= ?1",
            params![since_clause], |r| r.get(0),
        )?;

        // Pending questions
        let pending_questions: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_questions WHERE status = 'pending'", [], |r| r.get(0),
        )?;

        // Recent KB entries (since timestamp)
        let new_kb: i64 = conn.query_row(
            "SELECT COUNT(*) FROM knowledge WHERE created_at >= ?1",
            params![since_clause], |r| r.get(0),
        ).unwrap_or(0);

        Ok(serde_json::json!({
            "open": open,
            "running": running,
            "blocked": blocked,
            "completed": completed,
            "failed": failed,
            "pendingQuestions": pending_questions,
            "newKBEntries": new_kb,
        }))
    }

    /// Clear all done board tasks
    pub fn clear_done_board_tasks(&self) -> SqliteResult<i64> {
        let conn = self.conn();
        let result = conn
            .execute("DELETE FROM board_tasks WHERE status = 'done'", [])?;
        Ok(result as i64)
    }

    fn row_to_board_task(row: &rusqlite::Row) -> SqliteResult<BoardTask> {
        let status_str: String = row.get("status")?;
        let status = BoardTaskStatus::from_str(&status_str).unwrap_or(BoardTaskStatus::Open);
        let auto_execute: i32 = row.get("auto_execute").unwrap_or(0);
        let hidden: i32 = row.get("hidden").unwrap_or(0);

        Ok(BoardTask {
            id: row.get("id")?,
            title: row.get("title")?,
            description: row.get("description")?,
            status,
            priority: row.get("priority")?,
            category: row.get("category")?,
            project: row.get("project")?,
            server: row.get("server")?,
            due_date: row.get("due_date")?,
            parent_id: row.get("parent_id")?,
            assignee: row.get("assignee")?,
            auto_execute: auto_execute != 0,
            prompt_template: row.get("prompt_template")?,
            hidden: hidden != 0,
            retry_count: row.get("retry_count").unwrap_or(0),
            max_retries: row.get("max_retries").unwrap_or(2),
            order_idx: row.get("order_idx")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
            claim_executor_id: row.get("claim_executor_id").unwrap_or(None),
            claim_executor_type: row.get("claim_executor_type").unwrap_or(None),
            claimed_at: row.get("claimed_at").unwrap_or(None),
        })
    }

    // ============ Board Task Notes ============

    /// Add a note to a board task
    pub fn add_board_task_note(
        &self,
        input: &AddBoardTaskNoteInput,
    ) -> SqliteResult<BoardTaskNote> {
        let now = chrono::Utc::now().to_rfc3339();
        let id = uuid::Uuid::new_v4().to_string();
        let note_type_str = input.note_type.as_deref().unwrap_or("note");

        let conn = self.conn();

        // Verify task exists
        let task_exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM board_tasks WHERE id = ?1",
            params![input.task_id],
            |row| row.get(0),
        )?;
        if !task_exists {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }

        let note_type = BoardNoteType::from_str(note_type_str).unwrap_or(BoardNoteType::Note);

        conn.execute(
            "INSERT INTO board_task_notes (id, task_id, content, note_type, author, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, input.task_id, input.content, note_type.as_str(), input.author, now],
        )?;

        Ok(BoardTaskNote {
            id,
            task_id: input.task_id.clone(),
            content: input.content.clone(),
            note_type,
            author: input.author.clone(),
            created_at: now,
        })
    }

    /// Get all notes for a board task (ordered by creation time ASC)
    pub fn get_board_task_notes(&self, task_id: &str) -> SqliteResult<Vec<BoardTaskNote>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, content, note_type, author, created_at
             FROM board_task_notes WHERE task_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![task_id], Self::row_to_board_task_note)?;
        let mut notes = Vec::new();
        for note in rows {
            notes.push(note?);
        }
        Ok(notes)
    }

    /// Get a board task with its notes
    pub fn get_board_task_with_notes(
        &self,
        id: &str,
    ) -> SqliteResult<Option<BoardTaskWithNotes>> {
        if let Some(task) = self.get_board_task(id)? {
            let notes = self.get_board_task_notes(&task.id)?;
            Ok(Some(BoardTaskWithNotes { task, notes }))
        } else {
            Ok(None)
        }
    }

    fn row_to_board_task_note(row: &rusqlite::Row) -> SqliteResult<BoardTaskNote> {
        let note_type_str: String = row.get("note_type")?;
        let note_type = BoardNoteType::from_str(&note_type_str).unwrap_or(BoardNoteType::Note);

        Ok(BoardTaskNote {
            id: row.get("id")?,
            task_id: row.get("task_id")?,
            content: row.get("content")?,
            note_type,
            author: row.get("author")?,
            created_at: row.get("created_at")?,
        })
    }

    // ============ Agent Questions (Pending Decisions) ============

    pub fn create_agent_question(
        &self,
        input: &CreateAgentQuestionInput,
    ) -> SqliteResult<AgentQuestion> {
        let now = chrono::Utc::now().to_rfc3339();
        let id = uuid::Uuid::new_v4().to_string();
        let q = AgentQuestion {
            id: id.clone(),
            task_id: input.task_id.clone(),
            slot_id: input.slot_id.clone(),
            session_id: input.session_id.clone(),
            question: input.question.clone(),
            context: input.context.clone().unwrap_or_default(),
            status: AgentQuestionStatus::Pending,
            answer: None,
            created_at: now.clone(),
            updated_at: now,
        };
        let conn = self.conn();
        conn.execute(
            "INSERT INTO agent_questions (id, task_id, slot_id, session_id, question, context, status, answer, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                q.id, q.task_id, q.slot_id, q.session_id,
                q.question, q.context, q.status.as_str(),
                q.answer, q.created_at, q.updated_at,
            ],
        )?;

        // Auto-block: if question is linked to a board task, mark task as blocked
        if let Some(ref tid) = q.task_id {
            conn.execute(
                "UPDATE board_tasks SET status = 'blocked', updated_at = ?1 WHERE id = ?2 AND status IN ('open', 'running')",
                params![q.created_at, tid],
            )?;
            tracing::info!(task_id = %tid, question_id = %q.id, "Question created → task auto-blocked");
        }

        Ok(q)
    }

    pub fn get_agent_question(&self, id: &str) -> SqliteResult<Option<AgentQuestion>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT * FROM agent_questions WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_agent_question(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_agent_questions(
        &self,
        status: Option<&str>,
    ) -> SqliteResult<Vec<AgentQuestion>> {
        let conn = self.conn();
        let mut questions = Vec::new();
        if let Some(s) = status {
            let mut stmt = conn.prepare(
                "SELECT * FROM agent_questions WHERE status = ?1 ORDER BY created_at DESC",
            )?;
            let rows = stmt.query_map(params![s], |row| Self::row_to_agent_question(row))?;
            for q in rows {
                questions.push(q?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT * FROM agent_questions ORDER BY created_at DESC",
            )?;
            let rows = stmt.query_map([], |row| Self::row_to_agent_question(row))?;
            for q in rows {
                questions.push(q?);
            }
        }
        Ok(questions)
    }

    /// List answered questions linked to a specific board task
    pub fn list_questions_for_task(&self, task_id: &str) -> SqliteResult<Vec<AgentQuestion>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM agent_questions WHERE task_id = ?1 AND status = 'answered' ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![task_id], |row| Self::row_to_agent_question(row))?;
        rows.collect()
    }

    /// Increment retry_count and set status back to open for retry
    pub fn increment_board_task_retry(&self, task_id: &str, new_retry: i64) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE board_tasks SET retry_count = ?1, status = 'open', updated_at = ?2 WHERE id = ?3",
            params![new_retry, now, task_id],
        )?;
        Ok(())
    }

    pub fn answer_agent_question(
        &self,
        id: &str,
        answer: &str,
    ) -> SqliteResult<Option<AgentQuestion>> {
        let now = chrono::Utc::now().to_rfc3339();
        {
            let conn = self.conn();
            conn.execute(
                "UPDATE agent_questions SET answer = ?1, status = 'answered', updated_at = ?2 WHERE id = ?3",
                params![answer, now, id],
            )?;

            // Auto-unblock: if all questions for the linked task are resolved, unblock the task
            // First get the task_id from this question
            let task_id: Option<String> = conn.query_row(
                "SELECT task_id FROM agent_questions WHERE id = ?1",
                params![id],
                |row| row.get(0),
            ).ok().flatten();

            if let Some(ref tid) = task_id {
                // Check if any pending questions remain for this task
                let pending_count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM agent_questions WHERE task_id = ?1 AND status = 'pending'",
                    params![tid],
                    |row| row.get(0),
                ).unwrap_or(0);

                if pending_count == 0 {
                    conn.execute(
                        "UPDATE board_tasks SET status = 'open', updated_at = ?1 WHERE id = ?2 AND status = 'blocked'",
                        params![now, tid],
                    )?;
                    tracing::info!(task_id = %tid, "All questions answered → task auto-unblocked");
                }
            }
        }
        self.get_agent_question(id)
    }

    pub fn dismiss_agent_question(&self, id: &str) -> SqliteResult<Option<AgentQuestion>> {
        let now = chrono::Utc::now().to_rfc3339();
        {
            let conn = self.conn();
            conn.execute(
                "UPDATE agent_questions SET status = 'dismissed', updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
        }
        self.get_agent_question(id)
    }

    fn row_to_agent_question(row: &rusqlite::Row) -> SqliteResult<AgentQuestion> {
        let status_str: String = row.get("status")?;
        let status =
            AgentQuestionStatus::from_str(&status_str).unwrap_or(AgentQuestionStatus::Pending);
        Ok(AgentQuestion {
            id: row.get("id")?,
            task_id: row.get("task_id")?,
            slot_id: row.get("slot_id")?,
            session_id: row.get("session_id")?,
            question: row.get("question")?,
            context: row.get("context")?,
            status,
            answer: row.get("answer")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }

    // ============ Knowledge Base ============

    /// Redact sensitive patterns from text before sending to external APIs
    pub fn redact_sensitive(text: &str) -> String {
        use once_cell::sync::Lazy;
        static REDACTIONS: Lazy<Vec<(regex::Regex, &'static str)>> = Lazy::new(|| {
            vec![
                (regex::Regex::new(r"(?i)sshpass\s+-p\s+'[^']*'").unwrap(), "sshpass -p '[REDACTED]'"),
                (regex::Regex::new(r"(?i)(password|passwd|pwd|secret_key)\s*[:=]\s*\S+").unwrap(), "$1=[REDACTED]"),
                (regex::Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}(:\d+)?\b").unwrap(), "[IP_REDACTED]"),
            ]
        });
        let mut result = text.to_string();
        for (re, replacement) in REDACTIONS.iter() {
            result = re.replace_all(&result, *replacement).to_string();
        }
        result
    }

    /// Check if text contains sensitive data patterns (passwords, API keys, etc.)
    fn contains_sensitive_data(text: &str) -> bool {
        use once_cell::sync::Lazy;
        static PATTERNS: Lazy<Vec<regex::Regex>> = Lazy::new(|| {
            [
                r"(?i)(password|passwd|pwd|secret_key)\s*[:=]\s*\S+",
                r"(?i)sshpass\s+-p\s+",
                r#"(?i)(api[_-]?key|token|secret)\s*[:=]\s*['"]?[A-Za-z0-9_\-]{20,}"#,
                r"(?i)ssh\s+\S+@\S+.*-p\s+'\S+'",
            ]
            .iter()
            .filter_map(|p| regex::Regex::new(p).ok())
            .collect()
        });
        PATTERNS.iter().any(|re| re.is_match(text))
    }

    /// Remember (upsert) a knowledge entry, with FTS similarity dedup
    pub fn kb_remember(&self, input: &KBRememberInput) -> SqliteResult<KBRememberResult> {
        let now = chrono::Utc::now().to_rfc3339();
        let source = input.source.as_deref().unwrap_or("conversation");
        let mut confidence = input.confidence.unwrap_or(1.0);
        let detail_str = input.detail.as_ref().map(|d| serde_json::to_string(d).unwrap_or_default());

        // Guard: reject infra category (servers.yaml + mission_infra_get already covers this)
        if input.category == "infra" {
            tracing::warn!(key = %input.key, "kb_remember: rejected infra category (use servers.yaml)");
            return Ok(KBRememberResult {
                entry: KnowledgeEntry {
                    id: String::new(),
                    category: input.category.clone(),
                    key: input.key.clone(),
                    summary: "REJECTED: infra entries should use servers.yaml + mission_infra_get".into(),
                    detail: None,
                    source: source.to_string(),
                    confidence: 0.0,
                    access_count: 0,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    last_accessed_at: None,
                    linked_task_id: None,
                },
                action: "rejected".into(),
                merged_key: None,
                similarity: None,
            });
        }

        // Sensitive data detection: warn and lower confidence
        let check_text = format!("{} {} {}", input.summary, detail_str.as_deref().unwrap_or(""), input.key);
        if Self::contains_sensitive_data(&check_text) {
            tracing::warn!(key = %input.key, category = %input.category,
                "KB entry may contain sensitive data (password/API key/credentials)");
            confidence = confidence.min(0.5);
        }

        let conn = self.conn();

        // 1. Exact match by (category, key) → update
        let updated = conn.execute(
            "UPDATE knowledge SET summary = ?1, detail = ?2, source = ?3, confidence = ?4, updated_at = ?5
             WHERE category = ?6 AND key = ?7",
            params![input.summary, detail_str, source, confidence, now, input.category, input.key],
        )?;

        if updated > 0 {
            let entry = Self::kb_get_by_category_key_with_conn(&conn, &input.category, &input.key)?;
            if let Some(ref e) = entry {
                Self::kb_sync_fts_with_conn(&conn, e)?;
            }
            return Ok(KBRememberResult {
                entry: entry.unwrap(),
                action: "updated".to_string(),
                merged_key: None,
                similarity: None,
            });
        }

        // 1b. Same key, different category → update in-place (re-categorize)
        let existing_by_key: Option<KnowledgeEntry> = {
            let mut stmt = conn.prepare("SELECT * FROM knowledge WHERE key = ?1")?;
            let mut rows = stmt.query(params![input.key])?;
            if let Some(row) = rows.next()? {
                Some(Self::row_to_knowledge_entry(row)?)
            } else {
                None
            }
        };
        if let Some(existing) = existing_by_key {
            let old_category = existing.category.clone();
            conn.execute(
                "UPDATE knowledge SET category = ?1, summary = ?2, detail = ?3, source = ?4, confidence = ?5, updated_at = ?6
                 WHERE id = ?7",
                params![input.category, input.summary, detail_str, source, confidence, now, existing.id],
            )?;
            let entry = Self::kb_get_by_category_key_with_conn(&conn, &input.category, &input.key)?;
            if let Some(ref e) = entry {
                Self::kb_sync_fts_with_conn(&conn, e)?;
            }
            tracing::info!(key = %input.key, from = %old_category, to = %input.category, "KB entry re-categorized");
            return Ok(KBRememberResult {
                entry: entry.unwrap(),
                action: "updated".to_string(),
                merged_key: None,
                similarity: None,
            });
        }

        // 2. Fuzzy dedup: check for similar entries in same category
        const SIMILARITY_THRESHOLD: f64 = 0.5;
        if let Some((sim, existing)) = Self::kb_find_similar_with_conn(
            &conn,
            &input.category,
            &format!("{} {}", input.key, input.summary),
            SIMILARITY_THRESHOLD,
        )? {
            // Merge: update the existing entry with new summary
            conn.execute(
                "UPDATE knowledge SET summary = ?1, detail = ?2, source = ?3, confidence = ?4, updated_at = ?5
                 WHERE id = ?6",
                params![input.summary, detail_str, source, confidence, now, existing.id],
            )?;
            let merged_key = existing.key.clone();
            let entry = Self::kb_get_by_category_key_with_conn(&conn, &existing.category, &existing.key)?;
            if let Some(ref e) = entry {
                Self::kb_sync_fts_with_conn(&conn, e)?;
            }
            return Ok(KBRememberResult {
                entry: entry.unwrap(),
                action: "merged".to_string(),
                merged_key: Some(merged_key),
                similarity: Some(sim),
            });
        }

        // 3. Insert new
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO knowledge (id, category, key, summary, detail, source, confidence, access_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9)",
            params![id, input.category, input.key, input.summary, detail_str, source, confidence, now, now],
        )?;

        let entry = KnowledgeEntry {
            id: id.clone(),
            category: input.category.clone(),
            key: input.key.clone(),
            summary: input.summary.clone(),
            detail: input.detail.clone(),
            source: source.to_string(),
            confidence,
            access_count: 0,
            created_at: now.clone(),
            updated_at: now,
            last_accessed_at: None,
            linked_task_id: None,
        };

        Self::kb_sync_fts_with_conn(&conn, &entry)?;

        Ok(KBRememberResult {
            entry,
            action: "created".to_string(),
            merged_key: None,
            similarity: None,
        })
    }

    /// Find the most similar entry in the same category (uses existing connection)
    fn kb_find_similar_with_conn(
        conn: &Connection,
        category: &str,
        new_text: &str,
        threshold: f64,
    ) -> SqliteResult<Option<(f64, KnowledgeEntry)>> {
        let entries = Self::kb_list_with_conn(conn, Some(category))?;

        let mut best: Option<(f64, KnowledgeEntry)> = None;
        for entry in entries {
            let existing_text = format!("{} {}", entry.key, entry.summary);
            let sim = token_jaccard_similarity(new_text, &existing_text);
            if sim >= threshold {
                match &best {
                    None => best = Some((sim, entry)),
                    Some((best_sim, _)) if sim > *best_sim => best = Some((sim, entry)),
                    _ => {}
                }
            }
        }

        Ok(best)
    }

    /// Set linked_task_id on a knowledge entry (for Board-aware consolidation)
    pub fn kb_set_linked_task_id(&self, key: &str, task_id: Option<&str>) -> SqliteResult<bool> {
        let conn = self.conn();
        let updated = conn.execute(
            "UPDATE knowledge SET linked_task_id = ?1 WHERE key = ?2",
            params![task_id, key],
        )?;
        Ok(updated > 0)
    }

    /// Get a knowledge entry by key
    pub fn kb_get(&self, key: &str) -> SqliteResult<Option<KnowledgeEntry>> {
        let conn = self.conn(); // Need write conn for access_count update
        let mut stmt = conn.prepare(
            "SELECT * FROM knowledge WHERE key = ?1"
        )?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            let mut entry = Self::row_to_knowledge_entry(row)?;
            // Bump access count
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE knowledge SET access_count = access_count + 1, last_accessed_at = ?1 WHERE id = ?2",
                params![now, entry.id],
            )?;
            entry.access_count += 1;
            entry.last_accessed_at = Some(now);
            Ok(Some(entry))
        } else {
            Ok(None)
        }
    }

    /// Get by category + key (internal, no access bump, uses existing connection)
    fn kb_get_by_category_key_with_conn(conn: &Connection, category: &str, key: &str) -> SqliteResult<Option<KnowledgeEntry>> {
        let mut stmt = conn.prepare(
            "SELECT * FROM knowledge WHERE category = ?1 AND key = ?2"
        )?;
        let mut rows = stmt.query(params![category, key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_knowledge_entry(row)?))
        } else {
            Ok(None)
        }
    }

    /// Search knowledge via FTS, with LIKE fallback for Chinese text
    pub fn kb_search(&self, query: &str, category: Option<&str>) -> SqliteResult<Vec<KnowledgeEntry>> {
        // Phase 1: FTS5 search (works well for English / space-separated tokens)
        let results = self.kb_search_fts(query, category)?;
        let results = if results.is_empty() {
            // Phase 2: LIKE fallback (works for Chinese and partial matches)
            self.kb_search_like(query, category)?
        } else {
            results
        };

        // Update access_count + last_accessed_at for all matched entries
        if !results.is_empty() {
            self.kb_update_access_stats(&results)?;
        }

        Ok(results)
    }

    /// Batch update access_count and last_accessed_at for search hits
    fn kb_update_access_stats(&self, entries: &[KnowledgeEntry]) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        let mut stmt = conn.prepare(
            "UPDATE knowledge SET access_count = access_count + 1, last_accessed_at = ?1 WHERE id = ?2"
        )?;
        for entry in entries {
            stmt.execute(params![now, entry.id])?;
        }
        Ok(())
    }

    /// FTS5-based search
    fn kb_search_fts(&self, query: &str, category: Option<&str>) -> SqliteResult<Vec<KnowledgeEntry>> {
        let fts_query = query.split_whitespace()
            .map(|w| format!("\"{}\"", w.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" OR ");

        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.read_conn();
        let mut results = Vec::new();

        if let Some(cat) = category {
            let like_pattern = format!("{}:%", cat);
            let mut stmt = conn.prepare(
                "SELECT k.* FROM knowledge k
                 JOIN knowledge_fts f ON k.rowid = f.rowid
                 WHERE knowledge_fts MATCH ?1 AND (k.category = ?2 OR k.category LIKE ?3)
                 ORDER BY rank"
            )?;
            let rows = stmt.query_map(params![fts_query, cat, like_pattern], |row| Self::row_to_knowledge_entry(row))?;
            for entry in rows {
                results.push(entry?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT k.* FROM knowledge k
                 JOIN knowledge_fts f ON k.rowid = f.rowid
                 WHERE knowledge_fts MATCH ?1
                 ORDER BY rank"
            )?;
            let rows = stmt.query_map(params![fts_query], |row| Self::row_to_knowledge_entry(row))?;
            for entry in rows {
                results.push(entry?);
            }
        }

        Ok(results)
    }

    /// LIKE-based fallback search for Chinese and partial matches
    fn kb_search_like(&self, query: &str, category: Option<&str>) -> SqliteResult<Vec<KnowledgeEntry>> {
        let keywords: Vec<String> = {
            let mut kw: Vec<String> = query.split_whitespace()
                .map(|w| w.to_string())
                .collect();
            // Add the full query as a keyword if it contains non-ASCII (Chinese)
            let trimmed = query.trim();
            if !trimmed.is_empty() && trimmed.chars().any(|c| !c.is_ascii()) && !kw.contains(&trimmed.to_string()) {
                kw.insert(0, trimmed.to_string());
            }
            kw
        };

        if keywords.is_empty() {
            return Ok(Vec::new());
        }

        // Build dynamic SQL with per-keyword LIKE parameters
        let mut sql = String::from("SELECT * FROM knowledge WHERE (");
        let mut like_parts: Vec<String> = Vec::new();
        for (i, _kw) in keywords.iter().enumerate() {
            let p = i + 1; // 1-indexed
            like_parts.push(format!(
                "(key LIKE ?{p} OR summary LIKE ?{p} OR COALESCE(detail,'') LIKE ?{p})"
            ));
        }
        sql.push_str(&like_parts.join(" OR "));
        sql.push(')');

        if category.is_some() {
            let p_cat = keywords.len() + 1;
            let p_like = keywords.len() + 2;
            sql.push_str(&format!(" AND (category = ?{} OR category LIKE ?{})", p_cat, p_like));
        }
        sql.push_str(" ORDER BY access_count DESC, updated_at DESC LIMIT 20");

        let conn = self.read_conn();
        let mut stmt = conn.prepare(&sql)?;

        // Bind parameters: %keyword% for each, then optional category + like pattern
        let like_params: Vec<String> = keywords.iter().map(|kw| format!("%{}%", kw)).collect();
        let mut param_values: Vec<&dyn rusqlite::types::ToSql> = like_params.iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();
        let cat_owned: String;
        let cat_like: String;
        if let Some(cat) = category {
            cat_owned = cat.to_string();
            cat_like = format!("{}:%", cat);
            param_values.push(&cat_owned);
            param_values.push(&cat_like);
        }

        let rows = stmt.query_map(rusqlite::params_from_iter(param_values.iter()), |row| {
            Self::row_to_knowledge_entry(row)
        })?;

        let mut results = Vec::new();
        for entry in rows {
            results.push(entry?);
        }
        Ok(results)
    }

    /// List knowledge entries, optionally filtered by category.
    /// Supports composite categories: querying "memory" also returns "memory:architecture" etc.
    pub fn kb_list(&self, category: Option<&str>) -> SqliteResult<Vec<KnowledgeEntry>> {
        let conn = self.read_conn();
        Self::kb_list_with_conn(&conn, category)
    }

    /// List knowledge entries using an existing connection (no lock)
    fn kb_list_with_conn(conn: &Connection, category: Option<&str>) -> SqliteResult<Vec<KnowledgeEntry>> {
        let mut entries = Vec::new();
        if let Some(cat) = category {
            let like_pattern = format!("{}:%", cat);
            let mut stmt = conn.prepare(
                "SELECT * FROM knowledge WHERE category = ?1 OR category LIKE ?2 ORDER BY updated_at DESC"
            )?;
            let rows = stmt.query_map(params![cat, like_pattern], |row| Self::row_to_knowledge_entry(row))?;
            for entry in rows {
                entries.push(entry?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT * FROM knowledge ORDER BY category, updated_at DESC"
            )?;
            let rows = stmt.query_map([], |row| Self::row_to_knowledge_entry(row))?;
            for entry in rows {
                entries.push(entry?);
            }
        }
        Ok(entries)
    }

    /// List knowledge entries with pagination support.
    /// Used by kb_analyze v2 for chunked analysis.
    pub fn kb_list_paginated(&self, category: Option<&str>, limit: u32, offset: u32) -> SqliteResult<Vec<KnowledgeEntry>> {
        let conn = self.read_conn();
        let mut entries = Vec::new();
        if let Some(cat) = category {
            let like_pattern = format!("{}:%", cat);
            let mut stmt = conn.prepare(
                "SELECT * FROM knowledge WHERE category = ?1 OR category LIKE ?2 ORDER BY updated_at DESC LIMIT ?3 OFFSET ?4"
            )?;
            let rows = stmt.query_map(params![cat, like_pattern, limit, offset], |row| Self::row_to_knowledge_entry(row))?;
            for entry in rows {
                entries.push(entry?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT * FROM knowledge ORDER BY category, updated_at DESC LIMIT ?1 OFFSET ?2"
            )?;
            let rows = stmt.query_map(params![limit, offset], |row| Self::row_to_knowledge_entry(row))?;
            for entry in rows {
                entries.push(entry?);
            }
        }
        Ok(entries)
    }

    /// Forget (delete) a knowledge entry by key
    pub fn kb_forget(&self, key: &str) -> SqliteResult<bool> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM knowledge WHERE key = ?1",
            params![key],
        )?;
        if deleted > 0 {
            self.fts_dirty.store(true, Ordering::Relaxed);
        }
        Ok(deleted > 0)
    }

    /// Batch delete multiple knowledge entries by keys. Returns count of deleted entries.
    pub fn kb_batch_forget(&self, keys: &[String]) -> SqliteResult<usize> {
        let conn = self.conn();
        let mut deleted = 0;
        for key in keys {
            let n = conn.execute(
                "DELETE FROM knowledge WHERE key = ?1",
                params![key],
            )?;
            if n > 0 {
                deleted += 1;
            }
        }
        if deleted > 0 {
            // Immediate rebuild after batch — don't wait for autopilot_tick
            conn.execute_batch("INSERT INTO knowledge_fts(knowledge_fts) VALUES('rebuild')")?;
            self.fts_dirty.store(false, Ordering::Relaxed);
            tracing::info!(deleted, "kb_batch_forget: FTS rebuilt after batch delete");
        }
        Ok(deleted)
    }

    /// Sync FTS index for a knowledge entry (uses existing connection)
    fn kb_sync_fts_with_conn(conn: &Connection, entry: &KnowledgeEntry) -> SqliteResult<()> {
        let rowid: i64 = conn.query_row(
            "SELECT rowid FROM knowledge WHERE id = ?1",
            params![entry.id],
            |row| row.get(0),
        )?;
        let detail_str = entry.detail.as_ref()
            .map(|d| serde_json::to_string(d).unwrap_or_default())
            .unwrap_or_default();

        // Delete old FTS entry (must provide actual indexed values for external content)
        let old_values: Option<(String, String, String, String)> = conn.query_row(
            "SELECT key, summary, COALESCE(detail, ''), category FROM knowledge WHERE id = ?1",
            params![entry.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).ok();
        if let Some((old_key, old_summary, old_detail, old_category)) = old_values {
            conn.execute(
                "INSERT INTO knowledge_fts(knowledge_fts, rowid, key, summary, detail, category) VALUES('delete', ?1, ?2, ?3, ?4, ?5)",
                params![rowid, old_key, old_summary, old_detail, old_category],
            ).ok();
        }

        conn.execute(
            "INSERT INTO knowledge_fts(rowid, key, summary, detail, category) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![rowid, entry.key, entry.summary, detail_str, entry.category],
        )?;
        Ok(())
    }

    /// Get KB category counts for summary string
    pub fn kb_summary(&self) -> SqliteResult<Vec<(String, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT category, COUNT(*) as cnt FROM knowledge GROUP BY category ORDER BY cnt DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.collect()
    }

    /// Get top N hot keys by access_count (for instructions injection)
    pub fn kb_hot_keys(&self, limit: i64) -> SqliteResult<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT key FROM knowledge ORDER BY access_count DESC, updated_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| row.get::<_, String>(0))?;
        rows.collect()
    }

    /// Find stale entries: never accessed and older than N days
    pub fn kb_find_stale(&self, days: i64) -> SqliteResult<Vec<KnowledgeEntry>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM knowledge WHERE access_count = 0 \
             AND last_accessed_at IS NULL \
             AND julianday('now') - julianday(updated_at) > ?1 \
             ORDER BY updated_at ASC",
        )?;
        let rows = stmt.query_map(params![days], |row| Self::row_to_knowledge_entry(row))?;
        rows.collect()
    }

    /// Find potential duplicates using Jaccard similarity on key+summary text.
    /// Returns pairs with similarity score (threshold: 0.6).
    pub fn kb_find_duplicates(&self) -> SqliteResult<Vec<(KnowledgeEntry, KnowledgeEntry, f64)>> {
        const DUP_THRESHOLD: f64 = 0.6;
        let entries = self.kb_list(None)?;
        let mut duplicates = Vec::new();

        // Group by category for O(n²) within each category (not globally)
        let mut by_cat: std::collections::HashMap<String, Vec<&KnowledgeEntry>> = std::collections::HashMap::new();
        for e in &entries {
            by_cat.entry(e.category.clone()).or_default().push(e);
        }

        for (_cat, group) in &by_cat {
            for i in 0..group.len() {
                let text_a = format!("{} {}", group[i].key, group[i].summary);
                for j in (i + 1)..group.len() {
                    let text_b = format!("{} {}", group[j].key, group[j].summary);
                    let sim = token_jaccard_similarity(&text_a, &text_b);
                    if sim >= DUP_THRESHOLD {
                        duplicates.push((group[i].clone(), group[j].clone(), sim));
                    }
                }
            }
        }

        // Sort by similarity descending
        duplicates.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        Ok(duplicates)
    }

    /// Get KB statistics for governance
    pub fn kb_stats(&self) -> SqliteResult<serde_json::Value> {
        let conn = self.read_conn();
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM knowledge", [], |row| row.get(0)
        )?;
        let never_accessed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM knowledge WHERE access_count = 0 AND last_accessed_at IS NULL",
            [], |row| row.get(0),
        )?;
        let most_accessed: Option<(String, String, i64)> = conn.query_row(
            "SELECT category, key, access_count FROM knowledge ORDER BY access_count DESC LIMIT 1",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).ok();
        let oldest: Option<(String, String, String)> = conn.query_row(
            "SELECT category, key, updated_at FROM knowledge ORDER BY updated_at ASC LIMIT 1",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).ok();
        drop(conn);

        let mut stats = serde_json::json!({
            "total": total,
            "neverAccessed": never_accessed,
        });
        if let Some((cat, key, count)) = most_accessed {
            stats["mostAccessed"] = serde_json::json!({"category": cat, "key": key, "accessCount": count});
        }
        if let Some((cat, key, updated)) = oldest {
            stats["oldest"] = serde_json::json!({"category": cat, "key": key, "updatedAt": updated});
        }

        // Category breakdown (raw subcategories + parent rollup)
        let summary = self.kb_summary()?;
        let raw: std::collections::HashMap<String, i64> = summary.into_iter().collect();
        stats["categories"] = serde_json::json!(raw);

        // Parent category rollup: "memory:architecture" counts toward "memory" total
        let mut parents: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for (cat, count) in &raw {
            let parent = cat.split(':').next().unwrap_or(cat);
            *parents.entry(parent.to_string()).or_default() += count;
        }
        // Only include rollup if there are subcategories
        if parents.len() < raw.len() {
            stats["categoryRollup"] = serde_json::json!(parents);
        }

        Ok(stats)
    }

    /// Auto-GC: delete infra (duplicates servers.yaml), expired bugfix (>30d),
    /// and stale zero-access entries (>14d, non-protected categories).
    pub fn kb_auto_gc(&self) -> SqliteResult<usize> {
        let conn = self.conn();
        let mut deleted = 0;

        // Rule 1: infra entries — servers.yaml + mission_infra_get already covers this
        deleted += conn.execute("DELETE FROM knowledge WHERE category = 'infra'", [])?;

        // Rule 2: memory:bugfix older than 30 days
        deleted += conn.execute(
            "DELETE FROM knowledge WHERE category = 'memory:bugfix' \
             AND julianday('now') - julianday(updated_at) > 30",
            [],
        )?;

        // Rule 3: zero access + 14 days stale + not in protected categories
        deleted += conn.execute(
            "DELETE FROM knowledge WHERE access_count = 0 \
             AND last_accessed_at IS NULL \
             AND julianday('now') - julianday(updated_at) > 14 \
             AND category NOT IN ('preference', 'memory:decision', 'memory:architecture', 'project')",
            [],
        )?;

        if deleted > 0 {
            conn.execute_batch("INSERT INTO knowledge_fts(knowledge_fts) VALUES('rebuild')")?;
            self.fts_dirty.store(false, Ordering::Relaxed);
            tracing::info!(deleted, "kb_auto_gc: cleaned up entries, FTS rebuilt");
        }

        Ok(deleted)
    }

    // ============ KB Operation Queue ============

    /// Save a consolidation plan from kb_analyze into the operation queue
    pub fn kb_ops_save_plan(
        &self,
        plan_id: &str,
        task_id: Option<&str>,
        operations: &[KBOperation],
    ) -> SqliteResult<usize> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        let mut saved = 0;
        for (i, op) in operations.iter().enumerate() {
            let id = format!("{}-{}", plan_id, i);
            conn.execute(
                "INSERT OR REPLACE INTO kb_operation_queue
                 (id, plan_id, task_id, operation, target_keys, rationale, status, priority, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?8)",
                params![
                    id,
                    plan_id,
                    task_id,
                    op.operation,
                    serde_json::to_string(&op.target_keys).unwrap_or_default(),
                    op.rationale,
                    i as i32,
                    now,
                ],
            )?;
            saved += 1;
        }
        Ok(saved)
    }

    /// List operations by plan_id or all pending
    pub fn kb_ops_list(
        &self,
        plan_id: Option<&str>,
        status: Option<&str>,
    ) -> SqliteResult<Vec<KBOperationRow>> {
        let conn = self.conn();
        let (sql, params_vec): (String, Vec<Box<dyn rusqlite::ToSql>>) = match (plan_id, status) {
            (Some(pid), Some(s)) => (
                "SELECT id, plan_id, task_id, operation, target_keys, rationale, status, priority, result, created_at, executed_at, error
                 FROM kb_operation_queue WHERE plan_id = ?1 AND status = ?2 ORDER BY priority".to_string(),
                vec![Box::new(pid.to_string()) as Box<dyn rusqlite::ToSql>, Box::new(s.to_string())],
            ),
            (Some(pid), None) => (
                "SELECT id, plan_id, task_id, operation, target_keys, rationale, status, priority, result, created_at, executed_at, error
                 FROM kb_operation_queue WHERE plan_id = ?1 ORDER BY priority".to_string(),
                vec![Box::new(pid.to_string()) as Box<dyn rusqlite::ToSql>],
            ),
            (None, Some(s)) => (
                "SELECT id, plan_id, task_id, operation, target_keys, rationale, status, priority, result, created_at, executed_at, error
                 FROM kb_operation_queue WHERE status = ?1 ORDER BY created_at DESC, priority".to_string(),
                vec![Box::new(s.to_string()) as Box<dyn rusqlite::ToSql>],
            ),
            (None, None) => (
                "SELECT id, plan_id, task_id, operation, target_keys, rationale, status, priority, result, created_at, executed_at, error
                 FROM kb_operation_queue ORDER BY created_at DESC, priority".to_string(),
                vec![],
            ),
        };
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(KBOperationRow {
                id: row.get(0)?,
                plan_id: row.get(1)?,
                task_id: row.get(2)?,
                operation: row.get(3)?,
                target_keys: row.get(4)?,
                rationale: row.get(5)?,
                status: row.get(6)?,
                priority: row.get(7)?,
                result: row.get(8)?,
                created_at: row.get(9)?,
                executed_at: row.get(10)?,
                error: row.get(11)?,
            })
        })?;
        rows.collect()
    }

    /// Update operation status
    pub fn kb_ops_update_status(
        &self,
        op_id: &str,
        status: &str,
        result: Option<&str>,
        error: Option<&str>,
    ) -> SqliteResult<bool> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        let executed_at = if status == "done" || status == "failed" { Some(now.as_str()) } else { None };
        let updated = conn.execute(
            "UPDATE kb_operation_queue SET status = ?1, result = ?2, error = ?3, executed_at = ?4 WHERE id = ?5",
            params![status, result, error, executed_at, op_id],
        )?;
        Ok(updated > 0)
    }

    /// Update a dispatched kb_operation by its associated submit task_id.
    /// Called when a submit task finishes — updates status and stores the task result.
    pub fn kb_ops_complete_by_task_id(
        &self,
        task_id: &str,
        new_status: &str,
        result: Option<&str>,
    ) -> SqliteResult<bool> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        let pattern = format!("%task_id={}%", task_id);
        let updated = conn.execute(
            "UPDATE kb_operation_queue SET status = ?1, result = COALESCE(?2, result), executed_at = ?3
             WHERE status = 'dispatched' AND result LIKE ?4",
            params![new_status, result, now, pattern],
        )?;
        Ok(updated > 0)
    }

    /// Expire stale pending kb_operations older than `ttl_secs` seconds.
    /// Returns the number of expired operations.
    pub fn kb_ops_expire_stale(&self, ttl_secs: i64) -> SqliteResult<usize> {
        let conn = self.conn();
        let cutoff = (chrono::Utc::now() - chrono::Duration::seconds(ttl_secs)).to_rfc3339();
        let updated = conn.execute(
            "UPDATE kb_operation_queue SET status = 'expired', executed_at = ?1
             WHERE status = 'pending' AND created_at < ?2",
            params![chrono::Utc::now().to_rfc3339(), cutoff],
        )?;
        Ok(updated)
    }

    /// Get queue status summary for a plan
    pub fn kb_ops_plan_summary(&self, plan_id: &str) -> SqliteResult<serde_json::Value> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT status, COUNT(*) FROM kb_operation_queue WHERE plan_id = ?1 GROUP BY status"
        )?;
        let mut summary = serde_json::Map::new();
        let mut total = 0i64;
        stmt.query_map(params![plan_id], |row| {
            let s: String = row.get(0)?;
            let c: i64 = row.get(1)?;
            Ok((s, c))
        })?.for_each(|r| {
            if let Ok((s, c)) = r {
                summary.insert(s, serde_json::json!(c));
                total += c;
            }
        });
        summary.insert("total".to_string(), serde_json::json!(total));
        Ok(serde_json::Value::Object(summary))
    }

    // ============ Skill Engine ============

    /// Upsert a skill topic (metadata)
    pub fn skill_topic_upsert(
        &self,
        topic: &str,
        description: Option<&str>,
        aka: Option<&str>,
        allowed_tools: Option<&str>,
        file_path: &str,
        requires_json: Option<&str>,
        actions_json: Option<&str>,
    ) -> SqliteResult<()> {
        self.skill_topic_upsert_full(topic, description, aka, allowed_tools, file_path, requires_json, actions_json, None)
    }

    /// Upsert a skill topic with context_hooks (Phase 4)
    pub fn skill_topic_upsert_full(
        &self,
        topic: &str,
        description: Option<&str>,
        aka: Option<&str>,
        allowed_tools: Option<&str>,
        file_path: &str,
        requires_json: Option<&str>,
        actions_json: Option<&str>,
        context_hooks_json: Option<&str>,
    ) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO skill_topics (topic, description, aka, allowed_tools, file_path, requires_json, actions_json, context_hooks_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
             ON CONFLICT(topic) DO UPDATE SET
                description = ?2, aka = ?3, allowed_tools = ?4, file_path = ?5, requires_json = ?6, actions_json = ?7, context_hooks_json = ?8, updated_at = ?9",
            params![topic, description, aka, allowed_tools, file_path, requires_json, actions_json, context_hooks_json, now],
        )?;
        Ok(())
    }

    /// Get a skill topic by name
    pub fn skill_topic_get(&self, topic: &str) -> SqliteResult<Option<SkillTopic>> {
        let conn = self.conn();
        conn.query_row(
            "SELECT topic, description, aka, allowed_tools, file_path,
                    hit_count, last_hit_at, fragment_count, total_lines, checksum,
                    requires_json, actions_json, context_hooks_json, created_at, updated_at
             FROM skill_topics WHERE topic = ?1",
            params![topic],
            |row| {
                Ok(SkillTopic {
                    topic: row.get(0)?,
                    description: row.get(1)?,
                    aka: row.get(2)?,
                    allowed_tools: row.get(3)?,
                    file_path: row.get(4)?,
                    hit_count: row.get(5)?,
                    last_hit_at: row.get(6)?,
                    fragment_count: row.get(7)?,
                    total_lines: row.get(8)?,
                    checksum: row.get(9)?,
                    requires_json: row.get(10)?,
                    actions_json: row.get(11)?,
                    context_hooks_json: row.get(12)?,
                    created_at: row.get(13)?,
                    updated_at: row.get(14)?,
                })
            },
        )
        .optional()
    }

    /// List all skill topics
    pub fn skill_topic_list(&self) -> SqliteResult<Vec<SkillTopic>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT topic, description, aka, allowed_tools, file_path,
                    hit_count, last_hit_at, fragment_count, total_lines, checksum,
                    requires_json, actions_json, context_hooks_json, created_at, updated_at
             FROM skill_topics ORDER BY topic",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SkillTopic {
                topic: row.get(0)?,
                description: row.get(1)?,
                aka: row.get(2)?,
                allowed_tools: row.get(3)?,
                file_path: row.get(4)?,
                hit_count: row.get(5)?,
                last_hit_at: row.get(6)?,
                fragment_count: row.get(7)?,
                total_lines: row.get(8)?,
                checksum: row.get(9)?,
                requires_json: row.get(10)?,
                actions_json: row.get(11)?,
                context_hooks_json: row.get(12)?,
                created_at: row.get(13)?,
                updated_at: row.get(14)?,
            })
        })?;
        rows.collect()
    }

    /// Record a hit on a skill topic (for Hook injection tracking)
    pub fn skill_topic_hit(&self, topic: &str) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE skill_topics SET hit_count = hit_count + 1, last_hit_at = ?1 WHERE topic = ?2",
            params![now, topic],
        )?;
        Ok(())
    }

    /// Insert a skill block (section or fragment)
    pub fn skill_block_insert(
        &self,
        topic: &str,
        block_type: &str,
        title: Option<&str>,
        content: &str,
        sort_order: i32,
    ) -> SqliteResult<String> {
        let conn = self.conn();
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO skill_blocks (id, topic, block_type, title, content, sort_order, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?7)",
            params![id, topic, block_type, title, content, sort_order, now],
        )?;

        // Sync FTS
        let rowid: i64 = conn.query_row(
            "SELECT rowid FROM skill_blocks WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO skill_fts(rowid, topic, title, content) VALUES (?1, ?2, ?3, ?4)",
            params![rowid, topic, title.unwrap_or(""), content],
        )?;

        // Update fragment count if fragment
        if block_type == "fragment" {
            conn.execute(
                "UPDATE skill_topics SET fragment_count = fragment_count + 1 WHERE topic = ?1",
                params![topic],
            )?;
        }

        Ok(id)
    }

    /// Update a skill block's content
    pub fn skill_block_update(&self, id: &str, content: &str) -> SqliteResult<bool> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        let updated = conn.execute(
            "UPDATE skill_blocks SET content = ?1, updated_at = ?2 WHERE id = ?3 AND status = 'active'",
            params![content, now, id],
        )?;
        if updated > 0 {
            // Rebuild FTS for this block
            let rowid: i64 = conn.query_row(
                "SELECT rowid FROM skill_blocks WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )?;
            let (topic, title): (String, Option<String>) = conn.query_row(
                "SELECT topic, title FROM skill_blocks WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO skill_fts(rowid, topic, title, content) VALUES (?1, ?2, ?3, ?4)",
                params![rowid, topic, title.unwrap_or_default(), content],
            )?;
        }
        Ok(updated > 0)
    }

    /// Get all active blocks for a topic, ordered by sort_order
    pub fn skill_blocks_for_topic(&self, topic: &str) -> SqliteResult<Vec<SkillBlock>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, topic, block_type, title, content, sort_order, status, created_at, updated_at
             FROM skill_blocks
             WHERE topic = ?1 AND status = 'active'
             ORDER BY block_type DESC, sort_order ASC",
        )?;
        let rows = stmt.query_map(params![topic], |row| {
            Ok(SkillBlock {
                id: row.get(0)?,
                topic: row.get(1)?,
                block_type: row.get(2)?,
                title: row.get(3)?,
                content: row.get(4)?,
                sort_order: row.get(5)?,
                status: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;
        rows.collect()
    }

    /// Search skill blocks using FTS5
    pub fn skill_search_fts(&self, query: &str) -> SqliteResult<Vec<SkillSearchResult>> {
        let conn = self.read_conn();
        let tokens: Vec<&str> = query.split_whitespace().collect();
        if tokens.is_empty() {
            return Ok(vec![]);
        }

        let fts_query = tokens
            .iter()
            .map(|t| format!("\"{}\"", t.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" OR ");

        let mut stmt = conn.prepare(
            "SELECT sb.topic, sb.title, snippet(skill_fts, 2, '>>>', '<<<', '...', 32) as snippet,
                    st.file_path, st.description
             FROM skill_fts
             JOIN skill_blocks sb ON sb.rowid = skill_fts.rowid
             JOIN skill_topics st ON st.topic = sb.topic
             WHERE skill_fts MATCH ?1 AND sb.status = 'active'
             ORDER BY rank
             LIMIT 20",
        )?;

        let rows = stmt.query_map(params![fts_query], |row| {
            Ok(SkillSearchResult {
                topic: row.get(0)?,
                section_title: row.get(1)?,
                snippet: row.get(2)?,
                file_path: row.get(3)?,
                description: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// Search skill topics by name/aka (exact + fuzzy)
    pub fn skill_search_topics(&self, query: &str) -> SqliteResult<Vec<SkillTopic>> {
        let conn = self.conn();
        let query_lower = query.to_lowercase();
        let pattern = format!("%{}%", query_lower);

        let mut stmt = conn.prepare(
            "SELECT topic, description, aka, allowed_tools, file_path,
                    hit_count, last_hit_at, fragment_count, total_lines, checksum,
                    requires_json, actions_json, context_hooks_json, created_at, updated_at
             FROM skill_topics
             WHERE LOWER(topic) LIKE ?1
                OR LOWER(COALESCE(description, '')) LIKE ?1
                OR LOWER(COALESCE(aka, '')) LIKE ?1
             ORDER BY hit_count DESC
             LIMIT 10",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok(SkillTopic {
                topic: row.get(0)?,
                description: row.get(1)?,
                aka: row.get(2)?,
                allowed_tools: row.get(3)?,
                file_path: row.get(4)?,
                hit_count: row.get(5)?,
                last_hit_at: row.get(6)?,
                fragment_count: row.get(7)?,
                total_lines: row.get(8)?,
                checksum: row.get(9)?,
                requires_json: row.get(10)?,
                actions_json: row.get(11)?,
                context_hooks_json: row.get(12)?,
                created_at: row.get(13)?,
                updated_at: row.get(14)?,
            })
        })?;
        rows.collect()
    }

    /// Update skill_topics metadata after materialization
    pub fn skill_topic_update_stats(&self, topic: &str, total_lines: i32, checksum: &str) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE skill_topics SET total_lines = ?1, checksum = ?2, updated_at = ?3 WHERE topic = ?4",
            params![total_lines, checksum, now, topic],
        )?;
        Ok(())
    }

    /// Set block status (for lifecycle management: active → merged/archived)
    pub fn skill_block_set_status(&self, id: &str, status: &str) -> SqliteResult<bool> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        let updated = conn.execute(
            "UPDATE skill_blocks SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, now, id],
        )?;
        Ok(updated > 0)
    }

    /// Delete all blocks for a topic (used before re-ingest)
    pub fn skill_blocks_delete_topic(&self, topic: &str) -> SqliteResult<usize> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM skill_blocks WHERE topic = ?1",
            params![topic],
        )?;
        Ok(deleted)
    }

    /// Rebuild skill FTS index from scratch
    pub fn skill_rebuild_fts(&self) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute_batch("INSERT INTO skill_fts(skill_fts) VALUES('rebuild')")?;
        Ok(())
    }

    /// Insert a skill execution log entry
    pub fn skill_execution_insert(
        &self,
        id: &str,
        skill_topic: &str,
        action_id: &str,
        steps_total: i32,
        triggered_by: &str,
    ) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO skill_executions (id, skill_topic, action_id, status, steps_total, triggered_by, created_at)
             VALUES (?1, ?2, ?3, 'running', ?4, ?5, ?6)",
            params![id, skill_topic, action_id, steps_total, triggered_by, now],
        )?;
        Ok(())
    }

    /// Update a skill execution (progress or completion)
    pub fn skill_execution_update(
        &self,
        id: &str,
        status: &str,
        steps_completed: i32,
        context_json: Option<&str>,
        error: Option<&str>,
    ) -> SqliteResult<()> {
        self.skill_execution_update_with_duration(id, status, steps_completed, context_json, error, None)
    }

    /// Update a skill execution with optional duration tracking
    pub fn skill_execution_update_with_duration(
        &self,
        id: &str,
        status: &str,
        steps_completed: i32,
        context_json: Option<&str>,
        error: Option<&str>,
        duration_ms: Option<i64>,
    ) -> SqliteResult<()> {
        let conn = self.conn();
        let completed_at = if status == "success" || status == "failed" || status == "cancelled" {
            Some(chrono::Utc::now().to_rfc3339())
        } else {
            None
        };
        conn.execute(
            "UPDATE skill_executions SET status = ?1, steps_completed = ?2, context_json = ?3, error = ?4, completed_at = ?5, duration_ms = ?6
             WHERE id = ?7",
            params![status, steps_completed, context_json, error, completed_at, duration_ms, id],
        )?;
        Ok(())
    }

    /// Get aggregated execution statistics for a skill topic
    pub fn skill_execution_stats(&self, topic: Option<&str>) -> SqliteResult<Vec<crate::types::SkillExecutionStat>> {
        let conn = self.read_conn();
        let row_mapper = |row: &rusqlite::Row| {
            Ok(crate::types::SkillExecutionStat {
                action_id: row.get(0)?,
                total: row.get(1)?,
                successes: row.get(2)?,
                failures: row.get(3)?,
                avg_duration_ms: row.get(4)?,
                last_run: row.get(5)?,
            })
        };

        let mut stats = Vec::new();
        if let Some(t) = topic {
            let mut stmt = conn.prepare(
                "SELECT action_id, COUNT(*) as total,
                        SUM(CASE WHEN status='success' THEN 1 ELSE 0 END) as successes,
                        SUM(CASE WHEN status='failed' THEN 1 ELSE 0 END) as failures,
                        AVG(duration_ms) as avg_duration_ms,
                        MAX(created_at) as last_run
                 FROM skill_executions
                 WHERE skill_topic = ?1
                 GROUP BY action_id
                 ORDER BY last_run DESC"
            )?;
            let rows = stmt.query_map(params![t], row_mapper)?;
            for s in rows { stats.push(s?); }
        } else {
            let mut stmt = conn.prepare(
                "SELECT skill_topic || '/' || action_id as action_id, COUNT(*) as total,
                        SUM(CASE WHEN status='success' THEN 1 ELSE 0 END) as successes,
                        SUM(CASE WHEN status='failed' THEN 1 ELSE 0 END) as failures,
                        AVG(duration_ms) as avg_duration_ms,
                        MAX(created_at) as last_run
                 FROM skill_executions
                 GROUP BY skill_topic, action_id
                 ORDER BY last_run DESC"
            )?;
            let rows = stmt.query_map([], row_mapper)?;
            for s in rows { stats.push(s?); }
        }
        Ok(stats)
    }

    /// Check if a skill action is currently running (for concurrency guard)
    pub fn skill_execution_is_running(&self, skill_topic: &str, action_id: &str) -> SqliteResult<bool> {
        let conn = self.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM skill_executions WHERE skill_topic = ?1 AND action_id = ?2 AND status = 'running'",
            params![skill_topic, action_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    // ============ Skill Version Rollback (Phase 4) ============

    /// Save a skill version snapshot before materialize
    pub fn skill_version_save(&self, topic: &str, content: &str, checksum: &str) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO skill_versions (topic, content, checksum, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![topic, content, checksum, now],
        )?;
        // Keep only the 10 most recent versions per topic
        conn.execute(
            "DELETE FROM skill_versions WHERE topic = ?1 AND id NOT IN (
                SELECT id FROM skill_versions WHERE topic = ?1 ORDER BY id DESC LIMIT 10
            )",
            params![topic],
        )?;
        Ok(())
    }

    /// List recent versions for a skill topic
    pub fn skill_version_list(&self, topic: &str, limit: i64) -> SqliteResult<Vec<crate::types::SkillVersion>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, topic, content, checksum, created_at FROM skill_versions
             WHERE topic = ?1 ORDER BY id DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![topic, limit], |row| {
            Ok(crate::types::SkillVersion {
                id: row.get(0)?,
                topic: row.get(1)?,
                content: row.get(2)?,
                checksum: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        let mut versions = Vec::new();
        for v in rows { versions.push(v?); }
        Ok(versions)
    }

    /// Get a specific skill version by ID
    pub fn skill_version_get(&self, id: i64) -> SqliteResult<Option<crate::types::SkillVersion>> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT id, topic, content, checksum, created_at FROM skill_versions WHERE id = ?1",
            params![id],
            |row| {
                Ok(crate::types::SkillVersion {
                    id: row.get(0)?,
                    topic: row.get(1)?,
                    content: row.get(2)?,
                    checksum: row.get(3)?,
                    created_at: row.get(4)?,
                })
            },
        ).optional()
    }

    // ============ Message Pipeline Tracking ============



    /// Get conversations pending deep analysis (conversation-level watermark).
    /// Returns completed user conversations that haven't been analyzed at the current version.
    pub fn get_pending_deep_analysis(&self, current_version: i32, max_retries: i32) -> SqliteResult<Vec<Conversation>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM conversations
             WHERE status = 'completed'
               AND conversation_type = 'user'
               AND ended_at < datetime('now', '-5 minutes')
               AND analysis_retries < ?1
               AND (analyzed_at IS NULL OR analysis_version < ?2)

             UNION ALL

             SELECT * FROM conversations
             WHERE status = 'active'
               AND conversation_type = 'user'
               AND analysis_retries < ?1
               AND (SELECT COUNT(*) FROM conversation_messages m
                    WHERE m.session_id = conversations.id
                      AND m.id > COALESCE(conversations.deep_analyzed_message_id, 0)
                      AND m.role IN ('user', 'assistant')) >= 100

             ORDER BY started_at ASC"
        )?;
        let rows = stmt.query_map(params![max_retries, current_version], |row| Self::row_to_conversation(row))?;
        let mut convs = Vec::new();
        for c in rows { convs.push(c?); }
        Ok(convs)
    }

    /// Lightweight check: are there any conversations pending deep analysis?
    pub fn has_pending_deep_analysis(&self, current_version: i32, max_retries: i32) -> SqliteResult<bool> {
        let conn = self.conn();
        conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM conversations
                WHERE status = 'completed'
                  AND conversation_type = 'user'
                  AND ended_at < datetime('now', '-5 minutes')
                  AND analysis_retries < ?1
                  AND (analyzed_at IS NULL OR analysis_version < ?2)

                UNION ALL

                SELECT 1 FROM conversations
                WHERE status = 'active'
                  AND conversation_type = 'user'
                  AND analysis_retries < ?1
                  AND (SELECT COUNT(*) FROM conversation_messages m
                       WHERE m.session_id = conversations.id
                         AND m.id > COALESCE(conversations.deep_analyzed_message_id, 0)
                         AND m.role IN ('user', 'assistant')) >= 100
            )",
            params![max_retries, current_version],
            |row| row.get(0),
        )
    }

    /// Count conversations pending deep analysis.
    pub fn count_pending_deep_analysis(&self, current_version: i32, max_retries: i32) -> SqliteResult<i64> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT COUNT(*) FROM (
                SELECT id FROM conversations
                WHERE status = 'completed'
                  AND conversation_type = 'user'
                  AND ended_at < datetime('now', '-5 minutes')
                  AND analysis_retries < ?1
                  AND (analyzed_at IS NULL OR analysis_version < ?2)

                UNION ALL

                SELECT id FROM conversations
                WHERE status = 'active'
                  AND conversation_type = 'user'
                  AND analysis_retries < ?1
                  AND (SELECT COUNT(*) FROM conversation_messages m
                       WHERE m.session_id = conversations.id
                         AND m.id > COALESCE(conversations.deep_analyzed_message_id, 0)
                         AND m.role IN ('user', 'assistant')) >= 100
            )",
            params![max_retries, current_version],
            |row| row.get(0),
        )
    }

    /// Count distinct sessions with pending realtime messages.
    pub fn count_pending_realtime(&self) -> SqliteResult<i64> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT COUNT(DISTINCT c.id) FROM conversations c
             JOIN conversation_messages m ON c.id = m.session_id
             WHERE c.conversation_type = 'user'
               AND m.timestamp > COALESCE(c.realtime_forwarded_at, c.started_at)
               AND m.role IN ('user', 'assistant')",
            [],
            |row| row.get(0),
        )
    }

    /// Per-session pending realtime message summary (session_id, msg_count, oldest_timestamp).
    pub fn pending_realtime_detail(&self) -> SqliteResult<Vec<(String, i64, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT c.id, COUNT(*) as cnt, MIN(m.timestamp) as oldest
             FROM conversations c
             JOIN conversation_messages m ON c.id = m.session_id
             WHERE c.conversation_type = 'user'
               AND m.timestamp > COALESCE(c.realtime_forwarded_at, c.started_at)
               AND m.role IN ('user', 'assistant')
             GROUP BY c.id
             ORDER BY cnt DESC
             LIMIT 20"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Per-conversation pending deep analysis summary (id, ended_at, retries).
    pub fn pending_deep_detail(&self, current_version: i32, max_retries: i32) -> SqliteResult<Vec<(String, String, i32)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, COALESCE(ended_at, '[active]'), analysis_retries FROM conversations
             WHERE status = 'completed'
               AND conversation_type = 'user'
               AND ended_at < datetime('now', '-5 minutes')
               AND analysis_retries < ?1
               AND (analyzed_at IS NULL OR analysis_version < ?2)

             UNION ALL

             SELECT id, '[checkpoint]', analysis_retries FROM conversations
             WHERE status = 'active'
               AND conversation_type = 'user'
               AND analysis_retries < ?1
               AND (SELECT COUNT(*) FROM conversation_messages m
                    WHERE m.session_id = conversations.id
                      AND m.id > COALESCE(conversations.deep_analyzed_message_id, 0)
                      AND m.role IN ('user', 'assistant')) >= 100

             ORDER BY 2 ASC
             LIMIT 20"
        )?;
        let rows = stmt.query_map(params![max_retries, current_version], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i32>(2)?))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Mark a conversation's deep analysis as complete with the given version.
    pub fn mark_analysis_complete(&self, id: &str, version: i32) -> SqliteResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET analyzed_at = ?1, analysis_version = ?2, analysis_retries = 0, deep_analyzed_message_id = 0 WHERE id = ?3",
            params![now, version, id],
        )?;
        Ok(())
    }

    /// Advance the deep analysis checkpoint watermark for an active session.
    pub fn update_deep_checkpoint(&self, id: &str, message_id: i64) -> SqliteResult<()> {
        self.conn().execute(
            "UPDATE conversations SET deep_analyzed_message_id = ?1, analysis_retries = 0 WHERE id = ?2",
            params![message_id, id],
        )?;
        Ok(())
    }

    /// Increment analysis retry count for a failed deep analysis attempt.
    pub fn mark_analysis_failed(&self, id: &str) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET analysis_retries = analysis_retries + 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    // ============ Conversations ============

    /// Upsert a conversation session
    pub fn upsert_conversation(&self, conv: &Conversation) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO conversations (id, project, slot_id, source, model, git_branch, jsonl_path, parent_session_id, task_id, message_count, started_at, ended_at, status, analyzed_at, conversation_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT(id) DO UPDATE SET
                slot_id = COALESCE(?3, slot_id),
                model = COALESCE(?5, model),
                git_branch = COALESCE(?6, git_branch),
                message_count = ?10,
                ended_at = ?12,
                status = ?13,
                conversation_type = COALESCE(?15, conversation_type)",
            params![
                conv.id, conv.project, conv.slot_id, conv.source, conv.model,
                conv.git_branch, conv.jsonl_path, conv.parent_session_id, conv.task_id,
                conv.message_count, conv.started_at, conv.ended_at, conv.status,
                conv.analyzed_at, conv.conversation_type,
            ],
        )?;
        Ok(())
    }

    /// Get child (subagent) conversations for a parent session.
    pub fn get_child_conversations(&self, parent_session_id: &str) -> SqliteResult<Vec<Conversation>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM conversations WHERE parent_session_id = ?1 ORDER BY started_at ASC"
        )?;
        let rows = stmt.query_map(params![parent_session_id], |row| Self::row_to_conversation(row))?;
        let mut convs = Vec::new();
        for c in rows { convs.push(c?); }
        Ok(convs)
    }

    /// Insert a conversation message, returns the auto-increment ID.
    /// Dedup via UNIQUE index on message_uuid — duplicate inserts are silently ignored.
    pub fn insert_conversation_message(&self, msg: &ConversationMessage) -> SqliteResult<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR IGNORE INTO conversation_messages (session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                msg.session_id, msg.role, msg.content, msg.raw_content,
                msg.message_uuid, msg.parent_uuid, msg.model, msg.timestamp, msg.metadata,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Batch insert conversation messages within a transaction.
    /// Returns IDs of actually inserted rows (dedup via UNIQUE index).
    pub fn insert_conversation_messages_batch(&self, messages: &[ConversationMessage]) -> SqliteResult<Vec<i64>> {
        if messages.is_empty() {
            return Ok(Vec::new());
        }
        let session_id = &messages[0].session_id;
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let mut inserted_ids = Vec::new();
        for msg in messages {
            tx.execute(
                "INSERT OR IGNORE INTO conversation_messages (session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    msg.session_id, msg.role, msg.content, msg.raw_content,
                    msg.message_uuid, msg.parent_uuid, msg.model, msg.timestamp, msg.metadata,
                ],
            )?;
            if tx.changes() > 0 {
                inserted_ids.push(tx.last_insert_rowid());
            }
        }
        if !inserted_ids.is_empty() {
            // Update message count once at the end
            tx.execute(
                "UPDATE conversations SET message_count = (SELECT COUNT(*) FROM conversation_messages WHERE session_id = ?1) WHERE id = ?1",
                params![session_id],
            )?;
        }
        tx.commit()?;
        Ok(inserted_ids)
    }

    /// Get a conversation by ID
    pub fn get_conversation(&self, id: &str) -> SqliteResult<Option<Conversation>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare("SELECT * FROM conversations WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_conversation(row)?))
        } else {
            Ok(None)
        }
    }

    /// List conversations, optionally filtered by status and conversation_type.
    /// conv_type: None = user+worker (default), Some("meta") = system, Some("system") = meta+worker, Some("all") = everything.
    pub fn list_conversations(&self, status: Option<&str>, limit: i64, conv_type: Option<&str>) -> SqliteResult<Vec<Conversation>> {
        let conn = self.read_conn();
        let mut convs = Vec::new();
        let type_clause = match conv_type {
            Some("all") => String::new(),
            Some("meta") => " AND conversation_type = 'meta'".to_string(),
            Some("worker") => " AND conversation_type = 'worker'".to_string(),
            Some("system") => " AND conversation_type IN ('meta', 'worker')".to_string(),
            _ => " AND conversation_type IN ('user', 'worker')".to_string(),
        };
        if let Some(s) = status {
            let sql = format!(
                "SELECT * FROM conversations WHERE status = ?1{} ORDER BY started_at DESC LIMIT ?2",
                type_clause
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![s, limit], |row| Self::row_to_conversation(row))?;
            for c in rows { convs.push(c?); }
        } else {
            {
                let sql = format!(
                    "SELECT * FROM conversations WHERE status = 'active'{} ORDER BY started_at DESC",
                    type_clause
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map([], |row| Self::row_to_conversation(row))?;
                for c in rows { convs.push(c?); }
            }
            let remaining = limit - convs.len() as i64;
            if remaining > 0 {
                let sql = format!(
                    "SELECT * FROM conversations WHERE status != 'active'{} ORDER BY started_at DESC LIMIT ?1",
                    type_clause
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![remaining], |row| Self::row_to_conversation(row))?;
                for c in rows { convs.push(c?); }
            }
            convs.sort_by(|a, b| {
                let a_active = a.status == "active";
                let b_active = b.status == "active";
                match (a_active, b_active) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => b.started_at.cmp(&a.started_at),
                }
            });
        }
        Ok(convs)
    }

    /// Get conversation messages, optionally since a given ID
    pub fn get_conversation_messages(
        &self,
        session_id: &str,
        since_id: Option<i64>,
        limit: i64,
    ) -> SqliteResult<Vec<ConversationMessage>> {
        let conn = self.read_conn();
        let mut msgs = Vec::new();
        if let Some(since) = since_id {
            let mut stmt = conn.prepare(
                "SELECT * FROM conversation_messages WHERE session_id = ?1 AND id > ?2 ORDER BY id ASC LIMIT ?3"
            )?;
            let rows = stmt.query_map(params![session_id, since, limit], |row| Self::row_to_conversation_message(row))?;
            for m in rows { msgs.push(m?); }
        } else {
            // Return last N messages
            let mut stmt = conn.prepare(
                "SELECT * FROM (SELECT * FROM conversation_messages WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2) ORDER BY id ASC"
            )?;
            let rows = stmt.query_map(params![session_id, limit], |row| Self::row_to_conversation_message(row))?;
            for m in rows { msgs.push(m?); }
        }
        Ok(msgs)
    }

    /// Search conversation messages by content
    pub fn search_conversation_messages(&self, query: &str, limit: i64) -> SqliteResult<Vec<ConversationMessage>> {
        let pattern = format!("%{}%", query);
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM conversation_messages WHERE content LIKE ?1 ORDER BY timestamp DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![pattern, limit], |row| Self::row_to_conversation_message(row))?;
        let mut msgs = Vec::new();
        for m in rows { msgs.push(m?); }
        Ok(msgs)
    }

    // ============ Conversation Tool Calls (Audit) ============

    /// Insert a tool call record (from tool_use block in assistant message)
    pub fn insert_tool_call(&self, tc: &ToolCallRecord) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR IGNORE INTO conversation_tool_calls (id, session_id, message_id, tool_name, input_summary, raw_input, output_summary, raw_output, status, duration_ms, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                tc.id, tc.session_id, tc.message_id, tc.tool_name,
                tc.input_summary, tc.raw_input, tc.output_summary, tc.raw_output,
                tc.status, tc.duration_ms, tc.timestamp,
            ],
        )?;
        Ok(())
    }

    /// Batch insert tool call records
    pub fn insert_tool_calls_batch(&self, calls: &[ToolCallRecord]) -> SqliteResult<usize> {
        if calls.is_empty() {
            return Ok(0);
        }
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let mut count = 0usize;
        for tc in calls {
            tx.execute(
                "INSERT OR IGNORE INTO conversation_tool_calls (id, session_id, message_id, tool_name, input_summary, raw_input, output_summary, raw_output, status, duration_ms, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    tc.id, tc.session_id, tc.message_id, tc.tool_name,
                    tc.input_summary, tc.raw_input, tc.output_summary, tc.raw_output,
                    tc.status, tc.duration_ms, tc.timestamp,
                ],
            )?;
            if tx.changes() > 0 {
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    /// Update tool call with output (from tool_result block in user message)
    pub fn update_tool_call_output(&self, tool_use_id: &str, output_summary: &str, raw_output: &str, status: &str) -> SqliteResult<bool> {
        let conn = self.conn();
        let changes = conn.execute(
            "UPDATE conversation_tool_calls SET output_summary = ?1, raw_output = ?2, status = ?3 WHERE id = ?4",
            params![output_summary, raw_output, status, tool_use_id],
        )?;
        Ok(changes > 0)
    }

    /// Get tool calls for a session (for audit trace)
    pub fn get_tool_calls_by_session(&self, session_id: &str, tool_filter: Option<&[String]>, limit: i64) -> SqliteResult<Vec<ToolCallRecord>> {
        let conn = self.read_conn();
        let mut calls = Vec::new();
        if let Some(filter) = tool_filter {
            if filter.is_empty() {
                return Ok(calls);
            }
            let placeholders: Vec<String> = (0..filter.len()).map(|i| format!("?{}", i + 2)).collect();
            let sql = format!(
                "SELECT * FROM conversation_tool_calls WHERE session_id = ?1 AND tool_name IN ({}) ORDER BY rowid ASC LIMIT ?{}",
                placeholders.join(","),
                filter.len() + 2
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            params_vec.push(Box::new(session_id.to_string()));
            for f in filter {
                params_vec.push(Box::new(f.clone()));
            }
            params_vec.push(Box::new(limit));
            let refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
            let rows = stmt.query_map(refs.as_slice(), |row| Self::row_to_tool_call(row))?;
            for r in rows { calls.push(r?); }
        } else {
            let mut stmt = conn.prepare(
                "SELECT * FROM conversation_tool_calls WHERE session_id = ?1 ORDER BY rowid ASC LIMIT ?2"
            )?;
            let rows = stmt.query_map(params![session_id, limit], |row| Self::row_to_tool_call(row))?;
            for r in rows { calls.push(r?); }
        }
        Ok(calls)
    }

    /// Get a single tool call by ID (for audit detail drilldown)
    pub fn get_tool_call_by_id(&self, tool_use_id: &str) -> SqliteResult<Option<ToolCallRecord>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare("SELECT * FROM conversation_tool_calls WHERE id = ?1")?;
        let mut rows = stmt.query(params![tool_use_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_tool_call(row)?))
        } else {
            Ok(None)
        }
    }

    /// Get tool call statistics for a session
    pub fn get_tool_call_stats(&self, session_id: &str) -> SqliteResult<Vec<(String, i64, i64, i64)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT tool_name,
                    COUNT(*) as total,
                    SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) as success_count,
                    SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END) as error_count
             FROM conversation_tool_calls WHERE session_id = ?1
             GROUP BY tool_name ORDER BY total DESC"
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut stats = Vec::new();
        for r in rows { stats.push(r?); }
        Ok(stats)
    }

    fn row_to_tool_call(row: &rusqlite::Row) -> SqliteResult<ToolCallRecord> {
        Ok(ToolCallRecord {
            id: row.get("id")?,
            session_id: row.get("session_id")?,
            message_id: row.get("message_id")?,
            tool_name: row.get("tool_name")?,
            input_summary: row.get("input_summary")?,
            raw_input: row.get("raw_input")?,
            output_summary: row.get("output_summary")?,
            raw_output: row.get("raw_output")?,
            status: row.get("status")?,
            duration_ms: row.get("duration_ms")?,
            timestamp: row.get("timestamp")?,
        })
    }

    /// Batch insert conversation events (system events from JSONL: turn_duration, etc.)
    pub fn insert_conversation_events_batch(&self, events: &[crate::types::ConversationEvent]) -> SqliteResult<usize> {
        if events.is_empty() {
            return Ok(0);
        }
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let mut count = 0usize;
        for event in events {
            tx.execute(
                "INSERT INTO conversation_events (session_id, event_type, content, raw_data, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![event.session_id, event.event_type, event.content, event.raw_data, event.timestamp],
            )?;
            count += 1;
        }
        tx.commit()?;
        Ok(count)
    }

    /// Get conversation events for a session, optionally filtered by event_type
    pub fn get_conversation_events(
        &self,
        session_id: &str,
        event_type: Option<&str>,
        limit: i64,
    ) -> SqliteResult<Vec<crate::types::ConversationEvent>> {
        let conn = self.read_conn();
        let mut events = Vec::new();
        if let Some(et) = event_type {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, event_type, content, raw_data, timestamp
                 FROM conversation_events WHERE session_id = ?1 AND event_type = ?2
                 ORDER BY id ASC LIMIT ?3"
            )?;
            let rows = stmt.query_map(params![session_id, et, limit], |row| {
                Ok(crate::types::ConversationEvent {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    event_type: row.get(2)?,
                    content: row.get(3)?,
                    raw_data: row.get(4)?,
                    timestamp: row.get(5)?,
                })
            })?;
            for e in rows { events.push(e?); }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, event_type, content, raw_data, timestamp
                 FROM conversation_events WHERE session_id = ?1
                 ORDER BY id ASC LIMIT ?2"
            )?;
            let rows = stmt.query_map(params![session_id, limit], |row| {
                Ok(crate::types::ConversationEvent {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    event_type: row.get(2)?,
                    content: row.get(3)?,
                    raw_data: row.get(4)?,
                    timestamp: row.get(5)?,
                })
            })?;
            for e in rows { events.push(e?); }
        }
        Ok(events)
    }

    /// Get agent trajectory: all agent_* messages linked to a specific tool_use_id
    pub fn get_agent_trajectory(
        &self,
        tool_use_id: &str,
        limit: i64,
    ) -> SqliteResult<Vec<ConversationMessage>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata
             FROM conversation_messages WHERE parent_uuid = ?1 AND role LIKE 'agent_%'
             ORDER BY id ASC LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![tool_use_id, limit], |row| {
            Self::row_to_conversation_message(row)
        })?;
        let mut msgs = Vec::new();
        for m in rows { msgs.push(m?); }
        Ok(msgs)
    }

    /// Get event type summary across all sessions or for a specific session
    pub fn get_event_type_summary(
        &self,
        session_id: Option<&str>,
    ) -> SqliteResult<Vec<(String, i64)>> {
        let conn = self.read_conn();
        let mut summary = Vec::new();
        if let Some(sid) = session_id {
            let mut stmt = conn.prepare(
                "SELECT event_type, COUNT(*) as cnt FROM conversation_events
                 WHERE session_id = ?1 GROUP BY event_type ORDER BY cnt DESC"
            )?;
            let rows = stmt.query_map(params![sid], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            for r in rows { summary.push(r?); }
        } else {
            let mut stmt = conn.prepare(
                "SELECT event_type, COUNT(*) as cnt FROM conversation_events
                 GROUP BY event_type ORDER BY cnt DESC"
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            for r in rows { summary.push(r?); }
        }
        Ok(summary)
    }

    /// Delete old progress/hook events older than cutoff timestamp
    pub fn cleanup_old_events(&self, cutoff: &str) -> SqliteResult<usize> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM conversation_events
             WHERE event_type IN ('progress:bash_progress', 'progress:mcp_progress', 'hook_progress', 'progress:waiting_for_task')
             AND timestamp < ?1",
            params![cutoff],
        )?;
        Ok(deleted)
    }

    /// Get all session IDs that have at least one event in conversation_events
    pub fn get_sessions_with_events(&self) -> SqliteResult<std::collections::HashSet<String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT session_id FROM conversation_events"
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut set = std::collections::HashSet::new();
        for r in rows { set.insert(r?); }
        Ok(set)
    }

    /// Count tool calls still in 'pending' status (missing output)
    pub fn count_pending_tool_calls(&self) -> SqliteResult<i64> {
        let conn = self.read_conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM conversation_tool_calls WHERE status = 'pending'",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Get session IDs that have pending tool calls (for output patching)
    pub fn get_sessions_with_pending_tool_calls(&self) -> SqliteResult<Vec<String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT session_id FROM conversation_tool_calls WHERE status = 'pending'"
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Get all session IDs that have at least one tool call in conversation_tool_calls
    pub fn get_sessions_with_tool_calls(&self) -> SqliteResult<std::collections::HashSet<String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT session_id FROM conversation_tool_calls"
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut set = std::collections::HashSet::new();
        for r in rows { set.insert(r?); }
        Ok(set)
    }

    /// Get raw messages for tool call backfill (assistant+user roles with raw_content)
    pub fn get_messages_for_tool_call_backfill(&self, session_id: &str) -> SqliteResult<Vec<(String, String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT role, raw_content, timestamp FROM conversation_messages
             WHERE session_id = ?1 AND raw_content IS NOT NULL AND raw_content != ''
             AND role IN ('assistant', 'user', 'thinking', 'system', 'tool_result')
             ORDER BY id ASC"
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Get all conversations with their JSONL paths (for backfill)
    pub fn get_conversations_with_jsonl(&self) -> SqliteResult<Vec<(String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, jsonl_path FROM conversations WHERE jsonl_path IS NOT NULL AND jsonl_path != ''"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Mark a conversation as analyzed
    pub fn mark_conversation_analyzed(&self, id: &str) -> SqliteResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET analyzed_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// Get conversations that are completed but not yet analyzed
    pub fn get_unanalyzed_conversations(&self) -> SqliteResult<Vec<Conversation>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM conversations WHERE status = 'completed' AND analyzed_at IS NULL ORDER BY started_at DESC"
        )?;
        let rows = stmt.query_map([], |row| Self::row_to_conversation(row))?;
        let mut convs = Vec::new();
        for c in rows { convs.push(c?); }
        Ok(convs)
    }

    /// Mark a conversation as completed
    pub fn complete_conversation(&self, id: &str) -> SqliteResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET status = 'completed', ended_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// Complete stale active conversations whose last message is older than the given cutoff.
    /// Returns the number of conversations marked completed.
    pub fn complete_stale_conversations(&self, cutoff: &str) -> SqliteResult<usize> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT c.id FROM conversations c
             WHERE c.status = 'active'
               AND (SELECT MAX(m.timestamp) FROM conversation_messages m WHERE m.session_id = c.id) < ?1"
        )?;
        let ids: Vec<String> = stmt.query_map(params![cutoff], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        let now = chrono::Utc::now().to_rfc3339();
        for id in &ids {
            conn.execute(
                "UPDATE conversations SET status = 'completed', ended_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
        }
        Ok(ids.len())
    }

    /// Mark a conversation as compacted (replaced by context compaction).
    pub fn mark_conversation_compacted(&self, id: &str) -> SqliteResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET status = 'compacted', ended_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// Set the task_id on a conversation.
    pub fn set_conversation_task_id(&self, id: &str, task_id: &str) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET task_id = ?1 WHERE id = ?2",
            params![task_id, id],
        )?;
        Ok(())
    }

    /// Get all conversations sharing the same task_id.
    pub fn get_conversations_by_task_id(&self, task_id: &str) -> SqliteResult<Vec<Conversation>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM conversations WHERE task_id = ?1 ORDER BY started_at ASC"
        )?;
        let rows = stmt.query_map(params![task_id], |row| Self::row_to_conversation(row))?;
        let mut convs = Vec::new();
        for c in rows { convs.push(c?); }
        Ok(convs)
    }

    /// Re-activate a completed conversation when new messages arrive.
    pub fn reactivate_conversation(&self, id: &str) -> SqliteResult<usize> {
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET status = 'active', ended_at = NULL WHERE id = ?1 AND status = 'completed'",
            params![id],
        )
    }

    /// Get conversation messages not yet forwarded to memory analysis.
    /// Returns messages from today (UTC) for user CLI sessions only (no PTY, no subagents).
    pub fn get_pending_memory_messages(&self, today: &str) -> SqliteResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        // Single JOIN query: get all pending messages at once
        // Excludes: PTY sessions (slot_id IS NOT NULL), subagent sessions (id LIKE 'agent-%')
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT m.*, COALESCE(c.project, '') as c_project, c.memory_forwarded_at
             FROM conversation_messages m
             JOIN conversations c ON c.id = m.session_id
             WHERE c.conversation_type = 'user'
               AND m.timestamp >= ?1
               AND m.timestamp > COALESCE(c.memory_forwarded_at, ?1)
               AND m.role IN ('user', 'assistant')
             ORDER BY c.started_at DESC, m.id ASC"
        )?;

        let mut results: Vec<(String, String, Vec<ConversationMessage>)> = Vec::new();

        let rows = stmt.query_map(params![today], |row| {
            let msg = Self::row_to_conversation_message(row)?;
            let project: String = row.get("c_project")?;
            Ok((msg, project))
        })?;

        for row in rows {
            let (msg, project) = row?;
            let session_id = msg.session_id.clone();
            if let Some(entry) = results.iter_mut().find(|(id, _, _)| id == &session_id) {
                entry.2.push(msg);
            } else {
                results.push((session_id, project, vec![msg]));
            }
        }

        Ok(results)
    }

    /// Update memory_forwarded_at for a conversation
    pub fn update_memory_forwarded_at(&self, session_id: &str, timestamp: &str) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET memory_forwarded_at = ?1 WHERE id = ?2",
            params![timestamp, session_id],
        )?;
        Ok(())
    }

    /// Get pending USER-ONLY messages for user-voice extraction.
    /// Same logic as get_pending_memory_messages but only returns role='user'.
    pub fn get_pending_user_voice_messages(&self) -> SqliteResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT m.*, COALESCE(c.project, '') as c_project, c.user_voice_forwarded_at
             FROM conversation_messages m
             JOIN conversations c ON c.id = m.session_id
             WHERE c.conversation_type = 'user'
               AND m.timestamp > COALESCE(c.user_voice_forwarded_at, c.started_at)
               AND m.role = 'user'
             ORDER BY m.timestamp ASC"
        )?;

        let mut results: Vec<(String, String, Vec<ConversationMessage>)> = Vec::new();

        let rows = stmt.query_map([], |row| {
            let msg = Self::row_to_conversation_message(row)?;
            let project: String = row.get("c_project")?;
            Ok((msg, project))
        })?;

        for row in rows {
            let (msg, project) = row?;
            let session_id = msg.session_id.clone();
            if let Some(entry) = results.iter_mut().find(|(id, _, _)| id == &session_id) {
                entry.2.push(msg);
            } else {
                results.push((session_id, project, vec![msg]));
            }
        }

        Ok(results)
    }

    /// Update user_voice_forwarded_at for a conversation
    pub fn update_user_voice_forwarded_at(&self, session_id: &str, timestamp: &str) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET user_voice_forwarded_at = ?1 WHERE id = ?2",
            params![timestamp, session_id],
        )?;
        Ok(())
    }

    /// Get pending messages for unified realtime extraction (replaces separate user_voice + memory).
    /// Returns all user+assistant messages since realtime_forwarded_at watermark.
    /// Uses fair-queuing: per-session cap (10 msgs) + oldest-first ordering to prevent starvation.
    pub fn get_pending_realtime_messages(&self) -> SqliteResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        self.get_pending_realtime_messages_with_limit(50)
    }

    /// Get pending realtime messages with a configurable limit.
    /// Fair-queuing: each session gets at most 10 messages per batch, ordered by oldest first.
    pub fn get_pending_realtime_messages_with_limit(&self, limit: usize) -> SqliteResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "WITH ranked AS (
                SELECT m.*, COALESCE(c.project, '') as c_project,
                    ROW_NUMBER() OVER(PARTITION BY m.session_id ORDER BY m.timestamp ASC) as rn
                FROM conversation_messages m
                JOIN conversations c ON c.id = m.session_id
                WHERE c.conversation_type = 'user'
                  AND m.timestamp > COALESCE(c.realtime_forwarded_at, c.started_at)
                  AND m.role IN ('user', 'assistant')
            )
            SELECT * FROM ranked
            WHERE rn <= 10
            ORDER BY timestamp ASC
            LIMIT ?1"
        )?;

        let mut results: Vec<(String, String, Vec<ConversationMessage>)> = Vec::new();

        let rows = stmt.query_map(params![limit as i64], |row| {
            let msg = Self::row_to_conversation_message(row)?;
            let project: String = row.get("c_project")?;
            Ok((msg, project))
        })?;

        for row in rows {
            let (msg, project) = row?;
            let session_id = msg.session_id.clone();
            if let Some(entry) = results.iter_mut().find(|(id, _, _)| id == &session_id) {
                entry.2.push(msg);
            } else {
                results.push((session_id, project, vec![msg]));
            }
        }

        Ok(results)
    }

    /// Update realtime_forwarded_at watermark for a conversation.
    pub fn update_realtime_forwarded_at(&self, session_id: &str, timestamp: &str) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET realtime_forwarded_at = ?1 WHERE id = ?2",
            params![timestamp, session_id],
        )?;
        Ok(())
    }

    fn row_to_conversation(row: &rusqlite::Row) -> SqliteResult<Conversation> {
        Ok(Conversation {
            id: row.get("id")?,
            project: row.get("project")?,
            slot_id: row.get("slot_id")?,
            source: row.get("source")?,
            model: row.get("model")?,
            git_branch: row.get("git_branch")?,
            jsonl_path: row.get("jsonl_path")?,
            parent_session_id: row.get("parent_session_id").unwrap_or(None),
            task_id: row.get("task_id").unwrap_or(None),
            message_count: row.get("message_count")?,
            started_at: row.get("started_at")?,
            ended_at: row.get("ended_at")?,
            status: row.get("status")?,
            analyzed_at: row.get("analyzed_at")?,
            analysis_version: row.get("analysis_version").unwrap_or(0),
            analysis_retries: row.get("analysis_retries").unwrap_or(0),
            deep_analyzed_message_id: row.get("deep_analyzed_message_id").unwrap_or(0),
            chat_type: row.get("chat_type").unwrap_or(None),
            conversation_type: row.get("conversation_type").unwrap_or_else(|_| "user".to_string()),
        })
    }

    fn row_to_conversation_message(row: &rusqlite::Row) -> SqliteResult<ConversationMessage> {
        Ok(ConversationMessage {
            id: row.get("id")?,
            session_id: row.get("session_id")?,
            role: row.get("role")?,
            content: row.get("content")?,
            raw_content: row.get("raw_content")?,
            message_uuid: row.get("message_uuid")?,
            parent_uuid: row.get("parent_uuid")?,
            model: row.get("model")?,
            timestamp: row.get("timestamp")?,
            metadata: row.get("metadata")?,
        })
    }

    // ============ Router Chat Sessions ============

    /// Find or create a router_chat conversation for a given task_id
    pub fn router_chat_get_or_create(&self, task_id: &str, model: &str) -> SqliteResult<String> {
        let conn = self.conn();
        // Find existing active router_chat conversation for this task
        let existing: Option<String> = conn.query_row(
            "SELECT id FROM conversations WHERE task_id = ?1 AND chat_type = 'router_chat' AND status = 'active' LIMIT 1",
            params![task_id],
            |row| row.get(0),
        ).optional()?;

        if let Some(id) = existing {
            return Ok(id);
        }

        // Create new
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO conversations (id, source, model, task_id, chat_type, message_count, started_at, status)
             VALUES (?1, 'router_chat', ?2, ?3, 'router_chat', 0, ?4, 'active')",
            params![id, model, task_id, now],
        )?;
        Ok(id)
    }

    /// Load router_chat message history as OpenAI-format messages array
    pub fn router_chat_load_history(&self, conv_id: &str) -> SqliteResult<Vec<serde_json::Value>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT role, content FROM conversation_messages WHERE session_id = ?1 ORDER BY id ASC"
        )?;
        let msgs: Vec<serde_json::Value> = stmt.query_map(params![conv_id], |row| {
            let role: String = row.get(0)?;
            let content: String = row.get(1)?;
            Ok(serde_json::json!({"role": role, "content": content}))
        })?.filter_map(|r| r.ok()).collect();
        Ok(msgs)
    }

    /// Append messages to a router_chat conversation
    pub fn router_chat_append_messages(
        &self,
        conv_id: &str,
        messages: &[(String, String)], // (role, content) pairs
    ) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        for (role, content) in messages {
            conn.execute(
                "INSERT INTO conversation_messages (session_id, role, content, timestamp)
                 VALUES (?1, ?2, ?3, ?4)",
                params![conv_id, role, content, now],
            )?;
        }
        // Update message count
        conn.execute(
            "UPDATE conversations SET message_count = (SELECT COUNT(*) FROM conversation_messages WHERE session_id = ?1) WHERE id = ?1",
            params![conv_id],
        )?;
        Ok(())
    }

    // ============ Slot Task History ============

    /// Insert a new slot task record (status=pending)
    pub fn insert_slot_task(&self, task: &SlotTask) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO slot_tasks (id, slot_id, task_type, status, prompt_summary, source_sessions, output_count, created_at, started_at, completed_at, duration_ms, error, conversation_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                task.id, task.slot_id, task.task_type, task.status,
                task.prompt_summary, task.source_sessions, task.output_count,
                task.created_at, task.started_at, task.completed_at,
                task.duration_ms, task.error, task.conversation_id,
            ],
        )?;
        Ok(())
    }

    /// Update slot task status to running
    pub fn slot_task_set_running(&self, id: &str) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE slot_tasks SET status = 'running', started_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// Mark slot task as completed
    pub fn slot_task_set_completed(&self, id: &str, output_count: i64) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE slot_tasks SET status = 'completed', completed_at = ?1, output_count = ?2,
             duration_ms = CAST((julianday(?1) - julianday(COALESCE(started_at, created_at))) * 86400000 AS INTEGER)
             WHERE id = ?3",
            params![now, output_count, id],
        )?;
        Ok(())
    }

    /// Mark slot task as failed
    pub fn slot_task_set_failed(&self, id: &str, error: &str) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE slot_tasks SET status = 'failed', completed_at = ?1, error = ?2,
             duration_ms = CAST((julianday(?1) - julianday(COALESCE(started_at, created_at))) * 86400000 AS INTEGER)
             WHERE id = ?3",
            params![now, error, id],
        )?;
        Ok(())
    }

    /// List slot tasks with optional filters
    pub fn list_slot_tasks(
        &self,
        slot_id: Option<&str>,
        task_type: Option<&str>,
        status: Option<&str>,
        limit: i64,
    ) -> SqliteResult<Vec<SlotTask>> {
        let conn = self.read_conn();
        let mut sql = String::from(
            "SELECT id, slot_id, task_type, status, prompt_summary, source_sessions,
                    output_count, created_at, started_at, completed_at, duration_ms, error, conversation_id
             FROM slot_tasks WHERE 1=1"
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(s) = slot_id {
            param_values.push(Box::new(s.to_string()));
            sql.push_str(&format!(" AND slot_id = ?{}", param_values.len()));
        }
        if let Some(t) = task_type {
            param_values.push(Box::new(t.to_string()));
            sql.push_str(&format!(" AND task_type = ?{}", param_values.len()));
        }
        if let Some(st) = status {
            param_values.push(Box::new(st.to_string()));
            sql.push_str(&format!(" AND status = ?{}", param_values.len()));
        }
        param_values.push(Box::new(limit));
        sql.push_str(&format!(" ORDER BY created_at DESC LIMIT ?{}", param_values.len()));

        let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_ref.as_slice(), Self::row_to_slot_task)?;
        rows.collect()
    }

    /// Get slot task stats summary
    pub fn slot_task_stats(&self, slot_id: Option<&str>) -> SqliteResult<serde_json::Value> {
        let conn = self.read_conn();
        let (where_clause, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(s) = slot_id {
            ("WHERE slot_id = ?1", vec![Box::new(s.to_string())])
        } else {
            ("", vec![])
        };

        let sql = format!(
            "SELECT
                COUNT(*) as total,
                SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed,
                SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) as failed,
                SUM(CASE WHEN status = 'running' THEN 1 ELSE 0 END) as running,
                SUM(output_count) as total_output,
                AVG(CASE WHEN status = 'completed' THEN duration_ms END) as avg_duration_ms,
                task_type,
                COUNT(*) as type_count
             FROM slot_tasks {where_clause}
             GROUP BY task_type"
        );

        let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let mut type_stats = Vec::new();
        let mut total = 0i64;
        let mut completed = 0i64;
        let mut failed = 0i64;
        let mut running = 0i64;
        let mut total_output = 0i64;

        let rows = stmt.query_map(params_ref.as_slice(), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4).unwrap_or(0),
                row.get::<_, Option<f64>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;

        for row in rows {
            let (t, c, f, r, o, avg, task_type, count) = row?;
            total += t;
            completed += c;
            failed += f;
            running += r;
            total_output += o;
            type_stats.push(serde_json::json!({
                "taskType": task_type,
                "count": count,
                "avgDurationMs": avg,
            }));
        }

        Ok(serde_json::json!({
            "total": total,
            "completed": completed,
            "failed": failed,
            "running": running,
            "totalOutput": total_output,
            "byType": type_stats,
        }))
    }

    /// Reaper: force-fail stale pending/running tasks older than threshold.
    /// Returns the number of tasks reaped.
    pub fn reap_stale_slot_tasks(&self, max_age_secs: i64) -> SqliteResult<usize> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE slot_tasks SET status = 'failed', error = 'reaper: stale task', completed_at = ?1
             WHERE status IN ('pending', 'running')
               AND julianday(?1) - julianday(created_at) > ?2 / 86400.0",
            params![now, max_age_secs as f64],
        )
    }

    /// Startup cleanup: force-fail all pending/running tasks (leftover from previous daemon).
    pub fn cleanup_orphan_slot_tasks(&self) -> SqliteResult<usize> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE slot_tasks SET status = 'failed', error = 'daemon restart: orphan cleanup', completed_at = ?1
             WHERE status IN ('pending', 'running')",
            params![now],
        )
    }

    /// Get the ID of the currently running slot task (if any)
    pub fn get_running_slot_task(&self, slot_id: &str) -> SqliteResult<Option<String>> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT id FROM slot_tasks WHERE slot_id = ?1 AND status = 'running' ORDER BY created_at DESC LIMIT 1",
            params![slot_id],
            |row| row.get(0),
        ).optional()
    }

    fn row_to_slot_task(row: &rusqlite::Row) -> SqliteResult<SlotTask> {
        Ok(SlotTask {
            id: row.get("id")?,
            slot_id: row.get("slot_id")?,
            task_type: row.get("task_type")?,
            status: row.get("status")?,
            prompt_summary: row.get("prompt_summary")?,
            source_sessions: row.get("source_sessions")?,
            output_count: row.get("output_count").unwrap_or(0),
            created_at: row.get("created_at")?,
            started_at: row.get("started_at")?,
            completed_at: row.get("completed_at")?,
            duration_ms: row.get("duration_ms")?,
            error: row.get("error")?,
            conversation_id: row.get("conversation_id")?,
        })
    }

    // ── Token Usage Ledger ──────────────────────────────────────────

    /// Insert a token usage record into the ledger (append-only).
    pub fn insert_token_usage(
        &self,
        conversation_id: &str,
        slot_id: Option<&str>,
        slot_task_id: Option<&str>,
        model: Option<&str>,
        input_tokens: i64,
        cache_creation_tokens: i64,
        cache_read_tokens: i64,
        output_tokens: i64,
    ) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO token_usage_ledger
                (conversation_id, slot_id, slot_task_id, model,
                 input_tokens, cache_creation_tokens, cache_read_tokens, output_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                conversation_id,
                slot_id,
                slot_task_id,
                model,
                input_tokens,
                cache_creation_tokens,
                cache_read_tokens,
                output_tokens,
            ],
        )?;
        Ok(())
    }

    /// Query aggregated token stats from the ledger.
    /// Supports filtering by conversation_id, slot_id, and time range.
    /// group_by: "session" | "slot" | "model" | "day" | None (total).
    pub fn token_stats(
        &self,
        conversation_id: Option<&str>,
        slot_id: Option<&str>,
        since: Option<&str>,
        group_by: Option<&str>,
    ) -> SqliteResult<Vec<std::collections::HashMap<String, serde_json::Value>>> {
        let conn = self.read_conn();

        let group_col = match group_by {
            Some("session") => Some("conversation_id"),
            Some("slot") => Some("slot_id"),
            Some("model") => Some("model"),
            Some("day") => Some("date(created_at)"),
            _ => None,
        };

        let mut sql = String::from("SELECT ");
        if let Some(col) = group_col {
            sql.push_str(&format!("{col} AS group_key, "));
        }
        sql.push_str(
            "SUM(input_tokens) AS total_input,
             SUM(cache_creation_tokens) AS total_cache_creation,
             SUM(cache_read_tokens) AS total_cache_read,
             SUM(output_tokens) AS total_output,
             COUNT(*) AS record_count
             FROM token_usage_ledger WHERE 1=1",
        );

        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1;

        if let Some(cid) = conversation_id {
            sql.push_str(&format!(" AND conversation_id = ?{idx}"));
            param_values.push(Box::new(cid.to_string()));
            idx += 1;
        }
        if let Some(sid) = slot_id {
            sql.push_str(&format!(" AND slot_id = ?{idx}"));
            param_values.push(Box::new(sid.to_string()));
            idx += 1;
        }
        if let Some(s) = since {
            sql.push_str(&format!(" AND created_at >= ?{idx}"));
            param_values.push(Box::new(s.to_string()));
            let _ = idx; // suppress unused warning
        }

        if let Some(col) = group_col {
            sql.push_str(&format!(" GROUP BY {col} ORDER BY total_output DESC"));
        }

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let col_count = stmt.column_count();
        let col_names: Vec<String> = (0..col_count)
            .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
            .collect();

        let rows = stmt.query_map(params_ref.as_slice(), |row| {
            let mut map = std::collections::HashMap::new();
            for (i, name) in col_names.iter().enumerate() {
                let val: rusqlite::types::Value = row.get(i)?;
                let json_val = match val {
                    rusqlite::types::Value::Null => serde_json::Value::Null,
                    rusqlite::types::Value::Integer(n) => serde_json::json!(n),
                    rusqlite::types::Value::Real(f) => serde_json::json!(f),
                    rusqlite::types::Value::Text(s) => serde_json::json!(s),
                    rusqlite::types::Value::Blob(_) => serde_json::Value::Null,
                };
                map.insert(name.clone(), json_val);
            }
            Ok(map)
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    fn row_to_knowledge_entry(row: &rusqlite::Row) -> SqliteResult<KnowledgeEntry> {
        let detail_str: Option<String> = row.get("detail")?;
        let detail = detail_str.and_then(|s| serde_json::from_str(&s).ok());

        Ok(KnowledgeEntry {
            id: row.get("id")?,
            category: row.get("category")?,
            key: row.get("key")?,
            summary: row.get("summary")?,
            detail,
            source: row.get("source")?,
            confidence: row.get("confidence")?,
            access_count: row.get("access_count")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
            last_accessed_at: row.get("last_accessed_at")?,
            linked_task_id: row.get("linked_task_id").unwrap_or(None),
        })
    }
}

/// Tokenize text for similarity comparison.
/// Chinese: character-level unigrams. English: lowercase words (len >= 2).
fn tokenize_for_similarity(text: &str) -> HashSet<String> {
    let mut tokens = HashSet::new();
    let mut ascii_word = String::new();

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            ascii_word.push(ch.to_ascii_lowercase());
        } else {
            if ascii_word.len() >= 2 {
                tokens.insert(ascii_word.clone());
            }
            ascii_word.clear();
            // CJK character → insert as unigram
            if ch as u32 > 0x2E80 {
                tokens.insert(ch.to_string());
            }
        }
    }
    if ascii_word.len() >= 2 {
        tokens.insert(ascii_word);
    }
    tokens
}

/// Token-level Jaccard similarity (supports Chinese + English mixed text)
fn token_jaccard_similarity(a: &str, b: &str) -> f64 {
    let ta = tokenize_for_similarity(a);
    let tb = tokenize_for_similarity(b);
    if ta.is_empty() && tb.is_empty() {
        return 0.0;
    }
    let intersection = ta.intersection(&tb).count();
    let union = ta.union(&tb).count();
    if union == 0 { 0.0 } else { intersection as f64 / union as f64 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TaskStatus;

    fn create_test_task(id: &str) -> Task {
        Task {
            id: id.to_string(),
            role: "worker".to_string(),
            prompt: "Test prompt".to_string(),
            status: TaskStatus::Queued,
            slot_id: None,
            session_id: None,
            result: None,
            error: None,
            created_at: 1234567890,
            started_at: None,
            finished_at: None,
        }
    }

    #[test]
    fn test_insert_and_get_task() {
        let db = MissionDB::in_memory().unwrap();
        let task = create_test_task("task-1");

        db.insert_task(&task).unwrap();
        let retrieved = db.get_task("task-1").unwrap().unwrap();

        assert_eq!(retrieved.id, "task-1");
        assert_eq!(retrieved.role, "worker");
        assert_eq!(retrieved.status, TaskStatus::Queued);
    }

    #[test]
    fn test_update_task() {
        let db = MissionDB::in_memory().unwrap();
        let task = create_test_task("task-2");
        db.insert_task(&task).unwrap();

        let update = TaskUpdate {
            status: Some(TaskStatus::Running),
            slot_id: Some("slot-1".to_string()),
            started_at: Some(1234567891),
            ..Default::default()
        };

        db.update_task("task-2", &update).unwrap();
        let retrieved = db.get_task("task-2").unwrap().unwrap();

        assert_eq!(retrieved.status, TaskStatus::Running);
        assert_eq!(retrieved.slot_id, Some("slot-1".to_string()));
        assert_eq!(retrieved.started_at, Some(1234567891));
    }

    #[test]
    fn test_get_tasks_by_status() {
        let db = MissionDB::in_memory().unwrap();

        let task1 = create_test_task("task-1");
        let mut task2 = create_test_task("task-2");
        task2.status = TaskStatus::Running;
        let task3 = create_test_task("task-3");

        db.insert_task(&task1).unwrap();
        db.insert_task(&task2).unwrap();
        db.insert_task(&task3).unwrap();

        let queued = db.get_tasks_by_status(TaskStatus::Queued).unwrap();
        assert_eq!(queued.len(), 2);

        let running = db.get_tasks_by_status(TaskStatus::Running).unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].id, "task-2");
    }

    #[test]
    fn test_get_queued_tasks_by_role() {
        let db = MissionDB::in_memory().unwrap();

        let task1 = create_test_task("task-1");
        let mut task2 = create_test_task("task-2");
        task2.role = "other".to_string();
        let task3 = create_test_task("task-3");

        db.insert_task(&task1).unwrap();
        db.insert_task(&task2).unwrap();
        db.insert_task(&task3).unwrap();

        let tasks = db.get_queued_tasks_by_role("worker").unwrap();
        assert_eq!(tasks.len(), 2);

        let tasks = db.get_queued_tasks_by_role("other").unwrap();
        assert_eq!(tasks.len(), 1);
    }

    #[test]
    fn test_inbox_messages() {
        let db = MissionDB::in_memory().unwrap();

        let msg1 = InboxMessage {
            id: "msg-1".to_string(),
            task_id: "task-1".to_string(),
            from_role: "worker".to_string(),
            content: "Hello".to_string(),
            read: false,
            created_at: 1234567890,
        };

        let msg2 = InboxMessage {
            id: "msg-2".to_string(),
            task_id: "task-1".to_string(),
            from_role: "worker".to_string(),
            content: "World".to_string(),
            read: true,
            created_at: 1234567891,
        };

        db.insert_inbox_message(&msg1).unwrap();
        db.insert_inbox_message(&msg2).unwrap();

        let all = db.get_inbox_messages(false, 10).unwrap();
        assert_eq!(all.len(), 2);

        let unread = db.get_inbox_messages(true, 10).unwrap();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].id, "msg-1");

        db.mark_inbox_read("msg-1").unwrap();
        let unread = db.get_inbox_messages(true, 10).unwrap();
        assert_eq!(unread.len(), 0);
    }

    #[test]
    fn test_events() {
        let db = MissionDB::in_memory().unwrap();

        let id1 = db
            .insert_event("task-1", EventType::TaskCreated, None, 1234567890)
            .unwrap();
        assert!(id1 > 0);

        let data = serde_json::json!({"progress": 50});
        let id2 = db
            .insert_event("task-1", EventType::TaskProgress, Some(&data), 1234567891)
            .unwrap();
        assert!(id2 > id1);

        let events = db.get_events_by_task("task-1").unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, EventType::TaskCreated);
        assert_eq!(events[1].event_type, EventType::TaskProgress);
        assert!(events[1].data.is_some());
    }

    #[test]
    fn test_slot_sessions() {
        let db = MissionDB::in_memory().unwrap();

        assert!(db.get_slot_session("slot-1").unwrap().is_none());

        db.set_slot_session("slot-1", "session-abc").unwrap();
        assert_eq!(
            db.get_slot_session("slot-1").unwrap(),
            Some("session-abc".to_string())
        );

        db.set_slot_session("slot-1", "session-xyz").unwrap();
        assert_eq!(
            db.get_slot_session("slot-1").unwrap(),
            Some("session-xyz".to_string())
        );

        let all = db.get_all_slot_sessions().unwrap();
        assert_eq!(all.len(), 1);

        db.delete_slot_session("slot-1").unwrap();
        assert!(db.get_slot_session("slot-1").unwrap().is_none());
    }

    #[test]
    fn test_get_all_tasks() {
        let db = MissionDB::in_memory().unwrap();

        for i in 0..5 {
            let mut task = create_test_task(&format!("task-{}", i));
            task.created_at = 1234567890 + i;
            db.insert_task(&task).unwrap();
        }

        let tasks = db.get_all_tasks(3).unwrap();
        assert_eq!(tasks.len(), 3);
        // Should be ordered by created_at DESC
        assert_eq!(tasks[0].id, "task-4");
        assert_eq!(tasks[1].id, "task-3");
        assert_eq!(tasks[2].id, "task-2");
    }
}
