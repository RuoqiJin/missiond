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

    /// Load router_chat message history as OpenAI-format messages array.
    /// Full history load — used by callers that need everything (stats, export).
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

    /// Get rolling summary and cursor for a conversation.
    /// Returns (summary_text, last_summarized_msg_id).
    pub fn router_chat_get_summary(&self, conv_id: &str) -> DbResult<(Option<String>, i64)> {
        let conn = self.read_conn();
        let result: (Option<String>, i64) = conn.query_row(
            "SELECT rolling_summary, COALESCE(last_summarized_msg_id, 0)
             FROM conversations WHERE id = ?1",
            params![conv_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(result)
    }

    /// Load only active (unsummarized) messages — those after the summary cursor.
    pub fn router_chat_load_active_history(&self, conv_id: &str, after_id: i64) -> DbResult<Vec<serde_json::Value>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT role, content FROM conversation_messages
             WHERE session_id = ?1 AND id > ?2
             ORDER BY id ASC"
        )?;
        let msgs: Vec<serde_json::Value> = stmt.query_map(params![conv_id, after_id], |row| {
            let role: String = row.get(0)?;
            let content: String = row.get(1)?;
            Ok(serde_json::json!({"role": role, "content": content}))
        })?.filter_map(|r| r.ok()).collect();
        Ok(msgs)
    }

    /// Count unsummarized messages (for threshold check).
    pub fn router_chat_unsummarized_count(&self, conv_id: &str, after_id: i64) -> DbResult<i64> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT COUNT(*) FROM conversation_messages WHERE session_id = ?1 AND id > ?2",
            params![conv_id, after_id],
            |row| row.get(0),
        ).map_err(Into::into)
    }

    /// Load a batch of oldest unsummarized messages for compression.
    /// Returns Vec<(msg_id, role, content)> ordered by id ASC, limited to `batch_size`.
    pub fn router_chat_load_compressible(
        &self,
        conv_id: &str,
        after_id: i64,
        batch_size: i64,
    ) -> DbResult<Vec<(i64, String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, role, content FROM conversation_messages
             WHERE session_id = ?1 AND id > ?2
             ORDER BY id ASC LIMIT ?3"
        )?;
        let rows: Vec<(i64, String, String)> = stmt.query_map(
            params![conv_id, after_id, batch_size],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// Update rolling summary and advance the cursor (optimistic lock on cursor).
    pub fn router_chat_update_summary(
        &self,
        conv_id: &str,
        new_summary: &str,
        new_cursor: i64,
        expected_old_cursor: i64,
    ) -> DbResult<bool> {
        let conn = self.conn();
        let updated = conn.execute(
            "UPDATE conversations
             SET rolling_summary = ?1, last_summarized_msg_id = ?2
             WHERE id = ?3 AND COALESCE(last_summarized_msg_id, 0) = ?4",
            params![new_summary, new_cursor, conv_id, expected_old_cursor],
        )?;
        Ok(updated > 0)
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

    // ============ Jarvis UI Sessions ============

    /// Find or create a jarvis_ui conversation. If conversation_id is provided, reuse it.
    pub fn jarvis_get_or_create(&self, conversation_id: Option<&str>) -> DbResult<String> {
        let conn = self.conn();

        // Reuse existing conversation if ID provided
        if let Some(id) = conversation_id {
            let exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = ?1 AND source = 'jarvis_ui')",
                params![id],
                |row| row.get(0),
            )?;
            if exists {
                return Ok(id.to_string());
            }
        }

        // Create new
        let id = format!("jarvis-{}", uuid::Uuid::new_v4().simple());
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO conversations (id, source, model, chat_type, message_count, started_at, status, conversation_type)
             VALUES (?1, 'jarvis_ui', 'claude-code', 'jarvis', 0, ?2, 'active', 'jarvis')",
            params![id, now],
        )?;
        Ok(id)
    }

    /// Save user+assistant message pair to a jarvis conversation
    pub fn jarvis_save_exchange(
        &self,
        conv_id: &str,
        user_message: &str,
        assistant_message: &str,
    ) -> DbResult<()> {
        self.router_chat_append_messages(conv_id, &[
            ("user".to_string(), user_message.to_string()),
            ("assistant".to_string(), assistant_message.to_string()),
        ])
    }

    /// Update jarvis conversation title (first user message as title)
    pub fn jarvis_update_title(&self, conv_id: &str, title: &str) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET session_timeline = ?2 WHERE id = ?1",
            params![conv_id, title],
        )?;
        Ok(())
    }

    /// Phase 7: Find the most recent active Jarvis UI conversation.
    pub fn find_latest_jarvis_conversation(&self) -> DbResult<Option<String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id FROM conversations \
             WHERE source = 'jarvis_ui' AND status = 'active' \
             ORDER BY started_at DESC LIMIT 1"
        )?;
        let id = stmt.query_row([], |row| row.get::<_, String>(0)).ok();
        Ok(id)
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

    /// Archive + delete messages from a router_chat conversation.
    /// count: None = all messages, Some(n) = last n messages only.
    /// Returns (archived_count, remaining_count).
    pub fn router_chat_clear(&self, conversation_id: &str, count: Option<i64>) -> DbResult<(i64, i64)> {
        let conn = self.conn();
        let is_router: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM conversations WHERE id = ?1 AND chat_type = 'router_chat'",
            params![conversation_id],
            |row| row.get(0),
        )?;
        if !is_router {
            return Ok((0, 0));
        }
        let tx = conn.unchecked_transaction()?;
        // Determine which messages to archive
        let id_clause = if let Some(n) = count {
            // Last N messages by id DESC
            format!(
                "id IN (SELECT id FROM conversation_messages WHERE session_id = ?1 ORDER BY id DESC LIMIT {})",
                n
            )
        } else {
            "session_id = ?1".to_string()
        };
        // 1. Archive
        tx.execute(
            &format!(
                "INSERT INTO router_chat_archive (original_id, session_id, role, content, timestamp, archive_reason)
                 SELECT id, session_id, role, content, timestamp, 'clear'
                 FROM conversation_messages WHERE {}", id_clause
            ),
            params![conversation_id],
        )?;
        // 2. Delete
        let deleted = tx.execute(
            &format!("DELETE FROM conversation_messages WHERE {}", id_clause),
            params![conversation_id],
        )? as i64;
        // 3. Update message_count
        let remaining: i64 = tx.query_row(
            "SELECT COUNT(*) FROM conversation_messages WHERE session_id = ?1",
            params![conversation_id],
            |row| row.get(0),
        )?;
        tx.execute(
            "UPDATE conversations SET message_count = ?1 WHERE id = ?2",
            params![remaining, conversation_id],
        )?;
        tx.commit()?;
        Ok((deleted, remaining))
    }

    /// Archive + delete messages from all router_chat conversations linked to a task.
    /// count: None = all messages, Some(n) = last n messages from each conversation.
    /// Returns total archived count.
    pub fn router_chat_clear_by_task(&self, task_id: &str, count: Option<i64>) -> DbResult<i64> {
        let conn = self.conn();
        // Get all conversation IDs for this task
        let mut stmt = conn.prepare(
            "SELECT id FROM conversations WHERE task_id = ?1 AND chat_type = 'router_chat'"
        )?;
        let conv_ids: Vec<String> = stmt.query_map(params![task_id], |row| row.get(0))?
            .filter_map(|r| r.ok()).collect();
        drop(stmt);

        let mut total = 0i64;
        for cid in &conv_ids {
            let (archived, _) = self.router_chat_clear(cid, count)?;
            total += archived;
        }
        Ok(total)
    }

    /// Delete a single router_chat message by its ID. Archives first, then deletes.
    pub fn router_chat_delete_message(&self, message_id: i64) -> DbResult<Option<String>> {
        let conn = self.conn();
        // Verify it belongs to a router_chat conversation
        let session_id: Option<String> = conn.query_row(
            "SELECT cm.session_id FROM conversation_messages cm
             JOIN conversations c ON c.id = cm.session_id
             WHERE cm.id = ?1 AND c.chat_type = 'router_chat'",
            params![message_id],
            |row| row.get(0),
        ).optional()?;

        let session_id = match session_id {
            Some(sid) => sid,
            None => return Ok(None),
        };

        let tx = conn.unchecked_transaction()?;
        // Archive
        tx.execute(
            "INSERT INTO router_chat_archive (original_id, session_id, role, content, timestamp, archive_reason)
             SELECT id, session_id, role, content, timestamp, 'delete_message'
             FROM conversation_messages WHERE id = ?1",
            params![message_id],
        )?;
        // Delete
        tx.execute(
            "DELETE FROM conversation_messages WHERE id = ?1",
            params![message_id],
        )?;
        // Update message_count
        tx.execute(
            "UPDATE conversations SET message_count = (SELECT COUNT(*) FROM conversation_messages WHERE session_id = ?1) WHERE id = ?1",
            params![session_id],
        )?;
        tx.commit()?;
        Ok(Some(session_id))
    }

    /// Archive + delete a router_chat conversation and all its messages.
    pub fn router_chat_delete(&self, conversation_id: &str) -> DbResult<(i64, i64)> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        // Archive messages first
        tx.execute(
            "INSERT INTO router_chat_archive (original_id, session_id, role, content, timestamp, archive_reason)
             SELECT id, session_id, role, content, timestamp, 'delete'
             FROM conversation_messages WHERE session_id = ?1
             AND EXISTS (SELECT 1 FROM conversations WHERE id = ?1 AND chat_type = 'router_chat')",
            params![conversation_id],
        )?;
        let msg_deleted = tx.execute(
            "DELETE FROM conversation_messages WHERE session_id = ?1
             AND EXISTS (SELECT 1 FROM conversations WHERE id = ?1 AND chat_type = 'router_chat')",
            params![conversation_id],
        )? as i64;
        let conv_deleted = tx.execute(
            "DELETE FROM conversations WHERE id = ?1 AND chat_type = 'router_chat'",
            params![conversation_id],
        )? as i64;
        tx.commit()?;
        Ok((conv_deleted, msg_deleted))
    }

    /// Archive + delete all router_chat conversations linked to a task.
    pub fn router_chat_delete_by_task(&self, task_id: &str) -> DbResult<(i64, i64)> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        // Archive all messages
        tx.execute(
            "INSERT INTO router_chat_archive (original_id, session_id, role, content, timestamp, archive_reason)
             SELECT m.id, m.session_id, m.role, m.content, m.timestamp, 'delete'
             FROM conversation_messages m
             INNER JOIN conversations c ON c.id = m.session_id
             WHERE c.task_id = ?1 AND c.chat_type = 'router_chat'",
            params![task_id],
        )?;
        let msg_deleted = tx.execute(
            "DELETE FROM conversation_messages WHERE session_id IN
             (SELECT id FROM conversations WHERE task_id = ?1 AND chat_type = 'router_chat')",
            params![task_id],
        )? as i64;
        let conv_deleted = tx.execute(
            "DELETE FROM conversations WHERE task_id = ?1 AND chat_type = 'router_chat'",
            params![task_id],
        )? as i64;
        tx.commit()?;
        Ok((conv_deleted, msg_deleted))
    }

    /// Restore archived messages back to conversation_messages.
    /// Returns count of restored messages.
    pub fn router_chat_restore(&self, conversation_id: &str) -> DbResult<i64> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let restored = tx.execute(
            "INSERT INTO conversation_messages (session_id, role, content, timestamp)
             SELECT session_id, role, content, timestamp
             FROM router_chat_archive WHERE session_id = ?1
             ORDER BY original_id ASC",
            params![conversation_id],
        )? as i64;
        if restored > 0 {
            tx.execute(
                "DELETE FROM router_chat_archive WHERE session_id = ?1",
                params![conversation_id],
            )?;
            tx.execute(
                "UPDATE conversations SET message_count = (SELECT COUNT(*) FROM conversation_messages WHERE session_id = ?1) WHERE id = ?1",
                params![conversation_id],
            )?;
        }
        tx.commit()?;
        Ok(restored)
    }


}
