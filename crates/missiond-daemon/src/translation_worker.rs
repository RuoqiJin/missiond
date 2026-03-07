//! Translation Worker — async background translation of thinking messages.
//!
//! Listens for thinking_message events on the Timeline broadcast, translates
//! the full content to Chinese via MiniMax HTTP, and stores the result in
//! the `message_translations` table.
//!
//! Architecture: event-driven via broadcast subscription + DB fallback polling.
//! - Event-driven: woken when a thinking_message is persisted to timeline.
//! - Polling: fallback 120s sweep for missed entries (restart recovery).
//!
//! Translation lifecycle events (TranslationStarted/Completed/Failed) are
//! published to EventBus with a virtual slot_id for Slot swimlane visibility.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::event_bus::{DaemonEvent, TimelineEvent};
use crate::minimax_client::MiniMaxClient;
use crate::state::AppState;

/// Virtual slot_id for translation events — routes them to the Slot swimlane.
const TRANSLATION_SLOT_ID: &str = "translation-worker";

/// Delay between consecutive translation API calls.
const INTER_CALL_DELAY_SECS: u64 = 5;

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

/// Translate a single thinking message.
async fn translate_message(
    client: &MiniMaxClient,
    state: &AppState,
    message_id: i64,
    content: &str,
) -> Result<()> {
    let content_chars = content.len();

    // Publish TranslationStarted
    state.event_bus.publish(DaemonEvent::TranslationStarted {
        message_id,
        slot_id: TRANSLATION_SLOT_ID.to_string(),
        content_chars,
    });

    let start = std::time::Instant::now();

    // Build messages with system prompt
    let messages = vec![
        crate::minimax_client::ChatMessage {
            role: "system".to_string(),
            content: TRANSLATION_SYSTEM_PROMPT.to_string(),
        },
        crate::minimax_client::ChatMessage {
            role: "user".to_string(),
            content: content.to_string(),
        },
    ];

    // Use higher max_tokens for translation (output ~ input length)
    let max_tokens = ((content_chars / 2) as u32 + 500).min(8192);
    let result = client.chat(&messages, Some(max_tokens)).await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(translation) if !translation.is_empty() => {
            // Store in DB
            state.mission.db().insert_translation(
                message_id,
                &translation,
                "MiniMax-M2.5-highspeed",
                duration_ms,
            )?;

            // Preview: first ~80 chars
            let preview: String = translation.chars().take(80).collect();

            state.event_bus.publish(DaemonEvent::TranslationCompleted {
                message_id,
                slot_id: TRANSLATION_SLOT_ID.to_string(),
                preview,
                duration_ms,
            });

            info!(message_id, duration_ms, chars = translation.len(), "Translation completed");
            Ok(())
        }
        Ok(_) => {
            let err = "empty translation returned";
            state.event_bus.publish(DaemonEvent::TranslationFailed {
                message_id,
                slot_id: TRANSLATION_SLOT_ID.to_string(),
                error: err.to_string(),
            });
            warn!(message_id, "Translation: empty response from MiniMax");
            Err(anyhow::anyhow!(err))
        }
        Err(e) => {
            state.event_bus.publish(DaemonEvent::TranslationFailed {
                message_id,
                slot_id: TRANSLATION_SLOT_ID.to_string(),
                error: e.to_string(),
            });
            warn!(message_id, error = %e, "Translation failed");
            Err(e)
        }
    }
}

/// Extract message_id from a thinking_message TimelineEvent payload.
fn extract_thinking_message_id(event: &TimelineEvent) -> Option<i64> {
    if event.event.wire_type() != "thinking_message" {
        return None;
    }
    let payload = event.event.to_frontend_payload();
    payload.get("message_id").and_then(|v| v.as_i64())
}

/// Spawn the translation worker.
pub(crate) fn spawn_translation_worker(
    state: Arc<AppState>,
    timeline_rx: broadcast::Receiver<TimelineEvent>,
) {
    let api_key = match crate::minimax_client::load_api_key() {
        Some(key) => key,
        None => {
            warn!("Translation worker: MiniMax API key not found, worker disabled");
            return;
        }
    };

    let client = MiniMaxClient::new(api_key);

    tokio::spawn(async move {
        info!("Translation worker started (event-driven + {IDLE_POLL_SECS}s fallback poll)");

        // Initial delay to let daemon stabilize
        tokio::time::sleep(Duration::from_secs(15)).await;

        run_loop(state, client, timeline_rx).await;
    });
}

async fn run_loop(
    state: Arc<AppState>,
    client: MiniMaxClient,
    mut timeline_rx: broadcast::Receiver<TimelineEvent>,
) {
    loop {
        // Hybrid wait: event-driven (broadcast) OR idle timeout (DB poll)
        let maybe_msg_id = tokio::select! {
            result = timeline_rx.recv() => {
                match result {
                    Ok(event) => extract_thinking_message_id(&event),
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

        if let Some(msg_id) = maybe_msg_id {
            // Event-driven path: translate a specific message
            process_single(&client, &state, msg_id).await;
            tokio::time::sleep(Duration::from_secs(INTER_CALL_DELAY_SECS)).await;
        } else {
            // Poll path: find thinking messages without translations
            poll_pending(&client, &state).await;
        }
    }
}

/// Process a single message by ID.
async fn process_single(client: &MiniMaxClient, state: &AppState, message_id: i64) {
    // Check if already translated
    match state.mission.db().has_translation(message_id) {
        Ok(true) => {
            debug!(message_id, "Translation: already exists, skipping");
            return;
        }
        Err(e) => {
            warn!(message_id, error = %e, "Translation: DB check failed");
            return;
        }
        _ => {}
    }

    // Fetch full content
    let content = match state.mission.db().get_conversation_message_by_id(message_id) {
        Ok(Some(msg)) => msg.content,
        Ok(None) => {
            debug!(message_id, "Translation: message not found");
            return;
        }
        Err(e) => {
            warn!(message_id, error = %e, "Translation: DB fetch failed");
            return;
        }
    };

    // Skip very short thinking blocks (< 50 chars not worth translating)
    if content.len() < 50 {
        debug!(message_id, len = content.len(), "Translation: too short, skipping");
        return;
    }

    if let Err(e) = translate_message(client, state, message_id, &content).await {
        warn!(message_id, error = %e, "Translation: failed");
    }
}

/// Poll DB for thinking messages that don't have translations yet.
async fn poll_pending(client: &MiniMaxClient, state: &AppState) {
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

        process_single(client, state, msg_id).await;
        translated += 1;
        tokio::time::sleep(Duration::from_secs(INTER_CALL_DELAY_SECS)).await;
    }

    if translated > 0 {
        info!(translated, total_checked = rows.len(), "Translation poll: batch completed");
    }
}
