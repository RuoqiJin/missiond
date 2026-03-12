//! Worker Registry — runtime control for background workers.
//!
//! Provides cooperative pause/resume via `tokio::sync::watch` channels.
//! Workers call `ctx.wait_if_paused()` at their loop boundary;
//! external callers (MCP tools) use `handle.set_state()` to control them.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::watch;
use tracing::info;

/// Worker lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkerState {
    Running,
    Paused,
}

/// Global registry — lives on AppState, manages all worker handles.
pub struct WorkerRegistry {
    workers: Mutex<HashMap<String, Arc<WorkerHandle>>>,
}

/// External control handle (held by registry, used by MCP tools).
pub struct WorkerHandle {
    pub name: String,
    state_tx: watch::Sender<WorkerState>,
    pub tasks_processed: AtomicU64,
    pub tasks_failed: AtomicU64,
    pub last_active_at: AtomicI64,
}

/// Internal context (held by worker, used inside its run loop).
pub struct WorkerContext {
    state_rx: watch::Receiver<WorkerState>,
    handle: Arc<WorkerHandle>,
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
        Self { workers: Mutex::new(HashMap::new()) }
    }

    /// Register a worker and return its context. Called by `spawn_worker`.
    pub fn register(&self, name: &str) -> WorkerContext {
        let (tx, rx) = watch::channel(WorkerState::Running);
        let handle = Arc::new(WorkerHandle {
            name: name.to_string(),
            state_tx: tx,
            tasks_processed: AtomicU64::new(0),
            tasks_failed: AtomicU64::new(0),
            last_active_at: AtomicI64::new(now_epoch()),
        });
        self.workers.lock().unwrap().insert(name.to_string(), Arc::clone(&handle));
        WorkerContext { state_rx: rx, handle }
    }

    /// Get a handle for external control (MCP tools).
    pub fn get(&self, name: &str) -> Option<Arc<WorkerHandle>> {
        self.workers.lock().unwrap().get(name).cloned()
    }

    /// List all workers and their stats.
    pub fn list_all(&self) -> Vec<WorkerInfo> {
        let map = self.workers.lock().unwrap();
        let mut out: Vec<_> = map.values().map(|h| WorkerInfo {
            name: h.name.clone(),
            state: *h.state_tx.borrow(),
            tasks_processed: h.tasks_processed.load(Ordering::Relaxed),
            tasks_failed: h.tasks_failed.load(Ordering::Relaxed),
            last_active_at: h.last_active_at.load(Ordering::Relaxed),
        }).collect();
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
}

// ── Handle (external control) ─────────────────────────────────────

impl WorkerHandle {
    pub fn set_state(&self, state: WorkerState) {
        let _ = self.state_tx.send(state);
        info!(worker = %self.name, ?state, "Worker state changed");
    }

    pub fn current_state(&self) -> WorkerState {
        *self.state_tx.borrow()
    }
}

// ── Context (worker-internal) ─────────────────────────────────────

impl WorkerContext {
    /// Block until state is Running. Call at the top of the worker's main loop.
    /// If already Running, returns immediately (zero cost).
    pub async fn wait_if_paused(&mut self) {
        if *self.state_rx.borrow_and_update() == WorkerState::Running {
            return;
        }
        info!(worker = %self.handle.name, "Worker paused, waiting to resume...");
        while self.state_rx.changed().await.is_ok() {
            if *self.state_rx.borrow_and_update() == WorkerState::Running {
                info!(worker = %self.handle.name, "Worker resumed");
                return;
            }
        }
    }

    /// For use inside `tokio::select!` — resolves when state changes (e.g. pause while sleeping).
    pub async fn state_changed(&mut self) {
        let _ = self.state_rx.changed().await;
    }

    /// Record a successful task completion.
    pub fn record_success(&self) {
        self.handle.tasks_processed.fetch_add(1, Ordering::Relaxed);
        self.handle.last_active_at.store(now_epoch(), Ordering::Relaxed);
    }

    /// Record a failed task.
    pub fn record_failure(&self) {
        self.handle.tasks_failed.fetch_add(1, Ordering::Relaxed);
        self.handle.last_active_at.store(now_epoch(), Ordering::Relaxed);
    }

    /// Check if currently paused (non-blocking).
    pub fn is_paused(&self) -> bool {
        *self.state_rx.borrow() == WorkerState::Paused
    }
}
