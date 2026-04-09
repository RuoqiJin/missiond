//! KnowledgeStore — PostgreSQL implementation.

use async_trait::async_trait;
use crate::db::error::DbResult;
use crate::db::traits::KnowledgeStore;
use crate::db::shared::{token_jaccard_similarity, infer_kb_type, contains_sensitive_data};
use crate::types::*;
use super::PgMissionStore;
use std::collections::HashMap;

/// Helper: convert a sqlx Row into KnowledgeEntry.
fn row_to_knowledge_entry(
    id: String, category: String, key: String, summary: String,
    detail: Option<String>, source: String, confidence: f64,
    access_count: i64, created_at: String, updated_at: String,
    last_accessed_at: Option<String>, linked_task_id: Option<String>,
    kb_type: Option<String>, scope_task_id: Option<String>,
    utility_score: Option<f64>,
) -> KnowledgeEntry {
    let detail_parsed = detail.and_then(|s| serde_json::from_str(&s).ok());
    KnowledgeEntry {
        id,
        category,
        key,
        summary,
        detail: detail_parsed,
        source,
        confidence,
        access_count,
        created_at,
        updated_at,
        last_accessed_at,
        linked_task_id,
        kb_type: kb_type.unwrap_or_else(|| "fact".to_string()),
        context_snippet: None,
        scope_task_id,
        utility_score: utility_score.unwrap_or(0.5),
    }
}

/// Tuple type matching the SELECT * columns we fetch from knowledge table.
type KBRow = (
    String, String, String, String, Option<String>, String, f64,
    i64, String, String, Option<String>, Option<String>,
    Option<String>, Option<String>, Option<f64>,
);

fn kb_row_to_entry(r: KBRow) -> KnowledgeEntry {
    row_to_knowledge_entry(
        r.0, r.1, r.2, r.3, r.4, r.5, r.6,
        r.7, r.8, r.9, r.10, r.11, r.12, r.13, r.14,
    )
}

/// Utility hit boost constant (same as MissionDB::UTILITY_HIT_BOOST).
const UTILITY_HIT_BOOST: f64 = 0.15;

/// The common SELECT column list for knowledge entries.
const KB_COLS: &str = "id, category, key, summary, detail, source, confidence, access_count, created_at, updated_at, last_accessed_at, linked_task_id, kb_type, scope_task_id, utility_score";

#[cfg(feature = "postgres")]
#[async_trait]
impl KnowledgeStore for PgMissionStore {
    // ========== Core CRUD ==========

    async fn kb_remember(&self, input: &KBRememberInput) -> DbResult<KBRememberResult> {
        let now = chrono::Utc::now().to_rfc3339();
        let source = input.source.as_deref().unwrap_or("conversation");
        let mut confidence = input.confidence.unwrap_or(1.0);
        let detail_str = input.detail.as_ref().map(|d| serde_json::to_string(d).unwrap_or_default());

        // Guard: reject infra category
        if input.category == "infra" {
            return Ok(KBRememberResult {
                entry: KnowledgeEntry {
                    id: String::new(),
                    category: input.category.clone(),
                    key: input.key.clone(),
                    summary: "REJECTED: infra entries should use servers.yaml + mission_infra_get".into(),
                    detail: None,
                    source: source.to_string(),
                    confidence: 0.0,
                    access_count: 0,
                    created_at: now.clone(),
                    updated_at: now,
                    last_accessed_at: None,
                    linked_task_id: None,
                    kb_type: infer_kb_type(&input.category).to_string(),
                    context_snippet: None,
                    scope_task_id: None,
                    utility_score: 0.0,
                },
                action: "rejected".into(),
                merged_key: None,
                similarity: None,
            });
        }

        // Sensitive data detection
        let check_text = format!("{} {} {}", input.summary, detail_str.as_deref().unwrap_or(""), input.key);
        if contains_sensitive_data(&check_text) {
            confidence = confidence.min(0.5);
        }

        // 1. Exact match by (category, key) → update
        let updated = sqlx::query(
            "UPDATE knowledge SET summary = $1, detail = $2, source = $3, confidence = $4, updated_at = $5,
             utility_score = GREATEST(utility_score, 0.8)
             WHERE category = $6 AND key = $7"
        )
        .bind(&input.summary)
        .bind(&detail_str)
        .bind(source)
        .bind(confidence)
        .bind(&now)
        .bind(&input.category)
        .bind(&input.key)
        .execute(&self.pool)
        .await?;

        if updated.rows_affected() > 0 {
            let entry = self.pg_get_by_category_key(&input.category, &input.key).await?;
            return Ok(KBRememberResult {
                entry: entry.unwrap(),
                action: "updated".to_string(),
                merged_key: None,
                similarity: None,
            });
        }

        // 1b. Same key, different category → re-categorize
        let existing_by_key: Option<KBRow> = sqlx::query_as(&format!(
            "SELECT {} FROM knowledge WHERE key = $1", KB_COLS
        ))
        .bind(&input.key)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(existing) = existing_by_key {
            let existing_entry = kb_row_to_entry(existing);
            sqlx::query(
                "UPDATE knowledge SET category = $1, summary = $2, detail = $3, source = $4, confidence = $5, updated_at = $6,
                 utility_score = GREATEST(utility_score, 0.8)
                 WHERE id = $7"
            )
            .bind(&input.category)
            .bind(&input.summary)
            .bind(&detail_str)
            .bind(source)
            .bind(confidence)
            .bind(&now)
            .bind(&existing_entry.id)
            .execute(&self.pool)
            .await?;
            let entry = self.pg_get_by_category_key(&input.category, &input.key).await?;
            return Ok(KBRememberResult {
                entry: entry.unwrap(),
                action: "updated".to_string(),
                merged_key: None,
                similarity: None,
            });
        }

        // 2. Fuzzy dedup: check for similar entries in same category
        const SIMILARITY_THRESHOLD: f64 = 0.5;
        let candidates = self.pg_list_by_category(&input.category).await?;
        let new_text = format!("{} {}", input.key, input.summary);
        let mut best: Option<(f64, KnowledgeEntry)> = None;
        for entry in candidates {
            let existing_text = format!("{} {}", entry.key, entry.summary);
            let sim = token_jaccard_similarity(&new_text, &existing_text);
            if sim >= SIMILARITY_THRESHOLD {
                match &best {
                    None => best = Some((sim, entry)),
                    Some((best_sim, _)) if sim > *best_sim => best = Some((sim, entry)),
                    _ => {}
                }
            }
        }

        if let Some((sim, existing)) = best {
            sqlx::query(
                "UPDATE knowledge SET summary = $1, detail = $2, source = $3, confidence = $4, updated_at = $5,
                 utility_score = GREATEST(utility_score, 0.8)
                 WHERE id = $6"
            )
            .bind(&input.summary)
            .bind(&detail_str)
            .bind(source)
            .bind(confidence)
            .bind(&now)
            .bind(&existing.id)
            .execute(&self.pool)
            .await?;
            let merged_key = existing.key.clone();
            let entry = self.pg_get_by_category_key(&existing.category, &existing.key).await?;
            return Ok(KBRememberResult {
                entry: entry.unwrap(),
                action: "merged".to_string(),
                merged_key: Some(merged_key),
                similarity: Some(sim),
            });
        }

        // 3. Insert new (ON CONFLICT guards against concurrent race on UNIQUE(category, key))
        let id = uuid::Uuid::new_v4().to_string();
        let kb_type = infer_kb_type(&input.category);
        sqlx::query(
            "INSERT INTO knowledge (id, category, key, summary, detail, source, confidence, access_count, created_at, updated_at, kb_type)
             VALUES ($1, $2, $3, $4, $5, $6, $7, 0, $8, $9, $10)
             ON CONFLICT (category, key) DO UPDATE SET
                summary = EXCLUDED.summary, detail = EXCLUDED.detail,
                source = EXCLUDED.source, confidence = EXCLUDED.confidence,
                updated_at = EXCLUDED.updated_at,
                utility_score = GREATEST(knowledge.utility_score, 0.8)"
        )
        .bind(&id)
        .bind(&input.category)
        .bind(&input.key)
        .bind(&input.summary)
        .bind(&detail_str)
        .bind(source)
        .bind(confidence)
        .bind(&now)
        .bind(&now)
        .bind(kb_type)
        .execute(&self.pool)
        .await?;

        let entry = KnowledgeEntry {
            id,
            category: input.category.clone(),
            key: input.key.clone(),
            summary: input.summary.clone(),
            detail: input.detail.clone(),
            source: source.to_string(),
            confidence,
            access_count: 0,
            created_at: now.clone(),
            updated_at: now,
            last_accessed_at: None,
            linked_task_id: None,
            kb_type: kb_type.to_string(),
            context_snippet: None,
            scope_task_id: None,
            utility_score: 0.5,
        };

        Ok(KBRememberResult {
            entry,
            action: "created".to_string(),
            merged_key: None,
            similarity: None,
        })
    }

