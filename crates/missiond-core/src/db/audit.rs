use rusqlite::params;
use super::error::DbResult;
use crate::types::*;
use super::MissionDB;

impl MissionDB {
    // ============ Conversation Tool Calls (Audit) ============

    /// Insert a tool call record (from tool_use block in assistant message)
    pub fn insert_tool_call(&self, tc: &ToolCallRecord) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR IGNORE INTO conversation_tool_calls (id, session_id, message_id, tool_name, input_summary, raw_input, output_summary, raw_output, status, duration_ms, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                tc.id, tc.session_id, tc.message_id, tc.tool_name,
                tc.input_summary, tc.raw_input, tc.output_summary, tc.raw_output,
                tc.status, tc.duration_ms, tc.timestamp,
            ],
        )?;
        Ok(())
    }

    /// Batch insert tool call records
    pub fn insert_tool_calls_batch(&self, calls: &[ToolCallRecord]) -> DbResult<usize> {
        if calls.is_empty() {
            return Ok(0);
        }
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let mut count = 0usize;
        for tc in calls {
            tx.execute(
                "INSERT OR IGNORE INTO conversation_tool_calls (id, session_id, message_id, tool_name, input_summary, raw_input, output_summary, raw_output, status, duration_ms, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    tc.id, tc.session_id, tc.message_id, tc.tool_name,
                    tc.input_summary, tc.raw_input, tc.output_summary, tc.raw_output,
                    tc.status, tc.duration_ms, tc.timestamp,
                ],
            )?;
            if tx.changes() > 0 {
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    /// Update tool call with output (from tool_result block in user message)
    pub fn update_tool_call_output(&self, tool_use_id: &str, output_summary: &str, raw_output: &str, status: &str) -> DbResult<bool> {
        let conn = self.conn();
        let changes = conn.execute(
            "UPDATE conversation_tool_calls SET output_summary = ?1, raw_output = ?2, status = ?3 WHERE id = ?4",
            params![output_summary, raw_output, status, tool_use_id],
        )?;
        Ok(changes > 0)
    }

    /// Get tool calls for a session (for audit trace)
    pub fn get_tool_calls_by_session(&self, session_id: &str, tool_filter: Option<&[String]>, limit: i64) -> DbResult<Vec<ToolCallRecord>> {
        let conn = self.read_conn();
        let mut calls = Vec::new();
        if let Some(filter) = tool_filter {
            if filter.is_empty() {
                return Ok(calls);
            }
            let placeholders: Vec<String> = (0..filter.len()).map(|i| format!("?{}", i + 2)).collect();
            let sql = format!(
                "SELECT * FROM conversation_tool_calls WHERE session_id = ?1 AND tool_name IN ({}) ORDER BY rowid ASC LIMIT ?{}",
                placeholders.join(","),
                filter.len() + 2
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            params_vec.push(Box::new(session_id.to_string()));
            for f in filter {
                params_vec.push(Box::new(f.clone()));
            }
            params_vec.push(Box::new(limit));
            let refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
            let rows = stmt.query_map(refs.as_slice(), |row| Self::row_to_tool_call(row))?;
            for r in rows { calls.push(r?); }
        } else {
            let mut stmt = conn.prepare(
                "SELECT * FROM conversation_tool_calls WHERE session_id = ?1 ORDER BY rowid ASC LIMIT ?2"
            )?;
            let rows = stmt.query_map(params![session_id, limit], |row| Self::row_to_tool_call(row))?;
            for r in rows { calls.push(r?); }
        }
        Ok(calls)
    }

    /// Get a single tool call by ID (for audit detail drilldown)
    pub fn get_tool_call_by_id(&self, tool_use_id: &str) -> DbResult<Option<ToolCallRecord>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare("SELECT * FROM conversation_tool_calls WHERE id = ?1")?;
        let mut rows = stmt.query(params![tool_use_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_tool_call(row)?))
        } else {
            Ok(None)
        }
    }

    /// Get tool call statistics for a session
    pub fn get_tool_call_stats(&self, session_id: &str) -> DbResult<Vec<(String, i64, i64, i64)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT tool_name,
                    COUNT(*) as total,
                    SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) as success_count,
                    SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END) as error_count
             FROM conversation_tool_calls WHERE session_id = ?1
             GROUP BY tool_name ORDER BY total DESC"
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut stats = Vec::new();
        for r in rows { stats.push(r?); }
        Ok(stats)
    }

    fn row_to_tool_call(row: &rusqlite::Row) -> rusqlite::Result<ToolCallRecord> {
        Ok(ToolCallRecord {
            id: row.get("id")?,
            session_id: row.get("session_id")?,
            message_id: row.get("message_id")?,
            tool_name: row.get("tool_name")?,
            input_summary: row.get("input_summary")?,
            raw_input: row.get("raw_input")?,
            output_summary: row.get("output_summary")?,
            raw_output: row.get("raw_output")?,
            status: row.get("status")?,
            duration_ms: row.get("duration_ms")?,
            timestamp: row.get("timestamp")?,
        })
    }

    /// Batch insert conversation events (system events from JSONL: turn_duration, etc.)
    pub fn insert_conversation_events_batch(&self, events: &[crate::types::ConversationEvent]) -> DbResult<usize> {
        if events.is_empty() {
            return Ok(0);
        }
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let mut count = 0usize;
        for event in events {
            tx.execute(
                "INSERT INTO conversation_events (session_id, event_type, content, raw_data, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![event.session_id, event.event_type, event.content, event.raw_data, event.timestamp],
            )?;
            count += 1;
        }
        tx.commit()?;
        Ok(count)
    }

    /// Get conversation events for a session, optionally filtered by event_type
    pub fn get_conversation_events(
        &self,
        session_id: &str,
        event_type: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<crate::types::ConversationEvent>> {
        let conn = self.read_conn();
        let mut events = Vec::new();
        if let Some(et) = event_type {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, event_type, content, raw_data, timestamp
                 FROM conversation_events WHERE session_id = ?1 AND event_type = ?2
                 ORDER BY id ASC LIMIT ?3"
            )?;
            let rows = stmt.query_map(params![session_id, et, limit], |row| {
                Ok(crate::types::ConversationEvent {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    event_type: row.get(2)?,
                    content: row.get(3)?,
                    raw_data: row.get(4)?,
                    timestamp: row.get(5)?,
                })
            })?;
            for e in rows { events.push(e?); }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, event_type, content, raw_data, timestamp
                 FROM conversation_events WHERE session_id = ?1
                 ORDER BY id ASC LIMIT ?2"
            )?;
            let rows = stmt.query_map(params![session_id, limit], |row| {
                Ok(crate::types::ConversationEvent {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    event_type: row.get(2)?,
                    content: row.get(3)?,
                    raw_data: row.get(4)?,
                    timestamp: row.get(5)?,
                })
            })?;
            for e in rows { events.push(e?); }
        }
        Ok(events)
    }

    /// Get agent trajectory: all agent_* messages linked to a specific tool_use_id
    pub fn get_agent_trajectory(
        &self,
        tool_use_id: &str,
        limit: i64,
    ) -> DbResult<Vec<ConversationMessage>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata
             FROM conversation_messages WHERE parent_uuid = ?1 AND role LIKE 'agent_%'
             ORDER BY id ASC LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![tool_use_id, limit], |row| {
            Self::row_to_conversation_message(row)
        })?;
        let mut msgs = Vec::new();
        for m in rows { msgs.push(m?); }
        Ok(msgs)
    }

    /// Get event type summary across all sessions or for a specific session
    pub fn get_event_type_summary(
        &self,
        session_id: Option<&str>,
    ) -> DbResult<Vec<(String, i64)>> {
        let conn = self.read_conn();
        let mut summary = Vec::new();
        if let Some(sid) = session_id {
            let mut stmt = conn.prepare(
                "SELECT event_type, COUNT(*) as cnt FROM conversation_events
                 WHERE session_id = ?1 GROUP BY event_type ORDER BY cnt DESC"
            )?;
            let rows = stmt.query_map(params![sid], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            for r in rows { summary.push(r?); }
        } else {
            let mut stmt = conn.prepare(
                "SELECT event_type, COUNT(*) as cnt FROM conversation_events
                 GROUP BY event_type ORDER BY cnt DESC"
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            for r in rows { summary.push(r?); }
        }
        Ok(summary)
    }

    /// Delete old progress/hook events older than cutoff timestamp
    pub fn cleanup_old_events(&self, cutoff: &str) -> DbResult<usize> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM conversation_events
             WHERE event_type IN ('progress:bash_progress', 'progress:mcp_progress', 'hook_progress', 'progress:waiting_for_task')
             AND timestamp < ?1",
            params![cutoff],
        )?;
        Ok(deleted)
    }

    /// Get all session IDs that have at least one event in conversation_events
    pub fn get_sessions_with_events(&self) -> DbResult<std::collections::HashSet<String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT session_id FROM conversation_events"
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut set = std::collections::HashSet::new();
        for r in rows { set.insert(r?); }
        Ok(set)
    }

    /// Count tool calls still in 'pending' status (missing output)
    pub fn count_pending_tool_calls(&self) -> DbResult<i64> {
        let conn = self.read_conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM conversation_tool_calls WHERE status = 'pending'",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Get session IDs that have pending tool calls (for output patching)
    pub fn get_sessions_with_pending_tool_calls(&self) -> DbResult<Vec<String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT session_id FROM conversation_tool_calls WHERE status = 'pending'"
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Get all session IDs that have at least one tool call in conversation_tool_calls
    pub fn get_sessions_with_tool_calls(&self) -> DbResult<std::collections::HashSet<String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT session_id FROM conversation_tool_calls"
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut set = std::collections::HashSet::new();
        for r in rows { set.insert(r?); }
        Ok(set)
    }

    /// Get raw messages for tool call backfill (assistant+user roles with raw_content)
    pub fn get_messages_for_tool_call_backfill(&self, session_id: &str) -> DbResult<Vec<(String, String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT role, raw_content, timestamp FROM conversation_messages
             WHERE session_id = ?1 AND raw_content IS NOT NULL AND raw_content != ''
             AND role IN ('assistant', 'user', 'thinking', 'system', 'tool_result')
             ORDER BY id ASC"
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Get all conversations with their JSONL paths (for backfill)
    pub fn get_conversations_with_jsonl(&self) -> DbResult<Vec<(String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, jsonl_path FROM conversations WHERE jsonl_path IS NOT NULL AND jsonl_path != ''"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Mark a conversation as analyzed
    pub fn mark_conversation_analyzed(&self, id: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET analyzed_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// Get conversations that are completed but not yet analyzed
    pub fn get_unanalyzed_conversations(&self) -> DbResult<Vec<Conversation>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM conversations WHERE status = 'completed' AND analyzed_at IS NULL ORDER BY started_at DESC"
        )?;
        let rows = stmt.query_map([], |row| Self::row_to_conversation(row))?;
        let mut convs = Vec::new();
        for c in rows { convs.push(c?); }
        Ok(convs)
    }

    /// Mark a conversation as completed
    pub fn complete_conversation(&self, id: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET status = 'completed', ended_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// Complete stale active conversations whose last message is older than the given cutoff.
    /// Returns the number of conversations marked completed.
    pub fn complete_stale_conversations(&self, cutoff: &str) -> DbResult<usize> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT c.id FROM conversations c
             WHERE c.status = 'active'
               AND (SELECT MAX(m.timestamp) FROM conversation_messages m WHERE m.session_id = c.id) < ?1"
        )?;
        let ids: Vec<String> = stmt.query_map(params![cutoff], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        let now = chrono::Utc::now().to_rfc3339();
        for id in &ids {
            conn.execute(
                "UPDATE conversations SET status = 'completed', ended_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
        }
        Ok(ids.len())
    }

    /// Mark a conversation as compacted (replaced by context compaction).
    pub fn mark_conversation_compacted(&self, id: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET status = 'compacted', ended_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// Set the task_id on a conversation.
    pub fn set_conversation_task_id(&self, id: &str, task_id: &str) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET task_id = ?1 WHERE id = ?2",
            params![task_id, id],
        )?;
        Ok(())
    }

    /// Get all conversations sharing the same task_id.
    pub fn get_conversations_by_task_id(&self, task_id: &str) -> DbResult<Vec<Conversation>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM conversations WHERE task_id = ?1 ORDER BY started_at ASC"
        )?;
        let rows = stmt.query_map(params![task_id], |row| Self::row_to_conversation(row))?;
        let mut convs = Vec::new();
        for c in rows { convs.push(c?); }
        Ok(convs)
    }

    /// Re-activate a completed conversation when new messages arrive.
    pub fn reactivate_conversation(&self, id: &str) -> DbResult<usize> {
        let conn = self.conn();
        Ok(conn.execute(
            "UPDATE conversations SET status = 'active', ended_at = NULL WHERE id = ?1 AND status = 'completed'",
            params![id],
        )?)
    }

    /// Get conversation messages not yet forwarded to memory analysis.
    /// Returns messages from today (UTC) for user CLI sessions only (no PTY, no subagents).
    pub fn get_pending_memory_messages(&self, today: &str) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        tokio::task::block_in_place(|| {
            // Single JOIN query: get all pending messages at once
            // Excludes: PTY sessions (slot_id IS NOT NULL), subagent sessions (id LIKE 'agent-%')
            let conn = self.read_conn();
            let mut stmt = conn.prepare(
                "SELECT m.*, COALESCE(c.project, '') as c_project, c.memory_forwarded_at
                 FROM conversation_messages m
                 JOIN conversations c ON c.id = m.session_id
                 WHERE c.conversation_type = 'user'
                   AND m.timestamp >= ?1
                   AND m.timestamp > COALESCE(c.memory_forwarded_at, ?1)
                   AND m.role IN ('user', 'assistant')
                 ORDER BY c.started_at DESC, m.id ASC"
            )?;

            let mut results: Vec<(String, String, Vec<ConversationMessage>)> = Vec::new();

            let rows = stmt.query_map(params![today], |row| {
                let msg = Self::row_to_conversation_message(row)?;
                let project: String = row.get("c_project")?;
                Ok((msg, project))
            })?;

            for row in rows {
                let (msg, project) = row?;
                let session_id = msg.session_id.clone();
                if let Some(entry) = results.iter_mut().find(|(id, _, _)| id == &session_id) {
                    entry.2.push(msg);
                } else {
                    results.push((session_id, project, vec![msg]));
                }
            }

            Ok(results)
        })
    }

    /// Update memory_forwarded_at for a conversation
    pub fn update_memory_forwarded_at(&self, session_id: &str, timestamp: &str) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET memory_forwarded_at = ?1 WHERE id = ?2",
            params![timestamp, session_id],
        )?;
        Ok(())
    }

    /// Get pending USER-ONLY messages for user-voice extraction.
    /// Same logic as get_pending_memory_messages but only returns role='user'.
    pub fn get_pending_user_voice_messages(&self) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT m.*, COALESCE(c.project, '') as c_project, c.user_voice_forwarded_at
             FROM conversation_messages m
             JOIN conversations c ON c.id = m.session_id
             WHERE c.conversation_type = 'user'
               AND m.timestamp > COALESCE(c.user_voice_forwarded_at, c.started_at)
               AND m.role = 'user'
             ORDER BY m.timestamp ASC"
        )?;

        let mut results: Vec<(String, String, Vec<ConversationMessage>)> = Vec::new();

        let rows = stmt.query_map([], |row| {
            let msg = Self::row_to_conversation_message(row)?;
            let project: String = row.get("c_project")?;
            Ok((msg, project))
        })?;

        for row in rows {
            let (msg, project) = row?;
            let session_id = msg.session_id.clone();
            if let Some(entry) = results.iter_mut().find(|(id, _, _)| id == &session_id) {
                entry.2.push(msg);
            } else {
                results.push((session_id, project, vec![msg]));
            }
        }

        Ok(results)
    }

    /// Update user_voice_forwarded_at for a conversation
    pub fn update_user_voice_forwarded_at(&self, session_id: &str, timestamp: &str) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET user_voice_forwarded_at = ?1 WHERE id = ?2",
            params![timestamp, session_id],
        )?;
        Ok(())
    }

    /// Get pending messages for unified realtime extraction (replaces separate user_voice + memory).
    /// Returns all user+assistant messages since realtime_forwarded_at watermark.
    /// Uses fair-queuing: per-session cap (10 msgs) + oldest-first ordering to prevent starvation.
    pub fn get_pending_realtime_messages(&self) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        self.get_pending_realtime_messages_with_limit(50)
    }

    /// Get pending realtime messages with a configurable limit.
    /// Fair-queuing: each session gets at most 15 messages per batch, ordered by oldest first.
    /// Includes tool_result messages (file contents, command outputs) for richer memory extraction.
    pub fn get_pending_realtime_messages_with_limit(&self, limit: usize) -> DbResult<Vec<(String, String, Vec<ConversationMessage>)>> {
        tokio::task::block_in_place(|| {
            let conn = self.read_conn();
            let mut stmt = conn.prepare(
                "WITH ranked AS (
                    SELECT m.*, COALESCE(c.project, '') as c_project,
                        ROW_NUMBER() OVER(PARTITION BY m.session_id ORDER BY m.timestamp ASC) as rn
                    FROM conversation_messages m
                    JOIN conversations c ON c.id = m.session_id
                    WHERE c.conversation_type = 'user'
                      AND m.timestamp > COALESCE(c.realtime_forwarded_at, c.started_at)
                      AND m.role IN ('user', 'assistant', 'tool_result')
                )
                SELECT * FROM ranked
                WHERE rn <= 15
                ORDER BY timestamp ASC
                LIMIT ?1"
            )?;

            let mut results: Vec<(String, String, Vec<ConversationMessage>)> = Vec::new();

            let rows = stmt.query_map(params![limit as i64], |row| {
                let msg = Self::row_to_conversation_message(row)?;
                let project: String = row.get("c_project")?;
                Ok((msg, project))
            })?;

            for row in rows {
                let (msg, project) = row?;
                let session_id = msg.session_id.clone();
                if let Some(entry) = results.iter_mut().find(|(id, _, _)| id == &session_id) {
                    entry.2.push(msg);
                } else {
                    results.push((session_id, project, vec![msg]));
                }
            }

            Ok(results)
        })
    }

    /// Update realtime_forwarded_at watermark for a conversation.
    pub fn update_realtime_forwarded_at(&self, session_id: &str, timestamp: &str) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET realtime_forwarded_at = ?1 WHERE id = ?2",
            params![timestamp, session_id],
        )?;
        Ok(())
    }



}
