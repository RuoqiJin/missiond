//! Codex CLI subprocess wrapper for OpenAI models.
//!
//! Calls the OpenAI Codex CLI (`@openai/codex`) in non-interactive JSON mode
//! to access GPT-5.4 vision capabilities via ChatGPT subscription.
//!
//! Key feature: supports `-i` flag for direct image file input,
//! making it ideal for vision tasks without base64-in-prompt hacks.
//!
//! JSONL event format (codex exec --json):
//! - `thread.started`: session created
//! - `turn.started`: inference begins
//! - `item.completed`: response text in `item.text`
//! - `turn.completed`: inference done, contains `usage`
//!
//! Instrumentation: every request emits CliRequestStarted/Completed
//! events via EventBus for timeline tracking.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::event_bus::{DaemonEvent, TimelineEntry};
use crate::gemini_client::current_parent_span_id;

/// Absolute safety cap: kill process no matter what after this duration.
const ABSOLUTE_TIMEOUT: Duration = Duration::from_secs(300); // 5 min

/// Codex CLI subprocess wrapper with timeline instrumentation.
#[derive(Clone)]
pub(crate) struct CodexCli {
    binary: String,
    default_model: String,
    idle_timeout: Duration,
    event_tx: mpsc::UnboundedSender<TimelineEntry>,
}

/// Response from a Codex CLI invocation.
#[allow(dead_code)]
pub(crate) struct CodexCliResponse {
    pub content: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl CodexCli {
    pub fn new(
        binary: String,
        default_model: String,
        idle_timeout: Duration,
        event_tx: mpsc::UnboundedSender<TimelineEntry>,
    ) -> Self {
        Self { binary, default_model, idle_timeout, event_tx }
    }

    /// Call Codex CLI with optional image file for vision tasks.
    ///
    /// Uses `codex exec --json --ephemeral --skip-git-repo-check` for headless operation.
    /// Image is passed via `-i <path>` flag (native vision support).
    /// Emits CliRequestStarted/Completed events for timeline tracking.
    pub async fn call(
        &self,
        prompt: &str,
        caller: &str,
        model: Option<&str>,
        image_path: Option<&Path>,
        idle_timeout_override: Option<Duration>,
        image_hash: Option<&str>,
    ) -> Result<CodexCliResponse> {
        let model = model.unwrap_or(&self.default_model);
        let idle_timeout = idle_timeout_override.unwrap_or(self.idle_timeout);
        let has_image = image_path.is_some();
        let request_id = uuid::Uuid::new_v4().to_string();
        let span_id = uuid::Uuid::new_v4().to_string();

        if prompt.is_empty() {
            return Err(anyhow!("Empty prompt"));
        }

        // Emit started event
        self.emit_started(&request_id, caller, model, prompt, has_image, image_hash, &span_id);

        let api_start = Instant::now();

        let result = self.exec_cli(prompt, model, image_path, idle_timeout).await;
        let duration_ms = api_start.elapsed().as_millis() as u64;

        // Emit completed event
        self.emit_completed(&request_id, caller, model, prompt.len(), &result, duration_ms, image_hash, &span_id);

        result
    }

    /// Internal: execute the CLI subprocess.
    async fn exec_cli(
        &self,
        prompt: &str,
        model: &str,
        image_path: Option<&Path>,
        idle_timeout: Duration,
    ) -> Result<CodexCliResponse> {
        info!(model, prompt_len = prompt.len(), has_image = image_path.is_some(),
              "Codex CLI: calling");

        let mut cmd = tokio::process::Command::new(&self.binary);
        cmd.args(["exec", "--json", "--ephemeral", "--skip-git-repo-check"]);
        cmd.args(["-m", model]);
        cmd.args(["-C", "/tmp"]);

        // IMPORTANT: prompt MUST come before -i, because -i takes variadic <FILE>...
        // and will consume the prompt as an image path if placed after -i.
        cmd.arg(prompt);

        if let Some(img) = image_path {
            cmd.args(["-i", &img.to_string_lossy()]);
        }
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn()
            .map_err(|e| anyhow!("Failed to spawn codex CLI '{}': {}", self.binary, e))?;

        let stdout = child.stdout.take()
            .ok_or_else(|| anyhow!("Failed to capture stdout"))?;
        let mut lines = BufReader::new(stdout).lines();

        let mut events: Vec<Value> = Vec::new();
        let absolute_deadline = tokio::time::Instant::now() + ABSOLUTE_TIMEOUT;

        let stream_result: Result<(), String> = async {
            loop {
                let remaining = absolute_deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    return Err("absolute timeout (5min)".to_string());
                }
                let effective_timeout = idle_timeout.min(remaining);

                match tokio::time::timeout(effective_timeout, lines.next_line()).await {
                    Ok(Ok(Some(line))) => {
                        if line.trim().is_empty() { continue; }
                        match serde_json::from_str::<Value>(&line) {
                            Ok(event) => events.push(event),
                            Err(e) => {
                                warn!(line_len = line.len(), error = %e,
                                      "Codex CLI: skipping non-JSON line");
                            }
                        }
                    }
                    Ok(Ok(None)) => return Ok(()),
                    Ok(Err(e)) => return Err(format!("IO error: {}", e)),
                    Err(_) => return Err(format!("idle timeout ({}s no output)", idle_timeout.as_secs())),
                }
            }
        }.await;

        if let Err(ref reason) = stream_result {
            warn!(reason, "Codex CLI: killing process");
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(anyhow!("Codex CLI timed out: {}", reason));
        }

        let status = child.wait().await
            .map_err(|e| anyhow!("Codex CLI process error: {}", e))?;
        if !status.success() {
            let stderr_msg = if let Some(mut stderr) = child.stderr.take() {
                let mut buf = String::new();
                let _ = tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut buf).await;
                buf.chars().take(500).collect::<String>()
            } else {
                String::new()
            };
            return Err(anyhow!("Codex CLI exited with {}: {}", status, stderr_msg));
        }

