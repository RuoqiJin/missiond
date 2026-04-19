//! `MessageEvent` — conversation message logging.
//!
//! Covers v1 variants: `ConversationMessageLogged`, `ImageMessageInserted`.

use serde::{Deserialize, Serialize};

use super::super::domain::Domain;
use super::super::event_trait::DomainEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageEvent {
    /// A new conversation message was logged (user or assistant).
    Logged {
        message_id: i64,
        session_id: String,
        parent_session_id: Option<String>,
        /// Slot ID if this is a PTY slot session (None = master CLI session).
        slot_id: Option<String>,
        role: String,
        content_chars: usize,
        /// Truncated preview for timeline display.
        preview: String,
    },
    /// A message with image content was inserted, needs async vision processing.
    ImageInserted {
        message_id: i64,
        session_id: String,
    },
}

impl DomainEvent for MessageEvent {
    fn domain() -> Domain {
        Domain::Message
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Logged { .. } => "logged",
            Self::ImageInserted { .. } => "image_inserted",
        }
    }

    fn payload_size_hint(&self) -> usize {
        match self {
            // `preview` is typically ≤400 chars; `content_chars` is a number
            // but we keep a comfortable ceiling for wide previews.
            Self::Logged { .. } => 4096,
            Self::ImageInserted { .. } => 256,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_message() {
        assert_eq!(MessageEvent::domain(), Domain::Message);
    }

    #[test]
    fn kind_returns_non_empty_for_every_variant() {
        let cases = [
            MessageEvent::Logged {
                message_id: 0,
                session_id: "s".into(),
                parent_session_id: None,
                slot_id: None,
                role: "user".into(),
                content_chars: 0,
                preview: "".into(),
            },
            MessageEvent::ImageInserted {
                message_id: 0,
                session_id: "s".into(),
            },
        ];
        for c in &cases {
            assert!(!c.kind().is_empty());
        }
    }

    #[test]
    fn serde_round_trip() {
        let ev = MessageEvent::Logged {
            message_id: 42,
            session_id: "sess-1".into(),
            parent_session_id: Some("parent".into()),
            slot_id: Some("slot-x".into()),
            role: "assistant".into(),
            content_chars: 1024,
            preview: "hi there".into(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: MessageEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn payload_size_hint_big_for_logged() {
        let big = MessageEvent::Logged {
            message_id: 0,
            session_id: "s".into(),
            parent_session_id: None,
            slot_id: None,
            role: "user".into(),
            content_chars: 0,
            preview: "".into(),
        };
        assert!(big.payload_size_hint() > 1000);
    }
}
