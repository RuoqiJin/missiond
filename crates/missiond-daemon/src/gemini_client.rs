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
//! Instrumentation: every request emits a `DaemonEvent::GeminiRequestCompleted`
//! via EventBus for persistent logging. Caller identity flows through `task_local!`.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use governor::{Quota, RateLimiter, clock::DefaultClock, state::{InMemoryState, NotKeyed}};
use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::event_bus::{DaemonEvent, TimelineEntry};
use crate::gemini_cli::GeminiCli;

type GovernorLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

// ===== Request-scoped context (zero-cost propagation via task_local) =====

tokio::task_local! {
    /// Caller session ID — set at IPC handler entry point.
    pub(crate) static REQUEST_SESSION_ID: String;
    /// Caller identity — set by each call site before calling send().
    pub(crate) static REQUEST_CALLER: String;
}

/// Read session ID from task-local context (returns None if not set).
fn current_session_id() -> Option<String> {
    REQUEST_SESSION_ID.try_with(|id| id.clone()).ok()
}

/// Read caller identity from task-local context.
fn current_caller() -> String {
    REQUEST_CALLER.try_with(|c| c.clone()).unwrap_or_else(|_| "unknown".to_string())
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

    /// Create a CLI-mode client that routes through Gemini CLI subprocess.
    pub fn with_cli(cli: GeminiCli, event_tx: tokio::sync::mpsc::UnboundedSender<TimelineEntry>) -> Self {
        // CLI mode: relax rate limit (Google One AI Pro has generous limits)
        let quota = Quota::per_minute(NonZeroU32::new(60).unwrap());
        Self {
            rate_limiter: Arc::new(RateLimiter::direct(quota)),
            semaphore: Arc::new(Semaphore::new(3)),
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
        let queue_start = Instant::now();
        let permit = self.semaphore.acquire().await
            .map_err(|_| anyhow!("Gemini semaphore closed"))?;

        // 2. Wait for RPM budget
        self.rate_limiter.until_ready().await;
        let queue_wait = queue_start.elapsed();

        // 3. Dispatch based on mode (measure API duration)
        let api_start = Instant::now();
        let api_mode;
        let result = if let Some(cli) = &self.cli {
            api_mode = "cli";
            let messages = body.get("messages").and_then(|v| v.as_array())
                .ok_or_else(|| anyhow!("CLI mode: missing 'messages' in body"))?;
            let max_tokens = body.get("max_tokens").and_then(|v| v.as_u64()).map(|n| n as u32);

            // Whitelist: only these models are allowed via CLI. Others fall back to default.
            let cli_model = body.get("model").and_then(|v| v.as_str())
                .and_then(|m| match m {
                    "gemini-3.1-flash-lite" | "gemini-3.1-pro-preview" => Some(m),
                    _ => None,
                });

            let resp = cli.call(messages, cli_model, max_tokens, None).await;
            drop(permit);
            let resp = resp?;

            Ok(serde_json::json!({
                "choices": [{"message": {"content": resp.content}, "finish_reason": "stop"}],
                "model": resp.model,
            }))
        } else {
            api_mode = "http";
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
                // Retry path — emit event after retry completes
                let retry_result = self.retry_with_backoff(http_client, url, jwt, body, 3).await;
                let api_duration = api_start.elapsed();
                self.emit_event(&request_id, &caller, session_id, api_mode, &model, prompt_chars,
                    &retry_result, queue_wait, api_duration, 1); // at least 1 retry
                return retry_result;
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
        };
        let api_duration = api_start.elapsed();

        // 4. Emit instrumentation event
        self.emit_event(&request_id, &caller, session_id, api_mode, &model, prompt_chars,
            &result, queue_wait, api_duration, 0);

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

    fn emit_event(
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
    ) {
        let (status, response_chars, error_msg) = match result {
            Ok(v) => {
                let chars = v.pointer("/choices/0/message/content")
                    .and_then(|c| c.as_str())
                    .map(|s| s.len())
                    .unwrap_or(0);
                ("ok".to_string(), chars, None)
            }
            Err(e) => {
                let msg = e.to_string();
                let status = if msg.contains("timed out") { "timeout" } else { "error" };
                (status.to_string(), 0, Some(msg))
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
        let event = DaemonEvent::GeminiRequestCompleted {
            request_id: request_id.to_string(),
            caller: caller.to_string(),
            session_id,
            api_mode: api_mode.to_string(),
            model: model.to_string(),
            prompt_chars,
            response_chars,
            queue_wait_ms: queue_wait.as_millis() as u64,
            duration_ms: api_duration.as_millis() as u64,
            retry_count,
            status,
            error_msg,
        };
        let entry = TimelineEntry {
            event,
            trace_id: trace_id,
            span_id: uuid::Uuid::new_v4().to_string(),
            parent_span_id: None,
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
