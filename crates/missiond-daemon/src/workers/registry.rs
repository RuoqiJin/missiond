//! Worker Registry — runtime control for background workers.
//!
//! Dual-mode pause evaluation:
//! - **Legacy**: per-worker `watch<WorkerState>` (for MCP `mission_worker control pause X`)
//! - **ControlTree**: centralized `watch<ControlTree>` with cascade dependencies
//!
//! A worker is considered paused if EITHER source says so (OR semantics).
//! Workers call `ctx.wait_if_paused()` at their loop boundary.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;
use sqlx::PgPool;
use tokio::sync::watch;
use tracing::info;

use crate::control_tree::{ControlTree, Dependency};

/// Worker lifecycle state (legacy per-worker control).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkerState {
    Running,
    Paused,
}

/// Runtime lifecycle exposed to operators. This is intentionally separate
/// from the legacy pause control state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkerLifecycleState {
    Idle,
    Running,
    Blocked,
    Retrying,
    Failed,
    Completed,
    Paused,
}

impl WorkerLifecycleState {
    fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Blocked | Self::Retrying)
    }
}

const WORKER_STALE_AFTER_SECS: i64 = 120;

/// Global registry — lives on AppState, manages all worker handles.
pub struct WorkerRegistry {
    workers: Mutex<HashMap<String, Arc<WorkerHandle>>>,
    persistence: Arc<Mutex<Option<PgPool>>>,
}

/// External control handle (held by registry, used by MCP tools).
pub struct WorkerHandle {
    pub name: String,
    state_tx: watch::Sender<WorkerState>,
    pub tasks_processed: AtomicU64,
    pub tasks_failed: AtomicU64,
    pub last_active_at: AtomicI64,
    pub last_heartbeat_at: AtomicI64,
    runtime: Mutex<WorkerRuntime>,
    persistence: Arc<Mutex<Option<PgPool>>>,
}

/// Internal context (held by worker, used inside its run loop).
pub struct WorkerContext {
    name: String,
    deps: Vec<Dependency>,
    legacy_rx: watch::Receiver<WorkerState>,
    tree_rx: watch::Receiver<ControlTree>,
    handle: Arc<WorkerHandle>,
}

#[derive(Debug, Clone)]
struct WorkerRuntime {
    lifecycle: WorkerLifecycleState,
    current_task_id: Option<String>,
    current_slot_id: Option<String>,
    last_progress_at: i64,
    last_error: Option<String>,
    lease_expires_at: Option<i64>,
    status: Option<String>,
}

impl Default for WorkerRuntime {
    fn default() -> Self {
        Self {
            lifecycle: WorkerLifecycleState::Idle,
            current_task_id: None,
            current_slot_id: None,
            last_progress_at: 0,
            last_error: None,
            lease_expires_at: None,
            status: None,
        }
    }
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

// ── Registry ──────────────────────────────────────────────────────

impl WorkerRegistry {
    pub fn new() -> Self {
        Self {
            workers: Mutex::new(HashMap::new()),
            persistence: Arc::new(Mutex::new(None)),
        }
    }

    pub fn attach_persistence(&self, pool: PgPool) {
        *self.persistence.lock().unwrap() = Some(pool);
    }

    /// Register a worker with ControlTree dependencies.
    /// Returns a WorkerContext that evaluates both legacy state AND ControlTree.
    pub fn register_with_deps(
        &self,
        name: &str,
        deps: Vec<Dependency>,
        tree_rx: watch::Receiver<ControlTree>,
    ) -> WorkerContext {
        let (tx, rx) = watch::channel(WorkerState::Running);
        let handle = Arc::new(WorkerHandle {
            name: name.to_string(),
            state_tx: tx,
            tasks_processed: AtomicU64::new(0),
            tasks_failed: AtomicU64::new(0),
            last_active_at: AtomicI64::new(now_epoch()),
            last_heartbeat_at: AtomicI64::new(now_epoch()),
            runtime: Mutex::new(WorkerRuntime::default()),
            persistence: Arc::clone(&self.persistence),
        });
        self.workers
            .lock()
            .unwrap()
            .insert(name.to_string(), Arc::clone(&handle));
        WorkerContext {
            name: name.to_string(),
            deps,
            legacy_rx: rx,
            tree_rx,
            handle,
        }
    }

