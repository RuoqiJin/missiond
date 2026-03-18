//! PostgreSQL backend for MissionD (M2).
//!
//! Implements all 12 domain traits via sqlx + PgPool.
//! No spawn_blocking needed — sqlx is natively async.

#[cfg(feature = "postgres")]
use sqlx::PgPool;

/// PostgreSQL-backed MissionStore implementation.
///
/// Unlike SqliteMissionStore, this does NOT use DbExecutor/spawn_blocking.
/// All queries go through sqlx's async connection pool directly.
#[cfg(feature = "postgres")]
#[derive(Clone)]
pub struct PgMissionStore {
    pool: PgPool,
}

#[cfg(feature = "postgres")]
impl PgMissionStore {
    /// Connect to PostgreSQL and run migrations.
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        use sqlx::postgres::PgPoolOptions;
        use std::time::Duration;

        let pool = PgPoolOptions::new()
            .max_connections(20)
            .min_connections(2)
            .acquire_timeout(Duration::from_secs(5))
            .idle_timeout(Duration::from_secs(600))
            .connect(database_url)
            .await?;

        // Run embedded migrations (SQL baked into binary at compile time).
        // Does NOT need a compile-time DB connection — only reads files.
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await?;

        Ok(Self { pool })
    }

    /// Create from an existing pool (for testing).
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get a reference to the underlying pool.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

// MissionStore super-trait implementation
#[cfg(feature = "postgres")]
#[async_trait::async_trait]
impl crate::db::traits::MissionStore for PgMissionStore {
    async fn init(&self) -> crate::db::error::DbResult<()> {
        // Migrations are run in connect(), nothing else needed.
        Ok(())
    }
}

// M2-6: SQLite → PostgreSQL data migration
#[cfg(all(feature = "sqlite", feature = "postgres"))]
pub mod migrate_from_sqlite;

// Domain trait implementations — one file per trait:
#[cfg(feature = "postgres")]
mod vision;
#[cfg(feature = "postgres")]
mod event;
#[cfg(feature = "postgres")]
mod skill;
#[cfg(feature = "postgres")]
mod observability;
#[cfg(feature = "postgres")]
mod timeline;
#[cfg(feature = "postgres")]
mod retrospective;
#[cfg(feature = "postgres")]
mod slot;
#[cfg(feature = "postgres")]
mod board;
#[cfg(feature = "postgres")]
mod conversation;
#[cfg(feature = "postgres")]
mod message;
#[cfg(feature = "postgres")]
mod tool_call;
#[cfg(feature = "postgres")]
mod knowledge;
