//! Claude Code confirm parser
//!
//! Parses Claude Code tool confirmation dialogs.

use std::collections::HashMap;
use std::sync::Arc;

use once_cell::sync::Lazy;
use regex::Regex;

use super::patterns::CompiledPatterns;
use super::types::{
    ConfirmAction, ConfirmInfo, ConfirmKey, ConfirmOption, ConfirmParser, ConfirmResponse,
    ConfirmType, ParserContext, ParserMeta, ToolInfo,
};

/// Internal: parameter pattern (not CLI surface, stable)
static PARAM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(\w+):\s*("[^"]*"|[^,)]+)"#).unwrap());

/// Internal: Y/n prompt cleanup pattern (not CLI surface, stable)
static YN_CLEANUP_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\s*\[Y/n\].*|\s*\(yes/no\).*").unwrap());

/// Claude Code confirm parser
///
/// Parses confirmation dialogs and formats responses:
/// - Options-style: 1. Yes, 2. ..., 3. No (use arrow keys + Enter)
/// - Y/n style: [Y/n] or (yes/no) prompts
pub struct ClaudeCodeConfirmParser {
    meta: ParserMeta,
    patterns: Arc<CompiledPatterns>,
}

impl Default for ClaudeCodeConfirmParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeCodeConfirmParser {
    /// Create with external patterns
    pub fn with_patterns(patterns: Arc<CompiledPatterns>) -> Self {
        Self {
            meta: ParserMeta {
                name: "claude-code-confirm".to_string(),
                description: "Parses Claude Code tool confirmation dialogs".to_string(),
                priority: 100,
                version: "1.0.0".to_string(),
            },
            patterns,
        }
    }

    /// Create with default embedded patterns
    pub fn new() -> Self {
        let patterns = Arc::new(
            super::patterns::default_compiled(crate::types::CliEngine::ClaudeCode)
                .expect("embedded claude-code patterns must parse"),
        );
        Self::with_patterns(patterns)
    }

    /// Check for options-style confirmation (tool confirm or AskUserQuestion)
    fn is_option_confirm(&self, text: &str) -> bool {
        let has_standard = self
            .patterns
            .regex("confirm.option_trigger")
            .map_or(false, |re| re.is_match(text));
        let ask_user_marker = self.patterns.ask_user_marker().unwrap_or("Enter to select");
        let has_ask_user = self
            .patterns
            .regex("confirm.ask_user_trigger")
            .map_or(false, |re| re.is_match(text))
            && text.contains(ask_user_marker);
        let cancel_marker = self.patterns.cancel_marker().unwrap_or("Esc to cancel");
        (has_standard || has_ask_user) && text.contains(cancel_marker)
    }

    /// Check for Y/n style confirmation
    fn is_yes_no_confirm(&self, text: &str) -> bool {
        self.patterns
            .regex("confirm.yes_no")
            .map_or(false, |re| re.is_match(text))
    }

