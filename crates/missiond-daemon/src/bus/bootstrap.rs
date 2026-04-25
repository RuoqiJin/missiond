//! `BusServices` — daemon-side aggregate that owns every event-bus subsystem.
//!
//! Construction is a single async call at daemon boot:
//!
//! ```ignore
//! let bus = BusServices::bootstrap(store.pool(), &control_manager).await?;
//! let handle = bus.start(shutdown_rx).await?;
//! state.bus = bus;
//! // later on shutdown:
//! handle.shutdown().await;
//! ```
//!
//! `BusServices` is cheap to `Arc::clone` around state; all inner fields are
//! already `Arc` so clone is a handful of atomic increments.
//!
//! Wiring decisions:
//!   * Postgres is the only backend wired today. SQLite is deprecated (see
//!     user memory `feedback_drop_sqlite`) and never gets an event_log table.
//!   * The control gate is adapted from `watch::Receiver<ControlTree>` via
//!     [`crate::bus::control_gate_adapter::ControlTreeGate`].
//!   * Metrics use the shared `AtomicBusMetrics`; Phase 8 can swap a
//!     Prometheus backend per DC025.
//!   * BlobStore defaults to PgBlobStore (DC006). If in the future we wire
//!     LocalFileBlobStore for >1 MB payloads, this module is the place.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use missiond_core::event::{
    blob_store::{BlobStore, PgBlobStore},
    dispatcher::{
        register_all_domains, DispatchError, DispatchMetrics, Dispatcher, DispatcherBuilder,
        TailSource,
    },
    events::{
        BoardEvent, ExecutionEvent, IncidentEvent, LlmEvent, MemoryEvent, MessageEvent,
        ObservabilityEvent, QuestionEvent, SessionEvent, SlotEvent, SystemEvent, TaskEvent,
        WorkerEvent,
    },
    log::{
        writer::spawn_log_writer, AppendAck, AppendError, AppendOpts, Log, LogWriterHandle, Seq,
    },
    metrics::{
        emitter::ObservabilityAppender, spawn_bus_metrics_emitter, AtomicBusMetrics,
        BusMetricsEmitterHandle,
    },
    subscription::{
        cursor_store::{CursorStore, PgCursorStore},
        failure::{DlqSink, PgDlqSink},
    },
    DomainEvent,
};
use sqlx::PgPool;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::bus::control_gate_adapter::ControlTreeGate;
use crate::control_tree::ControlManager;

/// Aggregate of every event-bus subsystem. Cheap to share via `Arc`.
pub struct BusServices {
    pub log: Arc<LogWriterHandle>,
    pub blob_store: Arc<dyn BlobStore>,
    pub cursor_store: Arc<dyn CursorStore>,
    pub dispatcher: Dispatcher,
    pub control_gate: ControlTreeGate,
    pub metrics: Arc<AtomicBusMetrics>,
    /// Dead-letter queue sink used by Phase 7 subscribers. Shared so every
    /// `bus.subscribe::<T>` call can hand the same backend to its
    /// `FailureRouter`.
    pub dlq: Arc<dyn DlqSink>,
    /// Tail source shared with the dispatcher loop. Stored so `start()` can
    /// spawn the tail task without re-constructing it.
    tail_source: Arc<dyn TailSource>,
    /// DB pool — kept around for retention / orphan-cleanup wiring in Phase 8.
    #[allow(dead_code)]
    pg_pool: PgPool,
}

/// Handle returned by [`BusServices::start`]. Dropping it signals shutdown.
pub struct BusStartHandle {
    shutdown_tx: watch::Sender<bool>,
    dispatcher_join: Option<JoinHandle<Result<DispatchMetrics, DispatchError>>>,
    metrics_handle: Option<BusMetricsEmitterHandle>,
}

