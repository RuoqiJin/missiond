//! Codex Ingestion Worker — background poller for OpenAI Codex operation logs.
//!
//! Polls `~/.codex/state_5.sqlite` threads table every 30s for new/updated threads.
//! Reads corresponding JSONL rollout files, extracts tool calls (function_call +
//! function_call_output pairs), and writes them to MissionD's conversations +
//! tool_calls tables via the Store trait.
//!
//! Design:
//! - Uses `conversations.id = codex_thread_id` directly (no ALTER TABLE).
//! - Real timestamps from JSONL, never NOW().
//! - Watermark tracking via in-memory + DB watermarks keyed by rollout file →
//!   (file mtime, file size, parsed line count).
//!   Codex's `threads.updated_at` only bumps on metadata changes (title, archive,
//!   first message), NOT on every JSONL append, so it's useless as a freshness signal.
//!   The rollout JSONL file's mtime+size is the only reliable "this thread changed" check.
//! - WorkerKind::Local (no LLM dependency — pure I/O + parsing).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};

use crate::state::AppState;

use super::{BackgroundWorker, WorkerContext, WorkerKind};

/// Poll interval between checks (seconds).
const POLL_INTERVAL_SECS: u64 = 10;

/// Initial delay before first run.
const STARTUP_DELAY_SECS: u64 = 15;

/// Codex SQLite database path relative to home.
const CODEX_DB_RELATIVE: &str = ".codex/state_5.sqlite";

/// Max JSONL lines to read per thread per poll cycle (safety valve).
const MAX_LINES_PER_THREAD: usize = 50_000;

/// Reparse overlap when a Codex rollout changed. Function call/output pairs can
/// straddle the previous cursor, and DB UUID dedup keeps this overlap safe.
const REPARSE_OVERLAP_LINES: i64 = 200;

/// Old unchanged files with an existing MissionD conversation are considered
/// already imported and can be bootstrapped into persistent watermarks.
const QUIET_FILE_THRESHOLD_SECS: u64 = 3600;

/// Batch size for tool call inserts.
const TOOL_CALL_BATCH_SIZE: usize = 100;

const CODEX_SIZE_WATERMARK_PREFIX: &str = "codex-size:";
const CODEX_MTIME_WATERMARK_PREFIX: &str = "codex-mtime:";
const CODEX_LINE_WATERMARK_PREFIX: &str = "codex-lines:";

// ── JSONL event types ──

#[derive(Debug, Deserialize)]
struct CodexJsonlEvent {
    timestamp: String,
    #[serde(rename = "type")]
    event_type: String,
    payload: Value,
}

/// File-based freshness watermark — what we last saw on disk for a thread's rollout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileWatermark {
    mtime_secs: i64,
    size: u64,
    age_secs: u64,
}

/// Minimal thread row from Codex's SQLite.
#[derive(Debug)]
struct CodexThread {
    id: String,
    rollout_path: String,
    created_at: i64,
    updated_at: i64,
    cwd: String,
    model: Option<String>,
    title: String,
    git_branch: Option<String>,
}

/// Parsed function_call from JSONL.
#[derive(Debug, Clone)]
struct ParsedToolCall {
    call_id: String,
    tool_name: String,
    arguments: String,
    timestamp: String,
    /// Output from matching function_call_output (if found).
    output: Option<String>,
    output_timestamp: Option<String>,
}

/// Parsed text message (agent or user) from JSONL.
#[derive(Debug, Clone)]
struct ParsedMessage {
    role: String, // "assistant" or "user"
    content: String,
    timestamp: String,
    line_no: usize,
    source_event_hash: String,
}

// ── Worker ──

pub(crate) struct CodexIngestionWorker;

impl BackgroundWorker for CodexIngestionWorker {
    const KIND: WorkerKind = WorkerKind::Local;

