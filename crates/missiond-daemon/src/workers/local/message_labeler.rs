//! Message Labeler Worker.
//!
//! Central deterministic label infrastructure for conversation messages:
//! - writes rule evidence into `message_label_evidence`
//! - refreshes the legacy `message_labels` projection
//! - advances a per-session consumer watermark only after durable writes
//!
//! Rules in this module are pure and reusable by turn extraction where a
//! shared predicate is required.

use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use anyhow::Result;
use missiond_core::event::events::SessionEvent;
use missiond_core::event::subscription::SubscriptionOpts;
use missiond_core::types::{Conversation, ConversationMessage, MessageLabelEvidenceInput};
use regex::Regex;
use serde::Serialize;
use serde_json::json;
use tracing::{debug, info, warn};

use super::{BackgroundWorker, WorkerContext, WorkerKind};
use crate::state::AppState;

pub(crate) const CONSUMER_NAME: &str = "message_labeler:v1";
const LABEL_SOURCE: &str = "message_labeler";
const RULE_VERSION: &str = "20260531.1";

const BATCH_INTERVAL_SECS: u64 = 5;
const RECONCILE_INTERVAL_SECS: u64 = 60;
const DEFAULT_STARTUP_BACKFILL_LIMIT: i64 = 25;
const RECONCILE_BACKFILL_LIMIT: i64 = 50;
const MAX_MESSAGES_PER_SESSION: i64 = 10_000;
const STARTUP_BACKFILL_LIMIT_ENV: &str = "MISSIOND_MESSAGE_LABELER_STARTUP_LIMIT";

const NOISE_TOOL_RESULT_CHARS: usize = 10_000;
const NOISE_THINKING_CHARS: usize = 5_000;
const BINARY_RATIO_THRESHOLD: f64 = 0.3;

static GIT_COMMIT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[([a-zA-Z0-9_\-/\.\s]+?)\s+(?:\([^\)]+\)\s+)?([a-f0-9]{7,40})\]\s+(.*)").unwrap()
});

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LabelSessionOutcome {
    pub session_id: String,
    pub source: Option<String>,
    pub messages_seen: usize,
    pub evidence_upserted: usize,
    pub projections_refreshed: usize,
    pub max_message_id: Option<i64>,
    pub watermark_advanced: bool,
}

/// A git commit detected from conversation content.
#[derive(Debug, Clone)]
pub(crate) struct CommitDetection {
    pub session_id: String,
    pub message_id: i64,
    pub branch: String,
    pub commit_hash: String,
    pub summary: String,
}

pub struct MessageLabelerWorker;

impl BackgroundWorker for MessageLabelerWorker {
    const KIND: WorkerKind = WorkerKind::Local;

    fn name(&self) -> &'static str {
        "message_labeler"
    }

    fn run(
        self,
        state: Arc<AppState>,
        ctx: WorkerContext,
    ) -> impl std::future::Future<Output = ()> + Send {
        run_message_labeler(state, ctx)
    }
}

async fn run_message_labeler(state: Arc<AppState>, mut ctx: WorkerContext) {
    let mut sub = match state
        .bus
        .subscribe::<SessionEvent>(
            "message_labeler",
            SubscriptionOpts::named("message_labeler"),
        )
        .await
    {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "message_labeler: bus subscribe failed, worker exiting");
            return;
        }
    };

    super::wait_for_background_db_grace("message_labeler").await;
    process_pending_sessions(&state, startup_backfill_limit(), "startup").await;

    let mut dirty: HashSet<String> = HashSet::new();
    let mut batch_tick =
        tokio::time::interval(tokio::time::Duration::from_secs(BATCH_INTERVAL_SECS));
    let mut reconcile_tick =
        tokio::time::interval(tokio::time::Duration::from_secs(RECONCILE_INTERVAL_SECS));
    batch_tick.tick().await;
    reconcile_tick.tick().await;

    loop {
        ctx.wait_if_paused().await;
        tokio::select! {
            ack_opt = sub.next() => {
                let Some(ack) = ack_opt else { break; };
                if let SessionEvent::Organized { session_id } = ack.event() {
                    ctx.begin_event("session", ack.seq().0, None);
                    debug!(session_id = %session_id, "message_labeler: queued organized session");
                    dirty.insert(session_id.clone());
                    ctx.progress(format!("queued label session {session_id}"));
                    ctx.complete("session queued for labeling");
                }
                ack.ack().await;
            }
            _ = batch_tick.tick(), if !dirty.is_empty() => {
                let batch: Vec<String> = dirty.drain().collect();
                ctx.begin_poll(Some(300));
                ctx.progress(format!("labeling {} sessions", batch.len()));
                process_sessions(&state, &batch, "event").await;
                ctx.record_success();
            }
            _ = reconcile_tick.tick() => {
                ctx.begin_poll(Some(300));
                ctx.progress("reconciling labeler watermarks");
                process_pending_sessions(&state, RECONCILE_BACKFILL_LIMIT, "reconcile").await;
                ctx.record_success();
            }
        }
    }
}

