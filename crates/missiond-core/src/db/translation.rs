//! Message translation database operations.

use super::MissionDB;
use super::error::DbResult;

impl MissionDB {
    /// Insert a translation for a message.
    pub fn insert_translation(
        &self,
        message_id: i64,
        translation: &str,
        model: &str,
        duration_ms: u64,
    ) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR REPLACE INTO message_translations (message_id, translation, model, duration_ms)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![message_id, translation, model, duration_ms as i64],
        )?;
        Ok(())
    }

    /// Get translation for a message. Returns (translation, created_at).
    pub fn get_translation(&self, message_id: i64) -> DbResult<Option<(String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT translation, created_at FROM message_translations WHERE message_id = ?1"
        )?;
        let result = stmt.query_row(rusqlite::params![message_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        });
        match result {
            Ok(r) => Ok(Some(r)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Check if a translation exists for a message.
    pub fn has_translation(&self, message_id: i64) -> DbResult<bool> {
        let conn = self.read_conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM message_translations WHERE message_id = ?1",
            rusqlite::params![message_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }
}
