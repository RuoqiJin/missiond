use std::collections::HashSet;
use async_trait::async_trait;
use super::SqliteMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::EventStore;
use crate::types::*;

#[async_trait]
impl EventStore for SqliteMissionStore {
    async fn insert_conversation_events_batch(&self, events: &[ConversationEvent]) -> DbResult<usize> {
        let events = events.to_vec();
        self.executor.run(move |db| db.insert_conversation_events_batch(&events)).await
    }

    async fn get_conversation_events(&self, session_id: &str, event_type: Option<&str>, limit: i64) -> DbResult<Vec<ConversationEvent>> {
        let session_id = session_id.to_owned();
        let event_type = event_type.map(|s| s.to_owned());
        self.executor.run(move |db| db.get_conversation_events(&session_id, event_type.as_deref(), limit)).await
    }

    async fn is_compact_boundary_event(&self, session_id: &str, event_uuid: &str) -> DbResult<bool> {
        let session_id = session_id.to_owned();
        let event_uuid = event_uuid.to_owned();
        self.executor.run(move |db| db.is_compact_boundary_event(&session_id, &event_uuid)).await
    }

    async fn get_agent_trajectory(&self, tool_use_id: &str, limit: i64) -> DbResult<Vec<ConversationMessage>> {
        let tool_use_id = tool_use_id.to_owned();
        self.executor.run(move |db| db.get_agent_trajectory(&tool_use_id, limit)).await
    }

    async fn get_event_type_summary(&self, session_id: Option<&str>) -> DbResult<Vec<(String, i64)>> {
        let session_id = session_id.map(|s| s.to_owned());
        self.executor.run(move |db| db.get_event_type_summary(session_id.as_deref())).await
    }

    async fn cleanup_old_events(&self, cutoff: &str) -> DbResult<usize> {
        let cutoff = cutoff.to_owned();
        self.executor.run(move |db| db.cleanup_old_events(&cutoff)).await
    }

    async fn get_sessions_with_events(&self) -> DbResult<HashSet<String>> {
        self.executor.run(|db| db.get_sessions_with_events()).await
    }
}
