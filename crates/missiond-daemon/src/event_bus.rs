//! Event Bus — centralized event dispatch for daemon module decoupling.
//!
//! Phase 6: Cognitive Timeline architecture.
//! All events flow through MPSC → Timeline Writer → SQLite → broadcast<TimelineEvent>.
//! This guarantees: persistent storage, global monotonic seq, causal ordering.

use std::sync::atomic::{AtomicU64, Ordering};
use missiond_core::CliEngine;
use serde::{Serialize, Deserialize};
use serde_json::json;
use tokio::sync::mpsc;
use tracing::debug;

/// Domain events that flow between daemon modules.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Fields used in Debug output via publish() tracing
pub(crate) enum DaemonEvent {
    // ===== Slot lifecycle =====
    /// A slot transitioned to Idle (extraction/submit can trigger).
    SlotBecameIdle { slot_id: String },

    // ===== Task scheduling =====
    /// A new submit task was created and queued.
    TaskCreated { task_id: String },
    /// A submit task completed (slot returned to idle).
    TaskCompleted { task_id: String },

    // ===== Decision engine =====
    /// A new master-target question was created.
    QuestionCreated { question_id: String },

    // ===== LLM Gateway (LEGACY — use CliRequestStarted/Completed/CliToolActivity) =====
    /// DEPRECATED: Use CliRequestStarted instead. Kept for backward compat with stored timeline data.
    GeminiRequestStarted {
        request_id: String,
        caller: String,
        session_id: Option<String>,
        model: String,
        prompt_chars: usize,
        /// Full prompt text — stored in gemini_requests table, NOT sent to frontend.
        prompt_text: Option<String>,
    },
    /// DEPRECATED: Use CliRequestCompleted instead.
    GeminiRequestCompleted {
        request_id: String,
        caller: String,
        session_id: Option<String>,
        api_mode: String,
        model: String,
        prompt_chars: usize,
        response_chars: usize,
        queue_wait_ms: u64,
        duration_ms: u64,
        retry_count: u32,
        status: String,
        error_msg: Option<String>,
        /// Full response text — stored in gemini_requests table, NOT sent to frontend.
        response_text: Option<String>,
    },

    /// DEPRECATED: Use CliToolActivity instead.
    GeminiToolActivity {
        request_id: String,
        /// Sequential index within this request (1, 2, 3...)
        tool_seq: u32,
        /// "tool_use" or "tool_result"
        activity: String,
        /// Tool name (e.g. "read_file", "grep", "bash")
        tool_name: Option<String>,
        /// Truncated input preview (≤200 chars)
        input_preview: Option<String>,
        /// Truncated result preview (≤500 chars)
        result_preview: Option<String>,
        /// Whether the tool result was an error
        is_error: bool,
    },

    // ===== Phase 5: Frontend push events =====
    /// A decision question was resolved by the decision engine.
    DecisionResolved {
        question_id: String,
        tier: String,
        duration_ms: u64,
    },
    /// A question was answered or dismissed by a human.
    QuestionResolved {
        question_id: String,
        resolution: String,
    },
    /// Memory extraction phase transition.
    MemoryPhaseChanged {
        slot_id: String,
        phase: String,
        active_type: Option<String>,
    },
    // ===== Board Task lifecycle =====
    /// A new board task was created.
    BoardTaskCreated {
        task_id: String,
        title: String,
        category: String,
    },
    /// Board task status changed (toggle, manual update).
    BoardTaskStatusChanged {
        task_id: String,
        old_status: String,
        new_status: String,
    },
    /// A note was added to a board task.
    BoardTaskNoteAdded {
        task_id: String,
        note_id: String,
        content_preview: String,
    },
    /// A board task was claimed by a slot/session.
    BoardTaskClaimed {
        task_id: String,
        slot_id: String,
    },
    /// A board task was deleted.
    BoardTaskDeleted {
        task_id: String,
        title: String,
    },
    /// Board task fields updated (title, category, priority, etc.).
    BoardTaskUpdated {
        task_id: String,
        status: String,
        category: String,
    },
    /// Slot state transition (extends SlotBecameIdle with prev state).
    SlotStateChanged {
        slot_id: String,
        new_state: String,
        prev_state: String,
    },

