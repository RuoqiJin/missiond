//! Retrospective Worker — automatic session performance analysis.
//!
//! Polls for completed sessions that meet analysis thresholds
//! (message count > 100, tool calls > 50, duration > 1h, error rate > 25%).
//! Runs `mission_retrospective` quick mode, saves results to DB.
//! High-anomaly sessions get full Gemini analysis.

use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn, debug};

use crate::state::AppState;

/// Poll interval between checks (1 hour).
const POLL_INTERVAL_SECS: u64 = 3600;

/// Startup delay to let the system stabilize.
const STARTUP_DELAY_SECS: u64 = 120;

/// Waste ratio threshold for triggering full (Gemini) analysis.
const FULL_ANALYSIS_WASTE_THRESHOLD: f64 = 30.0;

/// Error rate threshold for triggering full analysis.
const FULL_ANALYSIS_ERROR_THRESHOLD: f64 = 25.0;

/// Rate limit between session analyses (seconds).
const INTER_SESSION_DELAY_SECS: u64 = 10;

pub(crate) struct RetroWorker;

impl super::BackgroundWorker for RetroWorker {
    fn name(&self) -> &'static str { "retro_worker" }

    async fn run(self, state: Arc<AppState>, mut ctx: super::WorkerContext) {
        info!("Retro worker started (poll: {}s, startup delay: {}s)",
              POLL_INTERVAL_SECS, STARTUP_DELAY_SECS);

        tokio::time::sleep(Duration::from_secs(STARTUP_DELAY_SECS)).await;

        loop {
            ctx.wait_if_paused().await;

            match process_pending(&state).await {
                Ok(count) => {
                    if count > 0 {
                        info!(count, "Retro worker: analyzed sessions");
                        ctx.record_success();
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Retro worker: processing error");
                    ctx.record_failure();
                }
            }

            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
        }
    }
}

async fn process_pending(state: &AppState) -> anyhow::Result<usize> {
    let db = state.mission.db();

    let pending = db.get_sessions_needing_retrospective()
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;

    if pending.is_empty() {
        return Ok(0);
    }

    debug!(count = pending.len(), "Retro worker: found sessions needing analysis");

    let mut analyzed = 0;
    for (session_id, msg_count, tool_count, error_rate) in &pending {
        match analyze_session(state, session_id, *msg_count, *tool_count, *error_rate).await {
            Ok(_) => {
                analyzed += 1;
                // Rate limit between sessions
                tokio::time::sleep(Duration::from_secs(INTER_SESSION_DELAY_SECS)).await;
            }
            Err(e) => {
                warn!(session_id, error = %e, "Retro worker: session analysis failed");
            }
        }
    }

    Ok(analyzed)
}

async fn analyze_session(
    state: &AppState,
    session_id: &str,
    msg_count: i64,
    tool_count: i64,
    error_rate: f64,
) -> anyhow::Result<()> {
    let db = state.mission.db();

    // Determine trigger reason
    let trigger = if error_rate > 25.0 {
        format!("error_rate_{:.0}%", error_rate)
    } else if msg_count > 100 {
        format!("msg_count_{}", msg_count)
    } else if tool_count > 50 {
        format!("tool_count_{}", tool_count)
    } else {
        "duration_1h+".to_string()
    };

    debug!(session_id, trigger = %trigger, "Retro worker: analyzing session");

    // Run quick analysis via the handler
    let args = serde_json::json!({ "sessionId": session_id, "depth": "quick" });
    let result = crate::handlers::retrospective::handle(state, "mission_retrospective", args).await?;

    // Extract the JSON text from ToolResult
    let stats_text = result.content.first()
        .map(|c| match c {
            missiond_mcp::tools::ToolContent::Text { text } => text.as_str(),
        })
        .unwrap_or("{}");

    // Check if full analysis is warranted
    let stats: serde_json::Value = serde_json::from_str(stats_text).unwrap_or_default();
    let waste_str = stats["meta"]["wasteRatio"].as_str().unwrap_or("0%");
    let waste_val: f64 = waste_str.trim_end_matches('%').parse().unwrap_or(0.0);

    let needs_full = waste_val > FULL_ANALYSIS_WASTE_THRESHOLD
        || error_rate > FULL_ANALYSIS_ERROR_THRESHOLD;

    let full_analysis = if needs_full {
        debug!(session_id, waste = %waste_str, error_rate, "Retro worker: triggering full analysis");
        let full_args = serde_json::json!({ "sessionId": session_id, "depth": "full" });
        match crate::handlers::retrospective::handle(state, "mission_retrospective", full_args).await {
            Ok(full_result) => {
                full_result.content.first()
                    .map(|c| match c {
                        missiond_mcp::tools::ToolContent::Text { text } => text.clone(),
                    })
            }
            Err(e) => {
                warn!(session_id, error = %e, "Retro worker: full analysis failed, keeping quick stats");
                None
            }
        }
    } else {
        None
    };

    // Persist results
    db.save_retrospective_result(
        session_id,
        &trigger,
        stats_text,
        full_analysis.as_deref(),
    ).map_err(|e| anyhow::anyhow!("DB error saving retrospective: {}", e))?;

    info!(session_id, trigger = %trigger, full = needs_full, "Retro worker: session analyzed");
    Ok(())
}
