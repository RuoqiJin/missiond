//! Rate-limited, retry-aware Gemini API client.
//!
//! All daemon components that call Gemini (or any model via the Router API)
//! should go through `GeminiClient` to avoid 429 RESOURCE_EXHAUSTED errors.
//!
//! Supports two modes (selected at init via `llm.yaml`):
//! - **HTTP mode** (default): XJP Router / Vertex AI with 20 RPM rate limit + 429 retry
//! - **CLI mode**: Gemini CLI subprocess via Google One AI Pro (no Vertex rate limits)
//!
//! Both modes share semaphore + rate_limiter and return OpenAI-compatible JSON,
//! so all call sites work identically regardless of provider.
//!
//! Instrumentation: every request emits a `DaemonEvent::CliRequestCompleted`
//! via EventBus for persistent logging. Caller identity flows through `task_local!`.

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex as StdMutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use governor::{Quota, RateLimiter, clock::DefaultClock, state::{InMemoryState, NotKeyed}};
use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::event_bus::{DaemonEvent, TimelineEntry};
use crate::gemini_cli::{GeminiCli, GeminiCliProgress};

type GovernorLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

// ===== Request-scoped context (zero-cost propagation via task_local) =====

tokio::task_local! {
    /// Caller session ID — set at IPC handler entry point.
    pub(crate) static REQUEST_SESSION_ID: String;
    /// Caller identity — set by each call site before calling send().
    pub(crate) static REQUEST_CALLER: String;
    /// Parent span ID for cross-lane causal linking (OpenTelemetry-style context propagation).
    /// Set by IPC handler from AppState.last_msg_span cache; read by emit_started_event().
    pub(crate) static PARENT_SPAN_ID: String;
}

/// Read session ID from task-local context (returns None if not set).
pub(crate) fn current_session_id() -> Option<String> {
    REQUEST_SESSION_ID.try_with(|id| id.clone()).ok()
}

/// Read caller identity from task-local context.
fn current_caller() -> String {
    REQUEST_CALLER.try_with(|c| c.clone()).unwrap_or_else(|_| "unknown".to_string())
}

/// Read parent span ID from task-local context (returns None if not set).
/// Used for cross-lane causal linking: parent's span_id → child's parent_span_id.
pub(crate) fn current_parent_span_id() -> Option<String> {
    PARENT_SPAN_ID.try_with(|id| id.clone()).ok()
}


/// Shared Gemini rate limiter + retry client.
/// Stored in `AppState` and used by all Gemini call sites.
#[derive(Clone)]
pub(crate) struct GeminiClient {
    rate_limiter: Arc<GovernorLimiter>,
    semaphore: Arc<Semaphore>,
    cli: Option<Arc<GeminiCli>>,
    event_tx: tokio::sync::mpsc::UnboundedSender<TimelineEntry>,
    /// Atomic counters for DaemonStats aggregation.
    pub(crate) request_count: Arc<AtomicU64>,
    pub(crate) error_count: Arc<AtomicU64>,
    pub(crate) retry_count: Arc<AtomicU64>,
}

/// Drop guard for `GeminiClient::send()`.
/// Guarantees every `CliRequestStarted` gets a corresponding `CliRequestCompleted`,
/// even on early-return errors (`?`) or future cancellation (`send_best_effort` timeout).
struct RequestGuard<'a> {
    client: &'a GeminiClient,
    request_id: String,
    caller: String,
    session_id: Option<String>,
    api_mode: String,
    model: String,
    prompt_chars: usize,
    queue_wait: Duration,
    span_id: String,
    api_start: Instant,
    completed: bool,
}

impl Drop for RequestGuard<'_> {
    fn drop(&mut self) {
        if !self.completed {
            let err: Result<serde_json::Value> = Err(anyhow!("request cancelled (future dropped)"));
            self.client.emit_completed_event(
                &self.request_id, &self.caller, self.session_id.clone(),
                &self.api_mode, &self.model, self.prompt_chars,
                &err, self.queue_wait, self.api_start.elapsed(), 0, &self.span_id,
            );
        }
    }
}

