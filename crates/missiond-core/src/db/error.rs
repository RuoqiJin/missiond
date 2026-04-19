//! Database error types — anti-corruption layer between sqlx and business logic.
//!
//! PostgreSQL (sqlx) errors convert to `DbError`, so callers only ever see `DbResult<T>`.
//! (SQLite backend removed in v0.4.23 Stage 2E.)

/// Database-specific error type that hides the underlying driver.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[cfg(feature = "postgres")]
    #[error("sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("not found: {entity} {id}")]
    NotFound { entity: &'static str, id: String },

    #[error("constraint: {0}")]
    Constraint(String),

    #[error("{0}")]
    Other(String),
}

/// Result alias used by all DB methods.
pub type DbResult<T> = Result<T, DbError>;
