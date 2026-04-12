//! Tagger & Chunker Worker — Stage 3 of the Cognitive Pipeline.
//!
//! Listens to SessionOrganized events from S2 Organizer, then:
//! 1. Extracts structured Turns from flat conversation_messages
//! 2. Applies noise labels to overlong/binary tool results
//! 3. Emits TurnExtracted events for downstream S4 Embedder
//!
//! Pure rules, zero LLM calls.
//!
//! Design doc: `docs/designs/phase4-tagger-chunker.md`

use std::collections::{BTreeSet, HashSet};
use std::sync::{Arc, LazyLock};

use regex::Regex;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use super::{BackgroundWorker, WorkerContext, WorkerKind};
use crate::event_bus::{DaemonEvent, TimelineEvent};
use crate::state::{AppState, EmbeddingTask};
use missiond_core::types::{ConversationMessage, RawTurn};

/// Fixed-window batch interval (same as S2 Organizer).
const BATCH_INTERVAL_SECS: u64 = 5;

/// Max messages to fetch per session in one pass.
const MAX_MESSAGES_PER_SESSION: i64 = 10_000;

/// Truncate user_content to this many chars.
const USER_CONTENT_MAX_CHARS: usize = 2000;

/// Noise threshold: tool_result content longer than this is marked as noise.
const NOISE_TOOL_RESULT_CHARS: usize = 10_000;

/// Noise threshold: thinking content longer than this is marked as long thinking.
const NOISE_THINKING_CHARS: usize = 5_000;

/// Binary detection: if more than 30% of first 500 chars are non-printable.
const BINARY_RATIO_THRESHOLD: f64 = 0.3;

/// Source tag for message_labels written by this worker.
const LABEL_SOURCE: &str = "tagger_chunker";

/// Regex to extract branch, commit hash, and summary from git commit output.
/// Matches standard:      `[main 9facdfa] refactor: some changes`
/// Matches root:          `[main (root-commit) 9facdfa] initial commit`
/// Matches detached HEAD: `[detached HEAD 56cbfdd] fix something`
/// Captures: (1) branch/ref, (2) hash, (3) summary
static GIT_COMMIT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[([a-zA-Z0-9_\-/\.\s]+?)\s+(?:\([^\)]+\)\s+)?([a-f0-9]{7,40})\]\s+(.*)")
        .unwrap()
});

/// A git commit detected from conversation content.
struct CommitDetection {
    session_id: String,
    message_id: i64,
    branch: String,
    commit_hash: String,
    summary: String,
}

pub struct TaggerChunkerWorker {
    pub timeline_rx: broadcast::Receiver<TimelineEvent>,
}

impl BackgroundWorker for TaggerChunkerWorker {
    const KIND: WorkerKind = WorkerKind::Local;

    fn name(&self) -> &'static str {
        "tagger_chunker"
    }

    fn run(
        self,
        state: Arc<AppState>,
        ctx: WorkerContext,
    ) -> impl std::future::Future<Output = ()> + Send {
        run_tagger_chunker(state, ctx, self.timeline_rx)
    }
}

