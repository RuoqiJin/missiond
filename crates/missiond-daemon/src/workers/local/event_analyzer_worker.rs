//! Event Analyzer Worker — detects git commits from Claude Code conversation logs.
//!
//! Subscribes to the timeline broadcast and listens for ToolCompleted events
//! from Bash tool executions. When a git commit signature is detected in the
//! tool output, emits a ContextualCommitDetected event enriched with
//! conversation context (session, slot) for causal tracking.
//!
//! Pure regex matching, zero LLM calls.

use std::sync::{Arc, LazyLock};

use regex::Regex;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

use super::{BackgroundWorker, WorkerContext, WorkerKind};
use crate::event_bus::{DaemonEvent, TimelineEvent};
use crate::state::AppState;

/// Regex to extract branch, commit hash, and summary from git commit output.
/// Matches: `[main 9facdfa] refactor: some changes`
/// Captures: (1) branch, (2) hash, (3) summary
static GIT_COMMIT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[([a-zA-Z0-9_\-/\.]+)\s+([a-f0-9]{7,40})\]\s+(.*)").unwrap()
});

/// Quick check substring — avoids regex on every Bash tool output.
const COMMIT_HINT: &str = " files changed,";
const COMMIT_HINT2: &str = " file changed,";
const COMMIT_HINT3: &str = "create mode";

pub(crate) struct EventAnalyzerWorker {
    pub timeline_rx: broadcast::Receiver<TimelineEvent>,
}

impl BackgroundWorker for EventAnalyzerWorker {
    const KIND: WorkerKind = WorkerKind::Local;

    fn name(&self) -> &'static str {
        "event-analyzer"
    }

    fn run(
        self,
        state: Arc<AppState>,
        ctx: WorkerContext,
    ) -> impl std::future::Future<Output = ()> + Send {
        run_event_analyzer(state, ctx, self.timeline_rx)
    }
}

async fn run_event_analyzer(
    state: Arc<AppState>,
    mut ctx: WorkerContext,
    mut rx: broadcast::Receiver<TimelineEvent>,
) {
    loop {
        ctx.wait_if_paused().await;
        match rx.recv().await {
            Ok(te) => {
                if let DaemonEvent::ToolCompleted {
                    ref session_id,
                    ref slot_id,
                    ref tool_name,
                    ref output_summary,
                    is_error,
                    ..
                } = te.event
                {
                    // Fast-filter: only Bash tool, non-error results
                    if tool_name != "Bash" || is_error {
                        continue;
                    }

                    // Fast-filter: check for git commit output hints before regex
                    if !output_summary.contains(COMMIT_HINT)
                        && !output_summary.contains(COMMIT_HINT2)
                        && !output_summary.contains(COMMIT_HINT3)
                        && !output_summary.contains("insertions(+)")
                        && !output_summary.contains("deletions(-)")
                    {
                        debug!(
                            session_id = %session_id,
                            "EventAnalyzer: Bash output skipped (no git commit hint)"
                        );
                        continue;
                    }

                    // Regex extract: [branch hash] summary
                    if let Some(caps) = GIT_COMMIT_RE.captures(output_summary) {
                        let branch = caps[1].to_string();
                        let commit_hash = caps[2].to_string();
                        let summary = caps[3].trim().to_string();

                        info!(
                            branch = %branch,
                            commit_hash = %commit_hash,
                            summary = %summary,
                            session_id = %session_id,
                            slot_id = ?slot_id,
                            "EventAnalyzer: git commit detected from conversation"
                        );

                        let event = DaemonEvent::ContextualCommitDetected {
                            commit_hash,
                            branch,
                            summary,
                            conversation_id: session_id.clone(),
                            message_id: 0, // ToolCompleted does not carry message_id
                            session_id: session_id.clone(),
                            slot_id: slot_id.clone(),
                        };

                        state.event_bus.publish(event);
                        ctx.record_success();
                    }
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                debug!(skipped = n, "EventAnalyzer: broadcast lagged");
            }
            Err(broadcast::error::RecvError::Closed) => {
                error!("EventAnalyzer: broadcast channel closed, shutting down");
                break;
            }
        }
    }
}
