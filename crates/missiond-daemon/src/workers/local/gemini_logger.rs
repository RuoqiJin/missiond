//! Gemini Request Logger — subscribes to Timeline broadcast, persists LLM call logs.
//!
//! Two-step persistence: insert on started (with prompt_text), update on completed
//! (with response_text). Full content lives in `gemini_requests` table; timeline
//! only stores request_id references.

use std::sync::Arc;
use tracing::{info, warn};

use crate::state::AppState;
use missiond_core::event::events::{LlmEvent, Provider};
use missiond_core::event::subscription::SubscriptionOpts;

pub(crate) struct GeminiLoggerWorker;

impl super::BackgroundWorker for GeminiLoggerWorker {
    const KIND: super::WorkerKind = super::WorkerKind::Local;

    fn name(&self) -> &'static str {
        "gemini_logger"
    }

    async fn run(self, state: Arc<AppState>, mut ctx: super::WorkerContext) {
        // Startup cleanup: remove logs older than 7 days
        if let Ok(deleted) = state.store.gemini_log_cleanup(7).await {
            if deleted > 0 {
                info!(deleted, "Gemini log: cleaned up old entries");
            }
        }

        let mut sub = match state
            .bus
            .subscribe::<LlmEvent>("gemini_logger", SubscriptionOpts::named("gemini_logger"))
            .await
        {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "gemini_logger: bus subscribe failed, worker exiting");
                return;
            }
        };

        loop {
            ctx.wait_if_paused().await;
            let Some(ack) = sub.next().await else {
                info!("Gemini log subscriber: subscription closed");
                break;
            };
            ctx.begin_event("llm", ack.seq().0, None);
            ctx.progress("persisting LLM event log");
            handle_event(&state, ack.event()).await;
            ctx.record_success();
            ack.ack().await;
        }
    }
}

async fn handle_event(state: &AppState, event: &LlmEvent) {
    let store = state.store.as_ref();
    match event {
        // Unified CLI engine events
        LlmEvent::RequestStarted {
            provider,
            request_id,
            caller,
            session_id,
            model,
            prompt_chars,
            prompt_text,
            ..
        } => {
            if *provider != Provider::Gemini {
                return;
            }
            if let Err(e) = store
                .gemini_log_insert_started(
                    request_id,
                    caller,
                    session_id.as_deref(),
                    model,
                    *prompt_chars as i64,
                    prompt_text.as_deref(),
                )
                .await
            {
                warn!(error = %e, provider = ?provider, "CLI log: failed to insert started");
            }
        }
        LlmEvent::RequestCompleted {
            provider,
            request_id,
            response_chars,
            duration_ms,
            status,
            error_msg,
            response_text,
            extra,
            ..
        } => {
            if *provider != Provider::Gemini {
                return;
            }
            let api_mode_default = format!("{:?}-cli", provider);
            let api_mode = extra
                .get("api_mode")
                .and_then(|v| v.as_str())
                .unwrap_or(&api_mode_default);
            let queue_wait_ms = extra
                .get("queue_wait_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let retry_count = extra
                .get("retry_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if let Err(e) = store
                .gemini_log_update_completed(
                    request_id,
                    api_mode,
                    *response_chars as i64,
                    queue_wait_ms as i64,
                    *duration_ms as i64,
                    retry_count as i64,
                    status,
                    error_msg.as_deref(),
                    response_text.as_deref(),
                )
                .await
            {
                warn!(error = %e, provider = ?provider, "CLI log: failed to update completed");
            }
        }
        // Legacy engine-specific events
        LlmEvent::LegacyGeminiRequestStarted {
            request_id,
            caller,
            session_id,
            model,
            prompt_chars,
            prompt_text,
        } => {
            if let Err(e) = store
                .gemini_log_insert_started(
                    request_id,
                    caller,
                    session_id.as_deref(),
                    model,
                    *prompt_chars as i64,
                    prompt_text.as_deref(),
                )
                .await
            {
                warn!(error = %e, "Gemini log: failed to insert started");
            }
        }
        LlmEvent::LegacyGeminiRequestCompleted {
            request_id,
            api_mode,
            response_chars,
            queue_wait_ms,
            duration_ms,
            retry_count,
            status,
            error_msg,
            response_text,
            ..
        } => {
            if let Err(e) = store
                .gemini_log_update_completed(
                    request_id,
                    api_mode,
                    *response_chars as i64,
                    *queue_wait_ms as i64,
                    *duration_ms as i64,
                    *retry_count as i64,
                    status,
                    error_msg.as_deref(),
                    response_text.as_deref(),
                )
                .await
            {
                warn!(error = %e, "Gemini log: failed to update completed");
            }
        }
        LlmEvent::LegacyCodexRequestStarted { .. }
        | LlmEvent::LegacyCodexRequestCompleted { .. } => {}
        _ => {}
    }
}
