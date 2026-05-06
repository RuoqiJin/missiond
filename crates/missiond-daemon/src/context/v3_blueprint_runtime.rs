use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub(crate) const DEFAULT_MODEL_PROFILE: &str = "coding-default-opus-4-7";
pub(crate) const DEFAULT_TIMEOUT_SECS: i64 = 1800;
pub(crate) const MIN_TIMEOUT_SECS: i64 = 60;
pub(crate) const MAX_TIMEOUT_SECS: i64 = 7200;
pub(crate) const DEFAULT_CC_SWARM_TIMEOUT_SECS: i64 = 600;
pub(crate) const MIN_CC_SWARM_TIMEOUT_SECS: i64 = 60;
pub(crate) const MAX_CC_SWARM_TIMEOUT_SECS: i64 = 7200;
pub(crate) const DEFAULT_PTY_SEND_TIMEOUT_SECS: i64 = 300;
pub(crate) const MIN_PTY_SEND_TIMEOUT_SECS: i64 = 1;
pub(crate) const MAX_PTY_SEND_TIMEOUT_SECS: i64 = 7200;
pub(crate) const DEFAULT_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS: i64 = 60;
pub(crate) const MIN_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS: i64 = 10;
pub(crate) const MAX_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS: i64 = 600;
pub(crate) const DEFAULT_COMPUTE_PTY_SPAWN_TIMEOUT_SECS: i64 = 30;
pub(crate) const MIN_COMPUTE_PTY_SPAWN_TIMEOUT_SECS: i64 = 1;
pub(crate) const MAX_COMPUTE_PTY_SPAWN_TIMEOUT_SECS: i64 = 600;
pub(crate) const DEFAULT_MINIMAX_MODEL: &str = "MiniMax-M2.5-highspeed";
pub(crate) const DEFAULT_MINIMAX_DIRECT_HTTP_TIMEOUT_SECS: u64 = 30;
pub(crate) const DEFAULT_MINIMAX_QUOTA_THROTTLE_SECS: u64 = 60;
pub(crate) const DEFAULT_MINIMAX_MAX_TOKENS: u32 = 500;
pub(crate) const WATCHDOG_GRACE_SECS: i64 = 120;
pub(crate) const MISSING_SESSION_PROBE_SECS: i64 = 120;
pub(crate) const DEFAULT_SLOT_TTL_SECS: i64 = 14400;
pub(crate) const MIN_SLOT_TTL_SECS: i64 = 300;
pub(crate) const MAX_SLOT_TTL_SECS: i64 = 28800;
pub(crate) const DEFAULT_SLOT_EXTEND_SECS: i64 = 3600;
pub(crate) const MAX_SLOT_EXTEND_SECS: i64 = 3600;
pub(crate) const DEFAULT_SLOT_DEFAULT_CWD: &str = "/Users/jinchen/Projects";
pub(crate) const DEFAULT_SLOT_MCP_CONFIG: &str = "/Users/jinchen/.xjp-mission/xjp-mcp-config.json";
pub(crate) const DEFAULT_ALLOWED_CWD_PREFIXES: [&str; 4] = [
    "/Users/jinchen/Projects",
    "/Users/jinchen/Downloads",
    "/Users/jinchen/Documents",
    "/tmp",
];
pub(crate) const DEFAULT_FLOW_LLM_MAX_TOKENS: u32 = 65536;
pub(crate) const DEFAULT_FLOW_SLOT_MODEL: &str = "opus";
pub(crate) const DEFAULT_FLOW_SLOT_TIMEOUT_SECS: u64 = 3600;
pub(crate) const DEFAULT_FLOW_PARALLELISM: usize = 3;
pub(crate) const DEFAULT_FLOW_PARALLEL_TIMEOUT_SECS: u64 = 1800;
pub(crate) const DEFAULT_CASCADE_MANIFEST_PATH: &str =
    "/Users/jinchen/Projects/universe.intent.lisp";
pub(crate) const DEFAULT_CASCADE_ALLOWED_ROOT: &str = "/Users/jinchen/Projects";
pub(crate) const DEFAULT_CASCADE_TRIGGER_ENABLED: bool = true;
pub(crate) const DEFAULT_CASCADE_MAX_CYCLES: usize = 3;
pub(crate) const MAX_CASCADE_MAX_CYCLES: usize = 12;
pub(crate) const DEFAULT_PROJECT_UNIVERSE_MANIFEST: &str =
    "/Users/jinchen/Projects/universe.intent.lisp";
pub(crate) const DEFAULT_PROJECT_INTENT_PATH_CANDIDATES: [&str; 3] = [
    ".missiond/intent.lisp",
    ".jarvis/intent.lisp",
    "intent.lisp",
];
pub(crate) const DEFAULT_CAPABILITY_REVIEW_SIDECAR: &str =
    ".missiond/v3/runtime/capability-usage-review.json";
