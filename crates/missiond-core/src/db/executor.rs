//! Async wrapper around MissionDB — offloads blocking SQLite calls to
//! `tokio::task::spawn_blocking` to prevent stalling the Tokio reactor.
//!
//! Phase 3 PR6: Conservative approach — only hot-path calls (FTS5 search,
//! batch task queries) use `run()`. Lightweight calls use `sync_ref()`.

use std::sync::Arc;
use super::error::{DbError, DbResult};
use super::MissionDB;

/// Async executor for MissionDB operations.
///
/// - `run(|db| ...)` — offloads to `spawn_blocking` (use for FTS5, batch scans)
/// - `sync_ref()` — direct synchronous access (use for single-row lookups)
#[derive(Clone)]
pub struct DbExecutor {
    db: Arc<MissionDB>,
}

impl DbExecutor {
    pub fn new(db: Arc<MissionDB>) -> Self {
        Self { db }
    }

    /// Execute a closure on a blocking thread pool.
    /// Use for FTS5 search, table scans, and any operation that may take >1ms.
    pub async fn run<T, F>(&self, f: F) -> DbResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&MissionDB) -> DbResult<T> + Send + 'static,
    {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || f(&db))
            .await
            .map_err(|e| DbError::Other(format!("spawn_blocking join: {e}")))?
    }

    /// Direct synchronous reference for lightweight single-row lookups.
    /// Avoid for FTS5 search or batch operations.
    pub fn sync_ref(&self) -> &MissionDB {
        &self.db
    }
}
