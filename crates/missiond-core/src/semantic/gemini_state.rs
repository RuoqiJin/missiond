//! Gemini CLI state parser — detects terminal states from Gemini CLI's Ink TUI.
//!
//! Gemini CLI uses React Ink for full-screen TUI rendering. Terminal output contains
//! box drawing characters (╭╮╰╯│─), braille spinners (⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏), and
//! structured UI components.
//!
//! Detection strategy: bottom-up scan with box drawing sanitization.

use std::sync::Arc;

use super::patterns::CompiledPatterns;
use super::types::{ParserContext, ParserMeta, State, StateDetectionResult, StateParser};

/// State parser for Gemini CLI interactive terminal (Ink TUI).
pub struct GeminiCliStateParser {
    meta: ParserMeta,
    patterns: Arc<CompiledPatterns>,
}

impl GeminiCliStateParser {
    /// Create with external patterns
    pub fn with_patterns(patterns: Arc<CompiledPatterns>) -> Self {
        Self {
            meta: ParserMeta {
                name: "gemini-cli".to_string(),
                description: "Gemini CLI (Ink TUI) state parser".to_string(),
                priority: 10,
                version: "1.0.0".to_string(),
            },
            patterns,
        }
    }

    /// Create with default embedded patterns
    pub fn new() -> Self {
        let patterns = Arc::new(
            super::patterns::default_compiled(crate::types::CliEngine::Gemini)
                .expect("embedded gemini-cli patterns must parse"),
        );
        Self::with_patterns(patterns)
    }

    /// Strip box drawing characters and trailing whitespace for clean text analysis.
    fn sanitize_line(&self, line: &str) -> String {
        if let Some(re) = self.patterns.regex("box_drawing.strip") {
            let cleaned = re.replace_all(line, " ");
            cleaned.trim_end().to_string()
        } else {
            line.trim_end().to_string()
        }
    }

    /// Check if a line contains any spinner character
    fn has_spinner_char(&self, line: &str) -> bool {
        line.chars().any(|c| self.patterns.is_spinner_char(c))
    }
}

impl StateParser for GeminiCliStateParser {
    fn meta(&self) -> &ParserMeta {
        &self.meta
    }

