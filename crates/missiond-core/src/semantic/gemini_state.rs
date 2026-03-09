//! Gemini CLI state parser — detects terminal states from Gemini CLI's Ink TUI.
//!
//! Gemini CLI uses React Ink for full-screen TUI rendering. Terminal output contains
//! box drawing characters (╭╮╰╯│─), braille spinners (⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏), and
//! structured UI components.
//!
//! Detection strategy: bottom-up scan with box drawing sanitization.

use once_cell::sync::Lazy;
use regex::Regex;

use super::types::{ParserContext, ParserMeta, State, StateDetectionResult, StateParser};

/// Box drawing characters to strip for clean text analysis.
static BOX_DRAWING_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[╭╮╰╯│─├┤┬┴┼┌┐└┘╌╍]").unwrap());
/// Braille spinner characters used by Ink's `dots` spinner type.
static SPINNER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]").unwrap());
/// Gemini prompt indicator: `> ` at start of cleaned line (inside bordered input box).
static PROMPT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*>\s").unwrap());
/// Thinking/loading indicator keywords.
static THINKING_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(Thinking\s*\.\.\.?|esc to cancel)").unwrap());
/// Error indicator.
static ERROR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^Error:|error:").unwrap());
/// Footer signature: `/model ` prefix indicates the Gemini footer bar.
static FOOTER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"/model\s+\S").unwrap());
/// Tool execution indicators (checklist items with status).
static TOOL_EXEC_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(Executing|Running|✓|✗|⠏)\s+\w").unwrap());
/// Placeholder text in empty input prompt.
static PLACEHOLDER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"Type your message|@path/to/file").unwrap());

/// State parser for Gemini CLI interactive terminal (Ink TUI).
pub struct GeminiCliStateParser {
    meta: ParserMeta,
}

impl GeminiCliStateParser {
    pub fn new() -> Self {
        Self {
            meta: ParserMeta {
                name: "gemini-cli".to_string(),
                description: "Gemini CLI (Ink TUI) state parser".to_string(),
                priority: 10,
                version: "1.0.0".to_string(),
            },
        }
    }

    /// Strip box drawing characters and trailing whitespace for clean text analysis.
    fn sanitize_line(line: &str) -> String {
        let cleaned = BOX_DRAWING_RE.replace_all(line, " ");
        cleaned.trim_end().to_string()
    }
}

impl StateParser for GeminiCliStateParser {
    fn meta(&self) -> &ParserMeta {
        &self.meta
    }

    fn detect_state(&self, context: &ParserContext) -> Option<StateDetectionResult> {
        // Sanitize and filter empty lines
        let lines: Vec<String> = context
            .last_lines
            .iter()
            .map(|l| Self::sanitize_line(l))
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
        // Check bottom 20 lines (covers footer + input + loading indicator)
        let scan_depth = non_empty.len().min(20);
        for line in non_empty.iter().rev().take(scan_depth) {
            if SPINNER_RE.is_match(line) {
                has_spinner = true;
            }
            if THINKING_RE.is_match(line) {
                has_thinking = true;
            }
            if PROMPT_RE.is_match(line) {
                has_prompt = true;
            }
            if FOOTER_RE.is_match(line) {
                has_footer = true;
            }
            if ERROR_RE.is_match(line) {
                has_error = true;
            }
            if TOOL_EXEC_RE.is_match(line) {
                has_tool_exec = true;
            }
            if PLACEHOLDER_RE.is_match(line) {
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
        assert_eq!(
            GeminiCliStateParser::sanitize_line("│ > hello │"),
            "  > hello"
        );
        assert_eq!(
            GeminiCliStateParser::sanitize_line("╭─────╮"),
            ""  // all replaced with spaces, then trim_end removes them
        );
    }
}
