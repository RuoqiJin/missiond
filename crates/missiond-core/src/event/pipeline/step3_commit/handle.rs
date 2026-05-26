//! Handle — producer-facing facade that implements the [`Log`] trait.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-3 commit
//! `handle`:
//!
//! > LogWriterHandle — 客户端 facade,impl Log trait;把 log.append() 调用
//! > 封装成 PendingAppend 送入 channel,等待 oneshot ack
//! > implements "Log trait (定义在 log/mod.rs)"
//!
//! # Responsibilities
//!
//! * Serialize the `DomainEvent` into bytes and compute the claim-check
//!   eligibility flag.
//! * Run step-1 `check_causation` (the causation guard lives in the
//!   step1_guard module per the 7-step layout — the handle is the
//!   producer's earliest synchronous hop).
//! * For ephemeral appends, short-circuit with a volatile seq from the
//!   process-local [`AtomicI64`] counter (see
//!   `intent-event-bus-execution.lisp` DC009).
//! * For durable appends, enqueue a [`PendingAppend`] onto the bounded
//!   channel and await the writer's `oneshot` reply.
//!
//! The handle holds no DB connection; the writer task owns the pool. This
//! keeps the `Log` trait dyn-compatible in principle (modulo the generic
//! `append<E>` method — see `LogReadable` split in `log/mod.rs`).

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

use crate::event::blob_store::CLAIM_CHECK_THRESHOLD;
use crate::event::domain::Domain;
use crate::event::event_trait::DomainEvent;
use crate::event::log::reader::{LogReader, LoggedEvent};
use crate::event::log::{AppendAck, AppendError, AppendOpts, Log, LogError, Seq};
use crate::event::metrics::BusMetrics;
use crate::event::pipeline::step1_guard::check_causation;

use super::backpressure::PendingAppend;

/// Producer-facing handle to the LogWriter task. Cheap to clone.
#[derive(Clone)]
pub struct LogWriterHandle {
    pub(super) tx: mpsc::Sender<PendingAppend>,
    #[cfg(feature = "postgres")]
    pub(super) reader: Option<LogReader>,
    /// Fallback seq source for the ephemeral fast-path in Phase 2. Dispatcher
    /// owns real seq assignment in Phase 3 — here we just hand out a
    /// process-local, monotonic counter so producers don't get `Seq(0)`.
    pub(super) volatile_counter: Arc<AtomicI64>,
    pub(super) metrics: Arc<dyn BusMetrics>,
}

impl LogWriterHandle {
    /// Raw append-channel access for internal tests / Phase-3 dispatcher
    /// integration. Normal producers go through [`Log::append`].
    pub fn sender(&self) -> mpsc::Sender<PendingAppend> {
        self.tx.clone()
    }
}

#[async_trait]
impl Log for LogWriterHandle {
    async fn append<E>(&self, event: E, opts: AppendOpts) -> Result<AppendAck, AppendError>
    where
        E: DomainEvent,
    {
        let kind: &'static str = event.kind();
        let domain: Domain = E::domain();
        // Frozen lisp §4.4 causation-loop-guard — enforced at both the PG
        // writer and the InMemoryLog so behavior is uniform.
        if let Err(err) = check_causation(&opts) {
            self.metrics.record_reject(domain, "causation");
            return Err(err);
        }
        let payload_bytes = serde_json::to_vec(&event)?;
        let payload_inline_eligible = payload_bytes.len() <= CLAIM_CHECK_THRESHOLD;

        // Ephemeral fast-path — no DB round-trip, no channel send.
        if opts.ephemeral {
            let next = self.volatile_counter.fetch_sub(1, Ordering::Relaxed);
            self.metrics
                .record_append(domain, true, payload_bytes.len());
            return Ok(AppendAck::Volatile { seq: Seq(next) });
        }
        let payload_len = payload_bytes.len();

        let producer_id = if opts.producer_id.is_empty() {
            self.metrics.record_reject(domain, "empty_producer_id");
            return Err(AppendError::Other("producer_id must not be empty".into()));
        } else {
            opts.producer_id
        };

        let (ack_tx, ack_rx) = oneshot::channel();
        let span = opts.span.unwrap_or_default();
        let pending = PendingAppend {
            domain,
            kind,
            payload_bytes,
            payload_inline_eligible,
            ephemeral: false,
            producer_id,
            dedupe_key: opts.dedupe_key,
            causation_depth: opts.causation_depth as i16,
            trace_id: span.trace_id,
            span_id: span.span_id,
            parent_span_id: span.parent_span_id,
            ack: ack_tx,
        };

        match self.tx.try_send(pending) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(pending)) => {
                self.metrics
                    .record_append(pending.domain, false, pending.payload_bytes.len());
                return Err(AppendError::Backpressure);
            }
            Err(mpsc::error::TrySendError::Closed(pending)) => {
                self.metrics
                    .record_append(pending.domain, false, pending.payload_bytes.len());
                return Err(AppendError::LogUnavailable("writer task shut down".into()));
            }
        }

        match ack_rx.await {
            Ok(r) => r,
            Err(_) => {
                self.metrics.record_append(domain, false, payload_len);
                Err(AppendError::LogUnavailable(
                    "writer ack channel dropped".into(),
                ))
            }
        }
    }

    async fn read_from(
        &self,
        domain: Domain,
        after_seq: Seq,
        limit: usize,
    ) -> Result<Vec<LoggedEvent>, LogError> {
        #[cfg(feature = "postgres")]
        {
            match &self.reader {
                Some(r) => r.read_from(domain, after_seq, limit).await,
                None => Err(LogError::Other(
                    "LogWriter configured without a reader".into(),
                )),
            }
        }
        #[cfg(not(feature = "postgres"))]
        {
            let _ = (domain, after_seq, limit);
            Err(LogError::Other(
                "pg feature disabled; LogWriter.read_from unavailable".into(),
            ))
        }
    }

    async fn head_seq(&self) -> Result<Seq, LogError> {
        #[cfg(feature = "postgres")]
        {
            match &self.reader {
                Some(r) => r.head_seq().await,
                None => Err(LogError::Other(
                    "LogWriter configured without a reader".into(),
                )),
            }
        }
        #[cfg(not(feature = "postgres"))]
        {
            Err(LogError::Other(
                "pg feature disabled; LogWriter.head_seq unavailable".into(),
            ))
        }
    }
}
