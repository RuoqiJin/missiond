pub mod executions;
pub mod migrations;
pub mod skills;
pub mod subscriptions;
pub mod users;

use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

pub struct Database {
    pool: PgPool,
}

impl Database {
    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;
        migrations::run(&pool).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

pub(crate) fn ts_string(row: &sqlx::postgres::PgRow, name: &str) -> Result<String, sqlx::Error> {
    let dt: chrono::DateTime<chrono::Utc> = row.try_get(name)?;
    Ok(dt.to_rfc3339())
}
