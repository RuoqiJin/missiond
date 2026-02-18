//! SQLite database operations for missiond
//!
//! Mirrors the TypeScript implementation in packages/missiond/src/db/index.ts

use rusqlite::{params, Connection, Result as SqliteResult};
use std::path::Path;

use crate::types::{
    AddBoardTaskNoteInput, BoardNoteType, BoardTask, BoardTaskNote, BoardTaskStatus,
    BoardTaskWithNotes, CreateBoardTaskInput, EventType, InboxMessage, Task, TaskEvent,
    TaskStatus, TaskUpdate, UpdateBoardTaskInput,
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
  order_idx INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_board_tasks_status ON board_tasks(status);
CREATE INDEX IF NOT EXISTS idx_board_tasks_parent ON board_tasks(parent_id);

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
"#;

/// SQLite database operations class
pub struct MissionDB {
    conn: Connection,
}

impl MissionDB {
    /// Create a new database connection
    pub fn new<P: AsRef<Path>>(db_path: P) -> SqliteResult<Self> {
        let conn = Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    /// Alias for new - opens a database file
    pub fn open<P: AsRef<Path>>(db_path: P) -> SqliteResult<Self> {
        Self::new(db_path)
    }

    /// Close the database connection
    pub fn close(self) {
        // Connection is automatically closed when dropped
        drop(self.conn);
    }

    /// Create an in-memory database (for testing)
    pub fn in_memory() -> SqliteResult<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> SqliteResult<()> {
        self.conn.execute_batch(SCHEMA)?;
        self.migrate()?;
        Ok(())
    }

    /// Run schema migrations for existing databases
    fn migrate(&self) -> SqliteResult<()> {
        // Phase D: Add autopilot columns to board_tasks
        let columns: Vec<String> = self.conn
            .prepare("PRAGMA table_info(board_tasks)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();

        if !columns.iter().any(|c| c == "assignee") {
            self.conn.execute_batch(
                "ALTER TABLE board_tasks ADD COLUMN assignee TEXT;
                 ALTER TABLE board_tasks ADD COLUMN auto_execute INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE board_tasks ADD COLUMN prompt_template TEXT;"
            )?;
        }
        Ok(())
    }

    // ============ Tasks ============

    /// Insert a new task
    pub fn insert_task(&self, task: &Task) -> SqliteResult<()> {
        self.conn.execute(
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
        self.conn.execute(&sql, params.as_slice())?;
        Ok(())
    }

    /// Get a task by ID
    pub fn get_task(&self, id: &str) -> SqliteResult<Option<Task>> {
        let mut stmt = self.conn.prepare("SELECT * FROM tasks WHERE id = ?")?;
        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_task(row)?))
        } else {
            Ok(None)
        }
    }

    /// Get all tasks by status
    pub fn get_tasks_by_status(&self, status: TaskStatus) -> SqliteResult<Vec<Task>> {
        let mut stmt = self
            .conn
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
        let mut stmt = self.conn.prepare(
            "SELECT * FROM tasks WHERE status = 'queued' AND role = ? ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![role], |row| Self::row_to_task(row))?;

        let mut tasks = Vec::new();
        for task in rows {
            tasks.push(task?);
        }
        Ok(tasks)
    }

    /// Get all tasks (for listing)
    pub fn get_all_tasks(&self, limit: i64) -> SqliteResult<Vec<Task>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM tasks ORDER BY created_at DESC LIMIT ?")?;
        let rows = stmt.query_map(params![limit], |row| Self::row_to_task(row))?;

        let mut tasks = Vec::new();
        for task in rows {
            tasks.push(task?);
        }
        Ok(tasks)
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
        self.conn.execute(
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

        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params![limit], |row| Self::row_to_inbox_message(row))?;

        let mut messages = Vec::new();
        for msg in rows {
            messages.push(msg?);
        }
        Ok(messages)
    }

    /// Mark an inbox message as read
    pub fn mark_inbox_read(&self, id: &str) -> SqliteResult<()> {
        self.conn
            .execute("UPDATE inbox SET read = 1 WHERE id = ?", params![id])?;
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

        self.conn.execute(
            "INSERT INTO events (task_id, type, data, timestamp)
             VALUES (?1, ?2, ?3, ?4)",
            params![task_id, event_type.as_str(), data_str, timestamp],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Get events by task ID
    pub fn get_events_by_task(&self, task_id: &str) -> SqliteResult<Vec<TaskEvent>> {
        let mut stmt = self
            .conn
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
        let mut stmt = self
            .conn
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
        self.conn.execute(
            "INSERT INTO slot_sessions (slot_id, session_id, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(slot_id) DO UPDATE SET session_id = ?2, updated_at = ?3",
            params![slot_id, session_id, now],
        )?;
        Ok(())
    }

    /// Delete a slot session
    pub fn delete_slot_session(&self, slot_id: &str) -> SqliteResult<()> {
        self.conn
            .execute("DELETE FROM slot_sessions WHERE slot_id = ?", params![slot_id])?;
        Ok(())
    }

    /// Alias for delete_slot_session
    pub fn clear_slot_session(&self, slot_id: &str) {
        let _ = self.delete_slot_session(slot_id);
    }

    /// Get all slot sessions
    pub fn get_all_slot_sessions(&self) -> SqliteResult<Vec<(String, String)>> {
        let mut stmt = self
            .conn
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

    // ============ Board Tasks ============

    /// Insert a new board task
    pub fn insert_board_task(&self, task: &BoardTask) -> SqliteResult<()> {
        self.conn.execute(
            "INSERT INTO board_tasks (id, title, description, status, priority, category, project, server, due_date, parent_id, assignee, auto_execute, prompt_template, order_idx, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
        let max_order: i64 = if let Some(ref pid) = input.parent_id {
            self.conn
                .query_row(
                    "SELECT COALESCE(MAX(order_idx), -1) FROM board_tasks WHERE parent_id = ?1",
                    params![pid],
                    |row| row.get(0),
                )
                .unwrap_or(-1)
        } else {
            self.conn
                .query_row(
                    "SELECT COALESCE(MAX(order_idx), -1) FROM board_tasks WHERE parent_id IS NULL",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(-1)
        };

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
            order_idx: max_order + 1,
            created_at: now.clone(),
            updated_at: now,
        };

        self.insert_board_task(&task)?;
        Ok(task)
    }

    /// Get a board task by ID
    pub fn get_board_task(&self, id: &str) -> SqliteResult<Option<BoardTask>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM board_tasks WHERE id = ?")?;
        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_board_task(row)?))
        } else {
            Ok(None)
        }
    }

    /// List all board tasks (optionally filtered by status)
    pub fn list_board_tasks(&self, status: Option<&str>) -> SqliteResult<Vec<BoardTask>> {
        let mut tasks = Vec::new();

        if let Some(s) = status {
            let mut stmt = self
                .conn
                .prepare("SELECT * FROM board_tasks WHERE status = ?1 ORDER BY order_idx ASC")?;
            let rows = stmt.query_map(params![s], |row| Self::row_to_board_task(row))?;
            for task in rows {
                tasks.push(task?);
            }
        } else {
            let mut stmt = self
                .conn
                .prepare("SELECT * FROM board_tasks ORDER BY order_idx ASC")?;
            let rows = stmt.query_map([], |row| Self::row_to_board_task(row))?;
            for task in rows {
                tasks.push(task?);
            }
        }

        Ok(tasks)
    }

    /// Update a board task
    pub fn update_board_task(&self, id: &str, update: &UpdateBoardTaskInput) -> SqliteResult<Option<BoardTask>> {
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

        if let Some(auto_exec) = update.auto_execute {
            fields.push("auto_execute = ?".to_string());
            values.push(Box::new(auto_exec as i32));
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
        self.conn.execute(&sql, params.as_slice())?;

        self.get_board_task(id)
    }

    /// Delete a board task and all descendants
    pub fn delete_board_task(&self, id: &str) -> SqliteResult<i64> {
        // Collect all descendant IDs recursively
        let mut to_delete = vec![id.to_string()];
        let mut i = 0;
        while i < to_delete.len() {
            let current = to_delete[i].clone();
            let mut stmt = self
                .conn
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
            self.conn
                .execute("DELETE FROM board_task_notes WHERE task_id = ?", params![tid])?;
            let r = self
                .conn
                .execute("DELETE FROM board_tasks WHERE id = ?", params![tid])?;
            deleted += r as i64;
        }
        Ok(deleted)
    }

    /// Toggle a board task status (open <-> done)
    pub fn toggle_board_task(&self, id: &str) -> SqliteResult<Option<BoardTask>> {
        if let Some(task) = self.get_board_task(id)? {
            let new_status = match task.status {
                BoardTaskStatus::Open => "done",
                BoardTaskStatus::Done => "open",
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

    /// List board tasks eligible for autopilot execution
    /// (auto_execute=true, status=open, due_date <= now, has assignee)
    pub fn list_autopilot_tasks(&self) -> SqliteResult<Vec<BoardTask>> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut stmt = self.conn.prepare(
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

    /// Clear all done board tasks
    pub fn clear_done_board_tasks(&self) -> SqliteResult<i64> {
        let result = self
            .conn
            .execute("DELETE FROM board_tasks WHERE status = 'done'", [])?;
        Ok(result as i64)
    }

    fn row_to_board_task(row: &rusqlite::Row) -> SqliteResult<BoardTask> {
        let status_str: String = row.get("status")?;
        let status = BoardTaskStatus::from_str(&status_str).unwrap_or(BoardTaskStatus::Open);
        let auto_execute: i32 = row.get("auto_execute").unwrap_or(0);

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
            order_idx: row.get("order_idx")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
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

        // Verify task exists
        let task_exists: bool = self.conn.query_row(
            "SELECT COUNT(*) > 0 FROM board_tasks WHERE id = ?1",
            params![input.task_id],
            |row| row.get(0),
        )?;
        if !task_exists {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }

        let note_type = BoardNoteType::from_str(note_type_str).unwrap_or(BoardNoteType::Note);

        self.conn.execute(
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
        let mut stmt = self.conn.prepare(
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
            let notes = self.get_board_task_notes(id)?;
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
