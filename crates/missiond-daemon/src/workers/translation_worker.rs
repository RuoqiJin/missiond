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
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::event_bus::{DaemonEvent, TraceContext, TimelineEvent};
use crate::minimax_client::ChatMessage;
use crate::state::AppState;

/// Virtual slot_id for translation events — routes them to the Slot swimlane.
const TRANSLATION_SLOT_ID: &str = "translation-worker";

/// Poll interval when idle (no pending translations).
const IDLE_POLL_SECS: u64 = 120;

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
async fn translate_message(
    state: &AppState,
    ctx: &ThinkingTraceCtx,
    content: &str,
) -> Result<()> {
    let content_chars = content.len();

    let minimax = state.minimax.as_ref()
        .ok_or_else(|| anyhow::anyhow!("MiniMax gateway not available"))?;

    // Single span_id for the entire translation lifecycle (Started + Completed/Failed)
    let translation_span_id = uuid::Uuid::new_v4().to_string();
    let trace_ctx = || TraceContext {
        trace_id: ctx.trace_id.clone(),
        span_id: Some(translation_span_id.clone()),
        parent_span_id: ctx.span_id.clone(), // Links back to thinking_message
        ..Default::default()
    };

    // Publish TranslationStarted
    state.event_bus.publish_traced(
        DaemonEvent::TranslationStarted {
            message_id: ctx.message_id,
            slot_id: TRANSLATION_SLOT_ID.to_string(),
            content_chars,
        },
        trace_ctx(),
    );

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
    // Pass trace context to MiniMax Gateway for WorkerLlmCall linking
    let result = minimax.call_translation(
        messages,
        Some(max_tokens),
        None,
        ctx.trace_id.clone(),
        Some(translation_span_id.clone()), // WorkerLlmCall's parent = translation span
    ).await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(translation) if !translation.is_empty() => {
            // Store in DB
            state.mission.db().insert_translation(
                ctx.message_id,
                &translation,
                "MiniMax-M2.5-highspeed",
                duration_ms,
            )?;

            // Preview: first ~80 chars
            let preview: String = translation.chars().take(80).collect();

            state.event_bus.publish_traced(
                DaemonEvent::TranslationCompleted {
                    message_id: ctx.message_id,
                    slot_id: TRANSLATION_SLOT_ID.to_string(),
                    preview,
                    duration_ms,
                },
                trace_ctx(),
            );

            info!(message_id = ctx.message_id, duration_ms, chars = translation.len(), "Translation completed");
            Ok(())
        }
        Ok(_) => {
            let err = "empty translation returned";
            state.event_bus.publish_traced(
                DaemonEvent::TranslationFailed {
                    message_id: ctx.message_id,
                    slot_id: TRANSLATION_SLOT_ID.to_string(),
                    error: err.to_string(),
                },
                trace_ctx(),
            );
            warn!(message_id = ctx.message_id, "Translation: empty response from MiniMax");
            Err(anyhow::anyhow!(err))
        }
        Err(e) => {
            state.event_bus.publish_traced(
                DaemonEvent::TranslationFailed {
                    message_id: ctx.message_id,
                    slot_id: TRANSLATION_SLOT_ID.to_string(),
                    error: e.to_string(),
                },
                trace_ctx(),
            );
            warn!(message_id = ctx.message_id, error = %e, "Translation failed");
            Err(e)
        }
    }
}

/// Extract trace context from a thinking_message TimelineEvent.
fn extract_thinking_ctx(event: &TimelineEvent) -> Option<ThinkingTraceCtx> {
    if event.event.wire_type() != "thinking_message" {
        return None;
    }
    let payload = event.event.to_frontend_payload();
    let message_id = payload.get("message_id").and_then(|v| v.as_i64())?;
    Some(ThinkingTraceCtx {
        message_id,
        trace_id: event.trace_id.clone(),
        span_id: Some(event.span_id.clone()),
    })
}

pub(crate) struct TranslationWorker {
    pub timeline_rx: broadcast::Receiver<TimelineEvent>,
}

impl super::BackgroundWorker for TranslationWorker {
    fn name(&self) -> &'static str { "translation_worker" }

    async fn run(self, state: Arc<AppState>) {
        info!("Translation worker started (event-driven + {IDLE_POLL_SECS}s fallback poll, rate: gateway-managed)");

        // Initial delay to let daemon stabilize
        tokio::time::sleep(Duration::from_secs(15)).await;

        run_loop(state, self.timeline_rx).await;
    }
}

async fn run_loop(
    state: Arc<AppState>,
    mut timeline_rx: broadcast::Receiver<TimelineEvent>,
) {
    loop {
        // Hybrid wait: event-driven (broadcast) OR idle timeout (DB poll)
        let maybe_ctx = tokio::select! {
            result = timeline_rx.recv() => {
                match result {
                    Ok(event) => extract_thinking_ctx(&event),
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "Translation worker: broadcast lagged, will poll DB");
                        None // Trigger DB poll below
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Translation worker: broadcast closed, shutting down");
                        return;
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(IDLE_POLL_SECS)) => {
                None // Trigger DB poll
            }
        };

        if let Some(ctx) = maybe_ctx {
            // Event-driven path: translate a specific message
            process_single(&state, ctx).await;
            // No local sleep — Gateway handles rate limiting
        } else {
            // Poll path: find thinking messages without translations
            poll_pending(&state).await;
        }
    }
}

/// Process a single message with its trace context.
async fn process_single(state: &AppState, ctx: ThinkingTraceCtx) {
    // Check if already translated
    match state.mission.db().has_translation(ctx.message_id) {
        Ok(true) => {
            debug!(message_id = ctx.message_id, "Translation: already exists, skipping");
            return;
        }
        Err(e) => {
            warn!(message_id = ctx.message_id, error = %e, "Translation: DB check failed");
            return;
        }
        _ => {}
    }

    // Fetch full content
    let content = match state.mission.db().get_conversation_message_by_id(ctx.message_id) {
        Ok(Some(msg)) => msg.content,
        Ok(None) => {
            debug!(message_id = ctx.message_id, "Translation: message not found");
            return;
        }
        Err(e) => {
            warn!(message_id = ctx.message_id, error = %e, "Translation: DB fetch failed");
            return;
        }
    };

    // Skip very short thinking blocks (< 50 chars not worth translating)
    if content.len() < 50 {
        debug!(message_id = ctx.message_id, len = content.len(), "Translation: too short, skipping");
        return;
    }

    if let Err(e) = translate_message(state, &ctx, &content).await {
        warn!(message_id = ctx.message_id, error = %e, "Translation: failed");
    }
}

/// Poll DB for thinking messages that don't have translations yet.
async fn poll_pending(state: &AppState) {
    // Find thinking_message timeline entries from the last 24h that lack translations
    let db = state.mission.db();
    let rows = match db.query_timeline_filtered(
        Some("thinking_message"),
        None,
        None,
        None,
        50,
        0,
    ) {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "Translation poll: DB query failed");
            return;
        }
    };

    let mut translated = 0;
    for row in &rows {
        // Extract message_id from payload
        let payload: serde_json::Value = serde_json::from_str(&row.payload).unwrap_or_default();
        let msg_id = match payload.get("message_id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => continue,
        };

        // Skip if already translated
        match db.has_translation(msg_id) {
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

        process_single(state, ctx).await;
        translated += 1;
        // No local sleep — Gateway handles rate limiting
    }

    if translated > 0 {
        info!(translated, total_checked = rows.len(), "Translation poll: batch completed");
    }
}