pub(crate) const DEFAULT_PROTECTED_TOOL_PATTERNS: [&str; 12] = [
    "mission_execution",
    "mission_intent",
    "mission_forge_",
    "mission_sys_",
    "mission_daemon_update",
    "mission_health",
    "mission_power_control",
    "mission_kb_ops",
    "mission_audit",
    "mission_pty_signal",
    "mission_pty_confirm",
    "mission_incident",
];
pub(crate) const DEFAULT_PROTECTED_FLOW_PATTERNS: [&str; 4] = [
    "engineering",
    "F-execution-log-governance",
    "F-incident-reaction",
    "F-capability-usage-monitoring",
];
pub(crate) const DEFAULT_MEMORY_PENDING_MESSAGE_LIMIT: usize = 60;
pub(crate) const DEFAULT_MEMORY_TOOL_RESULT_PREVIEW_CHARS: usize = 1000;
pub(crate) const DEFAULT_MEMORY_ASSISTANT_PREVIEW_CHARS: usize = 500;
pub(crate) const DEFAULT_CONVERSATION_GET_TAIL: i64 = 50;
pub(crate) const DEFAULT_CONVERSATION_SEARCH_LIMIT: i64 = 10;
pub(crate) const DEFAULT_MESSAGE_SEARCH_LIMIT: i64 = 20;
pub(crate) const DEFAULT_CONTEXT_BEFORE: i64 = 3;
pub(crate) const DEFAULT_CONTEXT_AFTER: i64 = 5;
pub(crate) const DEFAULT_CONVERSATION_EVENTS_LIMIT: i64 = 100;
pub(crate) const DEFAULT_AGENT_TRAJECTORY_LIMIT: i64 = 200;
pub(crate) const DEFAULT_TIMELINE_QUERY_LIMIT: i64 = 50;
pub(crate) const MAX_TIMELINE_QUERY_LIMIT: i64 = 200;
pub(crate) const DEFAULT_TIMELINE_SEARCH_LIMIT: i64 = 20;
pub(crate) const MAX_TIMELINE_SEARCH_LIMIT: i64 = 100;
pub(crate) const DEFAULT_INTENT_ROUTER_MODEL: &str = "claude-opus-4.6";
pub(crate) const DEFAULT_INTENT_ROUTER_TIMEOUT_MS: u64 = 10_000;
pub(crate) const DEFAULT_VISION_CODEX_BINARY: &str = "codex";
pub(crate) const DEFAULT_VISION_CODEX_MODEL: &str = "gpt-5.4";
pub(crate) const DEFAULT_VISION_CODEX_IDLE_TIMEOUT_SECS: u64 = 120;
pub(crate) const DEFAULT_VISION_CODEX_ABSOLUTE_TIMEOUT_SECS: u64 = 300;
pub(crate) const DEFAULT_AUTOPILOT_STALE_CONVERSATION_MINUTES: i64 = 10;
pub(crate) const DEFAULT_AUTOPILOT_SLOT_TASK_REAP_STALE_SECS: i64 = 1800;
pub(crate) const DEFAULT_AUTOPILOT_RECOVER_STALE_RUNNING_MINUTES: i64 = 15;
pub(crate) const DEFAULT_AUTOPILOT_SLOT_FAILURE_THROTTLE_SECS: i64 = 1800;
pub(crate) const DEFAULT_AUTOPILOT_DEPLOY_REVIEW_TIMEOUT_SECS: u64 = 600;
pub(crate) const DEFAULT_AUTOPILOT_DYNAMIC_SLOT_EXPIRING_SOON_SECS: i64 = 900;
pub(crate) const DEFAULT_AUTOPILOT_STALE_BOARD_PROGRESS_MINUTES: i64 = 30;
pub(crate) const DEFAULT_AUTOPILOT_COMPLETED_JOB_GC_MINUTES: i64 = 30;
pub(crate) const DEFAULT_AUTOPILOT_IDLE_PERSISTENT_SLOT_SECS: u64 = 30 * 60;
pub(crate) const DEFAULT_AUTOPILOT_RECENT_INTENTS_WINDOW_SECS: i64 = 1800;
pub(crate) const DEFAULT_AUTOPILOT_USER_STUCK_COOLDOWN_SECS: i64 = 1800;
pub(crate) const DEFAULT_AUTOPILOT_DIRECTION_SHIFT_COOLDOWN_SECS: i64 = 3600;
pub(crate) const DEFAULT_LEARNING_REALTIME_EXTRACTION_TIMEOUT_SECS: u64 = 300;
pub(crate) const DEFAULT_LEARNING_DECISION_TIER3_TIMEOUT_SECS: u64 = 300;
pub(crate) const DEFAULT_LEARNING_HABIT_SCAN_TIMEOUT_SECS: u64 = 600;
pub(crate) const DEFAULT_LEARNING_TIMELINE_ANALYSIS_INTERVAL_SECS: i64 = 12 * 3600;
pub(crate) const DEFAULT_LEARNING_TIMELINE_ANALYSIS_WINDOW_HOURS: i64 = 12;
pub(crate) const DEFAULT_LEARNING_TIMELINE_ERROR_LIMIT: i64 = 20;
pub(crate) const DEFAULT_LEARNING_TIMELINE_LLM_SAMPLE_LIMIT: i64 = 50;
pub(crate) const DEFAULT_LEARNING_TIMELINE_SLOW_EVENT_LIMIT: usize = 20;
pub(crate) const DEFAULT_LEARNING_TIMELINE_SLOW_THRESHOLD_MS: i64 = 60_000;
pub(crate) const DEFAULT_LEARNING_IDLE_EXPLORE_INTERVAL_SECS: i64 = 2 * 3600;
pub(crate) const DEFAULT_LEARNING_HABIT_SCAN_INTERVAL_SECS: i64 = 4 * 3600;
pub(crate) const DEFAULT_LEARNING_HABIT_SCAN_BATCH_SIZE: usize = 5;
pub(crate) const DEFAULT_LEARNING_KB_AUTO_GC_INTERVAL_SECS: i64 = 3600;
pub(crate) const DEFAULT_LEARNING_KB_CONSOLIDATION_INTERVAL_SECS: i64 = 86400;
pub(crate) const DEFAULT_LEARNING_KB_REFLECTION_INTERVAL_SECS: i64 = 7 * 86400;
pub(crate) const DEFAULT_LEARNING_KB_REFLECTION_UTILITY_THRESHOLD: f64 = 0.3;
pub(crate) const DEFAULT_LEARNING_KB_REFLECTION_MIN_ACCESS: i64 = 3;
pub(crate) const DEFAULT_LEARNING_KB_REFLECTION_MAX_ENTRIES: usize = 20;
pub(crate) const DEFAULT_LEARNING_KB_REFLECTION_MAX_TOKENS: u32 = 2000;
pub(crate) const DEFAULT_LEARNING_DECISION_HARVEST_INTERVAL_SECS: i64 = 86400;
pub(crate) const DEFAULT_LEARNING_COOCCURRENCE_REFRESH_INTERVAL_SECS: i64 = 6 * 3600;
pub(crate) const DEFAULT_DAILY_SONNET_PROFILE: &str = "daily-sonnet";
pub(crate) const DEFAULT_QUICK_HAIKU_PROFILE: &str = "quick-haiku";
pub(crate) const DEFAULT_RESEARCH_PROFILE: &str = "research-default";
pub(crate) const DEFAULT_CODEX_MASTER_PROFILE: &str = "codex-master-gpt-5-5-xhigh";
pub(crate) const DEFAULT_GEMINI_ULTRA_PRO_PROFILE: &str = "gemini-ultra-pro-preview";
pub(crate) const DEFAULT_ROUTER_CHAT_MODEL: &str = "gemini-3.1-pro";
pub(crate) const DEFAULT_ROUTER_CHAT_MAX_TOKENS: u32 = 16384;
pub(crate) const DEFAULT_ROUTER_FILE_CHAT_MAX_TOKENS: u32 = 65536;
pub(crate) const DEFAULT_ROUTER_FLOW_GEMINI_MODEL: &str = "gemini-3.1-pro";
pub(crate) const DEFAULT_ROUTER_STATELESS_SONNET_MODEL: &str = "claude-sonnet";
pub(crate) const DEFAULT_ROUTER_QUEUED_SONNET_MODEL: &str = "claude-sonnet";
pub(crate) const DEFAULT_ROUTER_ANTHROPIC_URGENT_MODEL: &str = "claude-opus-4-6";
pub(crate) const DEFAULT_ROUTER_ANTHROPIC_OPS_MODEL: &str = "claude-sonnet-4-6";
pub(crate) const DEFAULT_ROUTER_ANTHROPIC_DOCS_TEST_CHORE_MODEL: &str = "claude-haiku-4-5-20251001";
pub(crate) const DEFAULT_ROUTER_COMPRESS_MODEL: &str = "gemini-3.1-pro";
pub(crate) const DEFAULT_ROUTER_COMPRESS_CHANNEL: &str = "google";
pub(crate) const DEFAULT_ROUTER_COMPRESS_MAX_TOKENS: u32 = 2048;
pub(crate) const DEFAULT_ROUTER_COMPRESS_CHAR_BUDGET_CHARS: usize = 100_000;
pub(crate) const DEFAULT_ROUTER_DIRECT_HTTP_TIMEOUT_SECS: u64 = 60;
pub(crate) const DEFAULT_ROUTER_GEMINI_PTY_QUEUE_TIMEOUT_SECS: u64 = 30;
pub(crate) const DEFAULT_ROUTER_GEMINI_HTTP_QUEUE_TIMEOUT_SECS: u64 = 300;
pub(crate) const DEFAULT_ROUTER_GEMINI_FILE_UPLOAD_TIMEOUT_SECS: u64 = 600;
pub(crate) const DEFAULT_ROUTER_GEMINI_FILE_POLL_TIMEOUT_SECS: u64 = 300;
pub(crate) const DEFAULT_ROUTER_GEMINI_CLI_ABSOLUTE_TIMEOUT_SECS: u64 = 900;
pub(crate) const DEFAULT_ROUTER_GEMINI_CLI_TOOL_EXEC_TIMEOUT_SECS: u64 = 300;
pub(crate) const DEFAULT_ROUTER_QUEUED_SONNET_QUOTA_THROTTLE_SECS: u64 = 30;
pub(crate) const DEFAULT_ROUTER_QUEUED_SONNET_MAX_TOKENS: u32 = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkstationRuntimeConfig {
    slot_default_profiles: HashMap<String, String>,
    slot_templates: HashMap<String, SlotTemplateRuntimeConfig>,
    model_profile_spawn_args: HashMap<String, Option<String>>,
    startup_slots: Vec<StartupSlotRuntimeConfig>,
    workstation_pool: Vec<WorkstationPoolRuntimeConfig>,
    allowed_cwd_prefixes: Vec<PathBuf>,
    pub timeout_policy: TimeoutPolicy,
    pub cc_swarm_timeout_policy: SimpleTimeoutPolicy,
    pub pty_send_timeout_policy: SimpleTimeoutPolicy,
    pub dynamic_slot_spawn_timeout_policy: SimpleTimeoutPolicy,
    pub swarm_capacity_policy: SwarmCapacityPolicy,
    pub slot_ttl_policy: SlotTtlPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SlotTemplateRuntimeConfig {
    pub name: String,
    pub role: String,
    pub description: String,
    pub default_model_profile: Option<String>,
    pub mcp_config: Option<String>,
    pub default_cwd: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StartupSlotRuntimeConfig {
    pub task_type: String,
    pub engine: String,
    pub lifecycle: String,
    pub slot_id: Option<String>,
    pub role: Option<String>,
    pub model_profile: Option<String>,
    pub timeout_secs: u64,
    pub skip_permissions: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SwarmCapacityPolicy {
    pub default_claude_workers: usize,
    pub max_claude_workers: usize,
    pub default_gemini_workers: usize,
    pub max_gemini_workers: usize,
    pub dynamic_slot_limit: i64,
    pub delegate_rate_per_minute: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkstationPoolRuntimeConfig {
    pub id: String,
    pub engine: String,
    pub role: String,
    pub slot_id: String,
    pub task_type: String,
    pub model_profile: Option<String>,
    pub model: Option<String>,
    pub task_classes: Vec<String>,
    pub capabilities: Vec<String>,
    pub max_concurrency: usize,
    pub timeout_secs: u64,
    pub default_use: String,
    pub accepts_boardtask: bool,
    pub write_allowed: bool,
    pub reasoning_effort: Option<String>,
    pub search_enabled: bool,
    pub sandbox: Option<String>,
    pub approval_policy: Option<String>,
    pub tool_policy_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FlowRuntimeConfig {
    pub llm_call_default_max_tokens: u32,
    pub slot_task_default_model: String,
    pub slot_task_default_timeout_secs: u64,
    pub parallel_slot_default_parallelism: usize,
    pub parallel_slot_default_timeout_secs: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ComputePrimitivesRuntimeConfig {
    pub pty_spawn_timeout_policy: SimpleTimeoutPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MinimaxRuntimeConfig {
    pub model: String,
    pub direct_http_timeout_secs: u64,
    pub quota_throttle_secs: u64,
    pub default_max_tokens: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TimeoutPolicy {
    pub default_secs: i64,
    pub min_secs: i64,
    pub max_secs: i64,
    pub watchdog_grace_secs: i64,
    pub missing_session_probe_secs: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SimpleTimeoutPolicy {
    pub default_secs: i64,
    pub min_secs: i64,
    pub max_secs: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlotTtlPolicy {
    pub default_secs: i64,
    pub min_secs: i64,
    pub max_secs: i64,
    pub default_extend_secs: i64,
    pub max_extend_secs: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CascadeRuntimeConfig {
    pub default_manifest_path: PathBuf,
    pub allowed_root: PathBuf,
    pub trigger_enabled: bool,
    pub default_max_cycles: usize,
    pub max_cycles_limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectRegistryRuntimeConfig {
    pub intent_path_candidates: Vec<String>,
    pub default_universe_manifest: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CapabilityGovernanceRuntimeConfig {
    pub review_sidecar_path: PathBuf,
    pub protected_tool_patterns: Vec<String>,
    pub protected_flow_patterns: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MemoryKbRuntimeConfig {
    pub pending_message_limit: usize,
    pub tool_result_preview_chars: usize,
    pub assistant_preview_chars: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConversationIngestionRuntimeConfig {
    pub conversation_get_tail_default: i64,
    pub conversation_search_default_limit: i64,
    pub message_search_default_limit: i64,
    pub context_before_default: i64,
    pub context_after_default: i64,
    pub conversation_events_default_limit: i64,
    pub agent_trajectory_default_limit: i64,
    pub timeline_query_default_limit: i64,
    pub timeline_query_max_limit: i64,
    pub timeline_search_default_limit: i64,
    pub timeline_search_max_limit: i64,
    pub intent_router_model: String,
    pub intent_router_timeout_ms: u64,
    pub vision_codex_binary: String,
    pub vision_codex_model: String,
    pub vision_codex_idle_timeout_secs: u64,
    pub vision_codex_absolute_timeout_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AutopilotRuntimeConfig {
    pub boardtask_timeout_policy: TimeoutPolicy,
    pub stale_conversation_minutes: i64,
    pub slot_task_reap_stale_secs: i64,
    pub recover_stale_running_minutes: i64,
    pub slot_failure_throttle_secs: i64,
    pub deploy_review_timeout_secs: u64,
    pub dynamic_slot_expiring_soon_secs: i64,
    pub stale_board_progress_minutes: i64,
    pub completed_job_gc_minutes: i64,
    pub idle_persistent_slot_secs: u64,
    pub recent_intents_window_secs: i64,
    pub user_stuck_cooldown_secs: i64,
    pub direction_shift_cooldown_secs: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RouterRuntimeConfig {
    pub default_chat_model: String,
    pub chat_default_max_tokens: u32,
    pub file_chat_default_max_tokens: u32,
    pub flow_gemini_model: String,
    pub stateless_sonnet_model: String,
    pub queued_sonnet_model: String,
    pub anthropic_urgent_model: String,
    pub anthropic_ops_model: String,
    pub anthropic_docs_test_chore_model: String,
    pub compress_model: String,
    pub compress_channel: String,
    pub compress_max_tokens: u32,
    pub compress_char_budget_chars: usize,
    pub direct_http_timeout_secs: u64,
    pub gemini_pty_queue_timeout_secs: u64,
    pub gemini_http_queue_timeout_secs: u64,
    pub gemini_file_upload_timeout_secs: u64,
    pub gemini_file_poll_timeout_secs: u64,
    pub gemini_cli_absolute_timeout_secs: u64,
    pub gemini_cli_tool_exec_timeout_secs: u64,
    pub queued_sonnet_quota_throttle_secs: u64,
    pub queued_sonnet_default_max_tokens: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LearningEngineRuntimeConfig {
    pub realtime_extraction_timeout_secs: u64,
    pub decision_tier3_timeout_secs: u64,
    pub habit_scan_timeout_secs: u64,
    pub timeline_analysis_interval_secs: i64,
    pub timeline_analysis_window_hours: i64,
    pub timeline_error_limit: i64,
    pub timeline_llm_sample_limit: i64,
    pub timeline_slow_event_limit: usize,
    pub timeline_slow_threshold_ms: i64,
    pub idle_explore_interval_secs: i64,
    pub habit_scan_interval_secs: i64,
    pub habit_scan_batch_size: usize,
    pub kb_auto_gc_interval_secs: i64,
    pub kb_consolidation_interval_secs: i64,
    pub kb_reflection_interval_secs: i64,
    pub kb_reflection_utility_threshold: f64,
    pub kb_reflection_min_access: i64,
    pub kb_reflection_max_entries: usize,
    pub kb_reflection_max_tokens: u32,
    pub decision_harvest_interval_secs: i64,
    pub cooccurrence_refresh_interval_secs: i64,
}

#[derive(Debug)]
pub(crate) enum BlueprintConfigError {
    Read { path: PathBuf, message: String },
    Parse(String),
}

impl fmt::Display for BlueprintConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, message } => {
                write!(
                    f,
                    "failed to read V3 blueprint {}: {}",
                    path.display(),
                    message
                )
            }
            Self::Parse(message) => write!(
                f,
                "failed to parse V3 blueprint runtime config: {}",
                message
            ),
        }
    }
}

impl std::error::Error for BlueprintConfigError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompiledRuntimeSnapshot {
    pub kind: String,
    pub path: PathBuf,
    pub schema_version: String,
    pub source_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompiledRuntimeLoad {
    pub snapshot: Option<CompiledRuntimeSnapshot>,
    pub diagnostics: Vec<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompiledProjectUniverse {
    pub projects: Vec<CompiledProjectUniverseEntry>,
    pub maturity: Vec<CompiledProjectMaturityEntry>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub(crate) struct CompiledProjectUniverseEntry {
    pub id: Option<String>,
    pub kind: Option<String>,
    pub root: Option<String>,
    pub path: Option<String>,
    pub intent: Option<String>,
    pub backend: Option<String>,
    pub frontend: Option<String>,
    pub status: Option<String>,
    pub surface: Option<String>,
    #[serde(default)]
    pub checks: Vec<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub(crate) struct CompiledProjectMaturityEntry {
    pub id: Option<String>,
    pub current: Option<String>,
    pub target: Option<String>,
    #[serde(default)]
    pub gap: Vec<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompiledWorkflowContracts {
    pub workflows: Vec<CompiledWorkflowEntry>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub(crate) struct CompiledWorkflowEntry {
    pub file: String,
    pub name: Option<String>,
    pub workflow_id: Option<String>,
    pub status: Option<String>,
    pub owner: Option<String>,
    pub authority: Option<String>,
    #[serde(default)]
    pub source_plans: Vec<String>,
    #[serde(default)]
    pub steps: Vec<String>,
    pub risk_gate_count: usize,
    pub completion_criteria_count: usize,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompiledPayloadLoad<T> {
    pub payload: Option<T>,
    pub snapshot: Option<CompiledRuntimeSnapshot>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CompiledRuntimeEnvelope {
    schema_version: String,
    source_hash: String,
    #[allow(dead_code)]
    generated_at: Option<serde_json::Value>,
    diagnostics: Vec<serde_json::Value>,
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct CompiledV3Payload {
    #[serde(default)]
    forms: Vec<CompiledSexpNode>,
}

#[derive(Debug, Deserialize)]
struct CompiledSexpNode {
    #[serde(rename = "type")]
    node_type: String,
    #[serde(default)]
    value: Option<String>,
    #[serde(rename = "kind", default)]
    list_kind: Option<String>,
    #[serde(default)]
    children: Vec<CompiledSexpNode>,
}

#[derive(Debug, Deserialize)]
struct CompiledProjectUniversePayload {
    #[serde(default)]
    projects: Vec<CompiledProjectUniverseEntry>,
    #[serde(default)]
    maturity: Vec<CompiledProjectMaturityEntry>,
}

#[derive(Debug, Deserialize)]
struct CompiledWorkflowsPayload {
    #[serde(default)]
    workflows: Vec<CompiledWorkflowEntry>,
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self {
            default_secs: DEFAULT_TIMEOUT_SECS,
            min_secs: MIN_TIMEOUT_SECS,
            max_secs: MAX_TIMEOUT_SECS,
            watchdog_grace_secs: WATCHDOG_GRACE_SECS,
            missing_session_probe_secs: MISSING_SESSION_PROBE_SECS,
        }
    }
}

impl Default for SimpleTimeoutPolicy {
    fn default() -> Self {
        Self {
            default_secs: DEFAULT_CC_SWARM_TIMEOUT_SECS,
            min_secs: MIN_CC_SWARM_TIMEOUT_SECS,
            max_secs: MAX_CC_SWARM_TIMEOUT_SECS,
        }
    }
}

impl SimpleTimeoutPolicy {
    fn pty_send_default() -> Self {
        Self {
            default_secs: DEFAULT_PTY_SEND_TIMEOUT_SECS,
            min_secs: MIN_PTY_SEND_TIMEOUT_SECS,
            max_secs: MAX_PTY_SEND_TIMEOUT_SECS,
        }
    }

    fn dynamic_slot_spawn_default() -> Self {
        Self {
            default_secs: DEFAULT_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS,
            min_secs: MIN_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS,
            max_secs: MAX_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS,
        }
    }
}

impl Default for SlotTtlPolicy {
    fn default() -> Self {
        Self {
            default_secs: DEFAULT_SLOT_TTL_SECS,
            min_secs: MIN_SLOT_TTL_SECS,
            max_secs: MAX_SLOT_TTL_SECS,
            default_extend_secs: DEFAULT_SLOT_EXTEND_SECS,
            max_extend_secs: MAX_SLOT_EXTEND_SECS,
        }
    }
}

impl Default for SwarmCapacityPolicy {
    fn default() -> Self {
        Self {
            default_claude_workers: 8,
            max_claude_workers: 16,
            default_gemini_workers: 2,
            max_gemini_workers: 6,
            dynamic_slot_limit: 20,
            delegate_rate_per_minute: 24,
        }
    }
}

fn default_slot_templates() -> HashMap<String, SlotTemplateRuntimeConfig> {
    let mut templates = HashMap::new();
    templates.insert(
        "coder".to_string(),
        SlotTemplateRuntimeConfig {
            name: "coder".to_string(),
            role: "coder".to_string(),
            description: "Dynamic coder slot (ephemeral)".to_string(),
            default_model_profile: Some(DEFAULT_MODEL_PROFILE.to_string()),
            mcp_config: Some(DEFAULT_SLOT_MCP_CONFIG.to_string()),
            default_cwd: DEFAULT_SLOT_DEFAULT_CWD.to_string(),
        },
    );
    templates.insert(
        "researcher".to_string(),
        SlotTemplateRuntimeConfig {
            name: "researcher".to_string(),
            role: "coder".to_string(),
            description: "Dynamic researcher slot (read-only analysis)".to_string(),
            default_model_profile: Some(DEFAULT_RESEARCH_PROFILE.to_string()),
            mcp_config: Some(DEFAULT_SLOT_MCP_CONFIG.to_string()),
            default_cwd: DEFAULT_SLOT_DEFAULT_CWD.to_string(),
        },
    );
    templates.insert(
        "ops".to_string(),
        SlotTemplateRuntimeConfig {
            name: "ops".to_string(),
            role: "operator".to_string(),
            description: "Dynamic ops slot (ephemeral)".to_string(),
            default_model_profile: Some(DEFAULT_DAILY_SONNET_PROFILE.to_string()),
            mcp_config: Some(DEFAULT_SLOT_MCP_CONFIG.to_string()),
            default_cwd: DEFAULT_SLOT_DEFAULT_CWD.to_string(),
        },
    );
    templates
}

impl Default for WorkstationRuntimeConfig {
    fn default() -> Self {
        let slot_templates = default_slot_templates();
        let slot_default_profiles = slot_templates
            .iter()
            .filter_map(|(name, template)| {
                template
                    .default_model_profile
                    .as_ref()
                    .map(|profile| (name.clone(), profile.clone()))
            })
            .collect();
        let mut model_profile_spawn_args = HashMap::new();
        model_profile_spawn_args.insert(DEFAULT_MODEL_PROFILE.to_string(), None);
        model_profile_spawn_args.insert(DEFAULT_RESEARCH_PROFILE.to_string(), None);
        model_profile_spawn_args.insert(
            DEFAULT_DAILY_SONNET_PROFILE.to_string(),
            Some("sonnet".to_string()),
        );
        model_profile_spawn_args.insert(
            DEFAULT_QUICK_HAIKU_PROFILE.to_string(),
            Some("haiku".to_string()),
        );
        model_profile_spawn_args.insert(
            DEFAULT_CODEX_MASTER_PROFILE.to_string(),
            Some("gpt-5.5".to_string()),
        );
        model_profile_spawn_args.insert(
            DEFAULT_GEMINI_ULTRA_PRO_PROFILE.to_string(),
            Some("gemini-3.1-pro-preview".to_string()),
        );
        let startup_slots = vec![
            StartupSlotRuntimeConfig {
                task_type: "arch_maintenance".to_string(),
                engine: "claude-code".to_string(),
                lifecycle: "persistent".to_string(),
                slot_id: Some("slot-arch-maint".to_string()),
                role: Some("arch-maint".to_string()),
                model_profile: Some(DEFAULT_MODEL_PROFILE.to_string()),
                timeout_secs: 600,
                skip_permissions: true,
            },
            StartupSlotRuntimeConfig {
                task_type: "strategy_analyst".to_string(),
                engine: "gemini".to_string(),
                lifecycle: "persistent".to_string(),
                slot_id: Some("slot-gemini-strategy".to_string()),
                role: Some("strategy".to_string()),
                model_profile: None,
                timeout_secs: 600,
                skip_permissions: true,
            },
            StartupSlotRuntimeConfig {
                task_type: "gemini_router".to_string(),
                engine: "gemini".to_string(),
                lifecycle: "persistent".to_string(),
                slot_id: Some("slot-gemini-router".to_string()),
                role: Some("gemini-router".to_string()),
                model_profile: None,
                timeout_secs: 120,
                skip_permissions: true,
            },
            StartupSlotRuntimeConfig {
                task_type: "lisp_survey".to_string(),
                engine: "claude-code".to_string(),
                lifecycle: "persistent".to_string(),
                slot_id: Some("lisp-surveyor".to_string()),
                role: Some("coder".to_string()),
                model_profile: Some(DEFAULT_MODEL_PROFILE.to_string()),
                timeout_secs: 900,
                skip_permissions: true,
            },
        ];
        let workstation_pool = vec![
            WorkstationPoolRuntimeConfig {
                id: "claude-code-default".to_string(),
                engine: "claude-code".to_string(),
                role: "coder".to_string(),
                slot_id: "slot-claude-code-default".to_string(),
                task_type: "claude_code_default".to_string(),
                model_profile: Some(DEFAULT_MODEL_PROFILE.to_string()),
                model: None,
                task_classes: vec![
                    "code".to_string(),
                    "implementation".to_string(),
                    "review".to_string(),
                    "context-pack".to_string(),
                    "ops".to_string(),
                ],
                capabilities: vec![
                    "code-read".to_string(),
                    "code-write".to_string(),
                    "scoped-commit".to_string(),
                    "mcp".to_string(),
                ],
                max_concurrency: 1,
                timeout_secs: 1800,
                default_use: "code-implementation".to_string(),
                accepts_boardtask: true,
                write_allowed: true,
                reasoning_effort: None,
                search_enabled: false,
                sandbox: None,
                approval_policy: None,
                tool_policy_path: None,
            },
            WorkstationPoolRuntimeConfig {
                id: "gemini-ultra-pro".to_string(),
                engine: "gemini".to_string(),
                role: "researcher".to_string(),
                slot_id: "slot-gemini-ultra".to_string(),
                task_type: "gemini_ultra".to_string(),
                model_profile: Some(DEFAULT_GEMINI_ULTRA_PRO_PROFILE.to_string()),
                model: None,
                task_classes: vec![
                    "research".to_string(),
                    "review".to_string(),
                    "context-pack".to_string(),
                    "lisp-compression".to_string(),
                    "general".to_string(),
                ],
                capabilities: vec![
                    "read-only".to_string(),
                    "analysis".to_string(),
                    "design-review".to_string(),
                ],
                max_concurrency: 1,
                timeout_secs: 900,
                default_use: "research-review".to_string(),
                accepts_boardtask: true,
                write_allowed: false,
                reasoning_effort: None,
                search_enabled: false,
                sandbox: None,
                approval_policy: Some("plan".to_string()),
                tool_policy_path: Some(
                    ".missiond/v3/policies/gemini-readonly-policy.toml".to_string(),
                ),
            },
            WorkstationPoolRuntimeConfig {
                id: "claude-code-fast-patch".to_string(),
                engine: "claude-code".to_string(),
                role: "patcher".to_string(),
                slot_id: "slot-claude-code-fast-patch".to_string(),
                task_type: "claude_code_fast_patch".to_string(),
                model_profile: Some(DEFAULT_DAILY_SONNET_PROFILE.to_string()),
                model: None,
                task_classes: vec![
                    "patch".to_string(),
                    "test".to_string(),
                    "chore".to_string(),
                    "low-risk-fast-path".to_string(),
                ],
                capabilities: vec![
                    "code-read".to_string(),
                    "code-write".to_string(),
                    "scoped-commit".to_string(),
                    "narrow-patch".to_string(),
                    "mcp".to_string(),
                ],
                max_concurrency: 1,
                timeout_secs: 900,
                default_use: "narrow-patch".to_string(),
                accepts_boardtask: true,
                write_allowed: true,
                reasoning_effort: None,
                search_enabled: false,
                sandbox: None,
                approval_policy: None,
                tool_policy_path: None,
            },
            WorkstationPoolRuntimeConfig {
                id: "gemini-fast-survey".to_string(),
                engine: "gemini".to_string(),
                role: "survey".to_string(),
                slot_id: "slot-gemini-fast-survey".to_string(),
                task_type: "gemini_fast_survey".to_string(),
                model_profile: None,
                model: Some("gemini-2.5-flash".to_string()),
                task_classes: vec![
                    "survey".to_string(),
                    "summary".to_string(),
                    "mechanical-scan".to_string(),
                ],
                capabilities: vec!["read-only".to_string(), "summary".to_string()],
                max_concurrency: 1,
                timeout_secs: 600,
                default_use: "low-authority-survey".to_string(),
                accepts_boardtask: true,
                write_allowed: false,
                reasoning_effort: None,
                search_enabled: false,
                sandbox: None,
                approval_policy: Some("plan".to_string()),
                tool_policy_path: Some(
                    ".missiond/v3/policies/gemini-readonly-policy.toml".to_string(),
                ),
            },
            WorkstationPoolRuntimeConfig {
                id: "codex-master-control".to_string(),
                engine: "codex".to_string(),
                role: "orchestrator".to_string(),
                slot_id: "slot-codex-master-control".to_string(),
                task_type: "codex_master_control".to_string(),
                model_profile: Some(DEFAULT_CODEX_MASTER_PROFILE.to_string()),
                model: None,
                task_classes: vec![
                    "master-control".to_string(),
                    "orchestration".to_string(),
                    "governance".to_string(),
                    "night-audit".to_string(),
                ],
                capabilities: vec![
                    "board-write".to_string(),
                    "kb-write".to_string(),
                    "execution-log".to_string(),
                    "dispatch".to_string(),
                    "code-read".to_string(),
                    "code-write".to_string(),
                    "shell-exec".to_string(),
                    "search".to_string(),
                    "mcp".to_string(),
                    "full-access".to_string(),
                ],
                max_concurrency: 1,
                timeout_secs: 7200,
                default_use: "resident-master-control".to_string(),
                accepts_boardtask: false,
                write_allowed: true,
                reasoning_effort: Some("xhigh".to_string()),
                search_enabled: true,
                sandbox: Some("danger-full-access".to_string()),
                approval_policy: Some("never".to_string()),
                tool_policy_path: None,
            },
        ];
        Self {
            slot_default_profiles,
            slot_templates,
            model_profile_spawn_args,
            startup_slots,
            workstation_pool,
            allowed_cwd_prefixes: DEFAULT_ALLOWED_CWD_PREFIXES
                .iter()
                .map(PathBuf::from)
                .collect(),
            timeout_policy: TimeoutPolicy::default(),
            cc_swarm_timeout_policy: SimpleTimeoutPolicy::default(),
            pty_send_timeout_policy: SimpleTimeoutPolicy::pty_send_default(),
            dynamic_slot_spawn_timeout_policy: SimpleTimeoutPolicy::dynamic_slot_spawn_default(),
            swarm_capacity_policy: SwarmCapacityPolicy::default(),
            slot_ttl_policy: SlotTtlPolicy::default(),
        }
    }
}

impl Default for FlowRuntimeConfig {
    fn default() -> Self {
        Self {
            llm_call_default_max_tokens: DEFAULT_FLOW_LLM_MAX_TOKENS,
            slot_task_default_model: DEFAULT_FLOW_SLOT_MODEL.to_string(),
            slot_task_default_timeout_secs: DEFAULT_FLOW_SLOT_TIMEOUT_SECS,
            parallel_slot_default_parallelism: DEFAULT_FLOW_PARALLELISM,
            parallel_slot_default_timeout_secs: DEFAULT_FLOW_PARALLEL_TIMEOUT_SECS,
        }
    }
}

impl Default for ComputePrimitivesRuntimeConfig {
    fn default() -> Self {
        Self {
            pty_spawn_timeout_policy: SimpleTimeoutPolicy {
                default_secs: DEFAULT_COMPUTE_PTY_SPAWN_TIMEOUT_SECS,
                min_secs: MIN_COMPUTE_PTY_SPAWN_TIMEOUT_SECS,
                max_secs: MAX_COMPUTE_PTY_SPAWN_TIMEOUT_SECS,
            },
        }
    }
}

impl Default for MinimaxRuntimeConfig {
    fn default() -> Self {
        Self {
            model: DEFAULT_MINIMAX_MODEL.to_string(),
            direct_http_timeout_secs: DEFAULT_MINIMAX_DIRECT_HTTP_TIMEOUT_SECS,
            quota_throttle_secs: DEFAULT_MINIMAX_QUOTA_THROTTLE_SECS,
            default_max_tokens: DEFAULT_MINIMAX_MAX_TOKENS,
        }
    }
}

impl Default for RouterRuntimeConfig {
    fn default() -> Self {
        Self {
            default_chat_model: DEFAULT_ROUTER_CHAT_MODEL.to_string(),
            chat_default_max_tokens: DEFAULT_ROUTER_CHAT_MAX_TOKENS,
            file_chat_default_max_tokens: DEFAULT_ROUTER_FILE_CHAT_MAX_TOKENS,
            flow_gemini_model: DEFAULT_ROUTER_FLOW_GEMINI_MODEL.to_string(),
            stateless_sonnet_model: DEFAULT_ROUTER_STATELESS_SONNET_MODEL.to_string(),
            queued_sonnet_model: DEFAULT_ROUTER_QUEUED_SONNET_MODEL.to_string(),
            anthropic_urgent_model: DEFAULT_ROUTER_ANTHROPIC_URGENT_MODEL.to_string(),
            anthropic_ops_model: DEFAULT_ROUTER_ANTHROPIC_OPS_MODEL.to_string(),
            anthropic_docs_test_chore_model: DEFAULT_ROUTER_ANTHROPIC_DOCS_TEST_CHORE_MODEL
                .to_string(),
            compress_model: DEFAULT_ROUTER_COMPRESS_MODEL.to_string(),
            compress_channel: DEFAULT_ROUTER_COMPRESS_CHANNEL.to_string(),
            compress_max_tokens: DEFAULT_ROUTER_COMPRESS_MAX_TOKENS,
            compress_char_budget_chars: DEFAULT_ROUTER_COMPRESS_CHAR_BUDGET_CHARS,
            direct_http_timeout_secs: DEFAULT_ROUTER_DIRECT_HTTP_TIMEOUT_SECS,
            gemini_pty_queue_timeout_secs: DEFAULT_ROUTER_GEMINI_PTY_QUEUE_TIMEOUT_SECS,
            gemini_http_queue_timeout_secs: DEFAULT_ROUTER_GEMINI_HTTP_QUEUE_TIMEOUT_SECS,
            gemini_file_upload_timeout_secs: DEFAULT_ROUTER_GEMINI_FILE_UPLOAD_TIMEOUT_SECS,
            gemini_file_poll_timeout_secs: DEFAULT_ROUTER_GEMINI_FILE_POLL_TIMEOUT_SECS,
            gemini_cli_absolute_timeout_secs: DEFAULT_ROUTER_GEMINI_CLI_ABSOLUTE_TIMEOUT_SECS,
            gemini_cli_tool_exec_timeout_secs: DEFAULT_ROUTER_GEMINI_CLI_TOOL_EXEC_TIMEOUT_SECS,
            queued_sonnet_quota_throttle_secs: DEFAULT_ROUTER_QUEUED_SONNET_QUOTA_THROTTLE_SECS,
            queued_sonnet_default_max_tokens: DEFAULT_ROUTER_QUEUED_SONNET_MAX_TOKENS,
        }
    }
}

impl Default for CascadeRuntimeConfig {
    fn default() -> Self {
        Self {
            default_manifest_path: PathBuf::from(DEFAULT_CASCADE_MANIFEST_PATH),
            allowed_root: PathBuf::from(DEFAULT_CASCADE_ALLOWED_ROOT),
            trigger_enabled: DEFAULT_CASCADE_TRIGGER_ENABLED,
            default_max_cycles: DEFAULT_CASCADE_MAX_CYCLES,
            max_cycles_limit: MAX_CASCADE_MAX_CYCLES,
        }
    }
}

impl Default for ProjectRegistryRuntimeConfig {
    fn default() -> Self {
        Self {
            intent_path_candidates: DEFAULT_PROJECT_INTENT_PATH_CANDIDATES
                .iter()
                .map(|value| value.to_string())
                .collect(),
            default_universe_manifest: PathBuf::from(DEFAULT_PROJECT_UNIVERSE_MANIFEST),
        }
    }
}

impl Default for CapabilityGovernanceRuntimeConfig {
    fn default() -> Self {
        Self {
            review_sidecar_path: PathBuf::from(DEFAULT_CAPABILITY_REVIEW_SIDECAR),
            protected_tool_patterns: DEFAULT_PROTECTED_TOOL_PATTERNS
                .iter()
                .map(|value| value.to_string())
                .collect(),
            protected_flow_patterns: DEFAULT_PROTECTED_FLOW_PATTERNS
                .iter()
                .map(|value| value.to_string())
                .collect(),
        }
    }
}

impl Default for MemoryKbRuntimeConfig {
    fn default() -> Self {
        Self {
            pending_message_limit: DEFAULT_MEMORY_PENDING_MESSAGE_LIMIT,
            tool_result_preview_chars: DEFAULT_MEMORY_TOOL_RESULT_PREVIEW_CHARS,
            assistant_preview_chars: DEFAULT_MEMORY_ASSISTANT_PREVIEW_CHARS,
        }
    }
}

impl Default for ConversationIngestionRuntimeConfig {
    fn default() -> Self {
        Self {
            conversation_get_tail_default: DEFAULT_CONVERSATION_GET_TAIL,
            conversation_search_default_limit: DEFAULT_CONVERSATION_SEARCH_LIMIT,
            message_search_default_limit: DEFAULT_MESSAGE_SEARCH_LIMIT,
            context_before_default: DEFAULT_CONTEXT_BEFORE,
            context_after_default: DEFAULT_CONTEXT_AFTER,
            conversation_events_default_limit: DEFAULT_CONVERSATION_EVENTS_LIMIT,
            agent_trajectory_default_limit: DEFAULT_AGENT_TRAJECTORY_LIMIT,
            timeline_query_default_limit: DEFAULT_TIMELINE_QUERY_LIMIT,
            timeline_query_max_limit: MAX_TIMELINE_QUERY_LIMIT,
            timeline_search_default_limit: DEFAULT_TIMELINE_SEARCH_LIMIT,
            timeline_search_max_limit: MAX_TIMELINE_SEARCH_LIMIT,
            intent_router_model: DEFAULT_INTENT_ROUTER_MODEL.to_string(),
            intent_router_timeout_ms: DEFAULT_INTENT_ROUTER_TIMEOUT_MS,
            vision_codex_binary: DEFAULT_VISION_CODEX_BINARY.to_string(),
            vision_codex_model: DEFAULT_VISION_CODEX_MODEL.to_string(),
            vision_codex_idle_timeout_secs: DEFAULT_VISION_CODEX_IDLE_TIMEOUT_SECS,
            vision_codex_absolute_timeout_secs: DEFAULT_VISION_CODEX_ABSOLUTE_TIMEOUT_SECS,
        }
    }
}

impl Default for AutopilotRuntimeConfig {
    fn default() -> Self {
        Self {
            boardtask_timeout_policy: TimeoutPolicy::default(),
            stale_conversation_minutes: DEFAULT_AUTOPILOT_STALE_CONVERSATION_MINUTES,
            slot_task_reap_stale_secs: DEFAULT_AUTOPILOT_SLOT_TASK_REAP_STALE_SECS,
            recover_stale_running_minutes: DEFAULT_AUTOPILOT_RECOVER_STALE_RUNNING_MINUTES,
            slot_failure_throttle_secs: DEFAULT_AUTOPILOT_SLOT_FAILURE_THROTTLE_SECS,
            deploy_review_timeout_secs: DEFAULT_AUTOPILOT_DEPLOY_REVIEW_TIMEOUT_SECS,
            dynamic_slot_expiring_soon_secs: DEFAULT_AUTOPILOT_DYNAMIC_SLOT_EXPIRING_SOON_SECS,
            stale_board_progress_minutes: DEFAULT_AUTOPILOT_STALE_BOARD_PROGRESS_MINUTES,
            completed_job_gc_minutes: DEFAULT_AUTOPILOT_COMPLETED_JOB_GC_MINUTES,
            idle_persistent_slot_secs: DEFAULT_AUTOPILOT_IDLE_PERSISTENT_SLOT_SECS,
            recent_intents_window_secs: DEFAULT_AUTOPILOT_RECENT_INTENTS_WINDOW_SECS,
            user_stuck_cooldown_secs: DEFAULT_AUTOPILOT_USER_STUCK_COOLDOWN_SECS,
            direction_shift_cooldown_secs: DEFAULT_AUTOPILOT_DIRECTION_SHIFT_COOLDOWN_SECS,
        }
    }
}

impl Default for LearningEngineRuntimeConfig {
    fn default() -> Self {
        Self {
            realtime_extraction_timeout_secs: DEFAULT_LEARNING_REALTIME_EXTRACTION_TIMEOUT_SECS,
            decision_tier3_timeout_secs: DEFAULT_LEARNING_DECISION_TIER3_TIMEOUT_SECS,
            habit_scan_timeout_secs: DEFAULT_LEARNING_HABIT_SCAN_TIMEOUT_SECS,
            timeline_analysis_interval_secs: DEFAULT_LEARNING_TIMELINE_ANALYSIS_INTERVAL_SECS,
            timeline_analysis_window_hours: DEFAULT_LEARNING_TIMELINE_ANALYSIS_WINDOW_HOURS,
            timeline_error_limit: DEFAULT_LEARNING_TIMELINE_ERROR_LIMIT,
            timeline_llm_sample_limit: DEFAULT_LEARNING_TIMELINE_LLM_SAMPLE_LIMIT,
            timeline_slow_event_limit: DEFAULT_LEARNING_TIMELINE_SLOW_EVENT_LIMIT,
            timeline_slow_threshold_ms: DEFAULT_LEARNING_TIMELINE_SLOW_THRESHOLD_MS,
            idle_explore_interval_secs: DEFAULT_LEARNING_IDLE_EXPLORE_INTERVAL_SECS,
            habit_scan_interval_secs: DEFAULT_LEARNING_HABIT_SCAN_INTERVAL_SECS,
            habit_scan_batch_size: DEFAULT_LEARNING_HABIT_SCAN_BATCH_SIZE,
            kb_auto_gc_interval_secs: DEFAULT_LEARNING_KB_AUTO_GC_INTERVAL_SECS,
            kb_consolidation_interval_secs: DEFAULT_LEARNING_KB_CONSOLIDATION_INTERVAL_SECS,
            kb_reflection_interval_secs: DEFAULT_LEARNING_KB_REFLECTION_INTERVAL_SECS,
            kb_reflection_utility_threshold: DEFAULT_LEARNING_KB_REFLECTION_UTILITY_THRESHOLD,
            kb_reflection_min_access: DEFAULT_LEARNING_KB_REFLECTION_MIN_ACCESS,
            kb_reflection_max_entries: DEFAULT_LEARNING_KB_REFLECTION_MAX_ENTRIES,
            kb_reflection_max_tokens: DEFAULT_LEARNING_KB_REFLECTION_MAX_TOKENS,
            decision_harvest_interval_secs: DEFAULT_LEARNING_DECISION_HARVEST_INTERVAL_SECS,
            cooccurrence_refresh_interval_secs: DEFAULT_LEARNING_COOCCURRENCE_REFRESH_INTERVAL_SECS,
        }
    }
}

impl WorkstationRuntimeConfig {
    pub(crate) fn load_for_current_dir() -> Result<Self, BlueprintConfigError> {
        let cwd = std::env::current_dir().map_err(|err| BlueprintConfigError::Read {
            path: PathBuf::from("."),
            message: err.to_string(),
        })?;
        let root = nearest_missiond_root(&cwd);
        Self::load_for_project_root(Some(root.to_string_lossy().as_ref()))
    }

    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        match load_runtime_blueprint_source(project_root)? {
            Some(source) => parse_workstation_config(&source),
            None => Ok(Self::default()),
        }
    }

    pub(crate) fn default_model_profile_for_template(&self, template: &str) -> Option<&str> {
        self.slot_default_profiles.get(template).map(String::as_str)
    }

    pub(crate) fn slot_template(&self, template: &str) -> Option<&SlotTemplateRuntimeConfig> {
        self.slot_templates.get(template)
    }

    pub(crate) fn allowed_cwd_prefixes(&self) -> &[PathBuf] {
        &self.allowed_cwd_prefixes
    }

    pub(crate) fn available_slot_template_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.slot_templates.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    pub(crate) fn startup_slots(&self) -> &[StartupSlotRuntimeConfig] {
        &self.startup_slots
    }

    pub(crate) fn workstation_pool(&self) -> &[WorkstationPoolRuntimeConfig] {
        &self.workstation_pool
    }

    pub(crate) fn boardtask_pool_candidates(
        &self,
        task_class: &str,
    ) -> Vec<&WorkstationPoolRuntimeConfig> {
        let task_class = task_class.trim();
        let mut candidates: Vec<&WorkstationPoolRuntimeConfig> = self
            .workstation_pool
            .iter()
            .filter(|worker| {
                worker.accepts_boardtask
                    && worker.task_classes.iter().any(|class| {
                        class == task_class || (task_class == "general" && class == "general")
                    })
            })
            .collect();
        if candidates.is_empty() && task_class != "code" {
            candidates = self
                .workstation_pool
                .iter()
                .filter(|worker| {
                    worker.accepts_boardtask
                        && worker.task_classes.iter().any(|class| class == "code")
                })
                .collect();
        }
        candidates.sort_by_key(|worker| if worker.write_allowed { 1 } else { 0 });
        candidates
    }

    #[allow(dead_code)]
    pub(crate) fn default_spawn_model_for_template(
        &self,
        template: &str,
    ) -> Result<Option<String>, BlueprintConfigError> {
        let profile = self
            .default_model_profile_for_template(template)
            .unwrap_or(DEFAULT_MODEL_PROFILE);
        self.spawn_model_for_profile(profile)
    }

    pub(crate) fn spawn_model_for_profile(
        &self,
        profile: &str,
    ) -> Result<Option<String>, BlueprintConfigError> {
        let normalized = normalize_model_profile_name(profile);
        let profile = match normalized.as_str() {
            "default" | "claude-code-default" | "coding-default" | "opus-4-7-default" => {
                DEFAULT_MODEL_PROFILE
            }
            "research" | "research-default" | "gemini-default" | "gemini-researcher" => {
                DEFAULT_RESEARCH_PROFILE
            }
            "sonnet" => DEFAULT_DAILY_SONNET_PROFILE,
            "haiku" => DEFAULT_QUICK_HAIKU_PROFILE,
            other => other,
        };
        self.model_profile_spawn_args
            .get(profile)
            .cloned()
            .ok_or_else(|| {
                BlueprintConfigError::Parse(format!(
                    "unknown workstation model-profile {}",
                    profile
                ))
            })
    }

    /// V3 workstation-config :: model-profile research-default binding.
    ///
    /// Returns true when the given profile name (after alias normalization)
    /// pins routing to the workstation-pool gemini researcher worker. This is
    /// the signal mission_task_delegate uses to prefer the gemini researcher
    /// slot over auto-provisioning a Claude coder slot for read-only research.
    pub(crate) fn profile_routes_to_gemini_researcher(profile: &str) -> bool {
        let normalized = normalize_model_profile_name(profile);
        matches!(
            normalized.as_str(),
            "research" | "research-default" | "gemini-default" | "gemini-researcher"
        )
    }

    /// V3 workstation-pool :: gemini researcher candidate.
    ///
    /// Returns the slot_id of the first registered workstation-pool worker
    /// that accepts BoardTasks, runs Gemini, and is read-only. None when the
    /// pool has no such worker (e.g. before V3 startup registers the pool).
    pub(crate) fn gemini_researcher_pool_slot_id(&self) -> Option<&str> {
        self.workstation_pool
            .iter()
            .find(|worker| {
                worker.accepts_boardtask && worker.engine == "gemini" && worker.role == "researcher"
            })
            .map(|worker| worker.slot_id.as_str())
    }

    pub(crate) fn clamp_timeout_secs(&self, timeout_secs: Option<i64>) -> i64 {
        let raw = match timeout_secs {
            Some(value) if value > 0 => value,
            _ => self.timeout_policy.default_secs,
        };
        raw.clamp(self.timeout_policy.min_secs, self.timeout_policy.max_secs)
    }

    pub(crate) fn clamp_cc_swarm_timeout_ms(&self, timeout_ms: Option<u64>) -> u64 {
        let min_ms = (self.cc_swarm_timeout_policy.min_secs.max(1) as u64).saturating_mul(1000);
        let max_ms = (self.cc_swarm_timeout_policy.max_secs.max(1) as u64).saturating_mul(1000);
        let default_ms =
            (self.cc_swarm_timeout_policy.default_secs.max(1) as u64).saturating_mul(1000);
        let raw = timeout_ms.filter(|value| *value > 0).unwrap_or(default_ms);
        raw.clamp(min_ms, max_ms)
    }

    pub(crate) fn clamp_pty_send_timeout_ms(&self, timeout_ms: Option<u64>) -> u64 {
        let min_ms = (self.pty_send_timeout_policy.min_secs.max(1) as u64).saturating_mul(1000);
        let max_ms = (self.pty_send_timeout_policy.max_secs.max(1) as u64).saturating_mul(1000);
        let default_ms =
            (self.pty_send_timeout_policy.default_secs.max(1) as u64).saturating_mul(1000);
        let raw = timeout_ms.filter(|value| *value > 0).unwrap_or(default_ms);
        raw.clamp(min_ms, max_ms)
    }

    pub(crate) fn dynamic_slot_spawn_timeout_secs(&self) -> u64 {
        self.dynamic_slot_spawn_timeout_policy
            .default_secs
            .clamp(
                self.dynamic_slot_spawn_timeout_policy.min_secs,
                self.dynamic_slot_spawn_timeout_policy.max_secs,
            )
            .max(1) as u64
    }

    pub(crate) fn clamp_swarm_claude_workers(&self, value: Option<usize>) -> usize {
        value
            .unwrap_or(self.swarm_capacity_policy.default_claude_workers)
            .clamp(0, self.swarm_capacity_policy.max_claude_workers)
    }

    pub(crate) fn clamp_swarm_gemini_workers(&self, value: Option<usize>) -> usize {
        value
            .unwrap_or(self.swarm_capacity_policy.default_gemini_workers)
            .clamp(0, self.swarm_capacity_policy.max_gemini_workers)
    }

    pub(crate) fn dynamic_slot_limit(&self) -> i64 {
        self.swarm_capacity_policy.dynamic_slot_limit.max(1)
    }

    pub(crate) fn delegate_rate_per_minute(&self) -> usize {
        self.swarm_capacity_policy.delegate_rate_per_minute.max(1)
    }

    pub(crate) fn clamp_slot_ttl_secs(&self, ttl_secs: Option<i64>) -> i64 {
        let raw = match ttl_secs {
            Some(value) if value > 0 => value,
            _ => self.slot_ttl_policy.default_secs,
        };
        raw.clamp(self.slot_ttl_policy.min_secs, self.slot_ttl_policy.max_secs)
    }

    pub(crate) fn default_slot_extend_secs(&self) -> i64 {
        self.slot_ttl_policy
            .default_extend_secs
            .clamp(self.slot_ttl_policy.min_secs, self.max_slot_extend_secs())
    }

    pub(crate) fn max_slot_extend_secs(&self) -> i64 {
        self.slot_ttl_policy
            .max_extend_secs
            .clamp(self.slot_ttl_policy.min_secs, self.slot_ttl_policy.max_secs)
    }
}

impl FlowRuntimeConfig {
    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        match load_runtime_blueprint_source(project_root)? {
            Some(source) => parse_flow_runtime_policy(&source),
            None => Ok(Self::default()),
        }
    }
}

impl ComputePrimitivesRuntimeConfig {
    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        match load_runtime_blueprint_source(project_root)? {
            Some(source) => parse_compute_runtime_policy(&source),
            None => Ok(Self::default()),
        }
    }

    pub(crate) fn pty_spawn_timeout_secs(&self) -> u64 {
        self.pty_spawn_timeout_policy
            .default_secs
            .clamp(
                self.pty_spawn_timeout_policy.min_secs,
                self.pty_spawn_timeout_policy.max_secs,
            )
            .max(1) as u64
    }
}

impl MinimaxRuntimeConfig {
    pub(crate) fn load_for_current_dir() -> Result<Self, BlueprintConfigError> {
        let cwd = std::env::current_dir().map_err(|err| BlueprintConfigError::Read {
            path: PathBuf::from("."),
            message: err.to_string(),
        })?;
        let root = nearest_missiond_root(&cwd);
        Self::load_for_project_root(Some(root.to_string_lossy().as_ref()))
    }

    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        match load_runtime_blueprint_source(project_root)? {
            Some(source) => parse_minimax_runtime_policy(&source),
            None => Ok(Self::default()),
        }
    }

    pub(crate) fn direct_http_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.direct_http_timeout_secs.max(1))
    }

    pub(crate) fn quota_throttle_sleep(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.quota_throttle_secs.max(1))
    }
}

impl RouterRuntimeConfig {
    pub(crate) fn load_for_current_dir() -> Result<Self, BlueprintConfigError> {
        let cwd = std::env::current_dir().map_err(|err| BlueprintConfigError::Read {
            path: PathBuf::from("."),
            message: err.to_string(),
        })?;
        let root = nearest_missiond_root(&cwd);
        Self::load_for_project_root(Some(root.to_string_lossy().as_ref()))
    }

    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        match load_runtime_blueprint_source(project_root)? {
            Some(source) => parse_router_runtime_policy(&source),
            None => Ok(Self::default()),
        }
    }

    pub(crate) fn direct_http_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.direct_http_timeout_secs.max(1))
    }

    pub(crate) fn gemini_pty_queue_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.gemini_pty_queue_timeout_secs.max(1))
    }

    pub(crate) fn gemini_http_queue_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.gemini_http_queue_timeout_secs.max(1))
    }

    pub(crate) fn gemini_file_upload_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.gemini_file_upload_timeout_secs.max(1))
    }

    pub(crate) fn gemini_file_poll_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.gemini_file_poll_timeout_secs.max(1))
    }

    pub(crate) fn gemini_cli_absolute_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.gemini_cli_absolute_timeout_secs.max(1))
    }

    pub(crate) fn gemini_cli_tool_exec_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.gemini_cli_tool_exec_timeout_secs.max(1))
    }

    pub(crate) fn queued_sonnet_quota_throttle(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.queued_sonnet_quota_throttle_secs.max(1))
    }
}

