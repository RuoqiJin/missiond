//! `BoardEvent` — Board Task lifecycle.
//!
//! Covers v1 variants: `BoardTaskCreated`, `BoardTaskStatusChanged`,
//! `BoardTaskNoteAdded`, `BoardTaskClaimed`, `BoardTaskDeleted`,
//! `BoardTaskUpdated`.

use serde::{Deserialize, Serialize};

use super::super::domain::Domain;
use super::super::event_trait::DomainEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BoardEvent {
    TaskCreated {
        task_id: String,
        title: String,
        category: String,
    },
    StatusChanged {
        task_id: String,
        old_status: String,
        new_status: String,
    },
    NoteAdded {
        task_id: String,
        note_id: String,
        content_preview: String,
    },
    Claimed {
        task_id: String,
        slot_id: String,
    },
    Deleted {
        task_id: String,
        title: String,
    },
    Updated {
        task_id: String,
        status: String,
        category: String,
    },
}

impl DomainEvent for BoardEvent {
    fn domain() -> Domain {
        Domain::Board
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::TaskCreated { .. } => "task_created",
            Self::StatusChanged { .. } => "status_changed",
            Self::NoteAdded { .. } => "note_added",
            Self::Claimed { .. } => "claimed",
            Self::Deleted { .. } => "deleted",
            Self::Updated { .. } => "updated",
        }
    }

    // All Board variants are tiny metadata tuples; 256 byte default is fine.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_board() {
        assert_eq!(BoardEvent::domain(), Domain::Board);
    }

    #[test]
    fn kind_returns_non_empty_for_every_variant() {
        let cases = [
            BoardEvent::TaskCreated {
                task_id: "t".into(),
                title: "a".into(),
                category: "c".into(),
            },
            BoardEvent::StatusChanged {
                task_id: "t".into(),
                old_status: "todo".into(),
                new_status: "done".into(),
            },
            BoardEvent::NoteAdded {
                task_id: "t".into(),
                note_id: "n".into(),
                content_preview: "p".into(),
            },
            BoardEvent::Claimed {
                task_id: "t".into(),
                slot_id: "s".into(),
            },
            BoardEvent::Deleted {
                task_id: "t".into(),
                title: "a".into(),
            },
            BoardEvent::Updated {
                task_id: "t".into(),
                status: "s".into(),
                category: "c".into(),
            },
        ];
        for c in &cases {
            assert!(!c.kind().is_empty());
        }
    }

    #[test]
    fn serde_round_trip() {
        let ev = BoardEvent::NoteAdded {
            task_id: "t-1".into(),
            note_id: "n-1".into(),
            content_preview: "hello".into(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: BoardEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }
}
