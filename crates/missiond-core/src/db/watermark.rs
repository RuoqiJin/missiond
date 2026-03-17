//! Consumer watermarks: decoupled per-consumer cursors for conversation processing.
//! Part of the Three-Layer Conversation Log Refactor (P0).

use rusqlite::params;
use super::error::DbResult;
use super::MissionDB;

impl MissionDB {
    // ============ Consumer Watermarks ============

    /// Get watermark for a consumer on a specific session.
    pub fn watermark_get(&self, consumer: &str, session_id: &str) -> DbResult<Option<(Option<i64>, Option<String>)>> {
        let conn = self.read_conn();
        let result = conn.query_row(
            "SELECT last_processed_msg_id, last_processed_time FROM consumer_watermarks
             WHERE consumer_name = ?1 AND session_id = ?2",
            params![consumer, session_id],
            |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<String>>(1)?)),
        );
        match result {
            Ok(r) => Ok(Some(r)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Update (upsert) watermark for a consumer on a session by timestamp.
    pub fn watermark_advance_time(&self, consumer: &str, session_id: &str, timestamp: &str) -> DbResult<()> {
        self.conn().execute(
            "INSERT INTO consumer_watermarks (consumer_name, session_id, last_processed_time)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(consumer_name, session_id) DO UPDATE SET last_processed_time = ?3",
            params![consumer, session_id, timestamp],
        )?;
        Ok(())
    }

    /// Update (upsert) watermark for a consumer on a session by message ID.
    pub fn watermark_advance_msg_id(&self, consumer: &str, session_id: &str, msg_id: i64) -> DbResult<()> {
        self.conn().execute(
            "INSERT INTO consumer_watermarks (consumer_name, session_id, last_processed_msg_id)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(consumer_name, session_id) DO UPDATE SET last_processed_msg_id = ?3",
            params![consumer, session_id, msg_id],
        )?;
        Ok(())
    }

    /// Update watermark with both message ID and extra JSON metadata.
    pub fn watermark_advance_full(&self, consumer: &str, session_id: &str, msg_id: Option<i64>, timestamp: Option<&str>, extra: Option<&str>) -> DbResult<()> {
        self.conn().execute(
            "INSERT INTO consumer_watermarks (consumer_name, session_id, last_processed_msg_id, last_processed_time, extra)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(consumer_name, session_id) DO UPDATE SET
                last_processed_msg_id = COALESCE(?3, last_processed_msg_id),
                last_processed_time = COALESCE(?4, last_processed_time),
                extra = COALESCE(?5, extra)",
            params![consumer, session_id, msg_id, timestamp, extra],
        )?;
        Ok(())
    }

    /// Get all sessions with watermarks for a given consumer.
    pub fn watermark_list(&self, consumer: &str) -> DbResult<Vec<(String, Option<i64>, Option<String>)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT session_id, last_processed_msg_id, last_processed_time
             FROM consumer_watermarks WHERE consumer_name = ?1"
        )?;
        let rows = stmt.query_map(params![consumer], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, Option<String>>(2)?))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    // ============ Message Labels ============

    /// Insert a label on a message (rule-based, synchronous).
    pub fn label_set(&self, message_id: i64, label: &str, value: &str, source: &str) -> DbResult<()> {
        self.conn().execute(
            "INSERT OR REPLACE INTO message_labels (message_id, label, value, source)
             VALUES (?1, ?2, ?3, ?4)",
            params![message_id, label, value, source],
        )?;
        Ok(())
    }

    /// Batch insert labels for multiple messages (single transaction, single fsync).
    pub fn label_set_batch(&self, labels: &[(i64, &str, &str, &str)]) -> DbResult<usize> {
        if labels.is_empty() {
            return Ok(0);
        }
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO message_labels (message_id, label, value, source)
             VALUES (?1, ?2, ?3, ?4)"
        )?;
        let mut count = 0;
        for (msg_id, label, value, source) in labels {
            stmt.execute(params![msg_id, label, value, source])?;
            count += 1;
        }
        drop(stmt);
        tx.commit()?;
        Ok(count)
    }

    /// Get all labels for a message.
    pub fn label_get(&self, message_id: i64) -> DbResult<Vec<(String, String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT label, COALESCE(value, ''), source FROM message_labels WHERE message_id = ?1"
        )?;
        let rows = stmt.query_map(params![message_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Get labels for multiple messages (batch).
    pub fn label_get_batch(&self, message_ids: &[i64]) -> DbResult<std::collections::HashMap<i64, Vec<(String, String)>>> {
        if message_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let conn = self.read_conn();
        let placeholders: String = message_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT message_id, label, COALESCE(value, '') FROM message_labels WHERE message_id IN ({})",
            placeholders
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        let mut result: std::collections::HashMap<i64, Vec<(String, String)>> = std::collections::HashMap::new();
        for r in rows {
            let (msg_id, label, value) = r?;
            result.entry(msg_id).or_default().push((label, value));
        }
        Ok(result)
    }

    /// Find messages by label (for filtering).
    pub fn label_find_messages(&self, label: &str, value: Option<&str>, limit: i64) -> DbResult<Vec<i64>> {
        let conn = self.read_conn();
        let mut ids = Vec::new();
        if let Some(v) = value {
            let mut stmt = conn.prepare(
                "SELECT message_id FROM message_labels WHERE label = ?1 AND value = ?2 LIMIT ?3"
            )?;
            let rows = stmt.query_map(params![label, v, limit], |row| row.get::<_, i64>(0))?;
            for r in rows { ids.push(r?); }
        } else {
            let mut stmt = conn.prepare(
                "SELECT message_id FROM message_labels WHERE label = ?1 LIMIT ?2"
            )?;
            let rows = stmt.query_map(params![label, limit], |row| row.get::<_, i64>(0))?;
            for r in rows { ids.push(r?); }
        }
        Ok(ids)
    }
}
