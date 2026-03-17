use async_trait::async_trait;
use super::SqliteMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::MessageStore;
use crate::types::*;

#[async_trait]
impl MessageStore for SqliteMissionStore {
    async fn insert_conversation_message(&self, msg: &ConversationMessage) -> DbResult<i64> {
        let msg = msg.clone();
        self.executor.run(move |db| db.insert_conversation_message(&msg)).await
    }

    async fn insert_conversation_messages_batch(&self, messages: &[ConversationMessage]) -> DbResult<Vec<i64>> {
        let messages = messages.to_vec();
        self.executor.run(move |db| db.insert_conversation_messages_batch(&messages)).await
    }

    async fn get_conversation_message_by_id(&self, id: i64) -> DbResult<Option<ConversationMessage>> {
        self.executor.run(move |db| db.get_conversation_message_by_id(id)).await
    }

    async fn get_conversation_messages(&self, session_id: &str, since_id: Option<i64>, limit: i64) -> DbResult<Vec<ConversationMessage>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_conversation_messages(&session_id, since_id, limit)).await
    }

    async fn get_messages_around(&self, message_id: i64, before: i64, after: i64) -> DbResult<Option<(String, Vec<ConversationMessage>)>> {
        self.executor.run(move |db| db.get_messages_around(message_id, before, after)).await
    }

    async fn search_conversation_messages(&self, query: &str, limit: i64) -> DbResult<Vec<ConversationMessage>> {
        let query = query.to_owned();
        self.executor.run(move |db| db.search_conversation_messages(&query, limit)).await
    }

    async fn search_messages_filtered(&self, query: &str, session_id: Option<&str>, role: Option<&str>, tool_name: Option<&str>, time_after: Option<&str>, limit: i64) -> DbResult<Vec<ConversationMessage>> {
        let query = query.to_owned();
        let session_id = session_id.map(|s| s.to_owned());
        let role = role.map(|s| s.to_owned());
        let tool_name = tool_name.map(|s| s.to_owned());
        let time_after = time_after.map(|s| s.to_owned());
        self.executor.run(move |db| db.search_messages_filtered(&query, session_id.as_deref(), role.as_deref(), tool_name.as_deref(), time_after.as_deref(), limit)).await
    }

    async fn search_conversation_sessions_fts(&self, query: &str, limit: i64) -> DbResult<Vec<(String, f64)>> {
        let query = query.to_owned();
        self.executor.run(move |db| db.search_conversation_sessions_fts(&query, limit)).await
    }

    async fn get_session_fts_snippets(&self, session_id: &str, query: &str, limit: usize) -> DbResult<Vec<(String, String)>> {
        let session_id = session_id.to_owned();
        let query = query.to_owned();
        self.executor.run(move |db| db.get_session_fts_snippets(&session_id, &query, limit)).await
    }

    async fn search_conversation_sessions_fts_filtered(&self, query: &str, limit: i64, time_after: Option<&str>, project: Option<&str>) -> DbResult<Vec<(String, f64)>> {
        let query = query.to_owned();
        let time_after = time_after.map(|s| s.to_owned());
        let project = project.map(|s| s.to_owned());
        self.executor.run(move |db| db.search_conversation_sessions_fts_filtered(&query, limit, time_after.as_deref(), project.as_deref())).await
    }

    async fn get_first_user_message(&self, session_id: &str) -> DbResult<Option<String>> {
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.get_first_user_message(&session_id)).await
    }
}