    // ===== Phase 6 L4: Proactive Agency =====
    /// Timeline Analyst generated an insight from periodic analysis.
    InsightGenerated {
        category: String,
        priority: String,
        title: String,
    },

    // ===== Git Activity =====
    /// A new git commit was detected in a monitored repository.
    GitCommitDetected {
        repo: String,
        hash: String,
        short_hash: String,
        author: String,
        message: String,
        /// Commit timestamp as ISO-8601 string.
        committed_at: String,
    },

    // ===== Slot Command Dispatch =====
    /// A task/prompt was dispatched to a slot for execution.
    SlotTaskDispatched {
        slot_id: String,
        /// Associated task_id if dispatched from mission_submit queue.
        task_id: Option<String>,
        /// Purpose: "submit", "extraction", "deep_analysis", "user_voice", "consolidation"
        purpose: String,
        prompt_chars: usize,
        /// Truncated preview of the dispatched prompt.
        preview: String,
    },

    // ===== Conversation Activity =====
    /// A new conversation message was logged (user or assistant).
    ConversationMessageLogged {
        message_id: i64,
        session_id: String,
        parent_session_id: Option<String>,
        /// Slot ID if this is a PTY slot session (None = master CLI session).
        slot_id: Option<String>,
        role: String,
        content_chars: usize,
        /// Truncated preview for timeline display.
        preview: String,
    },

    // ===== Codex CLI (LEGACY — use CliRequestStarted/Completed) =====
    /// DEPRECATED: Use CliRequestStarted instead.
    CodexRequestStarted {
        request_id: String,
        caller: String,
        model: String,
        prompt_chars: usize,
        has_image: bool,
        prompt_text: Option<String>,
        image_hash: Option<String>,
    },
    /// DEPRECATED: Use CliRequestCompleted instead.
    CodexRequestCompleted {
        request_id: String,
        caller: String,
        model: String,
        prompt_chars: usize,
        response_chars: usize,
        duration_ms: u64,
        status: String,
        error_msg: Option<String>,
        response_text: Option<String>,
        input_tokens: u64,
        output_tokens: u64,
        image_hash: Option<String>,
    },

    // ===== Vision Worker =====
    /// A message with image content was inserted, needs async vision processing.
    ImageMessageInserted {
        message_id: i64,
        session_id: String,
    },

    // ===== Briefing Worker =====
    /// Briefing worker found pending entries and started processing.
    BriefingBatchStarted {
        pending_count: usize,
    },
    /// A timeline entry's summary was updated by the briefing worker.
    /// Frontend uses target_seq + summary to do in-place cache update.
    BriefingSummaryGenerated {
        /// The seq of the timeline entry whose summary was updated.
        target_seq: i64,
        /// The new semantic summary.
        summary: String,
        /// How the summary was produced: "minimax", "static_rule", "tool_skip"
        method: String,
    },

    // ===== Translation Worker =====
    /// Translation worker picked up a thinking message for translation.
    TranslationStarted {
        message_id: i64,
        /// Virtual slot_id for Slot swimlane routing (e.g. "translation-worker").
        slot_id: String,
        content_chars: usize,
    },
    /// Translation completed successfully.
    TranslationCompleted {
        message_id: i64,
        slot_id: String,
        /// First ~80 chars of the translated text for timeline preview.
        preview: String,
        duration_ms: u64,
    },
    /// Translation failed.
    TranslationFailed {
        message_id: i64,
        slot_id: String,
        error: String,
    },

