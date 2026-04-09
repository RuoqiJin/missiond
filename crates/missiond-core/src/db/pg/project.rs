//! Project registry — PostgreSQL persistence layer.

use async_trait::async_trait;
use sqlx::PgPool;
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
}

pub async fn upsert_project(pool: &PgPool, config: &ProjectConfig) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO projects (id, path, intent_path, active, slots, github_url, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5::text[], $6, COALESCE($7, NOW()), NOW())
         ON CONFLICT (id) DO UPDATE SET
           path = EXCLUDED.path,
           intent_path = EXCLUDED.intent_path,
           active = EXCLUDED.active,
           slots = EXCLUDED.slots,
           github_url = COALESCE(EXCLUDED.github_url, projects.github_url),
           updated_at = NOW()"
    )
    .bind(&config.id)
    .bind(&config.path)
    .bind(&config.intent_path)
    .bind(config.active)
    .bind(&config.slots)
    .bind(&config.github_url)
    .bind(config.created_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_projects(pool: &PgPool) -> Result<Vec<ProjectConfig>, sqlx::Error> {
    let rows: Vec<(String, String, Option<String>, bool, Vec<String>, Option<String>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)> =
        sqlx::query_as(
            "SELECT id, path, intent_path, active, slots, github_url, created_at, updated_at FROM projects ORDER BY id"
        )
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(row_to_config).collect())
}

pub async fn list_active_projects(pool: &PgPool) -> Result<Vec<ProjectConfig>, sqlx::Error> {
    let rows: Vec<(String, String, Option<String>, bool, Vec<String>, Option<String>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)> =
        sqlx::query_as(
            "SELECT id, path, intent_path, active, slots, created_at, updated_at FROM projects WHERE active = true ORDER BY id"
        )
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(row_to_config).collect())
}

pub async fn get_project(pool: &PgPool, id: &str) -> Result<Option<ProjectConfig>, sqlx::Error> {
    let row: Option<(String, String, Option<String>, bool, Vec<String>, Option<String>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)> =
        sqlx::query_as(
            "SELECT id, path, intent_path, active, slots, github_url, created_at, updated_at FROM projects WHERE id = $1"
        )
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

pub async fn delete_project(pool: &PgPool, id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

fn row_to_config(r: (String, String, Option<String>, bool, Vec<String>, Option<String>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)) -> ProjectConfig {
    ProjectConfig {
        id: r.0,
        path: r.1,
        intent_path: r.2,
        active: r.3,
        slots: r.4,
        github_url: r.5,
        created_at: Some(r.6),
        updated_at: Some(r.7),
    }
}
