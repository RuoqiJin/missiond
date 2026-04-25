//! SonnetGateway — centralized Actor for all Claude Sonnet **chat** API calls.
//!
//! v0.3 (intent-worker.lisp § xjp-router-gateway): embedding has moved to the
//! xjp-router client. Sonnet only handles chat lanes (interactive / translation /
//! briefing / intent). The legacy `embedding` priority lane has been removed.
//!
//! Architecture:
//! - 4 priority channels (interactive > translation > briefing > intent)
//! - `tokio::select! { biased; }` ensures strict priority ordering
//! - Independent rate limiter (30 RPM, separate from Gemini governor)
//! - Semaphore-based concurrency limit (max 3 in-flight)
//! - Automatic EventBus emission for every call

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use tokio::sync::{mpsc, oneshot, Semaphore};
use tokio::time::Instant;
use tracing::{debug, info, warn};

use crate::minimax_client::ChatMessage;
use crate::minimax_gateway::{GatewayRequest, Priority, QuotaTracker};
use missiond_core::event::events::WorkerEvent;

// ── Constants ──────────────────────────────────────────────────────

/// Sonnet rate limit: 30 RPM (independent from Gemini's 30 RPM).
const QUOTA_MAX: usize = 1800;
const QUOTA_WINDOW: Duration = Duration::from_secs(3600);

/// Max concurrent in-flight API requests.
const MAX_CONCURRENCY: usize = 3;

/// Channel capacity per priority level.
const CHANNEL_CAPACITY: usize = 64;

/// Minimum inter-request delay (burst prevention).
const MIN_INTER_REQUEST_MS: u64 = 300;

/// Quota reservation threshold for low-priority requests.
const QUOTA_RESERVE_THRESHOLD: usize = 200;

/// Sonnet model name on xjp-router (cpapi deprecated → claude-sonnet).
const SONNET_MODEL: &str = "claude-sonnet";

// ── SonnetHandle (Clone, given to workers) ──────────────────────────

/// Lightweight handle cloned into AppState for all workers to use.
#[derive(Clone)]
pub(crate) struct SonnetHandle {
    tx_interactive: mpsc::Sender<GatewayRequest>,
    tx_translation: mpsc::Sender<GatewayRequest>,
    tx_briefing: mpsc::Sender<GatewayRequest>,
    tx_intent: mpsc::Sender<GatewayRequest>,
}

impl SonnetHandle {
    /// Send a request at the given priority. Returns the LLM response content.
    /// Fail-fast: checks gate BEFORE enqueuing to avoid wasting channel capacity.
    async fn send(
        tx: &mpsc::Sender<GatewayRequest>,
        messages: Vec<ChatMessage>,
        max_tokens: Option<u32>,
        caller: &'static str,
        priority: Priority,
        task_id: Option<String>,
        trace_id: Option<String>,
        parent_span_id: Option<String>,
    ) -> Result<String> {
        // P1: Fail-fast gate check — reject before entering channel queue
        crate::llm_gate::check(crate::llm_gate::LlmProvider::Sonnet)?;

        let (reply_tx, reply_rx) = oneshot::channel();
        let req = GatewayRequest {
            messages,
            max_tokens,
            caller,
            priority,
            task_id,
            trace_id,
            parent_span_id,
            reply_tx,
        };
        tx.send(req)
            .await
            .map_err(|_| anyhow!("SonnetGateway channel closed"))?;
        reply_rx
            .await
            .map_err(|_| anyhow!("SonnetGateway dropped response"))?
    }

    /// P0: Interactive / MCP calls (highest priority).
    pub async fn call_interactive(
        &self,
        messages: Vec<ChatMessage>,
        max_tokens: Option<u32>,
        caller: &'static str,
    ) -> Result<String> {
        Self::send(
            &self.tx_interactive,
            messages,
            max_tokens,
            caller,
            Priority::Interactive,
            None,
            None,
            None,
        )
        .await
    }

    /// P2: Translation worker calls (with optional causal linking context).
    pub async fn call_translation(
        &self,
        messages: Vec<ChatMessage>,
        max_tokens: Option<u32>,
        task_id: Option<String>,
        trace_id: Option<String>,
        parent_span_id: Option<String>,
    ) -> Result<String> {
        Self::send(
            &self.tx_translation,
            messages,
            max_tokens,
            "translation",
            Priority::Translation,
            task_id,
            trace_id,
            parent_span_id,
        )
        .await
    }