fn startup_backfill_limit() -> i64 {
    super::env_i64_bounded(
        STARTUP_BACKFILL_LIMIT_ENV,
        DEFAULT_STARTUP_BACKFILL_LIMIT,
        0,
        500,
    )
}

async fn process_sessions(state: &AppState, session_ids: &[String], reason: &str) {
    for sid in session_ids {
        match label_session(state, sid).await {
            Ok(outcome) => {
                debug!(
                    session_id = %sid,
                    reason,
                    messages = outcome.messages_seen,
                    evidence = outcome.evidence_upserted,
                    projection = outcome.projections_refreshed,
                    "message_labeler: session labeled"
                );
            }
            Err(e) => {
                warn!(session_id = %sid, reason, error = %e, "message_labeler: session failed")
            }
        }
    }
}

async fn process_pending_sessions(state: &AppState, limit: i64, reason: &str) {
    let sessions = match state
        .store
        .message_labeler_pending_sessions(CONSUMER_NAME, None, limit)
        .await
    {
        Ok(sessions) => sessions,
        Err(e) => {
            warn!(error = %e, reason, "message_labeler: pending query failed");
            return;
        }
    };
    if !sessions.is_empty() {
        info!(
            reason,
            sessions = sessions.len(),
            "message_labeler: processing pending sessions"
        );
        process_sessions(state, &sessions, reason).await;
    }
}

pub(crate) async fn label_session(
    state: &AppState,
    session_id: &str,
) -> Result<LabelSessionOutcome> {
    let since_id = state
        .store
        .watermark_get(CONSUMER_NAME, session_id)
        .await?
        .and_then(|(message_id, _)| message_id)
        .unwrap_or(0);
    label_session_since(state, session_id, since_id).await
}

pub(crate) async fn replay_session(
    state: &AppState,
    session_id: &str,
) -> Result<LabelSessionOutcome> {
    label_session_since(state, session_id, 0).await
}

async fn label_session_since(
    state: &AppState,
    session_id: &str,
    since_id: i64,
) -> Result<LabelSessionOutcome> {
    let conversation = state.store.get_conversation(session_id).await?;
    let source = conversation.as_ref().map(|c| c.source.clone());
    let Some(conversation) = conversation else {
        return Ok(LabelSessionOutcome {
            session_id: session_id.to_string(),
            source,
            messages_seen: 0,
            evidence_upserted: 0,
            projections_refreshed: 0,
            max_message_id: None,
            watermark_advanced: false,
        });
    };

    let messages = state
        .store
        .get_conversation_messages(session_id, Some(since_id), MAX_MESSAGES_PER_SESSION)
        .await?;

    if messages.is_empty() {
        return Ok(LabelSessionOutcome {
            session_id: session_id.to_string(),
            source,
            messages_seen: 0,
            evidence_upserted: 0,
            projections_refreshed: 0,
            max_message_id: None,
            watermark_advanced: false,
        });
    }

    let message_ids = messages.iter().map(|m| m.id).collect::<Vec<_>>();
    let max_message_id = message_ids.iter().copied().max();
    let max_timestamp = messages.last().map(|m| m.timestamp.as_str());
    let evidence = collect_message_label_evidence(&conversation, &messages);

    let evidence_upserted = state
        .store
        .message_label_evidence_upsert_batch(&evidence)
        .await?;
    let projections_refreshed = state
        .store
        .message_label_projection_refresh(&message_ids)
        .await?;

    if let Some(max_id) = max_message_id {
        let extra = json!({
            "ruleVersion": RULE_VERSION,
            "source": LABEL_SOURCE,
        })
        .to_string();
        state
            .store
            .watermark_advance_full(
                CONSUMER_NAME,
                session_id,
                Some(max_id),
                max_timestamp,
                Some(&extra),
            )
            .await?;
    }

    Ok(LabelSessionOutcome {
        session_id: session_id.to_string(),
        source,
        messages_seen: messages.len(),
        evidence_upserted,
        projections_refreshed,
        max_message_id,
        watermark_advanced: max_message_id.is_some(),
    })
}

