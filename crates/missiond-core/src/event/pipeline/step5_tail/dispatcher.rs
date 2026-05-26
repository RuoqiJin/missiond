//! Dispatcher tail loop — long-poll + control-gate + per-topic fan-out.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-5 tail
//! `tail-mechanism`:
//!
//! > run_tail 主循环 + dispatch_one + control-gate 调用,编排各 TailSource impl
//! > source        "PostgreSQL 长轮询 SELECT WHERE seq > last_dispatched LIMIT 256 每 100ms"
//! > ordering      "严格按 seq 升序,同 batch 保持 INSERT 顺序"
//! > missed-events "崩溃重启从 last_dispatched 继续,不替订阅者补发"
//!
//! Contract (see [`super`] module-level doc for the full list):
//!   1. Read up to [`super::TAIL_BATCH_LIMIT`] events with
//!      `seq > last_dispatched_seq` across **all** domains.
//!   2. For each event, in seq order:
//!      - Look up the `ControlGate` → skip (drop) if the domain is paused.
//!        The seq cursor still advances so paused events are not retried.
//!      - Deserialize via the registered `AnyTopic` and fan out.
//!   3. Wait [`super::TAIL_POLL_INTERVAL`] before the next read. Exit early
//!      on `shutdown`.
//!
//! Failure modes:
//!   * Unknown domain label (corrupt row, schema mismatch) → log + drop
//!     the row + advance cursor. We do NOT halt the dispatcher — a single
//!     bad row must not stall the bus.
//!   * Payload deserialize failure → same: log + drop + advance. The
//!     subscriber path (Phase 4) has its own retries; the dispatcher is
//!     at-most-once for live fan-out.
//!   * Tail source error → return [`DispatchError::Tail`] up to the
//!     supervisor. Restart strategy is the daemon's job.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use tokio::sync::watch;
use tokio::time::timeout;

use crate::event::blob_store::BlobStore;
use crate::event::log::reader::LoggedEvent;
use crate::event::log::Seq;
use crate::event::metrics::BusMetrics;
use crate::event::pipeline::step6_gate::ControlGate;
use crate::event::pipeline::step7_fanout::registry::TopicRegistry;

use super::metrics::DispatchMetrics;
use super::tail_source::{DispatchError, TailSource};
use super::{TAIL_BATCH_LIMIT, TAIL_POLL_INTERVAL};

/// Main dispatcher loop. Invoked from `Dispatcher::run`.
///
/// The loop is bounded by `shutdown`: when `*shutdown.borrow()` becomes
/// `true` the loop returns `Err(DispatchError::Shutdown)` with current
/// metrics preserved via the last_dispatched_seq counter.
pub async fn run_tail<G>(
    tail_source: Arc<dyn TailSource>,
    _blob_store: Arc<dyn BlobStore>,
    registry: Arc<TopicRegistry>,
    last_dispatched_seq: Arc<AtomicI64>,
    control: G,
    mut shutdown: watch::Receiver<bool>,
    bus_metrics: Arc<dyn BusMetrics>,
) -> Result<DispatchMetrics, DispatchError>
where
    G: ControlGate + Clone + 'static,
{
    let mut metrics = DispatchMetrics::default();

    loop {
        // Fast shutdown check before each poll.
        if *shutdown.borrow() {
            return Err(DispatchError::Shutdown);
        }

        let cursor = Seq(last_dispatched_seq.load(Ordering::Acquire));
        let batch = tail_source
            .read_all_from(cursor, TAIL_BATCH_LIMIT)
            .await
            .map_err(DispatchError::Tail)?;

        metrics.rows_read += batch.len() as u64;
        let was_full = batch.len() == TAIL_BATCH_LIMIT;

        for logged in batch {
            dispatch_one(
                &registry,
                &control,
                &logged,
                &mut metrics,
                bus_metrics.as_ref(),
            );
            last_dispatched_seq.store(logged.seq.0, Ordering::Release);
            metrics.last_seq = logged.seq.0;
        }

        // If we emptied the batch (or none was available), wait the
        // poll interval; `timeout` on the shutdown receiver lets us wake
        // promptly on shutdown.
        if !was_full {
            match timeout(TAIL_POLL_INTERVAL, shutdown.changed()).await {
                Ok(Ok(())) => {
                    // shutdown flipped
                    if *shutdown.borrow() {
                        return Err(DispatchError::Shutdown);
                    }
                }
                Ok(Err(_)) => {
                    // shutdown tx dropped — treat as shutdown.
                    return Err(DispatchError::Shutdown);
                }
                Err(_) => {
                    // poll interval elapsed; loop again.
                }
            }
        }
    }
}

/// Route a single LoggedEvent to its topic, respecting the control gate.
///
/// Metrics are updated in place. Errors are swallowed (logged at WARN) on
/// purpose — see module docs. This keeps one bad row from stalling the
/// whole tail loop.
fn dispatch_one(
    registry: &TopicRegistry,
    control: &impl ControlGate,
    logged: &LoggedEvent,
    metrics: &mut DispatchMetrics,
    bus_metrics: &dyn BusMetrics,
) {
    // Control-gate check first. Paused ⇒ drop + advance cursor.
    if control.is_domain_paused(logged.domain) {
        metrics.rows_dropped_paused += 1;
        bus_metrics.record_control_gate_dropped(logged.domain);
        tracing::debug!(
            seq = logged.seq.0,
            domain = logged.domain.as_str(),
            "tail: domain paused; dropping"
        );
        return;
    }

    let Some(topic) = registry.any_topic(logged.domain) else {
        metrics.rows_dropped_unknown_domain += 1;
        tracing::warn!(
            seq = logged.seq.0,
            domain = logged.domain.as_str(),
            "tail: unregistered domain topic; dropping"
        );
        return;
    };

    match topic.fanout_json(&logged.payload) {
        Ok(()) => {
            metrics.rows_fanned_out += 1;
            bus_metrics.record_topic_depth(logged.domain, topic.receiver_count());
        }
        Err(e) => {
            metrics.rows_dropped_deserialize += 1;
            bus_metrics.record_reject(logged.domain, "payload_deserialize");
            tracing::warn!(
                seq = logged.seq.0,
                domain = logged.domain.as_str(),
                error = %e,
                "tail: payload deserialize failed; dropping"
            );
        }
    }
}