    fn name(&self) -> &'static str {
        "codex_ingestion"
    }

    async fn run(self, state: Arc<AppState>, mut ctx: WorkerContext) {
        let db_path = match codex_db_path() {
            Some(p) if p.exists() => p,
            _ => {
                info!("Codex ingestion: ~/.codex/state_5.sqlite not found, worker idle");
                // Park forever — no Codex installation detected
                std::future::pending::<()>().await;
                return;
            }
        };

        info!(
            db = %db_path.display(),
            "Codex ingestion worker started (poll interval: {POLL_INTERVAL_SECS}s)"
        );

        tokio::time::sleep(Duration::from_secs(STARTUP_DELAY_SECS)).await;

        // Watermark: thread_id → (file mtime, file size) of the rollout JSONL.
        // We deliberately ignore Codex's `threads.updated_at` because it does NOT
        // bump on every JSONL append. Persistent watermarks prevent blue-green
        // restarts from reparsing all historical Codex JSONL.
        let mut watermarks: HashMap<String, FileWatermark> = HashMap::new();

        loop {
            ctx.wait_if_paused().await;

            match poll_and_ingest(&state, &db_path, &mut watermarks).await {
                Ok(ingested) => {
                    if ingested > 0 {
                        ctx.record_success();
                        info!(tool_calls = ingested, "Codex ingestion: batch completed");
                    }
                }
                Err(e) => {
                    ctx.record_failure();
                    warn!(error = %e, "Codex ingestion: poll failed");
                }
            }

            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
        }
    }
}

// ── Core logic ──

fn codex_db_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(CODEX_DB_RELATIVE))
}

/// One poll cycle: read Codex SQLite → find updated threads → parse JSONL → write to MissionD.
async fn poll_and_ingest(
    state: &AppState,
    db_path: &Path,
    watermarks: &mut HashMap<String, FileWatermark>,
) -> Result<usize> {
    // Open Codex's SQLite in read-only mode (non-blocking for Codex).
    let db_path_owned = db_path.to_path_buf();
    let threads = tokio::task::spawn_blocking(move || read_codex_threads(&db_path_owned))
        .await
        .context("spawn_blocking join")??;

    let mut total_ingested = 0usize;
    let persisted_watermarks = state
        .store
        .get_all_reconcile_watermarks()
        .await
        .unwrap_or_default();

    for thread in &threads {
        // Read rollout file metadata — this is our authoritative freshness signal.
        let rollout_path = PathBuf::from(&thread.rollout_path);
        let current_wm = match read_file_watermark(&rollout_path) {
            Some(wm) => wm,
            None => continue, // file missing or unreadable — skip silently
        };

        // Skip if file hasn't grown or been touched since last poll.
        if let Some(prev) = watermarks.get(&thread.id).copied() {
            if same_file_watermark(current_wm, prev) {
                continue;
            }
        }
        if persisted_codex_watermark_matches(
            &persisted_watermarks,
            &thread.rollout_path,
            current_wm,
        ) {
            watermarks.insert(thread.id.clone(), current_wm);
            continue;
        }

        if current_wm.age_secs > QUIET_FILE_THRESHOLD_SECS
            && state
                .store
                .get_conversation(&thread.id)
                .await
                .ok()
                .flatten()
                .is_some()
            && !persisted_watermarks.contains_key(&codex_size_watermark_key(&thread.rollout_path))
        {
            persist_codex_file_watermarks(
                state,
                &thread.rollout_path,
                current_wm,
                persisted_watermarks
                    .get(&codex_line_watermark_key(&thread.rollout_path))
                    .copied(),
            )
            .await;
            watermarks.insert(thread.id.clone(), current_wm);
            continue;
        }

        let skip_before_line = persisted_watermarks
            .get(&codex_line_watermark_key(&thread.rollout_path))
            .copied()
            .unwrap_or(0)
            .saturating_sub(REPARSE_OVERLAP_LINES)
            .max(0) as usize;

        match process_thread(state, thread, skip_before_line).await {
            Ok(outcome) => {
                total_ingested += outcome.ingested;
                watermarks.insert(thread.id.clone(), current_wm);
                persist_codex_file_watermarks(
                    state,
                    &thread.rollout_path,
                    current_wm,
                    Some(outcome.total_lines as i64),
                )
                .await;
                if outcome.ingested > 0 {
                    debug!(
                        thread_id = %&thread.id[..8.min(thread.id.len())],
                        tool_calls = outcome.ingested,
                        size = current_wm.size,
                        "Codex ingestion: thread processed"
                    );
                }
            }
            Err(e) => {
                warn!(
                    thread_id = %&thread.id[..8.min(thread.id.len())],
                    error = %e,
                    "Codex ingestion: failed to process thread"
                );
                // Don't update watermark — retry next cycle.
            }
        }
    }

    Ok(total_ingested)
}