pub(crate) async fn pending_sessions(
    state: &AppState,
    source: Option<&str>,
    limit: i64,
) -> Result<Vec<String>> {
    Ok(state
        .store
        .message_labeler_pending_sessions(CONSUMER_NAME, source, limit)
        .await?)
}

pub(crate) async fn audit(state: &AppState, source: Option<&str>) -> Result<serde_json::Value> {
    Ok(state
        .store
        .message_labeler_audit(CONSUMER_NAME, source)
        .await?)
}

pub(crate) fn collect_message_label_evidence(
    conversation: &Conversation,
    messages: &[ConversationMessage],
) -> Vec<MessageLabelEvidenceInput> {
    let mut labels = Vec::new();
    let mut seen: HashSet<(i64, String, String, String, String, String)> = HashSet::new();

    for msg in messages {
        collect_noise_labels(msg, &mut labels, &mut seen);
        collect_tool_labels(msg, conversation, &mut labels, &mut seen);
        if conversation.source == "claude_code" {
            collect_claudecode_origin_labels(msg, conversation, &mut labels, &mut seen);
        }
    }

    labels
}

fn collect_noise_labels(
    msg: &ConversationMessage,
    labels: &mut Vec<MessageLabelEvidenceInput>,
    seen: &mut HashSet<(i64, String, String, String, String, String)>,
) {
    match msg.role.as_str() {
        "tool_result" => {
            if msg.content.len() > NOISE_TOOL_RESULT_CHARS {
                push_label(
                    labels,
                    seen,
                    msg.id,
                    "noise_long_output",
                    "true",
                    "noise.tool_result.long",
                    50,
                    "tool_result content exceeds noise threshold",
                    json!({"chars": msg.content.len(), "threshold": NOISE_TOOL_RESULT_CHARS}),
                );
            }
            if is_binary_like(&msg.content) {
                push_label(
                    labels,
                    seen,
                    msg.id,
                    "noise_binary",
                    "true",
                    "noise.tool_result.binary",
                    50,
                    "tool_result content is binary-like",
                    json!({"sampleBytes": 500, "threshold": BINARY_RATIO_THRESHOLD}),
                );
            }
        }
        "thinking" => {
            if msg.content.len() > NOISE_THINKING_CHARS {
                push_label(
                    labels,
                    seen,
                    msg.id,
                    "thinking_long",
                    "true",
                    "noise.thinking.long",
                    50,
                    "thinking content exceeds threshold",
                    json!({"chars": msg.content.len(), "threshold": NOISE_THINKING_CHARS}),
                );
            }
        }
        _ => {}
    }
}

fn collect_tool_labels(
    msg: &ConversationMessage,
    conversation: &Conversation,
    labels: &mut Vec<MessageLabelEvidenceInput>,
    seen: &mut HashSet<(i64, String, String, String, String, String)>,
) {
    if msg.has_tool_use {
        if let Some(ref tool_name) = msg.tool_name {
            let class = match tool_name.as_str() {
                "Bash" => classify_bash_command(&msg.content),
                other => other.to_lowercase(),
            };
            push_label(
                labels,
                seen,
                msg.id,
                "tool_class",
                &class,
                "tool.class",
                60,
                "classified tool call",
                json!({"toolName": tool_name}),
            );
        }
    }

    if let Some(commit) = detect_git_commit_in_message(msg, &conversation.id) {
        push_label(
            labels,
            seen,
            msg.id,
            "tool_action",
            "commit",
            "tool.git.commit",
            60,
            "detected git commit output",
            json!({"branch": &commit.branch, "commitHash": &commit.commit_hash}),
        );
        push_label(
            labels,
            seen,
            msg.id,
            "commit_hash",
            &commit.commit_hash,
            "tool.git.commit_hash",
            60,
            "detected git commit hash",
            json!({"summary": &commit.summary}),
        );
        push_label(
            labels,
            seen,
            msg.id,
            "commit_branch",
            &commit.branch,
            "tool.git.commit_branch",
            60,
            "detected git commit branch",
            json!({"commitHash": &commit.commit_hash}),
        );
    }
}

