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

        // Run migrations from SQL files at runtime.
        // No compile-time DB connection needed (unlike sqlx::migrate!() macro).
        let migrator = sqlx::migrate::Migrator::new(
            std::path::Path::new("./migrations")
        ).await?;
        migrator.run(&pool).await?;

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

// Domain trait implementations will be added in subsequent files:
// - conversation.rs  (ConversationStore)
// - message.rs       (MessageStore)
// - tool_call.rs     (ToolCallStore)
// - event.rs         (EventStore)
// - retrospective.rs (RetrospectiveStore)
// - vision.rs        (VisionStore)
// - knowledge.rs     (KnowledgeStore)
// - board.rs         (BoardStore)
// - timeline.rs      (TimelineStore)
// - slot.rs          (SlotStore)
// - skill.rs         (SkillStore)
// - observability.rs (ObservabilityStore)
