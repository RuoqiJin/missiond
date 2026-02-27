//! Claude Code state parser
//!
//! Detects Claude Code CLI states from terminal output.
//!
//! ## Claude Code TUI Layout
//!
//! ```text
//! [Content area - messages, responses, tool output]
//! ──────────────────── (separator)
//! ❯  (prompt / input)
//! ──────────────────── (separator)
//! ⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt  (bottom bar - PERMANENT)
//! ```
//!
//! ## Detection Strategy
//!
//! - **"esc to interrupt" is a permanent bottom bar element** — NOT a state indicator.
//! - **Spinner line** (`✳ Determining…`) is the true processing indicator.
//! - We use `last_non_empty_lines(N)` to focus on the active region.
//! - Detection order: Confirm → Idle → Processing → Responding → Error.

use once_cell::sync::Lazy;
use regex::Regex;

use super::status::SPINNER_CHARS;
use super::types::{
    ConfirmType, ParserContext, ParserMeta, State, StateDetectionResult, StateMeta, StateParser,
};

/// Phase hint extracted from spinner status line
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhaseHint {
    Thinking,
    ToolRunning,
    Unknown(String),
}

/// Regex patterns for state detection
static OPTION_CONFIRM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?mi)^[\s❯>]*1\.\s*(Yes|Allow)").unwrap());

static YES_NO_CONFIRM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\[Y/n\]|\(yes/no\)|Allow\?|Do you want to proceed").unwrap());

static PROMPT_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[❯>]\s*").unwrap());

/// Spinner line pattern: optional whitespace + spinner char + whitespace + text
/// Matches: `✳ Determining…`, `  ✻ Reading...`, `· Processing`
/// Does NOT match bottom bar: `⏵⏵ bypass permissions on ... · esc to interrupt`
static SPINNER_LINE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    let chars = SPINNER_CHARS
        .iter()
        .map(|c| regex::escape(&c.to_string()))
        .collect::<Vec<_>>()
        .join("");
    Regex::new(&format!(r"^\s*[{}]\s+\S", chars)).unwrap()
});

/// Claude Code state parser
pub struct ClaudeCodeStateParser {
    meta: ParserMeta,
}

impl Default for ClaudeCodeStateParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeCodeStateParser {
    pub fn new() -> Self {
        Self {
            meta: ParserMeta {
                name: "claude-code-state".to_string(),
                description: "Detects Claude Code CLI states".to_string(),
                priority: 100,
                version: "2.0.0".to_string(),
            },
        }
    }

    /// Check for options-style confirmation (full text, dialog spans many lines)
    fn is_option_confirm(&self, text: &str) -> bool {
        OPTION_CONFIRM_PATTERN.is_match(text) && text.contains("Esc to cancel")
    }

    /// Check for Y/n style confirmation
    fn is_yes_no_confirm(&self, text: &str) -> bool {
        YES_NO_CONFIRM_PATTERN.is_match(text)
    }

    /// Check if any line has a prompt indicator
    fn has_prompt_in(&self, lines: &[&str]) -> bool {
        lines
            .iter()
            .any(|line| PROMPT_PATTERN.is_match(line.trim()))
    }

    /// Check if the slash command autocomplete menu is visible.
    ///
    /// Detected by: prompt with `/` typed + menu items (`  /command-name    description`)
    /// Screen format (from real alacritty capture):
    /// ```text
    /// ❯ /
    /// ────────────────────
    ///   /my-deploy-agent             描述...
    ///   /my-mcp                      描述...
    /// ```
    fn has_slash_menu(&self, active_lines: &[&str]) -> bool {
        // 1. A prompt line with `/` typed (❯ / or ❯ /partial)
        //    Search in active_lines (wider window) since menu items push prompt up.
        let has_slash_prompt = active_lines.iter().any(|line| {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix('❯').or_else(|| trimmed.strip_prefix('>')) {
                rest.trim_start().starts_with('/')
            } else {
                false
            }
        });
        if !has_slash_prompt {
            return false;
        }

        // 2. At least 2 menu item lines: /command-name (starts with / + alpha)
        let menu_count = active_lines
            .iter()
            .filter(|line| {
                let trimmed = line.trim();
                trimmed.starts_with('/')
                    && trimmed.len() > 2
                    && trimmed.as_bytes().get(1).map_or(false, |b| b.is_ascii_alphabetic())
            })
            .count();
        menu_count >= 2
    }