    async fn kb_get(&self, key: &str) -> DbResult<Option<KnowledgeEntry>> {
        let row: Option<KBRow> = sqlx::query_as(&format!(
            "SELECT {} FROM knowledge WHERE key = $1", KB_COLS
        ))
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => {
                let mut entry = kb_row_to_entry(r);
                // Bump access count + utility score
                let now = chrono::Utc::now().to_rfc3339();
                let new_utility = (entry.utility_score + UTILITY_HIT_BOOST * (1.0 - entry.utility_score)).min(1.0);
                sqlx::query(
                    "UPDATE knowledge SET access_count = access_count + 1, last_accessed_at = $1,
                     utility_score = $3 WHERE id = $2"
                )
                .bind(&now)
                .bind(&entry.id)
                .bind(new_utility)
                .execute(&self.pool)
                .await?;
                entry.access_count += 1;
                entry.last_accessed_at = Some(now);
                entry.utility_score = new_utility;
                Ok(Some(entry))
            }
            None => Ok(None),
        }
    }

    async fn kb_get_by_id(&self, id: &str) -> DbResult<Option<KnowledgeEntry>> {
        let row: Option<KBRow> = sqlx::query_as(&format!(
            "SELECT {} FROM knowledge WHERE id = $1", KB_COLS
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(kb_row_to_entry))
    }

    async fn kb_get_id_by_key(&self, key: &str) -> DbResult<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM knowledge WHERE key = $1"
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.0))
    }

    async fn kb_update(&self, key: &str, new_category: Option<&str>, new_summary: Option<&str>, new_detail: Option<&serde_json::Value>, new_confidence: Option<f64>, new_linked_task_id: Option<&str>) -> DbResult<Option<(KnowledgeEntry, bool)>> {
        // Find existing entry
        let existing_row: Option<KBRow> = sqlx::query_as(&format!(
            "SELECT {} FROM knowledge WHERE key = $1", KB_COLS
        ))
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
        let existing = match existing_row {
            Some(r) => kb_row_to_entry(r),
            None => return Ok(None),
        };

        let now = chrono::Utc::now().to_rfc3339();
        let mut content_changed = false;

        // Build dynamic SET clauses
        let mut sets: Vec<String> = vec!["updated_at = $1".to_string()];
        let mut param_idx = 2u32;
        let detail_str = new_detail.map(|d| serde_json::to_string(d).unwrap_or_default());

        if new_category.is_some() { sets.push(format!("category = ${}", param_idx)); param_idx += 1; }
        if new_summary.is_some() { sets.push(format!("summary = ${}", param_idx)); param_idx += 1; content_changed = true; }
        if detail_str.is_some() { sets.push(format!("detail = ${}", param_idx)); param_idx += 1; content_changed = true; }
        if new_confidence.is_some() { sets.push(format!("confidence = ${}", param_idx)); param_idx += 1; }
        if new_linked_task_id.is_some() { sets.push(format!("linked_task_id = ${}", param_idx)); param_idx += 1; }

        // Only updated_at — nothing else to change
        if param_idx == 2 {
            return Ok(Some((existing, false)));
        }

        let sql = format!("UPDATE knowledge SET {} WHERE id = ${}", sets.join(", "), param_idx);

        // We need to use a raw query with dynamic binds
        // Build the query dynamically
        let mut query = sqlx::query(&sql);
        query = query.bind(&now);
        if let Some(v) = new_category { query = query.bind(v.to_string()); }
        if let Some(v) = new_summary { query = query.bind(v.to_string()); }
        if let Some(v) = &detail_str { query = query.bind(v.clone()); }
        if let Some(v) = new_confidence { query = query.bind(v); }
        if let Some(v) = new_linked_task_id {
            let val: Option<String> = if v.is_empty() { None } else { Some(v.to_string()) };
            query = query.bind(val);
        }
        query = query.bind(&existing.id);
        query.execute(&self.pool).await?;

        // Re-fetch
        let final_category = new_category.unwrap_or(&existing.category);
        let entry = self.pg_get_by_category_key(final_category, key).await?;
        let entry = entry.unwrap_or(existing);
        Ok(Some((entry, content_changed)))
    }

    async fn kb_set_linked_task_id(&self, key: &str, task_id: Option<&str>) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE knowledge SET linked_task_id = $1 WHERE key = $2"
        )
        .bind(task_id)
        .bind(key)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn kb_forget(&self, key: &str) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM knowledge WHERE key = $1")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn kb_batch_forget(&self, keys: &[String]) -> DbResult<usize> {
        let mut deleted = 0usize;
        for key in keys {
            let result = sqlx::query("DELETE FROM knowledge WHERE key = $1")
                .bind(key)
                .execute(&self.pool)
                .await?;
            if result.rows_affected() > 0 {
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    async fn kb_list(&self, category: Option<&str>) -> DbResult<Vec<KnowledgeEntry>> {
        let rows: Vec<KBRow> = if let Some(cat) = category {
            let like_pattern = format!("{}:%", cat);
            sqlx::query_as(&format!(
                "SELECT {} FROM knowledge WHERE category = $1 OR category LIKE $2 ORDER BY updated_at DESC", KB_COLS
            ))
            .bind(cat)
            .bind(like_pattern)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as(&format!(
                "SELECT {} FROM knowledge ORDER BY category, updated_at DESC", KB_COLS
            ))
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows.into_iter().map(kb_row_to_entry).collect())
    }

    async fn kb_list_paginated(&self, category: Option<&str>, limit: u32, offset: u32) -> DbResult<Vec<KnowledgeEntry>> {
        let rows: Vec<KBRow> = if let Some(cat) = category {
            let like_pattern = format!("{}:%", cat);
            sqlx::query_as(&format!(
                "SELECT {} FROM knowledge WHERE category = $1 OR category LIKE $2 ORDER BY updated_at DESC LIMIT $3 OFFSET $4", KB_COLS
            ))
            .bind(cat)
            .bind(like_pattern)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as(&format!(
                "SELECT {} FROM knowledge ORDER BY category, updated_at DESC LIMIT $1 OFFSET $2", KB_COLS
            ))
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows.into_iter().map(kb_row_to_entry).collect())
    }

    async fn kb_list_by_scope(&self, task_id: &str) -> DbResult<Vec<KnowledgeEntry>> {
        let rows: Vec<KBRow> = sqlx::query_as(&format!(
            "SELECT {} FROM knowledge WHERE scope_task_id = $1", KB_COLS
        ))
        .bind(task_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(kb_row_to_entry).collect())
    }

    async fn kb_clear_scope(&self, id: &str) -> DbResult<()> {
        sqlx::query("UPDATE knowledge SET scope_task_id = NULL, updated_at = $1 WHERE id = $2")
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn kb_update_access_stats(&self, entries: &[KnowledgeEntry]) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        for entry in entries {
            sqlx::query(
                "UPDATE knowledge SET access_count = access_count + 1, last_accessed_at = $1,
                 utility_score = LEAST(1.0, utility_score + $3 * (1.0 - utility_score))
                 WHERE id = $2
                 AND (last_accessed_at IS NULL OR EXTRACT(EPOCH FROM ($1::timestamp - last_accessed_at::timestamp)) > 3600)"
            )
            .bind(&now)
            .bind(&entry.id)
            .bind(UTILITY_HIT_BOOST)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    // ========== Search ==========

    async fn kb_search(&self, query: &str, category: Option<&str>) -> DbResult<Vec<KnowledgeEntry>> {
        // Phase 1: FTS search using tsvector
        let results = self.pg_search_fts(query, category).await?;
        if !results.is_empty() {
            return Ok(results);
        }
        // Phase 2: LIKE fallback for Chinese and partial matches
        self.pg_search_like(query, category).await
    }

    async fn kb_search_ranked(&self, query: &str, category: Option<&str>, limit: usize) -> DbResult<Vec<(KnowledgeEntry, usize)>> {
        let results = self.kb_search(query, category).await?;
        Ok(results.into_iter().take(limit).enumerate().map(|(i, e)| (e, i)).collect())
    }

    async fn kb_search_fts_ranked(&self, query: &str, category: Option<&str>) -> DbResult<Vec<(String, usize, Option<String>)>> {
        let tsquery = query.split_whitespace()
            .map(|w| w.replace('\'', ""))
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" | ");
        if tsquery.is_empty() {
            return Ok(Vec::new());
        }

        let rows: Vec<(String, Option<String>)> = if let Some(cat) = category {
            let like_pattern = format!("{}:%", cat);
            sqlx::query_as(
                "SELECT k.id, ts_headline('simple', COALESCE(k.detail, k.summary), plainto_tsquery('simple', $1), 'StartSel=**, StopSel=**, MaxFragments=2, MaxWords=40')
                 FROM knowledge k
                 WHERE k.fts_doc @@ plainto_tsquery('simple', $1) AND (k.category = $2 OR k.category LIKE $3)
                 ORDER BY ts_rank(k.fts_doc, plainto_tsquery('simple', $1)) DESC
                 LIMIT 100"
            )
            .bind(query)
            .bind(cat)
            .bind(like_pattern)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as(
                "SELECT k.id, ts_headline('simple', COALESCE(k.detail, k.summary), plainto_tsquery('simple', $1), 'StartSel=**, StopSel=**, MaxFragments=2, MaxWords=40')
                 FROM knowledge k
                 WHERE k.fts_doc @@ plainto_tsquery('simple', $1)
                 ORDER BY ts_rank(k.fts_doc, plainto_tsquery('simple', $1)) DESC
                 LIMIT 100"
            )
            .bind(query)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows.into_iter().enumerate().map(|(rank, (id, snippet))| {
            let snip = snippet.filter(|s| s.contains("**"));
            (id, rank, snip)
        }).collect())
    }

    async fn kb_search_like_ranked(&self, query: &str, category: Option<&str>) -> DbResult<Vec<(String, usize)>> {
        let keywords: Vec<String> = {
            let mut kw: Vec<String> = query.split_whitespace().map(|w| w.to_string()).collect();
            let trimmed = query.trim();
            if !trimmed.is_empty() && trimmed.chars().any(|c| !c.is_ascii()) && !kw.contains(&trimmed.to_string()) {
                kw.insert(0, trimmed.to_string());
            }
            kw
        };
        if keywords.is_empty() {
            return Ok(Vec::new());
        }

        // Build dynamic LIKE query
        let mut sql = String::from("SELECT id FROM knowledge WHERE (");
        let mut like_parts: Vec<String> = Vec::new();
        for (i, _) in keywords.iter().enumerate() {
            let p = i + 1;
            like_parts.push(format!(
                "(key LIKE ${p} OR summary LIKE ${p} OR COALESCE(detail,'') LIKE ${p})"
            ));
        }
        sql.push_str(&like_parts.join(" OR "));
        sql.push(')');
        if category.is_some() {
            let p_cat = keywords.len() + 1;
            let p_like = keywords.len() + 2;
            sql.push_str(&format!(" AND (category = ${} OR category LIKE ${})", p_cat, p_like));
        }
        sql.push_str(" ORDER BY access_count DESC, updated_at DESC LIMIT 30");

        let mut q = sqlx::query_as::<_, (String,)>(&sql);
        for kw in &keywords {
            q = q.bind(format!("%{}%", kw));
        }
        if let Some(cat) = category {
            q = q.bind(cat.to_string());
            q = q.bind(format!("{}:%", cat));
        }
        let rows = q.fetch_all(&self.pool).await?;
        Ok(rows.into_iter().enumerate().map(|(rank, (id,))| (id, rank)).collect())
    }

    async fn kb_search_fts_ranked_scoped(&self, query: &str, category: Option<&str>, project_id: Option<&str>) -> DbResult<Vec<(String, usize, Option<String>)>> {
        if project_id.is_none() {
            return self.kb_search_fts_ranked(query, category).await;
        }
        let tsquery = query.split_whitespace()
            .map(|w| w.replace('\'', ""))
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" | ");
        if tsquery.is_empty() {
            return Ok(Vec::new());
        }

        let rows: Vec<(String, Option<String>)> = if let Some(cat) = category {
            let like_pattern = format!("{}:%", cat);
            sqlx::query_as(
                "SELECT k.id, ts_headline('simple', COALESCE(k.detail, k.summary), plainto_tsquery('simple', $1), 'StartSel=**, StopSel=**, MaxFragments=2, MaxWords=40')
                 FROM knowledge k
                 WHERE k.fts_doc @@ plainto_tsquery('simple', $1) AND (k.category = $2 OR k.category LIKE $3)
                   AND (k.project_id = $4 OR k.project_id IS NULL)
                 ORDER BY ts_rank(k.fts_doc, plainto_tsquery('simple', $1)) DESC
                 LIMIT 100"
            )
            .bind(query)
            .bind(cat)
            .bind(like_pattern)
            .bind(project_id.unwrap())
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as(
                "SELECT k.id, ts_headline('simple', COALESCE(k.detail, k.summary), plainto_tsquery('simple', $1), 'StartSel=**, StopSel=**, MaxFragments=2, MaxWords=40')
                 FROM knowledge k
                 WHERE k.fts_doc @@ plainto_tsquery('simple', $1)
                   AND (k.project_id = $2 OR k.project_id IS NULL)
                 ORDER BY ts_rank(k.fts_doc, plainto_tsquery('simple', $1)) DESC
                 LIMIT 100"
            )
            .bind(query)
            .bind(project_id.unwrap())
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows.into_iter().enumerate().map(|(rank, (id, snippet))| {
            let snip = snippet.filter(|s| s.contains("**"));
            (id, rank, snip)
        }).collect())
    }

    async fn kb_search_like_ranked_scoped(&self, query: &str, category: Option<&str>, project_id: Option<&str>) -> DbResult<Vec<(String, usize)>> {
        if project_id.is_none() {
            return self.kb_search_like_ranked(query, category).await;
        }
        let keywords: Vec<String> = {
            let mut kw: Vec<String> = query.split_whitespace().map(|w| w.to_string()).collect();
            let trimmed = query.trim();
            if !trimmed.is_empty() && trimmed.chars().any(|c| !c.is_ascii()) && !kw.contains(&trimmed.to_string()) {
                kw.insert(0, trimmed.to_string());
            }
            kw
        };
        if keywords.is_empty() {
            return Ok(Vec::new());
        }

        let mut sql = String::from("SELECT id FROM knowledge WHERE (");
        let mut like_parts: Vec<String> = Vec::new();
        for (i, _) in keywords.iter().enumerate() {
            let p = i + 1;
            like_parts.push(format!(
                "(key LIKE ${p} OR summary LIKE ${p} OR COALESCE(detail,'') LIKE ${p})"
            ));
        }
        sql.push_str(&like_parts.join(" OR "));
        sql.push(')');
        let mut next_param = keywords.len() + 1;
        if let Some(_cat) = category {
            sql.push_str(&format!(" AND (category = ${} OR category LIKE ${})", next_param, next_param + 1));
            next_param += 2;
        }
        sql.push_str(&format!(" AND (project_id = ${} OR project_id IS NULL)", next_param));
        next_param += 1;
        let _ = next_param;
        sql.push_str(" ORDER BY access_count DESC, updated_at DESC LIMIT 30");

        let mut q = sqlx::query_as::<_, (String,)>(&sql);
        for kw in &keywords {
            q = q.bind(format!("%{}%", kw));
        }
        if let Some(cat) = category {
            q = q.bind(cat.to_string());
            q = q.bind(format!("{}:%", cat));
        }
        q = q.bind(project_id.unwrap().to_string());
        let rows = q.fetch_all(&self.pool).await?;
        Ok(rows.into_iter().enumerate().map(|(rank, (id,))| (id, rank)).collect())
    }

    // ========== Embeddings ==========

    async fn kb_set_embedding(&self, id: &str, embedding: &[f32], provider: &str) -> DbResult<()> {
        let bytes = crate::embedding::f32_vec_to_bytes(embedding);
        // Format as PostgreSQL vector literal: [0.1,0.2,...]
        let vec_str = format!("[{}]", embedding.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(","));
        sqlx::query(
            "UPDATE knowledge SET embedding = $1, embedding_provider = $2, embedding_vec = $3::vector WHERE id = $4"
        )
        .bind(bytes)
        .bind(provider)
        .bind(&vec_str)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn kb_load_embeddings(&self, category: &str) -> DbResult<Vec<(String, Vec<f32>)>> {
        let like_pattern = format!("{}:%", category);
        let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT id, embedding FROM knowledge
             WHERE (category = $1 OR category LIKE $2) AND embedding IS NOT NULL"
        )
        .bind(category)
        .bind(like_pattern)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id, blob)| {
            (id, crate::embedding::bytes_to_f32_vec(&blob))
        }).collect())
    }

    async fn kb_load_all_embeddings(&self) -> DbResult<Vec<(String, Vec<f32>)>> {
        let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT id, embedding FROM knowledge WHERE embedding IS NOT NULL"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id, blob)| {
            (id, crate::embedding::bytes_to_f32_vec(&blob))
        }).collect())
    }

    async fn kb_entries_missing_embedding(&self, category: Option<&str>) -> DbResult<Vec<(String, String, String)>> {
        if let Some(cat) = category {
            let like_pattern = format!("{}:%", cat);
            let rows: Vec<(String, String, String)> = sqlx::query_as(
                "SELECT id, summary, COALESCE(detail, '') FROM knowledge
                 WHERE (category = $1 OR category LIKE $2) AND embedding IS NULL"
            )
            .bind(cat)
            .bind(like_pattern)
            .fetch_all(&self.pool)
            .await?;
            Ok(rows)
        } else {
            let rows: Vec<(String, String, String)> = sqlx::query_as(
                "SELECT id, summary, COALESCE(detail, '') FROM knowledge WHERE embedding IS NULL"
            )
            .fetch_all(&self.pool)
            .await?;
            Ok(rows)
        }
    }

    async fn kb_entries_stale_embedding(&self, current_provider: &str, limit: i64) -> DbResult<Vec<(String, String, String)>> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT id, summary, COALESCE(detail, '') FROM knowledge
             WHERE embedding IS NOT NULL AND embedding_provider IS NOT NULL
               AND embedding_provider != $1
             LIMIT $2"
        )
        .bind(current_provider)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // ========== Stats & GC ==========

    async fn kb_stats(&self) -> DbResult<serde_json::Value> {
        let (total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM knowledge")
            .fetch_one(&self.pool).await?;
        let (never_accessed,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM knowledge WHERE access_count = 0 AND last_accessed_at IS NULL"
        ).fetch_one(&self.pool).await?;

        let (utility_high,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM knowledge WHERE utility_score >= 0.7"
        ).fetch_one(&self.pool).await.unwrap_or((0,));
        let (utility_medium,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM knowledge WHERE utility_score >= 0.3 AND utility_score < 0.7"
        ).fetch_one(&self.pool).await.unwrap_or((0,));
        let (utility_low,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM knowledge WHERE utility_score < 0.3"
        ).fetch_one(&self.pool).await.unwrap_or((0,));

        let (gc_candidates,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM knowledge WHERE
             utility_score * POWER(
               CASE
                 WHEN category = 'memory:debug' THEN 0.9517
                 WHEN category = 'memory:bugfix' THEN 0.9772
                 WHEN category = 'memory:ops' THEN 0.9675
                 WHEN category = 'memory:feature' THEN 0.9923
                 WHEN category LIKE 'memory%' THEN 0.9885
                 ELSE 1.0
               END,
               GREATEST(0, EXTRACT(EPOCH FROM (NOW() - COALESCE(last_accessed_at, updated_at)::timestamp)) / 86400)
             ) < 0.15
             AND EXTRACT(EPOCH FROM (NOW() - created_at::timestamp)) / 86400 > 7
             AND category NOT IN ('preference', 'memory:decision', 'memory:architecture', 'project', 'policy:decision')"
        ).fetch_one(&self.pool).await.unwrap_or((0,));

        let most_accessed: Option<(String, String, i64)> = sqlx::query_as(
            "SELECT category, key, access_count FROM knowledge ORDER BY access_count DESC LIMIT 1"
        ).fetch_optional(&self.pool).await?;
        let oldest: Option<(String, String, String)> = sqlx::query_as(
            "SELECT category, key, updated_at FROM knowledge ORDER BY updated_at ASC LIMIT 1"
        ).fetch_optional(&self.pool).await?;

        let mut stats = serde_json::json!({
            "total": total,
            "neverAccessed": never_accessed,
            "utilityDistribution": {
                "high": utility_high,
                "medium": utility_medium,
                "low": utility_low,
            },
            "gcCandidates": gc_candidates,
        });
        if let Some((cat, key, count)) = most_accessed {
            stats["mostAccessed"] = serde_json::json!({"category": cat, "key": key, "accessCount": count});
        }
        if let Some((cat, key, updated)) = oldest {
            stats["oldest"] = serde_json::json!({"category": cat, "key": key, "updatedAt": updated});
        }

        // Category breakdown
        let summary = self.kb_summary().await?;
        let raw: HashMap<String, i64> = summary.into_iter().collect();
        stats["categories"] = serde_json::json!(raw);

        // Parent category rollup
        let mut parents: HashMap<String, i64> = HashMap::new();
        for (cat, count) in &raw {
            let parent = cat.split(':').next().unwrap_or(cat);
            *parents.entry(parent.to_string()).or_default() += count;
        }
        if parents.len() < raw.len() {
            stats["categoryRollup"] = serde_json::json!(parents);
        }

        Ok(stats)
    }

    async fn kb_summary(&self) -> DbResult<Vec<(String, i64)>> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT category, COUNT(*) as cnt FROM knowledge GROUP BY category ORDER BY cnt DESC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn kb_hot_keys(&self, limit: i64) -> DbResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT key FROM knowledge ORDER BY access_count DESC, updated_at DESC LIMIT $1"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn kb_find_stale(&self, days: i64) -> DbResult<Vec<KnowledgeEntry>> {
        let rows: Vec<KBRow> = sqlx::query_as(&format!(
            "SELECT {} FROM knowledge WHERE access_count = 0
             AND last_accessed_at IS NULL
             AND EXTRACT(EPOCH FROM (NOW() - updated_at::timestamp)) / 86400 > $1
             ORDER BY updated_at ASC", KB_COLS
        ))
        .bind(days)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(kb_row_to_entry).collect())
    }

    async fn kb_find_duplicates(&self) -> DbResult<Vec<(KnowledgeEntry, KnowledgeEntry, f64)>> {
        const DUP_THRESHOLD: f64 = 0.6;
        let entries = self.kb_list(None).await?;
        let mut duplicates = Vec::new();

        let mut by_cat: HashMap<String, Vec<&KnowledgeEntry>> = HashMap::new();
        for e in &entries {
            by_cat.entry(e.category.clone()).or_default().push(e);
        }

        for (_cat, group) in &by_cat {
            for i in 0..group.len() {
                let text_a = format!("{} {}", group[i].key, group[i].summary);
                for j in (i + 1)..group.len() {
                    let text_b = format!("{} {}", group[j].key, group[j].summary);
                    let sim = token_jaccard_similarity(&text_a, &text_b);
                    if sim >= DUP_THRESHOLD {
                        duplicates.push((group[i].clone(), group[j].clone(), sim));
                    }
                }
            }
        }

        duplicates.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        Ok(duplicates)
    }

    async fn embedding_stats(&self) -> DbResult<serde_json::Value> {
        let (kb_total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM knowledge")
            .fetch_one(&self.pool).await?;
        let (kb_embedded,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM knowledge WHERE embedding IS NOT NULL"
        ).fetch_one(&self.pool).await?;
        let kb_providers: Vec<(String, i64)> = sqlx::query_as(
            "SELECT COALESCE(embedding_provider, 'none'), COUNT(*) FROM knowledge GROUP BY embedding_provider ORDER BY COUNT(*) DESC"
        ).fetch_all(&self.pool).await?;

        let (skill_total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM skill_topics")
            .fetch_one(&self.pool).await?;
        let (skill_embedded,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM skill_topics WHERE embedding IS NOT NULL"
        ).fetch_one(&self.pool).await?;

        let (conv_total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM conversations")
            .fetch_one(&self.pool).await?;
        let (conv_summarized,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversations WHERE llm_summary IS NOT NULL"
        ).fetch_one(&self.pool).await?;
        let (conv_embedded,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM conversations WHERE embedding IS NOT NULL"
        ).fetch_one(&self.pool).await?;
        let conv_providers: Vec<(String, i64)> = sqlx::query_as(
            "SELECT COALESCE(embedding_provider, 'none'), COUNT(*) FROM conversations WHERE embedding IS NOT NULL GROUP BY embedding_provider ORDER BY COUNT(*) DESC"
        ).fetch_all(&self.pool).await?;

        let kb_provider_map: HashMap<String, i64> = kb_providers.into_iter().collect();
        let conv_provider_map: HashMap<String, i64> = conv_providers.into_iter().collect();

        Ok(serde_json::json!({
            "kb": {
                "total": kb_total,
                "embedded": kb_embedded,
                "missing": kb_total - kb_embedded,
                "coverage": if kb_total > 0 { format!("{:.1}%", kb_embedded as f64 / kb_total as f64 * 100.0) } else { "N/A".into() },
                "providers": kb_provider_map,
            },
            "skill": {
                "total": skill_total,
                "embedded": skill_embedded,
                "missing": skill_total - skill_embedded,
                "coverage": if skill_total > 0 { format!("{:.1}%", skill_embedded as f64 / skill_total as f64 * 100.0) } else { "N/A".into() },
            },
            "conversation": {
                "total": conv_total,
                "summarized": conv_summarized,
                "embedded": conv_embedded,
                "missing": conv_total - conv_embedded,
                "coverage": if conv_total > 0 { format!("{:.1}%", conv_embedded as f64 / conv_total as f64 * 100.0) } else { "N/A".into() },
                "providers": conv_provider_map,
            },
        }))
    }

    async fn kb_auto_gc(&self) -> DbResult<usize> {
        let mut deleted = 0u64;

        // Phase 1: Delete infra entries
        let r = sqlx::query("DELETE FROM knowledge WHERE category = 'infra'")
            .execute(&self.pool).await?;
        deleted += r.rows_affected();

        // Phase 2: Darwinian pruning with read-time decay
        let r = sqlx::query(
            "DELETE FROM knowledge WHERE
             utility_score * POWER(
               CASE
                 WHEN category = 'memory:debug' THEN 0.9517
                 WHEN category = 'memory:bugfix' THEN 0.9772
                 WHEN category = 'memory:ops' THEN 0.9675
                 WHEN category = 'memory:feature' THEN 0.9923
                 WHEN category LIKE 'memory%' THEN 0.9885
                 ELSE 1.0
               END,
               GREATEST(0, EXTRACT(EPOCH FROM (NOW() - COALESCE(last_accessed_at, updated_at)::timestamp)) / 86400)
             ) < 0.15
             AND EXTRACT(EPOCH FROM (NOW() - created_at::timestamp)) / 86400 > 7
             AND category NOT IN ('preference', 'memory:decision', 'memory:architecture', 'project', 'policy:decision')
             AND scope_task_id IS NULL"
        )
        .execute(&self.pool)
        .await?;
        deleted += r.rows_affected();

        // Phase 3: Clean up dangling edges
        if deleted > 0 {
            sqlx::query(
                "DELETE FROM knowledge_edges WHERE source_id NOT IN (SELECT id FROM knowledge)
                 OR target_id NOT IN (SELECT id FROM knowledge)"
            )
            .execute(&self.pool)
            .await?;
        }

        Ok(deleted as usize)
    }

    async fn kb_adjust_confidence(&self, id: &str, delta: f64) -> DbResult<Option<f64>> {
        sqlx::query(
            "UPDATE knowledge SET confidence = GREATEST(0.1, LEAST(1.0, confidence + $1)), updated_at = $2 WHERE id = $3"
        )
        .bind(delta)
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?;
        let row: Option<(f64,)> = sqlx::query_as(
            "SELECT confidence FROM knowledge WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.0))
    }

    async fn kb_batch_apply_utility_feedback(&self, kb_ids: &[String], success: bool) -> DbResult<usize> {
        if kb_ids.is_empty() { return Ok(0); }
        let now = chrono::Utc::now().to_rfc3339();
        let mut count = 0u64;
        if success {
            for id in kb_ids {
                let r = sqlx::query(
                    "UPDATE knowledge SET utility_score = LEAST(1.0, utility_score + $1 * (1.0 - utility_score)),
                     updated_at = $2 WHERE id = $3"
                )
                .bind(UTILITY_HIT_BOOST)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await?;
                count += r.rows_affected();
            }
        } else {
            for id in kb_ids {
                let r = sqlx::query(
                    "UPDATE knowledge SET utility_score = GREATEST(0.1, utility_score * 0.90),
                     updated_at = $1 WHERE id = $2"
                )
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await?;
                count += r.rows_affected();
            }
        }
        Ok(count as usize)
    }

    async fn kb_list_low_confidence(&self, threshold: f64, limit: usize) -> DbResult<Vec<KnowledgeEntry>> {
        let rows: Vec<KBRow> = sqlx::query_as(&format!(
            "SELECT {} FROM knowledge WHERE confidence < $1 ORDER BY confidence ASC LIMIT $2", KB_COLS
        ))
        .bind(threshold)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(kb_row_to_entry).collect())
    }

    async fn kb_list_low_utility(&self, threshold: f64, min_access: i64, limit: usize) -> DbResult<Vec<KnowledgeEntry>> {
        let rows: Vec<KBRow> = sqlx::query_as(&format!(
            "SELECT {} FROM knowledge WHERE utility_score < $1 AND access_count >= $2
             ORDER BY utility_score ASC LIMIT $3", KB_COLS
        ))
        .bind(threshold)
        .bind(min_access)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(kb_row_to_entry).collect())
    }

    async fn kb_mark_needs_re_extraction(&self, ids: &[String]) -> DbResult<usize> {
        if ids.is_empty() { return Ok(0); }
        let now = chrono::Utc::now().to_rfc3339();
        let mut count = 0u64;
        for id in ids {
            let r = sqlx::query(
                "UPDATE knowledge SET needs_re_extraction = 1, updated_at = $1 WHERE id = $2"
            )
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
            count += r.rows_affected();
        }
        Ok(count as usize)
    }

    async fn kb_list_stale_state_entries(&self, stale_days: i64, limit: usize) -> DbResult<Vec<KnowledgeEntry>> {
        let rows: Vec<KBRow> = sqlx::query_as(&format!(
            "SELECT {} FROM knowledge
             WHERE kb_type = 'state'
               AND (last_accessed_at IS NULL OR EXTRACT(EPOCH FROM (NOW() - last_accessed_at::timestamp)) / 86400 > $1)
               AND confidence >= 0.3
             ORDER BY last_accessed_at ASC NULLS FIRST
             LIMIT $2", KB_COLS
        ))
        .bind(stale_days)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(kb_row_to_entry).collect())
    }

    // ========== FTS management ==========

    async fn kb_rebuild_fts_if_dirty(&self) -> DbResult<bool> {
        // In PostgreSQL with GENERATED ALWAYS tsvector, no manual rebuild needed.
        Ok(false)
    }

    // ========== KB Operations queue ==========

    async fn kb_ops_save_plan(&self, plan_id: &str, task_id: Option<&str>, operations: &[KBOperation]) -> DbResult<usize> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut saved = 0usize;
        for (i, op) in operations.iter().enumerate() {
            let id = format!("{}-{}", plan_id, i);
            sqlx::query(
                "INSERT INTO kb_operation_queue (id, plan_id, task_id, operation, target_keys, rationale, status, priority, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, 'pending', $7, $8)
                 ON CONFLICT (id) DO UPDATE SET
                    plan_id = EXCLUDED.plan_id, task_id = EXCLUDED.task_id,
                    operation = EXCLUDED.operation, target_keys = EXCLUDED.target_keys,
                    rationale = EXCLUDED.rationale, status = EXCLUDED.status,
                    priority = EXCLUDED.priority, created_at = EXCLUDED.created_at"
            )
            .bind(&id)
            .bind(plan_id)
            .bind(task_id)
            .bind(&op.operation)
            .bind(serde_json::to_string(&op.target_keys).unwrap_or_default())
            .bind(&op.rationale)
            .bind(i as i32)
            .bind(&now)
            .execute(&self.pool)
            .await?;
            saved += 1;
        }
        Ok(saved)
    }

    async fn kb_ops_list(&self, plan_id: Option<&str>, status: Option<&str>) -> DbResult<Vec<KBOperationRow>> {
        let base = "SELECT id, plan_id, task_id, operation, target_keys, rationale, status, priority, result, created_at, executed_at, error FROM kb_operation_queue";
        type OpRow = (String, String, Option<String>, String, String, Option<String>, String, i32, Option<String>, String, Option<String>, Option<String>);
        let rows: Vec<OpRow> = match (plan_id, status) {
            (Some(pid), Some(s)) => {
                sqlx::query_as(&format!("{} WHERE plan_id = $1 AND status = $2 ORDER BY priority", base))
                    .bind(pid).bind(s)
                    .fetch_all(&self.pool).await?
            }
            (Some(pid), None) => {
                sqlx::query_as(&format!("{} WHERE plan_id = $1 ORDER BY priority", base))
                    .bind(pid)
                    .fetch_all(&self.pool).await?
            }
            (None, Some(s)) => {
                sqlx::query_as(&format!("{} WHERE status = $1 ORDER BY created_at DESC, priority", base))
                    .bind(s)
                    .fetch_all(&self.pool).await?
            }
            (None, None) => {
                sqlx::query_as(&format!("{} ORDER BY created_at DESC, priority", base))
                    .fetch_all(&self.pool).await?
            }
        };
        Ok(rows.into_iter().map(|r| KBOperationRow {
            id: r.0, plan_id: r.1, task_id: r.2, operation: r.3,
            target_keys: r.4, rationale: r.5, status: r.6, priority: r.7,
            result: r.8, created_at: r.9, executed_at: r.10, error: r.11,
        }).collect())
    }

    async fn kb_ops_update_status(&self, op_id: &str, status: &str, result: Option<&str>, error: Option<&str>) -> DbResult<bool> {
        let now = chrono::Utc::now().to_rfc3339();
        let executed_at: Option<&str> = if status == "done" || status == "failed" { Some(&now) } else { None };
        let r = sqlx::query(
            "UPDATE kb_operation_queue SET status = $1, result = $2, error = $3, executed_at = $4 WHERE id = $5"
        )
        .bind(status)
        .bind(result)
        .bind(error)
        .bind(executed_at)
        .bind(op_id)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() > 0)
    }

    async fn kb_ops_complete_by_task_id(&self, task_id: &str, new_status: &str, result: Option<&str>) -> DbResult<bool> {
        let now = chrono::Utc::now().to_rfc3339();
        let pattern = format!("%task_id={}%", task_id);
        let r = sqlx::query(
            "UPDATE kb_operation_queue SET status = $1, result = COALESCE($2, result), executed_at = $3
             WHERE status = 'dispatched' AND result LIKE $4"
        )
        .bind(new_status)
        .bind(result)
        .bind(&now)
        .bind(&pattern)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() > 0)
    }

    async fn kb_ops_expire_stale(&self, ttl_secs: i64) -> DbResult<usize> {
        let now = chrono::Utc::now().to_rfc3339();
        let cutoff = (chrono::Utc::now() - chrono::Duration::seconds(ttl_secs)).to_rfc3339();
        let r = sqlx::query(
            "UPDATE kb_operation_queue SET status = 'expired', executed_at = $1
             WHERE status = 'pending' AND created_at < $2"
        )
        .bind(&now)
        .bind(&cutoff)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() as usize)
    }

    async fn kb_ops_plan_summary(&self, plan_id: &str) -> DbResult<serde_json::Value> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT status, COUNT(*) FROM kb_operation_queue WHERE plan_id = $1 GROUP BY status"
        )
        .bind(plan_id)
        .fetch_all(&self.pool)
        .await?;
        let mut summary = serde_json::Map::new();
        let mut total = 0i64;
        for (s, c) in rows {
            summary.insert(s, serde_json::json!(c));
            total += c;
        }
        summary.insert("total".to_string(), serde_json::json!(total));
        Ok(serde_json::Value::Object(summary))
    }

    // ========== Knowledge Graph edges ==========

    async fn kb_add_edge(&self, source_id: &str, target_id: &str, relation_type: &str, weight: f64) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO knowledge_edges (source_id, target_id, relation_type, weight, created_at)
             VALUES ($1, $2, $3, $4, NOW())
             ON CONFLICT (source_id, target_id, relation_type) DO UPDATE SET
                weight = EXCLUDED.weight, created_at = EXCLUDED.created_at"
        )
        .bind(source_id)
        .bind(target_id)
        .bind(relation_type)
        .bind(weight)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn kb_get_edges(&self, id: &str) -> DbResult<Vec<KBEdge>> {
        let rows: Vec<(String, String, String, f64, String)> = sqlx::query_as(
            "SELECT source_id, target_id, relation_type, weight, created_at
             FROM knowledge_edges WHERE source_id = $1 OR target_id = $1"
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| KBEdge {
            source_id: r.0, target_id: r.1, relation_type: r.2, weight: r.3, created_at: r.4,
        }).collect())
    }

    async fn kb_expand_related(&self, ids: &[String], max_extra: usize) -> DbResult<Vec<String>> {
        if ids.is_empty() || max_extra == 0 { return Ok(vec![]); }
        let id_set: std::collections::HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        let mut neighbors: Vec<(String, f64)> = Vec::new();
        for id in ids {
            let rows: Vec<(String, f64)> = sqlx::query_as(
                "SELECT target_id, weight FROM knowledge_edges WHERE source_id = $1
                 UNION ALL
                 SELECT source_id, weight FROM knowledge_edges WHERE target_id = $1"
            )
            .bind(id)
            .fetch_all(&self.pool)
            .await?;
            for (nid, w) in rows {
                if !id_set.contains(nid.as_str()) {
                    neighbors.push((nid, w));
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        neighbors.retain(|(id, _)| seen.insert(id.clone()));
        neighbors.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        neighbors.truncate(max_extra);
        Ok(neighbors.into_iter().map(|(id, _)| id).collect())
    }

    async fn kb_delete_edges_for(&self, id: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM knowledge_edges WHERE source_id = $1 OR target_id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn kb_log_co_access(&self, kb_ids: &[String], source: &str, session_id: Option<&str>) -> DbResult<()> {
        if kb_ids.len() < 2 { return Ok(()); }
        for id in kb_ids {
            let others: Vec<&str> = kb_ids.iter().filter(|x| *x != id).map(|s| s.as_str()).collect();
            let others_json = serde_json::to_string(&others).unwrap_or_default();
            sqlx::query(
                "INSERT INTO kb_access_log (kb_id, co_accessed_ids, context_source, session_id) VALUES ($1, $2, $3, $4)"
            )
            .bind(id)
            .bind(&others_json)
            .bind(source)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn get_session_cited_kb_ids(&self, session_id: &str) -> DbResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT kb_id FROM kb_access_log WHERE session_id = $1"
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn kb_compute_cooccurrence(&self, since_hours: i64, top_n: usize) -> DbResult<HashMap<String, Vec<String>>> {
        // Clean old data first (created_at is TEXT, cast to timestamp for comparison)
        let cleanup_window = format!("-{} hours", since_hours * 2);
        sqlx::query(
            "DELETE FROM kb_access_log WHERE created_at::timestamp < NOW() + $1::interval"
        )
        .bind(&cleanup_window)
        .execute(&self.pool)
        .await?;

        let cutoff = format!("-{} hours", since_hours);
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT kb_id, co_accessed_ids FROM kb_access_log
             WHERE created_at::timestamp >= NOW() + $1::interval"
        )
        .bind(&cutoff)
        .fetch_all(&self.pool)
        .await?;

        let mut freq: HashMap<String, HashMap<String, u32>> = HashMap::new();
        for (kb_id, co_json) in &rows {
            let co_ids: Vec<String> = serde_json::from_str(co_json).unwrap_or_default();
            let entry = freq.entry(kb_id.clone()).or_default();
            for co_id in co_ids {
                *entry.entry(co_id).or_insert(0) += 1;
            }
        }

        let mut result = HashMap::new();
        for (kb_id, counts) in freq {
            let mut sorted: Vec<(String, u32)> = counts.into_iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(&a.1));
            sorted.truncate(top_n);
            if !sorted.is_empty() {
                result.insert(kb_id, sorted.into_iter().map(|(id, _)| id).collect());
            }
        }
        Ok(result)
    }

    // ========== Prompt snapshots ==========

    async fn save_prompt_snapshot(&self, task_id: &str, prompt: &str, cited_kb_ids: &[String], category: &str) -> DbResult<()> {
        let kb_ids_json = serde_json::to_string(cited_kb_ids).unwrap_or_else(|_| "[]".to_string());
        sqlx::query(
            "INSERT INTO prompt_snapshots (task_id, prompt, cited_kb_ids, category, created_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (task_id) DO UPDATE SET
                prompt = EXCLUDED.prompt, cited_kb_ids = EXCLUDED.cited_kb_ids,
                category = EXCLUDED.category, created_at = EXCLUDED.created_at"
        )
        .bind(task_id)
        .bind(prompt)
        .bind(&kb_ids_json)
        .bind(category)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_prompt_snapshot_outcome(&self, task_id: &str, outcome: &str) -> DbResult<()> {
        sqlx::query("UPDATE prompt_snapshots SET task_outcome = $1 WHERE task_id = $2")
            .bind(outcome)
            .bind(task_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_prompt_snapshots(&self, category: &str, limit: usize) -> DbResult<Vec<(String, String, String)>> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT task_id, prompt, task_outcome FROM prompt_snapshots
             WHERE category = $1 AND task_outcome IS NOT NULL
             ORDER BY created_at DESC LIMIT $2"
        )
        .bind(category)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn list_modified_snapshots(&self, limit: usize) -> DbResult<Vec<(String, String, String, String)>> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT ps.task_id, ps.prompt, ps.cited_kb_ids, ps.created_at
             FROM prompt_snapshots ps
             WHERE ps.task_outcome = 'success'
               AND ps.cited_kb_ids != '[]'
               AND EXISTS (
                   SELECT 1 FROM knowledge k
                   WHERE position(k.id in ps.cited_kb_ids) > 0
                     AND k.updated_at > ps.created_at
               )
             ORDER BY ps.created_at DESC
             LIMIT $1"
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // ========== AST links ==========

    async fn kb_add_ast_link(&self, kb_id: &str, symbol_name: &str, file_path: Option<&str>, ast_node_id: Option<&str>, relation: &str, confidence: f64) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO kb_ast_links (kb_id, ast_node_id, symbol_name, file_path, relation, confidence)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT DO NOTHING"
        )
        .bind(kb_id)
        .bind(ast_node_id)
        .bind(symbol_name)
        .bind(file_path)
        .bind(relation)
        .bind(confidence)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn kb_get_memories_for_symbol(&self, symbol_name: &str) -> DbResult<Vec<KnowledgeEntry>> {
        let rows: Vec<KBRow> = sqlx::query_as(
            "SELECT k.id, k.category, k.key, k.summary, k.detail, k.source, k.confidence,
                    k.access_count, k.created_at, k.updated_at, k.last_accessed_at,
                    k.linked_task_id, k.kb_type, k.scope_task_id, k.utility_score
             FROM knowledge k
             JOIN kb_ast_links l ON l.kb_id = k.id
             WHERE l.symbol_name = $1
             ORDER BY k.utility_score DESC, k.updated_at DESC
             LIMIT 10"
        )
        .bind(symbol_name)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(kb_row_to_entry).collect())
    }

    async fn kb_get_memories_for_file(&self, file_path: &str) -> DbResult<Vec<(String, KnowledgeEntry)>> {
        let rows: Vec<(String, String, String, String, String, Option<String>, String, f64, i64, String, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<f64>)> = sqlx::query_as(
            "SELECT l.symbol_name, k.id, k.category, k.key, k.summary, k.detail,
                    k.source, k.confidence, k.access_count, k.created_at, k.updated_at,
                    k.last_accessed_at, k.linked_task_id, k.kb_type, k.scope_task_id, k.utility_score
             FROM knowledge k
             JOIN kb_ast_links l ON l.kb_id = k.id
             WHERE l.file_path = $1
             ORDER BY l.symbol_name, k.utility_score DESC
             LIMIT 30"
        )
        .bind(file_path)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| {
            let symbol = r.0;
            let entry = row_to_knowledge_entry(
                r.1, r.2, r.3, r.4, r.5, r.6, r.7,
                r.8, r.9, r.10, r.11, r.12, r.13, r.14, r.15,
            );
            (symbol, entry)
        }).collect())
    }

    async fn kb_delete_ast_links_for(&self, kb_id: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM kb_ast_links WHERE kb_id = $1")
            .bind(kb_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn kb_ast_lazy_resolve(&self, link_id: i64, symbol_name: &str) -> DbResult<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM ast_nodes WHERE name = $1 LIMIT 1"
        )
        .bind(symbol_name)
        .fetch_optional(&self.pool)
        .await?;
        if let Some((ref nid,)) = row {
            sqlx::query("UPDATE kb_ast_links SET ast_node_id = $1 WHERE id = $2")
                .bind(nid)
                .bind(link_id)
                .execute(&self.pool)
                .await?;
        }
        Ok(row.map(|r| r.0))
    }
}

// ========== Private helpers ==========

#[cfg(feature = "postgres")]
impl PgMissionStore {
    /// Get KB entry by category + key (no access bump).
    async fn pg_get_by_category_key(&self, category: &str, key: &str) -> DbResult<Option<KnowledgeEntry>> {
        let row: Option<KBRow> = sqlx::query_as(&format!(
            "SELECT {} FROM knowledge WHERE category = $1 AND key = $2", KB_COLS
        ))
        .bind(category)
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(kb_row_to_entry))
    }

    /// List KB entries by category (for fuzzy dedup in kb_remember).
    async fn pg_list_by_category(&self, category: &str) -> DbResult<Vec<KnowledgeEntry>> {
        let like_pattern = format!("{}:%", category);
        let rows: Vec<KBRow> = sqlx::query_as(&format!(
            "SELECT {} FROM knowledge WHERE category = $1 OR category LIKE $2 ORDER BY updated_at DESC", KB_COLS
        ))
        .bind(category)
        .bind(like_pattern)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(kb_row_to_entry).collect())
    }

    /// FTS search using PostgreSQL tsvector.
    async fn pg_search_fts(&self, query: &str, category: Option<&str>) -> DbResult<Vec<KnowledgeEntry>> {
        if query.split_whitespace().count() == 0 {
            return Ok(Vec::new());
        }
        let rows: Vec<KBRow> = if let Some(cat) = category {
            let like_pattern = format!("{}:%", cat);
            sqlx::query_as(&format!(
                "SELECT {} FROM knowledge
                 WHERE fts_doc @@ plainto_tsquery('simple', $1) AND (category = $2 OR category LIKE $3)
                 ORDER BY ts_rank(fts_doc, plainto_tsquery('simple', $1)) DESC", KB_COLS
            ))
            .bind(query)
            .bind(cat)
            .bind(like_pattern)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as(&format!(
                "SELECT {} FROM knowledge
                 WHERE fts_doc @@ plainto_tsquery('simple', $1)
                 ORDER BY ts_rank(fts_doc, plainto_tsquery('simple', $1)) DESC", KB_COLS
            ))
            .bind(query)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows.into_iter().map(kb_row_to_entry).collect())
    }

    /// LIKE fallback for Chinese text and partial matches.
    async fn pg_search_like(&self, query: &str, category: Option<&str>) -> DbResult<Vec<KnowledgeEntry>> {
        let keywords: Vec<String> = {
            let mut kw: Vec<String> = query.split_whitespace().map(|w| w.to_string()).collect();
            let trimmed = query.trim();
            if !trimmed.is_empty() && trimmed.chars().any(|c| !c.is_ascii()) && !kw.contains(&trimmed.to_string()) {
                kw.insert(0, trimmed.to_string());
            }
            kw
        };
        if keywords.is_empty() {
            return Ok(Vec::new());
        }

        let mut sql = format!("SELECT {} FROM knowledge WHERE (", KB_COLS);
        let mut like_parts: Vec<String> = Vec::new();
        for (i, _) in keywords.iter().enumerate() {
            let p = i + 1;
            like_parts.push(format!(
                "(key LIKE ${p} OR summary LIKE ${p} OR COALESCE(detail,'') LIKE ${p})"
            ));
        }
        sql.push_str(&like_parts.join(" OR "));
        sql.push(')');
        if category.is_some() {
            let p_cat = keywords.len() + 1;
            let p_like = keywords.len() + 2;
            sql.push_str(&format!(" AND (category = ${} OR category LIKE ${})", p_cat, p_like));
        }
        sql.push_str(" ORDER BY access_count DESC, updated_at DESC LIMIT 20");

        let mut q = sqlx::query_as::<_, KBRow>(&sql);
        for kw in &keywords {
            q = q.bind(format!("%{}%", kw));
        }
        if let Some(cat) = category {
            q = q.bind(cat.to_string());
            q = q.bind(format!("{}:%", cat));
        }
        let rows = q.fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(kb_row_to_entry).collect())
    }
}
