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

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{Context, Result};
use chrono::DateTime;
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

/// Codex rollout corpus roots relative to home.
const CODEX_SESSIONS_RELATIVE: &str = ".codex/sessions";
const CODEX_ARCHIVED_SESSIONS_RELATIVE: &str = ".codex/archived_sessions";

/// Max JSONL lines to read per thread per poll cycle (safety valve).
const MAX_LINES_PER_THREAD: usize = 50_000;

/// Reparse overlap when a Codex rollout changed. Function call/output pairs can
/// straddle the previous cursor, and DB UUID dedup keeps this overlap safe.
const REPARSE_OVERLAP_LINES: i64 = 200;

/// Batch size for tool call inserts.
const TOOL_CALL_BATCH_SIZE: usize = 100;

const CODEX_SIZE_WATERMARK_PREFIX: &str = "codex-size:";
const CODEX_MTIME_WATERMARK_PREFIX: &str = "codex-mtime:";
const CODEX_LINE_WATERMARK_PREFIX: &str = "codex-lines:";
const CODEX_COMPLETE_WATERMARK_PREFIX: &str = "codex-complete:";

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
    archived: bool,
    provider_indexed: bool,
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
    let threads =
        tokio::task::spawn_blocking(move || read_codex_threads_with_raw_rollouts(&db_path_owned))
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
            None => {
                record_codex_source_state(state, thread, "missing-stale", None, None).await;
                continue;
            }
        };

        // Codex thread metadata (archive state, model, cwd, branch) lives in
        // SQLite and can change even when the rollout JSONL is unchanged. Keep
        // that source-state synced before any watermark shortcut skips parsing.
        sync_codex_thread_metadata_if_needed(state, thread).await;

        // Skip if file hasn't grown or been touched since last poll.
        if let Some(prev) = watermarks.get(&thread.id).copied() {
            if same_file_watermark(current_wm, prev) {
                record_codex_source_state(
                    state,
                    thread,
                    codex_source_state(thread),
                    Some(current_wm),
                    None,
                )
                .await;
                continue;
            }
        }
        if persisted_codex_watermark_matches(
            &persisted_watermarks,
            &thread.rollout_path,
            current_wm,
        ) {
            watermarks.insert(thread.id.clone(), current_wm);
            record_codex_source_state(
                state,
                thread,
                codex_source_state(thread),
                Some(current_wm),
                None,
            )
            .await;
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
                let complete_for_watermark =
                    !outcome.reached_line_limit && !outcome.had_insert_error;
                persist_codex_file_watermarks(
                    state,
                    &thread.rollout_path,
                    current_wm,
                    Some(outcome.total_lines as i64),
                    complete_for_watermark,
                )
                .await;
                if complete_for_watermark {
                    watermarks.insert(thread.id.clone(), current_wm);
                }
                record_codex_source_state(
                    state,
                    thread,
                    codex_source_state(thread),
                    Some(current_wm),
                    Some(&outcome),
                )
                .await;
                if outcome.ingested > 0 {
                    debug!(
                        thread_id = %&thread.id[..8.min(thread.id.len())],
                        tool_calls = outcome.ingested,
                        size = current_wm.size,
                        reached_line_limit = outcome.reached_line_limit,
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

/// Read all Codex sqlite threads plus durable rollout JSONL files that Codex
/// sqlite no longer references. The raw rollout file's `session_meta.payload.id`
/// remains the canonical conversation id.
fn read_codex_threads_with_raw_rollouts(db_path: &Path) -> Result<Vec<CodexThread>> {
    let mut threads = read_codex_threads(db_path)?;
    let indexed_paths: HashSet<String> = threads.iter().map(|t| t.rollout_path.clone()).collect();
    let raw_threads = discover_raw_codex_threads(&indexed_paths)?;
    threads.extend(raw_threads);
    Ok(threads)
}

/// Read all threads from Codex SQLite (blocking — runs on spawn_blocking).
fn read_codex_threads(db_path: &Path) -> Result<Vec<CodexThread>> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("open codex sqlite")?;

    let mut stmt = conn.prepare(CODEX_THREADS_QUERY)?;

    let rows = stmt.query_map([], |row| {
        Ok(CodexThread {
            id: row.get(0)?,
            rollout_path: row.get(1)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
            archived: row.get::<_, i64>(4)? != 0,
            provider_indexed: true,
            cwd: row.get(5)?,
            model: row.get(6)?,
            title: row.get(7)?,
            git_branch: row.get(8)?,
        })
    })?;

    let mut threads = Vec::new();
    for row in rows {
        threads.push(row?);
    }
    Ok(threads)
}

fn discover_raw_codex_threads(indexed_paths: &HashSet<String>) -> Result<Vec<CodexThread>> {
    let mut files = Vec::new();
    if let Some(home) = dirs::home_dir() {
        collect_jsonl_files(&home.join(CODEX_SESSIONS_RELATIVE), &mut files);
        collect_jsonl_files(&home.join(CODEX_ARCHIVED_SESSIONS_RELATIVE), &mut files);
    }

    let mut threads = Vec::new();
    let mut seen_ids = HashSet::new();
    for path in files {
        let path_string = path.to_string_lossy().to_string();
        if indexed_paths.contains(&path_string) {
            continue;
        }
        let archived = path_string.contains("/.codex/archived_sessions/");
        match read_raw_codex_thread_meta(&path, archived) {
            Ok(Some(thread)) => {
                if seen_ids.insert(thread.id.clone()) {
                    threads.push(thread);
                }
            }
            Ok(None) => {}
            Err(e) => {
                debug!(path = %path.display(), error = %e, "Codex ingestion: raw rollout meta skipped");
            }
        }
    }
    Ok(threads)
}

fn collect_jsonl_files(root: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

fn read_raw_codex_thread_meta(path: &Path, archived: bool) -> Result<Option<CodexThread>> {
    use std::io::{BufRead, BufReader};

    let file = std::fs::File::open(path).context("open raw Codex rollout")?;
    let reader = BufReader::new(file);

    for line in reader.lines().take(25) {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let event: CodexJsonlEvent = match serde_json::from_str(&line) {
            Ok(event) => event,
            Err(_) => continue,
        };
        if event.event_type != "session_meta" {
            continue;
        }

        let id = event
            .payload
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            return Ok(None);
        }

        let meta = std::fs::metadata(path).ok();
        let updated_at = meta
            .and_then(|m| m.modified().ok())
            .and_then(|mtime| mtime.duration_since(UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_secs()).ok())
            .unwrap_or_else(|| parse_rfc3339_epoch(&event.timestamp).unwrap_or(0));
        let created_at = event
            .payload
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(parse_rfc3339_epoch)
            .or_else(|| parse_rfc3339_epoch(&event.timestamp))
            .unwrap_or(updated_at);
        let cwd = event
            .payload
            .get("cwd")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let model = event
            .payload
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&id)
            .to_string();

        return Ok(Some(CodexThread {
            id,
            rollout_path: path.to_string_lossy().to_string(),
            created_at,
            updated_at,
            archived,
            provider_indexed: false,
            cwd,
            model,
            title,
            git_branch: None,
        }));
    }

    Ok(None)
}

const CODEX_THREADS_QUERY: &str = "\
SELECT id, rollout_path, created_at, updated_at, archived, cwd, model, title, git_branch
FROM threads
ORDER BY updated_at DESC";

/// Process a single Codex thread: ensure conversation exists, parse JSONL, write tool calls.
struct ProcessedThread {
    ingested: usize,
    total_lines: usize,
    message_line_count: usize,
    first_timestamp: Option<String>,
    last_timestamp: Option<String>,
    reached_line_limit: bool,
    had_insert_error: bool,
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
            message_line_count: 0,
            first_timestamp: None,
            last_timestamp: None,
            reached_line_limit: false,
            had_insert_error: false,
        });
    }

    // Ensure conversation record exists (idempotent via upsert). Preserve the
    // existing count during replay so large, chunked rollouts do not regress the
    // list view to zero before the final refresh runs.
    let existing = state
        .store
        .get_conversation(&thread.id)
        .await
        .ok()
        .flatten();
    let conv = build_codex_conversation(
        state,
        thread,
        existing.as_ref().map(|c| c.message_count).unwrap_or(0),
        existing.and_then(|c| c.ended_at),
    )
    .await;
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
    let mut had_insert_error = false;

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
                    had_insert_error = true;
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
    //
    // BoardTask e1a5ac1f :: preserve provider metadata. The Codex JSONL
    // events expose the raw provider role via `event_msg.user_message`
    // / `event_msg.agent_message` (see `parse_jsonl`). Forward that
    // string into `raw_role` so the historical-row audit
    // (`audit_classification`) and the role-attribution report can
    // reason about the provider-side turn segmentation without having
    // to re-parse JSONL. Worker-class turn semantics (raw user input
    // that should not collapse into the human Logs tab) are then
    // applied at query-time via `classify_conversation_type` above.
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
                raw_role: Some(m.role.clone()),
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
                had_insert_error = true;
                warn!(
                    thread_id = %&thread.id[..8.min(thread.id.len())],
                    error = %e,
                    "Codex ingestion: message batch insert failed"
                );
            }
        }
    }

    if let Err(e) = state
        .store
        .refresh_conversation_message_count(&thread.id)
        .await
    {
        warn!(
            thread_id = %&thread.id[..8.min(thread.id.len())],
            error = %e,
            "Codex ingestion: failed to refresh conversation message_count"
        );
    }

    Ok(ProcessedThread {
        ingested: total,
        total_lines: parsed.total_lines,
        message_line_count: parsed.message_line_count,
        first_timestamp: parsed.first_timestamp,
        last_timestamp: parsed.last_timestamp,
        reached_line_limit: parsed.reached_line_limit,
        had_insert_error,
    })
}

