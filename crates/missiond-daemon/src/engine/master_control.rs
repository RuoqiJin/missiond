use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use missiond_core::event::events::{BoardEvent, IncidentEvent, QuestionEvent, SlotEvent};
use missiond_core::event::subscription::{CursorFlush, StartFrom, SubscriptionOpts};
use missiond_core::event::DomainEvent;
use missiond_core::types::{
    BoardTask, BoardTaskStatus, CreateBoardTaskInput, CONVERSATION_SOURCE_LABEL_CODEX_LOCAL_INDEX,
};
use missiond_core::{PTYSlot, PTYSpawnOptions, SessionState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::process::Command;
use tokio::sync::{watch, Notify, RwLock};
use tracing::{info, warn};

use crate::bus::BusServices;
use crate::context::v3_blueprint_runtime::{
    compiled_runtime_projection_status, WorkstationRuntimeConfig,
};
use crate::control_tree::CtlDomain;
use crate::state::AppState;

pub(crate) const MASTER_WORKER_ID: &str = "codex-master-control";
const MASTER_WORKER_LEGACY_AUTHOR_IDS: &[&str] = &["resident-codex-master", "resident-master"];
pub(crate) const MASTER_SLOT_ID: &str = "slot-codex-master-control";
pub(crate) const CHECKPOINT_RELATIVE_PATH: &str =
    ".missiond/v3/runtime/master-control-checkpoint.lisp";
const CHECKPOINT_RUNTIME_PATH: &str = "master-control-checkpoint.lisp";
const MASTER_CONTEXT_PACK_RUNTIME_DIR: &str = "master-control/context-packs";
const MASTER_SLOT_READY_TIMEOUT_SECS: u64 = 180;
const MASTER_ACTIVE_OBJECTIVE_HEARTBEAT_SECS: i64 = 900;
const MASTER_EVENT_SUBSCRIBER_CONSUMER: &str = "master_event_subscriber";
const MASTER_BOARD_SUBSCRIPTION: &str = "master_event_subscriber_board_v2_live";
const MASTER_SLOT_SUBSCRIPTION: &str = "master_event_subscriber_slot_v2_live";
const MASTER_QUESTION_SUBSCRIPTION: &str = "master_event_subscriber_question_v2_live";
/// Live IncidentEvent subscription used by master_control to wake on
/// `claude_code_mcp_missing` and `claude_code_mcp_reconnect_failed`
/// incidents; pinned by `claude-code-mcp-recovery :wake-resident-master`
/// so MCP recovery does not depend on the operator noticing PTY output.
const MASTER_INCIDENT_SUBSCRIPTION: &str = "master_event_subscriber_incident_v2_live";
const MASTER_MCP_APPROVED_TOOLS: &[&str] = &[
    "mission_agent",
    "mission_audit",
    "mission_tool_directory",
    "mission_agent_navigation",
    "mission_beacon",
    "mission_intent",
    "mission_board_query",
    "mission_board_create",
    "mission_board_update",
    "mission_board_delete",
    "mission_board_claim",
    "mission_board_note_add",
    "mission_board_decompose",
    "mission_board_retry",
    "mission_submit_phase_result",
    "mission_conversation_query",
    "mission_conversation_analyze",
    "mission_conversation_reconcile",
    "mission_retrospective_manage",
    "mission_embedding_ops",
    "mission_timeline",
    "mission_router_chat",
    "mission_router_chat_manage",
    "mission_question",
    "mission_llm_trace",
    "mission_decision_stats",
    "mission_gemini_auth",
    "mission_incident",
    "mission_codex_ops",
    "mission_capability_usage",
    "mission_kb_query",
    "mission_kb_remember",
    "mission_kb_mutate",
    "mission_kb_ops",
    "mission_code_search",
    "mission_skill_query",
    "mission_skill_context",
    "mission_skill_mutate",
    "mission_skill_exec",
    "mission_memory",
    "mission_insight",
    "mission_universe_graph",
    "mission_cascade_plan",
    "mission_cascade_trigger",
    "mission_cascade_lint",
    "mission_request",
    "mission_directive",
    "mission_plan",
    "mission_workflow",
    "mission_project",
    "mission_execution",
    "mission_task_delegate",
    "mission_swarm_run",
    "mission_task_submit",
    "mission_task_query",
    "mission_task_cancel",
    "mission_pty_spawn",
    "mission_pty_send",
    "mission_pty_read",
    "mission_pty_signal",
    "mission_pty_confirm",
    "mission_pty_status",
    "mission_pty_screenshot",
    "mission_job_poll",
    "mission_cc_query",
    "mission_cc_swarm",
    "mission_compute_slot",
    "mission_slot_history",
    "mission_pause",
    "mission_flow_run",
    "mission_forge_build",
    "mission_forge_lint",
    "mission_worker",
    "mission_control",
    "mission_minimax_process",
    "mission_sonnet_process",
    "mission_master_status",
    "mission_convergence_status",
    "mission_nightly_evolution",
    "mission_inbox",
    "mission_slots",
    "mission_infra_query",
    "mission_infra_ops",
    "mission_permission_query",
    "mission_permission_mutate",
    "mission_power_control",
    "mission_sys_logs",
    "mission_sys_config",
    "mission_global_instruction",
];
const MASTER_PHASES: &[&str] = &[
    "observe_event",
    "classify_objective",
    "create_context_pack",
    "dispatch_investigators",
    "compile_shards",
    "dispatch_implementers",
    "verify",
    "close_or_backfill",
    "blocked",
];

static MASTER_CONTROL_RUNTIME: OnceLock<Arc<MasterControlRuntime>> = OnceLock::new();

#[derive(Debug)]
pub(crate) struct MasterControlRuntime {
    queued_events: AtomicU64,
    processed_ticks: AtomicU64,
    last_event_seq: AtomicI64,
    last_checkpoint_at_epoch: AtomicI64,
    drift_backfill_tasks_created: AtomicU64,
    control_turns_sent: AtomicU64,
    last_control_turn_at_epoch: AtomicI64,
    last_control_objective_id: RwLock<Option<String>>,
    notify: Notify,
    last_event_cursor: RwLock<Option<String>>,
    last_event_summary: RwLock<Option<String>>,
    last_tick_id: RwLock<Option<String>>,
    blocked_reason: RwLock<Option<String>>,
    last_mcp_ready: RwLock<Option<bool>>,
    last_control_turn_error: RwLock<Option<String>>,
    last_drift_backfill_task_id: RwLock<Option<String>>,
    active_objective_id: RwLock<Option<String>>,
    phase: RwLock<String>,
    context_pack_path: RwLock<Option<String>>,
    delegated_task_ids: RwLock<Vec<String>>,
    last_verified_commit: RwLock<Option<String>>,
    resume_instruction: RwLock<String>,
}

impl Default for MasterControlRuntime {
    fn default() -> Self {
        Self {
            queued_events: AtomicU64::new(0),
            processed_ticks: AtomicU64::new(0),
            last_event_seq: AtomicI64::new(0),
            last_checkpoint_at_epoch: AtomicI64::new(0),
            drift_backfill_tasks_created: AtomicU64::new(0),
            control_turns_sent: AtomicU64::new(0),
            last_control_turn_at_epoch: AtomicI64::new(0),
            last_control_objective_id: RwLock::new(None),
            notify: Notify::new(),
            last_event_cursor: RwLock::new(None),
            last_event_summary: RwLock::new(None),
            last_tick_id: RwLock::new(None),
            blocked_reason: RwLock::new(None),
            last_mcp_ready: RwLock::new(None),
            last_control_turn_error: RwLock::new(None),
            last_drift_backfill_task_id: RwLock::new(None),
            active_objective_id: RwLock::new(None),
            phase: RwLock::new("observe_event".to_string()),
            context_pack_path: RwLock::new(None),
            delegated_task_ids: RwLock::new(Vec::new()),
            last_verified_commit: RwLock::new(None),
            resume_instruction: RwLock::new(
                "read checkpoint, inspect Board/event/provider evidence, then resume at phase"
                    .to_string(),
            ),
        }
    }
}

impl MasterControlRuntime {
    async fn record_wakeup(&self, domain: &str, kind: &str, seq: i64, preview: String) {
        self.queued_events.fetch_add(1, Ordering::Relaxed);
        self.last_event_seq.store(seq, Ordering::Relaxed);
        *self.last_event_cursor.write().await = Some(format!("{domain}:{seq}"));
        *self.last_event_summary.write().await = Some(format!("{domain}.{kind}: {preview}"));
        self.notify.notify_one();
    }

    async fn record_checkpoint_context(&self, domain: &str, kind: &str, seq: i64, preview: String) {
        self.last_event_seq.store(seq, Ordering::Relaxed);
        *self.last_event_cursor.write().await = Some(format!("{domain}:{seq}"));
        *self.last_event_summary.write().await = Some(format!("{domain}.{kind}: {preview}"));
    }

    async fn mark_tick(
        &self,
        reason: &str,
        mcp_ready: bool,
        decision: &MasterDecisionState,
    ) -> String {
        let tick = self.processed_ticks.fetch_add(1, Ordering::Relaxed) + 1;
        let tick_id = format!("master-tick-{tick:012}");
        *self.last_tick_id.write().await = Some(tick_id.clone());
        *self.last_mcp_ready.write().await = Some(mcp_ready);
        *self.blocked_reason.write().await = if mcp_ready {
            None
        } else {
            Some("codex missiond MCP not ready".to_string())
        };
        *self.active_objective_id.write().await = decision.active_objective_id.clone();
        *self.phase.write().await = decision.phase.clone();
        *self.context_pack_path.write().await = decision.context_pack_path.clone();
        *self.delegated_task_ids.write().await = decision.delegated_task_ids.clone();
        *self.last_verified_commit.write().await = decision.last_verified_commit.clone();
        *self.resume_instruction.write().await = decision.resume_instruction.clone();
        self.last_checkpoint_at_epoch
            .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
        tracing::debug!(tick_id = %tick_id, reason = reason, "master-control tick recorded");
        tick_id
    }

    async fn record_control_turn(
        &self,
        result: Result<(), String>,
        active_objective_id: Option<String>,
    ) {
        match result {
            Ok(()) => {
                self.control_turns_sent.fetch_add(1, Ordering::Relaxed);
                self.queued_events.store(0, Ordering::Relaxed);
                self.last_control_turn_at_epoch
                    .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
                *self.last_control_objective_id.write().await = active_objective_id;
                *self.last_control_turn_error.write().await = None;
            }
            Err(err) => {
                *self.last_control_turn_error.write().await = Some(err);
            }
        }
    }

    fn clear_queued_events(&self) {
        self.queued_events.store(0, Ordering::Relaxed);
    }

    pub(crate) async fn snapshot(&self) -> MasterControlRuntimeSnapshot {
        MasterControlRuntimeSnapshot {
            queued_events: self.queued_events.load(Ordering::Relaxed),
            processed_ticks: self.processed_ticks.load(Ordering::Relaxed),
            last_event_seq: self.last_event_seq.load(Ordering::Relaxed),
            last_checkpoint_at_epoch: self.last_checkpoint_at_epoch.load(Ordering::Relaxed),
            drift_backfill_tasks_created: self.drift_backfill_tasks_created.load(Ordering::Relaxed),
            control_turns_sent: self.control_turns_sent.load(Ordering::Relaxed),
            last_control_turn_at_epoch: self.last_control_turn_at_epoch.load(Ordering::Relaxed),
            last_control_objective_id: self.last_control_objective_id.read().await.clone(),
            last_event_cursor: self.last_event_cursor.read().await.clone(),
            last_event_summary: self.last_event_summary.read().await.clone(),
            last_tick_id: self.last_tick_id.read().await.clone(),
            blocked_reason: self.blocked_reason.read().await.clone(),
            last_mcp_ready: *self.last_mcp_ready.read().await,
            last_control_turn_error: self.last_control_turn_error.read().await.clone(),
            last_drift_backfill_task_id: self.last_drift_backfill_task_id.read().await.clone(),
            active_objective_id: self.active_objective_id.read().await.clone(),
            phase: self.phase.read().await.clone(),
            context_pack_path: self.context_pack_path.read().await.clone(),
            delegated_task_ids: self.delegated_task_ids.read().await.clone(),
            last_verified_commit: self.last_verified_commit.read().await.clone(),
            resume_instruction: self.resume_instruction.read().await.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MasterControlRuntimeSnapshot {
    pub(crate) queued_events: u64,
    pub(crate) processed_ticks: u64,
    pub(crate) last_event_seq: i64,
    pub(crate) last_checkpoint_at_epoch: i64,
    pub(crate) drift_backfill_tasks_created: u64,
    pub(crate) control_turns_sent: u64,
    pub(crate) last_control_turn_at_epoch: i64,
    pub(crate) last_control_objective_id: Option<String>,
    pub(crate) last_event_cursor: Option<String>,
    pub(crate) last_event_summary: Option<String>,
    pub(crate) last_tick_id: Option<String>,
    pub(crate) blocked_reason: Option<String>,
    pub(crate) last_mcp_ready: Option<bool>,
    pub(crate) last_control_turn_error: Option<String>,
    pub(crate) last_drift_backfill_task_id: Option<String>,
    pub(crate) active_objective_id: Option<String>,
    pub(crate) phase: String,
    pub(crate) context_pack_path: Option<String>,
    pub(crate) delegated_task_ids: Vec<String>,
    pub(crate) last_verified_commit: Option<String>,
    pub(crate) resume_instruction: String,
}

pub(crate) fn runtime() -> Arc<MasterControlRuntime> {
    MASTER_CONTROL_RUNTIME
        .get_or_init(|| Arc::new(MasterControlRuntime::default()))
        .clone()
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum WorkerCompletionEvidence {
    ProviderDurableLog {
        provider: &'static str,
        conversation_id: String,
    },
    MissionEvent {
        cursor: String,
    },
    BoardLifecycle {
        task_id: String,
        status: String,
    },
    PtyDiagnostic {
        provider: &'static str,
        state: String,
        confidence: f64,
    },
}

impl WorkerCompletionEvidence {
    pub(crate) fn authority_tier(&self) -> &'static str {
        match self {
            Self::ProviderDurableLog { .. } => "t1-provider-durable",
            Self::MissionEvent { .. } | Self::BoardLifecycle { .. } => "t2-mission-state",
            Self::PtyDiagnostic { .. } => "t3-diagnostic-only",
        }
    }
}

pub(crate) struct MasterControlService {
    bus: Arc<BusServices>,
    state: AppState,
    runtime: Arc<MasterControlRuntime>,
}

#[derive(Debug, Clone)]
struct MasterDecisionState {
    phase: String,
    active_objective_id: Option<String>,
    context_pack_path: Option<String>,
    delegated_task_ids: Vec<String>,
    last_verified_commit: Option<String>,
    resume_instruction: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveObjectivePromptContext {
    id: String,
    title: String,
    status: String,
    project: Option<String>,
    parent_id: Option<String>,
    description_excerpt: String,
}

impl ActiveObjectivePromptContext {
    fn from_task(task: &BoardTask) -> Self {
        Self {
            id: task.id.to_string(),
            title: task.title.clone(),
            status: task.status.as_str().to_string(),
            project: task.project.clone(),
            parent_id: task.parent_id.as_ref().map(ToString::to_string),
            description_excerpt: truncate_for_prompt(&task.description, 1200),
        }
    }
}

impl MasterControlService {
    pub(crate) fn new(
        bus: Arc<BusServices>,
        state: AppState,
        runtime: Arc<MasterControlRuntime>,
    ) -> Self {
        Self {
            bus,
            state,
            runtime,
        }
    }

    pub(crate) fn spawn(self, shutdown: watch::Receiver<bool>) {
        let service = Arc::new(self);
        spawn_master_event_subscriber(service.clone(), shutdown.clone());
        spawn_master_decision_loop(service, shutdown);
    }

    async fn tick(&self, reason: &str) {
        if self.master_control_paused() {
            let snapshot = self.runtime.snapshot().await;
            let tick_id = self
                .runtime
                .mark_tick(
                    reason,
                    false,
                    &MasterDecisionState {
                        phase: "paused".to_string(),
                        active_objective_id: snapshot.active_objective_id,
                        context_pack_path: None,
                        delegated_task_ids: Vec::new(),
                        last_verified_commit: None,
                        resume_instruction:
                            "master-control paused by operator; do not dispatch self-evolution"
                                .to_string(),
                    },
                )
                .await;
            *self.runtime.blocked_reason.write().await = Some(
                "mission_control paused strategy domain or orchestrator slot_role".to_string(),
            );
            if let Err(err) = self.write_checkpoint(reason, &tick_id, false).await {
                warn!(error = %err, "master-control paused checkpoint write failed");
            }
            return;
        }
        if matches!(reason, "daemon-startup" | "event-wakeup") {
            match self.ensure_code_drift_backfill_task().await {
                Ok(Some(task_id)) => {
                    self.runtime
                        .drift_backfill_tasks_created
                        .fetch_add(1, Ordering::Relaxed);
                    *self.runtime.last_drift_backfill_task_id.write().await = Some(task_id);
                }
                Ok(None) => {}
                Err(err) => warn!(error = %err, "code-drift backfill reconciliation failed"),
            }
        }
        if reason == "daemon-startup" {
            if let Err(err) = self.recover_open_master_objective().await {
                warn!(error = %err, "master-control open objective recovery failed");
            }
        }
        let mcp_ready = probe_codex_mcp_control_ready().await;
        let pre_tick_snapshot = self
            .snapshot_with_live_active_objective(self.runtime.snapshot().await)
            .await;
        let root = master_project_root(&self.state);
        let decision = classify_master_decision_state(reason, &pre_tick_snapshot, &root).await;
        let tick_id = self.runtime.mark_tick(reason, mcp_ready, &decision).await;
        if should_consume_event_without_control(reason, &pre_tick_snapshot, &decision) {
            self.runtime.clear_queued_events();
        }
        if let Err(err) = self
            .ensure_context_pack_materialized(&decision, reason, &tick_id)
            .await
        {
            warn!(error = %err, "master-control context-pack materialization failed");
        }
        if let Err(err) = self.write_checkpoint(reason, &tick_id, mcp_ready).await {
            warn!(error = %err, "master-control checkpoint write failed");
        }
        if reason == "daemon-startup" && mcp_ready {
            if let Err(err) = self.ensure_master_slot_running().await {
                warn!(error = %err, "master-control resident slot auto-start failed");
            }
        }
        if should_dispatch_control_turn(reason, &self.runtime.snapshot().await, mcp_ready) {
            let snapshot = self.runtime.snapshot().await;
            let active_objective = self.active_objective_prompt_context(&snapshot).await;
            let prompt =
                build_master_tick_prompt(&snapshot, reason, mcp_ready, active_objective.as_ref());
            let active_objective_id = snapshot.active_objective_id.clone();
            let result = self.dispatch_control_turn(&prompt).await;
            self.runtime
                .record_control_turn(result, active_objective_id)
                .await;
        }
    }

    fn master_control_paused(&self) -> bool {
        let tree = self.state.control_manager.current();
        tree.is_domain_paused(CtlDomain::Strategy) || tree.is_slot_role_paused("orchestrator")
    }

    async fn ensure_code_drift_backfill_task(&self) -> anyhow::Result<Option<String>> {
        ensure_code_drift_backfill_task_for_state(&self.state).await
    }

    async fn ensure_context_pack_materialized(
        &self,
        decision: &MasterDecisionState,
        reason: &str,
        tick_id: &str,
    ) -> std::io::Result<()> {
        let Some(context_pack_path) = decision.context_pack_path.as_deref() else {
            return Ok(());
        };
        let root = master_project_root(&self.state);
        let path = root.join(context_pack_path);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let snapshot = self.runtime.snapshot().await;
        tokio::fs::write(
            path,
            render_master_context_pack(&MasterContextPackRender {
                reason,
                tick_id,
                snapshot: &snapshot,
            }),
        )
        .await
    }

    async fn snapshot_with_live_active_objective(
        &self,
        mut snapshot: MasterControlRuntimeSnapshot,
    ) -> MasterControlRuntimeSnapshot {
        let Some(active_id) = snapshot.active_objective_id.clone() else {
            return snapshot;
        };
        match self.state.store.get_board_task(&active_id).await {
            Ok(Some(task)) if is_terminal_board_task_status(&task.status) => {
                let resume_instruction = format!(
                    "active objective {} is terminal ({:?}); observe next durable event",
                    active_id, task.status
                );
                let event_summary = format!(
                    "BoardEvent.status_changed: task_id={} {:?}->terminal",
                    active_id, task.status
                );
                snapshot.active_objective_id = None;
                snapshot.phase = "observe_event".to_string();
                snapshot.context_pack_path = None;
                snapshot.resume_instruction = resume_instruction.clone();
                snapshot.last_event_summary = Some(event_summary.clone());
                *self.runtime.active_objective_id.write().await = None;
                *self.runtime.phase.write().await = "observe_event".to_string();
                *self.runtime.context_pack_path.write().await = None;
                *self.runtime.resume_instruction.write().await = resume_instruction;
                *self.runtime.last_event_summary.write().await = Some(event_summary);
            }
            Ok(Some(_)) => {}
            Ok(None) => {
                let resume_instruction = format!(
                    "active objective {} no longer exists; observe next durable event",
                    active_id
                );
                let event_summary = format!("BoardEvent.deleted: task_id={} -> missing", active_id);
                snapshot.active_objective_id = None;
                snapshot.phase = "observe_event".to_string();
                snapshot.context_pack_path = None;
                snapshot.resume_instruction = resume_instruction.clone();
                snapshot.last_event_summary = Some(event_summary.clone());
                *self.runtime.active_objective_id.write().await = None;
                *self.runtime.phase.write().await = "observe_event".to_string();
                *self.runtime.context_pack_path.write().await = None;
                *self.runtime.resume_instruction.write().await = resume_instruction;
                *self.runtime.last_event_summary.write().await = Some(event_summary);
            }
            Err(err) => {
                warn!(
                    task_id = %active_id,
                    error = %err,
                    "master-control failed to verify active objective liveness"
                );
            }
        }
        snapshot
    }

    async fn active_objective_prompt_context(
        &self,
        snapshot: &MasterControlRuntimeSnapshot,
    ) -> Option<ActiveObjectivePromptContext> {
        let active_id = snapshot.active_objective_id.as_deref()?;
        match self.state.store.get_board_task(active_id).await {
            Ok(Some(task)) if !is_terminal_board_task_status(&task.status) => {
                Some(ActiveObjectivePromptContext::from_task(&task))
            }
            Ok(Some(_)) | Ok(None) => None,
            Err(err) => {
                warn!(
                    task_id = %active_id,
                    error = %err,
                    "master-control failed to load active objective prompt context"
                );
                None
            }
        }
    }

    async fn recover_open_master_objective(&self) -> anyhow::Result<Option<String>> {
        let snapshot = self.runtime.snapshot().await;
        if snapshot.queued_events > 0 || snapshot.active_objective_id.is_some() {
            return Ok(snapshot.active_objective_id);
        }
        let mut tasks = self
            .state
            .store
            .list_board_tasks(Some("running"), false)
            .await?;
        tasks.extend(
            self.state
                .store
                .list_board_tasks(Some("open"), false)
                .await?,
        );
        let Some(task) = select_recoverable_master_objective(&tasks) else {
            return Ok(None);
        };
        let task_id = task.id.to_string();
        self.runtime
            .record_wakeup(
                "BoardEvent",
                "task_created",
                0,
                board_event_preview(&BoardEvent::TaskCreated {
                    task_id: task_id.clone(),
                    title: task.title.clone(),
                    category: task.category.clone(),
                }),
            )
            .await;
        Ok(Some(task_id))
    }

    async fn write_checkpoint(
        &self,
        reason: &str,
        tick_id: &str,
        mcp_ready: bool,
    ) -> std::io::Result<()> {
        let root = master_project_root(&self.state);
        let path = checkpoint_path_for_root(&root);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let snapshot = self.runtime.snapshot().await;
        let active_objective = self.active_objective_prompt_context(&snapshot).await;
        let prompt_preview =
            should_dispatch_control_turn(reason, &snapshot, mcp_ready).then(|| {
                build_master_tick_prompt(&snapshot, reason, mcp_ready, active_objective.as_ref())
                    .chars()
                    .take(1200)
                    .collect::<String>()
            });
        let body = render_checkpoint(&MasterCheckpointRender {
            slot_id: MASTER_SLOT_ID,
            reason,
            tick_id,
            mcp_ready,
            snapshot: &snapshot,
            prompt_preview,
        });
        tokio::fs::write(path, body).await
    }

    async fn dispatch_control_turn(&self, prompt: &str) -> Result<(), String> {
        self.ensure_master_slot_running().await?;
        self.wait_for_master_slot_ready(Duration::from_secs(MASTER_SLOT_READY_TIMEOUT_SECS))
            .await?;
        self.ensure_master_slot_expected_model().await?;
        self.state
            .pty
            .send_fire_and_forget(MASTER_SLOT_ID, prompt)
            .await
            .map_err(|err| err.to_string())
    }

    async fn wait_for_master_slot_ready(&self, timeout: Duration) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            match self.state.pty.get_status(MASTER_SLOT_ID).await {
                Some(info)
                    if matches!(info.state, SessionState::Idle | SessionState::SlashMenu) =>
                {
                    return Ok(());
                }
                Some(info) if matches!(info.state, SessionState::Exited | SessionState::Error) => {
                    return Err(format!("master slot not running: {:?}", info.state));
                }
                Some(_) => {}
                None => return Err(format!("No PTY session for slot: {MASTER_SLOT_ID}")),
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(format!(
                    "master slot did not become idle within {:?}",
                    timeout
                ));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    async fn ensure_master_slot_expected_model(&self) -> Result<(), String> {
        let Some(screen) = self.state.pty.get_screen_text(MASTER_SLOT_ID).await else {
            return Ok(());
        };
        let Some(actual) = codex_master_model_mismatch(&screen) else {
            return Ok(());
        };
        warn!(
            actual = %actual,
            expected = "gpt-5.5 xhigh",
            "master-control Codex slot model mismatch; restarting before control turn"
        );
        self.state
            .pty
            .kill(MASTER_SLOT_ID)
            .await
            .map_err(|err| err.to_string())?;
        self.ensure_master_slot_running().await?;
        self.wait_for_master_slot_ready(Duration::from_secs(60))
            .await?;
        if let Some(screen) = self.state.pty.get_screen_text(MASTER_SLOT_ID).await {
            if let Some(actual) = codex_master_model_mismatch(&screen) {
                return Err(format!(
                    "master Codex model mismatch after restart: expected gpt-5.5 xhigh, saw {actual}"
                ));
            }
        }
        Ok(())
    }

    async fn ensure_master_slot_running(&self) -> Result<(), String> {
        if let Some(info) = self.state.pty.get_status(MASTER_SLOT_ID).await {
            if !matches!(info.state, SessionState::Exited | SessionState::Error) {
                return Ok(());
            }
        }
        let slot = self
            .state
            .mission
            .list_slots()
            .into_iter()
            .find(|slot| slot.config.id == MASTER_SLOT_ID)
            .ok_or_else(|| format!("master slot not configured: {MASTER_SLOT_ID}"))?;
        let pty_slot = PTYSlot {
            id: slot.config.id.clone(),
            role: slot.config.role.clone(),
            cwd: slot
                .config
                .project_root
                .clone()
                .or(slot.config.cwd.clone())
                .map(PathBuf::from),
            engine: slot.config.engine,
        };
        crate::slot_orchestrator::spawner::spawn_tracked_slot(
            &self.state.pty,
            &self.state.store,
            &self.state.pty_session_uuids,
            &self.state.project_registry,
            self.state.permission.learned(),
            &pty_slot,
            PTYSpawnOptions {
                auto_restart: true,
                wait_for_idle: false,
                timeout_secs: None,
                mcp_config: slot.config.mcp_config.clone().map(PathBuf::from),
                dangerously_skip_permissions: slot
                    .config
                    .dangerously_skip_permissions
                    .unwrap_or(false),
                model: slot.config.model.clone(),
                reasoning_effort: slot.config.reasoning_effort.clone(),
                search_enabled: slot.config.search_enabled.unwrap_or(false),
                sandbox: slot.config.sandbox.clone(),
                approval_policy: slot.config.approval_policy.clone(),
                tool_policy_path: slot.config.tool_policy_path.clone().map(PathBuf::from),
                extra_env: std::collections::HashMap::new(),
                initial_prompt: slot.config.initial_prompt.clone(),
            },
            slot.config.env.as_ref(),
        )
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
    }
}

pub(crate) async fn ensure_code_drift_backfill_task_for_state(
    state: &AppState,
) -> anyhow::Result<Option<String>> {
    let Some(drift) = detect_code_first_drift(&master_project_root(state)).await? else {
        return Ok(None);
    };
    if let Some(existing) = state
        .store
        .find_open_task_by_dedupe_key(&drift.dedupe_key)
        .await?
    {
        return Ok(Some(existing.id.to_string()));
    }
    let input = CreateBoardTaskInput {
        title: "Backfill Lisp/checker for code-first change".to_string(),
        description: Some(format!(
            "MissionD detected code changes without a same-diff Lisp/evidence delta.\n\nChanged files:\n{}\n\nRequired closeout:\n1. Map the behavior to the relevant project/V3 Lisp surface.\n2. Add or update checker coverage.\n3. Add evidence or a concise waiver note explaining why code-first was necessary.",
            drift.files.join("\n")
        )),
        priority: Some("medium".to_string()),
        category: Some("dev".to_string()),
        project: Some("missiond".to_string()),
        auto_execute: Some(false),
        hidden: Some(false),
        dedupe_key: Some(drift.dedupe_key.clone()),
        context_intent: Some("code".to_string()),
        ..Default::default()
    };
    let task = state.storage().store.create_board_task(&input).await?;
    let ev = BoardEvent::TaskCreated {
        task_id: task.id.to_string(),
        title: task.title.clone(),
        category: task.category.clone(),
    };
    notify_board_event_direct(&ev);
    let _ = state.storage().bus.publish_board(ev).await;
    Ok(Some(task.id.to_string()))
}

pub(crate) fn start_master_control_service(
    bus: &Arc<BusServices>,
    state: &AppState,
    shutdown_rx: watch::Receiver<bool>,
) {
    let service = MasterControlService::new(bus.clone(), state.clone(), runtime());
    service.spawn(shutdown_rx);
    info!("master-control service started (event subscriber + checkpoint loop)");
}

pub(crate) fn notify_board_event_direct(event: &BoardEvent) {
    let event = event.clone();
    tokio::spawn(async move {
        if !should_wake_for_board_event(&event, true) {
            return;
        }
        let kind = event.kind();
        let preview = board_event_preview(&event);
        runtime()
            .record_wakeup("BoardEvent", kind, 0, preview)
            .await;
    });
}

pub(crate) fn is_master_control_note_author(author: Option<&str>) -> bool {
    let Some(author) = author else {
        return false;
    };
    author == MASTER_WORKER_ID || MASTER_WORKER_LEGACY_AUTHOR_IDS.contains(&author)
}

fn spawn_master_event_subscriber(
    service: Arc<MasterControlService>,
    shutdown: watch::Receiver<bool>,
) {
    spawn_board_event_sub(service.clone(), shutdown.clone());
    spawn_slot_event_sub(service.clone(), shutdown.clone());
    spawn_question_event_sub(service.clone(), shutdown.clone());
    spawn_incident_event_sub(service, shutdown);
}

fn master_live_subscription_opts() -> SubscriptionOpts {
    let mut opts = SubscriptionOpts::named(MASTER_EVENT_SUBSCRIBER_CONSUMER);
    opts.start_from = StartFrom::Latest;
    opts.cursor_flush = CursorFlush::PerEvent;
    opts
}

fn spawn_board_event_sub(service: Arc<MasterControlService>, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let mut sub = match service
            .bus
            .subscribe::<BoardEvent>(MASTER_BOARD_SUBSCRIPTION, master_live_subscription_opts())
            .await
        {
            Ok(sub) => sub,
            Err(err) => {
                warn!(error = %err, "master-control board subscription failed");
                return;
            }
        };
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    let seq = ack.seq().0;
                    if seq <= 0 || !should_wake_for_board_event(ack.event(), false) {
                        ack.ack().await;
                        continue;
                    }
                    let kind = ack.event().kind();
                    let preview = board_event_preview(ack.event());
                    service.runtime.record_wakeup("BoardEvent", kind, seq, preview).await;
                    ack.ack().await;
                }
            }
        }
    });
}

fn spawn_slot_event_sub(service: Arc<MasterControlService>, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let mut sub = match service
            .bus
            .subscribe::<SlotEvent>(MASTER_SLOT_SUBSCRIPTION, master_live_subscription_opts())
            .await
        {
            Ok(sub) => sub,
            Err(err) => {
                warn!(error = %err, "master-control slot subscription failed");
                return;
            }
        };
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    let seq = ack.seq().0;
                    if !should_wake_for_slot_event(ack.event(), seq) {
                        ack.ack().await;
                        continue;
                    }
                    let kind = ack.event().kind();
                    let preview = slot_event_preview(ack.event());
                    service.runtime.record_wakeup("SlotEvent", kind, seq, preview).await;
                    ack.ack().await;
                }
            }
        }
    });
}

