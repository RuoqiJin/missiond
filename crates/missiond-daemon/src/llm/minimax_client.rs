//! MiniMax M2.5 HTTP client — direct OpenAI-compatible API.
//!
//! Used by the Briefing Worker for semantic summarization of timeline events.
//! Direct HTTP call (not PTY slot) — lightweight, fast, no Claude Code overhead.
//!
//! Endpoint: https://api.minimaxi.com/v1/chat/completions
//! Model: MiniMax-M2.5-highspeed (~100 TPS)
//! Auth: Bearer token from `xjp secret get minimax/api-key-domestic`

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::context::v3_blueprint_runtime::MinimaxRuntimeConfig;

const API_URL: &str = "https://api.minimaxi.com/v1/chat/completions";
const MODEL: &str = "MiniMax-M2.5-highspeed";

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: &'a [ChatMessage],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: String,
}

/// Lightweight MiniMax API client.
#[derive(Clone)]
pub(crate) struct MiniMaxClient {
    http: reqwest::Client,
    api_key: String,
    default_max_tokens: u32,
}

impl MiniMaxClient {
    pub fn new_with_runtime_config(api_key: String, config: &MinimaxRuntimeConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(config.direct_http_timeout())
            .build()
            .expect("failed to build HTTP client");
        Self {
            http,
            api_key,
            default_max_tokens: config.default_max_tokens,
        }
    }

    /// Send a chat completion request. Returns the assistant's content with `<think>` tags stripped.
    pub async fn chat(&self, messages: &[ChatMessage], max_tokens: Option<u32>) -> Result<String> {
        let body = ChatRequest {
            model: MODEL,
            max_tokens: max_tokens.unwrap_or(self.default_max_tokens),
            messages,
        };

        let resp = self
            .http
            .post(API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("MiniMax request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("MiniMax API error {}: {}", status, body));
        }

        let data: ChatResponse = resp
            .json()
            .await
            .map_err(|e| anyhow!("MiniMax response parse error: {}", e))?;

        let content = data
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(strip_think_tags(&content))
    }

    /// Summarize text to a target length. Returns the summary.
    pub async fn summarize(&self, text: &str, max_chars: usize) -> Result<String> {
        let prompt = format!(
            "请用不超过{}字简洁总结以下内容的核心要点。直接输出总结，不要前缀。\n\n{}",
            max_chars, text
        );
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: prompt,
        }];
        self.chat(&messages, Some(self.default_max_tokens)).await
    }
}

/// Strip `<think>...</think>` blocks from MiniMax responses (extended thinking output).
fn strip_think_tags(s: &str) -> String {
    let mut result = s.to_string();
    // Remove all <think>...</think> blocks (greedy within each block)
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result[start..].find("</think>") {
            let end_abs = start + end + "</think>".len();
            result = format!("{}{}", &result[..start], result[end_abs..].trim_start());
        } else {
            // Unclosed <think> — remove from <think> to end
            result = result[..start].to_string();
            break;
        }
    }
    result.trim().to_string()
}

/// Load MiniMax API key from xjp secret store.
pub(crate) fn load_api_key() -> Option<String> {
    match std::process::Command::new("xjp")
        .args(["secret", "get", "minimax/api-key-domestic"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let json_str = String::from_utf8_lossy(&output.stdout);
            // Parse JSON: {"key": "...", "namespace": "...", "value": "sk-cp-..."}
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json_str) {
                v.get("value")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            } else {
                // Fallback: raw string
                let trimmed = json_str.trim().to_string();
                if trimmed.starts_with("sk-") {
                    Some(trimmed)
                } else {
                    None
                }
            }
        }
        Ok(output) => {
            warn!(stderr = %String::from_utf8_lossy(&output.stderr), "xjp secret get failed");
            None
        }
        Err(e) => {
            warn!(error = %e, "Failed to run xjp secret get");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_think_tags() {
        assert_eq!(strip_think_tags("hello"), "hello");
        assert_eq!(
            strip_think_tags("<think>\nsome reasoning\n</think>\nThe answer is 42."),
            "The answer is 42."
        );
        assert_eq!(strip_think_tags("<think>a</think>B<think>c</think>D"), "BD");
        assert_eq!(strip_think_tags("<think>unclosed"), "");
    }

    #[test]
    fn projects_default_max_tokens_from_runtime_config() {
        let config = MinimaxRuntimeConfig {
            default_max_tokens: 123,
            ..MinimaxRuntimeConfig::default()
        };
        let client = MiniMaxClient::new_with_runtime_config("token".to_string(), &config);
        assert_eq!(client.default_max_tokens, 123);
    }
}