fn collect_claudecode_origin_labels(
    msg: &ConversationMessage,
    conversation: &Conversation,
    labels: &mut Vec<MessageLabelEvidenceInput>,
    seen: &mut HashSet<(i64, String, String, String, String, String)>,
) {
    if is_local_command_artifact(&msg.content) {
        push_label(
            labels,
            seen,
            msg.id,
            "origin_layer",
            "local_command",
            "claudecode.origin.local_command",
            100,
            "ClaudeCode local command artifact",
            json!({}),
        );
        push_label(
            labels,
            seen,
            msg.id,
            "speaker",
            "terminal_artifact",
            "claudecode.speaker.terminal_artifact",
            100,
            "ClaudeCode local command artifact",
            json!({}),
        );
    }

    if is_missiond_runtime_prompt(&msg.content) {
        push_label(
            labels,
            seen,
            msg.id,
            "origin_layer",
            "missiond_prompt",
            "claudecode.origin.missiond_prompt",
            90,
            "MissionD-generated worker prompt",
            json!({}),
        );
        push_label(
            labels,
            seen,
            msg.id,
            "speaker",
            "missiond_runtime",
            "claudecode.speaker.missiond_runtime",
            90,
            "MissionD-generated worker prompt",
            json!({}),
        );
    }

    if is_image_context(&msg.content) {
        push_label(
            labels,
            seen,
            msg.id,
            "origin_layer",
            "file_context",
            "claudecode.origin.file_context",
            80,
            "ClaudeCode image/file context marker",
            json!({"hasHumanTail": image_context_has_human_tail(&msg.content)}),
        );
        if !image_context_has_human_tail(&msg.content) {
            push_label(
                labels,
                seen,
                msg.id,
                "speaker",
                "provider_system",
                "claudecode.speaker.provider_system",
                80,
                "pure ClaudeCode provider context",
                json!({"kind": "image"}),
            );
        }
    } else if is_provider_context(&msg.content) {
        push_label(
            labels,
            seen,
            msg.id,
            "origin_layer",
            "provider_context",
            "claudecode.origin.provider_context",
            80,
            "ClaudeCode provider context",
            json!({}),
        );
        push_label(
            labels,
            seen,
            msg.id,
            "speaker",
            "provider_system",
            "claudecode.speaker.provider_system",
            80,
            "ClaudeCode provider context",
            json!({}),
        );
    }

    if conversation.conversation_type == "worker"
        && matches!(msg.role.as_str(), "user" | "worker_user")
    {
        push_label(
            labels,
            seen,
            msg.id,
            "speaker",
            "worker_agent",
            "claudecode.speaker.worker_agent",
            70,
            "worker conversation user-role message",
            json!({"conversationType": conversation.conversation_type}),
        );
    }

    if conversation.conversation_type == "subagent"
        && matches!(msg.role.as_str(), "user" | "agent_user")
    {
        push_label(
            labels,
            seen,
            msg.id,
            "speaker",
            "subagent",
            "claudecode.speaker.subagent",
            70,
            "subagent conversation user-role message",
            json!({"conversationType": conversation.conversation_type}),
        );
    }

    if matches!(
        msg.role.as_str(),
        "user"
            | "worker_user"
            | "agent_user"
            | "assistant"
            | "agent_assistant"
            | "tool_result"
            | "thinking"
            | "compact_summary"
    ) {
        push_label(
            labels,
            seen,
            msg.id,
            "authority",
            "durable_provider_log",
            "claudecode.authority.durable_provider_log",
            10,
            "message imported from durable provider transcript",
            json!({"source": conversation.source}),
        );
    }
}

