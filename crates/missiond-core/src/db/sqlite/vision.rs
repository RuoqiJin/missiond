use async_trait::async_trait;
use super::SqliteMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::VisionStore;

#[async_trait]
impl VisionStore for SqliteMissionStore {
    async fn get_image_description(&self, image_hash: &str) -> DbResult<Option<String>> {
        let image_hash = image_hash.to_owned();
        self.executor.run(move |db| db.get_image_description(&image_hash)).await
    }

    async fn save_image_description(&self, image_hash: &str, media_type: &str, description: &str) -> DbResult<()> {
        let image_hash = image_hash.to_owned();
        let media_type = media_type.to_owned();
        let description = description.to_owned();
        self.executor.run(move |db| db.save_image_description(&image_hash, &media_type, &description)).await
    }

    async fn update_message_content(&self, message_id: i64, new_content: &str) -> DbResult<()> {
        let new_content = new_content.to_owned();
        self.executor.run(move |db| db.update_message_content(message_id, &new_content)).await
    }

    async fn get_message_raw_content(&self, message_id: i64) -> DbResult<Option<String>> {
        self.executor.run(move |db| db.get_message_raw_content(message_id)).await
    }

    async fn find_unprocessed_image_messages(&self, limit: usize) -> DbResult<Vec<(i64, String)>> {
        self.executor.run(move |db| db.find_unprocessed_image_messages(limit)).await
    }

    async fn mark_vision_permanently_failed(&self, message_id: i64) -> DbResult<bool> {
        self.executor.run(move |db| db.mark_vision_permanently_failed(message_id)).await
    }

    async fn image_description_count(&self) -> DbResult<i64> {
        self.executor.run(|db| db.image_description_count()).await
    }

    async fn insert_translation(&self, message_id: i64, translation: &str, model: &str, duration_ms: u64) -> DbResult<()> {
        let translation = translation.to_owned();
        let model = model.to_owned();
        self.executor.run(move |db| db.insert_translation(message_id, &translation, &model, duration_ms)).await
    }

    async fn get_translation(&self, message_id: i64) -> DbResult<Option<(String, String)>> {
        self.executor.run(move |db| db.get_translation(message_id)).await
    }

    async fn has_translation(&self, message_id: i64) -> DbResult<bool> {
        self.executor.run(move |db| db.has_translation(message_id)).await
    }
}
