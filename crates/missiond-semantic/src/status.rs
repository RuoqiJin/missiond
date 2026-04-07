//! Claude Code status parser
//!
//! Parses status bar information (spinner + status text) from Claude Code CLI output.
//! Regex patterns loaded from external CompiledPatterns (hot-reloadable YAML).

use std::sync::Arc;

use super::patterns::CompiledPatterns;
use super::types::{ClaudeCodeStatus, ParserContext, ParserMeta, StatusParser, StatusPhase};

/// Legacy fallback: spinner characters (used only when CompiledPatterns unavailable)
pub const SPINNER_CHARS: &[char] = &['·', '✻', '✽', '✶', '✳', '✢', '*'];

/// Claude Code status parser
///
/// Parses status bar information:
/// - Spinner character
/// - Status text (e.g., "Precipitating...")
/// - Phase hint (thinking, tool, etc.)
/// - Interruptible state
pub struct ClaudeCodeStatusParser {
    meta: ParserMeta,
    patterns: Arc<CompiledPatterns>,
}

impl ClaudeCodeStatusParser {
    /// Create with external patterns
    pub fn with_patterns(patterns: Arc<CompiledPatterns>) -> Self {
        Self {
            meta: ParserMeta {
                name: "claude-code-status".to_string(),
                description: "Parses Claude Code status bar (spinner + status text)".to_string(),
                priority: 95,
                version: "2.0.0".to_string(),
            },
            patterns,
        }
    }

    /// Create with default embedded patterns
    pub fn new() -> Self {
        let patterns = Arc::new(
            super::patterns::default_compiled(missiond_shared::CliEngine::ClaudeCode)
                .expect("embedded claude-code patterns must parse"),
        );
        Self::with_patterns(patterns)
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
    fn extract_phase_from_parens(&self, parens_content: &str) -> Option<String> {
        let skip_words = self.patterns.phase_skip_words();

        let extract_first_word = |text: &str| -> Option<String> {
            let first_word = text.trim().split_whitespace().next()?;
            if first_word.len() >= 3 && first_word.chars().all(|c| c.is_alphabetic()) {
                if skip_words.iter().any(|w| w == first_word) {
                    return None;
                }
                return Some(first_word.to_lowercase());
            }
            None
        };

        if parens_content.contains('·') {
            let parts: Vec<&str> = parens_content.split('·').collect();
            let last_part = parts.last()?.trim();
            extract_first_word(last_part)
        } else {
            extract_first_word(parens_content)
        }
    }

    /// Determine phase from hint or status text
    fn determine_phase(&self, spinner: &str, status_text: &str, phase_hint: Option<&str>) -> StatusPhase {
        let keywords = self.patterns.phase_keywords();

        // Check phase hint against configured keywords
        if let Some(hint) = phase_hint {
            if let Some(thinking_words) = keywords.get("thinking") {
                if thinking_words.iter().any(|w| w == hint) {
                    return StatusPhase::Thinking;
                }
            }
            if let Some(tool_words) = keywords.get("tool_running") {
                if tool_words.iter().any(|w| w == hint) {
                    return StatusPhase::ToolRunning;
                }
            }
        }

        // Check status text for tool indicators
        let status_lower = status_text.to_lowercase();
        if status_lower.contains("tool") {
            return StatusPhase::ToolRunning;
        }

        // Default to thinking if spinner is active
        if spinner.chars().any(|c| self.patterns.is_spinner_char(c)) {
            return StatusPhase::Thinking;
        }

        StatusPhase::Unknown
    }
}

impl Default for ClaudeCodeStatusParser {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusParser for ClaudeCodeStatusParser {
    fn meta(&self) -> &ParserMeta {
        &self.meta
    }

    fn can_parse(&self, context: &ParserContext) -> bool {
        let Some(re) = self.patterns.regex("status_bar.pattern") else {
            return false;
        };
        context
            .last_lines
            .iter()
            .any(|line| re.is_match(line.trim()))
    }

