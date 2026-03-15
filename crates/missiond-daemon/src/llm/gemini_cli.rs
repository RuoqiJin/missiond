//! General-purpose Gemini CLI subprocess wrapper.
//!
//! Calls the official Gemini CLI (`@google/gemini-cli`) in headless streaming JSON mode
//! to access Google One AI Pro models without Vertex AI rate limits.
//!
//! Architecture: Uses `-o stream-json` (NDJSON) instead of `-o json` to enable
//! idle-timeout based liveness detection. As long as the CLI emits events
//! (init, message, thinking deltas, result), the process is considered alive.
//!
//! Two-tier idle timeout: Gemini CLI is agentic and calls tools (file reads,
//! code search) during reasoning. Tool execution can take much longer than text
//! generation, so we use an extended timeout (5min) between tool_use → tool_result,
//! and the normal idle timeout (120s) otherwise.
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
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Absolute safety cap: kill process no matter what after this duration.
const ABSOLUTE_TIMEOUT: Duration = Duration::from_secs(900); // 15 minutes

/// Extended idle timeout when Gemini CLI is executing tools (agentic mode).
/// Tool execution (file reads, code search) can take much longer than text generation.
const TOOL_EXEC_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

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

/// Real-time progress event emitted during CLI execution.
#[derive(Debug)]
pub(crate) struct GeminiCliProgress {
    /// Sequential index within this request (1, 2, 3...)
    pub tool_seq: u32,
    /// "tool_use" or "tool_result"
    pub activity: String,
    /// Tool name (e.g. "read_file", "grep", "bash")
    pub tool_name: Option<String>,
    /// Truncated input preview (≤200 chars)
    pub input_preview: Option<String>,
    /// Truncated result preview (≤500 chars)
    pub result_preview: Option<String>,
    /// Whether the tool result was an error
    pub is_error: bool,
}

impl GeminiCli {
    pub fn new(binary: String, default_model: String, idle_timeout: Duration) -> Self {
        Self { binary, default_model, idle_timeout }
    }

    /// Core method: convert messages to prompt, call CLI with stream-json, parse NDJSON events.
    ///
    /// Idle timeout: process is killed only if no stdout line arrives within `idle_timeout`.
    /// This means long thinking (with streaming deltas) or large responses never cause timeout.
    ///
    /// `progress_tx`: optional channel to emit real-time tool activity events.
    pub async fn call(
        &self,
        messages: &[Value],
        model: Option<&str>,
        _max_tokens: Option<u32>,
        idle_timeout_override: Option<Duration>,
        progress_tx: Option<mpsc::UnboundedSender<GeminiCliProgress>>,
    ) -> Result<GeminiCliResponse> {
        let model = model.unwrap_or(&self.default_model);
        let idle_timeout = idle_timeout_override.unwrap_or(self.idle_timeout);
        let prompt = messages_to_prompt(messages);

        if prompt.is_empty() {
            return Err(anyhow!("Empty prompt after message conversion"));
        }

        info!(model, prompt_len = prompt.len(), idle_timeout_secs = idle_timeout.as_secs(),
              "Gemini CLI: calling (stream-json mode)");

        let auth_config = resolve_gemini_auth_config();
        info!(mode = %auth_config.mode, "Gemini CLI: using {} mode", auth_config.mode);

        let mut child = self.spawn_cli(&prompt, model, &auth_config)?;
        let events = self.stream_events(&mut child, idle_timeout, &progress_tx).await?;

        // Wait for process to fully exit
        let status = child.wait().await
            .map_err(|e| anyhow!("Gemini CLI process error: {}", e))?;
        if !status.success() {
            let stderr_msg = read_stderr(&mut child).await;

            // OAuth expired detection: auto-fallback to apikey and retry once
            if auth_config.mode == "google" && is_auth_error(&stderr_msg) {
                warn!("Gemini CLI: OAuth auth failed, attempting fallback to API key mode");
                let fallback = resolve_apikey_config();
                if let Some(ref fallback_config) = fallback {
                    // Sync settings.json to apikey mode
                    sync_settings_json("gemini-api-key");
                    if let Ok(mut retry_child) = self.spawn_cli(&prompt, model, fallback_config) {
                        let retry_result = self.stream_events(&mut retry_child, idle_timeout, &progress_tx).await;
                        let retry_status = retry_child.wait().await;
                        if let (Ok(events), Ok(s)) = (&retry_result, &retry_status) {
                            if s.success() {
                                info!("Gemini CLI: fallback to API key succeeded");
                                return parse_stream_events(events, model);
                            }
                        }
                        let _ = retry_child.kill().await;
                    }
                }
                return Err(anyhow!("Gemini CLI: OAuth expired, API key fallback also failed. Run `gemini` to re-authenticate. stderr: {}", stderr_msg));
            }

            return Err(anyhow!("Gemini CLI exited with {}: {}", status, stderr_msg));
        }

        parse_stream_events(&events, model)
    }

