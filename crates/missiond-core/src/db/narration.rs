//! Message narration database operations (step-by-step explanations + cursor tracking).

use super::MissionDB;
use super::error::DbResult;
use crate::types::ConversationMessage;

impl MissionDB {
    // ── Message Narrations (GPT-5.4 step explanations) ──

    /// Batch-insert narrations for a session's messages.
    pub fn insert_narrations(&self, narrations: &[(i64, &str, &str, &str, &str)]) -> DbResult<usize> {
        let conn = self.conn();
        let mut count = 0;
        for (message_id, session_id, step_title, step_intent, step_result) in narrations {
            conn.execute(
                "INSERT OR REPLACE INTO message_narrations (message_id, session_id, step_title, step_intent, step_result)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![message_id, session_id, step_title, step_intent, step_result],
            )?;
            count += 1;
        }
        Ok(count)
    }

    /// Get all narrations for a session.
    pub fn get_narrations_for_session(&self, session_id: &str) -> DbResult<Vec<(i64, String, String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT message_id, step_title, step_intent, step_result FROM message_narrations WHERE session_id = ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![session_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get sessions that need narration (have unnarrated messages).
    /// Legacy: still used as fallback to discover new sessions without cursors.
    pub fn get_sessions_needing_narration(&self, min_unnarrated: i64) -> DbResult<Vec<(String, i64)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT cm.session_id, COUNT(*) as unnarrated
             FROM conversation_messages cm
             JOIN conversations c ON c.id = cm.session_id
             LEFT JOIN message_narrations mn ON mn.message_id = cm.id
             WHERE mn.message_id IS NULL
               AND c.conversation_type = 'user'
               AND cm.role IN ('user', 'assistant')
             GROUP BY cm.session_id
             HAVING unnarrated >= ?1
             ORDER BY MAX(cm.timestamp) DESC
             LIMIT 5"
        )?;
        let rows = stmt.query_map(rusqlite::params![min_unnarrated], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── Narration Cursor (batch progress tracking) ──

    /// Get or create a cursor for a session. Returns (last_processed_id, batch_index, status, retry_count, total_messages).
    pub fn get_or_create_narration_cursor(&self, session_id: &str) -> DbResult<(i64, i64, String, i64, i64)> {
        let conn = self.conn();
        // Count total user/assistant messages for this session
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM conversation_messages WHERE session_id = ?1 AND role IN ('user', 'assistant')",
            rusqlite::params![session_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO narration_cursors (session_id, total_messages) VALUES (?1, ?2)",
            rusqlite::params![session_id, total],
        )?;
        // Also update total_messages in case new messages arrived
        conn.execute(
            "UPDATE narration_cursors SET total_messages = ?2 WHERE session_id = ?1",
            rusqlite::params![session_id, total],
        )?;
        let row = conn.query_row(
            "SELECT last_processed_id, batch_index, status, retry_count, total_messages FROM narration_cursors WHERE session_id = ?1",
            rusqlite::params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )?;
        Ok(row)
    }

    /// Fetch next batch of unnarrated messages after the cursor position.
    /// Returns messages in ASC order, limited to batch_size.
    pub fn fetch_narration_batch(&self, session_id: &str, after_id: i64, batch_size: i64) -> DbResult<Vec<ConversationMessage>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM conversation_messages
             WHERE session_id = ?1 AND id > ?2 AND role IN ('user', 'assistant')
             ORDER BY id ASC LIMIT ?3"
        )?;
        let rows = stmt.query_map(rusqlite::params![session_id, after_id, batch_size], |row| {
            Self::row_to_conversation_message(row)
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get the last narration for a session (for context passing between batches).
    /// Returns (message_id, step_title, step_intent, step_result) of the last narrated message.
    pub fn get_last_narration(&self, session_id: &str) -> DbResult<Option<(i64, String, String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT message_id, step_title, step_intent, step_result FROM message_narrations
             WHERE session_id = ?1 ORDER BY message_id DESC LIMIT 1"
        )?;
        let mut rows = stmt.query_map(rusqlite::params![session_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;
        Ok(rows.next().and_then(|r| r.ok()))
    }

    /// Advance cursor after a successful batch. Updates in a transaction with narration inserts.
    pub fn commit_narration_batch(
        &self,
        session_id: &str,
        last_msg_id: i64,
        narrations: &[(i64, &str, &str, &str, &str)],
    ) -> DbResult<usize> {
        let conn = self.conn();
        // Insert narrations
        let mut count = 0;
        for (message_id, sid, step_title, step_intent, step_result) in narrations {
            conn.execute(
                "INSERT OR REPLACE INTO message_narrations (message_id, session_id, step_title, step_intent, step_result)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![message_id, sid, step_title, step_intent, step_result],
            )?;
            count += 1;
        }
        // Advance cursor
        conn.execute(
            "UPDATE narration_cursors SET last_processed_id = ?2, batch_index = batch_index + 1,
             status = 'idle', retry_count = 0, updated_at = datetime('now')
             WHERE session_id = ?1",
            rusqlite::params![session_id, last_msg_id],
        )?;
        Ok(count)
    }

    /// Mark cursor as processing.
    pub fn mark_narration_cursor_processing(&self, session_id: &str) -> DbResult<()> {
        self.conn().execute(
            "UPDATE narration_cursors SET status = 'processing', updated_at = datetime('now') WHERE session_id = ?1",
            rusqlite::params![session_id],
        )?;
        Ok(())
    }

    /// Increment retry count on failure. Sets status to 'error' if max retries exceeded.
    pub fn mark_narration_cursor_failed(&self, session_id: &str, max_retries: i64) -> DbResult<bool> {
        let conn = self.conn();
        conn.execute(
            "UPDATE narration_cursors SET retry_count = retry_count + 1, updated_at = datetime('now') WHERE session_id = ?1",
            rusqlite::params![session_id],
        )?;
        let retry_count: i64 = conn.query_row(
            "SELECT retry_count FROM narration_cursors WHERE session_id = ?1",
            rusqlite::params![session_id],
            |row| row.get(0),
        )?;
        if retry_count >= max_retries {
            conn.execute(
                "UPDATE narration_cursors SET status = 'error' WHERE session_id = ?1",
                rusqlite::params![session_id],
            )?;
            Ok(true) // permanently failed
        } else {
            conn.execute(
                "UPDATE narration_cursors SET status = 'idle' WHERE session_id = ?1",
                rusqlite::params![session_id],
            )?;
            Ok(false) // will retry
        }
    }
}
