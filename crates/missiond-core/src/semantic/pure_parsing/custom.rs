//! Pure function implementations for semantic terminal parsing.
//!
//! All functions are stateless, no &self, no side effects.
//! Zero external crates — pure Rust std only.

use super::generated::SemanticParsing;

pub struct SemanticParsingImpl;

impl SemanticParsing for SemanticParsingImpl {
    /// Braille pattern range: U+2800..=U+28FF
    /// Also matches common spinner chars: ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ⣾⣽⣻⢿⡿⣟⣯⣷ ◐◑◒◓ ◴◷◶◵
    fn is_spinner_char(c: char) -> bool {
        matches!(c,
            '\u{2800}'..='\u{28FF}' | // Braille patterns block (covers all braille singles)
            '◐' | '◑' | '◒' | '◓' | // Circle quarters
            '◴' | '◵' | '◶' | '◷'   // Arc spinners
        )
    }

    /// State-machine split: tracks quote depth to avoid splitting inside quotes.
    fn split_args(input: &str) -> Vec<String> {
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        let mut escape_next = false;

        for c in input.chars() {
            if escape_next {
                current.push(c);
                escape_next = false;
                continue;
            }
            match c {
                '\\' => {
                    current.push(c);
                    escape_next = true;
                }
                '"' => {
                    current.push(c);
                    in_quotes = !in_quotes;
                }
                ',' if !in_quotes => {
                    let trimmed = current.trim().to_string();
                    if !trimmed.is_empty() {
                        parts.push(trimmed);
                    }
                    current.clear();
                }
                _ => current.push(c),
            }
        }

        let trimmed = current.trim().to_string();
        if !trimmed.is_empty() {
            parts.push(trimmed);
        }
        parts
    }

    /// Splits on '·' (middle dot) separator, returns the last segment after it.
    /// "3m 2s · thinking" → Some("thinking")
    /// "1s · Read" → Some("Read")
    /// "5s" → None (no separator = no phase hint)
    fn extract_phase_from_parens(parens_content: &str) -> Option<String> {
        let parts: Vec<&str> = parens_content.split('·').collect();
        if parts.len() < 2 {
            return None;
        }
        let phase = parts.last()?.trim();
        if phase.is_empty() {
            return None;
        }
        // Skip purely numeric/timer segments
        let has_alpha = phase.chars().any(|c| c.is_alphabetic());
        if !has_alpha {
            return None;
        }
        Some(phase.to_string())
    }

    /// Remove box-drawing characters (U+2500..U+257F) and trim.
    fn sanitize_line(line: &str) -> String {
        line.chars()
            .filter(|c| !matches!(c, '\u{2500}'..='\u{257F}'))
            .collect::<String>()
            .trim()
            .to_string()
    }

    /// Matches patterns like "(5s ·" or "(3m 2s ·" — timer with middle dot.
    /// Uses char-by-char scanning, no regex.
    fn has_activity_timer(line: &str) -> bool {
        let chars: Vec<char> = line.chars().collect();
        let len = chars.len();

        // Find '(' then look for timer pattern followed by '·'
        let mut i = 0;
        while i < len {
            if chars[i] == '(' {
                // Scan for digits followed by 's' or 'm'
                let mut j = i + 1;
                let mut found_timer = false;

                while j < len && chars[j] != ')' {
                    // Skip whitespace
                    while j < len && chars[j] == ' ' {
                        j += 1;
                    }
                    if j >= len {
                        break;
                    }

                    // Check for digit+unit pattern (e.g. "5s", "3m")
                    let mut k = j;
                    while k < len && chars[k].is_ascii_digit() {
                        k += 1;
                    }
                    if k > j && k < len && (chars[k] == 's' || chars[k] == 'm') {
                        found_timer = true;
                        j = k + 1;
                        continue;
                    }

                    // Check for middle dot '·'
                    if found_timer && chars[j] == '·' {
                        return true;
                    }

                    j += 1;
                }
            }
            i += 1;
        }
        false
    }

