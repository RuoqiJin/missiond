//! MinimaxGateway — centralized Actor for all MiniMax API calls.
//!
//! All workers (Briefing, Translation, Embedding) and MCP tools route
//! through this gateway for unified rate limiting, priority scheduling,
//! and quota tracking.
//!
//! Architecture:
//! - 4 priority channels (interactive > embedding > translation > briefing)
//! - `tokio::select! { biased; }` ensures strict priority ordering
//! - Sliding-window quota tracker (300 requests / 5 hours)
//! - Semaphore-based concurrency limit (max 3 in-flight)
//! - Automatic EventBus emission for every call (zero boilerplate for workers)

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use tokio::sync::{mpsc, oneshot, RwLock, Semaphore};
use tokio::time::Instant;
use tracing::{debug, info, warn};

use crate::event_bus::TimelineEntry;
use crate::minimax_client::{ChatMessage, MiniMaxClient};

// ── Constants ──────────────────────────────────────────────────────

/// MiniMax Coding Plan quota: 300 prompts per 5 hours.
const QUOTA_MAX: usize = 300;
const QUOTA_WINDOW: Duration = Duration::from_secs(5 * 3600);

/// Max concurrent in-flight API requests.
const MAX_CONCURRENCY: usize = 3;

/// Channel capacity per priority level.
const CHANNEL_CAPACITY: usize = 64;

/// Minimum inter-request delay to avoid burst (shared across all callers).
const MIN_INTER_REQUEST_MS: u64 = 500;

// ── Request / Response ─────────────────────────────────────────────

/// A request routed through the gateway.
pub(crate) struct GatewayRequest {
    pub messages: Vec<ChatMessage>,
    pub max_tokens: Option<u32>,
    pub caller: &'static str,
    pub task_id: Option<String>,
    pub reply_tx: oneshot::Sender<Result<String>>,
}

// ── QuotaTracker ───────────────────────────────────────────────────

/// Sliding-window quota tracker (300 req / 5h).
#[derive(Debug)]
pub(crate) struct QuotaTracker {
    history: VecDeque<Instant>,
    max_quota: usize,
    window: Duration,
}

impl QuotaTracker {
    pub fn new(max_quota: usize, window: Duration) -> Self {
        Self {
            history: VecDeque::new(),
            max_quota,
            window,
        }
    }

    /// Purge expired entries and return current usage count.
    pub fn used(&mut self) -> usize {
        let now = Instant::now();
        while let Some(&t) = self.history.front() {
            if now.duration_since(t) > self.window {
                self.history.pop_front();
            } else {
                break;
            }
        }
        self.history.len()
    }

    /// Record a new request. Returns false if quota exceeded.
    pub fn try_record(&mut self) -> bool {
        let used = self.used();
        if used >= self.max_quota {
            return false;
        }
        self.history.push_back(Instant::now());
        true
    }

    pub fn remaining(&mut self) -> usize {
        self.max_quota.saturating_sub(self.used())
    }
}

// ── MinimaxHandle (Clone, given to workers) ────────────────────────

/// Lightweight handle cloned into AppState for all workers to use.
#[derive(Clone)]
pub(crate) struct MinimaxHandle {
    tx_interactive: mpsc::Sender<GatewayRequest>,
    tx_embedding: mpsc::Sender<GatewayRequest>,
    tx_translation: mpsc::Sender<GatewayRequest>,
    tx_briefing: mpsc::Sender<GatewayRequest>,
    pub quota: Arc<RwLock<QuotaTracker>>,
}

impl MinimaxHandle {
    /// Send a request at the given priority. Returns the LLM response content.
    async fn send(
        tx: &mpsc::Sender<GatewayRequest>,
        messages: Vec<ChatMessage>,
        max_tokens: Option<u32>,
        caller: &'static str,
        task_id: Option<String>,
    ) -> Result<String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let req = GatewayRequest {
            messages,
            max_tokens,
            caller,
            task_id,
            reply_tx,
        };
        tx.send(req).await.map_err(|_| anyhow!("MinimaxGateway channel closed"))?;
        reply_rx.await.map_err(|_| anyhow!("MinimaxGateway dropped response"))?
    }

    /// P0: Interactive / MCP calls (highest priority).
    pub async fn call_interactive(
        &self,
        messages: Vec<ChatMessage>,
        max_tokens: Option<u32>,
        caller: &'static str,
    ) -> Result<String> {
        Self::send(&self.tx_interactive, messages, max_tokens, caller, None).await
    }

    /// P1: Embedding worker calls.
    pub async fn call_embedding(
        &self,
        messages: Vec<ChatMessage>,
        max_tokens: Option<u32>,
        task_id: Option<String>,
    ) -> Result<String> {
        Self::send(&self.tx_embedding, messages, max_tokens, "embedding", task_id).await
    }

    /// P2: Translation worker calls.
    pub async fn call_translation(
        &self,
        messages: Vec<ChatMessage>,
        max_tokens: Option<u32>,
        task_id: Option<String>,
    ) -> Result<String> {
        Self::send(&self.tx_translation, messages, max_tokens, "translation", task_id).await
    }

    /// P3: Briefing worker calls (lowest priority).
    pub async fn call_briefing(
        &self,
        messages: Vec<ChatMessage>,
        max_tokens: Option<u32>,
        task_id: Option<String>,
    ) -> Result<String> {
        Self::send(&self.tx_briefing, messages, max_tokens, "briefing", task_id).await
    }
}

// ── Gateway Actor ──────────────────────────────────────────────────

