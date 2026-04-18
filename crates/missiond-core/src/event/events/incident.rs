//! `IncidentEvent` — incident reported/resolved lifecycle.
//!
//! New in v2. Replaces the v1 `incident_tx` MPSC bypass (see frozen lisp
//! §4.1 `dead-bypass`). The Phase-0 v1 code routed incidents directly to a
//! mpsc channel → `aiops::process_incident`; here they flow through the
//! log.
//!
//! Re-uses `MissionIncident` from `crate::types::incident` so that Phase-2
//! shim layer can `.into()` existing producers 1:1.

use serde::{Deserialize, Serialize};

use super::super::domain::Domain;
use super::super::event_trait::DomainEvent;
use crate::types::MissionIncident;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IncidentEvent {
    /// A new incident was detected by a sensor (health check, PTY slot, etc.).
    Reported {
        incident: MissionIncident,
    },
    /// An incident was resolved (manual close, auto-resolve, or timeout).
    Resolved {
        incident_id: String,
        /// Free-form reason: "manual", "auto_recovery", "expired", etc.
        reason: String,
    },
}

impl DomainEvent for IncidentEvent {
    fn domain() -> Domain {
        Domain::Incident
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Reported { .. } => "reported",
            Self::Resolved { .. } => "resolved",
        }
    }

    fn payload_size_hint(&self) -> usize {
        match self {
            // `raw_payload` is arbitrary JSON (webhook body) + description.
            Self::Reported { .. } => 8192,
            Self::Resolved { .. } => 256,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{IncidentSeverity, IncidentSource};

    fn sample_incident() -> MissionIncident {
        MissionIncident {
            id: "inc-1".into(),
            severity: IncidentSeverity::Warning,
            source: IncidentSource::HealthCheck,
            title: "disk high".into(),
            description: "disk usage at 85%".into(),
            server_id: Some("s-1".into()),
            raw_payload: serde_json::json!({"percent": 85}),
            created_at: "2026-04-19T00:00:00Z".into(),
        }
    }

    #[test]
    fn domain_is_incident() {
        assert_eq!(IncidentEvent::domain(), Domain::Incident);
    }

    #[test]
    fn kind_returns_non_empty_for_every_variant() {
        let cases = [
            IncidentEvent::Reported {
                incident: sample_incident(),
            },
            IncidentEvent::Resolved {
                incident_id: "inc-1".into(),
                reason: "manual".into(),
            },
        ];
        for c in &cases {
            assert!(!c.kind().is_empty());
        }
    }

    #[test]
    fn serde_round_trip_reported() {
        let ev = IncidentEvent::Reported {
            incident: sample_incident(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: IncidentEvent = serde_json::from_str(&json).unwrap();
        // Can't derive PartialEq (serde_json::Value) — compare via re-serialize.
        assert_eq!(
            serde_json::to_string(&ev).unwrap(),
            serde_json::to_string(&back).unwrap()
        );
    }

    #[test]
    fn payload_size_hint_big_for_reported() {
        let big = IncidentEvent::Reported {
            incident: sample_incident(),
        };
        assert!(big.payload_size_hint() > 1000);
    }
}
