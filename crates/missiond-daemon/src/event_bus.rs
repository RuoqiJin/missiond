//! Event Bus — centralized event dispatch for daemon module decoupling.
//!
//! Replaces scattered `tokio::sync::Notify` signals with a typed event system.
//! All inter-module communication goes through DaemonEvent publish/subscribe.

use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::broadcast;
use tracing::debug;

/// Domain events that flow between daemon modules.
#[derive(Debug, Clone)]
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
}

/// Broadcast-based event bus for daemon inter-module communication.
///
/// Capacity is set at construction time. If a subscriber falls behind,
/// it will receive `RecvError::Lagged` and skip missed events.
pub(crate) struct EventBus {
    tx: broadcast::Sender<DaemonEvent>,
    pub(crate) publish_count: AtomicU64,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx, publish_count: AtomicU64::new(0) }
    }

    /// Publish an event to all subscribers. Silently drops if no subscribers.
    pub fn publish(&self, event: DaemonEvent) {
        self.publish_count.fetch_add(1, Ordering::Relaxed);
        debug!(event = ?event, "EventBus: publish");
        let _ = self.tx.send(event);
    }

    /// Create a new subscriber. Each subscriber has an independent receive buffer.
    pub fn subscribe(&self) -> broadcast::Receiver<DaemonEvent> {
        self.tx.subscribe()
    }

    /// Get a clone of the sender (for passing to components that emit events).
    pub fn sender(&self) -> broadcast::Sender<DaemonEvent> {
        self.tx.clone()
    }
}