/// Snapshot a rollout file's freshness fingerprint. Returns None if the file
/// is missing or its metadata can't be read.
fn read_file_watermark(path: &Path) -> Option<FileWatermark> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    Some(FileWatermark {
        mtime_secs: mtime
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_secs()).ok())?,
        size: meta.len(),
        age_secs: mtime
            .elapsed()
            .ok()
            .map(|d| d.as_secs())
            .unwrap_or(u64::MAX),
    })
}

/// Read all threads from Codex SQLite (blocking — runs on spawn_blocking).
fn read_codex_threads(db_path: &Path) -> Result<Vec<CodexThread>> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("open codex sqlite")?;

    let mut stmt = conn.prepare(
        "SELECT id, rollout_path, created_at, updated_at, cwd, model, title, git_branch
         FROM threads
         WHERE archived = 0
         ORDER BY updated_at DESC
         LIMIT 200",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(CodexThread {
            id: row.get(0)?,
            rollout_path: row.get(1)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
            cwd: row.get(4)?,
            model: row.get(5)?,
            title: row.get(6)?,
            git_branch: row.get(7)?,
        })
    })?;

    let mut threads = Vec::new();
    for row in rows {
        threads.push(row?);
    }
    Ok(threads)
}

/// Process a single Codex thread: ensure conversation exists, parse JSONL, write tool calls.
struct ProcessedThread {
    ingested: usize,
    total_lines: usize,
}