    /// Parse tool info from confirmation text
    ///
    /// Supports formats:
    /// - `server-name - tool_name(key: "value")`
    /// - `server-name - tool_name(key: "value") (MCP)`
    /// - `Use skill "skill-name"`
    fn parse_tool_info(&self, text: &str) -> Option<ToolInfo> {
        // Try standard MCP tool format first
        let tool_info_re = self.patterns.regex("confirm.tool_info");
        if let Some(caps) = tool_info_re.and_then(|re| re.captures(text)) {
            let mcp_server = caps.get(1)?.as_str().to_string();
            let name = caps.get(2)?.as_str().to_string();
            let params_str = caps.get(3)?.as_str();

            // Parse parameters
            let mut params = HashMap::new();
            for caps in PARAM_PATTERN.captures_iter(params_str) {
                if let (Some(key), Some(value)) = (caps.get(1), caps.get(2)) {
                    let key = key.as_str().to_string();
                    let mut value = value.as_str().to_string();

                    // Remove quotes if present
                    if value.starts_with('"') && value.ends_with('"') {
                        value = value[1..value.len() - 1].to_string();
                    }

                    params.insert(key, value);
                }
            }

            return Some(ToolInfo {
                name,
                mcp_server: Some(mcp_server),
                params,
            });
        }

        // Try Skill confirmation format: Use skill "skill-name"
        let skill_re = self.patterns.regex("confirm.skill_confirm");
        if let Some(caps) = skill_re.and_then(|re| re.captures(text)) {
            let skill_name = caps.get(1)?.as_str().to_string();
            return Some(ToolInfo {
                name: format!("Skill({})", skill_name),
                mcp_server: None,
                params: HashMap::new(),
            });
        }

        // Try built-in tool type line: "Read file", "Bash command", etc.
        let builtin_type_re = self.patterns.regex("confirm.builtin_type");
        if let Some(caps) = builtin_type_re.and_then(|re| re.captures(text)) {
            let tool_name = caps.get(1)?.as_str().to_string();
            return Some(ToolInfo {
                name: tool_name,
                mcp_server: None,
                params: HashMap::new(),
            });
        }

        // Try built-in tool call pattern: Read(...), Search(...), Glob(...), etc.
        let builtin_call_re = self.patterns.regex("confirm.builtin_call");
        if let Some(caps) = builtin_call_re.and_then(|re| re.captures(text)) {
            let tool_name = caps.get(1)?.as_str().to_string();
            // Normalize Search → Grep
            let tool_name = if tool_name == "Search" { "Grep".to_string() } else { tool_name };
            return Some(ToolInfo {
                name: tool_name,
                mcp_server: None,
                params: HashMap::new(),
            });
        }

        None
    }

    /// Parse options from text
    fn parse_options(&self, text: &str) -> Option<Vec<ConfirmOption>> {
        let option_line_re = self.patterns.regex("confirm.option_line")?;
        let mut options = Vec::new();

        for line in text.lines() {
            if let Some(caps) = option_line_re.captures(line) {
                if let (Some(num_match), Some(label_match)) = (caps.get(1), caps.get(2)) {
                    if let Ok(num) = num_match.as_str().parse::<u32>() {
                        options.push(ConfirmOption {
                            key: ConfirmKey::Number(num),
                            label: label_match.as_str().trim().to_string(),
                            is_default: num == 1,
                        });
                    }
                }
            }
        }

        if options.is_empty() {
            None
        } else {
            Some(options)
        }
    }

    /// Extract the main prompt/question
    fn extract_prompt(&self, text: &str) -> String {
        let option_line_re = self.patterns.regex("confirm.option_line");
        let yes_no_re = self.patterns.regex("confirm.yes_no");
        let mut prompt_lines = Vec::new();

        for line in text.lines() {
            // Stop at options
            if option_line_re.map_or(false, |re| re.is_match(line)) {
                break;
            }

            // Handle Y/n type prompts - extract text before the prompt indicator
            if yes_no_re.map_or(false, |re| re.is_match(line)) {
                let cleaned = YN_CLEANUP_PATTERN.replace(line, "");
                let trimmed = cleaned.trim();
                if !trimmed.is_empty() {
                    prompt_lines.push(trimmed.to_string());
                }
                break;
            }

            let trimmed = line.trim();
            if !trimmed.is_empty() {
                prompt_lines.push(trimmed.to_string());
            }
        }

        prompt_lines.join("\n")
    }
}

impl ConfirmParser for ClaudeCodeConfirmParser {
    fn meta(&self) -> &ParserMeta {
        &self.meta
    }

    fn detect_confirm(&self, context: &ParserContext) -> Option<ConfirmInfo> {
        let text = context.text();

        // Check for options-style confirm (Claude Code tool usage)
        // Format: "❯ 1. Yes" or "  1. Yes" (with optional leading arrow/spaces)
        if self.is_option_confirm(&text) {
            let tool = self.parse_tool_info(&text);
            let options = self.parse_options(&text);

            return Some(ConfirmInfo {
                confirm_type: ConfirmType::Options,
                prompt: self.extract_prompt(&text),
                options,
                tool,
                raw_prompt: text,
            });
        }

        // Check for simple Y/n confirm
        if self.is_yes_no_confirm(&text) {
            return Some(ConfirmInfo {
                confirm_type: ConfirmType::YesNo,
                prompt: self.extract_prompt(&text),
                options: Some(vec![
                    ConfirmOption {
                        key: ConfirmKey::Char("y".to_string()),
                        label: "Yes".to_string(),
                        is_default: true,
                    },
                    ConfirmOption {
                        key: ConfirmKey::Char("n".to_string()),
                        label: "No".to_string(),
                        is_default: false,
                    },
                ]),
                tool: None,
                raw_prompt: text,
            });
        }

        None
    }

