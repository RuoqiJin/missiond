//! VisionStore + translation — PostgreSQL implementation.

use async_trait::async_trait;
use crate::db::error::DbResult;
use crate::db::traits::VisionStore;
use super::PgMissionStore;

#[cfg(feature = "postgres")]
#[async_trait]
impl VisionStore for PgMissionStore {
    async fn get_image_description(&self, image_hash: &str) -> DbResult<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT description FROM image_descriptions WHERE image_hash = $1"
        )
        .bind(image_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.0))
    }

    async fn save_image_description(&self, image_hash: &str, media_type: &str, description: &str) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO image_descriptions (image_hash, media_type, description, char_count)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (image_hash) DO UPDATE SET
                media_type = EXCLUDED.media_type,
                description = EXCLUDED.description,
                char_count = EXCLUDED.char_count"
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
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT raw_content FROM conversation_messages WHERE id = $1"
        )
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
             LIMIT $1"
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
             WHERE id = $1 AND content LIKE '%[图片: %'"
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

    async fn insert_translation(&self, message_id: i64, translation: &str, model: &str, duration_ms: u64) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO message_translations (message_id, translation, model, duration_ms)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (message_id) DO UPDATE SET
                translation = EXCLUDED.translation,
                model = EXCLUDED.model,
                duration_ms = EXCLUDED.duration_ms"
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
            "SELECT translation, created_at FROM message_translations WHERE message_id = $1"
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    async fn has_translation(&self, message_id: i64) -> DbResult<bool> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM message_translations WHERE message_id = $1"
        )
        .bind(message_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }
}
