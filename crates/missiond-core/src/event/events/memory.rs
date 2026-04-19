//! `MemoryEvent` — memory extraction phase lifecycle + KB mutation + deep analysis.
//!
//! Covers v1 variants: `MemoryPhaseChanged`, `DeepAnalysisCompleted`,
//! `KBBatchMutated`, `TurnExtracted`, `IntentAnalyzed`.
//!
//! Decision DC001: `DeepAnalysisCompleted` / `KBBatchMutated` / `TurnExtracted`
//! / `IntentAnalyzed` all belong here because KB + turns + intents = memory
//! store mutations, even though the v0 survey also considered Session as a
//! home. `SessionOrganized` is the exception — that stays in `SessionEvent`
//! because it's a session-lifecycle transition, not a memory mutation.

use serde::{Deserialize, Serialize};

use super::super::domain::Domain;
use super::super::event_trait::DomainEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemoryEvent {
    /// Memory extraction phase transition for a memory slot.
    PhaseChanged {
        slot_id: String,
        phase: String,
        active_type: Option<String>,
    },
    /// Deep analysis for a session finished (Strategy Worker wrote KB entries).
    /// Consumer: kb_consolidation accumulator.
    DeepAnalysisCompleted {
        session_id: String,
        kb_entries_created: u32,
    },
    /// KB entries were mutated (batch semantics to prevent event storms).
    /// Emitted from kb_remember/kb_forget with count aggregation.
    KBBatchMutated {
        count: u32,
        categories: Vec<String>,
        /// "created" | "updated" | "deleted" | "mixed"
        action: String,
    },
    /// S3 Chunker extracted turns from a session.
    TurnExtracted {
        session_id: String,
        turn_count: usize,
    },
    /// Intent Analyst analyzed turns for a session.
    IntentAnalyzed {
        session_id: String,
        intent_type: String,
    },
}

impl DomainEvent for MemoryEvent {
    fn domain() -> Domain {
        Domain::Memory
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::PhaseChanged { .. } => "phase_changed",
            Self::DeepAnalysisCompleted { .. } => "deep_analysis_completed",
            Self::KBBatchMutated { .. } => "kb_batch_mutated",
            Self::TurnExtracted { .. } => "turn_extracted",
            Self::IntentAnalyzed { .. } => "intent_analyzed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_memory() {
        assert_eq!(MemoryEvent::domain(), Domain::Memory);
    }

    #[test]
    fn kind_returns_non_empty_for_every_variant() {
        let cases = [
            MemoryEvent::PhaseChanged {
                slot_id: "s".into(),
                phase: "p".into(),
                active_type: None,
            },
            MemoryEvent::DeepAnalysisCompleted {
                session_id: "s".into(),
                kb_entries_created: 0,
            },
            MemoryEvent::KBBatchMutated {
                count: 0,
                categories: vec![],
                action: "created".into(),
            },
            MemoryEvent::TurnExtracted {
                session_id: "s".into(),
                turn_count: 0,
            },
            MemoryEvent::IntentAnalyzed {
                session_id: "s".into(),
                intent_type: "explore".into(),
            },
        ];
        for c in &cases {
            assert!(!c.kind().is_empty());
        }
    }

    #[test]
    fn serde_round_trip() {
        let ev = MemoryEvent::KBBatchMutated {
            count: 7,
            categories: vec!["architecture".into(), "bugfix".into()],
            action: "mixed".into(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: MemoryEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }
}
