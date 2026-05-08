//! `SessionEvent` — conversation session lifecycle.
//!
//! Covers v1 variants: `SessionCompleted`, `JarvisTaskCompleted`, `SessionOrganized`.
//!
//! Decision DC001: `SessionOrganized` stays here because it's a session-level
//! state transition ("S2 organizer done"), distinct from KB mutations which
//! live in `MemoryEvent`. `TurnExtracted` / `IntentAnalyzed` moved to
//! MemoryEvent because they produce memory-store rows.

use serde::{Deserialize, Serialize};

use super::super::domain::Domain;
use super::super::event_trait::DomainEvent;

/// How a conversation session ended.
///
/// Duplicated from `missiond_daemon::event_bus::SessionEndStatus` for the
/// core-crate move. Phase 8 will delete the daemon-side copy.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionEndStatus {
    /// Normal completion (user exited, /exit, or natural end).
    Success,
    /// User interrupted or PTY force-killed.
    Aborted,
    /// Abnormal crash or error.
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionEvent {
    /// A conversation session ended (PTY exit or status → completed).
    Completed {
        session_id: String,
        slot_id: Option<String>,
        message_count: u32,
        duration_secs: u64,
        status: SessionEndStatus,
    },
    /// A Jarvis async task completed — worker slot returned result.
    JarvisTaskCompleted {
        conversation_id: String,
        task_id: String,
    },
    /// S2 Organizer finished processing a session (parent links + compaction
    /// spliced).
    Organized { session_id: String },
}

impl DomainEvent for SessionEvent {
    fn domain() -> Domain {
        Domain::Session
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Completed { .. } => "completed",
            Self::JarvisTaskCompleted { .. } => "jarvis_task_completed",
            Self::Organized { .. } => "organized",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_session() {
        assert_eq!(SessionEvent::domain(), Domain::Session);
    }

    #[test]
    fn kind_returns_non_empty_for_every_variant() {
        let cases = [
            SessionEvent::Completed {
                session_id: "s".into(),
                slot_id: None,
                message_count: 0,
                duration_secs: 0,
                status: SessionEndStatus::Success,
            },
            SessionEvent::JarvisTaskCompleted {
                conversation_id: "c".into(),
                task_id: "t".into(),
            },
            SessionEvent::Organized {
                session_id: "s".into(),
            },
        ];
        for c in &cases {
            assert!(!c.kind().is_empty());
        }
    }

    #[test]
    fn serde_round_trip_end_status() {
        let ev = SessionEvent::Completed {
            session_id: "sess-99".into(),
            slot_id: Some("slot-x".into()),
            message_count: 12,
            duration_secs: 30,
            status: SessionEndStatus::Aborted,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: SessionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }
}