fn push_label(
    labels: &mut Vec<MessageLabelEvidenceInput>,
    seen: &mut HashSet<(i64, String, String, String, String, String)>,
    message_id: i64,
    label: &str,
    value: &str,
    rule_id: &str,
    priority: i32,
    reason: &str,
    evidence: serde_json::Value,
) {
    let key = (
        message_id,
        label.to_string(),
        value.to_string(),
        LABEL_SOURCE.to_string(),
        rule_id.to_string(),
        RULE_VERSION.to_string(),
    );
    if !seen.insert(key) {
        return;
    }
    labels.push(MessageLabelEvidenceInput {
        message_id,
        label: label.to_string(),
        value: value.to_string(),
        source: LABEL_SOURCE.to_string(),
        rule_id: rule_id.to_string(),
        rule_version: RULE_VERSION.to_string(),
        confidence: 1.0,
        priority,
        reason: Some(reason.to_string()),
        evidence,
    });
}

pub(crate) fn detect_git_commits(
    messages: &[ConversationMessage],
    session_id: &str,
) -> Vec<CommitDetection> {
    messages
        .iter()
        .filter_map(|msg| detect_git_commit_in_message(msg, session_id))
        .collect()
}

fn detect_git_commit_in_message(
    msg: &ConversationMessage,
    session_id: &str,
) -> Option<CommitDetection> {
    if !matches!(
        msg.role.as_str(),
        "user" | "worker_user" | "agent_user" | "tool_result"
    ) {
        return None;
    }
    let caps = GIT_COMMIT_RE.captures(&msg.content)?;
    Some(CommitDetection {
        session_id: session_id.to_string(),
        message_id: msg.id,
        branch: caps.get(1)?.as_str().to_string(),
        commit_hash: caps.get(2)?.as_str().to_string(),
        summary: caps.get(3)?.as_str().trim().to_string(),
    })
}

