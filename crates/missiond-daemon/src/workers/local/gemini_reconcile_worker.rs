//! Gemini CLI Reconcile Worker — background integrity checker for Gemini CLI sessions.
//!
//! Parallel to ReconcileWorker (which handles Claude Code .jsonl files),
//! this worker scans `~/.gemini/tmp/*/chats/*.json` and ensures all messages
//! are ingested into the conversation pipeline.
//!
//! Watermark: message count (not byte offset) stored in `reconcile_watermarks`.
//! Parser: reuses `gemini_cli::parser` (parse_session, gemini_messages_to_cc).
//! Dedup: `insert_conversation_messages_batch` uses ON CONFLICT DO NOTHING on message_uuid.

use std::path::Path;
use std::sync::Arc;

use tracing::{debug, info, warn};

use missiond_core::gemini_cli::{
    discover_chat_dirs, gemini_messages_to_cc, list_session_files, parse_session, ProjectMap,
};

use crate::events_sync;
use crate::state::AppState;

use super::{BackgroundWorker, WorkerContext, WorkerKind};

/// Watermark key prefix to distinguish from Claude Code watermarks.
const WATERMARK_PREFIX: &str = "gemini:";
/// Initial delay before first run (let system settle after startup).
const INITIAL_DELAY_SECS: u64 = 60;
/// Interval between runs.
const INTERVAL_SECS: u64 = 86400; // 24 hours
/// Flush batch to DB after this many messages.
const BATCH_FLUSH_SIZE: usize = 500;
/// Log progress every N files.
const PROGRESS_LOG_INTERVAL: usize = 200;
/// File not modified for this long → completed (1 hour).
const ACTIVE_THRESHOLD_SECS: u64 = 3600;

pub(crate) struct GeminiReconcileWorker;

impl BackgroundWorker for GeminiReconcileWorker {
    const KIND: WorkerKind = WorkerKind::Local;

    fn name(&self) -> &'static str {
        "gemini_reconcile"
    }

    async fn run(self, state: Arc<AppState>, mut ctx: WorkerContext) {
        tokio::time::sleep(std::time::Duration::from_secs(INITIAL_DELAY_SECS)).await;

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(INTERVAL_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            ctx.wait_if_paused().await;
            run_gemini_reconciliation(&state).await;
            ctx.record_success();
        }
    }
}

/// Run reconciliation immediately (callable from MCP tool).
pub(crate) async fn run_gemini_reconciliation_now(state: &AppState) {
    run_gemini_reconciliation(state).await;
}