async fn process_thread(
    state: &AppState,
    thread: &CodexThread,
    skip_before_line: usize,
) -> Result<ProcessedThread> {
    let rollout_path = PathBuf::from(&thread.rollout_path);
    if !rollout_path.exists() {
        debug!(
            thread_id = %&thread.id[..8.min(thread.id.len())],
            path = %rollout_path.display(),
            "Codex ingestion: JSONL file missing, skip"
        );
        return Ok(ProcessedThread {
            ingested: 0,
            total_lines: 0,
        });
    }

    // Ensure conversation record exists (idempotent via upsert).
    let started_at = epoch_to_iso(thread.created_at);
    let conv = missiond_core::types::Conversation {
        id: thread.id.clone(),
        project: Some(thread.cwd.clone()),
        project_id: {
            let registry = state.project_registry.read().await;
            registry.resolve(&thread.cwd).map(|s| s.to_string())
        },
        slot_id: None,
        source: "codex_cli".to_string(),
        model: thread.model.clone(),
        git_branch: thread.git_branch.clone(),
        jsonl_path: Some(thread.rollout_path.clone()),
        parent_session_id: None,
        task_id: None,
        message_count: 0,
        started_at,
        ended_at: None,
        status: "active".to_string(),
        analyzed_at: None,
        analysis_version: 0,
        analysis_retries: 0,
        deep_analyzed_message_id: 0,
        chat_type: Some("codex_cli".to_string()),
        conversation_type: "user".to_string(),
        updated_at: Some(epoch_to_iso(thread.updated_at)),
        llm_summary: None,
        embedding_provider: None,
        session_timeline: None,
        timeline_built_at: None,
    };
    if let Err(e) = state.store.upsert_conversation(&conv).await {
        warn!(
            thread_id = %&thread.id[..8.min(thread.id.len())],
            error = %e,
            "Codex ingestion: failed to upsert conversation"
        );
        // Non-fatal — tool calls can still be inserted if conversation already exists.
    }

    // Parse JSONL — extract tool calls + text messages.
    let parsed = tokio::task::spawn_blocking({
        let path = rollout_path.clone();
        let thread_id = thread.id.clone();
        move || parse_jsonl(&path, &thread_id, skip_before_line)
    })
    .await
    .context("spawn_blocking join")??;

    let mut total = 0usize;

    // Sanitize null bytes — Codex JSONL can contain 0x00 which PostgreSQL rejects.
    let sanitize = |s: &str| -> String { s.replace('\0', "") };

    // ── Insert tool calls ──
    if !parsed.tool_calls.is_empty() {
        let records: Vec<missiond_core::types::ToolCallRecord> = parsed
            .tool_calls
            .iter()
            .map(|tc| missiond_core::types::ToolCallRecord {
                id: tc.call_id.clone(),
                session_id: thread.id.clone(),
                message_id: None,
                tool_name: sanitize(&tc.tool_name),
                input_summary: Some(sanitize(&summarize_input(&tc.tool_name, &tc.arguments))),
                raw_input: Some(sanitize(&tc.arguments)),
                output_summary: tc.output.as_ref().map(|o| sanitize(&truncate(o, 500))),
                raw_output: tc.output.as_ref().map(|o| sanitize(o)),
                status: if tc.output.is_some() {
                    "success".to_string()
                } else {
                    "pending".to_string()
                },
                duration_ms: compute_duration_ms(&tc.timestamp, tc.output_timestamp.as_deref()),
                timestamp: tc.timestamp.clone(),
            })
            .collect();

        for chunk in records.chunks(TOOL_CALL_BATCH_SIZE) {
            match state.store.insert_tool_calls_batch(chunk).await {
                Ok(n) => total += n,
                Err(e) => {
                    warn!(
                        thread_id = %&thread.id[..8.min(thread.id.len())],
                        error = %e,
                        "Codex ingestion: tool call batch insert failed"
                    );
                }
            }
        }
    }

    // ── Insert text messages (agent + user) ──
    if !parsed.messages.is_empty() {
        let msgs: Vec<missiond_core::types::ConversationMessage> = parsed
            .messages
            .iter()
            .map(|m| missiond_core::types::ConversationMessage {
                id: 0, // auto-increment
                session_id: thread.id.clone(),
                role: m.role.clone(),
                content: sanitize(&m.content),
                raw_content: Some(
                    serde_json::json!({
                        "source": "codex_ingestion",
                        "jsonl_line": m.line_no,
                        "source_event_hash": m.source_event_hash,
                    })
                    .to_string(),
                ),
                message_uuid: Some(codex_message_uuid(&thread.id, m)),
                parent_uuid: None,
                model: thread.model.clone(),
                timestamp: m.timestamp.clone(),
                metadata: Some(r#"{"source":"codex_ingestion"}"#.to_string()),
                tool_name: None,
                raw_role: None,
                content_types: Some(r#"["text"]"#.to_string()),
                has_image: false,
                has_tool_use: false,
                has_tool_result: false,
                token_count: None,
                seq: None,
                role_display: None,
            })
            .collect();

        match state.store.insert_conversation_messages_batch(&msgs).await {
            Ok(ids) => {
                if !ids.is_empty() {
                    debug!(
                        thread_id = %&thread.id[..8.min(thread.id.len())],
                        count = ids.len(),
                        "Codex ingestion: messages inserted"
                    );
                }
                total += ids.len();
            }
            Err(e) => {
                warn!(
                    thread_id = %&thread.id[..8.min(thread.id.len())],
                    error = %e,
                    "Codex ingestion: message batch insert failed"
                );
            }
        }
    }

    Ok(ProcessedThread {
        ingested: total,
        total_lines: parsed.total_lines,
    })
}

/// Parse result: tool calls + text messages.
struct ParsedThread {
    tool_calls: Vec<ParsedToolCall>,
    messages: Vec<ParsedMessage>,
    total_lines: usize,
}

/// Parse JSONL file, extract tool calls + agent/user messages.
fn parse_jsonl(path: &Path, _thread_id: &str, skip_before_line: usize) -> Result<ParsedThread> {
    use std::io::{BufRead, BufReader};

    let file = std::fs::File::open(path).context("open JSONL")?;
    let reader = BufReader::new(file);

    let mut calls: Vec<ParsedToolCall> = Vec::new();
    let mut messages: Vec<ParsedMessage> = Vec::new();
    let mut call_id_to_idx: HashMap<String, usize> = HashMap::new();
    let mut line_count = 0usize;

    for line in reader.lines() {
        if line_count >= MAX_LINES_PER_THREAD {
            break;
        }
        line_count += 1;

        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        if line.is_empty() {
            continue;
        }
        if line_count <= skip_before_line {
            continue;
        }
        let source_event_hash = short_sha256(&line, 16);

        let event: CodexJsonlEvent = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        match event.event_type.as_str() {
            "response_item" => {
                let payload_type = event
                    .payload
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                match payload_type {
                    "function_call" => {
                        let name = event
                            .payload
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let arguments = event
                            .payload
                            .get("arguments")
                            .and_then(|v| v.as_str())
                            .unwrap_or("{}")
                            .to_string();
                        let call_id = event
                            .payload
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        if call_id.is_empty() {
                            continue;
                        }

                        let idx = calls.len();
                        calls.push(ParsedToolCall {
                            call_id: call_id.clone(),
                            tool_name: name,
                            arguments,
                            timestamp: event.timestamp,
                            output: None,
                            output_timestamp: None,
                        });
                        call_id_to_idx.insert(call_id, idx);
                    }
                    "function_call_output" => {
                        let call_id = event
                            .payload
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let output = event
                            .payload
                            .get("output")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        if let Some(&idx) = call_id_to_idx.get(call_id) {
                            if let Some(tc) = calls.get_mut(idx) {
                                tc.output = Some(output);
                                tc.output_timestamp = Some(event.timestamp.clone());
                            }
                        }
                    }
                    "message" => {
                        // Agent text message — extract from content array or direct string
                        let text = extract_message_text(&event.payload);
                        if !text.is_empty() {
                            messages.push(ParsedMessage {
                                role: "assistant".to_string(),
                                content: text,
                                timestamp: event.timestamp,
                                line_no: line_count,
                                source_event_hash: source_event_hash.clone(),
                            });
                        }
                    }
                    _ => {}
                }
            }
            "event_msg" => {
                let msg_type = event
                    .payload
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                match msg_type {
                    "user_message" => {
                        let text = event
                            .payload
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if !text.is_empty() {
                            messages.push(ParsedMessage {
                                role: "user".to_string(),
                                content: text,
                                timestamp: event.timestamp,
                                line_no: line_count,
                                source_event_hash: source_event_hash.clone(),
                            });
                        }
                    }
                    "agent_message" => {
                        let text = event
                            .payload
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if !text.is_empty() {
                            messages.push(ParsedMessage {
                                role: "assistant".to_string(),
                                content: text,
                                timestamp: event.timestamp,
                                line_no: line_count,
                                source_event_hash: source_event_hash.clone(),
                            });
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    Ok(ParsedThread {
        tool_calls: calls,
        messages,
        total_lines: line_count,
    })
}

fn same_file_watermark(a: FileWatermark, b: FileWatermark) -> bool {
    a.size == b.size && a.mtime_secs == b.mtime_secs
}

fn codex_size_watermark_key(path: &str) -> String {
    format!("{CODEX_SIZE_WATERMARK_PREFIX}{path}")
}

fn codex_mtime_watermark_key(path: &str) -> String {
    format!("{CODEX_MTIME_WATERMARK_PREFIX}{path}")
}

fn codex_line_watermark_key(path: &str) -> String {
    format!("{CODEX_LINE_WATERMARK_PREFIX}{path}")
}

fn persisted_codex_watermark_matches(
    watermarks: &HashMap<String, i64>,
    path: &str,
    current: FileWatermark,
) -> bool {
    watermarks.get(&codex_size_watermark_key(path)).copied() == i64::try_from(current.size).ok()
        && watermarks.get(&codex_mtime_watermark_key(path)).copied() == Some(current.mtime_secs)
}

async fn persist_codex_file_watermarks(
    state: &AppState,
    path: &str,
    current: FileWatermark,
    total_lines: Option<i64>,
) {
    if let Ok(size) = i64::try_from(current.size) {
        let _ = state
            .store
            .upsert_reconcile_watermark(&codex_size_watermark_key(path), size)
            .await;
    }
    let _ = state
        .store
        .upsert_reconcile_watermark(&codex_mtime_watermark_key(path), current.mtime_secs)
        .await;
    if let Some(total_lines) = total_lines {
        let _ = state
            .store
            .upsert_reconcile_watermark(&codex_line_watermark_key(path), total_lines)
            .await;
    }
}

fn short_sha256(input: &str, chars: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    digest.chars().take(chars).collect()
}

fn codex_message_uuid(thread_id: &str, message: &ParsedMessage) -> String {
    format!(
        "codex-cli:{thread_id}:line-{}:{}:{}",
        message.line_no, message.role, message.source_event_hash
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_message_uuid_is_non_null_and_stable() {
        let message = ParsedMessage {
            role: "assistant".to_string(),
            content: "done".to_string(),
            timestamp: "2026-05-03T00:00:00Z".to_string(),
            line_no: 42,
            source_event_hash: short_sha256(r#"{"type":"response_item"}"#, 16),
        };
        let first = codex_message_uuid("thread-123", &message);
        let second = codex_message_uuid("thread-123", &message);
        assert_eq!(first, second);
        assert!(first.starts_with("codex-cli:thread-123:line-42:assistant:"));
        assert!(!first.ends_with(':'));
    }
}

/// Extract text content from a response_item/message payload.
fn extract_message_text(payload: &Value) -> String {
    // Try direct string first (some messages have text directly)
    if let Some(text) = payload.as_str() {
        return text.to_string();
    }

    // Try content array: [{"type": "output_text", "text": "..."}, ...]
    if let Some(content) = payload.get("content").and_then(|v| v.as_array()) {
        let mut parts = Vec::new();
        for item in content {
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                parts.push(text);
            }
        }
        if !parts.is_empty() {
            return parts.join("");
        }
    }

    // Try top-level text field
    if let Some(text) = payload.get("text").and_then(|v| v.as_str()) {
        return text.to_string();
    }

    String::new()
}

// ── Helpers ──

/// Convert epoch seconds to ISO 8601 string.
fn epoch_to_iso(epoch_secs: i64) -> String {
    chrono::DateTime::from_timestamp(epoch_secs, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Summarize tool call input for display (first 200 chars of key field).
fn summarize_input(tool_name: &str, arguments: &str) -> String {
    // Try to extract the most meaningful field from arguments JSON.
    if let Ok(args) = serde_json::from_str::<Value>(arguments) {
        let key_field = match tool_name {
            "exec_command" => args.get("cmd").and_then(|v| v.as_str()),
            "read_file" | "write_file" => args.get("path").and_then(|v| v.as_str()),
            _ => args
                .get("cmd")
                .or_else(|| args.get("path"))
                .or_else(|| args.get("text"))
                .and_then(|v| v.as_str()),
        };
        if let Some(field) = key_field {
            return truncate(field, 200);
        }
    }
    truncate(arguments, 200)
}

/// Truncate string to max chars.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{}...", truncated)
    }
}

/// Compute duration between two ISO timestamps in milliseconds.
fn compute_duration_ms(start: &str, end: Option<&str>) -> Option<i64> {
    let end = end?;
    let start_dt = chrono::DateTime::parse_from_rfc3339(start).ok()?;
    let end_dt = chrono::DateTime::parse_from_rfc3339(end).ok()?;
    let duration = end_dt.signed_duration_since(start_dt);
    Some(duration.num_milliseconds())
}
