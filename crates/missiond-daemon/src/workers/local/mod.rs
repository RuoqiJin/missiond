//! Local workers — pure computation, no LLM dependency.
//! Workers in this directory get no ControlTree provider dependencies.

use std::time::Duration;

use tracing::info;

pub(crate) mod ast_sync_worker;
pub(crate) mod code_prefetch;
pub(crate) mod codex_ingestion_worker;
pub(crate) mod conversation_logger;
pub(crate) mod conversation_organizer;
pub(crate) mod experience_harvester;
pub(crate) mod gemini_logger;
pub(crate) mod gemini_reconcile_worker;
pub(crate) mod message_labeler;
pub(crate) mod pty_event_worker;
pub(crate) mod reconcile_worker;
pub(crate) mod tagger_chunker;
pub(crate) mod xjpcode_briefing_worker;

// Re-exports so workers can use `super::BackgroundWorker` etc.
pub(crate) use super::{BackgroundWorker, WorkerContext, WorkerKind};

const DEFAULT_BACKGROUND_DB_GRACE_SECS: u64 = 30;
const MAX_BACKGROUND_DB_GRACE_SECS: u64 = 300;

pub(crate) const BACKGROUND_DB_GRACE_ENV: &str = "MISSIOND_BACKGROUND_DB_GRACE_SECS";

fn env_u64_bounded(name: &str, default: u64, min: u64, max: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|value| value.clamp(min, max))
        .unwrap_or(default)
}

pub(crate) fn background_db_grace_secs() -> u64 {
    env_u64_bounded(
        BACKGROUND_DB_GRACE_ENV,
        DEFAULT_BACKGROUND_DB_GRACE_SECS,
        0,
        MAX_BACKGROUND_DB_GRACE_SECS,
    )
}

pub(crate) async fn wait_for_background_db_grace(worker_name: &str) {
    let secs = background_db_grace_secs();
    if secs == 0 {
        return;
    }
    info!(
        worker = worker_name,
        secs,
        env = BACKGROUND_DB_GRACE_ENV,
        "background DB worker startup grace before maintenance batch"
    );
    tokio::time::sleep(Duration::from_secs(secs)).await;
}

pub(crate) fn env_i64_bounded(name: &str, default: i64, min: i64, max: i64) -> i64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .map(|value| value.clamp(min, max))
        .unwrap_or(default)
}