    fn spawn_cli(&self, prompt: &str, model: &str, auth: &GeminiAuthConfig) -> Result<tokio::process::Child> {
        let mut cmd = tokio::process::Command::new(&self.binary);
        cmd.args(["-p", prompt, "-m", model, "-o", "stream-json", "--yolo"])
            .stdin(std::process::Stdio::null())  // Prevent interactive prompts from hanging
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        if let Some(ref key) = auth.api_key {
            cmd.env("GEMINI_API_KEY", key);
        }

        cmd.spawn()
            .map_err(|e| anyhow!("Failed to spawn gemini CLI '{}': {}", self.binary, e))
    }

    /// Stream NDJSON events from a running CLI process.
    async fn stream_events(
        &self,
        child: &mut tokio::process::Child,
        idle_timeout: Duration,
        progress_tx: &Option<mpsc::UnboundedSender<GeminiCliProgress>>,
    ) -> Result<Vec<Value>> {
        let stdout = child.stdout.take()
            .ok_or_else(|| anyhow!("Failed to capture stdout"))?;
        let mut lines = BufReader::new(stdout).lines();
        let mut events: Vec<Value> = Vec::new();
        let absolute_deadline = tokio::time::Instant::now() + ABSOLUTE_TIMEOUT;
        let mut awaiting_tool_result = false;
        let mut tool_seq: u32 = 0;

        let stream_result: Result<(), String> = async {
            loop {
                let remaining = absolute_deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    return Err("absolute timeout (15min)".to_string());
                }
                let current_idle = if awaiting_tool_result { TOOL_EXEC_TIMEOUT } else { idle_timeout };
                let effective_timeout = current_idle.min(remaining);

                match tokio::time::timeout(effective_timeout, lines.next_line()).await {
                    Ok(Ok(Some(line))) => {
                        if line.trim().is_empty() { continue; }
                        match serde_json::from_str::<Value>(&line) {
                            Ok(event) => {
                                let event_type = event.get("type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                match event_type {
                                    "tool_use" => {
                                        awaiting_tool_result = true;
                                        tool_seq += 1;
                                        if let Some(ref tx) = progress_tx {
                                            let _ = tx.send(extract_tool_use_progress(&event, tool_seq));
                                        }
                                    }
                                    "tool_result" => {
                                        awaiting_tool_result = false;
                                        tool_seq += 1;
                                        if let Some(ref tx) = progress_tx {
                                            let _ = tx.send(extract_tool_result_progress(&event, tool_seq));
                                        }
                                    }
                                    _ => {}
                                }
                                if event_type == "result" || event_type == "error" {
                                    events.push(event);
                                    return Ok(());
                                }
                                events.push(event);
                            }
                            Err(e) => {
                                warn!(line_len = line.len(), error = %e,
                                      "Gemini CLI: skipping non-JSON line");
                            }
                        }
                    }
                    Ok(Ok(None)) => return Ok(()),
                    Ok(Err(e)) => return Err(format!("IO error: {}", e)),
                    Err(_) => {
                        let mode = if awaiting_tool_result { "tool_exec" } else { "idle" };
                        return Err(format!("{} timeout ({}s no output)", mode, effective_timeout.as_secs()));
                    }
                }
            }
        }.await;

        if let Err(ref reason) = stream_result {
            warn!(reason, "Gemini CLI: killing process");
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(anyhow!("Gemini CLI timed out: {}", reason));
        }

        Ok(events)
    }

    /// Convenience: single prompt string.
    #[allow(dead_code)]
    pub async fn prompt(&self, text: &str, model: Option<&str>) -> Result<String> {
        let messages = vec![serde_json::json!({"role": "user", "content": text})];
        let resp = self.call(&messages, model, None, None, None).await?;
        Ok(resp.content)
    }
}

/// Default system instruction prepended to all Gemini CLI prompts.
const DEFAULT_SYSTEM_INSTRUCTION: &str = "System: 不要修改任何代码。只调查分析和回答问题。";

/// Convert OpenAI-style messages array to a single prompt string.
fn messages_to_prompt(messages: &[Value]) -> String {
    let mut parts = vec![DEFAULT_SYSTEM_INSTRUCTION.to_string()];
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

/// Truncate a string to `max` chars, appending "…" if truncated.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let end: String = s.chars().take(max).collect();
        format!("{}…", end)
    }
}

