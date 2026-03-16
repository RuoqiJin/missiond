use rusqlite::{params, Connection, OptionalExtension};
use super::error::DbResult;
use std::sync::atomic::Ordering;
use crate::types::*;
use super::MissionDB;
use std::collections::HashSet;

impl MissionDB {
    // ============ Knowledge Base ============

    /// Redact sensitive patterns from text before sending to external APIs
    pub fn redact_sensitive(text: &str) -> String {
        use once_cell::sync::Lazy;
        static REDACTIONS: Lazy<Vec<(regex::Regex, &'static str)>> = Lazy::new(|| {
            vec![
                (regex::Regex::new(r"(?i)sshpass\s+-p\s+'[^']*'").unwrap(), "sshpass -p '[REDACTED]'"),
                (regex::Regex::new(r"(?i)(password|passwd|pwd|secret_key)\s*[:=]\s*\S+").unwrap(), "$1=[REDACTED]"),
                (regex::Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}(:\d+)?\b").unwrap(), "[IP_REDACTED]"),
            ]
        });
        let mut result = text.to_string();
        for (re, replacement) in REDACTIONS.iter() {
            result = re.replace_all(&result, *replacement).to_string();
        }
        result
    }

    /// Check if text contains sensitive data patterns (passwords, API keys, etc.)
    fn contains_sensitive_data(text: &str) -> bool {
        use once_cell::sync::Lazy;
        static PATTERNS: Lazy<Vec<regex::Regex>> = Lazy::new(|| {
            [
                r"(?i)(password|passwd|pwd|secret_key)\s*[:=]\s*\S+",
                r"(?i)sshpass\s+-p\s+",
                r#"(?i)(api[_-]?key|token|secret)\s*[:=]\s*['"]?[A-Za-z0-9_\-]{20,}"#,
                r"(?i)ssh\s+\S+@\S+.*-p\s+'\S+'",
            ]
            .iter()
            .filter_map(|p| regex::Regex::new(p).ok())
            .collect()
        });
        PATTERNS.iter().any(|re| re.is_match(text))
    }

    /// Remember (upsert) a knowledge entry, with FTS similarity dedup
    pub fn kb_remember(&self, input: &KBRememberInput) -> DbResult<KBRememberResult> {
        let now = chrono::Utc::now().to_rfc3339();
        let source = input.source.as_deref().unwrap_or("conversation");
        let mut confidence = input.confidence.unwrap_or(1.0);
        let detail_str = input.detail.as_ref().map(|d| serde_json::to_string(d).unwrap_or_default());

        // Guard: reject infra category (servers.yaml + mission_infra_get already covers this)
        if input.category == "infra" {
            tracing::warn!(key = %input.key, "kb_remember: rejected infra category (use servers.yaml)");
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
                    updated_at: now.clone(),
                    last_accessed_at: None,
                    linked_task_id: None,
                    kb_type: Self::infer_kb_type(&input.category).to_string(),
                    context_snippet: None,
                    scope_task_id: None,
                    utility_score: 0.0,
                },
                action: "rejected".into(),
                merged_key: None,
                similarity: None,
            });
        }

        // Sensitive data detection: warn and lower confidence
        let check_text = format!("{} {} {}", input.summary, detail_str.as_deref().unwrap_or(""), input.key);
        if Self::contains_sensitive_data(&check_text) {
            tracing::warn!(key = %input.key, category = %input.category,
                "KB entry may contain sensitive data (password/API key/credentials)");
            confidence = confidence.min(0.5);
        }

        let conn = self.conn();

        // 1. Exact match by (category, key) → update
        // Reset utility_score to 0.8 on explicit update (Gemini ARB: user-updated entries must not be low-ranked)
        let updated = conn.execute(
            "UPDATE knowledge SET summary = ?1, detail = ?2, source = ?3, confidence = ?4, updated_at = ?5,
             utility_score = MAX(utility_score, 0.8)
             WHERE category = ?6 AND key = ?7",
            params![input.summary, detail_str, source, confidence, now, input.category, input.key],
        )?;

        if updated > 0 {
            let entry = Self::kb_get_by_category_key_with_conn(&conn, &input.category, &input.key)?;
            if let Some(ref e) = entry {
                Self::kb_sync_fts_with_conn(&conn, e)?;
            }
            return Ok(KBRememberResult {
                entry: entry.unwrap(),
                action: "updated".to_string(),
                merged_key: None,
                similarity: None,
            });
        }

        // 1b. Same key, different category → update in-place (re-categorize)
        let existing_by_key: Option<KnowledgeEntry> = {
            let mut stmt = conn.prepare("SELECT * FROM knowledge WHERE key = ?1")?;
            let mut rows = stmt.query(params![input.key])?;
            if let Some(row) = rows.next()? {
                Some(Self::row_to_knowledge_entry(row)?)
            } else {
                None
            }
        };
        if let Some(existing) = existing_by_key {
            let old_category = existing.category.clone();
            conn.execute(
                "UPDATE knowledge SET category = ?1, summary = ?2, detail = ?3, source = ?4, confidence = ?5, updated_at = ?6,
                 utility_score = MAX(utility_score, 0.8)
                 WHERE id = ?7",
                params![input.category, input.summary, detail_str, source, confidence, now, existing.id],
            )?;
            let entry = Self::kb_get_by_category_key_with_conn(&conn, &input.category, &input.key)?;
            if let Some(ref e) = entry {
                Self::kb_sync_fts_with_conn(&conn, e)?;
            }
            tracing::info!(key = %input.key, from = %old_category, to = %input.category, "KB entry re-categorized");
            return Ok(KBRememberResult {
                entry: entry.unwrap(),
                action: "updated".to_string(),
                merged_key: None,
                similarity: None,
            });
        }

        // 2. Fuzzy dedup: check for similar entries in same category
        const SIMILARITY_THRESHOLD: f64 = 0.5;
        if let Some((sim, existing)) = Self::kb_find_similar_with_conn(
            &conn,
            &input.category,
            &format!("{} {}", input.key, input.summary),
            SIMILARITY_THRESHOLD,
        )? {
            // Merge: update the existing entry with new summary
            conn.execute(
                "UPDATE knowledge SET summary = ?1, detail = ?2, source = ?3, confidence = ?4, updated_at = ?5,
                 utility_score = MAX(utility_score, 0.8)
                 WHERE id = ?6",
                params![input.summary, detail_str, source, confidence, now, existing.id],
            )?;
            let merged_key = existing.key.clone();
            let entry = Self::kb_get_by_category_key_with_conn(&conn, &existing.category, &existing.key)?;
            if let Some(ref e) = entry {
                Self::kb_sync_fts_with_conn(&conn, e)?;
            }
            return Ok(KBRememberResult {
                entry: entry.unwrap(),
                action: "merged".to_string(),
                merged_key: Some(merged_key),
                similarity: Some(sim),
            });
        }

        // 3. Insert new
        let id = uuid::Uuid::new_v4().to_string();
        let kb_type = Self::infer_kb_type(&input.category);
        conn.execute(
            "INSERT INTO knowledge (id, category, key, summary, detail, source, confidence, access_count, created_at, updated_at, kb_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10)",
            params![id, input.category, input.key, input.summary, detail_str, source, confidence, now, now, kb_type],
        )?;

        let entry = KnowledgeEntry {
            id: id.clone(),
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

        Self::kb_sync_fts_with_conn(&conn, &entry)?;

        Ok(KBRememberResult {
            entry,
            action: "created".to_string(),
            merged_key: None,
            similarity: None,
        })
    }

    /// Find the most similar entry in the same category (uses existing connection)
    fn kb_find_similar_with_conn(
        conn: &Connection,
        category: &str,
        new_text: &str,
        threshold: f64,
    ) -> DbResult<Option<(f64, KnowledgeEntry)>> {
        let entries = Self::kb_list_with_conn(conn, Some(category))?;

        let mut best: Option<(f64, KnowledgeEntry)> = None;
        for entry in entries {
            let existing_text = format!("{} {}", entry.key, entry.summary);
            let sim = token_jaccard_similarity(new_text, &existing_text);
            if sim >= threshold {
                match &best {
                    None => best = Some((sim, entry)),
                    Some((best_sim, _)) if sim > *best_sim => best = Some((sim, entry)),
                    _ => {}
                }
            }
        }

        Ok(best)
    }

    /// Set linked_task_id on a knowledge entry (for Board-aware consolidation)
    pub fn kb_set_linked_task_id(&self, key: &str, task_id: Option<&str>) -> DbResult<bool> {
        let conn = self.conn();
        let updated = conn.execute(
            "UPDATE knowledge SET linked_task_id = ?1 WHERE key = ?2",
            params![task_id, key],
        )?;
        Ok(updated > 0)
    }

    /// Partial update: only modify specified fields. Returns (updated_entry, content_changed).
    pub fn kb_update(
        &self,
        key: &str,
        new_category: Option<&str>,
        new_summary: Option<&str>,
        new_detail: Option<&serde_json::Value>,
        new_confidence: Option<f64>,
        new_linked_task_id: Option<&str>,
    ) -> DbResult<Option<(KnowledgeEntry, bool)>> {
        let conn = self.conn();
        // Find existing entry
        let existing: Option<KnowledgeEntry> = {
            let mut stmt = conn.prepare("SELECT * FROM knowledge WHERE key = ?1")?;
            let mut rows = stmt.query(params![key])?;
            if let Some(row) = rows.next()? {
                Some(Self::row_to_knowledge_entry(row)?)
            } else {
                None
            }
        };
        let existing = match existing {
            Some(e) => e,
            None => return Ok(None),
        };

        let now = chrono::Utc::now().to_rfc3339();
        let mut content_changed = false;

        // Build dynamic SET clauses
        let mut sets: Vec<String> = vec!["updated_at = ?1".to_string()];
        let mut param_idx = 2u32;

        // Collect values as strings for rusqlite params
        let detail_str = new_detail.map(|d| serde_json::to_string(d).unwrap_or_default());

        if new_category.is_some() { sets.push(format!("category = ?{}", param_idx)); param_idx += 1; }
        if new_summary.is_some() { sets.push(format!("summary = ?{}", param_idx)); param_idx += 1; content_changed = true; }
        if detail_str.is_some() { sets.push(format!("detail = ?{}", param_idx)); param_idx += 1; content_changed = true; }
        if new_confidence.is_some() { sets.push(format!("confidence = ?{}", param_idx)); param_idx += 1; }
        if new_linked_task_id.is_some() { sets.push(format!("linked_task_id = ?{}", param_idx)); param_idx += 1; }

        // Only updated_at — nothing else to change
        if param_idx == 2 {
            return Ok(Some((existing, false)));
        }

        let sql = format!("UPDATE knowledge SET {} WHERE id = ?{}", sets.join(", "), param_idx);

        // Build params dynamically
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(now)];
        if let Some(v) = new_category { param_values.push(Box::new(v.to_string())); }
        if let Some(v) = new_summary { param_values.push(Box::new(v.to_string())); }
        if let Some(v) = detail_str { param_values.push(Box::new(v)); }
        if let Some(v) = new_confidence { param_values.push(Box::new(v)); }
        if let Some(v) = new_linked_task_id {
            let val = if v.is_empty() { None } else { Some(v.to_string()) };
            param_values.push(Box::new(val));
        }
        param_values.push(Box::new(existing.id.clone()));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|b| b.as_ref()).collect();
        conn.execute(&sql, param_refs.as_slice())?;

        // Re-fetch & sync FTS
        let final_category = new_category.unwrap_or(&existing.category);
        let entry = Self::kb_get_by_category_key_with_conn(&conn, final_category, key)?;
        if let Some(ref e) = entry {
            Self::kb_sync_fts_with_conn(&conn, e)?;
        }

        let entry = entry.unwrap_or(existing);
        Ok(Some((entry, content_changed)))
    }

    /// Get a knowledge entry by key
    /// Hit boost constant for utility_score: score += HIT_BOOST * (1.0 - score)
    const UTILITY_HIT_BOOST: f64 = 0.15;

    pub fn kb_get(&self, key: &str) -> DbResult<Option<KnowledgeEntry>> {
        let conn = self.conn(); // Need write conn for access_count update
        let mut stmt = conn.prepare(
            "SELECT * FROM knowledge WHERE key = ?1"
        )?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            let mut entry = Self::row_to_knowledge_entry(row)?;
            // Bump access count + utility score (Darwinian boost)
            let now = chrono::Utc::now().to_rfc3339();
            let new_utility = (entry.utility_score + Self::UTILITY_HIT_BOOST * (1.0 - entry.utility_score)).min(1.0);
            conn.execute(
                "UPDATE knowledge SET access_count = access_count + 1, last_accessed_at = ?1,
                 utility_score = ?3 WHERE id = ?2",
                params![now, entry.id, new_utility],
            )?;
            entry.access_count += 1;
            entry.last_accessed_at = Some(now.clone());
            entry.utility_score = new_utility;
            Ok(Some(entry))
        } else {
            Ok(None)
        }
    }

    /// Get by category + key (internal, no access bump, uses existing connection)
    fn kb_get_by_category_key_with_conn(conn: &Connection, category: &str, key: &str) -> DbResult<Option<KnowledgeEntry>> {
        let mut stmt = conn.prepare(
            "SELECT * FROM knowledge WHERE category = ?1 AND key = ?2"
        )?;
        let mut rows = stmt.query(params![category, key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_knowledge_entry(row)?))
        } else {
            Ok(None)
        }
    }

    // ── Embedding storage methods ──────────────────────────────────

    /// Store embedding BLOB + provider tag for a knowledge entry (f32 little-endian bytes)
    pub fn kb_set_embedding(&self, id: &str, embedding: &[f32], provider: &str) -> DbResult<()> {
        let conn = self.conn();
        let bytes = crate::embedding::f32_vec_to_bytes(embedding);
        conn.execute(
            "UPDATE knowledge SET embedding = ?1, embedding_provider = ?2 WHERE id = ?3",
            params![bytes, provider, id],
        )?;
        Ok(())
    }

    /// List KB entries with embedding from a different provider (stale after model switch).
    pub fn kb_entries_stale_embedding(&self, current_provider: &str, limit: i64) -> DbResult<Vec<(String, String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, summary, COALESCE(detail, '') FROM knowledge
             WHERE embedding IS NOT NULL AND embedding_provider IS NOT NULL
               AND embedding_provider != ?1
             LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![current_provider, limit], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        Ok(rows.collect::<Result<Vec<_>, rusqlite::Error>>()?)
    }

    /// Load all embeddings for a given category (e.g. "policy:decision").
    /// Returns Vec<(id, embedding)> for entries with non-NULL embeddings.
    pub fn kb_load_embeddings(&self, category: &str) -> DbResult<Vec<(String, Vec<f32>)>> {
        let conn = self.read_conn();
        let like_pattern = format!("{}:%", category);
        let mut stmt = conn.prepare(
            "SELECT id, embedding FROM knowledge
             WHERE (category = ?1 OR category LIKE ?2) AND embedding IS NOT NULL"
        )?;
        let rows = stmt.query_map(params![category, like_pattern], |row| {
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

    /// Load ALL KB embeddings regardless of category (for hybrid search cache).
    pub fn kb_load_all_embeddings(&self) -> DbResult<Vec<(String, Vec<f32>)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, embedding FROM knowledge WHERE embedding IS NOT NULL"
        )?;
        let rows = stmt.query_map([], |row| {
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

    /// FTS5-based search returning ranked (id, rank) pairs for RRF merge.
    /// FTS5 ranked search returning (id, rank, snippet).
    /// Snippet extracts hit context from detail (col 2) with markdown bold highlights,
    /// falling back to summary (col 1) if detail has no match.
    pub fn kb_search_fts_ranked(&self, query: &str, category: Option<&str>) -> DbResult<Vec<(String, usize, Option<String>)>> {
        let fts_query = query.split_whitespace()
            .map(|w| format!("\"{}\"", w.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" OR ");
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.read_conn();
        let mut results = Vec::new();
        // snippet(fts_table, col_idx, open_mark, close_mark, ellipsis, max_tokens)
        // col 2 = detail, col 1 = summary
        let snippet_sql = "snippet(knowledge_fts, 2, '**', '**', '...', 40)";
        let snippet_fallback_sql = "snippet(knowledge_fts, 1, '**', '**', '...', 30)";
        if let Some(cat) = category {
            let like_pattern = format!("{}:%", cat);
            let sql = format!(
                "SELECT k.id, {snip}, {snip_fb} FROM knowledge k
                 JOIN knowledge_fts f ON k.rowid = f.rowid
                 WHERE knowledge_fts MATCH ?1 AND (k.category = ?2 OR k.category LIKE ?3)
                 ORDER BY rank LIMIT 100",
                snip = snippet_sql, snip_fb = snippet_fallback_sql
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![fts_query, cat, like_pattern], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            for (rank, row) in rows.enumerate() {
                let (id, detail_snip, summary_snip) = row?;
                // Use detail snippet if non-empty and contains highlight markers
                let snippet = if !detail_snip.is_empty() && detail_snip.contains("**") {
                    Some(detail_snip)
                } else if !summary_snip.is_empty() && summary_snip.contains("**") {
                    Some(summary_snip)
                } else {
                    None
                };
                results.push((id, rank, snippet));
            }
        } else {
            let sql = format!(
                "SELECT k.id, {snip}, {snip_fb} FROM knowledge k
                 JOIN knowledge_fts f ON k.rowid = f.rowid
                 WHERE knowledge_fts MATCH ?1
                 ORDER BY rank LIMIT 100",
                snip = snippet_sql, snip_fb = snippet_fallback_sql
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![fts_query], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            for (rank, row) in rows.enumerate() {
                let (id, detail_snip, summary_snip) = row?;
                let snippet = if !detail_snip.is_empty() && detail_snip.contains("**") {
                    Some(detail_snip)
                } else if !summary_snip.is_empty() && summary_snip.contains("**") {
                    Some(summary_snip)
                } else {
                    None
                };
                results.push((id, rank, snippet));
            }
        }
        Ok(results)
    }

    /// LIKE-based search returning ranked (id, rank) pairs for RRF merge.
    pub fn kb_search_like_ranked(&self, query: &str, category: Option<&str>) -> DbResult<Vec<(String, usize)>> {
        let keywords: Vec<String> = {
            let mut kw: Vec<String> = query.split_whitespace()
                .map(|w| w.to_string())
                .collect();
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
        for (i, _kw) in keywords.iter().enumerate() {
            let p = i + 1;
            like_parts.push(format!(
                "(key LIKE ?{p} OR summary LIKE ?{p} OR COALESCE(detail,'') LIKE ?{p})"
            ));
        }
        sql.push_str(&like_parts.join(" OR "));
        sql.push(')');
        if category.is_some() {
            let p_cat = keywords.len() + 1;
            let p_like = keywords.len() + 2;
            sql.push_str(&format!(" AND (category = ?{} OR category LIKE ?{})", p_cat, p_like));
        }
        sql.push_str(" ORDER BY access_count DESC, updated_at DESC LIMIT 30");
        let conn = self.read_conn();
        let mut stmt = conn.prepare(&sql)?;
        let like_params: Vec<String> = keywords.iter().map(|kw| format!("%{}%", kw)).collect();
        let mut param_values: Vec<&dyn rusqlite::types::ToSql> = like_params.iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();
        let cat_owned: String;
        let cat_like: String;
        if let Some(cat) = category {
            cat_owned = cat.to_string();
            cat_like = format!("{}:%", cat);
            param_values.push(&cat_owned);
            param_values.push(&cat_like);
        }
        let rows = stmt.query_map(rusqlite::params_from_iter(param_values.iter()), |row| {
            row.get::<_, String>(0)
        })?;
        let mut results = Vec::new();
        for (rank, id) in rows.enumerate() {
            results.push((id?, rank));
        }
        Ok(results)
    }

    /// List knowledge entries where embedding IS NULL (for backfill).
    /// Returns (id, summary, detail_text).
    pub fn kb_entries_missing_embedding(&self, category: Option<&str>) -> DbResult<Vec<(String, String, String)>> {
        let conn = self.read_conn();
        let mut entries = Vec::new();
        if let Some(cat) = category {
            let like_pattern = format!("{}:%", cat);
            let mut stmt = conn.prepare(
                "SELECT id, summary, COALESCE(detail, '') FROM knowledge
                 WHERE (category = ?1 OR category LIKE ?2) AND embedding IS NULL"
            )?;
            let rows = stmt.query_map(params![cat, like_pattern], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?;
            for row in rows {
                entries.push(row?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, summary, COALESCE(detail, '') FROM knowledge WHERE embedding IS NULL"
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?;
            for row in rows {
                entries.push(row?);
            }
        }
        Ok(entries)
    }

    /// Get a knowledge entry by its ID (for hybrid search result lookup)
    pub fn kb_get_by_id(&self, id: &str) -> DbResult<Option<KnowledgeEntry>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare("SELECT * FROM knowledge WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_knowledge_entry(row)?))
        } else {
            Ok(None)
        }
    }

    /// Get a knowledge entry ID by key (lightweight, no access bump)
    pub fn kb_get_id_by_key(&self, key: &str) -> DbResult<Option<String>> {
        let conn = self.read_conn();
        Ok(conn.query_row(
            "SELECT id FROM knowledge WHERE key = ?1",
            params![key],
            |row| row.get(0),
        ).optional()?)
    }

    /// Search knowledge with ranked results (for hybrid search RRF fusion).
    /// Returns (entry, rank_position) where rank is 0-based.
    pub fn kb_search_ranked(
        &self,
        query: &str,
        category: Option<&str>,
        limit: usize,
    ) -> DbResult<Vec<(KnowledgeEntry, usize)>> {
        let results = self.kb_search(query, category)?;
        Ok(results
            .into_iter()
            .take(limit)
            .enumerate()
            .map(|(i, e)| (e, i))
            .collect())
    }

    /// Search knowledge via FTS, with LIKE fallback for Chinese text.
    /// NOTE: Does NOT bump access_count. Callers that represent user-initiated searches
    /// should explicitly call `kb_update_access_stats()` on the results.
    pub fn kb_search(&self, query: &str, category: Option<&str>) -> DbResult<Vec<KnowledgeEntry>> {
        // Phase 1: FTS5 search (works well for English / space-separated tokens)
        let results = self.kb_search_fts(query, category)?;
        let results = if results.is_empty() {
            // Phase 2: LIKE fallback (works for Chinese and partial matches)
            self.kb_search_like(query, category)?
        } else {
            results
        };

        Ok(results)
    }

    /// Batch update access_count, last_accessed_at, and utility_score for search hits.
    /// Anti-spam: skip utility boost if last_accessed_at < 1 hour ago.
    pub fn kb_update_access_stats(&self, entries: &[KnowledgeEntry]) -> DbResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now();
        let now_str = now.to_rfc3339();
        let mut stmt = conn.prepare(
            "UPDATE knowledge SET access_count = access_count + 1, last_accessed_at = ?1,
             utility_score = MIN(1.0, utility_score + ?3 * (1.0 - utility_score))
             WHERE id = ?2
             AND (last_accessed_at IS NULL OR julianday(?1) - julianday(last_accessed_at) > 0.0417)"
        )?;
        for entry in entries {
            stmt.execute(params![now_str, entry.id, Self::UTILITY_HIT_BOOST])?;
        }
        Ok(())
    }

    /// FTS5-based search
    fn kb_search_fts(&self, query: &str, category: Option<&str>) -> DbResult<Vec<KnowledgeEntry>> {
        let fts_query = query.split_whitespace()
            .map(|w| format!("\"{}\"", w.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" OR ");

        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.read_conn();
        let mut results = Vec::new();

        if let Some(cat) = category {
            let like_pattern = format!("{}:%", cat);
            let mut stmt = conn.prepare(
                "SELECT k.* FROM knowledge k
                 JOIN knowledge_fts f ON k.rowid = f.rowid
                 WHERE knowledge_fts MATCH ?1 AND (k.category = ?2 OR k.category LIKE ?3)
                 ORDER BY rank"
            )?;
            let rows = stmt.query_map(params![fts_query, cat, like_pattern], |row| Self::row_to_knowledge_entry(row))?;
            for entry in rows {
                results.push(entry?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT k.* FROM knowledge k
                 JOIN knowledge_fts f ON k.rowid = f.rowid
                 WHERE knowledge_fts MATCH ?1
                 ORDER BY rank"
            )?;
            let rows = stmt.query_map(params![fts_query], |row| Self::row_to_knowledge_entry(row))?;
            for entry in rows {
                results.push(entry?);
            }
        }

        Ok(results)
    }

    /// LIKE-based fallback search for Chinese and partial matches
    fn kb_search_like(&self, query: &str, category: Option<&str>) -> DbResult<Vec<KnowledgeEntry>> {
        let keywords: Vec<String> = {
            let mut kw: Vec<String> = query.split_whitespace()
                .map(|w| w.to_string())
                .collect();
            // Add the full query as a keyword if it contains non-ASCII (Chinese)
            let trimmed = query.trim();
            if !trimmed.is_empty() && trimmed.chars().any(|c| !c.is_ascii()) && !kw.contains(&trimmed.to_string()) {
                kw.insert(0, trimmed.to_string());
            }
            kw
        };

        if keywords.is_empty() {
            return Ok(Vec::new());
        }

        // Build dynamic SQL with per-keyword LIKE parameters
        let mut sql = String::from("SELECT * FROM knowledge WHERE (");
        let mut like_parts: Vec<String> = Vec::new();
        for (i, _kw) in keywords.iter().enumerate() {
            let p = i + 1; // 1-indexed
            like_parts.push(format!(
                "(key LIKE ?{p} OR summary LIKE ?{p} OR COALESCE(detail,'') LIKE ?{p})"
            ));
        }
        sql.push_str(&like_parts.join(" OR "));
        sql.push(')');

        if category.is_some() {
            let p_cat = keywords.len() + 1;
            let p_like = keywords.len() + 2;
            sql.push_str(&format!(" AND (category = ?{} OR category LIKE ?{})", p_cat, p_like));
        }
        sql.push_str(" ORDER BY access_count DESC, updated_at DESC LIMIT 20");

        let conn = self.read_conn();
        let mut stmt = conn.prepare(&sql)?;

        // Bind parameters: %keyword% for each, then optional category + like pattern
        let like_params: Vec<String> = keywords.iter().map(|kw| format!("%{}%", kw)).collect();
        let mut param_values: Vec<&dyn rusqlite::types::ToSql> = like_params.iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();
        let cat_owned: String;
        let cat_like: String;
        if let Some(cat) = category {
            cat_owned = cat.to_string();
            cat_like = format!("{}:%", cat);
            param_values.push(&cat_owned);
            param_values.push(&cat_like);
        }

        let rows = stmt.query_map(rusqlite::params_from_iter(param_values.iter()), |row| {
            Self::row_to_knowledge_entry(row)
        })?;

        let mut results = Vec::new();
        for entry in rows {
            results.push(entry?);
        }
        Ok(results)
    }

    /// List knowledge entries, optionally filtered by category.
    /// Supports composite categories: querying "memory" also returns "memory:architecture" etc.
    pub fn kb_list(&self, category: Option<&str>) -> DbResult<Vec<KnowledgeEntry>> {
        let conn = self.read_conn();
        Self::kb_list_with_conn(&conn, category)
    }

    /// List knowledge entries using an existing connection (no lock)
    fn kb_list_with_conn(conn: &Connection, category: Option<&str>) -> DbResult<Vec<KnowledgeEntry>> {
        let mut entries = Vec::new();
        if let Some(cat) = category {
            let like_pattern = format!("{}:%", cat);
            let mut stmt = conn.prepare(
                "SELECT * FROM knowledge WHERE category = ?1 OR category LIKE ?2 ORDER BY updated_at DESC"
            )?;
            let rows = stmt.query_map(params![cat, like_pattern], |row| Self::row_to_knowledge_entry(row))?;
            for entry in rows {
                entries.push(entry?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT * FROM knowledge ORDER BY category, updated_at DESC"
            )?;
            let rows = stmt.query_map([], |row| Self::row_to_knowledge_entry(row))?;
            for entry in rows {
                entries.push(entry?);
            }
        }
        Ok(entries)
    }

    /// List knowledge entries with pagination support.
    /// Used by kb_analyze v2 for chunked analysis.
    pub fn kb_list_paginated(&self, category: Option<&str>, limit: u32, offset: u32) -> DbResult<Vec<KnowledgeEntry>> {
        let conn = self.read_conn();
        let mut entries = Vec::new();
        if let Some(cat) = category {
            let like_pattern = format!("{}:%", cat);
            let mut stmt = conn.prepare(
                "SELECT * FROM knowledge WHERE category = ?1 OR category LIKE ?2 ORDER BY updated_at DESC LIMIT ?3 OFFSET ?4"
            )?;
            let rows = stmt.query_map(params![cat, like_pattern, limit, offset], |row| Self::row_to_knowledge_entry(row))?;
            for entry in rows {
                entries.push(entry?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT * FROM knowledge ORDER BY category, updated_at DESC LIMIT ?1 OFFSET ?2"
            )?;
            let rows = stmt.query_map(params![limit, offset], |row| Self::row_to_knowledge_entry(row))?;
            for entry in rows {
                entries.push(entry?);
            }
        }
        Ok(entries)
    }

    /// Forget (delete) a knowledge entry by key
    pub fn kb_forget(&self, key: &str) -> DbResult<bool> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM knowledge WHERE key = ?1",
            params![key],
        )?;
        if deleted > 0 {
            self.fts_dirty.store(true, Ordering::Relaxed);
        }
        Ok(deleted > 0)
    }

    /// Batch delete multiple knowledge entries by keys. Returns count of deleted entries.
    pub fn kb_batch_forget(&self, keys: &[String]) -> DbResult<usize> {
        let conn = self.conn();
        let mut deleted = 0;
        for key in keys {
            let n = conn.execute(
                "DELETE FROM knowledge WHERE key = ?1",
                params![key],
            )?;
            if n > 0 {
                deleted += 1;
            }
        }
        if deleted > 0 {
            // Immediate rebuild after batch — don't wait for autopilot_tick
            conn.execute_batch("INSERT INTO knowledge_fts(knowledge_fts) VALUES('rebuild')")?;
            self.fts_dirty.store(false, Ordering::Relaxed);
            tracing::info!(deleted, "kb_batch_forget: FTS rebuilt after batch delete");
        }
        Ok(deleted)
    }

    /// List KB entries scoped to a specific task (Working Memory scratchpad).
    pub fn kb_list_by_scope(&self, task_id: &str) -> DbResult<Vec<KnowledgeEntry>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM knowledge WHERE scope_task_id = ?1"
        )?;
        let entries = stmt.query_map(params![task_id], Self::row_to_knowledge_entry)?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    /// Graduate a scratchpad entry to global KB by clearing its scope.
    pub fn kb_clear_scope(&self, id: &str) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE knowledge SET scope_task_id = NULL, updated_at = ?1 WHERE id = ?2",
            params![chrono::Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    /// Save a prompt snapshot for Skill auto-verification replay.
    pub fn save_prompt_snapshot(
        &self, task_id: &str, prompt: &str, cited_kb_ids: &[String], category: &str,
    ) -> DbResult<()> {
        let conn = self.conn();
        let kb_ids_json = serde_json::to_string(cited_kb_ids).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT OR REPLACE INTO prompt_snapshots (task_id, prompt, cited_kb_ids, category, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![task_id, prompt, kb_ids_json, category, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Record task outcome for a prompt snapshot (used by Skill auto-verification).
    pub fn update_prompt_snapshot_outcome(&self, task_id: &str, outcome: &str) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE prompt_snapshots SET task_outcome = ?1 WHERE task_id = ?2",
            params![outcome, task_id],
        )?;
        Ok(())
    }

    /// List prompt snapshots by category for Skill verification replay.
    pub fn list_prompt_snapshots(&self, category: &str, limit: usize) -> DbResult<Vec<(String, String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT task_id, prompt, task_outcome FROM prompt_snapshots
             WHERE category = ?1 AND task_outcome IS NOT NULL
             ORDER BY created_at DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![category, limit as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// Sync FTS index for a knowledge entry (uses existing connection)
    fn kb_sync_fts_with_conn(conn: &Connection, entry: &KnowledgeEntry) -> DbResult<()> {
        let rowid: i64 = conn.query_row(
            "SELECT rowid FROM knowledge WHERE id = ?1",
            params![entry.id],
            |row| row.get(0),
        )?;
        let detail_str = entry.detail.as_ref()
            .map(|d| serde_json::to_string(d).unwrap_or_default())
            .unwrap_or_default();

        // Delete old FTS entry (must provide actual indexed values for external content)
        let old_values: Option<(String, String, String, String)> = conn.query_row(
            "SELECT key, summary, COALESCE(detail, ''), category FROM knowledge WHERE id = ?1",
            params![entry.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).ok();
        if let Some((old_key, old_summary, old_detail, old_category)) = old_values {
            conn.execute(
                "INSERT INTO knowledge_fts(knowledge_fts, rowid, key, summary, detail, category) VALUES('delete', ?1, ?2, ?3, ?4, ?5)",
                params![rowid, old_key, old_summary, old_detail, old_category],
            ).ok();
        }

        conn.execute(
            "INSERT INTO knowledge_fts(rowid, key, summary, detail, category) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![rowid, entry.key, entry.summary, detail_str, entry.category],
        )?;
        Ok(())
    }

    /// Get KB category counts for summary string
    pub fn kb_summary(&self) -> DbResult<Vec<(String, i64)>> {
        tokio::task::block_in_place(|| {
            let conn = self.read_conn();
            let mut stmt = conn.prepare(
                "SELECT category, COUNT(*) as cnt FROM knowledge GROUP BY category ORDER BY cnt DESC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            Ok(rows.collect::<Result<Vec<_>, rusqlite::Error>>()?)
        })
    }

    /// Get top N hot keys by access_count (for instructions injection)
    pub fn kb_hot_keys(&self, limit: i64) -> DbResult<Vec<String>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT key FROM knowledge ORDER BY access_count DESC, updated_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, rusqlite::Error>>()?)
    }

    /// Find stale entries: never accessed and older than N days
    pub fn kb_find_stale(&self, days: i64) -> DbResult<Vec<KnowledgeEntry>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM knowledge WHERE access_count = 0 \
             AND last_accessed_at IS NULL \
             AND julianday('now') - julianday(updated_at) > ?1 \
             ORDER BY updated_at ASC",
        )?;
        let rows = stmt.query_map(params![days], |row| Self::row_to_knowledge_entry(row))?;
        Ok(rows.collect::<Result<Vec<_>, rusqlite::Error>>()?)
    }

    /// Find potential duplicates using Jaccard similarity on key+summary text.
    /// Returns pairs with similarity score (threshold: 0.6).
    pub fn kb_find_duplicates(&self) -> DbResult<Vec<(KnowledgeEntry, KnowledgeEntry, f64)>> {
        const DUP_THRESHOLD: f64 = 0.6;
        let entries = self.kb_list(None)?;
        let mut duplicates = Vec::new();

        // Group by category for O(n²) within each category (not globally)
        let mut by_cat: std::collections::HashMap<String, Vec<&KnowledgeEntry>> = std::collections::HashMap::new();
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

        // Sort by similarity descending
        duplicates.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        Ok(duplicates)
    }

    /// Get embedding coverage stats across all three systems (KB, Skill, Conversation).
    pub fn embedding_stats(&self) -> DbResult<serde_json::Value> {
        let conn = self.read_conn();

        // KB stats
        let kb_total: i64 = conn.query_row("SELECT COUNT(*) FROM knowledge", [], |row| row.get(0))?;
        let kb_embedded: i64 = conn.query_row(
            "SELECT COUNT(*) FROM knowledge WHERE embedding IS NOT NULL", [], |row| row.get(0)
        )?;
        let kb_providers: Vec<(String, i64)> = {
            let mut stmt = conn.prepare(
                "SELECT COALESCE(embedding_provider, 'none'), COUNT(*) FROM knowledge GROUP BY embedding_provider ORDER BY COUNT(*) DESC"
            )?;
            let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
            rows.filter_map(|r| r.ok()).collect()
        };

        // Skill stats
        let skill_total: i64 = conn.query_row("SELECT COUNT(*) FROM skill_topics", [], |row| row.get(0))?;
        let skill_embedded: i64 = conn.query_row(
            "SELECT COUNT(*) FROM skill_topics WHERE embedding IS NOT NULL", [], |row| row.get(0)
        )?;

        // Conversation stats
        let conv_total: i64 = conn.query_row("SELECT COUNT(*) FROM conversations", [], |row| row.get(0))?;
        let conv_summarized: i64 = conn.query_row(
            "SELECT COUNT(*) FROM conversations WHERE llm_summary IS NOT NULL", [], |row| row.get(0)
        )?;
        let conv_embedded: i64 = conn.query_row(
            "SELECT COUNT(*) FROM conversations WHERE embedding IS NOT NULL", [], |row| row.get(0)
        )?;
        let conv_providers: Vec<(String, i64)> = {
            let mut stmt = conn.prepare(
                "SELECT COALESCE(embedding_provider, 'none'), COUNT(*) FROM conversations WHERE embedding IS NOT NULL GROUP BY embedding_provider ORDER BY COUNT(*) DESC"
            )?;
            let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
            rows.filter_map(|r| r.ok()).collect()
        };

        let kb_provider_map: std::collections::HashMap<String, i64> = kb_providers.into_iter().collect();
        let conv_provider_map: std::collections::HashMap<String, i64> = conv_providers.into_iter().collect();

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

    /// Get KB statistics for governance
    pub fn kb_stats(&self) -> DbResult<serde_json::Value> {
        let conn = self.read_conn();
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM knowledge", [], |row| row.get(0)
        )?;
        let never_accessed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM knowledge WHERE access_count = 0 AND last_accessed_at IS NULL",
            [], |row| row.get(0),
        )?;
        // Utility score distribution
        let utility_high: i64 = conn.query_row(
            "SELECT COUNT(*) FROM knowledge WHERE utility_score >= 0.7", [], |row| row.get(0)
        ).unwrap_or(0);
        let utility_medium: i64 = conn.query_row(
            "SELECT COUNT(*) FROM knowledge WHERE utility_score >= 0.3 AND utility_score < 0.7", [], |row| row.get(0)
        ).unwrap_or(0);
        let utility_low: i64 = conn.query_row(
            "SELECT COUNT(*) FROM knowledge WHERE utility_score < 0.3", [], |row| row.get(0)
        ).unwrap_or(0);
        let gc_candidates: i64 = conn.query_row(
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
               MAX(0, julianday('now') - julianday(COALESCE(last_accessed_at, updated_at)))
             ) < 0.15
             AND julianday('now') - julianday(created_at) > 7
             AND category NOT IN ('preference', 'memory:decision', 'memory:architecture', 'project', 'policy:decision')",
            [], |row| row.get(0)
        ).unwrap_or(0);
        let most_accessed: Option<(String, String, i64)> = conn.query_row(
            "SELECT category, key, access_count FROM knowledge ORDER BY access_count DESC LIMIT 1",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).ok();
        let oldest: Option<(String, String, String)> = conn.query_row(
            "SELECT category, key, updated_at FROM knowledge ORDER BY updated_at ASC LIMIT 1",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).ok();
        drop(conn);

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

        // Category breakdown (raw subcategories + parent rollup)
        let summary = self.kb_summary()?;
        let raw: std::collections::HashMap<String, i64> = summary.into_iter().collect();
        stats["categories"] = serde_json::json!(raw);

        // Parent category rollup: "memory:architecture" counts toward "memory" total
        let mut parents: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for (cat, count) in &raw {
            let parent = cat.split(':').next().unwrap_or(cat);
            *parents.entry(parent.to_string()).or_default() += count;
        }
        // Only include rollup if there are subcategories
        if parents.len() < raw.len() {
            stats["categoryRollup"] = serde_json::json!(parents);
        }

        Ok(stats)
    }

    /// Darwinian GC: read-time decay + prune low-utility entries.
    /// **No batch UPDATE** — utility_score is decayed at read time (Gemini ARB: eliminate write amplification).
    /// Effective score = base_score * POWER(daily_factor, days_since_last_interaction).
    /// Protected categories (preference, policy:decision, memory:architecture, memory:decision, project)
    /// are never auto-deleted.
    pub fn kb_auto_gc(&self) -> DbResult<usize> {
        let conn = self.conn();
        let mut deleted = 0;

        // Phase 1: Delete infra entries — servers.yaml + mission_infra_get covers this
        deleted += conn.execute("DELETE FROM knowledge WHERE category = 'infra'", [])?;

        // Phase 2: Darwinian pruning with read-time decay (no batch UPDATE needed).
        // effective_score = utility_score * POWER(daily_factor, days_since_interaction)
        // Prune when effective_score < 0.15 AND age > 7d (cold start grace period).
        // CASE maps category → daily_factor (0.5^(1/half_life)):
        //   memory:debug=14d→0.9517, memory:bugfix=30d→0.9772, memory:ops=21d→0.9675,
        //   memory:feature=90d→0.9923, memory*=60d→0.9885, other=1.0 (no decay)
        deleted += conn.execute(
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
               MAX(0, julianday('now') - julianday(COALESCE(last_accessed_at, updated_at)))
             ) < 0.15
             AND julianday('now') - julianday(created_at) > 7
             AND category NOT IN ('preference', 'memory:decision', 'memory:architecture', 'project', 'policy:decision')
             AND scope_task_id IS NULL",
            [],
        )?;

        // Phase 3: Clean up dangling edges for deleted nodes
        if deleted > 0 {
            conn.execute(
                "DELETE FROM knowledge_edges WHERE source_id NOT IN (SELECT id FROM knowledge)
                 OR target_id NOT IN (SELECT id FROM knowledge)",
                [],
            )?;
            conn.execute_batch("INSERT INTO knowledge_fts(knowledge_fts) VALUES('rebuild')")?;
            self.fts_dirty.store(false, Ordering::Relaxed);
            tracing::info!(deleted, "kb_auto_gc (darwin): pruned low-utility entries, cleaned edges, FTS rebuilt");
        }

        Ok(deleted)
    }


    /// Atomically adjust confidence for a KB entry by ID.
    /// Clamps result to [0.1, 1.0]. Returns the new confidence value.
    /// Used by the negative/positive feedback loop after task completion.
    pub fn kb_adjust_confidence(&self, id: &str, delta: f64) -> DbResult<Option<f64>> {
        let conn = self.conn();
        // Atomic: compute new value in SQL, clamp to [0.1, 1.0]
        conn.execute(
            "UPDATE knowledge SET confidence = MAX(0.1, MIN(1.0, confidence + ?1)), updated_at = ?2 WHERE id = ?3",
            params![delta, chrono::Utc::now().to_rfc3339(), id],
        )?;
        // Read back the new value
        let new_conf: Option<f64> = conn.query_row(
            "SELECT confidence FROM knowledge WHERE id = ?1",
            params![id],
            |row| row.get(0),
        ).optional()?;
        Ok(new_conf)
    }

    /// List KB entries with confidence below threshold (for idle exploration).
    pub fn kb_list_low_confidence(&self, threshold: f64, limit: usize) -> DbResult<Vec<KnowledgeEntry>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM knowledge WHERE confidence < ?1 ORDER BY confidence ASC LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![threshold, limit as i64], |row| Self::row_to_knowledge_entry(row))?;
        let mut entries = Vec::new();
        for entry in rows { entries.push(entry?); }
        Ok(entries)
    }

    // ============ KB Operation Queue ============

    /// Save a consolidation plan from kb_analyze into the operation queue
    pub fn kb_ops_save_plan(
        &self,
        plan_id: &str,
        task_id: Option<&str>,
        operations: &[KBOperation],
    ) -> DbResult<usize> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        let mut saved = 0;
        for (i, op) in operations.iter().enumerate() {
            let id = format!("{}-{}", plan_id, i);
            conn.execute(
                "INSERT OR REPLACE INTO kb_operation_queue
                 (id, plan_id, task_id, operation, target_keys, rationale, status, priority, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?8)",
                params![
                    id,
                    plan_id,
                    task_id,
                    op.operation,
                    serde_json::to_string(&op.target_keys).unwrap_or_default(),
                    op.rationale,
                    i as i32,
                    now,
                ],
            )?;
            saved += 1;
        }
        Ok(saved)
    }

    /// List operations by plan_id or all pending
    pub fn kb_ops_list(
        &self,
        plan_id: Option<&str>,
        status: Option<&str>,
    ) -> DbResult<Vec<KBOperationRow>> {
        let conn = self.conn();
        let (sql, params_vec): (String, Vec<Box<dyn rusqlite::ToSql>>) = match (plan_id, status) {
            (Some(pid), Some(s)) => (
                "SELECT id, plan_id, task_id, operation, target_keys, rationale, status, priority, result, created_at, executed_at, error
                 FROM kb_operation_queue WHERE plan_id = ?1 AND status = ?2 ORDER BY priority".to_string(),
                vec![Box::new(pid.to_string()) as Box<dyn rusqlite::ToSql>, Box::new(s.to_string())],
            ),
            (Some(pid), None) => (
                "SELECT id, plan_id, task_id, operation, target_keys, rationale, status, priority, result, created_at, executed_at, error
                 FROM kb_operation_queue WHERE plan_id = ?1 ORDER BY priority".to_string(),
                vec![Box::new(pid.to_string()) as Box<dyn rusqlite::ToSql>],
            ),
            (None, Some(s)) => (
                "SELECT id, plan_id, task_id, operation, target_keys, rationale, status, priority, result, created_at, executed_at, error
                 FROM kb_operation_queue WHERE status = ?1 ORDER BY created_at DESC, priority".to_string(),
                vec![Box::new(s.to_string()) as Box<dyn rusqlite::ToSql>],
            ),
            (None, None) => (
                "SELECT id, plan_id, task_id, operation, target_keys, rationale, status, priority, result, created_at, executed_at, error
                 FROM kb_operation_queue ORDER BY created_at DESC, priority".to_string(),
                vec![],
            ),
        };
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(KBOperationRow {
                id: row.get(0)?,
                plan_id: row.get(1)?,
                task_id: row.get(2)?,
                operation: row.get(3)?,
                target_keys: row.get(4)?,
                rationale: row.get(5)?,
                status: row.get(6)?,
                priority: row.get(7)?,
                result: row.get(8)?,
                created_at: row.get(9)?,
                executed_at: row.get(10)?,
                error: row.get(11)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, rusqlite::Error>>()?)
    }

    /// Update operation status
    pub fn kb_ops_update_status(
        &self,
        op_id: &str,
        status: &str,
        result: Option<&str>,
        error: Option<&str>,
    ) -> DbResult<bool> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        let executed_at = if status == "done" || status == "failed" { Some(now.as_str()) } else { None };
        let updated = conn.execute(
            "UPDATE kb_operation_queue SET status = ?1, result = ?2, error = ?3, executed_at = ?4 WHERE id = ?5",
            params![status, result, error, executed_at, op_id],
        )?;
        Ok(updated > 0)
    }

    /// Update a dispatched kb_operation by its associated submit task_id.
    /// Called when a submit task finishes — updates status and stores the task result.
    pub fn kb_ops_complete_by_task_id(
        &self,
        task_id: &str,
        new_status: &str,
        result: Option<&str>,
    ) -> DbResult<bool> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        let pattern = format!("%task_id={}%", task_id);
        let updated = conn.execute(
            "UPDATE kb_operation_queue SET status = ?1, result = COALESCE(?2, result), executed_at = ?3
             WHERE status = 'dispatched' AND result LIKE ?4",
            params![new_status, result, now, pattern],
        )?;
        Ok(updated > 0)
    }

    /// Expire stale pending kb_operations older than `ttl_secs` seconds.
    /// Returns the number of expired operations.
    pub fn kb_ops_expire_stale(&self, ttl_secs: i64) -> DbResult<usize> {
        let conn = self.conn();
        let cutoff = (chrono::Utc::now() - chrono::Duration::seconds(ttl_secs)).to_rfc3339();
        let updated = conn.execute(
            "UPDATE kb_operation_queue SET status = 'expired', executed_at = ?1
             WHERE status = 'pending' AND created_at < ?2",
            params![chrono::Utc::now().to_rfc3339(), cutoff],
        )?;
        Ok(updated)
    }

    /// Get queue status summary for a plan
    pub fn kb_ops_plan_summary(&self, plan_id: &str) -> DbResult<serde_json::Value> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT status, COUNT(*) FROM kb_operation_queue WHERE plan_id = ?1 GROUP BY status"
        )?;
        let mut summary = serde_json::Map::new();
        let mut total = 0i64;
        stmt.query_map(params![plan_id], |row| {
            let s: String = row.get(0)?;
            let c: i64 = row.get(1)?;
            Ok((s, c))
        })?.for_each(|r| {
            if let Ok((s, c)) = r {
                summary.insert(s, serde_json::json!(c));
                total += c;
            }
        });
        summary.insert("total".to_string(), serde_json::json!(total));
        Ok(serde_json::Value::Object(summary))
    }

    pub(crate) fn row_to_knowledge_entry(row: &rusqlite::Row) -> rusqlite::Result<KnowledgeEntry> {
        let detail_str: Option<String> = row.get("detail")?;
        let detail = detail_str.and_then(|s| serde_json::from_str(&s).ok());
        let kb_type: String = row.get("kb_type").unwrap_or_else(|_| "fact".to_string());

        Ok(KnowledgeEntry {
            id: row.get("id")?,
            category: row.get("category")?,
            key: row.get("key")?,
            summary: row.get("summary")?,
            detail,
            source: row.get("source")?,
            confidence: row.get("confidence")?,
            access_count: row.get("access_count")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
            last_accessed_at: row.get("last_accessed_at")?,
            linked_task_id: row.get("linked_task_id").unwrap_or(None),
            kb_type,
            context_snippet: None,
            scope_task_id: row.get("scope_task_id").unwrap_or(None),
            utility_score: row.get("utility_score").unwrap_or(0.5),
        })
    }

    /// Infer KB type from category prefix.
    // ============ Knowledge Graph (Edges) ============

    /// Add a directed edge between two KB entries.
    pub fn kb_add_edge(&self, source_id: &str, target_id: &str, relation_type: &str, weight: f64) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR REPLACE INTO knowledge_edges (source_id, target_id, relation_type, weight, created_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now'))",
            params![source_id, target_id, relation_type, weight],
        )?;
        Ok(())
    }

    /// Get all edges involving a KB entry (as source or target).
    pub fn kb_get_edges(&self, id: &str) -> DbResult<Vec<crate::types::KBEdge>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT source_id, target_id, relation_type, weight, created_at
             FROM knowledge_edges WHERE source_id = ?1 OR target_id = ?1"
        )?;
        let rows = stmt.query_map(params![id], |row| {
            Ok(crate::types::KBEdge {
                source_id: row.get(0)?,
                target_id: row.get(1)?,
                relation_type: row.get(2)?,
                weight: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    /// Expand a set of KB IDs by following 1-hop edges.
    /// Returns neighbor IDs not already in the input set, capped at `max_extra`.
    pub fn kb_expand_related(&self, ids: &[String], max_extra: usize) -> DbResult<Vec<String>> {
        if ids.is_empty() || max_extra == 0 { return Ok(vec![]); }
        let conn = self.read_conn();
        let id_set: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        let mut neighbors: Vec<(String, f64)> = Vec::new();
        for id in ids {
            let mut stmt = conn.prepare(
                "SELECT target_id, weight FROM knowledge_edges WHERE source_id = ?1
                 UNION ALL
                 SELECT source_id, weight FROM knowledge_edges WHERE target_id = ?1"
            )?;
            let rows: Vec<(String, f64)> = stmt.query_map(params![id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?.filter_map(|r| r.ok()).collect();
            for (nid, w) in rows {
                if !id_set.contains(nid.as_str()) {
                    neighbors.push((nid, w));
                }
            }
        }
        // Deduplicate and sort by weight descending
        let mut seen = HashSet::new();
        neighbors.retain(|(id, _)| seen.insert(id.clone()));
        neighbors.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        neighbors.truncate(max_extra);
        Ok(neighbors.into_iter().map(|(id, _)| id).collect())
    }

    /// Delete all edges involving a KB entry (cleanup on forget).
    pub fn kb_delete_edges_for(&self, id: &str) -> DbResult<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM knowledge_edges WHERE source_id = ?1 OR target_id = ?1", params![id])?;
        Ok(())
    }

    // ============ Co-access Pattern Tracking ============

    /// Log a group of KB entries that were co-accessed in the same context pipeline.
    pub fn kb_log_co_access(&self, kb_ids: &[String], source: &str) -> DbResult<()> {
        if kb_ids.len() < 2 { return Ok(()); }
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "INSERT INTO kb_access_log (kb_id, co_accessed_ids, context_source) VALUES (?1, ?2, ?3)"
        )?;
        for id in kb_ids {
            let others: Vec<&str> = kb_ids.iter().filter(|x| *x != id).map(|s| s.as_str()).collect();
            let others_json = serde_json::to_string(&others).unwrap_or_default();
            stmt.execute(params![id, others_json, source])?;
        }
        Ok(())
    }

    /// Compute co-occurrence matrix from access log.
    /// Returns: KB ID → top co-accessed KB IDs (sorted by frequency).
    pub fn kb_compute_cooccurrence(&self, since_hours: i64, top_n: usize) -> DbResult<std::collections::HashMap<String, Vec<String>>> {
        let conn = self.read_conn();
        // Clean old data first
        conn.execute(
            "DELETE FROM kb_access_log WHERE created_at < datetime('now', ?1)",
            params![format!("-{} hours", since_hours * 2)], // retain 2x window
        )?;
        let mut stmt = conn.prepare(
            "SELECT kb_id, co_accessed_ids FROM kb_access_log
             WHERE created_at >= datetime('now', ?1)"
        )?;
        let cutoff = format!("-{} hours", since_hours);
        let rows: Vec<(String, String)> = stmt.query_map(params![cutoff], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?.filter_map(|r| r.ok()).collect();

        // Build frequency map: kb_id → { co_id → count }
        let mut freq: std::collections::HashMap<String, std::collections::HashMap<String, u32>> = std::collections::HashMap::new();
        for (kb_id, co_json) in &rows {
            let co_ids: Vec<String> = serde_json::from_str(co_json).unwrap_or_default();
            let entry = freq.entry(kb_id.clone()).or_default();
            for co_id in co_ids {
                *entry.entry(co_id).or_insert(0) += 1;
            }
        }

        // Rank and truncate to top_n per entry
        let mut result = std::collections::HashMap::new();
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

    // ============ Stale State Verification ============

    /// List state-type KB entries not accessed in `stale_days` days.
    pub fn kb_list_stale_state_entries(&self, stale_days: i64, limit: usize) -> DbResult<Vec<KnowledgeEntry>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM knowledge
             WHERE kb_type = 'state'
               AND (last_accessed_at IS NULL OR julianday('now') - julianday(last_accessed_at) > ?1)
               AND confidence >= 0.3
             ORDER BY last_accessed_at ASC NULLS FIRST
             LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![stale_days, limit as i64], Self::row_to_knowledge_entry)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ============ Shadow Replay ============

    /// List prompt snapshots where cited KB entries have been modified since snapshot creation.
    pub fn list_modified_snapshots(&self, limit: usize) -> DbResult<Vec<(String, String, String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT ps.task_id, ps.prompt, ps.cited_kb_ids, ps.created_at
             FROM prompt_snapshots ps
             WHERE ps.task_outcome = 'success'
               AND ps.cited_kb_ids != '[]'
               AND EXISTS (
                   SELECT 1 FROM knowledge k
                   WHERE instr(ps.cited_kb_ids, k.id) > 0
                     AND k.updated_at > ps.created_at
               )
             ORDER BY ps.created_at DESC
             LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?.filter_map(|r| r.ok()).collect();
        Ok(rows)
    }

    // ============ KB-AST Graph Links ============

    /// Add a link between a KB entry and an AST code symbol.
    /// Uses symbol_name as stable anchor; ast_node_id is a cache that may go stale.
    pub fn kb_add_ast_link(
        &self, kb_id: &str, symbol_name: &str, file_path: Option<&str>,
        ast_node_id: Option<&str>, relation: &str, confidence: f64,
    ) -> DbResult<()> {
        let conn = self.conn();
        // Upsert: avoid duplicate (kb_id, symbol_name) pairs
        conn.execute(
            "INSERT INTO kb_ast_links (kb_id, ast_node_id, symbol_name, file_path, relation, confidence)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT DO NOTHING",
            params![kb_id, ast_node_id, symbol_name, file_path, relation, confidence],
        )?;
        Ok(())
    }

    /// Get all KB entries linked to a given AST symbol name.
    pub fn kb_get_memories_for_symbol(&self, symbol_name: &str) -> DbResult<Vec<KnowledgeEntry>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT k.* FROM knowledge k
             JOIN kb_ast_links l ON l.kb_id = k.id
             WHERE l.symbol_name = ?1
             ORDER BY k.utility_score DESC, k.updated_at DESC
             LIMIT 10"
        )?;
        let rows = stmt.query_map(params![symbol_name], Self::row_to_knowledge_entry)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get all KB entries linked to symbols in a given file, grouped by symbol.
    pub fn kb_get_memories_for_file(&self, file_path: &str) -> DbResult<Vec<(String, KnowledgeEntry)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT l.symbol_name, k.id, k.category, k.key, k.summary, k.detail,
                    k.source, k.confidence, k.access_count, k.created_at, k.updated_at,
                    k.last_accessed_at, k.linked_task_id, k.kb_type, k.scope_task_id, k.utility_score
             FROM knowledge k
             JOIN kb_ast_links l ON l.kb_id = k.id
             WHERE l.file_path = ?1
             ORDER BY l.symbol_name, k.utility_score DESC
             LIMIT 30"
        )?;
        let rows = stmt.query_map(params![file_path], |row| {
            let symbol: String = row.get(0)?;
            let entry = KnowledgeEntry {
                id: row.get(1)?,
                category: row.get(2)?,
                key: row.get(3)?,
                summary: row.get(4)?,
                detail: row.get::<_, Option<String>>(5)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
                source: row.get(6)?,
                confidence: row.get(7)?,
                access_count: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
                last_accessed_at: row.get(11)?,
                linked_task_id: row.get(12)?,
                kb_type: row.get::<_, Option<String>>(13)?.unwrap_or_else(|| "fact".to_string()),
                context_snippet: None,
                scope_task_id: row.get(14)?,
                utility_score: row.get::<_, Option<f64>>(15)?.unwrap_or(0.5),
            };
            Ok((symbol, entry))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Delete all AST links for a KB entry (cleanup on forget).
    pub fn kb_delete_ast_links_for(&self, kb_id: &str) -> DbResult<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM kb_ast_links WHERE kb_id = ?1", params![kb_id])?;
        Ok(())
    }

    /// Lazy resolution: fix stale ast_node_id by re-resolving from symbol_name.
    pub fn kb_ast_lazy_resolve(&self, link_id: i64, symbol_name: &str) -> DbResult<Option<String>> {
        let conn = self.conn();
        let new_id: Option<String> = conn.query_row(
            "SELECT id FROM ast_nodes WHERE name = ?1 LIMIT 1",
            params![symbol_name],
            |row| row.get(0),
        ).optional()?;
        if let Some(ref nid) = new_id {
            conn.execute(
                "UPDATE kb_ast_links SET ast_node_id = ?1 WHERE id = ?2",
                params![nid, link_id],
            )?;
        }
        Ok(new_id)
    }

    /// rule: policy/preference/system_rule → always-apply directives
    /// fact: memory/architecture/frontend_standard → contextual knowledge
    /// goal: feature/project/design_spec → aspirational targets
    /// state: ops/debug/bugfix → current/transient status
    pub fn infer_kb_type(category: &str) -> &'static str {
        let prefix = category.split(':').next().unwrap_or(category);
        match prefix {
            "policy" | "preference" | "system_rule" | "decision" => "rule",
            "feature" | "feature_request" | "project" | "project-requirement" | "design_spec" => "goal",
            "memory" if category.contains("ops") || category.contains("debug") || category.contains("bugfix") => "state",
            "ops" => "state",
            _ => "fact", // memory:architecture, architecture, frontend_standard, etc.
        }
    }
}

/// Tokenize text for similarity comparison.
/// Chinese: character-level unigrams. English: lowercase words (len >= 2).
fn tokenize_for_similarity(text: &str) -> HashSet<String> {
    let mut tokens = HashSet::new();
    let mut ascii_word = String::new();

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            ascii_word.push(ch.to_ascii_lowercase());
        } else {
            if ascii_word.len() >= 2 {
                tokens.insert(ascii_word.clone());
            }
            ascii_word.clear();
            // CJK character → insert as unigram
            if ch as u32 > 0x2E80 {
                tokens.insert(ch.to_string());
            }
        }
    }
    if ascii_word.len() >= 2 {
        tokens.insert(ascii_word);
    }
    tokens
}

/// Token-level Jaccard similarity (supports Chinese + English mixed text)
fn token_jaccard_similarity(a: &str, b: &str) -> f64 {
    let ta = tokenize_for_similarity(a);
    let tb = tokenize_for_similarity(b);
    if ta.is_empty() && tb.is_empty() {
        return 0.0;
    }
    let intersection = ta.intersection(&tb).count();
    let union = ta.union(&tb).count();
    if union == 0 { 0.0 } else { intersection as f64 / union as f64 }
}
