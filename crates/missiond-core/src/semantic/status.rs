//! Claude Code status parser
//!
//! Parses status bar information (spinner + status text) from Claude Code CLI output.

use once_cell::sync::Lazy;
use regex::Regex;

use super::types::{ClaudeCodeStatus, ParserContext, ParserMeta, StatusParser, StatusPhase};

/// Spinner characters used by Claude Code
pub const SPINNER_CHARS: &[char] = &['·', '✻', '✽', '✶', '✳', '✢'];

/// Status text pattern: spinner + text + (stats/info)
///
/// Claude Code v2.x has two formats:
/// - Legacy: "· Precipitating… (esc to interrupt · thinking)"
/// - v2.x:   "✢ Undulating… (3m 2s · ↓ 2.8k tokens · thinking)"
///
/// This pattern matches both: spinner + status_text + (anything in parens)
static STATUS_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^([·✻✽✶✳✢])\s+(\S+…?)\s*\((.+)\)\s*$").unwrap()
});

/// Claude Code status parser
///
/// Parses status bar information:
/// - Spinner character
/// - Status text (e.g., "Precipitating...")
/// - Phase hint (thinking, tool, etc.)
/// - Interruptible state
pub struct ClaudeCodeStatusParser {
    meta: ParserMeta,
}

impl Default for ClaudeCodeStatusParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeCodeStatusParser {
    /// Create a new Claude Code status parser
    pub fn new() -> Self {
        Self {
            meta: ParserMeta {
                name: "claude-code-status".to_string(),
                description: "Parses Claude Code status bar (spinner + status text)".to_string(),
                priority: 95,
                version: "1.0.0".to_string(),
            },
        }
    }

    /// Extract phase hint from parenthesized status info.
    ///
    /// Formats:
    /// - "thinking" → Some("thinking") (simple, single word)
    /// - "thought for 14s" → Some("thought") (simple, first word)
    /// - "esc to interrupt · thinking" → Some("thinking") (legacy)
    /// - "3m 2s · ↓ 2.8k tokens · thinking" → Some("thinking") (v2.x)
    /// - "3m 27s · ↓ 3.5k tokens · thought for 14s" → Some("thought")
    /// - "esc to interrupt" → None (no phase hint)
    fn extract_phase_from_parens(parens_content: &str) -> Option<String> {
        if parens_content.contains('·') {
            // Multi-segment: take last segment's first word
            let parts: Vec<&str> = parens_content.split('·').collect();
            let last_part = parts.last()?.trim();
            let first_word = last_part.split_whitespace().next()?;
            if first_word.len() >= 3 && first_word.chars().all(|c| c.is_alphabetic()) {
                // Skip non-phase words like "esc", "interrupt"
                if matches!(first_word, "esc" | "interrupt" | "shift" | "tab") {
                    return None;
                }
                return Some(first_word.to_lowercase());
            }
            None
        } else {
            // Simple format: single word or "word for N"
            let first_word = parens_content.trim().split_whitespace().next()?;
            if first_word.len() >= 3 && first_word.chars().all(|c| c.is_alphabetic()) {
                // Skip non-phase words
                if matches!(first_word, "esc" | "interrupt" | "shift" | "tab") {
                    return None;
                }
                return Some(first_word.to_lowercase());
            }
            None
        }
    }

    /// Determine phase from hint or status text
    fn determine_phase(&self, spinner: &str, status_text: &str, phase_hint: Option<&str>) -> StatusPhase {
        // Check phase hint first
        if let Some(hint) = phase_hint {
            match hint {
                "thinking" | "thought" => return StatusPhase::Thinking,
                "tool" | "running" => return StatusPhase::ToolRunning,
                _ => {}
            }
        }

        // Check status text for tool indicators
        let status_lower = status_text.to_lowercase();
        if status_lower.contains("tool") {
            return StatusPhase::ToolRunning;
        }

        // Default to thinking if spinner is active
        if SPINNER_CHARS.iter().any(|c| spinner.contains(*c)) {
            return StatusPhase::Thinking;
        }

        StatusPhase::Unknown
    }
}

impl StatusParser for ClaudeCodeStatusParser {
    fn meta(&self) -> &ParserMeta {
        &self.meta
    }

    fn can_parse(&self, context: &ParserContext) -> bool {
        context
            .last_lines
            .iter()
            .any(|line| STATUS_PATTERN.is_match(line.trim()))
    }

