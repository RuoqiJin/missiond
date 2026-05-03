//! `SystemEvent` — system-level signals (config changes, tool activity,
//! insights, proactive pushes, git commit detection).
//!
//! Covers v1 variants: `ConfigFileChanged`, `ToolCompleted`, `InsightGenerated`,
//! `JarvisProactivePush`, `ContextualCommitDetected`.
//!
//! Decision DC001:
//!   - `JarvisProactivePush` belongs here — it's a system→conversation
//!     injection, not a pure message event.
//!   - `ContextualCommitDetected` belongs here because downstream consumers
//!     (arch_maintenance, lisp_survey) react to it as a system-level
//!     repository signal, not as a memory mutation.

use serde::{Deserialize, Serialize};

use super::super::domain::Domain;
use super::super::event_trait::DomainEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SystemEvent {
    /// A monitored config file was changed.
    ConfigChanged {
        path: String,
        /// "modified", "created", "deleted".
        kind: String,
    },
    /// A high-value tool completed execution (Bash, Write, Edit, MCP tools).
    ToolCompleted {
        session_id: String,
        slot_id: Option<String>,
        tool_name: String,
        status: String,
        is_error: bool,
        input_summary: Option<String>,
        output_summary: String,
    },
    /// Timeline Analyst generated an insight from periodic analysis.
    InsightGenerated {
        category: String,
        priority: String,
        title: String,
    },
    /// MissionD proactively pushed context to a Jarvis conversation.
    JarvisProactivePush {
        conversation_id: String,
        /// "user_stuck" | "direction_shift" | "scope_creep".
        trigger_reason: String,
        summary: String,
    },
    /// A git commit was detected from a Claude Code conversation tool_result.
    ContextualCommitDetected {
        commit_hash: String,
        branch: String,
        summary: String,
        conversation_id: String,
        message_id: i64,
        session_id: String,
        slot_id: Option<String>,
    },
    /// An external project/service reported a durable domain event into
    /// MissionD's bus. Payload is stringified JSON so SystemEvent keeps Eq
    /// semantics while preserving the original service envelope.
    ExternalServiceEvent {
        service_id: String,
        event_id: String,
        event_kind: String,
        summary: String,
        trace_id: Option<String>,
        payload_json: String,
    },
}

impl DomainEvent for SystemEvent {
    fn domain() -> Domain {
        Domain::System
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::ConfigChanged { .. } => "config_changed",
            Self::ToolCompleted { .. } => "tool_completed",
            Self::InsightGenerated { .. } => "insight_generated",
            Self::JarvisProactivePush { .. } => "jarvis_proactive_push",
            Self::ContextualCommitDetected { .. } => "contextual_commit_detected",
            Self::ExternalServiceEvent { .. } => "external_service_event",
        }
    }

    fn payload_size_hint(&self) -> usize {
        match self {
            // Tool output_summary can be a few KB.
            Self::ToolCompleted { .. } => 4096,
            // Commit summary can hold diff snippets / commit subject+body.
            Self::ContextualCommitDetected { .. } => 4096,
            // External service envelopes carry compact JSON and summaries.
            Self::ExternalServiceEvent { .. } => 4096,
            // Proactive push summary is bounded but not tiny.
            Self::JarvisProactivePush { .. } => 2048,
            _ => 256,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_system() {
        assert_eq!(SystemEvent::domain(), Domain::System);
    }

    #[test]
    fn kind_returns_non_empty_for_every_variant() {
        let cases = [
            SystemEvent::ConfigChanged {
                path: "/tmp/x".into(),
                kind: "modified".into(),
            },
            SystemEvent::ToolCompleted {
                session_id: "s".into(),
                slot_id: None,
                tool_name: "bash".into(),
                status: "ok".into(),
                is_error: false,
                input_summary: None,
                output_summary: "ok".into(),
            },
            SystemEvent::InsightGenerated {
                category: "c".into(),
                priority: "p".into(),
                title: "t".into(),
            },
            SystemEvent::JarvisProactivePush {
                conversation_id: "c".into(),
                trigger_reason: "user_stuck".into(),
                summary: "s".into(),
            },
            SystemEvent::ContextualCommitDetected {
                commit_hash: "abc".into(),
                branch: "main".into(),
                summary: "s".into(),
                conversation_id: "c".into(),
                message_id: 0,
                session_id: "s".into(),
                slot_id: None,
            },
            SystemEvent::ExternalServiceEvent {
                service_id: "auth".into(),
                event_id: "evt-1".into(),
                event_kind: "login_succeeded".into(),
                summary: "auth login succeeded".into(),
                trace_id: Some("trace-1".into()),
                payload_json: "{}".into(),
            },
        ];
        for c in &cases {
            assert!(!c.kind().is_empty());
        }
    }

    #[test]
    fn serde_round_trip() {
        let ev = SystemEvent::ContextualCommitDetected {
            commit_hash: "deadbeef".into(),
            branch: "refactor/event-bus-v2".into(),
            summary: "phase 1 schema".into(),
            conversation_id: "conv-1".into(),
            message_id: 123,
            session_id: "sess-1".into(),
            slot_id: None,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: SystemEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn payload_size_hint_big_for_tool_completed() {
        let big = SystemEvent::ToolCompleted {
            session_id: "s".into(),
            slot_id: None,
            tool_name: "bash".into(),
            status: "ok".into(),
            is_error: false,
            input_summary: None,
            output_summary: "output".into(),
        };
        assert!(big.payload_size_hint() > 1000);
    }

    #[test]
    fn external_service_event_serde_round_trip() {
        let ev = SystemEvent::ExternalServiceEvent {
            service_id: "auth".into(),
            event_id: "evt-auth-1".into(),
            event_kind: "login_failed".into(),
            summary: "auth login failed".into(),
            trace_id: Some("trace-auth-1".into()),
            payload_json: r#"{"tenant_id":7}"#.into(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: SystemEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        assert_eq!(back.kind(), "external_service_event");
    }
}
