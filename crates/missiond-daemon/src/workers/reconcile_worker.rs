//! Daily Reconcile Worker — background JSONL-to-DB integrity checker.
//!
//! Periodically scans all JSONL files under `~/.claude/projects/`, compares
//! file sizes against watermarks, and re-ingests any gaps using
//! `insert_conversation_messages_batch()` (ON CONFLICT DO NOTHING).
//!
//! Design: Lazy Initialization — conversation records are only created
//! when actual messages are found, preventing ghost/empty sessions.
//!
//! Design doc: `docs/designs/reconcile-worker.md`

use std::path::Path;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};
use tracing::{debug, info, warn};

use crate::events_sync;
use crate::state::AppState;

use super::{BackgroundWorker, WorkerContext};

/// Sentinel key in reconcile_watermarks for global completion timestamp.
const GLOBAL_WATERMARK_KEY: &str = "__last_full_reconcile__";
/// Yield after this many lines to avoid starving other tasks.
const YIELD_EVERY_N_LINES: usize = 100;
/// Flush batch to DB after this many messages (prevents OOM on large files).
const BATCH_FLUSH_SIZE: usize = 500;
/// Initial delay before first run (let system settle after startup).
const INITIAL_DELAY_SECS: u64 = 300; // 5 minutes
/// Interval between runs.
const INTERVAL_SECS: u64 = 86400; // 24 hours
/// Log progress every N files.
const PROGRESS_LOG_INTERVAL: usize = 100;
/// File not modified for this long → completed (1 hour).
const ACTIVE_THRESHOLD_SECS: u64 = 3600;

pub(crate) struct ReconcileWorker;

impl BackgroundWorker for ReconcileWorker {
    fn name(&self) -> &'static str { "reconcile" }

    async fn run(self, state: Arc<AppState>, mut ctx: WorkerContext) {
        // Delay first run to let startup settle
        tokio::time::sleep(std::time::Duration::from_secs(INITIAL_DELAY_SECS)).await;

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(INTERVAL_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            ctx.wait_if_paused().await;
            run_reconciliation(&state).await;
            ctx.record_success();
        }
    }
}

/// Run reconciliation immediately (called from MCP tool or daily timer).
pub(crate) async fn run_reconciliation_now(state: &AppState) {
    run_reconciliation(state).await;
}

async fn run_reconciliation(state: &AppState) {
    info!("Reconcile: starting daily scan");
    let start = chrono::Utc::now();

    // Load all watermarks in one query
    let watermarks = match state.store.get_all_reconcile_watermarks().await {
        Ok(w) => w,
        Err(e) => {
            warn!(error = %e, "Reconcile: failed to load watermarks");
            return;
        }
    };

    // Scan all JSONL files
    let projects_dir = dirs::home_dir()
        .map(|h| h.join(".claude").join("projects"))
        .unwrap_or_default();

    let files = match scan_jsonl_files(&projects_dir).await {
        Ok(f) => f,
        Err(e) => {
            warn!(error = %e, "Reconcile: failed to scan projects dir");
            return;
        }
    };

    let total_files = files.len();
    let mut files_reconciled = 0usize;
    let mut messages_recovered = 0usize;
    let mut files_skipped = 0usize;
    let mut files_processed = 0usize;

    info!(total_files, "Reconcile: scan started");

    for file_path in &files {
        files_processed += 1;

        // Progress logging every N files
        if files_processed % PROGRESS_LOG_INTERVAL == 0 {
            info!(
                progress = format!("{}/{}", files_processed, total_files),
                files_reconciled,
                messages_recovered,
                "Reconcile: progress"
            );
        }

        let path_str = file_path.to_string_lossy().to_string();

        // Check current file size and metadata
        let metadata = match tokio::fs::metadata(file_path).await {
            Ok(m) => m,
            Err(_) => continue,
        };
        let current_size = metadata.len() as i64;

        let last_size = watermarks.get(&path_str).copied().unwrap_or(0);

        if current_size <= last_size {
            files_skipped += 1;
            continue;
        }

        // Extract session_id from file path
        let session_id = match file_path.file_stem().and_then(|s| s.to_str()) {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => continue,
        };

        // Determine status from file modification time
        let status = infer_status(&metadata);
        // Determine conversation_type from session_id pattern
        let conv_type = infer_conversation_type(&session_id);

        // Reconcile the gap (lazy init: only creates conversation if messages found)
        let recovered = reconcile_file_gap(
            state, &path_str, &session_id, last_size, &status, &conv_type,
        ).await;
        messages_recovered += recovered;

        // Update watermark to current size
        if let Err(e) = state.store.upsert_reconcile_watermark(&path_str, current_size).await {
            warn!(path = %path_str, error = %e, "Reconcile: failed to update watermark");
        }

        files_reconciled += 1;

        // Yield between files to avoid starving other tasks
        tokio::task::yield_now().await;
    }

    // Update global completion timestamp
    let ts = start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    if let Err(e) = state.store.upsert_reconcile_watermark(GLOBAL_WATERMARK_KEY, start.timestamp()).await {
        warn!(error = %e, "Reconcile: failed to update global watermark");
    }

    info!(
        total_files,
        files_skipped,
        files_reconciled,
        messages_recovered,
        completed_at = %ts,
        "Reconcile: daily scan complete"
    );
}