impl CascadeRuntimeConfig {
    pub(crate) fn load_for_current_dir() -> Result<Self, BlueprintConfigError> {
        let cwd = std::env::current_dir().map_err(|err| BlueprintConfigError::Read {
            path: PathBuf::from("."),
            message: err.to_string(),
        })?;
        let root = nearest_missiond_root(&cwd);
        Self::load_for_project_root(Some(root.to_string_lossy().as_ref()))
    }

    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        match load_runtime_blueprint_source(project_root)? {
            Some(source) => parse_cascade_policy(&source),
            None => Ok(Self::default()),
        }
    }

    pub(crate) fn env_or_default_manifest_path(&self) -> PathBuf {
        std::env::var("UNIVERSE_MANIFEST")
            .map(PathBuf::from)
            .unwrap_or_else(|_| self.default_manifest_path.clone())
    }

    pub(crate) fn env_or_allowed_root(&self) -> PathBuf {
        std::env::var("UNIVERSE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| self.allowed_root.clone())
    }

    pub(crate) fn env_or_trigger_enabled(&self) -> bool {
        std::env::var("CASCADE_TRIGGER_ENABLED")
            .ok()
            .and_then(|value| parse_bool_token(&value))
            .unwrap_or(self.trigger_enabled)
    }

    pub(crate) fn clamp_max_cycles(&self, max_cycles: Option<usize>) -> usize {
        let raw = max_cycles
            .filter(|value| *value > 0)
            .unwrap_or(self.default_max_cycles);
        raw.clamp(1, self.max_cycles_limit.max(1))
    }
}

impl ProjectRegistryRuntimeConfig {
    pub(crate) fn load_for_current_dir() -> Result<Self, BlueprintConfigError> {
        let cwd = std::env::current_dir().map_err(|err| BlueprintConfigError::Read {
            path: PathBuf::from("."),
            message: err.to_string(),
        })?;
        let root = nearest_missiond_root(&cwd);
        Self::load_for_project_root(Some(root.to_string_lossy().as_ref()))
    }

    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        match load_runtime_blueprint_source(project_root)? {
            Some(source) => parse_project_registry_policy(&source),
            None => Ok(Self::default()),
        }
    }

    pub(crate) fn env_or_default_universe_manifest(&self) -> PathBuf {
        std::env::var("UNIVERSE_MANIFEST")
            .map(PathBuf::from)
            .unwrap_or_else(|_| self.default_universe_manifest.clone())
    }
}

impl CapabilityGovernanceRuntimeConfig {
    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        match load_runtime_blueprint_source(project_root)? {
            Some(source) => parse_capability_governance_policy(&source),
            None => Ok(Self::default()),
        }
    }

    pub(crate) fn is_protected_tool(&self, name: &str) -> bool {
        self.protected_tool_patterns.iter().any(|pattern| {
            if pattern.ends_with('_') {
                name.starts_with(pattern)
            } else {
                name == pattern
            }
        })
    }

    pub(crate) fn is_protected_flow(&self, name: &str) -> bool {
        self.protected_flow_patterns
            .iter()
            .any(|pattern| name == pattern || name.starts_with(pattern))
    }
}

impl MemoryKbRuntimeConfig {
    pub(crate) fn load_for_current_dir() -> Result<Self, BlueprintConfigError> {
        let cwd = std::env::current_dir().map_err(|err| BlueprintConfigError::Read {
            path: PathBuf::from("."),
            message: err.to_string(),
        })?;
        let root = nearest_missiond_root(&cwd);
        Self::load_for_project_root(Some(root.to_string_lossy().as_ref()))
    }

    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        match load_runtime_blueprint_source(project_root)? {
            Some(source) => parse_memory_kb_policy(&source),
            None => Ok(Self::default()),
        }
    }
}

impl ConversationIngestionRuntimeConfig {
    pub(crate) fn load_for_current_dir() -> Result<Self, BlueprintConfigError> {
        let cwd = std::env::current_dir().map_err(|err| BlueprintConfigError::Read {
            path: PathBuf::from("."),
            message: err.to_string(),
        })?;
        let root = nearest_missiond_root(&cwd);
        Self::load_for_project_root(Some(root.to_string_lossy().as_ref()))
    }

    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        match load_runtime_blueprint_source(project_root)? {
            Some(source) => parse_conversation_ingestion_policy(&source),
            None => Ok(Self::default()),
        }
    }

    pub(crate) fn timeline_query_limit(&self, requested: Option<i64>) -> i64 {
        requested
            .unwrap_or(self.timeline_query_default_limit)
            .min(self.timeline_query_max_limit)
    }

    pub(crate) fn timeline_search_limit(&self, requested: Option<i64>) -> i64 {
        requested
            .unwrap_or(self.timeline_search_default_limit)
            .min(self.timeline_search_max_limit)
    }

    pub(crate) fn intent_router_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.intent_router_timeout_ms.max(1))
    }

    pub(crate) fn vision_codex_idle_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.vision_codex_idle_timeout_secs.max(1))
    }

    pub(crate) fn vision_codex_absolute_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.vision_codex_absolute_timeout_secs.max(1))
    }
}

