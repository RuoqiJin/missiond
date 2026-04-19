//! `WorkerEvent` — inline worker telemetry (LLM gateway, translation, narration,
//! briefing).
//!
//! Covers v1 variants: `WorkerLlmCall`, `TranslationStarted/Completed/Failed`,
//! `NarrationSessionStarted/BatchCompleted/SessionCompleted/Failed`,
//! `BriefingBatchStarted`, `BriefingSummaryGenerated`.

use serde::{Deserialize, Serialize};

use super::super::domain::Domain;
use super::super::event_trait::DomainEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WorkerEvent {
    /// A unified LLM gateway (MiniMax / Sonnet / etc.) call completed.
    LlmCall {
        caller: String,
        task_id: Option<String>,
        status: String,
        prompt_chars: usize,
        response_chars: usize,
        duration_ms: u64,
        queue_wait_ms: u64,
    },

    // ── Translation worker ──
    TranslationStarted {
        message_id: i64,
        slot_id: String,
        content_chars: usize,
    },
    TranslationCompleted {
        message_id: i64,
        slot_id: String,
        preview: String,
        duration_ms: u64,
    },
    TranslationFailed {
        message_id: i64,
        slot_id: String,
        error: String,
    },

    // ── Step-narrator worker (batch-based) ──
    NarrationSessionStarted {
        session_id: String,
        total_messages: usize,
        already_narrated: usize,
    },
    NarrationBatchCompleted {
        session_id: String,
        batch_index: usize,
        processed_count: usize,
        total_messages: usize,
        duration_ms: u64,
    },
    NarrationSessionCompleted {
        session_id: String,
        total_narrated: usize,
    },
    NarrationFailed {
        session_id: String,
        batch_index: usize,
        error: String,
        will_retry: bool,
    },

    // ── Briefing worker ──
    BriefingBatchStarted {
        pending_count: usize,
    },
    BriefingSummaryGenerated {
        /// The seq of the timeline entry whose summary was updated.
        target_seq: i64,
        /// The new semantic summary.
        summary: String,
        /// How the summary was produced: "minimax", "static_rule", "tool_skip".
        method: String,
    },
}

impl DomainEvent for WorkerEvent {
    fn domain() -> Domain {
        Domain::Worker
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::LlmCall { .. } => "llm_call",
            Self::TranslationStarted { .. } => "translation_started",
            Self::TranslationCompleted { .. } => "translation_completed",
            Self::TranslationFailed { .. } => "translation_failed",
            Self::NarrationSessionStarted { .. } => "narration_session_started",
            Self::NarrationBatchCompleted { .. } => "narration_batch_completed",
            Self::NarrationSessionCompleted { .. } => "narration_session_completed",
            Self::NarrationFailed { .. } => "narration_failed",
            Self::BriefingBatchStarted { .. } => "briefing_batch_started",
            Self::BriefingSummaryGenerated { .. } => "briefing_summary_generated",
        }
    }

    fn payload_size_hint(&self) -> usize {
        match self {
            // Summary text can be a few KB.
            Self::BriefingSummaryGenerated { .. } => 4096,
            _ => 256,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_worker() {
        assert_eq!(WorkerEvent::domain(), Domain::Worker);
    }

    #[test]
    fn kind_returns_non_empty_for_every_variant() {
        let cases = vec![
            WorkerEvent::LlmCall {
                caller: "c".into(),
                task_id: None,
                status: "ok".into(),
                prompt_chars: 0,
                response_chars: 0,
                duration_ms: 0,
                queue_wait_ms: 0,
            },
            WorkerEvent::TranslationStarted {
                message_id: 0,
                slot_id: "s".into(),
                content_chars: 0,
            },
            WorkerEvent::TranslationCompleted {
                message_id: 0,
                slot_id: "s".into(),
                preview: "p".into(),
                duration_ms: 0,
            },
            WorkerEvent::TranslationFailed {
                message_id: 0,
                slot_id: "s".into(),
                error: "e".into(),
            },
            WorkerEvent::NarrationSessionStarted {
                session_id: "s".into(),
                total_messages: 0,
                already_narrated: 0,
            },
            WorkerEvent::NarrationBatchCompleted {
                session_id: "s".into(),
                batch_index: 0,
                processed_count: 0,
                total_messages: 0,
                duration_ms: 0,
            },
            WorkerEvent::NarrationSessionCompleted {
                session_id: "s".into(),
                total_narrated: 0,
            },
            WorkerEvent::NarrationFailed {
                session_id: "s".into(),
                batch_index: 0,
                error: "e".into(),
                will_retry: false,
            },
            WorkerEvent::BriefingBatchStarted { pending_count: 0 },
            WorkerEvent::BriefingSummaryGenerated {
                target_seq: 1,
                summary: "s".into(),
                method: "minimax".into(),
            },
        ];
        for c in &cases {
            assert!(!c.kind().is_empty());
        }
    }

    #[test]
    fn serde_round_trip() {
        let ev = WorkerEvent::BriefingSummaryGenerated {
            target_seq: 42,
            summary: "summary body".into(),
            method: "minimax".into(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: WorkerEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn payload_size_hint_big_for_briefing_summary() {
        let big = WorkerEvent::BriefingSummaryGenerated {
            target_seq: 1,
            summary: "x".into(),
            method: "minimax".into(),
        };
        assert!(big.payload_size_hint() > 1000);
    }
}