/// Live IncidentEvent subscriber. Pinned by `claude-code-mcp-recovery
/// :wake-resident-master true`: when `pty_event_worker` publishes a
/// `claude_code_mcp_missing` or `claude_code_mcp_reconnect_failed`
/// incident, master_control records a wakeup so the resident master is
/// rescheduled without requiring the operator to observe the PTY screen.
/// All other incident kinds are diagnostic-only here and are acked
/// without waking the master.
fn spawn_incident_event_sub(
    service: Arc<MasterControlService>,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let mut sub = match service
            .bus
            .subscribe::<IncidentEvent>(
                MASTER_INCIDENT_SUBSCRIPTION,
                master_live_subscription_opts(),
            )
            .await
        {
            Ok(sub) => sub,
            Err(err) => {
                warn!(error = %err, "master-control incident subscription failed");
                return;
            }
        };
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    let seq = ack.seq().0;
                    if seq <= 0 || !should_wake_for_incident_event(ack.event()) {
                        ack.ack().await;
                        continue;
                    }
                    let kind = ack.event().kind();
                    let preview = incident_event_preview(ack.event());
                    service
                        .runtime
                        .record_wakeup("IncidentEvent", kind, seq, preview)
                        .await;
                    ack.ack().await;
                }
            }
        }
    });
}

