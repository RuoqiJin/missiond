// Background workers — organized by LLM dependency.
// Directory structure is the contract: sonnet/, codex/, gemini/, local/.
//
// - sonnet/  → Sonnet API (via SonnetGateway)
// - codex/   → Claude Code / Codex CLI (via SlotManager PTY)
// - gemini/  → Gemini CLI (via SlotManager PTY)
// - local/   → Pure computation, no LLM dependency
pub mod codex;
pub mod gemini;
pub mod local;
pub mod registry;
pub mod sonnet;

use std::future::Future;
use std::sync::Arc;
use tracing::info;

use crate::control_tree::{CtlProvider, Dependency};
use crate::state::AppState;
pub use registry::{WorkerContext, WorkerRegistry};

/// Worker LLM dependency category — directory = contract.
/// Determines which ControlTree provider deps are auto-injected by `spawn_worker`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerKind {
    /// Calls Sonnet API (via SonnetGateway)
    Sonnet,
    /// Calls Claude Code / Codex CLI (via SlotManager PTY)
    Codex,
    /// Calls Gemini CLI (via SlotManager PTY)
    Gemini,
    /// Pure computation — no LLM dependency
    Local,
}

/// Unified lifecycle trait for background workers.
///
/// Workers implement `run()` which contains their main loop.
/// Shutdown is handled externally by `spawn_worker()` via `tokio::select!`
/// — when the shutdown signal fires, the worker's future is dropped.
///
/// `const KIND` declares the LLM dependency (must match the directory).
/// `spawn_worker` auto-injects ControlTree deps from KIND.
/// `extra_deps()` adds domain-level deps (e.g. Memory for embedding).
pub(crate) trait BackgroundWorker: Send + 'static {
    /// LLM dependency category. Must match the directory the worker lives in.
    const KIND: WorkerKind;

    /// Human-readable name for logging.
    fn name(&self) -> &'static str;

    /// Additional domain dependencies beyond the auto-injected provider dep.
    /// Override only when needed (e.g. embedding_worker adds Domain::Memory).
    fn extra_deps(&self) -> Vec<Dependency> {
        Vec::new()
    }

    /// Run the worker's main loop. This future runs until natural completion
    /// or is cancelled by the shutdown signal in `spawn_worker`.
    fn run(self, state: Arc<AppState>, ctx: WorkerContext) -> impl Future<Output = ()> + Send;
}

/// Spawn a background worker with unified lifecycle management.
///
/// - Auto-injects ControlTree provider deps from `W::KIND`
/// - Merges with `extra_deps()` for domain-level deps
/// - Registers the worker in the global WorkerRegistry
/// - Logs start/stop with worker name
/// - Integrates with the shutdown watch channel for graceful termination
pub(crate) fn spawn_worker<W: BackgroundWorker>(
    worker: W,
    state: Arc<AppState>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    let name = worker.name();
    let mut deps = match W::KIND {
        WorkerKind::Sonnet => vec![Dependency::Provider(CtlProvider::Sonnet)],
        WorkerKind::Codex => vec![Dependency::Provider(CtlProvider::Codex)],
        WorkerKind::Gemini => vec![Dependency::Provider(CtlProvider::Gemini)],
        WorkerKind::Local => vec![],
    };
    deps.extend(worker.extra_deps());
    let tree_rx = state.control_manager.subscribe();
    let ctx = state
        .worker_registry
        .register_with_deps(name, deps, tree_rx);
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
