//! `Domain` — the static domains of the event bus.
//!
//! Frozen lisp §4.2.a: every `DomainEvent` enum maps to exactly one `Domain`,
//! and the topic registry is keyed by `Domain`. The set started at 12 in
//! Phase 1; `planned-event-extensions` in the same lisp explicitly anticipates
//! growth ("12 域是起点不是终点"). `Execution` was added when
//! `mission_execution` (commit 4ab7994) needed live status / audit
//! projection alongside the durable companion log.

use serde::{Deserialize, Serialize};

/// Static domain key for the event bus topic registry.
///
/// Each variant corresponds to exactly one `DomainEvent` enum defined in
/// `crate::event::events::*`. Dispatcher uses this to route log tail → topic;
/// control-gate maps this (many-to-one) onto the coarser `CtlDomain`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Domain {
    Slot,
    Board,
    Task,
    Question,
    Llm,
    Worker,
    Memory,
    Message,
    Session,
    System,
    Observability,
    Incident,
    Execution,
}

impl Domain {
    /// Stable string label — used for DB column `event_log.domain` and
    /// metrics tags. Must never change without a migration.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Slot => "slot",
            Self::Board => "board",
            Self::Task => "task",
            Self::Question => "question",
            Self::Llm => "llm",
            Self::Worker => "worker",
            Self::Memory => "memory",
            Self::Message => "message",
            Self::Session => "session",
            Self::System => "system",
            Self::Observability => "observability",
            Self::Incident => "incident",
            Self::Execution => "execution",
        }
    }

    /// Full ordered list — used by `bus.topics()` to enumerate topics at
    /// startup. Order is stable and mirrors declaration order.
    pub const ALL: [Domain; 13] = [
        Domain::Slot,
        Domain::Board,
        Domain::Task,
        Domain::Question,
        Domain::Llm,
        Domain::Worker,
        Domain::Memory,
        Domain::Message,
        Domain::Session,
        Domain::System,
        Domain::Observability,
        Domain::Incident,
        Domain::Execution,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_domains_have_unique_labels() {
        let mut seen = std::collections::HashSet::new();
        for d in Domain::ALL {
            assert!(seen.insert(d.as_str()), "duplicate label: {}", d.as_str());
        }
        assert_eq!(seen.len(), Domain::ALL.len());
    }

    #[test]
    fn all_domains_serde_round_trip() {
        for d in Domain::ALL {
            let json = serde_json::to_string(&d).unwrap();
            let back: Domain = serde_json::from_str(&json).unwrap();
            assert_eq!(d, back);
        }
    }
}