/// Filter for IncidentEvent live subscription: master_control only wakes
/// on Lisp-pinned MCP-recovery incidents
/// (`claude_code_mcp_missing` / `claude_code_mcp_reconnect_failed`),
/// matched against the `kind` field stamped into `raw_payload` by
/// `pty_event_worker::handle_mcp_tool_error`. Other incidents flow through
/// the existing aiops/question-incident pipelines and must not trigger a
/// resident-master control turn.
fn should_wake_for_incident_event(event: &IncidentEvent) -> bool {
    let incident = match event {
        IncidentEvent::Reported { incident } => incident,
        IncidentEvent::StaleSubscription { .. } | IncidentEvent::Resolved { .. } => return false,
    };
    matches!(
        incident
            .raw_payload
            .get("kind")
            .and_then(|value| value.as_str()),
        Some(CLAUDE_CODE_MCP_MISSING_INCIDENT_KIND)
            | Some(CLAUDE_CODE_MCP_RECONNECT_FAILED_INCIDENT_KIND)
    )
}

/// Bounded preview for IncidentEvent record_wakeup metadata. Includes the
/// stamped `kind` (when present) and a short prefix of the title so the
/// resident master's checkpoint records why it was woken.
fn incident_event_preview(event: &IncidentEvent) -> String {
    match event {
        IncidentEvent::Reported { incident } => {
            let kind = incident
                .raw_payload
                .get("kind")
                .and_then(|value| value.as_str())
                .unwrap_or("incident_reported");
            let mut title = incident.title.clone();
            if title.chars().count() > 120 {
                title = title.chars().take(120).collect();
            }
            format!("kind={kind} id={} title={title}", incident.id)
        }
        IncidentEvent::Resolved {
            incident_id,
            reason,
        } => format!("incident_id={incident_id} reason={reason}"),
        IncidentEvent::StaleSubscription { incident } => {
            format!("stale_subscription id={}", incident.id)
        }
    }
}

fn spawn_question_event_sub(
    service: Arc<MasterControlService>,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let mut sub = match service
            .bus
            .subscribe::<QuestionEvent>(
                MASTER_QUESTION_SUBSCRIPTION,
                master_live_subscription_opts(),
            )
            .await
        {
            Ok(sub) => sub,
            Err(err) => {
                warn!(error = %err, "master-control question subscription failed");
                return;
            }
        };
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                ack = sub.next() => {
                    let Some(ack) = ack else { break; };
                    let seq = ack.seq().0;
                    if seq <= 0 {
                        ack.ack().await;
                        continue;
                    }
                    let kind = ack.event().kind();
                    let preview = question_event_preview(ack.event());
                    service.runtime.record_wakeup("QuestionEvent", kind, seq, preview).await;
                    ack.ack().await;
                }
            }
        }
    });
}

fn spawn_master_decision_loop(
    service: Arc<MasterControlService>,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        service.tick("daemon-startup").await;
        let heartbeat_secs = std::env::var("MISSIOND_MASTER_HEARTBEAT_INTERVAL_SECS")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .filter(|secs| *secs >= 60)
            .unwrap_or(300);
        let mut interval = tokio::time::interval(Duration::from_secs(heartbeat_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => {
                    service.tick("daemon-restart-before-exit").await;
                    break;
                }
                _ = service.runtime.notify.notified() => {
                    service.tick("event-wakeup").await;
                }
                _ = interval.tick() => {
                    service.tick("periodic-heartbeat").await;
                }
            }
        }
    });
}

pub(crate) async fn write_startup_checkpoint_for_slot(
    slot_config: &missiond_core::types::SlotConfig,
) {
    if slot_config.id != MASTER_SLOT_ID {
        return;
    }
    let Some(cwd) = slot_config.cwd.as_deref() else {
        return;
    };
    let root = PathBuf::from(cwd);
    let runtime = runtime();
    runtime
        .record_checkpoint_context(
            "DaemonRestart",
            "startup",
            0,
            "runtime slot registered".to_string(),
        )
        .await;
    let snapshot = runtime.snapshot().await;
    let path = checkpoint_path_for_root(&root);
    if let Some(parent) = path.parent() {
        if let Err(err) = tokio::fs::create_dir_all(parent).await {
            warn!(error = %err, path = %parent.display(), "Failed to create master-control checkpoint directory");
            return;
        }
    }
    let body = render_checkpoint(&MasterCheckpointRender {
        slot_id: MASTER_SLOT_ID,
        reason: "runtime-slot-registered",
        tick_id: "master-startup-registration",
        mcp_ready: false,
        snapshot: &snapshot,
        prompt_preview: None,
    });
    if let Err(err) = tokio::fs::write(&path, body).await {
        warn!(error = %err, path = %path.display(), "Failed to write master-control startup checkpoint");
    }
}

pub(crate) fn checkpoint_path_for_root(root: &Path) -> PathBuf {
    if let Some(runtime_dir) = missiond_runtime_dir_from_env() {
        return runtime_dir.join(CHECKPOINT_RUNTIME_PATH);
    }
    root.join(CHECKPOINT_RELATIVE_PATH)
}

struct MasterCheckpointRender<'a> {
    slot_id: &'a str,
    reason: &'a str,
    tick_id: &'a str,
    mcp_ready: bool,
    snapshot: &'a MasterControlRuntimeSnapshot,
    prompt_preview: Option<String>,
}

fn render_checkpoint(input: &MasterCheckpointRender<'_>) -> String {
    let updated_at = chrono::Utc::now().to_rfc3339();
    format!(
        "(master-control-checkpoint\n  :schema \"missiond.master-control-checkpoint.v3\"\n  :worker codex-master-control\n  :slot-id {}\n  :tick-id {}\n  :reason {}\n  :checkpoint-at {}\n  :event-cursor {}\n  :last-event-seq {}\n  :last-event {}\n  :queued-events {}\n  :processed-ticks {}\n  :active-objective-id {}\n  :phase {}\n  :context-pack-path {}\n  :delegated-task-ids {}\n  :blocked-reason {}\n  :last-verified-commit {}\n  :resume-instruction {}\n  :drift-backfill-tasks-created {}\n  :last-drift-backfill-task-id {}\n  :control-turns-sent {}\n  :last-control-turn-at-epoch {}\n  :last-control-objective-id {}\n  :last-control-turn-error {}\n  :mcp-ready {}\n  :objective \"resident master-control watches MissionD events and delegates through BoardTask/Autopilot\"\n  :resume-from [latest-master-control-checkpoint BoardTask mission_execution event-bus provider-log]\n  :resume-plan ((step s1 :logic \"observe_event then classify_objective from durable event evidence\")\n                (step s2 :logic \"create_context_pack and dispatch_investigators before compile_shards\")\n                (step s3 :logic \"dispatch_implementers only with exact write_scope/must_not_touch/acceptance\")\n                (step s4 :logic \"verify durable evidence before close_or_backfill\"))\n  :last-control-prompt {}\n  :updated-at {}\n)\n",
        lisp_string(input.slot_id),
        lisp_string(input.tick_id),
        lisp_string(input.reason),
        lisp_string(&updated_at),
        lisp_option_string(input.snapshot.last_event_cursor.as_deref()),
        input.snapshot.last_event_seq,
        lisp_option_string(input.snapshot.last_event_summary.as_deref()),
        input.snapshot.queued_events,
        input.snapshot.processed_ticks,
        lisp_option_string(input.snapshot.active_objective_id.as_deref()),
        lisp_string(&input.snapshot.phase),
        lisp_option_string(input.snapshot.context_pack_path.as_deref()),
        lisp_string_list(&input.snapshot.delegated_task_ids),
        lisp_option_string(input.snapshot.blocked_reason.as_deref()),
        lisp_option_string(input.snapshot.last_verified_commit.as_deref()),
        lisp_string(&input.snapshot.resume_instruction),
        input.snapshot.drift_backfill_tasks_created,
        lisp_option_string(input.snapshot.last_drift_backfill_task_id.as_deref()),
        input.snapshot.control_turns_sent,
        input.snapshot.last_control_turn_at_epoch,
        lisp_option_string(input.snapshot.last_control_objective_id.as_deref()),
        lisp_option_string(input.snapshot.last_control_turn_error.as_deref()),
        if input.mcp_ready { "true" } else { "false" },
        lisp_option_string(input.prompt_preview.as_deref()),
        lisp_string(&updated_at),
    )
}

