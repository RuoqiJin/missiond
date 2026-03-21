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
    name: String,
    deps: Vec<Dependency>,
    legacy_rx: watch::Receiver<WorkerState>,
    tree_rx: watch::Receiver<ControlTree>,
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
        Self {
            workers: Mutex::new(HashMap::new()),
        }
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
        let mut out: Vec<_> = map
            .values()
            .map(|h| WorkerInfo {
                name: h.name.clone(),
                state: *h.state_tx.borrow(),
                tasks_processed: h.tasks_processed.load(Ordering::Relaxed),
                tasks_failed: h.tasks_failed.load(Ordering::Relaxed),
                last_active_at: h.last_active_at.load(Ordering::Relaxed),
            })
            .collect();
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
        if !self.is_effectively_paused() {
            return;
        }
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
                return;
            }
            // Spurious wakeup (unrelated change) — keep waiting
        }
    }

    /// Record a successful task completion.
    pub fn record_success(&self) {
        self.handle.tasks_processed.fetch_add(1, Ordering::Relaxed);
        self.handle
            .last_active_at
            .store(now_epoch(), Ordering::Relaxed);
    }

    /// Record a failed task.
    pub fn record_failure(&self) {
        self.handle.tasks_failed.fetch_add(1, Ordering::Relaxed);
        self.handle
            .last_active_at
            .store(now_epoch(), Ordering::Relaxed);
    }

    /// Check if currently paused (non-blocking).
    pub fn is_paused(&self) -> bool {
        self.is_effectively_paused()
    }
}