    // ===== Step Narrator Worker (batch-based) =====
    /// Step Narrator started processing a specific session.
    NarrationSessionStarted {
        session_id: String,
        total_messages: usize,
        already_narrated: usize,
    },
    /// A single batch (≤10 messages) completed successfully.
    NarrationBatchCompleted {
        session_id: String,
        batch_index: usize,
        processed_count: usize,
        total_messages: usize,
        duration_ms: u64,
    },
    /// All pending batches for a session have been processed.
    NarrationSessionCompleted {
        session_id: String,
        total_narrated: usize,
    },
    /// A batch failed for a session.
    NarrationFailed {
        session_id: String,
        batch_index: usize,
        error: String,
        will_retry: bool,
    },

    // ===== MiniMax Gateway =====
    /// A MiniMax API call was completed through the gateway (unified for all workers).
    WorkerLlmCall {
        caller: String,
        task_id: Option<String>,
        status: String,
        prompt_chars: usize,
        response_chars: usize,
        duration_ms: u64,
        queue_wait_ms: u64,
    },

    // ===== Tool Completion (auto-instrumentation) =====
    /// A high-value tool completed execution (Bash, Write, Edit, MCP tools).
    /// Emitted from message_handler when tool_result arrives for whitelisted tools.
    ToolCompleted {
        session_id: String,
        slot_id: Option<String>,
        tool_name: String,
        status: String,
        is_error: bool,
        input_summary: Option<String>,
        output_summary: String,
    },

    // ===== Perception Layer: Config Awareness =====
    /// A monitored config file was changed (slots.yaml, prompts, servers.yaml, etc.).
    ConfigFileChanged {
        path: String,
        /// "modified", "created", "deleted"
        kind: String,
    },

    // ===== Unified CLI Engine Events =====
    /// Unified: an external CLI engine request was sent.
    /// Replaces engine-specific GeminiRequestStarted / CodexRequestStarted.
    CliRequestStarted {
        engine: CliEngine,
        request_id: String,
        caller: String,
        session_id: Option<String>,
        model: String,
        prompt_chars: usize,
        prompt_text: Option<String>,
        /// Engine-specific extra fields (e.g. has_image, image_hash for Codex).
        extra: serde_json::Value,
    },
    /// Unified: an external CLI engine request completed.
    /// Replaces engine-specific GeminiRequestCompleted / CodexRequestCompleted.
    CliRequestCompleted {
        engine: CliEngine,
        request_id: String,
        caller: String,
        session_id: Option<String>,
        model: String,
        prompt_chars: usize,
        response_chars: usize,
        duration_ms: u64,
        status: String,
        error_msg: Option<String>,
        response_text: Option<String>,
        /// Engine-specific extra fields (e.g. queue_wait_ms, retry_count for Gemini;
        /// input_tokens, output_tokens for Codex).
        extra: serde_json::Value,
    },
    // ===== Jarvis Async Dispatch =====
    /// A Jarvis async task completed — worker slot returned result.
    /// Frontend uses conversation_id to refetch and append the new message.
    JarvisTaskCompleted {
        conversation_id: String,
        task_id: String,
    },

    /// Unified: CLI engine tool activity (real-time tool use).
    /// Replaces GeminiToolActivity.
    CliToolActivity {
        engine: CliEngine,
        request_id: String,
        tool_seq: u32,
        activity: String,
        tool_name: Option<String>,
        input_preview: Option<String>,
        result_preview: Option<String>,
        is_error: bool,
    },
}

