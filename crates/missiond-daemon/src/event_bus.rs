//! Event Bus — centralized event dispatch for daemon module decoupling.
//!
//! Replaces scattered `tokio::sync::Notify` signals with a typed event system.
//! All inter-module communication goes through DaemonEvent publish/subscribe.

use tokio::sync::broadcast;
use tracing::debug;

/// Domain events that flow between daemon modules.
#[derive(Debug, Clone)]
pub(crate) enum DaemonEvent {
    // ===== Slot lifecycle =====
    /// A slot transitioned to Idle (extraction/submit can trigger).
    SlotBecameIdle { slot_id: String },
    /// A slot transitioned away from Idle.
    SlotBecameBusy { slot_id: String },

    // ===== Extraction pipeline =====
    /// Extraction completed successfully on a lane.
    ExtractionCompleted { lane: &'static str },

    // ===== Task scheduling =====
    /// A new submit task was created and queued.
    TaskCreated { task_id: String },
    /// A submit task completed (slot returned to idle).
    TaskCompleted { task_id: String },

    // ===== Decision engine =====
    /// A new master-target question was created.
    QuestionCreated { question_id: String },

    // ===== Memory system =====
    /// Memory extraction paused.
    MemoryPaused,
    /// Memory extraction resumed.
    MemoryResumed,

    // ===== Health / AIOps =====
    /// An incident was detected by health scan or PTY error detection.
    IncidentDetected { incident_id: String },
}

/// Broadcast-based event bus for daemon inter-module communication.
///
/// Capacity is set at construction time. If a subscriber falls behind,
/// it will receive `RecvError::Lagged` and skip missed events.
pub(crate) struct EventBus {
    tx: broadcast::Sender<DaemonEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Publish an event to all subscribers. Silently drops if no subscribers.
    pub fn publish(&self, event: DaemonEvent) {
        debug!(event = ?event, "EventBus: publish");
        let _ = self.tx.send(event);
    }

    /// Create a new subscriber. Each subscriber has an independent receive buffer.
    pub fn subscribe(&self) -> broadcast::Receiver<DaemonEvent> {
        self.tx.subscribe()
    }
}