/// The gateway actor — runs as an independent tokio task.
pub(crate) struct MinimaxGateway {
    rx_interactive: mpsc::Receiver<GatewayRequest>,
    rx_embedding: mpsc::Receiver<GatewayRequest>,
    rx_translation: mpsc::Receiver<GatewayRequest>,
    rx_briefing: mpsc::Receiver<GatewayRequest>,
    client: MiniMaxClient,
    quota: Arc<RwLock<QuotaTracker>>,
    concurrency: Arc<Semaphore>,
    event_tx: tokio::sync::mpsc::UnboundedSender<TimelineEntry>,
}

impl MinimaxGateway {
    /// Main loop: biased select ensures strict priority ordering.
    pub async fn run(mut self) {
        info!("MinimaxGateway started (quota: {}/{:?}, concurrency: {})",
            QUOTA_MAX, QUOTA_WINDOW, MAX_CONCURRENCY);

        let mut last_request_at = Instant::now() - Duration::from_secs(10);

        loop {
            // 1. Biased priority select
            let req = tokio::select! {
                biased;
                Some(r) = self.rx_interactive.recv() => r,
                Some(r) = self.rx_embedding.recv() => r,
                Some(r) = self.rx_translation.recv() => r,
                Some(r) = self.rx_briefing.recv() => r,
                else => {
                    info!("MinimaxGateway: all senders dropped, shutting down");
                    break;
                }
            };

            let queue_wait_start = Instant::now();

            // 2. Quota check (spin-wait with backoff if exhausted)
            loop {
                let can_proceed = {
                    let mut q = self.quota.write().await;
                    q.try_record()
                };
                if can_proceed {
                    break;
                }
                warn!(caller = req.caller, "MinimaxGateway: quota exhausted (300/5h), throttling 60s");
                tokio::time::sleep(Duration::from_secs(60)).await;
            }

            // 3. Min inter-request delay (prevent burst)
            let since_last = Instant::now().duration_since(last_request_at);
            let min_gap = Duration::from_millis(MIN_INTER_REQUEST_MS);
            if since_last < min_gap {
                tokio::time::sleep(min_gap - since_last).await;
            }
            last_request_at = Instant::now();

            // 4. Acquire concurrency permit
            let permit = self.concurrency.clone().acquire_owned().await;
            let permit = match permit {
                Ok(p) => p,
                Err(_) => {
                    let _ = req.reply_tx.send(Err(anyhow!("Gateway semaphore closed")));
                    continue;
                }
            };

            let queue_wait_ms = queue_wait_start.elapsed().as_millis() as u64;

            // 5. Execute request (spawned for concurrency)
            let client = self.client.clone();
            let event_tx = self.event_tx.clone();
            tokio::spawn(async move {
                let api_start = std::time::Instant::now();
                let prompt_chars: usize = req.messages.iter().map(|m| m.content.len()).sum();

                let result = client.chat(&req.messages, req.max_tokens).await;

                let duration_ms = api_start.elapsed().as_millis() as u64;
                let (status, response_chars) = match &result {
                    Ok(s) => ("ok", s.len()),
                    Err(_) => ("error", 0),
                };

                debug!(
                    caller = req.caller,
                    task_id = req.task_id.as_deref().unwrap_or("-"),
                    prompt_chars,
                    response_chars,
                    duration_ms,
                    queue_wait_ms,
                    status,
                    "minimax_gateway_call"
                );

                // Emit timeline event
                let summary = format!(
                    "MiniMax {} → {}ch ({}ms, q:{}ms)",
                    req.caller, response_chars, duration_ms, queue_wait_ms
                );
                let event = crate::event_bus::DaemonEvent::WorkerLlmCall {
                    caller: req.caller.to_string(),
                    task_id: req.task_id.clone(),
                    status: status.to_string(),
                    prompt_chars,
                    response_chars,
                    duration_ms,
                    queue_wait_ms,
                };
                let entry = TimelineEntry {
                    event,
                    trace_id: req.task_id,
                    span_id: uuid::Uuid::new_v4().to_string(),
                    parent_span_id: None,
                    summary: Some(summary),
                };
                let _ = event_tx.send(entry);

                // Reply to caller
                let _ = req.reply_tx.send(result);
                drop(permit);
            });
        }
    }
}

// ── Initialization ─────────────────────────────────────────────────

/// Create the gateway and its handle. Call once at startup.
/// Returns (handle_for_AppState, gateway_actor_to_spawn).
pub(crate) fn create_minimax_gateway(
    event_tx: tokio::sync::mpsc::UnboundedSender<TimelineEntry>,
) -> Option<(MinimaxHandle, MinimaxGateway)> {
    let api_key = crate::minimax_client::load_api_key()?;
    let client = MiniMaxClient::new(api_key);

    let (tx_interactive, rx_interactive) = mpsc::channel(CHANNEL_CAPACITY);
    let (tx_embedding, rx_embedding) = mpsc::channel(CHANNEL_CAPACITY);
    let (tx_translation, rx_translation) = mpsc::channel(CHANNEL_CAPACITY);
    let (tx_briefing, rx_briefing) = mpsc::channel(CHANNEL_CAPACITY);

    let quota = Arc::new(RwLock::new(QuotaTracker::new(QUOTA_MAX, QUOTA_WINDOW)));

    let handle = MinimaxHandle {
        tx_interactive,
        tx_embedding,
        tx_translation,
        tx_briefing,
        quota: quota.clone(),
    };

    let gateway = MinimaxGateway {
        rx_interactive,
        rx_embedding,
        rx_translation,
        rx_briefing,
        client,
        quota,
        concurrency: Arc::new(Semaphore::new(MAX_CONCURRENCY)),
        event_tx,
    };

    Some((handle, gateway))
}
