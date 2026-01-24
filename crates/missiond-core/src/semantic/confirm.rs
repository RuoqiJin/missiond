//! Claude Code confirm parser
//!
//! Parses Claude Code tool confirmation dialogs.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;

use super::types::{
    ConfirmAction, ConfirmInfo, ConfirmKey, ConfirmOption, ConfirmParser, ConfirmResponse,
    ConfirmType, ParserContext, ParserMeta, ToolInfo,
};

/// Regex patterns for confirm parsing
static OPTION_CONFIRM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?mi)^[\s❯>]*1\.\s*(Yes|Allow)").unwrap());

static YES_NO_CONFIRM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\[Y/n\]|\(yes/no\)|Allow\?|Do you want to proceed").unwrap());

/// Tool info pattern: server - tool_name(params) or server - tool_name(params) (MCP)
static TOOL_INFO_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(\S+)\s*-\s*(\w+)\s*\(([^)]*)\)(?:\s*\(MCP\))?").unwrap());

/// Parameter pattern: key: "value" or key: value
static PARAM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(\w+):\s*("[^"]*"|[^,)]+)"#).unwrap());

/// Option line pattern: number. label (with optional leading ❯ or > and spaces)
static OPTION_LINE_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[\s❯>]*(\d+)\.\s*(.+)$").unwrap());

/// Y/n prompt cleanup pattern
static YN_CLEANUP_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\s*\[Y/n\].*|\s*\(yes/no\).*").unwrap());

/// Claude Code confirm parser
///
/// Parses confirmation dialogs and formats responses:
/// - Options-style: 1. Yes, 2. ..., 3. No (use arrow keys + Enter)
/// - Y/n style: [Y/n] or (yes/no) prompts
pub struct ClaudeCodeConfirmParser {
    meta: ParserMeta,
}

impl Default for ClaudeCodeConfirmParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeCodeConfirmParser {
    /// Create a new Claude Code confirm parser
    pub fn new() -> Self {
        Self {
            meta: ParserMeta {
                name: "claude-code-confirm".to_string(),
                description: "Parses Claude Code tool confirmation dialogs".to_string(),
                priority: 100,
                version: "1.0.0".to_string(),
            },
        }
    }

    /// Check for options-style confirmation
    fn is_option_confirm(&self, text: &str) -> bool {
        OPTION_CONFIRM_PATTERN.is_match(text) && text.contains("Esc to cancel")
    }

    /// Check for Y/n style confirmation
    fn is_yes_no_confirm(&self, text: &str) -> bool {
        YES_NO_CONFIRM_PATTERN.is_match(text)
    }

    /// Parse tool info from confirmation text
    ///
    /// Supports formats:
    /// - `xjp-mcp - xjp_secret_get(key: "value")`
    /// - `xjp-mcp - xjp_secret_get(key: "value") (MCP)`
    fn parse_tool_info(&self, text: &str) -> Option<ToolInfo> {
        let caps = TOOL_INFO_PATTERN.captures(text)?;

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

        Some(ToolInfo {
            name,
            mcp_server: Some(mcp_server),
            params,
        })
    }

    /// Parse options from text
    fn parse_options(&self, text: &str) -> Option<Vec<ConfirmOption>> {
        let mut options = Vec::new();

        for line in text.lines() {
            if let Some(caps) = OPTION_LINE_PATTERN.captures(line) {
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
        let mut prompt_lines = Vec::new();

        for line in text.lines() {
            // Stop at options
            if OPTION_LINE_PATTERN.is_match(line) {
                break;
            }

            // Handle Y/n type prompts - extract text before the prompt indicator
            if YES_NO_CONFIRM_PATTERN.is_match(line) {
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
        let text = r#"xjp-mcp - xjp_secret_get(key: "test_value")"#;
        let tool = parser.parse_tool_info(text);
        assert!(tool.is_some());
        let tool = tool.unwrap();
        assert_eq!(tool.name, "xjp_secret_get");
        assert_eq!(tool.mcp_server, Some("xjp-mcp".to_string()));
        assert_eq!(tool.params.get("key"), Some(&"test_value".to_string()));

        // With (MCP) suffix
        let text = r#"xjp-mcp - xjp_secret_get(key: "value") (MCP)"#;
        let tool = parser.parse_tool_info(text);
        assert!(tool.is_some());
        assert_eq!(tool.unwrap().name, "xjp_secret_get");

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
            "xjp-mcp - xjp_secret_get(key: \"test\")",
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
        assert_eq!(info.tool.as_ref().unwrap().name, "xjp_secret_get");
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
