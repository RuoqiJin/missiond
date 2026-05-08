//! Bus self-emission — periodic task that converts a [`MetricsSnapshot`]
//! into a stream of [`ObservabilityEvent::BusMetric`] appends.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.4 observability:
//!
//! > bus self-observation → ObservabilityEvent must be ephemeral=true,
//! > otherwise an observation self-loop forms.
//!
//! The emitter runs inside a tokio task; tear-down is via the returned
//! [`BusMetricsEmitterHandle`]. `shutdown()` sends a stop signal and awaits
//! the task.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::super::domain::Domain;
use super::super::events::ObservabilityEvent;
use super::super::log::{AppendError, AppendOpts, Seq};
use super::AtomicBusMetrics;

/// Interval between metric snapshots. Frozen lisp doesn't pin an exact
/// value; 10 s keeps the observability channel at well under 1% of the
/// total event rate for any sane workload.
pub const METRICS_EMIT_INTERVAL: Duration = Duration::from_secs(10);

/// The subset of the [`Log`](crate::event::Log) trait the emitter needs.
///
/// Used instead of `Arc<dyn Log>` because `Log::append` is generic (hence
/// not dyn-compatible). We narrow the contract to a single concrete event
/// type (`ObservabilityEvent`) so we can erase the type with a trait
/// object.
#[async_trait]
pub trait ObservabilityAppender: Send + Sync {
    /// Append a single `BusMetric` event with ephemeral=true.
    async fn append_observability(
        &self,
        event: ObservabilityEvent,
        opts: AppendOpts,
    ) -> Result<Seq, AppendError>;
}

/// Handle returned by [`spawn_bus_metrics_emitter`]. Drop = signal shutdown.
pub struct BusMetricsEmitterHandle {
    shutdown_tx: watch::Sender<bool>,
    join: Option<JoinHandle<()>>,
}

impl BusMetricsEmitterHandle {
    /// Gracefully stop the emitter and wait for the task to exit.
    pub async fn shutdown(mut self) {
        let _ = self.shutdown_tx.send(true);
        if let Some(h) = self.join.take() {
            let _ = h.await;
        }
    }
}

impl Drop for BusMetricsEmitterHandle {
    fn drop(&mut self) {
        // Best-effort signal for the case where the caller forgot to
        // shutdown(). The task will still observe the flip on the next tick.
        let _ = self.shutdown_tx.send(true);
    }
}

/// Spawn the periodic metric emitter.
///
/// Semantics:
///
/// * Every `interval` the emitter takes a snapshot of `metrics` and appends
///   a handful of `ObservabilityEvent::BusMetric` events (one per counter
///   family) with `AppendOpts { ephemeral: true, .. }`.
/// * Append failures are logged at WARN and otherwise swallowed — the bus
///   must not crash because observability can't be emitted.
/// * The task exits cleanly when the handle is dropped or `shutdown()` is
///   called.
pub fn spawn_bus_metrics_emitter(
    metrics: Arc<AtomicBusMetrics>,
    appender: Arc<dyn ObservabilityAppender>,
    interval: Duration,
) -> BusMetricsEmitterHandle {
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let producer_id = "bus_metrics_emitter".to_string();
    let join = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Consume the immediate first tick so callers don't see a burst on
        // startup.
        ticker.tick().await;

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    emit_once(metrics.as_ref(), appender.as_ref(), &producer_id).await;
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        tracing::info!("bus_metrics_emitter: shutdown signal received");
                        return;
                    }
                }
            }
        }
    });
    BusMetricsEmitterHandle {
        shutdown_tx,
        join: Some(join),
    }
}

async fn emit_once(
    metrics: &AtomicBusMetrics,
    appender: &dyn ObservabilityAppender,
    producer_id: &str,
) {
    let snap = metrics.snapshot();

    // Each counter family becomes one `BusMetric` event.
    let simple: [(&str, f64); 5] = [
        ("append_ok_total", snap.append_ok as f64),
        ("append_fail_total", snap.append_fail as f64),
        ("append_bytes_total", snap.append_bytes as f64),
        ("reject_total", snap.reject as f64),
        ("dispatch_lag_events", snap.dispatch_lag as f64),
    ];
    for (name, value) in simple {
        let event = ObservabilityEvent::BusMetric {
            name: name.into(),
            value,
            labels: serde_json::Value::Null,
        };
        submit(appender, event, producer_id).await;
    }

    // Per-domain append counters.
    for (domain, count) in &snap.per_domain_append {
        let event = ObservabilityEvent::BusMetric {
            name: "append_ok_by_domain".into(),
            value: *count as f64,
            labels: labels_domain(*domain),
        };
        submit(appender, event, producer_id).await;
    }

    // Per-domain control-gate drops.
    for (domain, count) in &snap.per_domain_dropped {
        let event = ObservabilityEvent::BusMetric {
            name: "control_gate_dropped_by_domain".into(),
            value: *count as f64,
            labels: labels_domain(*domain),
        };
        submit(appender, event, producer_id).await;
    }

    // Per-topic queue depth (gauge).
    for (domain, depth) in &snap.per_domain_topic_depth {
        let event = ObservabilityEvent::BusMetric {
            name: "topic_queue_depth".into(),
            value: *depth as f64,
            labels: labels_domain(*domain),
        };
        submit(appender, event, producer_id).await;
    }

    // Per-subscription lag.
    for (sub, lag) in &snap.per_sub_lag {
        let event = ObservabilityEvent::BusMetric {
            name: "subscription_lag_seq".into(),
            value: *lag as f64,
            labels: labels_subscription(sub),
        };
        submit(appender, event, producer_id).await;
    }
}

