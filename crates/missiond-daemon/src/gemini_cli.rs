//! General-purpose Gemini CLI subprocess wrapper.
//!
//! Calls the official Gemini CLI (`@google/gemini-cli`) in headless JSON mode
//! to access Google One AI Pro models without Vertex AI rate limits.
//!
//! Usage:
//! ```ignore
//! let cli = GeminiCli::new("gemini", "gemini-3.1-pro-preview", Duration::from_secs(120));
//! let resp = cli.call(&messages, None, None, None).await?;
//! ```

use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::Value;
use tracing::{info, warn};

/// Gemini CLI subprocess wrapper.
#[derive(Clone, Debug)]
pub(crate) struct GeminiCli {
    binary: String,
    default_model: String,
    default_timeout: Duration,
}

/// Response from a Gemini CLI invocation.
pub(crate) struct GeminiCliResponse {
    pub content: String,
    pub model: String,
}

impl GeminiCli {
    pub fn new(binary: String, default_model: String, default_timeout: Duration) -> Self {
        Self { binary, default_model, default_timeout }
    }

    /// Core method: convert messages to prompt, call CLI, parse response.
    pub async fn call(
        &self,
        messages: &[Value],
        model: Option<&str>,
        _max_tokens: Option<u32>,
        timeout: Option<Duration>,
    ) -> Result<GeminiCliResponse> {
        let model = model.unwrap_or(&self.default_model);
        let timeout = timeout.unwrap_or(self.default_timeout);
        let prompt = messages_to_prompt(messages);

        if prompt.is_empty() {
            return Err(anyhow!("Empty prompt after message conversion"));
        }

        info!(model, prompt_len = prompt.len(), "Gemini CLI: calling");

        let child = tokio::process::Command::new(&self.binary)
            .args(["-p", &prompt, "-m", model, "-o", "json"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| anyhow!("Failed to spawn gemini CLI '{}': {}", self.binary, e))?;

        let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => return Err(anyhow!("Gemini CLI process error: {}", e)),
            Err(_) => return Err(anyhow!("Gemini CLI timed out after {}s", timeout.as_secs())),
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Gemini CLI exited with {}: {}", output.status, stderr.chars().take(500).collect::<String>()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_cli_response(&stdout, model)
    }

    /// Convenience: single prompt string.
    pub async fn prompt(&self, text: &str, model: Option<&str>) -> Result<String> {
        let messages = vec![serde_json::json!({"role": "user", "content": text})];
        let resp = self.call(&messages, model, None, None).await?;
        Ok(resp.content)
    }
}

/// Convert OpenAI-style messages array to a single prompt string.
fn messages_to_prompt(messages: &[Value]) -> String {
    let mut parts = Vec::new();
    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
        if content.is_empty() { continue; }
        let label = match role {
            "system" => "System",
            "assistant" => "Assistant",
            _ => "User",
        };
        parts.push(format!("{}: {}", label, content));
    }
    parts.join("\n\n--------\n\n")
}

/// Parse Gemini CLI JSON output.
/// Expected format: `{"session_id": "...", "response": "text", "stats": {...}}`
fn parse_cli_response(stdout: &str, model: &str) -> Result<GeminiCliResponse> {
    // CLI may output non-JSON lines before the JSON (e.g., warnings).
    // Find the first line starting with '{' and try to parse from there.
    let json_start = stdout.find('{')
        .ok_or_else(|| anyhow!("Gemini CLI: no JSON in output (len={})", stdout.len()))?;
    let json_str = &stdout[json_start..];

    let parsed: Value = serde_json::from_str(json_str)
        .map_err(|e| anyhow!("Gemini CLI: failed to parse JSON: {} (first 200 chars: {})",
            e, json_str.chars().take(200).collect::<String>()))?;

    let content = parsed.get("response")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Gemini CLI: no 'response' field in output"))?
        .to_string();

    info!(content_len = content.len(), "Gemini CLI: response received");

    Ok(GeminiCliResponse {
        content,
        model: model.to_string(),
    })
}
