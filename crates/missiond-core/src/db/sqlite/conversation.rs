use async_trait::async_trait;
use super::SqliteMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::ConversationStore;
use crate::types::*;

#[async_trait]
impl ConversationStore for SqliteMissionStore {
    // -- conversation.rs: session CRUD --

    async fn upsert_conversation(&self, conv: &Conversation) -> DbResult<()> {
        let conv = conv.clone();
        self.executor.run(move |db| db.upsert_conversation(&conv)).await
    }

    async fn ensure_conversation_exists(&self, _session_id: &str, _project_path: &str, _jsonl_path: &str, _status: &str, _conversation_type: &str, _parent_session_id: Option<&str>, _started_at: Option<&str>) -> DbResult<()> {
        // SQLite stub — ReconcileWorker only runs with PG
        Ok(())
    }

    async fn refresh_conversation_message_count(&self, _session_id: &str) -> DbResult<()> {
        // SQLite stub
        Ok(())
    }

    async fn get_conversation(&self, id: &str) -> DbResult<Option<Conversation>> {
        let id = id.to_owned();
        self.executor.run(move |db| db.get_conversation(&id)).await
    }

    async fn list_conversations(&self, status: Option<&str>, limit: i64, conv_type: Option<&str>, task_id: Option<&str>, since: Option<&str>, until: Option<&str>, source: Option<&str>) -> DbResult<Vec<Conversation>> {
        let status = status.map(|s| s.to_owned());
        let conv_type = conv_type.map(|s| s.to_owned());
        let task_id = task_id.map(|s| s.to_owned());
        let since = since.map(|s| s.to_owned());
        let until = until.map(|s| s.to_owned());
        let source = source.map(|s| s.to_owned());
        self.executor.run(move |db| db.list_conversations(status.as_deref(), limit, conv_type.as_deref(), task_id.as_deref(), since.as_deref(), until.as_deref(), source.as_deref())).await
    }

    async fn get_child_conversations(&self, parent_session_id: &str) -> DbResult<Vec<Conversation>> {
        let parent_session_id = parent_session_id.to_owned();
        self.executor.run(move |db| db.get_child_conversations(&parent_session_id)).await
    }

    async fn fix_orphan_parent_links(&self, _session_ids: &[String]) -> DbResult<usize> {
        Ok(0) // SQLite deprecated
    }

    async fn link_compaction_fragment(&self, _fragment_id: &str, _original_id: &str) -> DbResult<bool> {
        Ok(false) // SQLite deprecated
    }

    // -- conversation.rs: deep analysis tracking --

    async fn get_pending_deep_analysis(&self, current_version: i32, max_retries: i32) -> DbResult<Vec<Conversation>> {
        self.executor.run(move |db| db.get_pending_deep_analysis(current_version, max_retries)).await
    }

    async fn has_pending_deep_analysis(&self, current_version: i32, max_retries: i32) -> DbResult<bool> {
        self.executor.run(move |db| db.has_pending_deep_analysis(current_version, max_retries)).await
    }

    async fn count_pending_deep_analysis(&self, current_version: i32, max_retries: i32) -> DbResult<i64> {
        self.executor.run(move |db| db.count_pending_deep_analysis(current_version, max_retries)).await
    }

    async fn count_pending_realtime(&self) -> DbResult<i64> {
        self.executor.run(|db| db.count_pending_realtime()).await
    }

    async fn pending_realtime_detail(&self) -> DbResult<Vec<(String, i64, String)>> {
        self.executor.run(|db| db.pending_realtime_detail()).await
    }

    async fn pending_deep_detail(&self, current_version: i32, max_retries: i32) -> DbResult<Vec<(String, String, i32)>> {
        self.executor.run(move |db| db.pending_deep_detail(current_version, max_retries)).await
    }

    async fn mark_analysis_complete(&self, id: &str, version: i32) -> DbResult<()> {
        let id = id.to_owned();
        self.executor.run(move |db| db.mark_analysis_complete(&id, version)).await
    }

