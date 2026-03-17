use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use missiond_core::{
    CorePermissionDecision, MissionControl, PermissionPolicy,
    PTYManager, SkillIndex, InfraConfig, CCTasksWatcher, DbExecutor,
};
use tokio::sync::Mutex;

use crate::control_tree::ControlManager;
use crate::daemon_stats::DaemonStats;
use crate::event_bus::EventBus;
use crate::gemini_client::GeminiClient;
use crate::mcp_client::McpProcessClient;
use crate::minimax_gateway::MinimaxHandle;
use crate::sonnet_gateway::SonnetHandle;
use crate::prompts::PromptStore;
use crate::slot_dispatch::SlotDispatchGuard;
use crate::workers::WorkerRegistry;

// --- Well-known slot IDs (shared across all daemon modules) ---
pub(crate) const MEMORY_SLOT_ID: &str = "slot-memory";           // Fast lane (realtime)
pub(crate) const MEMORY_SLOW_SLOT_ID: &str = "slot-memory-slow";  // Slow lane (deep + consolidation)
pub(crate) const SUPERVISOR_SLOT_ID: &str = "slot-supervisor";    // Supervisor (Opus patrol)


/// Extraction phase state machine. Replaces rigid 120s cooldown with
/// event-driven completion detection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ExtractionPhase {
    /// Ready for next extraction trigger.
    Idle,
    /// send() is in flight (waiting for TextComplete).
    Sending,
    /// send() returned but slot is still processing MCP calls.
    /// Will transition to Idle when slot's SessionState becomes Idle.
    WaitingForSlotIdle,
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
}

/// Deep analysis schema version. Bump this when the analysis prompt changes
/// to trigger re-analysis of all previously analyzed conversations.
pub(crate) const CURRENT_ANALYSIS_VERSION: i32 = 1;
/// Max retries for a single conversation's deep analysis before giving up.
pub(crate) const MAX_ANALYSIS_RETRIES: i32 = 2;