impl DaemonEvent {
    /// Wire type name for frontend and timeline persistence.
    pub fn wire_type(&self) -> &'static str {
        match self {
            Self::SlotBecameIdle { .. } => "slot_state_changed",
            Self::TaskCreated { .. } => "task_lifecycle",
            Self::TaskCompleted { .. } => "task_lifecycle",
            Self::QuestionCreated { .. } => "question_created",
            Self::GeminiRequestStarted { .. } => "gemini_request_started",
            Self::GeminiRequestCompleted { .. } => "gemini_request_completed",
            Self::GeminiToolActivity { .. } => "gemini_tool_activity",
            Self::DecisionResolved { .. } => "decision_made",
            Self::QuestionResolved { .. } => "question_resolved",
            Self::MemoryPhaseChanged { .. } => "memory_phase_changed",
            Self::BoardTaskCreated { .. } => "board_task_created",
            Self::BoardTaskStatusChanged { .. } => "board_task_status_changed",
            Self::BoardTaskNoteAdded { .. } => "board_task_note_added",
            Self::BoardTaskClaimed { .. } => "board_task_claimed",
            Self::BoardTaskDeleted { .. } => "board_task_deleted",
            Self::BoardTaskUpdated { .. } => "board_task_updated",
            Self::SlotStateChanged { .. } => "slot_state_changed",
            Self::InsightGenerated { .. } => "insight_generated",
            Self::GitCommitDetected { .. } => "git_commit",
            Self::SlotTaskDispatched { .. } => "slot_task_dispatched",
            Self::ConversationMessageLogged { ref role, .. } => {
                match role.as_str() {
                    "user" => "user_message",
                    "system" => "system_message",
                    "thinking" => "thinking_message",
                    _ => "assistant_message",
                }
            }
            Self::CodexRequestStarted { .. } => "codex_request_started",
            Self::CodexRequestCompleted { .. } => "codex_request_completed",
            Self::ImageMessageInserted { .. } => "image_message_inserted",
            Self::BriefingBatchStarted { .. } => "briefing_batch_started",
            Self::BriefingSummaryGenerated { .. } => "briefing_summary_generated",
            Self::TranslationStarted { .. } => "translation_started",
            Self::TranslationCompleted { .. } => "translation_completed",
            Self::TranslationFailed { .. } => "translation_failed",
            Self::NarrationSessionStarted { .. } => "narration_session_started",
            Self::NarrationBatchCompleted { .. } => "narration_batch_completed",
            Self::NarrationSessionCompleted { .. } => "narration_session_completed",
            Self::NarrationFailed { .. } => "narration_failed",
            Self::WorkerLlmCall { .. } => "worker_llm_call",
            Self::ToolCompleted { .. } => "tool_completed",
            Self::ConfigFileChanged { .. } => "config_file_changed",
            Self::JarvisTaskCompleted { .. } => "jarvis_task_completed",
            Self::CliRequestStarted { .. } => "cli_request_started",
            Self::CliRequestCompleted { .. } => "cli_request_completed",
            Self::CliToolActivity { .. } => "cli_tool_activity",
        }
    }

    /// Build frontend-compatible JSON payload (without seq/ts envelope).
    pub fn to_frontend_payload(&self) -> serde_json::Value {
        match self {
            Self::SlotBecameIdle { slot_id } =>
                json!({ "slot_id": slot_id, "new_state": "Idle" }),
            Self::TaskCreated { task_id } =>
                json!({ "task_id": task_id, "action": "created" }),
            Self::TaskCompleted { task_id } =>
                json!({ "task_id": task_id, "action": "completed" }),
            Self::QuestionCreated { question_id } =>
                json!({ "question_id": question_id }),
            Self::GeminiRequestStarted {
                request_id, caller, session_id, model,
                prompt_chars, ..
            } => json!({
                "request_id": request_id,
                "caller": caller,
                "session_id": session_id,
                "model": model,
                "prompt_chars": prompt_chars,
            }),
            Self::GeminiRequestCompleted {
                request_id, caller, session_id, api_mode, model,
                prompt_chars, response_chars, queue_wait_ms,
                duration_ms, retry_count, status, error_msg, ..
            } => json!({
                "caller": caller,
                "model": model,
                "duration_ms": duration_ms,
                "status": status,
                "error": error_msg,
                "request_id": request_id,
                "session_id": session_id,
                "api_mode": api_mode,
                "prompt_chars": prompt_chars,
                "response_chars": response_chars,
                "queue_wait_ms": queue_wait_ms,
                "retry_count": retry_count,
            }),
            Self::GeminiToolActivity {
                request_id, tool_seq, activity, tool_name,
                input_preview, result_preview, is_error,
            } => json!({
                "request_id": request_id,
                "tool_seq": tool_seq,
                "activity": activity,
                "tool_name": tool_name,
                "input_preview": input_preview,
                "result_preview": result_preview,
                "is_error": is_error,
            }),
            Self::DecisionResolved { question_id, tier, duration_ms } =>
                json!({ "question_id": question_id, "tier": tier, "duration_ms": duration_ms }),
            Self::QuestionResolved { question_id, resolution } =>
                json!({ "question_id": question_id, "resolution": resolution }),
            Self::MemoryPhaseChanged { slot_id, phase, active_type } =>
                json!({ "slot_id": slot_id, "phase": phase, "active_type": active_type }),
            Self::BoardTaskCreated { task_id, title, category } =>
                json!({ "task_id": task_id, "title": title, "category": category, "action": "created" }),
            Self::BoardTaskStatusChanged { task_id, old_status, new_status } =>
                json!({ "task_id": task_id, "old_status": old_status, "new_status": new_status, "action": "status_changed" }),
            Self::BoardTaskNoteAdded { task_id, note_id, content_preview } =>
                json!({ "task_id": task_id, "note_id": note_id, "content_preview": content_preview, "action": "note_added" }),
            Self::BoardTaskClaimed { task_id, slot_id } =>
                json!({ "task_id": task_id, "slot_id": slot_id, "action": "claimed" }),
            Self::BoardTaskDeleted { task_id, title } =>
                json!({ "task_id": task_id, "title": title, "action": "deleted" }),
            Self::BoardTaskUpdated { task_id, status, category } =>
                json!({ "task_id": task_id, "status": status, "category": category, "action": "updated" }),
            Self::SlotStateChanged { slot_id, new_state, prev_state } =>
                json!({ "slot_id": slot_id, "new_state": new_state, "prev_state": prev_state }),
            Self::InsightGenerated { category, priority, title } =>
                json!({ "category": category, "priority": priority, "title": title }),
            Self::GitCommitDetected { repo, hash, short_hash, author, message, committed_at } =>
                json!({ "repo": repo, "hash": hash, "short_hash": short_hash, "author": author, "message": message, "committed_at": committed_at }),
            Self::SlotTaskDispatched { slot_id, task_id, purpose, prompt_chars, preview } =>
                json!({ "slot_id": slot_id, "task_id": task_id, "purpose": purpose, "prompt_chars": prompt_chars, "preview": preview }),
            Self::ConversationMessageLogged { message_id, session_id, parent_session_id, slot_id, role, content_chars, preview } =>
                json!({ "message_id": message_id, "session_id": session_id, "parent_session_id": parent_session_id, "slot_id": slot_id, "role": role, "content_chars": content_chars, "preview": preview }),
            Self::CodexRequestStarted {
                request_id, caller, model, prompt_chars, has_image, image_hash, ..
            } => json!({
                "request_id": request_id,
                "caller": caller,
                "model": model,
                "prompt_chars": prompt_chars,
                "has_image": has_image,
                "image_hash": image_hash,
            }),
            Self::CodexRequestCompleted {
                request_id, caller, model, prompt_chars, response_chars,
                duration_ms, status, error_msg, input_tokens, output_tokens, image_hash, ..
            } => json!({
                "request_id": request_id,
                "caller": caller,
                "model": model,
                "prompt_chars": prompt_chars,
                "response_chars": response_chars,
                "duration_ms": duration_ms,
                "status": status,
                "error": error_msg,
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "image_hash": image_hash,
            }),
            Self::ToolCompleted {
                session_id, slot_id, tool_name, status,
                is_error, input_summary, output_summary,
            } => json!({
                "session_id": session_id,
                "slot_id": slot_id,
                "tool_name": tool_name,
                "status": status,
                "is_error": is_error,
                "input_summary": input_summary,
                "output_summary": output_summary,
            }),
            Self::ImageMessageInserted { message_id, session_id } =>
                json!({ "message_id": message_id, "session_id": session_id }),
            Self::BriefingBatchStarted { pending_count } =>
                json!({ "pending_count": pending_count }),
            Self::BriefingSummaryGenerated { target_seq, summary, method } =>
                json!({ "target_seq": target_seq, "summary": summary, "method": method }),
            Self::TranslationStarted { message_id, slot_id, content_chars } =>
                json!({ "message_id": message_id, "slot_id": slot_id, "content_chars": content_chars }),
            Self::TranslationCompleted { message_id, slot_id, preview, duration_ms } =>
                json!({ "message_id": message_id, "slot_id": slot_id, "preview": preview, "duration_ms": duration_ms }),
            Self::TranslationFailed { message_id, slot_id, error } =>
                json!({ "message_id": message_id, "slot_id": slot_id, "error": error }),
            Self::NarrationSessionStarted { session_id, total_messages, already_narrated } =>
                json!({ "session_id": session_id, "total_messages": total_messages, "already_narrated": already_narrated }),
            Self::NarrationBatchCompleted { session_id, batch_index, processed_count, total_messages, duration_ms } =>
                json!({ "session_id": session_id, "batch_index": batch_index, "processed_count": processed_count, "total_messages": total_messages, "duration_ms": duration_ms }),
            Self::NarrationSessionCompleted { session_id, total_narrated } =>
                json!({ "session_id": session_id, "total_narrated": total_narrated }),
            Self::NarrationFailed { session_id, batch_index, error, will_retry } =>
                json!({ "session_id": session_id, "batch_index": batch_index, "error": error, "will_retry": will_retry }),
            Self::WorkerLlmCall {
                caller, task_id, status, prompt_chars,
                response_chars, duration_ms, queue_wait_ms,
            } => json!({
                "caller": caller,
                "task_id": task_id,
                "status": status,
                "prompt_chars": prompt_chars,
                "response_chars": response_chars,
                "duration_ms": duration_ms,
                "queue_wait_ms": queue_wait_ms,
            }),
            Self::JarvisTaskCompleted { conversation_id, task_id } =>
                json!({ "conversation_id": conversation_id, "task_id": task_id }),
            Self::CliRequestStarted {
                engine, request_id, caller, session_id, model,
                prompt_chars, extra, ..
            } => {
                let mut v = json!({
                    "engine": engine.to_string(),
                    "request_id": request_id,
                    "caller": caller,
                    "session_id": session_id,
                    "model": model,
                    "prompt_chars": prompt_chars,
                });
                if let serde_json::Value::Object(map) = extra {
                    for (k, val) in map {
                        v[k] = val.clone();
                    }
                }
                v
            }
            Self::CliRequestCompleted {
                engine, request_id, caller, session_id, model,
                prompt_chars, response_chars, duration_ms,
                status, error_msg, extra, ..
            } => {
                let mut v = json!({
                    "engine": engine.to_string(),
                    "request_id": request_id,
                    "caller": caller,
                    "session_id": session_id,
                    "model": model,
                    "prompt_chars": prompt_chars,
                    "response_chars": response_chars,
                    "duration_ms": duration_ms,
                    "status": status,
                    "error": error_msg,
                });
                if let serde_json::Value::Object(map) = extra {
                    for (k, val) in map {
                        v[k] = val.clone();
                    }
                }
                v
            }
            Self::CliToolActivity {
                engine, request_id, tool_seq, activity, tool_name,
                input_preview, result_preview, is_error,
            } => json!({
                "engine": engine.to_string(),
                "request_id": request_id,
                "tool_seq": tool_seq,
                "activity": activity,
                "tool_name": tool_name,
                "input_preview": input_preview,
                "result_preview": result_preview,
                "is_error": is_error,
            }),
            Self::ConfigFileChanged { path, kind } => json!({
                "path": path,
                "kind": kind,
            }),
        }
    }
}