    async fn update_deep_checkpoint(&self, id: &str, message_id: i64) -> DbResult<()> {
        let id = id.to_owned();
        self.executor.run(move |db| db.update_deep_checkpoint(&id, message_id)).await
    }

    async fn mark_analysis_failed(&self, id: &str) -> DbResult<()> {
        let id = id.to_owned();
        self.executor.run(move |db| db.mark_analysis_failed(&id)).await
    }

    // -- conversation.rs: habit scanning --

    async fn get_unscanned_conversations(&self, limit: usize) -> DbResult<Vec<Conversation>> {
        self.executor.run(move |db| db.get_unscanned_conversations(limit)).await
    }

    async fn mark_habit_scanned(&self, id: &str) -> DbResult<()> {
        let id = id.to_owned();
        self.executor.run(move |db| db.mark_habit_scanned(&id)).await
    }

    async fn count_unscanned_conversations(&self) -> DbResult<i64> {
        self.executor.run(|db| db.count_unscanned_conversations()).await
    }

    async fn count_scannable_conversations(&self) -> DbResult<i64> {
        self.executor.run(|db| db.count_scannable_conversations()).await
    }

    // -- conversation.rs: summary & embeddings --

    async fn set_conversation_summary(&self, id: &str, summary: &str) -> DbResult<()> {
        let id = id.to_owned();
        let summary = summary.to_owned();
        self.executor.run(move |db| db.set_conversation_summary(&id, &summary)).await
    }

    async fn clear_conversation_summary(&self, id: &str) -> DbResult<()> {
        let id = id.to_owned();
        self.executor.run(move |db| db.clear_conversation_summary(&id)).await
    }

    async fn set_conversation_embedding(&self, id: &str, embedding: &[f32], provider: &str) -> DbResult<()> {
        let id = id.to_owned();
        let embedding = embedding.to_vec();
        let provider = provider.to_owned();
        self.executor.run(move |db| db.set_conversation_embedding(&id, &embedding, &provider)).await
    }

    async fn load_conversation_embeddings(&self, provider: &str) -> DbResult<Vec<(String, Vec<f32>)>> {
        let provider = provider.to_owned();
        self.executor.run(move |db| db.load_conversation_embeddings(&provider)).await
    }

    async fn conversations_missing_summary(&self, limit: i64) -> DbResult<Vec<String>> {
        self.executor.run(move |db| db.conversations_missing_summary(limit)).await
    }

    async fn conversations_stale_embedding(&self, current_provider: &str, limit: i64) -> DbResult<Vec<String>> {
        let current_provider = current_provider.to_owned();
        self.executor.run(move |db| db.conversations_stale_embedding(&current_provider, limit)).await
    }

    // -- conversation.rs: topic vectors --

    async fn set_conversation_topic_vectors(&self, session_id: &str, topics: &[(String, Vec<f32>)], provider: &str) -> DbResult<()> {
        let session_id = session_id.to_owned();
        let topics = topics.to_vec();
        let provider = provider.to_owned();
        self.executor.run(move |db| db.set_conversation_topic_vectors(&session_id, &topics, &provider)).await
    }

    async fn load_conversation_topic_vectors(&self, provider: &str) -> DbResult<Vec<(String, Vec<Vec<f32>>)>> {
        let provider = provider.to_owned();
        self.executor.run(move |db| db.load_conversation_topic_vectors(&provider)).await
    }