impl AutopilotRuntimeConfig {
    pub(crate) fn load_for_current_dir() -> Result<Self, BlueprintConfigError> {
        let cwd = std::env::current_dir().map_err(|err| BlueprintConfigError::Read {
            path: PathBuf::from("."),
            message: err.to_string(),
        })?;
        let root = nearest_missiond_root(&cwd);
        Self::load_for_project_root(Some(root.to_string_lossy().as_ref()))
    }

    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        match load_runtime_blueprint_source(project_root)? {
            Some(source) => parse_autopilot_policy(&source),
            None => Ok(Self::default()),
        }
    }

    pub(crate) fn deploy_review_timeout_ms(&self) -> u64 {
        self.deploy_review_timeout_secs.saturating_mul(1000)
    }
}

impl LearningEngineRuntimeConfig {
    pub(crate) fn load_for_current_dir() -> Result<Self, BlueprintConfigError> {
        let cwd = std::env::current_dir().map_err(|err| BlueprintConfigError::Read {
            path: PathBuf::from("."),
            message: err.to_string(),
        })?;
        let root = nearest_missiond_root(&cwd);
        Self::load_for_project_root(Some(root.to_string_lossy().as_ref()))
    }

    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        match load_runtime_blueprint_source(project_root)? {
            Some(source) => parse_learning_engine_policy(&source),
            None => Ok(Self::default()),
        }
    }

    pub(crate) fn realtime_extraction_timeout_ms(&self) -> u64 {
        self.realtime_extraction_timeout_secs.saturating_mul(1000)
    }

    pub(crate) fn decision_tier3_timeout_ms(&self) -> u64 {
        self.decision_tier3_timeout_secs.saturating_mul(1000)
    }

    pub(crate) fn habit_scan_timeout_ms(&self) -> u64 {
        self.habit_scan_timeout_secs.saturating_mul(1000)
    }

    pub(crate) fn timeline_window_arg(&self) -> String {
        format!("{}h", self.timeline_analysis_window_hours)
    }
}

pub(crate) fn parse_workstation_config(
    source: &str,
) -> Result<WorkstationRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "workstation-config")
        .ok_or_else(|| BlueprintConfigError::Parse("missing (workstation-config ...)".into()))?;
    let mut config = WorkstationRuntimeConfig::default();
    for form in find_forms(&block, "model-profile") {
        let tokens = tokenize_lisp(&form);
        if tokens.len() < 3 {
            continue;
        }
        let profile = tokens[2].clone();
        if let Some(spawn_model_arg) = keyword_value(&tokens, ":spawn-model-arg") {
            config
                .model_profile_spawn_args
                .insert(profile, parse_spawn_model_arg(&spawn_model_arg)?);
        }
    }
    let slot_template_forms = find_forms(&block, "slot-template");
    if !slot_template_forms.is_empty() {
        config.slot_templates.clear();
        config.slot_default_profiles.clear();
        for form in slot_template_forms {
            let tokens = tokenize_lisp(&form);
            if tokens.len() < 3 {
                continue;
            }
            let template = tokens[2].clone();
            let default_model_profile = optional_non_nil_keyword(&tokens, ":default-model-profile");
            if let Some(profile) = default_model_profile.as_ref() {
                config
                    .slot_default_profiles
                    .insert(template.clone(), profile.clone());
            }
            config.slot_templates.insert(
                template.clone(),
                SlotTemplateRuntimeConfig {
                    name: template,
                    role: non_empty_keyword(&tokens, ":role")?,
                    description: non_empty_keyword(&tokens, ":description")?,
                    default_model_profile,
                    mcp_config: optional_non_nil_keyword(&tokens, ":mcp-config"),
                    default_cwd: non_empty_keyword(&tokens, ":default-cwd")?,
                },
            );
        }
    }
    if config.slot_templates.is_empty() {
        return Err(BlueprintConfigError::Parse(
            "workstation-config must declare at least one slot-template".into(),
        ));
    }
    let cwd_policy_form = find_forms(&block, "cwd-policy")
        .into_iter()
        .find(|form| {
            let tokens = tokenize_lisp(form);
            tokens.get(2).is_some_and(|name| name == "dynamic-slot")
        })
        .ok_or_else(|| {
            BlueprintConfigError::Parse(
                "missing (cwd-policy dynamic-slot ...) in workstation-config".into(),
            )
        })?;
    let cwd_policy_tokens = tokenize_lisp(&cwd_policy_form);
    let allowed_prefixes = string_list_keyword(&cwd_policy_tokens, ":allowed-prefixes")?;
    if allowed_prefixes.is_empty() {
        return Err(BlueprintConfigError::Parse(
            "cwd-policy dynamic-slot :allowed-prefixes must not be empty".into(),
        ));
    }
    config.allowed_cwd_prefixes = allowed_prefixes.into_iter().map(PathBuf::from).collect();
    let startup_slot_forms = find_forms(&block, "startup-slot");
    if !startup_slot_forms.is_empty() {
        config.startup_slots.clear();
        for form in startup_slot_forms {
            let tokens = tokenize_lisp(&form);
            if tokens.len() < 3 {
                continue;
            }
            let skip_permissions = keyword_value(&tokens, ":skip_permissions")
                .and_then(|value| parse_bool_token(&value))
                .unwrap_or(true);
            config.startup_slots.push(StartupSlotRuntimeConfig {
                task_type: tokens[2].clone(),
                engine: non_empty_keyword(&tokens, ":engine")?,
                lifecycle: non_empty_keyword(&tokens, ":lifecycle")?,
                slot_id: optional_non_nil_keyword(&tokens, ":slot_id"),
                role: optional_non_nil_keyword(&tokens, ":role"),
                model_profile: optional_non_nil_keyword(&tokens, ":model_profile"),
                timeout_secs: u64_keyword(&tokens, ":timeout_secs")?,
                skip_permissions,
            });
        }
    }
    if let Some(pool_block) = find_form(source, "workstation-pool") {
        let worker_forms = find_forms(&pool_block, "worker");
        if !worker_forms.is_empty() {
            config.workstation_pool.clear();
            for form in worker_forms {
                let tokens = tokenize_lisp(&form);
                if tokens.len() < 3 {
                    continue;
                }
                let accepts_boardtask = keyword_value(&tokens, ":accepts-boardtask")
                    .or_else(|| keyword_value(&tokens, ":accepts_boardtask"))
                    .and_then(|value| parse_bool_token(&value))
                    .unwrap_or(true);
                let write_allowed = keyword_value(&tokens, ":write-allowed")
                    .or_else(|| keyword_value(&tokens, ":write_allowed"))
                    .and_then(|value| parse_bool_token(&value))
                    .unwrap_or(false);
                let max_concurrency = usize_keyword(&tokens, ":max-concurrency")
                    .or_else(|_| usize_keyword(&tokens, ":max_concurrency"))?;
                let timeout_secs = u64_keyword(&tokens, ":timeout-secs")
                    .or_else(|_| u64_keyword(&tokens, ":timeout_secs"))?;
                let search_enabled = keyword_value(&tokens, ":search")
                    .or_else(|| keyword_value(&tokens, ":search-enabled"))
                    .or_else(|| keyword_value(&tokens, ":search_enabled"))
                    .and_then(|value| parse_bool_token(&value))
                    .unwrap_or(false);
                config.workstation_pool.push(WorkstationPoolRuntimeConfig {
                    id: tokens[2].clone(),
                    engine: non_empty_keyword(&tokens, ":engine")?,
                    role: non_empty_keyword(&tokens, ":role")?,
                    slot_id: non_empty_keyword(&tokens, ":slot-id")
                        .or_else(|_| non_empty_keyword(&tokens, ":slot_id"))?,
                    task_type: non_empty_keyword(&tokens, ":task-type")
                        .or_else(|_| non_empty_keyword(&tokens, ":task_type"))?,
                    model_profile: optional_non_nil_keyword(&tokens, ":model-profile")
                        .or_else(|| optional_non_nil_keyword(&tokens, ":model_profile")),
                    model: optional_non_nil_keyword(&tokens, ":model"),
                    task_classes: string_list_keyword(&tokens, ":task-classes")
                        .or_else(|_| string_list_keyword(&tokens, ":task_classes"))?,
                    capabilities: string_list_keyword(&tokens, ":capabilities")?,
                    max_concurrency,
                    timeout_secs,
                    default_use: non_empty_keyword(&tokens, ":default-use")
                        .or_else(|_| non_empty_keyword(&tokens, ":default_use"))?,
                    accepts_boardtask,
                    write_allowed,
                    reasoning_effort: optional_non_nil_keyword(&tokens, ":reasoning-effort")
                        .or_else(|| optional_non_nil_keyword(&tokens, ":reasoning_effort")),
                    search_enabled,
                    sandbox: optional_non_nil_keyword(&tokens, ":sandbox"),
                    approval_policy: optional_non_nil_keyword(&tokens, ":approval-policy")
                        .or_else(|| optional_non_nil_keyword(&tokens, ":approval_policy")),
                    tool_policy_path: optional_non_nil_keyword(&tokens, ":tool-policy-path")
                        .or_else(|| optional_non_nil_keyword(&tokens, ":tool_policy_path")),
                });
            }
        }
    }
    if config.workstation_pool.is_empty() {
        return Err(BlueprintConfigError::Parse(
            "workstation-pool must declare at least one worker".into(),
        ));
    }
    if !config.workstation_pool.iter().any(|worker| {
        worker.accepts_boardtask
            && worker.engine == "claude-code"
            && worker.model_profile.as_deref() == Some(DEFAULT_MODEL_PROFILE)
    }) {
        return Err(BlueprintConfigError::Parse(
            "workstation-pool must include a Claude Code default BoardTask worker".into(),
        ));
    }
    if !config.workstation_pool.iter().any(|worker| {
        worker.accepts_boardtask && worker.engine == "gemini" && !worker.write_allowed
    }) {
        return Err(BlueprintConfigError::Parse(
            "workstation-pool must include a read-only Gemini BoardTask worker".into(),
        ));
    }
    if config.workstation_pool.iter().any(|worker| {
        worker.accepts_boardtask
            && worker.engine == "gemini"
            && !worker.write_allowed
            && worker.tool_policy_path.as_deref().unwrap_or("").is_empty()
    }) {
        return Err(BlueprintConfigError::Parse(
            "read-only Gemini workstation-pool workers must declare :tool-policy-path".into(),
        ));
    }
    if !config.workstation_pool.iter().any(|worker| {
        worker.id == "codex-master-control"
            && worker.engine == "codex"
            && worker.role == "orchestrator"
            && worker.model_profile.as_deref() == Some(DEFAULT_CODEX_MASTER_PROFILE)
            && worker.reasoning_effort.as_deref() == Some("xhigh")
            && worker.search_enabled
            && !worker.accepts_boardtask
    }) {
        return Err(BlueprintConfigError::Parse(
            "workstation-pool must include a non-shard Codex master-control worker".into(),
        ));
    }
    let timeout_form = find_forms(&block, "timeout-policy")
        .into_iter()
        .find(|form| {
            let tokens = tokenize_lisp(form);
            tokens
                .get(2)
                .is_some_and(|name| name == "boardtask-dispatch")
        })
        .ok_or_else(|| {
            BlueprintConfigError::Parse(
                "missing (timeout-policy boardtask-dispatch ...) in workstation-config".into(),
            )
        })?;
    let timeout_tokens = tokenize_lisp(&timeout_form);
    config.timeout_policy = TimeoutPolicy {
        default_secs: int_keyword(&timeout_tokens, ":default_secs")?,
        min_secs: int_keyword(&timeout_tokens, ":min_secs")?,
        max_secs: int_keyword(&timeout_tokens, ":max_secs")?,
        watchdog_grace_secs: int_keyword(&timeout_tokens, ":watchdog_grace_secs")?,
        missing_session_probe_secs: int_keyword(&timeout_tokens, ":missing_session_probe_secs")?,
    };
    let cc_swarm_timeout_form = find_forms(&block, "timeout-policy")
        .into_iter()
        .find(|form| {
            let tokens = tokenize_lisp(form);
            tokens.get(2).is_some_and(|name| name == "claudecode-swarm")
        })
        .ok_or_else(|| {
            BlueprintConfigError::Parse(
                "missing (timeout-policy claudecode-swarm ...) in workstation-config".into(),
            )
        })?;
    let cc_swarm_timeout_tokens = tokenize_lisp(&cc_swarm_timeout_form);
    config.cc_swarm_timeout_policy = SimpleTimeoutPolicy {
        default_secs: int_keyword(&cc_swarm_timeout_tokens, ":default_secs")?,
        min_secs: int_keyword(&cc_swarm_timeout_tokens, ":min_secs")?,
        max_secs: int_keyword(&cc_swarm_timeout_tokens, ":max_secs")?,
    };
    let pty_send_timeout_form = find_forms(&block, "timeout-policy")
        .into_iter()
        .find(|form| {
            let tokens = tokenize_lisp(form);
            tokens
                .get(2)
                .is_some_and(|name| name == "pty-send-blocking")
        })
        .ok_or_else(|| {
            BlueprintConfigError::Parse(
                "missing (timeout-policy pty-send-blocking ...) in workstation-config".into(),
            )
        })?;
    let pty_send_timeout_tokens = tokenize_lisp(&pty_send_timeout_form);
    config.pty_send_timeout_policy = SimpleTimeoutPolicy {
        default_secs: int_keyword(&pty_send_timeout_tokens, ":default_secs")?,
        min_secs: int_keyword(&pty_send_timeout_tokens, ":min_secs")?,
        max_secs: int_keyword(&pty_send_timeout_tokens, ":max_secs")?,
    };
    let dynamic_slot_spawn_timeout_form = find_forms(&block, "timeout-policy")
        .into_iter()
        .find(|form| {
            let tokens = tokenize_lisp(form);
            tokens
                .get(2)
                .is_some_and(|name| name == "dynamic-slot-spawn")
        })
        .ok_or_else(|| {
            BlueprintConfigError::Parse(
                "missing (timeout-policy dynamic-slot-spawn ...) in workstation-config".into(),
            )
        })?;
    let dynamic_slot_spawn_timeout_tokens = tokenize_lisp(&dynamic_slot_spawn_timeout_form);
    config.dynamic_slot_spawn_timeout_policy = SimpleTimeoutPolicy {
        default_secs: int_keyword(&dynamic_slot_spawn_timeout_tokens, ":default_secs")?,
        min_secs: int_keyword(&dynamic_slot_spawn_timeout_tokens, ":min_secs")?,
        max_secs: int_keyword(&dynamic_slot_spawn_timeout_tokens, ":max_secs")?,
    };
    let capacity_form = find_forms(&block, "capacity-policy")
        .into_iter()
        .find(|form| {
            let tokens = tokenize_lisp(form);
            tokens.get(2).is_some_and(|name| name == "swarm-workers")
        })
        .ok_or_else(|| {
            BlueprintConfigError::Parse(
                "missing (capacity-policy swarm-workers ...) in workstation-config".into(),
            )
        })?;
    let capacity_tokens = tokenize_lisp(&capacity_form);
    config.swarm_capacity_policy = SwarmCapacityPolicy {
        default_claude_workers: usize_keyword(&capacity_tokens, ":default_claude_workers")?,
        max_claude_workers: usize_keyword(&capacity_tokens, ":max_claude_workers")?,
        default_gemini_workers: usize_keyword(&capacity_tokens, ":default_gemini_workers")?,
        max_gemini_workers: usize_keyword(&capacity_tokens, ":max_gemini_workers")?,
        dynamic_slot_limit: int_keyword(&capacity_tokens, ":dynamic_slot_limit")?,
        delegate_rate_per_minute: usize_keyword(&capacity_tokens, ":delegate_rate_per_minute")?,
    };
    let ttl_form = find_forms(&block, "ttl-policy")
        .into_iter()
        .find(|form| {
            let tokens = tokenize_lisp(form);
            tokens.get(2).is_some_and(|name| name == "dynamic-slot")
        })
        .ok_or_else(|| {
            BlueprintConfigError::Parse(
                "missing (ttl-policy dynamic-slot ...) in workstation-config".into(),
            )
        })?;
    let ttl_tokens = tokenize_lisp(&ttl_form);
    config.slot_ttl_policy = SlotTtlPolicy {
        default_secs: int_keyword(&ttl_tokens, ":default_secs")?,
        min_secs: int_keyword(&ttl_tokens, ":min_secs")?,
        max_secs: int_keyword(&ttl_tokens, ":max_secs")?,
        default_extend_secs: int_keyword(&ttl_tokens, ":default_extend_secs")?,
        max_extend_secs: int_keyword(&ttl_tokens, ":max_extend_secs")?,
    };
    if config.timeout_policy.min_secs > config.timeout_policy.max_secs {
        return Err(BlueprintConfigError::Parse(
            "workstation timeout :min_secs must be <= :max_secs".into(),
        ));
    }
    if config.cc_swarm_timeout_policy.min_secs > config.cc_swarm_timeout_policy.max_secs {
        return Err(BlueprintConfigError::Parse(
            "claudecode-swarm timeout :min_secs must be <= :max_secs".into(),
        ));
    }
    if config.cc_swarm_timeout_policy.default_secs < config.cc_swarm_timeout_policy.min_secs
        || config.cc_swarm_timeout_policy.default_secs > config.cc_swarm_timeout_policy.max_secs
    {
        return Err(BlueprintConfigError::Parse(
            "claudecode-swarm timeout :default_secs must be within :min_secs..:max_secs".into(),
        ));
    }
    if config.pty_send_timeout_policy.min_secs > config.pty_send_timeout_policy.max_secs {
        return Err(BlueprintConfigError::Parse(
            "pty-send-blocking timeout :min_secs must be <= :max_secs".into(),
        ));
    }
    if config.pty_send_timeout_policy.default_secs < config.pty_send_timeout_policy.min_secs
        || config.pty_send_timeout_policy.default_secs > config.pty_send_timeout_policy.max_secs
    {
        return Err(BlueprintConfigError::Parse(
            "pty-send-blocking timeout :default_secs must be within :min_secs..:max_secs".into(),
        ));
    }
    if config.swarm_capacity_policy.default_claude_workers
        > config.swarm_capacity_policy.max_claude_workers
    {
        return Err(BlueprintConfigError::Parse(
            "swarm-workers :default_claude_workers must be <= :max_claude_workers".into(),
        ));
    }
    if config.swarm_capacity_policy.default_gemini_workers
        > config.swarm_capacity_policy.max_gemini_workers
    {
        return Err(BlueprintConfigError::Parse(
            "swarm-workers :default_gemini_workers must be <= :max_gemini_workers".into(),
        ));
    }
    if config.swarm_capacity_policy.dynamic_slot_limit <= 0 {
        return Err(BlueprintConfigError::Parse(
            "swarm-workers :dynamic_slot_limit must be positive".into(),
        ));
    }
    if config.swarm_capacity_policy.delegate_rate_per_minute == 0 {
        return Err(BlueprintConfigError::Parse(
            "swarm-workers :delegate_rate_per_minute must be positive".into(),
        ));
    }
    if config.slot_ttl_policy.min_secs > config.slot_ttl_policy.max_secs {
        return Err(BlueprintConfigError::Parse(
            "dynamic-slot ttl :min_secs must be <= :max_secs".into(),
        ));
    }
    if config.slot_ttl_policy.default_extend_secs > config.slot_ttl_policy.max_extend_secs {
        return Err(BlueprintConfigError::Parse(
            "dynamic-slot :default_extend_secs must be <= :max_extend_secs".into(),
        ));
    }
    if config.slot_ttl_policy.max_extend_secs < config.slot_ttl_policy.min_secs {
        return Err(BlueprintConfigError::Parse(
            "dynamic-slot :max_extend_secs must be >= :min_secs".into(),
        ));
    }
    if config.slot_ttl_policy.max_extend_secs > config.slot_ttl_policy.max_secs {
        return Err(BlueprintConfigError::Parse(
            "dynamic-slot :max_extend_secs must be <= :max_secs".into(),
        ));
    }
    Ok(config)
}

pub(crate) fn parse_flow_runtime_policy(
    source: &str,
) -> Result<FlowRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "flow-runtime-policy")
        .ok_or_else(|| BlueprintConfigError::Parse("missing (flow-runtime-policy ...)".into()))?;
    let tokens = tokenize_lisp(&block);
    let slot_task_default_model = keyword_value(&tokens, ":slot-task-default-model")
        .ok_or_else(|| BlueprintConfigError::Parse("missing :slot-task-default-model".into()))?;
    let cfg = FlowRuntimeConfig {
        llm_call_default_max_tokens: u32_keyword(&tokens, ":llm-call-default-max-tokens")?,
        slot_task_default_model,
        slot_task_default_timeout_secs: u64_keyword(&tokens, ":slot-task-default-timeout-secs")?,
        parallel_slot_default_parallelism: usize_keyword(
            &tokens,
            ":parallel-slot-default-parallelism",
        )?,
        parallel_slot_default_timeout_secs: u64_keyword(
            &tokens,
            ":parallel-slot-default-timeout-secs",
        )?,
    };
    if cfg.slot_task_default_model.trim().is_empty() {
        return Err(BlueprintConfigError::Parse(
            ":slot-task-default-model must not be empty".into(),
        ));
    }
    if cfg.parallel_slot_default_parallelism == 0 {
        return Err(BlueprintConfigError::Parse(
            ":parallel-slot-default-parallelism must be positive".into(),
        ));
    }
    Ok(cfg)
}

