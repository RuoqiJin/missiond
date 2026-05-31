use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use missiond_core::db::traits::MissionStore;
use missiond_core::{
    CCTasksWatcher, CorePermissionDecision, InfraConfig, MissionControl, PTYManager,
    PermissionPolicy, SkillIndex, SkillMeta,
};
use tokio::sync::Mutex;

use missiond_core::types::SharedProjectRegistry;

use crate::app_ports::StorePorts;
use crate::bus::BusServices;
use crate::control_tree::ControlManager;
use crate::daemon_stats::DaemonStats;
use crate::gemini_client::GeminiClient;
use crate::mcp_client::McpProcessClient;
use crate::minimax_gateway::MinimaxHandle;
use crate::prompts::PromptStore;
use crate::runtime_actors::{ExtractionLaneHandle, ExtractionLaneKind, SlotActorHandle};
use crate::slot_dispatch::SlotDispatchGuard;
use crate::slot_orchestrator::AgentSlotManager;
use crate::sonnet_gateway::SonnetHandle;
use crate::workers::WorkerRegistry;

// --- Well-known slot IDs (shared across all daemon modules) ---
pub(crate) const MEMORY_SLOT_ID: &str = "slot-memory"; // Fast lane (realtime)
pub(crate) const MEMORY_SLOW_SLOT_ID: &str = "slot-memory-slow"; // Slow lane (deep + consolidation)
pub(crate) const SUPERVISOR_SLOT_ID: &str = "slot-supervisor"; // Supervisor (Opus patrol)

#[derive(Clone)]
pub(crate) struct SharedSkillIndex {
    inner: Arc<std::sync::RwLock<SkillIndex>>,
}

impl SharedSkillIndex {
    pub(crate) fn new(index: SkillIndex) -> Self {
        Self {
            inner: Arc::new(std::sync::RwLock::new(index)),
        }
    }

    pub(crate) fn replace(&self, index: SkillIndex) {
        *self.write() = index;
    }

    pub(crate) fn list(&self) -> Vec<SkillMeta> {
        self.read().list().to_vec()
    }

    pub(crate) fn search(&self, query: &str) -> Vec<SkillMeta> {
        self.read()
            .search(query)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
    }

    pub(crate) fn get(&self, name: &str) -> Option<SkillMeta> {
        self.read().get(name).cloned()
    }

