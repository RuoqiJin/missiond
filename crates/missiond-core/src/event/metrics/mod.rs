//! Bus metrics — the counters / gauges emitted by the event bus itself.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.4 observability:
//!
//! > append_rate / reject_rate
//! > dispatch_lag (tail cursor relative to max seq)
//! > per-topic queue depth + slow consumer count
//! > per-subscription lag (head seq - last_acked_seq)
//! > ephemeral vs persistent ratio
//! > control-gate dropped count
//!
//! Design:
//!
//! * [`BusMetrics`] is a trait so callers can plug a Prometheus backend in
//!   production or [`NoopMetrics`] in tests.
//! * [`AtomicBusMetrics`] is the MVP collector — stores all counters in
//!   `AtomicU64` and exposes a [`snapshot`](AtomicBusMetrics::snapshot) call
//!   for the periodic emitter.
//! * [`emitter::BusMetricsEmitter`] spawns a tokio task that calls
//!   `snapshot()` every 10s and appends an [`crate::event::ObservabilityEvent::BusMetric`]
//!   per counter with `AppendOpts::ephemeral = true`.
//!
//! Production Prometheus backend is deferred (see D002 in
//! `intent-event-bus-execution.lisp`).

pub mod emitter;

pub use emitter::{spawn_bus_metrics_emitter, BusMetricsEmitterHandle, METRICS_EMIT_INTERVAL};

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::domain::Domain;

/// Trait the bus components (log writer, dispatcher, subscription) call into
/// to record metrics.
///
/// All methods are fire-and-forget; implementations must be cheap.
pub trait BusMetrics: Send + Sync {
    /// Record a successful or failed append.
    fn record_append(&self, domain: Domain, success: bool, bytes: usize);

    /// Record an append rejected by a guard (causation, schema, etc.).
    fn record_reject(&self, domain: Domain, reason: &str);

    /// Observed dispatcher tail lag (head_seq - last_dispatched_seq).
    fn record_dispatch_lag(&self, lag_events: i64);

    /// Per-topic queue depth (broadcast channel capacity used).
    fn record_topic_depth(&self, domain: Domain, depth: usize);

    /// Per-subscription lag (head_seq - last_acked_seq).
    fn record_subscription_lag(&self, subscription: &str, lag: i64);

    /// A subscriber observed `RecvError::Lagged`.
    fn record_lagged(&self, subscription: &str);

    /// A subscriber consistently runs behind (Slow consumer).
    fn record_slow_consumer(&self, subscription: &str);

    /// Control-gate dropped an event on this domain.
    fn record_control_gate_dropped(&self, domain: Domain);
}

/// No-op metrics — default for tests / initial daemon boot before wiring a
/// real backend.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopMetrics;

impl BusMetrics for NoopMetrics {
    fn record_append(&self, _: Domain, _: bool, _: usize) {}
    fn record_reject(&self, _: Domain, _: &str) {}
    fn record_dispatch_lag(&self, _: i64) {}
    fn record_topic_depth(&self, _: Domain, _: usize) {}
    fn record_subscription_lag(&self, _: &str, _: i64) {}
    fn record_lagged(&self, _: &str) {}
    fn record_slow_consumer(&self, _: &str) {}
    fn record_control_gate_dropped(&self, _: Domain) {}
}

/// Atomic-counter backed MVP collector. Production eventually swaps in a
/// Prometheus backend (D002 deferred).
///
/// All counters use `Ordering::Relaxed` — we only care about eventual
/// consistency for observability.
#[derive(Default)]
pub struct AtomicBusMetrics {
    append_ok: AtomicU64,
    append_fail: AtomicU64,
    append_bytes: AtomicU64,
    reject: AtomicU64,
    dispatch_lag: AtomicU64,
    lagged: AtomicU64,
    slow_consumer: AtomicU64,
    control_gate_dropped: AtomicU64,
    // Per-domain breakdowns, captured in a plain Mutex<HashMap> for
    // simplicity. The emitter clones the snapshot once per tick.
    per_domain_append: std::sync::Mutex<HashMap<Domain, u64>>,
    per_domain_reject: std::sync::Mutex<HashMap<Domain, u64>>,
    per_domain_dropped: std::sync::Mutex<HashMap<Domain, u64>>,
    per_domain_topic_depth: std::sync::Mutex<HashMap<Domain, u64>>,
    per_sub_lag: std::sync::Mutex<HashMap<String, i64>>,
}