    /// Legacy register (no ControlTree deps). For workers that don't call LLMs.
    pub fn register(&self, name: &str, tree_rx: watch::Receiver<ControlTree>) -> WorkerContext {
        self.register_with_deps(name, Vec::new(), tree_rx)
    }

    /// Get a handle for external control (MCP tools).
    pub fn get(&self, name: &str) -> Option<Arc<WorkerHandle>> {
        self.workers.lock().unwrap().get(name).cloned()
    }

    /// List all workers and their stats.
    pub fn list_all(&self) -> Vec<WorkerInfo> {
        let map = self.workers.lock().unwrap();
        let mut out: Vec<_> = map.values().map(|h| h.info_snapshot()).collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

/// Snapshot of a worker's state + stats (for MCP response).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerInfo {
    pub name: String,
    pub state: WorkerState,
    pub tasks_processed: u64,
    pub tasks_failed: u64,
    pub last_active_at: i64,
    pub health: WorkerHealthSnapshot,
}

/// Structured worker health used by mission_worker, mission_health, Board,
/// and deterministic runbook generation.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerHealthSnapshot {
    pub name: String,
    pub lifecycle: WorkerLifecycleState,
    pub paused: bool,
    pub effective_paused: bool,
    pub tasks_processed: u64,
    pub tasks_failed: u64,
    pub last_active_at: i64,
    pub last_heartbeat_at: i64,
    pub last_progress_at: i64,
    pub current_task_id: Option<String>,
    pub current_slot_id: Option<String>,
    pub last_error: Option<String>,
    pub lease_expires_at: Option<i64>,
    pub status: Option<String>,
    pub stale: bool,
    pub stale_reason: Option<String>,
    pub heartbeat_age_secs: i64,
}

// ── Handle (external control) ─────────────────────────────────────

impl WorkerHandle {
    pub fn set_state(&self, state: WorkerState) {
        let _ = self.state_tx.send(state);
        {
            let mut runtime = self.runtime.lock().unwrap();
            runtime.lifecycle = match state {
                WorkerState::Running => WorkerLifecycleState::Idle,
                WorkerState::Paused => WorkerLifecycleState::Paused,
            };
            runtime.status = Some(match state {
                WorkerState::Running => "resumed".to_string(),
                WorkerState::Paused => "paused".to_string(),
            });
            runtime.last_progress_at = now_epoch();
        }
        self.last_heartbeat_at.store(now_epoch(), Ordering::Relaxed);
        self.persist_snapshot();
        info!(worker = %self.name, ?state, "Worker state changed");
    }

    pub fn current_state(&self) -> WorkerState {
        *self.state_tx.borrow()
    }

    fn info_snapshot(&self) -> WorkerInfo {
        let state = self.current_state();
        let tasks_processed = self.tasks_processed.load(Ordering::Relaxed);
        let tasks_failed = self.tasks_failed.load(Ordering::Relaxed);
        let last_active_at = self.last_active_at.load(Ordering::Relaxed);
        let health = self.health_snapshot();
        WorkerInfo {
            name: self.name.clone(),
            state,
            tasks_processed,
            tasks_failed,
            last_active_at,
            health,
        }
    }

