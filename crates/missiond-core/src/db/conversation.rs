use rusqlite::{params, OptionalExtension};
use super::error::DbResult;
use crate::types::*;
use super::MissionDB;

impl MissionDB {
    // ============ Message Pipeline Tracking ============



    /// Get conversations pending deep analysis (conversation-level watermark).
    /// Returns completed user conversations that haven't been analyzed at the current version.
    pub fn get_pending_deep_analysis(&self, current_version: i32, max_retries: i32) -> DbResult<Vec<Conversation>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM conversations
             WHERE status = 'completed'
               AND conversation_type = 'user'
               AND ended_at < datetime('now', '-5 minutes')
               AND analysis_retries < ?1
               AND (analyzed_at IS NULL OR analysis_version < ?2)

             UNION ALL

             SELECT * FROM conversations
             WHERE status = 'compacted'
               AND conversation_type = 'user'
               AND analysis_retries < ?1
               AND (analyzed_at IS NULL OR analysis_version < ?2)

             UNION ALL

             SELECT * FROM conversations
             WHERE status = 'active'
               AND conversation_type = 'user'
               AND analysis_retries < ?1
               AND (SELECT COUNT(*) FROM conversation_messages m
                    WHERE m.session_id = conversations.id
                      AND m.id > COALESCE(conversations.deep_analyzed_message_id, 0)
                      AND m.role IN ('user', 'assistant')) >= 100

