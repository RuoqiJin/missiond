//! Project registry — PostgreSQL persistence layer.

use async_trait::async_trait;
use sqlx::PgPool;
use serde_json::json;
use crate::db::error::DbResult;
use crate::db::traits::ProjectStore;
use crate::types::ProjectConfig;
use super::PgMissionStore;

#[cfg(feature = "postgres")]
#[async_trait]
impl ProjectStore for PgMissionStore {
    async fn list_projects(&self) -> DbResult<Vec<ProjectConfig>> {
        Ok(list_projects(&self.pool).await?)
    }

    async fn get_project(&self, id: &str) -> DbResult<Option<ProjectConfig>> {
        Ok(get_project(&self.pool, id).await?)
    }

    async fn upsert_project(&self, config: &ProjectConfig) -> DbResult<()> {
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

    async fn recent_conversations_by_project(&self, project_id: &str, limit: i64) -> DbResult<Vec<serde_json::Value>> {
        Ok(recent_conversations_by_project(&self.pool, project_id, limit).await?)
    }

    async fn kb_stats_by_project(&self, project_id: &str) -> DbResult<serde_json::Value> {
        Ok(kb_stats_by_project(&self.pool, project_id).await?)
    }
}

pub async fn upsert_project(pool: &PgPool, config: &ProjectConfig) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO projects (id, path, intent_path, active, slots, github_url, kind, vault_path, parent_id, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5::text[], $6, $7, $8, $9, COALESCE($10, NOW()), NOW())
         ON CONFLICT (id) DO UPDATE SET
           path = EXCLUDED.path,
           intent_path = EXCLUDED.intent_path,
           active = EXCLUDED.active,
           slots = EXCLUDED.slots,
           github_url = COALESCE(EXCLUDED.github_url, projects.github_url),
           kind = EXCLUDED.kind,
           vault_path = COALESCE(EXCLUDED.vault_path, projects.vault_path),
           parent_id = EXCLUDED.parent_id,
           updated_at = NOW()"
    )
    .bind(&config.id)
    .bind(&config.path)
    .bind(&config.intent_path)
    .bind(config.active)
    .bind(&config.slots)
    .bind(&config.github_url)
    .bind(&config.kind)
    .bind(&config.vault_path)
    .bind(&config.parent_id)
    .bind(config.created_at)
    .execute(pool)
    .await?;
    Ok(())
}

type ProjectRow = (String, String, Option<String>, bool, Vec<String>, Option<String>, String, Option<String>, Option<String>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>);

const PROJECT_COLS: &str = "id, path, intent_path, active, slots, github_url, kind, vault_path, parent_id, created_at, updated_at";

pub async fn list_projects(pool: &PgPool) -> Result<Vec<ProjectConfig>, sqlx::Error> {
    let rows: Vec<ProjectRow> =
        sqlx::query_as(&format!("SELECT {} FROM projects ORDER BY id", PROJECT_COLS))
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(row_to_config).collect())
}

pub async fn list_active_projects(pool: &PgPool) -> Result<Vec<ProjectConfig>, sqlx::Error> {
    let rows: Vec<ProjectRow> =
        sqlx::query_as(&format!("SELECT {} FROM projects WHERE active = true ORDER BY id", PROJECT_COLS))
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(row_to_config).collect())
}

pub async fn get_project(pool: &PgPool, id: &str) -> Result<Option<ProjectConfig>, sqlx::Error> {
    let row: Option<ProjectRow> =
        sqlx::query_as(&format!("SELECT {} FROM projects WHERE id = $1", PROJECT_COLS))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(row_to_config))
}

pub async fn update_project_active(pool: &PgPool, id: &str, active: bool) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE projects SET active = $2, updated_at = NOW() WHERE id = $1"
    )
    .bind(id)
    .bind(active)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn update_project_slots(pool: &PgPool, id: &str, slots: &[String]) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE projects SET slots = $2::text[], updated_at = NOW() WHERE id = $1"
    )
    .bind(id)
    .bind(slots)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn backfill_project_id(pool: &PgPool, project_id: &str, path_pattern: &str) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE conversations SET project_id = $1 WHERE project_id IS NULL AND project LIKE $2"
    )
    .bind(project_id)
    .bind(path_pattern)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn delete_project(pool: &PgPool, id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn conversation_stats_by_project(pool: &PgPool, project_id: &str) -> Result<serde_json::Value, sqlx::Error> {
    let row: (i64, i64, i64, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT COUNT(*) as total,
                COUNT(CASE WHEN status = 'active' THEN 1 END) as active,
                COUNT(CASE WHEN status = 'completed' THEN 1 END) as completed,
                MAX(updated_at) as last_activity
         FROM conversations WHERE project_id = $1"
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    Ok(json!({
        "total": row.0,
        "active": row.1,
        "completed": row.2,
        "last_activity": row.3,
    }))
}

pub async fn recent_conversations_by_project(pool: &PgPool, project_id: &str, limit: i64) -> Result<Vec<serde_json::Value>, sqlx::Error> {
    let rows: Vec<(String, Option<String>, String, Option<String>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT id, slot_name, status, summary, created_at, updated_at
         FROM conversations WHERE project_id = $1
         ORDER BY updated_at DESC LIMIT $2"
    )
    .bind(project_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| json!({
        "id": r.0,
        "slot_name": r.1,
        "status": r.2,
        "summary": r.3,
        "created_at": r.4,
        "updated_at": r.5,
    })).collect())
}

pub async fn kb_stats_by_project(pool: &PgPool, project_id: &str) -> Result<serde_json::Value, sqlx::Error> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT category, COUNT(*) as cnt
         FROM knowledge
         WHERE project_id = $1 OR project_id IS NULL
         GROUP BY category ORDER BY cnt DESC"
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let total: i64 = rows.iter().map(|r| r.1).sum();
    let by_category: serde_json::Map<String, serde_json::Value> = rows.into_iter()
        .map(|(cat, cnt)| (cat, serde_json::Value::from(cnt)))
        .collect();
    Ok(json!({
        "total": total,
        "by_category": by_category,
    }))
}

fn row_to_config(r: ProjectRow) -> ProjectConfig {
    ProjectConfig {
        id: r.0,
        path: r.1,
        intent_path: r.2,
        active: r.3,
        slots: r.4,
        github_url: r.5,
        kind: r.6,
        vault_path: r.7,
        parent_id: r.8,
        created_at: Some(r.9),
        updated_at: Some(r.10),
    }
}
