//! PostgreSQL backend for MissionD.
//!
//! Implements all 12 domain traits via sqlx + PgPool.
//! No spawn_blocking needed — sqlx is natively async.
//! PostgreSQL is the only MissionD runtime store; provider-local indexes are
//! ingested by workers, not used as a MissionD database backend.

#[cfg(feature = "postgres")]
use sqlx::PgPool;

/// PostgreSQL-backed MissionStore implementation.
///
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
        // Keep this call in missiond-core so release builds package the latest
        // canonical-source and slot-session cleanup migrations with the store.
        // Does NOT need a compile-time DB connection — only reads files.
        sqlx::migrate!("./migrations").run(&pool).await?;

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

    /// Fix identity sequences that may be desynced after bulk imports.
    /// Safe to call on every startup — only advances sequences that are behind.
    pub async fn fix_identity_sequences(&self) {
        use tracing::{info, warn};

        let checks: &[(&str, &str, &str)] = &[
            (
                "conversation_messages",
                "id",
                "conversation_messages_id_seq",
            ),
            ("conversation_events", "id", "conversation_events_id_seq"),
            ("token_usage_ledger", "id", "token_usage_ledger_id_seq"),
            // system_timeline dropped in v1.3.0 SSOT cutover (event_log SSOT).
        ];

        let mut fixed = 0usize;
        for &(table, col, seq) in checks {
            let sql = format!(
                "SELECT setval('{seq}', GREATEST(nextval('{seq}'), COALESCE((SELECT MAX({col}) FROM {table}), 0) + 1), false)"
            );
            match sqlx::query(&sql).execute(&self.pool).await {
                Ok(_) => fixed += 1,
                Err(e) => warn!(table, seq, error = %e, "Failed to fix identity sequence"),
            }
        }
        if fixed > 0 {
            info!(fixed, "Identity sequence health check complete");
        }
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

// Domain trait implementations — one file per trait:
// Note: tool_call/event/retrospective merged into conversation (v0.4.23).
// Note: vision merged into observability (v0.4.23 — Stage 2C.5).
#[cfg(feature = "postgres")]
mod artifact_commit;
#[cfg(feature = "postgres")]
mod board;
#[cfg(feature = "postgres")]
mod conversation;
#[cfg(feature = "postgres")]
mod directive;
#[cfg(feature = "postgres")]
mod infra;
#[cfg(feature = "postgres")]
mod knowledge;
#[cfg(feature = "postgres")]
mod message;
#[cfg(feature = "postgres")]
mod observability;
#[cfg(feature = "postgres")]
pub mod project;
#[cfg(feature = "postgres")]
mod skill;
#[cfg(feature = "postgres")]
mod slot;
#[cfg(feature = "postgres")]
mod timeline;
