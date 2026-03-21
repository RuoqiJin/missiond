//! ObservabilityStore — PostgreSQL implementation.
//!
//! Covers: Gemini log, incidents, token ledger, watermarks, labels,
//!         backfill progress, and router chat sessions.

use async_trait::async_trait;
use sqlx::{Column, Row};
use crate::db::error::DbResult;
use crate::db::traits::{ObservabilityStore, BackfillPhaseStatus};
use crate::types::*;
use std::collections::HashMap;
use super::PgMissionStore;

#[cfg(feature = "postgres")]
#[async_trait]
impl ObservabilityStore for PgMissionStore {
    // ── gemini_log.rs ─────────────────────────────────────────────

    async fn gemini_log_insert_started(
        &self,
        id: &str,
        caller: &str,
        session_id: Option<&str>,
        model: &str,
        prompt_chars: i64,
        prompt_text: Option<&str>,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO gemini_requests (id, caller, session_id, api_mode, model, prompt_chars, response_chars, queue_wait_ms, duration_ms, retry_count, status, prompt_text)
             VALUES ($1, $2, $3, 'pending', $4, $5, 0, 0, 0, 0, 'pending', $6)"
        )
        .bind(id)
        .bind(caller)
        .bind(session_id)
        .bind(model)
        .bind(prompt_chars)
        .bind(prompt_text)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn gemini_log_update_completed(
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
        sqlx::query(
            "UPDATE gemini_requests SET api_mode = $2, response_chars = $3, queue_wait_ms = $4,
                    duration_ms = $5, retry_count = $6, status = $7, error_msg = $8, response_text = $9
             WHERE id = $1"
        )
        .bind(id)
        .bind(api_mode)
        .bind(response_chars)
        .bind(queue_wait_ms)
        .bind(duration_ms)
        .bind(retry_count)
        .bind(status)
        .bind(error_msg)
        .bind(response_text)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn gemini_log_get_content(&self, request_id: &str) -> DbResult<Option<serde_json::Value>> {
        let row = sqlx::query(
            "SELECT id, caller, model, prompt_chars, response_chars, duration_ms, status, error_msg, prompt_text, response_text, created_at
             FROM gemini_requests WHERE id = $1"
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            use sqlx::Row;
            serde_json::json!({
                "id": r.get::<String, _>("id"),
                "caller": r.get::<String, _>("caller"),
                "model": r.get::<String, _>("model"),
                "prompt_chars": r.get::<i64, _>("prompt_chars"),
                "response_chars": r.get::<i64, _>("response_chars"),
                "duration_ms": r.get::<Option<i64>, _>("duration_ms"),
                "status": r.get::<String, _>("status"),
                "error_msg": r.get::<Option<String>, _>("error_msg"),
                "prompt_text": r.get::<Option<String>, _>("prompt_text"),
                "response_text": r.get::<Option<String>, _>("response_text"),
                "created_at": r.get::<String, _>("created_at"),
            })
        }))
    }

    async fn gemini_log_query(
        &self,
        caller: Option<&str>,
        session_id: Option<&str>,
        status: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<serde_json::Value>> {
        // Use explicit branching for type-safe sqlx parameter binding.
        let rows = match (caller, session_id, status) {
            (None, None, None) => {
                sqlx::query(
                    "SELECT id, caller, session_id, api_mode, model, prompt_chars, response_chars, queue_wait_ms, duration_ms, retry_count, status, error_msg, created_at
                     FROM gemini_requests ORDER BY created_at DESC LIMIT $1"
                )
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(c), None, None) => {
                sqlx::query(
                    "SELECT id, caller, session_id, api_mode, model, prompt_chars, response_chars, queue_wait_ms, duration_ms, retry_count, status, error_msg, created_at
                     FROM gemini_requests WHERE caller = $1 ORDER BY created_at DESC LIMIT $2"
                )
                .bind(c)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(s), None) => {
                sqlx::query(
                    "SELECT id, caller, session_id, api_mode, model, prompt_chars, response_chars, queue_wait_ms, duration_ms, retry_count, status, error_msg, created_at
                     FROM gemini_requests WHERE session_id = $1 ORDER BY created_at DESC LIMIT $2"
                )
                .bind(s)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None, Some(st)) => {
                sqlx::query(
                    "SELECT id, caller, session_id, api_mode, model, prompt_chars, response_chars, queue_wait_ms, duration_ms, retry_count, status, error_msg, created_at
                     FROM gemini_requests WHERE status = $1 ORDER BY created_at DESC LIMIT $2"
                )
                .bind(st)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(c), Some(s), None) => {
                sqlx::query(
                    "SELECT id, caller, session_id, api_mode, model, prompt_chars, response_chars, queue_wait_ms, duration_ms, retry_count, status, error_msg, created_at
                     FROM gemini_requests WHERE caller = $1 AND session_id = $2 ORDER BY created_at DESC LIMIT $3"
                )
                .bind(c)
                .bind(s)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(c), None, Some(st)) => {
                sqlx::query(
                    "SELECT id, caller, session_id, api_mode, model, prompt_chars, response_chars, queue_wait_ms, duration_ms, retry_count, status, error_msg, created_at
                     FROM gemini_requests WHERE caller = $1 AND status = $2 ORDER BY created_at DESC LIMIT $3"
                )
                .bind(c)
                .bind(st)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(s), Some(st)) => {
                sqlx::query(
                    "SELECT id, caller, session_id, api_mode, model, prompt_chars, response_chars, queue_wait_ms, duration_ms, retry_count, status, error_msg, created_at
                     FROM gemini_requests WHERE session_id = $1 AND status = $2 ORDER BY created_at DESC LIMIT $3"
                )
                .bind(s)
                .bind(st)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(c), Some(s), Some(st)) => {
                sqlx::query(
                    "SELECT id, caller, session_id, api_mode, model, prompt_chars, response_chars, queue_wait_ms, duration_ms, retry_count, status, error_msg, created_at
                     FROM gemini_requests WHERE caller = $1 AND session_id = $2 AND status = $3 ORDER BY created_at DESC LIMIT $4"
                )
                .bind(c)
                .bind(s)
                .bind(st)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
        };

        Ok(rows.iter().map(|r| {
            use sqlx::Row;
            serde_json::json!({
                "id": r.get::<String, _>("id"),
                "caller": r.get::<String, _>("caller"),
                "session_id": r.get::<Option<String>, _>("session_id"),
                "api_mode": r.get::<String, _>("api_mode"),
                "model": r.get::<String, _>("model"),
                "prompt_chars": r.get::<i64, _>("prompt_chars"),
                "response_chars": r.get::<i64, _>("response_chars"),
                "queue_wait_ms": r.get::<i64, _>("queue_wait_ms"),
                "duration_ms": r.get::<i64, _>("duration_ms"),
                "retry_count": r.get::<i64, _>("retry_count"),
                "status": r.get::<String, _>("status"),
                "error_msg": r.get::<Option<String>, _>("error_msg"),
                "created_at": r.get::<String, _>("created_at"),
            })
        }).collect())
    }

    async fn gemini_log_stats(&self) -> DbResult<serde_json::Value> {
        // Overall stats (last 7 days)
        let row = sqlx::query(
            "SELECT COUNT(*), SUM(CASE WHEN status='ok' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status!='ok' THEN 1 ELSE 0 END),
                    COALESCE(AVG(duration_ms), 0), COALESCE(AVG(queue_wait_ms), 0),
                    COALESCE(SUM(prompt_chars), 0), COALESCE(SUM(response_chars), 0)
             FROM gemini_requests WHERE created_at >= to_char(NOW() AT TIME ZONE 'UTC' - INTERVAL '7 days', 'YYYY-MM-DD HH24:MI:SS')"
        )
        .fetch_one(&self.pool)
        .await?;

        use sqlx::Row;
        let total: i64 = row.get(0);
        let ok_count: i64 = row.get(1);
        let err_count: i64 = row.get(2);
        let avg_duration: f64 = row.get(3);
        let avg_queue: f64 = row.get(4);
        let total_prompt: i64 = row.get(5);
        let total_response: i64 = row.get(6);

        // By caller
        let caller_rows = sqlx::query(
            "SELECT caller, COUNT(*), COALESCE(AVG(duration_ms), 0), SUM(CASE WHEN status!='ok' THEN 1 ELSE 0 END)
             FROM gemini_requests WHERE created_at >= to_char(NOW() AT TIME ZONE 'UTC' - INTERVAL '7 days', 'YYYY-MM-DD HH24:MI:SS')
             GROUP BY caller"
        )
        .fetch_all(&self.pool)
        .await?;

        let by_caller: Vec<serde_json::Value> = caller_rows.iter().map(|r| {
            serde_json::json!({
                "caller": r.get::<String, _>(0),
                "count": r.get::<i64, _>(1),
                "avg_duration_ms": r.get::<f64, _>(2) as i64,
                "errors": r.get::<i64, _>(3),
            })
        }).collect();

        // Slow requests (>30s)
        let slow_rows = sqlx::query(
            "SELECT id, caller, session_id, model, duration_ms, queue_wait_ms, prompt_chars, response_chars, status, created_at
             FROM gemini_requests WHERE duration_ms > 30000
               AND created_at >= to_char(NOW() AT TIME ZONE 'UTC' - INTERVAL '7 days', 'YYYY-MM-DD HH24:MI:SS')
             ORDER BY duration_ms DESC LIMIT 10"
        )
        .fetch_all(&self.pool)
        .await?;

        let slow: Vec<serde_json::Value> = slow_rows.iter().map(|r| {
            serde_json::json!({
                "id": r.get::<String, _>("id"),
                "caller": r.get::<String, _>("caller"),
                "session_id": r.get::<Option<String>, _>("session_id"),
                "model": r.get::<String, _>("model"),
                "duration_ms": r.get::<i64, _>("duration_ms"),
                "queue_wait_ms": r.get::<i64, _>("queue_wait_ms"),
                "prompt_chars": r.get::<i64, _>("prompt_chars"),
                "response_chars": r.get::<i64, _>("response_chars"),
                "status": r.get::<String, _>("status"),
                "created_at": r.get::<String, _>("created_at"),
            })
        }).collect();

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

    async fn gemini_log_cleanup(&self, retention_days: i64) -> DbResult<i64> {
        let result = sqlx::query(
            "DELETE FROM gemini_requests WHERE created_at < to_char(NOW() AT TIME ZONE 'UTC' - make_interval(days => $1), 'YYYY-MM-DD HH24:MI:SS')"
        )
        .bind(retention_days as i32)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as i64)
    }

    async fn gemini_file_cache_get(&self, file_hash: &str) -> DbResult<Option<String>> {
        let now = chrono::Utc::now().timestamp();
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT file_uri FROM gemini_file_uploads WHERE file_hash = $1 AND expires_at > $2"
        )
        .bind(file_hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.0))
    }

    async fn gemini_file_cache_put(
        &self,
        file_hash: &str,
        file_uri: &str,
        mime_type: &str,
        expires_at: i64,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO gemini_file_uploads (file_hash, file_uri, mime_type, expires_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (file_hash) DO UPDATE SET
                file_uri = EXCLUDED.file_uri,
                mime_type = EXCLUDED.mime_type,
                expires_at = EXCLUDED.expires_at"
        )
        .bind(file_hash)
        .bind(file_uri)
        .bind(mime_type)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn gemini_file_cache_gc(&self) -> DbResult<i64> {
        let now = chrono::Utc::now().timestamp();
        let result = sqlx::query("DELETE FROM gemini_file_uploads WHERE expires_at < $1")
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() as i64)
    }

    // ── incidents ─────────────────────────────────────────────────

    async fn insert_incident(
        &self,
        id: &str,
        severity: &str,
        source: &str,
        title: &str,
        description: &str,
        server_id: Option<&str>,
        raw_payload: Option<&str>,
        board_task_id: Option<&str>,
        dedupe_key: &str,
    ) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO incidents (id, severity, source, title, description, server_id, raw_payload, board_task_id, dedupe_key, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"
        )
        .bind(id)
        .bind(severity)
        .bind(source)
        .bind(title)
        .bind(description)
        .bind(server_id)
        .bind(raw_payload)
        .bind(board_task_id)
        .bind(dedupe_key)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn has_recent_incident(&self, dedupe_key: &str, window_secs: i64) -> DbResult<bool> {
        let now = chrono::Utc::now().to_rfc3339();
        // PG: use EXTRACT(EPOCH FROM ...) for date diff
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM incidents
             WHERE dedupe_key = $1
               AND EXTRACT(EPOCH FROM ($2::timestamp - created_at::timestamp)) < $3"
        )
        .bind(dedupe_key)
        .bind(&now)
        .bind(window_secs as f64)
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    async fn list_incidents(&self, limit: i64) -> DbResult<Vec<IncidentRow>> {
        let rows = sqlx::query(
            "SELECT id, severity, source, title, description, server_id, board_task_id, dedupe_key, created_at
             FROM incidents ORDER BY created_at DESC LIMIT $1"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(|r| {
            use sqlx::Row;
            IncidentRow {
                id: r.get("id"),
                severity: r.get("severity"),
                source: r.get("source"),
                title: r.get("title"),
                description: r.get("description"),
                server_id: r.get("server_id"),
                board_task_id: r.get("board_task_id"),
                dedupe_key: r.get("dedupe_key"),
                created_at: r.get("created_at"),
            }
        }).collect())
    }

    // ── token ledger ──────────────────────────────────────────────

    async fn insert_token_usage(
        &self,
        conversation_id: &str,
        slot_id: Option<&str>,
        slot_task_id: Option<&str>,
        model: Option<&str>,
        input_tokens: i64,
        cache_creation_tokens: i64,
        cache_read_tokens: i64,
        output_tokens: i64,
        message_id: Option<i64>,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO token_usage_ledger
                (conversation_id, slot_id, slot_task_id, model,
                 input_tokens, cache_creation_tokens, cache_read_tokens, output_tokens,
                 message_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (message_id) WHERE message_id IS NOT NULL DO NOTHING"
        )
        .bind(conversation_id)
        .bind(slot_id)
        .bind(slot_task_id)
        .bind(model)
        .bind(input_tokens)
        .bind(cache_creation_tokens)
        .bind(cache_read_tokens)
        .bind(output_tokens)
        .bind(message_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn token_stats(
        &self,
        conversation_id: Option<&str>,
        slot_id: Option<&str>,
        since: Option<&str>,
        group_by: Option<&str>,
    ) -> DbResult<Vec<HashMap<String, serde_json::Value>>> {
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
             FROM token_usage_ledger WHERE 1=1"
        );

        // Build filter params — use explicit branching for type safety with sqlx
        // Count the dynamic params to figure out parameter indices
        let mut param_idx = 0u32;
        let mut cond_parts: Vec<String> = Vec::new();
        if conversation_id.is_some() {
            param_idx += 1;
            cond_parts.push(format!(" AND conversation_id = ${param_idx}"));
        }
        if slot_id.is_some() {
            param_idx += 1;
            cond_parts.push(format!(" AND slot_id = ${param_idx}"));
        }
        if since.is_some() {
            param_idx += 1;
            cond_parts.push(format!(" AND created_at >= ${param_idx}"));
        }

        for cond in &cond_parts {
            sql.push_str(cond);
        }

        if let Some(col) = group_col {
            sql.push_str(&format!(" GROUP BY {col} ORDER BY total_output DESC"));
        }

        // We need to dynamically bind params. Use a raw query approach.
        let rows = sqlx::query(&sql);
        let rows = if let Some(cid) = conversation_id { rows.bind(cid) } else { rows };
        let rows = if let Some(sid) = slot_id { rows.bind(sid) } else { rows };
        let rows = if let Some(s) = since { rows.bind(s) } else { rows };
        let fetched = rows.fetch_all(&self.pool).await?;

        let mut results = Vec::new();
        for r in &fetched {
            use sqlx::Row;
            let mut map = HashMap::new();
            // Try to read all columns by index
            let col_count = r.columns().len();
            for i in 0..col_count {
                let col_name = r.columns()[i].name().to_string();
                // Try integer first, then float, then string
                let val: serde_json::Value = if let Ok(v) = r.try_get::<i64, _>(i) {
                    serde_json::json!(v)
                } else if let Ok(v) = r.try_get::<f64, _>(i) {
                    serde_json::json!(v)
                } else if let Ok(v) = r.try_get::<String, _>(i) {
                    serde_json::json!(v)
                } else {
                    serde_json::Value::Null
                };
                map.insert(col_name, val);
            }
            results.push(map);
        }
        Ok(results)
    }

    // ── watermarks ────────────────────────────────────────────────

    async fn watermark_get(&self, consumer: &str, session_id: &str) -> DbResult<Option<(Option<i64>, Option<String>)>> {
        let row: Option<(Option<i64>, Option<String>)> = sqlx::query_as(
            "SELECT last_processed_msg_id, last_processed_time FROM consumer_watermarks
             WHERE consumer_name = $1 AND session_id = $2"
        )
        .bind(consumer)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn watermark_advance_time(&self, consumer: &str, session_id: &str, timestamp: &str) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO consumer_watermarks (consumer_name, session_id, last_processed_time)
             VALUES ($1, $2, $3)
             ON CONFLICT(consumer_name, session_id) DO UPDATE SET last_processed_time = $3"
        )
        .bind(consumer)
        .bind(session_id)
        .bind(timestamp)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn watermark_advance_msg_id(&self, consumer: &str, session_id: &str, msg_id: i64) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO consumer_watermarks (consumer_name, session_id, last_processed_msg_id)
             VALUES ($1, $2, $3)
             ON CONFLICT(consumer_name, session_id) DO UPDATE SET last_processed_msg_id = $3"
        )
        .bind(consumer)
        .bind(session_id)
        .bind(msg_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn watermark_advance_full(
        &self,
        consumer: &str,
        session_id: &str,
        msg_id: Option<i64>,
        timestamp: Option<&str>,
        extra: Option<&str>,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO consumer_watermarks (consumer_name, session_id, last_processed_msg_id, last_processed_time, extra)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT(consumer_name, session_id) DO UPDATE SET
                last_processed_msg_id = COALESCE($3, consumer_watermarks.last_processed_msg_id),
                last_processed_time = COALESCE($4, consumer_watermarks.last_processed_time),
                extra = COALESCE($5, consumer_watermarks.extra)"
        )
        .bind(consumer)
        .bind(session_id)
        .bind(msg_id)
        .bind(timestamp)
        .bind(extra)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn watermark_list(&self, consumer: &str) -> DbResult<Vec<(String, Option<i64>, Option<String>)>> {
        let rows: Vec<(String, Option<i64>, Option<String>)> = sqlx::query_as(
            "SELECT session_id, last_processed_msg_id, last_processed_time
             FROM consumer_watermarks WHERE consumer_name = $1"
        )
        .bind(consumer)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // ── labels ────────────────────────────────────────────────────

    async fn label_set(&self, message_id: i64, label: &str, value: &str, source: &str) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO message_labels (message_id, label, value, source)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (message_id, label) DO UPDATE SET
                value = EXCLUDED.value,
                source = EXCLUDED.source"
        )
        .bind(message_id)
        .bind(label)
        .bind(value)
        .bind(source)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn label_set_batch(&self, labels: &[(i64, &str, &str, &str)]) -> DbResult<usize> {
        if labels.is_empty() {
            return Ok(0);
        }
        let mut count = 0usize;
        for (msg_id, label, value, source) in labels {
            sqlx::query(
                "INSERT INTO message_labels (message_id, label, value, source)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (message_id, label) DO UPDATE SET
                    value = EXCLUDED.value,
                    source = EXCLUDED.source"
            )
            .bind(msg_id)
            .bind(label)
            .bind(value)
            .bind(source)
            .execute(&self.pool)
            .await?;
            count += 1;
        }
        Ok(count)
    }

    async fn label_get(&self, message_id: i64) -> DbResult<Vec<(String, String, String)>> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT label, COALESCE(value, ''), source FROM message_labels WHERE message_id = $1"
        )
        .bind(message_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn label_get_batch(&self, message_ids: &[i64]) -> DbResult<HashMap<i64, Vec<(String, String)>>> {
        if message_ids.is_empty() {
            return Ok(HashMap::new());
        }
        // PG: use ANY($1) with array parameter
        let rows = sqlx::query(
            "SELECT message_id, label, COALESCE(value, '') FROM message_labels WHERE message_id = ANY($1)"
        )
        .bind(message_ids)
        .fetch_all(&self.pool)
        .await?;

        let mut result: HashMap<i64, Vec<(String, String)>> = HashMap::new();
        for r in &rows {
            use sqlx::Row;
            let msg_id: i64 = r.get("message_id");
            let label: String = r.get("label");
            let value: String = r.get(2);
            result.entry(msg_id).or_default().push((label, value));
        }
        Ok(result)
    }

    async fn label_find_messages(&self, label: &str, value: Option<&str>, limit: i64) -> DbResult<Vec<i64>> {
        if let Some(v) = value {
            let rows: Vec<(i64,)> = sqlx::query_as(
                "SELECT message_id FROM message_labels WHERE label = $1 AND value = $2 LIMIT $3"
            )
            .bind(label)
            .bind(v)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
            Ok(rows.into_iter().map(|r| r.0).collect())
        } else {
            let rows: Vec<(i64,)> = sqlx::query_as(
                "SELECT message_id FROM message_labels WHERE label = $1 LIMIT $2"
            )
            .bind(label)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
            Ok(rows.into_iter().map(|r| r.0).collect())
        }
    }

    // ── backfill ──────────────────────────────────────────────────

    async fn backfill_get_phase(&self, phase: &str) -> DbResult<Option<BackfillPhaseStatus>> {
        let row = sqlx::query(
            "SELECT phase, status, last_cursor, total_estimated, processed, failed, started_at, completed_at
             FROM backfill_progress WHERE phase = $1"
        )
        .bind(phase)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            use sqlx::Row;
            BackfillPhaseStatus {
                phase: r.get("phase"),
                status: r.get("status"),
                last_cursor: r.get("last_cursor"),
                total_estimated: r.get("total_estimated"),
                processed: r.get("processed"),
                failed: r.get("failed"),
                started_at: r.get("started_at"),
                completed_at: r.get("completed_at"),
            }
        }))
    }

    async fn backfill_list_phases(&self) -> DbResult<Vec<BackfillPhaseStatus>> {
        let rows = sqlx::query(
            "SELECT phase, status, last_cursor, total_estimated, processed, failed, started_at, completed_at
             FROM backfill_progress ORDER BY phase"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(|r| {
            use sqlx::Row;
            BackfillPhaseStatus {
                phase: r.get("phase"),
                status: r.get("status"),
                last_cursor: r.get("last_cursor"),
                total_estimated: r.get("total_estimated"),
                processed: r.get("processed"),
                failed: r.get("failed"),
                started_at: r.get("started_at"),
                completed_at: r.get("completed_at"),
            }
        }).collect())
    }

    async fn backfill_start_phase(&self, phase: &str, total_estimated: i64) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO backfill_progress (phase, status, last_cursor, total_estimated, processed, failed, started_at, updated_at)
             VALUES ($1, 'running', 0, $2, 0, 0, $3, $3)
             ON CONFLICT(phase) DO UPDATE SET
               status = 'running',
               last_cursor = CASE WHEN backfill_progress.status = 'completed' THEN 0 ELSE backfill_progress.last_cursor END,
               total_estimated = $2,
               processed = CASE WHEN backfill_progress.status = 'completed' THEN 0 ELSE backfill_progress.processed END,
               failed = CASE WHEN backfill_progress.status = 'completed' THEN 0 ELSE backfill_progress.failed END,
               started_at = CASE WHEN backfill_progress.status = 'completed' THEN $3 ELSE backfill_progress.started_at END,
               updated_at = $3"
        )
        .bind(phase)
        .bind(total_estimated)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn backfill_update_progress(
        &self,
        phase: &str,
        new_cursor: i64,
        batch_success: i64,
        batch_failed: i64,
    ) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE backfill_progress
             SET last_cursor = $2, processed = processed + $3, failed = failed + $4, updated_at = $5
             WHERE phase = $1"
        )
        .bind(phase)
        .bind(new_cursor)
        .bind(batch_success)
        .bind(batch_failed)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn backfill_complete_phase(&self, phase: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE backfill_progress
             SET status = 'completed', completed_at = $2, updated_at = $2
             WHERE phase = $1"
        )
        .bind(phase)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn backfill_record_failure(&self, session_id: &str, phase: &str, error: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO backfill_failures (session_id, phase, retry_count, last_error, updated_at)
             VALUES ($1, $2, 1, $3, $4)
             ON CONFLICT(session_id, phase) DO UPDATE SET
               retry_count = backfill_failures.retry_count + 1,
               last_error = $3,
               updated_at = $4"
        )
        .bind(session_id)
        .bind(phase)
        .bind(error)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn backfill_retryable_failures(&self, phase: &str, max_retries: i64, limit: i64) -> DbResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT session_id FROM backfill_failures
             WHERE phase = $1 AND retry_count < $2
               AND updated_at < to_char(NOW() AT TIME ZONE 'UTC' - INTERVAL '5 minutes', 'YYYY-MM-DD HH24:MI:SS')
             ORDER BY updated_at ASC LIMIT $3"
        )
        .bind(phase)
        .bind(max_retries)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn backfill_retryable_failures_no_cooldown(&self, phase: &str, max_retries: i64) -> DbResult<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM backfill_failures WHERE phase = $1 AND retry_count < $2"
        )
        .bind(phase)
        .bind(max_retries)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    async fn backfill_clear_failure(&self, session_id: &str, phase: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM backfill_failures WHERE session_id = $1 AND phase = $2")
            .bind(session_id)
            .bind(phase)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn conversations_missing_summary_cursor(&self, cursor: i64, limit: i64) -> DbResult<Vec<(i64, String)>> {
        // PG: conversations.id is TEXT. Use ROW_NUMBER() as a stable cursor surrogate
        // mirroring SQLite's implicit rowid pagination.
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT rn, cid FROM (
                SELECT ROW_NUMBER() OVER (ORDER BY started_at, id) AS rn, id AS cid
                FROM conversations
                WHERE (llm_summary IS NULL OR llm_summary = '[timeout]')
                  AND status = 'completed' AND message_count >= 6
                  AND conversation_type IN ('user', 'worker')
            ) sub WHERE rn > $1
            ORDER BY rn ASC LIMIT $2"
        )
        .bind(cursor)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn conversations_needing_topic_vectors_cursor(
        &self,
        provider: &str,
        cursor: i64,
        limit: i64,
    ) -> DbResult<Vec<(i64, String)>> {
        // PG: conversations.id is TEXT. Use ROW_NUMBER() as cursor surrogate.
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT rn, cid FROM (
                SELECT ROW_NUMBER() OVER (ORDER BY c.started_at, c.id) AS rn, c.id AS cid
                FROM conversations c
                WHERE c.llm_summary IS NOT NULL
                  AND c.conversation_type IN ('user', 'worker')
                  AND NOT EXISTS (
                      SELECT 1 FROM conversation_topic_vectors tv
                      WHERE tv.session_id = c.id AND tv.embedding_provider = $1
                  )
            ) sub WHERE rn > $2
            ORDER BY rn ASC LIMIT $3"
        )
        .bind(provider)
        .bind(cursor)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn conversations_missing_summary_count(&self) -> DbResult<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversations
             WHERE (llm_summary IS NULL OR llm_summary = '[timeout]')
               AND status = 'completed' AND message_count >= 6
               AND conversation_type IN ('user', 'worker')"
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    async fn conversations_needing_topic_vectors_count(&self, provider: &str) -> DbResult<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversations c
             WHERE c.llm_summary IS NOT NULL
               AND c.conversation_type IN ('user', 'worker')
               AND NOT EXISTS (
                   SELECT 1 FROM conversation_topic_vectors tv
                   WHERE tv.session_id = c.id AND tv.embedding_provider = $1
               )"
        )
        .bind(provider)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    // ── router chat ───────────────────────────────────────────────

    async fn router_chat_get_or_create(&self, task_id: &str, model: &str) -> DbResult<String> {
        // Find existing active router_chat conversation for this task
        let existing: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM conversations WHERE task_id = $1 AND chat_type = 'router_chat' AND status = 'active' LIMIT 1"
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((id,)) = existing {
            return Ok(id);
        }

        // Create new
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO conversations (id, source, model, task_id, chat_type, message_count, started_at, status)
             VALUES ($1, 'router_chat', $2, $3, 'router_chat', 0, $4, 'active')"
        )
        .bind(&id)
        .bind(model)
        .bind(task_id)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    async fn router_chat_load_history(&self, conv_id: &str) -> DbResult<Vec<serde_json::Value>> {
        let rows = sqlx::query(
            "SELECT role, content FROM conversation_messages WHERE session_id = $1 ORDER BY id ASC"
        )
        .bind(conv_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(|r| {
            use sqlx::Row;
            let role: String = r.get("role");
            let content: String = r.get("content");
            serde_json::json!({"role": role, "content": content})
        }).collect())
    }

    async fn router_chat_get_summary(&self, conv_id: &str) -> DbResult<(Option<String>, i64)> {
        let row: (Option<String>, i64) = sqlx::query_as(
            "SELECT rolling_summary, COALESCE(last_summarized_msg_id, 0)
             FROM conversations WHERE id = $1"
        )
        .bind(conv_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    async fn router_chat_load_active_history(&self, conv_id: &str, after_id: i64) -> DbResult<Vec<serde_json::Value>> {
        let rows = sqlx::query(
            "SELECT role, content FROM conversation_messages
             WHERE session_id = $1 AND id > $2
             ORDER BY id ASC"
        )
        .bind(conv_id)
        .bind(after_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(|r| {
            use sqlx::Row;
            let role: String = r.get("role");
            let content: String = r.get("content");
            serde_json::json!({"role": role, "content": content})
        }).collect())
    }

    async fn router_chat_unsummarized_count(&self, conv_id: &str, after_id: i64) -> DbResult<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversation_messages WHERE session_id = $1 AND id > $2"
        )
        .bind(conv_id)
        .bind(after_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    async fn router_chat_load_compressible(
        &self,
        conv_id: &str,
        after_id: i64,
        batch_size: i64,
    ) -> DbResult<Vec<(i64, String, String)>> {
        let rows: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT id, role, content FROM conversation_messages
             WHERE session_id = $1 AND id > $2
             ORDER BY id ASC LIMIT $3"
        )
        .bind(conv_id)
        .bind(after_id)
        .bind(batch_size)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn router_chat_update_summary(
        &self,
        conv_id: &str,
        new_summary: &str,
        new_cursor: i64,
        expected_old_cursor: i64,
    ) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE conversations
             SET rolling_summary = $1, last_summarized_msg_id = $2
             WHERE id = $3 AND COALESCE(last_summarized_msg_id, 0) = $4"
        )
        .bind(new_summary)
        .bind(new_cursor)
        .bind(conv_id)
        .bind(expected_old_cursor)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn router_chat_append_messages(
        &self,
        conv_id: &str,
        messages: &[(String, String)],
    ) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        for (role, content) in messages {
            sqlx::query(
                "INSERT INTO conversation_messages (session_id, role, content, timestamp)
                 VALUES ($1, $2, $3, $4)"
            )
            .bind(conv_id)
            .bind(role)
            .bind(content)
            .bind(&now)
            .execute(&self.pool)
            .await?;
        }
        // Update message count
        sqlx::query(
            "UPDATE conversations SET message_count = (SELECT COUNT(*) FROM conversation_messages WHERE session_id = $1) WHERE id = $1"
        )
        .bind(conv_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn jarvis_get_or_create(&self, conversation_id: Option<&str>) -> DbResult<String> {
        // Reuse existing conversation if ID provided
        if let Some(id) = conversation_id {
            let (exists,): (bool,) = sqlx::query_as(
                "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = $1 AND source = 'jarvis_ui')"
            )
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
            if exists {
                return Ok(id.to_string());
            }
        }

        // Create new
        let id = format!("jarvis-{}", uuid::Uuid::new_v4().simple());
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO conversations (id, source, model, chat_type, message_count, started_at, status, conversation_type)
             VALUES ($1, 'jarvis_ui', 'claude-code', 'jarvis', 0, $2, 'active', 'user')"
        )
        .bind(&id)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    async fn jarvis_save_exchange(
        &self,
        conv_id: &str,
        user_message: &str,
        assistant_message: &str,
    ) -> DbResult<()> {
        self.router_chat_append_messages(conv_id, &[
            ("user".to_string(), user_message.to_string()),
            ("assistant".to_string(), assistant_message.to_string()),
        ]).await
    }

    async fn jarvis_update_title(&self, conv_id: &str, title: &str) -> DbResult<()> {
        sqlx::query("UPDATE conversations SET session_timeline = $2 WHERE id = $1")
            .bind(conv_id)
            .bind(title)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn find_latest_jarvis_conversation(&self) -> DbResult<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM conversations \
             WHERE source = 'jarvis_ui' AND status = 'active' \
             ORDER BY started_at DESC LIMIT 1"
        )
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.0))
    }

    async fn router_chat_list(&self, limit: i64) -> DbResult<Vec<serde_json::Value>> {
        let rows = sqlx::query(
            "SELECT c.id, c.task_id, c.model, c.message_count, c.started_at, c.status,
                    COALESCE((SELECT SUM(LENGTH(m.content)) FROM conversation_messages m WHERE m.session_id = c.id), 0) AS total_chars
             FROM conversations c
             WHERE c.chat_type = 'router_chat'
             ORDER BY c.started_at DESC
             LIMIT $1"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(|r| {
            use sqlx::Row;
            let total_chars: i64 = r.get("total_chars");
            serde_json::json!({
                "id": r.get::<String, _>("id"),
                "task_id": r.get::<Option<String>, _>("task_id"),
                "model": r.get::<Option<String>, _>("model"),
                "message_count": r.get::<i64, _>("message_count"),
                "started_at": r.get::<String, _>("started_at"),
                "status": r.get::<String, _>("status"),
                "total_chars": total_chars,
                "estimated_tokens": total_chars / 4,
            })
        }).collect())
    }

    async fn router_chat_stats(&self) -> DbResult<serde_json::Value> {
        let row = sqlx::query(
            "SELECT COUNT(DISTINCT c.id),
                    COUNT(m.id),
                    COALESCE(SUM(LENGTH(m.content)), 0)
             FROM conversations c
             LEFT JOIN conversation_messages m ON m.session_id = c.id
             WHERE c.chat_type = 'router_chat'"
        )
        .fetch_one(&self.pool)
        .await?;

        use sqlx::Row;
        let total_convs: i64 = row.get(0);
        let total_msgs: i64 = row.get(1);
        let total_chars: i64 = row.get(2);

        // By model
        let model_rows = sqlx::query(
            "SELECT COALESCE(c.model, 'unknown'), COUNT(DISTINCT c.id), COUNT(m.id), COALESCE(SUM(LENGTH(m.content)), 0)
             FROM conversations c
             LEFT JOIN conversation_messages m ON m.session_id = c.id
             WHERE c.chat_type = 'router_chat'
             GROUP BY c.model"
        )
        .fetch_all(&self.pool)
        .await?;

        let by_model: Vec<serde_json::Value> = model_rows.iter().map(|r| {
            let chars: i64 = r.get(3);
            serde_json::json!({
                "model": r.get::<String, _>(0),
                "conversations": r.get::<i64, _>(1),
                "messages": r.get::<i64, _>(2),
                "total_chars": chars,
                "estimated_tokens": chars / 4,
            })
        }).collect();

        // By day (last 30 days)
        let day_rows = sqlx::query(
            "SELECT DATE(m.timestamp) AS day, COUNT(*) AS msg_count, SUM(LENGTH(m.content)) AS chars
             FROM conversation_messages m
             INNER JOIN conversations c ON c.id = m.session_id
             WHERE c.chat_type = 'router_chat' AND m.timestamp >= (NOW() - INTERVAL '30 days')
             GROUP BY day ORDER BY day DESC"
        )
        .fetch_all(&self.pool)
        .await?;

        let by_day: Vec<serde_json::Value> = day_rows.iter().map(|r| {
            use sqlx::Row;
            let chars: i64 = r.get("chars");
            serde_json::json!({
                "day": r.get::<String, _>("day"),
                "messages": r.get::<i64, _>("msg_count"),
                "total_chars": chars,
                "estimated_tokens": chars / 4,
            })
        }).collect();

        Ok(serde_json::json!({
            "total_conversations": total_convs,
            "total_messages": total_msgs,
            "total_chars": total_chars,
            "estimated_tokens": total_chars / 4,
            "by_model": by_model,
            "by_day": by_day,
        }))
    }

    async fn router_chat_clear(&self, conversation_id: &str, count: Option<i64>) -> DbResult<(i64, i64)> {
        // Verify it's a router_chat conversation
        let (is_router,): (bool,) = sqlx::query_as(
            "SELECT COUNT(*) > 0 FROM conversations WHERE id = $1 AND chat_type = 'router_chat'"
        )
        .bind(conversation_id)
        .fetch_one(&self.pool)
        .await?;
        if !is_router {
            return Ok((0, 0));
        }

        // Use a transaction
        let mut tx = self.pool.begin().await?;

        if let Some(n) = count {
            // Archive last N messages
            sqlx::query(
                "INSERT INTO router_chat_archive (original_id, session_id, role, content, timestamp, archive_reason)
                 SELECT id, session_id, role, content, timestamp, 'clear'
                 FROM conversation_messages
                 WHERE id IN (SELECT id FROM conversation_messages WHERE session_id = $1 ORDER BY id DESC LIMIT $2)"
            )
            .bind(conversation_id)
            .bind(n)
            .execute(&mut *tx)
            .await?;

            // Delete
            let del = sqlx::query(
                "DELETE FROM conversation_messages
                 WHERE id IN (SELECT id FROM conversation_messages WHERE session_id = $1 ORDER BY id DESC LIMIT $2)"
            )
            .bind(conversation_id)
            .bind(n)
            .execute(&mut *tx)
            .await?;
            let deleted = del.rows_affected() as i64;

            // Update message_count
            let (remaining,): (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM conversation_messages WHERE session_id = $1"
            )
            .bind(conversation_id)
            .fetch_one(&mut *tx)
            .await?;

            sqlx::query("UPDATE conversations SET message_count = $1 WHERE id = $2")
                .bind(remaining)
                .bind(conversation_id)
                .execute(&mut *tx)
                .await?;

            tx.commit().await?;
            Ok((deleted, remaining))
        } else {
            // Archive all
            sqlx::query(
                "INSERT INTO router_chat_archive (original_id, session_id, role, content, timestamp, archive_reason)
                 SELECT id, session_id, role, content, timestamp, 'clear'
                 FROM conversation_messages WHERE session_id = $1"
            )
            .bind(conversation_id)
            .execute(&mut *tx)
            .await?;

            let del = sqlx::query(
                "DELETE FROM conversation_messages WHERE session_id = $1"
            )
            .bind(conversation_id)
            .execute(&mut *tx)
            .await?;
            let deleted = del.rows_affected() as i64;

            sqlx::query("UPDATE conversations SET message_count = 0 WHERE id = $1")
                .bind(conversation_id)
                .execute(&mut *tx)
                .await?;

            tx.commit().await?;
            Ok((deleted, 0))
        }
    }

    async fn router_chat_clear_by_task(&self, task_id: &str, count: Option<i64>) -> DbResult<i64> {
        // Get all conversation IDs for this task
        let conv_ids: Vec<(String,)> = sqlx::query_as(
            "SELECT id FROM conversations WHERE task_id = $1 AND chat_type = 'router_chat'"
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await?;

        let mut total = 0i64;
        for (cid,) in &conv_ids {
            let (archived, _) = self.router_chat_clear(cid, count).await?;
            total += archived;
        }
        Ok(total)
    }

    async fn router_chat_delete_message(&self, message_id: i64) -> DbResult<Option<String>> {
        // Verify it belongs to a router_chat conversation
        let session_id: Option<(String,)> = sqlx::query_as(
            "SELECT cm.session_id FROM conversation_messages cm
             JOIN conversations c ON c.id = cm.session_id
             WHERE cm.id = $1 AND c.chat_type = 'router_chat'"
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;

        let session_id = match session_id {
            Some((sid,)) => sid,
            None => return Ok(None),
        };

        let mut tx = self.pool.begin().await?;

        // Archive
        sqlx::query(
            "INSERT INTO router_chat_archive (original_id, session_id, role, content, timestamp, archive_reason)
             SELECT id, session_id, role, content, timestamp, 'delete_message'
             FROM conversation_messages WHERE id = $1"
        )
        .bind(message_id)
        .execute(&mut *tx)
        .await?;

        // Delete
        sqlx::query("DELETE FROM conversation_messages WHERE id = $1")
            .bind(message_id)
            .execute(&mut *tx)
            .await?;

        // Update message_count
        sqlx::query(
            "UPDATE conversations SET message_count = (SELECT COUNT(*) FROM conversation_messages WHERE session_id = $1) WHERE id = $1"
        )
        .bind(&session_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(Some(session_id))
    }

    async fn router_chat_delete(&self, conversation_id: &str) -> DbResult<(i64, i64)> {
        let mut tx = self.pool.begin().await?;

        // Archive messages first
        sqlx::query(
            "INSERT INTO router_chat_archive (original_id, session_id, role, content, timestamp, archive_reason)
             SELECT id, session_id, role, content, timestamp, 'delete'
             FROM conversation_messages WHERE session_id = $1
             AND EXISTS (SELECT 1 FROM conversations WHERE id = $1 AND chat_type = 'router_chat')"
        )
        .bind(conversation_id)
        .execute(&mut *tx)
        .await?;

        let msg_del = sqlx::query(
            "DELETE FROM conversation_messages WHERE session_id = $1
             AND EXISTS (SELECT 1 FROM conversations WHERE id = $1 AND chat_type = 'router_chat')"
        )
        .bind(conversation_id)
        .execute(&mut *tx)
        .await?;
        let msg_deleted = msg_del.rows_affected() as i64;

        let conv_del = sqlx::query(
            "DELETE FROM conversations WHERE id = $1 AND chat_type = 'router_chat'"
        )
        .bind(conversation_id)
        .execute(&mut *tx)
        .await?;
        let conv_deleted = conv_del.rows_affected() as i64;

        tx.commit().await?;
        Ok((conv_deleted, msg_deleted))
    }

    async fn router_chat_delete_by_task(&self, task_id: &str) -> DbResult<(i64, i64)> {
        let mut tx = self.pool.begin().await?;

        // Archive all messages
        sqlx::query(
            "INSERT INTO router_chat_archive (original_id, session_id, role, content, timestamp, archive_reason)
             SELECT m.id, m.session_id, m.role, m.content, m.timestamp, 'delete'
             FROM conversation_messages m
             INNER JOIN conversations c ON c.id = m.session_id
             WHERE c.task_id = $1 AND c.chat_type = 'router_chat'"
        )
        .bind(task_id)
        .execute(&mut *tx)
        .await?;

        let msg_del = sqlx::query(
            "DELETE FROM conversation_messages WHERE session_id IN
             (SELECT id FROM conversations WHERE task_id = $1 AND chat_type = 'router_chat')"
        )
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
        let msg_deleted = msg_del.rows_affected() as i64;

        let conv_del = sqlx::query(
            "DELETE FROM conversations WHERE task_id = $1 AND chat_type = 'router_chat'"
        )
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
        let conv_deleted = conv_del.rows_affected() as i64;

        tx.commit().await?;
        Ok((conv_deleted, msg_deleted))
    }

    async fn router_chat_restore(&self, conversation_id: &str) -> DbResult<i64> {
        let mut tx = self.pool.begin().await?;

        let ins = sqlx::query(
            "INSERT INTO conversation_messages (session_id, role, content, timestamp)
             SELECT session_id, role, content, timestamp
             FROM router_chat_archive WHERE session_id = $1
             ORDER BY original_id ASC"
        )
        .bind(conversation_id)
        .execute(&mut *tx)
        .await?;
        let restored = ins.rows_affected() as i64;

        if restored > 0 {
            sqlx::query("DELETE FROM router_chat_archive WHERE session_id = $1")
                .bind(conversation_id)
                .execute(&mut *tx)
                .await?;

            sqlx::query(
                "UPDATE conversations SET message_count = (SELECT COUNT(*) FROM conversation_messages WHERE session_id = $1) WHERE id = $1"
            )
            .bind(conversation_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(restored)
    }

    // ── watcher_cursors ──────────────────────────────────────────

    async fn load_watcher_cursors(&self) -> DbResult<HashMap<String, u64>> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT file_path, byte_offset FROM watcher_cursors"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(p, o)| (p, o as u64)).collect())
    }

    async fn upsert_watcher_cursors_batch(&self, cursors: &HashMap<String, u64>) -> DbResult<()> {
        for (path, offset) in cursors {
            sqlx::query(
                "INSERT INTO watcher_cursors (file_path, byte_offset, updated_at)
                 VALUES ($1, $2, to_char(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS'))
                 ON CONFLICT (file_path) DO UPDATE SET byte_offset = $2,
                    updated_at = to_char(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS')"
            )
            .bind(path)
            .bind(*offset as i64)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn delete_watcher_cursor(&self, file_path: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM watcher_cursors WHERE file_path = $1")
            .bind(file_path)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -- reconcile_watermarks --

    async fn get_reconcile_watermark(&self, path: &str) -> DbResult<Option<i64>> {
        let row = sqlx::query("SELECT last_reconciled_size FROM reconcile_watermarks WHERE jsonl_path = $1")
            .bind(path)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get::<i64, _>("last_reconciled_size")))
    }

    async fn upsert_reconcile_watermark(&self, path: &str, size: i64) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO reconcile_watermarks (jsonl_path, last_reconciled_size, last_reconciled_at)
             VALUES ($1, $2, NOW())
             ON CONFLICT (jsonl_path) DO UPDATE SET last_reconciled_size = $2, last_reconciled_at = NOW()"
        )
            .bind(path)
            .bind(size)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_all_reconcile_watermarks(&self) -> DbResult<HashMap<String, i64>> {
        let rows = sqlx::query("SELECT jsonl_path, last_reconciled_size FROM reconcile_watermarks")
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for row in rows {
            map.insert(
                row.get::<String, _>("jsonl_path"),
                row.get::<i64, _>("last_reconciled_size"),
            );
        }
        Ok(map)
    }

    // ── gemini_cli_watermarks ──────────────────────────────────

    async fn load_gemini_cursors(&self) -> DbResult<HashMap<String, i64>> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT session_file, last_msg_count FROM gemini_cli_watermarks"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().collect())
    }

    async fn save_gemini_cursor(&self, file_path: &str, session_id: &str, msg_count: i64) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO gemini_cli_watermarks (session_file, session_id, last_msg_count, last_reconciled_at)
             VALUES ($1, $2, $3, NOW())
             ON CONFLICT (session_file) DO UPDATE SET
                last_msg_count = $3,
                session_id = $2,
                last_reconciled_at = NOW()"
        )
        .bind(file_path)
        .bind(session_id)
        .bind(msg_count)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
