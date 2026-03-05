//! Database error types — anti-corruption layer between rusqlite and business logic.
//!
//! Phase 3 will swap `Sqlite(rusqlite::Error)` for `Sqlx(sqlx::Error)` with zero
//! impact on callers that only use `DbResult<T>`.

/// Database-specific error type that hides the underlying driver.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("not found: {entity} {id}")]
    NotFound { entity: &'static str, id: String },

    #[error("constraint: {0}")]
    Constraint(String),

    #[error("{0}")]
    Other(String),
}

/// Result alias used by all DB methods.
pub type DbResult<T> = Result<T, DbError>;
