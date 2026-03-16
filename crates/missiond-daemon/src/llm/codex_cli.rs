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
//! Rate limiting: governor (20 RPM) + semaphore (2 concurrent).
//! Instrumentation: every request emits CliRequestStarted/Completed
//! events via EventBus for timeline tracking.

use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use governor::{Quota, RateLimiter, clock::DefaultClock, state::{InMemoryState, NotKeyed}};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, Semaphore};
use tracing::{info, warn};

use crate::event_bus::{DaemonEvent, TimelineEntry};
use crate::gemini_client::current_parent_span_id;

type GovernorLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

/// Absolute safety cap: kill process no matter what after this duration.
const ABSOLUTE_TIMEOUT: Duration = Duration::from_secs(300); // 5 min

/// RPM limit for Codex CLI calls.
const CODEX_RPM: u32 = 20;
/// Max concurrent Codex CLI subprocesses.
const CODEX_MAX_CONCURRENT: usize = 2;

/// Check if Codex is disabled via persistent flag file.
/// Flag file: `$MISSIOND_HOME/codex_disabled` (or `~/.xjp-mission/codex_disabled`).
pub(crate) fn is_codex_disabled() -> bool {
    let home = std::env::var("MISSIOND_HOME")
        .or_else(|_| std::env::var("XJP_MISSION_HOME"))
        .unwrap_or_else(|_| {
            let h = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
            if h.join(".missiond").exists() {
                h.join(".missiond").to_string_lossy().to_string()
            } else {
                h.join(".xjp-mission").to_string_lossy().to_string()
            }
        });
    std::path::Path::new(&home).join("codex_disabled").exists()
}

/// Set or clear the Codex disabled flag file.
pub(crate) fn set_codex_disabled(disabled: bool) {
    let home = std::env::var("MISSIOND_HOME")
        .or_else(|_| std::env::var("XJP_MISSION_HOME"))
        .unwrap_or_else(|_| {
            let h = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
            if h.join(".missiond").exists() {
                h.join(".missiond").to_string_lossy().to_string()
            } else {
                h.join(".xjp-mission").to_string_lossy().to_string()
            }
        });
    let flag = std::path::PathBuf::from(home).join("codex_disabled");
    if disabled {
        let _ = std::fs::write(&flag, chrono::Utc::now().to_rfc3339());
        warn!("Codex disabled — flag file created at {:?}", flag);
    } else {
        let _ = std::fs::remove_file(&flag);
        info!("Codex enabled — flag file removed");
    }
}

/// Codex CLI subprocess wrapper with rate limiting and timeline instrumentation.
///
/// Rate limiting: governor (20 RPM) + semaphore (2 concurrent).
/// Clone shares the same limiter/semaphore via Arc.
#[derive(Clone)]
pub(crate) struct CodexCli {
    binary: String,
    default_model: String,
    idle_timeout: Duration,
    event_tx: mpsc::UnboundedSender<TimelineEntry>,
    rate_limiter: Arc<GovernorLimiter>,
    semaphore: Arc<Semaphore>,
}

/// Response from a Codex CLI invocation.
#[allow(dead_code)]
pub(crate) struct CodexCliResponse {
    pub content: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Drop guard: guarantees every Started event gets a matching Completed event,
/// even on early-return errors or future cancellation.
struct RequestGuard<'a> {
    cli: &'a CodexCli,
    request_id: String,
    caller: String,
    model: String,
    prompt_chars: usize,
    image_hash: Option<String>,
    span_id: String,
    queue_wait_ms: u64,
    api_start: Instant,
    completed: bool,
}

impl Drop for RequestGuard<'_> {
    fn drop(&mut self) {
        if !self.completed {
            let err: Result<CodexCliResponse> = Err(anyhow!("request cancelled (future dropped)"));
            self.cli.emit_completed(
                &self.request_id, &self.caller, &self.model,
                self.prompt_chars, &err,
                self.api_start.elapsed().as_millis() as u64,
                self.queue_wait_ms,
                self.image_hash.as_deref(), &self.span_id,
            );
        }
    }
}

impl CodexCli {
    pub fn new(
        binary: String,
        default_model: String,
        idle_timeout: Duration,
        event_tx: mpsc::UnboundedSender<TimelineEntry>,
    ) -> Self {
        let quota = Quota::per_minute(NonZeroU32::new(CODEX_RPM).unwrap());
        info!(rpm = CODEX_RPM, concurrency = CODEX_MAX_CONCURRENT, "Codex CLI: rate limiter initialized");
        Self {
            binary,
            default_model,
            idle_timeout,
            event_tx,
            rate_limiter: Arc::new(RateLimiter::direct(quota)),
            semaphore: Arc::new(Semaphore::new(CODEX_MAX_CONCURRENT)),
        }
    }