impl std::fmt::Debug for AtomicBusMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AtomicBusMetrics").finish_non_exhaustive()
    }
}

impl AtomicBusMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert to an `Arc<dyn BusMetrics>` for shared ownership.
    pub fn into_dyn(self) -> Arc<dyn BusMetrics> {
        Arc::new(self)
    }

    /// Snapshot the current counters. Used by the emitter to drive
    /// `ObservabilityEvent::BusMetric`.
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            append_ok: self.append_ok.load(Ordering::Relaxed),
            append_fail: self.append_fail.load(Ordering::Relaxed),
            append_bytes: self.append_bytes.load(Ordering::Relaxed),
            reject: self.reject.load(Ordering::Relaxed),
            dispatch_lag: self.dispatch_lag.load(Ordering::Relaxed) as i64,
            lagged: self.lagged.load(Ordering::Relaxed),
            slow_consumer: self.slow_consumer.load(Ordering::Relaxed),
            control_gate_dropped: self.control_gate_dropped.load(Ordering::Relaxed),
            per_domain_append: self.per_domain_append.lock().unwrap().clone(),
            per_domain_reject: self.per_domain_reject.lock().unwrap().clone(),
            per_domain_dropped: self.per_domain_dropped.lock().unwrap().clone(),
            per_domain_topic_depth: self.per_domain_topic_depth.lock().unwrap().clone(),
            per_sub_lag: self.per_sub_lag.lock().unwrap().clone(),
        }
    }
}

impl BusMetrics for AtomicBusMetrics {
    fn record_append(&self, domain: Domain, success: bool, bytes: usize) {
        if success {
            self.append_ok.fetch_add(1, Ordering::Relaxed);
        } else {
            self.append_fail.fetch_add(1, Ordering::Relaxed);
        }
        self.append_bytes.fetch_add(bytes as u64, Ordering::Relaxed);
        let mut map = self.per_domain_append.lock().unwrap();
        *map.entry(domain).or_insert(0) += 1;
    }

    fn record_reject(&self, domain: Domain, _reason: &str) {
        self.reject.fetch_add(1, Ordering::Relaxed);
        let mut map = self.per_domain_reject.lock().unwrap();
        *map.entry(domain).or_insert(0) += 1;
    }

    fn record_dispatch_lag(&self, lag_events: i64) {
        self.dispatch_lag
            .store(lag_events.max(0) as u64, Ordering::Relaxed);
    }

    fn record_topic_depth(&self, domain: Domain, depth: usize) {
        let mut map = self.per_domain_topic_depth.lock().unwrap();
        map.insert(domain, depth as u64);
    }

    fn record_subscription_lag(&self, subscription: &str, lag: i64) {
        let mut map = self.per_sub_lag.lock().unwrap();
        map.insert(subscription.to_string(), lag);
    }

    fn record_lagged(&self, _subscription: &str) {
        self.lagged.fetch_add(1, Ordering::Relaxed);
    }

    fn record_slow_consumer(&self, _subscription: &str) {
        self.slow_consumer.fetch_add(1, Ordering::Relaxed);
    }

    fn record_control_gate_dropped(&self, domain: Domain) {
        self.control_gate_dropped.fetch_add(1, Ordering::Relaxed);
        let mut map = self.per_domain_dropped.lock().unwrap();
        *map.entry(domain).or_insert(0) += 1;
    }
}

/// Plain-data snapshot of the current metric state. Cheap to clone; the
/// emitter converts each field into one `ObservabilityEvent::BusMetric`.
#[derive(Debug, Clone, Default)]
pub struct MetricsSnapshot {
    pub append_ok: u64,
    pub append_fail: u64,
    pub append_bytes: u64,
    pub reject: u64,
    pub dispatch_lag: i64,
    pub lagged: u64,
    pub slow_consumer: u64,
    pub control_gate_dropped: u64,
    pub per_domain_append: HashMap<Domain, u64>,
    pub per_domain_reject: HashMap<Domain, u64>,
    pub per_domain_dropped: HashMap<Domain, u64>,
    pub per_domain_topic_depth: HashMap<Domain, u64>,
    pub per_sub_lag: HashMap<String, i64>,
}

