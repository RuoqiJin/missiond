// Background workers — each implements BackgroundWorker trait.
// Spawned via spawn_worker() in main.rs with unified lifecycle management.
pub mod embedding_worker;
pub mod vision_worker;
pub mod step_narrator;
pub mod translation_worker;
pub mod briefing_worker;
pub mod code_prefetch;
pub mod experience_harvester;
pub mod ast_sync_worker;
pub mod gemini_logger;
pub mod conversation_logger;
pub mod pty_event_worker;
pub mod retro_worker;
pub mod arch_maintenance_worker;
pub mod strategy_worker;
pub mod registry;

use std::future::Future;
use std::sync::Arc;
use tracing::info;

use crate::state::AppState;
pub use registry::{WorkerRegistry, WorkerContext};

/// Unified lifecycle trait for background workers.
///
/// Workers implement `run()` which contains their main loop.
/// Shutdown is handled externally by `spawn_worker()` via `tokio::select!`
/// — when the shutdown signal fires, the worker's future is dropped.
///
/// `WorkerContext` provides runtime pause/resume control:
/// - Call `ctx.wait_if_paused()` at the top of the main loop
/// - Use `ctx.record_success()` / `ctx.record_failure()` for stats
/// - Workers that don't need pause support can ignore `_ctx`
pub(crate) trait BackgroundWorker: Send + 'static {
    /// Human-readable name for logging.
    fn name(&self) -> &'static str;

    /// Run the worker's main loop. This future runs until natural completion
    /// or is cancelled by the shutdown signal in `spawn_worker`.
    fn run(self, state: Arc<AppState>, ctx: WorkerContext) -> impl Future<Output = ()> + Send;
}

/// Spawn a background worker with unified lifecycle management.
///
/// - Registers the worker in the global WorkerRegistry
/// - Logs start/stop with worker name
/// - Integrates with the shutdown watch channel for graceful termination
/// - Returns a JoinHandle for optional tracking
pub(crate) fn spawn_worker<W: BackgroundWorker>(
    worker: W,
    state: Arc<AppState>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    let name = worker.name();
    let ctx = state.worker_registry.register(name);
    tokio::spawn(async move {
        tokio::select! {
            biased;
            Ok(_) = shutdown.wait_for(|&v| v) => {
                info!(worker = name, "Background worker: shutdown");
            }
            _ = worker.run(state, ctx) => {
                info!(worker = name, "Background worker: completed");
            }
        }
    })
}