/// Safety valve: max time to wait for slot to return to Idle after send() returns.
pub(crate) const MAX_WAIT_FOR_IDLE_SECS: i64 = 900;

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
    pub(crate) mission: Arc<MissionControl>,
    pub(crate) permission: Arc<PermissionPolicy>,
    pub(crate) pty: Arc<PTYManager>,
    pub(crate) cc_tasks: Arc<Mutex<CCTasksWatcher>>,
    pub(crate) skills: Arc<SkillIndex>,
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
    /// Pause switch for memory extraction tasks (realtime, deep_analysis, sync, GC).
    pub(crate) memory_paused: Arc<std::sync::atomic::AtomicBool>,
    /// Epoch secs when memory was paused. 0 = not paused. Used for TTL auto-resume.
    pub(crate) memory_paused_at: Arc<std::sync::atomic::AtomicI64>,
    /// Pause switch for ALL autopilot tasks (board tasks, submit tasks).
    pub(crate) global_paused: Arc<std::sync::atomic::AtomicBool>,
    /// Epoch secs when global pause was activated. 0 = not paused.
    pub(crate) global_paused_at: Arc<std::sync::atomic::AtomicI64>,
    /// Per-slot consecutive failure count for autopilot throttling.
    pub(crate) slot_fail_counts: Arc<std::sync::Mutex<HashMap<String, (i32, i64)>>>,  // (count, last_fail_at)
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
    /// In-memory multi-topic embedding cache for conversation search (session_id, [topic_vectors]).
    /// MaxSim: search score = max(cosine(query, topic_i)) per session.
    pub(crate) conversation_topic_cache: missiond_core::embedding::TopicCache,
    /// In-memory embedding cache for skill topics (topic_name, vector).
    pub(crate) skill_embedding_cache: missiond_core::embedding::EmbeddingCache,
    /// In-memory embedding cache for ALL KB entries (for hybrid search).
    pub(crate) kb_search_cache: missiond_core::embedding::EmbeddingCache,
    /// Embedding worker channel: event-driven summary + embedding generation.
    pub(crate) embedding_tx: tokio::sync::mpsc::Sender<EmbeddingTask>,
    /// AIOps: incident event bus sender (try_send only, capacity 100).
    pub(crate) incident_tx: tokio::sync::mpsc::Sender<missiond_core::types::MissionIncident>,
    /// Centralized event bus for inter-module communication (replaces Notify signals).
    pub(crate) event_bus: Arc<EventBus>,
    /// Async DB executor — offloads hot-path SQLite calls to spawn_blocking.
    pub(crate) db_exec: DbExecutor,
    /// Process-level daemon statistics (counters + histograms).
    pub(crate) stats: Arc<DaemonStats>,
    /// Centralized LLM prompts with file-based hot-reload.
    pub(crate) prompts: Arc<PromptStore>,
    /// Wakeup signal for briefing worker when a long conversation message is logged.
    pub(crate) briefing_notify: Arc<tokio::sync::Notify>,
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
    /// Per-slot dispatch guard — prevents concurrent dispatch to the same PTY slot.
    pub(crate) slot_dispatch: Arc<SlotDispatchGuard>,
    /// Wakeup signal for board dispatch when a slot becomes idle.
    pub(crate) board_dispatch_notify: Arc<tokio::sync::Notify>,
    /// Gemini watch: background health probe active flag.
    pub(crate) gemini_watch_active: Arc<std::sync::atomic::AtomicBool>,
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
    pub(crate) session_task_bindings: Arc<std::sync::Mutex<HashMap<String, Vec<SessionTaskBinding>>>>,
    /// Per-file async lock for sys_config patch operations (prevents TOCTOU races).
    pub(crate) config_file_locks: Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    /// In-memory async job store — tracks long-running operations.
    pub(crate) job_store: Arc<tokio::sync::RwLock<HashMap<String, missiond_core::types::AsyncJob>>>,
    /// Embedding backfill enabled flag (from llm.yaml `backfill_enabled`).
    pub(crate) backfill_enabled: Arc<std::sync::atomic::AtomicBool>,
}

/// Event-driven embedding tasks — the Worker sleeps until triggered.
#[derive(Debug, Clone)]
pub(crate) enum EmbeddingTask {
    /// Generate summary + embedding for a single completed session.
    ProcessSession(String),
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
}

/// Backfill phases — processed in order, each yields between batches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackfillPhase {
    KbStale, KbMissing, SkillStale, SkillMissing,
    ConvTopicVectors, ConvSummary, ConvRetry, AstNodes, Timeline,
}

impl BackfillPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::KbStale => "kb_stale", Self::KbMissing => "kb_missing",
            Self::SkillStale => "skill_stale", Self::SkillMissing => "skill_missing",
            Self::ConvTopicVectors => "conv_topic_vectors", Self::ConvSummary => "conv_summary",
            Self::ConvRetry => "conv_retry", Self::AstNodes => "ast_nodes", Self::Timeline => "timeline",
        }
    }
    pub fn next(&self) -> Option<Self> {
        match self {
            Self::KbStale => Some(Self::KbMissing), Self::KbMissing => Some(Self::SkillStale),
            Self::SkillStale => Some(Self::SkillMissing), Self::SkillMissing => Some(Self::ConvTopicVectors),
            Self::ConvTopicVectors => Some(Self::ConvSummary), Self::ConvSummary => Some(Self::ConvRetry),
            Self::ConvRetry => Some(Self::AstNodes), Self::AstNodes => Some(Self::Timeline), Self::Timeline => None,
        }
    }
    pub fn first() -> Self { Self::KbStale }
    pub fn all() -> &'static [Self] {
        &[Self::KbStale, Self::KbMissing, Self::SkillStale, Self::SkillMissing,
          Self::ConvTopicVectors, Self::ConvSummary, Self::ConvRetry, Self::AstNodes, Self::Timeline]
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