    async fn get_conversation_topics(&self, session_id: &str) -> DbResult<Vec<String>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_conversation_topics(&session_id)).await
    }

    async fn conversations_needing_topic_vectors(&self, provider: &str, limit: i64) -> DbResult<Vec<String>> {
        let provider = provider.to_owned();
        self.executor.run(move |db| db.conversations_needing_topic_vectors(&provider, limit)).await
    }

    // -- conversation.rs: timeline reconstruction --

    async fn conversations_needing_timeline(&self, limit: i64) -> DbResult<Vec<String>> {
        self.executor.run(move |db| db.conversations_needing_timeline(limit)).await
    }

    async fn get_compaction_fragments(&self, parent_id: &str) -> DbResult<Vec<(String, String, i64)>> {
        let parent_id = parent_id.to_owned();
        self.executor.run(move |db| db.get_compaction_fragments(&parent_id)).await
    }

    async fn get_last_assistant_content(&self, session_id: &str) -> DbResult<Option<String>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_last_assistant_content(&session_id)).await
    }

    async fn set_session_timeline(&self, parent_id: &str, timeline_json: &str) -> DbResult<bool> {
        let parent_id = parent_id.to_owned();
        let timeline_json = timeline_json.to_owned();
        self.executor.run(move |db| db.set_session_timeline(&parent_id, &timeline_json)).await
    }

    // -- audit.rs: conversation lifecycle --

    async fn mark_conversation_analyzed(&self, id: &str) -> DbResult<()> {
        let id = id.to_owned();
        self.executor.run(move |db| db.mark_conversation_analyzed(&id)).await
    }

    async fn get_unanalyzed_conversations(&self) -> DbResult<Vec<Conversation>> {
        self.executor.run(|db| db.get_unanalyzed_conversations()).await
    }

    async fn complete_conversation(&self, id: &str) -> DbResult<()> {
        let id = id.to_owned();
        self.executor.run(move |db| db.complete_conversation(&id)).await
    }

    async fn save_conversation_exit_code(&self, id: &str, exit_code: i32) -> DbResult<()> {
        let id = id.to_owned();
        self.executor.run(move |db| db.save_conversation_exit_code(&id, exit_code)).await
    }

    async fn complete_stale_conversations(&self, cutoff: &str) -> DbResult<usize> {
        let cutoff = cutoff.to_owned();
        self.executor.run(move |db| db.complete_stale_conversations(&cutoff)).await
    }

    async fn mark_conversation_compacted(&self, id: &str) -> DbResult<()> {
        let id = id.to_owned();
        self.executor.run(move |db| db.mark_conversation_compacted(&id)).await
    }

    async fn set_conversation_task_id(&self, id: &str, task_id: &str) -> DbResult<()> {
        let id = id.to_owned();
        let task_id = task_id.to_owned();
        self.executor.run(move |db| db.set_conversation_task_id(&id, &task_id)).await
    }

    async fn get_conversations_by_task_id(&self, task_id: &str) -> DbResult<Vec<Conversation>> {
        let task_id = task_id.to_owned();
        self.executor.run(move |db| db.get_conversations_by_task_id(&task_id)).await
    }

    async fn reactivate_conversation(&self, id: &str) -> DbResult<usize> {
        let id = id.to_owned();
        self.executor.run(move |db| db.reactivate_conversation(&id)).await
    }

    // -- message embeddings (independent table) — not supported on SQLite, PG only --

    async fn insert_message_embedding(&self, _message_id: i64, _session_id: &str, _embedding_vec: &[f32], _model_version: &str) -> DbResult<()> {
        Ok(()) // No-op on SQLite
    }
    async fn insert_message_embedding_skip(&self, _message_id: i64, _skip_reason: &str) -> DbResult<()> {
        Ok(())
    }
    async fn insert_message_embeddings_batch(&self, _entries: &[(i64, &str, Vec<f32>, &str)]) -> DbResult<usize> {
        Ok(0)
    }
    async fn insert_message_embedding_skips_batch(&self, _entries: &[(i64, &str)]) -> DbResult<usize> {
        Ok(0)
    }
    async fn messages_pending_embedding(&self, _cursor: i64, _limit: i64) -> DbResult<Vec<(i64, String, String, String)>> {
        Ok(vec![])
    }
    async fn message_embedding_stats(&self) -> DbResult<serde_json::Value> {
        Ok(serde_json::json!({"error": "SQLite backend does not support message embeddings"}))
    }

    // -- audit.rs: extraction watermarks --

    async fn get_pending_memory_messages(&self, today: &str) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        let today = today.to_owned();
        self.executor.run(move |db| db.get_pending_memory_messages(&today)).await
    }

    async fn update_memory_forwarded_at(&self, session_id: &str, timestamp: &str) -> DbResult<()> {
        let session_id = session_id.to_owned();
        let timestamp = timestamp.to_owned();
        self.executor.run(move |db| db.update_memory_forwarded_at(&session_id, &timestamp)).await
    }

    async fn get_pending_user_voice_messages(&self) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        self.executor.run(|db| db.get_pending_user_voice_messages()).await
    }

    async fn update_user_voice_forwarded_at(&self, session_id: &str, timestamp: &str) -> DbResult<()> {
        let session_id = session_id.to_owned();
        let timestamp = timestamp.to_owned();
        self.executor.run(move |db| db.update_user_voice_forwarded_at(&session_id, &timestamp)).await
    }

    async fn get_pending_realtime_messages(&self) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        self.executor.run(|db| db.get_pending_realtime_messages()).await
    }

    async fn get_pending_realtime_messages_with_limit(&self, limit: usize) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        self.executor.run(move |db| db.get_pending_realtime_messages_with_limit(limit)).await
    }

    async fn update_realtime_forwarded_at(&self, session_id: &str, timestamp: &str) -> DbResult<()> {
        let session_id = session_id.to_owned();
        let timestamp = timestamp.to_owned();
        self.executor.run(move |db| db.update_realtime_forwarded_at(&session_id, &timestamp)).await
    }

    // -- conversation_turns (S3 Tagger & Chunker) — SQLite deprecated --
    async fn get_last_turn_end_message_id(&self, _session_id: &str) -> DbResult<Option<i64>> { Ok(None) }
    async fn get_max_turn_idx(&self, _session_id: &str) -> DbResult<Option<i32>> { Ok(None) }
    async fn insert_conversation_turns_batch(&self, _session_id: &str, _base_idx: i32, _turns: &[RawTurn]) -> DbResult<usize> { Ok(0) }
    async fn insert_message_labels_batch(&self, _labels: &[(i64, &str, &str, &str)]) -> DbResult<usize> { Ok(0) }
    async fn sessions_pending_turn_extraction(&self, _limit: i64) -> DbResult<Vec<String>> { Ok(vec![]) }
    async fn sessions_recently_active_without_turns(&self, _since_minutes: i64, _limit: i64) -> DbResult<Vec<String>> { Ok(vec![]) }

    // -- S4 per-turn embedding — SQLite deprecated --
    async fn turns_pending_embedding(&self, _session_id: &str, _provider: &str) -> DbResult<Vec<ConversationTurn>> { Ok(vec![]) }
    async fn update_turn_topics_batch(&self, _updates: &[(i64, &str)]) -> DbResult<usize> { Ok(0) }
    async fn set_conversation_turn_vectors(&self, _session_id: &str, _vectors: &[(String, i32, Vec<f32>)], _provider: &str) -> DbResult<usize> { Ok(0) }
    async fn sessions_with_turns_but_no_vectors(&self, _provider: &str, _cursor: i64, _limit: i64) -> DbResult<Vec<String>> { Ok(vec![]) }

    // -- Phase 6: user_intents — SQLite deprecated --
    async fn insert_user_intent(&self, _session_id: &str, _turn_range_start: i32, _turn_range_end: i32, _intent_type: &str, _confidence: f32, _summary: Option<&str>, _context_json: Option<&str>, _related_goal_id: Option<&str>) -> DbResult<i64> { Ok(0) }
    async fn get_intent_coverage(&self, _session_id: &str) -> DbResult<Option<i32>> { Ok(None) }
    async fn get_turns_after(&self, _session_id: &str, _after_idx: i32) -> DbResult<Vec<ConversationTurn>> { Ok(vec![]) }
    async fn update_turns_intent_group(&self, _session_id: &str, _turn_range_start: i32, _turn_range_end: i32, _intent_id: i64) -> DbResult<()> { Ok(()) }
    async fn get_recent_intents(&self, _since_secs: i64) -> DbResult<Vec<UserIntent>> { Ok(vec![]) }
    async fn sessions_pending_intent_analysis(&self, _limit: i64) -> DbResult<Vec<String>> { Ok(vec![]) }
}