    /// Check if any line is a spinner/status line (processing indicator)
    fn has_spinner_line(&self, lines: &[&str]) -> bool {
        lines.iter().any(|line| SPINNER_LINE_PATTERN.is_match(line))
    }

    /// Check if the spinner is actively processing (not a stale/completion spinner).
    ///
    /// Claude Code spinner formats:
    /// - **Active**: `✻ Kneading…` or `✳ Combobulating… (5m · thinking)` — always has `…`
    /// - **Completion**: `✻ Baked for 7m 11s` — no `…`, shows total elapsed time
    ///
    /// Completion spinners remain on screen after task finishes. They must NOT
    /// be treated as active processing indicators.
    fn has_active_spinner(&self, lines: &[&str]) -> bool {
        lines.iter().any(|line| {
            SPINNER_LINE_PATTERN.is_match(line)
                && (line.contains('…') || line.contains("..."))
        })
    }

    /// Check if any line looks like a tool call header.
    ///
    /// Matches lines like:
    /// - `⏺ Bash(command="ls -la")`
    /// - `⏺ Read (file.rs)`
    /// - `⏺ missiond - mission_kb_search (MCP)`
    ///
    /// Does NOT match response text that happens to contain ⏺.
    fn has_tool_call_line(&self, lines: &[&str]) -> bool {
        lines.iter().any(|line| {
            let trimmed = line.trim();
            let Some(after) = trimmed.strip_prefix('⏺') else {
                return false;
            };
            let after = after.trim_start();
            // Tool call: ⏺ followed by a tool name (capitalized word or contains "(MCP)")
            after.contains("(MCP)")
                || after.starts_with("Bash")
                || after.starts_with("Read")
                || after.starts_with("Edit")
                || after.starts_with("Write")
                || after.starts_with("Glob")
                || after.starts_with("Grep")
                || after.starts_with("Task")
                || after.starts_with("WebFetch")
                || after.starts_with("WebSearch")
                || after.starts_with("LSP")
                || after.starts_with("NotebookEdit")
        })
    }

    /// Extract phase hint from spinner status line.
    ///
    /// Claude Code v2.x status line formats:
    /// - `✢ Smooshing… (thinking)` → Thinking (simple format)
    /// - `✢ Undulating… (3m 2s · ↓ 2.8k tokens · thinking)` → Thinking
    /// - `✻ Running… (5s · tool)` → ToolRunning
    /// - `· Improvising… (esc to interrupt · thinking)` → Thinking (legacy)
    /// - `✽ Processing… (3m 27s · ↓ 3.5k tokens · thought for 14s)` → Thinking
    ///
    /// Extraction strategy:
    /// 1. If parens has `·` separator, take last segment's first word
    /// 2. If parens is a single word (like "thinking"), use it directly
    fn extract_phase_hint(&self, lines: &[&str]) -> Option<PhaseHint> {
        for line in lines {
            if !SPINNER_LINE_PATTERN.is_match(line) {
                continue;
            }
            let trimmed = line.trim();
            // Find the last (...) section
            let paren_start = trimmed.rfind('(')?;
            let paren_end = trimmed.rfind(')')?;
            if paren_end <= paren_start {
                continue;
            }
            let parens_content = &trimmed[paren_start + 1..paren_end];

            // Try to extract phase word
            let phase_word = if parens_content.contains('·') {
                // Multi-segment: take last segment after ·
                let parts: Vec<&str> = parens_content.split('·').collect();
                let last_part = parts.last()?.trim();
                let first_word = last_part.split_whitespace().next()?;
                if first_word.len() >= 3 && first_word.chars().all(|c| c.is_alphabetic()) {
                    Some(first_word)
                } else {
                    None
                }
            } else {
                // Simple format: "(thinking)" or "(thought for 14s)"
                let first_word = parens_content.trim().split_whitespace().next()?;
                if first_word.len() >= 3 && first_word.chars().all(|c| c.is_alphabetic()) {
                    Some(first_word)
                } else {
                    None
                }
            };

            if let Some(word) = phase_word {
                return Some(match word.to_lowercase().as_str() {
                    "thinking" | "thought" => PhaseHint::Thinking,
                    "tool" | "running" => PhaseHint::ToolRunning,
                    // Skip common non-phase words
                    "esc" | "shift" | "tab" | "bypass" => continue,
                    other => PhaseHint::Unknown(other.to_string()),
                });
            }
        }
        None
    }
}

