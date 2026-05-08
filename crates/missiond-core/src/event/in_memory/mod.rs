//! In-memory bus — production-semantics implementation of the full event
//! bus that runs without PostgreSQL.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.4 testing-story:
//!
//! > in-memory-bus must share the production semantics — single writer
//! > assigning seq, append-ack, seq-ordered replay. Not a raw AtomicU64.
//!
//! Use cases:
//!
//! 1. **Unit tests** — drive end-to-end producer → log → dispatcher →
//!    subscription flows without Docker / PG.
//! 2. **Chaos tests** — inject failures at each layer (log failed state,
//!    subscriber panic, slow consumer, …) via programmable hooks.
//! 3. **Boot fallback** — daemon can run in a degraded "in-memory" mode
//!    when PG is unavailable (not wired in Phase 5; Phase 8 task).
//!
//! # Composition
//!
//! ```text
//!  InMemoryBus
//!   ├── Arc<InMemoryLog>              — append + read
//!   ├── Arc<InMemoryBlobStore>        — claim-check
//!   ├── Arc<InMemoryCursorStore>      — subscription cursors
//!   ├── Arc<Dispatcher>               — topic fan-out (shared impl with PG)
//!   └── Arc<InMemoryControlGate>      — programmable pause for tests
//! ```
//!
//! Calling [`InMemoryBus::start`] spawns the dispatcher tail loop and
//! returns a [`InMemoryBusHandle`]; drop the handle to tear down.

pub mod blob_store;
pub mod control_gate;
pub mod cursor_store;
pub mod log;
pub mod observability;
pub mod storage;
pub mod writer_task;

pub use blob_store::InMemoryBlobStore;
pub use control_gate::InMemoryControlGate;
pub use cursor_store::InMemoryCursorStore;
pub use log::InMemoryLog;
pub use storage::{StoredRow, IN_MEMORY_APPEND_CAPACITY};

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::watch;

use super::blob_store::BlobStore;
use super::dispatcher::{
    register_all_domains, DispatchError, DispatchMetrics, Dispatcher, DispatcherBuilder, TailError,
    TailSource,
};
use super::log::reader::LoggedEvent;
use super::log::{Log, Seq};
use super::metrics::{AtomicBusMetrics, BusMetrics};

/// Aggregate in-memory bus — holds every subsystem so tests can interact
/// with any layer.
pub struct InMemoryBus {
    pub log: Arc<InMemoryLog>,
    pub blob_store: Arc<InMemoryBlobStore>,
    pub cursor_store: Arc<InMemoryCursorStore>,
    pub dispatcher: Dispatcher,
    pub control_gate: Arc<InMemoryControlGate>,
    pub metrics: Arc<AtomicBusMetrics>,
}

/// Handle returned by [`InMemoryBus::start`]. Drop = tear-down.
pub struct InMemoryBusHandle {
    shutdown_tx: watch::Sender<bool>,
    dispatcher_join: Option<tokio::task::JoinHandle<Result<DispatchMetrics, DispatchError>>>,
}

impl InMemoryBusHandle {
    /// Wait for the dispatcher loop to finish (returns its final metrics).
    pub async fn join(mut self) -> Result<DispatchMetrics, DispatchError> {
        let _ = self.shutdown_tx.send(true);
        match self.dispatcher_join.take() {
            Some(h) => match h.await {
                Ok(result) => result,
                Err(e) => Err(DispatchError::Tail(TailError::LogRead(format!(
                    "dispatcher task panicked: {e}"
                )))),
            },
            None => Ok(Default::default()),
        }
    }

    /// Signal shutdown without awaiting. The loop finishes after its next
    /// poll iteration.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

impl Drop for InMemoryBusHandle {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
    }
}

impl InMemoryBus {
    /// Build a bus with default wiring — no metrics, default dispatcher
    /// builder covering every entry in `Domain::ALL`.
    pub async fn new() -> Self {
        let metrics = Arc::new(AtomicBusMetrics::new());
        let metrics_dyn: Arc<dyn BusMetrics> = metrics.clone();

        let blob_store = Arc::new(InMemoryBlobStore::new());
        let blob_dyn: Arc<dyn BlobStore> = blob_store.clone();

        let log = InMemoryLog::spawn(blob_dyn, metrics_dyn.clone());
        let cursor_store = Arc::new(InMemoryCursorStore::new());
        let control_gate = Arc::new(InMemoryControlGate::new());
        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();

        Self {
            log,
            blob_store,
            cursor_store,
            dispatcher,
            control_gate,
            metrics,
        }
    }

    /// Start the dispatcher tail loop and return a handle for shutdown.
    pub async fn start(&self) -> InMemoryBusHandle {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let tail_source: Arc<dyn TailSource> = Arc::new(InMemoryTailSource::new(self.log.clone()));
        let dispatcher = self.dispatcher.clone();
        let gate = ArcControlGate::new(self.control_gate.clone());
        let blob: Arc<dyn BlobStore> = self.blob_store.clone();

        let join =
            tokio::spawn(async move { dispatcher.run(tail_source, blob, gate, shutdown_rx).await });

        InMemoryBusHandle {
            shutdown_tx,
            dispatcher_join: Some(join),
        }
    }

