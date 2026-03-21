//! Gemini Request Logger — subscribes to Timeline broadcast, persists LLM call logs.
//!
//! Two-step persistence: insert on started (with prompt_text), update on completed
//! (with response_text). Full content lives in `gemini_requests` table; timeline
//! only stores request_id references.

use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::event_bus::{self, TimelineEvent};
use crate::state::AppState;

pub(crate) struct GeminiLoggerWorker {
    pub timeline_rx: broadcast::Receiver<TimelineEvent>,
}

impl super::BackgroundWorker for GeminiLoggerWorker {
    fn name(&self) -> &'static str {
        "gemini_logger"
    }

    async fn run(self, state: Arc<AppState>, _ctx: super::WorkerContext) {
        let mut rx = self.timeline_rx;

        // Startup cleanup: remove logs older than 7 days
        if let Ok(deleted) = state.store.gemini_log_cleanup(7).await {
            if deleted > 0 {
                info!(deleted, "Gemini log: cleaned up old entries");
            }
        }

        loop {
            match rx.recv().await {
                Ok(te) => handle_event(&state, &te.event).await,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(
                        skipped = n,
                        "Gemini log subscriber lagged, some requests not logged"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("Gemini log subscriber: event bus closed");
                    break;
                }
            }
        }
    }
}

async fn handle_event(state: &AppState, event: &event_bus::DaemonEvent) {
    let store = state.store.as_ref();
    match event {
        // Unified CLI engine events
        event_bus::DaemonEvent::CliRequestStarted {
            engine,
            request_id,
            caller,
            session_id,
            model,
            prompt_chars,
            prompt_text,
            ..
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
                warn!(error = %e, engine = %engine, "CLI log: failed to insert started");
            }
        }
        event_bus::DaemonEvent::CliRequestCompleted {
            engine,
            request_id,
            response_chars,
            duration_ms,
            status,
            error_msg,
            response_text,
            extra,
            ..
        } => {
            let api_mode_default = format!("{}-cli", engine);
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
                warn!(error = %e, engine = %engine, "CLI log: failed to update completed");
            }
        }
        // Legacy engine-specific events
        event_bus::DaemonEvent::GeminiRequestStarted {
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
        event_bus::DaemonEvent::GeminiRequestCompleted {
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
        event_bus::DaemonEvent::CodexRequestStarted {
            request_id,
            caller,
            model,
            prompt_chars,
            prompt_text,
            ..
        } => {
            if let Err(e) = store
                .gemini_log_insert_started(
                    request_id,
                    caller,
                    None,
                    model,
                    *prompt_chars as i64,
                    prompt_text.as_deref(),
                )
                .await
            {
                warn!(error = %e, "Codex log: failed to insert started");
            }
        }
        event_bus::DaemonEvent::CodexRequestCompleted {
            request_id,
            response_chars,
            duration_ms,
            status,
            error_msg,
            response_text,
            ..
        } => {
            if let Err(e) = store
                .gemini_log_update_completed(
                    request_id,
                    "codex-cli",
                    *response_chars as i64,
                    0,
                    *duration_ms as i64,
                    0,
                    status,
                    error_msg.as_deref(),
                    response_text.as_deref(),
                )
                .await
            {
                warn!(error = %e, "Codex log: failed to update completed");
            }
        }
        _ => {}
    }
}
