//! `LlmEvent` — external LLM / CLI-engine request lifecycle + tool activity.
//!
//! Folds six v1 variant families into a single domain enum tagged by
//! [`Provider`]:
//!   - LEGACY `GeminiRequestStarted` / `GeminiRequestCompleted` / `GeminiToolActivity`
//!   - LEGACY `CodexRequestStarted` / `CodexRequestCompleted`
//!   - Unified `CliRequestStarted` / `CliRequestCompleted` / `CliToolActivity`
//!
//! Phase 1 intentionally keeps both legacy and unified shapes: Phase 6 will
//! merge them after the DB migration confirms wire compatibility. The
//! `Provider` tag on the unified variants records which backend served the
//! request so downstream consumers (e.g. WS wire format) can still split by
//! provider.

use serde::{Deserialize, Serialize};

use super::super::domain::Domain;
use super::super::event_trait::DomainEvent;

/// Logical LLM provider served for this request. Used to split metrics and
/// to preserve wire-format compatibility during v1→v2 migration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Sonnet,
    Codex,
    Gemini,
    Claude,
}

impl Provider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sonnet => "sonnet",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Claude => "claude",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LlmEvent {
    /// Unified CLI engine request start (Phase 1+).
    RequestStarted {
        provider: Provider,
        request_id: String,
        caller: String,
        session_id: Option<String>,
        model: String,
        prompt_chars: usize,
        /// Full prompt text — may be large (stored in gemini_requests / DB, not
        /// typically sent to frontend).
        prompt_text: Option<String>,
        /// Engine-specific extra fields (e.g. has_image, image_hash for Codex).
        #[serde(default)]
        extra: serde_json::Value,
    },
    /// Unified CLI engine request completion.
    RequestCompleted {
        provider: Provider,
        request_id: String,
        caller: String,
        session_id: Option<String>,
        model: String,
        prompt_chars: usize,
        response_chars: usize,
        duration_ms: u64,
        status: String,
        error_msg: Option<String>,
        /// Full response text — may be large.
        response_text: Option<String>,
        /// Engine-specific extra fields (queue_wait_ms, retry_count for Gemini;
        /// input_tokens, output_tokens for Codex, etc.).
        #[serde(default)]
        extra: serde_json::Value,
    },
    /// Unified CLI engine tool activity (real-time tool use during a request).
    ToolActivity {
        provider: Provider,
        request_id: String,
        tool_seq: u32,
        activity: String,
        tool_name: Option<String>,
        input_preview: Option<String>,
        result_preview: Option<String>,
        is_error: bool,
    },
    // ── LEGACY — kept for v1 timeline row round-trip. Phase 6 folds into
    //    RequestStarted/Completed with Provider::Gemini. ──
    /// LEGACY: use `RequestStarted { provider: Gemini, .. }` in new code.
    LegacyGeminiRequestStarted {
        request_id: String,
        caller: String,
        session_id: Option<String>,
        model: String,
        prompt_chars: usize,
        prompt_text: Option<String>,
    },
    /// LEGACY: use `RequestCompleted { provider: Gemini, .. }` in new code.
    LegacyGeminiRequestCompleted {
        request_id: String,
        caller: String,
        session_id: Option<String>,
        api_mode: String,
        model: String,
        prompt_chars: usize,
        response_chars: usize,
        queue_wait_ms: u64,
        duration_ms: u64,
        retry_count: u32,
        status: String,
        error_msg: Option<String>,
        response_text: Option<String>,
    },
    /// LEGACY: use `ToolActivity { provider: Gemini, .. }` in new code.
    LegacyGeminiToolActivity {
        request_id: String,
        tool_seq: u32,
        activity: String,
        tool_name: Option<String>,
        input_preview: Option<String>,
        result_preview: Option<String>,
        is_error: bool,
    },
    /// LEGACY: use `RequestStarted { provider: Codex, .. }` in new code.
    LegacyCodexRequestStarted {
        request_id: String,
        caller: String,
        model: String,
        prompt_chars: usize,
        has_image: bool,
        prompt_text: Option<String>,
        image_hash: Option<String>,
    },
    /// LEGACY: use `RequestCompleted { provider: Codex, .. }` in new code.
    LegacyCodexRequestCompleted {
        request_id: String,
        caller: String,
        model: String,
        prompt_chars: usize,
        response_chars: usize,
        duration_ms: u64,
        status: String,
        error_msg: Option<String>,
        response_text: Option<String>,
        input_tokens: u64,
        output_tokens: u64,
        image_hash: Option<String>,
    },
}