    /// Convenience: append an event through the log.
    pub async fn append<E>(
        &self,
        event: E,
        opts: super::log::AppendOpts,
    ) -> Result<super::log::AppendAck, super::log::AppendError>
    where
        E: super::event_trait::DomainEvent,
    {
        self.log.append(event, opts).await
    }
}

/// [`TailSource`] adapter for [`InMemoryLog`]. Scans all rows across every
/// domain in seq order — matches the PG `SELECT WHERE seq > $1` contract.
pub struct InMemoryTailSource {
    log: Arc<InMemoryLog>,
}

impl InMemoryTailSource {
    pub fn new(log: Arc<InMemoryLog>) -> Self {
        Self { log }
    }
}

#[async_trait]
impl TailSource for InMemoryTailSource {
    async fn read_all_from(
        &self,
        after_seq: Seq,
        limit: usize,
    ) -> Result<Vec<LoggedEvent>, TailError> {
        // Iterate all domains and filter; identical to what the PG version
        // does with SQL but cheaper with an in-memory vec.
        Ok(read_all_since(&self.log, after_seq, limit))
    }
}

/// All-domain tail read helper — mirrors PG dispatcher's cross-domain
/// SELECT. Kept as a free function (not on `Log`, which stays per-domain).
fn read_all_since(log: &InMemoryLog, after_seq: Seq, limit: usize) -> Vec<LoggedEvent> {
    let snapshot = log.rows_snapshot();
    let mut out = Vec::new();
    for r in snapshot.iter() {
        if r.seq > after_seq {
            out.push(crate::event::in_memory::log::stored_to_logged_pub(r));
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

// ControlGate wrapper — the dispatcher wants a `G: ControlGate + Clone`,
// so we manually implement Clone over Arc.
#[derive(Clone)]
struct ArcControlGate {
    inner: Arc<InMemoryControlGate>,
}

impl ArcControlGate {
    fn new(inner: Arc<InMemoryControlGate>) -> Self {
        Self { inner }
    }
}

impl super::dispatcher::ControlGate for ArcControlGate {
    fn is_ctl_domain_paused(&self, d: super::dispatcher::control_gate::CtlDomain) -> bool {
        self.inner.is_ctl_domain_paused(d)
    }
}

// Clone impl for Arc<InMemoryControlGate> indirectly — dispatcher needs
// `Clone`, and `Arc<InMemoryControlGate>` would satisfy it via the
// existing `impl<T: ControlGate + ?Sized> ControlGate for Arc<T>`. Wrap
// the Arc so the existing trait impls compose.

/// Helper to wait until the dispatcher drains a specific seq. Tests use this
/// instead of `sleep`.
pub async fn await_cursor(dispatcher: &Dispatcher, target: Seq, max: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < max {
        if dispatcher.last_dispatched_seq() >= target {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::events::BoardEvent;
    use crate::event::log::AppendOpts;

    fn board(i: usize) -> BoardEvent {
        BoardEvent::TaskCreated {
            task_id: format!("t-{i}"),
            title: "t".into(),
            category: "c".into(),
        }
    }

    #[tokio::test]
    async fn new_bus_has_fresh_state() {
        let bus = InMemoryBus::new().await;
        assert_eq!(bus.log.head_seq().await.unwrap(), Seq(0));
        assert_eq!(bus.log.row_count(), 0);
    }

    #[tokio::test]
    async fn append_and_read_roundtrip() {
        let bus = InMemoryBus::new().await;
        for i in 1..=5 {
            bus.append(
                board(i),
                AppendOpts {
                    producer_id: "p".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        assert_eq!(bus.log.head_seq().await.unwrap(), Seq(5));
        let got = bus
            .log
            .read_from(super::super::Domain::Board, Seq(0), 100)
            .await
            .unwrap();
        assert_eq!(got.len(), 5);
    }

    #[tokio::test]
    async fn start_then_shutdown_cycles_cleanly() {
        let bus = InMemoryBus::new().await;
        let handle = bus.start().await;
        // Tiny delay so the tail loop gets at least one tick.
        tokio::time::sleep(Duration::from_millis(30)).await;
        let result = handle.join().await;
        assert!(
            matches!(result, Err(DispatchError::Shutdown) | Ok(_)),
            "unexpected join result {result:?}"
        );
    }

    #[tokio::test]
    async fn dispatcher_sees_appended_events() {
        let bus = InMemoryBus::new().await;
        let mut rx = bus.dispatcher.topic::<BoardEvent>().subscribe();
        let handle = bus.start().await;

        for i in 1..=3 {
            bus.append(
                board(i),
                AppendOpts {
                    producer_id: "p".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        // Expect 3 events on the live topic within a reasonable window.
        let mut got = 0usize;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while got < 3 && tokio::time::Instant::now() < deadline {
            if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                got += 1;
            }
        }
        assert_eq!(got, 3);

        let _ = handle.join().await;
    }
}
