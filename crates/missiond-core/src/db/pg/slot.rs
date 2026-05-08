//! SlotStore — PostgreSQL implementation.

use super::PgMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::SlotStore;
use crate::types::*;
use async_trait::async_trait;
use sqlx::Row;

#[cfg(feature = "postgres")]
#[async_trait]
impl SlotStore for PgMissionStore {
    // -- slot.rs: session mapping --

    async fn get_slot_session(&self, slot_id: &str) -> DbResult<Option<String>> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT session_id FROM slot_sessions WHERE slot_id = $1")
                .bind(slot_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| r.0))
    }

    async fn set_slot_session(&self, slot_id: &str, session_id: &str) -> DbResult<()> {
        let now = chrono::Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT INTO slot_sessions (slot_id, session_id, updated_at)
             VALUES ($1, $2, $3)
             ON CONFLICT (slot_id) DO UPDATE SET session_id = $2, updated_at = $3",
        )
        .bind(slot_id)
        .bind(session_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn cleanup_pty_placeholder(&self, slot_id: &str) -> DbResult<()> {
        let session_id = format!("pty-{}", slot_id);
        let mut tx = self.pool.begin().await?;

        // Delete messages
        sqlx::query("DELETE FROM conversation_messages WHERE session_id = $1")
            .bind(&session_id)
            .execute(&mut *tx)
            .await?;

        // Delete events
        sqlx::query("DELETE FROM conversation_events WHERE session_id = $1")
            .bind(&session_id)
            .execute(&mut *tx)
            .await?;

        // Delete turns
        sqlx::query("DELETE FROM conversation_turns WHERE session_id = $1")
            .bind(&session_id)
            .execute(&mut *tx)
            .await?;

        // Delete the conversation
        sqlx::query("DELETE FROM conversations WHERE id = $1")
            .bind(&session_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn delete_slot_session(&self, slot_id: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM slot_sessions WHERE slot_id = $1")
            .bind(slot_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_all_slot_sessions(&self) -> DbResult<Vec<(String, String)>> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT slot_id, session_id FROM slot_sessions")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows)
    }

    async fn get_slot_for_session(&self, session_id: &str) -> DbResult<Option<String>> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT slot_id FROM slot_sessions WHERE session_id = $1")
                .bind(session_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| r.0))
    }

    // -- slot.rs: slot tasks --

    async fn insert_slot_task(&self, task: &SlotTask) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO slot_tasks (id, slot_id, task_type, status, prompt_summary, source_sessions, output_count, created_at, started_at, completed_at, duration_ms, error, conversation_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)"
        )
        .bind(&task.id)
        .bind(&task.slot_id)
        .bind(&task.task_type)
        .bind(&task.status)
        .bind(&task.prompt_summary)
        .bind(&task.source_sessions)
        .bind(task.output_count)
        .bind(&task.created_at)
        .bind(&task.started_at)
        .bind(&task.completed_at)
        .bind(task.duration_ms)
        .bind(&task.error)
        .bind(&task.conversation_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn slot_task_set_running(&self, id: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE slot_tasks SET status = 'running', started_at = $1 WHERE id = $2")
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn slot_task_set_completed(&self, id: &str, output_count: i64) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE slot_tasks SET status = 'completed', completed_at = $1, output_count = $2,
             duration_ms = CAST(EXTRACT(EPOCH FROM ($1::timestamp - COALESCE(started_at::timestamp, created_at::timestamp))) * 1000 AS BIGINT)
             WHERE id = $3"
        )
        .bind(&now)
        .bind(output_count)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn slot_task_set_failed(&self, id: &str, error: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE slot_tasks SET status = 'failed', completed_at = $1, error = $2,
             duration_ms = CAST(EXTRACT(EPOCH FROM ($1::timestamp - COALESCE(started_at::timestamp, created_at::timestamp))) * 1000 AS BIGINT)
             WHERE id = $3"
        )
        .bind(&now)
        .bind(error)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_slot_tasks(
        &self,
        slot_id: Option<&str>,
        task_type: Option<&str>,
        status: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<SlotTask>> {
        // Build dynamic query with optional filters
        let mut sql = String::from(
            "SELECT id, slot_id, task_type, status, prompt_summary, source_sessions,
                    output_count, created_at, started_at, completed_at, duration_ms, error, conversation_id
             FROM slot_tasks WHERE true"
        );
        let mut param_idx = 1u32;

        if slot_id.is_some() {
            sql.push_str(&format!(" AND slot_id = ${param_idx}"));
            param_idx += 1;
        }
        if task_type.is_some() {
            sql.push_str(&format!(" AND task_type = ${param_idx}"));
            param_idx += 1;
        }
        if status.is_some() {
            sql.push_str(&format!(" AND status = ${param_idx}"));
            param_idx += 1;
        }
        sql.push_str(&format!(" ORDER BY created_at DESC LIMIT ${param_idx}"));

        // We need to bind dynamically; use a match on the number of optional params
        let rows = match (slot_id, task_type, status) {
            (Some(s), Some(t), Some(st)) => {
                sqlx::query(&sql)
                    .bind(s)
                    .bind(t)
                    .bind(st)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await?
            }
            (Some(s), Some(t), None) => {
                sqlx::query(&sql)
                    .bind(s)
                    .bind(t)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await?
            }
            (Some(s), None, Some(st)) => {
                sqlx::query(&sql)
                    .bind(s)
                    .bind(st)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await?
            }
            (None, Some(t), Some(st)) => {
                sqlx::query(&sql)
                    .bind(t)
                    .bind(st)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await?
            }
            (Some(s), None, None) => {
                sqlx::query(&sql)
                    .bind(s)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await?
            }
            (None, Some(t), None) => {
                sqlx::query(&sql)
                    .bind(t)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await?
            }
            (None, None, Some(st)) => {
                sqlx::query(&sql)
                    .bind(st)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await?
            }
            (None, None, None) => sqlx::query(&sql).bind(limit).fetch_all(&self.pool).await?,
        };

        let results = rows.iter().map(|row| row_to_slot_task(row)).collect();
        Ok(results)
    }

    async fn slot_task_stats(&self, slot_id: Option<&str>) -> DbResult<serde_json::Value> {
        let (where_clause, has_param) = if slot_id.is_some() {
            ("WHERE slot_id = $1", true)
        } else {
            ("", false)
        };

        let sql = format!(
            "SELECT
                COUNT(*) as total,
                SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed,
                SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) as failed,
                SUM(CASE WHEN status = 'running' THEN 1 ELSE 0 END) as running,
                COALESCE(SUM(output_count), 0) as total_output,
                AVG(CASE WHEN status = 'completed' THEN duration_ms END) as avg_duration_ms,
                task_type,
                COUNT(*) as type_count
             FROM slot_tasks {where_clause}
             GROUP BY task_type"
        );

        let rows = if has_param {
            sqlx::query(&sql)
                .bind(slot_id.unwrap())
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query(&sql).fetch_all(&self.pool).await?
        };

        let mut total = 0i64;
        let mut completed = 0i64;
        let mut failed = 0i64;
        let mut running = 0i64;
        let mut total_output = 0i64;
        let mut type_stats = Vec::new();

        for row in &rows {
            let t: i64 = row.get("total");
            let c: i64 = row.get("completed");
            let f: i64 = row.get("failed");
            let r: i64 = row.get("running");
            let o: i64 = row.get("total_output");
            let avg: Option<f64> = row.get("avg_duration_ms");
            let task_type: String = row.get("task_type");
            let count: i64 = row.get("type_count");

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

    async fn reap_stale_slot_tasks(&self, max_age_secs: i64) -> DbResult<usize> {
        let now = chrono::Utc::now().to_rfc3339();
        let result = sqlx::query(
            "UPDATE slot_tasks SET status = 'failed', error = 'reaper: stale task', completed_at = $1
             WHERE status IN ('pending', 'running')
               AND EXTRACT(EPOCH FROM ($1::timestamp - created_at::timestamp)) > $2"
        )
        .bind(&now)
        .bind(max_age_secs as f64)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn find_stale_decision_tasks(&self, max_age_secs: i64) -> DbResult<Vec<SlotTask>> {
        let now = chrono::Utc::now().to_rfc3339();
        let rows = sqlx::query(
            "SELECT id, slot_id, task_type, status, prompt_summary, source_sessions,
                    output_count, created_at, started_at, completed_at, duration_ms, error, conversation_id
             FROM slot_tasks
             WHERE task_type = 'decision' AND status IN ('pending', 'running')
               AND EXTRACT(EPOCH FROM ($1::timestamp - created_at::timestamp)) > $2"
        )
        .bind(&now)
        .bind(max_age_secs as f64)
        .fetch_all(&self.pool)
        .await?;

        let results = rows.iter().map(|row| row_to_slot_task(row)).collect();
        Ok(results)
    }

    async fn cleanup_orphan_slot_tasks(&self) -> DbResult<usize> {
        let now = chrono::Utc::now().to_rfc3339();
        let result = sqlx::query(
            "UPDATE slot_tasks SET status = 'failed', error = 'daemon restart: orphan cleanup', completed_at = $1
             WHERE status IN ('pending', 'running')"
        )
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn last_completed_slot_task_at(&self, task_type: &str) -> DbResult<Option<i64>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT completed_at FROM slot_tasks WHERE task_type = $1 AND status = 'completed' ORDER BY completed_at DESC LIMIT 1"
        )
        .bind(task_type)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|r| {
            chrono::DateTime::parse_from_rfc3339(&r.0)
                .ok()
                .map(|dt| dt.timestamp())
        }))
    }

    async fn get_running_slot_task(&self, slot_id: &str) -> DbResult<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM slot_tasks WHERE slot_id = $1 AND status = 'running' ORDER BY created_at DESC LIMIT 1"
        )
        .bind(slot_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.0))
    }

    // -- task.rs: generic tasks --

    async fn insert_task(&self, task: &Task) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO tasks (id, role, prompt, status, slot_id, session_id, result, error, created_at, started_at, finished_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"
        )
        .bind(&task.id)
        .bind(&task.role)
        .bind(&task.prompt)
        .bind(task.status.as_str())
        .bind(&task.slot_id)
        .bind(&task.session_id)
        .bind(&task.result)
        .bind(&task.error)
        .bind(task.created_at)
        .bind(task.started_at)
        .bind(task.finished_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_task(&self, id: &str, update: &TaskUpdate) -> DbResult<()> {
        let mut fields = Vec::new();
        let mut param_idx = 1u32;

        // Build SET clause dynamically
        // We'll collect the SQL fragments and bind in order
        if update.status.is_some() {
            fields.push(format!("status = ${param_idx}"));
            param_idx += 1;
        }
        if update.slot_id.is_some() {
            fields.push(format!("slot_id = ${param_idx}"));
            param_idx += 1;
        }
        if update.session_id.is_some() {
            fields.push(format!("session_id = ${param_idx}"));
            param_idx += 1;
        }
        if update.result.is_some() {
            fields.push(format!("result = ${param_idx}"));
            param_idx += 1;
        }
        if update.error.is_some() {
            fields.push(format!("error = ${param_idx}"));
            param_idx += 1;
        }
        if update.started_at.is_some() {
            fields.push(format!("started_at = ${param_idx}"));
            param_idx += 1;
        }
        if update.finished_at.is_some() {
            fields.push(format!("finished_at = ${param_idx}"));
            param_idx += 1;
        }

        if fields.is_empty() {
            return Ok(());
        }

        let sql = format!(
            "UPDATE tasks SET {} WHERE id = ${}",
            fields.join(", "),
            param_idx
        );

        // Bind in the same order we built the SET clause
        let mut query = sqlx::query(&sql);
        if let Some(status) = &update.status {
            query = query.bind(status.as_str().to_string());
        }
        if let Some(slot_id) = &update.slot_id {
            query = query.bind(slot_id.clone());
        }
        if let Some(session_id) = &update.session_id {
            query = query.bind(session_id.clone());
        }
        if let Some(result) = &update.result {
            query = query.bind(result.clone());
        }
        if let Some(error) = &update.error {
            query = query.bind(error.clone());
        }
        if let Some(started_at) = &update.started_at {
            query = query.bind(*started_at);
        }
        if let Some(finished_at) = &update.finished_at {
            query = query.bind(*finished_at);
        }
        query = query.bind(id);

        query.execute(&self.pool).await?;
        Ok(())
    }

    async fn get_task(&self, id: &str) -> DbResult<Option<Task>> {
        // Support short ID prefix matching (like git short hashes)
        if id.len() < 36 {
            let prefix = format!("{}%", id);
            let rows = sqlx::query(
                "SELECT id, role, prompt, status, slot_id, session_id, result, error, created_at, started_at, finished_at
                 FROM tasks WHERE id LIKE $1 ORDER BY created_at DESC LIMIT 2"
            )
            .bind(&prefix)
            .fetch_all(&self.pool)
            .await?;
            match rows.len() {
                1 => Ok(Some(row_to_task(&rows[0]))),
                _ => Ok(None), // 0 = not found, 2+ = ambiguous
            }
        } else {
            let row = sqlx::query(
                "SELECT id, role, prompt, status, slot_id, session_id, result, error, created_at, started_at, finished_at
                 FROM tasks WHERE id = $1"
            )
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
            Ok(row.as_ref().map(row_to_task))
        }
    }

    async fn get_tasks_by_status(&self, status: TaskStatus) -> DbResult<Vec<Task>> {
        let rows = sqlx::query(
            "SELECT id, role, prompt, status, slot_id, session_id, result, error, created_at, started_at, finished_at
             FROM tasks WHERE status = $1 ORDER BY created_at ASC"
        )
        .bind(status.as_str())
        .fetch_all(&self.pool)
        .await?;
        let results = rows.iter().map(|row| row_to_task(row)).collect();
        Ok(results)
    }

    async fn get_queued_tasks_by_role(&self, role: &str) -> DbResult<Vec<Task>> {
        let rows = sqlx::query(
            "SELECT id, role, prompt, status, slot_id, session_id, result, error, created_at, started_at, finished_at
             FROM tasks WHERE status = 'queued' AND role = $1 ORDER BY created_at ASC"
        )
        .bind(role)
        .fetch_all(&self.pool)
        .await?;
        let results = rows.iter().map(|row| row_to_task(row)).collect();
        Ok(results)
    }

    async fn requeue_running_tasks_for_slot(&self, slot_id: &str) -> DbResult<usize> {
        let result = sqlx::query(
            "UPDATE tasks SET status = 'queued', slot_id = NULL, started_at = NULL WHERE status = 'running' AND slot_id = $1"
        )
        .bind(slot_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn get_all_tasks(&self, limit: i64) -> DbResult<Vec<Task>> {
        let rows = sqlx::query(
            "SELECT id, role, prompt, status, slot_id, session_id, result, error, created_at, started_at, finished_at
             FROM tasks ORDER BY created_at DESC LIMIT $1"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let results = rows.iter().map(|row| row_to_task(row)).collect();
        Ok(results)
    }

    async fn ack_completed_tasks(&self, since: Option<i64>) -> DbResult<Vec<Task>> {
        let cutoff = since.unwrap_or_else(|| chrono::Utc::now().timestamp_millis() - 3_600_000);
        let rows = sqlx::query(
            "SELECT id, role, prompt, status, slot_id, session_id, result, error, created_at, started_at, finished_at
             FROM tasks WHERE status IN ('done', 'failed') AND finished_at > $1 ORDER BY finished_at ASC"
        )
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await?;
        let results = rows.iter().map(|row| row_to_task(row)).collect();
        Ok(results)
    }

    // -- task.rs: inbox --

    async fn insert_inbox_message(&self, msg: &InboxMessage) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO inbox (id, task_id, from_role, content, read, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&msg.id)
        .bind(&msg.task_id)
        .bind(&msg.from_role)
        .bind(&msg.content)
        .bind(if msg.read { 1i32 } else { 0i32 })
        .bind(msg.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_inbox_messages(
        &self,
        unread_only: bool,
        limit: i64,
    ) -> DbResult<Vec<InboxMessage>> {
        let sql = if unread_only {
            "SELECT id, task_id, from_role, content, read, created_at FROM inbox WHERE read = 0 ORDER BY created_at DESC LIMIT $1"
        } else {
            "SELECT id, task_id, from_role, content, read, created_at FROM inbox ORDER BY created_at DESC LIMIT $1"
        };

        let rows = sqlx::query(sql).bind(limit).fetch_all(&self.pool).await?;

        let results = rows
            .iter()
            .map(|row| {
                let read_val: i32 = row.get("read");
                InboxMessage {
                    id: row.get("id"),
                    task_id: row.get("task_id"),
                    from_role: row.get("from_role"),
                    content: row.get("content"),
                    read: read_val == 1,
                    created_at: row.get("created_at"),
                }
            })
            .collect();
        Ok(results)
    }

    async fn mark_inbox_read(&self, id: &str) -> DbResult<()> {
        sqlx::query("UPDATE inbox SET read = 1 WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -- task.rs: events (legacy) removed v0.4.23 Phase 6 (table `events` dropped) --

    // -- dynamic_slot.rs --

    async fn create_dynamic_slot(&self, slot: &DynamicSlot) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO dynamic_slots (id, parent_slot_id, template, objective, config, status, created_at, ttl_seconds, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
        )
        .bind(&slot.id)
        .bind(&slot.parent_slot_id)
        .bind(&slot.template)
        .bind(&slot.objective)
        .bind(&slot.config)
        .bind(&slot.status)
        .bind(&slot.created_at)
        .bind(slot.ttl_seconds)
        .bind(&slot.expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_dynamic_slot(&self, id: &str) -> DbResult<Option<DynamicSlot>> {
        let row = sqlx::query(
            "SELECT id, parent_slot_id, template, objective, config, status, termination_reason,
                    created_at, terminated_at, ttl_seconds, expires_at, extend_count
             FROM dynamic_slots WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.as_ref().map(row_to_dynamic_slot))
    }

    async fn list_dynamic_slots(&self, status: Option<&str>) -> DbResult<Vec<DynamicSlot>> {
        let rows = if let Some(s) = status {
            sqlx::query(
                "SELECT id, parent_slot_id, template, objective, config, status, termination_reason,
                        created_at, terminated_at, ttl_seconds, expires_at, extend_count
                 FROM dynamic_slots WHERE status = $1 ORDER BY created_at DESC"
            )
            .bind(s)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, parent_slot_id, template, objective, config, status, termination_reason,
                        created_at, terminated_at, ttl_seconds, expires_at, extend_count
                 FROM dynamic_slots ORDER BY created_at DESC"
            )
            .fetch_all(&self.pool)
            .await?
        };

        let results = rows.iter().map(|row| row_to_dynamic_slot(row)).collect();
        Ok(results)
    }

    async fn count_active_dynamic_slots(&self) -> DbResult<i64> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM dynamic_slots WHERE status = 'active'")
                .fetch_one(&self.pool)
                .await?;
        Ok(count)
    }

    async fn terminate_dynamic_slot(&self, id: &str, reason: &str) -> DbResult<bool> {
        let now = chrono::Utc::now().to_rfc3339();
        let result = sqlx::query(
            "UPDATE dynamic_slots SET status = 'terminated', termination_reason = $1, terminated_at = $2
             WHERE id = $3 AND status = 'active'"
        )
        .bind(reason)
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn extend_dynamic_slot(
        &self,
        id: &str,
        additional_seconds: i64,
    ) -> DbResult<Option<String>> {
        // Check current state
        let row = sqlx::query(
            "SELECT expires_at, extend_count, created_at FROM dynamic_slots WHERE id = $1 AND status = 'active'"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        let row = match row {
            Some(r) => r,
            None => return Ok(None),
        };

        let current_expires: String = row.get("expires_at");
        let extend_count: i64 = row.get("extend_count");
        let created_at_str: String = row.get("created_at");

        if extend_count >= 2 {
            return Ok(None); // Max 2 extensions
        }

        // Calculate new expires_at
        let current_dt = chrono::DateTime::parse_from_rfc3339(&current_expires)
            .map_err(|e| sqlx::Error::Protocol(format!("Invalid expires_at: {}", e)))?;
        let new_expires = current_dt + chrono::Duration::seconds(additional_seconds);

        // Enforce absolute TTL cap: created_at + 8 hours (MAX_TTL_SECS)
        const MAX_TTL_SECS: i64 = 28800;
        if let Ok(created_dt) = chrono::DateTime::parse_from_rfc3339(&created_at_str) {
            let absolute_cap = created_dt + chrono::Duration::seconds(MAX_TTL_SECS);
            if new_expires > absolute_cap {
                return Ok(None); // Would exceed absolute 8h cap
            }
        }

        let new_expires_str = new_expires.to_rfc3339();

        sqlx::query(
            "UPDATE dynamic_slots SET expires_at = $1, extend_count = extend_count + 1
             WHERE id = $2 AND status = 'active'",
        )
        .bind(&new_expires_str)
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(Some(new_expires_str))
    }

    async fn find_expired_dynamic_slots(&self) -> DbResult<Vec<DynamicSlot>> {
        let now = chrono::Utc::now().to_rfc3339();
        let rows = sqlx::query(
            "SELECT id, parent_slot_id, template, objective, config, status, termination_reason,
                    created_at, terminated_at, ttl_seconds, expires_at, extend_count
             FROM dynamic_slots WHERE status = 'active' AND expires_at <= $1",
        )
        .bind(&now)
        .fetch_all(&self.pool)
        .await?;

        let results = rows.iter().map(|row| row_to_dynamic_slot(row)).collect();
        Ok(results)
    }

    async fn find_expiring_dynamic_slots(&self, within_seconds: i64) -> DbResult<Vec<DynamicSlot>> {
        let now = chrono::Utc::now().to_rfc3339();
        let deadline =
            (chrono::Utc::now() + chrono::Duration::seconds(within_seconds)).to_rfc3339();
        let rows = sqlx::query(
            "SELECT id, parent_slot_id, template, objective, config, status, termination_reason,
                    created_at, terminated_at, ttl_seconds, expires_at, extend_count
             FROM dynamic_slots WHERE status = 'active' AND expires_at > $1 AND expires_at <= $2",
        )
        .bind(&now)
        .bind(&deadline)
        .fetch_all(&self.pool)
        .await?;

        let results = rows.iter().map(|row| row_to_dynamic_slot(row)).collect();
        Ok(results)
    }
}

// -- Helper functions --

fn row_to_slot_task(row: &sqlx::postgres::PgRow) -> SlotTask {
    SlotTask {
        id: row.get("id"),
        slot_id: row.get("slot_id"),
        task_type: row.get("task_type"),
        status: row.get("status"),
        prompt_summary: row.get("prompt_summary"),
        source_sessions: row.get("source_sessions"),
        output_count: row.get::<Option<i64>, _>("output_count").unwrap_or(0),
        created_at: row.get("created_at"),
        started_at: row.get("started_at"),
        completed_at: row.get("completed_at"),
        duration_ms: row.get("duration_ms"),
        error: row.get("error"),
        conversation_id: row.get("conversation_id"),
    }
}

fn row_to_task(row: &sqlx::postgres::PgRow) -> Task {
    let status_str: String = row.get("status");
    let status = TaskStatus::from_str(&status_str).unwrap_or(TaskStatus::Queued);

    Task {
        id: row.get("id"),
        role: row.get("role"),
        prompt: row.get("prompt"),
        status,
        slot_id: row.get("slot_id"),
        session_id: row.get("session_id"),
        result: row.get("result"),
        error: row.get("error"),
        created_at: row.get("created_at"),
        started_at: row.get("started_at"),
        finished_at: row.get("finished_at"),
    }
}

fn row_to_dynamic_slot(row: &sqlx::postgres::PgRow) -> DynamicSlot {
    DynamicSlot {
        id: row.get("id"),
        parent_slot_id: row.get("parent_slot_id"),
        template: row.get("template"),
        objective: row.get("objective"),
        config: row.get("config"),
        status: row.get("status"),
        termination_reason: row.get("termination_reason"),
        created_at: row.get("created_at"),
        terminated_at: row.get("terminated_at"),
        ttl_seconds: row.get("ttl_seconds"),
        expires_at: row.get("expires_at"),
        extend_count: row.get("extend_count"),
    }
}