impl StateParser for ClaudeCodeStateParser {
    fn meta(&self) -> &ParserMeta {
        &self.meta
    }

    fn detect_state(&self, context: &ParserContext) -> Option<StateDetectionResult> {
        let text = context.text();
        let active_lines = context.last_non_empty_lines(12);

        // 0. Trust dialog during startup (auto-confirm)
        // Matches both old ("Yes, proceed") and new ("Yes, I trust this folder") formats
        if context.current_state == Some(State::Starting)
            && (text.contains("Yes, proceed") || text.contains("Yes, I trust this folder"))
            && text.contains("Enter to confirm")
        {
            return Some(
                StateDetectionResult::new(State::Starting, 0.95).with_meta(StateMeta {
                    needs_trust_confirm: Some(true),
                    confirm_type: None,
                }),
            );
        }

        // 1. Confirmation dialog (check full text since dialog spans many lines)
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

        // Key signals from active region
        let prompt_lines = context.last_non_empty_lines(3);
        let has_prompt = self.has_prompt_in(&prompt_lines);
        let has_spinner = self.has_spinner_line(&active_lines);
        let has_active_spinner = self.has_active_spinner(&active_lines);

        // 2. Idle or SlashMenu detection.
        //    Claude Code TUI layout when thinking:
        //      ✻ Thinking… (35s · thinking)   ← active spinner (has …)
        //      ────────────────────
        //      ❯                              ← prompt always visible
        //    When task completes, spinner stays on screen but changes format:
        //      ✻ Baked for 7m 11s             ← completion spinner (no …)
        //    Key insight: only spinners with … (ellipsis) indicate active processing.
        //    Completion spinners (no …) are stale — treat as no spinner.
        if !has_active_spinner {
            if self.has_slash_menu(&active_lines) {
                return Some(StateDetectionResult::new(State::SlashMenu, 0.9));
            }
            if has_prompt {
                return Some(StateDetectionResult::new(State::Idle, 0.9));
            }
        }
        // When active spinner is present: it takes precedence over prompt.
        // Phase hints (e.g., "(thinking)") appear after a few seconds —
        // their absence does NOT mean frozen.
        // Fall through to the spinner processing block (section 3) below.

        // 3. Processing: active spinner line present → Thinking or ToolRunning
        //    Phase hint from spinner status line is the MOST authoritative signal.
        //    Historical tool call lines (⏺ ToolName) may linger in scroll buffer
        //    from previous tool runs, so only use them as fallback when no phase hint.
        if has_active_spinner {
            let phase_hint = self.extract_phase_hint(&active_lines);
            match &phase_hint {
                Some(PhaseHint::ToolRunning) => {
                    return Some(StateDetectionResult::new(State::ToolRunning, 0.85));
                }
                Some(PhaseHint::Thinking) => {
                    // Phase hint explicitly says thinking — trust it even if
                    // old ⏺ tool lines are still visible in scroll buffer
                    return Some(StateDetectionResult::new(State::Thinking, 0.9));
                }
                _ => {
                    // No clear phase hint — fall back to tool call line check
                    if self.has_tool_call_line(&active_lines) {
                        return Some(StateDetectionResult::new(State::ToolRunning, 0.8));
                    }
                    return Some(StateDetectionResult::new(State::Thinking, 0.9));
                }
            }
        }

        // 4. Responding: no spinner, no prompt, but has ⏺ output blocks in active region.
        //    Note: In Claude Code v2.x, the spinner is present during the entire response
        //    generation phase, so Responding may rarely trigger. It serves as a safety net
        //    for brief transition windows where spinner disappears before prompt appears.
        //    Check active_lines (not full text) to avoid matching historical ⏺ in scroll buffer.
        if !has_spinner
            && !has_prompt
            && active_lines.iter().any(|l| l.trim().starts_with('⏺'))
        {
            return Some(StateDetectionResult::new(State::Responding, 0.85));
        }

        // 5. Error indicators — only match Claude Code's own error format.
        //    The ✖ prefix is Claude Code's error marker. Don't match broad "error:"
        //    patterns which trigger on compiler output (rustc, tsc, etc.) in tool results.
        if active_lines
            .iter()
            .any(|line| line.trim().starts_with('✖'))
        {
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

        // Prompt visible, no spinner → Idle
        let context = make_context(&["some previous output", "❯ "]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::Idle);

        // > prompt
        let context = make_context(&["> "]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::Idle);
    }

    #[test]
    fn test_detect_idle_with_permanent_bottom_bar() {
        let parser = ClaudeCodeStateParser::new();

        // Claude Code TUI: prompt + bottom bar with "esc to interrupt" → still Idle
        let context = make_context(&[
            "  Previous response",
            "────────────────────",
            "❯ ",
            "────────────────────",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        // No spinner line → Idle (bottom bar's "esc to interrupt" is ignored)
        assert_eq!(result.unwrap().state, State::Idle);
    }

    #[test]
    fn test_detect_thinking_by_spinner() {
        let parser = ClaudeCodeStateParser::new();

        // Spinner line present → Thinking
        let context = make_context(&[
            "✳ Determining… (thought for 1s)",
            "────────────────────",
            "❯ ",
            "────────────────────",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::Thinking);
    }

    #[test]
    fn test_detect_thinking_all_spinner_chars() {
        let parser = ClaudeCodeStateParser::new();

        for spinner in SPINNER_CHARS {
            let line = format!("{} Processing…", spinner);
            let context = make_context(&[&line]);
            let result = parser.detect_state(&context);
            assert!(
                result.is_some(),
                "Failed for spinner: {}",
                spinner
            );
            assert_eq!(
                result.unwrap().state,
                State::Thinking,
                "Wrong state for spinner: {}",
                spinner
            );
        }
    }

    #[test]
    fn test_detect_tool_running_by_tool_call_line() {
        let parser = ClaudeCodeStateParser::new();

        // Spinner + ⏺ Bash tool call line
        let context = make_context(&[
            "⏺ Bash(command=\"ls -la\")",
            "  │ total 42",
            "✻ Running…",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::ToolRunning);

        // Spinner + MCP tool call
        let context = make_context(&[
            "⏺ missiond - mission_kb_search (MCP)",
            "✳ Executing…",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::ToolRunning);

        // Response text mentioning ⏺ should NOT trigger ToolRunning
        let context = make_context(&[
            "  - ToolRunning 检测从依赖 ⏺│ 可见改为用 phase hint",
            "✳ Musing… (36s · ↓ 568 tokens)",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        // Should be Thinking, NOT ToolRunning (response text is not a tool call)
        assert_eq!(result.unwrap().state, State::Thinking);

        // Historical tool call line + spinner saying "(thinking)" → trust phase hint
        let context = make_context(&[
            "⏺ missiond - mission_kb_search (MCP)",
            "  │ Found 3 results",
            "────────────────────",
            "❯ ",
            "────────────────────",
            "✢ Drizzling… (thinking)",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        // Phase hint says "thinking" — trust it over historical ⏺ line
        assert_eq!(result.unwrap().state, State::Thinking);
    }

    #[test]
    fn test_detect_tool_running_by_phase_hint() {
        let parser = ClaudeCodeStateParser::new();

        // v2.x status line with "tool" phase hint (no ⏺│ in active region)
        let context = make_context(&[
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt",
            "────────────────────",
            "❯ ",
            "────────────────────",
            "✻ Running… (5s · tool)",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::ToolRunning);

        // "running" also maps to ToolRunning
        let context = make_context(&[
            "❯ ",
            "✢ Executing… (12s · ↓ 1.2k tokens · running)",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::ToolRunning);
    }

    #[test]
    fn test_detect_thinking_by_phase_hint() {
        let parser = ClaudeCodeStateParser::new();

        // Simple format: "(thinking)"
        let context = make_context(&[
            "❯ ",
            "✢ Smooshing… (thinking)",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::Thinking);

        // v2.x full format with "thinking" phase
        let context = make_context(&[
            "❯ ",
            "✢ Undulating… (3m 2s · ↓ 2.8k tokens · thinking)",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::Thinking);

        // "thought for N" → still Thinking
        let context = make_context(&[
            "❯ ",
            "✽ Undulating… (3m 27s · ↓ 3.5k tokens · thought for 14s)",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::Thinking);
    }

    #[test]
    fn test_detect_tool_running_simple_format() {
        let parser = ClaudeCodeStateParser::new();

        // Simple format: "(tool)"
        let context = make_context(&[
            "❯ ",
            "✻ Running… (tool)",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::ToolRunning);
    }

    #[test]
    fn test_detect_responding() {
        let parser = ClaudeCodeStateParser::new();

        // ⏺ output blocks, no spinner, no prompt
        let context = make_context(&[
            "⏺ Here is the response content",
            "  Some more content",
            "  And more details",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::Responding);
    }

    #[test]
    fn test_detect_option_confirm() {
        let parser = ClaudeCodeStateParser::new();

        let context = make_context(&[
            "example-mcp - example_tool(key: \"test\")",
            "❯ 1. Yes, allow this action",
            "  2. Yes, allow for this session",
            "  3. No, deny this action",
            "Esc to cancel",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.state, State::Confirming);
        assert_eq!(
            result.meta.unwrap().confirm_type,
            Some(ConfirmType::Options)
        );
    }

    #[test]
    fn test_detect_yesno_confirm() {
        let parser = ClaudeCodeStateParser::new();

        let context = make_context(&["Do you want to continue? [Y/n]"]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap().meta.unwrap().confirm_type,
            Some(ConfirmType::YesNo)
        );
    }

    #[test]
    fn test_detect_starting_trust_confirm() {
        let parser = ClaudeCodeStateParser::new();

        // Old format: "Yes, proceed"
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
        assert_eq!(result.meta.unwrap().needs_trust_confirm, Some(true));
    }

    #[test]
    fn test_detect_starting_trust_folder_confirm() {
        let parser = ClaudeCodeStateParser::new();

        // New format: "Yes, I trust this folder"
        let context = make_context_with_state(
            &[
                "Accessing workspace:",
                "/Users/testuser",
                "Quick safety check: Is this a project you created or one you trust?",
                "❯ 1. Yes, I trust this folder",
                "  2. No, exit",
                "Enter to confirm · Esc to cancel",
            ],
            State::Starting,
        );
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.state, State::Starting);
        assert_eq!(result.meta.unwrap().needs_trust_confirm, Some(true));
    }

    #[test]
    fn test_detect_error() {
        let parser = ClaudeCodeStateParser::new();

        // Claude Code's own error format: ✖ prefix
        let context = make_context(&["✖ Failed to execute command"]);
        assert_eq!(parser.detect_state(&context).unwrap().state, State::Error);

        let context = make_context(&["  ✖ Error: API request failed"]);
        assert_eq!(parser.detect_state(&context).unwrap().state, State::Error);
    }

    #[test]
    fn test_error_not_triggered_by_tool_output() {
        let parser = ClaudeCodeStateParser::new();

        // Compiler error in tool output should NOT trigger Error state
        let context = make_context(&["error[E0425]: cannot find value `x`"]);
        assert!(parser.detect_state(&context).is_none());

        let context = make_context(&["Error: Something went wrong"]);
        assert!(parser.detect_state(&context).is_none());

        // Compiler error with spinner present → Thinking, not Error
        let context = make_context(&[
            "  │ error: aborting due to previous error",
            "────────────────────",
            "❯ ",
            "────────────────────",
            "✢ Drizzling… (thinking)",
        ]);
        assert_eq!(parser.detect_state(&context).unwrap().state, State::Thinking);

        // Compiler error with prompt → Idle, not Error
        let context = make_context(&[
            "  │ error[E0425]: cannot find value `x`",
            "────────────────────",
            "❯ ",
        ]);
        assert_eq!(parser.detect_state(&context).unwrap().state, State::Idle);
    }

    #[test]
    fn test_no_detection() {
        let parser = ClaudeCodeStateParser::new();

        let context = make_context(&["random text", "nothing special"]);
        assert!(parser.detect_state(&context).is_none());
    }

    #[test]
    fn test_welcome_screen() {
        let parser = ClaudeCodeStateParser::new();

        let context = make_context(&[
            "    ✻",
            "    ▟█▙     Claude Code v2.1.50",
            "",
            "    Welcome to Claude Code!",
            "",
        ]);
        // No spinner LINE (✻ is part of ASCII art, not followed by space+text on same line)
        // Wait: "    ✻" → trimmed is "✻", SPINNER_LINE_PATTERN = `^\s*[spinners]\s+\S`
        // "    ✻" has no trailing \s+\S → doesn't match. Good.
        let result = parser.detect_state(&context);
        assert!(result.is_none());
    }

    #[test]
    fn test_prompt_mid_screen_with_empty_lines() {
        let parser = ClaudeCodeStateParser::new();

        let mut lines: Vec<&str> = Vec::new();
        lines.push("    ✻");
        lines.push("    ▟█▙     Claude Code v2.1.50");
        lines.push("");
        lines.push("  ⏺ Previous response");
        lines.push("");
        lines.push("❯ ");
        for _ in 0..18 {
            lines.push("");
        }
        assert_eq!(lines.len(), 24);
        let context = make_context(&lines);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::Idle);
    }

    #[test]
    fn test_thinking_mid_screen() {
        let parser = ClaudeCodeStateParser::new();

        let mut lines: Vec<&str> = Vec::new();
        for _ in 0..10 {
            lines.push("");
        }
        lines.push("✳ Determining… (35s · ↑ 454 tokens · thought for 18s)");
        lines.push("────────────────────");
        lines.push("❯ ");
        for _ in 0..11 {
            lines.push("");
        }
        assert_eq!(lines.len(), 24);
        let context = make_context(&lines);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        // Spinner line found → Thinking (overrides prompt)
        assert_eq!(result.unwrap().state, State::Thinking);
    }

    #[test]
    fn test_spinner_line_pattern_no_false_positive() {
        // Bottom bar should NOT match spinner pattern
        assert!(!SPINNER_LINE_PATTERN
            .is_match("  ⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt"));
        // Standalone spinner char without text should NOT match
        assert!(!SPINNER_LINE_PATTERN.is_match("    ✻"));
        assert!(!SPINNER_LINE_PATTERN.is_match("  ·"));
        // Valid spinner lines SHOULD match
        assert!(SPINNER_LINE_PATTERN.is_match("✳ Determining…"));
        assert!(SPINNER_LINE_PATTERN.is_match("  ✻ Reading file..."));
        assert!(SPINNER_LINE_PATTERN.is_match("· Processing (esc to interrupt)"));
    }

    #[test]
    fn test_spinner_with_dense_tool_output() {
        let parser = ClaudeCodeStateParser::new();

        // Spinner at bottom with many lines of tool output above
        // Window=12 should still catch the spinner
        let context = make_context(&[
            "  │ src/main.rs:10:5: warning: unused variable",
            "  │ src/main.rs:15:1: error: expected semicolon",
            "  │ src/lib.rs:30:10: warning: dead code",
            "  │ src/lib.rs:42:5: error: mismatched types",
            "  │ src/util.rs:5:1: warning: unused import",
            "  │ src/util.rs:20:8: error: cannot find value",
            "  │ 3 errors, 3 warnings emitted",
            "  │ error: could not compile `myproject`",
            "────────────────────",
            "❯ ",
            "────────────────────",
            "✢ Drizzling… (thinking)",
        ]);
        // Spinner found in last 12 non-empty lines
        assert_eq!(parser.detect_state(&context).unwrap().state, State::Thinking);
    }

    #[test]
    fn test_completion_spinner_is_idle() {
        let parser = ClaudeCodeStateParser::new();

        // Completion spinner: no ellipsis (…), shows total elapsed time.
        // Claude Code leaves this on screen after task finishes.
        // Must NOT be treated as active processing.
        let context = make_context(&[
            "  元循环状态：仍在持续。",
            "",
            "✻ Baked for 7m 11s",
            "",
            "────────────────────────────────────────",
            "❯ ",
            "────────────────────────────────────────",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle)",
        ]);
        // Completion spinner (no …) + prompt → Idle
        assert_eq!(parser.detect_state(&context).unwrap().state, State::Idle);
    }

    #[test]
    fn test_active_spinner_without_phase_hint_is_thinking() {
        let parser = ClaudeCodeStateParser::new();

        // Active spinner with ellipsis but no phase hint yet (first few seconds).
        // The … indicates ongoing processing — trust it.
        let context = make_context(&[
            "  Previous output",
            "",
            "✻ Kneading…",
            "",
            "────────────────────────────────────────",
            "❯ ",
            "────────────────────────────────────────",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle)",
        ]);
        // Active spinner (has …) → Thinking
        assert_eq!(parser.detect_state(&context).unwrap().state, State::Thinking);
    }

    #[test]
    fn test_active_spinner_with_prompt_is_thinking() {
        let parser = ClaudeCodeStateParser::new();

        // Active spinner WITH phase hint + prompt visible → still Thinking
        let context = make_context(&[
            "Previous output line",
            "",
            "✻ Analyzing… (45s · ↑ 300 tokens · thinking)",
            "",
            "────────────────────────────────────────",
            "❯ ",
            "────────────────────────────────────────",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle)",
        ]);
        // Spinner has active phase hint (thinking) → still Thinking
        assert_eq!(
            parser.detect_state(&context).unwrap().state,
            State::Thinking
        );
    }

    #[test]
    fn test_detect_slash_menu() {
        let parser = ClaudeCodeStateParser::new();

        // Real format from alacritty capture
        let context = make_context(&[
            "────────────────────────────────────────",
            "❯ /",
            "────────────────────────────────────────",
            "  /my-deploy-agent             description here",
            "  /my-mcp                      My MCP server",
            "  /missiond                    Claude Code multi-instance",
            "  /backend-deploy              backend deploy",
            "  /add-dir                     Add a new working directory",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::SlashMenu);
    }

    #[test]
    fn test_detect_slash_menu_partial_filter() {
        let parser = ClaudeCodeStateParser::new();

        // User typed /my to filter
        let context = make_context(&[
            "❯ /my",
            "────────────────────────────────────────",
            "  /my-deploy-agent             desc",
            "  /my-mcp                      desc",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().state, State::SlashMenu);
    }

    #[test]
    fn test_slash_prompt_without_menu_is_idle() {
        let parser = ClaudeCodeStateParser::new();

        // User typed / but no menu items visible (menu closed or not yet rendered)
        let context = make_context(&[
            "❯ /",
            "────────────────────────────────────────",
            "  ? for shortcuts",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        // No menu items → Idle, not SlashMenu
        assert_eq!(result.unwrap().state, State::Idle);
    }

    #[test]
    fn test_idle_with_slash_in_output_not_slash_menu() {
        let parser = ClaudeCodeStateParser::new();

        // Output contains /paths but prompt doesn't have /
        let context = make_context(&[
            "  /usr/local/bin/node",
            "  /etc/config",
            "────────────────────────────────────────",
            "❯ ",
        ]);
        let result = parser.detect_state(&context);
        assert!(result.is_some());
        // Prompt has no /, so it's Idle
        assert_eq!(result.unwrap().state, State::Idle);
    }

    #[test]
    fn test_full_tui_lifecycle() {
        let parser = ClaudeCodeStateParser::new();

        // Phase 1: Idle (prompt, no spinner)
        let idle_screen = make_context(&[
            "  Previous response text",
            "────────────────────",
            "❯ ",
            "────────────────────",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt",
        ]);
        assert_eq!(parser.detect_state(&idle_screen).unwrap().state, State::Idle);

        // Phase 2: Thinking (spinner appears)
        let thinking_screen = make_context(&[
            "✳ Determining… (2s · thinking)",
            "────────────────────",
            "❯ ",
            "────────────────────",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt",
        ]);
        assert_eq!(
            parser.detect_state(&thinking_screen).unwrap().state,
            State::Thinking
        );

        // Phase 3: Tool running (spinner + phase hint)
        let tool_screen = make_context(&[
            "⏺ Bash │ ls -la",
            "  │ output...",
            "✻ Running… (5s · tool)",
            "────────────────────",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt",
        ]);
        assert_eq!(
            parser.detect_state(&tool_screen).unwrap().state,
            State::ToolRunning
        );

        // Phase 4: Completion spinner remains (stale) — still Idle
        let completion_screen = make_context(&[
            "  ⏺ Response completed",
            "    Done!",
            "✻ Baked for 3m 22s",
            "────────────────────",
            "❯ ",
            "────────────────────",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt",
        ]);
        assert_eq!(
            parser.detect_state(&completion_screen).unwrap().state,
            State::Idle
        );

        // Phase 5: Back to Idle (spinner gone, prompt back)
        let idle_again = make_context(&[
            "  ⏺ Response completed",
            "    Done!",
            "────────────────────",
            "❯ ",
            "────────────────────",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt",
        ]);
        assert_eq!(
            parser.detect_state(&idle_again).unwrap().state,
            State::Idle
        );
    }
}
