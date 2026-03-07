use rusqlite::{params, OptionalExtension};
use super::error::DbResult;
use super::MissionDB;

impl MissionDB {
    // ============ Image Description Cache ============

    /// Get cached image description by SHA-256 hash.
    pub fn get_image_description(&self, image_hash: &str) -> DbResult<Option<String>> {
        let conn = self.read_conn();
        Ok(conn.query_row(
            "SELECT description FROM image_descriptions WHERE image_hash = ?1",
            params![image_hash],
            |row| row.get(0),
        ).optional()?)
    }

    /// Save image description to cache.
    pub fn save_image_description(
        &self,
        image_hash: &str,
        media_type: &str,
        description: &str,
    ) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR REPLACE INTO image_descriptions (image_hash, media_type, description, char_count)
             VALUES (?1, ?2, ?3, ?4)",
            params![image_hash, media_type, description, description.len()],
        )?;
        Ok(())
    }

    /// Update a conversation_message's content column (for vision enrichment).
    pub fn update_message_content(&self, message_id: i64, new_content: &str) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE conversation_messages SET content = ?1 WHERE id = ?2",
            params![new_content, message_id],
        )?;
        Ok(())
    }

    /// Get raw_content for a specific message (contains original JSON with base64 images).
    pub fn get_message_raw_content(&self, message_id: i64) -> DbResult<Option<String>> {
        let conn = self.read_conn();
        Ok(conn.query_row(
            "SELECT raw_content FROM conversation_messages WHERE id = ?1",
            params![message_id],
            |row| row.get(0),
        ).optional()?)
    }

    /// Find messages with unprocessed images (have raw_content with image blocks but content still has placeholder).
    pub fn find_unprocessed_image_messages(&self, limit: usize) -> DbResult<Vec<(i64, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, session_id FROM conversation_messages
             WHERE content LIKE '%[图片: %'
               AND raw_content IS NOT NULL
               AND raw_content LIKE '%\"type\":\"image\"%'
             ORDER BY id DESC
             LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut results = Vec::new();
        for r in rows { results.push(r?); }
        Ok(results)
    }

    /// Count of image_descriptions cache entries.
    pub fn image_description_count(&self) -> DbResult<i64> {
        let conn = self.read_conn();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM image_descriptions",
            [],
            |row| row.get(0),
        )?)
    }
}
