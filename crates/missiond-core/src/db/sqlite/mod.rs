//! SQLite backend for MissionStore traits.
//!
//! Wraps the existing synchronous `MissionDB` via `DbExecutor::run()` (spawn_blocking)
//! to provide async trait implementations that preserve slow-query monitoring.

// Note: tool_call/event/retrospective merged into conversation (v0.4.23).
// Note: vision merged into observability (v0.4.23 — Stage 2C.5).
mod conversation;
mod message;
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
    // Stage 2D: delegate to inherent MissionDB methods via DbExecutor.
    // watermarks + backfill + daemon_state migrated from ObservabilityStore/SlotStore.

    // ── consumer_watermarks ─────────────────────────────────────
    async fn watermark_get(&self, consumer: &str, session_id: &str) -> DbResult<Option<(Option<i64>, Option<String>)>> {
        let consumer = consumer.to_owned();
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.watermark_get(&consumer, &session_id)).await
    }

    async fn watermark_advance_time(&self, consumer: &str, session_id: &str, timestamp: &str) -> DbResult<()> {
        let consumer = consumer.to_owned();
        let session_id = session_id.to_owned();
        let timestamp = timestamp.to_owned();
        self.executor.run(move |db| db.watermark_advance_time(&consumer, &session_id, &timestamp)).await
    }

    async fn watermark_advance_msg_id(&self, consumer: &str, session_id: &str, msg_id: i64) -> DbResult<()> {
        let consumer = consumer.to_owned();
        let session_id = session_id.to_owned();
        self.executor.run(move |db| db.watermark_advance_msg_id(&consumer, &session_id, msg_id)).await
    }

    async fn watermark_advance_full(&self, consumer: &str, session_id: &str, msg_id: Option<i64>, timestamp: Option<&str>, extra: Option<&str>) -> DbResult<()> {
        let consumer = consumer.to_owned();
        let session_id = session_id.to_owned();
        let timestamp = timestamp.map(|s| s.to_owned());
        let extra = extra.map(|s| s.to_owned());
        self.executor.run(move |db| db.watermark_advance_full(&consumer, &session_id, msg_id, timestamp.as_deref(), extra.as_deref())).await
    }

    async fn watermark_list(&self, consumer: &str) -> DbResult<Vec<(String, Option<i64>, Option<String>)>> {
        let consumer = consumer.to_owned();
        self.executor.run(move |db| db.watermark_list(&consumer)).await
    }

    // ── watcher_cursors (no-op in SQLite mode) ───────────────────
    async fn load_watcher_cursors(&self) -> DbResult<std::collections::HashMap<String, u64>> {
        Ok(std::collections::HashMap::new())
    }
    async fn upsert_watcher_cursors_batch(&self, _cursors: &std::collections::HashMap<String, u64>) -> DbResult<()> {
        Ok(())
    }
    async fn delete_watcher_cursor(&self, _file_path: &str) -> DbResult<()> {
        Ok(())
    }

    // ── reconcile_watermarks (no-op in SQLite mode) ──────────────
    async fn get_reconcile_watermark(&self, _path: &str) -> DbResult<Option<i64>> {
        Ok(None)
    }
    async fn upsert_reconcile_watermark(&self, _path: &str, _size: i64) -> DbResult<()> {
        Ok(())
    }
    async fn get_all_reconcile_watermarks(&self) -> DbResult<std::collections::HashMap<String, i64>> {
        Ok(std::collections::HashMap::new())
    }

    // ── gemini_cli_watermarks (no-op in SQLite mode) ─────────────
    async fn load_gemini_cursors(&self) -> DbResult<std::collections::HashMap<String, i64>> {
        Ok(std::collections::HashMap::new())
    }
    async fn save_gemini_cursor(&self, _file_path: &str, _session_id: &str, _msg_count: i64) -> DbResult<()> {
        Ok(())
    }

    // ── backfill ────────────────────────────────────────────────
    async fn backfill_get_phase(&self, phase: &str) -> DbResult<Option<super::traits::BackfillPhaseStatus>> {
        let phase = phase.to_owned();
        self.executor.run(move |db| db.backfill_get_phase(&phase)).await
    }

    async fn backfill_list_phases(&self) -> DbResult<Vec<super::traits::BackfillPhaseStatus>> {
        self.executor.run(|db| db.backfill_list_phases()).await
    }

    async fn backfill_start_phase(&self, phase: &str, total_estimated: i64) -> DbResult<()> {
        let phase = phase.to_owned();
        self.executor.run(move |db| db.backfill_start_phase(&phase, total_estimated)).await
    }

    async fn backfill_update_progress(&self, phase: &str, new_cursor: i64, batch_success: i64, batch_failed: i64) -> DbResult<()> {
        let phase = phase.to_owned();
        self.executor.run(move |db| db.backfill_update_progress(&phase, new_cursor, batch_success, batch_failed)).await
    }

    async fn backfill_complete_phase(&self, phase: &str) -> DbResult<()> {
        let phase = phase.to_owned();
        self.executor.run(move |db| db.backfill_complete_phase(&phase)).await
    }

    async fn backfill_record_failure(&self, session_id: &str, phase: &str, error: &str) -> DbResult<()> {
        let session_id = session_id.to_owned();
        let phase = phase.to_owned();
        let error = error.to_owned();
        self.executor.run(move |db| db.backfill_record_failure(&session_id, &phase, &error)).await
    }

    async fn backfill_retryable_failures(&self, phase: &str, max_retries: i64, limit: i64) -> DbResult<Vec<String>> {
        let phase = phase.to_owned();
        self.executor.run(move |db| db.backfill_retryable_failures(&phase, max_retries, limit)).await
    }

    async fn backfill_retryable_failures_no_cooldown(&self, phase: &str, max_retries: i64) -> DbResult<i64> {
        let phase = phase.to_owned();
        self.executor.run(move |db| db.backfill_retryable_failures_no_cooldown(&phase, max_retries)).await
    }

    async fn backfill_clear_failure(&self, session_id: &str, phase: &str) -> DbResult<()> {
        let session_id = session_id.to_owned();
        let phase = phase.to_owned();
        self.executor.run(move |db| db.backfill_clear_failure(&session_id, &phase)).await
    }

    // ── daemon_state ────────────────────────────────────────────
    async fn daemon_state_get(&self, key: &str) -> DbResult<Option<i64>> {
        let key = key.to_owned();
        self.executor.run(move |db| db.daemon_state_get(&key)).await
    }

    async fn daemon_state_set(&self, key: &str, value: i64) -> DbResult<()> {
        let key = key.to_owned();
        self.executor.run(move |db| db.daemon_state_set(&key, value)).await
    }
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
