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
    /// `mission_capability_usage(action=snapshot|report)` finished computing
    /// a capability hotness snapshot. Live notification only — durable
    /// evidence remains in `conversation_tool_calls` / `board_tasks` and the
    /// JSON sidecar at `.missiond/v3/runtime/capability-usage-review.json`. See
    /// frozen lisp `planned-event-extensions :: CapabilityUsageObservability`.
    CapabilityUsageSnapshot {
        /// Primary classification window: `"7d" | "30d" | "90d" | "all"`.
        window: String,
        /// `"tool" | "flow" | "both"`.
        scope: String,
        /// RFC3339 timestamp the snapshot was generated at.
        generated_at: String,
        /// Total capability rows considered (registered ∪ observed).
        capability_count: u32,
        /// Subset flagged as protected by capability-evolution-governance.
        protected_count: u32,
        /// Subset eligible for review (stale | never-used | shadowed | quiet).
        candidate_count: u32,
    },
    /// `mission_capability_usage(action=candidates)` surfaced one capability
    /// that the read-model classifies as a review candidate. Emitted once
    /// per non-protected, non-active row so notifiers can de-dupe by
    /// `capability_id` per snapshot run. Producers must still emit a
    /// preceding `CapabilityUsageSnapshot` so consumers can group by
    /// `generated_at`.
    CapabilityStaleCandidate {
        capability_id: String,
        /// `"tool" | "flow"`.
        kind: String,
        /// `"stale" | "never-used" | "shadowed-by-better-capability" |
        /// "quiet" | "merge-candidate"`.
        classification: String,
        last_used_at: Option<String>,
        /// Same RFC3339 stamp as the parent `CapabilityUsageSnapshot` it
        /// belongs to — lets consumers correlate without joining on a
        /// stronger key.
        generated_at: String,
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
            Self::CapabilityUsageSnapshot { .. } => "capability_usage_snapshot",
            Self::CapabilityStaleCandidate { .. } => "capability_stale_candidate",
        }
    }

    fn payload_size_hint(&self) -> usize {
        match self {
            // Snapshots bundle slot state + worker stats → a few KB.
            Self::HealthSnapshot { .. } => 4096,
            Self::RetentionReport { .. } => 1024,
            // CapabilityUsageSnapshot carries only summary counts (no per-row
            // arrays); ~256 B is enough.
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
            ObservabilityEvent::CapabilityUsageSnapshot {
                window: "30d".into(),
                scope: "both".into(),
                generated_at: "2026-04-25T00:00:00Z".into(),
                capability_count: 0,
                protected_count: 0,
                candidate_count: 0,
            },
            ObservabilityEvent::CapabilityStaleCandidate {
                capability_id: "mission_minimax_process".into(),
                kind: "tool".into(),
                classification: "stale".into(),
                last_used_at: None,
                generated_at: "2026-04-25T00:00:00Z".into(),
            },
        ];
        for c in &cases {
            assert!(!c.kind().is_empty());
        }
    }

    #[test]
    fn capability_usage_serde_round_trip() {
        let snapshot = ObservabilityEvent::CapabilityUsageSnapshot {
            window: "30d".into(),
            scope: "both".into(),
            generated_at: "2026-04-25T01:02:03Z".into(),
            capability_count: 87,
            protected_count: 14,
            candidate_count: 9,
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        let back: ObservabilityEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(snapshot, back);

        let candidate = ObservabilityEvent::CapabilityStaleCandidate {
            capability_id: "mission_some_legacy".into(),
            kind: "tool".into(),
            classification: "shadowed-by-better-capability".into(),
            last_used_at: Some("2026-01-12T08:00:00Z".into()),
            generated_at: "2026-04-25T01:02:03Z".into(),
        };
        let json = serde_json::to_string(&candidate).unwrap();
        let back: ObservabilityEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(candidate, back);
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