    fn health_snapshot(&self) -> WorkerHealthSnapshot {
        let now = now_epoch();
        let state = self.current_state();
        let runtime = self.runtime.lock().unwrap().clone();
        let last_heartbeat_at = self.last_heartbeat_at.load(Ordering::Relaxed);
        let heartbeat_age_secs = now.saturating_sub(last_heartbeat_at);
        let lease_expired = runtime
            .lease_expires_at
            .map(|deadline| deadline > 0 && now > deadline)
            .unwrap_or(false);
        let effective_paused =
            state == WorkerState::Paused || runtime.lifecycle == WorkerLifecycleState::Paused;
        let stale_reason = if !effective_paused && lease_expired {
            Some("lease-expired".to_string())
        } else if !effective_paused
            && runtime.lifecycle.is_active()
            && heartbeat_age_secs > WORKER_STALE_AFTER_SECS
        {
            Some("heartbeat-stale".to_string())
        } else {
            None
        };
        WorkerHealthSnapshot {
            name: self.name.clone(),
            lifecycle: runtime.lifecycle,
            paused: state == WorkerState::Paused,
            effective_paused,
            tasks_processed: self.tasks_processed.load(Ordering::Relaxed),
            tasks_failed: self.tasks_failed.load(Ordering::Relaxed),
            last_active_at: self.last_active_at.load(Ordering::Relaxed),
            last_heartbeat_at,
            last_progress_at: runtime.last_progress_at,
            current_task_id: runtime.current_task_id,
            current_slot_id: runtime.current_slot_id,
            last_error: runtime.last_error,
            lease_expires_at: runtime.lease_expires_at,
            status: runtime.status,
            stale: stale_reason.is_some(),
            stale_reason,
            heartbeat_age_secs,
        }
    }

    fn persist_snapshot(&self) {
        let Some(pool) = self.persistence.lock().unwrap().clone() else {
            return;
        };
        let health = self.health_snapshot();
        tokio::spawn(async move {
            let current = json!({
                "taskId": health.current_task_id,
                "slotId": health.current_slot_id,
                "status": health.status,
            });
            let _ = sqlx::query(
                r#"
                INSERT INTO worker_runtime_state
                  (worker_name, lifecycle, current_task_id, current_slot_id,
                   last_heartbeat_at, last_progress_at, last_error, lease_expires_at,
                   tasks_processed, tasks_failed, current, stale, stale_reason, updated_at)
                VALUES (
                  $1, $2, $3, $4,
                  to_timestamp($5), NULLIF(to_timestamp($6), to_timestamp(0)),
                  $7, CASE WHEN $8::bigint IS NULL THEN NULL ELSE to_timestamp($8) END,
                  $9, $10, $11, $12, $13, now()
                )
                ON CONFLICT(worker_name) DO UPDATE SET
                  lifecycle = EXCLUDED.lifecycle,
                  current_task_id = EXCLUDED.current_task_id,
                  current_slot_id = EXCLUDED.current_slot_id,
                  last_heartbeat_at = EXCLUDED.last_heartbeat_at,
                  last_progress_at = EXCLUDED.last_progress_at,
                  last_error = EXCLUDED.last_error,
                  lease_expires_at = EXCLUDED.lease_expires_at,
                  tasks_processed = EXCLUDED.tasks_processed,
                  tasks_failed = EXCLUDED.tasks_failed,
                  current = EXCLUDED.current,
                  stale = EXCLUDED.stale,
                  stale_reason = EXCLUDED.stale_reason,
                  updated_at = now()
                "#,
            )
            .bind(&health.name)
            .bind(format!("{:?}", health.lifecycle).to_lowercase())
            .bind(&health.current_task_id)
            .bind(&health.current_slot_id)
            .bind(health.last_heartbeat_at)
            .bind(health.last_progress_at)
            .bind(&health.last_error)
            .bind(health.lease_expires_at)
            .bind(health.tasks_processed as i64)
            .bind(health.tasks_failed as i64)
            .bind(current)
            .bind(health.stale)
            .bind(&health.stale_reason)
            .execute(&pool)
            .await;
        });
    }
}

// ── Context (worker-internal) ─────────────────────────────────────

impl WorkerContext {
    /// Is this worker effectively paused? Combines legacy + ControlTree.
    fn is_effectively_paused(&self) -> bool {
        // Legacy per-worker pause
        if *self.legacy_rx.borrow() == WorkerState::Paused {
            return true;
        }
        // ControlTree cascade
        let tree = self.tree_rx.borrow();
        tree.is_effectively_paused(&self.name, &self.deps)
    }

