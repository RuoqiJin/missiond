use rusqlite::{params, OptionalExtension, Result as SqliteResult};
use crate::types::*;
use super::MissionDB;

impl MissionDB {
    // ============ Skill Engine ============

    /// Upsert a skill topic (metadata)
    pub fn skill_topic_upsert(
        &self,
        topic: &str,
        description: Option<&str>,
        aka: Option<&str>,
        allowed_tools: Option<&str>,
        file_path: &str,
        requires_json: Option<&str>,
        actions_json: Option<&str>,
    ) -> SqliteResult<()> {
        self.skill_topic_upsert_full(topic, description, aka, allowed_tools, file_path, requires_json, actions_json, None)
    }

    /// Upsert a skill topic with context_hooks (Phase 4)
    pub fn skill_topic_upsert_full(
        &self,
        topic: &str,
        description: Option<&str>,
        aka: Option<&str>,
        allowed_tools: Option<&str>,
        file_path: &str,
        requires_json: Option<&str>,
        actions_json: Option<&str>,
        context_hooks_json: Option<&str>,
    ) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO skill_topics (topic, description, aka, allowed_tools, file_path, requires_json, actions_json, context_hooks_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
             ON CONFLICT(topic) DO UPDATE SET
                description = ?2, aka = ?3, allowed_tools = ?4, file_path = ?5, requires_json = ?6, actions_json = ?7, context_hooks_json = ?8, updated_at = ?9",
            params![topic, description, aka, allowed_tools, file_path, requires_json, actions_json, context_hooks_json, now],
        )?;
        Ok(())
    }

    /// Get a skill topic by name
    pub fn skill_topic_get(&self, topic: &str) -> SqliteResult<Option<SkillTopic>> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT topic, description, aka, allowed_tools, file_path,
                    hit_count, last_hit_at, fragment_count, total_lines, checksum,
                    requires_json, actions_json, context_hooks_json, created_at, updated_at
             FROM skill_topics WHERE topic = ?1",
            params![topic],
            |row| {
                Ok(SkillTopic {
                    topic: row.get(0)?,
                    description: row.get(1)?,
                    aka: row.get(2)?,
                    allowed_tools: row.get(3)?,
                    file_path: row.get(4)?,
                    hit_count: row.get(5)?,
                    last_hit_at: row.get(6)?,
                    fragment_count: row.get(7)?,
                    total_lines: row.get(8)?,
                    checksum: row.get(9)?,
                    requires_json: row.get(10)?,
                    actions_json: row.get(11)?,
                    context_hooks_json: row.get(12)?,
                    created_at: row.get(13)?,
                    updated_at: row.get(14)?,
                })
            },
        )
        .optional()
    }

    /// List all skill topics
    pub fn skill_topic_list(&self) -> SqliteResult<Vec<SkillTopic>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT topic, description, aka, allowed_tools, file_path,
                    hit_count, last_hit_at, fragment_count, total_lines, checksum,
                    requires_json, actions_json, context_hooks_json, created_at, updated_at
             FROM skill_topics ORDER BY topic",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SkillTopic {
                topic: row.get(0)?,
                description: row.get(1)?,
                aka: row.get(2)?,
                allowed_tools: row.get(3)?,
                file_path: row.get(4)?,
                hit_count: row.get(5)?,
                last_hit_at: row.get(6)?,
                fragment_count: row.get(7)?,
                total_lines: row.get(8)?,
                checksum: row.get(9)?,
                requires_json: row.get(10)?,
                actions_json: row.get(11)?,
                context_hooks_json: row.get(12)?,
                created_at: row.get(13)?,
                updated_at: row.get(14)?,
            })
        })?;
        rows.collect()
    }

    /// Record a hit on a skill topic (for Hook injection tracking)
    pub fn skill_topic_hit(&self, topic: &str) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE skill_topics SET hit_count = hit_count + 1, last_hit_at = ?1 WHERE topic = ?2",
            params![now, topic],
        )?;
        Ok(())
    }

    /// Insert a skill block (section or fragment)
    pub fn skill_block_insert(
        &self,
        topic: &str,
        block_type: &str,
        title: Option<&str>,
        content: &str,
        sort_order: i32,
    ) -> SqliteResult<String> {
        let conn = self.conn();
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO skill_blocks (id, topic, block_type, title, content, sort_order, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?7)",
            params![id, topic, block_type, title, content, sort_order, now],
        )?;

        // Sync FTS
        let rowid: i64 = conn.query_row(
            "SELECT rowid FROM skill_blocks WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO skill_fts(rowid, topic, title, content) VALUES (?1, ?2, ?3, ?4)",
            params![rowid, topic, title.unwrap_or(""), content],
        )?;

        // Update fragment count if fragment
        if block_type == "fragment" {
            conn.execute(
                "UPDATE skill_topics SET fragment_count = fragment_count + 1 WHERE topic = ?1",
                params![topic],
            )?;
        }

        Ok(id)
    }

    /// Update a skill block's content
    pub fn skill_block_update(&self, id: &str, content: &str) -> SqliteResult<bool> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        let updated = conn.execute(
            "UPDATE skill_blocks SET content = ?1, updated_at = ?2 WHERE id = ?3 AND status = 'active'",
            params![content, now, id],
        )?;
        if updated > 0 {
            // Rebuild FTS for this block
            let rowid: i64 = conn.query_row(
                "SELECT rowid FROM skill_blocks WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )?;
            let (topic, title): (String, Option<String>) = conn.query_row(
                "SELECT topic, title FROM skill_blocks WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO skill_fts(rowid, topic, title, content) VALUES (?1, ?2, ?3, ?4)",
                params![rowid, topic, title.unwrap_or_default(), content],
            )?;
        }
        Ok(updated > 0)
    }

    /// Get all active blocks for a topic, ordered by sort_order
    pub fn skill_blocks_for_topic(&self, topic: &str) -> SqliteResult<Vec<SkillBlock>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, topic, block_type, title, content, sort_order, status, created_at, updated_at
             FROM skill_blocks
             WHERE topic = ?1 AND status = 'active'
             ORDER BY block_type DESC, sort_order ASC",
        )?;
        let rows = stmt.query_map(params![topic], |row| {
            Ok(SkillBlock {
                id: row.get(0)?,
                topic: row.get(1)?,
                block_type: row.get(2)?,
                title: row.get(3)?,
                content: row.get(4)?,
                sort_order: row.get(5)?,
                status: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;
        rows.collect()
    }

    /// Search skill blocks using FTS5
    pub fn skill_search_fts(&self, query: &str) -> SqliteResult<Vec<SkillSearchResult>> {
        let conn = self.read_conn();
        let tokens: Vec<&str> = query.split_whitespace().collect();
        if tokens.is_empty() {
            return Ok(vec![]);
        }

        let fts_query = tokens
            .iter()
            .map(|t| format!("\"{}\"", t.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" OR ");

        let mut stmt = conn.prepare(
            "SELECT sb.topic, sb.title, snippet(skill_fts, 2, '>>>', '<<<', '...', 32) as snippet,
                    st.file_path, st.description
             FROM skill_fts
             JOIN skill_blocks sb ON sb.rowid = skill_fts.rowid
             JOIN skill_topics st ON st.topic = sb.topic
             WHERE skill_fts MATCH ?1 AND sb.status = 'active'
             ORDER BY rank
             LIMIT 20",
        )?;

        let rows = stmt.query_map(params![fts_query], |row| {
            Ok(SkillSearchResult {
                topic: row.get(0)?,
                section_title: row.get(1)?,
                snippet: row.get(2)?,
                file_path: row.get(3)?,
                description: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// Search skill topics by name/aka (exact + fuzzy)
    pub fn skill_search_topics(&self, query: &str) -> SqliteResult<Vec<SkillTopic>> {
        let conn = self.read_conn();
        let query_lower = query.to_lowercase();
        let pattern = format!("%{}%", query_lower);

        let mut stmt = conn.prepare(
            "SELECT topic, description, aka, allowed_tools, file_path,
                    hit_count, last_hit_at, fragment_count, total_lines, checksum,
                    requires_json, actions_json, context_hooks_json, created_at, updated_at
             FROM skill_topics
             WHERE LOWER(topic) LIKE ?1
                OR LOWER(COALESCE(description, '')) LIKE ?1
                OR LOWER(COALESCE(aka, '')) LIKE ?1
             ORDER BY hit_count DESC
             LIMIT 10",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok(SkillTopic {
                topic: row.get(0)?,
                description: row.get(1)?,
                aka: row.get(2)?,
                allowed_tools: row.get(3)?,
                file_path: row.get(4)?,
                hit_count: row.get(5)?,
                last_hit_at: row.get(6)?,
                fragment_count: row.get(7)?,
                total_lines: row.get(8)?,
                checksum: row.get(9)?,
                requires_json: row.get(10)?,
                actions_json: row.get(11)?,
                context_hooks_json: row.get(12)?,
                created_at: row.get(13)?,
                updated_at: row.get(14)?,
            })
        })?;
        rows.collect()
    }

    /// Update skill_topics metadata after materialization
    pub fn skill_topic_update_stats(&self, topic: &str, total_lines: i32, checksum: &str) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE skill_topics SET total_lines = ?1, checksum = ?2, updated_at = ?3 WHERE topic = ?4",
            params![total_lines, checksum, now, topic],
        )?;
        Ok(())
    }

    /// Set block status (for lifecycle management: active → merged/archived)
    pub fn skill_block_set_status(&self, id: &str, status: &str) -> SqliteResult<bool> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        let updated = conn.execute(
            "UPDATE skill_blocks SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, now, id],
        )?;
        Ok(updated > 0)
    }

    /// Delete all blocks for a topic (used before re-ingest)
    pub fn skill_blocks_delete_topic(&self, topic: &str) -> SqliteResult<usize> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM skill_blocks WHERE topic = ?1",
            params![topic],
        )?;
        Ok(deleted)
    }

    /// Rebuild skill FTS index from scratch
    pub fn skill_rebuild_fts(&self) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute_batch("INSERT INTO skill_fts(skill_fts) VALUES('rebuild')")?;
        Ok(())
    }

    // ── Skill Embedding functions ──────────────────────────────────

    /// Store embedding BLOB + provider tag for a skill topic
    pub fn skill_set_topic_embedding(&self, topic: &str, embedding: &[f32], provider: &str) -> SqliteResult<()> {
        let conn = self.conn();
        let bytes = crate::embedding::f32_vec_to_bytes(embedding);
        conn.execute(
            "UPDATE skill_topics SET embedding = ?1, embedding_provider = ?2 WHERE topic = ?3",
            params![bytes, provider, topic],
        )?;
        Ok(())
    }

    /// Load all topic embeddings (for cache warmup)
    pub fn skill_load_topic_embeddings(&self) -> SqliteResult<Vec<(String, Vec<f32>)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT topic, embedding FROM skill_topics WHERE embedding IS NOT NULL"
        )?;
        let rows = stmt.query_map([], |row| {
            let topic: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((topic, blob))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (topic, blob) = row?;
            result.push((topic, crate::embedding::bytes_to_f32_vec(&blob)));
        }
        Ok(result)
    }

    /// List skill topics missing embedding (for backfill).
    /// Returns (topic, embed_text) where embed_text = "技能主题：{topic}\n{description}\n{all active block content}"
    pub fn skill_topics_missing_embedding(&self, limit: i64) -> SqliteResult<Vec<(String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT topic, COALESCE(description, '') FROM skill_topics WHERE embedding IS NULL LIMIT ?1"
        )?;
        let topics: Vec<(String, String)> = stmt.query_map(params![limit], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?.filter_map(|r| r.ok()).collect();

        let mut results = Vec::new();
        for (topic, description) in &topics {
            // Gather all active block content for this topic
            let mut block_stmt = conn.prepare(
                "SELECT COALESCE(title, ''), content FROM skill_blocks
                 WHERE topic = ?1 AND status = 'active' ORDER BY sort_order"
            )?;
            let blocks: Vec<(String, String)> = block_stmt.query_map(params![topic], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?.filter_map(|r| r.ok()).collect();

            let mut embed_text = format!("技能主题：{}\n{}", topic, description);
            for (title, content) in &blocks {
                if !title.is_empty() {
                    embed_text.push_str(&format!("\n{}", title));
                }
                // Truncate large blocks to keep embed text manageable
                let truncated = if content.len() > 1000 {
                    &content[..crate::embedding::char_boundary_at(content, 1000)]
                } else {
                    content.as_str()
                };
                embed_text.push_str(&format!("\n{}", truncated));
            }
            // Cap total embed text at 4000 chars
            if embed_text.len() > 4000 {
                embed_text.truncate(crate::embedding::char_boundary_at(&embed_text, 4000));
            }
            results.push((topic.clone(), embed_text));
        }
        Ok(results)
    }

    /// List skill topics with embedding from a different provider (stale after model switch)
    pub fn skill_topics_stale_embedding(&self, current_provider: &str, limit: i64) -> SqliteResult<Vec<(String, String)>> {
        let conn = self.read_conn();
        // Get stale topics, then build embed_text same as missing
        let mut stmt = conn.prepare(
            "SELECT topic, COALESCE(description, '') FROM skill_topics
             WHERE embedding IS NOT NULL AND embedding_provider IS NOT NULL
               AND embedding_provider != ?1
             LIMIT ?2"
        )?;
        let topics: Vec<(String, String)> = stmt.query_map(params![current_provider, limit], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?.filter_map(|r| r.ok()).collect();

        let mut results = Vec::new();
        for (topic, description) in &topics {
            let mut block_stmt = conn.prepare(
                "SELECT COALESCE(title, ''), content FROM skill_blocks
                 WHERE topic = ?1 AND status = 'active' ORDER BY sort_order"
            )?;
            let blocks: Vec<(String, String)> = block_stmt.query_map(params![topic], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?.filter_map(|r| r.ok()).collect();

            let mut embed_text = format!("技能主题：{}\n{}", topic, description);
            for (title, content) in &blocks {
                if !title.is_empty() {
                    embed_text.push_str(&format!("\n{}", title));
                }
                let truncated = if content.len() > 1000 {
                    &content[..crate::embedding::char_boundary_at(content, 1000)]
                } else {
                    content.as_str()
                };
                embed_text.push_str(&format!("\n{}", truncated));
            }
            if embed_text.len() > 4000 {
                embed_text.truncate(crate::embedding::char_boundary_at(&embed_text, 4000));
            }
            results.push((topic.clone(), embed_text));
        }
        Ok(results)
    }

    /// Insert a skill execution log entry
    pub fn skill_execution_insert(
        &self,
        id: &str,
        skill_topic: &str,
        action_id: &str,
        steps_total: i32,
        triggered_by: &str,
    ) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO skill_executions (id, skill_topic, action_id, status, steps_total, triggered_by, created_at)
             VALUES (?1, ?2, ?3, 'running', ?4, ?5, ?6)",
            params![id, skill_topic, action_id, steps_total, triggered_by, now],
        )?;
        Ok(())
    }

    /// Update a skill execution (progress or completion)
    pub fn skill_execution_update(
        &self,
        id: &str,
        status: &str,
        steps_completed: i32,
        context_json: Option<&str>,
        error: Option<&str>,
    ) -> SqliteResult<()> {
        self.skill_execution_update_with_duration(id, status, steps_completed, context_json, error, None)
    }

    /// Update a skill execution with optional duration tracking
    pub fn skill_execution_update_with_duration(
        &self,
        id: &str,
        status: &str,
        steps_completed: i32,
        context_json: Option<&str>,
        error: Option<&str>,
        duration_ms: Option<i64>,
    ) -> SqliteResult<()> {
        let conn = self.conn();
        let completed_at = if status == "success" || status == "failed" || status == "cancelled" {
            Some(chrono::Utc::now().to_rfc3339())
        } else {
            None
        };
        conn.execute(
            "UPDATE skill_executions SET status = ?1, steps_completed = ?2, context_json = ?3, error = ?4, completed_at = ?5, duration_ms = ?6
             WHERE id = ?7",
            params![status, steps_completed, context_json, error, completed_at, duration_ms, id],
        )?;
        Ok(())
    }

    /// Get aggregated execution statistics for a skill topic
    pub fn skill_execution_stats(&self, topic: Option<&str>) -> SqliteResult<Vec<crate::types::SkillExecutionStat>> {
        let conn = self.read_conn();
        let row_mapper = |row: &rusqlite::Row| {
            Ok(crate::types::SkillExecutionStat {
                action_id: row.get(0)?,
                total: row.get(1)?,
                successes: row.get(2)?,
                failures: row.get(3)?,
                avg_duration_ms: row.get(4)?,
                last_run: row.get(5)?,
            })
        };

        let mut stats = Vec::new();
        if let Some(t) = topic {
            let mut stmt = conn.prepare(
                "SELECT action_id, COUNT(*) as total,
                        SUM(CASE WHEN status='success' THEN 1 ELSE 0 END) as successes,
                        SUM(CASE WHEN status='failed' THEN 1 ELSE 0 END) as failures,
                        AVG(duration_ms) as avg_duration_ms,
                        MAX(created_at) as last_run
                 FROM skill_executions
                 WHERE skill_topic = ?1
                 GROUP BY action_id
                 ORDER BY last_run DESC"
            )?;
            let rows = stmt.query_map(params![t], row_mapper)?;
            for s in rows { stats.push(s?); }
        } else {
            let mut stmt = conn.prepare(
                "SELECT skill_topic || '/' || action_id as action_id, COUNT(*) as total,
                        SUM(CASE WHEN status='success' THEN 1 ELSE 0 END) as successes,
                        SUM(CASE WHEN status='failed' THEN 1 ELSE 0 END) as failures,
                        AVG(duration_ms) as avg_duration_ms,
                        MAX(created_at) as last_run
                 FROM skill_executions
                 GROUP BY skill_topic, action_id
                 ORDER BY last_run DESC"
            )?;
            let rows = stmt.query_map([], row_mapper)?;
            for s in rows { stats.push(s?); }
        }
        Ok(stats)
    }

    /// Check if a skill action is currently running (for concurrency guard)
    pub fn skill_execution_is_running(&self, skill_topic: &str, action_id: &str) -> SqliteResult<bool> {
        let conn = self.read_conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM skill_executions WHERE skill_topic = ?1 AND action_id = ?2 AND status = 'running'",
            params![skill_topic, action_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }


    // ============ Skill Version Rollback (Phase 4) ============

    /// Save a skill version snapshot before materialize
    pub fn skill_version_save(&self, topic: &str, content: &str, checksum: &str) -> SqliteResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO skill_versions (topic, content, checksum, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![topic, content, checksum, now],
        )?;
        // Keep only the 10 most recent versions per topic
        conn.execute(
            "DELETE FROM skill_versions WHERE topic = ?1 AND id NOT IN (
                SELECT id FROM skill_versions WHERE topic = ?1 ORDER BY id DESC LIMIT 10
            )",
            params![topic],
        )?;
        Ok(())
    }

    /// List recent versions for a skill topic
    pub fn skill_version_list(&self, topic: &str, limit: i64) -> SqliteResult<Vec<crate::types::SkillVersion>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, topic, content, checksum, created_at FROM skill_versions
             WHERE topic = ?1 ORDER BY id DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![topic, limit], |row| {
            Ok(crate::types::SkillVersion {
                id: row.get(0)?,
                topic: row.get(1)?,
                content: row.get(2)?,
                checksum: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        let mut versions = Vec::new();
        for v in rows { versions.push(v?); }
        Ok(versions)
    }

    /// Get a specific skill version by ID
    pub fn skill_version_get(&self, id: i64) -> SqliteResult<Option<crate::types::SkillVersion>> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT id, topic, content, checksum, created_at FROM skill_versions WHERE id = ?1",
            params![id],
            |row| {
                Ok(crate::types::SkillVersion {
                    id: row.get(0)?,
                    topic: row.get(1)?,
                    content: row.get(2)?,
                    checksum: row.get(3)?,
                    created_at: row.get(4)?,
                })
            },
        ).optional()
    }


}
