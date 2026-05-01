//! Translation Worker — async background translation of thinking messages.
//!
//! Listens for thinking_message events on the Timeline broadcast, translates
//! the full content to Chinese via MinimaxGateway (P2 priority), and stores
//! the result in the `message_translations` table.
//!
//! Architecture: event-driven via broadcast subscription + DB fallback polling.
//! - Event-driven: woken when a thinking_message is persisted to timeline.
//! - Polling: fallback 120s sweep for missed entries (restart recovery).
//!
//! Translation lifecycle events (TranslationStarted/Completed/Failed) are
//! published to EventBus with a virtual slot_id for Slot swimlane visibility.
//! Rate limiting delegated to MinimaxGateway (no local sleep).
//!
//! Cross-lane causal linking: thinking_message (Chat) → Translation (Slot) → WorkerLlmCall (GPT).
//! Trace context is extracted from broadcast/DB and propagated through function params.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tracing::{debug, info, warn};

use crate::minimax_client::ChatMessage;
use crate::state::AppState;
use missiond_core::event::events::{MessageEvent, WorkerEvent};
use missiond_core::event::subscription::SubscriptionOpts;

/// Virtual slot_id for translation events — routes them to the Slot swimlane.
const TRANSLATION_SLOT_ID: &str = "translation-worker";

/// Poll interval when idle (no pending translations).
const IDLE_POLL_SECS: u64 = 120;

/// Minimum seconds between poll_pending calls (cooldown).
const POLL_COOLDOWN_SECS: u64 = 60;

/// Consecutive failures before circuit breaker trips.
const CIRCUIT_BREAKER_THRESHOLD: u32 = 5;

/// How long the circuit breaker stays open (seconds).
const CIRCUIT_BREAKER_COOLDOWN_SECS: u64 = 300;

/// System prompt for structure-preserving translation.
const TRANSLATION_SYSTEM_PROMPT: &str = "\
你是一个专业的 IT 技术翻译引擎。你的任务是将大语言模型的内部思考过程 (Thinking Process) 准确、流畅地翻译成简体中文。