        parse_codex_events(&events, model)
    }

    fn emit_started(&self, request_id: &str, caller: &str, model: &str, prompt: &str, has_image: bool, image_hash: Option<&str>, span_id: &str) {
        let parent = current_parent_span_id();
        let event = DaemonEvent::CliRequestStarted {
            engine: missiond_core::CliEngine::Codex,
            request_id: request_id.to_string(),
            caller: caller.to_string(),
            session_id: None,
            model: model.to_string(),
            prompt_chars: prompt.len(),
            prompt_text: Some(prompt.to_string()),
            extra: serde_json::json!({
                "has_image": has_image,
                "image_hash": image_hash,
            }),
        };
        let entry = TimelineEntry {
            event,
            trace_id: Some("codex".to_string()),
            span_id: span_id.to_string(),
            parent_span_id: parent,
            summary: Some(format!("{} → {} ({}ch{})", caller, model, prompt.len(), if has_image { " +img" } else { "" })),
        };
        let _ = self.event_tx.send(entry);
    }

    fn emit_completed(
        &self,
        request_id: &str,
        caller: &str,
        model: &str,
        prompt_chars: usize,
        result: &Result<CodexCliResponse>,
        duration_ms: u64,
        image_hash: Option<&str>,
        span_id: &str,
    ) {
        let (status, response_chars, error_msg, response_text, input_tokens, output_tokens) = match result {
            Ok(resp) => (
                "ok".to_string(),
                resp.content.len(),
                None,
                Some(resp.content.clone()),
                resp.input_tokens,
                resp.output_tokens,
            ),
            Err(e) => {
                let msg = e.to_string();
                let s = if msg.contains("timed out") { "timeout" } else { "error" };
                (s.to_string(), 0, Some(msg), None, 0, 0)
            }
        };

        info!(request_id, caller, model, prompt_chars, response_chars,
              duration_ms, status = %status, "codex_request");

        let event = DaemonEvent::CliRequestCompleted {
            engine: missiond_core::CliEngine::Codex,
            request_id: request_id.to_string(),
            caller: caller.to_string(),
            session_id: None,
            model: model.to_string(),
            prompt_chars,
            response_chars,
            duration_ms,
            status,
            error_msg,
            response_text,
            extra: serde_json::json!({
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "image_hash": image_hash,
            }),
        };
        let parent = current_parent_span_id();
        let entry = TimelineEntry {
            event,
            trace_id: Some("codex".to_string()),
            span_id: span_id.to_string(),
            parent_span_id: parent,
            summary: Some(format!("{} → {} ({}ms)", caller, model, duration_ms)),
        };
        let _ = self.event_tx.send(entry);
    }
}

/// Parse Codex exec --json JSONL events into a response.
fn parse_codex_events(events: &[Value], requested_model: &str) -> Result<CodexCliResponse> {
    if events.is_empty() {
        return Err(anyhow!("Codex CLI: no events received"));
    }

    let mut content_parts: Vec<String> = Vec::new();
    let mut input_tokens: u64 = 0;
    let mut output_tokens: u64 = 0;

    for event in events {
        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match event_type {
            "item.completed" => {
                if let Some(text) = event.pointer("/item/text").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        content_parts.push(text.to_string());
                    }
                }
            }
            "turn.completed" => {
                if let Some(usage) = event.get("usage") {
                    input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                    output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                }
            }
            _ => {}
        }
    }

    if content_parts.is_empty() {
        return Err(anyhow!("Codex CLI: no content in {} events", events.len()));
    }

    let content = content_parts.join("\n");
    info!(content_len = content.len(), input_tokens, output_tokens, "Codex CLI: complete");

    Ok(CodexCliResponse {
        content,
        model: requested_model.to_string(),
        input_tokens,
        output_tokens,
    })
}
