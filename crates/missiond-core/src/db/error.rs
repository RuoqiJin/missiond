//! Database error types — anti-corruption layer between rusqlite/sqlx and business logic.
//!
//! Both SQLite (rusqlite) and PostgreSQL (sqlx) errors convert to `DbError`,
//! so callers only ever see `DbResult<T>`.

/// Database-specific error type that hides the underlying driver.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[cfg(feature = "sqlite")]
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

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