async fn record_codex_source_state(
    state: &AppState,
    thread: &CodexThread,
    raw_state: &str,
    current_wm: Option<FileWatermark>,
    outcome: Option<&ProcessedThread>,
) {
    let input = missiond_core::types::ConversationSourceStateInput {
        conversation_id: thread.id.clone(),
        source: "codex_cli".to_string(),
        raw_path: Some(thread.rollout_path.clone()),
        raw_state: raw_state.to_string(),
        raw_first_seen_at: outcome.and_then(|o| o.first_timestamp.clone()),
        raw_last_seen_at: outcome.and_then(|o| o.last_timestamp.clone()),
        raw_line_count: outcome.map(|o| o.total_lines as i64),
        raw_message_line_count: outcome.map(|o| o.message_line_count as i64),
        raw_hash: current_wm.map(|wm| codex_file_fingerprint(wm)),
        reason: Some(codex_source_state_reason(thread, raw_state).to_string()),
    };
    if let Err(e) = state.store.upsert_conversation_source_state(&input).await {
        debug!(
            thread_id = %&thread.id[..8.min(thread.id.len())],
            raw_state,
            error = %e,
            "Codex ingestion: failed to upsert conversation_source_state"
        );
    }
}

fn codex_source_state(thread: &CodexThread) -> &'static str {
    if !thread.provider_indexed {
        "sqlite-missing"
    } else if thread.archived {
        "archived"
    } else {
        "current"
    }
}