    fn detect_state(&self, context: &ParserContext) -> Option<StateDetectionResult> {
        let prompt_re = self.patterns.regex("prompt.input");
        let thinking_re = self.patterns.regex("thinking.pattern");
        let error_re = self.patterns.regex("error.pattern");
        let footer_re = self.patterns.regex("footer.pattern");
        let tool_exec_re = self.patterns.regex("tool_exec.pattern");
        let placeholder_re = self.patterns.regex("placeholder.pattern");

        // Sanitize and filter empty lines
        let lines: Vec<String> = context
            .last_lines
            .iter()
            .map(|l| self.sanitize_line(l))
            .collect();

        let non_empty: Vec<&str> = lines.iter().filter(|l| !l.trim().is_empty()).map(|s| s.as_str()).collect();

        if non_empty.is_empty() {
            return None;
        }

        let mut has_spinner = false;
        let mut has_prompt = false;
        let mut has_thinking = false;
        let mut has_footer = false;
        let mut has_error = false;
        let mut has_tool_exec = false;
        let mut has_placeholder = false;

        // Bottom-up scan — Ink TUI layers: Footer → InputPrompt → Loading/Tools → Messages
        let scan_depth = non_empty.len().min(20);
        for line in non_empty.iter().rev().take(scan_depth) {
            if self.has_spinner_char(line) {
                has_spinner = true;
            }
            if thinking_re.map_or(false, |re| re.is_match(line)) {
                has_thinking = true;
            }
            if prompt_re.map_or(false, |re| re.is_match(line)) {
                has_prompt = true;
            }
            if footer_re.map_or(false, |re| re.is_match(line)) {
                has_footer = true;
            }
            if error_re.map_or(false, |re| re.is_match(line)) {
                has_error = true;
            }
            if tool_exec_re.map_or(false, |re| re.is_match(line)) {
                has_tool_exec = true;
            }
            if placeholder_re.map_or(false, |re| re.is_match(line)) {
                has_placeholder = true;
            }
        }

        // === Priority-based state detection ===

        // 0. Error state (highest priority)
        if has_error && !has_spinner {
            return Some(StateDetectionResult::new(State::Error, 0.85));
        }

        // 1. Thinking/Processing — spinner + thinking keywords
        if has_spinner && has_thinking {
            // Distinguish tool execution from pure thinking
            if has_tool_exec {
                return Some(StateDetectionResult::new(State::ToolRunning, 0.9));
            }
            return Some(StateDetectionResult::new(State::Thinking, 0.9));
        }

        // 2. Spinner without thinking text — likely responding (streaming output)
        if has_spinner && !has_thinking {
            return Some(StateDetectionResult::new(State::Responding, 0.8));
        }

        // 3. Tool execution visible (checklist items) without spinner — tools completing
        if has_tool_exec && !has_spinner {
            // Tools just finished, but model may still be working
            // Low confidence — let debounce handle transition
            return Some(StateDetectionResult::new(State::ToolRunning, 0.6));
        }

        // 4. Idle — prompt visible, no spinner, footer present
        if has_prompt && !has_spinner {
            return Some(StateDetectionResult::new(State::Idle, 0.95));
        }

        // 5. Placeholder text visible (empty input prompt) — also idle
        if has_placeholder && !has_spinner {
            return Some(StateDetectionResult::new(State::Idle, 0.9));
        }

        // 6. Footer visible but no prompt or spinner — transitional
        //    Gemini is between states (just finished or about to start)
        if has_footer && !has_spinner && !has_prompt {
            // Likely idle but input prompt hasn't rendered yet
            return Some(StateDetectionResult::new(State::Idle, 0.5));
        }

        // No clear signal — return None to keep current state
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_context(lines: &[&str]) -> ParserContext {
        ParserContext::new(lines.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn test_idle_with_prompt() {
        let parser = GeminiCliStateParser::new();
        let ctx = make_context(&[
            "╭──────────────────────────────────────╮",
            "│ > hello world                        │",
            "╰──────────────────────────────────────╯",
            "~/Projects (main*)  /model gemini-3.1-pro  42%",
        ]);
        let result = parser.detect_state(&ctx).unwrap();
        assert_eq!(result.state, State::Idle);
        assert!(result.confidence >= 0.9);
    }

    #[test]
    fn test_idle_with_placeholder() {
        let parser = GeminiCliStateParser::new();
        let ctx = make_context(&[
            "╭──────────────────────────────────────╮",
            "│   Type your message or @path/to/file │",
            "╰──────────────────────────────────────╯",
            "~/Projects (main*)  /model gemini-3.1-pro",
        ]);
        let result = parser.detect_state(&ctx).unwrap();
        assert_eq!(result.state, State::Idle);
    }

    #[test]
    fn test_thinking_with_spinner() {
        let parser = GeminiCliStateParser::new();
        let ctx = make_context(&[
            "Some previous output...",
            "⠙ Thinking ... (esc to cancel, 5s)",
            "╭──────────────────────────────────────╮",
            "│ >                                    │",
            "╰──────────────────────────────────────╯",
        ]);
        let result = parser.detect_state(&ctx).unwrap();
        assert_eq!(result.state, State::Thinking);
    }

    #[test]
    fn test_responding_spinner_no_thinking() {
        let parser = GeminiCliStateParser::new();
        let ctx = make_context(&[
            "Here is the response text...",
            "⠋",
            "╭──────────────────────────────────────╮",
            "│ >                                    │",
            "╰──────────────────────────────────────╯",
        ]);
        let result = parser.detect_state(&ctx).unwrap();
        assert_eq!(result.state, State::Responding);
    }

    #[test]
    fn test_tool_running() {
        let parser = GeminiCliStateParser::new();
        let ctx = make_context(&[
            "⠹ Thinking ... (esc to cancel, 12s)",
            "✓ read_file  src/main.rs",
            "Executing grep  pattern: \"todo\"",
            "╭──────────────────────────────────────╮",
            "│ >                                    │",
            "╰──────────────────────────────────────╯",
        ]);
        let result = parser.detect_state(&ctx).unwrap();
        assert_eq!(result.state, State::ToolRunning);
    }

    #[test]
    fn test_error_state() {
        let parser = GeminiCliStateParser::new();
        let ctx = make_context(&[
            "Error: Authentication failed",
            "~/Projects  /model gemini-3.1-pro",
        ]);
        let result = parser.detect_state(&ctx).unwrap();
        assert_eq!(result.state, State::Error);
    }

    #[test]
    fn test_empty_screen() {
        let parser = GeminiCliStateParser::new();
        let ctx = make_context(&["", "", ""]);
        assert!(parser.detect_state(&ctx).is_none());
    }

    #[test]
    fn test_sanitize_box_drawing() {
        let parser = GeminiCliStateParser::new();
        assert_eq!(
            parser.sanitize_line("│ > hello │"),
            "  > hello"
        );
        assert_eq!(
            parser.sanitize_line("╭─────╮"),
            ""  // all replaced with spaces, then trim_end removes them
        );
    }
}
