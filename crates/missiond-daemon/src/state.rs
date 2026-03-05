use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use missiond_core::{
    CorePermissionDecision, MissionControl, PermissionPolicy,
    PTYManager, SkillIndex, InfraConfig, CCTasksWatcher,
};
use tokio::sync::Mutex;

use crate::mcp_client::McpProcessClient;


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


#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) mission: Arc<MissionControl>,
    pub(crate) permission: Arc<PermissionPolicy>,
    pub(crate) pty: Arc<PTYManager>,
    pub(crate) cc_tasks: Arc<Mutex<CCTasksWatcher>>,
    pub(crate) skills: Arc<SkillIndex>,
    pub(crate) infra: Arc<InfraConfig>,
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
    /// Signal to re-check fast lane extractions immediately.
    pub(crate) extraction_notify: Arc<tokio::sync::Notify>,
    /// Signal to re-check slow lane extractions immediately.
    pub(crate) slow_extraction_notify: Arc<tokio::sync::Notify>,
    /// Signal to re-dispatch queued submit tasks (fired on task creation/completion).
    pub(crate) submit_notify: Arc<tokio::sync::Notify>,
    /// Signal for Decision Engine: re-check target=master pending questions immediately.
    pub(crate) decision_notify: Arc<tokio::sync::Notify>,
    /// Last KB auto-GC timestamp (epoch secs). 0 = never run.
    pub(crate) last_auto_gc_at: Arc<std::sync::atomic::AtomicI64>,
    /// Last KB consolidation timestamp (epoch secs). 0 = never run.
    pub(crate) last_kb_consolidation_at: Arc<std::sync::atomic::AtomicI64>,
    /// Last decision harvest checkpoint timestamp (epoch secs). 0 = never run.
    pub(crate) last_decision_harvest_at: Arc<std::sync::atomic::AtomicI64>,
    /// Last supervisor patrol timestamp (epoch secs). 0 = never run.
    pub(crate) last_supervisor_patrol_at: Arc<std::sync::atomic::AtomicI64>,
    /// Pause switch for memory extraction tasks (realtime, deep_analysis, sync, GC).
    pub(crate) memory_paused: Arc<std::sync::atomic::AtomicBool>,
    /// Epoch secs when memory was paused. 0 = not paused. Used for TTL auto-resume.
    pub(crate) memory_paused_at: Arc<std::sync::atomic::AtomicI64>,
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
    /// Persistent xjp-mcp client (lazy-initialized, auto-reconnect on crash).
    pub(crate) xjp_mcp: Arc<McpProcessClient>,
    /// Flow engine reentry guard: task IDs currently being processed.
    pub(crate) flow_in_progress: Arc<std::sync::Mutex<HashSet<String>>>,
    /// Embedding service for semantic search (None if feature disabled or init failed).
    /// Supports pluggable providers: OllamaProvider (preferred) or FastEmbed (fallback).
    pub(crate) embedding_service: Option<Arc<dyn missiond_core::embedding::EmbeddingProvider>>,
    /// In-memory embedding cache for policy:decision entries (id, vector).
    pub(crate) embedding_cache: missiond_core::embedding::EmbeddingCache,
    /// In-memory embedding cache for conversation summaries (session_id, vector).
    pub(crate) conversation_embedding_cache: missiond_core::embedding::EmbeddingCache,
    /// In-memory embedding cache for skill topics (topic_name, vector).
    pub(crate) skill_embedding_cache: missiond_core::embedding::EmbeddingCache,
    /// In-memory embedding cache for ALL KB entries (for hybrid search).
    pub(crate) kb_search_cache: missiond_core::embedding::EmbeddingCache,
    /// Embedding worker channel: event-driven summary + embedding generation.
    pub(crate) embedding_tx: tokio::sync::mpsc::Sender<EmbeddingTask>,
    /// AIOps: incident event bus sender (try_send only, capacity 100).
    pub(crate) incident_tx: tokio::sync::mpsc::Sender<missiond_core::types::MissionIncident>,
}

/// Event-driven embedding tasks — the Worker sleeps until triggered.
#[derive(Debug)]
pub(crate) enum EmbeddingTask {
    /// Generate summary + embedding for a single completed session.
    ProcessSession(String),
    /// Incremental: embed a single KB entry after remember/update.
    ProcessKBEntry(String),
    /// Incremental: embed a single Skill topic after upsert.
    ProcessSkillTopic(String),
    /// Batch backfill all systems: KB → Skills → Conversations.
    BackfillAll,
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