impl DomainEvent for LlmEvent {
    fn domain() -> Domain {
        Domain::Llm
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::RequestStarted { .. } => "request_started",
            Self::RequestCompleted { .. } => "request_completed",
            Self::ToolActivity { .. } => "tool_activity",
            Self::LegacyGeminiRequestStarted { .. } => "legacy_gemini_request_started",
            Self::LegacyGeminiRequestCompleted { .. } => "legacy_gemini_request_completed",
            Self::LegacyGeminiToolActivity { .. } => "legacy_gemini_tool_activity",
            Self::LegacyCodexRequestStarted { .. } => "legacy_codex_request_started",
            Self::LegacyCodexRequestCompleted { .. } => "legacy_codex_request_completed",
        }
    }

    fn payload_size_hint(&self) -> usize {
        match self {
            // Full response text can be tens of KB.
            Self::RequestCompleted {
                response_text: Some(_),
                ..
            }
            | Self::LegacyGeminiRequestCompleted {
                response_text: Some(_),
                ..
            }
            | Self::LegacyCodexRequestCompleted {
                response_text: Some(_),
                ..
            } => 16384,
            Self::RequestStarted {
                prompt_text: Some(_),
                ..
            }
            | Self::LegacyGeminiRequestStarted {
                prompt_text: Some(_),
                ..
            }
            | Self::LegacyCodexRequestStarted {
                prompt_text: Some(_),
                ..
            } => 8192,
            Self::ToolActivity { .. } | Self::LegacyGeminiToolActivity { .. } => 4096,
            _ => 1024,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_llm() {
        assert_eq!(LlmEvent::domain(), Domain::Llm);
    }

    #[test]
    fn kind_returns_non_empty_for_every_variant() {
        let cases = vec![
            LlmEvent::RequestStarted {
                provider: Provider::Sonnet,
                request_id: "r".into(),
                caller: "c".into(),
                session_id: None,
                model: "m".into(),
                prompt_chars: 0,
                prompt_text: None,
                extra: serde_json::Value::Null,
            },
            LlmEvent::RequestCompleted {
                provider: Provider::Claude,
                request_id: "r".into(),
                caller: "c".into(),
                session_id: None,
                model: "m".into(),
                prompt_chars: 0,
                response_chars: 0,
                duration_ms: 0,
                status: "ok".into(),
                error_msg: None,
                response_text: None,
                extra: serde_json::Value::Null,
            },
            LlmEvent::ToolActivity {
                provider: Provider::Codex,
                request_id: "r".into(),
                tool_seq: 0,
                activity: "tool_use".into(),
                tool_name: None,
                input_preview: None,
                result_preview: None,
                is_error: false,
            },
            LlmEvent::LegacyGeminiRequestStarted {
                request_id: "r".into(),
                caller: "c".into(),
                session_id: None,
                model: "m".into(),
                prompt_chars: 0,
                prompt_text: None,
            },
            LlmEvent::LegacyGeminiRequestCompleted {
                request_id: "r".into(),
                caller: "c".into(),
                session_id: None,
                api_mode: "rest".into(),
                model: "m".into(),
                prompt_chars: 0,
                response_chars: 0,
                queue_wait_ms: 0,
                duration_ms: 0,
                retry_count: 0,
                status: "ok".into(),
                error_msg: None,
                response_text: None,
            },
            LlmEvent::LegacyGeminiToolActivity {
                request_id: "r".into(),
                tool_seq: 0,
                activity: "x".into(),
                tool_name: None,
                input_preview: None,
                result_preview: None,
                is_error: false,
            },
            LlmEvent::LegacyCodexRequestStarted {
                request_id: "r".into(),
                caller: "c".into(),
                model: "m".into(),
                prompt_chars: 0,
                has_image: false,
                prompt_text: None,
                image_hash: None,
            },
            LlmEvent::LegacyCodexRequestCompleted {
                request_id: "r".into(),
                caller: "c".into(),
                model: "m".into(),
                prompt_chars: 0,
                response_chars: 0,
                duration_ms: 0,
                status: "ok".into(),
                error_msg: None,
                response_text: None,
                input_tokens: 0,
                output_tokens: 0,
                image_hash: None,
            },
        ];
        for c in &cases {
            assert!(!c.kind().is_empty());
        }
    }

    #[test]
    fn serde_round_trip_provider() {
        let ev = LlmEvent::RequestCompleted {
            provider: Provider::Codex,
            request_id: "r-1".into(),
            caller: "caller".into(),
            session_id: Some("s-1".into()),
            model: "sonnet-4.7".into(),
            prompt_chars: 100,
            response_chars: 200,
            duration_ms: 500,
            status: "ok".into(),
            error_msg: None,
            response_text: Some("answer".into()),
            extra: serde_json::json!({"input_tokens": 42}),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: LlmEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn payload_size_hint_big_for_completed_with_response_text() {
        let big = LlmEvent::RequestCompleted {
            provider: Provider::Claude,
            request_id: "r".into(),
            caller: "c".into(),
            session_id: None,
            model: "m".into(),
            prompt_chars: 0,
            response_chars: 0,
            duration_ms: 0,
            status: "ok".into(),
            error_msg: None,
            response_text: Some("huge".into()),
            extra: serde_json::Value::Null,
        };
        assert!(big.payload_size_hint() > 1000);
    }
}
