//! Claude Code state parser
//!
//! Detects Claude Code CLI states from terminal output.

use once_cell::sync::Lazy;
use regex::Regex;

use super::types::{
    ConfirmType, ParserContext, ParserMeta, State, StateDetectionResult, StateMeta, StateParser,
};

/// Regex patterns for state detection
static OPTION_CONFIRM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?mi)^[\s❯>]*1\.\s*(Yes|Allow)").unwrap());

static YES_NO_CONFIRM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\[Y/n\]|\(yes/no\)|Allow\?|Do you want to proceed").unwrap());

static PROMPT_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[❯>]\s*").unwrap());

/// Claude Code state parser
///
/// Detects various CLI states:
/// - `starting`: Initial startup, may need trust confirmation
/// - `idle`: Prompt visible, waiting for input
/// - `thinking`: Processing (esc to interrupt visible)
/// - `tool_running`: Running a tool
/// - `confirming`: Waiting for user confirmation
/// - `error`: Error state
pub struct ClaudeCodeStateParser {
    meta: ParserMeta,
}

impl Default for ClaudeCodeStateParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeCodeStateParser {
    /// Create a new Claude Code state parser
    pub fn new() -> Self {
        Self {
            meta: ParserMeta {
                name: "claude-code-state".to_string(),
                description: "Detects Claude Code CLI states".to_string(),
                priority: 100,
                version: "1.0.0".to_string(),
            },
        }
    }

    /// Check if text indicates running state (spinner visible)
    fn is_running(&self, text: &str) -> bool {
        text.contains("esc to interrupt")
    }

    /// Check for options-style confirmation
    fn is_option_confirm(&self, text: &str) -> bool {
        OPTION_CONFIRM_PATTERN.is_match(text) && text.contains("Esc to cancel")
    }

    /// Check for Y/n style confirmation
    fn is_yes_no_confirm(&self, text: &str) -> bool {
        YES_NO_CONFIRM_PATTERN.is_match(text)
    }

    /// Check if any line has a prompt indicator
    fn has_prompt(&self, lines: &[String]) -> bool {
        lines
            .iter()
            .any(|line| PROMPT_PATTERN.is_match(line.trim()))
    }
}

impl StateParser for ClaudeCodeStateParser {
    fn meta(&self) -> &ParserMeta {
        &self.meta
    }

