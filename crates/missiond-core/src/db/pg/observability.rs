//! ObservabilityStore — PostgreSQL implementation.
//!
//! Covers: Gemini log, incidents, token ledger, labels,
//!         conversation-specific backfill cursors, vision/translation,
//!         and router chat sessions.
//!
//! Note: watermarks + backfill progress + daemon_state migrated to InfraStore
//!       in Stage 2D. See pg/infra.rs.

use super::PgMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::{ConversationStore, ObservabilityStore};
use crate::types::*;
use async_trait::async_trait;
use sqlx::{Column, Row};
use std::collections::HashMap;

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
             VALUES ($1, $2, $3, 'pending', $4, $5, 0, 0, 0, 0, 'pending', $6)
             ON CONFLICT (id) DO NOTHING"
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

    async fn gemini_log_get_content(
        &self,
        request_id: &str,
    ) -> DbResult<Option<serde_json::Value>> {
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

        Ok(rows
            .iter()
            .map(|r| {
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
            })
            .collect())
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

        let by_caller: Vec<serde_json::Value> = caller_rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "caller": r.get::<String, _>(0),
                    "count": r.get::<i64, _>(1),
                    "avg_duration_ms": r.get::<f64, _>(2) as i64,
                    "errors": r.get::<i64, _>(3),
                })
            })
            .collect();

        // Slow requests (>30s)
        let slow_rows = sqlx::query(
            "SELECT id, caller, session_id, model, duration_ms, queue_wait_ms, prompt_chars, response_chars, status, created_at
             FROM gemini_requests WHERE duration_ms > 30000
               AND created_at >= to_char(NOW() AT TIME ZONE 'UTC' - INTERVAL '7 days', 'YYYY-MM-DD HH24:MI:SS')
             ORDER BY duration_ms DESC LIMIT 10"
        )
        .fetch_all(&self.pool)
        .await?;

        let slow: Vec<serde_json::Value> = slow_rows
            .iter()
            .map(|r| {
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
            })
            .collect();

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
            "SELECT file_uri FROM gemini_file_uploads WHERE file_hash = $1 AND expires_at > $2",
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
                expires_at = EXCLUDED.expires_at",
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
               AND EXTRACT(EPOCH FROM ($2::timestamp - created_at::timestamp)) < $3",
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

        Ok(rows
            .iter()
            .map(|r| {
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
            })
            .collect())
    }

    async fn get_incident_by_id(&self, id: &str) -> DbResult<Option<IncidentRow>> {
        let row = sqlx::query(
            "SELECT id, severity, source, title, description, server_id, board_task_id, dedupe_key, created_at
             FROM incidents WHERE id = $1 LIMIT 1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
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
        }))
    }

    async fn update_incident_board_task_id(&self, id: &str, board_task_id: &str) -> DbResult<bool> {
        let result = sqlx::query("UPDATE incidents SET board_task_id = $2 WHERE id = $1")
            .bind(id)
            .bind(board_task_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
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
             ON CONFLICT (message_id) WHERE message_id IS NOT NULL DO NOTHING",
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
             FROM token_usage_ledger WHERE 1=1",
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
        let rows = if let Some(cid) = conversation_id {
            rows.bind(cid)
        } else {
            rows
        };
        let rows = if let Some(sid) = slot_id {
            rows.bind(sid)
        } else {
            rows
        };
        let rows = if let Some(s) = since {
            rows.bind(s)
        } else {
            rows
        };
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

    // ── labels ────────────────────────────────────────────────────

    async fn label_set(
        &self,
        message_id: i64,
        label: &str,
        value: &str,
        source: &str,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO message_labels (message_id, label, value, source)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (message_id, label) DO UPDATE SET
                value = EXCLUDED.value,
                source = EXCLUDED.source",
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
                    source = EXCLUDED.source",
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
            "SELECT label, COALESCE(value, ''), source FROM message_labels WHERE message_id = $1",
        )
        .bind(message_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn label_get_batch(
        &self,
        message_ids: &[i64],
    ) -> DbResult<HashMap<i64, Vec<(String, String)>>> {
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

    async fn label_find_messages(
        &self,
        label: &str,
        value: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<i64>> {
        if let Some(v) = value {
            let rows: Vec<(i64,)> = sqlx::query_as(
                "SELECT message_id FROM message_labels WHERE label = $1 AND value = $2 LIMIT $3",
            )
            .bind(label)
            .bind(v)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
            Ok(rows.into_iter().map(|r| r.0).collect())
        } else {
            let rows: Vec<(i64,)> =
                sqlx::query_as("SELECT message_id FROM message_labels WHERE label = $1 LIMIT $2")
                    .bind(label)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await?;
            Ok(rows.into_iter().map(|r| r.0).collect())
        }
    }

    async fn message_label_evidence_upsert_batch(
        &self,
        evidence: &[MessageLabelEvidenceInput],
    ) -> DbResult<usize> {
        if evidence.is_empty() {
            return Ok(0);
        }

        let mut changed = 0usize;
        for item in evidence {
            let result = sqlx::query(
                "INSERT INTO message_label_evidence
                    (message_id, label, value, source, rule_id, rule_version, confidence, priority, reason, evidence)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                 ON CONFLICT (message_id, label, value, source, rule_id, rule_version)
                 DO UPDATE SET
                    confidence = EXCLUDED.confidence,
                    priority = EXCLUDED.priority,
                    reason = EXCLUDED.reason,
                    evidence = EXCLUDED.evidence,
                    updated_at = (NOW() AT TIME ZONE 'UTC')::TEXT",
            )
            .bind(item.message_id)
            .bind(&item.label)
            .bind(&item.value)
            .bind(&item.source)
            .bind(&item.rule_id)
            .bind(&item.rule_version)
            .bind(item.confidence)
            .bind(item.priority)
            .bind(&item.reason)
            .bind(&item.evidence)
            .execute(&self.pool)
            .await?;
            changed += result.rows_affected() as usize;
        }
        Ok(changed)
    }

    async fn message_label_projection_refresh(&self, message_ids: &[i64]) -> DbResult<usize> {
        if message_ids.is_empty() {
            return Ok(0);
        }

        let result = sqlx::query(
            "WITH ranked AS (
                SELECT DISTINCT ON (message_id, label)
                    message_id, label, value, source
                FROM message_label_evidence
                WHERE message_id = ANY($1)
                ORDER BY
                    message_id,
                    label,
                    priority DESC,
                    confidence DESC,
                    updated_at DESC,
                    source ASC,
                    rule_id ASC,
                    value ASC
             )
             INSERT INTO message_labels (message_id, label, value, source)
             SELECT message_id, label, value, source FROM ranked
             ON CONFLICT (message_id, label) DO UPDATE SET
                value = EXCLUDED.value,
                source = EXCLUDED.source",
        )
        .bind(message_ids)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn message_labeler_pending_sessions(
        &self,
        consumer: &str,
        source: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "WITH session_max AS (
                SELECT session_id, MAX(id) AS max_message_id, COUNT(*) AS message_count
                FROM conversation_messages
                GROUP BY session_id
             )
             SELECT c.id
             FROM conversations c
             JOIN session_max sm ON sm.session_id = c.id
             LEFT JOIN consumer_watermarks w
               ON w.consumer_name = $1 AND w.session_id = c.id
             WHERE sm.message_count > 0
               AND ($2::TEXT IS NULL OR c.source = $2)
               AND COALESCE(w.last_processed_msg_id, 0) < sm.max_message_id
             ORDER BY COALESCE(c.updated_at, c.started_at) DESC NULLS LAST, c.id ASC
             LIMIT $3",
        )
        .bind(consumer)
        .bind(source)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn message_labeler_audit(
        &self,
        consumer: &str,
        source: Option<&str>,
    ) -> DbResult<serde_json::Value> {
        let total_messages: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM conversation_messages m
             JOIN conversations c ON c.id = m.session_id
             WHERE ($1::TEXT IS NULL OR c.source = $1)",
        )
        .bind(source)
        .fetch_one(&self.pool)
        .await?;

        let evidence_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM message_label_evidence e
             JOIN conversation_messages m ON m.id = e.message_id
             JOIN conversations c ON c.id = m.session_id
             WHERE ($1::TEXT IS NULL OR c.source = $1)",
        )
        .bind(source)
        .fetch_one(&self.pool)
        .await?;

        let projected_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM message_labels ml
             JOIN conversation_messages m ON m.id = ml.message_id
             JOIN conversations c ON c.id = m.session_id
             WHERE ml.source = 'message_labeler'
               AND ($1::TEXT IS NULL OR c.source = $1)",
        )
        .bind(source)
        .fetch_one(&self.pool)
        .await?;

        let pending_sessions_count: i64 = sqlx::query_scalar(
            "WITH session_max AS (
                SELECT session_id, MAX(id) AS max_message_id, COUNT(*) AS message_count
                FROM conversation_messages
                GROUP BY session_id
             )
             SELECT COUNT(*)
             FROM conversations c
             JOIN session_max sm ON sm.session_id = c.id
             LEFT JOIN consumer_watermarks w
               ON w.consumer_name = $1 AND w.session_id = c.id
             WHERE sm.message_count > 0
               AND ($2::TEXT IS NULL OR c.source = $2)
               AND COALESCE(w.last_processed_msg_id, 0) < sm.max_message_id",
        )
        .bind(consumer)
        .bind(source)
        .fetch_one(&self.pool)
        .await?;

        let watermarked_sessions: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM consumer_watermarks w
             JOIN conversations c ON c.id = w.session_id
             WHERE w.consumer_name = $1
               AND ($2::TEXT IS NULL OR c.source = $2)",
        )
        .bind(consumer)
        .bind(source)
        .fetch_one(&self.pool)
        .await?;

        let label_rows = sqlx::query(
            "SELECT e.label, e.value, COUNT(*) AS count
             FROM message_label_evidence e
             JOIN conversation_messages m ON m.id = e.message_id
             JOIN conversations c ON c.id = m.session_id
             WHERE ($1::TEXT IS NULL OR c.source = $1)
             GROUP BY e.label, e.value
             ORDER BY count DESC, e.label ASC, e.value ASC
             LIMIT 50",
        )
        .bind(source)
        .fetch_all(&self.pool)
        .await?;
        let labels = label_rows
            .into_iter()
            .map(|row| {
                serde_json::json!({
                    "label": row.get::<String, _>("label"),
                    "value": row.get::<String, _>("value"),
                    "count": row.get::<i64, _>("count"),
                })
            })
            .collect::<Vec<_>>();

        Ok(serde_json::json!({
            "consumer": consumer,
            "source": source,
            "totalMessages": total_messages,
            "evidenceRows": evidence_rows,
            "projectedRows": projected_rows,
            "watermarkedSessions": watermarked_sessions,
            "pendingSessions": pending_sessions_count,
            "labels": labels,
        }))
    }

    // ── conversation labels (EAV) ──────────────────────────────────

    async fn conversation_label_set(
        &self,
        session_id: &str,
        label: &str,
        value: &str,
        source: &str,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO conversation_labels (session_id, label, value, source)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (session_id, label) DO UPDATE SET
                value = EXCLUDED.value, source = EXCLUDED.source",
        )
        .bind(session_id)
        .bind(label)
        .bind(value)
        .bind(source)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn conversation_label_delete(&self, session_id: &str, label: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM conversation_labels WHERE session_id = $1 AND label = $2")
            .bind(session_id)
            .bind(label)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn conversation_label_get(&self, session_id: &str) -> DbResult<Vec<(String, String)>> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT label, COALESCE(value, '') FROM conversation_labels WHERE session_id = $1",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn conversation_label_get_batch(
        &self,
        session_ids: &[&str],
    ) -> DbResult<HashMap<String, Vec<(String, String)>>> {
        if session_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query(
            "SELECT session_id, label, COALESCE(value, '') FROM conversation_labels WHERE session_id = ANY($1)"
        ).bind(session_ids).fetch_all(&self.pool).await?;
        let mut result: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for r in &rows {
            use sqlx::Row;
            let sid: String = r.get("session_id");
            let label: String = r.get("label");
            let value: String = r.get(2);
            result.entry(sid).or_default().push((label, value));
        }
        Ok(result)
    }

    async fn conversation_label_find(
        &self,
        label: &str,
        value: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<String>> {
        if let Some(v) = value {
            let rows: Vec<(String,)> = sqlx::query_as(
                "SELECT session_id FROM conversation_labels WHERE label = $1 AND value = $2 LIMIT $3"
            ).bind(label).bind(v).bind(limit).fetch_all(&self.pool).await?;
            Ok(rows.into_iter().map(|r| r.0).collect())
        } else {
            let rows: Vec<(String,)> = sqlx::query_as(
                "SELECT session_id FROM conversation_labels WHERE label = $1 LIMIT $2",
            )
            .bind(label)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
            Ok(rows.into_iter().map(|r| r.0).collect())
        }
    }

    // ── conversation-specific backfill cursors (stay in ObservabilityStore) ──

    async fn conversations_missing_summary_cursor(
        &self,
        cursor: i64,
        limit: i64,
    ) -> DbResult<Vec<(i64, String)>> {
        // PG: conversations.id is TEXT. Use ROW_NUMBER() as a stable cursor surrogate
        // mirroring stable insertion-order pagination.
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT rn, cid FROM (
                SELECT ROW_NUMBER() OVER (ORDER BY started_at, id) AS rn, id AS cid
                FROM conversations
                WHERE (llm_summary IS NULL OR llm_summary = '[timeout]')
                  AND status = 'completed' AND message_count >= 6
                  AND conversation_type IN ('user', 'worker')
            ) sub WHERE rn > $1
            ORDER BY rn ASC LIMIT $2",
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
            ORDER BY rn ASC LIMIT $3",
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
               AND conversation_type IN ('user', 'worker')",
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
               )",
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
            "SELECT role, content FROM conversation_messages WHERE session_id = $1 ORDER BY id ASC",
        )
        .bind(conv_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                let role: String = r.get("role");
                let content: String = r.get("content");
                serde_json::json!({"role": role, "content": content})
            })
            .collect())
    }

    async fn router_chat_get_summary(&self, conv_id: &str) -> DbResult<(Option<String>, i64)> {
        let row: (Option<String>, i64) = sqlx::query_as(
            "SELECT rolling_summary, COALESCE(last_summarized_msg_id, 0)
             FROM conversations WHERE id = $1",
        )
        .bind(conv_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    async fn router_chat_load_active_history(
        &self,
        conv_id: &str,
        after_id: i64,
    ) -> DbResult<Vec<serde_json::Value>> {
        let rows = sqlx::query(
            "SELECT role, content FROM conversation_messages
             WHERE session_id = $1 AND id > $2
             ORDER BY id ASC",
        )
        .bind(conv_id)
        .bind(after_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                let role: String = r.get("role");
                let content: String = r.get("content");
                serde_json::json!({"role": role, "content": content})
            })
            .collect())
    }

    async fn router_chat_unsummarized_count(&self, conv_id: &str, after_id: i64) -> DbResult<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversation_messages WHERE session_id = $1 AND id > $2",
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
             ORDER BY id ASC LIMIT $3",
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
             WHERE id = $3 AND COALESCE(last_summarized_msg_id, 0) = $4",
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
                 VALUES ($1, $2, $3, $4)",
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
                "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = $1 AND source = 'jarvis_ui')",
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
             VALUES ($1, 'jarvis_ui', 'claude-code', 'jarvis', 0, $2, 'active', 'jarvis')"
        )
        .bind(&id)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    async fn jarvis_get_or_create_scoped(
        &self,
        conversation_id: Option<&str>,
        user_id: Option<&str>,
        tenant_id: Option<&str>,
        application_id: Option<&str>,
        channel: Option<&str>,
        topic_id: Option<&str>,
        topic_label: Option<&str>,
    ) -> DbResult<String> {
        let channel = channel.unwrap_or("jarvis_sse");

        if let Some(id) = conversation_id.map(str::trim).filter(|id| !id.is_empty()) {
            let row: Option<(String,)> = sqlx::query_as(
                "SELECT id FROM conversations
                 WHERE id = $1
                   AND source = 'jarvis_ui'
                   AND ($2::text IS NULL OR user_id IS NULL OR user_id = $2)
                   AND ($3::text IS NULL OR tenant_id IS NULL OR tenant_id = $3)
                   AND ($4::text IS NULL OR application_id IS NULL OR application_id = $4)
                   AND ($5::text IS NULL OR channel IS NULL OR channel = 'cli' OR channel = $5)",
            )
            .bind(id)
            .bind(user_id)
            .bind(tenant_id)
            .bind(application_id)
            .bind(Some(channel))
            .fetch_optional(&self.pool)
            .await?;
            if row.is_some() {
                sqlx::query(
                    "UPDATE conversations SET
                        user_id = COALESCE(user_id, $2),
                        tenant_id = COALESCE(tenant_id, $3),
                        application_id = COALESCE(application_id, $4),
                        channel = CASE
                            WHEN channel IS NULL OR channel = '' OR channel = 'cli' THEN COALESCE($5, channel, 'jarvis_sse')
                            ELSE channel
                        END,
                        topic_id = COALESCE(topic_id, $6),
                        topic_label = COALESCE(topic_label, $7)
                     WHERE id = $1",
                )
                .bind(id)
                .bind(user_id)
                .bind(tenant_id)
                .bind(application_id)
                .bind(Some(channel))
                .bind(topic_id)
                .bind(topic_label)
                .execute(&self.pool)
                .await?;
                return Ok(id.to_string());
            }
        }

        if let Some(tid) = topic_id {
            if let Some(conv) = ConversationStore::resolve_active_session(
                self,
                user_id,
                tenant_id,
                application_id,
                Some(channel),
                Some(tid),
            )
            .await?
            {
                return Ok(conv.id);
            }
        }

        if let Some(conv) = ConversationStore::resolve_active_session(
            self,
            user_id,
            tenant_id,
            application_id,
            Some(channel),
            None,
        )
        .await?
        {
            if topic_id.is_some() || topic_label.is_some() {
                sqlx::query(
                    "UPDATE conversations SET
                        topic_id = COALESCE(topic_id, $2),
                        topic_label = COALESCE(topic_label, $3)
                     WHERE id = $1",
                )
                .bind(&conv.id)
                .bind(topic_id)
                .bind(topic_label)
                .execute(&self.pool)
                .await?;
            }
            return Ok(conv.id);
        }

        let id = format!("jarvis-{}", uuid::Uuid::new_v4().simple());
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO conversations (
                id, source, model, chat_type, message_count, started_at, status,
                conversation_type, user_id, tenant_id, application_id, channel,
                topic_id, topic_label
             )
             VALUES (
                $1, 'jarvis_ui', 'claude-code', 'jarvis', 0, $2, 'active',
                'jarvis', $3, $4, $5, $6, $7, $8
             )",
        )
        .bind(&id)
        .bind(&now)
        .bind(user_id)
        .bind(tenant_id)
        .bind(application_id)
        .bind(channel)
        .bind(topic_id)
        .bind(topic_label)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    async fn jarvis_list_scoped(
        &self,
        user_id: Option<&str>,
        tenant_id: Option<&str>,
        application_id: Option<&str>,
        channel: Option<&str>,
        include_legacy_unscoped: bool,
        limit: i64,
    ) -> DbResult<Vec<serde_json::Value>> {
        let channel = channel.unwrap_or("jarvis");
        let limit = limit.clamp(1, 100);
        let rows = sqlx::query(
            "SELECT c.id, c.model, c.message_count::bigint AS message_count, c.started_at, c.status,
                    c.user_id, c.tenant_id, c.application_id, c.channel,
                    c.topic_id, c.topic_label, c.session_timeline,
                    lm.id AS last_message_id,
                    lm.role AS last_message_role,
                    lm.content AS last_message_content,
                    lm.timestamp AS last_message_timestamp,
                    COALESCE(
                        (SELECT MAX(m.timestamp) FROM conversation_messages m WHERE m.session_id = c.id),
                        c.started_at,
                        ''
                    ) AS updated_at
             FROM conversations c
             LEFT JOIN LATERAL (
                SELECT m.id, m.role, m.content, m.timestamp
                FROM conversation_messages m
                WHERE m.session_id = c.id
                  AND NOT (
                    m.role = 'assistant'
                    AND m.content LIKE '%\"missiond.jarvis-pending-confirmation.v1\"%'
                  )
                ORDER BY m.id DESC
                LIMIT 1
             ) lm ON TRUE
             WHERE c.source = 'jarvis_ui'
               AND (
                    (
                        ($1::text IS NULL OR c.user_id = $1)
                        AND ($2::text IS NULL OR c.tenant_id = $2)
                        AND ($3::text IS NULL OR c.application_id = $3)
                        AND (
                            $4::text IS NULL
                            OR c.channel IS NULL
                            OR c.channel = $4
                            OR (
                                $4::text IN ('jarvis', 'jarvis_sse', 'jarvis_mobile')
                                AND c.channel IN ('jarvis', 'jarvis_sse', 'jarvis_mobile')
                            )
                        )
                    )
                    OR (
                        $5::bool
                        AND c.user_id IS NULL
                        AND c.tenant_id IS NULL
                        AND c.application_id IS NULL
                    )
               )
             ORDER BY updated_at DESC, c.id DESC
             LIMIT $6",
        )
        .bind(user_id)
        .bind(tenant_id)
        .bind(application_id)
        .bind(Some(channel))
        .bind(include_legacy_unscoped)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                let id: String = r.get("id");
                let topic_label: Option<String> = r.get("topic_label");
                let timeline_title: Option<String> = r.get("session_timeline");
                let content: Option<String> = r.get("last_message_content");
                let preview = content
                    .as_deref()
                    .map(|value| value.chars().take(240).collect::<String>());
                let row_user_id: Option<String> = r.get("user_id");
                let row_tenant_id: Option<String> = r.get("tenant_id");
                let row_application_id: Option<String> = r.get("application_id");
                let scope_state = if row_user_id.is_none()
                    && row_tenant_id.is_none()
                    && row_application_id.is_none()
                {
                    "legacy_unscoped"
                } else {
                    "scoped"
                };
                serde_json::json!({
                    "id": id,
                    "model": r.get::<Option<String>, _>("model"),
                    "message_count": r.get::<Option<i64>, _>("message_count").unwrap_or(0),
                    "started_at": r.get::<Option<String>, _>("started_at"),
                    "updated_at": r.get::<Option<String>, _>("updated_at"),
                    "status": r.get::<Option<String>, _>("status"),
                    "title": topic_label
                        .clone()
                        .or_else(|| timeline_title.filter(|value| !value.trim().is_empty()))
                        .or_else(|| preview.clone()),
                    "topic_id": r.get::<Option<String>, _>("topic_id"),
                    "topic_label": topic_label,
                    "scope": {
                        "user_id": row_user_id,
                        "tenant_id": row_tenant_id,
                        "application_id": row_application_id,
                        "channel": r.get::<Option<String>, _>("channel"),
                        "state": scope_state,
                    },
                    "last_message": content.map(|content| serde_json::json!({
                        "id": r.get::<Option<i64>, _>("last_message_id"),
                        "role": r.get::<Option<String>, _>("last_message_role"),
                        "content": content,
                        "timestamp": r.get::<Option<String>, _>("last_message_timestamp"),
                    })),
                })
            })
            .collect())
    }

    async fn jarvis_history_scoped(
        &self,
        conversation_id: &str,
        user_id: Option<&str>,
        tenant_id: Option<&str>,
        application_id: Option<&str>,
        channel: Option<&str>,
        include_legacy_unscoped: bool,
        tail: i64,
    ) -> DbResult<Option<serde_json::Value>> {
        let channel = channel.unwrap_or("jarvis");
        let tail = tail.clamp(1, 300);
        let row = sqlx::query(
            "SELECT c.id, c.model, c.message_count::bigint AS message_count, c.started_at, c.status,
                    c.user_id, c.tenant_id, c.application_id, c.channel,
                    c.topic_id, c.topic_label, c.session_timeline
             FROM conversations c
             WHERE c.id = $1
               AND c.source = 'jarvis_ui'
               AND (
                    (
                        ($2::text IS NULL OR c.user_id = $2)
                        AND ($3::text IS NULL OR c.tenant_id = $3)
                        AND ($4::text IS NULL OR c.application_id = $4)
                        AND (
                            $5::text IS NULL
                            OR c.channel IS NULL
                            OR c.channel = $5
                            OR (
                                $5::text IN ('jarvis', 'jarvis_sse', 'jarvis_mobile')
                                AND c.channel IN ('jarvis', 'jarvis_sse', 'jarvis_mobile')
                            )
                        )
                    )
                    OR (
                        $6::bool
                        AND c.user_id IS NULL
                        AND c.tenant_id IS NULL
                        AND c.application_id IS NULL
                    )
               )",
        )
        .bind(conversation_id)
        .bind(user_id)
        .bind(tenant_id)
        .bind(application_id)
        .bind(Some(channel))
        .bind(include_legacy_unscoped)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let message_rows = sqlx::query(
            "SELECT id, role, content, timestamp
             FROM (
                SELECT id, role, content, timestamp
                FROM conversation_messages
                WHERE session_id = $1
                  AND NOT (
                    role = 'assistant'
                    AND content LIKE '%\"missiond.jarvis-pending-confirmation.v1\"%'
                  )
                ORDER BY id DESC
                LIMIT $2
             ) m
             ORDER BY id ASC",
        )
        .bind(conversation_id)
        .bind(tail)
        .fetch_all(&self.pool)
        .await?;

        use sqlx::Row;
        let row_user_id: Option<String> = row.get("user_id");
        let row_tenant_id: Option<String> = row.get("tenant_id");
        let row_application_id: Option<String> = row.get("application_id");
        let scope_state =
            if row_user_id.is_none() && row_tenant_id.is_none() && row_application_id.is_none() {
                "legacy_unscoped"
            } else {
                "scoped"
            };
        let topic_label: Option<String> = row.get("topic_label");
        let timeline_title: Option<String> = row.get("session_timeline");
        let messages = message_rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.get::<i64, _>("id"),
                    "role": r.get::<String, _>("role"),
                    "content": r.get::<String, _>("content"),
                    "timestamp": r.get::<Option<String>, _>("timestamp"),
                })
            })
            .collect::<Vec<_>>();
        let title_from_message = messages
            .iter()
            .find(|message| message.get("role").and_then(|value| value.as_str()) == Some("user"))
            .and_then(|message| message.get("content").and_then(|value| value.as_str()))
            .map(|value| value.chars().take(240).collect::<String>());

        Ok(Some(serde_json::json!({
            "conversation": {
                "id": row.get::<String, _>("id"),
                "model": row.get::<Option<String>, _>("model"),
                "message_count": row.get::<Option<i64>, _>("message_count").unwrap_or(0),
                "started_at": row.get::<Option<String>, _>("started_at"),
                "status": row.get::<Option<String>, _>("status"),
                "title": topic_label
                    .clone()
                    .or_else(|| timeline_title.filter(|value| !value.trim().is_empty()))
                    .or(title_from_message),
                "topic_id": row.get::<Option<String>, _>("topic_id"),
                "topic_label": topic_label,
                "scope": {
                    "user_id": row_user_id,
                    "tenant_id": row_tenant_id,
                    "application_id": row_application_id,
                    "channel": row.get::<Option<String>, _>("channel"),
                    "state": scope_state,
                },
            },
            "messages": messages,
        })))
    }

    async fn jarvis_save_exchange(
        &self,
        conv_id: &str,
        user_message: &str,
        assistant_message: &str,
    ) -> DbResult<()> {
        self.router_chat_append_messages(
            conv_id,
            &[
                ("user".to_string(), user_message.to_string()),
                ("assistant".to_string(), assistant_message.to_string()),
            ],
        )
        .await
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
             ORDER BY started_at DESC LIMIT 1",
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

        Ok(rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                let total_chars: i64 = r.get("total_chars");
                serde_json::json!({
                    "id": r.get::<String, _>("id"),
                    "task_id": r.get::<Option<String>, _>("task_id"),
                    "model": r.get::<Option<String>, _>("model"),
                    "message_count": Self::row_message_count(r),
                    "started_at": r.get::<String, _>("started_at"),
                    "status": r.get::<String, _>("status"),
                    "total_chars": total_chars,
                    "estimated_tokens": total_chars / 4,
                })
            })
            .collect())
    }

    async fn router_chat_stats(&self) -> DbResult<serde_json::Value> {
        let row = sqlx::query(
            "SELECT COUNT(DISTINCT c.id),
                    COUNT(m.id),
                    COALESCE(SUM(LENGTH(m.content)), 0)
             FROM conversations c
             LEFT JOIN conversation_messages m ON m.session_id = c.id
             WHERE c.chat_type = 'router_chat'",
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

        let by_model: Vec<serde_json::Value> = model_rows
            .iter()
            .map(|r| {
                let chars: i64 = r.get(3);
                serde_json::json!({
                    "model": r.get::<String, _>(0),
                    "conversations": r.get::<i64, _>(1),
                    "messages": r.get::<i64, _>(2),
                    "total_chars": chars,
                    "estimated_tokens": chars / 4,
                })
            })
            .collect();

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

        let by_day: Vec<serde_json::Value> = day_rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                let chars: i64 = r.get("chars");
                serde_json::json!({
                    "day": r.get::<String, _>("day"),
                    "messages": r.get::<i64, _>("msg_count"),
                    "total_chars": chars,
                    "estimated_tokens": chars / 4,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "total_conversations": total_convs,
            "total_messages": total_msgs,
            "total_chars": total_chars,
            "estimated_tokens": total_chars / 4,
            "by_model": by_model,
            "by_day": by_day,
        }))
    }

    async fn router_chat_clear(
        &self,
        conversation_id: &str,
        count: Option<i64>,
    ) -> DbResult<(i64, i64)> {
        // Verify it's a router_chat conversation
        let (is_router,): (bool,) = sqlx::query_as(
            "SELECT COUNT(*) > 0 FROM conversations WHERE id = $1 AND chat_type = 'router_chat'",
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
            let (remaining,): (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM conversation_messages WHERE session_id = $1")
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

            let del = sqlx::query("DELETE FROM conversation_messages WHERE session_id = $1")
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
            "SELECT id FROM conversations WHERE task_id = $1 AND chat_type = 'router_chat'",
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
             WHERE cm.id = $1 AND c.chat_type = 'router_chat'",
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
             AND EXISTS (SELECT 1 FROM conversations WHERE id = $1 AND chat_type = 'router_chat')",
        )
        .bind(conversation_id)
        .execute(&mut *tx)
        .await?;
        let msg_deleted = msg_del.rows_affected() as i64;

        let conv_del =
            sqlx::query("DELETE FROM conversations WHERE id = $1 AND chat_type = 'router_chat'")
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
             (SELECT id FROM conversations WHERE task_id = $1 AND chat_type = 'router_chat')",
        )
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
        let msg_deleted = msg_del.rows_affected() as i64;

        let conv_del = sqlx::query(
            "DELETE FROM conversations WHERE task_id = $1 AND chat_type = 'router_chat'",
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
             ORDER BY original_id ASC",
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

    // ── from VisionStore v0.4.23 ──────────────────────────────────
    // Source: image_descriptions (system-support) + message_translations (conversation-logs)

    async fn get_image_description(&self, image_hash: &str) -> DbResult<Option<String>> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT description FROM image_descriptions WHERE image_hash = $1")
                .bind(image_hash)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| r.0))
    }

    async fn save_image_description(
        &self,
        image_hash: &str,
        media_type: &str,
        description: &str,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO image_descriptions (image_hash, media_type, description, char_count)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (image_hash) DO UPDATE SET
                media_type = EXCLUDED.media_type,
                description = EXCLUDED.description,
                char_count = EXCLUDED.char_count",
        )
        .bind(image_hash)
        .bind(media_type)
        .bind(description)
        .bind(description.len() as i32)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_message_content(&self, message_id: i64, new_content: &str) -> DbResult<()> {
        sqlx::query("UPDATE conversation_messages SET content = $1 WHERE id = $2")
            .bind(new_content)
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_message_raw_content(&self, message_id: i64) -> DbResult<Option<String>> {
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT raw_content FROM conversation_messages WHERE id = $1")
                .bind(message_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.and_then(|r| r.0))
    }

    async fn find_unprocessed_image_messages(&self, limit: usize) -> DbResult<Vec<(i64, String)>> {
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id, session_id FROM conversation_messages
             WHERE content LIKE '%[图片: %'
               AND raw_content IS NOT NULL
               AND raw_content LIKE '%\"type\":\"image\"%'
             ORDER BY id DESC
             LIMIT $1",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn mark_vision_permanently_failed(&self, message_id: i64) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE conversation_messages
             SET content = REPLACE(content, '[图片: ', '[图片(解析失败): ')
             WHERE id = $1 AND content LIKE '%[图片: %'",
        )
        .bind(message_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn image_description_count(&self) -> DbResult<i64> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM image_descriptions")
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    async fn insert_translation(
        &self,
        message_id: i64,
        translation: &str,
        model: &str,
        duration_ms: u64,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO message_translations (message_id, translation, model, duration_ms)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (message_id) DO UPDATE SET
                translation = EXCLUDED.translation,
                model = EXCLUDED.model,
                duration_ms = EXCLUDED.duration_ms",
        )
        .bind(message_id)
        .bind(translation)
        .bind(model)
        .bind(duration_ms as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_translation(&self, message_id: i64) -> DbResult<Option<(String, String)>> {
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT translation, created_at FROM message_translations WHERE message_id = $1",
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn has_translation(&self, message_id: i64) -> DbResult<bool> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM message_translations WHERE message_id = $1")
                .bind(message_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(count > 0)
    }
}
