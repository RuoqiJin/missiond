use rusqlite::{params, OptionalExtension};
use super::error::DbResult;
use super::MissionDB;

impl MissionDB {
    // ============ Router Chat Sessions ============

    /// Find or create a router_chat conversation for a given task_id
    pub fn router_chat_get_or_create(&self, task_id: &str, model: &str) -> DbResult<String> {
        let conn = self.conn();
        // Find existing active router_chat conversation for this task
        let existing: Option<String> = conn.query_row(
            "SELECT id FROM conversations WHERE task_id = ?1 AND chat_type = 'router_chat' AND status = 'active' LIMIT 1",
            params![task_id],
            |row| row.get(0),
        ).optional()?;

        if let Some(id) = existing {
            return Ok(id);
        }

        // Create new
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO conversations (id, source, model, task_id, chat_type, message_count, started_at, status)
             VALUES (?1, 'router_chat', ?2, ?3, 'router_chat', 0, ?4, 'active')",
            params![id, model, task_id, now],
        )?;
        Ok(id)
    }

    /// Load router_chat message history as OpenAI-format messages array
    pub fn router_chat_load_history(&self, conv_id: &str) -> DbResult<Vec<serde_json::Value>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT role, content FROM conversation_messages WHERE session_id = ?1 ORDER BY id ASC"
        )?;
        let msgs: Vec<serde_json::Value> = stmt.query_map(params![conv_id], |row| {
            let role: String = row.get(0)?;
            let content: String = row.get(1)?;
            Ok(serde_json::json!({"role": role, "content": content}))
        })?.filter_map(|r| r.ok()).collect();
        Ok(msgs)
    }

    /// Append messages to a router_chat conversation
    pub fn router_chat_append_messages(
        &self,
        conv_id: &str,
        messages: &[(String, String)], // (role, content) pairs
    ) -> DbResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        for (role, content) in messages {
            conn.execute(
                "INSERT INTO conversation_messages (session_id, role, content, timestamp)
                 VALUES (?1, ?2, ?3, ?4)",
                params![conv_id, role, content, now],
            )?;
        }
        // Update message count
        conn.execute(
            "UPDATE conversations SET message_count = (SELECT COUNT(*) FROM conversation_messages WHERE session_id = ?1) WHERE id = ?1",
            params![conv_id],
        )?;
        Ok(())
    }

    /// List all router_chat conversations with message size stats
    pub fn router_chat_list(&self, limit: i64) -> DbResult<Vec<serde_json::Value>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT c.id, c.task_id, c.model, c.message_count, c.started_at, c.status,
                    COALESCE((SELECT SUM(LENGTH(m.content)) FROM conversation_messages m WHERE m.session_id = c.id), 0) as total_chars
             FROM conversations c
             WHERE c.chat_type = 'router_chat'
             ORDER BY c.started_at DESC
             LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            let total_chars: i64 = row.get(6)?;
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "task_id": row.get::<_, Option<String>>(1)?,
                "model": row.get::<_, Option<String>>(2)?,
                "message_count": row.get::<_, i64>(3)?,
                "started_at": row.get::<_, String>(4)?,
                "status": row.get::<_, String>(5)?,
                "total_chars": total_chars,
                "estimated_tokens": total_chars / 4,
            }))
        })?;
        Ok(rows.collect::<Result<Vec<_>, rusqlite::Error>>()?)
    }

    /// Aggregate stats for all router_chat conversations
    pub fn router_chat_stats(&self) -> DbResult<serde_json::Value> {
        let conn = self.read_conn();

        let (total_convs, total_msgs, total_chars): (i64, i64, i64) = conn.query_row(
            "SELECT COUNT(DISTINCT c.id),
                    COUNT(m.id),
                    COALESCE(SUM(LENGTH(m.content)), 0)
             FROM conversations c
             LEFT JOIN conversation_messages m ON m.session_id = c.id
             WHERE c.chat_type = 'router_chat'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;

        // By model
        let mut model_stmt = conn.prepare(
            "SELECT COALESCE(c.model, 'unknown'), COUNT(DISTINCT c.id), COUNT(m.id), COALESCE(SUM(LENGTH(m.content)), 0)
             FROM conversations c
             LEFT JOIN conversation_messages m ON m.session_id = c.id
             WHERE c.chat_type = 'router_chat'
             GROUP BY c.model"
        )?;
        let by_model: Vec<serde_json::Value> = model_stmt.query_map([], |row| {
            let chars: i64 = row.get(3)?;
            Ok(serde_json::json!({
                "model": row.get::<_, String>(0)?,
                "conversations": row.get::<_, i64>(1)?,
                "messages": row.get::<_, i64>(2)?,
                "total_chars": chars,
                "estimated_tokens": chars / 4,
            }))
        })?.filter_map(|r| r.ok()).collect();

        // By day (last 30 days)
        let mut day_stmt = conn.prepare(
            "SELECT DATE(m.timestamp) as day, COUNT(*) as msg_count, SUM(LENGTH(m.content)) as chars
             FROM conversation_messages m
             INNER JOIN conversations c ON c.id = m.session_id
             WHERE c.chat_type = 'router_chat' AND m.timestamp >= DATE('now', '-30 days')
             GROUP BY day ORDER BY day DESC"
        )?;
        let by_day: Vec<serde_json::Value> = day_stmt.query_map([], |row| {
            let chars: i64 = row.get(2)?;
            Ok(serde_json::json!({
                "day": row.get::<_, String>(0)?,
                "messages": row.get::<_, i64>(1)?,
                "total_chars": chars,
                "estimated_tokens": chars / 4,
            }))
        })?.filter_map(|r| r.ok()).collect();

        Ok(serde_json::json!({
            "total_conversations": total_convs,
            "total_messages": total_msgs,
            "total_chars": total_chars,
            "estimated_tokens": total_chars / 4,
            "by_model": by_model,
            "by_day": by_day,
        }))
    }

    /// Clear all messages from a router_chat conversation (keep the conversation record)
    pub fn router_chat_clear(&self, conversation_id: &str) -> DbResult<i64> {
        let conn = self.conn();
        let is_router: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM conversations WHERE id = ?1 AND chat_type = 'router_chat'",
            params![conversation_id],
            |row| row.get(0),
        )?;
        if !is_router {
            return Ok(0);
        }
        let deleted = conn.execute(
            "DELETE FROM conversation_messages WHERE session_id = ?1",
            params![conversation_id],
        )? as i64;
        conn.execute(
            "UPDATE conversations SET message_count = 0 WHERE id = ?1",
            params![conversation_id],
        )?;
        Ok(deleted)
    }

    /// Clear all messages from router_chat conversations linked to a task
    pub fn router_chat_clear_by_task(&self, task_id: &str) -> DbResult<i64> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM conversation_messages WHERE session_id IN
             (SELECT id FROM conversations WHERE task_id = ?1 AND chat_type = 'router_chat')",
            params![task_id],
        )? as i64;
        conn.execute(
            "UPDATE conversations SET message_count = 0
             WHERE task_id = ?1 AND chat_type = 'router_chat'",
            params![task_id],
        )?;
        Ok(deleted)
    }

    /// Delete a router_chat conversation and all its messages
    pub fn router_chat_delete(&self, conversation_id: &str) -> DbResult<(i64, i64)> {
        let conn = self.conn();
        let msg_deleted = conn.execute(
            "DELETE FROM conversation_messages WHERE session_id = ?1
             AND EXISTS (SELECT 1 FROM conversations WHERE id = ?1 AND chat_type = 'router_chat')",
            params![conversation_id],
        )? as i64;
        let conv_deleted = conn.execute(
            "DELETE FROM conversations WHERE id = ?1 AND chat_type = 'router_chat'",
            params![conversation_id],
        )? as i64;
        Ok((conv_deleted, msg_deleted))
    }

    /// Delete all router_chat conversations linked to a task
    pub fn router_chat_delete_by_task(&self, task_id: &str) -> DbResult<(i64, i64)> {
        let conn = self.conn();
        let msg_deleted = conn.execute(
            "DELETE FROM conversation_messages WHERE session_id IN
             (SELECT id FROM conversations WHERE task_id = ?1 AND chat_type = 'router_chat')",
            params![task_id],
        )? as i64;
        let conv_deleted = conn.execute(
            "DELETE FROM conversations WHERE task_id = ?1 AND chat_type = 'router_chat'",
            params![task_id],
        )? as i64;
        Ok((conv_deleted, msg_deleted))
    }


}
