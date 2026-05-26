//! Runtime actor command contracts for slot and extraction ownership.
//!
//! These contracts are intentionally small and side-effect free. Phase 1 code
//! can wrap the existing shared maps/atomics behind these handles before the
//! runtime moves to actor-owned state.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use missiond_core::PTYManager;
use missiond_domain::ids::SlotId;
use tokio::sync::oneshot;

use crate::state::{ExtractionPhase, ExtractionState};

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

#[derive(Clone)]
pub(crate) struct SlotActorHandle {
    slot_id: SlotId,
    pty: Arc<PTYManager>,
}

impl SlotActorHandle {
    pub(crate) fn new(slot_id: impl Into<String>, pty: Arc<PTYManager>) -> Self {
        Self {
            slot_id: SlotId::new(slot_id),
            pty,
        }
    }

    pub(crate) async fn snapshot(&self) -> SlotActorSnapshot {
        match self.pty.get_status(self.slot_id.as_str()).await {
            Some(info) => SlotActorSnapshot {
                slot_id: info.slot_id,
                session_id: None,
                current_task_id: info.current_task_id,
                provider_conversation_id: None,
                status: format!("{:?}", info.state),
                blocked_reason: None,
                lease_expires_at: None,
            },
            None => SlotActorSnapshot {
                slot_id: self.slot_id.to_string(),
                session_id: None,
                current_task_id: None,
                provider_conversation_id: None,
                status: "missing".to_string(),
                blocked_reason: Some("slot runtime not found".to_string()),
                lease_expires_at: None,
            },
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn handle_command(&self, command: SlotActorCommand) {
        match command {
            SlotActorCommand::EnsureReady { reply }
            | SlotActorCommand::Interrupt { reply, .. }
            | SlotActorCommand::Restart { reply, .. }
            | SlotActorCommand::Release { reply, .. } => {
                let _ = reply.send(Ok(self.snapshot().await));
            }
            SlotActorCommand::Snapshot { reply } => {
                let _ = reply.send(self.snapshot().await);
            }
            SlotActorCommand::SendTask {
                task_id,
                prompt,
                timeout_ms,
                reply,
            } => {
                let result = self
                    .pty
                    .send(self.slot_id.as_str(), &prompt, timeout_ms)
                    .await
                    .map(|_| ());
                let snapshot = self.snapshot().await;
                let _ = match result {
                    Ok(()) => reply.send(Ok(SlotActorSnapshot {
                        current_task_id: Some(task_id),
                        ..snapshot
                    })),
                    Err(err) => reply.send(Err(err)),
                };
            }
        }
    }
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

#[derive(Clone)]
pub(crate) struct ExtractionLaneHandle {
    lane: ExtractionLaneKind,
    state: Arc<tokio::sync::RwLock<ExtractionState>>,
    busy_since: Arc<AtomicI64>,
}

impl ExtractionLaneHandle {
    pub(crate) fn new(
        lane: ExtractionLaneKind,
        state: Arc<tokio::sync::RwLock<ExtractionState>>,
        busy_since: Arc<AtomicI64>,
    ) -> Self {
        Self {
            lane,
            state,
            busy_since,
        }
    }

    pub(crate) async fn snapshot(&self) -> ExtractionLaneSnapshot {
        let es = self.state.read().await;
        ExtractionLaneSnapshot::from_state(self.lane, &es, self.busy_since())
    }

    #[allow(dead_code)]
    pub(crate) async fn begin(
        &self,
        task_id: String,
        extraction_type: &'static str,
    ) -> anyhow::Result<ExtractionLaneSnapshot> {
        let now = chrono::Utc::now().timestamp();
        {
            let mut es = self.state.write().await;
            es.phase = ExtractionPhase::Sending;
            es.active_type = Some(extraction_type);
            es.phase_started_at = now;
            es.current_task_id = Some(task_id);
            es.reset_current_output_count();
            es.clear_pending_batch_replay();
        }
        self.busy_since.store(now, Ordering::SeqCst);
        Ok(self.snapshot().await)
    }

    #[allow(dead_code)]
    pub(crate) async fn mark_pending_batch_served(
        &self,
        batch_id: String,
        payload: String,
    ) -> anyhow::Result<ExtractionLaneSnapshot> {
        {
            let mut es = self.state.write().await;
            es.mark_pending_batch_served(batch_id, payload);
        }
        Ok(self.snapshot().await)
    }

    #[allow(dead_code)]
    pub(crate) async fn complete(
        &self,
        output_count: u32,
    ) -> anyhow::Result<ExtractionLaneSnapshot> {
        {
            let mut es = self.state.write().await;
            es.phase = ExtractionPhase::Idle;
            es.active_type = None;
            es.current_task_id = None;
            es.current_slot_task_id = None;
            es.current_output_count = output_count;
            es.clear_pending_batch_replay();
        }
        self.busy_since.store(0, Ordering::SeqCst);
        Ok(self.snapshot().await)
    }

    #[allow(dead_code)]
    pub(crate) async fn timeout(&self, _reason: String) -> anyhow::Result<ExtractionLaneSnapshot> {
        {
            let mut es = self.state.write().await;
            es.phase = ExtractionPhase::Idle;
            es.active_type = None;
            es.current_task_id = None;
            es.current_slot_task_id = None;
            es.clear_pending_batch_replay();
        }
        self.busy_since.store(0, Ordering::SeqCst);
        Ok(self.snapshot().await)
    }

    #[allow(dead_code)]
    pub(crate) async fn handle_command(&self, command: ExtractionLaneCommand) {
        match command {
            ExtractionLaneCommand::Begin {
                task_id,
                extraction_type,
                reply,
            } => {
                let _ = reply.send(self.begin(task_id, extraction_type).await);
            }
            ExtractionLaneCommand::MarkPendingBatchServed {
                batch_id,
                payload,
                reply,
            } => {
                let _ = reply.send(self.mark_pending_batch_served(batch_id, payload).await);
            }
            ExtractionLaneCommand::Complete {
                output_count,
                reply,
            } => {
                let _ = reply.send(self.complete(output_count).await);
            }
            ExtractionLaneCommand::Timeout { reason, reply } => {
                let _ = reply.send(self.timeout(reason).await);
            }
            ExtractionLaneCommand::Snapshot { reply } => {
                let _ = reply.send(self.snapshot().await);
            }
        }
    }

    fn busy_since(&self) -> i64 {
        self.busy_since.load(Ordering::SeqCst)
    }
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

impl ExtractionLaneSnapshot {
    pub(crate) fn from_state(
        lane: ExtractionLaneKind,
        state: &ExtractionState,
        busy_since: i64,
    ) -> Self {
        Self {
            lane,
            phase: state.phase,
            active_type: state.active_type,
            current_task_id: state.current_task_id.clone(),
            pending_batch_id: state.pending_batch_id.clone(),
            busy_since,
            next_probe_after: state.next_probe_after,
            current_output_count: state.current_output_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn extraction_state() -> ExtractionState {
        ExtractionState {
            phase: ExtractionPhase::Idle,
            active_type: None,
            phase_started_at: 0,
            current_deep_conv_id: None,
            watermark_targets: Vec::new(),
            current_task_id: None,
            current_slot_task_id: None,
            is_checkpoint: false,
            checkpoint_message_id: None,
            pending_served: false,
            pending_batch_id: None,
            pending_payload: None,
            pending_served_at: 0,
            pending_replay_count: 0,
            empty_probe_count: 0,
            next_probe_after: 0,
            current_output_count: 0,
            deep_analysis_zero_output_count: 0,
            deep_analysis_fuse_until: 0,
            input_skip_diagnostics: HashMap::new(),
        }
    }

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

    #[tokio::test]
    async fn extraction_lane_handle_owns_snapshot_transitions() {
        let handle = ExtractionLaneHandle::new(
            ExtractionLaneKind::Slow,
            Arc::new(tokio::sync::RwLock::new(extraction_state())),
            Arc::new(AtomicI64::new(0)),
        );
        let started = handle
            .begin("task-1".to_string(), "deep_analysis")
            .await
            .expect("begin");
        assert_eq!(started.phase, ExtractionPhase::Sending);
        assert_eq!(started.current_task_id.as_deref(), Some("task-1"));
        assert!(started.busy_since > 0);

        let completed = handle.complete(3).await.expect("complete");
        assert_eq!(completed.phase, ExtractionPhase::Idle);
        assert_eq!(completed.current_output_count, 3);
        assert_eq!(completed.busy_since, 0);
    }
}