impl BusStartHandle {
    /// Gracefully stop the dispatcher + metrics emitter and wait for the
    /// tasks to exit. Idempotent.
    pub async fn shutdown(mut self) -> Result<DispatchMetrics> {
        let _ = self.shutdown_tx.send(true);
        if let Some(m) = self.metrics_handle.take() {
            m.shutdown().await;
        }
        let metrics = match self.dispatcher_join.take() {
            Some(h) => match h.await {
                Ok(Ok(m)) => m,
                Ok(Err(DispatchError::Shutdown)) => DispatchMetrics::default(),
                Ok(Err(e)) => {
                    warn!(error = %e, "dispatcher loop exited with error");
                    DispatchMetrics::default()
                }
                Err(e) => {
                    warn!(error = %e, "dispatcher task panicked");
                    DispatchMetrics::default()
                }
            },
            None => DispatchMetrics::default(),
        };
        Ok(metrics)
    }
}

impl Drop for BusStartHandle {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
    }
}

impl BusServices {
    /// Build every subsystem. Safe to call on any tokio runtime — the
    /// `LogWriter` task is spawned here but the dispatcher tail + metrics
    /// emitter wait for [`Self::start`].
    pub async fn bootstrap(
        pool: PgPool,
        control_manager: &ControlManager,
    ) -> Result<Arc<Self>> {
        info!("bus: bootstrap starting");

        // Blob store (Postgres-backed per DC006).
        let blob_store: Arc<dyn BlobStore> = Arc::new(PgBlobStore::new(pool.clone()));

        // Log writer: one task drains the append channel.
        let log_handle = spawn_log_writer(pool.clone(), blob_store.clone());
        let log = Arc::new(log_handle);

        // Cursor store — used by Phase 4 subscribers (Phase 7 will wire them).
        let cursor_store: Arc<dyn CursorStore> = Arc::new(PgCursorStore::new(pool.clone()));

        // Dead-letter queue sink — Phase 7 subscribers route retry-exhausted
        // failures here instead of blocking the subscription forever.
        let dlq: Arc<dyn DlqSink> = Arc::new(PgDlqSink::new(pool.clone()));

        // Metrics collector — stays NoopMetrics-equivalent until we wire
        // counters inside the LogWriter; AtomicBusMetrics is cheap enough
        // to always instantiate.
        let metrics = Arc::new(AtomicBusMetrics::new());

        // Dispatcher — registers all 12 domains up front so `topic::<T>()`
        // never panics.
        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();

        // Control gate — adapter over the daemon's ControlManager.
        let control_gate = ControlTreeGate::new(control_manager.subscribe());

        // Tail source backing the dispatcher. Uses the Postgres tail reader.
        let tail_source: Arc<dyn TailSource> =
            Arc::new(missiond_core::event::dispatcher::PgTailSource::new(
                pool.clone(),
                blob_store.clone(),
            ));

        info!("bus: bootstrap complete");

        Ok(Arc::new(Self {
            log,
            blob_store,
            cursor_store,
            dispatcher,
            control_gate,
            metrics,
            dlq,
            tail_source,
            pg_pool: pool,
        }))
    }

    /// Start the dispatcher tail loop and the metrics emitter. Returns a
    /// handle that, when dropped, signals shutdown.
    pub async fn start(
        self: &Arc<Self>,
        external_shutdown: watch::Receiver<bool>,
    ) -> Result<BusStartHandle> {
        // Combined shutdown: triggered by either external shutdown or our
        // own handle drop.
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        spawn_shutdown_fuse(external_shutdown, shutdown_tx.clone());

        // Dispatcher loop.
        let dispatcher = self.dispatcher.clone();
        let tail_source = self.tail_source.clone();
        let blob = self.blob_store.clone();
        let gate = self.control_gate.clone();
        let dispatcher_join = tokio::spawn(async move {
            dispatcher.run(tail_source, blob, gate, shutdown_rx).await
        });

        // Metrics emitter — needs an ObservabilityAppender.
        let appender: Arc<dyn ObservabilityAppender> =
            Arc::new(LogObservabilityAdapter { log: self.log.clone() });
        let metrics_handle = spawn_bus_metrics_emitter(
            self.metrics.clone(),
            appender,
            missiond_core::event::metrics::emitter::METRICS_EMIT_INTERVAL,
        );

        Ok(BusStartHandle {
            shutdown_tx,
            dispatcher_join: Some(dispatcher_join),
            metrics_handle: Some(metrics_handle),
        })
    }

