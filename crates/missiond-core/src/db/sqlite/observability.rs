use async_trait::async_trait;
use super::SqliteMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::{ObservabilityStore, BackfillPhaseStatus};
use crate::types::*;
use std::collections::HashMap;

#[async_trait]
impl ObservabilityStore for SqliteMissionStore {
    // ── gemini_log.rs ─────────────────────────────────────────────

    async fn gemini_log_insert_started(&self, id: &str, caller: &str, session_id: Option<&str>, model: &str, prompt_chars: i64, prompt_text: Option<&str>) -> DbResult<()> {
        let id = id.to_owned();
        let caller = caller.to_owned();
        let session_id = session_id.map(|s| s.to_owned());
        let model = model.to_owned();
        let prompt_text = prompt_text.map(|s| s.to_owned());
        self.executor.run(move |db| db.gemini_log_insert_started(&id, &caller, session_id.as_deref(), &model, prompt_chars, prompt_text.as_deref())).await
    }

    async fn gemini_log_update_completed(&self, id: &str, api_mode: &str, response_chars: i64, queue_wait_ms: i64, duration_ms: i64, retry_count: i64, status: &str, error_msg: Option<&str>, response_text: Option<&str>) -> DbResult<()> {
        let id = id.to_owned();
        let api_mode = api_mode.to_owned();
        let status = status.to_owned();
        let error_msg = error_msg.map(|s| s.to_owned());
        let response_text = response_text.map(|s| s.to_owned());
        self.executor.run(move |db| db.gemini_log_update_completed(&id, &api_mode, response_chars, queue_wait_ms, duration_ms, retry_count, &status, error_msg.as_deref(), response_text.as_deref())).await
    }

    async fn gemini_log_get_content(&self, request_id: &str) -> DbResult<Option<serde_json::Value>> {
        let request_id = request_id.to_owned();
        self.executor.run(move |db| db.gemini_log_get_content(&request_id)).await
    }

    async fn gemini_log_query(&self, caller: Option<&str>, session_id: Option<&str>, status: Option<&str>, limit: i64) -> DbResult<Vec<serde_json::Value>> {
        let caller = caller.map(|s| s.to_owned());
        let session_id = session_id.map(|s| s.to_owned());
        let status = status.map(|s| s.to_owned());
        self.executor.run(move |db| db.gemini_log_query(caller.as_deref(), session_id.as_deref(), status.as_deref(), limit)).await
    }

    async fn gemini_log_stats(&self) -> DbResult<serde_json::Value> {
        self.executor.run(|db| db.gemini_log_stats()).await
    }

    async fn gemini_log_cleanup(&self, retention_days: i64) -> DbResult<i64> {
        self.executor.run(move |db| db.gemini_log_cleanup(retention_days)).await
    }

    async fn gemini_file_cache_get(&self, file_hash: &str) -> DbResult<Option<String>> {
        let file_hash = file_hash.to_owned();
        self.executor.run(move |db| db.gemini_file_cache_get(&file_hash)).await
    }

    async fn gemini_file_cache_put(&self, file_hash: &str, file_uri: &str, mime_type: &str, expires_at: i64) -> DbResult<()> {
        let file_hash = file_hash.to_owned();
        let file_uri = file_uri.to_owned();
        let mime_type = mime_type.to_owned();
        self.executor.run(move |db| db.gemini_file_cache_put(&file_hash, &file_uri, &mime_type, expires_at)).await
    }

    async fn gemini_file_cache_gc(&self) -> DbResult<i64> {
        self.executor.run(|db| db.gemini_file_cache_gc()).await
    }

    // ── incident.rs ───────────────────────────────────────────────

    async fn insert_incident(&self, id: &str, severity: &str, source: &str, title: &str, description: &str, server_id: Option<&str>, raw_payload: Option<&str>, board_task_id: Option<&str>, dedupe_key: &str) -> DbResult<()> {
        let id = id.to_owned();
        let severity = severity.to_owned();
        let source = source.to_owned();
        let title = title.to_owned();
        let description = description.to_owned();
        let server_id = server_id.map(|s| s.to_owned());
        let raw_payload = raw_payload.map(|s| s.to_owned());
        let board_task_id = board_task_id.map(|s| s.to_owned());
        let dedupe_key = dedupe_key.to_owned();
        self.executor.run(move |db| db.insert_incident(&id, &severity, &source, &title, &description, server_id.as_deref(), raw_payload.as_deref(), board_task_id.as_deref(), &dedupe_key)).await
    }

    async fn has_recent_incident(&self, dedupe_key: &str, window_secs: i64) -> DbResult<bool> {
        let dedupe_key = dedupe_key.to_owned();
        self.executor.run(move |db| db.has_recent_incident(&dedupe_key, window_secs)).await
    }

