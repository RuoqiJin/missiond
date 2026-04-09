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
            Ok(count) if count > 0 => success += 1,
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
    // 1. Determine incremental start point
    let since_id = state.store.get_last_turn_end_message_id(session_id).await?;

    // 2. Fetch incremental messages
    let messages = state
        .store
        .get_conversation_messages(session_id, since_id, MAX_MESSAGES_PER_SESSION)
        .await?;

    if messages.is_empty() {
        return Ok(0);
    }

    // 3. Get next turn_idx
    let next_idx = state
        .store
        .get_max_turn_idx(session_id)
        .await?
        .map(|i| i + 1)
        .unwrap_or(0);

    // 4. Check if session is completed (for tail handling)
    let is_completed = state
        .store
        .get_conversation(session_id)
        .await?
        .map(|c| c.status == "completed" || c.status == "compacted")
        .unwrap_or(false);

    // 5. Extract turns
    let mut raw_turns = extract_turns(&messages);

    // 6. Handle tail: drop last turn if session is still active and turn may be incomplete.
    // An assistant message with has_tool_use means it's waiting for tool_result — still incomplete.
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

    // 7. Collect noise labels
    let noise_labels = collect_noise_labels(&messages);
    if !noise_labels.is_empty() {
        let label_refs: Vec<(i64, &str, &str, &str)> = noise_labels
            .iter()
            .map(|(id, label, value)| (*id, label.as_str(), value.as_str(), LABEL_SOURCE))
            .collect();
        if let Err(e) = state.store.insert_message_labels_batch(&label_refs).await {
            warn!(error = %e, "TaggerChunker: label insert failed");
        }
    }

    // 7b. Collect tool labels + detect commits
    let (tool_labels, detected_commits) = collect_tool_labels(&messages, session_id);
    if !tool_labels.is_empty() {
        let label_refs: Vec<(i64, &str, &str, &str)> = tool_labels
            .iter()
            .map(|(id, label, value)| (*id, label.as_str(), value.as_str(), LABEL_SOURCE))
            .collect();
        if let Err(e) = state.store.insert_message_labels_batch(&label_refs).await {
            warn!(error = %e, "TaggerChunker: tool label insert failed");
        }
    }

    // 7c. Emit commit events
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

    // 8. Insert turns
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
        let is_turn_start = matches!(msg.role.as_str(), "user" | "agent_user" | "compact_summary");

        if is_turn_start {
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

struct TurnBuilder {
    start_message_id: i64,
    end_message_id: i64,
    user_content: String,
    tool_names: BTreeSet<String>,
    tool_call_count: i32,
    message_count: i32,
    has_code_change: bool,
    has_mcp_call: bool,
    started_at: String,
    ended_at: String,
}

impl TurnBuilder {
    fn new(msg: &ConversationMessage) -> Self {
        let user_content = if matches!(msg.role.as_str(), "user" | "agent_user") {
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
            started_at: msg.timestamp.clone(),
            ended_at: msg.timestamp.clone(),
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
            started_at: msg.timestamp.clone(),
            ended_at: msg.timestamp.clone(),
        }
    }

    fn append(&mut self, msg: &ConversationMessage) {
        self.end_message_id = msg.id;
        self.ended_at = msg.timestamp.clone();
        self.message_count += 1;

        if msg.has_tool_use {
            self.tool_call_count += 1;
            if let Some(ref name) = msg.tool_name {
                self.tool_names.insert(name.clone());
                if name.contains("Write") || name.contains("Edit") {
                    self.has_code_change = true;
                }
                if name.contains("mcp__") {
                    self.has_mcp_call = true;
                }
            }
        }
    }

    fn build(self) -> RawTurn {
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
        }
    }
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
fn classify_bash_command(content: &str) -> String {
    let cmd = content.trim_start();
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
    fn test_consecutive_user_messages() {
        let msgs = vec![
            make_msg(1, "user", "First"),
            make_msg(2, "user", "Second"),
            make_msg(3, "assistant", "Reply"),
        ];
        let turns = extract_turns(&msgs);
        assert_eq!(turns.len(), 2);
        // First turn: single user message with no response
        assert_eq!(turns[0].start_message_id, 1);
        assert_eq!(turns[0].end_message_id, 1);
        assert_eq!(turns[0].message_count, 1);
        // Second turn: user + assistant
        assert_eq!(turns[1].start_message_id, 2);
        assert_eq!(turns[1].end_message_id, 3);
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
        let mut msg = make_msg(1, "assistant", "git status");
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
}