fn labels_domain(d: Domain) -> serde_json::Value {
    serde_json::json!({ "domain": d.as_str() })
}

fn labels_subscription(s: &str) -> serde_json::Value {
    serde_json::json!({ "subscription": s })
}

async fn submit(
    appender: &dyn ObservabilityAppender,
    event: ObservabilityEvent,
    producer_id: &str,
) {
    let opts = AppendOpts {
        ephemeral: true,
        producer_id: producer_id.to_string(),
        ..Default::default()
    };
    if let Err(e) = appender.append_observability(event, opts).await {
        tracing::warn!(error = %e, "bus_metrics_emitter: append failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::metrics::BusMetrics;

    #[derive(Default)]
    struct RecordingAppender {
        appends: std::sync::Mutex<Vec<(String, f64, bool)>>,
    }

    #[async_trait]
    impl ObservabilityAppender for RecordingAppender {
        async fn append_observability(
            &self,
            event: ObservabilityEvent,
            opts: AppendOpts,
        ) -> Result<Seq, AppendError> {
            if let ObservabilityEvent::BusMetric { name, value, .. } = event {
                self.appends
                    .lock()
                    .unwrap()
                    .push((name, value, opts.ephemeral));
            }
            Ok(Seq(-1))
        }
    }

    #[tokio::test]
    async fn emit_once_generates_one_event_per_counter_family() {
        let metrics = AtomicBusMetrics::new();
        metrics.record_append(Domain::Board, true, 100);
        metrics.record_reject(Domain::Llm, "test");
        metrics.record_dispatch_lag(5);
        metrics.record_control_gate_dropped(Domain::Memory);
        metrics.record_subscription_lag("sub-a", 42);
        metrics.record_topic_depth(Domain::Board, 7);

        let appender = Arc::new(RecordingAppender::default());
        emit_once(&metrics, &*appender, "test").await;

        let snap = appender.appends.lock().unwrap().clone();
        // 5 simple + 1 per_domain_append + 1 per_domain_dropped
        //                + 1 topic_depth + 1 sub_lag = 9
        assert_eq!(snap.len(), 9, "snapshot produced: {:?}", snap);

        let names: Vec<&str> = snap.iter().map(|(n, _, _)| n.as_str()).collect();
        assert!(names.contains(&"append_ok_total"));
        assert!(names.contains(&"append_fail_total"));
        assert!(names.contains(&"reject_total"));
        assert!(names.contains(&"dispatch_lag_events"));
        assert!(names.contains(&"append_ok_by_domain"));
        assert!(names.contains(&"control_gate_dropped_by_domain"));
        assert!(names.contains(&"topic_queue_depth"));
        assert!(names.contains(&"subscription_lag_seq"));

        // Every submission must be ephemeral=true.
        for (_, _, ephem) in &snap {
            assert!(*ephem, "all emissions must have ephemeral=true");
        }
    }

    #[tokio::test]
    async fn emitter_stops_on_shutdown_signal() {
        let metrics = Arc::new(AtomicBusMetrics::new());
        let appender: Arc<dyn ObservabilityAppender> = Arc::new(RecordingAppender::default());

        let handle =
            spawn_bus_metrics_emitter(metrics.clone(), appender, Duration::from_millis(20));

        // Let one or two ticks run.
        tokio::time::sleep(Duration::from_millis(80)).await;

        handle.shutdown().await;
        // A second shutdown call is fine — handle already dropped.
    }

    #[tokio::test]
    async fn emit_once_does_not_propagate_appender_errors() {
        struct FailingAppender;
        #[async_trait]
        impl ObservabilityAppender for FailingAppender {
            async fn append_observability(
                &self,
                _event: ObservabilityEvent,
                _opts: AppendOpts,
            ) -> Result<Seq, AppendError> {
                Err(AppendError::LogUnavailable("boom".into()))
            }
        }

        let metrics = AtomicBusMetrics::new();
        metrics.record_append(Domain::Board, true, 100);
        // No panic; no crash. We rely on the logger picking up the WARN.
        emit_once(&metrics, &FailingAppender, "test").await;
    }
}