async fn run_tagger_chunker(
    state: Arc<AppState>,
    mut ctx: WorkerContext,
    mut rx: broadcast::Receiver<TimelineEvent>,
) {
    // Startup backfill: process completed sessions that have no turns yet
    startup_backfill(&state).await;

    let mut dirty: HashSet<String> = HashSet::new();
    let mut tick = tokio::time::interval(tokio::time::Duration::from_secs(BATCH_INTERVAL_SECS));

    loop {
        ctx.wait_if_paused().await;
        tokio::select! {
            event = rx.recv() => match event {
                Ok(te) => {
                    if let DaemonEvent::SessionOrganized { ref session_id } = te.event {
                        dirty.insert(session_id.clone());
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    debug!(skipped = n, "TaggerChunker: broadcast lagged");
                    continue;
                }
                Err(_) => break,
            },
            _ = tick.tick(), if !dirty.is_empty() => {
                let batch: Vec<String> = dirty.drain().collect();
                process_batch(&state, &batch).await;
            }
        }
    }
}

/// Backfill completed/compacted sessions that have messages but no turns.
async fn startup_backfill(state: &AppState) {
    let unprocessed = match state.store.sessions_pending_turn_extraction(500).await {
        Ok(ids) => ids,
        Err(e) => {
            warn!(error = %e, "TaggerChunker: backfill query failed");
            return;
        }
    };

    if unprocessed.is_empty() {
        return;
    }

    let total = unprocessed.len();
    let mut success = 0usize;
    for sid in &unprocessed {
        match process_session(state, sid).await {
            Ok(count) if count > 0 => {
                success += 1;
                // Trigger per-turn embedding (same as event-driven path)
                if let Err(e) = state.embedding_tx.try_send(EmbeddingTask::ProcessTurns {
                    session_id: sid.clone(),
                }) {
                    debug!(session = %sid, error = %e, "TaggerChunker: embedding_tx full or closed");
                }
            }
            Ok(_) => {}
            Err(e) => warn!(session = %sid, error = %e, "TaggerChunker: backfill failed"),
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
    info!(total, success, "TaggerChunker: startup backfill complete");
}

async fn process_batch(state: &AppState, session_ids: &[String]) {
    for sid in session_ids {
        match process_session(state, sid).await {
            Ok(count) if count > 0 => {
                state.event_bus.publish(DaemonEvent::TurnExtracted {
                    session_id: sid.clone(),
                    turn_count: count,
                });
                // Trigger S4 per-turn embedding
                let _ = state.embedding_tx.try_send(EmbeddingTask::ProcessTurns {
                    session_id: sid.clone(),
                });
                debug!(session = %sid, turns = count, "TaggerChunker: turns extracted");
            }
            Ok(_) => {}
            Err(e) => warn!(session = %sid, error = %e, "TaggerChunker: process failed"),
        }
    }
}

async fn process_session(state: &AppState, session_id: &str) -> anyhow::Result<usize> {
    // ── Stage 1: Fetch incremental messages ──
    let since_id = state.store.get_last_turn_end_message_id(session_id).await?;
    let messages = state
        .store
        .get_conversation_messages(session_id, since_id, MAX_MESSAGES_PER_SESSION)
        .await?;

    if messages.is_empty() {
        return Ok(0);
    }

    // ── Stage 2: Message Analysis (labels + events) ──
    // Independent of turn extraction. Always runs on every message batch.
    analyze_messages(state, session_id, &messages).await;

    // ── Stage 3: Turn Chunking (extract → tail-trim → insert) ──
    chunk_turns(state, session_id, &messages).await
}

/// Stage 2: Per-message analysis — noise labels, tool classification, commit detection.
///
/// This pipeline is independent of turn extraction and must always run,
/// even when the session is active and turns are incomplete.
async fn analyze_messages(state: &AppState, session_id: &str, messages: &[ConversationMessage]) {
    // Noise labels
    let noise_labels = collect_noise_labels(messages);
    if !noise_labels.is_empty() {
        let label_refs: Vec<(i64, &str, &str, &str)> = noise_labels
            .iter()
            .map(|(id, label, value)| (*id, label.as_str(), value.as_str(), LABEL_SOURCE))
            .collect();
        if let Err(e) = state.store.insert_message_labels_batch(&label_refs).await {
            warn!(error = %e, "TaggerChunker: label insert failed");
        }
    }

    // Tool classification + commit detection
    let (tool_labels, detected_commits) = collect_tool_labels(messages, session_id);
    if !tool_labels.is_empty() {
        let label_refs: Vec<(i64, &str, &str, &str)> = tool_labels
            .iter()
            .map(|(id, label, value)| (*id, label.as_str(), value.as_str(), LABEL_SOURCE))
            .collect();
        if let Err(e) = state.store.insert_message_labels_batch(&label_refs).await {
            warn!(error = %e, "TaggerChunker: tool label insert failed");
        }
    }

    // Emit commit events
    if !detected_commits.is_empty() {
        let slot_id = state
            .store
            .get_conversation(session_id)
            .await
            .ok()
            .flatten()
            .and_then(|c| c.slot_id);

        for commit in detected_commits {
            info!(
                branch = %commit.branch,
                commit_hash = %commit.commit_hash,
                summary = %commit.summary,
                session_id = %commit.session_id,
                "TaggerChunker: git commit detected"
            );
            state.event_bus.publish(DaemonEvent::ContextualCommitDetected {
                commit_hash: commit.commit_hash,
                branch: commit.branch,
                summary: commit.summary,
                conversation_id: commit.session_id.clone(),
                message_id: commit.message_id,
                session_id: commit.session_id,
                slot_id: slot_id.clone(),
            });
        }
    }
}

/// Stage 3: Turn chunking — extract structured turns, trim active tail, persist.
///
/// Returns the number of turns inserted. Active sessions may produce 0 turns
/// if the latest turn is still incomplete (waiting for tool_result).
async fn chunk_turns(
    state: &AppState,
    session_id: &str,
    messages: &[ConversationMessage],
) -> anyhow::Result<usize> {
    let next_idx = state
        .store
        .get_max_turn_idx(session_id)
        .await?
        .map(|i| i + 1)
        .unwrap_or(0);

    let is_completed = state
        .store
        .get_conversation(session_id)
        .await?
        .map(|c| c.status == "completed" || c.status == "compacted")
        .unwrap_or(false);

    let mut raw_turns = extract_turns(messages);

    // Tail handling: drop last turn if session is active and turn may be incomplete.
    if !is_completed && !raw_turns.is_empty() {
        let last = raw_turns.last().unwrap();
        if let Some(last_msg) = messages.iter().rfind(|m| m.id == last.end_message_id) {
            if last_msg.role != "assistant" || last_msg.has_tool_use {
                raw_turns.pop();
            }
        }
    }

    if raw_turns.is_empty() {
        return Ok(0);
    }

    let inserted = state
        .store
        .insert_conversation_turns_batch(session_id, next_idx, &raw_turns)
        .await?;

    Ok(inserted)
}

// ─── Turn extraction (pure rules) ───────────────────────────────────────

/// Extract structured Turns from a flat message stream.
///
/// Turn boundary: `user | agent_user | compact_summary` starts a new Turn.
/// Everything else appends to the current Turn.
fn extract_turns(messages: &[ConversationMessage]) -> Vec<RawTurn> {
    let mut turns: Vec<RawTurn> = Vec::new();
    let mut builder: Option<TurnBuilder> = None;

    for msg in messages {
        // Skip system command noise (compact, local-command, etc.) — never a turn boundary
        if is_system_command_noise(&msg.content) {
            if let Some(ref mut b) = builder {
                b.end_message_id = msg.id;
                b.ended_at = msg.timestamp.clone();
                b.message_count += 1;
            }
            continue;
        }

        let is_turn_start = matches!(msg.role.as_str(), "user" | "agent_user" | "compact_summary");

        if is_turn_start {
            // Merge rule: if previous turn has no assistant response (user sent but AI
            // never responded — interrupted, image parse failure, or retry), absorb this
            // new user message into it instead of creating a new turn.
            if let Some(ref mut b) = builder {
                if !b.has_assistant_reply {
                    b.absorb_retry(msg);
                    continue;
                }
            }
            // Flush previous turn
            if let Some(b) = builder.take() {
                turns.push(b.build());
            }
            builder = Some(TurnBuilder::new(msg));
        } else {
            match builder.as_mut() {
                Some(b) => b.append(msg),
                None => {
                    // Preamble: messages before first user message → Turn 0
                    builder = Some(TurnBuilder::new_preamble(msg));
                }
            }
        }
    }

    // Flush last turn
    if let Some(b) = builder {
        turns.push(b.build());
    }

    turns
}

/// Max outcome text length (chars).
const OUTCOME_MAX_CHARS: usize = 500;

/// Regex to extract file_path from tool_use content: `file_path: "/path/to/file"`
static FILE_PATH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"file_path:\s*"([^"]+)""#).unwrap()
});

/// One entry in the turn skeleton: a tool call with message IDs for on-demand retrieval.
#[derive(Debug, Clone, serde::Serialize)]
struct SkeletonEntry {
    tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cmd: Option<String>,
    req: i64,
    /// Result message ID (filled when the next tool_result arrives).
    #[serde(skip_serializing_if = "Option::is_none")]
    res: Option<i64>,
}

/// Compact skeleton for a turn: user message + tool calls + outcome message ID.
#[derive(Debug, Clone, serde::Serialize)]
struct TurnSkeleton {
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<i64>,
    calls: Vec<SkeletonEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<i64>,
}

struct TurnBuilder {
    start_message_id: i64,
    end_message_id: i64,
    user_content: String,
    tool_names: BTreeSet<String>,
    tool_call_count: i32,
    message_count: i32,
    has_code_change: bool,
    has_mcp_call: bool,
    has_assistant_reply: bool,
    started_at: String,
    ended_at: String,
    files_read: BTreeSet<String>,
    files_changed: BTreeSet<String>,
    outcome: String,
    skeleton_calls: Vec<SkeletonEntry>,
    skeleton_user: Option<i64>,
    skeleton_outcome: Option<i64>,
}

impl TurnBuilder {
    fn new(msg: &ConversationMessage) -> Self {
        let is_user = matches!(msg.role.as_str(), "user" | "agent_user");
        let user_content = if is_user {
            truncate_str(&msg.content, USER_CONTENT_MAX_CHARS)
        } else {
            String::new()
        };
        Self {
            start_message_id: msg.id,
            end_message_id: msg.id,
            user_content,
            tool_names: BTreeSet::new(),
            tool_call_count: 0,
            message_count: 1,
            has_code_change: false,
            has_mcp_call: false,
            has_assistant_reply: false,
            started_at: msg.timestamp.clone(),
            ended_at: msg.timestamp.clone(),
            files_read: BTreeSet::new(),
            files_changed: BTreeSet::new(),
            outcome: String::new(),
            skeleton_calls: Vec::new(),
            skeleton_user: if is_user { Some(msg.id) } else { None },
            skeleton_outcome: None,
        }
    }

    fn new_preamble(msg: &ConversationMessage) -> Self {
        Self {
            start_message_id: msg.id,
            end_message_id: msg.id,
            user_content: String::new(),
            tool_names: BTreeSet::new(),
            tool_call_count: 0,
            message_count: 1,
            has_code_change: false,
            has_mcp_call: false,
            has_assistant_reply: false,
            started_at: msg.timestamp.clone(),
            ended_at: msg.timestamp.clone(),
            files_read: BTreeSet::new(),
            files_changed: BTreeSet::new(),
            outcome: String::new(),
            skeleton_calls: Vec::new(),
            skeleton_user: None,
            skeleton_outcome: None,
        }
    }

    /// Absorb a retry/interrupted user message into the current stub turn.
    /// Replaces user_content with the newer message (the successful attempt),
    /// updates skeleton_user, and bumps message_count.
    fn absorb_retry(&mut self, msg: &ConversationMessage) {
        self.end_message_id = msg.id;
        self.ended_at = msg.timestamp.clone();
        self.message_count += 1;
        // Use the latest user message as the canonical intent
        if matches!(msg.role.as_str(), "user" | "agent_user") {
            let new_content = truncate_str(&msg.content, USER_CONTENT_MAX_CHARS);
            if !new_content.is_empty() {
                self.user_content = new_content;
            }
            self.skeleton_user = Some(msg.id);
        }
    }

    fn append(&mut self, msg: &ConversationMessage) {
        self.end_message_id = msg.id;
        self.ended_at = msg.timestamp.clone();
        self.message_count += 1;

        if msg.role == "assistant" {
            self.has_assistant_reply = true;
        }

        if msg.has_tool_use {
            self.tool_call_count += 1;
            if let Some(ref name) = msg.tool_name {
                self.tool_names.insert(name.clone());

                let is_write_tool = name.contains("Write") || name.contains("Edit");
                if is_write_tool {
                    self.has_code_change = true;
                }
                if name.contains("mcp__") {
                    self.has_mcp_call = true;
                }

                // Extract file path from tool content → short name
                let short = extract_file_short_name(&msg.content);
                if let Some(ref s) = short {
                    if is_write_tool {
                        self.files_changed.insert(s.clone());
                    } else if name == "Read" || name == "Grep" || name == "Glob" {
                        self.files_read.insert(s.clone());
                    }
                }

                // Skeleton: record tool call with message ID
                let cmd = if name == "Bash" {
                    // Extract short command hint for Bash (first 3 words)
                    let full = extract_bash_command(&msg.content);
                    let short: String = full.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
                    if short.is_empty() { None } else { Some(short) }
                } else {
                    None
                };
                self.skeleton_calls.push(SkeletonEntry {
                    tool: name.clone(),
                    file: short,
                    cmd,
                    req: msg.id,
                    res: None,
                });
            }
        } else if msg.has_tool_result {
            // Fill in the result message ID for the last skeleton entry
            if let Some(last) = self.skeleton_calls.last_mut() {
                if last.res.is_none() {
                    last.res = Some(msg.id);
                }
            }
        } else if msg.role == "assistant" && !is_thinking_leak(&msg.content) {
            // Track last assistant text as outcome (skip thinking blocks + Gemini instruction leaks)
            let trimmed = msg.content.trim();
            if !trimmed.is_empty() && trimmed.len() > 10 {
                self.outcome = truncate_str(trimmed, OUTCOME_MAX_CHARS);
                self.skeleton_outcome = Some(msg.id);
            }
        }
    }

    fn build(self) -> RawTurn {
        let skeleton = TurnSkeleton {
            user: self.skeleton_user,
            calls: self.skeleton_calls,
            outcome: self.skeleton_outcome,
        };
        let skeleton_json = serde_json::to_string(&skeleton).unwrap_or_default();

        RawTurn {
            start_message_id: self.start_message_id,
            end_message_id: self.end_message_id,
            user_content: self.user_content,
            tool_names: self.tool_names.into_iter().collect::<Vec<_>>().join(","),
            tool_call_count: self.tool_call_count,
            message_count: self.message_count,
            has_code_change: self.has_code_change,
            has_mcp_call: self.has_mcp_call,
            started_at: self.started_at,
            ended_at: self.ended_at,
            files_read: self.files_read.into_iter().collect::<Vec<_>>().join(","),
            files_changed: self.files_changed.into_iter().collect::<Vec<_>>().join(","),
            outcome: self.outcome,
            skeleton: skeleton_json,
        }
    }
}

/// Extract short file name from tool_use content containing `file_path: "..."`.
/// Returns just the file name (last path component), deduplication-friendly.
fn extract_file_short_name(content: &str) -> Option<String> {
    let caps = FILE_PATH_RE.captures(content)?;
    let full_path = caps.get(1)?.as_str();
    let short = full_path
        .rsplit('/')
        .next()
        .unwrap_or(full_path);
    if short.is_empty() {
        return None;
    }
    Some(short.to_string())
}

// ─── Tagger (noise labels) ─────────────────────────────────────────────

/// Collect noise labels for messages that should be filtered by downstream consumers.
fn collect_noise_labels(messages: &[ConversationMessage]) -> Vec<(i64, String, String)> {
    let mut labels = Vec::new();

    for msg in messages {
        match msg.role.as_str() {
            "tool_result" => {
                if msg.content.len() > NOISE_TOOL_RESULT_CHARS {
                    labels.push((msg.id, "noise_long_output".to_string(), "true".to_string()));
                }
                if is_binary_like(&msg.content) {
                    labels.push((msg.id, "noise_binary".to_string(), "true".to_string()));
                }
            }
            "thinking" => {
                if msg.content.len() > NOISE_THINKING_CHARS {
                    labels.push((msg.id, "thinking_long".to_string(), "true".to_string()));
                }
            }
            _ => {}
        }
    }

    labels
}

// ─── Tagger (tool labels + commit detection) ─────────────────────────────

/// Classify tool calls and detect git commits from conversation messages.
///
/// Returns (labels_to_write, detected_commits).
fn collect_tool_labels(
    messages: &[ConversationMessage],
    session_id: &str,
) -> (Vec<(i64, String, String)>, Vec<CommitDetection>) {
    let mut labels = Vec::new();
    let mut commits = Vec::new();

    for msg in messages {
        // Tool classification: assistant messages with has_tool_use
        if msg.has_tool_use {
            if let Some(ref tool_name) = msg.tool_name {
                let class = match tool_name.as_str() {
                    "Bash" => classify_bash_command(&msg.content),
                    other => other.to_lowercase(),
                };
                labels.push((msg.id, "tool_class".to_string(), class));
            }
        }

        // Git commit detection: user messages (tool_results come as user messages)
        if msg.role == "user" {
            if let Some(caps) = GIT_COMMIT_RE.captures(&msg.content) {
                let branch = caps[1].to_string();
                let commit_hash = caps[2].to_string();
                let summary = caps[3].trim().to_string();

                labels.push((msg.id, "tool_action".to_string(), "commit".to_string()));
                labels.push((msg.id, "commit_hash".to_string(), commit_hash.clone()));
                labels.push((msg.id, "commit_branch".to_string(), branch.clone()));

                commits.push(CommitDetection {
                    session_id: session_id.to_string(),
                    message_id: msg.id,
                    branch,
                    commit_hash,
                    summary,
                });
            }
        }
    }

    (labels, commits)
}

/// Classify a Bash command by its prefix.
/// Content format: `[Tool: Bash] command: "actual command here", description: "..."`
fn classify_bash_command(content: &str) -> String {
    // Extract the actual command from the content format
    let cmd = extract_bash_command(content);
    let cmd = cmd.trim_start();

    // Strip common prefixes like `cd /path &&` or `LC_ALL=C`
    let cmd = if let Some(pos) = cmd.find("&& ") {
        cmd[pos + 3..].trim_start()
    } else {
        cmd
    };

    if cmd.starts_with("git ") {
        "git".to_string()
    } else if cmd.starts_with("grep ") || cmd.starts_with("rg ") {
        "search".to_string()
    } else if cmd.starts_with("cargo ")
        || cmd.starts_with("npm ")
        || cmd.starts_with("make ")
        || cmd.starts_with("docker ")
    {
        "build".to_string()
    } else if cmd.starts_with("cat ")
        || cmd.starts_with("ls ")
        || cmd.starts_with("mkdir ")
        || cmd.starts_with("rm ")
        || cmd.starts_with("cp ")
        || cmd.starts_with("mv ")
    {
        "fs".to_string()
    } else {
        "shell".to_string()
    }
}

/// Extract the actual command string from `[Tool: Bash] command: "...", description: "..."`
fn extract_bash_command(content: &str) -> &str {
    // Try to find `command: "` pattern
    if let Some(start) = content.find("command: \"") {
        let after = &content[start + 10..]; // skip `command: "`
        // Find the closing quote (handle escaped quotes)
        if let Some(end) = after.find("\", description:") {
            return &after[..end];
        }
        // Fallback: just take the rest
        if let Some(end) = after.rfind('"') {
            return &after[..end];
        }
    }
    // Fallback: raw content (plain command text)
    content
}

/// Detect thinking/reasoning content leaked into assistant messages.
/// Covers Claude Code `[thinking]` prefix and Gemini CLI instruction restating.
fn is_thinking_leak(content: &str) -> bool {
    let t = content.trim();
    // Claude Code thinking
    t.starts_with("[thinking]")
        // Gemini CLI: restates system prompt / critical instructions as assistant output
        || t.starts_with("**Reviewing Instructions**")
        || t.starts_with("**Recalling Critical Instructions")
        || t.starts_with("**Reviewing Rules")
        // Gemini CLI: thinking-style status headers that aren't real content
        || (t.starts_with("**") && t.contains("CRITICAL INSTRUCTION"))
}

/// Detect system command noise that should never be a turn boundary.
/// These are Claude Code internal commands (/compact, etc.) stored as role="user".
fn is_system_command_noise(content: &str) -> bool {
    let t = content.trim();
    t.is_empty()
        || t.starts_with("<local-command-")
        || t.starts_with("<command-name>")
        || t.starts_with("<command-message>")
        || t.starts_with("<command-args>")
}

/// Quick heuristic: high ratio of non-printable chars in first 500 bytes suggests binary/base64.
fn is_binary_like(content: &str) -> bool {
    let sample = missiond_core::util::safe_byte_truncate(content, 500);
    if sample.is_empty() {
        return false;
    }
    let non_text = sample
        .chars()
        .filter(|c| !c.is_ascii_graphic() && !c.is_whitespace())
        .count();
    (non_text as f64) / (sample.len() as f64) > BINARY_RATIO_THRESHOLD
}

/// Truncate a string to max_chars, respecting char boundaries.
fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        s.chars().take(max_chars).collect()
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg(id: i64, role: &str, content: &str) -> ConversationMessage {
        ConversationMessage {
            id,
            session_id: "test-session".to_string(),
            role: role.to_string(),
            content: content.to_string(),
            raw_content: None,
            message_uuid: None,
            parent_uuid: None,
            model: None,
            timestamp: format!("2026-03-22T00:00:{:02}Z", id),
            metadata: None,
            tool_name: None,
            raw_role: None,
            content_types: None,
            has_image: false,
            has_tool_use: false,
            has_tool_result: false,
            token_count: None,
            seq: None,
            role_display: None,
        }
    }

    #[test]
    fn test_basic_turn_extraction() {
        let msgs = vec![
            make_msg(1, "user", "Hello"),
            make_msg(2, "assistant", "Hi there"),
            make_msg(3, "user", "Do something"),
            make_msg(4, "assistant", "Done"),
        ];
        let turns = extract_turns(&msgs);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].start_message_id, 1);
        assert_eq!(turns[0].end_message_id, 2);
        assert_eq!(turns[0].user_content, "Hello");
        assert_eq!(turns[0].message_count, 2);
        assert_eq!(turns[1].start_message_id, 3);
        assert_eq!(turns[1].end_message_id, 4);
    }

    #[test]
    fn test_preamble_turn() {
        let msgs = vec![
            make_msg(1, "system", "System message"),
            make_msg(2, "assistant", "Welcome"),
            make_msg(3, "user", "First real question"),
            make_msg(4, "assistant", "Answer"),
        ];
        let turns = extract_turns(&msgs);
        assert_eq!(turns.len(), 2);
        // Turn 0: preamble (system + assistant before first user)
        assert_eq!(turns[0].start_message_id, 1);
        assert_eq!(turns[0].end_message_id, 2);
        assert_eq!(turns[0].user_content, "");
        // Turn 1: first real user turn
        assert_eq!(turns[1].start_message_id, 3);
        assert_eq!(turns[1].end_message_id, 4);
    }

    #[test]
    fn test_consecutive_user_messages_merged() {
        // Consecutive user messages without assistant reply → absorbed into one turn
        let msgs = vec![
            make_msg(1, "user", "First"),
            make_msg(2, "user", "Second"),
            make_msg(3, "assistant", "Reply"),
        ];
        let turns = extract_turns(&msgs);
        assert_eq!(turns.len(), 1);
        // Both user messages merged, user_content = last user message
        assert_eq!(turns[0].start_message_id, 1);
        assert_eq!(turns[0].end_message_id, 3);
        assert_eq!(turns[0].user_content, "Second");
        assert_eq!(turns[0].message_count, 3);
    }

    #[test]
    fn test_compact_summary_starts_new_turn() {
        let msgs = vec![
            make_msg(1, "user", "Q1"),
            make_msg(2, "assistant", "A1"),
            make_msg(3, "compact_summary", "Context recap"),
            make_msg(4, "assistant", "Continuing"),
        ];
        let turns = extract_turns(&msgs);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[1].start_message_id, 3);
    }

    #[test]
    fn test_tool_use_tracking() {
        let mut msg2 = make_msg(2, "assistant", "Let me check");
        msg2.has_tool_use = true;
        msg2.tool_name = Some("Bash".to_string());

        let mut msg4 = make_msg(4, "assistant", "Editing file");
        msg4.has_tool_use = true;
        msg4.tool_name = Some("Edit".to_string());

        let msgs = vec![
            make_msg(1, "user", "Fix the bug"),
            msg2,
            make_msg(3, "tool_result", "output"),
            msg4,
        ];
        let turns = extract_turns(&msgs);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].tool_call_count, 2);
        assert!(turns[0].tool_names.contains("Bash"));
        assert!(turns[0].tool_names.contains("Edit"));
        assert!(turns[0].has_code_change);
    }

    #[test]
    fn test_is_binary_like() {
        assert!(!is_binary_like("Hello world"));
        assert!(!is_binary_like("fn main() { println!(\"hello\"); }"));
        // Many non-printable chars
        let binary = "\x00\x01\x02\x03\x04\x05\x06\x07\x08".repeat(60);
        assert!(is_binary_like(&binary));
    }

    #[test]
    fn test_noise_labels() {
        let long_output = "x".repeat(15_000);
        let msgs = vec![
            make_msg(1, "tool_result", &long_output),
            make_msg(2, "thinking", &"y".repeat(6000)),
            make_msg(3, "assistant", "short"),
        ];
        let labels = collect_noise_labels(&msgs);
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].1, "noise_long_output");
        assert_eq!(labels[1].1, "thinking_long");
    }

    #[test]
    fn test_tool_labels_bash_git() {
        let mut msg = make_msg(
            1,
            "assistant",
            r#"[Tool: Bash] command: "git status", description: "Show working tree status""#,
        );
        msg.has_tool_use = true;
        msg.tool_name = Some("Bash".to_string());

        let msgs = vec![msg];
        let (labels, commits) = collect_tool_labels(&msgs, "test-session");

        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].0, 1);
        assert_eq!(labels[0].1, "tool_class");
        assert_eq!(labels[0].2, "git");
        assert!(commits.is_empty());
    }

    #[test]
    fn test_tool_labels_bash_cd_then_git() {
        let mut msg = make_msg(
            1,
            "assistant",
            r#"[Tool: Bash] command: "cd <REPO_ROOT> && git log --oneline -5", description: "Show recent commits""#,
        );
        msg.has_tool_use = true;
        msg.tool_name = Some("Bash".to_string());

        let msgs = vec![msg];
        let (labels, _) = collect_tool_labels(&msgs, "test-session");

        assert_eq!(labels[0].2, "git");
    }

    #[test]
    fn test_extract_bash_command() {
        assert_eq!(
            extract_bash_command(r#"[Tool: Bash] command: "cargo build", description: "Build""#),
            "cargo build"
        );
        assert_eq!(extract_bash_command("plain command"), "plain command");
    }

    #[test]
    fn test_tool_labels_commit_detection() {
        let msg = make_msg(
            10,
            "user",
            "[main 9facdfa] refactor: unify workers\n 3 files changed, 42 insertions(+)",
        );
        let msgs = vec![msg];
        let (labels, commits) = collect_tool_labels(&msgs, "test-session");

        // Should produce 3 labels: tool_action, commit_hash, commit_branch
        assert_eq!(labels.len(), 3);
        assert_eq!(labels[0], (10, "tool_action".to_string(), "commit".to_string()));
        assert_eq!(labels[1], (10, "commit_hash".to_string(), "9facdfa".to_string()));
        assert_eq!(labels[2], (10, "commit_branch".to_string(), "main".to_string()));

        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].commit_hash, "9facdfa");
        assert_eq!(commits[0].branch, "main");
        assert_eq!(commits[0].summary, "refactor: unify workers");
        assert_eq!(commits[0].session_id, "test-session");
        assert_eq!(commits[0].message_id, 10);
    }

    #[test]
    fn test_tool_labels_no_false_positive() {
        let msg = make_msg(
            5,
            "user",
            "cargo build\n   Compiling missiond v0.1.0\n    Finished `dev` profile in 12.3s",
        );
        let msgs = vec![msg];
        let (labels, commits) = collect_tool_labels(&msgs, "test-session");

        // No commit labels, no commits detected
        assert!(labels.is_empty());
        assert!(commits.is_empty());
    }

    #[test]
    fn test_extract_file_short_name() {
        assert_eq!(
            extract_file_short_name(r#"[Tool: Read] file_path: "<REPO_ROOT>/src/main.rs""#),
            Some("main.rs".to_string())
        );
        assert_eq!(
            extract_file_short_name(r#"[Tool: Read] file_path: "/a/b/c.rs", limit: 120, offset: 1"#),
            Some("c.rs".to_string())
        );
        assert_eq!(
            extract_file_short_name(r#"[Tool: Bash] command: "cargo test""#),
            None
        );
    }

    #[test]
    fn test_turn_files_and_outcome() {
        let mut read_msg = make_msg(2, "assistant", r#"[Tool: Read] file_path: "/a/b/foo.rs""#);
        read_msg.has_tool_use = true;
        read_msg.tool_name = Some("Read".to_string());

        let mut edit_msg = make_msg(4, "assistant", r#"[Tool: Edit] file_path: "/a/b/bar.rs", old_string: "x""#);
        edit_msg.has_tool_use = true;
        edit_msg.tool_name = Some("Edit".to_string());

        let msgs = vec![
            make_msg(1, "user", "Fix the bug"),
            read_msg,
            make_msg(3, "tool_result", "file contents"),
            edit_msg,
            make_msg(5, "tool_result", "edit ok"),
            make_msg(6, "assistant", "I've fixed the bug in bar.rs by changing x to y."),
        ];
        let turns = extract_turns(&msgs);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].files_read, "foo.rs");
        assert_eq!(turns[0].files_changed, "bar.rs");
        assert_eq!(turns[0].outcome, "I've fixed the bug in bar.rs by changing x to y.");
    }

    #[test]
    fn test_absorb_retry_single_message_stubs() {
        // Simulates: user sends msg, interrupts, retries, then AI responds
        let msgs = vec![
            make_msg(1, "user", "[Request interrupted by user]"),
            make_msg(2, "user", "[Image parse failed]"),
            make_msg(3, "user", "Fix the auth bug"),
            make_msg(4, "assistant", "I'll fix it now"),
        ];
        let turns = extract_turns(&msgs);
        // All 3 user messages + 1 assistant → single turn (not 3 stubs + 1 turn)
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].message_count, 4);
        // user_content should be the LAST user message (the successful one)
        assert_eq!(turns[0].user_content, "Fix the auth bug");
        assert_eq!(turns[0].start_message_id, 1);
        assert_eq!(turns[0].end_message_id, 4);
    }

    #[test]
    fn test_absorb_retry_does_not_merge_after_response() {
        // Turn with user + assistant response should NOT absorb the next user message
        let msgs = vec![
            make_msg(1, "user", "Question 1"),
            make_msg(2, "assistant", "Answer 1"),
            make_msg(3, "user", "Question 2"),
            make_msg(4, "assistant", "Answer 2"),
        ];
        let turns = extract_turns(&msgs);
        // Two complete turns, no merging
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].user_content, "Question 1");
        assert_eq!(turns[1].user_content, "Question 2");
    }

    #[test]
    fn test_system_command_noise_skipped() {
        // /compact command noise should not create turns
        let msgs = vec![
            make_msg(1, "user", "Do something"),
            make_msg(2, "assistant", "Done"),
            make_msg(3, "user", "<local-command-caveat>Caveat: messages below...</local-command-caveat>"),
            make_msg(4, "user", "<command-name>/compact</command-name>\n            <command-message>compact</command-message>"),
            make_msg(5, "user", ""),
            make_msg(6, "user", "<local-command-stdout>Compacted</local-command-stdout>"),
            make_msg(7, "user", "Next question"),
            make_msg(8, "assistant", "Next answer"),
        ];
        let turns = extract_turns(&msgs);
        // Only 2 real turns: "Do something" + "Next question"
        // The 4 compact messages (3-6) are noise, absorbed into turn boundaries
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].user_content, "Do something");
        assert_eq!(turns[1].user_content, "Next question");
    }

    #[test]
    fn test_system_command_noise_not_false_positive() {
        // Normal messages with angle brackets should NOT be filtered
        let msgs = vec![
            make_msg(1, "user", "<task-notification>task result</task-notification>"),
            make_msg(2, "assistant", "Got it"),
        ];
        let turns = extract_turns(&msgs);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].message_count, 2);
    }

    #[test]
    fn test_gemini_thinking_leak_skipped_for_outcome() {
        // Gemini leaks "**Reviewing Instructions**" as assistant messages.
        // These should NOT become the outcome — the real answer should.
        let msgs = vec![
            make_msg(1, "user", "审计最新 commit"),
            make_msg(2, "assistant", "**Reviewing Instructions** \\n\\nI am recalling Critical Instructions 1 and 2.\nCRITICAL INSTRUCTION 1: ..."),
            make_msg(3, "assistant", "**Recalling Critical Instructions and Executing Search**\nCRITICAL INSTRUCTION 1: ..."),
            make_msg(4, "assistant", "审计结果：这个 commit 质量很高，架构优雅。"),
        ];
        let turns = extract_turns(&msgs);
        assert_eq!(turns.len(), 1);
        // Outcome should be the real answer, not the instruction leak
        assert_eq!(turns[0].outcome, "审计结果：这个 commit 质量很高，架构优雅。");
    }
}
