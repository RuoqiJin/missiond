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

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use governor::{Quota, RateLimiter, clock::DefaultClock, state::{InMemoryState, NotKeyed}};
use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::gemini_cli::GeminiCli;

type GovernorLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

/// Shared Gemini rate limiter + retry client.
/// Stored in `AppState` and used by all Gemini call sites.
#[derive(Clone)]
pub(crate) struct GeminiClient {
    rate_limiter: Arc<GovernorLimiter>,
    semaphore: Arc<Semaphore>,
    cli: Option<Arc<GeminiCli>>,
}

impl GeminiClient {
    /// Create a new HTTP-mode client with 20 RPM limit and 3 max concurrent requests.
    pub fn new() -> Self {
        let quota = Quota::per_minute(NonZeroU32::new(20).unwrap());
        Self {
            rate_limiter: Arc::new(RateLimiter::direct(quota)),
            semaphore: Arc::new(Semaphore::new(3)),
            cli: None,
        }
    }

    /// Create a CLI-mode client that routes through Gemini CLI subprocess.
    pub fn with_cli(cli: GeminiCli) -> Self {
        // CLI mode: relax rate limit (Google One AI Pro has generous limits)
        let quota = Quota::per_minute(NonZeroU32::new(60).unwrap());
        Self {
            rate_limiter: Arc::new(RateLimiter::direct(quota)),
            semaphore: Arc::new(Semaphore::new(3)),
            cli: Some(Arc::new(cli)),
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
        // 1. Acquire concurrency permit
        let permit = self.semaphore.acquire().await
            .map_err(|_| anyhow!("Gemini semaphore closed"))?;

        // 2. Wait for RPM budget
        self.rate_limiter.until_ready().await;

        // 3. Dispatch based on mode
        if let Some(cli) = &self.cli {
            // CLI mode: extract messages from body, call CLI
            // Ignore body's model — use CLI's configured default_model
            // (call sites may pass model names incompatible with CLI, e.g. "gemini-3.1-pro" vs "gemini-3.1-pro-preview")
            let messages = body.get("messages").and_then(|v| v.as_array())
                .ok_or_else(|| anyhow!("CLI mode: missing 'messages' in body"))?;
            let max_tokens = body.get("max_tokens").and_then(|v| v.as_u64()).map(|n| n as u32);

            let resp = cli.call(messages, None, max_tokens, None).await;
            drop(permit);
            let resp = resp?;

            // Convert to OpenAI-compatible format (all call sites use /choices/0/message/content)
            Ok(serde_json::json!({
                "choices": [{"message": {"content": resp.content}, "finish_reason": "stop"}],
                "model": resp.model,
            }))
        } else {
            // HTTP mode: existing logic
            let resp = http_client.post(url)
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {}", jwt))
                .json(body)
                .send()
                .await
                .map_err(|e| anyhow!("Gemini request failed: {}", e))?;

            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let err_body = resp.text().await.unwrap_or_default();
                drop(permit); // CRITICAL: release permit before sleeping
                warn!(body = %err_body, "Gemini 429 RESOURCE_EXHAUSTED, starting retry");
                return self.retry_with_backoff(http_client, url, jwt, body, 3).await;
            }

            drop(permit);

            if !resp.status().is_success() {
                let status = resp.status();
                let err_body = resp.text().await.unwrap_or_default();
                return Err(anyhow!("Router returned {}: {}", status, err_body));
            }

            resp.json().await
                .map_err(|e| anyhow!("Failed to parse Gemini response: {}", e))
        }
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
                drop(permit); // Release before next sleep
                if attempt < max_retries {
                    warn!(attempt, "Gemini 429 again, will retry");
                    continue;
                }
                let err_body = resp.text().await.unwrap_or_default();
                return Err(anyhow!("Gemini 429 after {} retries: {}", max_retries, err_body));
            }

            drop(permit);

            if !resp.status().is_success() {
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