    /// Block until the worker should run. Call at the top of the main loop.
    /// Wakes on EITHER legacy state change OR ControlTree change.
    /// P2 fix: no `biased` — fair scheduling between legacy and tree channels.
    pub async fn wait_if_paused(&mut self) {
        self.heartbeat("loop-ready");
        if !self.is_effectively_paused() {
            return;
        }
        self.set_lifecycle(
            WorkerLifecycleState::Paused,
            Some("paused by worker control tree".to_string()),
            None,
        );
        info!(worker = %self.name, "Worker paused (cascade), waiting to resume...");
        loop {
            tokio::select! {
                res = self.legacy_rx.changed() => {
                    if res.is_err() { return; }
                }
                res = self.tree_rx.changed() => {
                    if res.is_err() { return; }
                }
            }
            if !self.is_effectively_paused() {
                self.set_lifecycle(
                    WorkerLifecycleState::Idle,
                    Some("resumed".to_string()),
                    None,
                );
                info!(worker = %self.name, "Worker resumed");
                return;
            }
        }
    }

    /// P0 fix (Gemini audit): resolves ONLY when this worker's effective state
    /// transitions to paused. Prevents thundering herd — unrelated ControlTree
    /// mutations (e.g., pausing Opus) won't wake a Sonnet-dependent worker.
    pub async fn wait_until_paused(&mut self) {
        if self.is_effectively_paused() {
            return;
        }
        loop {
            tokio::select! {
                res = self.legacy_rx.changed() => {
                    if res.is_err() { std::future::pending::<()>().await; }
                }
                res = self.tree_rx.changed() => {
                    if res.is_err() { std::future::pending::<()>().await; }
                }
            }
            if self.is_effectively_paused() {
                self.set_lifecycle(
                    WorkerLifecycleState::Paused,
                    Some("paused by worker control tree".to_string()),
                    None,
                );
                return;
            }
            // Spurious wakeup (unrelated change) — keep waiting
        }
    }

    /// Record a successful task completion.
    pub fn record_success(&self) {
        self.complete("task completed");
    }

    /// Record a failed task.
    pub fn record_failure(&self) {
        self.fail("task failed");
    }

    /// Check if currently paused (non-blocking).
    pub fn is_paused(&self) -> bool {
        self.is_effectively_paused()
    }

    pub fn heartbeat(&self, status: impl Into<String>) {
        let now = now_epoch();
        self.handle.last_heartbeat_at.store(now, Ordering::Relaxed);
        {
            let mut runtime = self.handle.runtime.lock().unwrap();
            runtime.status = Some(status.into());
        }
        self.handle.persist_snapshot();
    }

    pub fn begin_task(
        &self,
        task_id: Option<String>,
        slot_id: Option<String>,
        lease_secs: Option<i64>,
    ) {
        let now = now_epoch();
        {
            let mut runtime = self.handle.runtime.lock().unwrap();
            runtime.lifecycle = WorkerLifecycleState::Running;
            runtime.current_task_id = task_id;
            runtime.current_slot_id = slot_id;
            runtime.last_progress_at = now;
            runtime.last_error = None;
            runtime.lease_expires_at = lease_secs.map(|secs| now.saturating_add(secs.max(1)));
            runtime.status = Some("running".to_string());
        }
        self.handle.last_active_at.store(now, Ordering::Relaxed);
        self.handle.last_heartbeat_at.store(now, Ordering::Relaxed);
        self.handle.persist_snapshot();
    }