// ── Trace Context ──

/// Trace context for causal chain tracking (Phase 6 L2).
/// Will be used in Step 6c when trace/span chains are activated.
#[derive(Debug, Clone, Default)]
pub(crate) struct TraceContext {
    /// Root ID spanning an entire causal chain (e.g. conversation session ID).
    pub trace_id: Option<String>,
    /// This event's span ID. Auto-generated if None.
    pub span_id: Option<String>,
    /// Parent span ID — links child (branch) events to parent (main timeline).
    pub parent_span_id: Option<String>,
    /// Human-readable summary for L2 semantic layer.
    pub summary: Option<String>,
}

// ── Timeline Entry (MPSC input) ──

/// Entry flowing through the MPSC channel to the Timeline Writer.
#[derive(Debug)]
pub(crate) struct TimelineEntry {
    pub event: DaemonEvent,
    pub trace_id: Option<String>,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub summary: Option<String>,
}

// ── Timeline Event (broadcast output) ──

/// Event with persistent seq from SQLite, broadcast to all consumers.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct TimelineEvent {
    /// Global monotonic sequence number (from SQLite AUTOINCREMENT).
    pub seq: i64,
    /// Causal trace root ID.
    pub trace_id: Option<String>,
    /// This event's span ID.
    pub span_id: String,
    /// Parent span ID (None = main timeline event).
    pub parent_span_id: Option<String>,
    /// The original domain event.
    pub event: DaemonEvent,
    /// Human-readable summary.
    pub summary: Option<String>,
    /// Unix timestamp in milliseconds.
    pub ts: i64,
}