impl RequestGuard<'_> {
    fn complete(mut self, result: &Result<serde_json::Value>, retry_count: u32) {
        self.completed = true;
        self.client.emit_completed_event(
            &self.request_id, &self.caller, self.session_id.clone(),
            &self.api_mode, &self.model, self.prompt_chars,
            result, self.queue_wait, self.api_start.elapsed(), retry_count, &self.span_id,
        );
    }
}

impl GeminiClient {
    /// Create a new HTTP-mode client with 20 RPM limit and 3 max concurrent requests.
    pub fn new(event_tx: tokio::sync::mpsc::UnboundedSender<TimelineEntry>) -> Self {
        let quota = Quota::per_minute(NonZeroU32::new(20).unwrap());
        Self {
            rate_limiter: Arc::new(RateLimiter::direct(quota)),
            semaphore: Arc::new(Semaphore::new(3)),
            cli: None,
            event_tx,
            request_count: Arc::new(AtomicU64::new(0)),
            error_count: Arc::new(AtomicU64::new(0)),
            retry_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Create a CLI-mode client that routes through Gemini CLI subprocess or PTY.
    ///
    /// Semaphore is 1 when PTY transport is active (PTY sessions are serial).
    pub fn with_cli(cli: GeminiCli, event_tx: tokio::sync::mpsc::UnboundedSender<TimelineEntry>) -> Self {
        // CLI mode: relax rate limit (Google One AI Pro has generous limits)
        let quota = Quota::per_minute(NonZeroU32::new(60).unwrap());
        // PTY mode: 1 concurrent (spawn-per-call, serial queue)
        // Subprocess mode: 3 concurrent (legacy, currently broken)
        let concurrency = if cli.has_pty() { 1 } else { 3 };
        Self {
            rate_limiter: Arc::new(RateLimiter::direct(quota)),
            semaphore: Arc::new(Semaphore::new(concurrency)),
            cli: Some(Arc::new(cli)),
            event_tx,
            request_count: Arc::new(AtomicU64::new(0)),
            error_count: Arc::new(AtomicU64::new(0)),
            retry_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Returns true if this client uses Gemini CLI mode.
    pub fn is_cli_mode(&self) -> bool {
        self.cli.is_some()
    }

    /// Send a request with rate limiting.
    ///
    /// In HTTP mode: POST to Router API with 429 retry.
    /// In CLI mode: extract messages from body, call CLI, return OpenAI-compatible JSON.
    pub async fn send(
        &self,
        http_client: &reqwest::Client,
        url: &str,
        jwt: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.send_with_timeout(http_client, url, jwt, body, None).await
    }

    /// Send a request with optional idle timeout override (CLI mode only).
    pub async fn send_with_timeout(
        &self,
        http_client: &reqwest::Client,
        url: &str,
        jwt: &str,
        body: &serde_json::Value,
        idle_timeout_override: Option<Duration>,
    ) -> Result<serde_json::Value> {
        // Kill switch: reject immediately when gemini gate is closed (AtomicBool — zero I/O)
        crate::llm_gate::check(crate::llm_gate::LlmProvider::Gemini)?;
        self.request_count.fetch_add(1, Ordering::Relaxed);
        let request_id = uuid::Uuid::new_v4().to_string();
        let caller = current_caller();
        let session_id = current_session_id();
        let model = body.get("model").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        let prompt_chars: usize = body.get("messages")
            .and_then(|v| v.as_array())
            .map(|msgs| msgs.iter()
                .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                .map(|s| s.len())
                .sum())
            .unwrap_or(0);

        // 1. Acquire concurrency permit (measure queue wait)
        //    PTY mode (semaphore=1): queue timeout prevents starvation.
        //    A long-running request (strategy_analyst 600s) would otherwise block
        //    user-facing requests (router_chat) indefinitely.
        let queue_start = Instant::now();
        let queue_timeout = if self.cli.as_ref().map_or(false, |c| c.has_pty()) {
            Duration::from_secs(30)
        } else {
            Duration::from_secs(300) // HTTP mode: more generous
        };
        let permit = tokio::time::timeout(queue_timeout, self.semaphore.clone().acquire_owned())
            .await
            .map_err(|_| anyhow!(
                "Gemini request queue timeout ({}s): another request is still processing. \
                 Caller: {}, model: {}",
                queue_timeout.as_secs(), caller, model
            ))?
            .map_err(|_| anyhow!("Gemini semaphore closed"))?;

        // 2. Wait for RPM budget
        self.rate_limiter.until_ready().await;
        let queue_wait = queue_start.elapsed();

        // 2b. Emit "request started" event with prompt preview
        let span_id = uuid::Uuid::new_v4().to_string();
        self.emit_started_event(
            &request_id, &caller, session_id.clone(), &model, prompt_chars, body, &span_id,
        );

        // 3. Drop guard — guarantees GeminiRequestCompleted on any exit path,
        //    including early-return errors and future cancellation (send_best_effort timeout).
        let api_mode = if self.cli.is_some() { "cli" } else { "http" };
        let guard = RequestGuard {
            client: self,
            request_id,
            caller,
            session_id,
            api_mode: api_mode.to_string(),
            model,
            prompt_chars,
            queue_wait,
            span_id,
            api_start: Instant::now(),
            completed: false,
        };

        // 4. All work in async move block — ? returns are captured here,
        //    never bypassing the guard in the outer scope.
        let request_id_clone = guard.request_id.clone();
        let span_id_clone = guard.span_id.clone();
        let session_id_clone = guard.session_id.clone();
        let event_tx_clone = self.event_tx.clone();

        let result: Result<serde_json::Value> = async move {
            if let Some(cli) = &self.cli {
                let messages = body.get("messages").and_then(|v| v.as_array())
                    .ok_or_else(|| anyhow!("CLI mode: missing 'messages' in body"))?;
                let max_tokens = body.get("max_tokens").and_then(|v| v.as_u64()).map(|n| n as u32);

                // Extract per-call channel override (injected by router_chat handler)
                let auth_override = body.get("_channel")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // Extract per-call API key alias (injected by router_chat handler)
                let api_key_alias = body.get("_api_key_alias")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // Extract optional working directory (for file-based agentic workflows)
                let workspace_path = body.get("_workspace")
                    .and_then(|v| v.as_str())
                    .map(std::path::PathBuf::from);

                // Whitelist: only these models are allowed via CLI. Others fall back to default.
                // Note: flash-lite is NOT supported by Gemini CLI (ModelNotFoundError).
                let cli_model = body.get("model").and_then(|v| v.as_str())
                    .and_then(|m| match m {
                        "gemini-3.1-pro-preview" => Some(m),
                        _ => None,
                    });

                // Create progress channel to emit real-time tool activity events
                let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<GeminiCliProgress>();

                // Spawn forwarder: converts GeminiCliProgress → DaemonEvent::CliToolActivity
                // Also collects tool calls for inclusion in the response.
                let fwd_request_id = request_id_clone;
                let fwd_span_id = span_id_clone;
                let fwd_session_id = session_id_clone;
                let fwd_event_tx = event_tx_clone;
                let collected_tools: Arc<StdMutex<Vec<serde_json::Value>>> =
                    Arc::new(StdMutex::new(Vec::new()));
                let collected_tools_clone = collected_tools.clone();
                let forwarder = tokio::spawn(async move {
                    while let Some(prog) = progress_rx.recv().await {
                        // Collect tool call info for response
                        {
                            let mut entry = serde_json::json!({
                                "seq": prog.tool_seq,
                                "activity": prog.activity,
                            });
                            if let Some(ref name) = prog.tool_name {
                                entry["tool_name"] = serde_json::json!(name);
                            }
                            if let Some(ref input) = prog.input_preview {
                                entry["input_preview"] = serde_json::json!(input);
                            }
                            if let Some(ref result) = prog.result_preview {
                                entry["result_preview"] = serde_json::json!(result);
                            }
                            if prog.is_error {
                                entry["is_error"] = serde_json::json!(true);
                            }
                            if let Ok(mut vec) = collected_tools_clone.lock() {
                                vec.push(entry);
                            }
                        }

                        let event = DaemonEvent::CliToolActivity {
                            engine: missiond_core::CliEngine::Gemini,
                            request_id: fwd_request_id.clone(),
                            tool_seq: prog.tool_seq,
                            activity: prog.activity.clone(),
                            tool_name: prog.tool_name.clone(),
                            input_preview: prog.input_preview.clone(),
                            result_preview: prog.result_preview.clone(),
                            is_error: prog.is_error,
                        };
                        let summary = match prog.activity.as_str() {
                            "tool_use" => format!("#{} → {}", prog.tool_seq, prog.tool_name.as_deref().unwrap_or("?")),
                            _ => format!("#{} ← {}", prog.tool_seq, if prog.is_error { "error" } else { "ok" }),
                        };
                        let entry = TimelineEntry {
                            event,
                            trace_id: fwd_session_id.clone(),
                            span_id: uuid::Uuid::new_v4().to_string(),
                            parent_span_id: Some(fwd_span_id.clone()),
                            summary: Some(summary),
                        };
                        let _ = fwd_event_tx.send(entry);
                    }
                });

                let resp = cli.call(messages, cli_model, max_tokens, idle_timeout_override, Some(progress_tx), auth_override.as_deref(), api_key_alias.as_deref(), workspace_path.as_deref()).await;
                drop(permit);
                // Drop the sender side is implicit (cli.call finished), wait for forwarder to drain
                let _ = forwarder.await;
                let resp = resp?;

                // Extract collected tool calls from forwarder
                let tool_calls = collected_tools.lock()
                    .map(|v| v.clone())
                    .unwrap_or_default();

                let mut result = serde_json::json!({
                    "choices": [{"message": {"content": resp.content}, "finish_reason": "stop"}],
                    "model": resp.model,
                });
                if !tool_calls.is_empty() {
                    result["tool_calls"] = serde_json::json!(tool_calls);
                }
                Ok(result)
            } else {
                let resp = http_client.post(url)
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {}", jwt))
                    .json(body)
                    .send()
                    .await
                    .map_err(|e| anyhow!("Gemini request failed: {}", e))?;

                if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    let err_body = resp.text().await.unwrap_or_default();
                    drop(permit);
                    warn!(body = %err_body, "Gemini 429 RESOURCE_EXHAUSTED, starting retry");
                    self.retry_count.fetch_add(1, Ordering::Relaxed);
                    return self.retry_with_backoff(http_client, url, jwt, body, 3).await;
                }

                drop(permit);

                if !resp.status().is_success() {
                    self.error_count.fetch_add(1, Ordering::Relaxed);
                    let status = resp.status();
                    let err_body = resp.text().await.unwrap_or_default();
                    return Err(anyhow!("Router returned {}: {}", status, err_body));
                }

                resp.json().await
                    .map_err(|e| anyhow!("Failed to parse Gemini response: {}", e))
            }
        }.await;

        // 5. Normal completion — emit real result (disarms guard drop)
        guard.complete(&result, 0);

        result
    }

    /// Send with a custom timeout. Returns `None` on any error (for best-effort calls).
    pub async fn send_best_effort(
        &self,
        http_client: &reqwest::Client,
        url: &str,
        jwt: &str,
        body: &serde_json::Value,
        timeout: Duration,
    ) -> Option<serde_json::Value> {
        match tokio::time::timeout(timeout, self.send(http_client, url, jwt, body)).await {
            Ok(Ok(v)) => Some(v),
            Ok(Err(e)) => {
                warn!(error = %e, "Gemini best-effort call failed");
                None
            }
            Err(_) => {
                warn!("Gemini best-effort call timed out");
                None
            }
        }
    }

    /// Emit "request started" event — fires when the prompt is dispatched to LLM.
    fn emit_started_event(
        &self,
        request_id: &str,
        caller: &str,
        session_id: Option<String>,
        model: &str,
        prompt_chars: usize,
        body: &serde_json::Value,
        span_id: &str,
    ) {
        // Extract full last-user-message text for DB storage (no truncation)
        let prompt_text = body.get("messages")
            .and_then(|v| v.as_array())
            .and_then(|msgs| msgs.iter().rfind(|m| m.get("role").and_then(|r| r.as_str()) == Some("user")))
            .and_then(|m| m.get("content").and_then(|c| c.as_str()))
            .map(|s| s.to_string());

        let trace_id = session_id.clone();
        let parent = current_parent_span_id(); // Cross-lane causal link
        let event = DaemonEvent::CliRequestStarted {
            engine: missiond_core::CliEngine::Gemini,
            request_id: request_id.to_string(),
            caller: caller.to_string(),
            session_id,
            model: model.to_string(),
            prompt_chars,
            prompt_text,
            extra: serde_json::Value::Object(Default::default()),
        };
        let entry = TimelineEntry {
            event,
            trace_id,
            span_id: span_id.to_string(),
            parent_span_id: parent,
            summary: Some(format!("{} → {} (prompt {}ch)", caller, model, prompt_chars)),
        };
        let _ = self.event_tx.send(entry);
    }

    /// Emit "request completed" event — fires when the LLM response arrives.
    fn emit_completed_event(
        &self,
        request_id: &str,
        caller: &str,
        session_id: Option<String>,
        api_mode: &str,
        model: &str,
        prompt_chars: usize,
        result: &Result<serde_json::Value>,
        queue_wait: Duration,
        api_duration: Duration,
        retry_count: u32,
        span_id: &str,
    ) {
        let (status, response_chars, error_msg, response_text) = match result {
            Ok(v) => {
                let content = v.pointer("/choices/0/message/content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                ("ok".to_string(), content.len(), None, Some(content.to_string()))
            }
            Err(e) => {
                let msg = e.to_string();
                let status = if msg.contains("timed out") { "timeout" } else { "error" };
                (status.to_string(), 0, Some(msg), None)
            }
        };

        info!(
            request_id,
            caller,
            session_id = session_id.as_deref().unwrap_or("-"),
            api_mode,
            model,
            prompt_chars,
            response_chars,
            queue_wait_ms = queue_wait.as_millis() as u64,
            duration_ms = api_duration.as_millis() as u64,
            retry_count,
            status = %status,
            "gemini_request"
        );

        let trace_id = session_id.clone();
        let parent = current_parent_span_id(); // Cross-lane causal link (same as started)
        let event = DaemonEvent::CliRequestCompleted {
            engine: missiond_core::CliEngine::Gemini,
            request_id: request_id.to_string(),
            caller: caller.to_string(),
            session_id,
            model: model.to_string(),
            prompt_chars,
            response_chars,
            duration_ms: api_duration.as_millis() as u64,
            status,
            error_msg,
            response_text,
            extra: serde_json::json!({
                "api_mode": api_mode,
                "queue_wait_ms": queue_wait.as_millis() as u64,
                "retry_count": retry_count,
            }),
        };
        // Use same span_id as the started event so they are linked as a pair
        let entry = TimelineEntry {
            event,
            trace_id,
            span_id: span_id.to_string(),
            parent_span_id: parent,
            summary: Some(format!("{} → {} ({}ms)", caller, model, api_duration.as_millis())),
        };
        let _ = self.event_tx.send(entry);
    }

    async fn retry_with_backoff(
        &self,
        http_client: &reqwest::Client,
        url: &str,
        jwt: &str,
        body: &serde_json::Value,
        max_retries: u32,
    ) -> Result<serde_json::Value> {
        for attempt in 1..=max_retries {
            let delay = Duration::from_secs(2u64.pow(attempt)); // 2s, 4s, 8s
            info!(attempt, max_retries, delay_secs = delay.as_secs(), "Gemini 429 retry backoff");
            tokio::time::sleep(delay).await;

            // Re-acquire permit + rate limit for each retry
            let permit = self.semaphore.acquire().await
                .map_err(|_| anyhow!("Gemini semaphore closed"))?;
            self.rate_limiter.until_ready().await;

            let resp = http_client.post(url)
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {}", jwt))
                .json(body)
                .send()
                .await
                .map_err(|e| anyhow!("Gemini request failed on retry {}: {}", attempt, e))?;

            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                self.retry_count.fetch_add(1, Ordering::Relaxed);
                drop(permit); // Release before next sleep
                if attempt < max_retries {
                    warn!(attempt, "Gemini 429 again, will retry");
                    continue;
                }
                self.error_count.fetch_add(1, Ordering::Relaxed);
                let err_body = resp.text().await.unwrap_or_default();
                return Err(anyhow!("Gemini 429 after {} retries: {}", max_retries, err_body));
            }

            drop(permit);

            if !resp.status().is_success() {
                self.error_count.fetch_add(1, Ordering::Relaxed);
                let status = resp.status();
                let err_body = resp.text().await.unwrap_or_default();
                return Err(anyhow!("Router returned {}: {}", status, err_body));
            }

            return resp.json().await
                .map_err(|e| anyhow!("Failed to parse Gemini response: {}", e));
        }
        unreachable!()
    }
}