    /// Call Codex CLI with optional image file for vision tasks.
    ///
    /// Uses `codex exec --json --ephemeral --skip-git-repo-check` for headless operation.
    /// Image is passed via `-i <path>` flag (native vision support).
    /// Rate limited: semaphore (concurrency) → governor (RPM) → exec.
    pub async fn call(
        &self,
        prompt: &str,
        caller: &str,
        model: Option<&str>,
        image_path: Option<&Path>,
        idle_timeout_override: Option<Duration>,
        image_hash: Option<&str>,
    ) -> Result<CodexCliResponse> {
        // Persistent kill switch: skip all Codex calls when disabled
        if is_codex_disabled() {
            info!(caller, "Codex CLI disabled via flag file, skipping call");
            return Err(anyhow!("Codex CLI is disabled (codex_disabled flag is set)"));
        }

        let model = model.unwrap_or(&self.default_model);
        let idle_timeout = idle_timeout_override.unwrap_or(self.idle_timeout);
        let has_image = image_path.is_some();
        let request_id = uuid::Uuid::new_v4().to_string();
        let span_id = uuid::Uuid::new_v4().to_string();

        if prompt.is_empty() {
            return Err(anyhow!("Empty prompt"));
        }

        // 1. Rate limiting: semaphore (concurrency) → governor (RPM)
        let queue_start = Instant::now();
        let _permit = self.semaphore.clone().acquire_owned().await
            .map_err(|_| anyhow!("Codex semaphore closed"))?;
        self.rate_limiter.until_ready().await;
        let queue_wait = queue_start.elapsed();
        let queue_wait_ms = queue_wait.as_millis() as u64;

        // 2. Emit started event AFTER acquiring permit (queue wait excluded from API time)
        self.emit_started(&request_id, caller, model, prompt, has_image, image_hash, &span_id, queue_wait_ms);

        let api_start = Instant::now();

        // 3. Drop guard: guarantees Completed event on any exit path
        let guard = RequestGuard {
            cli: self,
            request_id: request_id.clone(),
            caller: caller.to_string(),
            model: model.to_string(),
            prompt_chars: prompt.len(),
            image_hash: image_hash.map(|s| s.to_string()),
            span_id: span_id.clone(),
            queue_wait_ms,
            api_start,
            completed: false,
        };

        // 4. Execute
        let result = self.exec_cli(prompt, model, image_path, idle_timeout).await;
        let duration_ms = api_start.elapsed().as_millis() as u64;

        // 5. Normal completion — disarm guard
        let mut guard = guard;
        guard.completed = true;
        self.emit_completed(&request_id, caller, model, prompt.len(), &result, duration_ms, queue_wait_ms, image_hash, &span_id);

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

    fn emit_started(&self, request_id: &str, caller: &str, model: &str, prompt: &str, has_image: bool, image_hash: Option<&str>, span_id: &str, queue_wait_ms: u64) {
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
                "queue_wait_ms": queue_wait_ms,
            }),
        };
        let entry = TimelineEntry {
            event,
            trace_id: Some("codex".to_string()),
            span_id: span_id.to_string(),
            parent_span_id: parent,
            summary: Some(format!("{} → {} ({}ch{}, q:{}ms)", caller, model, prompt.len(), if has_image { " +img" } else { "" }, queue_wait_ms)),
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
        queue_wait_ms: u64,
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
                let s = if msg.contains("timed out") { "timeout" }
                    else if msg.contains("cancelled") { "cancelled" }
                    else { "error" };
                (s.to_string(), 0, Some(msg), None, 0, 0)
            }
        };

        info!(request_id, caller, model, prompt_chars, response_chars,
              duration_ms, queue_wait_ms, status = %status, "codex_request");

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
                "queue_wait_ms": queue_wait_ms,
            }),
        };
        let parent = current_parent_span_id();
        let entry = TimelineEntry {
            event,
            trace_id: Some("codex".to_string()),
            span_id: span_id.to_string(),
            parent_span_id: parent,
            summary: Some(format!("{} → {} ({}ms, q:{}ms)", caller, model, duration_ms, queue_wait_ms)),
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