    /// P3: Briefing worker calls.
    pub async fn call_briefing(
        &self,
        messages: Vec<ChatMessage>,
        max_tokens: Option<u32>,
        task_id: Option<String>,
    ) -> Result<String> {
        Self::send(
            &self.tx_briefing,
            messages,
            max_tokens,
            "briefing",
            Priority::Briefing,
            task_id,
            None,
            None,
        )
        .await
    }

    /// P4: Intent analyst calls (lowest priority).
    pub async fn call_intent(
        &self,
        messages: Vec<ChatMessage>,
        max_tokens: Option<u32>,
        task_id: Option<String>,
    ) -> Result<String> {
        Self::send(
            &self.tx_intent,
            messages,
            max_tokens,
            "intent",
            Priority::Intent,
            task_id,
            None,
            None,
        )
        .await
    }
}

// ── Sonnet HTTP Backend ─────────────────────────────────────────────

/// HTTP client for xjp-router's claude-sonnet endpoint.
struct SonnetBackend {
    http: reqwest::Client,
}

impl SonnetBackend {
    fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    async fn call(&self, messages: &[ChatMessage], max_tokens: Option<u32>) -> Result<String> {
        let (base_url, jwt) = crate::embedding_worker::resolve_llm_credentials().await?;
        let url = format!("{}/v1/chat/completions", base_url);

        let openai_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| serde_json::json!({"role": m.role, "content": m.content}))
            .collect();

        let body = serde_json::json!({
            "model": SONNET_MODEL,
            "messages": openai_messages,
            "max_tokens": max_tokens.unwrap_or(1024),
        });

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&jwt)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("Sonnet HTTP error: {}", e))?;

        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(anyhow!("Sonnet 429: rate limited by router"));
        }
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Sonnet HTTP {}: {}",
                status,
                missiond_core::util::safe_byte_truncate(&body_text, 200)
            ));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| anyhow!("Sonnet response parse error: {}", e))?;

        let content = json
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if content.is_empty() {
            return Err(anyhow!("Sonnet returned empty response"));
        }

        Ok(content)
    }
}

// ── Gateway Actor ──────────────────────────────────────────────────

/// The gateway actor — runs as an independent tokio task.
pub(crate) struct SonnetGateway {
    rx_interactive: mpsc::Receiver<GatewayRequest>,
    rx_translation: mpsc::Receiver<GatewayRequest>,
    rx_briefing: mpsc::Receiver<GatewayRequest>,
    rx_intent: mpsc::Receiver<GatewayRequest>,
    backend: SonnetBackend,
    quota: Arc<tokio::sync::RwLock<QuotaTracker>>,
    concurrency: Arc<Semaphore>,
    /// v2 bus: every `WorkerEvent::LlmCall` is published here.
    bus: Option<Arc<crate::bus::BusServices>>,
}

impl SonnetGateway {
    /// Attach the v2 bus so every emitted event flows through it.
    pub fn with_bus(mut self, bus: Arc<crate::bus::BusServices>) -> Self {
        self.bus = Some(bus);
        self
    }
}

