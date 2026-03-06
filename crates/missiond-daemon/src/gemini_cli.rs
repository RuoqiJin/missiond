//! General-purpose Gemini CLI subprocess wrapper.
//!
//! Calls the official Gemini CLI (`@google/gemini-cli`) in headless streaming JSON mode
//! to access Google One AI Pro models without Vertex AI rate limits.
//!
//! Architecture: Uses `-o stream-json` (NDJSON) instead of `-o json` to enable
//! idle-timeout based liveness detection. As long as the CLI emits events
//! (init, message, thinking deltas, result), the process is considered alive.
//! Only when no output arrives for `idle_timeout` seconds do we kill the process.
//! This elegantly handles both long prompts and long thinking times.
//!
//! Usage:
//! ```ignore
//! let cli = GeminiCli::new("gemini", "gemini-3.1-pro-preview", Duration::from_secs(120));
//! let resp = cli.call(&messages, None, None, None).await?;
//! ```

use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{info, warn};

/// Absolute safety cap: kill process no matter what after this duration.
const ABSOLUTE_TIMEOUT: Duration = Duration::from_secs(900); // 15 minutes

/// Gemini CLI subprocess wrapper.
#[derive(Clone, Debug)]
pub(crate) struct GeminiCli {
    binary: String,
    default_model: String,
    idle_timeout: Duration,
}

/// Response from a Gemini CLI invocation.
pub(crate) struct GeminiCliResponse {
    pub content: String,
    pub model: String,
}

impl GeminiCli {
    pub fn new(binary: String, default_model: String, idle_timeout: Duration) -> Self {
        Self { binary, default_model, idle_timeout }
    }

    /// Core method: convert messages to prompt, call CLI with stream-json, parse NDJSON events.
    ///
    /// Idle timeout: process is killed only if no stdout line arrives within `idle_timeout`.
    /// This means long thinking (with streaming deltas) or large responses never cause timeout.
    pub async fn call(
        &self,
        messages: &[Value],
        model: Option<&str>,
        _max_tokens: Option<u32>,
        idle_timeout_override: Option<Duration>,
    ) -> Result<GeminiCliResponse> {
        let model = model.unwrap_or(&self.default_model);
        let idle_timeout = idle_timeout_override.unwrap_or(self.idle_timeout);
        let prompt = messages_to_prompt(messages);

        if prompt.is_empty() {
            return Err(anyhow!("Empty prompt after message conversion"));
        }

        info!(model, prompt_len = prompt.len(), idle_timeout_secs = idle_timeout.as_secs(),
              "Gemini CLI: calling (stream-json mode)");

        let mut child = tokio::process::Command::new(&self.binary)
            .args(["-p", &prompt, "-m", model, "-o", "stream-json"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| anyhow!("Failed to spawn gemini CLI '{}': {}", self.binary, e))?;

        let stdout = child.stdout.take()
            .ok_or_else(|| anyhow!("Failed to capture stdout"))?;
        let mut lines = BufReader::new(stdout).lines();

        // Collect NDJSON events with idle timeout
        let mut events: Vec<Value> = Vec::new();
        let absolute_deadline = tokio::time::Instant::now() + ABSOLUTE_TIMEOUT;

        let stream_result: Result<(), String> = async {
            loop {
                let remaining = absolute_deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    return Err("absolute timeout (15min)".to_string());
                }
                let effective_timeout = idle_timeout.min(remaining);

                match tokio::time::timeout(effective_timeout, lines.next_line()).await {
                    Ok(Ok(Some(line))) => {
                        if line.trim().is_empty() { continue; }
                        match serde_json::from_str::<Value>(&line) {
                            Ok(event) => {
                                let event_type = event.get("type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                if event_type == "result" || event_type == "error" {
                                    events.push(event);
                                    return Ok(()); // Terminal event
                                }
                                events.push(event);
                            }
                            Err(e) => {
                                warn!(line_len = line.len(), error = %e,
                                      "Gemini CLI: skipping non-JSON line");
                            }
                        }
                    }
                    Ok(Ok(None)) => return Ok(()),  // EOF — process finished
                    Ok(Err(e)) => return Err(format!("IO error: {}", e)),
                    Err(_) => return Err(format!("idle timeout ({}s no output)", idle_timeout.as_secs())),
                }
            }
        }.await;

        // Always try to clean up the child process
        if let Err(ref reason) = stream_result {
            warn!(reason, "Gemini CLI: killing process");
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(anyhow!("Gemini CLI timed out: {}", reason));
        }

        // Wait for process to fully exit
        let status = child.wait().await
            .map_err(|e| anyhow!("Gemini CLI process error: {}", e))?;
        if !status.success() {
            // Try to get stderr for error context
            let stderr_msg = if let Some(mut stderr) = child.stderr.take() {
                let mut buf = String::new();
                let _ = tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut buf).await;
                buf.chars().take(500).collect::<String>()
            } else {
                String::new()
            };
            return Err(anyhow!("Gemini CLI exited with {}: {}", status, stderr_msg));
        }

        parse_stream_events(&events, model)
    }

    /// Convenience: single prompt string.
    #[allow(dead_code)]
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

/// Parse stream-json NDJSON events into a response.
///
/// Event types from Gemini CLI stream-json:
/// - `init`: session start, contains model
/// - `message` (role=user): echoed prompt
/// - `message` (role=assistant, delta=true): streaming response chunks
/// - `result`: final stats, status
/// - `error`: error occurred
fn parse_stream_events(events: &[Value], requested_model: &str) -> Result<GeminiCliResponse> {
    if events.is_empty() {
        return Err(anyhow!("Gemini CLI: no events received"));
    }

    // Collect all assistant message content (delta chunks)
    let mut content_parts: Vec<String> = Vec::new();
    let mut model = requested_model.to_string();

    for event in events {
        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match event_type {
            "init" => {
                if let Some(m) = event.get("model").and_then(|v| v.as_str()) {
                    model = m.to_string();
                }
            }
            "message" => {
                let role = event.get("role").and_then(|v| v.as_str()).unwrap_or("");
                if role == "assistant" || role == "model" {
                    if let Some(content) = event.get("content").and_then(|v| v.as_str()) {
                        content_parts.push(content.to_string());
                    }
                }
            }
            "error" => {
                let msg = event.get("message").and_then(|v| v.as_str())
                    .or_else(|| event.get("error").and_then(|v| v.as_str()))
                    .unwrap_or("unknown error");
                return Err(anyhow!("Gemini CLI error event: {}", msg));
            }
            "result" => {
                let status = event.get("status").and_then(|v| v.as_str()).unwrap_or("unknown");
                if status != "success" {
                    return Err(anyhow!("Gemini CLI result status: {}", status));
                }
            }
            _ => {
                info!(event_type, "Gemini CLI: unhandled event type");
            }
        }
    }

    if content_parts.is_empty() {
        return Err(anyhow!("Gemini CLI: no assistant content in {} events", events.len()));
    }

    // stream-json emits incremental delta chunks — concatenate all to get full response.
    let content = content_parts.join("");

    info!(content_len = content.len(), events = events.len(), "Gemini CLI: stream complete");

    Ok(GeminiCliResponse { content, model })
}
