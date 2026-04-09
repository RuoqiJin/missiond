use async_trait::async_trait;
use crate::db::error::DbResult;
use crate::db::traits::ProjectStore;
use crate::types::Project;
use super::PgMissionStore;

#[cfg(feature = "postgres")]
#[async_trait]
impl ProjectStore for PgMissionStore {
    async fn list_projects(&self) -> DbResult<Vec<Project>> {
        let rows: Vec<(String, String, String, bool, String, String)> = sqlx::query_as(
            "SELECT id, name, path, active, created_at, updated_at FROM projects ORDER BY name ASC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id, name, path, active, created_at, updated_at)| {
            Project { id, name, path, active, created_at, updated_at }
        }).collect())
    }

    async fn get_project(&self, id: &str) -> DbResult<Option<Project>> {
        let row: Option<(String, String, String, bool, String, String)> = sqlx::query_as(
            "SELECT id, name, path, active, created_at, updated_at FROM projects WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id, name, path, active, created_at, updated_at)| {
            Project { id, name, path, active, created_at, updated_at }
        }))
    }

    async fn upsert_project(&self, id: &str, name: &str, path: &str) -> DbResult<Project> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO projects (id, name, path, active, created_at, updated_at)
             VALUES ($1, $2, $3, true, $4, $5)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, path = EXCLUDED.path, updated_at = EXCLUDED.updated_at"
        )
        .bind(id)
        .bind(name)
        .bind(path)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(Project {
            id: id.to_string(),
            name: name.to_string(),
            path: path.to_string(),
            active: true,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    async fn set_project_active(&self, id: &str, active: bool) -> DbResult<Option<Project>> {
        let now = chrono::Utc::now().to_rfc3339();
        let result = sqlx::query(
            "UPDATE projects SET active = $1, updated_at = $2 WHERE id = $3"
        )
        .bind(active)
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.get_project(id).await
    }
}