fn codex_source_state_reason(thread: &CodexThread, raw_state: &str) -> &'static str {
    match raw_state {
        "sqlite-missing" => {
            "rollout JSONL has session_meta but Codex state_5.sqlite has no matching thread row"
        }
        "missing-stale" => "Codex state_5.sqlite references a rollout path that is missing on disk",
        "archived" => "Codex sqlite marks this thread archived; keep as historical source state",
        _ if thread.provider_indexed => "Codex sqlite thread and rollout JSONL are both visible",
        _ => "Codex rollout source state recorded by background ingestion",
    }
}

async fn sync_codex_thread_metadata_if_needed(state: &AppState, thread: &CodexThread) {
    let existing = match state.store.get_conversation(&thread.id).await {
        Ok(Some(conv)) => conv,
        _ => return,
    };

    let conv = build_codex_conversation(
        state,
        thread,
        existing.message_count,
        existing.ended_at.clone(),
    )
    .await;
    if existing.status == conv.status {
        return;
    }
    if let Err(e) = state.store.upsert_conversation(&conv).await {
        warn!(
            thread_id = %&thread.id[..8.min(thread.id.len())],
            error = %e,
            status = %conv.status,
            "Codex ingestion: failed to sync thread metadata"
        );
    }
}

async fn build_codex_conversation(
    state: &AppState,
    thread: &CodexThread,
    message_count: i64,
    ended_at: Option<String>,
) -> missiond_core::types::Conversation {
    // BoardTask e1a5ac1f :: provider-aware classification.
    //
    // Resolve any slot binding for this Codex thread BEFORE building the
    // Conversation row so the provider-aware classifier has a real
    // signal. Background-ingested Codex threads with no MissionD slot
    // get `conversation_type=codex_chat` (parallel to gemini_chat) and
    // a Codex thread riding a slot gets `conversation_type=worker` with
    // durable slot/task linkage.
    //
    // The legacy hardcoded `conversation_type: "user"` was the original
    // misclassification that landed Codex traffic in the human Logs
    // tab; the dry-run audit in
    // `db::conversation_query::audit_classification` flags any
    // historical row that still matches that pattern so we can clean
    // them up via the existing reconcile path rather than bulk DB
    // mutation.
    let slot_id = state
        .store
        .get_slot_for_session(&thread.id)
        .await
        .unwrap_or(None);
    let slot_category = slot_id
        .as_deref()
        .and_then(|id| state.mission.get_slot_category(id));
    let conversation_type = missiond_core::db::conversation_query::classify_conversation_type(
        slot_category.as_deref(),
        slot_id.as_deref(),
        &thread.id,
        "codex_cli",
    );
    // Durable task linkage: when the slot has a running task, persist
    // its id on the conversation row so worker chains stay queryable
    // by `task_id` without relying on the in-memory
    // `session_task_bindings` map.
    let task_id = match slot_id.as_deref() {
        Some(sid) => state.store.get_running_slot_task(sid).await.ok().flatten(),
        None => None,
    };
    let rollout_age_secs =
        read_file_watermark(Path::new(&thread.rollout_path)).map(|wm| wm.age_secs);
    let status = codex_thread_status(thread.archived, slot_id.is_some(), rollout_age_secs);

    missiond_core::types::Conversation {
        id: thread.id.clone(),
        project: Some(thread.cwd.clone()),
        project_id: {
            let registry = state.project_registry.read().await;
            registry.resolve(&thread.cwd).map(|s| s.to_string())
        },
        slot_id: slot_id.clone(),
        source: "codex_cli".to_string(),
        model: thread.model.clone(),
        git_branch: thread.git_branch.clone(),
        jsonl_path: Some(thread.rollout_path.clone()),
        parent_session_id: None,
        task_id,
        message_count,
        started_at: epoch_to_iso(thread.created_at),
        ended_at,
        status: status.to_string(),
        analyzed_at: None,
        analysis_version: 0,
        analysis_retries: 0,
        deep_analyzed_message_id: 0,
        chat_type: Some("codex_cli".to_string()),
        conversation_type,
        updated_at: Some(epoch_to_iso(thread.updated_at)),
        llm_summary: None,
        embedding_provider: None,
        session_timeline: None,
        timeline_built_at: None,
    }
}