impl TimelineEvent {
    /// Serialize to frontend-compatible JSON wire format.
    pub fn to_frontend_json(&self) -> String {
        let msg = json!({
            "type": self.event.wire_type(),
            "ts": self.ts,
            "seq": self.seq,
            "trace_id": self.trace_id,
            "span_id": self.span_id,
            "parent_span_id": self.parent_span_id,
            "payload": self.event.to_frontend_payload(),
        });
        msg.to_string()
    }
}

// ── Event Bus ──

/// MPSC-based event bus for daemon inter-module communication.
///
/// Phase 6 architecture: events flow through unbounded MPSC to a single
/// Timeline Writer task, which persists to SQLite (getting monotonic seq)
/// and then broadcasts to all consumers.
///
/// Unbounded channel ensures events are never dropped (causal chain integrity).
/// Queue depth is naturally bounded by SQLite write throughput (>>10K TPS in WAL mode)
/// vs event production rate (~50/sec peak).
pub(crate) struct EventBus {
    tx: mpsc::UnboundedSender<TimelineEntry>,
    pub(crate) publish_count: AtomicU64,
}

impl EventBus {
    pub fn new(tx: mpsc::UnboundedSender<TimelineEntry>) -> Self {
        Self { tx, publish_count: AtomicU64::new(0) }
    }

