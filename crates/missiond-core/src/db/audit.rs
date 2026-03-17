use rusqlite::params;
use tracing::{debug, warn};
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
    /// Dedup via UNIQUE index on event_uuid — duplicate inserts are silently ignored.
    pub fn insert_conversation_events_batch(&self, events: &[crate::types::ConversationEvent]) -> DbResult<usize> {
        if events.is_empty() {
            return Ok(0);
        }
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let mut count = 0usize;
        for event in events {
            tx.execute(
                "INSERT OR IGNORE INTO conversation_events (session_id, event_uuid, event_type, content, raw_data, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![event.session_id, event.event_uuid, event.event_type, event.content, event.raw_data, event.timestamp],
            )?;
            if tx.changes() > 0 {
                count += 1;
            }
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
                "SELECT id, session_id, event_uuid, event_type, content, raw_data, timestamp
                 FROM conversation_events WHERE session_id = ?1 AND event_type = ?2
                 ORDER BY id ASC LIMIT ?3"
            )?;
            let rows = stmt.query_map(params![session_id, et, limit], |row| {
                Ok(crate::types::ConversationEvent {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    event_uuid: row.get(2)?,
                    event_type: row.get(3)?,
                    content: row.get(4)?,
                    raw_data: row.get(5)?,
                    timestamp: row.get(6)?,
                })
            })?;
            for e in rows { events.push(e?); }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, event_uuid, event_type, content, raw_data, timestamp
                 FROM conversation_events WHERE session_id = ?1
                 ORDER BY id ASC LIMIT ?2"
            )?;
            let rows = stmt.query_map(params![session_id, limit], |row| {
                Ok(crate::types::ConversationEvent {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    event_uuid: row.get(2)?,
                    event_type: row.get(3)?,
                    content: row.get(4)?,
                    raw_data: row.get(5)?,
                    timestamp: row.get(6)?,
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

    /// Save exit code on a conversation (for ground truth learning).
    pub fn save_conversation_exit_code(&self, id: &str, exit_code: i32) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET exit_code = ?1 WHERE id = ?2",
            params![exit_code, id],
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

    // ============ Retrospective Analysis ============

    /// Get retrospective stats for a session: tool frequency, avg duration, error rates
    pub fn get_retrospective_tool_stats(&self, session_id: &str, limit: i64) -> DbResult<Vec<(String, i64, i64, i64, f64)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT tool_name,
                    COUNT(*) as total,
                    SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) as success_count,
                    SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END) as error_count,
                    COALESCE(AVG(duration_ms), 0) as avg_duration
             FROM conversation_tool_calls WHERE session_id = ?1
             GROUP BY tool_name ORDER BY total DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![session_id, limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, f64>(4)?,
            ))
        })?;
        let mut stats = Vec::new();
        for r in rows { stats.push(r?); }
        Ok(stats)
    }

    /// Get session meta: total calls, total duration, unique tools, compaction count
    pub fn get_retrospective_meta(&self, session_id: &str) -> DbResult<(i64, i64, i64, i64)> {
        let conn = self.read_conn();
        let (total_calls, total_duration, unique_tools): (i64, i64, i64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(duration_ms), 0), COUNT(DISTINCT tool_name)
             FROM conversation_tool_calls WHERE session_id = ?1",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let compact_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM conversation_events
             WHERE session_id = ?1 AND event_type = 'compact_boundary'",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok((total_calls, total_duration, unique_tools, compact_count))
    }

    /// Detect consecutive repeat patterns (Gaps-and-Islands)
    pub fn get_retrospective_repeat_patterns(&self, session_id: &str, min_streak: i64) -> DbResult<Vec<(String, i64, String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "WITH numbered AS (
                SELECT tool_name, timestamp,
                       ROW_NUMBER() OVER (ORDER BY rowid) as rn,
                       ROW_NUMBER() OVER (PARTITION BY tool_name ORDER BY rowid) as grn
                FROM conversation_tool_calls WHERE session_id = ?1
            )
            SELECT tool_name, COUNT(*) as streak, MIN(timestamp) as start_t, MAX(timestamp) as end_t
            FROM numbered GROUP BY tool_name, (rn - grn)
            HAVING COUNT(*) >= ?2 ORDER BY streak DESC"
        )?;
        let rows = stmt.query_map(params![session_id, min_streak], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut patterns = Vec::new();
        for r in rows { patterns.push(r?); }
        Ok(patterns)
    }

    /// Get ordered tool names for N-Gram analysis
    pub fn get_tool_name_sequence(&self, session_id: &str) -> DbResult<Vec<String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT tool_name FROM conversation_tool_calls WHERE session_id = ?1 ORDER BY rowid ASC"
        )?;
        let rows = stmt.query_map(params![session_id], |row| row.get::<_, String>(0))?;
        let mut seq = Vec::new();
        for r in rows { seq.push(r?); }
        Ok(seq)
    }

    /// Get high error rate tools
    pub fn get_retrospective_high_error_tools(&self, session_id: &str, min_error_rate: f64) -> DbResult<Vec<(String, f64, i64)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT tool_name,
                    ROUND(100.0 * SUM(CASE WHEN status='error' THEN 1 ELSE 0 END) / COUNT(*), 1) as error_rate,
                    COUNT(*) as total
             FROM conversation_tool_calls WHERE session_id = ?1
             GROUP BY tool_name HAVING error_rate > ?2 ORDER BY error_rate DESC"
        )?;
        let rows = stmt.query_map(params![session_id, min_error_rate], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut tools = Vec::new();
        for r in rows { tools.push(r?); }
        Ok(tools)
    }

    /// Get error samples for a tool (first and last error)
    pub fn get_tool_error_samples(&self, session_id: &str, tool_name: &str) -> DbResult<Vec<(String, String, String)>> {
        let conn = self.read_conn();
        // Get first error
        let mut samples = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT input_summary, output_summary, timestamp FROM conversation_tool_calls
             WHERE session_id = ?1 AND tool_name = ?2 AND status = 'error'
             ORDER BY rowid ASC LIMIT 1"
        )?;
        let rows = stmt.query_map(params![session_id, tool_name], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        for r in rows { samples.push(r?); }
        // Get last error (if different from first)
        let mut stmt2 = conn.prepare(
            "SELECT input_summary, output_summary, timestamp FROM conversation_tool_calls
             WHERE session_id = ?1 AND tool_name = ?2 AND status = 'error'
             ORDER BY rowid DESC LIMIT 1"
        )?;
        let rows2 = stmt2.query_map(params![session_id, tool_name], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        for r in rows2 {
            let sample = r?;
            if samples.is_empty() || samples[0].2 != sample.2 {
                samples.push(sample);
            }
        }
        Ok(samples)
    }

    /// Get first user message of a session (mission objective)
    pub fn get_first_user_message(&self, session_id: &str) -> DbResult<Option<String>> {
        let conn = self.read_conn();
        let result = conn.query_row(
            "SELECT content FROM conversation_messages
             WHERE session_id = ?1 AND role = 'user'
             ORDER BY id ASC LIMIT 1",
            params![session_id],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(content) => Ok(Some(content)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get tool calls with input_summary for detailed analysis (file paths, server names)
    pub fn get_tool_calls_for_detailed_analysis(&self, session_id: &str) -> DbResult<Vec<(String, String, String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT tool_name, COALESCE(input_summary, ''), COALESCE(output_summary, ''), status
             FROM conversation_tool_calls WHERE session_id = ?1 ORDER BY rowid ASC"
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Get tool calls with timestamps for error recovery chain analysis
    pub fn get_tool_calls_with_status_timeline(&self, session_id: &str) -> DbResult<Vec<(String, String, String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT tool_name, status, COALESCE(input_summary, ''), timestamp
             FROM conversation_tool_calls WHERE session_id = ?1 ORDER BY rowid ASC"
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    // ============ Retrospective Results Persistence ============

    /// Save retrospective quick stats for a session
    pub fn save_retrospective_result(
        &self,
        session_id: &str,
        trigger_reason: &str,
        quick_stats: &str,
        full_analysis: Option<&str>,
    ) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR REPLACE INTO retrospective_results (session_id, trigger_reason, quick_stats, full_analysis, created_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now'))",
            params![session_id, trigger_reason, quick_stats, full_analysis],
        )?;
        Ok(())
    }

    /// Check if a session already has a retrospective result
    pub fn has_retrospective_result(&self, session_id: &str) -> DbResult<bool> {
        let conn = self.read_conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM retrospective_results WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Get sessions needing retrospective (completed, not yet analyzed, meeting threshold).
    /// Belt-and-suspenders: excludes meta-agent sessions by both conversation_type AND slot_id prefix.
    pub fn get_sessions_needing_retrospective(&self) -> DbResult<Vec<(String, i64, i64, f64)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT c.id, c.message_count,
                    COALESCE((SELECT COUNT(*) FROM conversation_tool_calls tc WHERE tc.session_id = c.id), 0) as tool_count,
                    COALESCE(
                        (SELECT 100.0 * SUM(CASE WHEN tc2.status='error' THEN 1 ELSE 0 END) / NULLIF(COUNT(*), 0)
                         FROM conversation_tool_calls tc2 WHERE tc2.session_id = c.id), 0
                    ) as error_rate
             FROM conversations c
             WHERE c.status = 'completed'
               AND c.conversation_type = 'user'
               AND (c.slot_id IS NULL OR (
                   c.slot_id NOT LIKE 'slot-memory%'
                   AND c.slot_id NOT LIKE 'slot-diagnosis%'
                   AND c.slot_id NOT LIKE 'agent-%'
               ))
               AND c.id NOT IN (SELECT session_id FROM retrospective_results)
               AND (
                   c.message_count > 100
                   OR (SELECT COUNT(*) FROM conversation_tool_calls tc3 WHERE tc3.session_id = c.id) > 50
                   OR CAST((julianday(c.ended_at) - julianday(c.started_at)) * 24 * 60 AS INTEGER) > 60
               )
             ORDER BY c.ended_at DESC
             LIMIT 5"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, f64>(3)?,
            ))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Get ALL user sessions since a given time for retrospective backfill (no threshold filtering).
    /// Skips sessions that already have retrospective results unless `force` is true.
    /// Belt-and-suspenders: excludes meta-agent sessions by both conversation_type AND slot_id prefix.
    pub fn get_sessions_for_retro_backfill(&self, since: &str, force: bool) -> DbResult<Vec<(String, i64, i64, f64)>> {
        let conn = self.read_conn();
        let exclude_clause = if force { "" } else {
            "AND c.id NOT IN (SELECT session_id FROM retrospective_results)"
        };
        let sql = format!(
            "SELECT c.id, c.message_count,
                    COALESCE((SELECT COUNT(*) FROM conversation_tool_calls tc WHERE tc.session_id = c.id), 0) as tool_count,
                    COALESCE(
                        (SELECT 100.0 * SUM(CASE WHEN tc2.status='error' THEN 1 ELSE 0 END) / NULLIF(COUNT(*), 0)
                         FROM conversation_tool_calls tc2 WHERE tc2.session_id = c.id), 0
                    ) as error_rate
             FROM conversations c
             WHERE c.conversation_type = 'user'
               AND (c.slot_id IS NULL OR (
                   c.slot_id NOT LIKE 'slot-memory%'
                   AND c.slot_id NOT LIKE 'slot-diagnosis%'
                   AND c.slot_id NOT LIKE 'agent-%'
               ))
               AND c.message_count >= 6
               AND c.started_at >= ?1
               {exclude_clause}
             ORDER BY c.started_at ASC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![since], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, f64>(3)?,
            ))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// List retrospective results, newest first
    pub fn list_retrospective_results(&self, limit: i64) -> DbResult<Vec<(String, String, String, Option<String>, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT session_id, trigger_reason, quick_stats, full_analysis, created_at
             FROM retrospective_results ORDER BY created_at DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Get retrospective result for a specific session
    pub fn get_retrospective_result(&self, session_id: &str) -> DbResult<Option<(String, String, Option<String>, String)>> {
        let conn = self.read_conn();
        let result = conn.query_row(
            "SELECT trigger_reason, quick_stats, full_analysis, created_at
             FROM retrospective_results WHERE session_id = ?1",
            params![session_id],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
            )),
        );
        match result {
            Ok(r) => Ok(Some(r)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
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
        let rows = conn.execute(
            "UPDATE conversations SET realtime_forwarded_at = ?1 WHERE id = ?2",
            params![timestamp, session_id],
        )?;
        if rows == 0 {
            warn!(session_id, timestamp, "Watermark update: session not found (0 rows affected)");
        } else {
            debug!(session_id, timestamp, "Watermark advanced");
        }
        Ok(())
    }



}