pub(crate) fn parse_compute_runtime_policy(
    source: &str,
) -> Result<ComputePrimitivesRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "compute-runtime-policy").ok_or_else(|| {
        BlueprintConfigError::Parse("missing (compute-runtime-policy ...)".into())
    })?;
    let timeout_form = find_forms(&block, "timeout-policy")
        .into_iter()
        .find(|form| {
            let tokens = tokenize_lisp(form);
            tokens
                .get(2)
                .is_some_and(|name| name == "tracked-pty-spawn")
        })
        .ok_or_else(|| {
            BlueprintConfigError::Parse(
                "missing (timeout-policy tracked-pty-spawn ...) in compute-runtime-policy".into(),
            )
        })?;
    let timeout_tokens = tokenize_lisp(&timeout_form);
    let cfg = ComputePrimitivesRuntimeConfig {
        pty_spawn_timeout_policy: SimpleTimeoutPolicy {
            default_secs: int_keyword(&timeout_tokens, ":default_secs")?,
            min_secs: int_keyword(&timeout_tokens, ":min_secs")?,
            max_secs: int_keyword(&timeout_tokens, ":max_secs")?,
        },
    };
    if cfg.pty_spawn_timeout_policy.min_secs > cfg.pty_spawn_timeout_policy.max_secs {
        return Err(BlueprintConfigError::Parse(
            "tracked-pty-spawn timeout :min_secs must be <= :max_secs".into(),
        ));
    }
    if cfg.pty_spawn_timeout_policy.default_secs < cfg.pty_spawn_timeout_policy.min_secs
        || cfg.pty_spawn_timeout_policy.default_secs > cfg.pty_spawn_timeout_policy.max_secs
    {
        return Err(BlueprintConfigError::Parse(
            "tracked-pty-spawn timeout :default_secs must be within :min_secs..:max_secs".into(),
        ));
    }
    Ok(cfg)
}

pub(crate) fn parse_minimax_runtime_policy(
    source: &str,
) -> Result<MinimaxRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "minimax-runtime-policy").ok_or_else(|| {
        BlueprintConfigError::Parse("missing (minimax-runtime-policy ...)".into())
    })?;
    let tokens = tokenize_lisp(&block);
    let cfg = MinimaxRuntimeConfig {
        model: non_empty_keyword(&tokens, ":model")?,
        direct_http_timeout_secs: u64_keyword(&tokens, ":direct-http-timeout-secs")?,
        quota_throttle_secs: u64_keyword(&tokens, ":quota-throttle-secs")?,
        default_max_tokens: u32_keyword(&tokens, ":default-max-tokens")?,
    };
    if cfg.direct_http_timeout_secs == 0
        || cfg.quota_throttle_secs == 0
        || cfg.default_max_tokens == 0
    {
        return Err(BlueprintConfigError::Parse(
            "minimax-runtime-policy numeric budgets must be positive".into(),
        ));
    }
    Ok(cfg)
}

pub(crate) fn parse_router_runtime_policy(
    source: &str,
) -> Result<RouterRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "router-runtime-policy")
        .ok_or_else(|| BlueprintConfigError::Parse("missing (router-runtime-policy ...)".into()))?;
    let tokens = tokenize_lisp(&block);
    let cfg = RouterRuntimeConfig {
        default_chat_model: non_empty_keyword(&tokens, ":default-chat-model")?,
        chat_default_max_tokens: u32_keyword(&tokens, ":chat-default-max-tokens")?,
        file_chat_default_max_tokens: u32_keyword(&tokens, ":file-chat-default-max-tokens")?,
        flow_gemini_model: non_empty_keyword(&tokens, ":flow-gemini-model")?,
        stateless_sonnet_model: non_empty_keyword(&tokens, ":stateless-sonnet-model")?,
        queued_sonnet_model: non_empty_keyword(&tokens, ":queued-sonnet-model")?,
        anthropic_urgent_model: non_empty_keyword(&tokens, ":anthropic-urgent-model")?,
        anthropic_ops_model: non_empty_keyword(&tokens, ":anthropic-ops-model")?,
        anthropic_docs_test_chore_model: non_empty_keyword(
            &tokens,
            ":anthropic-docs-test-chore-model",
        )?,
        compress_model: non_empty_keyword(&tokens, ":compress-model")?,
        compress_channel: non_empty_keyword(&tokens, ":compress-channel")?,
        compress_max_tokens: u32_keyword(&tokens, ":compress-max-tokens")?,
        compress_char_budget_chars: usize_keyword(&tokens, ":compress-char-budget-chars")?,
        direct_http_timeout_secs: u64_keyword(&tokens, ":direct-http-timeout-secs")?,
        gemini_pty_queue_timeout_secs: u64_keyword(&tokens, ":gemini-pty-queue-timeout-secs")?,
        gemini_http_queue_timeout_secs: u64_keyword(&tokens, ":gemini-http-queue-timeout-secs")?,
        gemini_file_upload_timeout_secs: u64_keyword(&tokens, ":gemini-file-upload-timeout-secs")?,
        gemini_file_poll_timeout_secs: u64_keyword(&tokens, ":gemini-file-poll-timeout-secs")?,
        gemini_cli_absolute_timeout_secs: u64_keyword(
            &tokens,
            ":gemini-cli-absolute-timeout-secs",
        )?,
        gemini_cli_tool_exec_timeout_secs: u64_keyword(
            &tokens,
            ":gemini-cli-tool-exec-timeout-secs",
        )?,
        queued_sonnet_quota_throttle_secs: u64_keyword(
            &tokens,
            ":queued-sonnet-quota-throttle-secs",
        )?,
        queued_sonnet_default_max_tokens: u32_keyword(
            &tokens,
            ":queued-sonnet-default-max-tokens",
        )?,
    };
    if cfg.chat_default_max_tokens == 0
        || cfg.file_chat_default_max_tokens == 0
        || cfg.compress_max_tokens == 0
        || cfg.compress_char_budget_chars == 0
        || cfg.direct_http_timeout_secs == 0
        || cfg.gemini_pty_queue_timeout_secs == 0
        || cfg.gemini_http_queue_timeout_secs == 0
        || cfg.gemini_file_upload_timeout_secs == 0
        || cfg.gemini_file_poll_timeout_secs == 0
        || cfg.gemini_cli_absolute_timeout_secs == 0
        || cfg.gemini_cli_tool_exec_timeout_secs == 0
        || cfg.queued_sonnet_quota_throttle_secs == 0
        || cfg.queued_sonnet_default_max_tokens == 0
    {
        return Err(BlueprintConfigError::Parse(
            "router-runtime-policy numeric budgets must be positive".into(),
        ));
    }
    Ok(cfg)
}

pub(crate) fn parse_project_registry_policy(
    source: &str,
) -> Result<ProjectRegistryRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "project-registry-policy").ok_or_else(|| {
        BlueprintConfigError::Parse("missing (project-registry-policy ...)".into())
    })?;
    let tokens = tokenize_lisp(&block);
    let intent_path_candidates = string_list_keyword(&tokens, ":intent-path-candidates")?;
    if intent_path_candidates.is_empty() {
        return Err(BlueprintConfigError::Parse(
            ":intent-path-candidates must not be empty".into(),
        ));
    }
    let default_universe_manifest = keyword_value(&tokens, ":default-universe-manifest")
        .ok_or_else(|| BlueprintConfigError::Parse("missing :default-universe-manifest".into()))?;
    Ok(ProjectRegistryRuntimeConfig {
        intent_path_candidates,
        default_universe_manifest: PathBuf::from(default_universe_manifest),
    })
}

pub(crate) fn parse_cascade_policy(
    source: &str,
) -> Result<CascadeRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "cascade-policy")
        .ok_or_else(|| BlueprintConfigError::Parse("missing (cascade-policy ...)".into()))?;
    let tokens = tokenize_lisp(&block);
    let default_manifest_path = keyword_value(&tokens, ":default-manifest")
        .ok_or_else(|| BlueprintConfigError::Parse("missing :default-manifest".into()))?;
    let allowed_root = keyword_value(&tokens, ":allowed-root")
        .ok_or_else(|| BlueprintConfigError::Parse("missing :allowed-root".into()))?;
    let trigger_enabled = keyword_value(&tokens, ":trigger-enabled")
        .and_then(|value| parse_bool_token(&value))
        .ok_or_else(|| {
            BlueprintConfigError::Parse(":trigger-enabled must be true or false".into())
        })?;
    let default_max_cycles = usize_keyword(&tokens, ":default-max-cycles")?;
    let max_cycles_limit = usize_keyword(&tokens, ":max-cycles-limit")?;
    if default_max_cycles == 0 {
        return Err(BlueprintConfigError::Parse(
            ":default-max-cycles must be positive".into(),
        ));
    }
    if max_cycles_limit < default_max_cycles {
        return Err(BlueprintConfigError::Parse(
            ":max-cycles-limit must be >= :default-max-cycles".into(),
        ));
    }
    Ok(CascadeRuntimeConfig {
        default_manifest_path: PathBuf::from(default_manifest_path),
        allowed_root: PathBuf::from(allowed_root),
        trigger_enabled,
        default_max_cycles,
        max_cycles_limit,
    })
}

pub(crate) fn parse_capability_governance_policy(
    source: &str,
) -> Result<CapabilityGovernanceRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "capability-governance-policy").ok_or_else(|| {
        BlueprintConfigError::Parse("missing (capability-governance-policy ...)".into())
    })?;
    let tokens = tokenize_lisp(&block);
    let review_sidecar_path = keyword_value(&tokens, ":review-sidecar")
        .ok_or_else(|| BlueprintConfigError::Parse("missing :review-sidecar".into()))?;
    let protected_tool_patterns = string_list_keyword(&tokens, ":protected-tool-patterns")?;
    if protected_tool_patterns.is_empty() {
        return Err(BlueprintConfigError::Parse(
            ":protected-tool-patterns must not be empty".into(),
        ));
    }
    let protected_flow_patterns = string_list_keyword(&tokens, ":protected-flow-patterns")?;
    if protected_flow_patterns.is_empty() {
        return Err(BlueprintConfigError::Parse(
            ":protected-flow-patterns must not be empty".into(),
        ));
    }
    Ok(CapabilityGovernanceRuntimeConfig {
        review_sidecar_path: PathBuf::from(review_sidecar_path),
        protected_tool_patterns,
        protected_flow_patterns,
    })
}

pub(crate) fn parse_memory_kb_policy(
    source: &str,
) -> Result<MemoryKbRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "memory-kb-policy")
        .ok_or_else(|| BlueprintConfigError::Parse("missing (memory-kb-policy ...)".into()))?;
    let tokens = tokenize_lisp(&block);
    let pending_message_limit = usize_keyword(&tokens, ":pending-message-limit")?;
    let tool_result_preview_chars = usize_keyword(&tokens, ":tool-result-preview-chars")?;
    let assistant_preview_chars = usize_keyword(&tokens, ":assistant-preview-chars")?;
    if pending_message_limit == 0 {
        return Err(BlueprintConfigError::Parse(
            ":pending-message-limit must be positive".into(),
        ));
    }
    if tool_result_preview_chars == 0 || assistant_preview_chars == 0 {
        return Err(BlueprintConfigError::Parse(
            "memory preview char limits must be positive".into(),
        ));
    }
    Ok(MemoryKbRuntimeConfig {
        pending_message_limit,
        tool_result_preview_chars,
        assistant_preview_chars,
    })
}

pub(crate) fn parse_conversation_ingestion_policy(
    source: &str,
) -> Result<ConversationIngestionRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "conversation-ingestion-policy").ok_or_else(|| {
        BlueprintConfigError::Parse("missing (conversation-ingestion-policy ...)".into())
    })?;
    let tokens = tokenize_lisp(&block);
    let cfg = ConversationIngestionRuntimeConfig {
        conversation_get_tail_default: int_keyword(&tokens, ":conversation-get-tail-default")?,
        conversation_search_default_limit: int_keyword(
            &tokens,
            ":conversation-search-default-limit",
        )?,
        message_search_default_limit: int_keyword(&tokens, ":message-search-default-limit")?,
        context_before_default: int_keyword(&tokens, ":context-before-default")?,
        context_after_default: int_keyword(&tokens, ":context-after-default")?,
        conversation_events_default_limit: int_keyword(
            &tokens,
            ":conversation-events-default-limit",
        )?,
        agent_trajectory_default_limit: int_keyword(&tokens, ":agent-trajectory-default-limit")?,
        timeline_query_default_limit: int_keyword(&tokens, ":timeline-query-default-limit")?,
        timeline_query_max_limit: int_keyword(&tokens, ":timeline-query-max-limit")?,
        timeline_search_default_limit: int_keyword(&tokens, ":timeline-search-default-limit")?,
        timeline_search_max_limit: int_keyword(&tokens, ":timeline-search-max-limit")?,
        intent_router_model: non_empty_keyword(&tokens, ":intent-router-model")?,
        intent_router_timeout_ms: u64_keyword(&tokens, ":intent-router-timeout-ms")?,
        vision_codex_binary: non_empty_keyword(&tokens, ":vision-codex-binary")?,
        vision_codex_model: non_empty_keyword(&tokens, ":vision-codex-model")?,
        vision_codex_idle_timeout_secs: u64_keyword(&tokens, ":vision-codex-idle-timeout-secs")?,
        vision_codex_absolute_timeout_secs: u64_keyword(
            &tokens,
            ":vision-codex-absolute-timeout-secs",
        )?,
    };
    if [
        cfg.conversation_get_tail_default,
        cfg.conversation_search_default_limit,
        cfg.message_search_default_limit,
        cfg.context_before_default,
        cfg.context_after_default,
        cfg.conversation_events_default_limit,
        cfg.agent_trajectory_default_limit,
        cfg.timeline_query_default_limit,
        cfg.timeline_query_max_limit,
        cfg.timeline_search_default_limit,
        cfg.timeline_search_max_limit,
    ]
    .iter()
    .any(|value| *value <= 0)
    {
        return Err(BlueprintConfigError::Parse(
            "conversation-ingestion numeric limits must be positive".into(),
        ));
    }
    if cfg.intent_router_timeout_ms == 0 {
        return Err(BlueprintConfigError::Parse(
            "conversation-ingestion intent router timeout must be positive".into(),
        ));
    }
    if cfg.vision_codex_idle_timeout_secs == 0 || cfg.vision_codex_absolute_timeout_secs == 0 {
        return Err(BlueprintConfigError::Parse(
            "conversation-ingestion Codex vision timeouts must be positive".into(),
        ));
    }
    if cfg.timeline_query_max_limit < cfg.timeline_query_default_limit {
        return Err(BlueprintConfigError::Parse(
            ":timeline-query-max-limit must be >= :timeline-query-default-limit".into(),
        ));
    }
    if cfg.timeline_search_max_limit < cfg.timeline_search_default_limit {
        return Err(BlueprintConfigError::Parse(
            ":timeline-search-max-limit must be >= :timeline-search-default-limit".into(),
        ));
    }
    Ok(cfg)
}

pub(crate) fn parse_autopilot_policy(
    source: &str,
) -> Result<AutopilotRuntimeConfig, BlueprintConfigError> {
    let workstation = parse_workstation_config(source)?;
    let block = find_form(source, "autopilot-policy")
        .ok_or_else(|| BlueprintConfigError::Parse("missing (autopilot-policy ...)".into()))?;
    let tokens = tokenize_lisp(&block);
    let cfg = AutopilotRuntimeConfig {
        boardtask_timeout_policy: workstation.timeout_policy,
        stale_conversation_minutes: int_keyword(&tokens, ":stale-conversation-minutes")?,
        slot_task_reap_stale_secs: int_keyword(&tokens, ":slot-task-reap-stale-secs")?,
        recover_stale_running_minutes: int_keyword(&tokens, ":recover-stale-running-minutes")?,
        slot_failure_throttle_secs: int_keyword(&tokens, ":slot-failure-throttle-secs")?,
        deploy_review_timeout_secs: u64_keyword(&tokens, ":deploy-review-timeout-secs")?,
        dynamic_slot_expiring_soon_secs: int_keyword(&tokens, ":dynamic-slot-expiring-soon-secs")?,
        stale_board_progress_minutes: int_keyword(&tokens, ":stale-board-progress-minutes")?,
        completed_job_gc_minutes: int_keyword(&tokens, ":completed-job-gc-minutes")?,
        idle_persistent_slot_secs: u64_keyword(&tokens, ":idle-persistent-slot-secs")?,
        recent_intents_window_secs: int_keyword(&tokens, ":recent-intents-window-secs")?,
        user_stuck_cooldown_secs: int_keyword(&tokens, ":user-stuck-cooldown-secs")?,
        direction_shift_cooldown_secs: int_keyword(&tokens, ":direction-shift-cooldown-secs")?,
    };
    if [
        cfg.stale_conversation_minutes,
        cfg.slot_task_reap_stale_secs,
        cfg.recover_stale_running_minutes,
        cfg.slot_failure_throttle_secs,
        cfg.dynamic_slot_expiring_soon_secs,
        cfg.stale_board_progress_minutes,
        cfg.completed_job_gc_minutes,
        cfg.recent_intents_window_secs,
        cfg.user_stuck_cooldown_secs,
        cfg.direction_shift_cooldown_secs,
    ]
    .iter()
    .any(|value| *value <= 0)
    {
        return Err(BlueprintConfigError::Parse(
            "autopilot-policy numeric windows must be positive".into(),
        ));
    }
    Ok(cfg)
}

pub(crate) fn parse_learning_engine_policy(
    source: &str,
) -> Result<LearningEngineRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "learning-engine-policy").ok_or_else(|| {
        BlueprintConfigError::Parse("missing (learning-engine-policy ...)".into())
    })?;
    let tokens = tokenize_lisp(&block);
    let cfg = LearningEngineRuntimeConfig {
        realtime_extraction_timeout_secs: u64_keyword(
            &tokens,
            ":realtime-extraction-timeout-secs",
        )?,
        decision_tier3_timeout_secs: u64_keyword(&tokens, ":decision-tier3-timeout-secs")?,
        habit_scan_timeout_secs: u64_keyword(&tokens, ":habit-scan-timeout-secs")?,
        timeline_analysis_interval_secs: int_keyword(&tokens, ":timeline-analysis-interval-secs")?,
        timeline_analysis_window_hours: int_keyword(&tokens, ":timeline-analysis-window-hours")?,
        timeline_error_limit: int_keyword(&tokens, ":timeline-error-limit")?,
        timeline_llm_sample_limit: int_keyword(&tokens, ":timeline-llm-sample-limit")?,
        timeline_slow_event_limit: usize_keyword(&tokens, ":timeline-slow-event-limit")?,
        timeline_slow_threshold_ms: int_keyword(&tokens, ":timeline-slow-threshold-ms")?,
        idle_explore_interval_secs: int_keyword(&tokens, ":idle-explore-interval-secs")?,
        habit_scan_interval_secs: int_keyword(&tokens, ":habit-scan-interval-secs")?,
        habit_scan_batch_size: usize_keyword(&tokens, ":habit-scan-batch-size")?,
        kb_auto_gc_interval_secs: int_keyword(&tokens, ":kb-auto-gc-interval-secs")?,
        kb_consolidation_interval_secs: int_keyword(&tokens, ":kb-consolidation-interval-secs")?,
        kb_reflection_interval_secs: int_keyword(&tokens, ":kb-reflection-interval-secs")?,
        kb_reflection_utility_threshold: f64_keyword(&tokens, ":kb-reflection-utility-threshold")?,
        kb_reflection_min_access: int_keyword(&tokens, ":kb-reflection-min-access")?,
        kb_reflection_max_entries: usize_keyword(&tokens, ":kb-reflection-max-entries")?,
        kb_reflection_max_tokens: u32_keyword(&tokens, ":kb-reflection-max-tokens")?,
        decision_harvest_interval_secs: int_keyword(&tokens, ":decision-harvest-interval-secs")?,
        cooccurrence_refresh_interval_secs: int_keyword(
            &tokens,
            ":cooccurrence-refresh-interval-secs",
        )?,
    };
    if [
        cfg.timeline_analysis_interval_secs,
        cfg.timeline_analysis_window_hours,
        cfg.timeline_error_limit,
        cfg.timeline_llm_sample_limit,
        cfg.timeline_slow_threshold_ms,
        cfg.idle_explore_interval_secs,
        cfg.habit_scan_interval_secs,
        cfg.kb_auto_gc_interval_secs,
        cfg.kb_consolidation_interval_secs,
        cfg.kb_reflection_interval_secs,
        cfg.kb_reflection_min_access,
        cfg.decision_harvest_interval_secs,
        cfg.cooccurrence_refresh_interval_secs,
    ]
    .iter()
    .any(|value| *value <= 0)
    {
        return Err(BlueprintConfigError::Parse(
            "learning-engine-policy numeric windows must be positive".into(),
        ));
    }
    if cfg.timeline_slow_event_limit == 0 || cfg.habit_scan_batch_size == 0 {
        return Err(BlueprintConfigError::Parse(
            "learning-engine-policy batch/limit fields must be positive".into(),
        ));
    }
    if !(0.0..=1.0).contains(&cfg.kb_reflection_utility_threshold) {
        return Err(BlueprintConfigError::Parse(
            ":kb-reflection-utility-threshold must be within 0.0..1.0".into(),
        ));
    }
    Ok(cfg)
}

fn string_list_keyword(tokens: &[String], key: &str) -> Result<Vec<String>, BlueprintConfigError> {
    let Some(pos) = tokens.iter().position(|token| token == key) else {
        return Err(BlueprintConfigError::Parse(format!("missing {}", key)));
    };
    let Some(next) = tokens.get(pos + 1) else {
        return Err(BlueprintConfigError::Parse(format!(
            "missing value for {}",
            key
        )));
    };
    if next != "[" {
        return Ok(vec![next.clone()]);
    }
    let mut out = Vec::new();
    for token in tokens.iter().skip(pos + 2) {
        if token == "]" {
            return Ok(out);
        }
        out.push(token.clone());
    }
    Err(BlueprintConfigError::Parse(format!(
        "{} list must close with ]",
        key
    )))
}

fn int_keyword(tokens: &[String], key: &str) -> Result<i64, BlueprintConfigError> {
    let value = keyword_value(tokens, key)
        .ok_or_else(|| BlueprintConfigError::Parse(format!("missing {}", key)))?;
    value
        .parse::<i64>()
        .map_err(|_| BlueprintConfigError::Parse(format!("{} must be an integer", key)))
}

fn u32_keyword(tokens: &[String], key: &str) -> Result<u32, BlueprintConfigError> {
    let value = int_keyword(tokens, key)?;
    if value <= 0 || value > u32::MAX as i64 {
        return Err(BlueprintConfigError::Parse(format!(
            "{} must be a positive u32",
            key
        )));
    }
    Ok(value as u32)
}

fn u64_keyword(tokens: &[String], key: &str) -> Result<u64, BlueprintConfigError> {
    let value = int_keyword(tokens, key)?;
    if value <= 0 {
        return Err(BlueprintConfigError::Parse(format!(
            "{} must be a positive u64",
            key
        )));
    }
    Ok(value as u64)
}

