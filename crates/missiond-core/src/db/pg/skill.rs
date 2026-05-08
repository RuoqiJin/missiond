//! ProjectStore — PostgreSQL implementation (unified per memory pillar v0.4.23).
//!
//! Covers the full ProjectStore surface for PG backend:
//!   - projects table CRUD (7 methods, helpers in `pg/project.rs`)
//!   - skill topics, blocks, FTS search, embeddings, executions, versions
//!   - beacons, AST index
//!
//! Note: Rust coherence requires a single `impl ProjectStore for PgMissionStore`
//! block per crate. All 32 methods live here; `pg/project.rs` provides
//! free-function helpers that delegate to sqlx directly.

use super::project::{
    backfill_project_id, conversation_stats_by_project, get_project, kb_stats_by_project,
    list_projects, recent_conversations_by_project, update_project_active, upsert_project,
};
use super::PgMissionStore;
use crate::db::error::DbResult;
use crate::db::traits::{
    AstNodeRow, AstSearchHit, AstStats, AstSyncResult, BeaconInfo, BeaconNode, CodeNode,
    ModuleAstSummary, ProjectStore,
};
use crate::types::*;
use async_trait::async_trait;

#[cfg(feature = "postgres")]
#[async_trait]
impl ProjectStore for PgMissionStore {
    // ── projects table (from pg/project.rs) ───────────────────────

    async fn list_projects(&self) -> DbResult<Vec<crate::types::ProjectConfig>> {
        Ok(list_projects(&self.pool).await?)
    }

    async fn get_project(&self, id: &str) -> DbResult<Option<crate::types::ProjectConfig>> {
        Ok(get_project(&self.pool, id).await?)
    }

    async fn upsert_project(&self, config: &crate::types::ProjectConfig) -> DbResult<()> {
        Ok(upsert_project(&self.pool, config).await?)
    }

    async fn set_project_active(&self, id: &str, active: bool) -> DbResult<bool> {
        Ok(update_project_active(&self.pool, id, active).await?)
    }

    async fn backfill_project_id(&self, project_id: &str, path_pattern: &str) -> DbResult<u64> {
        Ok(backfill_project_id(&self.pool, project_id, path_pattern).await?)
    }

    async fn conversation_stats_by_project(&self, project_id: &str) -> DbResult<serde_json::Value> {
        Ok(conversation_stats_by_project(&self.pool, project_id).await?)
    }

    async fn recent_conversations_by_project(
        &self,
        project_id: &str,
        limit: i64,
    ) -> DbResult<Vec<serde_json::Value>> {
        Ok(recent_conversations_by_project(&self.pool, project_id, limit).await?)
    }

    async fn kb_stats_by_project(&self, project_id: &str) -> DbResult<serde_json::Value> {
        Ok(kb_stats_by_project(&self.pool, project_id).await?)
    }

    // ── skill topics ──────────────────────────────────────────────

    async fn skill_topic_upsert(
        &self,
        topic: &str,
        description: Option<&str>,
        aka: Option<&str>,
        allowed_tools: Option<&str>,
        file_path: &str,
        requires_json: Option<&str>,
        actions_json: Option<&str>,
    ) -> DbResult<()> {
        self.skill_topic_upsert_full(
            topic,
            description,
            aka,
            allowed_tools,
            file_path,
            requires_json,
            actions_json,
            None,
        )
        .await
    }