    async fn list_incidents(&self, limit: i64) -> DbResult<Vec<IncidentRow>> {
        self.executor.run(move |db| db.list_incidents(limit)).await
    }

    // ── incident.rs: token ledger ─────────────────────────────────

    async fn insert_token_usage(&self, conversation_id: &str, slot_id: Option<&str>, slot_task_id: Option<&str>, model: Option<&str>, input_tokens: i64, cache_creation_tokens: i64, cache_read_tokens: i64, output_tokens: i64, _message_id: Option<i64>) -> DbResult<()> {
        let conversation_id = conversation_id.to_owned();
        let slot_id = slot_id.map(|s| s.to_owned());
        let slot_task_id = slot_task_id.map(|s| s.to_owned());
        let model = model.map(|s| s.to_owned());
        self.executor.run(move |db| db.insert_token_usage(&conversation_id, slot_id.as_deref(), slot_task_id.as_deref(), model.as_deref(), input_tokens, cache_creation_tokens, cache_read_tokens, output_tokens)).await
    }

    async fn token_stats(&self, conversation_id: Option<&str>, slot_id: Option<&str>, since: Option<&str>, group_by: Option<&str>) -> DbResult<Vec<HashMap<String, serde_json::Value>>> {
        let conversation_id = conversation_id.map(|s| s.to_owned());
        let slot_id = slot_id.map(|s| s.to_owned());
        let since = since.map(|s| s.to_owned());
        let group_by = group_by.map(|s| s.to_owned());
        self.executor.run(move |db| db.token_stats(conversation_id.as_deref(), slot_id.as_deref(), since.as_deref(), group_by.as_deref())).await
    }

    // ── watermark.rs ──────────────────────────────────────────────

