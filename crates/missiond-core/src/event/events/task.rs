//! `TaskEvent` — submit-queue task lifecycle + cascade repair lifecycle.
//!
//! Covers v1 variants: `TaskCreated`, `TaskCompleted`, `CascadeTriggered`,
//! `CascadeCompleted`.
//!
//! Decision DC001: Cascade* events folded into TaskEvent because cascade
//! repair is a specialized task kind (it creates board-side follow-up tasks
//! and the lifecycle mirrors submit queue).

use serde::{Deserialize, Serialize};

use super::super::domain::Domain;
use super::super::event_trait::DomainEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskEvent {
    Created {
        task_id: String,
    },
    Completed {
        task_id: String,
    },
    /// A cascade repair was triggered (before execution).
    CascadeTriggered {
        service: String,
        changed: Vec<String>,
    },
    /// A cascade repair completed (after execution).
    CascadeCompleted {
        service: String,
        services_repaired: usize,
        services_failed: usize,
        hard_halted: bool,
        duration_ms: u128,
    },
}

impl DomainEvent for TaskEvent {
    fn domain() -> Domain {
        Domain::Task
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Created { .. } => "created",
            Self::Completed { .. } => "completed",
            Self::CascadeTriggered { .. } => "cascade_triggered",
            Self::CascadeCompleted { .. } => "cascade_completed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_task() {
        assert_eq!(TaskEvent::domain(), Domain::Task);
    }

    #[test]
    fn kind_returns_non_empty_for_every_variant() {
        let cases = [
            TaskEvent::Created {
                task_id: "t".into(),
            },
            TaskEvent::Completed {
                task_id: "t".into(),
            },
            TaskEvent::CascadeTriggered {
                service: "s".into(),
                changed: vec![],
            },
            TaskEvent::CascadeCompleted {
                service: "s".into(),
                services_repaired: 0,
                services_failed: 0,
                hard_halted: false,
                duration_ms: 0,
            },
        ];
        for c in &cases {
            assert!(!c.kind().is_empty());
        }
    }

    #[test]
    fn serde_round_trip_cascade() {
        let ev = TaskEvent::CascadeCompleted {
            service: "missiond-daemon".into(),
            services_repaired: 3,
            services_failed: 1,
            hard_halted: false,
            duration_ms: 12_345,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: TaskEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }
}