    async fn skill_topic_upsert_full(
        &self,
        topic: &str,
        description: Option<&str>,
        aka: Option<&str>,
        allowed_tools: Option<&str>,
        file_path: &str,
        requires_json: Option<&str>,
        actions_json: Option<&str>,
        context_hooks_json: Option<&str>,
    ) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO skill_topics (topic, description, aka, allowed_tools, file_path, requires_json, actions_json, context_hooks_json, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
             ON CONFLICT(topic) DO UPDATE SET
                description = $2, aka = $3, allowed_tools = $4, file_path = $5,
                requires_json = $6, actions_json = $7, context_hooks_json = $8, updated_at = $9"
        )
        .bind(topic)
        .bind(description)
        .bind(aka)
        .bind(allowed_tools)
        .bind(file_path)
        .bind(requires_json)
        .bind(actions_json)
        .bind(context_hooks_json)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn skill_topic_get(&self, topic: &str) -> DbResult<Option<SkillTopic>> {
        let row = sqlx::query(
            "SELECT topic, description, aka, allowed_tools, file_path,
                    hit_count, last_hit_at, fragment_count, total_lines, checksum,
                    requires_json, actions_json, context_hooks_json, created_at, updated_at
             FROM skill_topics WHERE topic = $1",
        )
        .bind(topic)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            use sqlx::Row;
            SkillTopic {
                topic: r.get("topic"),
                description: r.get("description"),
                aka: r.get("aka"),
                allowed_tools: r.get("allowed_tools"),
                file_path: r.get("file_path"),
                hit_count: r.get("hit_count"),
                last_hit_at: r.get("last_hit_at"),
                fragment_count: r.get("fragment_count"),
                total_lines: r.get("total_lines"),
                checksum: r.get("checksum"),
                requires_json: r.get("requires_json"),
                actions_json: r.get("actions_json"),
                context_hooks_json: r.get("context_hooks_json"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            }
        }))
    }

    async fn skill_topic_list(&self) -> DbResult<Vec<SkillTopic>> {
        let rows = sqlx::query(
            "SELECT topic, description, aka, allowed_tools, file_path,
                    hit_count, last_hit_at, fragment_count, total_lines, checksum,
                    requires_json, actions_json, context_hooks_json, created_at, updated_at
             FROM skill_topics ORDER BY topic",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                SkillTopic {
                    topic: r.get("topic"),
                    description: r.get("description"),
                    aka: r.get("aka"),
                    allowed_tools: r.get("allowed_tools"),
                    file_path: r.get("file_path"),
                    hit_count: r.get("hit_count"),
                    last_hit_at: r.get("last_hit_at"),
                    fragment_count: r.get("fragment_count"),
                    total_lines: r.get("total_lines"),
                    checksum: r.get("checksum"),
                    requires_json: r.get("requires_json"),
                    actions_json: r.get("actions_json"),
                    context_hooks_json: r.get("context_hooks_json"),
                    created_at: r.get("created_at"),
                    updated_at: r.get("updated_at"),
                }
            })
            .collect())
    }

    async fn skill_topic_hit(&self, topic: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE skill_topics SET hit_count = hit_count + 1, last_hit_at = $1 WHERE topic = $2",
        )
        .bind(&now)
        .bind(topic)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn skill_topic_update_stats(
        &self,
        topic: &str,
        total_lines: i32,
        checksum: &str,
    ) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE skill_topics SET total_lines = $1, checksum = $2, updated_at = $3 WHERE topic = $4"
        )
        .bind(total_lines)
        .bind(checksum)
        .bind(&now)
        .bind(topic)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── skill blocks ──────────────────────────────────────────────

    async fn skill_block_insert(
        &self,
        topic: &str,
        block_type: &str,
        title: Option<&str>,
        content: &str,
        sort_order: i32,
    ) -> DbResult<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO skill_blocks (id, topic, block_type, title, content, sort_order, status, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, 'active', $7, $7)"
        )
        .bind(&id)
        .bind(topic)
        .bind(block_type)
        .bind(title)
        .bind(content)
        .bind(sort_order)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        // Update fragment count if fragment
        if block_type == "fragment" {
            sqlx::query(
                "UPDATE skill_topics SET fragment_count = fragment_count + 1 WHERE topic = $1",
            )
            .bind(topic)
            .execute(&self.pool)
            .await?;
        }

        Ok(id)
    }

    async fn skill_block_update(&self, id: &str, content: &str) -> DbResult<bool> {
        let now = chrono::Utc::now().to_rfc3339();
        let result = sqlx::query(
            "UPDATE skill_blocks SET content = $1, updated_at = $2 WHERE id = $3 AND status = 'active'"
        )
        .bind(content)
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn skill_blocks_for_topic(&self, topic: &str) -> DbResult<Vec<SkillBlock>> {
        let rows = sqlx::query(
            "SELECT id, topic, block_type, title, content, sort_order, status, created_at, updated_at
             FROM skill_blocks
             WHERE topic = $1 AND status = 'active'
             ORDER BY block_type DESC, sort_order ASC"
        )
        .bind(topic)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                SkillBlock {
                    id: r.get("id"),
                    topic: r.get("topic"),
                    block_type: r.get("block_type"),
                    title: r.get("title"),
                    content: r.get("content"),
                    sort_order: r.get("sort_order"),
                    status: r.get("status"),
                    created_at: r.get("created_at"),
                    updated_at: r.get("updated_at"),
                }
            })
            .collect())
    }

    async fn skill_block_set_status(&self, id: &str, status: &str) -> DbResult<bool> {
        let now = chrono::Utc::now().to_rfc3339();
        let result =
            sqlx::query("UPDATE skill_blocks SET status = $1, updated_at = $2 WHERE id = $3")
                .bind(status)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn skill_blocks_delete_topic(&self, topic: &str) -> DbResult<usize> {
        let result = sqlx::query("DELETE FROM skill_blocks WHERE topic = $1")
            .bind(topic)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() as usize)
    }

    // ── skill search ──────────────────────────────────────────────

    async fn skill_search_fts(&self, query: &str) -> DbResult<Vec<SkillSearchResult>> {
        let tokens: Vec<&str> = query.split_whitespace().collect();
        if tokens.is_empty() {
            return Ok(vec![]);
        }

        // PG: use plainto_tsquery on the GENERATED tsvector column fts_doc
        let rows = sqlx::query(
            "SELECT sb.topic, sb.title,
                    ts_headline('simple', sb.content, plainto_tsquery('simple', $1),
                                'StartSel=>>>, StopSel=<<<, MaxFragments=3, MaxWords=32') AS snippet,
                    st.file_path, st.description
             FROM skill_blocks sb
             JOIN skill_topics st ON st.topic = sb.topic
             WHERE sb.fts_doc @@ plainto_tsquery('simple', $1) AND sb.status = 'active'
             ORDER BY ts_rank(sb.fts_doc, plainto_tsquery('simple', $1)) DESC
             LIMIT 20"
        )
        .bind(query)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                SkillSearchResult {
                    topic: r.get("topic"),
                    section_title: r.get("title"),
                    snippet: r.get("snippet"),
                    file_path: r.get("file_path"),
                    description: r.get("description"),
                }
            })
            .collect())
    }

    async fn skill_search_topics(&self, query: &str) -> DbResult<Vec<SkillTopic>> {
        let pattern = format!("%{}%", query.to_lowercase());
        let rows = sqlx::query(
            "SELECT topic, description, aka, allowed_tools, file_path,
                    hit_count, last_hit_at, fragment_count, total_lines, checksum,
                    requires_json, actions_json, context_hooks_json, created_at, updated_at
             FROM skill_topics
             WHERE LOWER(topic) LIKE $1
                OR LOWER(COALESCE(description, '')) LIKE $1
                OR LOWER(COALESCE(aka, '')) LIKE $1
             ORDER BY hit_count DESC
             LIMIT 10",
        )
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                SkillTopic {
                    topic: r.get("topic"),
                    description: r.get("description"),
                    aka: r.get("aka"),
                    allowed_tools: r.get("allowed_tools"),
                    file_path: r.get("file_path"),
                    hit_count: r.get("hit_count"),
                    last_hit_at: r.get("last_hit_at"),
                    fragment_count: r.get("fragment_count"),
                    total_lines: r.get("total_lines"),
                    checksum: r.get("checksum"),
                    requires_json: r.get("requires_json"),
                    actions_json: r.get("actions_json"),
                    context_hooks_json: r.get("context_hooks_json"),
                    created_at: r.get("created_at"),
                    updated_at: r.get("updated_at"),
                }
            })
            .collect())
    }

    async fn skill_rebuild_fts(&self) -> DbResult<()> {
        // PG uses GENERATED tsvector columns — no manual rebuild needed.
        Ok(())
    }

    // ── skill embeddings ──────────────────────────────────────────

    async fn skill_set_topic_embedding(
        &self,
        topic: &str,
        embedding: &[f32],
        provider: &str,
    ) -> DbResult<()> {
        let bytes = crate::embedding::f32_vec_to_bytes(embedding);
        let vec_str = format!(
            "[{}]",
            embedding
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        sqlx::query(
            "UPDATE skill_topics SET embedding = $1, embedding_provider = $2, embedding_vec = $4::vector WHERE topic = $3"
        )
        .bind(&bytes)
        .bind(provider)
        .bind(topic)
        .bind(&vec_str)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn skill_load_topic_embeddings(&self) -> DbResult<Vec<(String, Vec<f32>)>> {
        let rows: Vec<(String, Vec<u8>)> =
            sqlx::query_as("SELECT topic, embedding FROM skill_topics WHERE embedding IS NOT NULL")
                .fetch_all(&self.pool)
                .await?;

        Ok(rows
            .into_iter()
            .map(|(topic, blob)| (topic, crate::embedding::bytes_to_f32_vec(&blob)))
            .collect())
    }

    async fn skill_topics_missing_embedding(&self, limit: i64) -> DbResult<Vec<(String, String)>> {
        // Step 1: Get topics missing embedding
        let topics: Vec<(String, String)> = sqlx::query_as(
            "SELECT topic, COALESCE(description, '') FROM skill_topics WHERE embedding IS NULL LIMIT $1"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        // Step 2: Build embed text for each topic
        let mut results = Vec::new();
        for (topic, description) in &topics {
            let blocks: Vec<(String, String)> = sqlx::query_as(
                "SELECT COALESCE(title, ''), content FROM skill_blocks
                 WHERE topic = $1 AND status = 'active' ORDER BY sort_order",
            )
            .bind(topic)
            .fetch_all(&self.pool)
            .await?;

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

    async fn skill_topics_stale_embedding(
        &self,
        current_provider: &str,
        limit: i64,
    ) -> DbResult<Vec<(String, String)>> {
        let topics: Vec<(String, String)> = sqlx::query_as(
            "SELECT topic, COALESCE(description, '') FROM skill_topics
             WHERE embedding IS NOT NULL AND embedding_provider IS NOT NULL
               AND embedding_provider != $1
             LIMIT $2",
        )
        .bind(current_provider)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for (topic, description) in &topics {
            let blocks: Vec<(String, String)> = sqlx::query_as(
                "SELECT COALESCE(title, ''), content FROM skill_blocks
                 WHERE topic = $1 AND status = 'active' ORDER BY sort_order",
            )
            .bind(topic)
            .fetch_all(&self.pool)
            .await?;

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

    // ── skill executions ──────────────────────────────────────────

    async fn skill_execution_insert(
        &self,
        id: &str,
        skill_topic: &str,
        action_id: &str,
        steps_total: i32,
        triggered_by: &str,
    ) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO skill_executions (id, skill_topic, action_id, status, steps_total, triggered_by, created_at)
             VALUES ($1, $2, $3, 'running', $4, $5, $6)"
        )
        .bind(id)
        .bind(skill_topic)
        .bind(action_id)
        .bind(steps_total)
        .bind(triggered_by)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn skill_execution_update(
        &self,
        id: &str,
        status: &str,
        steps_completed: i32,
        context_json: Option<&str>,
        error: Option<&str>,
    ) -> DbResult<()> {
        self.skill_execution_update_with_duration(
            id,
            status,
            steps_completed,
            context_json,
            error,
            None,
        )
        .await
    }

    async fn skill_execution_update_with_duration(
        &self,
        id: &str,
        status: &str,
        steps_completed: i32,
        context_json: Option<&str>,
        error: Option<&str>,
        duration_ms: Option<i64>,
    ) -> DbResult<()> {
        let completed_at = if status == "success" || status == "failed" || status == "cancelled" {
            Some(chrono::Utc::now().to_rfc3339())
        } else {
            None
        };
        sqlx::query(
            "UPDATE skill_executions SET status = $1, steps_completed = $2, context_json = $3,
                    error = $4, completed_at = $5, duration_ms = $6
             WHERE id = $7",
        )
        .bind(status)
        .bind(steps_completed)
        .bind(context_json)
        .bind(error)
        .bind(completed_at.as_deref())
        .bind(duration_ms)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn skill_execution_stats(
        &self,
        topic: Option<&str>,
    ) -> DbResult<Vec<SkillExecutionStat>> {
        if let Some(t) = topic {
            let rows = sqlx::query(
                "SELECT action_id, COUNT(*) AS total,
                        SUM(CASE WHEN status='success' THEN 1 ELSE 0 END) AS successes,
                        SUM(CASE WHEN status='failed' THEN 1 ELSE 0 END) AS failures,
                        AVG(duration_ms) AS avg_duration_ms,
                        MAX(created_at) AS last_run
                 FROM skill_executions
                 WHERE skill_topic = $1
                 GROUP BY action_id
                 ORDER BY last_run DESC",
            )
            .bind(t)
            .fetch_all(&self.pool)
            .await?;

            Ok(rows
                .iter()
                .map(|r| {
                    use sqlx::Row;
                    SkillExecutionStat {
                        action_id: r.get("action_id"),
                        total: r.get("total"),
                        successes: r.get("successes"),
                        failures: r.get("failures"),
                        avg_duration_ms: r.get("avg_duration_ms"),
                        last_run: r.get("last_run"),
                    }
                })
                .collect())
        } else {
            let rows = sqlx::query(
                "SELECT skill_topic || '/' || action_id AS action_id, COUNT(*) AS total,
                        SUM(CASE WHEN status='success' THEN 1 ELSE 0 END) AS successes,
                        SUM(CASE WHEN status='failed' THEN 1 ELSE 0 END) AS failures,
                        AVG(duration_ms) AS avg_duration_ms,
                        MAX(created_at) AS last_run
                 FROM skill_executions
                 GROUP BY skill_topic, action_id
                 ORDER BY last_run DESC",
            )
            .fetch_all(&self.pool)
            .await?;

            Ok(rows
                .iter()
                .map(|r| {
                    use sqlx::Row;
                    SkillExecutionStat {
                        action_id: r.get("action_id"),
                        total: r.get("total"),
                        successes: r.get("successes"),
                        failures: r.get("failures"),
                        avg_duration_ms: r.get("avg_duration_ms"),
                        last_run: r.get("last_run"),
                    }
                })
                .collect())
        }
    }

    async fn skill_execution_is_running(
        &self,
        skill_topic: &str,
        action_id: &str,
    ) -> DbResult<bool> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM skill_executions WHERE skill_topic = $1 AND action_id = $2 AND status = 'running'"
        )
        .bind(skill_topic)
        .bind(action_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    // ── skill versions ────────────────────────────────────────────

    async fn skill_version_save(&self, topic: &str, content: &str, checksum: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO skill_versions (topic, content, checksum, created_at) VALUES ($1, $2, $3, $4)"
        )
        .bind(topic)
        .bind(content)
        .bind(checksum)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        // Keep only the 10 most recent versions per topic
        sqlx::query(
            "DELETE FROM skill_versions WHERE topic = $1 AND id NOT IN (
                SELECT id FROM skill_versions WHERE topic = $1 ORDER BY id DESC LIMIT 10
            )",
        )
        .bind(topic)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn skill_version_list(&self, topic: &str, limit: i64) -> DbResult<Vec<SkillVersion>> {
        let rows = sqlx::query(
            "SELECT id, topic, content, checksum, created_at FROM skill_versions
             WHERE topic = $1 ORDER BY id DESC LIMIT $2",
        )
        .bind(topic)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                SkillVersion {
                    id: r.get("id"),
                    topic: r.get("topic"),
                    content: r.get("content"),
                    checksum: r.get("checksum"),
                    created_at: r.get("created_at"),
                }
            })
            .collect())
    }

    async fn skill_version_get(&self, id: i64) -> DbResult<Option<SkillVersion>> {
        let row = sqlx::query(
            "SELECT id, topic, content, checksum, created_at FROM skill_versions WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            use sqlx::Row;
            SkillVersion {
                id: r.get("id"),
                topic: r.get("topic"),
                content: r.get("content"),
                checksum: r.get("checksum"),
                created_at: r.get("created_at"),
            }
        }))
    }

    // ── beacons ───────────────────────────────────────────────────

    async fn beacon_list(&self) -> DbResult<Vec<BeaconInfo>> {
        let rows = sqlx::query(
            "SELECT b.id, b.name, b.description, b.created_at, b.updated_at,
                    (SELECT COUNT(*) FROM beacon_nodes bn WHERE bn.beacon_id = b.id) AS node_count
             FROM beacons b
             ORDER BY b.name",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                BeaconInfo {
                    id: r.get("id"),
                    name: r.get("name"),
                    description: r.get("description"),
                    created_at: r.get("created_at"),
                    updated_at: r.get("updated_at"),
                    node_count: r.get::<i64, _>("node_count") as usize,
                }
            })
            .collect())
    }

    async fn beacon_ensure(&self, name: &str) -> DbResult<String> {
        // Try to get existing
        let existing: Option<(String,)> = sqlx::query_as("SELECT id FROM beacons WHERE name = $1")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;

        if let Some((id,)) = existing {
            return Ok(id);
        }

        // Create new
        let id = format!("bcn-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        sqlx::query("INSERT INTO beacons (id, name) VALUES ($1, $2)")
            .bind(&id)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(id)
    }

    async fn beacon_increment_harvest(&self, name: &str) -> DbResult<i64> {
        sqlx::query(
            "UPDATE beacons SET harvest_count = COALESCE(harvest_count, 0) + 1,
                    updated_at = to_char(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS')
             WHERE name = $1",
        )
        .bind(name)
        .execute(&self.pool)
        .await?;

        let row: Option<(i64,)> =
            sqlx::query_as("SELECT COALESCE(harvest_count, 0) FROM beacons WHERE name = $1")
                .bind(name)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| r.0).unwrap_or(0))
    }

    async fn beacon_set_description(&self, name: &str, description: &str) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE beacons SET description = $1,
                    updated_at = to_char(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS')
             WHERE name = $2",
        )
        .bind(description)
        .bind(name)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn beacon_node_upsert(
        &self,
        beacon_id: &str,
        repo: &str,
        file_path: &str,
        symbol_name: &str,
        annotation: Option<&str>,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO beacon_nodes (beacon_id, repo, file_path, symbol_name, annotation)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (beacon_id, repo, file_path, symbol_name)
             DO UPDATE SET annotation = COALESCE($5, beacon_nodes.annotation)",
        )
        .bind(beacon_id)
        .bind(repo)
        .bind(file_path)
        .bind(symbol_name)
        .bind(annotation)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn beacon_map(&self, beacon_name: &str) -> DbResult<Vec<BeaconNode>> {
        let rows = sqlx::query(
            "SELECT bn.beacon_id, b.name, bn.repo, bn.file_path, bn.symbol_name, bn.annotation,
                    an.signature, an.stub_content, an.start_line, an.end_line, an.node_type
             FROM beacon_nodes bn
             JOIN beacons b ON b.id = bn.beacon_id
             LEFT JOIN ast_nodes an ON an.repo = bn.repo AND an.file_path = bn.file_path AND an.name = bn.symbol_name
             WHERE b.name = $1
             ORDER BY bn.file_path, an.start_line"
        )
        .bind(beacon_name)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                BeaconNode {
                    beacon_id: r.get("beacon_id"),
                    beacon_name: r.get("name"),
                    repo: r.get("repo"),
                    file_path: r.get("file_path"),
                    symbol_name: r.get("symbol_name"),
                    annotation: r.get("annotation"),
                    signature: r.get("signature"),
                    stub_content: r.get("stub_content"),
                    start_line: r.get::<Option<i64>, _>("start_line").map(|v| v as usize),
                    end_line: r.get::<Option<i64>, _>("end_line").map(|v| v as usize),
                    node_type: r.get("node_type"),
                }
            })
            .collect())
    }

    async fn beacon_nodes_delete_file(&self, repo: &str, file_path: &str) -> DbResult<usize> {
        let result = sqlx::query("DELETE FROM beacon_nodes WHERE repo = $1 AND file_path = $2")
            .bind(repo)
            .bind(file_path)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn beacon_node_annotate(
        &self,
        beacon_name: &str,
        repo: &str,
        file_path: &str,
        symbol_name: &str,
        annotation: &str,
    ) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE beacon_nodes SET annotation = $1
             WHERE beacon_id = (SELECT id FROM beacons WHERE name = $2)
               AND repo = $3 AND file_path = $4 AND symbol_name = $5",
        )
        .bind(annotation)
        .bind(beacon_name)
        .bind(repo)
        .bind(file_path)
        .bind(symbol_name)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn beacon_search(&self, query: &str) -> DbResult<Vec<BeaconInfo>> {
        let pattern = format!("%{}%", query);
        let rows = sqlx::query(
            "SELECT b.id, b.name, b.description, b.created_at, b.updated_at,
                    (SELECT COUNT(*) FROM beacon_nodes bn WHERE bn.beacon_id = b.id) AS node_count
             FROM beacons b
             WHERE b.name LIKE $1 OR b.description LIKE $1
             ORDER BY b.name
             LIMIT 10",
        )
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                BeaconInfo {
                    id: r.get("id"),
                    name: r.get("name"),
                    description: r.get("description"),
                    created_at: r.get("created_at"),
                    updated_at: r.get("updated_at"),
                    node_count: r.get::<i64, _>("node_count") as usize,
                }
            })
            .collect())
    }

    async fn beacon_cleanup_empty(&self) -> DbResult<usize> {
        let result = sqlx::query(
            "DELETE FROM beacons WHERE id NOT IN (SELECT DISTINCT beacon_id FROM beacon_nodes)",
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }

    // ── AST index ─────────────────────────────────────────────────

    async fn ast_sync_file(
        &self,
        repo: &str,
        file_path: &str,
        commit_hash: &str,
        nodes: &[CodeNode],
    ) -> DbResult<AstSyncResult> {
        // Delete existing nodes for this file
        let del_result = sqlx::query("DELETE FROM ast_nodes WHERE repo = $1 AND file_path = $2")
            .bind(repo)
            .bind(file_path)
            .execute(&self.pool)
            .await?;
        let deleted = del_result.rows_affected() as usize;

        // Insert new nodes
        let mut inserted = 0usize;
        for node in nodes {
            let id = node.build_id(repo, file_path);
            let calls_json = serde_json::to_string(&node.calls).unwrap_or_default();

            sqlx::query(
                "INSERT INTO ast_nodes (id, repo, file_path, node_type, name, signature,
                    start_line, end_line, is_exported, docstring, stub_content, calls)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
            )
            .bind(&id)
            .bind(repo)
            .bind(file_path)
            .bind(&node.node_type)
            .bind(&node.name)
            .bind(&node.signature)
            .bind(node.start_line as i64)
            .bind(node.end_line as i64)
            .bind(node.is_exported)
            .bind(&node.docstring)
            .bind(&node.stub_content)
            .bind(&calls_json)
            .execute(&self.pool)
            .await?;
            inserted += 1;
        }

        // Upsert file metadata
        sqlx::query(
            "INSERT INTO ast_file_meta (repo, file_path, commit_hash, node_count)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (repo, file_path) DO UPDATE SET
                commit_hash = EXCLUDED.commit_hash,
                node_count = EXCLUDED.node_count",
        )
        .bind(repo)
        .bind(file_path)
        .bind(commit_hash)
        .bind(nodes.len() as i32)
        .execute(&self.pool)
        .await?;

        Ok(AstSyncResult { inserted, deleted })
    }

    async fn ast_delete_file(&self, repo: &str, file_path: &str) -> DbResult<usize> {
        let result = sqlx::query("DELETE FROM ast_nodes WHERE repo = $1 AND file_path = $2")
            .bind(repo)
            .bind(file_path)
            .execute(&self.pool)
            .await?;
        let deleted = result.rows_affected() as usize;

        sqlx::query("DELETE FROM ast_file_meta WHERE repo = $1 AND file_path = $2")
            .bind(repo)
            .bind(file_path)
            .execute(&self.pool)
            .await?;

        Ok(deleted)
    }

    async fn ast_search(&self, query: &str, limit: usize) -> DbResult<Vec<AstSearchHit>> {
        // PG: FTS first, then LIKE fallback for partial/camelCase matches
        let mut rows = sqlx::query(
            "SELECT n.id, n.repo, n.file_path, n.name, n.node_type, n.signature,
                    n.start_line, n.end_line, n.is_exported, n.docstring, n.stub_content, n.calls,
                    ts_rank(n.fts_doc, plainto_tsquery('simple', $1)) AS rank
             FROM ast_nodes n
             WHERE n.fts_doc @@ plainto_tsquery('simple', $1)
             ORDER BY rank DESC
             LIMIT $2",
        )
        .bind(query)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        // LIKE fallback for partial matches (camelCase, Chinese, substrings)
        if rows.is_empty() {
            let pattern = format!("%{}%", query);
            rows = sqlx::query(
                "SELECT n.id, n.repo, n.file_path, n.name, n.node_type, n.signature,
                        n.start_line, n.end_line, n.is_exported, n.docstring, n.stub_content, n.calls,
                        0.0::real AS rank
                 FROM ast_nodes n
                 WHERE n.name ILIKE $1 OR n.signature ILIKE $1
                 ORDER BY n.name
                 LIMIT $2"
            )
            .bind(&pattern)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?;
        }

        Ok(rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                let calls_json: String = r.get("calls");
                let calls: Vec<String> = serde_json::from_str(&calls_json).unwrap_or_default();
                AstSearchHit {
                    id: r.get("id"),
                    repo: r.get("repo"),
                    file_path: r.get("file_path"),
                    name: r.get("name"),
                    node_type: r.get("node_type"),
                    signature: r.get("signature"),
                    start_line: r.get::<i64, _>("start_line") as usize,
                    end_line: r.get::<i64, _>("end_line") as usize,
                    is_exported: r.get::<bool, _>("is_exported"),
                    docstring: r.get("docstring"),
                    stub_content: r.get("stub_content"),
                    calls,
                    rank: r.get::<f32, _>("rank") as f64,
                }
            })
            .collect())
    }

    async fn ast_get_file_nodes(&self, repo: &str, file_path: &str) -> DbResult<Vec<AstNodeRow>> {
        let rows = sqlx::query(
            "SELECT id, repo, file_path, node_type, name, signature,
                    start_line, end_line, is_exported, docstring, stub_content, calls,
                    embedding_provider
             FROM ast_nodes WHERE repo = $1 AND file_path = $2
             ORDER BY start_line",
        )
        .bind(repo)
        .bind(file_path)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                let calls_json: String = r.get("calls");
                let calls: Vec<String> = serde_json::from_str(&calls_json).unwrap_or_default();
                AstNodeRow {
                    id: r.get("id"),
                    repo: r.get("repo"),
                    file_path: r.get("file_path"),
                    node: CodeNode {
                        node_type: r.get("node_type"),
                        name: r.get("name"),
                        signature: r.get("signature"),
                        start_line: r.get::<i64, _>("start_line") as usize,
                        end_line: r.get::<i64, _>("end_line") as usize,
                        is_exported: r.get::<bool, _>("is_exported"),
                        docstring: r.get("docstring"),
                        stub_content: r.get("stub_content"),
                        calls,
                        beacons: Vec::new(),
                    },
                    embedding_provider: r.get("embedding_provider"),
                }
            })
            .collect())
    }

    async fn ast_find_related(&self, name: &str, limit: usize) -> DbResult<Vec<AstSearchHit>> {
        let rows = sqlx::query(
            "SELECT id, repo, file_path, name, node_type, signature,
                    start_line, end_line, is_exported, docstring, stub_content, calls
             FROM ast_nodes
             WHERE name = $1 AND node_type IN ('struct', 'enum', 'trait')
             LIMIT $2",
        )
        .bind(name)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                let calls_json: String = r.get("calls");
                let calls: Vec<String> = serde_json::from_str(&calls_json).unwrap_or_default();
                AstSearchHit {
                    id: r.get("id"),
                    repo: r.get("repo"),
                    file_path: r.get("file_path"),
                    name: r.get("name"),
                    node_type: r.get("node_type"),
                    signature: r.get("signature"),
                    start_line: r.get::<i64, _>("start_line") as usize,
                    end_line: r.get::<i64, _>("end_line") as usize,
                    is_exported: r.get::<bool, _>("is_exported"),
                    docstring: r.get("docstring"),
                    stub_content: r.get("stub_content"),
                    calls,
                    rank: 0.0,
                }
            })
            .collect())
    }

    async fn ast_get_file_commit(&self, repo: &str, file_path: &str) -> DbResult<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT commit_hash FROM ast_file_meta WHERE repo = $1 AND file_path = $2",
        )
        .bind(repo)
        .bind(file_path)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.0))
    }

    async fn ast_stats(&self) -> DbResult<AstStats> {
        let (total_nodes,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM ast_nodes")
            .fetch_one(&self.pool)
            .await?;
        let (total_files,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM ast_file_meta")
            .fetch_one(&self.pool)
            .await?;
        let (total_repos,): (i64,) =
            sqlx::query_as("SELECT COUNT(DISTINCT repo) FROM ast_file_meta")
                .fetch_one(&self.pool)
                .await?;
        let (embedded_nodes,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM ast_nodes WHERE embedding IS NOT NULL")
                .fetch_one(&self.pool)
                .await?;

        Ok(AstStats {
            total_nodes: total_nodes as usize,
            total_files: total_files as usize,
            total_repos: total_repos as usize,
            embedded_nodes: embedded_nodes as usize,
        })
    }

    async fn ast_set_embedding(
        &self,
        node_id: &str,
        embedding: &[u8],
        provider: &str,
    ) -> DbResult<()> {
        // Store BYTEA + pgvector embedding_vec for ANN search
        let floats = crate::embedding::bytes_to_f32_vec(embedding);
        let vec_str = format!(
            "[{}]",
            floats
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        sqlx::query(
            "UPDATE ast_nodes SET embedding = $1, embedding_provider = $2, embedding_vec = $4::vector WHERE id = $3"
        )
        .bind(embedding)
        .bind(provider)
        .bind(node_id)
        .bind(&vec_str)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn ast_find_unembedded(&self, limit: usize) -> DbResult<Vec<(String, String, String)>> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT id, repo, file_path FROM ast_nodes
             WHERE embedding IS NULL
             ORDER BY is_exported DESC, id
             LIMIT $1",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn ast_load_all_embeddings(&self) -> DbResult<Vec<(String, Vec<f32>)>> {
        let rows: Vec<(String, Vec<u8>)> =
            sqlx::query_as("SELECT id, embedding FROM ast_nodes WHERE embedding IS NOT NULL")
                .fetch_all(&self.pool)
                .await?;

        Ok(rows
            .into_iter()
            .map(|(id, blob)| (id, crate::embedding::bytes_to_f32_vec(&blob)))
            .collect())
    }

    async fn ast_search_ranked(&self, query: &str, limit: usize) -> DbResult<Vec<(String, usize)>> {
        let tokens: Vec<&str> = query.split_whitespace().filter(|w| w.len() > 1).collect();
        if tokens.is_empty() {
            return Ok(Vec::new());
        }

        // Use plainto_tsquery for PG FTS
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT n.id
             FROM ast_nodes n
             WHERE n.fts_doc @@ plainto_tsquery('simple', $1)
             ORDER BY ts_rank(n.fts_doc, plainto_tsquery('simple', $1)) DESC
             LIMIT $2",
        )
        .bind(query)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .enumerate()
            .map(|(rank, (id,))| (id, rank))
            .collect())
    }

    async fn ast_get_search_hit(&self, node_id: &str) -> DbResult<Option<AstSearchHit>> {
        let row = sqlx::query(
            "SELECT id, repo, file_path, name, node_type, signature,
                    start_line, end_line, is_exported, docstring, stub_content, calls
             FROM ast_nodes WHERE id = $1",
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            use sqlx::Row;
            let calls_json: String = r.get("calls");
            let calls: Vec<String> = serde_json::from_str(&calls_json).unwrap_or_default();
            AstSearchHit {
                id: r.get("id"),
                repo: r.get("repo"),
                file_path: r.get("file_path"),
                name: r.get("name"),
                node_type: r.get("node_type"),
                signature: r.get("signature"),
                start_line: r.get::<i64, _>("start_line") as usize,
                end_line: r.get::<i64, _>("end_line") as usize,
                is_exported: r.get::<bool, _>("is_exported"),
                docstring: r.get("docstring"),
                stub_content: r.get("stub_content"),
                calls,
                rank: 0.0,
            }
        }))
    }

    async fn ast_get_node(&self, node_id: &str) -> DbResult<Option<AstNodeRow>> {
        let row = sqlx::query(
            "SELECT id, repo, file_path, node_type, name, signature,
                    start_line, end_line, is_exported, docstring, stub_content, calls,
                    embedding_provider
             FROM ast_nodes WHERE id = $1",
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            use sqlx::Row;
            let calls_json: String = r.get("calls");
            let calls: Vec<String> = serde_json::from_str(&calls_json).unwrap_or_default();
            AstNodeRow {
                id: r.get("id"),
                repo: r.get("repo"),
                file_path: r.get("file_path"),
                node: CodeNode {
                    node_type: r.get("node_type"),
                    name: r.get("name"),
                    signature: r.get("signature"),
                    start_line: r.get::<i64, _>("start_line") as usize,
                    end_line: r.get::<i64, _>("end_line") as usize,
                    is_exported: r.get::<bool, _>("is_exported"),
                    docstring: r.get("docstring"),
                    stub_content: r.get("stub_content"),
                    calls,
                    beacons: Vec::new(),
                },
                embedding_provider: r.get("embedding_provider"),
            }
        }))
    }

    async fn ast_module_summaries(&self, repo: &str) -> DbResult<Vec<ModuleAstSummary>> {
        let rows = sqlx::query(
            "SELECT file_path, node_type, name, is_exported, docstring
             FROM ast_nodes WHERE repo = $1
             ORDER BY file_path, start_line",
        )
        .bind(repo)
        .fetch_all(&self.pool)
        .await?;

        if rows.is_empty() {
            return Ok(Vec::new());
        }

        let nodes: Vec<(String, String, String, bool, Option<String>)> = rows
            .iter()
            .map(|r| {
                use sqlx::Row;
                (
                    r.get::<String, _>("file_path"),
                    r.get::<String, _>("node_type"),
                    r.get::<String, _>("name"),
                    r.get::<bool, _>("is_exported"),
                    r.get::<Option<String>, _>("docstring"),
                )
            })
            .collect();

        // Group by crate key — reuse the same logic as SQLite
        use std::collections::HashMap;

        let mut crate_indices: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, (fp, ..)) in nodes.iter().enumerate() {
            let key = extract_crate_key(fp);
            crate_indices.entry(key).or_default().push(i);
        }

        let mut summaries = Vec::new();
        const ADAPTIVE_THRESHOLD: usize = 30;

        for (crate_key, indices) in &crate_indices {
            let public_count = indices.iter().filter(|&&i| nodes[i].3).count();

            if public_count > ADAPTIVE_THRESHOLD {
                let mut subdir_indices: HashMap<String, Vec<usize>> = HashMap::new();
                for &i in indices {
                    let subdir = extract_subdir_key(&nodes[i].0, crate_key);
                    subdir_indices.entry(subdir).or_default().push(i);
                }
                for (subdir_key, sub_indices) in &subdir_indices {
                    summaries.push(aggregate_summary(subdir_key, sub_indices, &nodes));
                }
            } else {
                summaries.push(aggregate_summary(crate_key, indices, &nodes));
            }
        }

        summaries.sort_by(|a, b| a.module_path.cmp(&b.module_path));
        Ok(summaries)
    }
}