// Allow sharing via Arc without extra wrapping.
impl<T: BusMetrics + ?Sized> BusMetrics for Arc<T> {
    fn record_append(&self, d: Domain, s: bool, b: usize) {
        (**self).record_append(d, s, b)
    }
    fn record_reject(&self, d: Domain, r: &str) {
        (**self).record_reject(d, r)
    }
    fn record_dispatch_lag(&self, l: i64) {
        (**self).record_dispatch_lag(l)
    }
    fn record_topic_depth(&self, d: Domain, depth: usize) {
        (**self).record_topic_depth(d, depth)
    }
    fn record_subscription_lag(&self, s: &str, l: i64) {
        (**self).record_subscription_lag(s, l)
    }
    fn record_lagged(&self, s: &str) {
        (**self).record_lagged(s)
    }
    fn record_slow_consumer(&self, s: &str) {
        (**self).record_slow_consumer(s)
    }
    fn record_control_gate_dropped(&self, d: Domain) {
        (**self).record_control_gate_dropped(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::metrics::BusMetrics;

    #[test]
    fn noop_metrics_compiles_and_does_nothing() {
        let m = NoopMetrics;
        m.record_append(Domain::Board, true, 100);
        m.record_reject(Domain::Llm, "test");
        m.record_lagged("sub-a");
        // No panics, no assertions — just ensure all methods are callable.
    }

    #[test]
    fn atomic_metrics_record_append_counts_and_bytes() {
        let m = AtomicBusMetrics::new();
        m.record_append(Domain::Board, true, 100);
        m.record_append(Domain::Board, true, 200);
        m.record_append(Domain::Llm, false, 50);

        let snap = m.snapshot();
        assert_eq!(snap.append_ok, 2);
        assert_eq!(snap.append_fail, 1);
        assert_eq!(snap.append_bytes, 350);
        assert_eq!(snap.per_domain_append.get(&Domain::Board), Some(&2));
        assert_eq!(snap.per_domain_append.get(&Domain::Llm), Some(&1));
    }

    #[test]
    fn atomic_metrics_rejects_separately() {
        let m = AtomicBusMetrics::new();
        m.record_reject(Domain::Memory, "causation");
        m.record_reject(Domain::Memory, "schema");
        m.record_reject(Domain::Slot, "other");

        let snap = m.snapshot();
        assert_eq!(snap.reject, 3);
        assert_eq!(snap.per_domain_reject.get(&Domain::Memory), Some(&2));
        assert_eq!(snap.per_domain_reject.get(&Domain::Slot), Some(&1));
    }

    #[test]
    fn atomic_metrics_dispatch_lag_is_gauge() {
        let m = AtomicBusMetrics::new();
        m.record_dispatch_lag(500);
        m.record_dispatch_lag(200); // should overwrite
        let snap = m.snapshot();
        assert_eq!(snap.dispatch_lag, 200);
    }

    #[test]
    fn atomic_metrics_control_gate_drops() {
        let m = AtomicBusMetrics::new();
        m.record_control_gate_dropped(Domain::Memory);
        m.record_control_gate_dropped(Domain::Memory);
        m.record_control_gate_dropped(Domain::Board);

        let snap = m.snapshot();
        assert_eq!(snap.control_gate_dropped, 3);
        assert_eq!(snap.per_domain_dropped.get(&Domain::Memory), Some(&2));
        assert_eq!(snap.per_domain_dropped.get(&Domain::Board), Some(&1));
    }

    #[test]
    fn atomic_metrics_subscription_lag_overwrites() {
        let m = AtomicBusMetrics::new();
        m.record_subscription_lag("sub-a", 100);
        m.record_subscription_lag("sub-a", 50);
        m.record_subscription_lag("sub-b", 10);

        let snap = m.snapshot();
        assert_eq!(snap.per_sub_lag.get("sub-a"), Some(&50));
        assert_eq!(snap.per_sub_lag.get("sub-b"), Some(&10));
    }

    #[test]
    fn atomic_metrics_lagged_and_slow_consumer() {
        let m = AtomicBusMetrics::new();
        m.record_lagged("sub-a");
        m.record_lagged("sub-b");
        m.record_slow_consumer("sub-a");

        let snap = m.snapshot();
        assert_eq!(snap.lagged, 2);
        assert_eq!(snap.slow_consumer, 1);
    }
}
