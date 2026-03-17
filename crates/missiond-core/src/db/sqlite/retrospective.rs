use async_trait::async_trait;
use super::SqliteMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::RetrospectiveStore;
use crate::types::*;

#[async_trait]
impl RetrospectiveStore for SqliteMissionStore {
    // -- audit.rs: retrospective --

    async fn save_retrospective_result(&self, session_id: &str, trigger_reason: &str, quick_stats: &str, full_analysis: Option<&str>) -> DbResult<()> {
        let session_id = session_id.to_owned();
        let trigger_reason = trigger_reason.to_owned();
        let quick_stats = quick_stats.to_owned();
        let full_analysis = full_analysis.map(|s| s.to_owned());
        self.executor.run(move |db| db.save_retrospective_result(&session_id, &trigger_reason, &quick_stats, full_analysis.as_deref())).await
    }

    async fn has_retrospective_result(&self, session_id: &str) -> DbResult<bool> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.has_retrospective_result(&session_id)).await
    }

    async fn get_sessions_needing_retrospective(&self) -> DbResult<Vec<(String, i64, i64, f64)>> {
        self.executor.run(|db| db.get_sessions_needing_retrospective()).await
    }

    async fn get_sessions_for_retro_backfill(&self, since: &str, force: bool) -> DbResult<Vec<(String, i64, i64, f64)>> {
        let since = since.to_owned();
        self.executor.run(move |db| db.get_sessions_for_retro_backfill(&since, force)).await
    }

    async fn list_retrospective_results(&self, limit: i64) -> DbResult<Vec<(String, String, String, Option<String>, String)>> {
        self.executor.run(move |db| db.list_retrospective_results(limit)).await
    }

    async fn get_retrospective_result(&self, session_id: &str) -> DbResult<Option<(String, String, Option<String>, String)>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_retrospective_result(&session_id)).await
    }

    // -- narration.rs --

    async fn insert_narrations(&self, narrations: &[(i64, &str, &str, &str, &str)]) -> DbResult<usize> {
        let narrations: Vec<(i64, String, String, String, String)> = narrations
            .iter()
            .map(|(id, a, b, c, d)| (*id, a.to_string(), b.to_string(), c.to_string(), d.to_string()))
            .collect();
        self.executor.run(move |db| {
            let refs: Vec<(i64, &str, &str, &str, &str)> = narrations
                .iter()
                .map(|(id, a, b, c, d)| (*id, a.as_str(), b.as_str(), c.as_str(), d.as_str()))
                .collect();
            db.insert_narrations(&refs)
        }).await
    }

    async fn get_narrations_for_session(&self, session_id: &str) -> DbResult<Vec<(i64, String, String, String)>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_narrations_for_session(&session_id)).await
    }

    async fn get_sessions_needing_narration(&self, min_unnarrated: i64) -> DbResult<Vec<(String, i64)>> {
        self.executor.run(move |db| db.get_sessions_needing_narration(min_unnarrated)).await
    }

    async fn get_or_create_narration_cursor(&self, session_id: &str) -> DbResult<(i64, i64, String, i64, i64)> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_or_create_narration_cursor(&session_id)).await
    }

    async fn fetch_narration_batch(&self, session_id: &str, after_id: i64, batch_size: i64) -> DbResult<Vec<ConversationMessage>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.fetch_narration_batch(&session_id, after_id, batch_size)).await
    }

    async fn get_last_narration(&self, session_id: &str) -> DbResult<Option<(i64, String, String, String)>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_last_narration(&session_id)).await
    }

    async fn commit_narration_batch(&self, session_id: &str, last_msg_id: i64, narrations: &[(i64, &str, &str, &str, &str)]) -> DbResult<usize> {
        let session_id = session_id.to_owned();
        let narrations: Vec<(i64, String, String, String, String)> = narrations
            .iter()
            .map(|(id, a, b, c, d)| (*id, a.to_string(), b.to_string(), c.to_string(), d.to_string()))
            .collect();
        self.executor.run(move |db| {
            let refs: Vec<(i64, &str, &str, &str, &str)> = narrations
                .iter()
                .map(|(id, a, b, c, d)| (*id, a.as_str(), b.as_str(), c.as_str(), d.as_str()))
                .collect();
            db.commit_narration_batch(&session_id, last_msg_id, &refs)
        }).await
    }

    async fn mark_narration_cursor_processing(&self, session_id: &str) -> DbResult<()> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.mark_narration_cursor_processing(&session_id)).await
    }

    async fn mark_narration_cursor_failed(&self, session_id: &str, max_retries: i64) -> DbResult<bool> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.mark_narration_cursor_failed(&session_id, max_retries)).await
    }
}