    fn format_response(&self, info: &ConfirmInfo, response: &ConfirmResponse) -> String {
        // Claude Code confirmation dialog uses ❯ to mark current selection
        // Navigate with arrow keys, confirm with Enter
        // Cannot input numbers directly as they may be intercepted by other dialogs (e.g., feedback)
        match response.action {
            ConfirmAction::Confirm => {
                // First option is selected, just press Enter
                "\r".to_string()
            }
            ConfirmAction::Deny => {
                match info.confirm_type {
                    ConfirmType::Options => {
                        // Move to third option (No) and press Enter: Down Down Enter
                        // \x1b[B is the ANSI escape code for down arrow
                        "\x1b[B\x1b[B\r".to_string()
                    }
                    ConfirmType::YesNo => {
                        // Just type 'n' and Enter
                        "n\r".to_string()
                    }
                }
            }
            ConfirmAction::Select => {
                // Move to specified option and press Enter
                if let Some(option) = response.option {
                    if option > 1 {
                        let down_keys = "\x1b[B".repeat((option - 1) as usize);
                        format!("{}\r", down_keys)
                    } else {
                        "\r".to_string()
                    }
                } else {
                    "\r".to_string()
                }
            }
            ConfirmAction::Input => {
                // Type custom value and press Enter
                if let Some(ref value) = response.value {
                    format!("{}\r", value)
                } else {
                    "\r".to_string()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_context(lines: &[&str]) -> ParserContext {
        ParserContext::new(lines.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn test_parse_tool_info() {
        let parser = ClaudeCodeConfirmParser::new();

        // Basic format
        let text = r#"example-mcp - example_tool(key: "test_value")"#;
        let tool = parser.parse_tool_info(text);
        assert!(tool.is_some());
        let tool = tool.unwrap();
        assert_eq!(tool.name, "example_tool");
        assert_eq!(tool.mcp_server, Some("example-mcp".to_string()));
        assert_eq!(tool.params.get("key"), Some(&"test_value".to_string()));

        // With (MCP) suffix
        let text = r#"example-mcp - example_tool(key: "value") (MCP)"#;
        let tool = parser.parse_tool_info(text);
        assert!(tool.is_some());
        assert_eq!(tool.unwrap().name, "example_tool");

        // Multiple parameters
        let text = r#"server - tool_name(param1: "val1", param2: "val2")"#;
        let tool = parser.parse_tool_info(text);
        assert!(tool.is_some());
        let tool = tool.unwrap();
        assert_eq!(tool.params.get("param1"), Some(&"val1".to_string()));
        assert_eq!(tool.params.get("param2"), Some(&"val2".to_string()));
    }

    #[test]
    fn test_parse_options() {
        let parser = ClaudeCodeConfirmParser::new();

        let text = "❯ 1. Yes, allow this action\n  2. Yes, allow for this session\n  3. No, deny this action";
        let options = parser.parse_options(text);
        assert!(options.is_some());
        let options = options.unwrap();
        assert_eq!(options.len(), 3);

        assert!(matches!(options[0].key, ConfirmKey::Number(1)));
        assert!(options[0].label.contains("Yes, allow this action"));
        assert!(options[0].is_default);

        assert!(matches!(options[1].key, ConfirmKey::Number(2)));
        assert!(!options[1].is_default);

        assert!(matches!(options[2].key, ConfirmKey::Number(3)));
        assert!(options[2].label.contains("No"));
    }

    #[test]
    fn test_detect_option_confirm() {
        let parser = ClaudeCodeConfirmParser::new();

        let context = make_context(&[
            "example-mcp - example_tool(key: \"test\")",
            "❯ 1. Yes, allow this action",
            "  2. Yes, allow for this session",
            "  3. No, deny this action",
            "Esc to cancel",
        ]);

        let result = parser.detect_confirm(&context);
        assert!(result.is_some());
        let info = result.unwrap();

        assert_eq!(info.confirm_type, ConfirmType::Options);
        assert!(info.tool.is_some());
        assert_eq!(info.tool.as_ref().unwrap().name, "example_tool");
        assert!(info.options.is_some());
        assert_eq!(info.options.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn test_detect_yesno_confirm() {
        let parser = ClaudeCodeConfirmParser::new();

        // [Y/n] format
        let context = make_context(&["Do you want to continue? [Y/n]"]);
        let result = parser.detect_confirm(&context);
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.confirm_type, ConfirmType::YesNo);
        assert!(info.options.is_some());
        assert_eq!(info.options.as_ref().unwrap().len(), 2);

        // (yes/no) format
        let context = make_context(&["Proceed with action? (yes/no)"]);
        let result = parser.detect_confirm(&context);
        assert!(result.is_some());
        assert_eq!(result.unwrap().confirm_type, ConfirmType::YesNo);
    }

    #[test]
    fn test_extract_prompt() {
        let parser = ClaudeCodeConfirmParser::new();

        // Options confirm
        let text =
            "Do you want to allow this?\n❯ 1. Yes\n  2. No\nEsc to cancel";
        let prompt = parser.extract_prompt(text);
        assert_eq!(prompt, "Do you want to allow this?");

        // Y/n confirm
        let text = "Continue? [Y/n]";
        let prompt = parser.extract_prompt(text);
        assert_eq!(prompt, "Continue?");
    }

    #[test]
    fn test_format_response_confirm() {
        let parser = ClaudeCodeConfirmParser::new();

        let info = ConfirmInfo {
            confirm_type: ConfirmType::Options,
            prompt: "Test".to_string(),
            options: None,
            tool: None,
            raw_prompt: "Test".to_string(),
        };

        // Confirm action
        let response = ConfirmResponse::confirm();
        assert_eq!(parser.format_response(&info, &response), "\r");
    }

    #[test]
    fn test_format_response_deny_options() {
        let parser = ClaudeCodeConfirmParser::new();

        let info = ConfirmInfo {
            confirm_type: ConfirmType::Options,
            prompt: "Test".to_string(),
            options: None,
            tool: None,
            raw_prompt: "Test".to_string(),
        };

        // Deny action for options (down down enter)
        let response = ConfirmResponse::deny();
        assert_eq!(parser.format_response(&info, &response), "\x1b[B\x1b[B\r");
    }

    #[test]
    fn test_format_response_deny_yesno() {
        let parser = ClaudeCodeConfirmParser::new();

        let info = ConfirmInfo {
            confirm_type: ConfirmType::YesNo,
            prompt: "Test".to_string(),
            options: None,
            tool: None,
            raw_prompt: "Test".to_string(),
        };

        // Deny action for Y/n
        let response = ConfirmResponse::deny();
        assert_eq!(parser.format_response(&info, &response), "n\r");
    }

    #[test]
    fn test_format_response_select() {
        let parser = ClaudeCodeConfirmParser::new();

        let info = ConfirmInfo {
            confirm_type: ConfirmType::Options,
            prompt: "Test".to_string(),
            options: None,
            tool: None,
            raw_prompt: "Test".to_string(),
        };

        // Select option 1 (no movement needed)
        let response = ConfirmResponse::select(1);
        assert_eq!(parser.format_response(&info, &response), "\r");

        // Select option 2 (one down)
        let response = ConfirmResponse::select(2);
        assert_eq!(parser.format_response(&info, &response), "\x1b[B\r");

        // Select option 3 (two downs)
        let response = ConfirmResponse::select(3);
        assert_eq!(parser.format_response(&info, &response), "\x1b[B\x1b[B\r");
    }

    #[test]
    fn test_format_response_input() {
        let parser = ClaudeCodeConfirmParser::new();

        let info = ConfirmInfo {
            confirm_type: ConfirmType::YesNo,
            prompt: "Test".to_string(),
            options: None,
            tool: None,
            raw_prompt: "Test".to_string(),
        };

        // Custom input
        let response = ConfirmResponse::input("custom value");
        assert_eq!(
            parser.format_response(&info, &response),
            "custom value\r"
        );
    }

    #[test]
    fn test_no_detection() {
        let parser = ClaudeCodeConfirmParser::new();

        let context = make_context(&["random text", "nothing special"]);
        let result = parser.detect_confirm(&context);
        assert!(result.is_none());
    }
}
