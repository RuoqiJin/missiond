use rusqlite::{params, Result as SqliteResult};
use crate::types::*;
use super::MissionDB;

impl MissionDB {
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
        tokio::task::block_in_place(|| {
            let conn = self.read_conn();
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
        })
    }

    /// Get all tasks by status
    pub fn get_tasks_by_status(&self, status: TaskStatus) -> SqliteResult<Vec<Task>> {
        tokio::task::block_in_place(|| {
            let conn = self.read_conn();
            let mut stmt = conn
                .prepare("SELECT * FROM tasks WHERE status = ? ORDER BY created_at ASC")?;
            let rows = stmt.query_map(params![status.as_str()], |row| Self::row_to_task(row))?;

            let mut tasks = Vec::new();
            for task in rows {
                tasks.push(task?);
            }
            Ok(tasks)
        })
    }

    /// Get queued tasks by role
    pub fn get_queued_tasks_by_role(&self, role: &str) -> SqliteResult<Vec<Task>> {
        let conn = self.read_conn();
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
        tokio::task::block_in_place(|| {
            let conn = self.read_conn();
            let mut stmt = conn
                .prepare("SELECT * FROM tasks ORDER BY created_at DESC LIMIT ?")?;
            let rows = stmt.query_map(params![limit], |row| Self::row_to_task(row))?;

            let mut tasks = Vec::new();
            for task in rows {
                tasks.push(task?);
            }
            Ok(tasks)
        })
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

        let conn = self.read_conn();
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
        let conn = self.read_conn();
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


}