/// Extract progress event from a tool_use NDJSON event.
///
/// Gemini CLI stream-json format:
/// ```json
/// {"type":"tool_use","tool_name":"read_file","tool_id":"...","parameters":{...}}
/// ```
fn extract_tool_use_progress(event: &Value, tool_seq: u32) -> GeminiCliProgress {
    // Gemini CLI uses "tool_name"; fallback to "name" for forward compat
    let tool_name = event.get("tool_name")
        .or_else(|| event.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    // Gemini CLI uses "parameters"; fallback to "input" for forward compat
    let input_preview = event.get("parameters")
        .or_else(|| event.get("input"))
        .map(|v| {
            if let Some(s) = v.as_str() {
                truncate(s, 200)
            } else {
                truncate(&v.to_string(), 200)
            }
        });
    GeminiCliProgress {
        tool_seq,
        activity: "tool_use".to_string(),
        tool_name,
        input_preview,
        result_preview: None,
        is_error: false,
    }
}

/// Extract progress event from a tool_result NDJSON event.
///
/// Gemini CLI stream-json format:
/// ```json
/// {"type":"tool_result","tool_id":"...","status":"success","output":"..."}
/// ```
fn extract_tool_result_progress(event: &Value, tool_seq: u32) -> GeminiCliProgress {
    // Gemini CLI uses "status" string; fallback to "is_error" bool
    let is_error = event.get("status")
        .and_then(|v| v.as_str())
        .map(|s| s != "success")
        .or_else(|| event.get("is_error").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    // Gemini CLI uses "output"; fallback to "result"/"content" for forward compat
    let result_preview = event.get("output")
        .or_else(|| event.get("result"))
        .or_else(|| event.get("content"))
        .map(|v| {
            if let Some(s) = v.as_str() {
                truncate(s, 500)
            } else {
                truncate(&v.to_string(), 500)
            }
        });
    // Gemini CLI tool_result includes tool_id but not tool_name; extract from tool_id prefix
    let tool_name = event.get("tool_id")
        .and_then(|v| v.as_str())
        .and_then(|id| id.split('_').next())
        .filter(|name| !name.is_empty())
        .map(|s| s.to_string());
    GeminiCliProgress {
        tool_seq,
        activity: "tool_result".to_string(),
        tool_name,
        input_preview: None,
        result_preview,
        is_error,
    }
}

// ===== Auth config helpers =====

/// Resolved auth configuration for a Gemini CLI subprocess.
struct GeminiAuthConfig {
    mode: String,       // "apikey" or "google"
    api_key: Option<String>,
}

/// Read llm.yaml to get gemini_api_key and gemini_auth_mode.
/// Then read ~/.gemini/settings.json as fallback for mode detection.
fn resolve_gemini_auth_config() -> GeminiAuthConfig {
    let llm_yaml = missiond_core::default_mission_home().join("llm.yaml");
    let llm_config = std::fs::read_to_string(&llm_yaml).ok()
        .and_then(|c| serde_yaml::from_str::<serde_yaml::Value>(&c).ok());

    let api_key = llm_config.as_ref()
        .and_then(|c| c.get("gemini_api_key").and_then(|v| v.as_str()).map(|s| s.to_string()));

    // Mode: llm.yaml gemini_auth_mode > settings.json selectedType > default apikey
    let mode = llm_config.as_ref()
        .and_then(|c| c.get("gemini_auth_mode").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| {
            dirs::home_dir()
                .and_then(|h| std::fs::read_to_string(h.join(".gemini/settings.json")).ok())
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.pointer("/security/auth/selectedType").and_then(|t| t.as_str()).map(|s| s.to_string()))
                .map(|t| if t == "oauth-personal" { "google".to_string() } else { "apikey".to_string() })
                .unwrap_or_else(|| "apikey".to_string())
        });

    GeminiAuthConfig {
        api_key: if mode == "apikey" { api_key } else { None },
        mode,
    }
}

/// Build an apikey-only config for fallback.
fn resolve_apikey_config() -> Option<GeminiAuthConfig> {
    let llm_yaml = missiond_core::default_mission_home().join("llm.yaml");
    let content = std::fs::read_to_string(&llm_yaml).ok()?;
    let config: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;
    let key = config.get("gemini_api_key").and_then(|v| v.as_str())?.to_string();
    Some(GeminiAuthConfig { mode: "apikey".to_string(), api_key: Some(key) })
}

/// Sync selectedType to ~/.gemini/settings.json.
fn sync_settings_json(selected_type: &str) {
    let path = match dirs::home_dir() {
        Some(h) => h.join(".gemini/settings.json"),
        None => return,
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let mut settings: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return,
    };
    if let Some(auth) = settings.pointer_mut("/security/auth") {
        if let Some(obj) = auth.as_object_mut() {
            obj.insert("selectedType".to_string(), serde_json::json!(selected_type));
        }
    }
    if let Ok(json) = serde_json::to_string_pretty(&settings) {
        let _ = std::fs::write(&path, json);
    }
}

/// Read stderr from a child process (best effort).
async fn read_stderr(child: &mut tokio::process::Child) -> String {
    if let Some(mut stderr) = child.stderr.take() {
        let mut buf = String::new();
        let _ = tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut buf).await;
        buf.chars().take(500).collect()
    } else {
        String::new()
    }
}

/// Detect OAuth/auth-related errors in stderr output.
fn is_auth_error(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    lower.contains("token expired")
        || lower.contains("auth required")
        || lower.contains("authentication")
        || lower.contains("login")
        || lower.contains("opening authentication page")
        || lower.contains("do you want to continue")
}
