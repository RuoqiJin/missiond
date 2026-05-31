//! InfraStore — PostgreSQL implementation.
//!
//! Scope (module system-support, Stage 2D migration):
//!   - watermarks (consumer/watcher/reconcile/gemini_cli)
//!   - backfill (progress/failures)
//!   - daemon_state
//!
//! Source: migrated from pg/observability.rs (watermarks + backfill)
//!         and pg/slot.rs (daemon_state) in Stage 2D.

use async_trait::async_trait;
use sqlx::Row;
use std::collections::HashMap;

use super::PgMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::{BackfillPhaseStatus, InfraStore};

#[async_trait]
impl InfraStore for PgMissionStore {
    // ── watermarks (from ObservabilityStore v0.4.x) ───────────────
    // ── consumer_watermarks ───────────────────────────────────────

    async fn watermark_get(
        &self,
        consumer: &str,
        session_id: &str,
    ) -> DbResult<Option<(Option<i64>, Option<String>)>> {
        let row: Option<(Option<i64>, Option<String>)> = sqlx::query_as(
            "SELECT last_processed_msg_id, last_processed_time FROM consumer_watermarks
             WHERE consumer_name = $1 AND session_id = $2",
        )
        .bind(consumer)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn watermark_advance_time(
        &self,
        consumer: &str,
        session_id: &str,
        timestamp: &str,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO consumer_watermarks (consumer_name, session_id, last_processed_time)
             VALUES ($1, $2, $3)
             ON CONFLICT(consumer_name, session_id) DO UPDATE SET last_processed_time = $3",
        )
        .bind(consumer)
        .bind(session_id)
        .bind(timestamp)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn watermark_advance_msg_id(
        &self,
        consumer: &str,
        session_id: &str,
        msg_id: i64,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO consumer_watermarks (consumer_name, session_id, last_processed_msg_id)
             VALUES ($1, $2, $3)
             ON CONFLICT(consumer_name, session_id) DO UPDATE SET
                last_processed_msg_id = CASE
                    WHEN consumer_watermarks.last_processed_msg_id IS NULL THEN EXCLUDED.last_processed_msg_id
                    ELSE GREATEST(consumer_watermarks.last_processed_msg_id, EXCLUDED.last_processed_msg_id)
                END",
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
                last_processed_msg_id = CASE
                    WHEN $3 IS NULL THEN consumer_watermarks.last_processed_msg_id
                    WHEN consumer_watermarks.last_processed_msg_id IS NULL THEN $3
                    ELSE GREATEST(consumer_watermarks.last_processed_msg_id, $3)
                END,
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

    async fn watermark_list(
        &self,
        consumer: &str,
    ) -> DbResult<Vec<(String, Option<i64>, Option<String>)>> {
        let rows: Vec<(String, Option<i64>, Option<String>)> = sqlx::query_as(
            "SELECT session_id, last_processed_msg_id, last_processed_time
             FROM consumer_watermarks WHERE consumer_name = $1",
        )
        .bind(consumer)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // ── watcher_cursors ──────────────────────────────────────────

    async fn load_watcher_cursors(&self) -> DbResult<HashMap<String, u64>> {
        let rows: Vec<(String, i64)> =
            sqlx::query_as("SELECT file_path, byte_offset FROM watcher_cursors")
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
                    updated_at = to_char(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS')",
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

    // ── reconcile_watermarks ─────────────────────────────────────

    async fn get_reconcile_watermark(&self, path: &str) -> DbResult<Option<i64>> {
        let row = sqlx::query(
            "SELECT last_reconciled_size FROM reconcile_watermarks WHERE jsonl_path = $1",
        )
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

    // ── gemini_cli_watermarks ────────────────────────────────────

    async fn load_gemini_cursors(&self) -> DbResult<HashMap<String, i64>> {
        let rows: Vec<(String, i64)> =
            sqlx::query_as("SELECT session_file, last_msg_count FROM gemini_cli_watermarks")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().collect())
    }

    async fn save_gemini_cursor(
        &self,
        file_path: &str,
        session_id: &str,
        msg_count: i64,
    ) -> DbResult<()> {
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

    // ── backfill (from ObservabilityStore v0.4.x) ────────────────

    async fn backfill_get_phase(&self, phase: &str) -> DbResult<Option<BackfillPhaseStatus>> {
        let row = sqlx::query(
            "SELECT phase, status, last_cursor, total_estimated, processed, failed, started_at, completed_at
             FROM backfill_progress WHERE phase = $1"
        )
        .bind(phase)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| BackfillPhaseStatus {
            phase: r.get("phase"),
            status: r.get("status"),
            last_cursor: r.get("last_cursor"),
            total_estimated: r.get("total_estimated"),
            processed: r.get("processed"),
            failed: r.get("failed"),
            started_at: r.get("started_at"),
            completed_at: r.get("completed_at"),
        }))
    }

    async fn backfill_list_phases(&self) -> DbResult<Vec<BackfillPhaseStatus>> {
        let rows = sqlx::query(
            "SELECT phase, status, last_cursor, total_estimated, processed, failed, started_at, completed_at
             FROM backfill_progress ORDER BY phase"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| BackfillPhaseStatus {
                phase: r.get("phase"),
                status: r.get("status"),
                last_cursor: r.get("last_cursor"),
                total_estimated: r.get("total_estimated"),
                processed: r.get("processed"),
                failed: r.get("failed"),
                started_at: r.get("started_at"),
                completed_at: r.get("completed_at"),
            })
            .collect())
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
             WHERE phase = $1",
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
             WHERE phase = $1",
        )
        .bind(phase)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn backfill_record_failure(
        &self,
        session_id: &str,
        phase: &str,
        error: &str,
    ) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO backfill_failures (session_id, phase, retry_count, last_error, updated_at)
             VALUES ($1, $2, 1, $3, $4)
             ON CONFLICT(session_id, phase) DO UPDATE SET
               retry_count = backfill_failures.retry_count + 1,
               last_error = $3,
               updated_at = $4",
        )
        .bind(session_id)
        .bind(phase)
        .bind(error)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn backfill_retryable_failures(
        &self,
        phase: &str,
        max_retries: i64,
        limit: i64,
    ) -> DbResult<Vec<String>> {
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

    async fn backfill_retryable_failures_no_cooldown(
        &self,
        phase: &str,
        max_retries: i64,
    ) -> DbResult<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM backfill_failures WHERE phase = $1 AND retry_count < $2",
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

    // ── daemon_state (from SlotStore v0.4.x) ─────────────────────

    async fn daemon_state_get(&self, key: &str) -> DbResult<Option<i64>> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM daemon_state WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| r.0.parse::<i64>().unwrap_or(0)))
    }

    async fn daemon_state_set(&self, key: &str, value: i64) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO daemon_state (key, value, updated_at) VALUES ($1, $2, $3)
             ON CONFLICT (key) DO UPDATE SET value = $2, updated_at = $3",
        )
        .bind(key)
        .bind(value.to_string())
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