pub(crate) fn build_master_tick_prompt(
    snapshot: &MasterControlRuntimeSnapshot,
    reason: &str,
    mcp_ready: bool,
    active_objective: Option<&ActiveObjectivePromptContext>,
) -> String {
    let active_boardtask = render_active_objective_prompt_block(active_objective);
    format!(
        "MissionD resident master tick.\nreason: {reason}\nphase: {}\nactive_objective_id: {}\ncontext_pack_path: {}\nevent_cursor: {}\nevent_summary: {}\nqueued_events: {}\nmcp_ready: {}\nactive_boardtask:\n{}\n\nUnknowns-first intake:\n关于用户提出的这个问题或需求，我还有哪些不知道的信息？这些未知信息分别应该从 SSOT、skill operational facts、项目代码、部署事实、事件总线、checker、还是用户决策入口取得？先补证据，再判断意图。\n\nIntent intake workflow:\n新鲜用户消息或外部应用请求优先走 .missiond/workflows/intent-intake-grounding.lisp：第一轮理解用户想做什么；第二轮优先用 mission_context_gather 聚合 KB / SSOT / project registry / skill evidence / infra evidence 来补事实；第三轮生成 request-local intent-alignment.lisp 或 work-order intent，并把 review_packet 返回给用户/主控确认。不要在 grounded intent 之前编 plan 或派实现工位。确认 intent 后，除非已有 workflow.lisp 可直接绑定，否则 plan.lisp 编写本身也是一个计划工位阶段：它需要读取 mission_context_gather 证据、mission_tool_directory 工具/资源目录和 workstation pool，输出 accepted shards。\n\nIntent inference:\n基于已补齐的证据，请判断用户此刻的真实意图、长期偏好或治理原则是什么；若判断成立，产出带 evidence_refs/confidence/supersession_scope 的 intent_memory_candidate，供 MissionD 意识层自我进化使用。高置信稳定意图请通过 mission_kb_remember 写入 category=memory:decision；低置信意图只保留为 needs-review/candidate artifact。\n\nHeuristic review question:\n请先审视 active objective 和相关 SSOT Lisp：颗粒度是否足够细？哪些架构可以更优雅？你还需要哪些证据、调查工位或 exact shard？\n\nDecision context:\n- If active_objective_id is none, return decision=no-op.\n- The active BoardTask above is the only load-bearing objective; if you need more detail, query that task by id, then follow its description.\n- For non-exact work, produce unknowns, inferred_user_intent, evidence_needed, and a context-pack/evidence plan before delegation.\n- Delegate only after an evidence-plan exists, or when exact-shard-ready=true is explicit.\n- If creating child BoardTasks while active_boardtask is present, set parentId to the active BoardTask id.\n\nAllowed action summary:\n- no-op\n- create/update BoardTask after performing the matching MissionD MCP mutation\n- write high-confidence intent memory through mission_kb_remember(category=memory:decision)\n- start/advance mission_request when producing a reviewable intent artifact for confirmation\n- call mission_context_gather for scoped fact grounding before intent or plan decisions\n- delegate plan-authoring or implementation worker through mission_swarm_run or mission_task_delegate when the next shard is concrete\n- blocked\n- close_or_backfill\n\nHard safety/tool rules are enforced by MissionD runtime metadata, workflow.lisp, and checkers; do not restate them as long prompt prose. PTY recognition is diagnostic only.\n\nReturn compact fields:\ndecision:\nreasoning_summary:\nunknowns:\ninferred_user_intent:\nintent_memory_candidate:\nevidence_needed:\ndelegation_plan?:\nnext_question_or_action:",
        snapshot.phase,
        snapshot.active_objective_id.as_deref().unwrap_or("none"),
        snapshot.context_pack_path.as_deref().unwrap_or("none"),
        snapshot.last_event_cursor.as_deref().unwrap_or("none"),
        snapshot.last_event_summary.as_deref().unwrap_or("none"),
        snapshot.queued_events,
        mcp_ready,
        active_boardtask
    )
}

fn render_active_objective_prompt_block(
    active_objective: Option<&ActiveObjectivePromptContext>,
) -> String {
    let Some(task) = active_objective else {
        return "none".to_string();
    };
    format!(
        "- id: {}\n- status: {}\n- project: {}\n- parentId: {}\n- title: {}\n- description_excerpt: {}",
        task.id,
        task.status,
        task.project.as_deref().unwrap_or("none"),
        task.parent_id.as_deref().unwrap_or("none"),
        task.title,
        task.description_excerpt
    )
}

fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut out: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        out.push_str("...");
    }
    out
}

struct MasterContextPackRender<'a> {
    reason: &'a str,
    tick_id: &'a str,
    snapshot: &'a MasterControlRuntimeSnapshot,
}

fn render_master_context_pack(input: &MasterContextPackRender<'_>) -> String {
    format!(
        "(master-control-context-pack\n  :schema \"missiond.master-control-context-pack.v1\"\n  :tick-id {}\n  :reason {}\n  :active-objective-id {}\n  :phase {}\n  :event-cursor {}\n  :event-summary {}\n  :resume-instruction {}\n  :prompt-style missiond-v3-ssot-review\n  :authority [missiond-v3-ssot-lisp skill-operational-facts checker-result final-convergence-static recent-v3-commit active-boardtask-description]\n  :active-objective-contract [intent-intake-grounding unknowns-first-intake mission-context-gather intent-inference intent-memory-capture plan-authoring-worker boardtask-description-overrides-default-self-review read-only-declared-project-roots mutation-before-decision-return]\n  :excluded-default-inputs [kb board-backlog event-history provider-durable-log historical-conversation]\n  :decision-options [no-op create-update-boardtask start-advance-mission-request write-intent-memory delegate-worker blocked close-or-backfill]\n  :runtime-rules [narrow-mcp-first scoped-grounding-query pty-diagnostic-only no-direct-code-edit no-recursive-worker-delegation board-mcp-mutation-before-final no-broad-kb-preload]\n  :updated-at {}\n)\n",
        lisp_string(input.tick_id),
        lisp_string(input.reason),
        lisp_option_string(input.snapshot.active_objective_id.as_deref()),
        lisp_string(&input.snapshot.phase),
        lisp_option_string(input.snapshot.last_event_cursor.as_deref()),
        lisp_option_string(input.snapshot.last_event_summary.as_deref()),
        lisp_string(&input.snapshot.resume_instruction),
        lisp_string(&chrono::Utc::now().to_rfc3339()),
    )
}

pub(crate) fn codex_master_model_mismatch(screen: &str) -> Option<String> {
    for line in screen.lines() {
        let normalized = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !normalized.contains("gpt-") {
            continue;
        }
        let is_model_line = normalized.contains("model:")
            || normalized.contains("·")
            || normalized.contains("/model to change");
        if !is_model_line {
            continue;
        }
        if normalized.contains("gpt-5.5") && normalized.contains("xhigh") {
            return None;
        }
        return Some(normalized);
    }
    None
}

#[derive(Debug, Clone)]
pub(crate) struct CodeFirstDrift {
    pub(crate) files: Vec<String>,
    pub(crate) dedupe_key: String,
}

pub(crate) async fn detect_code_first_drift(root: &Path) -> anyhow::Result<Option<CodeFirstDrift>> {
    let output = Command::new("git")
        .args(["diff", "--name-only"])
        .current_dir(root)
        .output()
        .await?;
    if !output.status.success() {
        return Ok(None);
    }
    let changed: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect();
    if !has_code_surface_delta(&changed) || has_lisp_or_evidence_delta(&changed) {
        return Ok(None);
    }
    let mut digest = Sha256::new();
    for file in &changed {
        if is_code_surface_file(file) {
            digest.update(file.as_bytes());
            digest.update(b"\n");
        }
    }
    let hash = format!("{:x}", digest.finalize());
    Ok(Some(CodeFirstDrift {
        files: changed
            .into_iter()
            .filter(|file| is_code_surface_file(file))
            .collect(),
        dedupe_key: format!("lisp-code-drift:{}", &hash[..16]),
    }))
}

fn has_code_surface_delta(files: &[String]) -> bool {
    files.iter().any(|file| is_code_surface_file(file))
}

fn has_lisp_or_evidence_delta(files: &[String]) -> bool {
    files
        .iter()
        .any(|file| file.ends_with(".lisp") || file.contains("/evidence/"))
}

fn is_code_surface_file(file: &str) -> bool {
    file.starts_with("crates/") || file.starts_with("packages/") || file.starts_with("scripts/")
}

pub(crate) fn should_dispatch_control_turn(
    reason: &str,
    snapshot: &MasterControlRuntimeSnapshot,
    mcp_ready: bool,
) -> bool {
    if !mcp_ready || !master_control_turns_enabled() {
        return false;
    }
    let now = chrono::Utc::now().timestamp();
    let outside_rate_limit = snapshot.last_control_turn_at_epoch == 0
        || now.saturating_sub(snapshot.last_control_turn_at_epoch) >= 30;
    let outside_objective_heartbeat = snapshot.last_control_turn_at_epoch == 0
        || now.saturating_sub(snapshot.last_control_turn_at_epoch)
            >= MASTER_ACTIVE_OBJECTIVE_HEARTBEAT_SECS;
    if reason == "event-wakeup" {
        let Some(active_id) = snapshot.active_objective_id.as_deref() else {
            return false;
        };
        // A newly-created top-level objective must wake the resident master
        // immediately even if a previous objective just sent a control turn.
        // The last-control objective key prevents the direct BoardEvent notify
        // and the bus subscriber copy from double-sending the same objective.
        if snapshot.last_control_objective_id.as_deref() != Some(active_id) {
            return true;
        }
        return outside_rate_limit;
    }
    if reason == "periodic-heartbeat" && snapshot.active_objective_id.is_some() {
        return outside_objective_heartbeat;
    }
    false
}

async fn classify_master_decision_state(
    reason: &str,
    snapshot: &MasterControlRuntimeSnapshot,
    project_root: &Path,
) -> MasterDecisionState {
    let summary = snapshot.last_event_summary.as_deref().unwrap_or("");
    let event_task_id = extract_task_id(summary);
    let terminal_status_event = is_terminal_board_status_event(summary);
    let terminal_active_objective_event = terminal_status_event
        && (event_task_id.as_deref() == snapshot.active_objective_id.as_deref()
            || snapshot.active_objective_id.is_none());
    let active_objective_id = if terminal_active_objective_event {
        None
    } else {
        snapshot.active_objective_id.clone().or_else(|| {
            if event_summary_can_start_objective(summary) {
                event_task_id.clone()
            } else {
                None
            }
        })
    };
    let phase = if terminal_active_objective_event {
        "observe_event"
    } else if reason == "daemon-startup" || reason == "periodic-heartbeat" {
        "observe_event"
    } else if summary.contains("QuestionEvent.") {
        "blocked"
    } else if summary.contains("SlotEvent.task_dispatched")
        || summary.contains("TaskDispatched")
        || summary.contains("task_dispatched")
    {
        "dispatch_implementers"
    } else if summary.contains("context-pack") || summary.contains("context_pack") {
        "create_context_pack"
    } else if summary.contains("BoardEvent.") && event_task_id.is_some() {
        "classify_objective"
    } else if snapshot.last_drift_backfill_task_id.is_some() {
        "close_or_backfill"
    } else {
        "observe_event"
    };
    let context_pack_path = active_objective_id
        .as_ref()
        .map(|id| master_context_pack_path_for_objective(project_root, id));
    let resume_instruction = match phase {
        "observe_event" => {
            "observe durable Board/event/provider evidence; do not delegate without a concrete objective"
        }
        "classify_objective" => {
            "query the active BoardTask, classify no-op/context-pack/delegate/blocked/backfill, then write a checkpoint note"
        }
        "create_context_pack" => {
            "create or update the objective context-pack and dispatch read-only investigators only"
        }
        "dispatch_investigators" => {
            "delegate read-only context organizers with context-pack path and no write scope"
        }
        "compile_shards" => {
            "compile accepted context-pack entries into exact file/region shards before coding"
        }
        "dispatch_implementers" => {
            "delegate only accepted shards with write_scope, must_not_touch, acceptance, model_profile, timeout_secs"
        }
        "verify" => "run static/runtime acceptance and collect durable completion evidence",
        "close_or_backfill" => {
            "close the objective only after durable evidence, or create visible Lisp/checker/evidence backfill"
        }
        "blocked" => "surface blocked reason/question and wait for human or tool resolution",
        _ => "resume from checkpoint using BoardTask, event bus, provider logs, and KB evidence",
    }
    .to_string();
    MasterDecisionState {
        phase: phase.to_string(),
        active_objective_id,
        context_pack_path,
        delegated_task_ids: Vec::new(),
        last_verified_commit: None,
        resume_instruction,
    }
}

fn extract_task_id(summary: &str) -> Option<String> {
    let marker = "task_id=";
    let start = summary.find(marker)? + marker.len();
    let rest = &summary[start..];
    let id = rest
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ')' || ch == '(')
        .next()
        .unwrap_or("")
        .trim();
    (!id.is_empty()).then(|| id.to_string())
}

fn is_terminal_board_status_event(summary: &str) -> bool {
    let lower = summary.to_ascii_lowercase();
    lower.contains("boardevent.status_changed")
        && (lower.contains("->done")
            || lower.contains("->completed")
            || lower.contains("->closed")
            || lower.contains("->failed")
            || lower.contains("->blocked")
            || lower.contains("->skipped")
            || lower.contains("->terminal"))
}

