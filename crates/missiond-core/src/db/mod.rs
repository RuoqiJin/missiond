//! Database operations for missiond.

pub mod error;
pub mod traits;
pub mod shared;
pub(crate) mod directive;

#[cfg(feature = "postgres")]
pub mod pg;

// SQLite backend — feature-gated, opt-in for migration tool only.
#[cfg(feature = "sqlite")]
pub mod executor;
#[cfg(feature = "sqlite")]
pub mod sqlite;
#[cfg(feature = "sqlite")]
mod task;
#[cfg(feature = "sqlite")]
mod slot;
#[cfg(feature = "sqlite")]
mod board;
#[cfg(feature = "sqlite")]
mod question;
#[cfg(feature = "sqlite")]
pub(crate) mod knowledge;
#[cfg(feature = "sqlite")]
mod skill;
#[cfg(feature = "sqlite")]
mod conversation;
#[cfg(feature = "sqlite")]
mod audit;
#[cfg(feature = "sqlite")]
mod router_chat;
#[cfg(feature = "sqlite")]
mod incident;
#[cfg(feature = "sqlite")]
mod gemini_log;
#[cfg(feature = "sqlite")]
mod vision;
#[cfg(feature = "sqlite")]
pub mod ast;
#[cfg(feature = "sqlite")]
pub mod beacon;
#[cfg(feature = "sqlite")]
mod migration;
#[cfg(feature = "sqlite")]
mod dynamic_slot;
#[cfg(feature = "sqlite")]
mod narration;
#[cfg(feature = "sqlite")]
mod translation;
#[cfg(feature = "sqlite")]
pub(crate) mod backfill;
#[cfg(feature = "sqlite")]
mod watermark;
#[cfg(feature = "sqlite")]
pub mod message_feed;

// Re-exports from shared (always available)
pub use shared::{BackfillPhaseStatus, TimelineRow, TimelineStats, LatencyStats};

#[cfg(feature = "sqlite")]
use rusqlite::Connection;
#[cfg(feature = "sqlite")]
use error::DbResult;
#[cfg(feature = "sqlite")]
use std::path::Path;
#[cfg(feature = "sqlite")]
use std::sync::atomic::{AtomicBool, Ordering};

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
pub fn derive_conversation_type(slot_category: Option<&str>, slot_id: Option<&str>, session_id: &str, source: &str) -> String {
    // Top-level interception for all Gemini family conversations
    // Regardless of whether they have a slot or not, they belong in the Gemini tab.
    if source == "gemini_cli" || source == "router_chat" {
        return "gemini_chat".to_string();
    }

    // 1. If we have a declarative category from the slot config, use it directly (Dynamic Routing)
    if let Some(cat) = slot_category {
        return cat.to_string();
    }

    // 2. Fallback heuristics for unmanaged/orphan sessions
    match slot_id {
        Some(_) => "worker".to_string(), // Fallback if slot_id is present but config is missing
        None if session_id.contains("-acompact-") => "compaction".to_string(),
        None if session_id.starts_with("agent-") => "subagent".to_string(),
        None => "user".to_string(),
    }
}

#[cfg(feature = "sqlite")]
pub struct MissionDB {
    conn: std::sync::Mutex<Connection>,
    /// Read-only connection for queries — avoids blocking on write Mutex (WAL concurrent reads)
    read_conn: std::sync::Mutex<Connection>,
    /// Dirty flag: set after kb_forget, cleared after FTS rebuild
    fts_dirty: AtomicBool,
}

#[cfg(feature = "sqlite")]
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
        // Use a shared in-memory database so both connections see the same tables.
        // file::memory: with cache=shared creates a named in-memory DB shared across connections.
        use std::sync::atomic::AtomicU64;
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let uri = format!("file:testdb{}?mode=memory&cache=shared", id);
        let conn = Connection::open_with_flags(
            &uri,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
                | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
                | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let read_conn = Connection::open_with_flags(
            &uri,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
                | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )?;
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
        let conn = self.conn.lock().map_err(|e| error::DbError::Other(format!("mutex poisoned: {}", e)))?;
        f(&conn)
    }

    /// Parse time string for "since" context — delegates to `db::shared::parse_since`.
    pub fn parse_time_since(&self, s: &str) -> String { shared::parse_since(s) }

    /// Parse time string for "until" context — delegates to `db::shared::parse_until`.
    pub fn parse_time_until(&self, s: &str) -> String { shared::parse_until(s) }

    /// Execute a closure with the read connection locked.
    /// WARNING: The closure MUST NOT contain any `.await` calls (rusqlite is synchronous).
    #[allow(dead_code)]
    pub(crate) fn with_read<T>(&self, f: impl FnOnce(&Connection) -> DbResult<T>) -> DbResult<T> {
        let conn = self.read_conn.lock().map_err(|e| error::DbError::Other(format!("mutex poisoned: {}", e)))?;
        f(&conn)
    }
}

#[cfg(all(test, feature = "sqlite"))]
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
pub mod conversation_query;