    // ── Publish convenience wrappers ───────────────────────────────────
    //
    // Each of these simply forwards to `Log::append` with the right
    // `AppendOpts`. Producer sites that don't have a dedupe_key or tracing
    // context just call these — the helper sets `producer_id` from the
    // module path so metrics carry enough label info.

    pub async fn publish_slot(&self, ev: SlotEvent) -> Result<AppendAck, AppendError> {
        self.publish(ev, default_opts("slot")).await
    }

    pub async fn publish_board(&self, ev: BoardEvent) -> Result<AppendAck, AppendError> {
        self.publish(ev, default_opts("board")).await
    }

    pub async fn publish_task(&self, ev: TaskEvent) -> Result<AppendAck, AppendError> {
        self.publish(ev, default_opts("task")).await
    }

    pub async fn publish_question(
        &self,
        ev: QuestionEvent,
    ) -> Result<AppendAck, AppendError> {
        self.publish(ev, default_opts("question")).await
    }

    pub async fn publish_llm(&self, ev: LlmEvent) -> Result<AppendAck, AppendError> {
        self.publish(ev, default_opts("llm")).await
    }

    pub async fn publish_worker(&self, ev: WorkerEvent) -> Result<AppendAck, AppendError> {
        // I007 ephemeral audit: mirror the v1 `is_ephemeral()` decisions for
        // high-volume telemetry so we don't bloat `event_log`.
        let ephemeral = matches!(
            ev,
            WorkerEvent::LlmCall { .. }
                | WorkerEvent::BriefingBatchStarted { .. }
                | WorkerEvent::BriefingSummaryGenerated { .. }
                | WorkerEvent::TranslationStarted { .. }
                | WorkerEvent::NarrationBatchCompleted { .. }
        );
        let mut opts = default_opts("worker");
        opts.ephemeral = ephemeral;
        self.publish(ev, opts).await
    }

    pub async fn publish_memory(&self, ev: MemoryEvent) -> Result<AppendAck, AppendError> {
        let ephemeral = matches!(
            ev,
            MemoryEvent::TurnExtracted { .. } | MemoryEvent::IntentAnalyzed { .. }
        );
        let mut opts = default_opts("memory");
        opts.ephemeral = ephemeral;
        self.publish(ev, opts).await
    }

    pub async fn publish_message(&self, ev: MessageEvent) -> Result<AppendAck, AppendError> {
        let ephemeral = matches!(ev, MessageEvent::ImageInserted { .. });
        let mut opts = default_opts("message");
        opts.ephemeral = ephemeral;
        self.publish(ev, opts).await
    }

    pub async fn publish_session(&self, ev: SessionEvent) -> Result<AppendAck, AppendError> {
        let ephemeral = matches!(ev, SessionEvent::Organized { .. });
        let mut opts = default_opts("session");
        opts.ephemeral = ephemeral;
        self.publish(ev, opts).await
    }

    pub async fn publish_system(&self, ev: SystemEvent) -> Result<AppendAck, AppendError> {
        self.publish(ev, default_opts("system")).await
    }

    pub async fn publish_observability(
        &self,
        ev: ObservabilityEvent,
    ) -> Result<AppendAck, AppendError> {
        // Observability events are always ephemeral (frozen lisp §4.4).
        let mut opts = default_opts("observability");
        opts.ephemeral = true;
        self.publish(ev, opts).await
    }

    pub async fn publish_incident(
        &self,
        ev: IncidentEvent,
    ) -> Result<AppendAck, AppendError> {
        self.publish(ev, default_opts("incident")).await
    }