async fn run_gemini_reconciliation(state: &AppState) {
    let gemini_home = match dirs::home_dir() {
        Some(h) => h.join(".gemini"),
        None => return,
    };
    if !gemini_home.join("tmp").exists() {
        debug!("Gemini reconcile: ~/.gemini/tmp not found, skipping");
        return;
    }

    info!("Gemini reconcile: starting scan");
    let start = chrono::Utc::now();

    // Load project map for directory → project path resolution
    let project_map = ProjectMap::load(&gemini_home).await;

    // Load all watermarks (prefix-filtered)
    let all_watermarks = match state.store.get_all_reconcile_watermarks().await {
        Ok(w) => w,
        Err(e) => {
            warn!(error = %e, "Gemini reconcile: failed to load watermarks");
            return;
        }
    };

    // Discover all chat directories
    let chat_dirs = discover_chat_dirs(&gemini_home).await;

    let mut total_files = 0usize;
    let mut files_reconciled = 0usize;
    let mut messages_recovered = 0usize;
    let mut files_skipped = 0usize;
    let mut files_processed = 0usize;

    for (dir_name, chats_dir) in &chat_dirs {
        let project_path = project_map
            .resolve_dir(dir_name)
            .unwrap_or_else(|| format!("gemini://{}", dir_name));

        let files = list_session_files(chats_dir).await;
        total_files += files.len();

        for file in &files {
            files_processed += 1;

            if files_processed % PROGRESS_LOG_INTERVAL == 0 {
                info!(
                    progress = format!("{}/{}", files_processed, total_files),
                    files_reconciled, messages_recovered, "Gemini reconcile: progress"
                );
            }

            let file_key = file.to_string_lossy().to_string();
            let watermark_key = format!("{}{}", WATERMARK_PREFIX, file_key);

            // Parse session to get current message count
            let session = match parse_session(file).await {
                Some(s) => s,
                None => continue,
            };

            let current_count = session.messages.len() as i64;
            let last_count = all_watermarks.get(&watermark_key).copied().unwrap_or(0);

            if current_count <= last_count {
                files_skipped += 1;
                continue;
            }

            // Convert new messages to CCMessageLine format
            let new_msgs = &session.messages[last_count as usize..];
            let cc_lines = gemini_messages_to_cc(new_msgs, &session, &project_path);

            if cc_lines.is_empty() {
                // Update watermark even if no convertible messages (e.g., all "info" type)
                let _ = state
                    .store
                    .upsert_reconcile_watermark(&watermark_key, current_count)
                    .await;
                files_skipped += 1;
                continue;
            }

            // Determine status from file modification time
            let status = infer_status(file).await;

            // Determine slot_id and conversation_type using the central logic
            let slot_id = state
                .store
                .get_slot_for_session(&session.session_id)
                .await
                .unwrap_or(None);
            let category = slot_id.as_deref().and_then(|id| state.mission.get_slot_category(id));
            let conv_type = missiond_core::db::derive_conversation_type(
                category.as_deref(),
                slot_id.as_deref(),
                &session.session_id,
                "gemini_cli",
            );

            // Ensure conversation exists (with correct source/chat_type for Gemini CLI)
            let first_ts = cc_lines.first().map(|m| m.timestamp.as_str());
            ensure_gemini_conversation(
                state,
                &session.session_id,
                &project_path,
                &file_key,
                &status,
                &conv_type,
                first_ts,
            )
            .await;

            // Build message batch (reuses the same pipeline as message_handler)
            let mut batch: Vec<missiond_core::types::ConversationMessage> = Vec::new();
            for msg in &cc_lines {
                let text_content = events_sync::extract_text_content(&msg.message.content);
                let content_types: Vec<&str> = msg
                    .message
                    .content
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|b| b.get("type").and_then(|t| t.as_str()))
                            .collect()
                    })
                    .unwrap_or_default();
                let is_tool_result =
                    !content_types.is_empty() && content_types.iter().all(|t| *t == "tool_result");
                let is_thinking =
                    !content_types.is_empty() && content_types.iter().all(|t| *t == "thinking");

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
                } else {
                    msg.message.role.clone()
                };

                let raw_content = events_sync::sanitize_raw_content(&msg.message.content);
                let tool_name = events_sync::extract_tool_names_csv(&msg.message.content);

                batch.push(missiond_core::types::ConversationMessage {
                    id: 0,
                    session_id: session.session_id.clone(),
                    raw_role: Some(msg.message.role.clone()),
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

                // Batch flush
                if batch.len() >= BATCH_FLUSH_SIZE {
                    batch.sort_by(|a, b| a.message_uuid.cmp(&b.message_uuid));
                    if let Ok(ids) = state.store.insert_conversation_messages_batch(&batch).await {
                        messages_recovered += ids.len();
                    }
                    batch.clear();
                    tokio::task::yield_now().await;
                }
            }

            // Flush remaining
            if !batch.is_empty() {
                batch.sort_by(|a, b| a.message_uuid.cmp(&b.message_uuid));
                if let Ok(ids) = state.store.insert_conversation_messages_batch(&batch).await {
                    messages_recovered += ids.len();
                }
            }

            // Update watermark
            if let Err(e) = state
                .store
                .upsert_reconcile_watermark(&watermark_key, current_count)
                .await
            {
                warn!(path = %file_key, error = %e, "Gemini reconcile: failed to update watermark");
            }

            // Refresh message count
            let _ = state
                .store
                .refresh_conversation_message_count(&session.session_id)
                .await;

            files_reconciled += 1;
            tokio::task::yield_now().await;
        }
    }

    let ts = start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    info!(
        total_files,
        files_skipped,
        files_reconciled,
        messages_recovered,
        completed_at = %ts,
        "Gemini reconcile: scan complete"
    );
}

/// Ensure a Gemini CLI conversation exists with correct source/chat_type.
async fn ensure_gemini_conversation(
    state: &AppState,
    session_id: &str,
    project_path: &str,
    jsonl_path: &str,
    status: &str,
    conversation_type: &str,
    first_timestamp: Option<&str>,
) {
    // Check if already exists
    if let Ok(Some(_)) = state.store.get_conversation(session_id).await {
        return;
    }

    let conv = missiond_core::types::Conversation {
        id: session_id.to_string(),
        project: Some(project_path.to_string()),
        project_id: None,
        slot_id: None,
        source: "gemini_cli".to_string(),
        model: None,
        git_branch: None,
        jsonl_path: Some(jsonl_path.to_string()),
        parent_session_id: None,
        task_id: None,
        message_count: 0,
        started_at: match first_timestamp.filter(|s| !s.is_empty()) {
            Some(ts) => ts.to_string(),
            None => chrono::Utc::now().to_rfc3339(),
        },
        ended_at: None,
        status: status.to_string(),
        analyzed_at: None,
        analysis_version: 0,
        analysis_retries: 0,
        deep_analyzed_message_id: 0,
        chat_type: Some("gemini_cli".to_string()),
        conversation_type: conversation_type.to_string(),
        updated_at: None,
        llm_summary: None,
        embedding_provider: None,
        session_timeline: None,
        timeline_built_at: None,
    };

    if let Err(e) = state.store.upsert_conversation(&conv).await {
        warn!(session = %session_id, error = %e, "Gemini reconcile: failed to create conversation");
    }
}

/// Infer conversation status from file modification time.
async fn infer_status(path: &Path) -> String {
    let modified = tokio::fs::metadata(path)
        .await
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs())
        .unwrap_or(u64::MAX);

    if modified > ACTIVE_THRESHOLD_SECS {
        "completed".to_string()
    } else {
        "active".to_string()
    }
}
