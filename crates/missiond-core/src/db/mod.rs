//! SQLite database operations for missiond
//!
//! Mirrors the TypeScript implementation in packages/missiond/src/db/index.ts

pub mod error;
pub mod executor;
mod task;
mod slot;
mod board;
mod question;
mod knowledge;
mod skill;
mod conversation;
mod audit;
mod router_chat;
mod incident;
mod gemini_log;
mod vision;
mod ast;

use rusqlite::Connection;
use error::{DbError, DbResult};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use std::collections::HashSet;


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
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_questions_status ON agent_questions(status);
-- Note: idx_agent_questions_target is created in migration block (target column added via ALTER TABLE)

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
    analyzed_at TEXT,
    updated_at TEXT
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
/// "meta" = memory slots, "compaction" = context compaction shards,
/// "subagent" = agent-* IDs, "worker" = other slots, "user" = direct CLI.
pub fn derive_conversation_type(slot_id: Option<&str>, session_id: &str) -> String {
    match slot_id {
        Some("slot-memory" | "slot-memory-slow") => "meta".to_string(),
        Some(_) => "worker".to_string(),
        None if session_id.contains("-acompact-") => "compaction".to_string(),
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
    pub fn new<P: AsRef<Path>>(db_path: P) -> DbResult<Self> {
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
    pub fn open<P: AsRef<Path>>(db_path: P) -> DbResult<Self> {
        Self::new(db_path)
    }

    /// Close the database connections
    pub fn close(self) {
        drop(self.read_conn);
        drop(self.conn);
    }

    /// Create an in-memory database (for testing)
    pub fn in_memory() -> DbResult<Self> {
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

    /// Execute a closure with the write connection locked.
    /// WARNING: The closure MUST NOT contain any `.await` calls (rusqlite is synchronous).
    #[allow(dead_code)]
    pub(crate) fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> DbResult<T>) -> DbResult<T> {
        let conn = self.conn.lock().map_err(|e| DbError::Other(format!("mutex poisoned: {}", e)))?;
        f(&conn)
    }

    /// Execute a closure with the read connection locked.
    /// WARNING: The closure MUST NOT contain any `.await` calls (rusqlite is synchronous).
    #[allow(dead_code)]
    pub(crate) fn with_read<T>(&self, f: impl FnOnce(&Connection) -> DbResult<T>) -> DbResult<T> {
        let conn = self.read_conn.lock().map_err(|e| DbError::Other(format!("mutex poisoned: {}", e)))?;
        f(&conn)
    }

    fn init(&self) -> DbResult<()> {
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

        Ok(())
    }

    // ── System Timeline (Phase 6) ──

    /// Batch-insert timeline entries in a single transaction.
    /// Returns the seq (AUTOINCREMENT rowid) for each entry.
    pub fn insert_timeline_batch(
        &self,
        entries: &[(Option<&str>, &str, Option<&str>, &str, Option<&str>, &str)],
    ) -> DbResult<Vec<i64>> {
        let conn = self.conn();
        conn.execute_batch("BEGIN")?;
        let mut seqs = Vec::with_capacity(entries.len());
        for (trace_id, span_id, parent_span_id, event_type, summary, payload) in entries {
            conn.execute(
                "INSERT INTO system_timeline (trace_id, span_id, parent_span_id, event_type, summary, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![trace_id, span_id, parent_span_id, event_type, summary, payload],
            )?;
            seqs.push(conn.last_insert_rowid());
        }
        conn.execute_batch("COMMIT")?;
        Ok(seqs)
    }

    /// Query timeline events after a given seq (for WS catch-up).
    pub fn query_timeline_since(&self, since_seq: i64, limit: usize) -> DbResult<Vec<TimelineRow>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT seq, trace_id, span_id, parent_span_id, event_type, summary, payload, created_at
             FROM system_timeline WHERE seq > ?1 ORDER BY seq ASC LIMIT ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![since_seq, limit as i64], |row| {
            Ok(TimelineRow {
                seq: row.get(0)?,
                trace_id: row.get(1)?,
                span_id: row.get(2)?,
                parent_span_id: row.get(3)?,
                event_type: row.get(4)?,
                summary: row.get(5)?,
                payload: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Get the latest seq in the timeline (for connected message).
    pub fn timeline_latest_seq(&self) -> DbResult<i64> {
        let conn = self.read_conn();
        let seq: i64 = conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM system_timeline",
            [],
            |row| row.get(0),
        )?;
        Ok(seq)
    }

    /// Delete timeline events older than N days. Returns count deleted.
    pub fn cleanup_timeline_ttl(&self, days: i64) -> DbResult<usize> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM system_timeline WHERE created_at < datetime('now', ?1)",
            rusqlite::params![format!("-{} days", days)],
        )?;
        Ok(deleted)
    }

    /// Find timeline entries that need semantic summary generation.
    /// Returns (seq, event_type, payload, summary) for entries where summary is
    /// the mechanical preview (from message_handler) and content is long enough
    /// to benefit from semantic summarization.
    pub fn find_timeline_needing_briefing(&self, min_content_chars: usize, limit: usize) -> DbResult<Vec<(i64, String, String, Option<String>)>> {
        let conn = self.read_conn();
        // Target: conversation messages (user/assistant/thinking) with long content
        // that still have the mechanical preview (or NULL summary).
        // Use payload.content_chars to filter long messages.
        let mut stmt = conn.prepare(
            "SELECT seq, event_type, payload, summary FROM system_timeline
             WHERE event_type IN ('user_message', 'assistant_message', 'thinking_message')
               AND json_extract(payload, '$.content_chars') > ?1
               AND (summary IS NULL OR summary = json_extract(payload, '$.preview'))
             ORDER BY seq DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![min_content_chars as i64, limit as i64], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Update the summary column for a timeline entry (used by briefing worker).
    pub fn update_timeline_summary(&self, seq: i64, summary: &str) -> DbResult<bool> {
        let conn = self.conn();
        let updated = conn.execute(
            "UPDATE system_timeline SET summary = ?1 WHERE seq = ?2",
            rusqlite::params![summary, seq],
        )?;
        Ok(updated > 0)
    }

    /// Insert a translation for a message.
    pub fn insert_translation(
        &self,
        message_id: i64,
        translation: &str,
        model: &str,
        duration_ms: u64,
    ) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR REPLACE INTO message_translations (message_id, translation, model, duration_ms)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![message_id, translation, model, duration_ms as i64],
        )?;
        Ok(())
    }

    /// Get translation for a message. Returns (translation, created_at).
    pub fn get_translation(&self, message_id: i64) -> DbResult<Option<(String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT translation, created_at FROM message_translations WHERE message_id = ?1"
        )?;
        let result = stmt.query_row(rusqlite::params![message_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        });
        match result {
            Ok(r) => Ok(Some(r)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Check if a translation exists for a message.
    pub fn has_translation(&self, message_id: i64) -> DbResult<bool> {
        let conn = self.read_conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM message_translations WHERE message_id = ?1",
            rusqlite::params![message_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    // ── Message Narrations (GPT-5.4 step explanations) ──

    /// Batch-insert narrations for a session's messages.
    pub fn insert_narrations(&self, narrations: &[(i64, &str, &str, &str, &str)]) -> DbResult<usize> {
        let conn = self.conn();
        let mut count = 0;
        for (message_id, session_id, step_title, step_intent, step_result) in narrations {
            conn.execute(
                "INSERT OR REPLACE INTO message_narrations (message_id, session_id, step_title, step_intent, step_result)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![message_id, session_id, step_title, step_intent, step_result],
            )?;
            count += 1;
        }
        Ok(count)
    }

    /// Get all narrations for a session.
    pub fn get_narrations_for_session(&self, session_id: &str) -> DbResult<Vec<(i64, String, String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT message_id, step_title, step_intent, step_result FROM message_narrations WHERE session_id = ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![session_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get sessions that need narration (have unnarrated messages).
    pub fn get_sessions_needing_narration(&self, min_unnarrated: i64) -> DbResult<Vec<(String, i64)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT cm.session_id, COUNT(*) as unnarrated
             FROM conversation_messages cm
             JOIN conversations c ON c.id = cm.session_id
             LEFT JOIN message_narrations mn ON mn.message_id = cm.id
             WHERE mn.message_id IS NULL
               AND c.conversation_type = 'user'
               AND cm.role IN ('user', 'assistant')
             GROUP BY cm.session_id
             HAVING unnarrated >= ?1
             ORDER BY MAX(cm.timestamp) DESC
             LIMIT 5"
        )?;
        let rows = stmt.query_map(rusqlite::params![min_unnarrated], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Fetch briefing summaries for a session's conversation messages.
    /// Returns a map of message_id → summary from system_timeline where the
    /// summary differs from the raw preview (i.e., a semantic summary exists).
    pub fn get_briefing_summaries_for_session(&self, session_id: &str) -> DbResult<std::collections::HashMap<i64, String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT json_extract(payload, '$.message_id'), summary
             FROM system_timeline
             WHERE event_type IN ('user_message', 'assistant_message', 'thinking_message')
               AND json_extract(payload, '$.session_id') = ?1
               AND summary IS NOT NULL
               AND summary != json_extract(payload, '$.preview')"
        )?;
        let mut map = std::collections::HashMap::new();
        let rows = stmt.query_map(rusqlite::params![session_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            if let Ok((msg_id, summary)) = row {
                map.insert(msg_id, summary);
            }
        }
        Ok(map)
    }

    // ── L3: Timeline Query Methods ──

    /// Filtered timeline query with optional event_type, trace_id, time range, and pagination.
    pub fn query_timeline_filtered(
        &self,
        event_type: Option<&str>,
        trace_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> DbResult<Vec<TimelineRow>> {
        let conn = self.read_conn();
        let mut conditions = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(et) = event_type {
            conditions.push(format!("event_type = ?{}", params.len() + 1));
            params.push(Box::new(et.to_string()));
        }
        if let Some(tid) = trace_id {
            conditions.push(format!("trace_id = ?{}", params.len() + 1));
            params.push(Box::new(tid.to_string()));
        }
        if let Some(s) = since {
            let ts = Self::parse_relative_time(s);
            conditions.push(format!("created_at >= ?{}", params.len() + 1));
            params.push(Box::new(ts));
        }
        if let Some(u) = until {
            let ts = Self::parse_relative_time(u);
            conditions.push(format!("created_at <= ?{}", params.len() + 1));
            params.push(Box::new(ts));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let sql = format!(
            "SELECT seq, trace_id, span_id, parent_span_id, event_type, summary, payload, created_at
             FROM system_timeline {} ORDER BY seq DESC LIMIT ?{} OFFSET ?{}",
            where_clause, params.len() + 1, params.len() + 2
        );
        params.push(Box::new(limit));
        params.push(Box::new(offset));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(TimelineRow {
                seq: row.get(0)?,
                trace_id: row.get(1)?,
                span_id: row.get(2)?,
                parent_span_id: row.get(3)?,
                event_type: row.get(4)?,
                summary: row.get(5)?,
                payload: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Stratified timeline query: returns up to `per_type_limit` events per event_type
    /// within the time range, using a single SQL query with window functions.
    /// Custom per-type limits can override the default (e.g., high-volume types get fewer).
    pub fn query_timeline_stratified(
        &self,
        since: &str,
        until: &str,
        per_type_limit: i64,
        type_limits: &std::collections::HashMap<String, i64>,
    ) -> DbResult<Vec<TimelineRow>> {
        let conn = self.read_conn();
        let ts_since = Self::parse_relative_time(since);
        let ts_until = Self::parse_relative_time(until);

        // Build per-type CASE expression for custom limits
        let limit_expr = if type_limits.is_empty() {
            format!("{}", per_type_limit)
        } else {
            let mut cases = String::from("CASE event_type ");
            for (et, lim) in type_limits {
                cases.push_str(&format!("WHEN '{}' THEN {} ", et.replace('\'', "''"), lim));
            }
            cases.push_str(&format!("ELSE {} END", per_type_limit));
            cases
        };

        let sql = format!(
            "SELECT seq, trace_id, span_id, parent_span_id, event_type, summary, payload, created_at
             FROM (
               SELECT *, ROW_NUMBER() OVER (PARTITION BY event_type ORDER BY seq DESC) as rn
               FROM system_timeline
               WHERE created_at >= ?1 AND created_at <= ?2
             )
             WHERE rn <= ({})
             ORDER BY seq DESC",
            limit_expr
        );

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params![ts_since, ts_until], |row| {
            Ok(TimelineRow {
                seq: row.get(0)?,
                trace_id: row.get(1)?,
                span_id: row.get(2)?,
                parent_span_id: row.get(3)?,
                event_type: row.get(4)?,
                summary: row.get(5)?,
                payload: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Query all events belonging to a single trace (causal chain).
    pub fn query_timeline_by_trace(&self, trace_id: &str) -> DbResult<Vec<TimelineRow>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT seq, trace_id, span_id, parent_span_id, event_type, summary, payload, created_at
             FROM system_timeline WHERE trace_id = ?1 ORDER BY seq ASC"
        )?;
        let rows = stmt.query_map(rusqlite::params![trace_id], |row| {
            Ok(TimelineRow {
                seq: row.get(0)?,
                trace_id: row.get(1)?,
                span_id: row.get(2)?,
                parent_span_id: row.get(3)?,
                event_type: row.get(4)?,
                summary: row.get(5)?,
                payload: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Timeline statistics: event count by type, trace stats, Gemini latency distribution.
    pub fn query_timeline_stats(&self, since: Option<&str>, until: Option<&str>) -> DbResult<TimelineStats> {
        let conn = self.read_conn();

        let mut conditions = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(s) = since {
            let ts = Self::parse_relative_time(s);
            conditions.push(format!("created_at >= ?{}", params.len() + 1));
            params.push(Box::new(ts));
        }
        if let Some(u) = until {
            let ts = Self::parse_relative_time(u);
            conditions.push(format!("created_at <= ?{}", params.len() + 1));
            params.push(Box::new(ts));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        // Total events
        let total_sql = format!("SELECT COUNT(*) FROM system_timeline {}", where_clause);
        let total_events: i64 = conn.query_row(&total_sql, param_refs.as_slice(), |r| r.get(0))?;

        // By type
        let by_type_sql = format!(
            "SELECT event_type, COUNT(*) FROM system_timeline {} GROUP BY event_type ORDER BY COUNT(*) DESC",
            where_clause
        );
        let mut stmt = conn.prepare(&by_type_sql)?;
        let by_type: Vec<(String, i64)> = stmt.query_map(param_refs.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?.collect::<Result<Vec<_>, _>>()?;

        // Traced events
        let traced_where = if where_clause.is_empty() {
            "WHERE trace_id IS NOT NULL AND trace_id != ''".to_string()
        } else {
            format!("{} AND trace_id IS NOT NULL AND trace_id != ''", where_clause)
        };
        let traced_sql = format!(
            "SELECT COUNT(*), COUNT(DISTINCT trace_id) FROM system_timeline {}",
            traced_where
        );
        let (traced_events, unique_traces): (i64, i64) = conn.query_row(
            &traced_sql, param_refs.as_slice(), |r| Ok((r.get(0)?, r.get(1)?))
        )?;

        // Gemini latency from payload JSON
        let gemini_sql = if where_clause.is_empty() {
            "SELECT json_extract(payload, '$.duration_ms') FROM system_timeline WHERE event_type = 'gemini_request_completed' ORDER BY seq DESC LIMIT 100".to_string()
        } else {
            format!(
                "SELECT json_extract(payload, '$.duration_ms') FROM system_timeline {} AND event_type = 'gemini_request_completed' ORDER BY seq DESC LIMIT 100",
                where_clause
            )
        };
        let mut stmt = conn.prepare(&gemini_sql)?;
        let durations: Vec<i64> = stmt.query_map(param_refs.as_slice(), |row| {
            row.get::<_, Option<i64>>(0)
        })?.filter_map(|r| r.ok().flatten()).collect();

        let gemini_latency = if durations.is_empty() {
            None
        } else {
            let mut sorted = durations.clone();
            sorted.sort();
            let len = sorted.len();
            let avg = sorted.iter().sum::<i64>() / len as i64;
            Some(LatencyStats {
                count: len as i64,
                avg_ms: avg,
                p50_ms: sorted[len / 2],
                p90_ms: sorted[(len as f64 * 0.9) as usize],
                p99_ms: sorted[(len as f64 * 0.99).min((len - 1) as f64) as usize],
            })
        };

        Ok(TimelineStats { total_events, by_type, traced_events, unique_traces, gemini_latency })
    }

    /// Search timeline by keyword in summary and payload.
    pub fn query_timeline_search(
        &self,
        keyword: &str,
        since: Option<&str>,
        until: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<TimelineRow>> {
        let conn = self.read_conn();
        let like_pattern = format!("%{}%", keyword);
        let mut conditions = vec![
            format!("(summary LIKE ?{} OR payload LIKE ?{})", 1, 2),
        ];
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
            Box::new(like_pattern.clone()),
            Box::new(like_pattern),
        ];

        if let Some(s) = since {
            let ts = Self::parse_relative_time(s);
            conditions.push(format!("created_at >= ?{}", params.len() + 1));
            params.push(Box::new(ts));
        }
        if let Some(u) = until {
            let ts = Self::parse_relative_time(u);
            conditions.push(format!("created_at <= ?{}", params.len() + 1));
            params.push(Box::new(ts));
        }

        let sql = format!(
            "SELECT seq, trace_id, span_id, parent_span_id, event_type, summary, payload, created_at
             FROM system_timeline WHERE {} ORDER BY seq DESC LIMIT ?{}",
            conditions.join(" AND "), params.len() + 1
        );
        params.push(Box::new(limit));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(TimelineRow {
                seq: row.get(0)?,
                trace_id: row.get(1)?,
                span_id: row.get(2)?,
                parent_span_id: row.get(3)?,
                event_type: row.get(4)?,
                summary: row.get(5)?,
                payload: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Parse relative time strings like "10min", "1h", "24h", "7d" into SQLite datetime expressions.
    fn parse_relative_time(s: &str) -> String {
        let s = s.trim();
        if let Some(mins) = s.strip_suffix("min").and_then(|v| v.parse::<i64>().ok()) {
            return format!("{}", (chrono::Utc::now() - chrono::Duration::minutes(mins)).format("%Y-%m-%d %H:%M:%S"));
        }
        if let Some(hours) = s.strip_suffix('h').and_then(|v| v.parse::<i64>().ok()) {
            return format!("{}", (chrono::Utc::now() - chrono::Duration::hours(hours)).format("%Y-%m-%d %H:%M:%S"));
        }
        if let Some(days) = s.strip_suffix('d').and_then(|v| v.parse::<i64>().ok()) {
            return format!("{}", (chrono::Utc::now() - chrono::Duration::days(days)).format("%Y-%m-%d %H:%M:%S"));
        }
        // Assume ISO datetime, pass through
        s.to_string()
    }
}

/// A row from the system_timeline table.
#[derive(Debug, Clone)]
pub struct TimelineRow {
    pub seq: i64,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub parent_span_id: Option<String>,
    pub event_type: String,
    pub summary: Option<String>,
    pub payload: String,
    pub created_at: String,
}

/// Timeline statistics returned by query_timeline_stats.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TimelineStats {
    pub total_events: i64,
    pub by_type: Vec<(String, i64)>,
    pub traced_events: i64,
    pub unique_traces: i64,
    pub gemini_latency: Option<LatencyStats>,
}

/// Latency percentile statistics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LatencyStats {
    pub count: i64,
    pub avg_ms: i64,
    pub p50_ms: i64,
    pub p90_ms: i64,
    pub p99_ms: i64,
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
    use crate::types::{Task, TaskStatus, TaskUpdate, InboxMessage, EventType};

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