    fn parse(&self, context: &ParserContext) -> Option<ClaudeCodeStatus> {
        for line in &context.last_lines {
            let trimmed = line.trim();
            if let Some(caps) = STATUS_PATTERN.captures(trimmed) {
                let spinner = caps.get(1)?.as_str().to_string();
                let status_text = caps.get(2)?.as_str().to_string();
                let parens_content = caps.get(3).map(|m| m.as_str()).unwrap_or("");

                // Extract phase hint from the parens content
                let phase_hint = Self::extract_phase_from_parens(parens_content);
                let phase = self.determine_phase(
                    &spinner,
                    &status_text,
                    phase_hint.as_deref(),
                );

                let interruptible = parens_content.contains("interrupt")
                    || parens_content.contains("esc")
                    || parens_content.contains("ESC");

                return Some(ClaudeCodeStatus {
                    spinner,
                    status_text,
                    phase,
                    interruptible,
                });
            }
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

    #[test]
    fn test_spinner_chars() {
        assert_eq!(SPINNER_CHARS.len(), 6);
        assert!(SPINNER_CHARS.contains(&'·'));
        assert!(SPINNER_CHARS.contains(&'✻'));
        assert!(SPINNER_CHARS.contains(&'✽'));
        assert!(SPINNER_CHARS.contains(&'✶'));
        assert!(SPINNER_CHARS.contains(&'✳'));
        assert!(SPINNER_CHARS.contains(&'✢'));
    }

    #[test]
    fn test_can_parse_with_status() {
        let parser = ClaudeCodeStatusParser::new();

        // Legacy format
        let context = make_context(&["· Precipitating… (esc to interrupt · thinking)"]);
        assert!(parser.can_parse(&context));

        let context = make_context(&["✻ Schlepping… (esc to interrupt)"]);
        assert!(parser.can_parse(&context));

        // v2.x format (real output from Claude Code v2.1.50)
        let context =
            make_context(&["✢ Undulating… (3m 2s · ↓ 2.8k tokens · thinking)"]);
        assert!(parser.can_parse(&context));

        let context =
            make_context(&["✽ Undulating… (3m 27s · ↓ 3.5k tokens · thought for 14s)"]);
        assert!(parser.can_parse(&context));
    }

    #[test]
    fn test_can_parse_without_status() {
        let parser = ClaudeCodeStatusParser::new();

        let context = make_context(&["random text", "no status here"]);
        assert!(!parser.can_parse(&context));

        let context = make_context(&["❯ "]);
        assert!(!parser.can_parse(&context));
    }

    #[test]
    fn test_parse_v2_thinking() {
        let parser = ClaudeCodeStatusParser::new();

        let context =
            make_context(&["✢ Undulating… (3m 2s · ↓ 2.8k tokens · thinking)"]);
        let result = parser.parse(&context);

        assert!(result.is_some());
        let status = result.unwrap();
        assert_eq!(status.spinner, "✢");
        assert_eq!(status.status_text, "Undulating…");
        assert_eq!(status.phase, StatusPhase::Thinking);
    }

    #[test]
    fn test_parse_v2_thought_for() {
        let parser = ClaudeCodeStatusParser::new();

        let context =
            make_context(&["✽ Undulating… (3m 27s · ↓ 3.5k tokens · thought for 14s)"]);
        let result = parser.parse(&context);

        assert!(result.is_some());
        let status = result.unwrap();
        assert_eq!(status.phase, StatusPhase::Thinking); // "thought" → Thinking
    }

    #[test]
    fn test_parse_v2_tool_phase() {
        let parser = ClaudeCodeStatusParser::new();

        let context = make_context(&["✻ Running… (5s · tool)"]);
        let result = parser.parse(&context);

        assert!(result.is_some());
        let status = result.unwrap();
        assert_eq!(status.phase, StatusPhase::ToolRunning);
    }

    #[test]
    fn test_parse_legacy_thinking_hint() {
        let parser = ClaudeCodeStatusParser::new();

        let context = make_context(&["· Precipitating… (esc to interrupt · thinking)"]);
        let result = parser.parse(&context);

        assert!(result.is_some());
        let status = result.unwrap();
        assert_eq!(status.spinner, "·");
        assert_eq!(status.status_text, "Precipitating…");
        assert_eq!(status.phase, StatusPhase::Thinking);
        assert!(status.interruptible);
    }

    #[test]
    fn test_parse_legacy_no_hint() {
        let parser = ClaudeCodeStatusParser::new();

        let context = make_context(&["✻ Schlepping… (esc to interrupt)"]);
        let result = parser.parse(&context);

        assert!(result.is_some());
        let status = result.unwrap();
        assert_eq!(status.spinner, "✻");
        assert_eq!(status.status_text, "Schlepping…");
        assert_eq!(status.phase, StatusPhase::Thinking); // Default to thinking with spinner
        assert!(status.interruptible);
    }

    #[test]
    fn test_parse_legacy_tool_hint() {
        let parser = ClaudeCodeStatusParser::new();

        let context = make_context(&["✶ Running… (esc to interrupt · tool)"]);
        let result = parser.parse(&context);

        assert!(result.is_some());
        let status = result.unwrap();
        assert_eq!(status.spinner, "✶");
        assert_eq!(status.status_text, "Running…");
        assert_eq!(status.phase, StatusPhase::ToolRunning);
        assert!(status.interruptible);
    }

    #[test]
    fn test_parse_tool_in_status_text() {
        let parser = ClaudeCodeStatusParser::new();

        let context = make_context(&["✳ Tool… (esc to interrupt)"]);
        let result = parser.parse(&context);

        assert!(result.is_some());
        let status = result.unwrap();
        assert_eq!(status.phase, StatusPhase::ToolRunning);
    }

    #[test]
    fn test_parse_all_spinners() {
        let parser = ClaudeCodeStatusParser::new();

        for spinner in SPINNER_CHARS {
            // Legacy format
            let line = format!("{} Status… (esc to interrupt)", spinner);
            let context = make_context(&[&line]);
            let result = parser.parse(&context);
            assert!(result.is_some(), "Failed for spinner (legacy): {}", spinner);
            assert_eq!(result.unwrap().spinner, spinner.to_string());

            // v2.x format
            let line = format!("{} Undulating… (3s · thinking)", spinner);
            let context = make_context(&[&line]);
            let result = parser.parse(&context);
            assert!(result.is_some(), "Failed for spinner (v2): {}", spinner);
            assert_eq!(result.unwrap().spinner, spinner.to_string());
        }
    }

    #[test]
    fn test_parse_with_leading_whitespace() {
        let parser = ClaudeCodeStatusParser::new();

        let context = make_context(&["  · Working… (esc to interrupt · thinking)"]);
        assert!(parser.can_parse(&context));

        let result = parser.parse(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().spinner, "·");
    }

    #[test]
    fn test_parse_returns_none_for_invalid() {
        let parser = ClaudeCodeStatusParser::new();

        let context = make_context(&["random text"]);
        assert!(parser.parse(&context).is_none());

        let context = make_context(&["· Missing parentheses"]);
        assert!(parser.parse(&context).is_none());

        let context = make_context(&["X Invalid… (esc to interrupt)"]);
        assert!(parser.parse(&context).is_none());
    }

    #[test]
    fn test_interruptible_detection() {
        let parser = ClaudeCodeStatusParser::new();

        // Legacy format: has "esc to interrupt"
        let context = make_context(&["· Working… (esc to interrupt)"]);
        let status = parser.parse(&context).unwrap();
        assert!(status.interruptible);

        // v2.x format: no "esc to interrupt" text
        let context = make_context(&["✢ Undulating… (3m 2s · ↓ 2.8k tokens · thinking)"]);
        let status = parser.parse(&context).unwrap();
        assert!(!status.interruptible); // v2 format doesn't explicitly say interruptible
    }

    #[test]
    fn test_parser_meta() {
        let parser = ClaudeCodeStatusParser::new();
        let meta = parser.meta();

        assert_eq!(meta.name, "claude-code-status");
        assert_eq!(meta.priority, 95);
        assert_eq!(meta.version, "1.0.0");
    }

    #[test]
    fn test_multiple_lines_finds_status() {
        let parser = ClaudeCodeStatusParser::new();

        let context = make_context(&[
            "Some output text",
            "More output",
            "· Processing… (esc to interrupt · thinking)",
            "Other stuff",
        ]);

        assert!(parser.can_parse(&context));
        let result = parser.parse(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().status_text, "Processing…");
    }

    #[test]
    fn test_extract_phase_from_parens() {
        // Simple format (v2.x short)
        assert_eq!(
            ClaudeCodeStatusParser::extract_phase_from_parens("thinking"),
            Some("thinking".to_string())
        );
        assert_eq!(
            ClaudeCodeStatusParser::extract_phase_from_parens("thought for 14s"),
            Some("thought".to_string())
        );
        assert_eq!(
            ClaudeCodeStatusParser::extract_phase_from_parens("tool"),
            Some("tool".to_string())
        );

        // Multi-segment with · separator
        assert_eq!(
            ClaudeCodeStatusParser::extract_phase_from_parens("esc to interrupt · thinking"),
            Some("thinking".to_string())
        );
        assert_eq!(
            ClaudeCodeStatusParser::extract_phase_from_parens("3m 2s · ↓ 2.8k tokens · thinking"),
            Some("thinking".to_string())
        );
        assert_eq!(
            ClaudeCodeStatusParser::extract_phase_from_parens("3m 27s · ↓ 3.5k tokens · thought for 14s"),
            Some("thought".to_string())
        );
        assert_eq!(
            ClaudeCodeStatusParser::extract_phase_from_parens("5s · tool"),
            Some("tool".to_string())
        );

        // No valid phase hint
        assert_eq!(
            ClaudeCodeStatusParser::extract_phase_from_parens("esc to interrupt"),
            None  // "esc" is in the skip list
        );
        assert_eq!(
            ClaudeCodeStatusParser::extract_phase_from_parens("3m 2s · ↓ 2.8k"),
            None  // "↓" is not alphabetic
        );
        assert_eq!(
            ClaudeCodeStatusParser::extract_phase_from_parens("5s"),
            None  // "5s" is not alphabetic
        );
    }
}