fn is_terminal_board_task_status(status: &BoardTaskStatus) -> bool {
    matches!(
        status,
        BoardTaskStatus::Done
            | BoardTaskStatus::Failed
            | BoardTaskStatus::Blocked
            | BoardTaskStatus::Skipped
    )
}

fn should_consume_event_without_control(
    reason: &str,
    snapshot: &MasterControlRuntimeSnapshot,
    decision: &MasterDecisionState,
) -> bool {
    reason == "event-wakeup"
        && decision.active_objective_id.is_none()
        && snapshot
            .last_event_summary
            .as_deref()
            .is_some_and(is_terminal_board_status_event)
}

fn sanitize_lisp_path_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn master_context_pack_path_for_objective(project_root: &Path, id: &str) -> String {
    let filename = format!("{}.lisp", sanitize_lisp_path_component(id));
    let path = if let Some(runtime_dir) = missiond_runtime_dir_from_env() {
        runtime_dir
            .join(MASTER_CONTEXT_PACK_RUNTIME_DIR)
            .join(filename)
    } else {
        project_root
            .join(".missiond/v3/runtime/master-control/context-packs")
            .join(filename)
    };
    path.to_string_lossy().to_string()
}

fn missiond_runtime_dir_from_env() -> Option<PathBuf> {
    std::env::var("MISSIOND_RUNTIME_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn master_control_turns_enabled() -> bool {
    match std::env::var("MISSIOND_MASTER_CONTROL_TURNS") {
        Ok(value) => {
            let value = value.trim().to_ascii_lowercase();
            !(value == "0" || value == "false" || value == "off" || value == "disabled")
        }
        Err(_) => true,
    }
}

pub(crate) async fn mission_master_status(state: &AppState) -> Value {
    let slots = state.slots();
    let storage = state.storage();
    let control = state.control_plane();
    let config = WorkstationRuntimeConfig::load_for_current_dir().ok();
    let worker = config.as_ref().and_then(|config| {
        config
            .workstation_pool()
            .iter()
            .find(|worker| worker.id == MASTER_WORKER_ID)
    });
    let slot_id = worker
        .map(|worker| worker.slot_id.as_str())
        .unwrap_or(MASTER_SLOT_ID);
    let pty_status = slots.pty.get_status(slot_id).await;
    let mission_slot_record = slots
        .mission
        .list_slots()
        .into_iter()
        .find(|slot| slot.config.id == slot_id);
    let checkpoint_root = mission_slot_record
        .as_ref()
        .and_then(|slot| slot.config.project_root.clone().or(slot.config.cwd.clone()))
        .map(PathBuf::from)
        .unwrap_or_else(|| master_project_root(state));
    let mission_slot = mission_slot_record.and_then(|slot| serde_json::to_value(slot).ok());
    let checkpoint_path = checkpoint_path_for_root(&checkpoint_root);
    let checkpoint_text = std::fs::read_to_string(&checkpoint_path).ok();
    let runtime_snapshot = runtime().snapshot().await;
    let commit_convergence = crate::engine::commit_convergence::status_snapshot().await;
    let lisp_code_sync = crate::engine::lisp_code_sync::status_snapshot_for_state(state).await;
    let nightly_evolution = crate::engine::nightly_evolution::status_snapshot().await;
    let shared_memory = storage.shared_memory.status_snapshot().await;
    let daemon_stats = control.stats.snapshot();
    let runtime_load_explanation = runtime_load_explanation(
        &daemon_stats,
        &lisp_code_sync,
        &shared_memory,
        &nightly_evolution,
    );
    let mcp_enabled = probe_codex_mcp_ready().await;
    let approval = probe_codex_mcp_approval_ready();
    let mcp_ready = mcp_enabled && approval.ready;

    let mut status = json!({
        "schema": "missiond.master-status.v2",
        "worker": MASTER_WORKER_ID,
        "slotId": slot_id,
        "configured": worker.is_some(),
        "acceptsBoardTask": worker.map(|worker| worker.accepts_boardtask).unwrap_or(false),
        "writeAllowed": worker.map(|worker| worker.write_allowed).unwrap_or(false),
        "modelProfile": worker.and_then(|worker| worker.model_profile.clone()),
        "reasoningEffort": worker.and_then(|worker| worker.reasoning_effort.clone()),
        "searchEnabled": worker.map(|worker| worker.search_enabled).unwrap_or(false),
        "sandbox": worker.and_then(|worker| worker.sandbox.clone()),
        "approvalPolicy": worker.and_then(|worker| worker.approval_policy.clone()),
        "mcpReady": mcp_ready,
        "mcpEnabled": mcp_enabled,
        "mcpApprovalReady": approval.ready,
        "mcp": {
            "source": "~/.codex/config.toml",
            "probe": "codex mcp list",
            "missiond": {"ready": mcp_enabled},
            "approval": {
                "ready": approval.ready,
                "requiredTools": MASTER_MCP_APPROVED_TOOLS,
                "missingTools": approval.missing_tools
            }
        },
        "pty": pty_status.and_then(|status| serde_json::to_value(status).ok()),
        "slot": mission_slot,
        "service": {
            "mode": "hybrid",
            "status": "registered",
            "phase": runtime_snapshot.phase,
            "phases": MASTER_PHASES,
            "activeObjectiveId": runtime_snapshot.active_objective_id,
            "contextPackPath": runtime_snapshot.context_pack_path,
            "delegatedTaskIds": runtime_snapshot.delegated_task_ids,
            "lastVerifiedCommit": runtime_snapshot.last_verified_commit,
            "resumeInstruction": runtime_snapshot.resume_instruction,
            "surfaces": [
                "master-checkpoint",
                "master-event-subscriber",
                "master-decision-loop",
                "master-delegation",
                "master-recovery",
                "night-scheduler",
                "commit-lisp-convergence-loop",
                "lisp-code-sync-loop",
                "nightly-evolution-loop"
            ],
            "queuedEvents": runtime_snapshot.queued_events,
            "processedTicks": runtime_snapshot.processed_ticks,
            "driftBackfillTasksCreated": runtime_snapshot.drift_backfill_tasks_created,
            "lastDriftBackfillTaskId": runtime_snapshot.last_drift_backfill_task_id,
            "controlTurnsSent": runtime_snapshot.control_turns_sent,
            "lastControlTurnAtEpoch": runtime_snapshot.last_control_turn_at_epoch,
            "lastControlTurnError": runtime_snapshot.last_control_turn_error,
            "eventCursor": runtime_snapshot.last_event_cursor,
            "lastEvent": runtime_snapshot.last_event_summary,
            "lastTickId": runtime_snapshot.last_tick_id,
            "lastCheckpointAtEpoch": runtime_snapshot.last_checkpoint_at_epoch,
            "lastMcpReady": runtime_snapshot.last_mcp_ready,
            "blockedReason": runtime_snapshot.blocked_reason
        },
        "checkpoint": {
            "path": checkpoint_path.display().to_string(),
            "exists": checkpoint_text.is_some(),
            "preview": checkpoint_text
                .as_ref()
                .map(|text| text.chars().take(1600).collect::<String>()),
        },
        "authority": {
            "primary": [
                "provider_jsonl",
                CONVERSATION_SOURCE_LABEL_CODEX_LOCAL_INDEX,
                "claude_jsonl",
                "gemini_chat_file"
            ],
            "secondary": ["missiond_event_bus", "board_task_lifecycle", "mission_execution"],
            "diagnostic": ["pty_recognition_snapshot"]
        }
    });
    status["service"]["commitConvergence"] = commit_convergence;
    status["service"]["lispCodeSync"] = lisp_code_sync;
    status["service"]["nightlyEvolution"] = nightly_evolution;
    status["service"]["sharedMemory"] = shared_memory;
    status["service"]["runtimeLoadExplanation"] = runtime_load_explanation;
    status["service"]["compiledRuntime"] = compiled_runtime_projection_status(&checkpoint_root);
    status["service"]["lastControlObjectiveId"] = json!(runtime_snapshot.last_control_objective_id);
    status
}

fn runtime_load_explanation(
    daemon_stats: &Value,
    lisp_code_sync: &Value,
    shared_memory: &Value,
    nightly_evolution: &Value,
) -> Value {
    fn u64_at(value: &Value, pointer: &str) -> u64 {
        value.pointer(pointer).and_then(Value::as_u64).unwrap_or(0)
    }
    fn i64_at(value: &Value, pointer: &str) -> i64 {
        value.pointer(pointer).and_then(Value::as_i64).unwrap_or(0)
    }

    let event_backlog = u64_at(daemon_stats, "/events/estimated_backlog");
    let autopilot_avg_ms = u64_at(daemon_stats, "/autopilot/avg_ms");
    let db_p95_us = u64_at(daemon_stats, "/db_exec/p95_us");
    let prefetch_total = u64_at(daemon_stats, "/prefetch/total");
    let prefetch_router_errors = u64_at(daemon_stats, "/prefetch/router/errors");
    let lisp_recent_reports = u64_at(lisp_code_sync, "/reportDirs/totalRecentReports5m");
    let lisp_over_limit = u64_at(lisp_code_sync, "/reportDirs/overLimitProjects");
    let storm_hits = u64_at(lisp_code_sync, "/stormCircuitHits");
    let recent_sync_tasks = u64_at(lisp_code_sync, "/recentSyncTaskCreations");
    let active_workflows = u64_at(shared_memory, "/activeWorkflowRuns");
    let stale_claims = u64_at(shared_memory, "/staleClaims");
    let cursor_lag_max = shared_memory
        .get("cursorLag")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .filter_map(|row| row.get("lag").and_then(Value::as_u64))
                .max()
        })
        .unwrap_or(0);
    let nightly_last_run = i64_at(nightly_evolution, "/lastRunAtEpoch");
    let nightly_findings = u64_at(nightly_evolution, "/lastFindingsCount");

    let mut suspects = Vec::new();
    if lisp_recent_reports > 0 || lisp_over_limit > 0 || recent_sync_tasks > 0 {
        suspects.push(json!({
            "component": "lisp-code-sync",
            "reason": "recent reports, over-retention projects, or sync task creations are non-zero",
            "signals": {
                "recentReports5m": lisp_recent_reports,
                "overLimitProjects": lisp_over_limit,
                "recentSyncTaskCreations": recent_sync_tasks,
                "stormCircuitHits": storm_hits
            }
        }));
    }
    if event_backlog > 0 {
        suspects.push(json!({
            "component": "eventbus",
            "reason": "published events exceed observed consumer counters",
            "signals": { "estimatedBacklog": event_backlog }
        }));
    }
    if active_workflows > 0 || stale_claims > 0 || cursor_lag_max > 0 {
        suspects.push(json!({
            "component": "shared-memory/workflow-runner",
            "reason": "active workflow runs, stale claims, or cursor lag are present",
            "signals": {
                "activeWorkflowRuns": active_workflows,
                "staleClaims": stale_claims,
                "maxCursorLag": cursor_lag_max
            }
        }));
    }
    if autopilot_avg_ms > 500 || db_p95_us > 500_000 {
        suspects.push(json!({
            "component": "autopilot/db",
            "reason": "autopilot or DB latency counters are elevated",
            "signals": {
                "autopilotAvgMs": autopilot_avg_ms,
                "dbP95Us": db_p95_us
            }
        }));
    }
    if prefetch_total > 0 || prefetch_router_errors > 0 {
        suspects.push(json!({
            "component": "context-prefetch",
            "reason": "prefetch counters are active; router errors may force fallback work",
            "signals": {
                "prefetchTotal": prefetch_total,
                "prefetchRouterErrors": prefetch_router_errors
            }
        }));
    }
    if nightly_last_run > 0 && nightly_findings > 0 {
        suspects.push(json!({
            "component": "nightly-evolution",
            "reason": "nightly evolution has recent findings; scheduled runs should stay disabled unless explicitly enabled",
            "signals": {
                "lastRunAtEpoch": nightly_last_run,
                "lastFindingsCount": nightly_findings
            }
        }));
    }

    let status = if suspects.is_empty() {
        "no_internal_hot_loop_indicated"
    } else {
        "diagnostic_required"
    };

    json!({
        "schema": "missiond.runtime-load-explanation.v1",
        "status": status,
        "suspects": suspects,
        "limits": [
            "This is daemon-internal attribution from counters, not OS process sampling.",
            "Use mission_infra_query(top_cpu) or Activity Monitor for process-level CPU confirmation."
        ]
    })
}

pub(crate) async fn probe_codex_mcp_ready() -> bool {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new("codex").args(["mcp", "list"]).output(),
    )
    .await;
    match output {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            codex_mcp_ready_from_output(&stdout, &stderr)
        }
        Ok(Err(err)) => {
            warn!(error = %err, "codex mcp list probe failed");
            false
        }
        Err(_) => {
            warn!("codex mcp list probe timed out");
            false
        }
    }
}

pub(crate) async fn probe_codex_mcp_control_ready() -> bool {
    probe_codex_mcp_ready().await && probe_codex_mcp_approval_ready().ready
}

#[derive(Debug, Clone)]
pub(crate) struct CodexMcpApprovalReadiness {
    pub(crate) ready: bool,
    pub(crate) missing_tools: Vec<String>,
}

pub(crate) fn probe_codex_mcp_approval_ready() -> CodexMcpApprovalReadiness {
    let config_path = home_dir().join(".codex/config.toml");
    match std::fs::read_to_string(config_path) {
        Ok(config) => codex_mcp_approval_ready_from_config(&config),
        Err(_) => CodexMcpApprovalReadiness {
            ready: false,
            missing_tools: MASTER_MCP_APPROVED_TOOLS
                .iter()
                .map(|tool| (*tool).to_string())
                .collect(),
        },
    }
}

pub(crate) fn codex_mcp_approval_ready_from_config(config: &str) -> CodexMcpApprovalReadiness {
    let missing_tools: Vec<String> = MASTER_MCP_APPROVED_TOOLS
        .iter()
        .filter(|tool| !codex_mcp_tool_approval_is_approve(config, tool))
        .map(|tool| (*tool).to_string())
        .collect();
    CodexMcpApprovalReadiness {
        ready: missing_tools.is_empty(),
        missing_tools,
    }
}