impl SonnetGateway {
    /// Main loop: biased select ensures strict priority ordering.
    pub async fn run(mut self) {
        info!(
            "SonnetGateway started (quota: {}/{:?}, concurrency: {})",
            QUOTA_MAX, QUOTA_WINDOW, MAX_CONCURRENCY
        );

        let mut last_request_at = Instant::now() - Duration::from_secs(10);

        loop {
            // 1. Acquire concurrency permit FIRST (prevents priority inversion)
            let permit = match self.concurrency.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => {
                    info!("SonnetGateway: semaphore closed, shutting down");
                    break;
                }
            };

            // 2. Biased priority select (no embedding lane — moved to xjp-router v0.3)
            let req = tokio::select! {
                biased;
                Some(r) = self.rx_interactive.recv() => r,
                Some(r) = self.rx_translation.recv() => r,
                Some(r) = self.rx_briefing.recv() => r,
                Some(r) = self.rx_intent.recv() => r,
                else => {
                    info!("SonnetGateway: all senders dropped, shutting down");
                    break;
                }
            };

            // 2b. Second-line defense: drain queued requests if gate closed after enqueue
            if crate::llm_gate::is_disabled(crate::llm_gate::LlmProvider::Sonnet) {
                warn!(
                    caller = req.caller,
                    "SonnetGateway: gate closed, draining queued request"
                );
                let _ = req.reply_tx.send(Err(crate::llm_gate::LlmGateError(
                    crate::llm_gate::LlmProvider::Sonnet,
                )
                .into()));
                drop(permit);
                continue;
            }

            let queue_wait_start = Instant::now();

            // 3. Quota check with reservation protection
            let mut quota_rejected = false;
            loop {
                let mut q = self.quota.write().await;
                let remaining = q.remaining();

                if !req.priority.is_protected() && remaining <= QUOTA_RESERVE_THRESHOLD {
                    drop(q);
                    warn!(
                        caller = req.caller,
                        remaining,
                        priority = ?req.priority,
                        "SonnetGateway: quota reserved for high-priority, rejecting"
                    );
                    quota_rejected = true;
                    break;
                }

                if q.try_record() {
                    drop(q);
                    break;
                }

                drop(q);
                warn!(
                    caller = req.caller,
                    "SonnetGateway: quota exhausted, throttling 30s"
                );
                tokio::time::sleep(Duration::from_secs(30)).await;
            }

            if quota_rejected {
                let _ = req
                    .reply_tx
                    .send(Err(anyhow!("Quota reserved for high-priority requests")));
                drop(permit);
                continue;
            }

            // 4. Min inter-request delay (prevent burst)
            let since_last = Instant::now().duration_since(last_request_at);
            let min_gap = Duration::from_millis(MIN_INTER_REQUEST_MS);
            if since_last < min_gap {
                tokio::time::sleep(min_gap - since_last).await;
            }
            last_request_at = Instant::now();

            let queue_wait_ms = queue_wait_start.elapsed().as_millis() as u64;

            // 5. Execute request (spawned for concurrency)
            let backend = self.backend.http.clone();
            let bus = self.bus.clone();
            tokio::spawn(async move {
                let api_start = std::time::Instant::now();
                let prompt_chars: usize = req.messages.iter().map(|m| m.content.len()).sum();

                let backend = SonnetBackend { http: backend };
                let result = backend.call(&req.messages, req.max_tokens).await;

                let duration_ms = api_start.elapsed().as_millis() as u64;
                let (status, response_chars) = match &result {
                    Ok(s) => ("ok", s.len()),
                    Err(_) => ("error", 0),
                };

                debug!(
                    caller = req.caller,
                    task_id = req.task_id.as_deref().unwrap_or("-"),
                    priority = ?req.priority,
                    prompt_chars,
                    response_chars,
                    duration_ms,
                    queue_wait_ms,
                    status,
                    "sonnet_gateway_call"
                );

                // Emit LlmCall event via v2 bus.
                let ev = WorkerEvent::LlmCall {
                    caller: req.caller.to_string(),
                    task_id: req.task_id.clone(),
                    status: status.to_string(),
                    prompt_chars,
                    response_chars,
                    duration_ms,
                    queue_wait_ms,
                };
                if let Some(bus) = bus {
                    let _ = bus.publish_worker(ev).await;
                }
                let _ = req.trace_id;
                let _ = req.parent_span_id;

                // Reply to caller
                let _ = req.reply_tx.send(result);
                drop(permit);
            });
        }
    }
}

// ── Initialization ─────────────────────────────────────────────────

/// Create the Sonnet gateway and its handle. Call once at startup.
/// Always succeeds (credentials resolved lazily per-request).
pub(crate) fn create_sonnet_gateway() -> (SonnetHandle, SonnetGateway) {
    let backend = SonnetBackend::new();

    let (tx_interactive, rx_interactive) = mpsc::channel(CHANNEL_CAPACITY);
    let (tx_translation, rx_translation) = mpsc::channel(CHANNEL_CAPACITY);
    let (tx_briefing, rx_briefing) = mpsc::channel(CHANNEL_CAPACITY);
    let (tx_intent, rx_intent) = mpsc::channel(CHANNEL_CAPACITY);

    let quota = Arc::new(tokio::sync::RwLock::new(QuotaTracker::new(
        QUOTA_MAX,
        QUOTA_WINDOW,
    )));

    let handle = SonnetHandle {
        tx_interactive,
        tx_translation,
        tx_briefing,
        tx_intent,
    };

    let gateway = SonnetGateway {
        rx_interactive,
        rx_translation,
        rx_briefing,
        rx_intent,
        backend,
        quota,
        concurrency: Arc::new(Semaphore::new(MAX_CONCURRENCY)),
        bus: None,
    };

    (handle, gateway)
}