// ── Helper functions (duplicated from ast.rs to avoid cross-module dep) ────

fn extract_crate_key(file_path: &str) -> String {
    let parts: Vec<&str> = file_path.split('/').collect();
    if parts.len() >= 2 && (parts[0] == "crates" || parts[0] == "packages") {
        format!("{}/{}", parts[0], parts[1])
    } else if !parts.is_empty() {
        parts[0].to_string()
    } else {
        "root".to_string()
    }
}

fn extract_subdir_key(file_path: &str, crate_key: &str) -> String {
    let after_crate = file_path
        .strip_prefix(crate_key)
        .unwrap_or(file_path)
        .trim_start_matches('/');
    let parts: Vec<&str> = after_crate.split('/').collect();
    let depth = if parts.len() > 2 {
        2
    } else {
        parts.len().saturating_sub(1)
    };
    if depth == 0 {
        crate_key.to_string()
    } else {
        format!("{}/{}", crate_key, parts[..depth].join("/"))
    }
}

fn aggregate_summary(
    module_path: &str,
    indices: &[usize],
    nodes: &[(String, String, String, bool, Option<String>)],
) -> ModuleAstSummary {
    let mut public_types = Vec::new();
    let mut public_functions = Vec::new();
    let mut total_nodes = 0usize;
    let mut files_set = std::collections::HashSet::new();
    let mut file_docs: Vec<(String, String)> = Vec::new();
    let mut seen_doc_files = std::collections::HashSet::new();

    for &i in indices {
        let (ref fp, ref ntype, ref name, is_exported, ref docstring) = nodes[i];
        total_nodes += 1;
        files_set.insert(fp.clone());

        if let Some(ref doc) = docstring {
            if !doc.is_empty() && seen_doc_files.insert(fp.clone()) {
                let truncated = if doc.len() > 120 {
                    let end = doc
                        .char_indices()
                        .take_while(|(i, _)| *i < 120)
                        .last()
                        .map(|(i, c)| i + c.len_utf8())
                        .unwrap_or(120);
                    format!("{}...", &doc[..end])
                } else {
                    doc.clone()
                };
                file_docs.push((fp.clone(), truncated));
            }
        }

        if is_exported {
            match ntype.as_str() {
                "struct" | "enum" | "trait" | "type_alias" | "interface" => {
                    public_types.push(name.clone());
                }
                "function" | "method" => {
                    public_functions.push(name.clone());
                }
                _ => {}
            }
        }
    }

    let mut files: Vec<String> = files_set.into_iter().collect();
    files.sort();

    ModuleAstSummary {
        module_path: module_path.to_string(),
        public_types,
        public_functions,
        total_nodes,
        files,
        file_docs,
    }
}