fn f64_keyword(tokens: &[String], key: &str) -> Result<f64, BlueprintConfigError> {
    let value = keyword_value(tokens, key)
        .ok_or_else(|| BlueprintConfigError::Parse(format!("missing {}", key)))?;
    value
        .parse::<f64>()
        .map_err(|_| BlueprintConfigError::Parse(format!("{} must be a number", key)))
}

fn usize_keyword(tokens: &[String], key: &str) -> Result<usize, BlueprintConfigError> {
    let value = keyword_value(tokens, key)
        .ok_or_else(|| BlueprintConfigError::Parse(format!("missing {}", key)))?;
    value
        .parse::<usize>()
        .map_err(|_| BlueprintConfigError::Parse(format!("{} must be a positive integer", key)))
}

fn keyword_value(tokens: &[String], key: &str) -> Option<String> {
    tokens
        .windows(2)
        .find(|pair| pair[0] == key)
        .map(|pair| pair[1].clone())
}

fn non_empty_keyword(tokens: &[String], key: &str) -> Result<String, BlueprintConfigError> {
    let value = keyword_value(tokens, key)
        .ok_or_else(|| BlueprintConfigError::Parse(format!("missing {}", key)))?;
    if value.trim().is_empty() {
        return Err(BlueprintConfigError::Parse(format!(
            "{} must not be empty",
            key
        )));
    }
    Ok(value)
}

fn optional_non_nil_keyword(tokens: &[String], key: &str) -> Option<String> {
    keyword_value(tokens, key).filter(|value| {
        let value = value.trim();
        !value.is_empty() && !matches!(value.to_ascii_lowercase().as_str(), "nil" | "none" | "null")
    })
}

fn normalize_model_profile_name(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('_', "-")
}

fn parse_spawn_model_arg(value: &str) -> Result<Option<String>, BlueprintConfigError> {
    let value = value.trim();
    if value.is_empty()
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "nil" | "none" | "null" | "default" | "claude-code-default"
        )
    {
        return Ok(None);
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    {
        return Err(BlueprintConfigError::Parse(
            "model-profile :spawn-model-arg must be a single safe CLI token".into(),
        ));
    }
    Ok(Some(value.to_string()))
}

fn parse_bool_token(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "t" | "1" | "yes" | "on" => Some(true),
        "false" | "nil" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn nearest_missiond_root(start: &Path) -> PathBuf {
    start
        .ancestors()
        .find(|candidate| candidate.join(".missiond").exists())
        .unwrap_or(start)
        .to_path_buf()
}

/// Locate the orchestrator's V3 blueprint, used as a fallback when a target
/// project lacks its own per-project override.
///
/// Resolution order:
/// 1. `MISSIOND_ORCHESTRATOR_ROOT` env -> $ROOT/.missiond/v3/missiond-blueprint.lisp
/// 2. Walk current cwd ancestors for `.missiond/v3/missiond-blueprint.lisp`
/// 3. Hardcoded `/Users/jinchen/Projects/missiond/.missiond/v3/missiond-blueprint.lisp`
///    — matches main.rs startup path and `universe.rs::locate_v3_blueprint`.
fn locate_orchestrator_blueprint() -> Option<PathBuf> {
    if let Ok(root) = std::env::var("MISSIOND_ORCHESTRATOR_ROOT") {
        let candidate = Path::new(&root)
            .join(".missiond")
            .join("v3")
            .join("missiond-blueprint.lisp");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors() {
            let candidate = ancestor
                .join(".missiond")
                .join("v3")
                .join("missiond-blueprint.lisp");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    let fallback =
        Path::new("/Users/jinchen/Projects/missiond/.missiond/v3/missiond-blueprint.lisp");
    if fallback.exists() {
        return Some(fallback.to_path_buf());
    }
    None
}

/// Resolve and read the V3 blueprint source for a target project.
///
/// Resolution:
/// 1. If `project_root` points at a project that has its own
///    `.missiond/v3/missiond-blueprint.lisp`, read that (per-project override).
/// 2. Otherwise (target lacks a v3 file, or `project_root` is absent), fall
///    back to the orchestrator blueprint via [`locate_orchestrator_blueprint`].
///    This makes workstation-config / router / cascade / governance behave as a
///    single inherited SSOT for registered external projects, rather than
///    failing dispatch (when `.missiond/` exists with no v3 file) or silently
///    degrading to embedded defaults (when no `.missiond/` exists at all).
/// 3. If neither target nor orchestrator blueprint can be located, return
///    `Ok(None)` and let the caller decide on defaults — preserves test/CLI
///    behavior outside any MissionD installation.
fn load_blueprint_source(
    project_root: Option<&str>,
) -> Result<Option<String>, BlueprintConfigError> {
    let target_path = project_root
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|root| {
            Path::new(root)
                .join(".missiond")
                .join("v3")
                .join("missiond-blueprint.lisp")
        })
        .filter(|p| p.exists());

    let blueprint_path = match target_path {
        Some(p) => p,
        None => match locate_orchestrator_blueprint() {
            Some(p) => p,
            None => return Ok(None),
        },
    };

    let source = fs::read_to_string(&blueprint_path).map_err(|err| BlueprintConfigError::Read {
        path: blueprint_path.clone(),
        message: err.to_string(),
    })?;
    Ok(Some(source))
}

fn load_runtime_blueprint_source(
    project_root: Option<&str>,
) -> Result<Option<String>, BlueprintConfigError> {
    let Some(root) = resolve_blueprint_root(project_root) else {
        return Ok(None);
    };
    if let Some(source) = load_compiled_v3_lisp_source(&root) {
        return Ok(Some(source));
    }
    load_blueprint_source(Some(root.to_string_lossy().as_ref()))
}

fn resolve_blueprint_root(project_root: Option<&str>) -> Option<PathBuf> {
    if let Some(root) = project_root
        .map(str::trim)
        .filter(|root| !root.is_empty())
        .map(PathBuf::from)
    {
        let target_blueprint = root
            .join(".missiond")
            .join("v3")
            .join("missiond-blueprint.lisp");
        if target_blueprint.exists() {
            return Some(root);
        }
    }
    locate_orchestrator_blueprint()
        .and_then(|path| path.parent().and_then(|v3| v3.parent()).map(Path::to_path_buf))
}

fn load_compiled_v3_lisp_source(project_root: &Path) -> Option<String> {
    let path = project_root
        .join(".missiond")
        .join("v3")
        .join("runtime")
        .join("compiled")
        .join("compiled-v3-blueprint.json");
    let raw = fs::read_to_string(&path).ok()?;
    let parsed: CompiledRuntimeEnvelope = serde_json::from_str(&raw).ok()?;
    if !parsed.diagnostics.is_empty() {
        return None;
    }
    let payload: CompiledV3Payload = serde_json::from_value(parsed.payload).ok()?;
    if payload.forms.is_empty() {
        return None;
    }
    let mut rendered = Vec::with_capacity(payload.forms.len());
    for form in &payload.forms {
        rendered.push(compiled_sexp_to_lisp(form)?);
    }
    let source = rendered.join("\n");
    source.contains("(missiond-blueprint").then_some(source)
}

fn compiled_sexp_to_lisp(node: &CompiledSexpNode) -> Option<String> {
    match node.node_type.as_str() {
        "atom" => node.value.clone(),
        "string" => node.value.as_deref().map(quote_lisp_string),
        "list" => {
            let open = if node.list_kind.as_deref() == Some("bracket") {
                "["
            } else {
                "("
            };
            let close = if node.list_kind.as_deref() == Some("bracket") {
                "]"
            } else {
                ")"
            };
            let mut parts = Vec::with_capacity(node.children.len());
            for child in &node.children {
                parts.push(compiled_sexp_to_lisp(child)?);
            }
            Some(format!("{open}{}{close}", parts.join(" ")))
        }
        _ => None,
    }
}

fn quote_lisp_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

pub(crate) fn load_compiled_runtime_snapshot(
    project_root: &Path,
    kind: &str,
    expected_source_hash: Option<&str>,
) -> CompiledRuntimeLoad {
    let file_name = match kind {
        "v3" => "compiled-v3-blueprint.json",
        "universe" => "compiled-project-universe.json",
        "workflows" => "compiled-workflows.json",
        other => {
            return CompiledRuntimeLoad {
                snapshot: None,
                diagnostics: vec![format!("unknown compiled runtime kind `{other}`")],
            };
        }
    };
    let path = project_root
        .join(".missiond")
        .join("v3")
        .join("runtime")
        .join("compiled")
        .join(file_name);
    if !path.exists() {
        return CompiledRuntimeLoad {
            snapshot: None,
            diagnostics: vec![format!(
                "compiled runtime snapshot missing: {}",
                path.display()
            )],
        };
    }
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) => {
            return CompiledRuntimeLoad {
                snapshot: None,
                diagnostics: vec![format!(
                    "failed to read compiled runtime snapshot {}: {err}",
                    path.display()
                )],
            };
        }
    };
    let parsed: CompiledRuntimeEnvelope = match serde_json::from_str(&raw) {
        Ok(parsed) => parsed,
        Err(err) => {
            return CompiledRuntimeLoad {
                snapshot: None,
                diagnostics: vec![format!(
                    "failed to parse compiled runtime snapshot {}: {err}",
                    path.display()
                )],
            };
        }
    };
    let mut diagnostics = Vec::new();
    if !parsed.diagnostics.is_empty() {
        diagnostics.push(format!(
            "compiled runtime snapshot {} contains {} diagnostic(s)",
            path.display(),
            parsed.diagnostics.len()
        ));
    }
    if let Some(expected) = expected_source_hash {
        if parsed.source_hash != expected {
            diagnostics.push(format!(
                "compiled runtime snapshot {} source_hash mismatch: expected {}, got {}",
                path.display(),
                expected,
                parsed.source_hash
            ));
            return CompiledRuntimeLoad {
                snapshot: None,
                diagnostics,
            };
        }
    }
    CompiledRuntimeLoad {
        snapshot: Some(CompiledRuntimeSnapshot {
            kind: kind.to_string(),
            path,
            schema_version: parsed.schema_version,
            source_hash: parsed.source_hash,
        }),
        diagnostics,
    }
}

#[allow(dead_code)]
pub(crate) fn load_compiled_project_universe(
    project_root: &Path,
    expected_source_hash: Option<&str>,
) -> CompiledPayloadLoad<CompiledProjectUniverse> {
    let loaded = load_compiled_payload::<CompiledProjectUniversePayload>(
        project_root,
        "universe",
        expected_source_hash,
    );
    CompiledPayloadLoad {
        payload: loaded.payload.map(|payload| CompiledProjectUniverse {
            projects: payload.projects,
            maturity: payload.maturity,
        }),
        snapshot: loaded.snapshot,
        diagnostics: loaded.diagnostics,
    }
}

#[allow(dead_code)]
pub(crate) fn load_compiled_workflow_contracts(
    project_root: &Path,
    expected_source_hash: Option<&str>,
) -> CompiledPayloadLoad<CompiledWorkflowContracts> {
    let loaded = load_compiled_payload::<CompiledWorkflowsPayload>(
        project_root,
        "workflows",
        expected_source_hash,
    );
    CompiledPayloadLoad {
        payload: loaded.payload.map(|payload| CompiledWorkflowContracts {
            workflows: payload.workflows,
        }),
        snapshot: loaded.snapshot,
        diagnostics: loaded.diagnostics,
    }
}

fn load_compiled_payload<T>(
    project_root: &Path,
    kind: &str,
    expected_source_hash: Option<&str>,
) -> CompiledPayloadLoad<T>
where
    T: for<'de> Deserialize<'de>,
{
    let file_name = match compiled_runtime_file_name(kind) {
        Some(file_name) => file_name,
        None => {
            return CompiledPayloadLoad {
                payload: None,
                snapshot: None,
                diagnostics: vec![format!("unknown compiled runtime kind `{kind}`")],
            };
        }
    };
    let path = project_root
        .join(".missiond")
        .join("v3")
        .join("runtime")
        .join("compiled")
        .join(file_name);
    if !path.exists() {
        return CompiledPayloadLoad {
            payload: None,
            snapshot: None,
            diagnostics: vec![format!(
                "compiled runtime snapshot missing: {}",
                path.display()
            )],
        };
    }
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) => {
            return CompiledPayloadLoad {
                payload: None,
                snapshot: None,
                diagnostics: vec![format!(
                    "failed to read compiled runtime snapshot {}: {err}",
                    path.display()
                )],
            };
        }
    };
    let parsed: CompiledRuntimeEnvelope = match serde_json::from_str(&raw) {
        Ok(parsed) => parsed,
        Err(err) => {
            return CompiledPayloadLoad {
                payload: None,
                snapshot: None,
                diagnostics: vec![format!(
                    "failed to parse compiled runtime snapshot {}: {err}",
                    path.display()
                )],
            };
        }
    };
    let mut diagnostics = Vec::new();
    if !parsed.diagnostics.is_empty() {
        diagnostics.push(format!(
            "compiled runtime snapshot {} contains {} diagnostic(s)",
            path.display(),
            parsed.diagnostics.len()
        ));
    }
    if let Some(expected) = expected_source_hash {
        if parsed.source_hash != expected {
            diagnostics.push(format!(
                "compiled runtime snapshot {} source_hash mismatch: expected {}, got {}",
                path.display(),
                expected,
                parsed.source_hash
            ));
            return CompiledPayloadLoad {
                payload: None,
                snapshot: None,
                diagnostics,
            };
        }
    }
    let payload = match serde_json::from_value(parsed.payload) {
        Ok(payload) => payload,
        Err(err) => {
            diagnostics.push(format!(
                "failed to decode compiled runtime payload {}: {err}",
                path.display()
            ));
            return CompiledPayloadLoad {
                payload: None,
                snapshot: None,
                diagnostics,
            };
        }
    };
    CompiledPayloadLoad {
        payload: Some(payload),
        snapshot: Some(CompiledRuntimeSnapshot {
            kind: kind.to_string(),
            path,
            schema_version: parsed.schema_version,
            source_hash: parsed.source_hash,
        }),
        diagnostics,
    }
}

fn compiled_runtime_file_name(kind: &str) -> Option<&'static str> {
    match kind {
        "v3" => Some("compiled-v3-blueprint.json"),
        "universe" => Some("compiled-project-universe.json"),
        "workflows" => Some("compiled-workflows.json"),
        _ => None,
    }
}

fn find_forms(source: &str, head: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        let Some((start, end)) = find_form_span(&source[offset..], head) else {
            break;
        };
        let absolute_start = offset + start;
        let absolute_end = offset + end;
        out.push(source[absolute_start..absolute_end].to_string());
        offset = absolute_end;
    }
    out
}

fn find_form(source: &str, head: &str) -> Option<String> {
    let (start, end) = find_form_span(source, head)?;
    Some(source[start..end].to_string())
}

fn find_form_span(source: &str, head: &str) -> Option<(usize, usize)> {
    let needle = format!("({}", head);
    let mut offset = 0;
    while offset < source.len() {
        let rel = source[offset..].find(&needle)?;
        let start = offset + rel;
        let after = source[start + needle.len()..].chars().next();
        if after.is_none_or(|c| c.is_whitespace() || c == ')' || c == '(') {
            let end = scan_form_end(source, start)?;
            return Some((start, end));
        }
        offset = start + needle.len();
    }
    None
}

