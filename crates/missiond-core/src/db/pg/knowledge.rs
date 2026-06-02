//! KbStore — PostgreSQL implementation.

use super::PgMissionStore;
use crate::db::error::DbResult;
use crate::db::shared::{contains_sensitive_data, infer_kb_type, token_jaccard_similarity};
use crate::db::traits::{EvidenceLaneStore, KbStore};
use crate::types::*;
use async_trait::async_trait;
#[cfg(feature = "postgres")]
use sqlx::Row;
use std::collections::HashMap;

/// Helper: convert a sqlx Row into KnowledgeEntry.
fn row_to_knowledge_entry(
    id: String,
    category: String,
    key: String,
    summary: String,
    detail: Option<String>,
    source: String,
    confidence: f64,
    access_count: i64,
    created_at: String,
    updated_at: String,
    last_accessed_at: Option<String>,
    linked_task_id: Option<String>,
    kb_type: Option<String>,
    scope_task_id: Option<String>,
    utility_score: Option<f64>,
    project_id: Option<String>,
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
        project_id,
    }
}

/// Tuple type matching the SELECT * columns we fetch from knowledge table.
type KBRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    f64,
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<f64>,
    Option<String>,
);

fn kb_row_to_entry(r: KBRow) -> KnowledgeEntry {
    row_to_knowledge_entry(
        r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9, r.10, r.11, r.12, r.13, r.14, r.15,
    )
}

/// Utility hit boost constant (same as MissionDB::UTILITY_HIT_BOOST).
const UTILITY_HIT_BOOST: f64 = 0.15;
const FUZZY_MERGE_THRESHOLD: f64 = 0.5;
const SAME_SESSION_FUZZY_MERGE_THRESHOLD: f64 = 0.35;

fn json_array_values(value: Option<&serde_json::Value>) -> Vec<serde_json::Value> {
    match value {
        Some(serde_json::Value::Array(values)) => values.clone(),
        Some(value) if !value.is_null() => vec![value.clone()],
        _ => Vec::new(),
    }
}

fn push_unique_json(values: &mut Vec<serde_json::Value>, value: serde_json::Value) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn source_session_from_detail(detail: Option<&serde_json::Value>) -> Option<String> {
    let obj = detail?.as_object()?;
    [
        "source_session",
        "sourceSession",
        "source_sessions",
        "sourceSessions",
    ]
    .iter()
    .find_map(|key| {
        obj.get(*key).and_then(|value| match value {
            serde_json::Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
            serde_json::Value::Array(values) => values.iter().find_map(|item| {
                item.as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
            }),
            _ => None,
        })
    })
}

fn same_source_session(
    left: Option<&serde_json::Value>,
    right: Option<&serde_json::Value>,
) -> bool {
    match (
        source_session_from_detail(left),
        source_session_from_detail(right),
    ) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

fn merge_detail_for_dedupe(
    existing: Option<serde_json::Value>,
    incoming: Option<serde_json::Value>,
    incoming_key: &str,
    incoming_source: &str,
    merged_at: &str,
    similarity: f64,
) -> Option<serde_json::Value> {
    let mut base = match existing {
        Some(serde_json::Value::Object(obj)) => obj,
        Some(other) => {
            let mut obj = serde_json::Map::new();
            obj.insert("previous_detail".to_string(), other);
            obj
        }
        None => serde_json::Map::new(),
    };

    if let Some(serde_json::Value::Object(incoming_obj)) = incoming {
        for (key, value) in incoming_obj {
            if matches!(
                key.as_str(),
                "evidence_refs"
                    | "source_sessions"
                    | "sourceSessions"
                    | "consolidated_from"
                    | "superseded_by"
                    | "supersededBy"
            ) {
                let mut merged = json_array_values(base.get(&key));
                for item in json_array_values(Some(&value)) {
                    push_unique_json(&mut merged, item);
                }
                base.insert(key, serde_json::Value::Array(merged));
            } else {
                base.entry(key).or_insert(value);
            }
        }
    }

    let mut merge_events = json_array_values(base.get("_dedupe_merge_events"));
    push_unique_json(
        &mut merge_events,
        serde_json::json!({
            "incoming_key": incoming_key,
            "incoming_source": incoming_source,
            "merged_at": merged_at,
            "similarity": similarity,
            "gate": "kb_remember.shared_dedupe_gate"
        }),
    );
    base.insert(
        "_dedupe_merge_events".to_string(),
        serde_json::Value::Array(merge_events),
    );

    Some(serde_json::Value::Object(base))
}

/// The common SELECT column list for knowledge entries.
const KB_COLS: &str = "id, category, key, summary, detail, source, confidence, access_count, created_at, updated_at, last_accessed_at, linked_task_id, kb_type, scope_task_id, utility_score, project_id";

type KnowledgeReviewRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    serde_json::Value,
    Option<String>,
    f64,
    String,
    Option<String>,
    bool,
);

fn review_row_to_state(r: KnowledgeReviewRow) -> KnowledgeReviewState {
    KnowledgeReviewState {
        id: r.0,
        knowledge_id: r.1,
        state: r.2,
        batch_id: r.3,
        reviewer: r.4,
        rationale: r.5,
        evidence_refs: r.6,
        superseded_by: r.7,
        confidence: r.8,
        reviewed_at: r.9,
        applied_at: r.10,
        is_current: r.11,
    }
}

#[cfg(feature = "postgres")]
fn evidence_item_from_row(row: &sqlx::postgres::PgRow) -> Result<EvidenceItemInput, sqlx::Error> {
    Ok(EvidenceItemInput {
        id: row.try_get("id")?,
        lane_id: row.try_get("lane_id")?,
        source_type: row.try_get("source_type")?,
        source_id: row.try_get("source_id")?,
        source_ref: row.try_get("source_ref")?,
        project_id: row.try_get("project_id")?,
        task_id: row.try_get("task_id")?,
        title: row.try_get("title")?,
        summary: row.try_get("summary")?,
        authority_class: row.try_get("authority_class")?,
        validity: row.try_get("validity")?,
        privacy_class: row.try_get("privacy_class")?,
        freshness: row.try_get("freshness")?,
        score: row.try_get("score")?,
        raw_policy: row.try_get("raw_policy")?,
        evidence_refs: row.try_get("evidence_refs")?,
        metadata: row.try_get("metadata")?,
    })
}

fn is_skill_evidence_projection(item: &EvidenceItemInput) -> bool {
    item.lane_id == "skill_evidence" || item.source_type == "skill_credential_ref"
}

fn skill_evidence_item_type(source_type: &str) -> &'static str {
    match source_type {
        "skill_procedure" => "procedure",
        "skill_operational_fact" | "infra_evidence" => "operational_fact",
        "skill_warning" => "warning",
        "skill_credential_ref" => "credential_ref",
        _ => "metadata",
    }
}

fn json_string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn json_i32_field(value: &serde_json::Value, keys: &[&str]) -> Option<i32> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(serde_json::Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
    })
}

