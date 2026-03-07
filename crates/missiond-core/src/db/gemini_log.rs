use rusqlite::params;
use super::error::DbResult;
use super::MissionDB;

impl MissionDB {
    // ============ Gemini Request Log ============

    /// Insert a Gemini request log entry (called when request starts).
    pub fn gemini_log_insert_started(
        &self,
        id: &str,
        caller: &str,
        session_id: Option<&str>,
        model: &str,
        prompt_chars: i64,
        prompt_text: Option<&str>,
    ) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO gemini_requests (id, caller, session_id, api_mode, model, prompt_chars, response_chars, queue_wait_ms, duration_ms, retry_count, status, prompt_text)
             VALUES (?1, ?2, ?3, 'pending', ?4, ?5, 0, 0, 0, 0, 'pending', ?6)",
            params![id, caller, session_id, model, prompt_chars, prompt_text],
        )?;
        Ok(())
    }

    /// Update a Gemini request log entry when request completes.
    pub fn gemini_log_update_completed(
        &self,
        id: &str,
        api_mode: &str,
        response_chars: i64,
        queue_wait_ms: i64,
        duration_ms: i64,
        retry_count: i64,
        status: &str,
        error_msg: Option<&str>,
        response_text: Option<&str>,
    ) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE gemini_requests SET api_mode = ?2, response_chars = ?3, queue_wait_ms = ?4, duration_ms = ?5, retry_count = ?6, status = ?7, error_msg = ?8, response_text = ?9 WHERE id = ?1",
            params![id, api_mode, response_chars, queue_wait_ms, duration_ms, retry_count, status, error_msg, response_text],
        )?;
        Ok(())
    }

    /// Fetch full prompt/response content by request ID.
    pub fn gemini_log_get_content(&self, request_id: &str) -> DbResult<Option<serde_json::Value>> {
        let conn = self.read_conn();
        let result = conn.query_row(
            "SELECT id, caller, model, prompt_chars, response_chars, duration_ms, status, error_msg, prompt_text, response_text, created_at
             FROM gemini_requests WHERE id = ?1",
            params![request_id],
            |row| Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "caller": row.get::<_, String>(1)?,
                "model": row.get::<_, String>(2)?,
                "prompt_chars": row.get::<_, i64>(3)?,
                "response_chars": row.get::<_, i64>(4)?,
                "duration_ms": row.get::<_, Option<i64>>(5)?,
                "status": row.get::<_, String>(6)?,
                "error_msg": row.get::<_, Option<String>>(7)?,
                "prompt_text": row.get::<_, Option<String>>(8)?,
                "response_text": row.get::<_, Option<String>>(9)?,
                "created_at": row.get::<_, String>(10)?,
            })),
        );
        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Query recent Gemini requests with optional filters.
    pub fn gemini_log_query(
        &self,
        caller: Option<&str>,
        session_id: Option<&str>,
        status: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<serde_json::Value>> {
        let conn = self.read_conn();
        let mut conditions = vec!["1=1".to_string()];
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(c) = caller {
            param_values.push(Box::new(c.to_string()));
            conditions.push(format!("caller = ?{}", param_values.len()));
        }
        if let Some(s) = session_id {
            param_values.push(Box::new(s.to_string()));
            conditions.push(format!("session_id = ?{}", param_values.len()));
        }
        if let Some(st) = status {
            param_values.push(Box::new(st.to_string()));
            conditions.push(format!("status = ?{}", param_values.len()));
        }
        param_values.push(Box::new(limit));
        let limit_idx = param_values.len();

        let sql = format!(
            "SELECT id, caller, session_id, api_mode, model, prompt_chars, response_chars, queue_wait_ms, duration_ms, retry_count, status, error_msg, created_at
             FROM gemini_requests
             WHERE {}
             ORDER BY created_at DESC
             LIMIT ?{}",
            conditions.join(" AND "), limit_idx
        );

        let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "caller": row.get::<_, String>(1)?,
                "session_id": row.get::<_, Option<String>>(2)?,
                "api_mode": row.get::<_, String>(3)?,
                "model": row.get::<_, String>(4)?,
                "prompt_chars": row.get::<_, i64>(5)?,
                "response_chars": row.get::<_, i64>(6)?,
                "queue_wait_ms": row.get::<_, i64>(7)?,
                "duration_ms": row.get::<_, i64>(8)?,
                "retry_count": row.get::<_, i64>(9)?,
                "status": row.get::<_, String>(10)?,
                "error_msg": row.get::<_, Option<String>>(11)?,
                "created_at": row.get::<_, String>(12)?,
            }))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Aggregate stats for Gemini requests (last 24h + last 7d).
    pub fn gemini_log_stats(&self) -> DbResult<serde_json::Value> {
        let conn = self.read_conn();

        // Overall stats (last 7 days)
        let (total, ok_count, err_count, avg_duration, avg_queue, total_prompt, total_response): (i64, i64, i64, f64, f64, i64, i64) = conn.query_row(
            "SELECT COUNT(*), SUM(CASE WHEN status='ok' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status!='ok' THEN 1 ELSE 0 END),
                    COALESCE(AVG(duration_ms), 0), COALESCE(AVG(queue_wait_ms), 0),
                    COALESCE(SUM(prompt_chars), 0), COALESCE(SUM(response_chars), 0)
             FROM gemini_requests WHERE created_at >= datetime('now', '-7 days')",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
        )?;

        // By caller
        let mut caller_stmt = conn.prepare(
            "SELECT caller, COUNT(*), COALESCE(AVG(duration_ms), 0), SUM(CASE WHEN status!='ok' THEN 1 ELSE 0 END)
             FROM gemini_requests WHERE created_at >= datetime('now', '-7 days')
             GROUP BY caller"
        )?;
        let by_caller: Vec<serde_json::Value> = caller_stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "caller": row.get::<_, String>(0)?,
                "count": row.get::<_, i64>(1)?,
                "avg_duration_ms": row.get::<_, f64>(2)? as i64,
                "errors": row.get::<_, i64>(3)?,
            }))
        })?.filter_map(|r| r.ok()).collect();

        // Slow requests (>30s)
        let mut slow_stmt = conn.prepare(
            "SELECT id, caller, session_id, model, duration_ms, queue_wait_ms, prompt_chars, response_chars, status, created_at
             FROM gemini_requests WHERE duration_ms > 30000 AND created_at >= datetime('now', '-7 days')
             ORDER BY duration_ms DESC LIMIT 10"
        )?;
        let slow: Vec<serde_json::Value> = slow_stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "caller": row.get::<_, String>(1)?,
                "session_id": row.get::<_, Option<String>>(2)?,
                "model": row.get::<_, String>(3)?,
                "duration_ms": row.get::<_, i64>(4)?,
                "queue_wait_ms": row.get::<_, i64>(5)?,
                "prompt_chars": row.get::<_, i64>(6)?,
                "response_chars": row.get::<_, i64>(7)?,
                "status": row.get::<_, String>(8)?,
                "created_at": row.get::<_, String>(9)?,
            }))
        })?.filter_map(|r| r.ok()).collect();

        Ok(serde_json::json!({
            "period": "7d",
            "total": total,
            "ok": ok_count,
            "errors": err_count,
            "avg_duration_ms": avg_duration as i64,
            "avg_queue_wait_ms": avg_queue as i64,
            "total_prompt_chars": total_prompt,
            "total_response_chars": total_response,
            "by_caller": by_caller,
            "slow_requests": slow,
        }))
    }

    /// Clean up old Gemini request logs (retention policy).
    pub fn gemini_log_cleanup(&self, retention_days: i64) -> DbResult<i64> {
        let conn = self.conn();
        let deleted = conn.execute(
            &format!("DELETE FROM gemini_requests WHERE created_at < datetime('now', '-{} days')", retention_days),
            [],
        )? as i64;
        Ok(deleted)
    }
}
