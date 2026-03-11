use rusqlite::{params, Connection, OptionalExtension};
use super::error::DbResult;
use std::sync::atomic::Ordering;
use crate::types::*;
use super::{MissionDB, token_jaccard_similarity};

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
        let updated = conn.execute(
            "UPDATE knowledge SET summary = ?1, detail = ?2, source = ?3, confidence = ?4, updated_at = ?5
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
                "UPDATE knowledge SET category = ?1, summary = ?2, detail = ?3, source = ?4, confidence = ?5, updated_at = ?6
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
                "UPDATE knowledge SET summary = ?1, detail = ?2, source = ?3, confidence = ?4, updated_at = ?5
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
        conn.execute(
            "INSERT INTO knowledge (id, category, key, summary, detail, source, confidence, access_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9)",
            params![id, input.category, input.key, input.summary, detail_str, source, confidence, now, now],
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
    pub fn kb_get(&self, key: &str) -> DbResult<Option<KnowledgeEntry>> {
        let conn = self.conn(); // Need write conn for access_count update
        let mut stmt = conn.prepare(
            "SELECT * FROM knowledge WHERE key = ?1"
        )?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            let mut entry = Self::row_to_knowledge_entry(row)?;
            // Bump access count
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE knowledge SET access_count = access_count + 1, last_accessed_at = ?1 WHERE id = ?2",
                params![now, entry.id],
            )?;
            entry.access_count += 1;
            entry.last_accessed_at = Some(now);
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
    pub fn kb_search_fts_ranked(&self, query: &str, category: Option<&str>) -> DbResult<Vec<(String, usize)>> {
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
                "SELECT k.id FROM knowledge k
                 JOIN knowledge_fts f ON k.rowid = f.rowid
                 WHERE knowledge_fts MATCH ?1 AND (k.category = ?2 OR k.category LIKE ?3)
                 ORDER BY rank"
            )?;
            let rows = stmt.query_map(params![fts_query, cat, like_pattern], |row| {
                row.get::<_, String>(0)
            })?;
            for (rank, id) in rows.enumerate() {
                results.push((id?, rank));
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT k.id FROM knowledge k
                 JOIN knowledge_fts f ON k.rowid = f.rowid
                 WHERE knowledge_fts MATCH ?1
                 ORDER BY rank"
            )?;
            let rows = stmt.query_map(params![fts_query], |row| {
                row.get::<_, String>(0)
            })?;
            for (rank, id) in rows.enumerate() {
                results.push((id?, rank));
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

    /// Batch update access_count and last_accessed_at for search hits
    pub fn kb_update_access_stats(&self, entries: &[KnowledgeEntry]) -> DbResult<()> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        let mut stmt = conn.prepare(
            "UPDATE knowledge SET access_count = access_count + 1, last_accessed_at = ?1 WHERE id = ?2"
        )?;
        for entry in entries {
            stmt.execute(params![now, entry.id])?;
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

    /// Auto-GC: delete infra (duplicates servers.yaml), expired bugfix (>30d),
    /// and stale zero-access entries (>14d, non-protected categories).
    pub fn kb_auto_gc(&self) -> DbResult<usize> {
        let conn = self.conn();
        let mut deleted = 0;

        // Rule 1: infra entries — servers.yaml + mission_infra_get already covers this
        deleted += conn.execute("DELETE FROM knowledge WHERE category = 'infra'", [])?;

        // Rule 2: memory:bugfix older than 30 days
        deleted += conn.execute(
            "DELETE FROM knowledge WHERE category = 'memory:bugfix' \
             AND julianday('now') - julianday(updated_at) > 30",
            [],
        )?;

        // Rule 2b: memory:debug older than 14 days (debug process logs lose value once resolved;
        // valuable insights should be distilled into memory:bugfix or architecture by then)
        deleted += conn.execute(
            "DELETE FROM knowledge WHERE category = 'memory:debug' \
             AND julianday('now') - julianday(updated_at) > 14",
            [],
        )?;

        // Rule 3: zero access + 14 days stale + not in protected categories
        deleted += conn.execute(
            "DELETE FROM knowledge WHERE access_count = 0 \
             AND last_accessed_at IS NULL \
             AND julianday('now') - julianday(updated_at) > 14 \
             AND category NOT IN ('preference', 'memory:decision', 'memory:architecture', 'project', 'policy:decision')",
            [],
        )?;

        if deleted > 0 {
            conn.execute_batch("INSERT INTO knowledge_fts(knowledge_fts) VALUES('rebuild')")?;
            self.fts_dirty.store(false, Ordering::Relaxed);
            tracing::info!(deleted, "kb_auto_gc: cleaned up entries, FTS rebuilt");
        }

        Ok(deleted)
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
        })
    }
}
