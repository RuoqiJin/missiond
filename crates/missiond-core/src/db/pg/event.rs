//! EventStore — PostgreSQL implementation.

use std::collections::HashSet;
use async_trait::async_trait;
use crate::db::error::DbResult;
use crate::db::traits::EventStore;
use crate::types::*;
use super::PgMissionStore;

#[cfg(feature = "postgres")]
#[async_trait]
impl EventStore for PgMissionStore {
    async fn insert_conversation_events_batch(&self, events: &[ConversationEvent]) -> DbResult<usize> {
        if events.is_empty() {
            return Ok(0);
        }
        let mut count = 0usize;
        for event in events {
            let result = sqlx::query(
                "INSERT INTO conversation_events (session_id, event_uuid, event_type, content, raw_data, timestamp)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (session_id, event_uuid) DO NOTHING"
            )
            .bind(&event.session_id)
            .bind(&event.event_uuid)
            .bind(&event.event_type)
            .bind(&event.content)
            .bind(&event.raw_data)
            .bind(&event.timestamp)
            .execute(&self.pool)
            .await?;
            if result.rows_affected() > 0 {
                count += 1;
            }
        }
        Ok(count)
    }

    async fn get_conversation_events(&self, session_id: &str, event_type: Option<&str>, limit: i64) -> DbResult<Vec<ConversationEvent>> {
        let rows = if let Some(et) = event_type {
            sqlx::query(
                "SELECT id, session_id, event_uuid, event_type, content, raw_data, timestamp
                 FROM conversation_events WHERE session_id = $1 AND event_type = $2
                 ORDER BY id ASC LIMIT $3"
            )
            .bind(session_id)
            .bind(et)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, session_id, event_uuid, event_type, content, raw_data, timestamp
                 FROM conversation_events WHERE session_id = $1
                 ORDER BY id ASC LIMIT $2"
            )
            .bind(session_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };

        let results: Vec<ConversationEvent> = rows.iter().map(|row| {
            use sqlx::Row;
            ConversationEvent {
                id: row.get("id"),
                session_id: row.get("session_id"),
                event_uuid: row.get("event_uuid"),
                event_type: row.get("event_type"),
                content: row.get("content"),
                raw_data: row.get("raw_data"),
                timestamp: row.get("timestamp"),
            }
        }).collect();
        Ok(results)
    }

    async fn is_compact_boundary_event(&self, session_id: &str, event_uuid: &str) -> DbResult<bool> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversation_events
             WHERE session_id = $1 AND event_uuid = $2 AND event_type = 'compact_boundary'"
        )
        .bind(session_id)
        .bind(event_uuid)
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    async fn get_agent_trajectory(&self, tool_use_id: &str, limit: i64) -> DbResult<Vec<ConversationMessage>> {
        let rows = sqlx::query(
            "SELECT id, session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata,
                    tool_name, raw_role, content_types, has_image, has_tool_use, has_tool_result, token_count
             FROM conversation_messages WHERE parent_uuid = $1 AND role LIKE 'agent_%'
             ORDER BY id ASC LIMIT $2"
        )
        .bind(tool_use_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let results: Vec<ConversationMessage> = rows.iter().map(|row| {
            use sqlx::Row;
            ConversationMessage {
                id: row.get("id"),
                session_id: row.get("session_id"),
                role: row.get("role"),
                content: row.get("content"),
                raw_content: row.get("raw_content"),
                message_uuid: row.get("message_uuid"),
                parent_uuid: row.get("parent_uuid"),
                model: row.get("model"),
                timestamp: row.get("timestamp"),
                metadata: row.get("metadata"),
                tool_name: row.get("tool_name"),
                raw_role: row.get("raw_role"),
                content_types: row.get("content_types"),
                has_image: row.get::<bool, _>("has_image"),
                has_tool_use: row.get::<bool, _>("has_tool_use"),
                has_tool_result: row.get::<bool, _>("has_tool_result"),
                token_count: row.get("token_count"),
                seq: None,
                role_display: None,
            }
        }).collect();
        Ok(results)
    }

    async fn get_event_type_summary(&self, session_id: Option<&str>) -> DbResult<Vec<(String, i64)>> {
        let rows = if let Some(sid) = session_id {
            sqlx::query(
                "SELECT event_type, COUNT(*) as cnt FROM conversation_events
                 WHERE session_id = $1 GROUP BY event_type ORDER BY cnt DESC"
            )
            .bind(sid)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT event_type, COUNT(*) as cnt FROM conversation_events
                 GROUP BY event_type ORDER BY cnt DESC"
            )
            .fetch_all(&self.pool)
            .await?
        };

        let results: Vec<(String, i64)> = rows.iter().map(|row| {
            use sqlx::Row;
            (row.get::<String, _>("event_type"), row.get::<i64, _>("cnt"))
        }).collect();
        Ok(results)
    }

    async fn cleanup_old_events(&self, cutoff: &str) -> DbResult<usize> {
        let result = sqlx::query(
            "DELETE FROM conversation_events
             WHERE event_type IN ('progress:bash_progress', 'progress:mcp_progress', 'hook_progress', 'progress:waiting_for_task')
             AND timestamp < $1"
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn get_sessions_with_events(&self) -> DbResult<HashSet<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT session_id FROM conversation_events"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }
}