    pub(crate) fn build_context(&self, query: &str) -> String {
        self.read().build_context(query)
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, SkillIndex> {
        self.inner.read().unwrap_or_else(|err| err.into_inner())
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, SkillIndex> {
        self.inner.write().unwrap_or_else(|err| err.into_inner())
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct RuntimePaths {
    pub(crate) home: PathBuf,
    pub(crate) project_root: PathBuf,
    pub(crate) slots_config: PathBuf,
    pub(crate) permission_config: PathBuf,
    pub(crate) learned_permissions: PathBuf,
    pub(crate) logs_dir: PathBuf,
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct StorageContext {
    pub(crate) store: Arc<dyn MissionStore>,
    pub(crate) bus: Arc<BusServices>,
    pub(crate) shared_memory: Arc<crate::engine::shared_memory::SharedMemoryService>,
    pub(crate) codex_replay: Arc<crate::engine::codex_replay::CodexReplayService>,
    pub(crate) provider_box: Arc<crate::provider_box::ProviderInteractionBox>,
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct SlotContext {
    pub(crate) mission: Arc<MissionControl>,
    pub(crate) pty: Arc<PTYManager>,
    pub(crate) slot_manager: Arc<AgentSlotManager>,
    pub(crate) slot_dispatch: Arc<SlotDispatchGuard>,
    pub(crate) pty_session_uuids: Arc<tokio::sync::RwLock<HashSet<String>>>,
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct WorkerContextState {
    pub(crate) registry: Arc<WorkerRegistry>,
    pub(crate) board_dispatch_notify: Arc<tokio::sync::Notify>,
    pub(crate) strategy_notify: Arc<tokio::sync::Notify>,
    pub(crate) retro_notify: Arc<tokio::sync::Notify>,
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct LlmContext {
    pub(crate) http_client: reqwest::Client,
    pub(crate) gemini: GeminiClient,
    pub(crate) minimax: Option<MinimaxHandle>,
    pub(crate) sonnet: Option<SonnetHandle>,
    pub(crate) prompts: Arc<PromptStore>,
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct ControlPlaneContext {
    pub(crate) permission: Arc<PermissionPolicy>,
    pub(crate) control_manager: Arc<ControlManager>,
    pub(crate) project_registry: SharedProjectRegistry,
    pub(crate) stats: Arc<DaemonStats>,
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct StoragePlane {
    pub(crate) store: Arc<dyn MissionStore>,
    pub(crate) ports: StorePorts,
    pub(crate) shared_memory: Arc<crate::engine::shared_memory::SharedMemoryService>,
    pub(crate) codex_replay: Arc<crate::engine::codex_replay::CodexReplayService>,
    pub(crate) provider_box: Arc<crate::provider_box::ProviderInteractionBox>,
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct EventPlane {
    pub(crate) bus: Arc<BusServices>,
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct SlotPlane {
    pub(crate) mission: Arc<MissionControl>,
    pub(crate) pty: Arc<PTYManager>,
    pub(crate) slot_manager: Arc<AgentSlotManager>,
    pub(crate) slot_dispatch: Arc<SlotDispatchGuard>,
    pub(crate) pty_session_uuids: Arc<tokio::sync::RwLock<HashSet<String>>>,
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct WorkerPlane {
    pub(crate) registry: Arc<WorkerRegistry>,
    pub(crate) board_dispatch_notify: Arc<tokio::sync::Notify>,
    pub(crate) strategy_notify: Arc<tokio::sync::Notify>,
    pub(crate) retro_notify: Arc<tokio::sync::Notify>,
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct KnowledgePlane {
    pub(crate) skills: SharedSkillIndex,
    pub(crate) shared_memory: Arc<crate::engine::shared_memory::SharedMemoryService>,
    pub(crate) project_registry: SharedProjectRegistry,
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct ObservabilityPlane {
    pub(crate) stats: Arc<DaemonStats>,
    pub(crate) bus: Arc<BusServices>,
    pub(crate) worker_registry: Arc<WorkerRegistry>,
}

pub(crate) struct AppStateContextBundle {
    pub(crate) runtime_paths: RuntimePaths,
    pub(crate) storage_ctx: StorageContext,
    pub(crate) slot_ctx: SlotContext,
    pub(crate) worker_ctx: WorkerContextState,
    pub(crate) llm_ctx: LlmContext,
    pub(crate) control_ctx: ControlPlaneContext,
}

pub(crate) struct AppStateBuilder;

impl AppStateBuilder {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn contexts(
        runtime_paths: RuntimePaths,
        storage_ctx: StorageContext,
        slot_ctx: SlotContext,
        worker_ctx: WorkerContextState,
        llm_ctx: LlmContext,
        control_ctx: ControlPlaneContext,
    ) -> AppStateContextBundle {
        AppStateContextBundle {
            runtime_paths,
            storage_ctx,
            slot_ctx,
            worker_ctx,
            llm_ctx,
            control_ctx,
        }
    }
}

/// Extraction phase state machine. Replaces rigid 120s cooldown with
/// event-driven completion detection.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub(crate) enum ExtractionPhase {
    /// Ready for next extraction trigger.
    Idle,
    /// send() is in flight (waiting for TextComplete).
    Sending,
    /// send() returned but slot is still processing MCP calls.
    /// Will transition to Idle when slot's SessionState becomes Idle.
    WaitingForSlotIdle,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InputSkipDiagnostic {
    pub(crate) reason: String,
    pub(crate) count: u32,
    pub(crate) last_seen_at: i64,
}

pub(crate) struct ExtractionState {
    pub(crate) phase: ExtractionPhase,
    /// Which extraction type is active: "realtime", "deep_analysis", "kb_consolidation"
    pub(crate) active_type: Option<&'static str>,
    /// When current phase started (epoch secs), for timeout detection.
    pub(crate) phase_started_at: i64,
    /// Conversation ID currently being deep-analyzed (for marking complete on Idle).
    pub(crate) current_deep_conv_id: Option<String>,
    /// Watermark targets for realtime extraction: (session_id, max_timestamp).
    /// On completion, update realtime_forwarded_at for each session.
    pub(crate) watermark_targets: Vec<(String, String)>,
    /// Task ID for the current extraction job (survives compaction).
    pub(crate) current_task_id: Option<String>,
    /// slot_tasks table row ID for the current extraction (for history tracking).
    pub(crate) current_slot_task_id: Option<String>,
    /// Whether current deep analysis is a checkpoint (active session) vs full (completed session).
    pub(crate) is_checkpoint: bool,
    /// Max message ID in the batch being analyzed (for advancing checkpoint watermark on completion).
    pub(crate) checkpoint_message_id: Option<i64>,
    /// Latch: mission_memory_pending already returned data in this extraction cycle.
    /// Prevents the agent from polling the same messages repeatedly (watermark advances only on completion).
    pub(crate) pending_served: bool,
    /// Cached batch id and rendered payload for bounded replay after provider context compaction.
    pub(crate) pending_batch_id: Option<String>,
    pub(crate) pending_payload: Option<String>,
    pub(crate) pending_served_at: i64,
    pub(crate) pending_replay_count: u32,
    /// Consecutive realtime scheduler probes that found no useful user-bearing work.
    /// Used for exponential idle backoff so empty queues do not keep waking workers.
    pub(crate) empty_probe_count: u32,
    /// Epoch secs before which realtime extraction should skip empty-queue probes.
    pub(crate) next_probe_after: i64,
    /// KB mutations created by the current extraction job.
    pub(crate) current_output_count: u32,
    /// Consecutive completed deep-analysis jobs that produced no KB mutations.
    pub(crate) deep_analysis_zero_output_count: u32,
    /// Epoch secs before which deep-analysis dispatch should be fused off.
    pub(crate) deep_analysis_fuse_until: i64,
    /// Explicit diagnostics for memory input filtered before agent extraction.
    pub(crate) input_skip_diagnostics: HashMap<String, InputSkipDiagnostic>,
}

impl ExtractionState {
    pub(crate) fn clear_pending_batch_replay(&mut self) {
        self.pending_served = false;
        self.pending_batch_id = None;
        self.pending_payload = None;
        self.pending_served_at = 0;
        self.pending_replay_count = 0;
    }

    pub(crate) fn mark_pending_batch_served(&mut self, batch_id: String, payload: String) {
        self.pending_served = true;
        self.pending_batch_id = Some(batch_id);
        self.pending_payload = Some(payload);
        self.pending_served_at = chrono::Utc::now().timestamp();
        self.pending_replay_count = 0;
    }

    pub(crate) fn reset_current_output_count(&mut self) {
        self.current_output_count = 0;
    }

    pub(crate) fn add_current_output_count(&mut self, count: u32) {
        self.current_output_count = self.current_output_count.saturating_add(count);
    }

    pub(crate) fn deep_analysis_fuse_active(&self, now: i64) -> bool {
        self.phase == ExtractionPhase::Idle && self.deep_analysis_fuse_until > now
    }

    pub(crate) fn record_deep_analysis_completion(
        &mut self,
        output_count: u32,
        threshold: u32,
        fuse_secs: i64,
        now: i64,
    ) -> bool {
        self.current_output_count = 0;
        if output_count > 0 {
            self.deep_analysis_zero_output_count = 0;
            self.deep_analysis_fuse_until = 0;
            return false;
        }

        self.deep_analysis_zero_output_count =
            self.deep_analysis_zero_output_count.saturating_add(1);
        if threshold > 0 && self.deep_analysis_zero_output_count >= threshold {
            self.deep_analysis_fuse_until = now.saturating_add(fuse_secs.max(1));
            true
        } else {
            false
        }
    }

    pub(crate) fn record_input_skip(&mut self, reason: &str, count: u32) {
        if count == 0 {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        let entry = self
            .input_skip_diagnostics
            .entry(reason.to_string())
            .or_insert_with(|| InputSkipDiagnostic {
                reason: reason.to_string(),
                count: 0,
                last_seen_at: now,
            });
        entry.count = entry.count.saturating_add(count);
        entry.last_seen_at = now;
    }

    pub(crate) fn input_skip_diagnostics(&self) -> Vec<InputSkipDiagnostic> {
        let mut diagnostics = self
            .input_skip_diagnostics
            .values()
            .cloned()
            .collect::<Vec<_>>();
        diagnostics.sort_by(|a, b| {
            b.last_seen_at
                .cmp(&a.last_seen_at)
                .then_with(|| a.reason.cmp(&b.reason))
        });
        diagnostics
    }
}

/// Deep analysis schema version. Bump this when the analysis prompt changes
/// to trigger re-analysis of all previously analyzed conversations.
pub(crate) const CURRENT_ANALYSIS_VERSION: i32 = 1;
/// Max retries for a single conversation's deep analysis before giving up.
pub(crate) const MAX_ANALYSIS_RETRIES: i32 = 2;

/// Safety valve: max time to wait for slot to return to Idle after send() returns.
pub(crate) const MAX_WAIT_FOR_IDLE_SECS: i64 = 900;

#[cfg(test)]
mod extraction_state_tests {
    use std::collections::HashMap;

    use super::{ExtractionPhase, ExtractionState};

    fn state() -> ExtractionState {
        ExtractionState {
            phase: ExtractionPhase::Sending,
            active_type: Some("realtime"),
            phase_started_at: 1,
            current_deep_conv_id: None,
            watermark_targets: Vec::new(),
            current_task_id: Some("task-1".to_string()),
            current_slot_task_id: Some("slot-task-1".to_string()),
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
    fn pending_batch_replay_state_is_cached_and_clearable() {
        let mut es = state();
        es.mark_pending_batch_served("batch-1".to_string(), "payload".to_string());
        assert!(es.pending_served);
        assert_eq!(es.pending_batch_id.as_deref(), Some("batch-1"));
        assert_eq!(es.pending_payload.as_deref(), Some("payload"));
        assert_eq!(es.pending_replay_count, 0);
        assert!(es.pending_served_at > 0);

        es.pending_replay_count = 2;
        es.clear_pending_batch_replay();
        assert!(!es.pending_served);
        assert!(es.pending_batch_id.is_none());
        assert!(es.pending_payload.is_none());
        assert_eq!(es.pending_served_at, 0);
        assert_eq!(es.pending_replay_count, 0);
    }

    #[test]
    fn deep_analysis_zero_output_completion_fuses_after_threshold() {
        let mut es = state();
        es.phase = ExtractionPhase::Idle;
        assert!(!es.record_deep_analysis_completion(0, 2, 60, 100));
        assert_eq!(es.deep_analysis_zero_output_count, 1);
        assert_eq!(es.deep_analysis_fuse_until, 0);

        assert!(es.record_deep_analysis_completion(0, 2, 60, 100));
        assert_eq!(es.deep_analysis_zero_output_count, 2);
        assert_eq!(es.deep_analysis_fuse_until, 160);
        assert!(es.deep_analysis_fuse_active(120));
    }

    #[test]
    fn deep_analysis_positive_output_resets_zero_output_fuse() {
        let mut es = state();
        es.deep_analysis_zero_output_count = 3;
        es.deep_analysis_fuse_until = 160;
        es.current_output_count = 5;

        assert!(!es.record_deep_analysis_completion(2, 2, 60, 100));
        assert_eq!(es.current_output_count, 0);
        assert_eq!(es.deep_analysis_zero_output_count, 0);
        assert_eq!(es.deep_analysis_fuse_until, 0);
    }

    #[test]
    fn input_skip_diagnostic_counts_are_accumulated() {
        let mut es = state();
        es.record_input_skip("deployment-monitor", 2);
        es.record_input_skip("deployment-monitor", 3);
        let diagnostics = es.input_skip_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].reason, "deployment-monitor");
        assert_eq!(diagnostics[0].count, 5);
        assert!(diagnostics[0].last_seen_at > 0);
    }
}

/// Per-slot JSONL progress tracking: tool call counts, current tool, etc.
/// Populated from tool_use/tool_result events, queried by mission_pty_status.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SlotProgress {
    pub(crate) session_id: String,
    pub(crate) tool_counts: HashMap<String, u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) current_tool: Option<CurrentToolInfo>,
    pub(crate) total_calls: u32,
    pub(crate) total_results: u32,
    pub(crate) error_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_activity: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CurrentToolInfo {
    pub(crate) name: String,
    pub(crate) started_at: String,
}

/// Implicit session→task binding: recorded when Claude Code queries/updates a Board task.
/// Used by auto-progress extraction to determine which tasks a session was working on.
#[derive(Debug, Clone)]
pub(crate) struct SessionTaskBinding {
    pub task_id: String,
    pub task_title: String,
    pub bound_at: i64,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) runtime_paths: RuntimePaths,
    pub(crate) storage_ctx: StorageContext,
    pub(crate) slot_ctx: SlotContext,
    pub(crate) worker_ctx: WorkerContextState,
    pub(crate) llm_ctx: LlmContext,
    pub(crate) control_ctx: ControlPlaneContext,
    pub(crate) startup_preflight: crate::startup_preflight::StartupPreflightReport,
    pub(crate) mission: Arc<MissionControl>,
    /// Trait-based PostgreSQL DB access.
    pub(crate) store: Arc<dyn MissionStore>,
    pub(crate) permission: Arc<PermissionPolicy>,
    pub(crate) pty: Arc<PTYManager>,
    pub(crate) cc_tasks: Arc<Mutex<CCTasksWatcher>>,
    pub(crate) skills: SharedSkillIndex,
    pub(crate) infra: Arc<std::sync::RwLock<InfraConfig>>,
    pub(crate) infra_path: std::path::PathBuf,
    /// JSONL session UUIDs belonging to PTY-managed slots.
    /// White-list: any session_id NOT in this set is a user CLI session.
    pub(crate) pty_session_uuids: Arc<tokio::sync::RwLock<HashSet<String>>>,
    /// Fast lane state machine (realtime extraction on slot-memory).
    pub(crate) extraction_state: Arc<tokio::sync::RwLock<ExtractionState>>,
    /// Slow lane state machine (deep analysis + kb consolidation on slot-memory-slow).
    pub(crate) slow_extraction_state: Arc<tokio::sync::RwLock<ExtractionState>>,
    /// Timestamp when slot-memory entered its current non-Idle state. 0 = idle.
    pub(crate) memory_slot_busy_since: Arc<std::sync::atomic::AtomicI64>,
    /// Timestamp when slot-memory-slow entered its current non-Idle state. 0 = idle.
    pub(crate) slow_slot_busy_since: Arc<std::sync::atomic::AtomicI64>,
    /// Hash of last synced CLAUDE.md managed section (to avoid unnecessary writes).
    pub(crate) claude_md_hash: Arc<std::sync::atomic::AtomicU64>,
    // Notify fields removed — replaced by EventBus (Phase 2 S3/S4)
    /// Last supervisor patrol timestamp (epoch secs). 0 = never run.
    pub(crate) last_supervisor_patrol_at: Arc<std::sync::atomic::AtomicI64>,
    // memory_paused / memory_paused_at removed — ControlTree is the single source of truth.
    // Use state.control_manager.current().is_domain_paused(CtlDomain::Memory) instead.
    /// Pause switch for ALL autopilot tasks (board tasks, submit tasks).
    pub(crate) global_paused: Arc<std::sync::atomic::AtomicBool>,
    /// Epoch secs when global pause was activated. 0 = not paused.
    pub(crate) global_paused_at: Arc<std::sync::atomic::AtomicI64>,
    /// Per-slot consecutive failure count for autopilot throttling.
    pub(crate) slot_fail_counts: Arc<std::sync::Mutex<HashMap<String, (i32, i64)>>>, // (count, last_fail_at)
    /// Per-slot current model (ANTHROPIC_MODEL) for env-change detection.
    pub(crate) slot_current_model: Arc<std::sync::Mutex<HashMap<String, String>>>,
    /// Screenshot broker for browser-based PTY screenshots.
    pub(crate) screenshot_broker: Arc<missiond_core::ws::ScreenshotBroker>,
    /// Jarvis request trace store for debugging.
    pub(crate) jarvis_trace: missiond_core::ws::JarvisTraceStore,
    /// Last complete response per slot (for submit task result tracking).
    pub(crate) slot_last_responses: Arc<tokio::sync::RwLock<HashMap<String, String>>>,
    /// Per-slot JSONL progress tracking (tool call counts, current tool, etc.).
    pub(crate) slot_progress: Arc<tokio::sync::RwLock<HashMap<String, SlotProgress>>>,
    /// Shared HTTP client for Router API calls (connection pool reuse).
    pub(crate) http_client: reqwest::Client,
    /// Rate-limited Gemini client (20 RPM, 3 concurrent, 429 auto-retry).
    pub(crate) gemini: GeminiClient,
    /// MiniMax Gateway handle — unified rate-limited access for all workers.
    pub(crate) minimax: Option<MinimaxHandle>,
    /// Sonnet Gateway handle — priority-based actor for all Sonnet API calls.
    pub(crate) sonnet: Option<SonnetHandle>,
    /// Persistent xjp-mcp client (lazy-initialized, auto-reconnect on crash).
    pub(crate) xjp_mcp: Arc<McpProcessClient>,
    /// Flow engine reentry guard: task IDs currently being processed.
    pub(crate) flow_in_progress: Arc<std::sync::Mutex<HashSet<String>>>,
    /// Embedding service for semantic search (None if feature disabled or init failed).
    /// Supports pluggable providers: OllamaProvider (preferred) or FastEmbed (fallback).
    pub(crate) embedding_service: Option<Arc<dyn missiond_core::embedding::EmbeddingProvider>>,
    /// In-memory embedding cache for policy:decision entries (id, vector).
    pub(crate) embedding_cache: missiond_core::embedding::EmbeddingCache,
    /// Removed in P3: conversation topic cache → pgvector HNSW replaces in-memory search.
    /// Kept as placeholder to avoid churn in AppState construction sites.
    #[allow(dead_code)]
    pub(crate) conversation_topic_cache: missiond_core::embedding::TopicCache,
    /// In-memory embedding cache for skill topics (topic_name, vector).
    pub(crate) skill_embedding_cache: missiond_core::embedding::EmbeddingCache,
    /// In-memory embedding cache for ALL KB entries (for hybrid search).
    pub(crate) kb_search_cache: missiond_core::embedding::EmbeddingCache,
    /// Embedding worker channel: event-driven summary + embedding generation.
    /// Kept as an internal worker queue (not a bus event) per the v2 frozen
    /// lisp §4.2.a `Domain::ALL` contract (no `EmbeddingEvent` domain exists;
    /// the domain set started at 12 and is extensible).
    pub(crate) embedding_tx: tokio::sync::mpsc::Sender<EmbeddingTask>,
    /// v2 event bus — every producer in the daemon publishes through this.
    pub(crate) bus: Arc<BusServices>,
    /// Codex app-server protocol replay runner for fixed Plan Mode automation loops.
    pub(crate) codex_replay: Arc<crate::engine::codex_replay::CodexReplayService>,
    /// Single provider CLI interaction boundary. Codex/Agy provider turns must enter here.
    pub(crate) provider_box: Arc<crate::provider_box::ProviderInteractionBox>,
    /// Durable multi-agent shared memory: event stream, artifacts, write leases,
    /// and agent cursors. This supersedes direct concurrent writes to
    /// `.missiond/tasks/**/shared-memory.lisp`; those files remain a
    /// compatibility projection only.
    pub(crate) shared_memory: Arc<crate::engine::shared_memory::SharedMemoryService>,
    /// Process-level daemon statistics (counters + histograms).
    pub(crate) stats: Arc<DaemonStats>,
    /// Centralized LLM prompts with file-based hot-reload.
    pub(crate) prompts: Arc<PromptStore>,
    /// Wakeup signal for strategy worker (SessionCompleted / sweeper reconciliation).
    pub(crate) strategy_notify: Arc<tokio::sync::Notify>,
    /// Wakeup signal for retro worker (SessionCompleted / sweeper reconciliation).
    pub(crate) retro_notify: Arc<tokio::sync::Notify>,
    /// AST sync worker channel: code indexing pipeline (P2 Holographic Context Engine).
    pub(crate) ast_sync_tx: tokio::sync::mpsc::Sender<crate::ast_sync_worker::AstSyncTask>,
    /// In-memory AST embedding cache for code prefetch hybrid search (P3).
    pub(crate) ast_embedding_cache: missiond_core::embedding::EmbeddingCache,
    /// Cache: session_id → last assistant_message span_id.
    /// Written by message_handler, read by ipc_handler for cross-lane causal linking.
    /// Links assistant_message (Chat lane) → CliRequestStarted (AI/LLM lane).
    pub(crate) last_msg_span: Arc<std::sync::Mutex<HashMap<String, String>>>,
    /// Runtime worker registry — pause/resume/stats for all background workers.
    pub(crate) worker_registry: Arc<WorkerRegistry>,
    /// Unified control tree — centralized pause/resume for all components.
    pub(crate) control_manager: Arc<ControlManager>,
    /// Project registry — path→project_id resolution + project metadata cache.
    pub(crate) project_registry: SharedProjectRegistry,
    /// Per-slot dispatch guard — prevents concurrent dispatch to the same PTY slot.
    pub(crate) slot_dispatch: Arc<SlotDispatchGuard>,
    /// Wakeup signal for board dispatch when a slot becomes idle.
    pub(crate) board_dispatch_notify: Arc<tokio::sync::Notify>,
    /// Gemini watch: background health probe active flag.
    pub(crate) gemini_watch_active: Arc<std::sync::atomic::AtomicBool>,
    /// Unified slot orchestrator — routes task_type → engine sub-manager (Claude Code / Gemini CLI).
    pub(crate) slot_manager: Arc<AgentSlotManager>,
    /// Gemini watch: abort handle for the background probe task.
    pub(crate) gemini_watch_handle: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    /// Gemini watch: attempt counter (for status reporting).
    pub(crate) gemini_watch_attempts: Arc<std::sync::atomic::AtomicU32>,
    /// Gemini watch: start timestamp (epoch secs). 0 = not running.
    pub(crate) gemini_watch_started_at: Arc<std::sync::atomic::AtomicI64>,
    /// Cache: task_id → cited KB entry IDs at dispatch time.
    /// Used by the feedback loop to adjust confidence on task success/failure.
    pub(crate) task_cited_kbs: Arc<std::sync::Mutex<HashMap<String, Vec<String>>>>,
    /// Co-occurrence cache: KB ID → top co-accessed KB IDs (refreshed every 6h).
    pub(crate) kb_cooccurrence_cache: Arc<tokio::sync::RwLock<HashMap<String, Vec<String>>>>,
    /// Slots pending graceful restart due to low context (detected "until auto-compact").
    /// Restart is deferred until the slot becomes Idle to avoid interrupting tasks.
    pub(crate) pending_compact_restart: Arc<std::sync::Mutex<HashSet<String>>>,
    /// Implicit session→task bindings: when Claude Code queries/updates a Board task,
    /// auto-record which session is working on which tasks (for auto-progress extraction).
    pub(crate) session_task_bindings:
        Arc<std::sync::Mutex<HashMap<String, Vec<SessionTaskBinding>>>>,
    /// Per-file async lock for sys_config patch operations (prevents TOCTOU races).
    pub(crate) config_file_locks:
        Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    /// In-memory async job store — tracks long-running operations.
    pub(crate) job_store: Arc<tokio::sync::RwLock<HashMap<String, missiond_core::types::AsyncJob>>>,
    /// Embedding backfill enabled flag (from llm.yaml `backfill_enabled`).
    pub(crate) backfill_enabled: Arc<std::sync::atomic::AtomicBool>,
    /// Intent Analyst enabled flag (from llm.yaml `intent_analyst_enabled`). Default: false.
    pub(crate) intent_analyst_enabled: bool,
    /// Phase 7: Last proactive trigger timestamps (epoch secs) per key.
    /// Key format: "{trigger_type}:{session_id}" for per-session cooldown.
    pub(crate) proactive_cooldowns: Arc<std::sync::Mutex<HashMap<String, i64>>>,
    /// Expectation tickets: slot spawns awaiting session_id discovery.
    /// Key: project_path, Value: (slot_id, prompt, spawn_timestamp).
    /// IngestionRouter claims these when a new session appears in the matching project.
    pub(crate) pending_slot_spawns:
        Arc<tokio::sync::RwLock<Vec<(String, String, String, tokio::time::Instant)>>>,
    /// Shared watcher-cursor map: `jsonl_path → highest-acked byte offset`.
    /// `conversation_logger` writes to this (replacing the old
    /// `cursor_ack_tx` MPSC) and the watcher persists by reading drains.
    pub(crate) conversation_cursor_map: Arc<tokio::sync::Mutex<HashMap<String, u64>>>,
}

#[allow(dead_code)]
impl AppState {
    pub(crate) fn runtime_paths(&self) -> &RuntimePaths {
        &self.runtime_paths
    }

    pub(crate) fn storage(&self) -> &StorageContext {
        &self.storage_ctx
    }

    pub(crate) fn slots(&self) -> &SlotContext {
        &self.slot_ctx
    }

    pub(crate) fn workers(&self) -> &WorkerContextState {
        &self.worker_ctx
    }

    pub(crate) fn llm(&self) -> &LlmContext {
        &self.llm_ctx
    }

    pub(crate) fn control_plane(&self) -> &ControlPlaneContext {
        &self.control_ctx
    }

    pub(crate) fn storage_plane(&self) -> StoragePlane {
        StoragePlane {
            store: Arc::clone(&self.store),
            ports: StorePorts::new(Arc::clone(&self.store)),
            shared_memory: Arc::clone(&self.shared_memory),
            codex_replay: Arc::clone(&self.codex_replay),
            provider_box: Arc::clone(&self.provider_box),
        }
    }

    pub(crate) fn event_plane(&self) -> EventPlane {
        EventPlane {
            bus: Arc::clone(&self.bus),
        }
    }

    pub(crate) fn slot_plane(&self) -> SlotPlane {
        SlotPlane {
            mission: Arc::clone(&self.mission),
            pty: Arc::clone(&self.pty),
            slot_manager: Arc::clone(&self.slot_manager),
            slot_dispatch: Arc::clone(&self.slot_dispatch),
            pty_session_uuids: Arc::clone(&self.pty_session_uuids),
        }
    }

    pub(crate) fn slot_actor(&self, slot_id: impl Into<String>) -> SlotActorHandle {
        SlotActorHandle::new(slot_id, Arc::clone(&self.pty))
    }

    pub(crate) fn worker_plane(&self) -> WorkerPlane {
        WorkerPlane {
            registry: Arc::clone(&self.worker_registry),
            board_dispatch_notify: Arc::clone(&self.board_dispatch_notify),
            strategy_notify: Arc::clone(&self.strategy_notify),
            retro_notify: Arc::clone(&self.retro_notify),
        }
    }

    pub(crate) fn knowledge_plane(&self) -> KnowledgePlane {
        KnowledgePlane {
            skills: self.skills.clone(),
            shared_memory: Arc::clone(&self.shared_memory),
            project_registry: self.project_registry.clone(),
        }
    }

    pub(crate) fn observability_plane(&self) -> ObservabilityPlane {
        ObservabilityPlane {
            stats: Arc::clone(&self.stats),
            bus: Arc::clone(&self.bus),
            worker_registry: Arc::clone(&self.worker_registry),
        }
    }

    pub(crate) fn fast_extraction_lane(&self) -> ExtractionLaneHandle {
        ExtractionLaneHandle::new(
            ExtractionLaneKind::Fast,
            Arc::clone(&self.extraction_state),
            Arc::clone(&self.memory_slot_busy_since),
        )
    }

    pub(crate) fn slow_extraction_lane(&self) -> ExtractionLaneHandle {
        ExtractionLaneHandle::new(
            ExtractionLaneKind::Slow,
            Arc::clone(&self.slow_extraction_state),
            Arc::clone(&self.slow_slot_busy_since),
        )
    }
}

/// v0.5.0: Memory-hook task submission.
///
/// Writes to `board_tasks` with `trigger_source='memory_hook'` + `category=<role>`.
/// The legacy `tasks` table is kept for pillar 二 (mission_task_submit MCP family);
/// memory pillar's 5 callers (state / kb / conversation_logger / pty_event / memory_scheduler)
/// all route through this function into `board_tasks` instead.
///
/// The `role` arg (e.g. `"memory"`) is stored as `category`; memory_scheduler uses
/// slot.config.role to match candidate slots.
pub(crate) async fn submit_task(
    store: &dyn MissionStore,
    role: &str,
    prompt: &str,
) -> anyhow::Result<String> {
    use missiond_core::types::{BoardTask, BoardTaskStatus, TaskId};

    let now_rfc = chrono::Utc::now().to_rfc3339();
    // Short preview for title (UI friendliness), full prompt stays in prompt_template.
    let title = {
        const MAX_TITLE: usize = 120;
        let first_line = prompt.lines().next().unwrap_or(prompt).trim();
        if first_line.is_empty() {
            format!("[memory] {}", &prompt[..prompt.len().min(MAX_TITLE)])
        } else if first_line.len() > MAX_TITLE {
            let mut end = MAX_TITLE;
            while !first_line.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            format!("{}…", &first_line[..end])
        } else {
            first_line.to_string()
        }
    };

    let task = BoardTask {
        id: TaskId::new(),
        title,
        description: String::new(),
        status: BoardTaskStatus::Open,
        priority: "medium".to_string(),
        category: role.to_string(),
        project: None,
        server: None,
        due_date: None,
        parent_id: None,
        assignee: None,
        auto_execute: false,
        prompt_template: Some(prompt.to_string()),
        hidden: true, // hide from default list views (memory-hook noise)
        retry_count: 0,
        max_retries: 2,
        order_idx: 0,
        created_at: now_rfc.clone(),
        updated_at: now_rfc,
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
        trigger_source: Some("memory_hook".to_string()),
        runtime_metadata: serde_json::json!({}),
        notes_count: 0,
    };

    let task_id = task.id.as_str().to_string();
    store
        .insert_board_task(&task)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create board task: {}", e))?;

    tracing::info!(task_id = %task_id, role = %role, "Memory-hook task created (board_tasks)");
    Ok(task_id)
}

/// Event-driven embedding tasks — the Worker sleeps until triggered.
#[derive(Debug, Clone)]
pub(crate) enum EmbeddingTask {
    /// Generate summary + embedding for a single completed session (legacy, reroutes to ProcessTurns if turns exist).
    ProcessSession(String),
    /// Per-turn embedding: process turns for a session after TurnExtracted event. Zero LLM.
    ProcessTurns { session_id: String },
    /// Incremental: embed a single KB entry after remember/update.
    ProcessKBEntry(String),
    /// Incremental: embed a single Skill topic after upsert.
    ProcessSkillTopic(String),
    /// Batch backfill: kicks off phase-by-phase processing with yield between batches.
    BackfillAll,
    /// Run a single batch of a specific backfill phase, then re-enqueue next batch.
    RunBackfillPhase { phase: BackfillPhase, cursor: i64 },
    /// Incremental: embed AST nodes after commit sync (P3 Holographic Context Engine).
    ProcessAstBatch(Vec<String>),
    /// Incremental: embed a single message after ConversationMessageLogged event.
    ProcessMessage {
        message_id: i64,
        session_id: String,
        role: String,
        content: String,
    },
}

/// Backfill phases — processed in order, each yields between batches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackfillPhase {
    KbStale,
    KbMissing,
    SkillStale,
    SkillMissing,
    ConvTopicVectors,
    ConvSummary,
    ConvRetry,
    AstNodes,
    Timeline,
    MessageEmbeddings,
}

impl BackfillPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::KbStale => "kb_stale",
            Self::KbMissing => "kb_missing",
            Self::SkillStale => "skill_stale",
            Self::SkillMissing => "skill_missing",
            Self::ConvTopicVectors => "conv_topic_vectors",
            Self::ConvSummary => "conv_summary",
            Self::ConvRetry => "conv_retry",
            Self::AstNodes => "ast_nodes",
            Self::Timeline => "timeline",
            Self::MessageEmbeddings => "message_embeddings",
        }
    }
    pub fn next(&self) -> Option<Self> {
        match self {
            Self::KbStale => Some(Self::KbMissing),
            Self::KbMissing => Some(Self::SkillStale),
            Self::SkillStale => Some(Self::SkillMissing),
            Self::SkillMissing => Some(Self::ConvTopicVectors),
            Self::ConvTopicVectors => Some(Self::ConvSummary),
            Self::ConvSummary => Some(Self::ConvRetry),
            Self::ConvRetry => Some(Self::AstNodes),
            Self::AstNodes => Some(Self::Timeline),
            Self::Timeline => Some(Self::MessageEmbeddings),
            Self::MessageEmbeddings => None,
        }
    }
    pub fn first() -> Self {
        Self::KbStale
    }
    pub fn all() -> &'static [Self] {
        &[
            Self::KbStale,
            Self::KbMissing,
            Self::SkillStale,
            Self::SkillMissing,
            Self::ConvTopicVectors,
            Self::ConvSummary,
            Self::ConvRetry,
            Self::AstNodes,
            Self::Timeline,
            Self::MessageEmbeddings,
        ]
    }
}

pub(crate) struct PermissionAdapter {
    pub(crate) permission: Arc<PermissionPolicy>,
}

impl missiond_core::PTYPermissionPolicy for PermissionAdapter {
    fn check_permission(
        &self,
        slot_id: &str,
        role: &str,
        tool_name: &str,
    ) -> missiond_core::pty::PermissionDecision {
        match self.permission.check_permission(slot_id, role, tool_name) {
            CorePermissionDecision::Allow => missiond_core::PermissionDecision::Allow,
            CorePermissionDecision::Confirm => missiond_core::PermissionDecision::Confirm,
            CorePermissionDecision::Deny => missiond_core::PermissionDecision::Deny,
        }
    }
}
