//! `SlotEvent` — PTY slot lifecycle.
//!
//! Covers v1 variants: `SlotBecameIdle`, `SlotStateChanged`, `SlotTaskDispatched`.
//! "Stuck" is reserved for the future (see frozen lisp §4.2.a).

use serde::{Deserialize, Serialize};

use super::super::domain::Domain;
use super::super::event_trait::DomainEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SlotEvent {
    /// A slot transitioned to Idle (extraction/submit can trigger).
    BecameIdle { slot_id: String },
    /// Slot state transition with previous state.
    StateChanged {
        slot_id: String,
        new_state: String,
        prev_state: String,
    },
    /// A task/prompt was dispatched to a slot for execution.
    TaskDispatched {
        slot_id: String,
        /// Associated task_id if dispatched from mission_submit queue.
        task_id: Option<String>,
        /// Purpose: "submit", "extraction", "deep_analysis", "user_voice", "consolidation".
        purpose: String,
        prompt_chars: usize,
        /// Truncated preview of the dispatched prompt.
        preview: String,
        /// KB entry IDs cited in the context injection (for causality tracking).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        cited_kb_ids: Vec<String>,
    },
    /// Future: a slot has failed to progress and needs intervention.
    /// Kept for API continuity with frozen lisp §4.2.a; emitted once Phase 2+
    /// produces stall detection. Not currently emitted in v1.
    Stuck {
        slot_id: String,
        reason: String,
        last_activity_ms_ago: u64,
    },
}

impl DomainEvent for SlotEvent {
    fn domain() -> Domain {
        Domain::Slot
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::BecameIdle { .. } => "became_idle",
            Self::StateChanged { .. } => "state_changed",
            Self::TaskDispatched { .. } => "task_dispatched",
            Self::Stuck { .. } => "stuck",
        }
    }

    fn payload_size_hint(&self) -> usize {
        match self {
            // `preview` ~200 chars; rest ids+metadata. 4 KB ceiling covers
            // outlier prompts with many cited KB ids.
            Self::TaskDispatched { .. } => 4096,
            _ => 256,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_slot() {
        assert_eq!(SlotEvent::domain(), Domain::Slot);
    }

    #[test]
    fn kind_returns_non_empty_for_every_variant() {
        let cases = [
            SlotEvent::BecameIdle {
                slot_id: "s1".into(),
            },
            SlotEvent::StateChanged {
                slot_id: "s1".into(),
                new_state: "Idle".into(),
                prev_state: "Busy".into(),
            },
            SlotEvent::TaskDispatched {
                slot_id: "s1".into(),
                task_id: None,
                purpose: "submit".into(),
                prompt_chars: 0,
                preview: String::new(),
                cited_kb_ids: Vec::new(),
            },
            SlotEvent::Stuck {
                slot_id: "s1".into(),
                reason: "x".into(),
                last_activity_ms_ago: 0,
            },
        ];
        for c in &cases {
            assert!(!c.kind().is_empty());
        }
    }

    #[test]
    fn serde_round_trip() {
        let ev = SlotEvent::TaskDispatched {
            slot_id: "slot-42".into(),
            task_id: Some("task-1".into()),
            purpose: "submit".into(),
            prompt_chars: 1234,
            preview: "do the thing".into(),
            cited_kb_ids: vec!["kb-1".into(), "kb-2".into()],
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: SlotEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn payload_size_hint_big_for_task_dispatched() {
        let big = SlotEvent::TaskDispatched {
            slot_id: "s1".into(),
            task_id: None,
            purpose: "submit".into(),
            prompt_chars: 0,
            preview: String::new(),
            cited_kb_ids: vec![],
        };
        assert!(big.payload_size_hint() > 1000);
    }
}
