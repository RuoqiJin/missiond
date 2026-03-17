//! SQLite backend for MissionStore traits.
//!
//! Wraps the existing synchronous `MissionDB` via `DbExecutor::run()` (spawn_blocking)
//! to provide async trait implementations that preserve slow-query monitoring.

mod conversation;
mod message;
mod tool_call;
mod event;
mod retrospective;
mod vision;
mod knowledge;
mod board;
mod timeline;
mod slot;
mod skill;
mod observability;

use std::sync::Arc;
use async_trait::async_trait;
use super::MissionDB;
use super::executor::{DbExecutor, OnRunCallback};
use super::error::DbResult;
use super::traits::MissionStore;

/// SQLite-backed implementation of all MissionStore domain traits.
/// Uses `DbExecutor` to bridge sync rusqlite calls into async context.
#[derive(Clone)]
pub struct SqliteMissionStore {
    executor: DbExecutor,
}

impl SqliteMissionStore {
    /// Create from an existing MissionDB instance.
    pub fn new(db: Arc<MissionDB>) -> Self {
        Self {
            executor: DbExecutor::new(db),
        }
    }

    /// Create with a custom latency callback for monitoring.
    pub fn with_callback(db: Arc<MissionDB>, cb: OnRunCallback) -> Self {
        let mut executor = DbExecutor::new(db);
        executor.set_on_run(cb);
        Self { executor }
    }

    /// Direct synchronous access for lightweight operations.
    pub fn sync_ref(&self) -> &MissionDB {
        self.executor.sync_ref()
    }

    /// Get a reference to the inner executor.
    pub fn executor(&self) -> &DbExecutor {
        &self.executor
    }
}

#[async_trait]
impl MissionStore for SqliteMissionStore {
    async fn init(&self) -> DbResult<()> {
        // MissionDB::init() is already called in MissionDB::new(),
        // but we expose it here for the trait contract.
        Ok(())
    }
}