fn codex_mcp_tool_approval_is_approve(config: &str, tool: &str) -> bool {
    let header = format!("[mcp_servers.missiond.tools.{tool}]");
    let Some(start) = config.find(&header) else {
        return false;
    };
    let section = &config[start + header.len()..];
    let end = section.find("\n[").unwrap_or(section.len());
    let body = &section[..end];
    body.lines().any(|line| {
        let line = line.trim();
        line == "approval_mode = \"approve\"" || line == "approval_mode='approve'"
    })
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Lisp-pinned incident kind: ClaudeCode worker advertised supports_mcp but no
/// `mission_*` tools surfaced after slot ready. The value is projected from
/// `claude-code-mcp-recovery :missing-incident-kind` in the V3 blueprint and is
/// the durable signal that wakes the resident master.
pub(crate) const CLAUDE_CODE_MCP_MISSING_INCIDENT_KIND: &str = "claude_code_mcp_missing";

/// Lisp-pinned incident kind: the `/mcp` arrow-key reconnect ritual completed
/// without surfacing any `mission_*` tool within the policy budget. Mirrors
/// `claude-code-mcp-recovery :reconnect-failed-incident-kind`.
pub(crate) const CLAUDE_CODE_MCP_RECONNECT_FAILED_INCIDENT_KIND: &str =
    "claude_code_mcp_reconnect_failed";

/// Diagnosis of a ClaudeCode worker's MCP mounting state, derived from the
/// slot's advertised `supports_mcp` trait and the actual mounted tool list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaudeCodeMcpState {
    /// `supports_mcp=false` — the slot never claimed MCP, no incident is owed.
    NotAdvertised,
    /// `supports_mcp=true` AND at least one `mission_*` tool is mounted.
    Healthy,
    /// `supports_mcp=true` BUT zero `mission_*` tools are mounted; this is the
    /// case that requires a durable `claude_code_mcp_missing` incident plus an
    /// arrow-key `/mcp` reconnect attempt before escalating to
    /// `claude_code_mcp_reconnect_failed`.
    AdvertisedButMissing,
}

/// Classify a slot's MCP state from its advertised capability and mounted
/// tool list. Pure function so master_control can unit-test the policy.
pub(crate) fn classify_claude_code_mcp_state(
    supports_mcp: bool,
    mounted_tools: &[String],
) -> ClaudeCodeMcpState {
    if !supports_mcp {
        return ClaudeCodeMcpState::NotAdvertised;
    }
    if mounted_tools
        .iter()
        .any(|name| name.starts_with("mission_"))
    {
        ClaudeCodeMcpState::Healthy
    } else {
        ClaudeCodeMcpState::AdvertisedButMissing
    }
}

/// Whether the given MCP state requires master_control to file the
/// `claude_code_mcp_missing` incident (and wake the resident master).
pub(crate) fn should_record_mcp_missing_incident(state: ClaudeCodeMcpState) -> bool {
    matches!(state, ClaudeCodeMcpState::AdvertisedButMissing)
}

pub(crate) fn codex_mcp_ready_from_output(stdout: &str, stderr: &str) -> bool {
    let combined = format!("{stdout}\n{stderr}");
    combined.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains("missiond")
            && lower.contains("enabled")
            && !lower.contains("disabled")
            && !lower.contains("failed")
    })
}

fn master_project_root(state: &AppState) -> PathBuf {
    state
        .mission
        .list_slots()
        .into_iter()
        .find(|slot| slot.config.id == MASTER_SLOT_ID)
        .and_then(|slot| slot.config.project_root.or(slot.config.cwd))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn lisp_option_string(value: Option<&str>) -> String {
    value.map(lisp_string).unwrap_or_else(|| "nil".to_string())
}

fn lisp_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn lisp_string_list(values: &[String]) -> String {
    if values.is_empty() {
        return "[]".to_string();
    }
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| lisp_string(value))
            .collect::<Vec<_>>()
            .join(" ")
    )
}

pub(crate) fn board_event_preview(event: &BoardEvent) -> String {
    match event {
        BoardEvent::TaskCreated {
            task_id,
            title,
            category,
        } => format!("task_id={task_id} category={category} title={title}"),
        BoardEvent::StatusChanged {
            task_id,
            old_status,
            new_status,
        } => format!("task_id={task_id} {old_status}->{new_status}"),
        BoardEvent::NoteAdded {
            task_id,
            note_id,
            content_preview,
        } => format!("task_id={task_id} note_id={note_id} preview={content_preview}"),
        BoardEvent::Claimed { task_id, slot_id } => {
            format!("task_id={task_id} slot_id={slot_id}")
        }
        BoardEvent::Deleted { task_id, title } => format!("task_id={task_id} title={title}"),
        BoardEvent::Updated {
            task_id,
            status,
            category,
        } => format!("task_id={task_id} status={status} category={category}"),
    }
}

fn should_wake_for_board_event(event: &BoardEvent, direct_notify: bool) -> bool {
    match event {
        BoardEvent::TaskCreated {
            title, category, ..
        } => !is_test_board_category(category) && !is_swarm_worker_task_title(title),
        BoardEvent::Updated {
            status, category, ..
        } => {
            if is_test_board_category(category) {
                return false;
            }
            let normalized = status.trim().to_ascii_lowercase();
            is_terminal_worker_status(&normalized)
                || !(category == "dev" && normalized == "running")
        }
        BoardEvent::StatusChanged { new_status, .. } => {
            let status = new_status.trim().to_ascii_lowercase();
            is_terminal_worker_status(&status)
                || (status != "done" && status != "completed" && status != "closed")
        }
        BoardEvent::NoteAdded { .. } => direct_notify,
        BoardEvent::Claimed { .. } | BoardEvent::Deleted { .. } => false,
    }
}

fn event_summary_can_start_objective(summary: &str) -> bool {
    summary.contains("BoardEvent.task_created:")
        && !summary.contains(" category=test ")
        && !summary.contains(" title=[smoke]")
}

fn is_terminal_worker_status(status: &str) -> bool {
    matches!(
        status,
        "done" | "completed" | "closed" | "failed" | "blocked"
    )
}

fn is_test_board_category(category: &str) -> bool {
    let category = category.trim().to_ascii_lowercase();
    matches!(category.as_str(), "test" | "smoke")
}

fn is_swarm_worker_task_title(title: &str) -> bool {
    title.starts_with("Investigate context for swarm objective")
        || title.starts_with("Survey exact shards for swarm objective")
        || title.starts_with("Implement accepted swarm shard")
}

fn select_recoverable_master_objective(
    tasks: &[missiond_core::types::BoardTask],
) -> Option<&missiond_core::types::BoardTask> {
    tasks
        .iter()
        .filter(|task| is_recoverable_master_objective(task))
        .max_by_key(|task| task.order_idx)
}

fn is_recoverable_master_objective(task: &missiond_core::types::BoardTask) -> bool {
    if task.hidden {
        return false;
    }
    if task.project.as_deref() != Some("missiond") {
        return false;
    }
    let status = task.status.as_str();
    if status != "open" && status != "running" {
        return false;
    }
    task.title.starts_with("Run project SSOT convergence wave:")
        || task.title.starts_with("Run M6 SSOT convergence")
        || task
            .description
            .contains(".missiond/workflows/project-ssot-convergence.lisp")
        || task
            .description
            .contains("project-ssot-convergence workflow")
}

fn slot_event_preview(event: &SlotEvent) -> String {
    match event {
        SlotEvent::BecameIdle { slot_id } => format!("slot_id={slot_id}"),
        SlotEvent::StateChanged {
            slot_id,
            new_state,
            prev_state,
        } => format!("slot_id={slot_id} {prev_state}->{new_state}"),
        SlotEvent::TaskDispatched {
            slot_id,
            task_id,
            purpose,
            prompt_chars,
            ..
        } => format!(
            "slot_id={slot_id} task_id={} purpose={purpose} prompt_chars={prompt_chars}",
            task_id.as_deref().unwrap_or("none")
        ),
        SlotEvent::Stuck {
            slot_id,
            reason,
            last_activity_ms_ago,
        } => {
            format!("slot_id={slot_id} reason={reason} last_activity_ms_ago={last_activity_ms_ago}")
        }
    }
}

fn slot_event_slot_id(event: &SlotEvent) -> &str {
    match event {
        SlotEvent::BecameIdle { slot_id }
        | SlotEvent::StateChanged { slot_id, .. }
        | SlotEvent::TaskDispatched { slot_id, .. }
        | SlotEvent::Stuck { slot_id, .. } => slot_id.as_str(),
    }
}

fn should_wake_for_slot_event(event: &SlotEvent, seq: i64) -> bool {
    seq > 0
        && slot_event_slot_id(event) != MASTER_SLOT_ID
        && !matches!(
            event,
            SlotEvent::BecameIdle { .. } | SlotEvent::TaskDispatched { .. }
        )
}