    async fn watermark_get(&self, consumer: &str, session_id: &str) -> DbResult<Option<(Option<i64>, Option<String>)>> {
        let consumer = consumer.to_owned();
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.watermark_get(&consumer, &session_id)).await
    }

    async fn watermark_advance_time(&self, consumer: &str, session_id: &str, timestamp: &str) -> DbResult<()> {
        let consumer = consumer.to_owned();
        let session_id = session_id.to_owned();
        let timestamp = timestamp.to_owned();
        self.executor.run(move |db| db.watermark_advance_time(&consumer, &session_id, &timestamp)).await
    }

    async fn watermark_advance_msg_id(&self, consumer: &str, session_id: &str, msg_id: i64) -> DbResult<()> {
        let consumer = consumer.to_owned();
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.watermark_advance_msg_id(&consumer, &session_id, msg_id)).await
    }

    async fn watermark_advance_full(&self, consumer: &str, session_id: &str, msg_id: Option<i64>, timestamp: Option<&str>, extra: Option<&str>) -> DbResult<()> {
        let consumer = consumer.to_owned();
        let session_id = session_id.to_owned();
        let timestamp = timestamp.map(|s| s.to_owned());
        let extra = extra.map(|s| s.to_owned());
        self.executor.run(move |db| db.watermark_advance_full(&consumer, &session_id, msg_id, timestamp.as_deref(), extra.as_deref())).await
    }

    async fn watermark_list(&self, consumer: &str) -> DbResult<Vec<(String, Option<i64>, Option<String>)>> {
        let consumer = consumer.to_owned();
        self.executor.run(move |db| db.watermark_list(&consumer)).await
    }

    // ── watermark.rs: labels ──────────────────────────────────────

    async fn label_set(&self, message_id: i64, label: &str, value: &str, source: &str) -> DbResult<()> {
        let label = label.to_owned();
        let value = value.to_owned();
        let source = source.to_owned();
        self.executor.run(move |db| db.label_set(message_id, &label, &value, &source)).await
    }

    async fn label_set_batch(&self, labels: &[(i64, &str, &str, &str)]) -> DbResult<usize> {
        let labels: Vec<(i64, String, String, String)> = labels.iter()
            .map(|(id, l, v, s)| (*id, l.to_string(), v.to_string(), s.to_string()))
            .collect();
        self.executor.run(move |db| {
            let refs: Vec<(i64, &str, &str, &str)> = labels.iter()
                .map(|(id, l, v, s)| (*id, l.as_str(), v.as_str(), s.as_str()))
                .collect();
            db.label_set_batch(&refs)
        }).await
    }

    async fn label_get(&self, message_id: i64) -> DbResult<Vec<(String, String, String)>> {
        self.executor.run(move |db| db.label_get(message_id)).await
    }

    async fn label_get_batch(&self, message_ids: &[i64]) -> DbResult<HashMap<i64, Vec<(String, String)>>> {
        let message_ids = message_ids.to_vec();
        self.executor.run(move |db| db.label_get_batch(&message_ids)).await
    }

    async fn label_find_messages(&self, label: &str, value: Option<&str>, limit: i64) -> DbResult<Vec<i64>> {
        let label = label.to_owned();
        let value = value.map(|s| s.to_owned());
        self.executor.run(move |db| db.label_find_messages(&label, value.as_deref(), limit)).await
    }

    // ── backfill.rs ───────────────────────────────────────────────

    async fn backfill_get_phase(&self, phase: &str) -> DbResult<Option<BackfillPhaseStatus>> {
        let phase = phase.to_owned();
        self.executor.run(move |db| db.backfill_get_phase(&phase)).await
    }

    async fn backfill_list_phases(&self) -> DbResult<Vec<BackfillPhaseStatus>> {
        self.executor.run(|db| db.backfill_list_phases()).await
    }

    async fn backfill_start_phase(&self, phase: &str, total_estimated: i64) -> DbResult<()> {
        let phase = phase.to_owned();
        self.executor.run(move |db| db.backfill_start_phase(&phase, total_estimated)).await
    }

    async fn backfill_update_progress(&self, phase: &str, new_cursor: i64, batch_success: i64, batch_failed: i64) -> DbResult<()> {
        let phase = phase.to_owned();
        self.executor.run(move |db| db.backfill_update_progress(&phase, new_cursor, batch_success, batch_failed)).await
    }

    async fn backfill_complete_phase(&self, phase: &str) -> DbResult<()> {
        let phase = phase.to_owned();
        self.executor.run(move |db| db.backfill_complete_phase(&phase)).await
    }

    async fn backfill_record_failure(&self, session_id: &str, phase: &str, error: &str) -> DbResult<()> {
        let session_id = session_id.to_owned();
        let phase = phase.to_owned();
        let error = error.to_owned();
        self.executor.run(move |db| db.backfill_record_failure(&session_id, &phase, &error)).await
    }

    async fn backfill_retryable_failures(&self, phase: &str, max_retries: i64, limit: i64) -> DbResult<Vec<String>> {
        let phase = phase.to_owned();
        self.executor.run(move |db| db.backfill_retryable_failures(&phase, max_retries, limit)).await
    }

    async fn backfill_retryable_failures_no_cooldown(&self, phase: &str, max_retries: i64) -> DbResult<i64> {
        let phase = phase.to_owned();
        self.executor.run(move |db| db.backfill_retryable_failures_no_cooldown(&phase, max_retries)).await
    }

    async fn backfill_clear_failure(&self, session_id: &str, phase: &str) -> DbResult<()> {
        let session_id = session_id.to_owned();
        let phase = phase.to_owned();
        self.executor.run(move |db| db.backfill_clear_failure(&session_id, &phase)).await
    }

    async fn conversations_missing_summary_cursor(&self, cursor: i64, limit: i64) -> DbResult<Vec<(i64, String)>> {
        self.executor.run(move |db| db.conversations_missing_summary_cursor(cursor, limit)).await
    }

    async fn conversations_needing_topic_vectors_cursor(&self, provider: &str, cursor: i64, limit: i64) -> DbResult<Vec<(i64, String)>> {
        let provider = provider.to_owned();
        self.executor.run(move |db| db.conversations_needing_topic_vectors_cursor(&provider, cursor, limit)).await
    }

    async fn conversations_missing_summary_count(&self) -> DbResult<i64> {
        self.executor.run(|db| db.conversations_missing_summary_count()).await
    }

    async fn conversations_needing_topic_vectors_count(&self, provider: &str) -> DbResult<i64> {
        let provider = provider.to_owned();
        self.executor.run(move |db| db.conversations_needing_topic_vectors_count(&provider)).await
    }

    // ── router_chat.rs ────────────────────────────────────────────

    async fn router_chat_get_or_create(&self, task_id: &str, model: &str) -> DbResult<String> {
        let task_id = task_id.to_owned();
        let model = model.to_owned();
        self.executor.run(move |db| db.router_chat_get_or_create(&task_id, &model)).await
    }

    async fn router_chat_load_history(&self, conv_id: &str) -> DbResult<Vec<serde_json::Value>> {
        let conv_id = conv_id.to_owned();
        self.executor.run(move |db| db.router_chat_load_history(&conv_id)).await
    }

    async fn router_chat_get_summary(&self, conv_id: &str) -> DbResult<(Option<String>, i64)> {
        let conv_id = conv_id.to_owned();
        self.executor.run(move |db| db.router_chat_get_summary(&conv_id)).await
    }

    async fn router_chat_load_active_history(&self, conv_id: &str, after_id: i64) -> DbResult<Vec<serde_json::Value>> {
        let conv_id = conv_id.to_owned();
        self.executor.run(move |db| db.router_chat_load_active_history(&conv_id, after_id)).await
    }

    async fn router_chat_unsummarized_count(&self, conv_id: &str, after_id: i64) -> DbResult<i64> {
        let conv_id = conv_id.to_owned();
        self.executor.run(move |db| db.router_chat_unsummarized_count(&conv_id, after_id)).await
    }

    async fn router_chat_load_compressible(&self, conv_id: &str, after_id: i64, batch_size: i64) -> DbResult<Vec<(i64, String, String)>> {
        let conv_id = conv_id.to_owned();
        self.executor.run(move |db| db.router_chat_load_compressible(&conv_id, after_id, batch_size)).await
    }

    async fn router_chat_update_summary(&self, conv_id: &str, new_summary: &str, new_cursor: i64, expected_old_cursor: i64) -> DbResult<bool> {
        let conv_id = conv_id.to_owned();
        let new_summary = new_summary.to_owned();
        self.executor.run(move |db| db.router_chat_update_summary(&conv_id, &new_summary, new_cursor, expected_old_cursor)).await
    }

    async fn router_chat_append_messages(&self, conv_id: &str, messages: &[(String, String)]) -> DbResult<()> {
        let conv_id = conv_id.to_owned();
        let messages = messages.to_vec();
        self.executor.run(move |db| db.router_chat_append_messages(&conv_id, &messages)).await
    }

    async fn jarvis_get_or_create(&self, conversation_id: Option<&str>) -> DbResult<String> {
        let conversation_id = conversation_id.map(|s| s.to_owned());
        self.executor.run(move |db| db.jarvis_get_or_create(conversation_id.as_deref())).await
    }

    async fn jarvis_save_exchange(&self, conv_id: &str, user_message: &str, assistant_message: &str) -> DbResult<()> {
        let conv_id = conv_id.to_owned();
        let user_message = user_message.to_owned();
        let assistant_message = assistant_message.to_owned();
        self.executor.run(move |db| db.jarvis_save_exchange(&conv_id, &user_message, &assistant_message)).await
    }

    async fn jarvis_update_title(&self, conv_id: &str, title: &str) -> DbResult<()> {
        let conv_id = conv_id.to_owned();
        let title = title.to_owned();
        self.executor.run(move |db| db.jarvis_update_title(&conv_id, &title)).await
    }

    async fn router_chat_list(&self, limit: i64) -> DbResult<Vec<serde_json::Value>> {
        self.executor.run(move |db| db.router_chat_list(limit)).await
    }

    async fn router_chat_stats(&self) -> DbResult<serde_json::Value> {
        self.executor.run(|db| db.router_chat_stats()).await
    }

    async fn router_chat_clear(&self, conversation_id: &str, count: Option<i64>) -> DbResult<(i64, i64)> {
        let conversation_id = conversation_id.to_owned();
        self.executor.run(move |db| db.router_chat_clear(&conversation_id, count)).await
    }

    async fn router_chat_clear_by_task(&self, task_id: &str, count: Option<i64>) -> DbResult<i64> {
        let task_id = task_id.to_owned();
        self.executor.run(move |db| db.router_chat_clear_by_task(&task_id, count)).await
    }

    async fn router_chat_delete_message(&self, message_id: i64) -> DbResult<Option<String>> {
        self.executor.run(move |db| db.router_chat_delete_message(message_id)).await
    }

    async fn router_chat_delete(&self, conversation_id: &str) -> DbResult<(i64, i64)> {
        let conversation_id = conversation_id.to_owned();
        self.executor.run(move |db| db.router_chat_delete(&conversation_id)).await
    }

    async fn router_chat_delete_by_task(&self, task_id: &str) -> DbResult<(i64, i64)> {
        let task_id = task_id.to_owned();
        self.executor.run(move |db| db.router_chat_delete_by_task(&task_id)).await
    }

    async fn router_chat_restore(&self, conversation_id: &str) -> DbResult<i64> {
        let conversation_id = conversation_id.to_owned();
        self.executor.run(move |db| db.router_chat_restore(&conversation_id)).await
    }

    // ── watcher_cursors (no-op in SQLite mode) ───────────────────
    async fn load_watcher_cursors(&self) -> DbResult<std::collections::HashMap<String, u64>> {
        Ok(std::collections::HashMap::new())
    }
    async fn upsert_watcher_cursors_batch(&self, _cursors: &std::collections::HashMap<String, u64>) -> DbResult<()> {
        Ok(())
    }
    async fn delete_watcher_cursor(&self, _file_path: &str) -> DbResult<()> {
        Ok(())
    }
}
