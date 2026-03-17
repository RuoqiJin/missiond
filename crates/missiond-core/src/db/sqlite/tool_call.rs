use async_trait::async_trait;
use super::SqliteMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::ToolCallStore;
use crate::types::*;
use std::collections::HashSet;

#[async_trait]
impl ToolCallStore for SqliteMissionStore {
    async fn insert_tool_call(&self, tc: &ToolCallRecord) -> DbResult<()> {
        let tc = tc.clone();
        self.executor.run(move |db| db.insert_tool_call(&tc)).await
    }

    async fn insert_tool_calls_batch(&self, calls: &[ToolCallRecord]) -> DbResult<usize> {
        let calls = calls.to_vec();
        self.executor.run(move |db| db.insert_tool_calls_batch(&calls)).await
    }

    async fn update_tool_call_output(&self, tool_use_id: &str, output_summary: &str, raw_output: &str, status: &str) -> DbResult<bool> {
        let tool_use_id = tool_use_id.to_owned();
        let output_summary = output_summary.to_owned();
        let raw_output = raw_output.to_owned();
        let status = status.to_owned();
        self.executor.run(move |db| db.update_tool_call_output(&tool_use_id, &output_summary, &raw_output, &status)).await
    }

    async fn get_tool_calls_by_session(&self, session_id: &str, tool_filter: Option<&[String]>, limit: i64) -> DbResult<Vec<ToolCallRecord>> {
        let session_id = session_id.to_owned();
        let tool_filter = tool_filter.map(|s| s.to_vec());
        self.executor.run(move |db| db.get_tool_calls_by_session(&session_id, tool_filter.as_deref(), limit)).await
    }

    async fn get_tool_call_by_id(&self, tool_use_id: &str) -> DbResult<Option<ToolCallRecord>> {
        let tool_use_id = tool_use_id.to_owned();
        self.executor.run(move |db| db.get_tool_call_by_id(&tool_use_id)).await
    }

    async fn get_tool_call_stats(&self, session_id: &str) -> DbResult<Vec<(String, i64, i64, i64)>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_tool_call_stats(&session_id)).await
    }

    async fn count_pending_tool_calls(&self) -> DbResult<i64> {
        self.executor.run(move |db| db.count_pending_tool_calls()).await
    }

    async fn get_sessions_with_pending_tool_calls(&self) -> DbResult<Vec<String>> {
        self.executor.run(move |db| db.get_sessions_with_pending_tool_calls()).await
    }

    async fn get_sessions_with_tool_calls(&self) -> DbResult<HashSet<String>> {
        self.executor.run(move |db| db.get_sessions_with_tool_calls()).await
    }

    async fn get_messages_for_tool_call_backfill(&self, session_id: &str) -> DbResult<Vec<(String, String, String)>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_messages_for_tool_call_backfill(&session_id)).await
    }

    async fn get_conversations_with_jsonl(&self) -> DbResult<Vec<(String, String)>> {
        self.executor.run(move |db| db.get_conversations_with_jsonl()).await
    }

    // -- Retrospective tool analysis --

    async fn get_retrospective_tool_stats(&self, session_id: &str, limit: i64) -> DbResult<Vec<(String, i64, i64, i64, f64)>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_retrospective_tool_stats(&session_id, limit)).await
    }

    async fn get_retrospective_meta(&self, session_id: &str) -> DbResult<(i64, i64, i64, i64)> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_retrospective_meta(&session_id)).await
    }

    async fn get_retrospective_repeat_patterns(&self, session_id: &str, min_streak: i64) -> DbResult<Vec<(String, i64, String, String)>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_retrospective_repeat_patterns(&session_id, min_streak)).await
    }

    async fn get_tool_name_sequence(&self, session_id: &str) -> DbResult<Vec<String>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_tool_name_sequence(&session_id)).await
    }

    async fn get_retrospective_high_error_tools(&self, session_id: &str, min_error_rate: f64) -> DbResult<Vec<(String, f64, i64)>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_retrospective_high_error_tools(&session_id, min_error_rate)).await
    }

    async fn get_tool_error_samples(&self, session_id: &str, tool_name: &str) -> DbResult<Vec<(String, String, String)>> {
        let session_id = session_id.to_owned();
        let tool_name = tool_name.to_owned();
        self.executor.run(move |db| db.get_tool_error_samples(&session_id, &tool_name)).await
    }

    async fn get_tool_calls_for_detailed_analysis(&self, session_id: &str) -> DbResult<Vec<(String, String, String, String)>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_tool_calls_for_detailed_analysis(&session_id)).await
    }

    async fn get_tool_calls_with_status_timeline(&self, session_id: &str) -> DbResult<Vec<(String, String, String, String)>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_tool_calls_with_status_timeline(&session_id)).await
    }
}