    fn detect_state(&self, context: &ParserContext) -> Option<StateDetectionResult> {
        let text = context.text();

        // Check for trust dialog during startup (auto-confirm)
        if context.current_state == Some(State::Starting)
            && text.contains("Yes, proceed")
            && text.contains("Enter to confirm")
        {
            return Some(
                StateDetectionResult::new(State::Starting, 0.95).with_meta(StateMeta {
                    needs_trust_confirm: Some(true),
                    confirm_type: None,
                }),
            );
        }

        // Check for running state (spinner visible)
        let is_running = self.is_running(&text);

        // Check for confirmation dialog
        // Note: ❯ and > are prompt indicators, need to match them before option number
        let is_option_confirm = self.is_option_confirm(&text);
        let is_yes_no_confirm = self.is_yes_no_confirm(&text);

        if is_option_confirm || is_yes_no_confirm {
            let confirm_type = if is_option_confirm {
                ConfirmType::Options
            } else {
                ConfirmType::YesNo
            };

            return Some(
                StateDetectionResult::new(State::Confirming, 0.95).with_meta(StateMeta {
                    needs_trust_confirm: None,
                    confirm_type: Some(confirm_type),
                }),
            );
        }

        // Check for busy state (running tools/thinking)
        if is_running {
            // Determine if thinking or tool_running
            // Tool running: "Tool:" or spinner with vertical bar
            if text.contains("Tool:") || (text.contains('⏺') && text.contains('│')) {
                return Some(StateDetectionResult::new(State::ToolRunning, 0.85));
            }
            return Some(StateDetectionResult::new(State::Thinking, 0.9));
        }

        // Check for idle state (prompt visible, no running indicator)
        // Match prompt: ❯ or > at start of line (with optional trailing space/content)
        if self.has_prompt(&context.last_lines) && !is_running {
            return Some(StateDetectionResult::new(State::Idle, 0.9));
        }

        // Check for error state
        if text.contains("Error:") || text.contains("error:") || text.contains('✖') {
            return Some(StateDetectionResult::new(State::Error, 0.7));
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_context(lines: &[&str]) -> ParserContext {
        ParserContext::new(lines.iter().map(|s| s.to_string()).collect())
    }

    fn make_context_with_state(lines: &[&str], state: State) -> ParserContext {
        ParserContext::new(lines.iter().map(|s| s.to_string()).collect()).with_state(state)
    }

    #[test]
    fn test_detect_idle_with_prompt() {
        let parser = ClaudeCodeStateParser::new();

        // Test with ❯ prompt
        let context = make_context(&["❯ ", "some previous output"]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.state, State::Idle);
        assert!(result.confidence >= 0.9);

        // Test with > prompt
        let context = make_context(&["> ", "some output"]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::Idle);
    }

    #[test]
    fn test_detect_thinking() {
        let parser = ClaudeCodeStateParser::new();

        let context = make_context(&["Processing...", "esc to interrupt"]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.state, State::Thinking);
        assert!(result.confidence >= 0.9);
    }

    #[test]
    fn test_detect_tool_running() {
        let parser = ClaudeCodeStateParser::new();

        // Test with Tool: indicator
        let context = make_context(&["Tool: Read file", "esc to interrupt"]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::ToolRunning);

        // Test with spinner and vertical bar
        let context = make_context(&["⏺ Running command │ ls -la", "esc to interrupt"]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::ToolRunning);
    }

    #[test]
    fn test_detect_option_confirm() {
        let parser = ClaudeCodeStateParser::new();

        let context = make_context(&[
            "xjp-mcp - xjp_secret_get(key: \"test\")",
            "❯ 1. Yes, allow this action",
            "  2. Yes, allow for this session",
            "  3. No, deny this action",
            "Esc to cancel",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.state, State::Confirming);
        assert!(result.meta.is_some());
        assert_eq!(
            result.meta.unwrap().confirm_type,
            Some(ConfirmType::Options)
        );
    }

    #[test]
    fn test_detect_yesno_confirm() {
        let parser = ClaudeCodeStateParser::new();

        // Test [Y/n] format
        let context = make_context(&["Do you want to continue? [Y/n]"]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.state, State::Confirming);
        assert_eq!(
            result.meta.unwrap().confirm_type,
            Some(ConfirmType::YesNo)
        );

        // Test (yes/no) format
        let context = make_context(&["Proceed? (yes/no)"]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::Confirming);
    }

    #[test]
    fn test_detect_starting_trust_confirm() {
        let parser = ClaudeCodeStateParser::new();

        let context = make_context_with_state(
            &[
                "Do you trust this project?",
                "Yes, proceed",
                "Enter to confirm",
            ],
            State::Starting,
        );
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.state, State::Starting);
        assert!(result.meta.is_some());
        assert_eq!(result.meta.unwrap().needs_trust_confirm, Some(true));
    }

    #[test]
    fn test_detect_error() {
        let parser = ClaudeCodeStateParser::new();

        // Test Error:
        let context = make_context(&["Error: Something went wrong"]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::Error);

        // Test error:
        let context = make_context(&["error: file not found"]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::Error);

        // Test ✖
        let context = make_context(&["✖ Failed to execute command"]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::Error);
    }

    #[test]
    fn test_no_detection() {
        let parser = ClaudeCodeStateParser::new();

        let context = make_context(&["random text", "nothing special"]);
        let result = parser.detect_state(&context);
        assert!(result.is_none());
    }
}
