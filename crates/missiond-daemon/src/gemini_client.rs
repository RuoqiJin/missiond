//! Rate-limited, retry-aware Gemini API client.
//!
//! All daemon components that call Gemini (or any model via the Router API)
//! should go through `GeminiClient` to avoid 429 RESOURCE_EXHAUSTED errors.
//!
//! GCP Tier 1 quota for gemini-3.1-pro is only 25 RPM. This module provides:
//! - RPM rate limiting via `governor` (20 RPM, leaving 5 headroom)
//! - Concurrency limiting via `tokio::sync::Semaphore` (max 3 concurrent)
//! - Automatic 429 retry with exponential backoff (up to 3 retries)

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use governor::{Quota, RateLimiter, clock::DefaultClock, state::{InMemoryState, NotKeyed}};
use tokio::sync::Semaphore;
use tracing::{info, warn};

type GovernorLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

/// Shared Gemini rate limiter + retry client.
/// Stored in `AppState` and used by all Gemini call sites.
#[derive(Clone)]
pub(crate) struct GeminiClient {
    rate_limiter: Arc<GovernorLimiter>,
    semaphore: Arc<Semaphore>,
}

impl GeminiClient {
    /// Create a new client with 20 RPM limit and 3 max concurrent requests.
    pub fn new() -> Self {
        let quota = Quota::per_minute(NonZeroU32::new(20).unwrap());
        Self {
            rate_limiter: Arc::new(RateLimiter::direct(quota)),
            semaphore: Arc::new(Semaphore::new(3)),
        }
    }

    /// Send a request to the Router/Gemini API with rate limiting and 429 retry.
    ///
    /// Handles:
    /// 1. Semaphore acquire (concurrency limit)
    /// 2. Rate limiter wait (RPM limit)
    /// 3. HTTP request
    /// 4. 429 → drop permit, exponential backoff, retry up to 3 times
    /// 5. Other errors → propagate
    /// 6. Success → parse JSON response
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

        // 3. Send request
        let resp = http_client.post(url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", jwt))
            .json(body)
            .send()
            .await
            .map_err(|e| anyhow!("Gemini request failed: {}", e))?;

        // 4. Handle 429 with retry
        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let err_body = resp.text().await.unwrap_or_default();
            drop(permit); // CRITICAL: release permit before sleeping
            warn!(body = %err_body, "Gemini 429 RESOURCE_EXHAUSTED, starting retry");
            return self.retry_with_backoff(http_client, url, jwt, body, 3).await;
        }

        drop(permit);

        // 5. Check other HTTP errors
        if !resp.status().is_success() {
            let status = resp.status();
            let err_body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Router returned {}: {}", status, err_body));
        }

        // 6. Parse JSON
        resp.json().await
            .map_err(|e| anyhow!("Failed to parse Gemini response: {}", e))
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
