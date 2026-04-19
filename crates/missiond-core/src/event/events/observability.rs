//! `ObservabilityEvent` — bus self-telemetry. All variants are ephemeral by
//! contract (see frozen lisp §4.4 `observability.self-emission`).
//!
//! New in v2. No v1 equivalent: the v1 `health_snapshot` was injected
//! directly into the WS feed (see phase0 inventory §7b).
//!
//! Invariant: `AppendOpts { ephemeral: true }` must be set when appending
//! any of these; otherwise observing observability creates a feedback
//! loop that blows up event_log retention.

use serde::{Deserialize, Serialize};

use super::super::domain::Domain;
use super::super::event_trait::DomainEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ObservabilityEvent {
    /// Periodic health snapshot of the whole daemon — replaces v1
    /// `health_snapshot` synthetic WS message.
    HealthSnapshot {
        /// Arbitrary health payload produced by the collector. Kept as
        /// `serde_json::Value` so the collector can evolve without
        /// touching the schema.
        payload: serde_json::Value,
        /// Millisecond timestamp (redundant with log `ts`, kept for
        /// consumers that cache by snapshot id).
        ts_ms: i64,
    },
    /// A bus-internal metric tick (append_rate, dispatch_lag, etc.).
    BusMetric {
        /// Metric family name.
        name: String,
        /// Metric value; `f64` supports both counts and rates.
        value: f64,
        /// Optional labels (domain, topic, subscription).
        #[serde(default)]
        labels: serde_json::Value,
    },
    /// A subscriber fell too far behind — used by the dispatcher to signal
    /// back-pressure.
    SlowConsumer {
        subscription_name: String,
        domain: String,
        lag_seq: i64,
        dropped_count: u64,
    },
    /// Daily retention cron summary. Emitted once per run.
    RetentionReport {
        /// Arbitrary JSON summary: counts of events/blobs/orphans deleted.
        summary: serde_json::Value,
    },
}

impl DomainEvent for ObservabilityEvent {
    fn domain() -> Domain {
        Domain::Observability
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::HealthSnapshot { .. } => "health_snapshot",
            Self::BusMetric { .. } => "bus_metric",
            Self::SlowConsumer { .. } => "slow_consumer",
            Self::RetentionReport { .. } => "retention_report",
        }
    }

    fn payload_size_hint(&self) -> usize {
        match self {
            // Snapshots bundle slot state + worker stats → a few KB.
            Self::HealthSnapshot { .. } => 4096,
            Self::RetentionReport { .. } => 1024,
            _ => 256,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_observability() {
        assert_eq!(ObservabilityEvent::domain(), Domain::Observability);
    }

    #[test]
    fn kind_returns_non_empty_for_every_variant() {
        let cases = [
            ObservabilityEvent::HealthSnapshot {
                payload: serde_json::json!({}),
                ts_ms: 0,
            },
            ObservabilityEvent::BusMetric {
                name: "append_rate".into(),
                value: 0.0,
                labels: serde_json::Value::Null,
            },
            ObservabilityEvent::SlowConsumer {
                subscription_name: "s".into(),
                domain: "memory".into(),
                lag_seq: 0,
                dropped_count: 0,
            },
            ObservabilityEvent::RetentionReport {
                summary: serde_json::json!({}),
            },
        ];
        for c in &cases {
            assert!(!c.kind().is_empty());
        }
    }

    #[test]
    fn serde_round_trip() {
        let ev = ObservabilityEvent::HealthSnapshot {
            payload: serde_json::json!({"slots": 12, "queue": 4}),
            ts_ms: 1_700_000_000_000,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: ObservabilityEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn payload_size_hint_big_for_health_snapshot() {
        let big = ObservabilityEvent::HealthSnapshot {
            payload: serde_json::json!({}),
            ts_ms: 0,
        };
        assert!(big.payload_size_hint() > 1000);
    }
}