【严格遵守以下规则】
1. 绝对保留原始文本中的所有 Markdown 格式（如加粗、列表、代码块 ``` 及其语言标识）。
2. 绝对保留原始文本中的所有 XML/HTML 标签（如 <context>, <step> 等），不得翻译标签名，不得改变标签闭合关系。
3. 代码块内部的代码和注释不要翻译，保持原样。
4. 专业计算机术语请使用行业通用中文表达（如 token, prompt, agent 也可以保留英文原文）。
5. 仅输出翻译后的结果，不要包含任何解释、寒暄或额外的包装词（如\"这是翻译结果\"）。";

/// Trace context extracted from the source thinking_message event.
struct ThinkingTraceCtx {
    message_id: i64,
    trace_id: Option<String>,
    /// span_id of the thinking_message event (becomes parent of translation span).
    span_id: Option<String>,
}

/// Translate a single thinking message via MinimaxGateway (P2: translation priority).
/// `ctx` carries the source thinking_message's trace context for causal linking.
async fn translate_message(state: &AppState, ctx: &ThinkingTraceCtx, content: &str) -> Result<()> {
    let content_chars = content.len();

    let sonnet = state
        .sonnet
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Sonnet gateway not available"))?;
    let model = sonnet.model().to_string();

    // Single span_id for the entire translation lifecycle (Started + Completed/Failed)
    let translation_span_id = uuid::Uuid::new_v4().to_string();
    let _ = ctx.span_id.clone(); // retained for future SpanContext wiring
    let _ = ctx.trace_id.clone();

    // Publish TranslationStarted
    let _ = state
        .bus
        .publish_worker(WorkerEvent::TranslationStarted {
            message_id: ctx.message_id,
            slot_id: TRANSLATION_SLOT_ID.to_string(),
            content_chars,
        })
        .await;

    let start = std::time::Instant::now();

    // Build messages with system prompt
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: TRANSLATION_SYSTEM_PROMPT.to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: content.to_string(),
        },
    ];

    // Use higher max_tokens for translation (output ~ input length)
    let max_tokens = ((content_chars / 2) as u32 + 500).min(8192);
    // Pass trace context to SonnetGateway for WorkerLlmCall linking
    let result = sonnet
        .call_translation(
            messages,
            Some(max_tokens),
            None,
            ctx.trace_id.clone(),
            Some(translation_span_id.clone()), // WorkerLlmCall's parent = translation span
        )
        .await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(translation) if !translation.is_empty() => {
            // Store in DB
            state
                .store
                .insert_translation(ctx.message_id, &translation, &model, duration_ms)
                .await?;

            // Preview: first ~80 chars
            let preview: String = translation.chars().take(80).collect();

            let _ = state
                .bus
                .publish_worker(WorkerEvent::TranslationCompleted {
                    message_id: ctx.message_id,
                    slot_id: TRANSLATION_SLOT_ID.to_string(),
                    preview,
                    duration_ms,
                })
                .await;

            info!(
                message_id = ctx.message_id,
                duration_ms,
                chars = translation.len(),
                "Translation completed"
            );
            Ok(())
        }
        Ok(_) => {
            let err = "empty translation returned";
            let _ = state
                .bus
                .publish_worker(WorkerEvent::TranslationFailed {
                    message_id: ctx.message_id,
                    slot_id: TRANSLATION_SLOT_ID.to_string(),
                    error: err.to_string(),
                })
                .await;
            warn!(
                message_id = ctx.message_id,
                "Translation: empty response from MiniMax"
            );
            Err(anyhow::anyhow!(err))
        }
        Err(e) => {
            let _ = state
                .bus
                .publish_worker(WorkerEvent::TranslationFailed {
                    message_id: ctx.message_id,
                    slot_id: TRANSLATION_SLOT_ID.to_string(),
                    error: e.to_string(),
                })
                .await;
            warn!(message_id = ctx.message_id, error = %e, "Translation failed");
            Err(e)
        }
    }
}

pub(crate) struct TranslationWorker;

impl super::BackgroundWorker for TranslationWorker {
    const KIND: super::WorkerKind = super::WorkerKind::Sonnet;

    fn name(&self) -> &'static str {
        "translation_worker"
    }

    async fn run(self, state: Arc<AppState>, ctx: super::WorkerContext) {
        info!("Translation worker started (event-driven + {IDLE_POLL_SECS}s fallback poll, rate: gateway-managed)");

        // Initial delay to let daemon stabilize
        tokio::time::sleep(Duration::from_secs(15)).await;

        run_loop(state, ctx).await;
    }
}

async fn run_loop(state: Arc<AppState>, mut wctx: super::WorkerContext) {
    let mut consecutive_failures: u32 = 0;

    // v2 subscription: MessageEvent::Logged(role="thinking") drives translation.
    let mut sub = match state
        .bus
        .subscribe::<MessageEvent>(
            "translation_worker",
            SubscriptionOpts::named("translation_worker"),
        )
        .await
    {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "Translation worker: bus subscribe failed, falling back to poll");
            run_loop_poll_only(state, wctx).await;
            return;
        }
    };
    let mut last_poll = tokio::time::Instant::now() - Duration::from_secs(POLL_COOLDOWN_SECS + 1);

    loop {
        // Cooperative pause: block here if externally paused via WorkerRegistry
        wctx.wait_if_paused().await;

        // Circuit breaker: pause after repeated failures
        if consecutive_failures >= CIRCUIT_BREAKER_THRESHOLD {
            warn!(
                failures = consecutive_failures,
                cooldown_secs = CIRCUIT_BREAKER_COOLDOWN_SECS,
                "Translation worker: circuit breaker tripped, pausing"
            );
            tokio::time::sleep(Duration::from_secs(CIRCUIT_BREAKER_COOLDOWN_SECS)).await;
            consecutive_failures = 0;
        }

        let maybe_ctx = tokio::select! {
            ack = sub.next() => {
                let Some(ack) = ack else {
                    info!("Translation worker: subscription closed, shutting down");
                    return;
                };
                let ctx_opt = if let MessageEvent::Logged { message_id, role, .. } = ack.event() {
                    if role == "thinking" {
                        Some(ThinkingTraceCtx {
                            message_id: *message_id,
                            trace_id: None,
                            span_id: None,
                        })
                    } else {
                        None
                    }
                } else {
                    None
                };
                ack.ack().await;
                ctx_opt
            }
            _ = tokio::time::sleep(Duration::from_secs(IDLE_POLL_SECS)) => {
                if last_poll.elapsed() >= Duration::from_secs(POLL_COOLDOWN_SECS) {
                    last_poll = tokio::time::Instant::now();
                    poll_pending(&state, &mut consecutive_failures).await;
                }
                continue;
            }
            _ = wctx.wait_until_paused() => {
                continue;
            }
        };

        if let Some(ctx) = maybe_ctx {
            let ok = process_single(&state, ctx).await;
            if ok {
                consecutive_failures = 0;
                wctx.record_success();
            } else {
                consecutive_failures += 1;
                wctx.record_failure();
            }
        }
    }
}

/// Degraded loop when the v2 subscription cannot be established. Pure poll.
async fn run_loop_poll_only(state: Arc<AppState>, mut wctx: super::WorkerContext) {
    let mut consecutive_failures: u32 = 0;
    loop {
        wctx.wait_if_paused().await;
        poll_pending(&state, &mut consecutive_failures).await;
        tokio::time::sleep(Duration::from_secs(IDLE_POLL_SECS)).await;
    }
}

/// Process a single message with its trace context. Returns true on success (or skip), false on failure.
async fn process_single(state: &AppState, ctx: ThinkingTraceCtx) -> bool {
    // Check if already translated
    match state.store.has_translation(ctx.message_id).await {
        Ok(true) => {
            debug!(
                message_id = ctx.message_id,
                "Translation: already exists, skipping"
            );
            return true;
        }
        Err(e) => {
            warn!(message_id = ctx.message_id, error = %e, "Translation: DB check failed");
            return false;
        }
        _ => {}
    }

    // Fetch full content
    let content = match state
        .store
        .get_conversation_message_by_id(ctx.message_id)
        .await
    {
        Ok(Some(msg)) => msg.content,
        Ok(None) => {
            debug!(
                message_id = ctx.message_id,
                "Translation: message not found"
            );
            return true; // Not a failure — message just doesn't exist
        }
        Err(e) => {
            warn!(message_id = ctx.message_id, error = %e, "Translation: DB fetch failed");
            return false;
        }
    };

    // Skip very short thinking blocks (< 50 chars not worth translating)
    if content.len() < 50 {
        debug!(
            message_id = ctx.message_id,
            len = content.len(),
            "Translation: too short, skipping"
        );
        return true;
    }

    match translate_message(state, &ctx, &content).await {
        Ok(()) => true,
        Err(e) => {
            warn!(message_id = ctx.message_id, error = %e, "Translation: failed");
            false
        }
    }
}

/// Poll DB for thinking messages that don't have translations yet.
///
/// v1.3.0 SSOT cutover: event_log.kind="logged" carries MessageEvent::Logged;
/// filter by `role=="thinking"` via the payload projection downstream.
async fn poll_pending(state: &AppState, consecutive_failures: &mut u32) {
    let rows = match state
        .store
        .query_timeline_filtered(Some("message::logged"), None, None, None, 50, 0)
        .await
    {
        Ok(r) => r
            .into_iter()
            .filter(|r| {
                serde_json::from_str::<serde_json::Value>(&r.payload)
                    .ok()
                    .and_then(|v| {
                        v.get("Logged")
                            .and_then(|inner| inner.get("role"))
                            .and_then(|role| role.as_str())
                            .map(|s| s == "thinking")
                    })
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>(),
        Err(e) => {
            warn!(error = %e, "Translation poll: DB query failed");
            return;
        }
    };

    let mut translated = 0;
    for row in &rows {
        // Circuit breaker: stop batch early if too many failures
        if *consecutive_failures >= CIRCUIT_BREAKER_THRESHOLD {
            debug!("Translation poll: circuit breaker threshold reached, stopping batch");
            break;
        }

        // Extract message_id from externally-tagged payload:
        //   `{"Logged":{"message_id": N, ...}}`
        let payload: serde_json::Value = serde_json::from_str(&row.payload).unwrap_or_default();
        let msg_id = match payload
            .get("Logged")
            .and_then(|inner| inner.get("message_id"))
            .and_then(|v| v.as_i64())
        {
            Some(id) => id,
            None => continue,
        };

        // Skip if already translated
        match state.store.has_translation(msg_id).await {
            Ok(true) => continue,
            Err(_) => continue,
            _ => {}
        }

        // Build trace context from DB row (for causal linking on poll path)
        let ctx = ThinkingTraceCtx {
            message_id: msg_id,
            trace_id: row.trace_id.clone(),
            span_id: row.span_id.clone(),
        };

        let ok = process_single(state, ctx).await;
        if ok {
            *consecutive_failures = 0;
            translated += 1;
        } else {
            *consecutive_failures += 1;
        }
    }

    if translated > 0 {
        info!(
            translated,
            total_checked = rows.len(),
            "Translation poll: batch completed"
        );
    }
}