fn codex_thread_status(
    archived: bool,
    slot_bound: bool,
    rollout_age_secs: Option<u64>,
) -> &'static str {
    if archived {
        "archived"
    } else if slot_bound {
        "active"
    } else if rollout_age_secs.unwrap_or(u64::MAX) > 3600 {
        "completed"
    } else {
        "active"
    }
}

/// Parse result: tool calls + text messages.
struct ParsedThread {
    tool_calls: Vec<ParsedToolCall>,
    messages: Vec<ParsedMessage>,
    total_lines: usize,
    message_line_count: usize,
    first_timestamp: Option<String>,
    last_timestamp: Option<String>,
    reached_line_limit: bool,
}

/// Parse JSONL file, extract tool calls + agent/user messages.
fn parse_jsonl(path: &Path, _thread_id: &str, skip_before_line: usize) -> Result<ParsedThread> {
    use std::io::{BufRead, BufReader};

    let file = std::fs::File::open(path).context("open JSONL")?;
    let reader = BufReader::new(file);

    let mut calls: Vec<ParsedToolCall> = Vec::new();
    let mut messages: Vec<ParsedMessage> = Vec::new();
    let mut call_id_to_idx: HashMap<String, usize> = HashMap::new();
    let mut physical_line_no = 0usize;
    let mut processed_lines = 0usize;
    let mut cursor_line = skip_before_line;
    let mut message_line_count = 0usize;
    let mut first_timestamp: Option<String> = None;
    let mut last_timestamp: Option<String> = None;
    let mut reached_line_limit = false;

    for line in reader.lines() {
        physical_line_no += 1;
        if physical_line_no <= skip_before_line {
            continue;
        }
        if processed_lines >= MAX_LINES_PER_THREAD {
            reached_line_limit = true;
            break;
        }
        processed_lines += 1;
        cursor_line = physical_line_no;

        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        if line.is_empty() {
            continue;
        }
        let source_event_hash = short_sha256(&line, 16);

        let event: CodexJsonlEvent = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if first_timestamp.is_none() {
            first_timestamp = Some(event.timestamp.clone());
        }
        last_timestamp = Some(event.timestamp.clone());

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
                                line_no: physical_line_no,
                                source_event_hash: source_event_hash.clone(),
                            });
                            message_line_count += 1;
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
                        let text = extract_event_msg_user_text(&event.payload);
                        if !text.is_empty() {
                            messages.push(ParsedMessage {
                                role: "user".to_string(),
                                content: text,
                                timestamp: event.timestamp,
                                line_no: physical_line_no,
                                source_event_hash: source_event_hash.clone(),
                            });
                            message_line_count += 1;
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
                                line_no: physical_line_no,
                                source_event_hash: source_event_hash.clone(),
                            });
                            message_line_count += 1;
                        }
                    }
                    "task_complete" => {
                        let text = event
                            .payload
                            .get("last_agent_message")
                            .and_then(|v| v.as_str())
                            .or_else(|| event.payload.get("message").and_then(|v| v.as_str()))
                            .unwrap_or("")
                            .to_string();
                        if !text.is_empty() {
                            messages.push(ParsedMessage {
                                role: "assistant".to_string(),
                                content: text,
                                timestamp: event.timestamp,
                                line_no: physical_line_no,
                                source_event_hash: source_event_hash.clone(),
                            });
                            message_line_count += 1;
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
        total_lines: cursor_line,
        message_line_count,
        first_timestamp,
        last_timestamp,
        reached_line_limit,
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

fn codex_complete_watermark_key(path: &str) -> String {
    format!("{CODEX_COMPLETE_WATERMARK_PREFIX}{path}")
}

fn persisted_codex_watermark_matches(
    watermarks: &HashMap<String, i64>,
    path: &str,
    current: FileWatermark,
) -> bool {
    watermarks.get(&codex_size_watermark_key(path)).copied() == i64::try_from(current.size).ok()
        && watermarks.get(&codex_mtime_watermark_key(path)).copied() == Some(current.mtime_secs)
        && watermarks.get(&codex_complete_watermark_key(path)).copied() == Some(1)
}

async fn persist_codex_file_watermarks(
    state: &AppState,
    path: &str,
    current: FileWatermark,
    total_lines: Option<i64>,
    complete: bool,
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
    let _ = state
        .store
        .upsert_reconcile_watermark(
            &codex_complete_watermark_key(path),
            if complete { 1 } else { 0 },
        )
        .await;
}

fn codex_file_fingerprint(wm: FileWatermark) -> String {
    format!("size:{}:mtime:{}", wm.size, wm.mtime_secs)
}

fn parse_rfc3339_epoch(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.timestamp())
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
    use missiond_core::db::conversation_query::{
        audit_classification, classify_conversation_type, ClassificationAuditInput,
    };

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

    // ── BoardTask e1a5ac1f :: Codex worker classification ──────────────
    //
    // The codex worker historically hardcoded `conversation_type=user`
    // and dropped `raw_role`. The pure-helper checks below pin the new
    // contract so a regression cannot silently land Codex traffic in
    // the human Logs tab again.

    #[test]
    fn codex_background_thread_classifies_as_codex_chat_not_user() {
        let v = classify_conversation_type(None, None, "codex-thread-uuid", "codex_cli");
        assert_eq!(
            v, "codex_chat",
            "background-ingested Codex threads must surface under the Codex tab, \
             not the human user Logs view"
        );
    }

    #[test]
    fn codex_thread_query_imports_full_history_including_archived() {
        assert!(
            !CODEX_THREADS_QUERY.contains("LIMIT 200"),
            "Codex ingestion must not stop at recent unarchived threads"
        );
        assert!(
            !CODEX_THREADS_QUERY.contains("archived = 0"),
            "archived Codex threads are historical source state and must still be imported"
        );
        assert!(CODEX_THREADS_QUERY.contains("archived"));
    }

    #[test]
    fn raw_rollout_meta_imports_session_missing_from_sqlite() {
        let path = std::env::temp_dir().join(format!(
            "missiond-codex-rollout-test-{}-{}.jsonl",
            std::process::id(),
            short_sha256("raw-rollout-meta", 8)
        ));
        let jsonl = r#"{"timestamp":"2026-05-10T00:00:00Z","type":"session_meta","payload":{"id":"raw-thread-123","timestamp":"2026-05-10T00:00:00Z","cwd":"/tmp/project","model":"gpt-5.5"}}"#;
        std::fs::write(&path, format!("{jsonl}\n")).expect("write raw rollout fixture");

        let thread = read_raw_codex_thread_meta(&path, false)
            .expect("read raw rollout meta")
            .expect("session_meta should produce a thread");
        assert_eq!(thread.id, "raw-thread-123");
        assert_eq!(thread.cwd, "/tmp/project");
        assert_eq!(thread.model.as_deref(), Some("gpt-5.5"));
        assert!(!thread.provider_indexed);
        assert_eq!(codex_source_state(&thread), "sqlite-missing");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn parse_jsonl_imports_task_complete_last_agent_message_as_final_assistant() {
        let path = std::env::temp_dir().join(format!(
            "missiond-codex-task-complete-test-{}-{}.jsonl",
            std::process::id(),
            short_sha256("task-complete", 8)
        ));
        let task_complete = serde_json::json!({
            "timestamp": "2026-05-24T12:17:11.949Z",
            "type": "event_msg",
            "payload": {
                "type": "task_complete",
                "last_agent_message": "## Findings\n- done\n\n## Evidence\n- rollout\n\n## Recommendations\n- keep codex worker lane\n\n## Verification\n- no edits"
            }
        });
        std::fs::write(&path, format!("{task_complete}\n")).expect("write codex fixture");

        let parsed = parse_jsonl(&path, "thread-task-complete", 0).expect("parse jsonl");
        assert_eq!(parsed.messages.len(), 1);
        let message = &parsed.messages[0];
        assert_eq!(message.role, "assistant");
        assert_eq!(message.line_no, 1);
        assert!(message.content.contains("## Findings"));
        assert!(message.content.contains("## Verification"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn codex_status_distinguishes_archived_slot_and_historical_threads() {
        assert_eq!(codex_thread_status(true, false, Some(1)), "archived");
        assert_eq!(codex_thread_status(false, true, Some(86_400)), "active");
        assert_eq!(codex_thread_status(false, false, Some(30)), "active");
        assert_eq!(codex_thread_status(false, false, Some(3_601)), "completed");
        assert_eq!(codex_thread_status(false, false, None), "completed");
    }

    #[test]
    fn persisted_codex_watermark_requires_complete_marker() {
        let path = "/tmp/codex-rollout.jsonl";
        let wm = FileWatermark {
            mtime_secs: 42,
            size: 100,
            age_secs: 0,
        };
        let mut watermarks = HashMap::from([
            (codex_size_watermark_key(path), 100),
            (codex_mtime_watermark_key(path), 42),
        ]);
        assert!(
            !persisted_codex_watermark_matches(&watermarks, path, wm),
            "partial paged parses must not be skipped after daemon restart"
        );
        watermarks.insert(codex_complete_watermark_key(path), 0);
        assert!(!persisted_codex_watermark_matches(&watermarks, path, wm));
        watermarks.insert(codex_complete_watermark_key(path), 1);
        assert!(persisted_codex_watermark_matches(&watermarks, path, wm));
    }

    #[test]
    fn codex_slot_thread_classifies_as_worker() {
        let v = classify_conversation_type(
            Some("worker"),
            Some("slot-codex-1"),
            "codex-thread-uuid",
            "codex_cli",
        );
        assert_eq!(v, "worker");
    }

    /// Pins the raw_role contract for inserted Codex messages. The
    /// audit's `codex_raw_role_missing` finding fires when the row
    /// stores no provider role; preserving it on every insert closes
    /// the loop with `audit_classification`.
    #[test]
    fn codex_message_inserts_preserve_raw_provider_role() {
        // The fix replaced `raw_role: None` with `raw_role:
        // Some(m.role.clone())`. Re-derive what the Codex worker would
        // emit for an `agent_message` event (provider role
        // "assistant") and an `event_msg.user_message` event (provider
        // role "user") — both must survive into the Conversation
        // message row.
        let assistant = ParsedMessage {
            role: "assistant".to_string(),
            content: "ok".to_string(),
            timestamp: "2026-05-03T00:00:00Z".to_string(),
            line_no: 1,
            source_event_hash: short_sha256("a", 16),
        };
        let user = ParsedMessage {
            role: "user".to_string(),
            content: "ping".to_string(),
            timestamp: "2026-05-03T00:00:01Z".to_string(),
            line_no: 2,
            source_event_hash: short_sha256("b", 16),
        };
        let raw_role_assistant = Some(assistant.role.clone());
        let raw_role_user = Some(user.role.clone());
        assert_eq!(raw_role_assistant.as_deref(), Some("assistant"));
        assert_eq!(raw_role_user.as_deref(), Some("user"));
    }

    /// End-to-end audit invariant: the dry-run report must NOT flag a
    /// row that the new Codex worker has just written. This is the
    /// post-fix steady state — historical rows are flagged separately
    /// and reconciled via `mission_conversation_reconcile`.
    #[test]
    fn audit_passes_post_fix_codex_background_row() {
        let input = ClassificationAuditInput {
            session_id: "thread-uuid",
            stored_conversation_type: "codex_chat",
            source: "codex_cli",
            slot_id: None,
            slot_category: None,
            raw_role_present: true,
        };
        assert!(audit_classification(&input).is_none());
    }

    #[test]
    fn audit_passes_post_fix_codex_worker_row() {
        let input = ClassificationAuditInput {
            session_id: "slot-codex-thread",
            stored_conversation_type: "worker",
            source: "codex_cli",
            slot_id: Some("slot-codex-1"),
            slot_category: Some("worker"),
            raw_role_present: true,
        };
        assert!(audit_classification(&input).is_none());
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

fn extract_event_msg_user_text(payload: &Value) -> String {
    match payload.get("message") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Object(_)) => payload
            .get("message")
            .and_then(|v| v.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
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