    pub async fn publish_execution(
        &self,
        ev: ExecutionEvent,
    ) -> Result<AppendAck, AppendError> {
        // Execution events are durable projections of the on-disk companion
        // log — keep them persistent (default) so audit consumers can replay.
        self.publish(ev, default_opts("execution")).await
    }

    /// Core `append` wrapper. Producer sites that need custom `AppendOpts`
    /// (dedupe_key, span context) should call this directly.
    pub async fn publish<E: DomainEvent>(
        &self,
        event: E,
        opts: AppendOpts,
    ) -> Result<AppendAck, AppendError> {
        self.log.append(event, opts).await
    }

    // ── Subscribe convenience ───────────────────────────────────────────
    //
    // Phase 7 consumers call `bus.subscribe::<T>(name, opts).await?` to get a
    // live Subscription<T> pre-wired to the dispatcher topic + cursor store +
    // DLQ. Combinators (`.debounce`, `.coalesce`, …) are exposed on
    // Subscription<T> and stack on top of the returned handle.

    /// Subscribe to a typed domain topic. Returns a `Subscription<T>` pre-
    /// wired to the shared log / topic / cursor store / DLQ. Combinators
    /// attach on top of the returned handle.
    pub async fn subscribe<T>(
        &self,
        name: &str,
        opts: missiond_core::event::subscription::SubscriptionOpts,
    ) -> Result<missiond_core::event::subscription::Subscription<T>, missiond_core::event::subscription::SubscribeError>
    where
        T: DomainEvent,
    {
        use missiond_core::event::log::LogReadable;
        let log: Arc<dyn LogReadable> = self.log.clone();
        let topic = self.dispatcher.topic::<T>();
        missiond_core::event::subscription::subscribe::<T>(
            name,
            opts,
            log,
            topic,
            self.cursor_store.clone(),
            self.dlq.clone(),
        )
        .await
    }
}

fn default_opts(producer: &str) -> AppendOpts {
    AppendOpts {
        producer_id: format!("daemon/{producer}"),
        ..Default::default()
    }
}

/// Adapter so the metrics emitter can append via the PG `LogWriterHandle`
/// without the caller needing to know about generic `Log::append`.
struct LogObservabilityAdapter {
    log: Arc<LogWriterHandle>,
}

#[async_trait]
impl ObservabilityAppender for LogObservabilityAdapter {
    async fn append_observability(
        &self,
        event: ObservabilityEvent,
        opts: AppendOpts,
    ) -> Result<Seq, AppendError> {
        let ack = self.log.append(event, opts).await?;
        Ok(ack.seq())
    }
}

/// When `external_shutdown` flips to `true`, also flip `local_shutdown_tx`.
/// Lets the dispatcher observe either signal.
fn spawn_shutdown_fuse(
    mut external_shutdown: watch::Receiver<bool>,
    local_shutdown_tx: watch::Sender<bool>,
) {
    tokio::spawn(async move {
        loop {
            if external_shutdown.changed().await.is_err() {
                return;
            }
            if *external_shutdown.borrow() {
                let _ = local_shutdown_tx.send(true);
                return;
            }
        }
    });
}

// Forward AppendOpts helper field, so producer sites that already have a
// `BusServices` handle can grab a well-formed default.
impl BusServices {
    /// Re-exported helper so callers can build `AppendOpts` without pulling
    /// in a `missiond-core` path for the common case.
    pub fn default_append_opts(producer: &str) -> AppendOpts {
        default_opts(producer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_opts_stamps_producer_id() {
        let o = default_opts("compute/task");
        assert_eq!(o.producer_id, "daemon/compute/task");
        assert!(!o.ephemeral);
        assert!(o.dedupe_key.is_none());
    }

    #[test]
    fn log_re_export_types_are_visible() {
        // Just ensure the re-exports compile; no behavior.
        let _ = missiond_core::event::log::writer::APPEND_CHANNEL_CAPACITY;
    }
}
