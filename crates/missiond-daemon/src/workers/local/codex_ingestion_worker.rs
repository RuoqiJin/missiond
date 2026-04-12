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
//! - Watermark tracking via in-memory HashMap keyed by thread_id → (file mtime, file size).
//!   Codex's `threads.updated_at` only bumps on metadata changes (title, archive,
//!   first message), NOT on every JSONL append, so it's useless as a freshness signal.
//!   The rollout JSONL file's mtime+size is the only reliable "this thread changed" check.
//! - WorkerKind::Local (no LLM dependency — pure I/O + parsing).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::state::AppState;

use super::{BackgroundWorker, WorkerContext, WorkerKind};

/// Poll interval between checks (seconds).
const POLL_INTERVAL_SECS: u64 = 30;

/// Initial delay before first run.
const STARTUP_DELAY_SECS: u64 = 15;

/// Codex SQLite database path relative to home.
const CODEX_DB_RELATIVE: &str = ".codex/state_5.sqlite";

/// Max JSONL lines to read per thread per poll cycle (safety valve).
const MAX_LINES_PER_THREAD: usize = 50_000;

/// Batch size for tool call inserts.
const TOOL_CALL_BATCH_SIZE: usize = 100;

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
    mtime: SystemTime,
    size: u64,
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
        // bump on every JSONL append. In-memory only — on daemon restart we re-scan
        // and INSERT OR IGNORE dedup catches anything we already have.
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

    for thread in &threads {
        // Read rollout file metadata — this is our authoritative freshness signal.
        let rollout_path = PathBuf::from(&thread.rollout_path);
        let current_wm = match read_file_watermark(&rollout_path) {
            Some(wm) => wm,
            None => continue, // file missing or unreadable — skip silently
        };

        // Skip if file hasn't grown or been touched since last poll.
        if let Some(prev) = watermarks.get(&thread.id).copied() {
            if current_wm == prev {
                continue;
            }
        }

        match process_thread(state, thread).await {
            Ok(count) => {
                total_ingested += count;
                watermarks.insert(thread.id.clone(), current_wm);
                if count > 0 {
                    debug!(
                        thread_id = %&thread.id[..8.min(thread.id.len())],
                        tool_calls = count,
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
        mtime,
        size: meta.len(),
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
async fn process_thread(state: &AppState, thread: &CodexThread) -> Result<usize> {
    let rollout_path = PathBuf::from(&thread.rollout_path);
    if !rollout_path.exists() {
        debug!(
            thread_id = %&thread.id[..8.min(thread.id.len())],
            path = %rollout_path.display(),
            "Codex ingestion: JSONL file missing, skip"
        );
        return Ok(0);
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

    // Parse JSONL and extract tool calls.
    let tool_calls = tokio::task::spawn_blocking({
        let path = rollout_path.clone();
        let thread_id = thread.id.clone();
        move || parse_jsonl_tool_calls(&path, &thread_id)
    })
    .await
    .context("spawn_blocking join")??;

    if tool_calls.is_empty() {
        return Ok(0);
    }

    // Convert to ToolCallRecord and batch insert.
    // Sanitize null bytes — Codex JSONL can contain 0x00 which PostgreSQL rejects.
    let sanitize = |s: &str| -> String { s.replace('\0', "") };
    let records: Vec<missiond_core::types::ToolCallRecord> = tool_calls
        .iter()
        .map(|tc| missiond_core::types::ToolCallRecord {
            id: tc.call_id.clone(),
            session_id: thread.id.clone(),
            message_id: None, // No FK to conversation_messages (Codex messages not ingested line-by-line)
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

    // Batch insert (INSERT OR IGNORE dedup by call_id PK).
    let mut total = 0usize;
    for chunk in records.chunks(TOOL_CALL_BATCH_SIZE) {
        match state.store.insert_tool_calls_batch(chunk).await {
            Ok(n) => total += n,
            Err(e) => {
                warn!(
                    thread_id = %&thread.id[..8.min(thread.id.len())],
                    error = %e,
                    "Codex ingestion: batch insert failed"
                );
            }
        }
    }

    Ok(total)
}

/// Parse JSONL file, pair function_call + function_call_output entries.
fn parse_jsonl_tool_calls(path: &Path, _thread_id: &str) -> Result<Vec<ParsedToolCall>> {
    use std::io::{BufRead, BufReader};

    let file = std::fs::File::open(path).context("open JSONL")?;
    let reader = BufReader::new(file);

    // Collect all function_call entries, then match with function_call_output.
    let mut calls: Vec<ParsedToolCall> = Vec::new();
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

        let event: CodexJsonlEvent = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        if event.event_type != "response_item" {
            continue;
        }

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
                        tc.output_timestamp = Some(event.timestamp);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(calls)
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