    // @beacon: eventbus
    /// Publish an event without trace context (backward-compatible).
    /// All 15 existing call sites use this — no changes needed.
    pub fn publish(&self, event: DaemonEvent) {
        self.publish_count.fetch_add(1, Ordering::Relaxed);
        debug!(event = ?event, "EventBus: publish");
        let entry = TimelineEntry {
            event,
            trace_id: None,
            span_id: uuid::Uuid::new_v4().to_string(),
            parent_span_id: None,
            summary: None,
        };
        let _ = self.tx.send(entry);
    }

    /// Publish an event with trace context for causal chain tracking (Phase 6 L2).
    pub fn publish_traced(&self, event: DaemonEvent, ctx: TraceContext) {
        self.publish_count.fetch_add(1, Ordering::Relaxed);
        debug!(event = ?event, trace_id = ?ctx.trace_id, "EventBus: publish_traced");
        let entry = TimelineEntry {
            event,
            trace_id: ctx.trace_id,
            span_id: ctx.span_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            parent_span_id: ctx.parent_span_id,
            summary: ctx.summary,
        };
        let _ = self.tx.send(entry);
    }

    /// Get a clone of the MPSC sender (for GeminiClient and other components).
    pub fn sender(&self) -> mpsc::UnboundedSender<TimelineEntry> {
        self.tx.clone()
    }
}
