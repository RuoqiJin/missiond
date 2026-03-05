use rusqlite::{params, OptionalExtension, Result as SqliteResult};
use crate::types::*;
use super::MissionDB;

impl MissionDB {
    // ============ Slot Sessions ============

    /// Get session ID for a slot
    pub fn get_slot_session(&self, slot_id: &str) -> SqliteResult<Option<String>> {
        let conn = self.read_conn();
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
        let conn = self.read_conn();
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
        let conn = self.read_conn();
        conn.query_row(
            "SELECT slot_id FROM slot_sessions WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        ).optional()
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

    /// Find stale decision slot tasks (for Decision Engine timeout recovery)
    pub fn find_stale_decision_tasks(&self, max_age_secs: i64) -> SqliteResult<Vec<SlotTask>> {
        let conn = self.read_conn();
        let now = chrono::Utc::now().to_rfc3339();
        let mut stmt = conn.prepare(
            "SELECT * FROM slot_tasks
             WHERE task_type = 'decision' AND status IN ('pending', 'running')
               AND julianday(?1) - julianday(created_at) > ?2 / 86400.0"
        )?;
        let rows = stmt.query_map(params![now, max_age_secs as f64], |row| {
            Ok(SlotTask {
                id: row.get("id")?,
                slot_id: row.get("slot_id")?,
                task_type: row.get("task_type")?,
                status: row.get("status")?,
                prompt_summary: row.get("prompt_summary")?,
                source_sessions: row.get("source_sessions")?,
                output_count: row.get::<_, Option<i64>>("output_count")?.unwrap_or(0),
                created_at: row.get("created_at")?,
                started_at: row.get("started_at")?,
                completed_at: row.get("completed_at")?,
                duration_ms: row.get("duration_ms")?,
                error: row.get("error")?,
                conversation_id: row.get("conversation_id")?,
            })
        })?;
        rows.collect()
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


}