    fn parse(&self, context: &ParserContext) -> Option<ClaudeCodeStatus> {
        let re = self.patterns.regex("status_bar.pattern")?;

        for line in &context.last_lines {
            let trimmed = line.trim();
            if let Some(caps) = re.captures(trimmed) {
                let spinner = caps.get(1)?.as_str().to_string();
                let status_text = caps.get(2)?.as_str().to_string();
                let parens_content = caps.get(3).map(|m| m.as_str()).unwrap_or("");

                let phase_hint = self.extract_phase_from_parens(parens_content);
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
    fn test_spinner_chars_legacy() {
        assert_eq!(SPINNER_CHARS.len(), 7);
    }

    #[test]
    fn test_can_parse_with_status() {
        let parser = ClaudeCodeStatusParser::new();

        // Legacy format
        let context = make_context(&["· Precipitating… (esc to interrupt · thinking)"]);
        assert!(parser.can_parse(&context));

        let context = make_context(&["✻ Schlepping… (esc to interrupt)"]);
        assert!(parser.can_parse(&context));

        // v2.x format
        let context = make_context(&["✢ Undulating… (3m 2s · ↓ 2.8k tokens · thinking)"]);
        assert!(parser.can_parse(&context));

        let context = make_context(&["✽ Undulating… (3m 27s · ↓ 3.5k tokens · thought for 14s)"]);
        assert!(parser.can_parse(&context));
    }

    #[test]
    fn test_can_parse_ascii_spinner() {
        let parser = ClaudeCodeStatusParser::new();

        let context = make_context(&["* Honking… (3s · thinking)"]);
        assert!(parser.can_parse(&context));

        let result = parser.parse(&context);
        assert!(result.is_some());
        let status = result.unwrap();
        assert_eq!(status.spinner, "*");
        assert_eq!(status.status_text, "Honking…");
        assert_eq!(status.phase, StatusPhase::Thinking);
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

        let context = make_context(&["✢ Undulating… (3m 2s · ↓ 2.8k tokens · thinking)"]);
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

        let context = make_context(&["✽ Undulating… (3m 27s · ↓ 3.5k tokens · thought for 14s)"]);
        let result = parser.parse(&context);

        assert!(result.is_some());
        let status = result.unwrap();
        assert_eq!(status.phase, StatusPhase::Thinking);
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
        assert_eq!(status.phase, StatusPhase::Thinking);
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

        for spinner in &parser.patterns.spinner_chars {
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

        let context = make_context(&["· Working… (esc to interrupt)"]);
        let status = parser.parse(&context).unwrap();
        assert!(status.interruptible);

        let context = make_context(&["✢ Undulating… (3m 2s · ↓ 2.8k tokens · thinking)"]);
        let status = parser.parse(&context).unwrap();
        assert!(!status.interruptible);
    }

    #[test]
    fn test_parser_meta() {
        let parser = ClaudeCodeStatusParser::new();
        let meta = parser.meta();

        assert_eq!(meta.name, "claude-code-status");
        assert_eq!(meta.priority, 95);
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
        let parser = ClaudeCodeStatusParser::new();

        assert_eq!(parser.extract_phase_from_parens("thinking"), Some("thinking".to_string()));
        assert_eq!(parser.extract_phase_from_parens("thought for 14s"), Some("thought".to_string()));
        assert_eq!(parser.extract_phase_from_parens("tool"), Some("tool".to_string()));
        assert_eq!(parser.extract_phase_from_parens("esc to interrupt · thinking"), Some("thinking".to_string()));
        assert_eq!(parser.extract_phase_from_parens("3m 2s · ↓ 2.8k tokens · thinking"), Some("thinking".to_string()));
        assert_eq!(parser.extract_phase_from_parens("esc to interrupt"), None);
        assert_eq!(parser.extract_phase_from_parens("3m 2s · ↓ 2.8k"), None);
        assert_eq!(parser.extract_phase_from_parens("5s"), None);
    }
}