    /// Check last non-empty line for prompt characters: ❯, >, $
    fn is_idle_prompt(lines: &[&str]) -> bool {
        let last = lines
            .iter()
            .rev()
            .find(|l| !l.trim().is_empty());

        match last {
            Some(line) => {
                let trimmed = line.trim();
                trimmed == "❯"
                    || trimmed == "$"
                    || trimmed.starts_with("> ")
                    || trimmed.starts_with("❯ ")
                    || trimmed.starts_with("$ ")
                    || trimmed == ">"
            }
            None => false,
        }
    }
}

// ═══════════════════════════════════════════════════════════
// TDD Test Suite — 17 cases from Forge declaration
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // --- is_spinner_char ---
    #[test]
    fn test_is_spinner_char_braille_dot() {
        assert_eq!(SemanticParsingImpl::is_spinner_char('⠋'), true);
    }

    #[test]
    fn test_is_spinner_char_braille_bar() {
        assert_eq!(SemanticParsingImpl::is_spinner_char('⣾'), true);
    }

    #[test]
    fn test_is_spinner_char_normal_a() {
        assert_eq!(SemanticParsingImpl::is_spinner_char('A'), false);
    }

    #[test]
    fn test_is_spinner_char_space() {
        assert_eq!(SemanticParsingImpl::is_spinner_char(' '), false);
    }

    // --- split_args ---
    #[test]
    fn test_split_args_simple() {
        let result = SemanticParsingImpl::split_args("a, b, c");
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_split_args_quoted() {
        let result = SemanticParsingImpl::split_args("name: \"foo\", path: /src");
        assert_eq!(result, vec!["name: \"foo\"", "path: /src"]);
    }

    // --- extract_phase_from_parens ---
    #[test]
    fn test_extract_phase_thinking() {
        let result = SemanticParsingImpl::extract_phase_from_parens("3m 2s · thinking");
        assert_eq!(result, Some("thinking".into()));
    }

    #[test]
    fn test_extract_phase_tool() {
        let result = SemanticParsingImpl::extract_phase_from_parens("1s · Read");
        assert_eq!(result, Some("Read".into()));
    }

    #[test]
    fn test_extract_phase_just_timer() {
        let result = SemanticParsingImpl::extract_phase_from_parens("5s");
        assert_eq!(result, None);
    }

    // --- sanitize_line ---
    #[test]
    fn test_sanitize_line_clean() {
        let result = SemanticParsingImpl::sanitize_line("hello world");
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_sanitize_line_box_chars() {
        let result = SemanticParsingImpl::sanitize_line("│ hello │");
        assert_eq!(result, "hello");
    }

    // --- has_activity_timer ---
    #[test]
    fn test_has_activity_timer_seconds() {
        assert_eq!(
            SemanticParsingImpl::has_activity_timer("(5s · thinking)"),
            true
        );
    }

    #[test]
    fn test_has_activity_timer_min_sec() {
        assert_eq!(
            SemanticParsingImpl::has_activity_timer("(3m 2s · Read)"),
            true
        );
    }

    #[test]
    fn test_has_activity_timer_no_timer() {
        assert_eq!(
            SemanticParsingImpl::has_activity_timer("hello world"),
            false
        );
    }

    // --- is_idle_prompt ---
    #[test]
    fn test_is_idle_prompt_chevron() {
        assert_eq!(SemanticParsingImpl::is_idle_prompt(&["❯"]), true);
    }

    #[test]
    fn test_is_idle_prompt_gt() {
        assert_eq!(SemanticParsingImpl::is_idle_prompt(&["> "]), true);
    }

    #[test]
    fn test_is_idle_prompt_output() {
        assert_eq!(
            SemanticParsingImpl::is_idle_prompt(&["Hello world"]),
            false
        );
    }
}
