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
impl super::traits::InfraStore for SqliteMissionStore {
    // Stage 2B.1 placeholder. Methods will land in Stage 2D (PG only).
}

// ============================================================================
// DirectiveLayerStore — fail-fast stub (SQLite deprecated; pillar 五 is PG-only)
// ============================================================================

#[async_trait]
impl super::traits::DirectiveLayerStore for SqliteMissionStore {
    async fn directive_insert(
        &self,
        _utterance_text: &str,
        _sexp_text: &str,
        _version: i32,
        _status: crate::types::DirectiveStatus,
        _compiler_model: Option<&str>,
        _references_json: Option<&serde_json::Value>,
    ) -> DbResult<uuid::Uuid> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }

    async fn directive_get(
        &self,
        _id: uuid::Uuid,
        _version: i32,
    ) -> DbResult<Option<crate::types::Directive>> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }

    async fn directive_update_status(
        &self,
        _id: uuid::Uuid,
        _version: i32,
        _new_status: crate::types::DirectiveStatus,
    ) -> DbResult<()> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }

    async fn directive_approve(&self, _id: uuid::Uuid, _version: i32) -> DbResult<()> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }

    async fn directive_list_by_status(
        &self,
        _status: crate::types::DirectiveStatus,
        _limit: i64,
    ) -> DbResult<Vec<crate::types::Directive>> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }

    async fn directive_get_version_chain(
        &self,
        _id: uuid::Uuid,
    ) -> DbResult<Vec<crate::types::Directive>> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }

    async fn plan_insert(
        &self,
        _board_task_id: &str,
        _source_directive_id: Option<uuid::Uuid>,
        _version: i32,
        _sexp_text: &str,
        _sexp_hash: &str,
        _status: crate::types::PlanStatus,
        _compiler_model: Option<&str>,
        _compiled_from: Option<&str>,
    ) -> DbResult<uuid::Uuid> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }

    async fn plan_get(&self, _id: uuid::Uuid) -> DbResult<Option<crate::types::Plan>> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }

    async fn plan_update_status(
        &self,
        _id: uuid::Uuid,
        _new_status: crate::types::PlanStatus,
    ) -> DbResult<()> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }

    async fn plan_supersede(&self, _old_id: uuid::Uuid, _new_id: uuid::Uuid) -> DbResult<()> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }

    async fn plan_list_by_task(&self, _board_task_id: &str) -> DbResult<Vec<crate::types::Plan>> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }

    async fn plan_get_latest(
        &self,
        _board_task_id: &str,
    ) -> DbResult<Option<crate::types::Plan>> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }

    async fn workflow_insert(
        &self,
        _name: &str,
        _sexp_text: &str,
        _match_rules: &serde_json::Value,
        _learned_from: Option<uuid::Uuid>,
    ) -> DbResult<uuid::Uuid> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }

    async fn workflow_get_by_name(
        &self,
        _name: &str,
    ) -> DbResult<Option<crate::types::Workflow>> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }

    async fn workflow_find_by_match(
        &self,
        _query_utterance: &str,
    ) -> DbResult<Vec<crate::types::Workflow>> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }

    async fn workflow_record_execution(
        &self,
        _id: uuid::Uuid,
        _success: bool,
        _cost_usd: Option<f64>,
    ) -> DbResult<()> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }

    async fn workflow_list_top_n(&self, _n: i64) -> DbResult<Vec<crate::types::Workflow>> {
        Err(super::super::error::DbError::Other(
            "DirectiveLayerStore not supported on SQLite (use PostgreSQL)".to_string(),
        ))
    }
}

// ProjectStore impl moved to sqlite/skill.rs (merged with skill_* methods per
// memory pillar v0.4.23 — single impl per trait coherence rule).
