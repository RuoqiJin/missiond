//! Runtime actor command contracts for slot and extraction ownership.
//!
//! These contracts are intentionally small and side-effect free. Phase 1 code
//! can wrap the existing shared maps/atomics behind these handles before the
//! runtime moves to actor-owned state.

use tokio::sync::oneshot;

use crate::state::ExtractionPhase;

#[allow(dead_code)]
pub(crate) enum SlotActorCommand {
    EnsureReady {
        reply: oneshot::Sender<anyhow::Result<SlotActorSnapshot>>,
    },
    SendTask {
        task_id: String,
        prompt: String,
        timeout_ms: u64,
        reply: oneshot::Sender<anyhow::Result<SlotActorSnapshot>>,
    },
    Interrupt {
        reason: String,
        reply: oneshot::Sender<anyhow::Result<SlotActorSnapshot>>,
    },
    Restart {
        reason: String,
        reply: oneshot::Sender<anyhow::Result<SlotActorSnapshot>>,
    },
    Release {
        reason: String,
        reply: oneshot::Sender<anyhow::Result<SlotActorSnapshot>>,
    },
    Snapshot {
        reply: oneshot::Sender<SlotActorSnapshot>,
    },
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SlotActorSnapshot {
    pub(crate) slot_id: String,
    pub(crate) session_id: Option<String>,
    pub(crate) current_task_id: Option<String>,
    pub(crate) provider_conversation_id: Option<String>,
    pub(crate) status: String,
    pub(crate) blocked_reason: Option<String>,
    pub(crate) lease_expires_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExtractionLaneKind {
    Fast,
    Slow,
}

#[allow(dead_code)]
pub(crate) enum ExtractionLaneCommand {
    Begin {
        task_id: String,
        extraction_type: &'static str,
        reply: oneshot::Sender<anyhow::Result<ExtractionLaneSnapshot>>,
    },
    MarkPendingBatchServed {
        batch_id: String,
        payload: String,
        reply: oneshot::Sender<anyhow::Result<ExtractionLaneSnapshot>>,
    },
    Complete {
        output_count: u32,
        reply: oneshot::Sender<anyhow::Result<ExtractionLaneSnapshot>>,
    },
    Timeout {
        reason: String,
        reply: oneshot::Sender<anyhow::Result<ExtractionLaneSnapshot>>,
    },
    Snapshot {
        reply: oneshot::Sender<ExtractionLaneSnapshot>,
    },
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExtractionLaneSnapshot {
    pub(crate) lane: ExtractionLaneKind,
    pub(crate) phase: ExtractionPhase,
    pub(crate) active_type: Option<&'static str>,
    pub(crate) current_task_id: Option<String>,
    pub(crate) pending_batch_id: Option<String>,
    pub(crate) busy_since: i64,
    pub(crate) next_probe_after: i64,
    pub(crate) current_output_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_snapshots_are_serializable_contracts() {
        let slot = SlotActorSnapshot {
            slot_id: "slot-coder-1".to_string(),
            session_id: Some("session-1".to_string()),
            current_task_id: Some("task-1".to_string()),
            provider_conversation_id: None,
            status: "idle".to_string(),
            blocked_reason: None,
            lease_expires_at: None,
        };
        let json = serde_json::to_value(slot).expect("slot snapshot json");
        assert_eq!(json["slotId"], "slot-coder-1");

        let lane = ExtractionLaneSnapshot {
            lane: ExtractionLaneKind::Fast,
            phase: ExtractionPhase::Idle,
            active_type: None,
            current_task_id: None,
            pending_batch_id: None,
            busy_since: 0,
            next_probe_after: 0,
            current_output_count: 0,
        };
        let json = serde_json::to_value(lane).expect("lane snapshot json");
        assert_eq!(json["lane"], "fast");
    }
}
