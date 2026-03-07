//! Event Bus — centralized event dispatch for daemon module decoupling.
//!
//! Phase 6: Cognitive Timeline architecture.
//! All events flow through MPSC → Timeline Writer → SQLite → broadcast<TimelineEvent>.
//! This guarantees: persistent storage, global monotonic seq, causal ordering.

use std::sync::atomic::{AtomicU64, Ordering};
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

    // ===== LLM Gateway =====
    /// A Gemini API request completed (success or failure).
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
    /// Board task created/updated/toggled.
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

    // ===== Vision Worker =====
    /// A message with image content was inserted, needs async vision processing.
    ImageMessageInserted {
        message_id: i64,
        session_id: String,
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
            Self::GeminiRequestCompleted { .. } => "gemini_request_completed",
            Self::DecisionResolved { .. } => "decision_made",
            Self::QuestionResolved { .. } => "question_resolved",
            Self::MemoryPhaseChanged { .. } => "memory_phase_changed",
            Self::BoardTaskUpdated { .. } => "board_task_updated",
            Self::SlotStateChanged { .. } => "slot_state_changed",
            Self::InsightGenerated { .. } => "insight_generated",
            Self::ImageMessageInserted { .. } => "image_message_inserted",
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
            Self::GeminiRequestCompleted {
                request_id, caller, session_id, api_mode, model,
                prompt_chars, response_chars, queue_wait_ms,
                duration_ms, retry_count, status, error_msg,
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
            Self::DecisionResolved { question_id, tier, duration_ms } =>
                json!({ "question_id": question_id, "tier": tier, "duration_ms": duration_ms }),
            Self::QuestionResolved { question_id, resolution } =>
                json!({ "question_id": question_id, "resolution": resolution }),
            Self::MemoryPhaseChanged { slot_id, phase, active_type } =>
                json!({ "slot_id": slot_id, "phase": phase, "active_type": active_type }),
            Self::BoardTaskUpdated { task_id, status, category } =>
                json!({ "task_id": task_id, "status": status, "category": category }),
            Self::SlotStateChanged { slot_id, new_state, prev_state } =>
                json!({ "slot_id": slot_id, "new_state": new_state, "prev_state": prev_state }),
            Self::InsightGenerated { category, priority, title } =>
                json!({ "category": category, "priority": priority, "title": title }),
            Self::ImageMessageInserted { message_id, session_id } =>
                json!({ "message_id": message_id, "session_id": session_id }),
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