fn question_event_preview(event: &QuestionEvent) -> String {
    match event {
        QuestionEvent::Created { question_id } => format!("question_id={question_id}"),
        QuestionEvent::Resolved {
            question_id,
            resolution,
        } => format!("question_id={question_id} resolution={resolution}"),
        QuestionEvent::DecisionResolved {
            question_id,
            tier,
            duration_ms,
        } => format!("question_id={question_id} tier={tier} duration_ms={duration_ms}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_control_records_mcp_missing_incident_when_supports_mcp_but_no_mission_tools() {
        // ClaudeCode worker advertised supports_mcp=true but the mounted tool
        // list does not contain any mission_* tool. Per the V3 blueprint
        // claude-code-mcp-recovery contract, master_control owes a durable
        // claude_code_mcp_missing incident so the resident master is woken.
        let mounted: Vec<String> = vec!["Bash".to_string(), "Read".to_string(), "Edit".to_string()];
        let state = classify_claude_code_mcp_state(true, &mounted);
        assert_eq!(state, ClaudeCodeMcpState::AdvertisedButMissing);
        assert!(should_record_mcp_missing_incident(state));
        assert_eq!(
            CLAUDE_CODE_MCP_MISSING_INCIDENT_KIND,
            "claude_code_mcp_missing"
        );

        // Healthy slot: at least one mission_* tool is mounted -> no incident.
        let healthy: Vec<String> = vec!["Bash".to_string(), "mission_pty_status".to_string()];
        let healthy_state = classify_claude_code_mcp_state(true, &healthy);
        assert_eq!(healthy_state, ClaudeCodeMcpState::Healthy);
        assert!(!should_record_mcp_missing_incident(healthy_state));

        // Slot never advertised supports_mcp: nothing to reconnect, no incident.
        let not_advertised = classify_claude_code_mcp_state(false, &[]);
        assert_eq!(not_advertised, ClaudeCodeMcpState::NotAdvertised);
        assert!(!should_record_mcp_missing_incident(not_advertised));

        // The reconnect-failed kind is the escalation after the arrow-key
        // ritual; pinned here so checker drift surfaces as a test failure.
        assert_eq!(
            CLAUDE_CODE_MCP_RECONNECT_FAILED_INCIDENT_KIND,
            "claude_code_mcp_reconnect_failed"
        );
    }

    #[test]
    fn incident_event_filter_only_wakes_on_claude_code_mcp_recovery_kinds() {
        use missiond_core::types::{IncidentSeverity, IncidentSource, MissionIncident};

        // Helper: build an IncidentEvent::Reported with `raw_payload.kind`.
        fn reported(kind: Option<&str>) -> IncidentEvent {
            let raw_payload = match kind {
                Some(k) => serde_json::json!({"kind": k, "slot_id": "slot-x"}),
                None => serde_json::json!({"slot_id": "slot-x"}),
            };
            IncidentEvent::Reported {
                incident: MissionIncident {
                    id: "inc-test".into(),
                    severity: IncidentSeverity::High,
                    source: IncidentSource::PtySlot,
                    title: "test".into(),
                    description: "test".into(),
                    server_id: None,
                    raw_payload,
                    created_at: "2026-05-06T00:00:00Z".into(),
                },
            }
        }

        // Lisp-pinned MCP-recovery kinds wake the master.
        assert!(should_wake_for_incident_event(&reported(Some(
            CLAUDE_CODE_MCP_MISSING_INCIDENT_KIND
        ))));
        assert!(should_wake_for_incident_event(&reported(Some(
            CLAUDE_CODE_MCP_RECONNECT_FAILED_INCIDENT_KIND
        ))));

        // Other incident kinds and missing kind tags are diagnostic-only —
        // master_control ignores them so unrelated aiops/question-incident
        // pipelines do not drag the resident master into a control turn.
        assert!(!should_wake_for_incident_event(&reported(Some(
            "disk_high"
        ))));
        assert!(!should_wake_for_incident_event(&reported(None)));

        // Resolved and StaleSubscription variants must never wake the
        // master from this subscription; they belong to retention/cleanup
        // pipelines.
        assert!(!should_wake_for_incident_event(&IncidentEvent::Resolved {
            incident_id: "inc-test".into(),
            reason: "manual".into(),
        }));
        assert!(!should_wake_for_incident_event(
            &IncidentEvent::StaleSubscription {
                incident: MissionIncident {
                    id: "inc-test".into(),
                    severity: IncidentSeverity::Warning,
                    source: IncidentSource::HealthCheck,
                    title: "stale".into(),
                    description: "stale".into(),
                    server_id: None,
                    raw_payload: serde_json::json!({
                        "kind": CLAUDE_CODE_MCP_MISSING_INCIDENT_KIND,
                    }),
                    created_at: "2026-05-06T00:00:00Z".into(),
                },
            }
        ));
    }

    #[test]
    fn codex_mcp_ready_requires_enabled_missiond_row() {
        let output = "Name Status\nmissiond /path enabled Unsupported\n";
        assert!(codex_mcp_ready_from_output(output, ""));
        assert!(!codex_mcp_ready_from_output(
            "missiond /path disabled Unsupported",
            ""
        ));
        assert!(!codex_mcp_ready_from_output("other /path enabled", ""));
    }

    #[test]
    fn codex_mcp_approval_ready_requires_master_tools() {
        let config = MASTER_MCP_APPROVED_TOOLS
            .iter()
            .map(|tool| {
                format!("[mcp_servers.missiond.tools.{tool}]\napproval_mode = \"approve\"\n")
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(codex_mcp_approval_ready_from_config(&config).ready);

        let partial = "[mcp_servers.missiond.tools.mission_intent]\napproval_mode = \"approve\"\n";
        let readiness = codex_mcp_approval_ready_from_config(partial);
        assert!(!readiness.ready);
        assert!(readiness
            .missing_tools
            .contains(&"mission_board_query".to_string()));
    }

    #[test]
    fn pty_diagnostic_is_not_completion_authority() {
        let evidence = WorkerCompletionEvidence::PtyDiagnostic {
            provider: "codex_cli",
            state: "idle".to_string(),
            confidence: 0.98,
        };
        assert_eq!(evidence.authority_tier(), "t3-diagnostic-only");
    }

    #[test]
    fn checkpoint_path_uses_v3_runtime_directory_when_no_external_runtime_is_configured() {
        let path = checkpoint_path_for_root(Path::new("/tmp/project"));
        if std::env::var("MISSIOND_RUNTIME_DIR")
            .ok()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
        {
            assert!(path.ends_with("master-control-checkpoint.lisp"));
        } else {
            assert_eq!(
                path,
                PathBuf::from("/tmp/project/.missiond/v3/runtime/master-control-checkpoint.lisp")
            );
        }
    }

    #[tokio::test]
    async fn startup_checkpoint_context_does_not_queue_control_wakeup() {
        let runtime = MasterControlRuntime::default();
        runtime
            .record_checkpoint_context(
                "DaemonRestart",
                "startup",
                0,
                "runtime slot registered".to_string(),
            )
            .await;
        let snapshot = runtime.snapshot().await;
        assert_eq!(snapshot.queued_events, 0);
        assert_eq!(
            snapshot.last_event_summary.as_deref(),
            Some("DaemonRestart.startup: runtime slot registered")
        );
    }

    fn test_board_task(
        id: &str,
        title: &str,
        description: &str,
        project: Option<&str>,
        category: &str,
        order_idx: i64,
    ) -> missiond_core::types::BoardTask {
        missiond_core::types::BoardTask {
            id: missiond_core::types::TaskId::from_trusted(id.to_string()),
            title: title.to_string(),
            description: description.to_string(),
            status: missiond_core::types::BoardTaskStatus::Open,
            priority: "high".to_string(),
            category: category.to_string(),
            project: project.map(ToString::to_string),
            server: None,
            due_date: None,
            parent_id: None,
            assignee: None,
            auto_execute: false,
            prompt_template: None,
            hidden: false,
            retry_count: 0,
            max_retries: 2,
            order_idx,
            created_at: "2026-05-03T00:00:00Z".to_string(),
            updated_at: "2026-05-03T00:00:00Z".to_string(),
            claim_executor_id: None,
            claim_executor_type: None,
            claimed_at: None,
            flow_phase: None,
            flow_context: None,
            flow_template: None,
            depends_on: Vec::new(),
            lease_expires_at: None,
            dedupe_key: None,
            timeout_secs: None,
            context_intent: None,
            trigger_source: None,
            runtime_metadata: serde_json::json!({}),
            notes_count: 0,
        }
    }

    #[test]
    fn master_startup_recovers_latest_open_ssot_objective() {
        let old = test_board_task(
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "Run project SSOT convergence wave: old",
            "",
            Some("missiond"),
            "infra",
            1,
        );
        let newest = test_board_task(
            "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            "Manual task",
            "Uses .missiond/workflows/project-ssot-convergence.lisp",
            Some("missiond"),
            "infra",
            2,
        );
        let ignored = test_board_task(
            "cccccccc-cccc-cccc-cccc-cccccccccccc",
            "Run project SSOT convergence wave: other",
            "",
            Some("other"),
            "infra",
            3,
        );
        let m6 = test_board_task(
            "dddddddd-dddd-dddd-dddd-dddddddddddd",
            "Run M6 SSOT convergence controller for Jarvis toolchain projects",
            "Use project-ssot-convergence workflow only.",
            Some("missiond"),
            "dev",
            4,
        );
        let tasks = vec![old, newest, ignored, m6];
        let selected = select_recoverable_master_objective(&tasks).expect("recover objective");
        assert_eq!(selected.id.as_str(), "dddddddd-dddd-dddd-dddd-dddddddddddd");
    }

    #[test]
    fn control_turn_dispatch_requires_active_objective() {
        let mut snapshot = MasterControlRuntimeSnapshot {
            queued_events: 1,
            processed_ticks: 1,
            last_event_seq: 42,
            last_checkpoint_at_epoch: 0,
            drift_backfill_tasks_created: 0,
            control_turns_sent: 0,
            last_control_turn_at_epoch: 0,
            last_control_objective_id: None,
            last_event_cursor: Some("BoardEvent:42".to_string()),
            last_event_summary: None,
            last_tick_id: None,
            blocked_reason: None,
            last_mcp_ready: Some(true),
            last_control_turn_error: None,
            last_drift_backfill_task_id: None,
            active_objective_id: None,
            phase: "observe_event".to_string(),
            context_pack_path: None,
            delegated_task_ids: Vec::new(),
            last_verified_commit: None,
            resume_instruction: "resume".to_string(),
        };
        assert!(!should_dispatch_control_turn(
            "event-wakeup",
            &snapshot,
            true
        ));
        assert!(!should_dispatch_control_turn(
            "daemon-startup",
            &snapshot,
            true
        ));
        assert!(!should_dispatch_control_turn(
            "periodic-heartbeat",
            &snapshot,
            true
        ));
        assert!(!should_dispatch_control_turn(
            "event-wakeup",
            &snapshot,
            false
        ));

        snapshot.active_objective_id = Some("abc".to_string());
        assert!(should_dispatch_control_turn(
            "event-wakeup",
            &snapshot,
            true
        ));
        snapshot.last_control_turn_at_epoch = chrono::Utc::now().timestamp();
        snapshot.last_control_objective_id = Some("previous".to_string());
        assert!(
            should_dispatch_control_turn("event-wakeup", &snapshot, true),
            "new objective must bypass the duplicate-turn rate limit"
        );
        snapshot.last_control_objective_id = Some("abc".to_string());
        assert!(
            !should_dispatch_control_turn("event-wakeup", &snapshot, true),
            "same objective should be rate-limited after a sent control turn"
        );
    }

    #[test]
    fn control_turn_retry_allows_periodic_after_slow_master_start() {
        let mut snapshot = MasterControlRuntimeSnapshot {
            queued_events: 1,
            processed_ticks: 2,
            last_event_seq: 42,
            last_checkpoint_at_epoch: 0,
            drift_backfill_tasks_created: 0,
            control_turns_sent: 0,
            last_control_turn_at_epoch: 0,
            last_control_objective_id: None,
            last_event_cursor: Some("BoardEvent:42".to_string()),
            last_event_summary: Some("BoardEvent.task_created: task_id=abc".to_string()),
            last_tick_id: None,
            blocked_reason: None,
            last_mcp_ready: Some(true),
            last_control_turn_error: Some("master slot did not become idle within 20s".to_string()),
            last_drift_backfill_task_id: None,
            active_objective_id: Some("abc".to_string()),
            phase: "classify_objective".to_string(),
            context_pack_path: Some(master_context_pack_path_for_objective(
                Path::new("/repo"),
                "abc",
            )),
            delegated_task_ids: Vec::new(),
            last_verified_commit: None,
            resume_instruction: "retry".to_string(),
        };
        assert!(should_dispatch_control_turn(
            "periodic-heartbeat",
            &snapshot,
            true
        ));

        snapshot.queued_events = 0;
        snapshot.last_control_turn_at_epoch = chrono::Utc::now().timestamp();
        assert!(!should_dispatch_control_turn(
            "periodic-heartbeat",
            &snapshot,
            true
        ));

        snapshot.last_control_turn_at_epoch =
            chrono::Utc::now().timestamp() - MASTER_ACTIVE_OBJECTIVE_HEARTBEAT_SECS;
        assert!(should_dispatch_control_turn(
            "periodic-heartbeat",
            &snapshot,
            true
        ));
    }

    #[tokio::test]
    async fn classify_preserves_active_objective_across_worker_events() {
        let root = Path::new("/repo");
        let mut snapshot = MasterControlRuntimeSnapshot {
            queued_events: 0,
            processed_ticks: 4,
            last_event_seq: 77,
            last_checkpoint_at_epoch: 0,
            drift_backfill_tasks_created: 0,
            control_turns_sent: 1,
            last_control_turn_at_epoch: 0,
            last_control_objective_id: Some("parent-objective".to_string()),
            last_event_cursor: Some("SlotEvent:77".to_string()),
            last_event_summary: Some(
                "SlotEvent.state_changed: slot_id=slot-gemini-ultra Responding->Idle".to_string(),
            ),
            last_tick_id: None,
            blocked_reason: None,
            last_mcp_ready: Some(true),
            last_control_turn_error: None,
            last_drift_backfill_task_id: None,
            active_objective_id: Some("parent-objective".to_string()),
            phase: "observe_event".to_string(),
            context_pack_path: Some(master_context_pack_path_for_objective(
                root,
                "parent-objective",
            )),
            delegated_task_ids: Vec::new(),
            last_verified_commit: None,
            resume_instruction: "resume".to_string(),
        };

        let decision = classify_master_decision_state("event-wakeup", &snapshot, root).await;
        assert_eq!(
            decision.active_objective_id.as_deref(),
            Some("parent-objective")
        );
        assert_eq!(
            decision.context_pack_path.as_deref(),
            Some(master_context_pack_path_for_objective(root, "parent-objective").as_str())
        );

        snapshot.last_event_summary =
            Some("BoardEvent.status_changed: task_id=worker-child Running->Done".to_string());
        let decision = classify_master_decision_state("event-wakeup", &snapshot, root).await;
        assert_eq!(
            decision.active_objective_id.as_deref(),
            Some("parent-objective")
        );
        assert_eq!(decision.phase, "classify_objective");

        snapshot.last_event_summary =
            Some("BoardEvent.status_changed: task_id=parent-objective Running->Done".to_string());
        let decision = classify_master_decision_state("event-wakeup", &snapshot, root).await;
        assert_eq!(decision.active_objective_id, None);
        assert_eq!(decision.phase, "observe_event");

        snapshot.active_objective_id = Some("parent-objective".to_string());
        snapshot.last_event_summary =
            Some("BoardEvent.status_changed: task_id=parent-objective Running->done".to_string());
        let decision = classify_master_decision_state("event-wakeup", &snapshot, root).await;
        assert_eq!(decision.active_objective_id, None);
        assert_eq!(decision.phase, "observe_event");
        assert!(should_consume_event_without_control(
            "event-wakeup",
            &snapshot,
            &decision
        ));

        snapshot.active_objective_id = Some("parent-objective".to_string());
        snapshot.last_event_summary =
            Some("BoardEvent.status_changed: task_id=parent-objective Done->terminal".to_string());
        let decision = classify_master_decision_state("periodic-heartbeat", &snapshot, root).await;
        assert_eq!(decision.active_objective_id, None);
        assert_eq!(decision.phase, "observe_event");
        assert_eq!(decision.context_pack_path, None);

        snapshot.active_objective_id = None;
        let decision = classify_master_decision_state("daemon-startup", &snapshot, root).await;
        assert_eq!(decision.active_objective_id, None);
    }

    #[tokio::test]
    async fn note_events_do_not_create_master_objectives() {
        let root = Path::new("/repo");
        let snapshot = MasterControlRuntimeSnapshot {
            queued_events: 1,
            processed_ticks: 1,
            last_event_seq: 88,
            last_checkpoint_at_epoch: 0,
            drift_backfill_tasks_created: 0,
            control_turns_sent: 0,
            last_control_turn_at_epoch: 0,
            last_control_objective_id: None,
            last_event_cursor: Some("BoardEvent:88".to_string()),
            last_event_summary: Some(
                "BoardEvent.note_added: task_id=smoke-task note_id=n preview=done".to_string(),
            ),
            last_tick_id: None,
            blocked_reason: None,
            last_mcp_ready: Some(true),
            last_control_turn_error: None,
            last_drift_backfill_task_id: None,
            active_objective_id: None,
            phase: "observe_event".to_string(),
            context_pack_path: None,
            delegated_task_ids: Vec::new(),
            last_verified_commit: None,
            resume_instruction: "resume".to_string(),
        };

        let decision = classify_master_decision_state("event-wakeup", &snapshot, root).await;
        assert_eq!(decision.active_objective_id, None);
        assert_eq!(decision.context_pack_path, None);
    }

    #[test]
    fn test_and_smoke_board_events_do_not_wake_master() {
        assert!(!should_wake_for_board_event(
            &BoardEvent::TaskCreated {
                task_id: "t".to_string(),
                title: "[smoke] board alias + large note receipt".to_string(),
                category: "test".to_string(),
            },
            true,
        ));
        assert!(!should_wake_for_board_event(
            &BoardEvent::Updated {
                task_id: "t".to_string(),
                status: "running".to_string(),
                category: "smoke".to_string(),
            },
            true,
        ));
        assert!(event_summary_can_start_objective(
            "BoardEvent.task_created: task_id=real category=dev title=Run M6 SSOT convergence"
        ));
        assert!(!event_summary_can_start_objective(
            "BoardEvent.task_created: task_id=smoke category=test title=[smoke] large note debug"
        ));
    }

    #[test]
    fn terminal_board_task_statuses_clear_active_objectives() {
        assert!(is_terminal_board_task_status(&BoardTaskStatus::Done));
        assert!(is_terminal_board_task_status(&BoardTaskStatus::Failed));
        assert!(is_terminal_board_task_status(&BoardTaskStatus::Blocked));
        assert!(is_terminal_board_task_status(&BoardTaskStatus::Skipped));
        assert!(!is_terminal_board_task_status(&BoardTaskStatus::Open));
        assert!(!is_terminal_board_task_status(&BoardTaskStatus::Running));
        assert!(!is_terminal_board_task_status(&BoardTaskStatus::Verifying));
    }

    #[test]
    fn master_tick_prompt_is_mcp_first_and_event_scoped() {
        let snapshot = MasterControlRuntimeSnapshot {
            queued_events: 2,
            processed_ticks: 1,
            last_event_seq: 42,
            last_checkpoint_at_epoch: 0,
            drift_backfill_tasks_created: 0,
            control_turns_sent: 0,
            last_control_turn_at_epoch: 0,
            last_control_objective_id: None,
            last_event_cursor: Some("BoardEvent:42".to_string()),
            last_event_summary: Some("BoardEvent.task_created: task_id=abc".to_string()),
            last_tick_id: None,
            blocked_reason: None,
            last_mcp_ready: Some(true),
            last_control_turn_error: None,
            last_drift_backfill_task_id: None,
            active_objective_id: Some("abc".to_string()),
            phase: "classify_objective".to_string(),
            context_pack_path: Some(master_context_pack_path_for_objective(
                Path::new("/repo"),
                "abc",
            )),
            delegated_task_ids: Vec::new(),
            last_verified_commit: None,
            resume_instruction: "resume".to_string(),
        };
        let prompt = build_master_tick_prompt(&snapshot, "event-wakeup", true, None);
        assert!(prompt.contains("event_summary: BoardEvent.task_created: task_id=abc"));
        assert!(prompt.contains("phase: classify_objective"));
        assert!(prompt.contains("active_objective_id: abc"));
        assert!(prompt.contains("Unknowns-first intake"));
        assert!(prompt.contains("我还有哪些不知道的信息"));
        assert!(prompt.contains("Intent inference"));
        assert!(prompt.contains("真实意图"));
        assert!(prompt.contains("intent_memory_candidate"));
        assert!(prompt.contains("mission_kb_remember"));
        assert!(prompt.contains("category=memory:decision"));
        assert!(prompt.contains("Heuristic review question"));
        assert!(prompt.contains("颗粒度是否足够细"));
        assert!(prompt.contains("哪些架构可以更优雅"));
        assert!(prompt.contains("If active_objective_id is none, return decision=no-op"));
        assert!(prompt.contains("The active BoardTask above is the only load-bearing objective"));
        assert!(prompt.contains("query that task by id"));
        assert!(prompt.contains("produce unknowns, inferred_user_intent, evidence_needed"));
        assert!(prompt.contains("exact-shard-ready=true"));
        assert!(prompt.contains("Hard safety/tool rules are enforced by MissionD runtime metadata"));
        assert!(prompt.contains("workflow.lisp"));
        assert!(prompt.contains("PTY recognition is diagnostic only"));
        assert!(prompt.contains("mission_swarm_run"));
        assert!(prompt.contains("mission_task_delegate"));
        assert!(prompt.contains("decision:"));
        assert!(prompt.contains("reasoning_summary:"));
        assert!(prompt.contains("unknowns:"));
        assert!(prompt.contains("inferred_user_intent:"));
        assert!(prompt.contains("intent_memory_candidate:"));
        assert!(prompt.contains("evidence_needed:"));
        assert!(prompt.contains("delegation_plan?:"));
        assert!(prompt.contains("next_question_or_action:"));
        assert!(prompt.contains("close_or_backfill"));
        assert!(!prompt.contains("Do not call mission_kb_query"));
        assert!(!prompt.contains("Do not call mission_daemon_update"));
    }

    #[test]
    fn master_tick_prompt_embeds_active_boardtask_and_child_creation_rule() {
        let snapshot = MasterControlRuntimeSnapshot {
            queued_events: 1,
            processed_ticks: 1,
            last_event_seq: 7,
            last_checkpoint_at_epoch: 0,
            drift_backfill_tasks_created: 0,
            control_turns_sent: 0,
            last_control_turn_at_epoch: 0,
            last_control_objective_id: None,
            last_event_cursor: Some("BoardEvent:7".to_string()),
            last_event_summary: Some("BoardEvent.task_created: task_id=auth-parent".to_string()),
            last_tick_id: None,
            blocked_reason: None,
            last_mcp_ready: Some(true),
            last_control_turn_error: None,
            last_drift_backfill_task_id: None,
            active_objective_id: Some("auth-parent".to_string()),
            phase: "classify_objective".to_string(),
            context_pack_path: Some(master_context_pack_path_for_objective(
                Path::new("/repo"),
                "auth-parent",
            )),
            delegated_task_ids: Vec::new(),
            last_verified_commit: None,
            resume_instruction: "resume".to_string(),
        };
        let active = ActiveObjectivePromptContext {
            id: "auth-parent".to_string(),
            title: "Run Auth M6 SSOT convergence under resident master".to_string(),
            status: "open".to_string(),
            project: Some("auth".to_string()),
            parent_id: None,
            description_excerpt:
                "Auth must reach M6 with tenant/application/product/user-group structure."
                    .to_string(),
        };
        let prompt = build_master_tick_prompt(&snapshot, "event-wakeup", true, Some(&active));

        assert!(prompt.contains("active_boardtask:"));
        assert!(prompt.contains("- id: auth-parent"));
        assert!(prompt.contains("- project: auth"));
        assert!(prompt.contains("Auth must reach M6"));
        assert!(prompt.contains("If active_objective_id is none, return decision=no-op"));
        assert!(prompt.contains("set parentId to the active BoardTask id"));
        assert!(prompt.contains("evidence_needed:"));
    }

    #[test]
    fn master_context_pack_records_architectural_prompt_contract() {
        let snapshot = MasterControlRuntimeSnapshot {
            queued_events: 1,
            processed_ticks: 7,
            last_event_seq: 99,
            last_checkpoint_at_epoch: 0,
            drift_backfill_tasks_created: 0,
            control_turns_sent: 1,
            last_control_turn_at_epoch: 0,
            last_control_objective_id: None,
            last_event_cursor: Some("BoardEvent:99".to_string()),
            last_event_summary: Some("BoardEvent.task_created: task_id=abc".to_string()),
            last_tick_id: None,
            blocked_reason: None,
            last_mcp_ready: Some(true),
            last_control_turn_error: None,
            last_drift_backfill_task_id: None,
            active_objective_id: Some("abc".to_string()),
            phase: "classify_objective".to_string(),
            context_pack_path: Some(master_context_pack_path_for_objective(
                Path::new("/repo"),
                "abc",
            )),
            delegated_task_ids: Vec::new(),
            last_verified_commit: None,
            resume_instruction: "resume from durable evidence".to_string(),
        };
        let rendered = render_master_context_pack(&MasterContextPackRender {
            reason: "event-wakeup",
            tick_id: "tick-1",
            snapshot: &snapshot,
        });
        assert!(rendered.contains("missiond.master-control-context-pack.v1"));
        assert!(rendered.contains(":prompt-style missiond-v3-ssot-review"));
        assert!(rendered.contains(":authority [missiond-v3-ssot-lisp skill-operational-facts checker-result final-convergence-static recent-v3-commit active-boardtask-description]"));
        assert!(rendered.contains(":active-objective-contract [intent-intake-grounding unknowns-first-intake mission-context-gather intent-inference intent-memory-capture plan-authoring-worker boardtask-description-overrides-default-self-review read-only-declared-project-roots mutation-before-decision-return]"));
        assert!(rendered.contains(":excluded-default-inputs [kb board-backlog event-history provider-durable-log historical-conversation]"));
        assert!(rendered.contains("start-advance-mission-request"));
        assert!(rendered.contains("write-intent-memory"));
        assert!(rendered.contains("scoped-grounding-query"));
        assert!(rendered.contains("no-broad-kb-preload"));
        assert!(rendered.contains("board-mcp-mutation-before-final"));
        assert!(rendered.contains("pty-diagnostic-only"));
        assert!(rendered.contains("no-direct-code-edit"));
    }

    #[test]
    fn codex_master_model_guard_detects_downgrade_footer() {
        let good = "│ model:     gpt-5.5 xhigh   /model to change │\n";
        assert_eq!(codex_master_model_mismatch(good), None);

        let downgraded = "  gpt-5.4-mini medium · ~/Projects/missiond\n";
        assert_eq!(
            codex_master_model_mismatch(downgraded),
            Some("gpt-5.4-mini medium · ~/Projects/missiond".to_string())
        );
    }

    #[test]
    fn slot_event_slot_id_extracts_master_slot() {
        let event = SlotEvent::BecameIdle {
            slot_id: MASTER_SLOT_ID.to_string(),
        };
        assert_eq!(slot_event_slot_id(&event), MASTER_SLOT_ID);
    }

    #[test]
    fn slot_wakeup_filter_rejects_volatile_idle_noise() {
        let idle = SlotEvent::BecameIdle {
            slot_id: "slot-claude-code-default".to_string(),
        };
        assert!(!should_wake_for_slot_event(&idle, 0));
        assert!(!should_wake_for_slot_event(&idle, 42));

        let master_changed = SlotEvent::StateChanged {
            slot_id: MASTER_SLOT_ID.to_string(),
            new_state: "Idle".to_string(),
            prev_state: "Thinking".to_string(),
        };
        assert!(!should_wake_for_slot_event(&master_changed, 42));

        let dispatched = SlotEvent::TaskDispatched {
            slot_id: "slot-claude-code-default".to_string(),
            task_id: Some("task-1".to_string()),
            purpose: "boardtask".to_string(),
            prompt_chars: 10,
            preview: "read-only".to_string(),
            cited_kb_ids: vec![],
        };
        assert!(!should_wake_for_slot_event(&dispatched, 42));

        let stuck = SlotEvent::Stuck {
            slot_id: "slot-claude-code-default".to_string(),
            reason: "no durable final".to_string(),
            last_activity_ms_ago: 120_000,
        };
        assert!(should_wake_for_slot_event(&stuck, 42));
    }

    #[test]
    fn board_wakeup_filter_keeps_worker_completion_edges() {
        let created = BoardEvent::TaskCreated {
            task_id: "t".to_string(),
            title: "new work".to_string(),
            category: "dev".to_string(),
        };
        assert!(should_wake_for_board_event(&created, false));

        let swarm_worker = BoardEvent::TaskCreated {
            task_id: "worker".to_string(),
            title: "Investigate context for swarm objective (1/2)".to_string(),
            category: "dev".to_string(),
        };
        assert!(!should_wake_for_board_event(&swarm_worker, false));

        let done = BoardEvent::StatusChanged {
            task_id: "t".to_string(),
            old_status: "Open".to_string(),
            new_status: "Done".to_string(),
        };
        assert!(should_wake_for_board_event(&done, true));
        assert!(should_wake_for_board_event(&done, false));

        let blocked = BoardEvent::StatusChanged {
            task_id: "t".to_string(),
            old_status: "Open".to_string(),
            new_status: "Blocked".to_string(),
        };
        assert!(should_wake_for_board_event(&blocked, false));

        let worker_running = BoardEvent::Updated {
            task_id: "worker".to_string(),
            status: "Running".to_string(),
            category: "dev".to_string(),
        };
        assert!(!should_wake_for_board_event(&worker_running, false));

        let worker_done = BoardEvent::Updated {
            task_id: "worker".to_string(),
            status: "Done".to_string(),
            category: "dev".to_string(),
        };
        assert!(should_wake_for_board_event(&worker_done, false));
    }

    #[test]
    fn master_note_author_aliases_are_self_notifications() {
        assert!(is_master_control_note_author(Some("codex-master-control")));
        assert!(is_master_control_note_author(Some("resident-codex-master")));
        assert!(is_master_control_note_author(Some("resident-master")));
        assert!(!is_master_control_note_author(Some("autopilot")));
        assert!(!is_master_control_note_author(None));
    }

    #[test]
    fn master_event_subscriber_is_live_only_and_per_event_flush() {
        assert!(MASTER_BOARD_SUBSCRIPTION.ends_with("_v2_live"));
        assert!(MASTER_SLOT_SUBSCRIPTION.ends_with("_v2_live"));
        assert!(MASTER_QUESTION_SUBSCRIPTION.ends_with("_v2_live"));

        let opts = master_live_subscription_opts();
        assert_eq!(opts.start_from, StartFrom::Latest);
        assert!(matches!(opts.cursor_flush, CursorFlush::PerEvent));
        assert_eq!(opts.consumer_name, MASTER_EVENT_SUBSCRIBER_CONSUMER);
    }

    #[test]
    fn code_first_drift_predicate_requires_code_without_lisp() {
        let code_only = vec!["packages/board/src/App.tsx".to_string()];
        assert!(has_code_surface_delta(&code_only));
        assert!(!has_lisp_or_evidence_delta(&code_only));

        let covered = vec![
            "packages/board/src/App.tsx".to_string(),
            ".missiond/frontend/board-blueprint.lisp".to_string(),
        ];
        assert!(has_code_surface_delta(&covered));
        assert!(has_lisp_or_evidence_delta(&covered));
    }

    #[test]
    fn runtime_load_explanation_points_to_internal_hot_loop_signals() {
        let stats = json!({
            "events": { "estimated_backlog": 3 },
            "autopilot": { "avg_ms": 0 },
            "db_exec": { "p95_us": 0 },
            "prefetch": { "total": 0, "router": { "errors": 0 } }
        });
        let lisp = json!({
            "reportDirs": { "totalRecentReports5m": 4, "overLimitProjects": 0 },
            "stormCircuitHits": 1,
            "recentSyncTaskCreations": 5
        });
        let shared = json!({
            "activeWorkflowRuns": 0,
            "staleClaims": 0,
            "cursorLag": []
        });
        let nightly = json!({
            "lastRunAtEpoch": 0,
            "lastFindingsCount": 0
        });

        let explanation = runtime_load_explanation(&stats, &lisp, &shared, &nightly);
        assert_eq!(explanation["status"], "diagnostic_required");
        let suspects = explanation["suspects"].as_array().unwrap();
        assert!(suspects.iter().any(|s| s["component"] == "lisp-code-sync"));
        assert!(suspects.iter().any(|s| s["component"] == "eventbus"));
    }
}