/// Infer conversation status from file modification time.
fn infer_status(metadata: &std::fs::Metadata) -> String {
    let modified = metadata.modified().ok()
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs())
        .unwrap_or(u64::MAX);

    if modified > ACTIVE_THRESHOLD_SECS {
        "completed".to_string()
    } else {
        "active".to_string()
    }
}

/// Infer conversation_type from session_id pattern.
fn infer_conversation_type(session_id: &str) -> String {
    if session_id.starts_with("agent-acompact-") {
        "compaction".to_string()
    } else if session_id.starts_with("agent-") {
        "subagent".to_string()
    } else {
        "user".to_string()
    }
}

/// Scan all .jsonl files under the projects directory (iterative BFS).
async fn scan_jsonl_files(projects_dir: &Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    let mut dirs_to_scan = vec![projects_dir.to_path_buf()];

    while let Some(dir) = dirs_to_scan.pop() {
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_dir() {
                dirs_to_scan.push(path);
            } else if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                let ps = path.to_string_lossy();
                if !ps.contains("tool-results") && !ps.contains("session-memory") {
                    files.push(path);
                }
            }
        }
    }

    Ok(files)
}

/// Reconcile a single file from `from_offset` to EOF.
/// Lazy initialization: only creates conversation record when messages are found.
/// Returns count of newly recovered messages.
async fn reconcile_file_gap(
    state: &AppState,
    jsonl_path: &str,
    session_id: &str,
    from_offset: i64,
    status: &str,
    conversation_type: &str,
) -> usize {
    let path = Path::new(jsonl_path);
    if !path.exists() {
        return 0;
    }

    // Check if slot session (affects role mapping)
    let is_slot_session = state.store.get_slot_for_session(session_id).await.unwrap_or(None).is_some();

    let file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) => {
            debug!(path = %jsonl_path, error = %e, "Reconcile: failed to open file");
            return 0;
        }
    };

    let mut reader = BufReader::new(file);

    // Seek to from_offset if > 0
    if from_offset > 0 {
        if let Err(e) = reader.seek(std::io::SeekFrom::Start(from_offset as u64)).await {
            warn!(path = %jsonl_path, error = %e, "Reconcile: seek failed");
            return 0;
        }
    }

    // NOTE: ensure_conversation_exists is NOT called here.
    // It's deferred to the first batch flush (lazy init).

    let project_path = Path::new(jsonl_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut batch: Vec<missiond_core::types::ConversationMessage> = Vec::with_capacity(BATCH_FLUSH_SIZE);
    let mut lines = reader.lines();
    let mut line_count = 0usize;
    let mut total_recovered = 0usize;
    let mut conversation_ensured = false;
    let sid = session_id.to_string();

    while let Ok(Some(line)) = lines.next_line().await {
        line_count += 1;

        // Yield periodically to avoid starving other tasks
        if line_count % YIELD_EVERY_N_LINES == 0 {
            tokio::task::yield_now().await;
        }

        if line.trim().is_empty() {
            continue;
        }

        let msg: missiond_core::CCMessageLine = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(_) => continue, // Skip non-message lines (progress, system, etc.)
        };

        // Skip tool_use lines (same as normal pipeline)
        if msg.message_type == "tool_use" {
            continue;
        }

        let text_content = events_sync::extract_text_content(&msg.message.content);
        let content_types: Vec<&str> = msg.message.content.as_array()
            .map(|arr| arr.iter()
                .filter_map(|b| b.get("type").and_then(|t| t.as_str()))
                .collect())
            .unwrap_or_default();
        let is_tool_result = !content_types.is_empty() && content_types.iter().all(|t| *t == "tool_result");
        let is_thinking = !content_types.is_empty() && content_types.iter().all(|t| *t == "thinking");

        if text_content.is_empty() && !is_tool_result {
            continue;
        }

        let content = if text_content.is_empty() {
            "[tool_result]".to_string()
        } else {
            text_content
        };

        let role = if is_tool_result {
            "tool_result".to_string()
        } else if is_thinking {
            "thinking".to_string()
        } else if msg.message.role == "user" && is_slot_session {
            "system".to_string()
        } else {
            msg.message.role.clone()
        };

        let raw_content = events_sync::sanitize_raw_content(&msg.message.content);
        let tool_name = events_sync::extract_tool_names_csv(&msg.message.content);

        batch.push(missiond_core::types::ConversationMessage {
            id: 0,
            session_id: sid.clone(),
            raw_role: None,
            role,
            content,
            raw_content,
            message_uuid: Some(msg.uuid.clone()),
            parent_uuid: msg.parent_uuid.clone(),
            model: msg.message.model.clone(),
            timestamp: msg.timestamp.clone(),
            metadata: None,
            tool_name,
            content_types: None,
            has_image: false,
            has_tool_use: false,
            has_tool_result: false,
            token_count: None,
            seq: None,
            role_display: None,
        });

        // Batch flush: avoid unbounded memory growth on large files
        if batch.len() >= BATCH_FLUSH_SIZE {
            // Lazy init: ensure conversation exists before first insert
            if !conversation_ensured {
                let _ = state.store.ensure_conversation_exists(
                    session_id, &project_path, jsonl_path, status, conversation_type,
                ).await;
                conversation_ensured = true;
            }
            // Sort by message_uuid to prevent PG deadlocks from concurrent batch inserts
            batch.sort_by(|a, b| a.message_uuid.cmp(&b.message_uuid));
            if let Ok(ids) = state.store.insert_conversation_messages_batch(&batch).await {
                total_recovered += ids.len();
            }
            batch.clear();
            tokio::task::yield_now().await;
        }
    }

    // Flush remaining
    if !batch.is_empty() {
        // Lazy init: ensure conversation exists before first insert
        if !conversation_ensured {
            let _ = state.store.ensure_conversation_exists(
                session_id, &project_path, jsonl_path, status, conversation_type,
            ).await;
        }
        batch.sort_by(|a, b| a.message_uuid.cmp(&b.message_uuid));
        if let Ok(ids) = state.store.insert_conversation_messages_batch(&batch).await {
            total_recovered += ids.len();
        }
    }

    // Update message_count from actual DB rows (self-sufficient, don't rely on hot path)
    if total_recovered > 0 {
        let _ = state.store.refresh_conversation_message_count(session_id).await;
        info!(
            session = %session_id,
            recovered = total_recovered,
            lines_read = line_count,
            "Reconcile: recovered missing messages"
        );
    }

    total_recovered
}
