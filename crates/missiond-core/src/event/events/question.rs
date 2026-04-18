//! `QuestionEvent` — decision-engine question lifecycle.
//!
//! Covers v1 variants: `QuestionCreated`, `QuestionResolved`, `DecisionResolved`.

use serde::{Deserialize, Serialize};

use super::super::domain::Domain;
use super::super::event_trait::DomainEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum QuestionEvent {
    Created {
        question_id: String,
    },
    Resolved {
        question_id: String,
        resolution: String,
    },
    DecisionResolved {
        question_id: String,
        tier: String,
        duration_ms: u64,
    },
}

impl DomainEvent for QuestionEvent {
    fn domain() -> Domain {
        Domain::Question
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Created { .. } => "created",
            Self::Resolved { .. } => "resolved",
            Self::DecisionResolved { .. } => "decision_resolved",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_question() {
        assert_eq!(QuestionEvent::domain(), Domain::Question);
    }

    #[test]
    fn kind_returns_non_empty_for_every_variant() {
        let cases = [
            QuestionEvent::Created {
                question_id: "q1".into(),
            },
            QuestionEvent::Resolved {
                question_id: "q1".into(),
                resolution: "yes".into(),
            },
            QuestionEvent::DecisionResolved {
                question_id: "q1".into(),
                tier: "t1".into(),
                duration_ms: 123,
            },
        ];
        for c in &cases {
            assert!(!c.kind().is_empty());
        }
    }

    #[test]
    fn serde_round_trip() {
        let ev = QuestionEvent::DecisionResolved {
            question_id: "q-abc".into(),
            tier: "tier1".into(),
            duration_ms: 9876,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: QuestionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }
}