fn scan_form_end(source: &str, start: usize) -> Option<usize> {
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escape = false;
    let mut in_comment = false;
    for (idx, ch) in source[start..].char_indices() {
        let abs = start + idx;
        if in_comment {
            if ch == '\n' {
                in_comment = false;
            }
            continue;
        }
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            ';' => in_comment = true,
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(abs + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}

fn tokenize_lisp(source: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = source.chars().peekable();
    let mut in_string = false;
    let mut escape = false;
    let mut in_comment = false;
    while let Some(ch) = chars.next() {
        if in_comment {
            if ch == '\n' {
                in_comment = false;
            }
            continue;
        }
        if in_string {
            if escape {
                current.push(ch);
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                tokens.push(std::mem::take(&mut current));
                in_string = false;
            } else {
                current.push(ch);
            }
            continue;
        }
        match ch {
            ';' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                in_comment = true;
            }
            '"' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                in_string = true;
            }
            '(' | ')' | '[' | ']' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                tokens.push(ch.to_string());
            }
            c if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLUEPRINT: &str = r#"
(missiond-blueprint
  (workstation-config
    (model-profile coding-default-opus-4-7 :spawn-model-arg nil)
    (model-profile daily-sonnet :spawn-model-arg "sonnet")
    (model-profile quick-haiku :spawn-model-arg "haiku")
    (slot-template coder :role coder :description "Dynamic coder slot (ephemeral)" :default-model-profile coding-default-opus-4-7 :mcp-config "/Users/jinchen/.xjp-mission/xjp-mcp-config.json" :default-cwd "/Users/jinchen/Projects")
    (slot-template researcher :role coder :description "Dynamic researcher slot (read-only analysis)" :default-model-profile coding-default-opus-4-7 :mcp-config "/Users/jinchen/.xjp-mission/xjp-mcp-config.json" :default-cwd "/Users/jinchen/Projects")
    (slot-template ops :role operator :description "Dynamic ops slot (ephemeral)" :default-model-profile daily-sonnet :mcp-config "/Users/jinchen/.xjp-mission/xjp-mcp-config.json" :default-cwd "/Users/jinchen/Projects")
    (cwd-policy dynamic-slot
      :allowed-prefixes ["/Users/jinchen/Projects" "/Users/jinchen/Downloads" "/Users/jinchen/Documents" "/tmp"])
    (startup-slot arch_maintenance :engine claude-code :lifecycle persistent :slot_id "slot-arch-maint" :role arch-maint :model_profile coding-default-opus-4-7 :timeout_secs 600 :skip_permissions true)
    (startup-slot strategy_analyst :engine gemini :lifecycle persistent :slot_id "slot-gemini-strategy" :role strategy :model_profile nil :timeout_secs 600 :skip_permissions true)
    (startup-slot gemini_router :engine gemini :lifecycle persistent :slot_id "slot-gemini-router" :role gemini-router :model_profile nil :timeout_secs 120 :skip_permissions true)
    (startup-slot lisp_survey :engine claude-code :lifecycle persistent :slot_id "lisp-surveyor" :role coder :model_profile coding-default-opus-4-7 :timeout_secs 900 :skip_permissions true)
    (timeout-policy boardtask-dispatch
      :default_secs 1800
      :min_secs 60
      :max_secs 7200
      :watchdog_grace_secs 120
      :missing_session_probe_secs 120)
    (timeout-policy claudecode-swarm
      :default_secs 600
      :min_secs 60
      :max_secs 7200)
    (timeout-policy pty-send-blocking
      :default_secs 300
      :min_secs 1
      :max_secs 7200)
    (timeout-policy dynamic-slot-spawn
      :default_secs 60
      :min_secs 10
      :max_secs 600)
    (capacity-policy swarm-workers
      :default_claude_workers 8
      :max_claude_workers 16
      :default_gemini_workers 2
      :max_gemini_workers 6
      :dynamic_slot_limit 20
      :delegate_rate_per_minute 24)
    (ttl-policy dynamic-slot
      :default_secs 14400
      :min_secs 300
      :max_secs 28800
      :default_extend_secs 3600
      :max_extend_secs 3600))
  (workstation-pool
    (worker claude-code-default
      :engine claude-code
      :role coder
      :slot-id "slot-claude-code-default"
      :task-type claude_code_default
      :model-profile coding-default-opus-4-7
      :model nil
      :task-classes [code implementation review context-pack ops]
      :capabilities [code-read code-write scoped-commit mcp]
      :max-concurrency 1
      :timeout-secs 1800
      :default-use code-implementation
      :accepts-boardtask true
      :write-allowed true)
    (worker claude-code-fast-patch
      :engine claude-code
      :role patcher
      :slot-id "slot-claude-code-fast-patch"
      :task-type claude_code_fast_patch
      :model-profile daily-sonnet
      :model nil
      :task-classes [patch test chore low-risk-fast-path]
      :capabilities [code-read code-write scoped-commit narrow-patch mcp]
      :max-concurrency 1
      :timeout-secs 900
      :default-use narrow-patch
      :accepts-boardtask true
      :write-allowed true)
    (worker gemini-ultra-pro
      :engine gemini
      :role researcher
      :slot-id "slot-gemini-ultra"
      :task-type gemini_ultra
      :model-profile gemini-ultra-pro-preview
      :model nil
      :approval-policy plan
      :tool-policy-path ".missiond/v3/policies/gemini-readonly-policy.toml"
      :task-classes [research review context-pack lisp-compression general]
      :capabilities [read-only analysis design-review]
      :max-concurrency 1
      :timeout-secs 900
      :default-use research-review
      :accepts-boardtask true
      :write-allowed false)
    (worker gemini-fast-survey
      :engine gemini
      :role survey
      :slot-id "slot-gemini-fast-survey"
      :task-type gemini_fast_survey
      :model-profile nil
      :model "gemini-2.5-flash"
      :approval-policy plan
      :tool-policy-path ".missiond/v3/policies/gemini-readonly-policy.toml"
      :task-classes [survey summary mechanical-scan]
      :capabilities [read-only summary]
      :max-concurrency 1
      :timeout-secs 600
      :default-use low-authority-survey
      :accepts-boardtask true
      :write-allowed false)
    (worker codex-master-control
      :engine codex
      :role orchestrator
      :slot-id "slot-codex-master-control"
      :task-type codex_master_control
      :model-profile codex-master-gpt-5-5-xhigh
      :model nil
      :reasoning-effort xhigh
      :search true
      :sandbox danger-full-access
      :approval-policy never
      :task-classes [master-control orchestration governance night-audit]
      :capabilities [board-write kb-write execution-log dispatch code-read code-write shell-exec search mcp full-access]
      :max-concurrency 1
      :timeout-secs 7200
      :default-use resident-master-control
      :accepts-boardtask false
      :write-allowed true))
	  (flow-runtime-policy
	    :llm-call-default-max-tokens 65536
	    :slot-task-default-model "opus"
	    :slot-task-default-timeout-secs 3600
	    :parallel-slot-default-parallelism 3
	    :parallel-slot-default-timeout-secs 1800)
  (compute-runtime-policy
    (timeout-policy tracked-pty-spawn
      :default_secs 30
      :min_secs 1
      :max_secs 600))
  (minimax-runtime-policy
    :model "MiniMax-M2.5-highspeed"
    :direct-http-timeout-secs 30
    :quota-throttle-secs 60
    :default-max-tokens 500)
	  (router-runtime-policy
	    :default-chat-model "gemini-3.1-pro"
	    :chat-default-max-tokens 16384
	    :file-chat-default-max-tokens 65536
    :flow-gemini-model "gemini-3.1-pro"
    :stateless-sonnet-model "claude-sonnet"
    :queued-sonnet-model "claude-sonnet"
    :anthropic-urgent-model "claude-opus-4-6"
    :anthropic-ops-model "claude-sonnet-4-6"
    :anthropic-docs-test-chore-model "claude-haiku-4-5-20251001"
    :compress-model "gemini-3.1-pro"
	    :compress-channel "google"
    :compress-max-tokens 2048
    :compress-char-budget-chars 100000
    :direct-http-timeout-secs 60
    :gemini-pty-queue-timeout-secs 30
    :gemini-http-queue-timeout-secs 300
    :gemini-file-upload-timeout-secs 600
    :gemini-file-poll-timeout-secs 300
    :gemini-cli-absolute-timeout-secs 900
    :gemini-cli-tool-exec-timeout-secs 300
    :queued-sonnet-quota-throttle-secs 30
    :queued-sonnet-default-max-tokens 1024)
	  (cascade-policy
	    :default-manifest "/Users/jinchen/Projects/universe.intent.lisp"
	    :allowed-root "/Users/jinchen/Projects"
    :trigger-enabled true
    :default-max-cycles 3
    :max-cycles-limit 12)
  (project-registry-policy
    :intent-path-candidates [".missiond/intent.lisp" ".jarvis/intent.lisp" "intent.lisp"]
    :default-universe-manifest "/Users/jinchen/Projects/universe.intent.lisp")
  (capability-governance-policy
    :review-sidecar ".missiond/v3/runtime/capability-usage-review.json"
    :protected-tool-patterns ["mission_execution" "mission_intent" "mission_forge_" "mission_sys_" "mission_daemon_update" "mission_health" "mission_power_control" "mission_kb_ops" "mission_audit" "mission_pty_signal" "mission_pty_confirm" "mission_incident"]
    :protected-flow-patterns ["engineering" "F-execution-log-governance" "F-incident-reaction" "F-capability-usage-monitoring"])
	  (memory-kb-policy
	    :pending-message-limit 60
	    :tool-result-preview-chars 1000
	    :assistant-preview-chars 500)
	  (learning-engine-policy
	    :realtime-extraction-timeout-secs 300
	    :decision-tier3-timeout-secs 300
	    :habit-scan-timeout-secs 600
	    :timeline-analysis-interval-secs 43200
	    :timeline-analysis-window-hours 12
	    :timeline-error-limit 20
	    :timeline-llm-sample-limit 50
	    :timeline-slow-event-limit 20
	    :timeline-slow-threshold-ms 60000
	    :idle-explore-interval-secs 7200
	    :habit-scan-interval-secs 14400
	    :habit-scan-batch-size 5
	    :kb-auto-gc-interval-secs 3600
	    :kb-consolidation-interval-secs 86400
	    :kb-reflection-interval-secs 604800
	    :kb-reflection-utility-threshold 0.3
	    :kb-reflection-min-access 3
	    :kb-reflection-max-entries 20
	    :kb-reflection-max-tokens 2000
	    :decision-harvest-interval-secs 86400
	    :cooccurrence-refresh-interval-secs 21600)
	  (conversation-ingestion-policy
	    :conversation-get-tail-default 50
    :conversation-search-default-limit 10
    :message-search-default-limit 20
    :context-before-default 3
    :context-after-default 5
    :conversation-events-default-limit 100
    :agent-trajectory-default-limit 200
    :timeline-query-default-limit 50
    :timeline-query-max-limit 200
    :timeline-search-default-limit 20
    :timeline-search-max-limit 100
    :intent-router-model "claude-opus-4.6"
    :intent-router-timeout-ms 10000
    :vision-codex-binary "codex"
    :vision-codex-model "gpt-5.4"
    :vision-codex-idle-timeout-secs 120
    :vision-codex-absolute-timeout-secs 300)
  (autopilot-policy
    :stale-conversation-minutes 10
    :slot-task-reap-stale-secs 1800
    :recover-stale-running-minutes 15
    :slot-failure-throttle-secs 1800
    :deploy-review-timeout-secs 600
    :dynamic-slot-expiring-soon-secs 900
    :stale-board-progress-minutes 30
    :completed-job-gc-minutes 30
    :idle-persistent-slot-secs 1800
    :recent-intents-window-secs 1800
    :user-stuck-cooldown-secs 1800
    :direction-shift-cooldown-secs 3600))
"#;

    #[test]
    fn parses_workstation_config_defaults() {
        let cfg = parse_workstation_config(BLUEPRINT).expect("parse");
        assert_eq!(
            cfg.default_model_profile_for_template("coder"),
            Some(DEFAULT_MODEL_PROFILE)
        );
        assert_eq!(
            cfg.default_model_profile_for_template("researcher"),
            Some(DEFAULT_MODEL_PROFILE)
        );
        assert_eq!(
            cfg.default_model_profile_for_template("ops"),
            Some("daily-sonnet")
        );
        assert_eq!(cfg.default_spawn_model_for_template("coder").unwrap(), None);
        assert_eq!(
            cfg.default_spawn_model_for_template("researcher").unwrap(),
            None
        );
        assert_eq!(
            cfg.default_spawn_model_for_template("ops").unwrap(),
            Some("sonnet".to_string())
        );
        let coder_template = cfg.slot_template("coder").expect("coder template");
        assert_eq!(coder_template.role, "coder");
        assert_eq!(coder_template.default_cwd, "/Users/jinchen/Projects");
        assert_eq!(
            coder_template.mcp_config.as_deref(),
            Some("/Users/jinchen/.xjp-mission/xjp-mcp-config.json")
        );
        assert_eq!(
            cfg.available_slot_template_names(),
            vec!["coder", "ops", "researcher"]
        );
        assert_eq!(
            cfg.allowed_cwd_prefixes(),
            &[
                PathBuf::from("/Users/jinchen/Projects"),
                PathBuf::from("/Users/jinchen/Downloads"),
                PathBuf::from("/Users/jinchen/Documents"),
                PathBuf::from("/tmp")
            ]
        );
        assert_eq!(
            cfg.spawn_model_for_profile("quick-haiku").unwrap(),
            Some("haiku".to_string())
        );
        assert_eq!(
            cfg.spawn_model_for_profile("coding_default_opus_4_7")
                .unwrap(),
            None
        );
        assert_eq!(
            cfg.spawn_model_for_profile(DEFAULT_CODEX_MASTER_PROFILE)
                .unwrap(),
            Some("gpt-5.5".to_string())
        );
        assert_eq!(cfg.startup_slots().len(), 4);
        assert_eq!(cfg.workstation_pool().len(), 5);
        assert_eq!(
            cfg.boardtask_pool_candidates("research")
                .first()
                .map(|worker| worker.id.as_str()),
            Some("gemini-ultra-pro")
        );
        let gemini = cfg
            .workstation_pool()
            .iter()
            .find(|worker| worker.id == "gemini-ultra-pro")
            .expect("gemini worker");
        assert!(!gemini.write_allowed);
        assert_eq!(gemini.approval_policy.as_deref(), Some("plan"));
        assert_eq!(
            gemini.tool_policy_path.as_deref(),
            Some(".missiond/v3/policies/gemini-readonly-policy.toml")
        );
        assert_eq!(
            cfg.boardtask_pool_candidates("code")
                .first()
                .map(|worker| worker.id.as_str()),
            Some("claude-code-default")
        );
        let master = cfg
            .workstation_pool()
            .iter()
            .find(|worker| worker.id == "codex-master-control")
            .expect("codex master worker");
        assert_eq!(master.engine, "codex");
        assert_eq!(
            master.model_profile.as_deref(),
            Some(DEFAULT_CODEX_MASTER_PROFILE)
        );
        assert_eq!(master.reasoning_effort.as_deref(), Some("xhigh"));
        assert!(master.search_enabled);
        assert!(!master.accepts_boardtask);
        let lisp_survey = cfg
            .startup_slots()
            .iter()
            .find(|slot| slot.task_type == "lisp_survey")
            .expect("lisp survey startup slot");
        assert_eq!(lisp_survey.engine, "claude-code");
        assert_eq!(
            lisp_survey.model_profile.as_deref(),
            Some(DEFAULT_MODEL_PROFILE)
        );
        assert_eq!(lisp_survey.timeout_secs, 900);
        assert_eq!(cfg.timeout_policy.default_secs, 1800);
        assert_eq!(cfg.timeout_policy.min_secs, 60);
        assert_eq!(cfg.timeout_policy.max_secs, 7200);
        assert_eq!(cfg.timeout_policy.watchdog_grace_secs, 120);
        assert_eq!(cfg.cc_swarm_timeout_policy.default_secs, 600);
        assert_eq!(cfg.cc_swarm_timeout_policy.min_secs, 60);
        assert_eq!(cfg.cc_swarm_timeout_policy.max_secs, 7200);
        assert_eq!(cfg.pty_send_timeout_policy.default_secs, 300);
        assert_eq!(cfg.pty_send_timeout_policy.min_secs, 1);
        assert_eq!(cfg.pty_send_timeout_policy.max_secs, 7200);
        assert_eq!(cfg.dynamic_slot_spawn_timeout_policy.default_secs, 60);
        assert_eq!(cfg.dynamic_slot_spawn_timeout_policy.min_secs, 10);
        assert_eq!(cfg.dynamic_slot_spawn_timeout_policy.max_secs, 600);
        assert_eq!(cfg.swarm_capacity_policy.default_claude_workers, 8);
        assert_eq!(cfg.swarm_capacity_policy.max_claude_workers, 16);
        assert_eq!(cfg.swarm_capacity_policy.default_gemini_workers, 2);
        assert_eq!(cfg.swarm_capacity_policy.max_gemini_workers, 6);
        assert_eq!(cfg.dynamic_slot_limit(), 20);
        assert_eq!(cfg.delegate_rate_per_minute(), 24);
        assert_eq!(cfg.slot_ttl_policy.default_secs, 14400);
        assert_eq!(cfg.slot_ttl_policy.min_secs, 300);
        assert_eq!(cfg.slot_ttl_policy.max_secs, 28800);
        assert_eq!(cfg.slot_ttl_policy.default_extend_secs, 3600);
        assert_eq!(cfg.slot_ttl_policy.max_extend_secs, 3600);
    }

    #[test]
    fn timeout_policy_clamps_values() {
        let cfg = parse_workstation_config(BLUEPRINT).expect("parse");
        assert_eq!(cfg.clamp_timeout_secs(None), 1800);
        assert_eq!(cfg.clamp_timeout_secs(Some(5)), 60);
        assert_eq!(cfg.clamp_timeout_secs(Some(99999)), 7200);
        assert_eq!(cfg.clamp_timeout_secs(Some(3300)), 3300);
        assert_eq!(cfg.clamp_cc_swarm_timeout_ms(None), 600_000);
        assert_eq!(cfg.clamp_cc_swarm_timeout_ms(Some(1000)), 60_000);
        assert_eq!(cfg.clamp_cc_swarm_timeout_ms(Some(99_999_999)), 7_200_000);
        assert_eq!(cfg.clamp_cc_swarm_timeout_ms(Some(900_000)), 900_000);
        assert_eq!(cfg.clamp_pty_send_timeout_ms(None), 300_000);
        assert_eq!(cfg.clamp_pty_send_timeout_ms(Some(500)), 1_000);
        assert_eq!(cfg.clamp_pty_send_timeout_ms(Some(99_999_999)), 7_200_000);
        assert_eq!(cfg.clamp_pty_send_timeout_ms(Some(42_000)), 42_000);
        assert_eq!(cfg.dynamic_slot_spawn_timeout_secs(), 60);
        assert_eq!(cfg.clamp_swarm_claude_workers(None), 8);
        assert_eq!(cfg.clamp_swarm_claude_workers(Some(999)), 16);
        assert_eq!(cfg.clamp_swarm_gemini_workers(None), 2);
        assert_eq!(cfg.clamp_swarm_gemini_workers(Some(999)), 6);
        assert_eq!(cfg.clamp_slot_ttl_secs(None), 14400);
        assert_eq!(cfg.clamp_slot_ttl_secs(Some(5)), 300);
        assert_eq!(cfg.clamp_slot_ttl_secs(Some(99_999)), 28800);
        assert_eq!(cfg.clamp_slot_ttl_secs(Some(3600)), 3600);
        assert_eq!(cfg.default_slot_extend_secs(), 3600);
        assert_eq!(cfg.max_slot_extend_secs(), 3600);
    }

    #[test]
    fn parses_flow_runtime_policy() {
        let cfg = parse_flow_runtime_policy(BLUEPRINT).expect("parse");
        assert_eq!(cfg.llm_call_default_max_tokens, 65536);
        assert_eq!(cfg.slot_task_default_model, "opus");
        assert_eq!(cfg.slot_task_default_timeout_secs, 3600);
        assert_eq!(cfg.parallel_slot_default_parallelism, 3);
        assert_eq!(cfg.parallel_slot_default_timeout_secs, 1800);
    }

    #[test]
    fn parses_compute_runtime_policy() {
        let cfg = parse_compute_runtime_policy(BLUEPRINT).expect("parse compute runtime policy");
        assert_eq!(cfg.pty_spawn_timeout_policy.default_secs, 30);
        assert_eq!(cfg.pty_spawn_timeout_policy.min_secs, 1);
        assert_eq!(cfg.pty_spawn_timeout_policy.max_secs, 600);
        assert_eq!(cfg.pty_spawn_timeout_secs(), 30);
    }

    #[test]
    fn parses_minimax_runtime_policy() {
        let cfg = parse_minimax_runtime_policy(BLUEPRINT).expect("parse minimax runtime policy");
        assert_eq!(cfg.model, DEFAULT_MINIMAX_MODEL);
        assert_eq!(cfg.direct_http_timeout_secs, 30);
        assert_eq!(
            cfg.direct_http_timeout(),
            std::time::Duration::from_secs(30)
        );
        assert_eq!(cfg.quota_throttle_secs, 60);
        assert_eq!(
            cfg.quota_throttle_sleep(),
            std::time::Duration::from_secs(60)
        );
        assert_eq!(cfg.default_max_tokens, 500);
    }

    #[test]
    fn parses_router_runtime_policy() {
        let cfg = parse_router_runtime_policy(BLUEPRINT).expect("parse router runtime policy");
        assert_eq!(cfg.default_chat_model, DEFAULT_ROUTER_CHAT_MODEL);
        assert_eq!(cfg.chat_default_max_tokens, 16384);
        assert_eq!(cfg.file_chat_default_max_tokens, 65536);
        assert_eq!(cfg.flow_gemini_model, DEFAULT_ROUTER_FLOW_GEMINI_MODEL);
        assert_eq!(
            cfg.stateless_sonnet_model,
            DEFAULT_ROUTER_STATELESS_SONNET_MODEL
        );
        assert_eq!(cfg.queued_sonnet_model, DEFAULT_ROUTER_QUEUED_SONNET_MODEL);
        assert_eq!(
            cfg.anthropic_urgent_model,
            DEFAULT_ROUTER_ANTHROPIC_URGENT_MODEL
        );
        assert_eq!(cfg.anthropic_ops_model, DEFAULT_ROUTER_ANTHROPIC_OPS_MODEL);
        assert_eq!(
            cfg.anthropic_docs_test_chore_model,
            DEFAULT_ROUTER_ANTHROPIC_DOCS_TEST_CHORE_MODEL
        );
        assert_eq!(cfg.compress_model, DEFAULT_ROUTER_COMPRESS_MODEL);
        assert_eq!(cfg.compress_channel, DEFAULT_ROUTER_COMPRESS_CHANNEL);
        assert_eq!(cfg.compress_max_tokens, 2048);
        assert_eq!(cfg.compress_char_budget_chars, 100_000);
        assert_eq!(cfg.direct_http_timeout_secs, 60);
        assert_eq!(
            cfg.direct_http_timeout(),
            std::time::Duration::from_secs(60)
        );
        assert_eq!(cfg.gemini_pty_queue_timeout_secs, 30);
        assert_eq!(
            cfg.gemini_pty_queue_timeout(),
            std::time::Duration::from_secs(30)
        );
        assert_eq!(cfg.gemini_http_queue_timeout_secs, 300);
        assert_eq!(
            cfg.gemini_http_queue_timeout(),
            std::time::Duration::from_secs(300)
        );
        assert_eq!(cfg.gemini_file_upload_timeout_secs, 600);
        assert_eq!(
            cfg.gemini_file_upload_timeout(),
            std::time::Duration::from_secs(600)
        );
        assert_eq!(cfg.gemini_file_poll_timeout_secs, 300);
        assert_eq!(
            cfg.gemini_file_poll_timeout(),
            std::time::Duration::from_secs(300)
        );
        assert_eq!(cfg.gemini_cli_absolute_timeout_secs, 900);
        assert_eq!(
            cfg.gemini_cli_absolute_timeout(),
            std::time::Duration::from_secs(900)
        );
        assert_eq!(cfg.gemini_cli_tool_exec_timeout_secs, 300);
        assert_eq!(
            cfg.gemini_cli_tool_exec_timeout(),
            std::time::Duration::from_secs(300)
        );
        assert_eq!(cfg.queued_sonnet_quota_throttle_secs, 30);
        assert_eq!(
            cfg.queued_sonnet_quota_throttle(),
            std::time::Duration::from_secs(30)
        );
        assert_eq!(cfg.queued_sonnet_default_max_tokens, 1024);
    }

    #[test]
    fn missing_router_runtime_policy_is_rejected() {
        let source = BLUEPRINT.replace("(router-runtime-policy", "(router-runtime-policy-disabled");
        let err = parse_router_runtime_policy(&source).expect_err("missing router runtime policy");
        assert!(err.to_string().contains("router-runtime-policy"));
    }

    #[test]
    fn parses_autopilot_policy() {
        let cfg = parse_autopilot_policy(BLUEPRINT).expect("parse autopilot policy");
        assert_eq!(cfg.boardtask_timeout_policy.default_secs, 1800);
        assert_eq!(cfg.boardtask_timeout_policy.watchdog_grace_secs, 120);
        assert_eq!(cfg.stale_conversation_minutes, 10);
        assert_eq!(cfg.slot_task_reap_stale_secs, 1800);
        assert_eq!(cfg.recover_stale_running_minutes, 15);
        assert_eq!(cfg.slot_failure_throttle_secs, 1800);
        assert_eq!(cfg.deploy_review_timeout_secs, 600);
        assert_eq!(cfg.deploy_review_timeout_ms(), 600_000);
        assert_eq!(cfg.dynamic_slot_expiring_soon_secs, 900);
        assert_eq!(cfg.stale_board_progress_minutes, 30);
        assert_eq!(cfg.completed_job_gc_minutes, 30);
        assert_eq!(cfg.idle_persistent_slot_secs, 1800);
        assert_eq!(cfg.recent_intents_window_secs, 1800);
        assert_eq!(cfg.user_stuck_cooldown_secs, 1800);
        assert_eq!(cfg.direction_shift_cooldown_secs, 3600);
    }

    #[test]
    fn missing_autopilot_policy_is_rejected() {
        let source = BLUEPRINT.replace("(autopilot-policy", "(autopilot-policy-disabled");
        let err = parse_autopilot_policy(&source).expect_err("missing autopilot policy");
        assert!(err.to_string().contains("autopilot-policy"));
    }

    #[test]
    fn missing_flow_runtime_policy_is_rejected() {
        let source = BLUEPRINT.replace("(flow-runtime-policy", "(flow-runtime-policy-disabled");
        let err = parse_flow_runtime_policy(&source).expect_err("missing flow policy");
        assert!(err.to_string().contains("flow-runtime-policy"));
    }

    #[test]
    fn missing_compute_runtime_policy_is_rejected() {
        let source = BLUEPRINT.replace(
            "(compute-runtime-policy",
            "(compute-runtime-policy-disabled",
        );
        let err = parse_compute_runtime_policy(&source).expect_err("missing compute policy");
        assert!(err.to_string().contains("compute-runtime-policy"));
    }

    #[test]
    fn missing_minimax_runtime_policy_is_rejected() {
        let source = BLUEPRINT.replace(
            "(minimax-runtime-policy",
            "(minimax-runtime-policy-disabled",
        );
        let err = parse_minimax_runtime_policy(&source).expect_err("missing minimax policy");
        assert!(err.to_string().contains("minimax-runtime-policy"));
    }

    #[test]
    fn missing_timeout_policy_is_rejected() {
        let err = parse_workstation_config(
            r#"(missiond-blueprint
  (workstation-config
    (slot-template coder :role coder :description "Dynamic coder slot (ephemeral)" :default-model-profile coding-default-opus-4-7 :default-cwd "/Users/jinchen/Projects")
    (cwd-policy dynamic-slot :allowed-prefixes ["/Users/jinchen/Projects"]))
  (workstation-pool
    (worker claude-code-default :engine claude-code :role coder :slot-id "slot-claude-code-default" :task-type claude_code_default :model-profile coding-default-opus-4-7 :model nil :task-classes [code] :capabilities [code-write] :max-concurrency 1 :timeout-secs 1800 :default-use code-implementation :accepts-boardtask true :write-allowed true)
    (worker gemini-ultra-pro :engine gemini :role researcher :slot-id "slot-gemini-ultra" :task-type gemini_ultra :model-profile gemini-ultra-pro-preview :model nil :approval-policy plan :tool-policy-path ".missiond/v3/policies/gemini-readonly-policy.toml" :task-classes [research] :capabilities [read-only] :max-concurrency 1 :timeout-secs 900 :default-use research-review :accepts-boardtask true :write-allowed false)
    (worker codex-master-control :engine codex :role orchestrator :slot-id "slot-codex-master-control" :task-type codex_master_control :model-profile codex-master-gpt-5-5-xhigh :model nil :reasoning-effort xhigh :search true :sandbox danger-full-access :approval-policy never :task-classes [master-control] :capabilities [board-write kb-write execution-log dispatch code-read code-write shell-exec search mcp full-access] :max-concurrency 1 :timeout-secs 7200 :default-use resident-master-control :accepts-boardtask false :write-allowed true)))"#,
        )
        .expect_err("missing policy");
        assert!(err
            .to_string()
            .contains("timeout-policy boardtask-dispatch"));
    }

    #[test]
    fn missing_cwd_policy_is_rejected() {
        let source = BLUEPRINT.replace(
            "(cwd-policy dynamic-slot",
            "(cwd-policy-disabled dynamic-slot",
        );
        let err = parse_workstation_config(&source).expect_err("missing cwd policy");
        assert!(err.to_string().contains("cwd-policy dynamic-slot"));
    }

    #[test]
    fn missing_ttl_policy_is_rejected() {
        let source = r#"
(missiond-blueprint
  (workstation-config
    (slot-template coder :role coder :description "Dynamic coder slot (ephemeral)" :default-model-profile coding-default-opus-4-7 :default-cwd "/Users/jinchen/Projects")
    (cwd-policy dynamic-slot :allowed-prefixes ["/Users/jinchen/Projects"])
    (timeout-policy boardtask-dispatch
      :default_secs 1800
      :min_secs 60
      :max_secs 7200
      :watchdog_grace_secs 120
      :missing_session_probe_secs 120)
    (timeout-policy claudecode-swarm
      :default_secs 600
      :min_secs 60
      :max_secs 7200)
    (timeout-policy pty-send-blocking
      :default_secs 300
      :min_secs 1
      :max_secs 7200)
    (timeout-policy dynamic-slot-spawn
      :default_secs 60
      :min_secs 10
      :max_secs 600)
    (capacity-policy swarm-workers
      :default_claude_workers 8
      :max_claude_workers 16
      :default_gemini_workers 2
      :max_gemini_workers 6
      :dynamic_slot_limit 20
      :delegate_rate_per_minute 24))
  (workstation-pool
    (worker claude-code-default :engine claude-code :role coder :slot-id "slot-claude-code-default" :task-type claude_code_default :model-profile coding-default-opus-4-7 :model nil :task-classes [code] :capabilities [code-write] :max-concurrency 1 :timeout-secs 1800 :default-use code-implementation :accepts-boardtask true :write-allowed true)
    (worker gemini-ultra-pro :engine gemini :role researcher :slot-id "slot-gemini-ultra" :task-type gemini_ultra :model-profile gemini-ultra-pro-preview :model nil :approval-policy plan :tool-policy-path ".missiond/v3/policies/gemini-readonly-policy.toml" :task-classes [research] :capabilities [read-only] :max-concurrency 1 :timeout-secs 900 :default-use research-review :accepts-boardtask true :write-allowed false)
    (worker codex-master-control :engine codex :role orchestrator :slot-id "slot-codex-master-control" :task-type codex_master_control :model-profile codex-master-gpt-5-5-xhigh :model nil :reasoning-effort xhigh :search true :sandbox danger-full-access :approval-policy never :task-classes [master-control] :capabilities [board-write kb-write execution-log dispatch code-read code-write shell-exec search mcp full-access] :max-concurrency 1 :timeout-secs 7200 :default-use resident-master-control :accepts-boardtask false :write-allowed true)))
"#;
        let err = parse_workstation_config(source).expect_err("missing ttl policy");
        assert!(err.to_string().contains("ttl-policy dynamic-slot"));
    }

    #[test]
    fn missing_capacity_policy_is_rejected() {
        let source = BLUEPRINT.replace(
            "(capacity-policy swarm-workers",
            "(capacity-policy-disabled swarm-workers",
        );
        let err = parse_workstation_config(&source).expect_err("missing capacity policy");
        assert!(err.to_string().contains("capacity-policy swarm-workers"));
    }

    #[test]
    fn parses_cascade_policy_defaults() {
        let cfg = parse_cascade_policy(BLUEPRINT).expect("parse cascade policy");
        assert_eq!(
            cfg.default_manifest_path,
            PathBuf::from(DEFAULT_CASCADE_MANIFEST_PATH)
        );
        assert_eq!(
            cfg.allowed_root,
            PathBuf::from(DEFAULT_CASCADE_ALLOWED_ROOT)
        );
        assert!(cfg.trigger_enabled);
        assert_eq!(cfg.default_max_cycles, 3);
        assert_eq!(cfg.max_cycles_limit, 12);
        assert_eq!(cfg.clamp_max_cycles(None), 3);
        assert_eq!(cfg.clamp_max_cycles(Some(0)), 3);
        assert_eq!(cfg.clamp_max_cycles(Some(8)), 8);
        assert_eq!(cfg.clamp_max_cycles(Some(99)), 12);
    }

    #[test]
    fn missing_cascade_policy_is_rejected() {
        let err = parse_cascade_policy("(missiond-blueprint)")
            .expect_err("missing cascade policy should fail");
        assert!(err.to_string().contains("cascade-policy"));
    }

    #[test]
    fn parses_project_registry_policy_defaults() {
        let cfg = parse_project_registry_policy(BLUEPRINT).expect("parse project policy");
        assert_eq!(
            cfg.intent_path_candidates,
            vec![
                ".missiond/intent.lisp".to_string(),
                ".jarvis/intent.lisp".to_string(),
                "intent.lisp".to_string()
            ]
        );
        assert_eq!(
            cfg.default_universe_manifest,
            PathBuf::from(DEFAULT_PROJECT_UNIVERSE_MANIFEST)
        );
    }

    #[test]
    fn missing_project_registry_policy_is_rejected() {
        let err = parse_project_registry_policy("(missiond-blueprint)")
            .expect_err("missing project registry policy should fail");
        assert!(err.to_string().contains("project-registry-policy"));
    }

    #[test]
    fn parses_capability_governance_policy_defaults() {
        let cfg = parse_capability_governance_policy(BLUEPRINT)
            .expect("parse capability governance policy");
        assert_eq!(
            cfg.review_sidecar_path,
            PathBuf::from(DEFAULT_CAPABILITY_REVIEW_SIDECAR)
        );
        assert!(cfg.is_protected_tool("mission_intent"));
        assert!(cfg.is_protected_tool("mission_forge_build"));
        assert!(cfg.is_protected_tool("mission_audit"));
        assert!(!cfg.is_protected_tool("mission_board_query"));
        assert!(cfg.is_protected_flow("engineering"));
        assert!(cfg.is_protected_flow("F-execution-log-governance"));
        assert!(!cfg.is_protected_flow("hello-parallel"));
    }

    #[test]
    fn missing_capability_governance_policy_is_rejected() {
        let err = parse_capability_governance_policy("(missiond-blueprint)")
            .expect_err("missing capability governance policy should fail");
        assert!(err.to_string().contains("capability-governance-policy"));
    }

    #[test]
    fn parses_memory_kb_policy_defaults() {
        let cfg = parse_memory_kb_policy(BLUEPRINT).expect("parse memory kb policy");
        assert_eq!(
            cfg.pending_message_limit,
            DEFAULT_MEMORY_PENDING_MESSAGE_LIMIT
        );
        assert_eq!(
            cfg.tool_result_preview_chars,
            DEFAULT_MEMORY_TOOL_RESULT_PREVIEW_CHARS
        );
        assert_eq!(
            cfg.assistant_preview_chars,
            DEFAULT_MEMORY_ASSISTANT_PREVIEW_CHARS
        );
    }

    #[test]
    fn missing_memory_kb_policy_is_rejected() {
        let err = parse_memory_kb_policy("(missiond-blueprint)")
            .expect_err("missing memory kb policy should fail");
        assert!(err.to_string().contains("memory-kb-policy"));
    }

    #[test]
    fn parses_learning_engine_policy_defaults() {
        let cfg = parse_learning_engine_policy(BLUEPRINT).expect("parse learning engine policy");
        assert_eq!(
            cfg.realtime_extraction_timeout_secs,
            DEFAULT_LEARNING_REALTIME_EXTRACTION_TIMEOUT_SECS
        );
        assert_eq!(cfg.realtime_extraction_timeout_ms(), 300_000);
        assert_eq!(cfg.decision_tier3_timeout_ms(), 300_000);
        assert_eq!(cfg.habit_scan_timeout_ms(), 600_000);
        assert_eq!(
            cfg.timeline_analysis_interval_secs,
            DEFAULT_LEARNING_TIMELINE_ANALYSIS_INTERVAL_SECS
        );
        assert_eq!(cfg.timeline_window_arg(), "12h");
        assert_eq!(
            cfg.timeline_error_limit,
            DEFAULT_LEARNING_TIMELINE_ERROR_LIMIT
        );
        assert_eq!(
            cfg.timeline_llm_sample_limit,
            DEFAULT_LEARNING_TIMELINE_LLM_SAMPLE_LIMIT
        );
        assert_eq!(
            cfg.timeline_slow_event_limit,
            DEFAULT_LEARNING_TIMELINE_SLOW_EVENT_LIMIT
        );
        assert_eq!(
            cfg.timeline_slow_threshold_ms,
            DEFAULT_LEARNING_TIMELINE_SLOW_THRESHOLD_MS
        );
        assert_eq!(
            cfg.idle_explore_interval_secs,
            DEFAULT_LEARNING_IDLE_EXPLORE_INTERVAL_SECS
        );
        assert_eq!(
            cfg.habit_scan_interval_secs,
            DEFAULT_LEARNING_HABIT_SCAN_INTERVAL_SECS
        );
        assert_eq!(
            cfg.habit_scan_batch_size,
            DEFAULT_LEARNING_HABIT_SCAN_BATCH_SIZE
        );
        assert_eq!(
            cfg.kb_auto_gc_interval_secs,
            DEFAULT_LEARNING_KB_AUTO_GC_INTERVAL_SECS
        );
        assert_eq!(
            cfg.kb_consolidation_interval_secs,
            DEFAULT_LEARNING_KB_CONSOLIDATION_INTERVAL_SECS
        );
        assert_eq!(
            cfg.kb_reflection_interval_secs,
            DEFAULT_LEARNING_KB_REFLECTION_INTERVAL_SECS
        );
        assert_eq!(
            cfg.kb_reflection_utility_threshold,
            DEFAULT_LEARNING_KB_REFLECTION_UTILITY_THRESHOLD
        );
        assert_eq!(
            cfg.kb_reflection_min_access,
            DEFAULT_LEARNING_KB_REFLECTION_MIN_ACCESS
        );
        assert_eq!(
            cfg.kb_reflection_max_entries,
            DEFAULT_LEARNING_KB_REFLECTION_MAX_ENTRIES
        );
        assert_eq!(
            cfg.kb_reflection_max_tokens,
            DEFAULT_LEARNING_KB_REFLECTION_MAX_TOKENS
        );
        assert_eq!(
            cfg.decision_harvest_interval_secs,
            DEFAULT_LEARNING_DECISION_HARVEST_INTERVAL_SECS
        );
        assert_eq!(
            cfg.cooccurrence_refresh_interval_secs,
            DEFAULT_LEARNING_COOCCURRENCE_REFRESH_INTERVAL_SECS
        );
    }

    #[test]
    fn missing_learning_engine_policy_is_rejected() {
        let err = parse_learning_engine_policy("(missiond-blueprint)")
            .expect_err("missing learning engine policy should fail");
        assert!(err.to_string().contains("learning-engine-policy"));
    }

    #[test]
    fn parses_conversation_ingestion_policy_defaults() {
        let cfg = parse_conversation_ingestion_policy(BLUEPRINT)
            .expect("parse conversation ingestion policy");
        assert_eq!(
            cfg.conversation_get_tail_default,
            DEFAULT_CONVERSATION_GET_TAIL
        );
        assert_eq!(
            cfg.conversation_search_default_limit,
            DEFAULT_CONVERSATION_SEARCH_LIMIT
        );
        assert_eq!(
            cfg.message_search_default_limit,
            DEFAULT_MESSAGE_SEARCH_LIMIT
        );
        assert_eq!(cfg.context_before_default, DEFAULT_CONTEXT_BEFORE);
        assert_eq!(cfg.context_after_default, DEFAULT_CONTEXT_AFTER);
        assert_eq!(
            cfg.conversation_events_default_limit,
            DEFAULT_CONVERSATION_EVENTS_LIMIT
        );
        assert_eq!(
            cfg.agent_trajectory_default_limit,
            DEFAULT_AGENT_TRAJECTORY_LIMIT
        );
        assert_eq!(cfg.timeline_query_limit(None), DEFAULT_TIMELINE_QUERY_LIMIT);
        assert_eq!(
            cfg.timeline_query_limit(Some(999)),
            MAX_TIMELINE_QUERY_LIMIT
        );
        assert_eq!(
            cfg.timeline_search_limit(None),
            DEFAULT_TIMELINE_SEARCH_LIMIT
        );
        assert_eq!(
            cfg.timeline_search_limit(Some(999)),
            MAX_TIMELINE_SEARCH_LIMIT
        );
        assert_eq!(cfg.intent_router_model, DEFAULT_INTENT_ROUTER_MODEL);
        assert_eq!(
            cfg.intent_router_timeout(),
            std::time::Duration::from_millis(DEFAULT_INTENT_ROUTER_TIMEOUT_MS)
        );
        assert_eq!(cfg.vision_codex_binary, DEFAULT_VISION_CODEX_BINARY);
        assert_eq!(cfg.vision_codex_model, DEFAULT_VISION_CODEX_MODEL);
        assert_eq!(
            cfg.vision_codex_idle_timeout(),
            std::time::Duration::from_secs(DEFAULT_VISION_CODEX_IDLE_TIMEOUT_SECS)
        );
        assert_eq!(
            cfg.vision_codex_absolute_timeout(),
            std::time::Duration::from_secs(DEFAULT_VISION_CODEX_ABSOLUTE_TIMEOUT_SECS)
        );
    }

    #[test]
    fn missing_conversation_ingestion_policy_is_rejected() {
        let err = parse_conversation_ingestion_policy("(missiond-blueprint)")
            .expect_err("missing conversation ingestion policy should fail");
        assert!(err.to_string().contains("conversation-ingestion-policy"));
    }

    #[test]
    fn compiled_runtime_snapshot_loads_generated_shape() {
        let temp = tempfile::tempdir().expect("tempdir");
        let compiled_dir = temp
            .path()
            .join(".missiond")
            .join("v3")
            .join("runtime")
            .join("compiled");
        fs::create_dir_all(&compiled_dir).expect("compiled dir");
        fs::write(
            compiled_dir.join("compiled-v3-blueprint.json"),
            r#"{
              "schema_version": "missiond.compiled-v3-blueprint.v1",
              "source_hash": "abc123",
              "generated_at": null,
              "diagnostics": [],
              "payload": {"blueprint": ".missiond/v3/missiond-blueprint.lisp"}
            }"#,
        )
        .expect("write compiled snapshot");

        let loaded = load_compiled_runtime_snapshot(temp.path(), "v3", Some("abc123"));
        assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
        let snapshot = loaded.snapshot.expect("snapshot");
        assert_eq!(snapshot.kind, "v3");
        assert_eq!(snapshot.schema_version, "missiond.compiled-v3-blueprint.v1");
        assert_eq!(snapshot.source_hash, "abc123");
    }

    #[test]
    fn compiled_runtime_snapshot_hash_mismatch_is_diagnostic_not_panic() {
        let temp = tempfile::tempdir().expect("tempdir");
        let compiled_dir = temp
            .path()
            .join(".missiond")
            .join("v3")
            .join("runtime")
            .join("compiled");
        fs::create_dir_all(&compiled_dir).expect("compiled dir");
        fs::write(
            compiled_dir.join("compiled-project-universe.json"),
            r#"{
              "schema_version": "missiond.compiled-project-universe.v1",
              "source_hash": "actual",
              "generated_at": null,
              "diagnostics": [],
              "payload": {"project_registry_present": true}
            }"#,
        )
        .expect("write compiled snapshot");

        let loaded = load_compiled_runtime_snapshot(temp.path(), "universe", Some("expected"));
        assert!(loaded.snapshot.is_none());
        assert!(
            loaded
                .diagnostics
                .iter()
                .any(|line| line.contains("source_hash mismatch")),
            "{:?}",
            loaded.diagnostics
        );
    }

    #[test]
    fn missing_compiled_runtime_snapshot_falls_back_with_diagnostic() {
        let temp = tempfile::tempdir().expect("tempdir");
        let loaded = load_compiled_runtime_snapshot(temp.path(), "workflows", None);
        assert!(loaded.snapshot.is_none());
        assert!(
            loaded
                .diagnostics
                .iter()
                .any(|line| line.contains("compiled runtime snapshot missing")),
            "{:?}",
            loaded.diagnostics
        );
    }

    #[test]
    fn compiled_project_universe_loads_structured_payload() {
        let temp = tempfile::tempdir().expect("tempdir");
        let compiled_dir = temp
            .path()
            .join(".missiond")
            .join("v3")
            .join("runtime")
            .join("compiled");
        fs::create_dir_all(&compiled_dir).expect("compiled dir");
        fs::write(
            compiled_dir.join("compiled-project-universe.json"),
            r#"{
              "schema_version": "missiond.compiled-project-universe.v1",
              "source_hash": "universe-hash",
              "generated_at": null,
              "diagnostics": [],
              "payload": {
                "projects": [{
                  "id": "auth",
                  "kind": "rust-service",
                  "root": "/repo/services/auth",
                  "path": null,
                  "intent": ".missiond/intent.lisp",
                  "backend": ".missiond/backend/auth-backend-blueprint.lisp",
                  "frontend": null,
                  "status": "v3-runtime-ssot",
                  "surface": "project-registry",
                  "checks": ["bash .missiond/check.sh"]
                }],
                "maturity": [{
                  "id": "auth",
                  "current": "M10",
                  "target": "M10",
                  "gap": []
                }]
              }
            }"#,
        )
        .expect("write compiled universe");

        let loaded = load_compiled_project_universe(temp.path(), Some("universe-hash"));
        assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
        let payload = loaded.payload.expect("payload");
        assert_eq!(payload.projects.len(), 1);
        assert_eq!(payload.projects[0].id.as_deref(), Some("auth"));
        assert_eq!(payload.projects[0].checks, vec!["bash .missiond/check.sh"]);
        assert_eq!(payload.maturity[0].current.as_deref(), Some("M10"));
    }

    #[test]
    fn compiled_workflow_contracts_load_structured_payload() {
        let temp = tempfile::tempdir().expect("tempdir");
        let compiled_dir = temp
            .path()
            .join(".missiond")
            .join("v3")
            .join("runtime")
            .join("compiled");
        fs::create_dir_all(&compiled_dir).expect("compiled dir");
        fs::write(
            compiled_dir.join("compiled-workflows.json"),
            r#"{
              "schema_version": "missiond.compiled-workflows.v1",
              "source_hash": "workflow-hash",
              "generated_at": null,
              "diagnostics": [],
              "payload": {
                "workflows": [{
                  "file": ".missiond/workflows/project-ssot-convergence.lisp",
                  "name": "project-ssot-convergence",
                  "workflow_id": "project-ssot-convergence",
                  "status": "active",
                  "owner": "resident-master-control",
                  "authority": "v3-project-blueprint-registry",
                  "source_plans": ["v3-runtime-ssot"],
                  "steps": ["s1", "s2"],
                  "risk_gate_count": 2,
                  "completion_criteria_count": 3
                }]
              }
            }"#,
        )
        .expect("write compiled workflows");

        let loaded = load_compiled_workflow_contracts(temp.path(), Some("workflow-hash"));
        assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
        let payload = loaded.payload.expect("payload");
        assert_eq!(payload.workflows.len(), 1);
        assert_eq!(
            payload.workflows[0].workflow_id.as_deref(),
            Some("project-ssot-convergence")
        );
        assert_eq!(payload.workflows[0].risk_gate_count, 2);
        assert_eq!(payload.workflows[0].completion_criteria_count, 3);
    }

    #[test]
    fn runtime_blueprint_source_prefers_compiled_v3_ast() {
        let temp = tempfile::tempdir().expect("tempdir");
        let compiled_dir = temp
            .path()
            .join(".missiond")
            .join("v3")
            .join("runtime")
            .join("compiled");
        fs::create_dir_all(&compiled_dir).expect("compiled dir");
        fs::write(
            temp.path()
                .join(".missiond")
                .join("v3")
                .join("missiond-blueprint.lisp"),
            r#"(missiond-blueprint (workstation-config (model-profile coding-default-opus-4-7 :spawn-model-arg nil)))"#,
        )
        .expect("fallback blueprint");
        fs::write(
            compiled_dir.join("compiled-v3-blueprint.json"),
            r#"{
              "schema_version": "missiond.compiled-v3-blueprint.v1",
              "source_hash": "compiled",
              "generated_at": null,
              "diagnostics": [],
              "payload": {
                "forms": [{
                  "type": "list",
                  "kind": "paren",
                  "children": [
                    {"type": "atom", "value": "missiond-blueprint"},
                    {"type": "list", "kind": "paren", "children": [
                      {"type": "atom", "value": "compiled-runtime-marker"}
                    ]}
                  ]
                }]
              }
            }"#,
        )
        .expect("compiled snapshot");

        let source =
            load_runtime_blueprint_source(Some(temp.path().to_string_lossy().as_ref()))
                .expect("runtime source")
                .expect("source");
        assert!(source.contains("compiled-runtime-marker"), "{source}");
    }

    #[test]
    fn runtime_blueprint_source_falls_back_when_compiled_ast_is_invalid() {
        let temp = tempfile::tempdir().expect("tempdir");
        let v3_dir = temp.path().join(".missiond").join("v3");
        fs::create_dir_all(v3_dir.join("runtime").join("compiled")).expect("compiled dir");
        fs::write(
            v3_dir.join("missiond-blueprint.lisp"),
            r#"(missiond-blueprint (fallback-runtime-marker))"#,
        )
        .expect("fallback blueprint");
        fs::write(
            v3_dir
                .join("runtime")
                .join("compiled")
                .join("compiled-v3-blueprint.json"),
            r#"{"schema_version":"missiond.compiled-v3-blueprint.v1","source_hash":"bad","diagnostics":[],"payload":{"forms":[]}}"#,
        )
        .expect("invalid compiled snapshot");

        let source =
            load_runtime_blueprint_source(Some(temp.path().to_string_lossy().as_ref()))
                .expect("runtime source")
                .expect("source");
        assert!(source.contains("fallback-runtime-marker"), "{source}");
    }
}