/// Classify a Bash command by its prefix.
pub(crate) fn classify_bash_command(content: &str) -> String {
    let cmd = extract_bash_command(content);
    let cmd = cmd.trim_start();

    let cmd = if let Some(pos) = cmd.find("&& ") {
        cmd[pos + 3..].trim_start()
    } else {
        cmd
    };

    let cmd = strip_env_prefix(cmd);

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

fn strip_env_prefix(cmd: &str) -> &str {
    let trimmed = cmd.trim_start();
    if let Some(rest) = trimmed.strip_prefix("LC_ALL=C ") {
        return rest.trim_start();
    }
    if let Some(rest) = trimmed.strip_prefix("LANG=C ") {
        return rest.trim_start();
    }
    trimmed
}

/// Extract the actual command string from `[Tool: Bash] command: "...", description: "..."`
pub(crate) fn extract_bash_command(content: &str) -> &str {
    if let Some(start) = content.find("command: \"") {
        let after = &content[start + 10..];
        if let Some(end) = after.find("\", description:") {
            return &after[..end];
        }
        if let Some(end) = after.rfind('"') {
            return &after[..end];
        }
    }
    content
}

/// Detect system command noise that should never be a turn boundary.
pub(crate) fn is_system_command_noise(content: &str) -> bool {
    let t = content.trim();
    t.is_empty()
        || t.starts_with("<local-command-")
        || t.starts_with("<command-name>")
        || t.starts_with("<command-message>")
        || t.starts_with("<command-args>")
}

fn is_local_command_artifact(content: &str) -> bool {
    let t = content.trim_start();
    is_system_command_noise(t)
        || t.starts_with("[Request interrupted")
        || t.starts_with("(Bash completed")
        || t.contains("<local-command-stdout>")
}

fn is_missiond_runtime_prompt(content: &str) -> bool {
    let t = content.trim_start();
    t.starts_with("Execute MissionD task ")
        || t.starts_with("Implement accepted swarm shard")
        || t.starts_with("Fix MissionD-side swarm ")
        || t.starts_with("Survey exact shards for swarm objective")
        || t.starts_with("Read-only smoke ")
        || t.starts_with("Read-only MissionD ")
        || t.starts_with("有新的对话内容待分析。")
        || contains_ci(t, "BoardTask ID")
        || contains_ci(t, "Task contract SSOT")
        || contains_ci(t, "## Swarm metadata")
        || contains_ci(t, "## Completion protocol")
        || contains_ci(t, "write_scope")
        || contains_ci(t, "must_not_touch")
}

fn is_provider_context(content: &str) -> bool {
    let t = content.trim_start();
    t.starts_with("The file ") && t.contains(" has been updated successfully.")
        || t.starts_with("<task-notification>")
        || t.starts_with("This session is being continued from a previous conversation")
        || t.starts_with("[Matched Skills")
}

fn is_image_context(content: &str) -> bool {
    let t = content.trim_start();
    t.starts_with("[Image:") || t.starts_with("[Image #")
}

fn image_context_has_human_tail(content: &str) -> bool {
    let t = content.trim_start();
    if !is_image_context(t) {
        return false;
    }
    match t.find(']') {
        Some(idx) => !t[idx + 1..].trim().is_empty(),
        None => false,
    }
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_conversation(source: &str, conversation_type: &str) -> Conversation {
        Conversation {
            id: "test-session".to_string(),
            project: None,
            project_id: None,
            slot_id: None,
            source: source.to_string(),
            model: None,
            git_branch: None,
            jsonl_path: None,
            parent_session_id: None,
            task_id: None,
            message_count: 0,
            started_at: "2026-05-31T00:00:00Z".to_string(),
            ended_at: None,
            status: "completed".to_string(),
            analyzed_at: None,
            analysis_version: 0,
            analysis_retries: 0,
            deep_analyzed_message_id: 0,
            chat_type: None,
            conversation_type: conversation_type.to_string(),
            updated_at: None,
            llm_summary: None,
            embedding_provider: None,
            session_timeline: None,
            timeline_built_at: None,
            user_id: None,
            tenant_id: None,
            application_id: None,
            channel: "cli".to_string(),
            topic_id: None,
            topic_label: None,
            context_capsule_hash: None,
        }
    }

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
            timestamp: format!("2026-05-31T00:00:{:02}Z", id),
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

    fn values_for(labels: &[MessageLabelEvidenceInput], label: &str) -> Vec<String> {
        labels
            .iter()
            .filter(|item| item.label == label)
            .map(|item| item.value.clone())
            .collect()
    }

    #[test]
    fn image_context_with_human_tail_does_not_mark_provider_speaker() {
        let conv = make_conversation("claude_code", "user");
        let msg = make_msg(1, "user", "[Image #6] 这个部分粘贴进微信编辑器后会怎样？");
        let labels = collect_message_label_evidence(&conv, &[msg]);
        assert!(values_for(&labels, "origin_layer").contains(&"file_context".to_string()));
        assert!(!values_for(&labels, "speaker").contains(&"provider_system".to_string()));
    }

    #[test]
    fn local_command_gets_terminal_artifact() {
        let conv = make_conversation("claude_code", "user");
        let msg = make_msg(
            1,
            "user",
            "<local-command-stdout>done</local-command-stdout>",
        );
        let labels = collect_message_label_evidence(&conv, &[msg]);
        assert!(values_for(&labels, "origin_layer").contains(&"local_command".to_string()));
        assert!(values_for(&labels, "speaker").contains(&"terminal_artifact".to_string()));
    }

    #[test]
    fn tool_labels_and_commit_share_rule_engine() {
        let conv = make_conversation("claude_code", "worker");
        let mut tool = make_msg(
            1,
            "assistant",
            r#"[Tool: Bash] command: "cd /tmp && git status", description: "status""#,
        );
        tool.has_tool_use = true;
        tool.tool_name = Some("Bash".to_string());
        let commit = make_msg(2, "worker_user", "[main abcdef1] implement labels");
        let labels = collect_message_label_evidence(&conv, &[tool, commit]);
        assert!(values_for(&labels, "tool_class").contains(&"git".to_string()));
        assert!(values_for(&labels, "tool_action").contains(&"commit".to_string()));
        assert!(values_for(&labels, "commit_hash").contains(&"abcdef1".to_string()));
    }

    #[test]
    fn classify_bash_strips_common_prefixes() {
        assert_eq!(
            classify_bash_command(
                r#"[Tool: Bash] command: "cd /repo && LC_ALL=C rg label", description: "search""#
            ),
            "search"
        );
    }
}
