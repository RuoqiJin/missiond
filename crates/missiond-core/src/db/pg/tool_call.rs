//! ToolCallStore — PostgreSQL implementation.

use async_trait::async_trait;
use crate::db::error::DbResult;
use crate::db::traits::ToolCallStore;
use crate::types::*;
use super::PgMissionStore;
use std::collections::HashSet;

#[cfg(feature = "postgres")]
#[async_trait]
impl ToolCallStore for PgMissionStore {
    async fn insert_tool_call(&self, tc: &ToolCallRecord) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO conversation_tool_calls (id, session_id, message_id, tool_name, input_summary, raw_input, output_summary, raw_output, status, duration_ms, timestamp)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (id) DO NOTHING"
        )
        .bind(&tc.id)
        .bind(&tc.session_id)
        .bind(tc.message_id)
        .bind(&tc.tool_name)
        .bind(&tc.input_summary)
        .bind(&tc.raw_input)
        .bind(&tc.output_summary)
        .bind(&tc.raw_output)
        .bind(&tc.status)
        .bind(tc.duration_ms)
        .bind(&tc.timestamp)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn insert_tool_calls_batch(&self, calls: &[ToolCallRecord]) -> DbResult<usize> {
        if calls.is_empty() {
            return Ok(0);
        }
        let mut count = 0usize;
        for tc in calls {
            let result = sqlx::query(
                "INSERT INTO conversation_tool_calls (id, session_id, message_id, tool_name, input_summary, raw_input, output_summary, raw_output, status, duration_ms, timestamp)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                 ON CONFLICT (id) DO NOTHING"
            )
            .bind(&tc.id)
            .bind(&tc.session_id)
            .bind(tc.message_id)
            .bind(&tc.tool_name)
            .bind(&tc.input_summary)
            .bind(&tc.raw_input)
            .bind(&tc.output_summary)
            .bind(&tc.raw_output)
            .bind(&tc.status)
            .bind(tc.duration_ms)
            .bind(&tc.timestamp)
            .execute(&self.pool)
            .await?;
            if result.rows_affected() > 0 {
                count += 1;
            }
        }
        Ok(count)
    }

    async fn update_tool_call_output(&self, tool_use_id: &str, output_summary: &str, raw_output: &str, status: &str) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE conversation_tool_calls SET output_summary = $1, raw_output = $2, status = $3 WHERE id = $4"
        )
        .bind(output_summary)
        .bind(raw_output)
        .bind(status)
        .bind(tool_use_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn get_tool_calls_by_session(&self, session_id: &str, tool_filter: Option<&[String]>, limit: i64) -> DbResult<Vec<ToolCallRecord>> {
        if let Some(filter) = tool_filter {
            if filter.is_empty() {
                return Ok(Vec::new());
            }
            // Build dynamic IN clause
            let placeholders: Vec<String> = (0..filter.len()).map(|i| format!("${}", i + 2)).collect();
            let sql = format!(
                "SELECT id, session_id, message_id, tool_name, input_summary, raw_input, output_summary, raw_output, status, duration_ms, timestamp
                 FROM conversation_tool_calls WHERE session_id = $1 AND tool_name IN ({}) ORDER BY id ASC LIMIT ${}",
                placeholders.join(","),
                filter.len() + 2
            );
            // sqlx doesn't support dynamic bind count easily, so use query_as with raw
            let mut query = sqlx::query_as::<_, (String, String, Option<i64>, String, Option<String>, Option<String>, Option<String>, Option<String>, String, Option<i64>, String)>(&sql);
            query = query.bind(session_id);
            for f in filter {
                query = query.bind(f);
            }
            query = query.bind(limit);
            let rows = query.fetch_all(&self.pool).await?;
            Ok(rows.into_iter().map(|r| ToolCallRecord {
                id: r.0, session_id: r.1, message_id: r.2, tool_name: r.3,
                input_summary: r.4, raw_input: r.5, output_summary: r.6, raw_output: r.7,
                status: r.8, duration_ms: r.9, timestamp: r.10,
            }).collect())
        } else {
            let rows: Vec<(String, String, Option<i64>, String, Option<String>, Option<String>, Option<String>, Option<String>, String, Option<i64>, String)> = sqlx::query_as(
                "SELECT id, session_id, message_id, tool_name, input_summary, raw_input, output_summary, raw_output, status, duration_ms, timestamp
                 FROM conversation_tool_calls WHERE session_id = $1 ORDER BY id ASC LIMIT $2"
            )
            .bind(session_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
            Ok(rows.into_iter().map(|r| ToolCallRecord {
                id: r.0, session_id: r.1, message_id: r.2, tool_name: r.3,
                input_summary: r.4, raw_input: r.5, output_summary: r.6, raw_output: r.7,
                status: r.8, duration_ms: r.9, timestamp: r.10,
            }).collect())
        }
    }

    async fn get_tool_call_by_id(&self, tool_use_id: &str) -> DbResult<Option<ToolCallRecord>> {
        let row: Option<(String, String, Option<i64>, String, Option<String>, Option<String>, Option<String>, Option<String>, String, Option<i64>, String)> = sqlx::query_as(
            "SELECT id, session_id, message_id, tool_name, input_summary, raw_input, output_summary, raw_output, status, duration_ms, timestamp
             FROM conversation_tool_calls WHERE id = $1"
        )
        .bind(tool_use_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| ToolCallRecord {
            id: r.0, session_id: r.1, message_id: r.2, tool_name: r.3,
            input_summary: r.4, raw_input: r.5, output_summary: r.6, raw_output: r.7,
            status: r.8, duration_ms: r.9, timestamp: r.10,
        }))
    }

    async fn get_tool_call_stats(&self, session_id: &str) -> DbResult<Vec<(String, i64, i64, i64)>> {
        let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(
            "SELECT tool_name,
                    COUNT(*) as total,
                    SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) as success_count,
                    SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END) as error_count
             FROM conversation_tool_calls WHERE session_id = $1
             GROUP BY tool_name ORDER BY total DESC"
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn count_pending_tool_calls(&self) -> DbResult<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversation_tool_calls WHERE status = 'pending'"
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    async fn get_sessions_with_pending_tool_calls(&self) -> DbResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT session_id FROM conversation_tool_calls WHERE status = 'pending'"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn get_sessions_with_tool_calls(&self) -> DbResult<HashSet<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT session_id FROM conversation_tool_calls"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn get_messages_for_tool_call_backfill(&self, session_id: &str) -> DbResult<Vec<(String, String, String)>> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT role, raw_content, timestamp FROM conversation_messages
             WHERE session_id = $1 AND raw_content IS NOT NULL AND raw_content != ''
             AND role IN ('assistant', 'user', 'thinking', 'system', 'tool_result')
             ORDER BY id ASC"
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_conversations_with_jsonl(&self) -> DbResult<Vec<(String, String)>> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, jsonl_path FROM conversations WHERE jsonl_path IS NOT NULL AND jsonl_path != ''"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // -- Retrospective tool analysis --

    async fn get_retrospective_tool_stats(&self, session_id: &str, limit: i64) -> DbResult<Vec<(String, i64, i64, i64, f64)>> {
        let rows: Vec<(String, i64, i64, i64, f64)> = sqlx::query_as(
            "SELECT tool_name,
                    COUNT(*) as total,
                    SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) as success_count,
                    SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END) as error_count,
                    COALESCE(AVG(duration_ms), 0) as avg_duration
             FROM conversation_tool_calls WHERE session_id = $1
             GROUP BY tool_name ORDER BY total DESC LIMIT $2"
        )
        .bind(session_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_retrospective_meta(&self, session_id: &str) -> DbResult<(i64, i64, i64, i64)> {
        let (total_calls, total_duration, unique_tools): (i64, i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(duration_ms), 0), COUNT(DISTINCT tool_name)
             FROM conversation_tool_calls WHERE session_id = $1"
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;
        let (compact_count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversation_events
             WHERE session_id = $1 AND event_type = 'compact_boundary'"
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;
        Ok((total_calls, total_duration, unique_tools, compact_count))
    }

    async fn get_retrospective_repeat_patterns(&self, session_id: &str, min_streak: i64) -> DbResult<Vec<(String, i64, String, String)>> {
        let rows: Vec<(String, i64, String, String)> = sqlx::query_as(
            "WITH numbered AS (
                SELECT tool_name, timestamp,
                       ROW_NUMBER() OVER (ORDER BY id) as rn,
                       ROW_NUMBER() OVER (PARTITION BY tool_name ORDER BY id) as grn
                FROM conversation_tool_calls WHERE session_id = $1
            )
            SELECT tool_name, COUNT(*) as streak, MIN(timestamp) as start_t, MAX(timestamp) as end_t
            FROM numbered GROUP BY tool_name, (rn - grn)
            HAVING COUNT(*) >= $2 ORDER BY streak DESC"
        )
        .bind(session_id)
        .bind(min_streak)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_tool_name_sequence(&self, session_id: &str) -> DbResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT tool_name FROM conversation_tool_calls WHERE session_id = $1 ORDER BY id ASC"
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn get_retrospective_high_error_tools(&self, session_id: &str, min_error_rate: f64) -> DbResult<Vec<(String, f64, i64)>> {
        let rows: Vec<(String, f64, i64)> = sqlx::query_as(
            "SELECT tool_name,
                    ROUND(100.0 * SUM(CASE WHEN status='error' THEN 1 ELSE 0 END)::numeric / COUNT(*), 1)::float8 as error_rate,
                    COUNT(*) as total
             FROM conversation_tool_calls WHERE session_id = $1
             GROUP BY tool_name HAVING (100.0 * SUM(CASE WHEN status='error' THEN 1 ELSE 0 END)::numeric / COUNT(*)) > $2
             ORDER BY error_rate DESC"
        )
        .bind(session_id)
        .bind(min_error_rate)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_tool_error_samples(&self, session_id: &str, tool_name: &str) -> DbResult<Vec<(String, String, String)>> {
        let mut samples = Vec::new();
        // First error
        let first: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT COALESCE(input_summary, ''), COALESCE(output_summary, ''), timestamp
             FROM conversation_tool_calls
             WHERE session_id = $1 AND tool_name = $2 AND status = 'error'
             ORDER BY id ASC LIMIT 1"
        )
        .bind(session_id)
        .bind(tool_name)
        .fetch_all(&self.pool)
        .await?;
        samples.extend(first);
        // Last error (if different from first)
        let last: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT COALESCE(input_summary, ''), COALESCE(output_summary, ''), timestamp
             FROM conversation_tool_calls
             WHERE session_id = $1 AND tool_name = $2 AND status = 'error'
             ORDER BY id DESC LIMIT 1"
        )
        .bind(session_id)
        .bind(tool_name)
        .fetch_all(&self.pool)
        .await?;
        for sample in last {
            if samples.is_empty() || samples[0].2 != sample.2 {
                samples.push(sample);
            }
        }
        Ok(samples)
    }

    async fn get_tool_calls_for_detailed_analysis(&self, session_id: &str) -> DbResult<Vec<(String, String, String, String)>> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT tool_name, COALESCE(input_summary, ''), COALESCE(output_summary, ''), status
             FROM conversation_tool_calls WHERE session_id = $1 ORDER BY id ASC"
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn get_tool_calls_with_status_timeline(&self, session_id: &str) -> DbResult<Vec<(String, String, String, String)>> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT tool_name, status, COALESCE(input_summary, ''), timestamp
             FROM conversation_tool_calls WHERE session_id = $1 ORDER BY id ASC"
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}