    pub fn progress(&self, status: impl Into<String>) {
        let now = now_epoch();
        {
            let mut runtime = self.handle.runtime.lock().unwrap();
            runtime.last_progress_at = now;
            runtime.status = Some(status.into());
        }
        self.handle.last_active_at.store(now, Ordering::Relaxed);
        self.handle.last_heartbeat_at.store(now, Ordering::Relaxed);
        self.handle.persist_snapshot();
    }

    pub fn block(&self, reason: impl Into<String>) {
        let reason = reason.into();
        self.set_lifecycle(
            WorkerLifecycleState::Blocked,
            Some(format!("blocked: {reason}")),
            Some(reason),
        );
    }

    pub fn retrying(&self, reason: impl Into<String>) {
        let reason = reason.into();
        self.set_lifecycle(
            WorkerLifecycleState::Retrying,
            Some(format!("retrying: {reason}")),
            Some(reason),
        );
    }

    pub fn fail(&self, error: impl Into<String>) {
        let error = error.into();
        self.handle.tasks_failed.fetch_add(1, Ordering::Relaxed);
        self.set_lifecycle(
            WorkerLifecycleState::Failed,
            Some(format!("failed: {error}")),
            Some(error),
        );
    }

    pub fn complete(&self, status: impl Into<String>) {
        self.handle.tasks_processed.fetch_add(1, Ordering::Relaxed);
        let now = now_epoch();
        {
            let mut runtime = self.handle.runtime.lock().unwrap();
            runtime.lifecycle = WorkerLifecycleState::Completed;
            runtime.current_task_id = None;
            runtime.current_slot_id = None;
            runtime.last_progress_at = now;
            runtime.last_error = None;
            runtime.lease_expires_at = None;
            runtime.status = Some(status.into());
        }
        self.handle.last_active_at.store(now, Ordering::Relaxed);
        self.handle.last_heartbeat_at.store(now, Ordering::Relaxed);
        self.handle.persist_snapshot();
    }

    fn set_lifecycle(
        &self,
        lifecycle: WorkerLifecycleState,
        status: Option<String>,
        error: Option<String>,
    ) {
        let now = now_epoch();
        {
            let mut runtime = self.handle.runtime.lock().unwrap();
            runtime.lifecycle = lifecycle;
            runtime.last_progress_at = now;
            runtime.status = status;
            runtime.last_error = error;
        }
        self.handle.last_active_at.store(now, Ordering::Relaxed);
        self.handle.last_heartbeat_at.store(now, Ordering::Relaxed);
        self.handle.persist_snapshot();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_tree::ControlTree;

    #[test]
    fn worker_health_reports_lifecycle_and_stale_state() {
        let registry = WorkerRegistry::new();
        let (_tx, rx) = watch::channel(ControlTree::default());
        let ctx = registry.register("worker-a", rx);

        ctx.begin_task(Some("task-1".into()), Some("slot-a".into()), Some(60));
        let info = registry.list_all().remove(0);
        assert_eq!(info.health.lifecycle, WorkerLifecycleState::Running);
        assert_eq!(info.health.current_task_id.as_deref(), Some("task-1"));
        assert!(!info.health.stale);

        ctx.handle.last_heartbeat_at.store(
            now_epoch().saturating_sub(WORKER_STALE_AFTER_SECS + 1),
            Ordering::Relaxed,
        );
        let info = registry.list_all().remove(0);
        assert!(info.health.stale);
        assert_eq!(info.health.stale_reason.as_deref(), Some("heartbeat-stale"));
    }

    #[test]
    fn record_success_keeps_legacy_counter_and_sets_completed() {
        let registry = WorkerRegistry::new();
        let (_tx, rx) = watch::channel(ControlTree::default());
        let ctx = registry.register("worker-b", rx);

        ctx.begin_task(Some("task-2".into()), None, None);
        ctx.record_success();

        let info = registry.list_all().remove(0);
        assert_eq!(info.tasks_processed, 1);
        assert_eq!(info.health.lifecycle, WorkerLifecycleState::Completed);
        assert!(info.health.current_task_id.is_none());
    }
}
