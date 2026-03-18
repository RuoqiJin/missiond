use super::MissionDB;
use super::error::DbResult;
use rusqlite::params;
pub use super::shared::BackfillPhaseStatus;

impl MissionDB {
    // ============ Backfill Progress ============

    /// Get the status of a backfill phase. Returns None if never started.
    pub fn backfill_get_phase(&self, phase: &str) -> DbResult<Option<BackfillPhaseStatus>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT phase, status, last_cursor, total_estimated, processed, failed, started_at, completed_at
             FROM backfill_progress WHERE phase = ?1"
        )?;
        let result = stmt.query_row(params![phase], |row| {
            Ok(BackfillPhaseStatus {
                phase: row.get(0)?,
                status: row.get(1)?,
                last_cursor: row.get(2)?,
                total_estimated: row.get(3)?,
                processed: row.get(4)?,
                failed: row.get(5)?,
                started_at: row.get(6)?,
                completed_at: row.get(7)?,
            })
        }).ok();
        Ok(result)
    }

    /// Get all backfill phases status (for dashboard).
    pub fn backfill_list_phases(&self) -> DbResult<Vec<BackfillPhaseStatus>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT phase, status, last_cursor, total_estimated, processed, failed, started_at, completed_at
             FROM backfill_progress ORDER BY phase"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(BackfillPhaseStatus {
                phase: row.get(0)?,
                status: row.get(1)?,
                last_cursor: row.get(2)?,
                total_estimated: row.get(3)?,
                processed: row.get(4)?,
                failed: row.get(5)?,
                started_at: row.get(6)?,
                completed_at: row.get(7)?,
            })
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Initialize or reset a backfill phase to 'running'.
    pub fn backfill_start_phase(&self, phase: &str, total_estimated: i64) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO backfill_progress (phase, status, last_cursor, total_estimated, processed, failed, started_at, updated_at)
             VALUES (?1, 'running', 0, ?2, 0, 0, datetime('now'), datetime('now'))
             ON CONFLICT(phase) DO UPDATE SET
               status = 'running',
               last_cursor = CASE WHEN status = 'completed' THEN 0 ELSE last_cursor END,
               total_estimated = ?2,
               processed = CASE WHEN status = 'completed' THEN 0 ELSE processed END,
               failed = CASE WHEN status = 'completed' THEN 0 ELSE failed END,
               started_at = CASE WHEN status = 'completed' THEN datetime('now') ELSE started_at END,
               updated_at = datetime('now')",
            params![phase, total_estimated],
        )?;
        Ok(())
    }

    /// Update cursor + counters after processing a batch.
    pub fn backfill_update_progress(
        &self,
        phase: &str,
        new_cursor: i64,
        batch_success: i64,
        batch_failed: i64,
    ) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE backfill_progress
             SET last_cursor = ?2, processed = processed + ?3, failed = failed + ?4, updated_at = datetime('now')
             WHERE phase = ?1",
            params![phase, new_cursor, batch_success, batch_failed],
        )?;
        Ok(())
    }

    /// Mark a phase as completed.
    pub fn backfill_complete_phase(&self, phase: &str) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE backfill_progress
             SET status = 'completed', completed_at = datetime('now'), updated_at = datetime('now')
             WHERE phase = ?1",
            params![phase],
        )?;
        Ok(())
    }

    // ============ Backfill Failures ============

    /// Record a failed session (upsert: increment retry_count).
    pub fn backfill_record_failure(&self, session_id: &str, phase: &str, error: &str) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO backfill_failures (session_id, phase, retry_count, last_error, updated_at)
             VALUES (?1, ?2, 1, ?3, datetime('now'))
             ON CONFLICT(session_id, phase) DO UPDATE SET
               retry_count = retry_count + 1,
               last_error = ?3,
               updated_at = datetime('now')",
            params![session_id, phase, error],
        )?;
        Ok(())
    }

    /// Get sessions that failed but are still retryable (retry_count < max_retries).
    /// Cooldown: only return failures older than 5 minutes to avoid rapid re-retry loops.
    pub fn backfill_retryable_failures(&self, phase: &str, max_retries: i64, limit: i64) -> DbResult<Vec<String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT session_id FROM backfill_failures
             WHERE phase = ?1 AND retry_count < ?2
               AND updated_at < datetime('now', '-5 minutes')
             ORDER BY updated_at ASC LIMIT ?3"
        )?;
        let rows = stmt.query_map(params![phase, max_retries, limit], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for r in rows { ids.push(r?); }
        Ok(ids)
    }

    /// Count failures still retryable (ignoring cooldown) — used to detect cooling-down state.
    pub fn backfill_retryable_failures_no_cooldown(&self, phase: &str, max_retries: i64) -> DbResult<i64> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT COUNT(*) FROM backfill_failures WHERE phase = ?1 AND retry_count < ?2",
            params![phase, max_retries],
            |row| row.get(0),
        ).map_err(Into::into)
    }

    /// Remove a failure record (on successful retry).
    pub fn backfill_clear_failure(&self, session_id: &str, phase: &str) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM backfill_failures WHERE session_id = ?1 AND phase = ?2",
            params![session_id, phase],
        )?;
        Ok(())
    }

    // ============ Cursor-based Conversation Queries ============

    /// Conversations missing summary, with cursor-based pagination using rowid.
    /// Also picks up rows with llm_summary = '[timeout]' (legacy pollution).
    pub fn conversations_missing_summary_cursor(&self, cursor: i64, limit: i64) -> DbResult<Vec<(i64, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT rowid, id FROM conversations
             WHERE (llm_summary IS NULL OR llm_summary = '[timeout]')
               AND status = 'completed' AND message_count >= 6
               AND conversation_type IN ('user', 'worker')
               AND rowid > ?1
             ORDER BY rowid ASC LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![cursor, limit], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Conversations needing topic vectors, with cursor-based pagination.
    pub fn conversations_needing_topic_vectors_cursor(
        &self,
        provider: &str,
        cursor: i64,
        limit: i64,
    ) -> DbResult<Vec<(i64, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT c.rowid, c.id FROM conversations c
             WHERE c.llm_summary IS NOT NULL
               AND c.conversation_type IN ('user', 'worker')
               AND c.rowid > ?2
               AND NOT EXISTS (
                   SELECT 1 FROM conversation_topic_vectors tv
                   WHERE tv.session_id = c.id AND tv.embedding_provider = ?1
               )
             ORDER BY c.rowid ASC LIMIT ?3"
        )?;
        let rows = stmt.query_map(params![provider, cursor, limit], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Count total conversations eligible for summary backfill.
    /// Includes rows with llm_summary = '[timeout]' (legacy pollution).
    pub fn conversations_missing_summary_count(&self) -> DbResult<i64> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT COUNT(*) FROM conversations
             WHERE (llm_summary IS NULL OR llm_summary = '[timeout]')
               AND status = 'completed' AND message_count >= 6
               AND conversation_type IN ('user', 'worker')",
            [],
            |row| row.get(0),
        ).map_err(Into::into)
    }

    /// Count conversations needing topic vectors.
    pub fn conversations_needing_topic_vectors_count(&self, provider: &str) -> DbResult<i64> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT COUNT(*) FROM conversations c
             WHERE c.llm_summary IS NOT NULL
               AND c.conversation_type IN ('user', 'worker')
               AND NOT EXISTS (
                   SELECT 1 FROM conversation_topic_vectors tv
                   WHERE tv.session_id = c.id AND tv.embedding_provider = ?1
               )",
            params![provider],
            |row| row.get(0),
        ).map_err(Into::into)
    }
}