             ORDER BY started_at ASC"
        )?;
        let rows = stmt.query_map(params![max_retries, current_version], |row| Self::row_to_conversation(row))?;
        let mut convs = Vec::new();
        for c in rows { convs.push(c?); }
        Ok(convs)
    }

    /// Lightweight check: are there any conversations pending deep analysis?
    pub fn has_pending_deep_analysis(&self, current_version: i32, max_retries: i32) -> DbResult<bool> {
        let conn = self.read_conn();
        Ok(conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM conversations
                WHERE status = 'completed'
                  AND conversation_type = 'user'
                  AND ended_at < datetime('now', '-5 minutes')
                  AND analysis_retries < ?1
                  AND (analyzed_at IS NULL OR analysis_version < ?2)

                UNION ALL

                SELECT 1 FROM conversations
                WHERE status = 'compacted'
                  AND conversation_type = 'user'
                  AND analysis_retries < ?1
                  AND (analyzed_at IS NULL OR analysis_version < ?2)

                UNION ALL

                SELECT 1 FROM conversations
                WHERE status = 'active'
                  AND conversation_type = 'user'
                  AND analysis_retries < ?1
                  AND (SELECT COUNT(*) FROM conversation_messages m
                       WHERE m.session_id = conversations.id
                         AND m.id > COALESCE(conversations.deep_analyzed_message_id, 0)
                         AND m.role IN ('user', 'assistant')) >= 100
            )",
            params![max_retries, current_version],
            |row| row.get(0),
        )?)
    }

    /// Count conversations pending deep analysis.
    pub fn count_pending_deep_analysis(&self, current_version: i32, max_retries: i32) -> DbResult<i64> {
        let conn = self.read_conn();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM (
                SELECT id FROM conversations
                WHERE status = 'completed'
                  AND conversation_type = 'user'
                  AND ended_at < datetime('now', '-5 minutes')
                  AND analysis_retries < ?1
                  AND (analyzed_at IS NULL OR analysis_version < ?2)

                UNION ALL

                SELECT id FROM conversations
                WHERE status = 'compacted'
                  AND conversation_type = 'user'
                  AND analysis_retries < ?1
                  AND (analyzed_at IS NULL OR analysis_version < ?2)

                UNION ALL

                SELECT id FROM conversations
                WHERE status = 'active'
                  AND conversation_type = 'user'
                  AND analysis_retries < ?1
                  AND (SELECT COUNT(*) FROM conversation_messages m
                       WHERE m.session_id = conversations.id
                         AND m.id > COALESCE(conversations.deep_analyzed_message_id, 0)
                         AND m.role IN ('user', 'assistant')) >= 100
            )",
            params![max_retries, current_version],
            |row| row.get(0),
        )?)
    }

    /// Count distinct sessions with pending realtime messages.
    pub fn count_pending_realtime(&self) -> DbResult<i64> {
        let conn = self.read_conn();
        Ok(conn.query_row(
            "SELECT COUNT(DISTINCT c.id) FROM conversations c
             JOIN conversation_messages m ON c.id = m.session_id
             WHERE c.conversation_type = 'user'
               AND m.timestamp > COALESCE(c.realtime_forwarded_at, c.started_at)
               AND m.role IN ('user', 'assistant')",
            [],
            |row| row.get(0),
        )?)
    }

    /// Per-session pending realtime message summary (session_id, msg_count, oldest_timestamp).
    pub fn pending_realtime_detail(&self) -> DbResult<Vec<(String, i64, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT c.id, COUNT(*) as cnt, MIN(m.timestamp) as oldest
             FROM conversations c
             JOIN conversation_messages m ON c.id = m.session_id
             WHERE c.conversation_type = 'user'
               AND m.timestamp > COALESCE(c.realtime_forwarded_at, c.started_at)
               AND m.role IN ('user', 'assistant')
             GROUP BY c.id
             ORDER BY cnt DESC
             LIMIT 20"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Per-conversation pending deep analysis summary (id, ended_at, retries).
    pub fn pending_deep_detail(&self, current_version: i32, max_retries: i32) -> DbResult<Vec<(String, String, i32)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, COALESCE(ended_at, '[active]'), analysis_retries FROM conversations
             WHERE status = 'completed'
               AND conversation_type = 'user'
               AND ended_at < datetime('now', '-5 minutes')
               AND analysis_retries < ?1
               AND (analyzed_at IS NULL OR analysis_version < ?2)

             UNION ALL

             SELECT id, '[checkpoint]', analysis_retries FROM conversations
             WHERE status = 'active'
               AND conversation_type = 'user'
               AND analysis_retries < ?1
               AND (SELECT COUNT(*) FROM conversation_messages m
                    WHERE m.session_id = conversations.id
                      AND m.id > COALESCE(conversations.deep_analyzed_message_id, 0)
                      AND m.role IN ('user', 'assistant')) >= 100

             ORDER BY 2 ASC
             LIMIT 20"
        )?;
        let rows = stmt.query_map(params![max_retries, current_version], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i32>(2)?))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Mark a conversation's deep analysis as complete with the given version.
    pub fn mark_analysis_complete(&self, id: &str, version: i32) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET analyzed_at = ?1, analysis_version = ?2, analysis_retries = 0, deep_analyzed_message_id = 0 WHERE id = ?3",
            params![now, version, id],
        )?;
        Ok(())
    }

    /// Advance the deep analysis checkpoint watermark for an active session.
    pub fn update_deep_checkpoint(&self, id: &str, message_id: i64) -> DbResult<()> {
        self.conn().execute(
            "UPDATE conversations SET deep_analyzed_message_id = ?1, analysis_retries = 0 WHERE id = ?2",
            params![message_id, id],
        )?;
        Ok(())
    }

    /// Increment analysis retry count for a failed deep analysis attempt.
    pub fn mark_analysis_failed(&self, id: &str) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET analysis_retries = analysis_retries + 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }


    // ============ Historical Habit Scan ============

    /// Get conversations not yet scanned for user habits.
    /// Only 'user' type conversations with enough messages. Returns oldest first.
    pub fn get_unscanned_conversations(&self, limit: usize) -> DbResult<Vec<Conversation>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM conversations
             WHERE habit_scanned_at IS NULL
               AND conversation_type = 'user'
               AND message_count >= 4
               AND status IN ('completed', 'compacted', 'active')
             ORDER BY started_at ASC
             LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| Self::row_to_conversation(row))?;
        let mut convs = Vec::new();
        for c in rows { convs.push(c?); }
        Ok(convs)
    }

    /// Mark a conversation as scanned for habits.
    pub fn mark_habit_scanned(&self, id: &str) -> DbResult<()> {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        self.conn().execute(
            "UPDATE conversations SET habit_scanned_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// Count conversations not yet scanned for habits.
    pub fn count_unscanned_conversations(&self) -> DbResult<i64> {
        let conn = self.read_conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM conversations
             WHERE habit_scanned_at IS NULL
               AND conversation_type = 'user'
               AND message_count >= 4",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Count total user conversations eligible for habit scan.
    pub fn count_scannable_conversations(&self) -> DbResult<i64> {
        let conn = self.read_conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM conversations
             WHERE conversation_type = 'user'
               AND message_count >= 4",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    // ============ Conversations ============

    /// Upsert a conversation session
    pub fn upsert_conversation(&self, conv: &Conversation) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO conversations (id, project, slot_id, source, model, git_branch, jsonl_path, parent_session_id, task_id, message_count, started_at, ended_at, status, analyzed_at, conversation_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT(id) DO UPDATE SET
                slot_id = COALESCE(?3, slot_id),
                model = COALESCE(?5, model),
                git_branch = COALESCE(?6, git_branch),
                message_count = ?10,
                ended_at = ?12,
                status = ?13,
                conversation_type = COALESCE(?15, conversation_type)",
            params![
                conv.id, conv.project, conv.slot_id, conv.source, conv.model,
                conv.git_branch, conv.jsonl_path, conv.parent_session_id, conv.task_id,
                conv.message_count, conv.started_at, conv.ended_at, conv.status,
                conv.analyzed_at, conv.conversation_type,
            ],
        )?;
        Ok(())
    }

    /// Get child (subagent) conversations for a parent session.
    pub fn get_child_conversations(&self, parent_session_id: &str) -> DbResult<Vec<Conversation>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM conversations WHERE parent_session_id = ?1 ORDER BY started_at ASC"
        )?;
        let rows = stmt.query_map(params![parent_session_id], |row| Self::row_to_conversation(row))?;
        let mut convs = Vec::new();
        for c in rows { convs.push(c?); }
        Ok(convs)
    }

    /// Insert a conversation message, returns the auto-increment ID.
    /// Dedup via UNIQUE index on message_uuid — duplicate inserts are silently ignored.
    pub fn insert_conversation_message(&self, msg: &ConversationMessage) -> DbResult<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR IGNORE INTO conversation_messages (session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata, tool_name)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                msg.session_id, msg.role, msg.content, msg.raw_content,
                msg.message_uuid, msg.parent_uuid, msg.model, msg.timestamp, msg.metadata,
                msg.tool_name,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Batch insert conversation messages within a transaction.
    /// Returns IDs of actually inserted rows (dedup via UNIQUE index).
    pub fn insert_conversation_messages_batch(&self, messages: &[ConversationMessage]) -> DbResult<Vec<i64>> {
        if messages.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let mut inserted_ids = Vec::new();
        for msg in messages {
            tx.execute(
                "INSERT OR IGNORE INTO conversation_messages (session_id, role, content, raw_content, message_uuid, parent_uuid, model, timestamp, metadata, tool_name)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    msg.session_id, msg.role, msg.content, msg.raw_content,
                    msg.message_uuid, msg.parent_uuid, msg.model, msg.timestamp, msg.metadata,
                    msg.tool_name,
                ],
            )?;
            if tx.changes() > 0 {
                inserted_ids.push(tx.last_insert_rowid());
            }
        }
        // message_count is auto-updated by trg_msg_count_insert trigger
        tx.commit()?;
        Ok(inserted_ids)
    }

    /// Get a conversation by ID
    pub fn get_conversation(&self, id: &str) -> DbResult<Option<Conversation>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare("SELECT * FROM conversations WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_conversation(row)?))
        } else {
            Ok(None)
        }
    }

    /// List conversations, optionally filtered by status, conversation_type, and task_id.
    /// conv_type: None = user+worker (default), Some("meta") = system, Some("system") = meta+worker, Some("all") = everything.
    /// List conversations, optionally filtered by status, conversation_type, task_id, and time range.
    /// conv_type: None = user+worker (default), Some("meta") = system, Some("system") = meta+worker, Some("all") = everything.
    pub fn list_conversations(&self, status: Option<&str>, limit: i64, conv_type: Option<&str>, task_id: Option<&str>, since: Option<&str>, until: Option<&str>) -> DbResult<Vec<Conversation>> {
        let conn = self.read_conn();
        let mut convs = Vec::new();
        let type_clause = match conv_type {
            Some("all") => String::new(),
            Some("meta") => " AND conversation_type = 'meta'".to_string(),
            Some("worker") => " AND conversation_type = 'worker'".to_string(),
            Some("system") => " AND conversation_type IN ('meta', 'worker')".to_string(),
            _ => " AND conversation_type IN ('user', 'worker')".to_string(),
        };

        // Build time range clause
        // Note: conversations.started_at uses ISO format with T separator (e.g., "2026-03-11T05:03:41.382Z")
        // while system_timeline.created_at uses space separator. We normalize to ISO T format here.
        let time_clause = {
            let mut parts = Vec::new();
            if let Some(s) = since {
                let ts = Self::parse_since(s).replace(' ', "T");
                parts.push(format!("started_at >= '{}'", ts.replace('\'', "''")));
            }
            if let Some(u) = until {
                let ts = Self::parse_until(u).replace(' ', "T");
                parts.push(format!("started_at <= '{}'", ts.replace('\'', "''")));
            }
            if parts.is_empty() { String::new() } else { format!(" AND {}", parts.join(" AND ")) }
        };

        // Fast path: filter by task_id (returns all matching conversations)
        if let Some(tid) = task_id {
            let sql = format!(
                "SELECT * FROM conversations WHERE task_id = ?1{}{} ORDER BY started_at ASC LIMIT ?2",
                if matches!(conv_type, Some("all")) { String::new() } else { type_clause.clone() },
                time_clause
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![tid, limit], |row| Self::row_to_conversation(row))?;
            for c in rows { convs.push(c?); }
            return Ok(convs);
        }

        // When time range is specified, use simplified query path
        if since.is_some() || until.is_some() {
            let status_clause = if let Some(s) = status {
                format!("status = '{}'", s.replace('\'', "''"))
            } else {
                "1=1".to_string()
            };
            let sql = format!(
                "SELECT * FROM conversations WHERE {}{}{} ORDER BY started_at DESC LIMIT ?1",
                status_clause, type_clause, time_clause
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![limit], |row| Self::row_to_conversation(row))?;
            for c in rows { convs.push(c?); }
            return Ok(convs);
        }

        if let Some(s) = status {
            let sql = format!(
                "SELECT * FROM conversations WHERE status = ?1{} ORDER BY started_at DESC LIMIT ?2",
                type_clause
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![s, limit], |row| Self::row_to_conversation(row))?;
            for c in rows { convs.push(c?); }
        } else {
            {
                let sql = format!(
                    "SELECT * FROM conversations WHERE status = 'active'{} ORDER BY started_at DESC",
                    type_clause
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map([], |row| Self::row_to_conversation(row))?;
                for c in rows { convs.push(c?); }
            }
            let remaining = limit - convs.len() as i64;
            if remaining > 0 {
                let sql = format!(
                    "SELECT * FROM conversations WHERE status != 'active'{} ORDER BY started_at DESC LIMIT ?1",
                    type_clause
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![remaining], |row| Self::row_to_conversation(row))?;
                for c in rows { convs.push(c?); }
            }
            convs.sort_by(|a, b| {
                let a_active = a.status == "active";
                let b_active = b.status == "active";
                match (a_active, b_active) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => b.started_at.cmp(&a.started_at),
                }
            });
        }
        Ok(convs)
    }

    /// Get conversation messages, optionally since a given ID
    /// Get a single conversation message by ID.
    pub fn get_conversation_message_by_id(&self, id: i64) -> DbResult<Option<ConversationMessage>> {
        let conn = self.read_conn();
        let result = conn.query_row(
            "SELECT * FROM conversation_messages WHERE id = ?1",
            params![id],
            |row| Self::row_to_conversation_message(row),
        );
        match result {
            Ok(msg) => Ok(Some(msg)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_conversation_messages(
        &self,
        session_id: &str,
        since_id: Option<i64>,
        limit: i64,
    ) -> DbResult<Vec<ConversationMessage>> {
        let conn = self.read_conn();
        let mut msgs = Vec::new();
        if let Some(since) = since_id {
            let mut stmt = conn.prepare(
                "SELECT * FROM conversation_messages WHERE session_id = ?1 AND id > ?2 ORDER BY id ASC LIMIT ?3"
            )?;
            let rows = stmt.query_map(params![session_id, since, limit], |row| Self::row_to_conversation_message(row))?;
            for m in rows { msgs.push(m?); }
        } else {
            // Return last N messages
            let mut stmt = conn.prepare(
                "SELECT * FROM (SELECT * FROM conversation_messages WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2) ORDER BY id ASC"
            )?;
            let rows = stmt.query_map(params![session_id, limit], |row| Self::row_to_conversation_message(row))?;
            for m in rows { msgs.push(m?); }
        }
        Ok(msgs)
    }

    /// Get messages around a specific message ID (context anchoring).
    /// Returns None if messageId not found. Otherwise returns (session_id, messages).
    pub fn get_messages_around(
        &self,
        message_id: i64,
        before: i64,
        after: i64,
    ) -> DbResult<Option<(String, Vec<ConversationMessage>)>> {
        let conn = self.read_conn();

        // Resolve session_id from anchor message
        let session_id: Option<String> = conn.query_row(
            "SELECT session_id FROM conversation_messages WHERE id = ?1",
            params![message_id],
            |row| row.get(0),
        ).optional()?;

        let session_id = match session_id {
            Some(sid) => sid,
            None => return Ok(None),
        };

        let sql = "\
            WITH before_msgs AS (
                SELECT * FROM conversation_messages
                WHERE session_id = ?1 AND id < ?2
                ORDER BY id DESC LIMIT ?3
            ),
            after_msgs AS (
                SELECT * FROM conversation_messages
                WHERE session_id = ?1 AND id > ?2
                ORDER BY id ASC LIMIT ?4
            ),
            anchor AS (
                SELECT * FROM conversation_messages WHERE id = ?2
            )
            SELECT * FROM (
                SELECT * FROM before_msgs
                UNION ALL
                SELECT * FROM anchor
                UNION ALL
                SELECT * FROM after_msgs
            ) ORDER BY id ASC";

        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(
            params![session_id, message_id, before, after],
            |row| Self::row_to_conversation_message(row),
        )?;
        let mut msgs = Vec::new();
        for m in rows { msgs.push(m?); }

        Ok(Some((session_id, msgs)))
    }

    /// Search conversation messages: AND-first FTS5, OR fallback, then LIKE.
    pub fn search_conversation_messages(&self, query: &str, limit: i64) -> DbResult<Vec<ConversationMessage>> {
        let conn = self.read_conn();
        let (and_query, or_query) = Self::build_conv_fts_queries(query);

        // Exclude meta/compaction conversations from search results
        let fts_sql =
            "SELECT m.* FROM conversation_messages m
             JOIN conversation_msg_fts f ON m.id = f.rowid
             JOIN conversations c ON m.session_id = c.id
             WHERE conversation_msg_fts MATCH ?1
               AND c.conversation_type NOT IN ('meta', 'compaction')
             ORDER BY f.rank
             LIMIT ?2";

        // Phase 1: AND — all terms
        let results = {
            let mut stmt = conn.prepare(fts_sql)?;
            let rows = stmt.query_map(params![and_query, limit], |row| Self::row_to_conversation_message(row))?;
            let mut msgs = Vec::new();
            for m in rows { msgs.push(m?); }
            msgs
        };
        if !results.is_empty() { return Ok(results); }

        // Phase 2: OR — any term
        let results = {
            let mut stmt = conn.prepare(fts_sql)?;
            let rows = stmt.query_map(params![or_query, limit], |row| Self::row_to_conversation_message(row))?;
            let mut msgs = Vec::new();
            for m in rows { msgs.push(m?); }
            msgs
        };
        if !results.is_empty() { return Ok(results); }

        // Phase 3: LIKE fallback for Chinese substrings
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT m.* FROM conversation_messages m
             JOIN conversations c ON m.session_id = c.id
             WHERE m.content LIKE ?1
               AND c.conversation_type NOT IN ('meta', 'compaction')
             ORDER BY m.timestamp DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![pattern, limit], |row| Self::row_to_conversation_message(row))?;
        let mut msgs = Vec::new();
        for m in rows { msgs.push(m?); }
        Ok(msgs)
    }

    /// Message-level search with SQL-pushed filters (session_id, role, tool_name, time_after).
    /// Fixes the .retain() bug by filtering at DB level instead of in memory.
    /// Three-phase strategy: AND → OR → LIKE fallback.
    pub fn search_messages_filtered(
        &self,
        query: &str,
        session_id: Option<&str>,
        role: Option<&str>,
        tool_name: Option<&str>,
        time_after: Option<&str>,
        limit: i64,
    ) -> DbResult<Vec<ConversationMessage>> {
        let conn = self.read_conn();
        let (and_query, or_query) = Self::build_conv_fts_queries(query);

        // Build dynamic WHERE conditions and params (after ?1=query, ?2=limit)
        let mut extra_conds = Vec::new();
        let mut extra_vals: Vec<String> = Vec::new();

        extra_conds.push("c.conversation_type NOT IN ('meta', 'compaction')".to_string());

        if let Some(sid) = session_id {
            extra_conds.push(format!("m.session_id = ?{}", 3 + extra_vals.len()));
            extra_vals.push(sid.to_string());
        }
        if let Some(r) = role {
            extra_conds.push(format!("m.role = ?{}", 3 + extra_vals.len()));
            extra_vals.push(r.to_string());
        }
        if let Some(tn) = tool_name {
            let idx = 3 + extra_vals.len();
            // Wrap field with commas to match first/last elements in CSV:
            // "Bash,Read" → ",Bash,Read," LIKE "%,Bash,%" ✓
            extra_conds.push(format!(
                "(',' || m.tool_name || ',') LIKE ?{idx}"
            ));
            extra_vals.push(format!("%,{},%", tn));
        }
        if let Some(ta) = time_after {
            extra_conds.push(format!("m.timestamp >= ?{}", 3 + extra_vals.len()));
            extra_vals.push(ta.to_string());
        }

        let where_clause = extra_conds.join(" AND ");

        let fts_sql = format!(
            "SELECT m.* FROM conversation_messages m
             JOIN conversation_msg_fts f ON m.id = f.rowid
             JOIN conversations c ON m.session_id = c.id
             WHERE conversation_msg_fts MATCH ?1
               AND {where_clause}
             ORDER BY f.rank
             LIMIT ?2"
        );

        let like_sql = format!(
            "SELECT m.* FROM conversation_messages m
             JOIN conversations c ON m.session_id = c.id
             WHERE m.content LIKE ?1
               AND {where_clause}
             ORDER BY m.timestamp DESC LIMIT ?2"
        );

        // Build rusqlite::types::Value vector for dynamic params
        fn make_params(query_val: &str, limit: i64, extra: &[String]) -> Vec<rusqlite::types::Value> {
            let mut v: Vec<rusqlite::types::Value> = Vec::new();
            v.push(rusqlite::types::Value::Text(query_val.to_string()));
            v.push(rusqlite::types::Value::Integer(limit));
            for s in extra {
                v.push(rusqlite::types::Value::Text(s.clone()));
            }
            v
        }

        fn exec_query(conn: &rusqlite::Connection, sql: &str, params: &[rusqlite::types::Value]) -> DbResult<Vec<ConversationMessage>> {
            let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p as &dyn rusqlite::types::ToSql).collect();
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(param_refs.as_slice(), |row| MissionDB::row_to_conversation_message(row))?;
            let mut msgs = Vec::new();
            for m in rows { msgs.push(m?); }
            Ok(msgs)
        }

        // Phase 1: AND
        let params = make_params(&and_query, limit, &extra_vals);
        let results = exec_query(&conn, &fts_sql, &params)?;
        if !results.is_empty() { return Ok(results); }

        // Phase 2: OR
        let params = make_params(&or_query, limit, &extra_vals);
        let results = exec_query(&conn, &fts_sql, &params)?;
        if !results.is_empty() { return Ok(results); }

        // Phase 3: LIKE fallback
        let pattern = format!("%{}%", query);
        let params = make_params(&pattern, limit, &extra_vals);
        exec_query(&conn, &like_sql, &params)
    }

    /// FTS5 search grouped by session — returns (session_id, best_bm25_score) ranked by BM25.
    /// Uses AND-first strategy: try all terms AND, fall back to OR, then LIKE.
    pub fn search_conversation_sessions_fts(&self, query: &str, limit: i64) -> DbResult<Vec<(String, f64)>> {
        let conn = self.read_conn();
        let (and_query, or_query) = Self::build_conv_fts_queries(query);

        // Exclude meta/compaction conversations from search results
        let bm25_sql =
            "SELECT m.session_id, MIN(f.rank) as best_score
             FROM conversation_messages m
             JOIN conversation_msg_fts f ON m.id = f.rowid
             JOIN conversations c ON m.session_id = c.id
             WHERE conversation_msg_fts MATCH ?1
               AND c.conversation_type NOT IN ('meta', 'compaction')
             GROUP BY m.session_id
             ORDER BY best_score ASC
             LIMIT ?2";

        // Phase 1: AND — all terms must appear
        let results = {
            let mut stmt = conn.prepare(bm25_sql)?;
            let rows = stmt.query_map(params![and_query, limit], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?;
            let mut r = Vec::new();
            for row in rows { r.push(row?); }
            r
        };
        if !results.is_empty() {
            return Ok(results);
        }

        // Phase 2: OR — any term matches
        let results = {
            let mut stmt = conn.prepare(bm25_sql)?;
            let rows = stmt.query_map(params![or_query, limit], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?;
            let mut r = Vec::new();
            for row in rows { r.push(row?); }
            r
        };
        if !results.is_empty() {
            return Ok(results);
        }

        // Phase 3: LIKE fallback for Chinese substrings
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT m.session_id, COUNT(*) as hits FROM conversation_messages m
             JOIN conversations c ON m.session_id = c.id
             WHERE m.content LIKE ?1
               AND c.conversation_type NOT IN ('meta', 'compaction')
             GROUP BY m.session_id
             ORDER BY hits DESC
             LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![pattern, limit], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })?;
        let mut results = Vec::new();
        for r in rows { results.push(r?); }
        Ok(results)
    }

    /// Get FTS5 snippet() matches for a specific session — returns top matching message snippets.
    /// Uses FTS5 native snippet() for efficient, tokenizer-aligned context extraction.
    pub fn get_session_fts_snippets(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> DbResult<Vec<(String, String)>> {
        let conn = self.read_conn();
        let (and_query, or_query) = Self::build_conv_fts_queries(query);

        let sql = "SELECT m.role, snippet(conversation_msg_fts, 0, '**', '**', '…', 48) as snip
             FROM conversation_messages m
             JOIN conversation_msg_fts f ON m.id = f.rowid
             WHERE conversation_msg_fts MATCH ?1 AND m.session_id = ?2
             ORDER BY f.rank
             LIMIT ?3";

        // Try AND first
        let results = {
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(params![and_query, session_id, limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut r = Vec::new();
            for row in rows { r.push(row?); }
            r
        };
        if !results.is_empty() { return Ok(results); }

        // Fallback to OR
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![or_query, session_id, limit as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut r = Vec::new();
        for row in rows { r.push(row?); }
        Ok(r)
    }

    /// FTS5 session search with time range filter.
    pub fn search_conversation_sessions_fts_filtered(
        &self,
        query: &str,
        limit: i64,
        time_after: Option<&str>,
        project: Option<&str>,
    ) -> DbResult<Vec<(String, f64)>> {
        let conn = self.read_conn();
        let (and_query, or_query) = Self::build_conv_fts_queries(query);

        let mut conditions = vec![
            "conversation_msg_fts MATCH ?1".to_string(),
            "c.conversation_type NOT IN ('meta', 'compaction')".to_string(),
        ];
        let mut param_idx = 2; // ?1 = query, ?2 = limit (at end)

        if time_after.is_some() {
            param_idx += 1;
            conditions.push(format!("c.started_at >= ?{}", param_idx));
        }
        if project.is_some() {
            param_idx += 1;
            conditions.push(format!("c.project = ?{}", param_idx));
        }

        let where_clause = conditions.join(" AND ");
        let sql = format!(
            "SELECT m.session_id, MIN(f.rank) as best_score
             FROM conversation_messages m
             JOIN conversation_msg_fts f ON m.id = f.rowid
             JOIN conversations c ON m.session_id = c.id
             WHERE {}
             GROUP BY m.session_id
             ORDER BY best_score ASC
             LIMIT ?2",
            where_clause
        );

        // Try AND first, then OR
        for q in [&and_query, &or_query] {
            let mut stmt = conn.prepare(&sql)?;
            let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
                Box::new(q.clone()),
                Box::new(limit),
            ];
            if let Some(ta) = time_after {
                params_vec.push(Box::new(ta.to_string()));
            }
            if let Some(p) = project {
                params_vec.push(Box::new(p.to_string()));
            }
            let param_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
            let rows = stmt.query_map(param_refs.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?;
            let mut r = Vec::new();
            for row in rows { r.push(row?); }
            if !r.is_empty() { return Ok(r); }
        }

        // Phase 3: LIKE fallback
        let pattern = format!("%{}%", query);
        let mut like_conditions = vec![
            "m.content LIKE ?1".to_string(),
            "c.conversation_type NOT IN ('meta', 'compaction')".to_string(),
        ];
        if time_after.is_some() {
            like_conditions.push("c.started_at >= ?3".to_string());
        }
        if project.is_some() {
            let idx = if time_after.is_some() { 4 } else { 3 };
            like_conditions.push(format!("c.project = ?{}", idx));
        }
        let like_sql = format!(
            "SELECT m.session_id, COUNT(*) as hits FROM conversation_messages m
             JOIN conversations c ON m.session_id = c.id
             WHERE {}
             GROUP BY m.session_id
             ORDER BY hits DESC LIMIT ?2",
            like_conditions.join(" AND ")
        );
        let mut stmt = conn.prepare(&like_sql)?;
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
            Box::new(pattern),
            Box::new(limit),
        ];
        if let Some(ta) = time_after {
            params_vec.push(Box::new(ta.to_string()));
        }
        if let Some(p) = project {
            params_vec.push(Box::new(p.to_string()));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })?;
        let mut r = Vec::new();
        for row in rows { r.push(row?); }
        Ok(r)
    }

    /// Build FTS5 MATCH queries: AND (strict) + OR (loose) variants.
    /// Returns (and_query, or_query). Caller tries AND first, falls back to OR.
    fn build_conv_fts_queries(query: &str) -> (String, String) {
        let terms: Vec<String> = query.split_whitespace()
            .map(|t| format!("\"{}\"", t.replace('"', "")))
            .collect();
        if terms.is_empty() {
            let q = format!("\"{}\"", query.replace('"', ""));
            (q.clone(), q)
        } else {
            (terms.join(" AND "), terms.join(" OR "))
        }
    }

    /// Store LLM-generated summary for a conversation.
    pub fn set_conversation_summary(&self, id: &str, summary: &str) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET llm_summary = ?1 WHERE id = ?2",
            params![summary, id],
        )?;
        Ok(())
    }

    /// Clear conversation summary + embedding so it can be regenerated (e.g., after timeline build).
    pub fn clear_conversation_summary(&self, id: &str) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE conversations SET llm_summary = NULL, embedding = NULL, embedding_provider = NULL WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Store conversation embedding + provider identifier.
    pub fn set_conversation_embedding(&self, id: &str, embedding: &[f32], provider: &str) -> DbResult<()> {
        let conn = self.conn();
        let bytes = crate::embedding::f32_vec_to_bytes(embedding);
        conn.execute(
            "UPDATE conversations SET embedding = ?1, embedding_provider = ?2 WHERE id = ?3",
            params![bytes, provider, id],
        )?;
        Ok(())
    }

    /// Load all conversation embeddings matching a specific provider. For cache warming.
    pub fn load_conversation_embeddings(&self, provider: &str) -> DbResult<Vec<(String, Vec<f32>)>> {
        let conn = self.read_conn();
        // Exclude meta/compaction from search embedding cache — they pollute results
        let mut stmt = conn.prepare(
            "SELECT id, embedding FROM conversations
             WHERE embedding IS NOT NULL AND embedding_provider = ?1
               AND conversation_type NOT IN ('meta', 'compaction')"
        )?;
        let rows = stmt.query_map(params![provider], |row| {
            let id: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((id, blob))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (id, blob) = row?;
            result.push((id, crate::embedding::bytes_to_f32_vec(&blob)));
        }
        Ok(result)
    }

    /// List conversation IDs that are completed but missing LLM summary (for backfill).
    /// Excludes meta/compaction/subagent — only user and worker conversations get independent summaries.
    pub fn conversations_missing_summary(&self, limit: i64) -> DbResult<Vec<String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id FROM conversations
             WHERE llm_summary IS NULL AND status = 'completed' AND message_count >= 6
               AND conversation_type IN ('user', 'worker')
             ORDER BY started_at DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for r in rows { ids.push(r?); }
        Ok(ids)
    }

    /// List conversation IDs with embeddings from a different provider (for re-embedding on provider switch).
    pub fn conversations_stale_embedding(&self, current_provider: &str, limit: i64) -> DbResult<Vec<String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id FROM conversations
             WHERE llm_summary IS NOT NULL AND embedding_provider IS NOT NULL
               AND embedding_provider != ?1
               AND conversation_type IN ('user', 'worker')
             ORDER BY started_at DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![current_provider, limit], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for r in rows { ids.push(r?); }
        Ok(ids)
    }


    // ============ Multi-Topic Vectors (MaxSim Search) ============

    /// Store topic vectors for a conversation session (replaces any existing).
    /// topics: [(topic_summary, embedding_vector)]
    pub fn set_conversation_topic_vectors(
        &self,
        session_id: &str,
        topics: &[(String, Vec<f32>)],
        provider: &str,
    ) -> DbResult<()> {
        let conn = self.conn();
        // Delete old topic vectors for this session
        conn.execute(
            "DELETE FROM conversation_topic_vectors WHERE session_id = ?1",
            params![session_id],
        )?;
        let mut stmt = conn.prepare(
            "INSERT INTO conversation_topic_vectors (session_id, chunk_idx, topic, embedding, embedding_provider)
             VALUES (?1, ?2, ?3, ?4, ?5)"
        )?;
        for (idx, (topic, vec)) in topics.iter().enumerate() {
            let bytes = crate::embedding::f32_vec_to_bytes(vec);
            stmt.execute(params![session_id, idx as i64, topic, bytes, provider])?;
        }
        Ok(())
    }

    /// Load all topic vectors for cache warming. Returns (session_id, [vectors]).
    /// Groups by session_id, ordered by chunk_idx.
    pub fn load_conversation_topic_vectors(&self, provider: &str) -> DbResult<Vec<(String, Vec<Vec<f32>>)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT tv.session_id, tv.embedding
             FROM conversation_topic_vectors tv
             JOIN conversations c ON c.id = tv.session_id
             WHERE tv.embedding_provider = ?1
               AND c.conversation_type NOT IN ('meta', 'compaction')
             ORDER BY tv.session_id, tv.chunk_idx"
        )?;
        let rows = stmt.query_map(params![provider], |row| {
            let sid: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((sid, blob))
        })?;

        let mut result: Vec<(String, Vec<Vec<f32>>)> = Vec::new();
        for row in rows {
            let (sid, blob) = row?;
            let vec = crate::embedding::bytes_to_f32_vec(&blob);
            if let Some(last) = result.last_mut() {
                if last.0 == sid {
                    last.1.push(vec);
                    continue;
                }
            }
            result.push((sid, vec![vec]));
        }
        Ok(result)
    }

    /// Get topic summaries for a session (for display in search results).
    pub fn get_conversation_topics(&self, session_id: &str) -> DbResult<Vec<String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT topic FROM conversation_topic_vectors
             WHERE session_id = ?1 ORDER BY chunk_idx"
        )?;
        let rows = stmt.query_map(params![session_id], |row| row.get::<_, String>(0))?;
        let mut topics = Vec::new();
        for r in rows { topics.push(r?); }
        Ok(topics)
    }

    /// List session IDs that have old single-vector embeddings but no topic vectors yet.
    /// Used for gradual migration during backfill.
    pub fn conversations_needing_topic_vectors(&self, provider: &str, limit: i64) -> DbResult<Vec<String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT c.id FROM conversations c
             WHERE c.llm_summary IS NOT NULL
               AND c.conversation_type IN ('user', 'worker')
               AND NOT EXISTS (
                   SELECT 1 FROM conversation_topic_vectors tv
                   WHERE tv.session_id = c.id AND tv.embedding_provider = ?1
               )
             ORDER BY c.started_at DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![provider, limit], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for r in rows { ids.push(r?); }
        Ok(ids)
    }

    // ============ Session Timeline Reconstruction ============

    /// Find completed parent conversations that have compaction children but no timeline yet.
    pub fn conversations_needing_timeline(&self, limit: i64) -> DbResult<Vec<String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT c.parent_session_id
             FROM conversations c
             JOIN conversations p ON p.id = c.parent_session_id
             WHERE c.conversation_type = 'compaction'
               AND p.status = 'completed'
               AND p.timeline_built_at IS NULL
             LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for r in rows { ids.push(r?); }
        Ok(ids)
    }

    /// Get compaction fragments for a parent session, ordered by started_at.
    /// Returns (fragment_id, started_at, message_count).
    pub fn get_compaction_fragments(&self, parent_id: &str) -> DbResult<Vec<(String, String, i64)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, started_at, message_count FROM conversations
             WHERE parent_session_id = ?1 AND conversation_type = 'compaction'
             ORDER BY started_at ASC, id ASC"
        )?;
        let rows = stmt.query_map(params![parent_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Get the last assistant message content from a conversation (for compaction summary extraction).
    pub fn get_last_assistant_content(&self, session_id: &str) -> DbResult<Option<String>> {
        let conn = self.read_conn();
        Ok(conn.query_row(
            "SELECT content FROM conversation_messages
             WHERE session_id = ?1 AND role = 'assistant'
             ORDER BY id DESC LIMIT 1",
            params![session_id],
            |row| row.get::<_, String>(0),
        ).optional()?)
    }

    /// Write session_timeline JSON and set timeline_built_at (CAS: only if timeline_built_at IS NULL).
    pub fn set_session_timeline(&self, parent_id: &str, timeline_json: &str) -> DbResult<bool> {
        let conn = self.conn();
        let updated = conn.execute(
            "UPDATE conversations SET session_timeline = ?1, timeline_built_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?2 AND timeline_built_at IS NULL",
            params![timeline_json, parent_id],
        )?;
        Ok(updated > 0)
    }

    pub(crate) fn row_to_conversation(row: &rusqlite::Row) -> rusqlite::Result<Conversation> {
        Ok(Conversation {
            id: row.get("id")?,
            project: row.get("project")?,
            slot_id: row.get("slot_id")?,
            source: row.get("source")?,
            model: row.get("model")?,
            git_branch: row.get("git_branch")?,
            jsonl_path: row.get("jsonl_path")?,
            parent_session_id: row.get("parent_session_id").unwrap_or(None),
            task_id: row.get("task_id").unwrap_or(None),
            message_count: row.get("message_count")?,
            started_at: row.get("started_at")?,
            ended_at: row.get("ended_at")?,
            status: row.get("status")?,
            analyzed_at: row.get("analyzed_at")?,
            analysis_version: row.get("analysis_version").unwrap_or(0),
            analysis_retries: row.get("analysis_retries").unwrap_or(0),
            deep_analyzed_message_id: row.get("deep_analyzed_message_id").unwrap_or(0),
            chat_type: row.get("chat_type").unwrap_or(None),
            conversation_type: row.get("conversation_type").unwrap_or_else(|_| "user".to_string()),
            updated_at: row.get("updated_at").unwrap_or(None),
            llm_summary: row.get("llm_summary").unwrap_or(None),
            embedding_provider: row.get("embedding_provider").unwrap_or(None),
            session_timeline: row.get("session_timeline").unwrap_or(None),
            timeline_built_at: row.get("timeline_built_at").unwrap_or(None),
        })
    }

    pub(crate) fn row_to_conversation_message(row: &rusqlite::Row) -> rusqlite::Result<ConversationMessage> {
        Ok(ConversationMessage {
            id: row.get("id")?,
            session_id: row.get("session_id")?,
            role: row.get("role")?,
            content: row.get("content")?,
            raw_content: row.get("raw_content")?,
            message_uuid: row.get("message_uuid")?,
            parent_uuid: row.get("parent_uuid")?,
            model: row.get("model")?,
            timestamp: row.get("timestamp")?,
            metadata: row.get("metadata")?,
            tool_name: row.get("tool_name").unwrap_or(None),
        })
    }
}