#[cfg(feature = "postgres")]
async fn upsert_skill_evidence_item_projection(
    pool: &sqlx::PgPool,
    item: &EvidenceItemInput,
) -> DbResult<()> {
    let item_type = skill_evidence_item_type(item.source_type.as_str());
    let skill = item
        .source_id
        .clone()
        .or_else(|| json_string_field(&item.metadata, &["skill", "source_skill", "sourceSkill"]))
        .unwrap_or_else(|| item.title.clone());
    let source_path = item
        .source_ref
        .clone()
        .or_else(|| json_string_field(&item.metadata, &["source_path", "sourcePath", "path"]))
        .unwrap_or_else(|| item.source_type.clone());
    let source_line = json_i32_field(&item.metadata, &["source_line", "sourceLine"]);
    let service_id = json_string_field(&item.metadata, &["service_id", "serviceId"]);
    let domain = json_string_field(&item.metadata, &["domain"]);
    let secret_ref = if item_type == "credential_ref" {
        item.source_id
            .clone()
            .or_else(|| json_string_field(&item.metadata, &["secret_ref", "secretRef"]))
    } else {
        None
    };
    let credential_inline_risk = item
        .metadata
        .get("credential_inline_risk")
        .or_else(|| item.metadata.get("credentialInlineRisk"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let confidence = item.score.unwrap_or(0.5).clamp(0.0, 1.0);
    let metadata = serde_json::json!({
        "evidence_item_id": item.id,
        "source_type": item.source_type,
        "raw_policy": item.raw_policy,
        "projection": "evidence_items.to_skill_evidence_items",
        "metadata": item.metadata,
    });

    sqlx::query(
        "INSERT INTO skill_evidence_items (
            id, skill, item_type, project_id, service_id, domain,
            source_path, source_line, title, summary, validity, confidence,
            secret_ref, credential_inline_risk, evidence_refs, metadata
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
         ON CONFLICT (skill, item_type, source_path, source_line, title) DO UPDATE SET
            project_id = EXCLUDED.project_id,
            service_id = EXCLUDED.service_id,
            domain = EXCLUDED.domain,
            summary = EXCLUDED.summary,
            validity = EXCLUDED.validity,
            confidence = EXCLUDED.confidence,
            secret_ref = EXCLUDED.secret_ref,
            credential_inline_risk = EXCLUDED.credential_inline_risk,
            evidence_refs = EXCLUDED.evidence_refs,
            metadata = EXCLUDED.metadata,
            updated_at = now()",
    )
    .bind(&item.id)
    .bind(skill)
    .bind(item_type)
    .bind(item.project_id.as_deref())
    .bind(service_id.as_deref())
    .bind(domain.as_deref())
    .bind(source_path)
    .bind(source_line)
    .bind(&item.title)
    .bind(&item.summary)
    .bind(&item.validity)
    .bind(confidence)
    .bind(secret_ref.as_deref())
    .bind(credential_inline_risk)
    .bind(&item.evidence_refs)
    .bind(&metadata)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(feature = "postgres")]
#[async_trait]
impl KbStore for PgMissionStore {
    // ========== Core CRUD ==========

    async fn kb_remember(&self, input: &KBRememberInput) -> DbResult<KBRememberResult> {
        let now = chrono::Utc::now().to_rfc3339();
        let source = input.source.as_deref().unwrap_or("conversation");
        let mut confidence = input.confidence.unwrap_or(1.0);
        let detail_str = input
            .detail
            .as_ref()
            .map(|d| serde_json::to_string(d).unwrap_or_default());

        // Guard: reject infra category
        if input.category == "infra" {
            return Ok(KBRememberResult {
                entry: KnowledgeEntry {
                    id: String::new(),
                    category: input.category.clone(),
                    key: input.key.clone(),
                    summary: "REJECTED: infra entries should use servers.yaml + mission_infra_get"
                        .into(),
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
                    project_id: None,
                },
                action: "rejected".into(),
                merged_key: None,
                similarity: None,
            });
        }

        // Sensitive data detection
        let check_text = format!(
            "{} {} {}",
            input.summary,
            detail_str.as_deref().unwrap_or(""),
            input.key
        );
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
            let entry = self
                .pg_get_by_category_key(&input.category, &input.key)
                .await?;
            return Ok(KBRememberResult {
                entry: entry.unwrap(),
                action: "updated".to_string(),
                merged_key: None,
                similarity: None,
            });
        }

        // 1b. Same key, different category → re-categorize
        let existing_by_key: Option<KBRow> =
            sqlx::query_as(&format!("SELECT {} FROM knowledge WHERE key = $1", KB_COLS))
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
            let entry = self
                .pg_get_by_category_key(&input.category, &input.key)
                .await?;
            return Ok(KBRememberResult {
                entry: entry.unwrap(),
                action: "updated".to_string(),
                merged_key: None,
                similarity: None,
            });
        }

        // 2. Shared dedupe gate: check for similar entries in same category.
        // The gate is intentionally in the core store so realtime extraction,
        // deep-analysis, manual MCP writes, and internal learning workers share
        // one merge path instead of racing two active KB keys for one topic.
        let candidates = self.pg_list_by_category(&input.category).await?;
        let new_text = format!("{} {}", input.key, input.summary);
        let mut best: Option<(f64, KnowledgeEntry)> = None;
        for entry in candidates {
            let existing_text = format!("{} {}", entry.key, entry.summary);
            let sim = token_jaccard_similarity(&new_text, &existing_text);
            let same_session = same_source_session(input.detail.as_ref(), entry.detail.as_ref());
            let threshold = if same_session {
                SAME_SESSION_FUZZY_MERGE_THRESHOLD
            } else {
                FUZZY_MERGE_THRESHOLD
            };
            if sim >= threshold {
                match &best {
                    None => best = Some((sim, entry)),
                    Some((best_sim, _)) if sim > *best_sim => best = Some((sim, entry)),
                    _ => {}
                }
            }
        }

        if let Some((sim, existing)) = best {
            let merged_detail = merge_detail_for_dedupe(
                existing.detail.clone(),
                input.detail.clone(),
                &input.key,
                source,
                &now,
                sim,
            );
            let merged_detail_str = merged_detail
                .as_ref()
                .map(|d| serde_json::to_string(d).unwrap_or_default());
            let merged_confidence = existing.confidence.max(confidence);
            sqlx::query(
                "UPDATE knowledge SET summary = $1, detail = $2, source = $3, confidence = $4, updated_at = $5,
                 utility_score = GREATEST(utility_score, 0.8)
                 WHERE id = $6"
            )
            .bind(&input.summary)
            .bind(&merged_detail_str)
            .bind(source)
            .bind(merged_confidence)
            .bind(&now)
            .bind(&existing.id)
            .execute(&self.pool)
            .await?;
            let merged_key = existing.key.clone();
            let entry = self
                .pg_get_by_category_key(&existing.category, &existing.key)
                .await?;
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
            "INSERT INTO knowledge (id, category, key, summary, detail, source, confidence, access_count, created_at, updated_at, kb_type, project_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, 0, $8, $9, $10, $11)
             ON CONFLICT (category, key) DO UPDATE SET
                summary = EXCLUDED.summary, detail = EXCLUDED.detail,
                source = EXCLUDED.source, confidence = EXCLUDED.confidence,
                updated_at = EXCLUDED.updated_at,
                utility_score = GREATEST(knowledge.utility_score, 0.8),
                project_id = COALESCE(EXCLUDED.project_id, knowledge.project_id)"
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
        .bind(&input.project_id)
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
            project_id: input.project_id.clone(),
        };

        Ok(KBRememberResult {
            entry,
            action: "created".to_string(),
            merged_key: None,
            similarity: None,
        })
    }

    async fn kb_get(&self, key: &str) -> DbResult<Option<KnowledgeEntry>> {
        let row: Option<KBRow> =
            sqlx::query_as(&format!("SELECT {} FROM knowledge WHERE key = $1", KB_COLS))
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        match row {
            Some(r) => {
                let mut entry = kb_row_to_entry(r);
                // Bump access count + utility score
                let now = chrono::Utc::now().to_rfc3339();
                let new_utility = (entry.utility_score
                    + UTILITY_HIT_BOOST * (1.0 - entry.utility_score))
                    .min(1.0);
                sqlx::query(
                    "UPDATE knowledge SET access_count = access_count + 1, last_accessed_at = $1,
                     utility_score = $3 WHERE id = $2",
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
        let row: Option<KBRow> =
            sqlx::query_as(&format!("SELECT {} FROM knowledge WHERE id = $1", KB_COLS))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(kb_row_to_entry))
    }

    async fn kb_get_id_by_key(&self, key: &str) -> DbResult<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as("SELECT id FROM knowledge WHERE key = $1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.0))
    }

    async fn kb_update(
        &self,
        key: &str,
        new_category: Option<&str>,
        new_summary: Option<&str>,
        new_detail: Option<&serde_json::Value>,
        new_confidence: Option<f64>,
        new_linked_task_id: Option<&str>,
        new_project_id: Option<&str>,
    ) -> DbResult<Option<(KnowledgeEntry, bool)>> {
        // Find existing entry
        let existing_row: Option<KBRow> =
            sqlx::query_as(&format!("SELECT {} FROM knowledge WHERE key = $1", KB_COLS))
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

        if new_category.is_some() {
            sets.push(format!("category = ${}", param_idx));
            param_idx += 1;
        }
        if new_summary.is_some() {
            sets.push(format!("summary = ${}", param_idx));
            param_idx += 1;
            content_changed = true;
        }
        if detail_str.is_some() {
            sets.push(format!("detail = ${}", param_idx));
            param_idx += 1;
            content_changed = true;
        }
        if new_confidence.is_some() {
            sets.push(format!("confidence = ${}", param_idx));
            param_idx += 1;
        }
        if new_linked_task_id.is_some() {
            sets.push(format!("linked_task_id = ${}", param_idx));
            param_idx += 1;
        }
        if new_project_id.is_some() {
            sets.push(format!("project_id = ${}", param_idx));
            param_idx += 1;
        }

        // Only updated_at — nothing else to change
        if param_idx == 2 {
            return Ok(Some((existing, false)));
        }

        let sql = format!(
            "UPDATE knowledge SET {} WHERE id = ${}",
            sets.join(", "),
            param_idx
        );

        // We need to use a raw query with dynamic binds
        // Build the query dynamically
        let mut query = sqlx::query(&sql);
        query = query.bind(&now);
        if let Some(v) = new_category {
            query = query.bind(v.to_string());
        }
        if let Some(v) = new_summary {
            query = query.bind(v.to_string());
        }
        if let Some(v) = &detail_str {
            query = query.bind(v.clone());
        }
        if let Some(v) = new_confidence {
            query = query.bind(v);
        }
        if let Some(v) = new_linked_task_id {
            let val: Option<String> = if v.is_empty() {
                None
            } else {
                Some(v.to_string())
            };
            query = query.bind(val);
        }
        if let Some(v) = new_project_id {
            let val: Option<String> = if v.is_empty() {
                None
            } else {
                Some(v.to_string())
            };
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
        let result = sqlx::query("UPDATE knowledge SET linked_task_id = $1 WHERE key = $2")
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
                "SELECT {} FROM knowledge ORDER BY category, updated_at DESC",
                KB_COLS
            ))
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows.into_iter().map(kb_row_to_entry).collect())
    }

    async fn kb_list_paginated(
        &self,
        category: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> DbResult<Vec<KnowledgeEntry>> {
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
                "SELECT {} FROM knowledge ORDER BY category, updated_at DESC LIMIT $1 OFFSET $2",
                KB_COLS
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
            "SELECT {} FROM knowledge WHERE scope_task_id = $1",
            KB_COLS
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

    async fn kb_review_upsert(
        &self,
        input: &KnowledgeReviewInput,
    ) -> DbResult<KnowledgeReviewState> {
        let now = chrono::Utc::now().to_rfc3339();
        let id = uuid::Uuid::new_v4().to_string();
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "UPDATE knowledge_review_state
             SET is_current = FALSE
             WHERE knowledge_id = $1 AND is_current = TRUE",
        )
        .bind(&input.knowledge_id)
        .execute(&mut *tx)
        .await?;

        let row: KnowledgeReviewRow = sqlx::query_as(
            "INSERT INTO knowledge_review_state
                (id, knowledge_id, state, batch_id, reviewer, rationale, evidence_refs,
                 superseded_by, confidence, reviewed_at, applied_at, is_current)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, TRUE)
             RETURNING id, knowledge_id, state, batch_id, reviewer, rationale, evidence_refs,
                 superseded_by, confidence, reviewed_at, applied_at, is_current",
        )
        .bind(&id)
        .bind(&input.knowledge_id)
        .bind(&input.state)
        .bind(&input.batch_id)
        .bind(&input.reviewer)
        .bind(&input.rationale)
        .bind(&input.evidence_refs)
        .bind(&input.superseded_by)
        .bind(input.confidence)
        .bind(&now)
        .bind(&input.applied_at)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(review_row_to_state(row))
    }

    async fn kb_review_current_for_ids(
        &self,
        ids: &[String],
    ) -> DbResult<HashMap<String, KnowledgeReviewState>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }

        let rows: Vec<KnowledgeReviewRow> = sqlx::query_as(
            "SELECT id, knowledge_id, state, batch_id, reviewer, rationale, evidence_refs,
                    superseded_by, confidence, reviewed_at, applied_at, is_current
             FROM knowledge_review_state
             WHERE is_current = TRUE AND knowledge_id = ANY($1)",
        )
        .bind(ids.to_vec())
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(review_row_to_state)
            .map(|state| (state.knowledge_id.clone(), state))
            .collect())
    }

    async fn kb_review_get_by_key(&self, key: &str) -> DbResult<Option<KnowledgeReviewState>> {
        let row: Option<KnowledgeReviewRow> = sqlx::query_as(
            "SELECT r.id, r.knowledge_id, r.state, r.batch_id, r.reviewer, r.rationale, r.evidence_refs,
                    r.superseded_by, r.confidence, r.reviewed_at, r.applied_at, r.is_current
             FROM knowledge_review_state r
             JOIN knowledge k ON k.id = r.knowledge_id
             WHERE r.is_current = TRUE AND k.key = $1"
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(review_row_to_state))
    }

    async fn kb_review_stats(&self) -> DbResult<serde_json::Value> {
        let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM knowledge")
            .fetch_one(&self.pool)
            .await?;
        let reviewed: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM knowledge_review_state WHERE is_current = TRUE")
                .fetch_one(&self.pool)
                .await?;
        let memory_total: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM knowledge WHERE category = 'memory' OR category LIKE 'memory:%'",
        )
        .fetch_one(&self.pool)
        .await?;
        let by_state: Vec<(String, i64)> = sqlx::query_as(
            "SELECT state, COUNT(*)
             FROM knowledge_review_state
             WHERE is_current = TRUE
             GROUP BY state
             ORDER BY state",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(serde_json::json!({
            "knowledge_total": total.0,
            "memory_total": memory_total.0,
            "reviewed_current": reviewed.0,
            "active_target_10pct_memory": (memory_total.0 as f64 * 0.10).round() as i64,
            "states": by_state
                .into_iter()
                .map(|(state, count)| serde_json::json!({"state": state, "count": count}))
                .collect::<Vec<_>>(),
        }))
    }

    // ========== Search ==========

    async fn kb_search(
        &self,
        query: &str,
        category: Option<&str>,
    ) -> DbResult<Vec<KnowledgeEntry>> {
        // Phase 1: FTS search using tsvector
        let results = self.pg_search_fts(query, category).await?;
        if !results.is_empty() {
            return Ok(results);
        }
        // Phase 2: LIKE fallback for Chinese and partial matches
        self.pg_search_like(query, category).await
    }

    async fn kb_search_ranked(
        &self,
        query: &str,
        category: Option<&str>,
        limit: usize,
    ) -> DbResult<Vec<(KnowledgeEntry, usize)>> {
        let results = self.kb_search(query, category).await?;
        Ok(results
            .into_iter()
            .take(limit)
            .enumerate()
            .map(|(i, e)| (e, i))
            .collect())
    }

    async fn kb_search_fts_ranked(
        &self,
        query: &str,
        category: Option<&str>,
    ) -> DbResult<Vec<(String, usize, Option<String>)>> {
        let tsquery = query
            .split_whitespace()
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

        Ok(rows
            .into_iter()
            .enumerate()
            .map(|(rank, (id, snippet))| {
                let snip = snippet.filter(|s| s.contains("**"));
                (id, rank, snip)
            })
            .collect())
    }

    async fn kb_search_like_ranked(
        &self,
        query: &str,
        category: Option<&str>,
    ) -> DbResult<Vec<(String, usize)>> {
        let keywords: Vec<String> = {
            let mut kw: Vec<String> = query.split_whitespace().map(|w| w.to_string()).collect();
            let trimmed = query.trim();
            if !trimmed.is_empty()
                && trimmed.chars().any(|c| !c.is_ascii())
                && !kw.contains(&trimmed.to_string())
            {
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
            sql.push_str(&format!(
                " AND (category = ${} OR category LIKE ${})",
                p_cat, p_like
            ));
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
        Ok(rows
            .into_iter()
            .enumerate()
            .map(|(rank, (id,))| (id, rank))
            .collect())
    }

    async fn kb_search_fts_ranked_scoped(
        &self,
        query: &str,
        category: Option<&str>,
        project_id: Option<&str>,
    ) -> DbResult<Vec<(String, usize, Option<String>)>> {
        if project_id.is_none() {
            return self.kb_search_fts_ranked(query, category).await;
        }
        let tsquery = query
            .split_whitespace()
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

        Ok(rows
            .into_iter()
            .enumerate()
            .map(|(rank, (id, snippet))| {
                let snip = snippet.filter(|s| s.contains("**"));
                (id, rank, snip)
            })
            .collect())
    }

    async fn kb_search_like_ranked_scoped(
        &self,
        query: &str,
        category: Option<&str>,
        project_id: Option<&str>,
    ) -> DbResult<Vec<(String, usize)>> {
        if project_id.is_none() {
            return self.kb_search_like_ranked(query, category).await;
        }
        let keywords: Vec<String> = {
            let mut kw: Vec<String> = query.split_whitespace().map(|w| w.to_string()).collect();
            let trimmed = query.trim();
            if !trimmed.is_empty()
                && trimmed.chars().any(|c| !c.is_ascii())
                && !kw.contains(&trimmed.to_string())
            {
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
            sql.push_str(&format!(
                " AND (category = ${} OR category LIKE ${})",
                next_param,
                next_param + 1
            ));
            next_param += 2;
        }
        sql.push_str(&format!(
            " AND (project_id = ${} OR project_id IS NULL)",
            next_param
        ));
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
        Ok(rows
            .into_iter()
            .enumerate()
            .map(|(rank, (id,))| (id, rank))
            .collect())
    }

    // ========== Embeddings ==========

    async fn kb_set_embedding(&self, id: &str, embedding: &[f32], provider: &str) -> DbResult<()> {
        let bytes = crate::embedding::f32_vec_to_bytes(embedding);
        // Format as PostgreSQL vector literal: [0.1,0.2,...]
        let vec_str = format!(
            "[{}]",
            embedding
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
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
             WHERE (category = $1 OR category LIKE $2) AND embedding IS NOT NULL",
        )
        .bind(category)
        .bind(like_pattern)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, blob)| (id, crate::embedding::bytes_to_f32_vec(&blob)))
            .collect())
    }

    async fn kb_load_all_embeddings(&self) -> DbResult<Vec<(String, Vec<f32>)>> {
        let rows: Vec<(String, Vec<u8>)> =
            sqlx::query_as("SELECT id, embedding FROM knowledge WHERE embedding IS NOT NULL")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|(id, blob)| (id, crate::embedding::bytes_to_f32_vec(&blob)))
            .collect())
    }

    async fn kb_entries_missing_embedding(
        &self,
        category: Option<&str>,
    ) -> DbResult<Vec<(String, String, String)>> {
        if let Some(cat) = category {
            let like_pattern = format!("{}:%", cat);
            let rows: Vec<(String, String, String)> = sqlx::query_as(
                "SELECT id, summary, COALESCE(detail, '') FROM knowledge
                 WHERE (category = $1 OR category LIKE $2) AND embedding IS NULL",
            )
            .bind(cat)
            .bind(like_pattern)
            .fetch_all(&self.pool)
            .await?;
            Ok(rows)
        } else {
            let rows: Vec<(String, String, String)> = sqlx::query_as(
                "SELECT id, summary, COALESCE(detail, '') FROM knowledge WHERE embedding IS NULL",
            )
            .fetch_all(&self.pool)
            .await?;
            Ok(rows)
        }
    }

    async fn kb_entries_stale_embedding(
        &self,
        current_provider: &str,
        limit: i64,
    ) -> DbResult<Vec<(String, String, String)>> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT id, summary, COALESCE(detail, '') FROM knowledge
             WHERE embedding IS NOT NULL AND embedding_provider IS NOT NULL
               AND embedding_provider != $1
             LIMIT $2",
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
            .fetch_one(&self.pool)
            .await?;
        let (never_accessed,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM knowledge WHERE access_count = 0 AND last_accessed_at IS NULL",
        )
        .fetch_one(&self.pool)
        .await?;

        let (utility_high,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM knowledge WHERE utility_score >= 0.7")
                .fetch_one(&self.pool)
                .await
                .unwrap_or((0,));
        let (utility_medium,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM knowledge WHERE utility_score >= 0.3 AND utility_score < 0.7",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or((0,));
        let (utility_low,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM knowledge WHERE utility_score < 0.3")
                .fetch_one(&self.pool)
                .await
                .unwrap_or((0,));

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
            "SELECT category, key, access_count FROM knowledge ORDER BY access_count DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        let oldest: Option<(String, String, String)> = sqlx::query_as(
            "SELECT category, key, updated_at FROM knowledge ORDER BY updated_at ASC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;

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
            stats["mostAccessed"] =
                serde_json::json!({"category": cat, "key": key, "accessCount": count});
        }
        if let Some((cat, key, updated)) = oldest {
            stats["oldest"] =
                serde_json::json!({"category": cat, "key": key, "updatedAt": updated});
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
            "SELECT category, COUNT(*) as cnt FROM knowledge GROUP BY category ORDER BY cnt DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn kb_hot_keys(&self, limit: i64) -> DbResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT key FROM knowledge ORDER BY access_count DESC, updated_at DESC LIMIT $1",
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
             ORDER BY updated_at ASC",
            KB_COLS
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
            .fetch_one(&self.pool)
            .await?;
        let (kb_embedded,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM knowledge WHERE embedding IS NOT NULL")
                .fetch_one(&self.pool)
                .await?;
        let kb_providers: Vec<(String, i64)> = sqlx::query_as(
            "SELECT COALESCE(embedding_provider, 'none'), COUNT(*) FROM knowledge GROUP BY embedding_provider ORDER BY COUNT(*) DESC"
        ).fetch_all(&self.pool).await?;

        let (skill_total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM skill_topics")
            .fetch_one(&self.pool)
            .await?;
        let (skill_embedded,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM skill_topics WHERE embedding IS NOT NULL")
                .fetch_one(&self.pool)
                .await?;

        let (conv_total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM conversations")
            .fetch_one(&self.pool)
            .await?;
        let (conv_summarized,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM conversations WHERE llm_summary IS NOT NULL")
                .fetch_one(&self.pool)
                .await?;
        let (conv_embedded,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM conversations WHERE embedding IS NOT NULL")
                .fetch_one(&self.pool)
                .await?;
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
            .execute(&self.pool)
            .await?;
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
                 OR target_id NOT IN (SELECT id FROM knowledge)",
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
        let row: Option<(f64,)> = sqlx::query_as("SELECT confidence FROM knowledge WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.0))
    }

    async fn kb_batch_apply_utility_feedback(
        &self,
        kb_ids: &[String],
        success: bool,
    ) -> DbResult<usize> {
        if kb_ids.is_empty() {
            return Ok(0);
        }
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
                     updated_at = $1 WHERE id = $2",
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

    async fn kb_list_low_confidence(
        &self,
        threshold: f64,
        limit: usize,
    ) -> DbResult<Vec<KnowledgeEntry>> {
        let rows: Vec<KBRow> = sqlx::query_as(&format!(
            "SELECT {} FROM knowledge WHERE confidence < $1 ORDER BY confidence ASC LIMIT $2",
            KB_COLS
        ))
        .bind(threshold)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(kb_row_to_entry).collect())
    }

    async fn kb_list_low_utility(
        &self,
        threshold: f64,
        min_access: i64,
        limit: usize,
    ) -> DbResult<Vec<KnowledgeEntry>> {
        let rows: Vec<KBRow> = sqlx::query_as(&format!(
            "SELECT {} FROM knowledge WHERE utility_score < $1 AND access_count >= $2
             ORDER BY utility_score ASC LIMIT $3",
            KB_COLS
        ))
        .bind(threshold)
        .bind(min_access)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(kb_row_to_entry).collect())
    }

    async fn kb_mark_needs_re_extraction(&self, ids: &[String]) -> DbResult<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let now = chrono::Utc::now().to_rfc3339();
        let mut count = 0u64;
        for id in ids {
            let r = sqlx::query(
                "UPDATE knowledge SET needs_re_extraction = 1, updated_at = $1 WHERE id = $2",
            )
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
            count += r.rows_affected();
        }
        Ok(count as usize)
    }

    async fn kb_list_stale_state_entries(
        &self,
        stale_days: i64,
        limit: usize,
    ) -> DbResult<Vec<KnowledgeEntry>> {
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

    async fn kb_ops_save_plan(
        &self,
        plan_id: &str,
        task_id: Option<&str>,
        operations: &[KBOperation],
    ) -> DbResult<usize> {
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

    async fn kb_ops_list(
        &self,
        plan_id: Option<&str>,
        status: Option<&str>,
    ) -> DbResult<Vec<KBOperationRow>> {
        let base = "SELECT id, plan_id, task_id, operation, target_keys, rationale, status, priority, result, created_at, executed_at, error FROM kb_operation_queue";
        type OpRow = (
            String,
            String,
            Option<String>,
            String,
            String,
            Option<String>,
            String,
            i32,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
        );
        let rows: Vec<OpRow> = match (plan_id, status) {
            (Some(pid), Some(s)) => {
                sqlx::query_as(&format!(
                    "{} WHERE plan_id = $1 AND status = $2 ORDER BY priority",
                    base
                ))
                .bind(pid)
                .bind(s)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(pid), None) => {
                sqlx::query_as(&format!("{} WHERE plan_id = $1 ORDER BY priority", base))
                    .bind(pid)
                    .fetch_all(&self.pool)
                    .await?
            }
            (None, Some(s)) => {
                sqlx::query_as(&format!(
                    "{} WHERE status = $1 ORDER BY created_at DESC, priority",
                    base
                ))
                .bind(s)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query_as(&format!("{} ORDER BY created_at DESC, priority", base))
                    .fetch_all(&self.pool)
                    .await?
            }
        };
        Ok(rows
            .into_iter()
            .map(|r| KBOperationRow {
                id: r.0,
                plan_id: r.1,
                task_id: r.2,
                operation: r.3,
                target_keys: r.4,
                rationale: r.5,
                status: r.6,
                priority: r.7,
                result: r.8,
                created_at: r.9,
                executed_at: r.10,
                error: r.11,
            })
            .collect())
    }

    async fn kb_ops_update_status(
        &self,
        op_id: &str,
        status: &str,
        result: Option<&str>,
        error: Option<&str>,
    ) -> DbResult<bool> {
        let now = chrono::Utc::now().to_rfc3339();
        let executed_at: Option<&str> = if status == "done" || status == "failed" {
            Some(&now)
        } else {
            None
        };
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

    async fn kb_ops_complete_by_task_id(
        &self,
        task_id: &str,
        new_status: &str,
        result: Option<&str>,
    ) -> DbResult<bool> {
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
             WHERE status = 'pending' AND created_at < $2",
        )
        .bind(&now)
        .bind(&cutoff)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() as usize)
    }

    async fn kb_ops_plan_summary(&self, plan_id: &str) -> DbResult<serde_json::Value> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT status, COUNT(*) FROM kb_operation_queue WHERE plan_id = $1 GROUP BY status",
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

    async fn kb_add_edge(
        &self,
        source_id: &str,
        target_id: &str,
        relation_type: &str,
        weight: f64,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO knowledge_edges (source_id, target_id, relation_type, weight, created_at)
             VALUES ($1, $2, $3, $4, NOW())
             ON CONFLICT (source_id, target_id, relation_type) DO UPDATE SET
                weight = EXCLUDED.weight, created_at = EXCLUDED.created_at",
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
             FROM knowledge_edges WHERE source_id = $1 OR target_id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| KBEdge {
                source_id: r.0,
                target_id: r.1,
                relation_type: r.2,
                weight: r.3,
                created_at: r.4,
            })
            .collect())
    }

    async fn kb_expand_related(&self, ids: &[String], max_extra: usize) -> DbResult<Vec<String>> {
        if ids.is_empty() || max_extra == 0 {
            return Ok(vec![]);
        }
        let id_set: std::collections::HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        let mut neighbors: Vec<(String, f64)> = Vec::new();
        for id in ids {
            let rows: Vec<(String, f64)> = sqlx::query_as(
                "SELECT target_id, weight FROM knowledge_edges WHERE source_id = $1
                 UNION ALL
                 SELECT source_id, weight FROM knowledge_edges WHERE target_id = $1",
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

    async fn kb_log_co_access(
        &self,
        kb_ids: &[String],
        source: &str,
        session_id: Option<&str>,
    ) -> DbResult<()> {
        if kb_ids.len() < 2 {
            return Ok(());
        }
        for id in kb_ids {
            let others: Vec<&str> = kb_ids
                .iter()
                .filter(|x| *x != id)
                .map(|s| s.as_str())
                .collect();
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
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT DISTINCT kb_id FROM kb_access_log WHERE session_id = $1")
                .bind(session_id)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn kb_compute_cooccurrence(
        &self,
        since_hours: i64,
        top_n: usize,
    ) -> DbResult<HashMap<String, Vec<String>>> {
        // Clean old data first (created_at is TEXT, cast to timestamp for comparison)
        let cleanup_window = format!("-{} hours", since_hours * 2);
        sqlx::query("DELETE FROM kb_access_log WHERE created_at::timestamp < NOW() + $1::interval")
            .bind(&cleanup_window)
            .execute(&self.pool)
            .await?;

        let cutoff = format!("-{} hours", since_hours);
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT kb_id, co_accessed_ids FROM kb_access_log
             WHERE created_at::timestamp >= NOW() + $1::interval",
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

    async fn save_prompt_snapshot(
        &self,
        task_id: &str,
        prompt: &str,
        cited_kb_ids: &[String],
        category: &str,
    ) -> DbResult<()> {
        let kb_ids_json = serde_json::to_string(cited_kb_ids).unwrap_or_else(|_| "[]".to_string());
        sqlx::query(
            "INSERT INTO prompt_snapshots (task_id, prompt, cited_kb_ids, category, created_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (task_id) DO UPDATE SET
                prompt = EXCLUDED.prompt, cited_kb_ids = EXCLUDED.cited_kb_ids,
                category = EXCLUDED.category, created_at = EXCLUDED.created_at",
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

    async fn list_prompt_snapshots(
        &self,
        category: &str,
        limit: usize,
    ) -> DbResult<Vec<(String, String, String)>> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT task_id, prompt, task_outcome FROM prompt_snapshots
             WHERE category = $1 AND task_outcome IS NOT NULL
             ORDER BY created_at DESC LIMIT $2",
        )
        .bind(category)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn list_modified_snapshots(
        &self,
        limit: usize,
    ) -> DbResult<Vec<(String, String, String, String)>> {
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
             LIMIT $1",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // ========== AST links ==========

    async fn kb_add_ast_link(
        &self,
        kb_id: &str,
        symbol_name: &str,
        file_path: Option<&str>,
        ast_node_id: Option<&str>,
        relation: &str,
        confidence: f64,
    ) -> DbResult<()> {
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
                    k.linked_task_id, k.kb_type, k.scope_task_id, k.utility_score, k.project_id
             FROM knowledge k
             JOIN kb_ast_links l ON l.kb_id = k.id
             WHERE l.symbol_name = $1
             ORDER BY k.utility_score DESC, k.updated_at DESC
             LIMIT 10",
        )
        .bind(symbol_name)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(kb_row_to_entry).collect())
    }

    async fn kb_get_memories_for_file(
        &self,
        file_path: &str,
    ) -> DbResult<Vec<(String, KnowledgeEntry)>> {
        // Note: project_id omitted to stay within sqlx 16-field tuple limit (symbol_name + 15 KB cols)
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
        Ok(rows
            .into_iter()
            .map(|r| {
                let symbol = r.0;
                let entry = row_to_knowledge_entry(
                    r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9, r.10, r.11, r.12, r.13, r.14,
                    r.15, None,
                );
                (symbol, entry)
            })
            .collect())
    }

    async fn kb_delete_ast_links_for(&self, kb_id: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM kb_ast_links WHERE kb_id = $1")
            .bind(kb_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn kb_ast_lazy_resolve(
        &self,
        link_id: i64,
        symbol_name: &str,
    ) -> DbResult<Option<String>> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT id FROM ast_nodes WHERE name = $1 LIMIT 1")
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
    async fn pg_get_by_category_key(
        &self,
        category: &str,
        key: &str,
    ) -> DbResult<Option<KnowledgeEntry>> {
        let row: Option<KBRow> = sqlx::query_as(&format!(
            "SELECT {} FROM knowledge WHERE category = $1 AND key = $2",
            KB_COLS
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
    async fn pg_search_fts(
        &self,
        query: &str,
        category: Option<&str>,
    ) -> DbResult<Vec<KnowledgeEntry>> {
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
                 ORDER BY ts_rank(fts_doc, plainto_tsquery('simple', $1)) DESC",
                KB_COLS
            ))
            .bind(query)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows.into_iter().map(kb_row_to_entry).collect())
    }

    /// LIKE fallback for Chinese text and partial matches.
    async fn pg_search_like(
        &self,
        query: &str,
        category: Option<&str>,
    ) -> DbResult<Vec<KnowledgeEntry>> {
        let keywords: Vec<String> = {
            let mut kw: Vec<String> = query.split_whitespace().map(|w| w.to_string()).collect();
            let trimmed = query.trim();
            if !trimmed.is_empty()
                && trimmed.chars().any(|c| !c.is_ascii())
                && !kw.contains(&trimmed.to_string())
            {
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
            sql.push_str(&format!(
                " AND (category = ${} OR category LIKE ${})",
                p_cat, p_like
            ));
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

#[cfg(feature = "postgres")]
#[async_trait]
impl EvidenceLaneStore for PgMissionStore {
    async fn get_evidence_item(&self, id: &str) -> DbResult<Option<EvidenceItemInput>> {
        let row = sqlx::query(
            "SELECT
                id, lane_id, source_type, source_id, source_ref, project_id, task_id,
                title, summary, authority_class, validity, privacy_class, freshness,
                score, raw_policy, evidence_refs, metadata
             FROM evidence_items
             WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| evidence_item_from_row(&row)).transpose()?)
    }

    async fn search_evidence_items(
        &self,
        input: &EvidenceSearchInput,
    ) -> DbResult<Vec<EvidenceItemInput>> {
        let allowed_lanes = if input.allowed_lanes.is_empty() {
            vec![
                "runtime_truth".to_string(),
                "project_ssot".to_string(),
                "reviewed_kb".to_string(),
                "active_board".to_string(),
                "skill_evidence".to_string(),
                "conversation_audit".to_string(),
                "cold_archive".to_string(),
                "support_refs".to_string(),
            ]
        } else {
            input.allowed_lanes.clone()
        };
        let query = input.query.trim();
        let like_query = format!("%{}%", query);
        let limit = input.limit.clamp(1, 50);
        let rows = sqlx::query(
            "SELECT
                id, lane_id, source_type, source_id, source_ref, project_id, task_id,
                title, summary, authority_class, validity, privacy_class, freshness,
                score, raw_policy, evidence_refs, metadata,
                CASE
                    WHEN $1 = '' THEN 0.0
                    ELSE ts_rank_cd(fts_doc, plainto_tsquery('simple', $1))::double precision
                END AS search_score
             FROM evidence_items
             WHERE lane_id = ANY($2::text[])
               AND ($3::text IS NULL OR project_id = $3 OR ($4::boolean AND project_id IS NULL))
               AND ($5::text IS NULL OR task_id = $5 OR task_id IS NULL)
               AND (
                    $1 = ''
                    OR fts_doc @@ plainto_tsquery('simple', $1)
                    OR title ILIKE $6
                    OR summary ILIKE $6
                    OR source_ref ILIKE $6
               )
             ORDER BY
                (project_id = $3) DESC NULLS LAST,
                (task_id = $5) DESC NULLS LAST,
                search_score DESC,
                COALESCE(score, 0.0) DESC,
                updated_at DESC
             LIMIT $7",
        )
        .bind(query)
        .bind(&allowed_lanes)
        .bind(input.project_id.as_deref())
        .bind(input.include_global)
        .bind(input.task_id.as_deref())
        .bind(&like_query)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let search_score: Option<f64> = row.try_get("search_score")?;
            let mut item = evidence_item_from_row(&row)?;
            item.score = item.score.or(search_score);
            items.push(item);
        }
        Ok(items)
    }

    async fn backfill_conversation_evidence_items(&self, limit: i64) -> DbResult<usize> {
        let limit = limit.clamp(1, 500);
        let (count,): (i64,) = sqlx::query_as(
            "WITH candidates AS (
                SELECT
                    c.id AS conversation_id,
                    COALESCE(NULLIF(c.project_id, ''), NULLIF(c.project, '')) AS project_id,
                    NULLIF(c.task_id, '') AS task_id,
                    COALESCE(NULLIF(c.conversation_type, ''), 'user') AS conversation_type,
                    NULLIF(c.jsonl_path, '') AS jsonl_path,
                    COALESCE(
                        NULLIF(c.llm_summary, ''),
                        NULLIF(c.rolling_summary, ''),
                        NULLIF(c.session_timeline, ''),
                        CONCAT('Conversation ', c.id, ' has ', COALESCE(c.message_count, 0), ' messages.')
                    ) AS summary,
                    COALESCE(NULLIF(c.updated_at, ''), NULLIF(c.ended_at, ''), NULLIF(c.started_at, '')) AS freshness_marker
                FROM conversations c
                WHERE NOT EXISTS (
                    SELECT 1 FROM conversation_episodes e WHERE e.conversation_id = c.id
                )
                ORDER BY COALESCE(NULLIF(c.updated_at, ''), NULLIF(c.ended_at, ''), NULLIF(c.started_at, '')) DESC NULLS LAST
                LIMIT $1
             ),
             upsert_episodes AS (
                INSERT INTO conversation_episodes (
                    id, conversation_id, project_id, task_id, conversation_type,
                    topic, outcome, summary, staleness, review_state,
                    derived_from_conversation, evidence_refs, metadata
                )
                SELECT
                    'conv-episode-' || substr(md5(conversation_id), 1, 20),
                    conversation_id,
                    project_id,
                    task_id,
                    conversation_type,
                    'conversation-summary',
                    'summarized-for-audit',
                    summary,
                    'unknown',
                    'needs_review',
                    TRUE,
                    jsonb_build_array(jsonb_build_object(
                        'source', 'conversation_messages',
                        'conversation_id', conversation_id,
                        'raw_policy', 'raw_opt_in_only'
                    )),
                    jsonb_build_object(
                        'projection', 'backfill_conversation_evidence_items',
                        'jsonl_path', jsonl_path,
                        'freshness_marker', freshness_marker
                    )
                FROM candidates
                ON CONFLICT (id) DO UPDATE SET
                    project_id = EXCLUDED.project_id,
                    task_id = EXCLUDED.task_id,
                    conversation_type = EXCLUDED.conversation_type,
                    summary = EXCLUDED.summary,
                    evidence_refs = EXCLUDED.evidence_refs,
                    metadata = EXCLUDED.metadata,
                    updated_at = now()
                RETURNING id, conversation_id, project_id, task_id, conversation_type, summary, evidence_refs
             ),
             upsert_facts AS (
                INSERT INTO conversation_fact_extracts (
                    id, episode_id, conversation_id, project_id, fact_key,
                    fact_summary, validity, staleness, confidence,
                    derived_from_conversation, source_message_ids, evidence_refs
                )
                SELECT
                    'conv-fact-' || substr(md5(conversation_id || ':summary'), 1, 20),
                    id,
                    conversation_id,
                    project_id,
                    'conversation-summary:' || conversation_id,
                    summary,
                    'needs_review',
                    'unknown',
                    0.5,
                    TRUE,
                    '[]'::jsonb,
                    evidence_refs
                FROM upsert_episodes
                ON CONFLICT (conversation_id, fact_key) DO UPDATE SET
                    episode_id = EXCLUDED.episode_id,
                    project_id = EXCLUDED.project_id,
                    fact_summary = EXCLUDED.fact_summary,
                    validity = EXCLUDED.validity,
                    staleness = EXCLUDED.staleness,
                    evidence_refs = EXCLUDED.evidence_refs,
                    updated_at = now()
                RETURNING id, episode_id, conversation_id, project_id, fact_key, fact_summary, evidence_refs
             ),
             upsert_episode_evidence AS (
                INSERT INTO evidence_items (
                    id, lane_id, source_type, source_id, source_ref, project_id, task_id,
                    title, summary, authority_class, validity, privacy_class, freshness,
                    score, raw_policy, evidence_refs, metadata
                )
                SELECT
                    'evi-conv-episode-' || substr(md5(conversation_id), 1, 20),
                    'conversation_audit',
                    'conversation_episode',
                    id,
                    conversation_id,
                    project_id,
                    task_id,
                    'Conversation episode',
                    summary,
                    'provider_durable_conversation_read_model',
                    'derived_from_conversation',
                    'audit',
                    'time_range_bound',
                    0.5,
                    'raw_opt_in_only',
                    evidence_refs,
                    jsonb_build_object(
                        'projection', 'conversation_episode',
                        'conversation_id', conversation_id,
                        'conversation_type', conversation_type,
                        'review_state', 'needs_review'
                    )
                FROM upsert_episodes
                ON CONFLICT (id) DO UPDATE SET
                    project_id = EXCLUDED.project_id,
                    task_id = EXCLUDED.task_id,
                    summary = EXCLUDED.summary,
                    evidence_refs = EXCLUDED.evidence_refs,
                    metadata = EXCLUDED.metadata,
                    updated_at = now()
                RETURNING id
             ),
             upsert_fact_evidence AS (
                INSERT INTO evidence_items (
                    id, lane_id, source_type, source_id, source_ref, project_id, task_id,
                    title, summary, authority_class, validity, privacy_class, freshness,
                    score, raw_policy, evidence_refs, metadata
                )
                SELECT
                    'evi-conv-fact-' || substr(md5(conversation_id || ':' || fact_key), 1, 20),
                    'conversation_audit',
                    'conversation_fact_extract',
                    id,
                    conversation_id,
                    project_id,
                    NULL,
                    fact_key,
                    fact_summary,
                    'provider_durable_conversation_read_model',
                    'needs_review',
                    'audit',
                    'time_range_bound',
                    0.5,
                    'raw_opt_in_only',
                    evidence_refs,
                    jsonb_build_object(
                        'projection', 'conversation_fact_extract',
                        'episode_id', episode_id,
                        'conversation_id', conversation_id,
                        'review_state', 'needs_review'
                    )
                FROM upsert_facts
                ON CONFLICT (id) DO UPDATE SET
                    project_id = EXCLUDED.project_id,
                    summary = EXCLUDED.summary,
                    evidence_refs = EXCLUDED.evidence_refs,
                    metadata = EXCLUDED.metadata,
                    updated_at = now()
                RETURNING id
             )
             SELECT
                (SELECT COUNT(*) FROM upsert_episode_evidence)
                + (SELECT COUNT(*) FROM upsert_fact_evidence)",
        )
        .bind(limit)
        .fetch_one(&self.pool)
        .await?;
        Ok(count as usize)
    }

    async fn upsert_evidence_items(&self, items: &[EvidenceItemInput]) -> DbResult<usize> {
        let mut written = 0usize;
        for item in items {
            let result = sqlx::query(
                "INSERT INTO evidence_items (
                    id, lane_id, source_type, source_id, source_ref, project_id, task_id,
                    title, summary, authority_class, validity, privacy_class, freshness,
                    score, raw_policy, evidence_refs, metadata
                 )
                 VALUES (
                    $1, $2, $3, $4, $5, $6, $7,
                    $8, $9, $10, $11, $12, $13,
                    $14, $15, $16, $17
                 )
                 ON CONFLICT (id) DO UPDATE SET
                    lane_id = EXCLUDED.lane_id,
                    source_type = EXCLUDED.source_type,
                    source_id = EXCLUDED.source_id,
                    source_ref = EXCLUDED.source_ref,
                    project_id = EXCLUDED.project_id,
                    task_id = EXCLUDED.task_id,
                    title = EXCLUDED.title,
                    summary = EXCLUDED.summary,
                    authority_class = EXCLUDED.authority_class,
                    validity = EXCLUDED.validity,
                    privacy_class = EXCLUDED.privacy_class,
                    freshness = EXCLUDED.freshness,
                    score = EXCLUDED.score,
                    raw_policy = EXCLUDED.raw_policy,
                    evidence_refs = EXCLUDED.evidence_refs,
                    metadata = EXCLUDED.metadata,
                    updated_at = now()",
            )
            .bind(&item.id)
            .bind(&item.lane_id)
            .bind(&item.source_type)
            .bind(item.source_id.as_deref())
            .bind(item.source_ref.as_deref())
            .bind(item.project_id.as_deref())
            .bind(item.task_id.as_deref())
            .bind(&item.title)
            .bind(&item.summary)
            .bind(&item.authority_class)
            .bind(&item.validity)
            .bind(&item.privacy_class)
            .bind(&item.freshness)
            .bind(item.score)
            .bind(&item.raw_policy)
            .bind(&item.evidence_refs)
            .bind(&item.metadata)
            .execute(&self.pool)
            .await?;
            written += result.rows_affected() as usize;
            if is_skill_evidence_projection(item) {
                upsert_skill_evidence_item_projection(&self.pool, item).await?;
            }
        }
        Ok(written)
    }

    async fn record_context_gather_run(&self, run: &ContextGatherRunInput) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO context_gather_runs (
                id, query, project_id, task_id, source_profile, lane_counts, metrics,
                raw_sources_included, credential_opt_in, conversation_opt_in,
                resolver_source, runtime_root_consistent, artifact_hash, diagnostics
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
             ON CONFLICT (id) DO UPDATE SET
                query = EXCLUDED.query,
                project_id = EXCLUDED.project_id,
                task_id = EXCLUDED.task_id,
                source_profile = EXCLUDED.source_profile,
                lane_counts = EXCLUDED.lane_counts,
                metrics = EXCLUDED.metrics,
                raw_sources_included = EXCLUDED.raw_sources_included,
                credential_opt_in = EXCLUDED.credential_opt_in,
                conversation_opt_in = EXCLUDED.conversation_opt_in,
                resolver_source = EXCLUDED.resolver_source,
                runtime_root_consistent = EXCLUDED.runtime_root_consistent,
                artifact_hash = EXCLUDED.artifact_hash,
                diagnostics = EXCLUDED.diagnostics",
        )
        .bind(&run.id)
        .bind(&run.query)
        .bind(run.project_id.as_deref())
        .bind(run.task_id.as_deref())
        .bind(&run.source_profile)
        .bind(&run.lane_counts)
        .bind(&run.metrics)
        .bind(run.raw_sources_included)
        .bind(run.credential_opt_in)
        .bind(run.conversation_opt_in)
        .bind(run.resolver_source.as_deref())
        .bind(run.runtime_root_consistent)
        .bind(run.artifact_hash.as_deref())
        .bind(&run.diagnostics)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_source_session_accepts_camel_and_snake_case_details() {
        let a = serde_json::json!({"source_session": "session-a"});
        let b = serde_json::json!({"sourceSessions": ["session-a", "session-b"]});
        let c = serde_json::json!({"sourceSession": "session-c"});

        assert!(same_source_session(Some(&a), Some(&b)));
        assert!(!same_source_session(Some(&a), Some(&c)));
    }

    #[test]
    fn merge_detail_for_dedupe_preserves_evidence_and_supersession_arrays() {
        let existing = serde_json::json!({
            "evidence_refs": ["lisp://old"],
            "superseded_by": ["kb-old"],
            "source_sessions": ["session-a"],
            "stable": true
        });
        let incoming = serde_json::json!({
            "evidence_refs": ["lisp://old", "code://new"],
            "superseded_by": "kb-new",
            "source_sessions": ["session-b"],
            "stable": false,
            "incoming_only": true
        });

        let merged = merge_detail_for_dedupe(
            Some(existing),
            Some(incoming),
            "memory:new",
            "deep-analysis",
            "2026-05-11T00:00:00Z",
            0.77,
        )
        .expect("merged detail");

        let obj = merged.as_object().expect("object");
        assert_eq!(obj.get("stable").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            obj.get("incoming_only").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            obj.get("evidence_refs")
                .and_then(|v| v.as_array())
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            obj.get("superseded_by")
                .and_then(|v| v.as_array())
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            obj.get("_dedupe_merge_events")
                .and_then(|v| v.as_array())
                .map(Vec::len),
            Some(1)
        );
    }
}
