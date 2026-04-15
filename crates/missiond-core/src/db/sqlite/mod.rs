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

#[async_trait]
impl super::traits::ProjectStore for SqliteMissionStore {
    async fn list_projects(&self) -> DbResult<Vec<crate::types::ProjectConfig>> {
        Ok(vec![])
    }
    async fn get_project(&self, _id: &str) -> DbResult<Option<crate::types::ProjectConfig>> {
        Ok(None)
    }
    async fn upsert_project(&self, _config: &crate::types::ProjectConfig) -> DbResult<()> {
        Ok(())
    }
    async fn set_project_active(&self, _id: &str, _active: bool) -> DbResult<bool> {
        Ok(false)
    }
    async fn backfill_project_id(&self, _project_id: &str, _path_pattern: &str) -> DbResult<u64> {
        Ok(0)
    }
    async fn conversation_stats_by_project(&self, _project_id: &str) -> DbResult<serde_json::Value> {
        Ok(serde_json::json!({"total": 0, "active": 0, "completed": 0, "last_activity": null}))
    }
    async fn recent_conversations_by_project(&self, _project_id: &str, _limit: i64) -> DbResult<Vec<serde_json::Value>> {
        Ok(vec![])
    }
    async fn kb_stats_by_project(&self, _project_id: &str) -> DbResult<serde_json::Value> {
        Ok(serde_json::json!({"total": 0, "by_category": {}}))
    }
}
