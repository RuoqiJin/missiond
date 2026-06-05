//! WebSocket Server implementation
//!
//! Compatible with the Node implementation:
//! - PTY attach: `ws://host:port/pty/<slotId>`
//! - Tasks events: `ws://host:port/tasks`
//!
//! Messages (PTY):
//! - { type: "screen", data: string }
//! - { type: "data", data: string }
//! - { type: "state", state: string, prevState: string }
//! - { type: "exit", code: number }

use super::jarvis_trace::JarvisTraceStore;
use crate::cc_tasks::{
    CCMessageLine, CCSession, CCTask, CCTaskChangeEvent, CCTasksOverview, CCTasksWatcher,
    WatcherEvent,
};
use crate::event::events::SystemEvent;
use crate::pty::{PTYManager, PTYSpawnOptions, SessionEvent, SessionState, Slot as PTYSlot};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Mutex};
use tokio_tungstenite::tungstenite::handshake::server::{
    Request as WsRequest, Response as WsResponse,
};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::{accept_hdr_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

/// Result of context enrichment, includes assembled context and intent classification.
#[derive(Debug, Clone, Default)]
pub struct ContextEnrichResult {
    /// Pre-assembled context string for injection (empty = no context needed).
    pub assembled: String,
    /// Classified intent from context pipeline (e.g. "chat:rules", "code:router", "general:fallback", "async:deploy").
    pub intent: Option<String>,
}

/// Callback type for context enrichment before sending to PTY.
/// Implemented by daemon to inject KB/Skill/Code context into Jarvis messages.
/// Takes user query, returns enrichment result with context and intent.
pub type ContextEnricherFn = Arc<
    dyn Fn(
            String,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = ContextEnrichResult> + Send>>
        + Send
        + Sync,
>;

/// Late-bound container for context enricher (set after AppState is constructed).
pub type ContextEnricherSlot = Arc<tokio::sync::RwLock<Option<ContextEnricherFn>>>;

/// Persisted grounding context returned by the daemon-side context gatherer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JarvisGroundingResult {
    pub grounding_context_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_pack_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_pack_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grounding_report_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grounding_report_artifact_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grounding_report_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grounding_worker_slot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grounding_worker_turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_sufficiency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_capsule_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_capsule_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_label: Option<String>,
    #[serde(default)]
    pub sources_used: Vec<String>,
    #[serde(default)]
    pub diagnostics: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JarvisGroundingRequest {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmed_intent_artifact_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmed_intent_lisp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    pub chat_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_label: Option<String>,
    #[serde(default)]
    pub permission_context: serde_json::Value,
    #[serde(default)]
    pub media_context: serde_json::Value,
    #[serde(default)]
    pub unknowns: Vec<String>,
}

pub type JarvisGroundingFn = Arc<
    dyn Fn(
            JarvisGroundingRequest,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<JarvisGroundingResult, String>> + Send>,
        > + Send
        + Sync,
>;

pub type JarvisGroundingSlot = Arc<tokio::sync::RwLock<Option<JarvisGroundingFn>>>;

/// Shared-artifact result for Jarvis intent/plan lifecycle artifacts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JarvisArtifactResult {
    pub artifact_id: String,
    pub artifact_hash: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JarvisArtifactRequest {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub payload: serde_json::Value,
    pub metadata: serde_json::Value,
}

pub type JarvisArtifactFn = Arc<
    dyn Fn(
            JarvisArtifactRequest,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<JarvisArtifactResult, String>> + Send>,
        > + Send
        + Sync,
>;

pub type JarvisArtifactSlot = Arc<tokio::sync::RwLock<Option<JarvisArtifactFn>>>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProviderBoxHttpRequest {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProviderBoxHttpResponse {
    pub status: u16,
    #[serde(default = "default_provider_box_content_type")]
    pub content_type: String,
    #[serde(default)]
    pub body: serde_json::Value,
}

fn default_provider_box_content_type() -> String {
    "application/json".to_string()
}

pub type ProviderBoxHttpFn = Arc<
    dyn Fn(
            ProviderBoxHttpRequest,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<ProviderBoxHttpResponse, String>> + Send>,
        > + Send
        + Sync,
>;

pub type ProviderBoxHttpSlot = Arc<tokio::sync::RwLock<Option<ProviderBoxHttpFn>>>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct InteractionEnvelope {
    #[serde(default = "default_interaction_channel")]
    channel: String,
    #[serde(default)]
    external_user_id: Option<String>,
    #[serde(default)]
    auth_token: Option<String>,
    #[serde(default)]
    conversation_id: Option<String>,
    #[serde(default)]
    message: serde_json::Value,
    #[serde(default)]
    attachments: Vec<serde_json::Value>,
    #[serde(default)]
    metadata: serde_json::Value,
}

fn default_interaction_channel() -> String {
    "web".to_string()
}

/// V3-projected Codex CLI authoring lane for Jarvis intent.lisp drafts.
#[derive(Debug, Clone)]
pub struct JarvisIntentAuthorConfig {
    pub slot_id: String,
    pub model: String,
    pub reasoning_effort: String,
    pub search_enabled: bool,
    pub sandbox: String,
    pub approval_policy: String,
    pub timeout_secs: u64,
}

impl Default for JarvisIntentAuthorConfig {
    fn default() -> Self {
        Self {
            slot_id: "slot-codex-intent-author".to_string(),
            model: "gpt-5.5".to_string(),
            reasoning_effort: "xhigh".to_string(),
            search_enabled: true,
            sandbox: "read-only".to_string(),
            approval_policy: "never".to_string(),
            timeout_secs: 180,
        }
    }
}

#[derive(Debug, Clone)]
pub struct JarvisPlanAuthorConfig {
    pub slot_id: String,
    pub model: String,
    pub reasoning_effort: String,
    pub search_enabled: bool,
    pub sandbox: String,
    pub approval_policy: String,
    pub timeout_secs: u64,
}

impl Default for JarvisPlanAuthorConfig {
    fn default() -> Self {
        Self {
            slot_id: "slot-codex-plan-author".to_string(),
            model: "gpt-5.5".to_string(),
            reasoning_effort: "xhigh".to_string(),
            search_enabled: true,
            sandbox: "read-only".to_string(),
            approval_policy: "never".to_string(),
            timeout_secs: 180,
        }
    }
}

#[derive(Debug, Clone)]
pub struct JarvisKeyJudgmentAuthorConfig {
    pub slot_id: String,
    pub model: String,
    pub reasoning_effort: String,
    pub search_enabled: bool,
    pub sandbox: String,
    pub approval_policy: String,
    pub timeout_secs: u64,
}

impl Default for JarvisKeyJudgmentAuthorConfig {
    fn default() -> Self {
        Self {
            slot_id: "slot-codex-key-judgment-author".to_string(),
            model: "gpt-5.5".to_string(),
            reasoning_effort: "xhigh".to_string(),
            search_enabled: true,
            sandbox: "read-only".to_string(),
            approval_policy: "never".to_string(),
            timeout_secs: 180,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct JarvisCodexIntentResponse {
    recognized_objective: String,
    intent_kind: String,
    understanding: String,
    review_text: String,
    #[serde(default)]
    assumptions: Vec<String>,
    #[serde(default)]
    non_goals: Vec<String>,
    #[serde(default)]
    acceptance_signals: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_jarvis_confidence")]
    confidence: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct JarvisCodexKeyJudgmentResponse {
    judgment: String,
    review_text: String,
    #[serde(default, deserialize_with = "deserialize_jarvis_confidence")]
    confidence: Option<String>,
    #[serde(default)]
    rejected_hypotheses: Vec<String>,
    #[serde(default)]
    evidence_refs: Vec<String>,
    #[serde(default)]
    planning_implications: Vec<String>,
    #[serde(default)]
    acceptance_focus: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct JarvisCodexPlanResponse {
    objective: String,
    review_text: String,
    execution_mode: String,
    requires_board_task: bool,
    steps: Vec<String>,
    #[serde(default)]
    direct_answer_draft: Option<String>,
    #[serde(default)]
    key_judgment: Option<String>,
    #[serde(default)]
    answer_policy: Option<String>,
    #[serde(default)]
    provider_hint: Option<String>,
    #[serde(default)]
    boundary: Option<String>,
    #[serde(default)]
    assumptions: Vec<String>,
    #[serde(default)]
    non_goals: Vec<String>,
    #[serde(default)]
    acceptance_signals: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_jarvis_confidence")]
    confidence: Option<String>,
    #[serde(default)]
    workstreams: Vec<serde_json::Value>,
    #[serde(default)]
    atom_tasks: Vec<serde_json::Value>,
    #[serde(default)]
    dependency_edges: Vec<serde_json::Value>,
    #[serde(default)]
    serial_groups: Vec<serde_json::Value>,
    #[serde(default)]
    parallel_groups: Vec<serde_json::Value>,
    #[serde(default)]
    assignment_policy: serde_json::Value,
}

fn deserialize_jarvis_confidence<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        serde_json::Value::String(text) => normalize_jarvis_confidence_text(&text),
        serde_json::Value::Number(number) => number.as_f64().map(normalize_jarvis_confidence_score),
        serde_json::Value::Bool(true) => Some("high".to_string()),
        serde_json::Value::Bool(false) => Some("low".to_string()),
        _ => None,
    }))
}

fn normalize_jarvis_confidence_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    match lower.as_str() {
        "high" | "medium" | "low" => Some(lower),
        _ => trimmed
            .parse::<f64>()
            .ok()
            .map(normalize_jarvis_confidence_score)
            .or_else(|| Some(trimmed.to_string())),
    }
}

fn normalize_jarvis_confidence_score(score: f64) -> String {
    if score >= 0.75 {
        "high".to_string()
    } else if score >= 0.4 {
        "medium".to_string()
    } else {
        "low".to_string()
    }
}

#[derive(Debug, Clone)]
struct JarvisAuthoredIntentDraft {
    objective: String,
    intent_kind: String,
    understanding: String,
    review_text: String,
    artifact_body: String,
    assumptions: Vec<String>,
    non_goals: Vec<String>,
    acceptance_signals: Vec<String>,
    confidence: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct JarvisKeyJudgmentArtifactRef {
    artifact_id: String,
    artifact_hash: Option<String>,
    artifact_path: Option<String>,
    judgment: String,
    review_text: Option<String>,
    confidence: Option<String>,
    rejected_hypotheses: Vec<String>,
    evidence_refs: Vec<String>,
    planning_implications: Vec<String>,
    acceptance_focus: Vec<String>,
}

#[derive(Debug, Clone)]
struct JarvisAuthoredKeyJudgmentDraft {
    judgment: String,
    review_text: String,
    artifact_body: String,
    confidence: Option<String>,
    rejected_hypotheses: Vec<String>,
    evidence_refs: Vec<String>,
    planning_implications: Vec<String>,
    acceptance_focus: Vec<String>,
}

#[derive(Debug, Clone)]
struct JarvisAuthoredPlanDraft {
    objective: String,
    review_text: String,
    execution_mode: String,
    requires_board_task: bool,
    artifact_body: String,
    steps: Vec<String>,
    direct_answer_draft: Option<String>,
    answer_policy: Option<String>,
    provider_hint: Option<String>,
    boundary: Option<String>,
    assumptions: Vec<String>,
    non_goals: Vec<String>,
    acceptance_signals: Vec<String>,
    confidence: Option<String>,
    key_judgment: Option<String>,
    atomization_graph: serde_json::Value,
    workstreams: Vec<serde_json::Value>,
    atom_tasks: Vec<serde_json::Value>,
    dependency_edges: Vec<serde_json::Value>,
    serial_groups: Vec<serde_json::Value>,
    parallel_groups: Vec<serde_json::Value>,
    assignment_policy: serde_json::Value,
}

#[derive(Debug, Clone)]
struct JarvisPlanAtomTask {
    atom_task_id: String,
    workstream_id: Option<String>,
    objective: String,
    category: String,
    assignee_engine: String,
    execution_order: String,
    depends_on: Vec<String>,
    parallel_group: Option<String>,
    read_scope: Vec<String>,
    write_scope: Vec<String>,
    acceptance: Vec<String>,
    raw: serde_json::Value,
}

#[derive(Debug, Clone)]
struct JarvisCreatedAtomBoardTask {
    atom_task_id: String,
    task: crate::types::BoardTask,
    category: String,
    assignee_engine: String,
    depends_on_atoms: Vec<String>,
    parallel_group: Option<String>,
    synthetic: bool,
}

#[derive(Debug, Clone)]
struct JarvisCreatedBoardTasks {
    parent_task: crate::types::BoardTask,
    atom_tasks: Vec<JarvisCreatedAtomBoardTask>,
    final_task_id: String,
}

#[derive(Clone, Default)]
struct JarvisProgressBus {
    system_event_tx: Option<tokio::sync::mpsc::Sender<SystemEvent>>,
    frontend_events_tx: Option<broadcast::Sender<String>>,
}

struct JarvisObservedVersion {
    version: Option<String>,
    source: Option<String>,
    diagnostic: Option<String>,
}

#[derive(Debug, Clone)]
struct JarvisProviderSlotMonitorSpec {
    phase: &'static str,
    role: &'static str,
    provider: String,
    engine: String,
    slot_id: String,
    model: Option<String>,
    model_profile: Option<String>,
    residency: &'static str,
    required_ready: bool,
    critical: bool,
}

/// WebSocket server options
pub struct WSServerOptions {
    /// Server port
    pub port: u16,
    /// PTY manager (optional, for PTY attach)
    pub pty_manager: Option<Arc<PTYManager>>,
    /// CC Tasks watcher (optional, for tasks events)
    pub cc_tasks_watcher: Option<Arc<Mutex<CCTasksWatcher>>>,
    /// Screenshot broker (optional, for browser-based PTY screenshots)
    pub screenshot_broker: Option<Arc<super::ScreenshotBroker>>,
    /// AIOps incident event bus sender (optional, for webhook endpoints)
    pub incident_tx: Option<tokio::sync::mpsc::Sender<crate::types::MissionIncident>>,
    /// External service events to publish into MissionD's SystemEvent bus.
    pub system_event_tx: Option<tokio::sync::mpsc::Sender<SystemEvent>>,
    /// Frontend event stream (pre-serialized JSON from daemon EventBus)
    pub frontend_events_tx: Option<broadcast::Sender<String>>,
    /// Database store for Jarvis chat + timeline queries (M4: trait-based)
    pub db: Option<Arc<dyn crate::db::traits::MissionStore>>,
    /// Context enricher for Jarvis chat completions (late-bound by daemon)
    pub context_enricher: ContextEnricherSlot,
    /// Grounded-dispatch context gatherer for Jarvis intent/plan gate.
    pub jarvis_grounding: JarvisGroundingSlot,
    /// Shared artifact writer for Jarvis intent/plan gate.
    pub jarvis_artifact_writer: JarvisArtifactSlot,
    /// Provider-box HTTP adapter, late-bound by daemon after provider drivers are assembled.
    pub provider_box_http: ProviderBoxHttpSlot,
    /// Number of native MCP tools (injected into Jarvis system prompt)
    pub tool_count: usize,
    /// V3-projected default slot for OpenAI-compatible chat completions.
    pub default_chat_slot: String,
    /// V3-projected Codex CLI authoring lane for intent.lisp.
    pub jarvis_intent_author: JarvisIntentAuthorConfig,
    /// V3-projected Codex CLI authoring lane for interaction-key-judgment.
    pub jarvis_key_judgment_author: JarvisKeyJudgmentAuthorConfig,
    /// V3-projected Codex CLI authoring lane for plan.lisp.
    pub jarvis_plan_author: JarvisPlanAuthorConfig,
}

/// PTY WebSocket Server
pub struct PTYWebSocketServer {
    port: u16,
    pty_manager: Option<Arc<PTYManager>>,
    cc_tasks_watcher: Option<Arc<Mutex<CCTasksWatcher>>>,
    screenshot_broker: Option<Arc<super::ScreenshotBroker>>,
    shutdown_tx: Option<broadcast::Sender<()>>,
    jarvis_trace: JarvisTraceStore,
    incident_tx: Option<tokio::sync::mpsc::Sender<crate::types::MissionIncident>>,
    system_event_tx: Option<tokio::sync::mpsc::Sender<SystemEvent>>,
    frontend_events_tx: Option<broadcast::Sender<String>>,
    db: Option<Arc<dyn crate::db::traits::MissionStore>>,
    context_enricher: ContextEnricherSlot,
    jarvis_grounding: JarvisGroundingSlot,
    jarvis_artifact_writer: JarvisArtifactSlot,
    provider_box_http: ProviderBoxHttpSlot,
    tool_count: usize,
    default_chat_slot: String,
    jarvis_intent_author: JarvisIntentAuthorConfig,
    jarvis_key_judgment_author: JarvisKeyJudgmentAuthorConfig,
    jarvis_plan_author: JarvisPlanAuthorConfig,
}

/// Jarvis system prompt — injected before context enrichment so Claude Code
/// knows it's running as Jarvis behind a Web Chat UI, not as a local terminal.
/// Tool count is dynamically injected at runtime via `jarvis_system_prompt()`.
#[allow(dead_code)]
fn jarvis_system_prompt(tool_count: usize) -> String {
    format!(
        "<system_info>\n\
[Identity] 你是 Jarvis，由 MissionD 多实例编排系统驱动的 AI 助手。\n\
[Environment] 你通过 MissionD 的 Web Chat UI 与用户交互，而非本地终端。用户在浏览器中与你对话。\n\
[Persistence] 用户的每条消息和你的回复已由系统自动持久化到数据库。不要告诉用户「对话是独立的」或「需要写入记忆文件才能保存」。\n\
[Capabilities] 你拥有完整的 MCP 工具集（MissionD {} 个 + xjp-mcp 平台工具）。当你不确定该用哪个工具时，务必先阅读你的操作手册：~/.claude/skills/jarvis-manual/SKILL.md\n\
[Style] 像一个智能 Web 助手一样自然对话，不要向用户暴露底层终端或 PTY 细节。不要使用 AskUserQuestion 工具向用户提问——直接在回复文本中提问即可。\n\
[CRITICAL] 禁止运行 pgrep、ps、kill 等进程诊断命令。MCP 工具由系统管理，你不需要检查进程状态。直接调用 MCP 工具即可，不要验证服务是否运行。\n\
[CRITICAL] 不要递归扫描 /Users、整个 home、Downloads 或全盘来寻找项目。优先使用 MissionD MCP 的 project/infra/skill evidence；如果目标仓库不在当前机器，必须快速失败并说明缺口。\n\
[CRITICAL] 对微信文章、长图、公众号排版等需要特定后台能力的任务，目标后台/API/项目能力不可用时必须快速失败并说明缺口；禁止伪造 fallback 草稿或假装已调用成功。\n\
</system_info>",
        tool_count
    )
}

// ── AIOps Webhook Parsers ──

#[allow(dead_code)]
fn jarvis_sync_timeout_ms() -> u64 {
    const DEFAULT_TIMEOUT_MS: u64 = 15 * 60 * 1000;
    const MIN_TIMEOUT_MS: u64 = 30 * 1000;
    const MAX_TIMEOUT_MS: u64 = 30 * 60 * 1000;

    std::env::var("MISSIOND_JARVIS_SYNC_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS)
}

#[allow(dead_code)]
fn jarvis_idle_without_final_grace_ms() -> u64 {
    const DEFAULT_GRACE_MS: u64 = 30 * 1000;
    const MIN_GRACE_MS: u64 = 10 * 1000;
    const MAX_GRACE_MS: u64 = 10 * 60 * 1000;

    std::env::var("MISSIOND_JARVIS_IDLE_WITHOUT_FINAL_GRACE_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_GRACE_MS)
        .clamp(MIN_GRACE_MS, MAX_GRACE_MS)
}

#[allow(dead_code)]
fn jarvis_final_settle_ms() -> u64 {
    const DEFAULT_SETTLE_MS: u64 = 30 * 1000;
    const MIN_SETTLE_MS: u64 = 500;
    const MAX_SETTLE_MS: u64 = 2 * 60 * 1000;

    std::env::var("MISSIOND_JARVIS_FINAL_SETTLE_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_SETTLE_MS)
        .clamp(MIN_SETTLE_MS, MAX_SETTLE_MS)
}

fn clamp_jarvis_task_wait_secs(value: Option<u64>) -> u64 {
    const DEFAULT_WAIT_SECS: u64 = 180;
    const MIN_WAIT_SECS: u64 = 15;
    const MAX_WAIT_SECS: u64 = 300;

    value
        .unwrap_or(DEFAULT_WAIT_SECS)
        .clamp(MIN_WAIT_SECS, MAX_WAIT_SECS)
}

fn jarvis_task_wait_secs() -> u64 {
    clamp_jarvis_task_wait_secs(
        std::env::var("MISSIOND_JARVIS_TASK_WAIT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok()),
    )
}

fn clamp_jarvis_public_stream_budget_secs(value: Option<u64>) -> u64 {
    const DEFAULT_BUDGET_SECS: u64 = 6;
    const MIN_BUDGET_SECS: u64 = 2;
    const MAX_BUDGET_SECS: u64 = 240;

    value
        .unwrap_or(DEFAULT_BUDGET_SECS)
        .clamp(MIN_BUDGET_SECS, MAX_BUDGET_SECS)
}

fn jarvis_public_stream_budget_secs() -> u64 {
    clamp_jarvis_public_stream_budget_secs(
        std::env::var("MISSIOND_JARVIS_PUBLIC_STREAM_BUDGET_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok()),
    )
}

fn clamp_jarvis_db_poll_timeout_secs(value: Option<u64>) -> u64 {
    const DEFAULT_TIMEOUT_SECS: u64 = 8;
    const MIN_TIMEOUT_SECS: u64 = 2;
    const MAX_TIMEOUT_SECS: u64 = 30;

    value
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
}

fn jarvis_db_poll_timeout_secs() -> u64 {
    clamp_jarvis_db_poll_timeout_secs(
        std::env::var("MISSIOND_JARVIS_DB_POLL_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok()),
    )
}

fn clamp_jarvis_visible_heartbeat_secs(value: Option<u64>) -> u64 {
    const DEFAULT_HEARTBEAT_SECS: u64 = 10;
    const MIN_HEARTBEAT_SECS: u64 = 3;
    const MAX_HEARTBEAT_SECS: u64 = 30;

    value
        .unwrap_or(DEFAULT_HEARTBEAT_SECS)
        .clamp(MIN_HEARTBEAT_SECS, MAX_HEARTBEAT_SECS)
}

fn jarvis_visible_heartbeat_secs() -> u64 {
    clamp_jarvis_visible_heartbeat_secs(
        std::env::var("MISSIOND_JARVIS_VISIBLE_HEARTBEAT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok()),
    )
}

fn jarvis_slot_auto_heal_enabled() -> bool {
    matches!(
        std::env::var("MISSIOND_JARVIS_SLOT_AUTO_HEAL")
            .unwrap_or_else(|_| "0".to_string())
            .as_str(),
        "1" | "true" | "TRUE" | "yes" | "on"
    )
}

fn jarvis_slot_auto_heal_timeout_secs() -> u64 {
    const DEFAULT_TIMEOUT_SECS: u64 = 45;
    const MIN_TIMEOUT_SECS: u64 = 5;
    const MAX_TIMEOUT_SECS: u64 = 180;

    std::env::var("MISSIOND_JARVIS_SLOT_AUTO_HEAL_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
}

fn missiond_env_flag_enabled(key: &str, default_value: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| {
            let value = value.trim();
            matches!(value, "1" | "true" | "TRUE" | "yes" | "on")
                || (!matches!(value, "0" | "false" | "FALSE" | "no" | "off") && default_value)
        })
        .unwrap_or(default_value)
}

fn jarvis_intent_plan_confirmation_required() -> bool {
    if std::env::var("MISSIOND_JARVIS_REQUIRE_CONFIRMATION").is_ok() {
        return missiond_env_flag_enabled("MISSIOND_JARVIS_REQUIRE_CONFIRMATION", false);
    }
    missiond_env_flag_enabled("MISSIOND_JARVIS_INTENT_PLAN_CONFIRMATION", false)
}

fn jarvis_artifact_projection_openai_delta_enabled() -> bool {
    missiond_env_flag_enabled("MISSIOND_JARVIS_ARTIFACT_PROJECTION_OPENAI_DELTA", false)
}

#[allow(dead_code)]
fn jarvis_confirm_bool(req: &serde_json::Value, key: &str) -> bool {
    fn bool_at<'a>(value: &'a serde_json::Value, key: &str) -> Option<bool> {
        value.get(key).and_then(|field| field.as_bool())
    }

    bool_at(req, key)
        .or_else(|| {
            req.get("missiond_confirm")
                .and_then(|confirm| bool_at(confirm, key))
        })
        .or_else(|| {
            req.get("missiond_confirm")
                .and_then(|confirm| confirm.get("confirm_payload"))
                .and_then(|payload| bool_at(payload, key))
        })
        .unwrap_or(false)
}

#[allow(dead_code)]
fn jarvis_confirm_string(req: &serde_json::Value, key: &str) -> Option<String> {
    fn string_at(value: &serde_json::Value, key: &str) -> Option<String> {
        value
            .get(key)
            .and_then(|field| field.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    }

    string_at(req, key)
        .or_else(|| {
            req.get("missiond_confirm")
                .and_then(|confirm| string_at(confirm, key))
        })
        .or_else(|| {
            req.get("missiond_confirm")
                .and_then(|confirm| confirm.get("confirm_payload"))
                .and_then(|payload| string_at(payload, key))
        })
}

fn openai_request_follow_task_id(req: &serde_json::Value) -> Option<String> {
    fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
        value
            .get(key)
            .and_then(|field| field.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    }

    fn follow_id_from(value: &serde_json::Value) -> Option<String> {
        string_field(value, "missiond_follow_task_id").or_else(|| {
            value.get("missiond_follow").and_then(|follow| {
                string_field(follow, "missiond_follow_task_id")
                    .or_else(|| string_field(follow, "task_id"))
            })
        })
    }

    follow_id_from(req).or_else(|| req.get("metadata").and_then(follow_id_from))
}

fn interaction_metadata_bool(envelope: &InteractionEnvelope, key: &str) -> bool {
    envelope
        .metadata
        .get(key)
        .and_then(|field| field.as_bool())
        .or_else(|| {
            envelope
                .metadata
                .get("missiond_confirm")
                .and_then(|confirm| confirm.get(key))
                .and_then(|field| field.as_bool())
        })
        .or_else(|| {
            envelope
                .metadata
                .get("missiond_confirm")
                .and_then(|confirm| confirm.get("confirm_payload"))
                .and_then(|payload| payload.get(key))
                .and_then(|field| field.as_bool())
        })
        .unwrap_or(false)
}

fn interaction_metadata_string(envelope: &InteractionEnvelope, key: &str) -> Option<String> {
    fn string_at(value: &serde_json::Value, key: &str) -> Option<String> {
        value
            .get(key)
            .and_then(|field| field.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    }

    string_at(&envelope.metadata, key)
        .or_else(|| {
            envelope
                .metadata
                .get("missiond_confirm")
                .and_then(|confirm| string_at(confirm, key))
        })
        .or_else(|| {
            envelope
                .metadata
                .get("missiond_confirm")
                .and_then(|confirm| confirm.get("confirm_payload"))
                .and_then(|payload| string_at(payload, key))
        })
}

fn json_string_vec_at(value: &serde_json::Value, key: &str) -> Option<Vec<String>> {
    value
        .get(key)
        .and_then(|field| field.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
}

fn interaction_metadata_string_vec(envelope: &InteractionEnvelope, key: &str) -> Vec<String> {
    json_string_vec_at(&envelope.metadata, key)
        .or_else(|| {
            envelope
                .metadata
                .get("missiond_confirm")
                .and_then(|confirm| json_string_vec_at(confirm, key))
        })
        .or_else(|| {
            envelope
                .metadata
                .get("missiond_confirm")
                .and_then(|confirm| confirm.get("confirm_payload"))
                .and_then(|payload| json_string_vec_at(payload, key))
        })
        .unwrap_or_default()
}

#[allow(dead_code)]
fn jarvis_confirm_string_vec(req: &serde_json::Value, key: &str) -> Vec<String> {
    json_string_vec_at(req, key)
        .or_else(|| {
            req.get("missiond_confirm")
                .and_then(|confirm| json_string_vec_at(confirm, key))
        })
        .or_else(|| {
            req.get("missiond_confirm")
                .and_then(|confirm| confirm.get("confirm_payload"))
                .and_then(|payload| json_string_vec_at(payload, key))
        })
        .unwrap_or_default()
}

fn extract_bearer_token(headers: &str) -> Option<String> {
    headers.lines().find_map(|line| {
        if line.to_ascii_lowercase().starts_with("authorization:") {
            let val = line.split_once(':')?.1.trim();
            val.strip_prefix("Bearer ")
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(ToOwned::to_owned)
        } else {
            None
        }
    })
}

fn normalize_interaction_message(message: &serde_json::Value) -> String {
    match message {
        serde_json::Value::String(text) => text.trim().to_string(),
        serde_json::Value::Object(map) => map
            .get("text")
            .or_else(|| map.get("content"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn interaction_short_hash(input: &str) -> String {
    Sha256::digest(input.as_bytes())
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn json_nested_string_field(value: &serde_json::Value, paths: &[&[&str]]) -> Option<String> {
    paths.iter().find_map(|path| {
        let mut cursor = value;
        for segment in *path {
            cursor = cursor.get(*segment)?;
        }
        cursor
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn attachment_url_from_value(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return Some(text.to_string());
    }
    json_nested_string_field(
        value,
        &[
            &["image_url", "url"],
            &["imageUrl"],
            &["image_url"],
            &["url"],
            &["uri"],
            &["signed_url", "url"],
            &["media_artifact", "signed_url", "url"],
            &["media_artifact", "url"],
            &["media_artifact", "uri"],
        ],
    )
}

fn redacted_attachment_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.to_ascii_lowercase().starts_with("data:") {
        return "data:<redacted>".to_string();
    }
    let without_fragment = trimmed
        .split_once('#')
        .map(|(base, _)| base)
        .unwrap_or(trimmed);
    match without_fragment.split_once('?') {
        Some((base, _)) if !base.is_empty() => format!("{base}?<redacted>"),
        _ => without_fragment.to_string(),
    }
}

fn xjp_image_ref_from_attachment(value: &serde_json::Value, url: Option<&str>) -> Option<String> {
    let explicit = json_nested_string_field(
        value,
        &[
            &["image_service_ref"],
            &["xjp_image_ref"],
            &["media_ref"],
            &["media_artifact", "image_service_ref"],
            &["media_artifact", "uri"],
        ],
    );
    if let Some(reference) = explicit {
        if reference.starts_with("xjp-image://") || reference.starts_with("media-artifact://") {
            return Some(reference);
        }
        return Some(format!("xjp-image://{}", reference.trim_matches('/')));
    }

    if let Some(id) = json_nested_string_field(
        value,
        &[
            &["xjp_image_id"],
            &["image_id"],
            &["media_id"],
            &["artifact_id"],
            &["media_artifact", "id"],
            &["media_artifact", "artifact_id"],
        ],
    ) {
        return Some(format!("xjp-image://images/{}", id.trim_matches('/')));
    }

    let Some(url) = url.map(str::trim).filter(|url| !url.is_empty()) else {
        return None;
    };
    if url.starts_with("xjp-image://") || url.starts_with("media-artifact://") {
        return Some(url.to_string());
    }
    let redacted = redacted_attachment_url(url);
    let lower = redacted.to_ascii_lowercase();
    if lower.contains("xjp-image")
        || lower.contains("xiaojin")
        || lower.contains("/v1/images/")
        || lower.contains("/images/")
    {
        return Some(redacted);
    }
    None
}

fn attachment_is_image_like(value: &serde_json::Value, url: Option<&str>) -> bool {
    if let Some(kind) = json_nested_string_field(
        value,
        &[
            &["kind"],
            &["type"],
            &["media_kind"],
            &["media_artifact", "kind"],
            &["media_artifact", "type"],
        ],
    ) {
        let lower = kind.to_ascii_lowercase();
        if lower.contains("image") || lower == "media_artifact" || lower == "imageurl" {
            return true;
        }
    }
    if let Some(media_type) = json_nested_string_field(
        value,
        &[
            &["media_type"],
            &["mime_type"],
            &["content_type"],
            &["media_artifact", "media_type"],
            &["media_artifact", "mime_type"],
        ],
    ) {
        if media_type.to_ascii_lowercase().starts_with("image/") {
            return true;
        }
    }
    if value.get("image_url").is_some()
        || value.get("imageUrl").is_some()
        || value.get("xjp_image_id").is_some()
        || value.get("media_artifact").is_some()
    {
        return true;
    }
    url.map(|url| {
        let lower = url.to_ascii_lowercase();
        lower.starts_with("data:image/")
            || lower.contains("/images/")
            || lower.ends_with(".png")
            || lower.ends_with(".jpg")
            || lower.ends_with(".jpeg")
            || lower.ends_with(".webp")
            || lower.ends_with(".gif")
    })
    .unwrap_or(false)
}

fn normalize_interaction_attachment_ref(
    value: &serde_json::Value,
    source: &str,
    index: usize,
) -> Option<serde_json::Value> {
    let url = attachment_url_from_value(value);
    if !attachment_is_image_like(value, url.as_deref()) {
        return None;
    }
    let data_url_rejected = url
        .as_deref()
        .map(|url| url.trim().to_ascii_lowercase().starts_with("data:"))
        .unwrap_or(false);
    let image_service_ref = if data_url_rejected {
        None
    } else {
        xjp_image_ref_from_attachment(value, url.as_deref())
    };
    let media_type = json_nested_string_field(
        value,
        &[
            &["media_type"],
            &["mime_type"],
            &["content_type"],
            &["media_artifact", "media_type"],
            &["media_artifact", "mime_type"],
        ],
    )
    .or_else(|| {
        url.as_deref()
            .filter(|url| url.to_ascii_lowercase().starts_with("data:image/"))
            .and_then(|url| url.split_once(';').map(|(prefix, _)| prefix))
            .and_then(|prefix| prefix.strip_prefix("data:"))
            .map(ToOwned::to_owned)
    });
    let artifact_id = json_nested_string_field(
        value,
        &[
            &["artifact_id"],
            &["media_artifact", "artifact_id"],
            &["media_artifact", "id"],
            &["image_id"],
            &["xjp_image_id"],
        ],
    );
    let id_seed = artifact_id
        .as_deref()
        .or(image_service_ref.as_deref())
        .or(url.as_deref())
        .unwrap_or(source);
    let attachment_id = json_nested_string_field(value, &[&["attachment_id"], &["id"]])
        .unwrap_or_else(|| format!("media-{}", interaction_short_hash(id_seed)));
    let status = if data_url_rejected {
        "rejected"
    } else if image_service_ref.is_some() {
        "accepted"
    } else {
        "accepted_remote_reference"
    };
    let transport = if data_url_rejected {
        "inline-data-url-rejected"
    } else if image_service_ref.is_some() {
        "xjp-image-service-ref"
    } else {
        "remote-url-ref"
    };
    let signed_url_present = json_nested_string_field(
        value,
        &[
            &["signed_url", "url"],
            &["signed_url"],
            &["media_artifact", "signed_url", "url"],
            &["media_artifact", "signed_url"],
        ],
    )
    .is_some()
        || url
            .as_deref()
            .map(|url| {
                let lower = url.to_ascii_lowercase();
                lower.contains("signature=")
                    || lower.contains("sig=")
                    || lower.contains("token=")
                    || lower.contains("expires=")
            })
            .unwrap_or(false);
    let mut attachment = serde_json::json!({
        "schema": "missiond.interaction-media-attachment.v1",
        "kind": "image",
        "attachment_id": attachment_id,
        "source": source,
        "index": index,
        "status": status,
        "transport": transport,
        "binary_owner": "xjp-image-service",
        "missiond_transport": "reference-only",
        "media_type": media_type,
        "image_service_ref": image_service_ref,
        "artifact_id": artifact_id,
        "artifact_hash": json_nested_string_field(value, &[&["artifact_hash"], &["hash"], &["media_artifact", "artifact_hash"]]),
        "sha256": json_nested_string_field(value, &[&["sha256"], &["digest"], &["media_artifact", "sha256"]]),
        "size_bytes": value.get("size_bytes").or_else(|| value.get("size")).or_else(|| value.pointer("/media_artifact/size_bytes")).cloned(),
        "width": value.get("width").or_else(|| value.pointer("/metadata/width")).or_else(|| value.pointer("/media_artifact/width")).cloned(),
        "height": value.get("height").or_else(|| value.pointer("/metadata/height")).or_else(|| value.pointer("/media_artifact/height")).cloned(),
        "detail": json_nested_string_field(value, &[&["detail"], &["image_url", "detail"]]),
        "source_url_redacted": url.as_deref().map(redacted_attachment_url),
        "signed_url_present": signed_url_present,
    });
    if data_url_rejected {
        if let Some(object) = attachment.as_object_mut() {
            object.insert(
                "diagnostics".to_string(),
                serde_json::json!([{
                    "code": "IMAGE_INLINE_DATA_URL_REQUIRES_XJP_UPLOAD",
                    "message": "Upload image bytes to xjp-image-service first; MissionD accepts reference-only media attachments."
                }]),
            );
        }
    }
    Some(crate::evidence_redactor::redact_json_value(&attachment))
}

fn normalize_interaction_attachments(
    attachments: &[serde_json::Value],
    source: &str,
) -> Vec<serde_json::Value> {
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();
    for (index, attachment) in attachments.iter().enumerate() {
        let Some(value) = normalize_interaction_attachment_ref(attachment, source, index) else {
            continue;
        };
        let key = value
            .get("attachment_id")
            .and_then(|field| field.as_str())
            .unwrap_or("")
            .to_string();
        if !key.is_empty() && !seen.insert(key) {
            continue;
        }
        normalized.push(value);
    }
    normalized
}

fn interaction_accepted_attachment_count(attachments: &[serde_json::Value]) -> usize {
    attachments
        .iter()
        .filter(|attachment| {
            attachment.get("status").and_then(|value| value.as_str()) != Some("rejected")
        })
        .count()
}

fn interaction_media_context(attachments: &[serde_json::Value]) -> serde_json::Value {
    let accepted_count = interaction_accepted_attachment_count(attachments);
    serde_json::json!({
        "schema": "missiond.interaction-media-context.v1",
        "binary_owner": "xjp-image-service",
        "missiond_transport": "reference-only",
        "attachment_count": attachments.len(),
        "accepted_attachment_count": accepted_count,
        "rejected_attachment_count": attachments.len().saturating_sub(accepted_count),
        "attachment_refs": attachments,
        "rules": [
            "iOS uploads image bytes to xjp-image-service before calling MissionD",
            "MissionD stores and replays image references, not base64 payloads",
            "signed URLs are redacted from public events and artifacts"
        ]
    })
}

fn interaction_media_summary_for_objective(attachments: &[serde_json::Value]) -> Option<String> {
    let accepted = interaction_accepted_attachment_count(attachments);
    if accepted == 0 {
        return None;
    }
    let refs = attachments
        .iter()
        .filter(|attachment| {
            attachment.get("status").and_then(|value| value.as_str()) != Some("rejected")
        })
        .filter_map(|attachment| {
            attachment
                .get("image_service_ref")
                .and_then(|value| value.as_str())
                .or_else(|| {
                    attachment
                        .get("artifact_id")
                        .and_then(|value| value.as_str())
                })
                .or_else(|| {
                    attachment
                        .get("attachment_id")
                        .and_then(|value| value.as_str())
                })
        })
        .take(5)
        .collect::<Vec<_>>()
        .join(", ");
    Some(if refs.is_empty() {
        format!("用户上传了 {accepted} 张图片，图片二进制由 xjp-image-service 托管。")
    } else {
        format!("用户上传了 {accepted} 张图片，图片引用：{refs}。")
    })
}

fn openai_request_latest_user_message(req: &serde_json::Value) -> Option<&serde_json::Value> {
    req.get("messages")
        .and_then(|messages| messages.as_array())
        .and_then(|messages| {
            messages.iter().rev().find(|message| {
                message.get("role").and_then(|value| value.as_str()) == Some("user")
            })
        })
}

fn openai_content_to_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(text) => text.trim().to_string(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .or_else(|| part.get("content"))
                    .and_then(|value| value.as_str())
            })
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::Value::Object(map) => map
            .get("text")
            .or_else(|| map.get("content"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn openai_request_user_message(req: &serde_json::Value) -> String {
    openai_request_latest_user_message(req)
        .and_then(|message| message.get("content"))
        .map(openai_content_to_text)
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| {
            req.get("prompt")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .unwrap_or_default()
                .to_string()
        })
}

fn openai_request_attachments(req: &serde_json::Value) -> Vec<serde_json::Value> {
    let mut raw = Vec::new();
    if let Some(attachments) = req.get("attachments").and_then(|value| value.as_array()) {
        raw.extend(attachments.iter().cloned());
    }
    if let Some(parts) = openai_request_latest_user_message(req)
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_array())
    {
        raw.extend(parts.iter().filter_map(|part| {
            let kind = part
                .get("type")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if kind.contains("image")
                || part.get("image_url").is_some()
                || part.get("imageUrl").is_some()
                || part.get("media_artifact").is_some()
            {
                Some(part.clone())
            } else {
                None
            }
        }));
    }
    normalize_interaction_attachments(&raw, "openai-chat-completions")
}

fn openai_request_to_interaction_envelope(req: &serde_json::Value) -> InteractionEnvelope {
    let mut metadata = req
        .get("metadata")
        .cloned()
        .filter(|value| value.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(object) = metadata.as_object_mut() {
        for key in [
            "missiond_confirm",
            "missiond_objective",
            "missiond_follow_task_id",
            "missiond_intent_confirmed",
            "missiond_plan_confirmed",
            "interaction_id",
            "tenant_id",
            "application_id",
            "product_id",
        ] {
            if let Some(value) = req.get(key) {
                object
                    .entry(key.to_string())
                    .or_insert_with(|| value.clone());
            }
        }
        if let Some(follow_task_id) = openai_request_follow_task_id(req) {
            object
                .entry("missiond_follow_task_id".to_string())
                .or_insert_with(|| serde_json::Value::String(follow_task_id));
        }
        object.insert(
            "wire_format".to_string(),
            serde_json::json!("openai-chat-completions"),
        );
    }

    InteractionEnvelope {
        channel: req
            .get("channel")
            .and_then(|value| value.as_str())
            .unwrap_or("jarvis")
            .to_string(),
        external_user_id: req
            .get("user")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned),
        auth_token: req
            .get("auth_token")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned),
        conversation_id: req
            .get("conversation_id")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned),
        message: serde_json::Value::String(openai_request_user_message(req)),
        attachments: openai_request_attachments(req),
        metadata,
    }
}

#[derive(Debug, Clone)]
struct InteractionAuthResolution {
    token: Option<String>,
    permission_context: serde_json::Value,
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn missiond_production_env() -> bool {
    matches!(
        std::env::var("XJP_ENV")
            .or_else(|_| std::env::var("APP_ENV"))
            .or_else(|_| std::env::var("RUST_ENV"))
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "prod" | "production"
    )
}

fn secret_store_strict_mode_enabled() -> bool {
    std::env::var("SECRET_STORE_STRICT")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or_else(missiond_production_env)
}

fn missiond_unconfigured_api_token_allowed() -> bool {
    env_flag("MISSIOND_ALLOW_UNCONFIGURED_API_TOKEN") && !missiond_production_env()
}

fn missiond_service_token_configured() -> bool {
    ["MISSIOND_INTERACTION_SERVICE_TOKEN", "MISSIOND_API_TOKEN"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .any(|value| !value.trim().is_empty())
}

fn interaction_token(envelope: &InteractionEnvelope, headers: &str) -> Option<String> {
    envelope
        .auth_token
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| extract_bearer_token(headers))
}

fn json_string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(|field| field.as_str())
            .map(str::trim)
            .filter(|field| !field.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn normalized_scope_value(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| !matches!(*value, "unknown" | "null" | "undefined"))
        .map(ToOwned::to_owned)
}

fn json_scope_string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value.get(*key).and_then(|field| match field {
            serde_json::Value::String(text) => normalized_scope_value(Some(text)),
            serde_json::Value::Null => None,
            other => normalized_scope_value(Some(&other.to_string())),
        })
    })
}

#[derive(Debug, Clone, Default)]
struct ConversationSessionScope {
    user_id: Option<String>,
    tenant_id: Option<String>,
    application_id: Option<String>,
    channel: String,
    topic_id: Option<String>,
    topic_label: Option<String>,
}

fn compact_topic_label(text: &str) -> Option<String> {
    let collapsed = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if collapsed.is_empty() {
        return None;
    }
    let mut label = collapsed.chars().take(96).collect::<String>();
    if collapsed.chars().count() > 96 {
        label.push_str("...");
    }
    Some(label)
}

fn stable_topic_id(
    user_id: Option<&str>,
    tenant_id: Option<&str>,
    application_id: Option<&str>,
    channel: &str,
    topic_label: Option<&str>,
) -> Option<String> {
    let topic_label = topic_label
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let normalized_topic = topic_label.to_ascii_lowercase();
    let input = format!(
        "{}|{}|{}|{}|{}",
        tenant_id.unwrap_or(""),
        user_id.unwrap_or(""),
        application_id.unwrap_or("missiond"),
        channel,
        normalized_topic
    );
    let digest = Sha256::digest(input.as_bytes());
    let short = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Some(format!("topic-{short}"))
}

fn conversation_scope_from_permission(
    envelope: &InteractionEnvelope,
    permission_context: &serde_json::Value,
    channel: &str,
    user_text: &str,
) -> ConversationSessionScope {
    let topic_label = compact_topic_label(user_text);
    let user_id = json_scope_string_field(permission_context, &["user_id"])
        .or_else(|| normalized_scope_value(envelope.external_user_id.as_deref()));
    let tenant_id = json_scope_string_field(permission_context, &["tenant_id"])
        .or_else(|| json_scope_string_field(&envelope.metadata, &["tenant_id"]));
    let application_id = json_scope_string_field(permission_context, &["application_id"])
        .or_else(|| json_scope_string_field(&envelope.metadata, &["application_id"]))
        .or_else(|| Some("missiond".to_string()));
    let topic_id = stable_topic_id(
        user_id.as_deref(),
        tenant_id.as_deref(),
        application_id.as_deref(),
        channel,
        topic_label.as_deref(),
    );
    ConversationSessionScope {
        user_id,
        tenant_id,
        application_id,
        channel: channel.to_string(),
        topic_id,
        topic_label,
    }
}

fn conversation_scope_from_request(
    req: &serde_json::Value,
    default_channel: &str,
    user_text: &str,
) -> ConversationSessionScope {
    let channel =
        json_scope_string_field(req, &["channel"]).unwrap_or_else(|| default_channel.to_string());
    let user_id = json_scope_string_field(req, &["user", "user_id", "userId", "external_user_id"]);
    let tenant_id = json_scope_string_field(req, &["tenant_id", "tenantId"]);
    let application_id = json_scope_string_field(req, &["application_id", "applicationId"])
        .or_else(|| Some("missiond".to_string()));
    let topic_label = compact_topic_label(user_text);
    let topic_id = stable_topic_id(
        user_id.as_deref(),
        tenant_id.as_deref(),
        application_id.as_deref(),
        &channel,
        topic_label.as_deref(),
    );
    ConversationSessionScope {
        user_id,
        tenant_id,
        application_id,
        channel,
        topic_id,
        topic_label,
    }
}

fn json_string_array_field(value: &serde_json::Value, keys: &[&str]) -> Vec<String> {
    for key in keys {
        if let Some(field) = value.get(*key) {
            if let Some(items) = field.as_array() {
                return items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
            }
            if let Some(text) = field.as_str() {
                return text
                    .split(|ch: char| ch == ',' || ch.is_whitespace())
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
            }
        }
    }
    Vec::new()
}

fn interaction_service_token_matches(token: &str) -> bool {
    ["MISSIOND_INTERACTION_SERVICE_TOKEN", "MISSIOND_API_TOKEN"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|value| value.trim().to_string())
        .any(|expected| !expected.is_empty() && expected == token)
}

fn validate_missiond_legacy_chat_bearer(
    token: Option<&str>,
) -> Result<(), (u16, &'static str, serde_json::Value)> {
    let Some(token) = token.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err((
            401,
            "Unauthorized",
            serde_json::json!({
                "error": {
                    "code": "MISSIOND_AUTH_REQUIRED",
                    "message": "Missing Authorization header"
                }
            }),
        ));
    };

    if interaction_service_token_matches(token) {
        return Ok(());
    }

    if missiond_service_token_configured() {
        return Err((
            401,
            "Unauthorized",
            serde_json::json!({
                "error": {
                    "code": "MISSIOND_AUTH_INVALID",
                    "message": "Invalid MissionD service token"
                }
            }),
        ));
    }

    if missiond_unconfigured_api_token_allowed() {
        return Ok(());
    }

    Err((
        503,
        "Service Unavailable",
        serde_json::json!({
            "error": {
                "code": "MISSIOND_SERVICE_TOKEN_UNCONFIGURED",
                "message": "MissionD service token is not configured; refusing arbitrary bearer tokens"
            }
        }),
    ))
}

fn interaction_default_capabilities(
    channel: &str,
    roles: &[String],
    scopes: &[String],
) -> Vec<String> {
    let mut capabilities = match channel {
        "service" => vec!["interaction:exact_workflow".to_string()],
        "wechat" => vec![
            "interaction:chat".to_string(),
            "identity:binding_required".to_string(),
        ],
        _ => vec![
            "interaction:chat".to_string(),
            "interaction:intent_plan".to_string(),
        ],
    };

    let elevated = roles.iter().any(|role| {
        matches!(
            role.as_str(),
            "admin" | "system_admin" | "tenant_admin" | "missiond_operator"
        )
    }) || scopes.iter().any(|scope| {
        matches!(
            scope.as_str(),
            "missiond.admin" | "missiond.operator" | "workflow:execute"
        )
    });
    if elevated {
        capabilities.extend([
            "board:create".to_string(),
            "worker:dispatch".to_string(),
            "interaction:exact_workflow".to_string(),
        ]);
    }

    capabilities.sort();
    capabilities.dedup();
    capabilities
}

fn interaction_permission_context_from_userinfo(
    envelope: &InteractionEnvelope,
    userinfo: &serde_json::Value,
    auth_endpoint: &str,
) -> serde_json::Value {
    let channel = envelope.channel.trim().to_ascii_lowercase();
    let roles = json_string_array_field(userinfo, &["roles", "role", "groups", "product_groups"]);
    let scopes = json_string_array_field(userinfo, &["scope", "scopes"]);
    let capabilities = interaction_default_capabilities(&channel, &roles, &scopes);
    serde_json::json!({
        "schema": "missiond.permission-context.v1",
        "authority": "auth",
        "resolution": "auth-userinfo",
        "auth_endpoint": auth_endpoint,
        "user_id": json_string_field(userinfo, &["sub", "user_id"]).unwrap_or_else(|| {
            envelope
                .external_user_id
                .as_deref()
                .unwrap_or("unknown")
                .to_string()
        }),
        "tenant_id": json_string_field(userinfo, &["tenant_id"]).unwrap_or_else(|| {
            envelope.metadata.get("tenant_id").and_then(|v| v.as_str()).unwrap_or("unknown").to_string()
        }),
        "tenant_slug": json_string_field(userinfo, &["tenant_slug"]),
        "application_id": json_string_field(userinfo, &["application_id", "aud"]).unwrap_or_else(|| {
            envelope.metadata.get("application_id").and_then(|v| v.as_str()).unwrap_or("missiond").to_string()
        }),
        "product_id": json_string_field(userinfo, &["product_id"]).or_else(|| {
            envelope.metadata.get("product_id").and_then(|v| v.as_str()).map(ToOwned::to_owned)
        }),
        "groups": json_string_array_field(userinfo, &["product_groups", "groups"]),
        "roles": if roles.is_empty() { vec!["user".to_string()] } else { roles },
        "scope": scopes,
        "channel": channel,
        "capabilities": capabilities,
        "subject": {
            "email": json_string_field(userinfo, &["email"]),
            "email_verified": userinfo.get("email_verified").and_then(|v| v.as_bool()),
            "name": json_string_field(userinfo, &["name"]),
        }
    })
}

fn verify_interaction_auth(
    envelope: &InteractionEnvelope,
    headers: &str,
) -> Result<Option<String>, (u16, &'static str, serde_json::Value)> {
    let channel = envelope.channel.trim().to_ascii_lowercase();
    let token = interaction_token(envelope, headers);

    if channel == "wechat" && token.is_none() {
        return Ok(None);
    }

    let Some(token) = token else {
        return Err((
            401,
            "Unauthorized",
            serde_json::json!({
                "error": {
                    "code": "INTERACTION_AUTH_REQUIRED",
                    "message": "Interaction channel requires an Auth bearer token or service token."
                }
            }),
        ));
    };

    Ok(Some(token))
}

fn interaction_binding_required_context(envelope: &InteractionEnvelope) -> serde_json::Value {
    let channel = envelope.channel.trim().to_ascii_lowercase();
    serde_json::json!({
        "schema": "missiond.permission-context.v1",
        "authority": "auth",
        "resolution": "identity-binding-required",
        "user_id": envelope.external_user_id,
        "tenant_id": serde_json::Value::Null,
        "application_id": "missiond",
        "product_id": serde_json::Value::Null,
        "groups": [],
        "roles": [],
        "channel": channel,
        "capabilities": interaction_default_capabilities(&channel, &[], &[]),
    })
}

fn interaction_envelope_user_id(envelope: &InteractionEnvelope, fallback: &str) -> String {
    normalized_scope_value(envelope.external_user_id.as_deref())
        .or_else(|| {
            json_scope_string_field(
                &envelope.metadata,
                &["user_id", "userId", "external_user_id", "externalUserId"],
            )
        })
        .unwrap_or_else(|| fallback.to_string())
}

fn interaction_service_permission_context(envelope: &InteractionEnvelope) -> serde_json::Value {
    let channel = envelope.channel.trim().to_ascii_lowercase();
    serde_json::json!({
        "schema": "missiond.permission-context.v1",
        "authority": "auth",
        "resolution": "service-token",
        "user_id": interaction_envelope_user_id(envelope, "missiond-service"),
        "tenant_id": envelope.metadata.get("tenant_id").and_then(|v| v.as_str()).unwrap_or("system"),
        "application_id": envelope.metadata.get("application_id").and_then(|v| v.as_str()).unwrap_or("missiond"),
        "product_id": envelope.metadata.get("product_id").and_then(|v| v.as_str()),
        "groups": ["service"],
        "roles": ["service"],
        "channel": channel,
        "capabilities": interaction_default_capabilities("service", &["service".to_string()], &["workflow:execute".to_string()]),
    })
}

fn interaction_dev_permission_context(envelope: &InteractionEnvelope) -> serde_json::Value {
    let channel = envelope.channel.trim().to_ascii_lowercase();
    serde_json::json!({
        "schema": "missiond.permission-context.v1",
        "authority": "auth",
        "resolution": "dev-skip-auth-userinfo",
        "user_id": interaction_envelope_user_id(envelope, "dev-user"),
        "tenant_id": envelope.metadata.get("tenant_id").and_then(|v| v.as_str()).unwrap_or("dev"),
        "application_id": envelope.metadata.get("application_id").and_then(|v| v.as_str()).unwrap_or("missiond"),
        "product_id": envelope.metadata.get("product_id").and_then(|v| v.as_str()),
        "groups": envelope.metadata.get("groups").cloned().unwrap_or_else(|| serde_json::json!([])),
        "roles": envelope.metadata.get("roles").cloned().unwrap_or_else(|| serde_json::json!(["user"])),
        "channel": channel,
        "capabilities": interaction_default_capabilities(&channel, &[], &[]),
    })
}

async fn resolve_interaction_auth(
    envelope: &InteractionEnvelope,
    headers: &str,
) -> Result<InteractionAuthResolution, (u16, &'static str, serde_json::Value)> {
    let token = verify_interaction_auth(envelope, headers)?;
    let Some(token_value) = token.clone() else {
        return Ok(InteractionAuthResolution {
            token,
            permission_context: interaction_binding_required_context(envelope),
        });
    };

    if interaction_service_token_matches(&token_value) {
        return Ok(InteractionAuthResolution {
            token,
            permission_context: interaction_service_permission_context(envelope),
        });
    }

    if env_flag("MISSIOND_INTERACTION_AUTH_SKIP_INTROSPECTION") {
        return Ok(InteractionAuthResolution {
            token,
            permission_context: interaction_dev_permission_context(envelope),
        });
    }

    let auth_endpoint = std::env::var("MISSIOND_INTERACTION_AUTH_USERINFO_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "https://auth.xiaojinpro.com/oidc/userinfo".to_string());
    let timeout_ms = std::env::var("MISSIOND_INTERACTION_AUTH_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(2500)
        .clamp(500, 8000);
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return Err((
                503,
                "Service Unavailable",
                serde_json::json!({
                    "error": {
                        "code": "INTERACTION_AUTH_UNAVAILABLE",
                        "message": format!("Failed to initialize Auth client: {}", error)
                    }
                }),
            ));
        }
    };
    let response = match client
        .get(&auth_endpoint)
        .bearer_auth(&token_value)
        .header("Accept", "application/json")
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return Err((
                503,
                "Service Unavailable",
                serde_json::json!({
                    "error": {
                        "code": "INTERACTION_AUTH_UNAVAILABLE",
                        "message": format!("Auth userinfo endpoint is unavailable: {}", error),
                        "auth_endpoint": auth_endpoint
                    }
                }),
            ));
        }
    };

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err((
            status.as_u16(),
            if status == reqwest::StatusCode::FORBIDDEN {
                "Forbidden"
            } else {
                "Unauthorized"
            },
            serde_json::json!({
                "error": {
                    "code": "INTERACTION_AUTH_INVALID",
                    "message": "Auth rejected the interaction bearer token.",
                    "auth_endpoint": auth_endpoint
                }
            }),
        ));
    }
    if !status.is_success() {
        return Err((
            503,
            "Service Unavailable",
            serde_json::json!({
                "error": {
                    "code": "INTERACTION_AUTH_UNAVAILABLE",
                    "message": format!("Auth userinfo endpoint returned {}", status),
                    "auth_endpoint": auth_endpoint
                }
            }),
        ));
    }

    let userinfo = match response.json::<serde_json::Value>().await {
        Ok(value) => value,
        Err(error) => {
            return Err((
                503,
                "Service Unavailable",
                serde_json::json!({
                    "error": {
                        "code": "INTERACTION_AUTH_UNAVAILABLE",
                        "message": format!("Auth userinfo response was not valid JSON: {}", error),
                        "auth_endpoint": auth_endpoint
                    }
                }),
            ));
        }
    };

    Ok(InteractionAuthResolution {
        token,
        permission_context: interaction_permission_context_from_userinfo(
            envelope,
            &userinfo,
            &auth_endpoint,
        ),
    })
}

/// Parse Deploy Center failure webhook into an incident.
/// Returns None for non-failure events (e.g. deploy success).
fn parse_deploy_webhook(body: &str) -> Option<crate::types::MissionIncident> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let event = v["event"].as_str()?;

    if event != "deploy_failed" {
        return None;
    }

    let project = v["project"].as_str().unwrap_or("unknown");
    let error_msg = v["error_message"].as_str().unwrap_or("无详情");

    Some(crate::types::MissionIncident {
        id: format!("inc-{}", uuid::Uuid::new_v4()),
        severity: crate::types::IncidentSeverity::High,
        source: crate::types::IncidentSource::DeployCenter,
        title: format!("部署失败: {}", project),
        description: format!(
            "项目 {} 部署失败。\n错误: {}\n\n建议操作：\n1. 检查构建日志\n2. 检查 Deploy Agent 状态\n3. 检查 GHCR 镜像是否推送成功",
            project, error_msg,
        ),
        server_id: None,
        raw_payload: v,
        created_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Parse test webhook — always produces a Warning incident for pipeline validation.
fn parse_test_webhook(body: &str) -> Option<crate::types::MissionIncident> {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::json!({}));
    let title = v["title"].as_str().unwrap_or("Webhook test incident");
    let severity_str = v["severity"].as_str().unwrap_or("warning");

    let severity = match severity_str {
        "critical" => crate::types::IncidentSeverity::Critical,
        "high" => crate::types::IncidentSeverity::High,
        _ => crate::types::IncidentSeverity::Warning,
    };

    Some(crate::types::MissionIncident {
        id: format!("inc-{}", uuid::Uuid::new_v4()),
        severity,
        source: crate::types::IncidentSource::Manual,
        title: title.to_string(),
        description: format!("Webhook test: {}", title),
        server_id: v["server_id"].as_str().map(|s| s.to_string()),
        raw_payload: v,
        created_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Parse an external service domain event into the MissionD system bus.
///
/// Minimal legacy envelope:
/// `{ service_id, event_id, event_kind, summary, trace_id?, payload? }`.
/// New producers should send `missiond.event-envelope.v1`, whose identity
/// fields are preserved under `payload._envelope` so waiters can filter by
/// project/correlation without widening `SystemEvent` every time a cloud
/// service adds a field.
fn parse_external_service_webhook(
    body: &str,
    default_service_id: &str,
    require_event_id: bool,
) -> Option<SystemEvent> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let service_id = v
        .get("service_id")
        .or_else(|| v.get("serviceId"))
        .and_then(|v| v.as_str())
        .unwrap_or(default_service_id)
        .to_string();
    let event_kind = v
        .get("event_kind")
        .or_else(|| v.get("eventKind"))
        .or_else(|| v.get("kind"))
        .and_then(|v| v.as_str())
        .unwrap_or("external_event")
        .to_string();
    let event_id = v
        .get("event_id")
        .or_else(|| v.get("eventId"))
        .or_else(|| v.get("id"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    if require_event_id && event_id.is_none() {
        return None;
    }
    let event_id = event_id.unwrap_or_else(|| format!("external-{}", uuid::Uuid::new_v4()));
    let summary = v
        .get("summary")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{} reported {}", service_id, event_kind));
    let trace_id = v
        .get("trace_id")
        .or_else(|| v.get("traceId"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let mut payload = match v.get("payload").cloned() {
        Some(serde_json::Value::Object(map)) => serde_json::Value::Object(map),
        Some(other) => serde_json::json!({ "value": other }),
        None => serde_json::json!({}),
    };
    let envelope = serde_json::json!({
        "schema_version": json_string(&v, &["schema_version", "schemaVersion"]).unwrap_or_else(|| "missiond.event-envelope.v1".to_string()),
        "event_id": event_id.clone(),
        "source": json_string(&v, &["source"]),
        "project_id": json_string(&v, &["project_id", "projectId"]),
        "service_id": service_id.clone(),
        "event_kind": event_kind.clone(),
        "subject": json_string(&v, &["subject"]),
        "correlation_id": json_string(&v, &["correlation_id", "correlationId"]),
        "trace_id": trace_id.clone(),
        "occurred_at": json_string(&v, &["occurred_at", "occurredAt"]),
        "observed_at": json_string(&v, &["observed_at", "observedAt"]),
        "authority": json_string(&v, &["authority"]),
        "privacy_class": json_string(&v, &["privacy_class", "privacyClass"]),
    });
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("_envelope".to_string(), envelope);
    }

    Some(SystemEvent::ExternalServiceEvent {
        service_id,
        event_id,
        event_kind,
        summary,
        trace_id,
        payload_json: payload.to_string(),
    })
}

fn json_string(v: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| v.get(*key).and_then(|value| value.as_str()))
        .map(str::to_string)
}

fn header_value<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.trim().eq_ignore_ascii_case(name) {
            Some(value.trim())
        } else {
            None
        }
    })
}

fn webhook_token_matches(headers: &str, expected_token: Option<&str>) -> bool {
    let Some(expected) = expected_token.filter(|token| !token.is_empty()) else {
        return true;
    };
    header_value(headers, "X-MissionD-Webhook-Token")
        .map(|actual| actual == expected)
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route<'a> {
    Pty { slot_id: &'a str },
    Tasks,
    Events,
    Invalid,
}

fn normalize_public_jarvis_path(path: &str) -> std::borrow::Cow<'_, str> {
    if path == "/jarvis" {
        return std::borrow::Cow::Borrowed("/");
    }
    if let Some(stripped) = path.strip_prefix("/jarvis/") {
        return std::borrow::Cow::Owned(format!("/{stripped}"));
    }
    std::borrow::Cow::Borrowed(path)
}

fn parse_route(path: &str) -> Route<'_> {
    if path == "/tasks" {
        return Route::Tasks;
    }
    if path == "/events" {
        return Route::Events;
    }
    if let Some(slot_id) = path.strip_prefix("/pty/") {
        if !slot_id.is_empty() && !slot_id.contains('/') {
            return Route::Pty { slot_id };
        }
    }
    Route::Invalid
}

fn close_frame(code: u16, reason: impl Into<String>) -> CloseFrame<'static> {
    CloseFrame {
        code: CloseCode::from(code),
        reason: reason.into().into(),
    }
}

fn parse_http_headers(headers: &str) -> HashMap<String, String> {
    headers
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect()
}

fn http_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "OK",
    }
}

async fn send_json<S: Serialize>(
    ws_tx: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<TcpStream>,
        Message,
    >,
    msg: &S,
) -> anyhow::Result<()> {
    let text = serde_json::to_string(msg)?;
    ws_tx.send(Message::Text(text)).await?;
    Ok(())
}

/// Messages from PTY to client
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum PtyOutMessage {
    Screen {
        data: String,
    },
    Data {
        data: String,
    },
    State {
        state: SessionState,
        #[serde(rename = "prevState")]
        prev_state: SessionState,
        #[serde(rename = "statusText", skip_serializing_if = "Option::is_none")]
        status_text: Option<String>,
    },
    Exit {
        code: i32,
    },
    #[serde(rename = "screenshot_request")]
    ScreenshotRequest {
        #[serde(rename = "requestId")]
        request_id: String,
    },
}

/// Messages from client to PTY
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum PtyInMessage {
    Input {
        data: String,
    },
    #[serde(rename = "screenshot_response")]
    ScreenshotResponse {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(default)]
        data: Option<String>,
        #[serde(default)]
        width: Option<u32>,
        #[serde(default)]
        height: Option<u32>,
        #[serde(default)]
        error: Option<String>,
    },
}

/// CC Tasks event messages
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TasksEventMessage {
    CcTasksSnapshot { sessions: Vec<CCSession> },
    CcTasksOverview { payload: CCTasksOverview },
    CcTasksChanged { payload: CCTaskChangeEvent },
    CcTaskStarted { payload: TaskEventPayload },
    CcTaskCompleted { payload: TaskEventPayload },
    CcSessionActive { payload: SessionEventPayload },
    CcSessionInactive { payload: SessionEventPayload },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TasksInMessage {
    GetTasks,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskEventPayload {
    session_id: String,
    project_name: String,
    task: CCTask,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionEventPayload {
    session_id: String,
    project_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
}

impl PTYWebSocketServer {
    /// Create a new WebSocket server
    pub fn new(options: WSServerOptions) -> Self {
        Self {
            port: options.port,
            pty_manager: options.pty_manager,
            cc_tasks_watcher: options.cc_tasks_watcher,
            screenshot_broker: options.screenshot_broker,
            shutdown_tx: None,
            jarvis_trace: JarvisTraceStore::new(),
            incident_tx: options.incident_tx,
            system_event_tx: options.system_event_tx,
            frontend_events_tx: options.frontend_events_tx,
            db: options.db,
            context_enricher: Arc::clone(&options.context_enricher),
            jarvis_grounding: Arc::clone(&options.jarvis_grounding),
            jarvis_artifact_writer: Arc::clone(&options.jarvis_artifact_writer),
            provider_box_http: Arc::clone(&options.provider_box_http),
            tool_count: options.tool_count,
            default_chat_slot: options.default_chat_slot,
            jarvis_intent_author: options.jarvis_intent_author,
            jarvis_key_judgment_author: options.jarvis_key_judgment_author,
            jarvis_plan_author: options.jarvis_plan_author,
        }
    }

    /// Get a reference to the Jarvis trace store (for MCP tools)
    pub fn jarvis_trace_store(&self) -> &JarvisTraceStore {
        &self.jarvis_trace
    }

    /// Get the context enricher slot for late-binding by daemon.
    pub fn context_enricher_slot(&self) -> &ContextEnricherSlot {
        &self.context_enricher
    }

    /// Get the Jarvis grounding slot for late-binding by daemon.
    pub fn jarvis_grounding_slot(&self) -> &JarvisGroundingSlot {
        &self.jarvis_grounding
    }

    /// Get the Jarvis artifact writer slot for late-binding by daemon.
    pub fn jarvis_artifact_writer_slot(&self) -> &JarvisArtifactSlot {
        &self.jarvis_artifact_writer
    }

    /// Get the provider-box HTTP adapter slot for late-binding by daemon.
    pub fn provider_box_http_slot(&self) -> &ProviderBoxHttpSlot {
        &self.provider_box_http
    }

    /// Start the server
    pub async fn start(&mut self) -> anyhow::Result<()> {
        let bind_addr = std::env::var("MISSION_WS_BIND").unwrap_or_else(|_| "0.0.0.0".to_string());
        let addr = format!("{}:{}", bind_addr, self.port);

        // Mirror IPC's stale-socket pattern: if bind fails with EADDRINUSE,
        // probe the port — if a live WS server responds, bail (real conflict);
        // if it's a stale listener from a dead process, wait for OS to reclaim.
        let listener = match TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                // Probe: can we connect? If yes, another daemon is genuinely alive.
                match tokio::net::TcpStream::connect(&format!("127.0.0.1:{}", self.port)).await {
                    Ok(_) => {
                        anyhow::bail!(
                            "Another instance is already serving WebSocket on port {}",
                            self.port
                        );
                    }
                    Err(_) => {
                        // Stale listener — OS hasn't reclaimed yet. Wait briefly and retry.
                        warn!(
                            port = self.port,
                            "WS port in TIME_WAIT, waiting for OS to release..."
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        TcpListener::bind(&addr).await?
                    }
                }
            }
            Err(e) => return Err(e.into()),
        };

        info!(port = self.port, bind = %bind_addr, "PTY WebSocket server started");

        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx.clone());

        let pty_manager = self.pty_manager.clone();
        let cc_tasks_watcher = self.cc_tasks_watcher.clone();
        let screenshot_broker = self.screenshot_broker.clone();
        let jarvis_trace = self.jarvis_trace.clone();
        let incident_tx = self.incident_tx.clone();
        let system_event_tx = self.system_event_tx.clone();
        let frontend_events_tx = self.frontend_events_tx.clone();
        let db = self.db.clone();
        let context_enricher = self.context_enricher.clone();
        let jarvis_grounding = self.jarvis_grounding.clone();
        let jarvis_artifact_writer = self.jarvis_artifact_writer.clone();
        let provider_box_http = self.provider_box_http.clone();
        let tool_count = self.tool_count;
        let default_chat_slot = self.default_chat_slot.clone();
        let jarvis_intent_author = self.jarvis_intent_author.clone();
        let jarvis_key_judgment_author = self.jarvis_key_judgment_author.clone();
        let jarvis_plan_author = self.jarvis_plan_author.clone();

        tokio::spawn(async move {
            let mut shutdown_rx = shutdown_tx.subscribe();
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, addr)) => {
                                let pty_manager = pty_manager.clone();
                                let cc_tasks_watcher = cc_tasks_watcher.clone();
                                let screenshot_broker = screenshot_broker.clone();
                                let jarvis_trace = jarvis_trace.clone();
                                let incident_tx = incident_tx.clone();
                                let system_event_tx = system_event_tx.clone();
                                let frontend_events_tx = frontend_events_tx.clone();
                                let db = db.clone();
                                let context_enricher = context_enricher.clone();
                                let jarvis_grounding = jarvis_grounding.clone();
                                let jarvis_artifact_writer = jarvis_artifact_writer.clone();
                                let provider_box_http = provider_box_http.clone();
                                let tool_count = tool_count;
                                let default_chat_slot = default_chat_slot.clone();
                                let jarvis_intent_author = jarvis_intent_author.clone();
                                let jarvis_key_judgment_author = jarvis_key_judgment_author.clone();
                                let jarvis_plan_author = jarvis_plan_author.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = Self::handle_connection(stream, addr, pty_manager, cc_tasks_watcher, screenshot_broker, jarvis_trace, incident_tx, system_event_tx, frontend_events_tx, db, context_enricher, jarvis_grounding, jarvis_artifact_writer, provider_box_http, tool_count, default_chat_slot, jarvis_intent_author, jarvis_key_judgment_author, jarvis_plan_author).await {
                                        error!(?e, ?addr, "WebSocket connection error");
                                    }
                                });
                            }
                            Err(e) => {
                                error!(?e, "Failed to accept connection");
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("WebSocket server shutting down");
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Stop the server
    pub async fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        info!("PTY WebSocket server stopped");
    }

    /// Handle HTTP health check (non-WebSocket request)
    async fn handle_health(mut stream: TcpStream) -> anyhow::Result<()> {
        let body = r#"{"status":"ok"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await?;
        Ok(())
    }

    /// HTTP API — compiled MissionD project universe.
    ///
    /// This endpoint is read-only and serves the compiled ABI projection. Hot
    /// callers must consume this JSON instead of parsing V3 Lisp directly.
    async fn handle_project_universe(mut stream: TcpStream) -> anyhow::Result<()> {
        let mut buf = vec![0u8; 4096];
        let _ = stream.read(&mut buf).await;

        let compiled_runtime_dir = Self::compiled_runtime_dir();
        let path = compiled_runtime_dir.join("compiled-project-universe.json");
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) => {
                let body = serde_json::json!({
                    "schema": "missiond.project-universe-api-error.v1",
                    "status": "unavailable",
                    "kind": "compiled_project_universe_unavailable",
                    "path": path,
                    "reason": err.to_string(),
                    "recovery": "Run node scripts/project-v3-contracts.mjs --write and node scripts/compile-v3-runtime.mjs --write, then restart MissionD."
                });
                return Self::send_http_error(
                    &mut stream,
                    503,
                    "Service Unavailable",
                    &body.to_string(),
                )
                .await;
            }
        };

        let parsed: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(parsed) => parsed,
            Err(err) => {
                let body = serde_json::json!({
                    "schema": "missiond.project-universe-api-error.v1",
                    "status": "unavailable",
                    "kind": "compiled_project_universe_invalid_json",
                    "path": path,
                    "reason": err.to_string(),
                    "recovery": "Regenerate compiled V3 runtime artifacts and restart MissionD."
                });
                return Self::send_http_error(
                    &mut stream,
                    503,
                    "Service Unavailable",
                    &body.to_string(),
                )
                .await;
            }
        };

        let actual_schema = parsed
            .get("schema_version")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let actual_source_hash = parsed
            .get("source_hash")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let diagnostics_count = parsed
            .get("diagnostics")
            .and_then(|value| value.as_array())
            .map(|rows| rows.len())
            .unwrap_or(0);
        if actual_schema != "missiond.compiled-project-universe.v1"
            || actual_source_hash != crate::v3_contracts::SOURCE_HASH
            || diagnostics_count > 0
        {
            let body = serde_json::json!({
                "schema": "missiond.project-universe-api-error.v1",
                "status": "unavailable",
                "kind": "compiled_project_universe_stale",
                "path": path,
                "expected": {
                    "schema_version": "missiond.compiled-project-universe.v1",
                    "source_hash": crate::v3_contracts::SOURCE_HASH,
                },
                "actual": {
                    "schema_version": actual_schema,
                    "source_hash": actual_source_hash,
                    "diagnostics_count": diagnostics_count,
                },
                "recovery": "Run node scripts/project-v3-contracts.mjs --write and node scripts/compile-v3-runtime.mjs --write, then restart MissionD."
            });
            return Self::send_http_error(
                &mut stream,
                503,
                "Service Unavailable",
                &body.to_string(),
            )
            .await;
        }

        let response = Self::http_json_response(raw);
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await?;
        Ok(())
    }

    fn http_json_response(body: String) -> String {
        format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/json\r\n\
             Access-Control-Allow-Origin: *\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n{}",
            body.len(),
            body
        )
    }

    /// #2: HTTP API — slot status endpoint
    async fn handle_slot_status(
        mut stream: TcpStream,
        pty_manager: Arc<PTYManager>,
        db: Option<Arc<dyn crate::db::traits::MissionStore>>,
    ) -> anyhow::Result<()> {
        // Consume the request
        let mut buf = vec![0u8; 4096];
        let _ = stream.read(&mut buf).await;

        let all_status = pty_manager.get_all_status().await;
        let mut status_values: Vec<serde_json::Value> = all_status
            .iter()
            .filter_map(|status| serde_json::to_value(status).ok())
            .collect();
        if let Some(db) = db.as_ref() {
            match db.list_board_tasks(Some("running"), true).await {
                Ok(running_tasks) => {
                    for value in &mut status_values {
                        let slot_id = value
                            .get("slotId")
                            .or_else(|| value.get("slot_id"))
                            .and_then(|slot_id| slot_id.as_str())
                            .map(str::to_string);
                        let Some(slot_id) = slot_id else {
                            continue;
                        };
                        if let Some(task) =
                            Self::active_board_task_for_slot_status(&running_tasks, &slot_id)
                        {
                            value["activeBoardTaskId"] = serde_json::json!(task.id.as_str());
                            value["currentTaskId"] = serde_json::json!(task.id.as_str());
                            value["activeBoardTask"] = Self::board_task_status_summary_json(task);
                        }
                    }
                }
                Err(err) => {
                    warn!(error = %err, "failed to enrich /api/slots with running BoardTask bindings");
                }
            }
        }
        let body = serde_json::to_string(&status_values).unwrap_or_else(|_| "[]".to_string());
        let response = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/json\r\n\
             Access-Control-Allow-Origin: *\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await?;
        Ok(())
    }

    fn active_board_task_for_slot_status<'a>(
        running_tasks: &'a [crate::types::BoardTask],
        slot_id: &str,
    ) -> Option<&'a crate::types::BoardTask> {
        running_tasks
            .iter()
            .filter(|task| {
                task.assignee.as_deref() == Some(slot_id)
                    || (task.claim_executor_type.as_deref() == Some("pty_slot")
                        && task.claim_executor_id.as_deref() == Some(slot_id))
            })
            .max_by(|a, b| a.updated_at.cmp(&b.updated_at))
    }

    fn board_task_status_summary_json(task: &crate::types::BoardTask) -> serde_json::Value {
        serde_json::json!({
            "id": task.id.as_str(),
            "title": task.title.as_str(),
            "status": task.status.as_str(),
            "project": task.project.as_deref(),
            "category": task.category.as_str(),
            "parentId": task.parent_id.as_ref().map(|id| id.as_str()),
            "assignee": task.assignee.as_deref(),
            "claimExecutorId": task.claim_executor_id.as_deref(),
            "claimExecutorType": task.claim_executor_type.as_deref(),
            "updatedAt": task.updated_at.as_str(),
        })
    }

    fn jarvis_slot_runtime_cwd() -> std::path::PathBuf {
        for key in [
            "MISSIOND_PROJECT_ROOT",
            "MISSIOND_ORCHESTRATOR_ROOT",
            "MISSIOND_REPO_ROOT",
            "MISSIOND_WORKSPACE_ROOT",
        ] {
            if let Ok(value) = std::env::var(key) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    return std::path::PathBuf::from(trimmed);
                }
            }
        }
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    }

    fn jarvis_slot_mcp_config() -> Option<std::path::PathBuf> {
        let path = Self::mission_home().join("xjp-mcp-config.json");
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    async fn maybe_auto_heal_jarvis_slot(
        pty_manager: &PTYManager,
        slot_id: &str,
    ) -> serde_json::Value {
        if !jarvis_slot_auto_heal_enabled() {
            return serde_json::json!({
                "status": "skipped",
                "reason": "MISSIOND_JARVIS_SLOT_AUTO_HEAL is not enabled",
            });
        }

        let Some(info) = pty_manager.get_status(slot_id).await else {
            return serde_json::json!({
                "status": "failed",
                "code": "JARVIS_SLOT_NOT_REGISTERED",
                "reason": format!("Default slot {slot_id} is not registered; cannot auto-heal without a projected slot."),
            });
        };

        if !matches!(info.state, SessionState::Exited | SessionState::Error) {
            return serde_json::json!({
                "status": "skipped",
                "reason": format!("Slot {slot_id} is {:?}; auto-heal only restarts Exited/Error slots.", info.state),
                "slot_state": format!("{:?}", info.state),
            });
        }

        let cwd = Self::jarvis_slot_runtime_cwd();
        let mut extra_env = HashMap::new();
        extra_env.insert("MISSIOND_SLOT_ID".to_string(), slot_id.to_string());
        extra_env.insert(
            "MISSIOND_JARVIS_AUTO_HEAL".to_string(),
            chrono::Utc::now().to_rfc3339(),
        );
        extra_env.insert(
            "MISSIOND_ALLOW_BROAD_SKIP_PERMISSIONS".to_string(),
            "true".to_string(),
        );
        extra_env.insert(
            "MISSIOND_PROVIDER_BOX_CLAUDE_CODE_MCP_AUTH_ALLOWLIST".to_string(),
            "mcp:missiond,mcp:supabase".to_string(),
        );
        let slot = PTYSlot {
            id: slot_id.to_string(),
            role: info.role.clone(),
            cwd: Some(cwd.clone()),
            engine: info.engine,
        };
        let options = PTYSpawnOptions {
            auto_restart: true,
            wait_for_idle: true,
            timeout_secs: Some(jarvis_slot_auto_heal_timeout_secs()),
            mcp_config: Self::jarvis_slot_mcp_config(),
            dangerously_skip_permissions: true,
            extra_env,
            ..Default::default()
        };

        match pty_manager.restart(&slot, options).await {
            Ok(restarted) => serde_json::json!({
                "status": "healed",
                "slot_id": slot_id,
                "cwd": cwd,
                "slot_state": format!("{:?}", restarted.state),
                "provider": format!("{:?}", restarted.engine),
                "checked_at": chrono::Utc::now().to_rfc3339(),
            }),
            Err(error) => serde_json::json!({
                "status": "failed",
                "code": "JARVIS_SLOT_AUTO_HEAL_FAILED",
                "slot_id": slot_id,
                "error": error.to_string(),
                "checked_at": chrono::Utc::now().to_rfc3339(),
            }),
        }
    }

    async fn build_jarvis_readiness(
        pty_manager: &PTYManager,
        default_slot: &str,
    ) -> serde_json::Value {
        let abi_freshness = Self::compiled_abi_freshness_check();
        let abi_freshness_ok =
            abi_freshness.get("ok").and_then(|value| value.as_bool()) == Some(true);
        let session_running = pty_manager.is_running(default_slot).await;
        let status = pty_manager.get_status(default_slot).await;
        let (mut readiness, mut reason, slot_state, recognition) = match status {
            None => (
                "slot_unavailable",
                format!("Default slot {} is not registered.", default_slot),
                None,
                None,
            ),
            Some(info) => {
                let slot_state = format!("{:?}", info.state);
                let recognition = info
                    .recognition
                    .as_ref()
                    .and_then(|r| serde_json::to_value(r).ok());
                match info.state {
                    SessionState::Idle => (
                        "ready",
                        format!("Default slot {} is idle.", default_slot),
                        Some(slot_state),
                        recognition,
                    ),
                    SessionState::Exited if session_running => (
                        "stale_slot",
                        format!(
                            "Default slot {} has a live PTY process but MissionD reports Exited; restart the slot.",
                            default_slot
                        ),
                        Some(slot_state),
                        recognition,
                    ),
                    SessionState::Exited => (
                        "slot_unavailable",
                        format!("Default slot {} is not running.", default_slot),
                        Some(slot_state),
                        recognition,
                    ),
                    other => (
                        "busy",
                        format!(
                            "Default slot {} is busy (state: {:?}).",
                            default_slot, other
                        ),
                        Some(slot_state),
                        recognition,
                    ),
                }
            }
        };
        if !abi_freshness_ok {
            readiness = "abi_freshness_mismatch";
            reason = abi_freshness
                .get("reason")
                .and_then(|value| value.as_str())
                .unwrap_or("Compiled V3 contract ABI differs from the MissionD binary.")
                .to_string();
        }

        serde_json::json!({
            "status": readiness,
            "default_slot": default_slot,
            "slot_state": slot_state,
            "session_running": session_running,
            "reason": reason,
            "recognition": recognition,
            "compiled_abi_freshness": abi_freshness,
            "auto_heal": {
                "enabled": jarvis_slot_auto_heal_enabled(),
                "trigger": "chat-request-or-local-ensure",
                "timeout_secs": jarvis_slot_auto_heal_timeout_secs(),
                "env": "MISSIOND_JARVIS_SLOT_AUTO_HEAL",
            },
            "checked_at": chrono::Utc::now().to_rfc3339(),
        })
    }

    async fn ensure_jarvis_slot_ready_for_chat(
        pty_manager: Option<&Arc<PTYManager>>,
        default_slot: &str,
    ) -> Result<serde_json::Value, serde_json::Value> {
        let Some(pty_manager) = pty_manager else {
            return Err(serde_json::json!({
                "error": {
                    "code": "JARVIS_SLOT_MANAGER_UNAVAILABLE",
                    "message": "MissionD PTY manager is unavailable; Jarvis cannot dispatch work."
                },
                "auto_heal": {
                    "status": "skipped",
                    "reason": "PTY manager unavailable"
                },
                "checked_at": chrono::Utc::now().to_rfc3339(),
            }));
        };

        let readiness = Self::build_jarvis_readiness(pty_manager, default_slot).await;
        let status = readiness
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        if status == "ready" {
            return Ok(serde_json::json!({
                "status": "ready",
                "readiness": readiness,
            }));
        }
        if status == "busy" {
            return Err(serde_json::json!({
                "error": {
                    "code": "JARVIS_SLOT_BUSY",
                    "message": readiness
                        .get("reason")
                        .and_then(|value| value.as_str())
                        .unwrap_or("Default Jarvis slot is busy.")
                },
                "readiness": readiness,
                "auto_heal": {
                    "status": "skipped",
                    "reason": "busy slots are not restarted automatically"
                },
            }));
        }
        if status == "abi_freshness_mismatch" {
            return Err(serde_json::json!({
                "error": {
                    "code": "ABI_FRESHNESS_MISMATCH",
                    "message": readiness
                        .get("reason")
                        .and_then(|value| value.as_str())
                        .unwrap_or("Compiled V3 contract ABI differs from the MissionD binary.")
                },
                "readiness": readiness,
                "auto_heal": {
                    "status": "skipped",
                    "reason": "ABI freshness mismatch is a deployment artifact problem, not a PTY slot problem"
                },
            }));
        }

        let auto_heal = Self::maybe_auto_heal_jarvis_slot(pty_manager, default_slot).await;
        let healed = auto_heal
            .get("status")
            .and_then(|value| value.as_str())
            .map(|value| value == "healed")
            .unwrap_or(false);
        if healed {
            let readiness_after_heal =
                Self::build_jarvis_readiness(pty_manager, default_slot).await;
            let status_after_heal = readiness_after_heal
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            if status_after_heal == "ready" {
                return Ok(serde_json::json!({
                    "status": "ready",
                    "readiness": readiness_after_heal,
                    "auto_heal": auto_heal,
                }));
            }
            return Err(serde_json::json!({
                "error": {
                    "code": "JARVIS_SLOT_AUTO_HEAL_NOT_READY",
                    "message": "Default Jarvis slot restarted but did not become ready."
                },
                "readiness": readiness_after_heal,
                "auto_heal": auto_heal,
            }));
        }

        Err(serde_json::json!({
            "error": {
                "code": "JARVIS_SLOT_UNAVAILABLE",
                "message": readiness
                    .get("reason")
                    .and_then(|value| value.as_str())
                    .unwrap_or("Default Jarvis slot is unavailable.")
            },
            "readiness": readiness,
            "auto_heal": auto_heal,
        }))
    }

    /// HTTP API — default Jarvis slot readiness.
    ///
    /// `/health` only proves the public proxy and daemon are reachable. Jarvis
    /// callers also need to know whether the default executable slot can accept
    /// a turn, or whether it is busy/unavailable.
    async fn handle_readiness(
        mut stream: TcpStream,
        pty_manager: Arc<PTYManager>,
        default_slot: String,
    ) -> anyhow::Result<()> {
        // Consume the request.
        let mut buf = vec![0u8; 4096];
        let _ = stream.read(&mut buf).await;

        let body = Self::build_jarvis_readiness(&pty_manager, &default_slot)
            .await
            .to_string();
        let response = Self::http_json_response(body);
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await?;
        Ok(())
    }

    async fn ensure_jarvis_provider_slots_for_monitor(
        pty_manager: &PTYManager,
        default_slot: &str,
        intent_author: &JarvisIntentAuthorConfig,
        key_judgment_author: &JarvisKeyJudgmentAuthorConfig,
        plan_author: &JarvisPlanAuthorConfig,
    ) -> serde_json::Value {
        let specs = Self::jarvis_provider_slot_monitor_specs(
            default_slot,
            intent_author,
            key_judgment_author,
            plan_author,
        );
        let mut handled_slot_ids = std::collections::HashSet::<String>::new();
        let mut actions = Vec::new();

        for spec in specs {
            if !spec.critical {
                actions.push(serde_json::json!({
                    "phase": spec.phase,
                    "slot_id": spec.slot_id,
                    "status": "skipped",
                    "reason": "provider slot is not critical for monitor readiness",
                }));
                continue;
            }
            if !handled_slot_ids.insert(spec.slot_id.clone()) {
                actions.push(serde_json::json!({
                    "phase": spec.phase,
                    "slot_id": spec.slot_id,
                    "status": "skipped",
                    "reason": "slot_id already handled for another monitor phase",
                }));
                continue;
            }

            let info = pty_manager.get_status(&spec.slot_id).await;
            let (before_status, ok, reason) =
                Self::jarvis_provider_slot_status(info.as_ref(), &spec);
            let before_state = info.as_ref().map(|slot| format!("{:?}", slot.state));
            if ok {
                actions.push(serde_json::json!({
                    "phase": spec.phase,
                    "slot_id": spec.slot_id,
                    "status": "skipped",
                    "before_status": before_status,
                    "before_state": before_state,
                    "reason": "provider slot already satisfies monitor readiness",
                }));
                continue;
            }

            if Self::jarvis_provider_slot_is_workspace_trust_blocked(info.as_ref()) {
                let workspace_trust =
                    Self::maybe_accept_jarvis_workspace_trust(pty_manager, &spec.slot_id).await;
                actions.push(serde_json::json!({
                    "phase": spec.phase,
                    "slot_id": spec.slot_id,
                    "status": "attempted",
                    "before_status": before_status,
                    "before_state": before_state,
                    "workspace_trust": workspace_trust,
                }));
                continue;
            }

            let restartable = info
                .as_ref()
                .map(|slot| matches!(slot.state, SessionState::Exited | SessionState::Error))
                .unwrap_or(false);
            if !restartable {
                actions.push(serde_json::json!({
                    "phase": spec.phase,
                    "slot_id": spec.slot_id,
                    "status": "skipped",
                    "before_status": before_status,
                    "before_state": before_state,
                    "reason": reason,
                }));
                continue;
            }

            let auto_heal = Self::maybe_auto_heal_jarvis_slot(pty_manager, &spec.slot_id).await;
            actions.push(serde_json::json!({
                "phase": spec.phase,
                "slot_id": spec.slot_id,
                "status": "attempted",
                "before_status": before_status,
                "before_state": before_state,
                "auto_heal": auto_heal,
            }));
        }

        serde_json::json!({
            "schema": "missiond.jarvis-provider-slot-ensure.v1",
            "actions": actions,
            "checked_at": chrono::Utc::now().to_rfc3339(),
        })
    }

    fn jarvis_provider_slot_is_workspace_trust_blocked(
        info: Option<&crate::pty::PTYAgentInfo>,
    ) -> bool {
        let Some(recognition) = info.and_then(|slot| slot.recognition.as_ref()) else {
            return false;
        };
        recognition.blocked_kind.as_deref() == Some("workspace_trust")
            || recognition.reason.contains("workspace_trust_prompt")
    }

    fn jarvis_workspace_trust_selection(screen: &str) -> &'static str {
        for line in screen.lines() {
            let trimmed = line
                .trim_start()
                .trim_start_matches(|c: char| matches!(c, '│' | '┃' | '║' | '┆' | '┊'))
                .trim_start();
            let selected = trimmed.starts_with('>')
                || trimmed.starts_with('›')
                || trimmed.starts_with('❯')
                || trimmed.starts_with('▸')
                || trimmed.starts_with('▶')
                || trimmed.starts_with('➜')
                || trimmed.starts_with('→');
            if !selected {
                continue;
            }
            let lower = trimmed
                .trim_start_matches(|c: char| matches!(c, '>' | '›' | '❯' | '▸' | '▶' | '➜' | '→'))
                .trim_start()
                .to_ascii_lowercase();
            if lower.starts_with("yes, i trust this folder") {
                return "trust";
            }
            if lower.starts_with("no, exit") {
                return "exit";
            }
        }
        "unknown"
    }

    async fn maybe_accept_jarvis_workspace_trust(
        pty_manager: &PTYManager,
        slot_id: &str,
    ) -> serde_json::Value {
        let mut steps = Vec::new();
        let mut screen = match pty_manager.get_screen(slot_id).await {
            Ok(screen) => screen,
            Err(error) => {
                return serde_json::json!({
                    "status": "failed",
                    "code": "JARVIS_WORKSPACE_TRUST_SCREEN_UNAVAILABLE",
                    "error": error.to_string(),
                    "checked_at": chrono::Utc::now().to_rfc3339(),
                });
            }
        };
        let mut selection = Self::jarvis_workspace_trust_selection(&screen);

        if selection == "exit" {
            for (key_name, bytes) in [("up", "\x1b[A"), ("down", "\x1b[B")] {
                match pty_manager.write(slot_id, bytes).await {
                    Ok(_) => {
                        steps.push(serde_json::json!({
                            "action": key_name,
                            "status": "sent",
                        }));
                    }
                    Err(error) => {
                        return serde_json::json!({
                            "status": "failed",
                            "code": "JARVIS_WORKSPACE_TRUST_MOVE_FAILED",
                            "action": key_name,
                            "error": error.to_string(),
                            "steps": steps,
                            "checked_at": chrono::Utc::now().to_rfc3339(),
                        });
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                screen = pty_manager.get_screen(slot_id).await.unwrap_or_default();
                selection = Self::jarvis_workspace_trust_selection(&screen);
                if selection == "trust" {
                    break;
                }
            }
        }

        if selection != "trust" {
            return serde_json::json!({
                "status": "failed",
                "code": "JARVIS_WORKSPACE_TRUST_SELECTION_UNVERIFIED",
                "selection": selection,
                "steps": steps,
                "checked_at": chrono::Utc::now().to_rfc3339(),
            });
        }

        if let Err(error) = pty_manager.write(slot_id, "\r").await {
            return serde_json::json!({
                "status": "failed",
                "code": "JARVIS_WORKSPACE_TRUST_ENTER_FAILED",
                "error": error.to_string(),
                "steps": steps,
                "checked_at": chrono::Utc::now().to_rfc3339(),
            });
        }
        steps.push(serde_json::json!({
            "action": "enter",
            "status": "sent",
        }));

        let started = std::time::Instant::now();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let Some(after) = pty_manager.get_status(slot_id).await else {
                return serde_json::json!({
                    "status": "failed",
                    "code": "JARVIS_WORKSPACE_TRUST_SLOT_MISSING_AFTER_ENTER",
                    "steps": steps,
                    "checked_at": chrono::Utc::now().to_rfc3339(),
                });
            };
            if matches!(after.state, SessionState::Idle) {
                return serde_json::json!({
                    "status": "accepted",
                    "slot_state": format!("{:?}", after.state),
                    "elapsed_ms": started.elapsed().as_millis(),
                    "steps": steps,
                    "checked_at": chrono::Utc::now().to_rfc3339(),
                });
            }
            if !Self::jarvis_provider_slot_is_workspace_trust_blocked(Some(&after))
                && !matches!(after.state, SessionState::Confirming)
            {
                return serde_json::json!({
                    "status": "advanced",
                    "slot_state": format!("{:?}", after.state),
                    "elapsed_ms": started.elapsed().as_millis(),
                    "steps": steps,
                    "checked_at": chrono::Utc::now().to_rfc3339(),
                });
            }
            if started.elapsed() >= std::time::Duration::from_secs(10) {
                return serde_json::json!({
                    "status": "failed",
                    "code": "JARVIS_WORKSPACE_TRUST_STILL_BLOCKED",
                    "slot_state": format!("{:?}", after.state),
                    "elapsed_ms": started.elapsed().as_millis(),
                    "steps": steps,
                    "checked_at": chrono::Utc::now().to_rfc3339(),
                });
            }
        }
    }

    /// Local-only control surface for deploy-center post-deploy smoke.
    ///
    /// Public `/jarvis/*` monitor paths stay read-only. The self-update lane calls
    /// this endpoint from Mac mini localhost after blue/green restart to restore
    /// the default Jarvis slot and critical provider-box lanes before judging
    /// monitor readiness.
    async fn handle_jarvis_slot_ensure(
        mut stream: TcpStream,
        addr: SocketAddr,
        pty_manager: Arc<PTYManager>,
        default_slot: String,
        jarvis_intent_author: JarvisIntentAuthorConfig,
        jarvis_key_judgment_author: JarvisKeyJudgmentAuthorConfig,
        jarvis_plan_author: JarvisPlanAuthorConfig,
    ) -> anyhow::Result<()> {
        let mut buf = vec![0u8; 4096];
        let _ = stream.read(&mut buf).await;

        if !addr.ip().is_loopback() {
            let body = serde_json::json!({
                "schema": "missiond.jarvis-slot-ensure.v1",
                "overall": "forbidden",
                "error": {
                    "code": "JARVIS_SLOT_ENSURE_LOCAL_ONLY",
                    "message": "Jarvis slot ensure is a localhost-only deploy smoke control surface."
                },
                "checked_at": chrono::Utc::now().to_rfc3339(),
            });
            return Self::send_http_error(&mut stream, 403, "Forbidden", &body.to_string()).await;
        }

        let before = Self::build_jarvis_readiness(&pty_manager, &default_slot).await;
        let before_status = before
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        if before_status == "busy" {
            let body = serde_json::json!({
                "schema": "missiond.jarvis-slot-ensure.v1",
                "overall": "busy",
                "default_slot": default_slot,
                "readiness_before": before,
                "auto_heal": {
                    "status": "skipped",
                    "reason": "busy slots are not restarted automatically"
                },
                "checked_at": chrono::Utc::now().to_rfc3339(),
            });
            return Self::send_http_error(&mut stream, 409, "Conflict", &body.to_string()).await;
        }
        if before_status == "abi_freshness_mismatch" {
            let body = serde_json::json!({
                "schema": "missiond.jarvis-slot-ensure.v1",
                "overall": "abi_freshness_mismatch",
                "default_slot": default_slot,
                "readiness_before": before,
                "auto_heal": {
                    "status": "skipped",
                    "reason": "ABI freshness mismatch is a deployment artifact problem, not a PTY slot problem"
                },
                "checked_at": chrono::Utc::now().to_rfc3339(),
            });
            return Self::send_http_error(
                &mut stream,
                503,
                "Service Unavailable",
                &body.to_string(),
            )
            .await;
        }

        let auto_heal = if before_status == "ready" {
            serde_json::json!({
                "status": "skipped",
                "reason": "default slot already ready"
            })
        } else {
            Self::maybe_auto_heal_jarvis_slot(&pty_manager, &default_slot).await
        };
        let after = Self::build_jarvis_readiness(&pty_manager, &default_slot).await;
        let after_status = after
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        let provider_slot_ensure = if after_status == "ready" {
            Self::ensure_jarvis_provider_slots_for_monitor(
                &pty_manager,
                &default_slot,
                &jarvis_intent_author,
                &jarvis_key_judgment_author,
                &jarvis_plan_author,
            )
            .await
        } else {
            serde_json::json!({
                "schema": "missiond.jarvis-provider-slot-ensure.v1",
                "status": "skipped",
                "reason": "default slot is not ready",
                "checked_at": chrono::Utc::now().to_rfc3339(),
            })
        };
        let provider_box_slots_after = Self::jarvis_provider_box_slots_snapshot(
            &pty_manager,
            &default_slot,
            &jarvis_intent_author,
            &jarvis_key_judgment_author,
            &jarvis_plan_author,
        )
        .await;
        let provider_slot_failures = provider_box_slots_after
            .get("summary")
            .and_then(|value| {
                value
                    .get("blocking_failures")
                    .or_else(|| value.get("critical_failures"))
            })
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let ok = after_status == "ready" && provider_slot_failures == 0;
        let body = serde_json::json!({
            "schema": "missiond.jarvis-slot-ensure.v1",
            "overall": if ok {
                "ready"
            } else if provider_slot_failures > 0 {
                "provider_slot_unavailable"
            } else {
                "unavailable"
            },
            "default_slot": default_slot,
            "readiness_before": before,
            "readiness_after": after,
            "auto_heal": auto_heal,
            "provider_slot_ensure": provider_slot_ensure,
            "provider_box_slots_after": provider_box_slots_after,
            "checked_at": chrono::Utc::now().to_rfc3339(),
        });
        if ok {
            let response = Self::http_json_response(body.to_string());
            stream.write_all(response.as_bytes()).await?;
            stream.shutdown().await?;
            Ok(())
        } else {
            Self::send_http_error(&mut stream, 503, "Service Unavailable", &body.to_string()).await
        }
    }

    fn file_check(id: &str, label: &str, path: std::path::PathBuf) -> serde_json::Value {
        let metadata = std::fs::metadata(&path).ok();
        serde_json::json!({
            "id": id,
            "label": label,
            "ok": metadata.is_some(),
            "critical": false,
            "status": if metadata.is_some() { "ok" } else { "missing" },
            "path": path,
            "size_bytes": metadata.as_ref().map(|m| m.len()),
            "modified_unix_secs": metadata
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs()),
        })
    }

    fn compiled_runtime_dir() -> std::path::PathBuf {
        if let Ok(dir) = std::env::var("MISSIOND_COMPILED_RUNTIME_DIR") {
            let trimmed = dir.trim();
            if !trimmed.is_empty() {
                return std::path::PathBuf::from(trimmed);
            }
        }
        if let Ok(dir) = std::env::var("MISSIOND_RUNTIME_DIR") {
            let trimmed = dir.trim();
            if !trimmed.is_empty() {
                return std::path::PathBuf::from(trimmed).join("compiled");
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let candidate = parent
                    .join(".missiond")
                    .join("v3")
                    .join("runtime")
                    .join("compiled");
                if candidate.exists() {
                    return candidate;
                }
            }
        }
        std::path::PathBuf::from(".missiond/v3/runtime/compiled")
    }

    fn compiled_abi_freshness_check() -> serde_json::Value {
        Self::compiled_abi_freshness_check_in_dir(Self::compiled_runtime_dir())
    }

    fn compiled_abi_freshness_check_in_dir(
        compiled_runtime_dir: impl AsRef<std::path::Path>,
    ) -> serde_json::Value {
        let compiled_runtime_dir = compiled_runtime_dir.as_ref();
        let contract_abi = Self::compiled_runtime_json_freshness_check(
            "compiled-contract-abi",
            "Compiled V3 contract ABI",
            compiled_runtime_dir.join("compiled-contract-abi.json"),
            crate::v3_contracts::SCHEMA_VERSION,
            crate::v3_contracts::SOURCE_HASH,
        );
        let runtime_config = Self::compiled_runtime_json_freshness_check(
            "compiled-runtime-config",
            "Compiled V3 runtime config",
            compiled_runtime_dir.join("compiled-runtime-config.json"),
            "missiond.compiled-runtime-config.v1",
            crate::v3_contracts::SOURCE_HASH,
        );
        let artifacts = vec![contract_abi, runtime_config];
        let ok = artifacts
            .iter()
            .all(|artifact| artifact.get("ok").and_then(|value| value.as_bool()) == Some(true));
        let failed: Vec<String> = artifacts
            .iter()
            .filter(|artifact| artifact.get("ok").and_then(|value| value.as_bool()) != Some(true))
            .filter_map(|artifact| {
                let id = artifact.get("id").and_then(|value| value.as_str())?;
                let status = artifact
                    .get("status")
                    .and_then(|value| value.as_str())
                    .unwrap_or("failed");
                Some(format!("{id}:{status}"))
            })
            .collect();
        serde_json::json!({
            "id": "compiled-abi-freshness",
            "label": "Compiled V3 ABI freshness",
            "ok": ok,
            "critical": true,
            "status": if ok { "ok" } else { "abi_freshness_mismatch" },
            "reason": if ok {
                "Compiled V3 contract ABI and runtime config match the MissionD binary."
            } else {
                "Compiled V3 contract ABI/runtime config differ from the MissionD binary; run node scripts/project-v3-contracts.mjs --write, node scripts/compile-v3-runtime.mjs --json, then redeploy MissionD."
            },
            "compiled_runtime_dir": compiled_runtime_dir,
            "binary_contract_abi": {
                "schema": crate::v3_contracts::SCHEMA_VERSION,
                "source_hash": crate::v3_contracts::SOURCE_HASH,
                "runtime_config_source_hash": crate::v3_contracts::RUNTIME_CONFIG_SOURCE_HASH,
                "project_universe_source_hash": crate::v3_contracts::PROJECT_UNIVERSE_SOURCE_HASH,
            },
            "failed": failed,
            "artifacts": artifacts,
        })
    }

    fn compiled_runtime_json_freshness_check(
        id: &str,
        label: &str,
        path: std::path::PathBuf,
        expected_schema: &str,
        expected_source_hash: &str,
    ) -> serde_json::Value {
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) => {
                return serde_json::json!({
                    "id": id,
                    "label": label,
                    "ok": false,
                    "critical": true,
                    "status": "missing_or_unreadable",
                    "path": path,
                    "expected_schema": expected_schema,
                    "expected_source_hash": expected_source_hash,
                    "diagnostic": "abi_freshness_mismatch",
                    "reason": err.to_string(),
                });
            }
        };
        let parsed: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(parsed) => parsed,
            Err(err) => {
                return serde_json::json!({
                    "id": id,
                    "label": label,
                    "ok": false,
                    "critical": true,
                    "status": "invalid_json",
                    "path": path,
                    "expected_schema": expected_schema,
                    "expected_source_hash": expected_source_hash,
                    "diagnostic": "abi_freshness_mismatch",
                    "reason": err.to_string(),
                });
            }
        };
        let actual_schema = parsed
            .get("schema_version")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let actual_source_hash = parsed
            .get("source_hash")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let diagnostics_count = parsed
            .get("diagnostics")
            .and_then(|value| value.as_array())
            .map(|rows| rows.len())
            .unwrap_or(0);
        let status = if actual_schema != expected_schema {
            "schema_mismatch"
        } else if actual_source_hash != expected_source_hash {
            "source_hash_mismatch"
        } else if diagnostics_count > 0 {
            "compiled_diagnostics_present"
        } else {
            "ok"
        };
        let ok = status == "ok";
        serde_json::json!({
            "id": id,
            "label": label,
            "ok": ok,
            "critical": true,
            "status": status,
            "path": path,
            "expected_schema": expected_schema,
            "actual_schema": actual_schema,
            "expected_source_hash": expected_source_hash,
            "actual_source_hash": actual_source_hash,
            "diagnostics_count": diagnostics_count,
            "diagnostic": if ok { "none" } else { "abi_freshness_mismatch" },
        })
    }

    fn mission_home() -> std::path::PathBuf {
        if let Ok(root) = std::env::var("MISSIOND_INSTALL_ROOT") {
            return std::path::PathBuf::from(root);
        }
        std::env::var("HOME")
            .map(|home| std::path::PathBuf::from(home).join(".xjp-mission"))
            .unwrap_or_else(|_| std::path::PathBuf::from(".xjp-mission"))
    }

    fn release_snapshot() -> serde_json::Value {
        let home = Self::mission_home();
        let active = std::env::var("MISSIOND_ACTIVE_LINK")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| home.join("active"));
        let active_target = std::fs::read_link(&active)
            .ok()
            .map(|p| p.to_string_lossy().to_string());
        serde_json::json!({
            "install_root": home,
            "active_link": active,
            "active_target": active_target,
            "daemon_pid": std::process::id(),
        })
    }

    fn jarvis_runtime_topology_snapshot(compiled_runtime_dir: &Path) -> serde_json::Value {
        let project_universe_path = compiled_runtime_dir.join("compiled-project-universe.json");
        let parsed = std::fs::read_to_string(&project_universe_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
        let topology = parsed
            .as_ref()
            .and_then(|value| value.pointer("/payload/jarvis_runtime_topologies"))
            .and_then(|value| value.as_array())
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| {
                        item.get("service_id").and_then(|value| value.as_str())
                            == Some("missiond-jarvis-edge")
                    })
                    .or_else(|| items.first())
            })
            .cloned();
        let mut topology = topology.unwrap_or_else(Self::jarvis_runtime_topology_fallback);
        if let Some(object) = topology.as_object_mut() {
            object.insert(
                "projection_path".to_string(),
                serde_json::Value::String(project_universe_path.display().to_string()),
            );
            object
                .entry("projection_source".to_string())
                .or_insert_with(|| {
                    serde_json::Value::String(
                        if parsed.is_some() {
                            "compiled-project-universe"
                        } else {
                            "compiled-fallback"
                        }
                        .to_string(),
                    )
                });
        }
        crate::evidence_redactor::redact_json_value(&topology)
    }

    fn jarvis_runtime_topology_fallback() -> serde_json::Value {
        serde_json::json!({
            "schema": "missiond.jarvis-runtime-topology.v1",
            "service_id": "missiond-jarvis-edge",
            "edge_node": "gcp-caddy-edge",
            "edge_domain": "jarvis.xiaojins.com",
            "edge_public_ip": "34.104.147.118",
            "edge_proxy": "caddy",
            "origin_node": "bwg-tunnel",
            "origin": "104.194.81.38:9876",
            "tunnel_server_url": "ws://104.194.81.38:9876/tunnel/ws",
            "tunnel_client_id": "rickyhqmac-mini-jarvis",
            "target_node": "rickyhq-macmini-m4",
            "target_service": "missiond",
            "target_local_url": "http://127.0.0.1:9120",
            "expected_deploy_agent_version": "10.7.15",
            "launchd_unit": "com.xiaojinpro.jarvis-tunnel",
            "launchd_plist": "~/Library/LaunchAgents/com.xiaojinpro.jarvis-tunnel.plist",
            "local_health_url": "http://127.0.0.1:9880/health",
            "route_generation": "jarvis-gcp-bwg-macmini-20260603",
            "proxy_no_buffer": true,
            "proxy_flush_interval": "-1",
            "proxy_read_timeout": "75s",
            "proxy_write_timeout": "75s",
            "proxy_stream_timeout": "0",
            "streaming_policy": "sse-no-buffer bounded-upstream-idle typed-terminal-diagnostic",
            "authority": "compiled-fallback"
        })
    }

    async fn jarvis_observed_deploy_agent_version(health_url: &str) -> JarvisObservedVersion {
        let health_url = health_url.trim();
        if health_url.is_empty() {
            return JarvisObservedVersion {
                version: None,
                source: None,
                diagnostic: Some("local_health_url_missing".to_string()),
            };
        }

        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(900))
            .build()
        {
            Ok(client) => client,
            Err(error) => {
                return JarvisObservedVersion {
                    version: None,
                    source: Some("local_health_url".to_string()),
                    diagnostic: Some(format!("client_build_failed: {error}")),
                };
            }
        };
        let response = match client.get(health_url).send().await {
            Ok(response) => response,
            Err(error) => {
                return JarvisObservedVersion {
                    version: None,
                    source: Some("local_health_url".to_string()),
                    diagnostic: Some(format!("health_request_failed: {error}")),
                };
            }
        };
        let status = response.status();
        let body = match response.json::<serde_json::Value>().await {
            Ok(body) => body,
            Err(error) => {
                return JarvisObservedVersion {
                    version: None,
                    source: Some("local_health_url".to_string()),
                    diagnostic: Some(format!("health_json_failed status={status}: {error}")),
                };
            }
        };
        let version = body
            .get("version")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        JarvisObservedVersion {
            version,
            source: Some("local_health_url".to_string()),
            diagnostic: if status.is_success() {
                None
            } else {
                Some(format!("health_status={status}"))
            },
        }
    }

    async fn jarvis_topology_checks(topology: &serde_json::Value) -> Vec<serde_json::Value> {
        let field = |name: &str| {
            topology
                .get(name)
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let observed_deploy_agent =
            Self::jarvis_observed_deploy_agent_version(&field("local_health_url")).await;
        let launchd_plist = field("launchd_plist");
        let launchd_path = Self::expand_home_path(&launchd_plist);
        let launchd_exists = launchd_path
            .as_ref()
            .map(|path| path.exists())
            .unwrap_or(false);
        let expected_version = field("expected_deploy_agent_version");
        let observed_version = observed_deploy_agent.version.or_else(|| {
            std::env::var("MISSIOND_JARVIS_TUNNEL_DEPLOY_AGENT_VERSION")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        });
        let version_ok = observed_version
            .as_deref()
            .map(|actual| actual == expected_version)
            .unwrap_or(true);

        vec![
            Self::declared_topology_check(
                "dns-edge",
                "Public DNS and edge node",
                &[
                    ("edge_domain", field("edge_domain")),
                    ("edge_public_ip", field("edge_public_ip")),
                    ("edge_node", field("edge_node")),
                ],
            ),
            Self::declared_topology_check(
                "caddy-route",
                "GCP Caddy route to BWG origin",
                &[
                    ("edge_proxy", field("edge_proxy")),
                    ("origin", field("origin")),
                    ("route_generation", field("route_generation")),
                    ("proxy_flush_interval", field("proxy_flush_interval")),
                    ("proxy_read_timeout", field("proxy_read_timeout")),
                    ("proxy_write_timeout", field("proxy_write_timeout")),
                    ("proxy_stream_timeout", field("proxy_stream_timeout")),
                ],
            ),
            Self::declared_topology_check(
                "tunnel-server",
                "BWG tunnel server binding",
                &[
                    ("origin_node", field("origin_node")),
                    ("tunnel_server_url", field("tunnel_server_url")),
                ],
            ),
            Self::declared_topology_check(
                "tunnel-client",
                "Mac mini Jarvis tunnel client binding",
                &[
                    ("tunnel_client_id", field("tunnel_client_id")),
                    ("target_node", field("target_node")),
                ],
            ),
            serde_json::json!({
                "id": "launchd-unit",
                "label": "Mac mini Jarvis tunnel LaunchAgent",
                "ok": launchd_exists,
                "critical": false,
                "status": if launchd_exists { "present" } else { "not_observed" },
                "unit": field("launchd_unit"),
                "path": launchd_path.as_ref().map(|path| path.display().to_string()).unwrap_or(launchd_plist),
                "reason": if launchd_exists {
                    "Jarvis tunnel client is declared as a persistent LaunchAgent."
                } else {
                    "LaunchAgent path was not observed on this host; route declaration remains visible for remote diagnostics."
                }
            }),
            serde_json::json!({
                "id": "deploy-agent-version",
                "label": "Jarvis tunnel deploy-agent version",
                "ok": version_ok,
                "critical": false,
                "status": if observed_version.is_some() {
                    if version_ok { "matched" } else { "version_drift" }
                } else {
                    "unobserved"
                },
                "expected": expected_version,
                "observed": observed_version,
                "observed_source": observed_deploy_agent.source,
                "health_url": field("local_health_url"),
                "diagnostic": observed_deploy_agent.diagnostic,
            }),
            serde_json::json!({
                "id": "jarvis-phase-capabilities",
                "label": "Jarvis phase execution capabilities",
                "ok": true,
                "critical": true,
                "status": "declared",
                "phases": ["identity", "intent", "grounding", "key_judgment", "plan", "communicator", "direct_answer", "board_dispatch"],
                "completion_authority": "artifact-or-task-result",
                "provider_box_output_contract": "must_write_file may complete file-writing workers"
            }),
        ]
    }

    fn declared_topology_check(
        id: &str,
        label: &str,
        fields: &[(&str, String)],
    ) -> serde_json::Value {
        let missing = fields
            .iter()
            .filter(|(_, value)| value.trim().is_empty())
            .map(|(name, _)| *name)
            .collect::<Vec<_>>();
        serde_json::json!({
            "id": id,
            "label": label,
            "ok": missing.is_empty(),
            "critical": true,
            "status": if missing.is_empty() { "declared" } else { "missing_topology_fields" },
            "fields": fields.iter().map(|(name, value)| serde_json::json!({
                "name": name,
                "value": value
            })).collect::<Vec<_>>(),
            "missing": missing,
        })
    }

    fn jarvis_grounding_worker_slot_id_for_monitor() -> String {
        Self::env_var_trimmed("MISSIOND_JARVIS_GROUNDING_WORKER_SLOT_ID")
            .unwrap_or_else(|| "slot-jarvis-grounding-claude".to_string())
    }

    fn jarvis_direct_answer_slot_id_for_monitor(provider: &str, fallback_slot: &str) -> String {
        Self::jarvis_text_only_slot_id(
            provider,
            Self::env_var_trimmed("MISSIOND_JARVIS_DIRECT_ANSWER_SLOT_ID")
                .or_else(|| Self::env_var_trimmed("MISSIOND_JARVIS_COMMUNICATOR_SLOT_ID"))
                .as_deref(),
            fallback_slot,
        )
    }

    fn jarvis_provider_slot_monitor_specs(
        default_slot: &str,
        intent_author: &JarvisIntentAuthorConfig,
        key_judgment_author: &JarvisKeyJudgmentAuthorConfig,
        plan_author: &JarvisPlanAuthorConfig,
    ) -> Vec<JarvisProviderSlotMonitorSpec> {
        let author_provider = Self::jarvis_author_text_provider();
        let communicator_provider = Self::jarvis_communicator_provider();
        let communicator_slot = Self::jarvis_communicator_slot_id(&communicator_provider);
        let direct_answer_slot = Self::jarvis_direct_answer_slot_id_for_monitor(
            &communicator_provider,
            &communicator_slot,
        );

        vec![
            JarvisProviderSlotMonitorSpec {
                phase: "default_chat",
                role: "chat-default",
                provider: "claude_code".to_string(),
                engine: "claude_code".to_string(),
                slot_id: default_slot.to_string(),
                model: None,
                model_profile: None,
                residency: "resident",
                required_ready: true,
                critical: true,
            },
            JarvisProviderSlotMonitorSpec {
                phase: "intent",
                role: "semantic-author",
                provider: author_provider.clone(),
                engine: Self::provider_box_engine_for_provider(&author_provider)
                    .unwrap_or("codex")
                    .to_string(),
                slot_id: Self::jarvis_author_text_slot_id_for_phase(
                    &author_provider,
                    "MISSIOND_JARVIS_INTENT_AUTHOR_SLOT_ID",
                    &intent_author.slot_id,
                ),
                model: Self::jarvis_author_text_model(&author_provider, &intent_author.model),
                model_profile: Some(intent_author.reasoning_effort.clone()),
                residency: "resident",
                required_ready: true,
                critical: true,
            },
            JarvisProviderSlotMonitorSpec {
                phase: "grounding",
                role: "context-gather-worker",
                provider: "claude_code".to_string(),
                engine: "claude_code".to_string(),
                slot_id: Self::jarvis_grounding_worker_slot_id_for_monitor(),
                model: None,
                model_profile: None,
                residency: "spawnable",
                required_ready: false,
                critical: true,
            },
            JarvisProviderSlotMonitorSpec {
                phase: "key_judgment",
                role: "semantic-author",
                provider: author_provider.clone(),
                engine: Self::provider_box_engine_for_provider(&author_provider)
                    .unwrap_or("codex")
                    .to_string(),
                slot_id: Self::jarvis_author_text_slot_id_for_phase(
                    &author_provider,
                    "MISSIOND_JARVIS_KEY_JUDGMENT_AUTHOR_SLOT_ID",
                    &key_judgment_author.slot_id,
                ),
                model: Self::jarvis_author_text_model(&author_provider, &key_judgment_author.model),
                model_profile: Some(key_judgment_author.reasoning_effort.clone()),
                residency: "resident",
                required_ready: true,
                critical: true,
            },
            JarvisProviderSlotMonitorSpec {
                phase: "plan",
                role: "semantic-author",
                provider: author_provider.clone(),
                engine: Self::provider_box_engine_for_provider(&author_provider)
                    .unwrap_or("codex")
                    .to_string(),
                slot_id: Self::jarvis_author_text_slot_id_for_phase(
                    &author_provider,
                    "MISSIOND_JARVIS_PLAN_AUTHOR_SLOT_ID",
                    &plan_author.slot_id,
                ),
                model: Self::jarvis_author_text_model(&author_provider, &plan_author.model),
                model_profile: Some(plan_author.reasoning_effort.clone()),
                residency: "resident",
                required_ready: true,
                critical: true,
            },
            JarvisProviderSlotMonitorSpec {
                phase: "communicator",
                role: "communication-officer",
                provider: communicator_provider.clone(),
                engine: Self::provider_box_engine_for_provider(&communicator_provider)
                    .unwrap_or("agy")
                    .to_string(),
                slot_id: communicator_slot.clone(),
                model: Self::jarvis_communicator_model_for_provider(&communicator_provider),
                model_profile: None,
                residency: "spawnable",
                required_ready: false,
                critical: true,
            },
            JarvisProviderSlotMonitorSpec {
                phase: "direct_answer",
                role: "direct-answer-provider",
                provider: communicator_provider.clone(),
                engine: Self::provider_box_engine_for_provider(&communicator_provider)
                    .unwrap_or("agy")
                    .to_string(),
                slot_id: direct_answer_slot,
                model: Self::jarvis_direct_answer_model(&communicator_provider),
                model_profile: None,
                residency: "spawnable",
                required_ready: false,
                critical: true,
            },
        ]
    }

    fn jarvis_slot_state_wire(state: SessionState) -> String {
        serde_json::to_value(state)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| format!("{:?}", state).to_ascii_lowercase())
    }

    fn jarvis_provider_slot_status(
        info: Option<&crate::pty::PTYAgentInfo>,
        spec: &JarvisProviderSlotMonitorSpec,
    ) -> (&'static str, bool, String) {
        let Some(info) = info else {
            return if spec.required_ready {
                (
                    "missing_required",
                    false,
                    "Required resident provider-box slot is not observed by PTYManager."
                        .to_string(),
                )
            } else {
                (
                    "not_observed_spawnable",
                    true,
                    "On-demand provider-box slot is not currently running; provider-box may spawn it when requested.".to_string(),
                )
            };
        };
        match info.state {
            SessionState::Idle => (
                "ready",
                true,
                "Provider slot is idle and ready.".to_string(),
            ),
            SessionState::Starting
            | SessionState::Thinking
            | SessionState::Responding
            | SessionState::ToolRunning => {
                let ok = !spec.required_ready;
                let reason = if ok {
                    "Provider slot is active; this is acceptable for an on-demand lane."
                } else {
                    "Required resident provider slot is active and not ready for a new Jarvis phase."
                };
                ("busy", ok, reason.to_string())
            }
            SessionState::SlashMenu | SessionState::Confirming => (
                "blocked",
                false,
                "Provider slot is waiting on an interactive surface or approval prompt."
                    .to_string(),
            ),
            SessionState::Error | SessionState::Exited => {
                if spec.required_ready {
                    (
                        "unavailable",
                        false,
                        "Provider slot is exited or errored.".to_string(),
                    )
                } else {
                    (
                        "not_observed_spawnable",
                        true,
                        "On-demand provider-box slot is exited; provider-box may respawn it when requested.".to_string(),
                    )
                }
            }
        }
    }

    fn jarvis_provider_slot_row(
        spec: &JarvisProviderSlotMonitorSpec,
        info: Option<&crate::pty::PTYAgentInfo>,
    ) -> serde_json::Value {
        let (status, ok, reason) = Self::jarvis_provider_slot_status(info, spec);
        let recognition = info.and_then(|slot| slot.recognition.as_ref());
        let screen_identity = recognition.and_then(|snapshot| snapshot.screen_identity.as_ref());
        serde_json::json!({
            "phase": spec.phase,
            "role": spec.role,
            "provider": &spec.provider,
            "engine": &spec.engine,
            "slot_id": &spec.slot_id,
            "model": &spec.model,
            "model_profile": &spec.model_profile,
            "residency": spec.residency,
            "required_ready": spec.required_ready,
            "critical": spec.critical,
            "ok": ok,
            "status": status,
            "reason": reason,
            "observed": info.map(|slot| serde_json::json!({
                "state": Self::jarvis_slot_state_wire(slot.state),
                "engine": format!("{:?}", slot.engine),
                "pid": slot.pid,
                "status_text": slot.status_text.clone(),
                "started_at": slot.started_at,
                "current_task_id": slot.current_task_id.clone(),
                "log_file": slot.log_file.display().to_string(),
            })),
            "recognition": recognition.map(|snapshot| serde_json::json!({
                "state": format!("{:?}", snapshot.state).to_ascii_lowercase(),
                "reason": snapshot.reason.clone(),
                "phase": snapshot.phase.clone(),
                "blocked_kind": snapshot.blocked_kind.clone(),
                "confidence": snapshot.confidence,
                "source": snapshot.source.clone(),
                "current_model": screen_identity.and_then(|identity| identity.current_model.clone()),
                "reasoning_effort": screen_identity.and_then(|identity| identity.reasoning_effort.clone()),
                "permission_mode": screen_identity.and_then(|identity| identity.permission_mode.clone()),
                "cwd": screen_identity.and_then(|identity| identity.cwd.clone()),
            })),
        })
    }

    async fn jarvis_provider_box_slots_snapshot(
        pty_manager: &PTYManager,
        default_slot: &str,
        intent_author: &JarvisIntentAuthorConfig,
        key_judgment_author: &JarvisKeyJudgmentAuthorConfig,
        plan_author: &JarvisPlanAuthorConfig,
    ) -> serde_json::Value {
        let specs = Self::jarvis_provider_slot_monitor_specs(
            default_slot,
            intent_author,
            key_judgment_author,
            plan_author,
        );
        let mut slots = Vec::with_capacity(specs.len());
        for spec in &specs {
            let info = pty_manager.get_status(&spec.slot_id).await;
            slots.push(Self::jarvis_provider_slot_row(spec, info.as_ref()));
        }
        let mut by_status = std::collections::BTreeMap::<String, usize>::new();
        let mut critical_failures = 0usize;
        let mut blocking_failures = 0usize;
        let mut advisory_failures = 0usize;
        for slot in &slots {
            if let Some(status) = slot.get("status").and_then(|value| value.as_str()) {
                *by_status.entry(status.to_string()).or_insert(0) += 1;
            }
            if slot.get("critical").and_then(|value| value.as_bool()) == Some(true)
                && slot.get("ok").and_then(|value| value.as_bool()) == Some(false)
            {
                critical_failures += 1;
                if slot.get("required_ready").and_then(|value| value.as_bool()) == Some(true) {
                    blocking_failures += 1;
                } else {
                    advisory_failures += 1;
                }
            }
        }
        serde_json::json!({
            "schema": "missiond.jarvis-provider-box-slots.v1",
            "slots": slots,
            "summary": {
                "total": specs.len(),
                "by_status": by_status,
                "critical_failures": critical_failures,
                "blocking_failures": blocking_failures,
                "advisory_failures": advisory_failures,
            }
        })
    }

    fn jarvis_provider_slot_checks(
        provider_box_slots: &serde_json::Value,
    ) -> Vec<serde_json::Value> {
        provider_box_slots
            .get("slots")
            .and_then(|value| value.as_array())
            .map(|slots| {
                slots
                    .iter()
                    .map(|slot| {
                        let phase = slot
                            .get("phase")
                            .and_then(|value| value.as_str())
                            .unwrap_or("unknown");
                        serde_json::json!({
                            "id": format!("provider-slot-{phase}"),
                            "label": format!("Jarvis provider-box slot: {phase}"),
                            "ok": slot.get("ok").and_then(|value| value.as_bool()).unwrap_or(false),
                            "critical": slot.get("critical").and_then(|value| value.as_bool()).unwrap_or(true)
                                && slot.get("required_ready").and_then(|value| value.as_bool()).unwrap_or(false),
                            "phase_critical": slot.get("critical"),
                            "status": slot.get("status"),
                            "slot_id": slot.get("slot_id"),
                            "provider": slot.get("provider"),
                            "engine": slot.get("engine"),
                            "role": slot.get("role"),
                            "residency": slot.get("residency"),
                            "required_ready": slot.get("required_ready"),
                            "reason": slot.get("reason"),
                            "blocked_kind": slot
                                .get("recognition")
                                .and_then(|recognition| recognition.get("blocked_kind")),
                            "recognition_reason": slot
                                .get("recognition")
                                .and_then(|recognition| recognition.get("reason")),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    fn jarvis_auth_secret_readiness() -> serde_json::Value {
        let auth_issuer = std::env::var("MISSIOND_INTERACTION_AUTH_ISSUER")
            .or_else(|_| std::env::var("AUTH_ISSUER"))
            .unwrap_or_else(|_| "https://auth.xiaojinpro.com".to_string());
        let auth_userinfo_url = std::env::var("MISSIOND_INTERACTION_AUTH_USERINFO_URL")
            .unwrap_or_else(|_| "https://auth.xiaojinpro.com/oidc/userinfo".to_string());
        let auth_jwks_url = std::env::var("MISSIOND_AUTH_JWKS_URL")
            .or_else(|_| std::env::var("AUTH_JWKS_URL"))
            .unwrap_or_else(|_| {
                format!(
                    "{}/.well-known/jwks.json",
                    auth_issuer.trim_end_matches('/')
                )
            });
        let service_token_configured = missiond_service_token_configured();
        let unconfigured_api_token_allowed = missiond_unconfigured_api_token_allowed();
        let strict_secrets = secret_store_strict_mode_enabled();
        let legacy_env_fallback_allowed = !strict_secrets;
        let auth_skip_introspection = env_flag("MISSIOND_INTERACTION_AUTH_SKIP_INTROSPECTION");
        let critical_auth = missiond_production_env() || strict_secrets;
        let service_token_mode = if service_token_configured {
            "configured"
        } else if unconfigured_api_token_allowed {
            "compat_unconfigured_allowed_nonprod"
        } else {
            "fail_closed_unconfigured"
        };

        serde_json::json!({
            "schema": "missiond.jarvis-auth-secret-readiness.v1",
            "auth": {
                "issuer": auth_issuer,
                "jwks_url": auth_jwks_url,
                "userinfo_url": auth_userinfo_url,
                "interaction_auth_mode": if auth_skip_introspection { "dev_skip_introspection" } else { "auth_userinfo" },
                "service_token_mode": service_token_mode,
                "service_token_configured": service_token_configured,
                "unconfigured_api_token_allowed": unconfigured_api_token_allowed,
            },
            "secret_store": {
                "strict": strict_secrets,
                "legacy_env_fallback_allowed": legacy_env_fallback_allowed,
                "policy": if strict_secrets { "strict_fail_closed" } else { "compat_warn" },
            },
            "media_upload": {
                "user_flow": "presign_object_upload_complete",
                "jarvis_payload": "image_service_ref_or_artifact_id_only",
                "internal_import_auth": "auth_service_jwt",
                "legacy_internal_token_policy": if strict_secrets { "disabled_in_strict" } else { "compat_migration" },
            },
            "checks": [
                {
                    "id": "auth-issuer-jwks",
                    "label": "Auth issuer and JWKS configured for MissionD/Jarvis",
                    "ok": !auth_issuer.trim().is_empty() && !auth_jwks_url.trim().is_empty(),
                    "critical": critical_auth,
                    "status": if !auth_issuer.trim().is_empty() && !auth_jwks_url.trim().is_empty() { "configured" } else { "missing" },
                    "issuer": auth_issuer,
                    "jwks_url": auth_jwks_url,
                },
                {
                    "id": "missiond-service-token",
                    "label": "MissionD interaction service token policy",
                    "ok": service_token_configured || unconfigured_api_token_allowed,
                    "critical": critical_auth,
                    "status": service_token_mode,
                    "reason": if service_token_configured {
                        "MISSIOND_INTERACTION_SERVICE_TOKEN or MISSIOND_API_TOKEN is configured."
                    } else if unconfigured_api_token_allowed {
                        "Explicit non-production compatibility switch allows unconfigured API token."
                    } else {
                        "MissionD refuses arbitrary bearer tokens when no service token is configured."
                    },
                },
                {
                    "id": "secret-store-strictness",
                    "label": "Secret Store fallback policy",
                    "ok": true,
                    "critical": false,
                    "status": if strict_secrets { "strict_fail_closed" } else { "compat_warn" },
                    "legacy_env_fallback_allowed": legacy_env_fallback_allowed,
                },
                {
                    "id": "media-upload-auth-contract",
                    "label": "Jarvis media upload auth contract",
                    "ok": true,
                    "critical": false,
                    "status": "auth_service_jwt_import",
                    "rule": "iOS uploads media through presign/complete; Jarvis receives only image_service_ref/artifact_id and internal import uses Auth service JWT.",
                }
            ]
        })
    }

    fn expand_home_path(path: &str) -> Option<std::path::PathBuf> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Some(rest) = trimmed.strip_prefix("~/") {
            if let Ok(home) = std::env::var("HOME") {
                return Some(std::path::PathBuf::from(home).join(rest));
            }
        }
        Some(std::path::PathBuf::from(trimmed))
    }

    async fn handle_jarvis_monitor(
        mut stream: TcpStream,
        pty_manager: Arc<PTYManager>,
        default_slot: String,
        jarvis_intent_author: JarvisIntentAuthorConfig,
        jarvis_key_judgment_author: JarvisKeyJudgmentAuthorConfig,
        jarvis_plan_author: JarvisPlanAuthorConfig,
    ) -> anyhow::Result<()> {
        let mut buf = vec![0u8; 4096];
        let _ = stream.read(&mut buf).await;

        let readiness = Self::build_jarvis_readiness(&pty_manager, &default_slot).await;
        let readiness_status = readiness
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let default_slot_status = pty_manager.get_status(&default_slot).await;
        let all_status = pty_manager.get_all_status().await;

        let home = Self::mission_home();
        let mcp_config = home.join("xjp-mcp-config.json");
        let mcp_binary = std::env::var("MISSIOND_MCP_BIN_PATH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| home.join("mission-mcp"));
        let compiled_runtime_dir = Self::compiled_runtime_dir();
        let compiled_runtime = compiled_runtime_dir.join("compiled-runtime-config.json");
        let runtime_topology = Self::jarvis_runtime_topology_snapshot(&compiled_runtime_dir);
        let compiled_abi_freshness =
            Self::compiled_abi_freshness_check_in_dir(&compiled_runtime_dir);
        let auth_secret_readiness = Self::jarvis_auth_secret_readiness();
        let provider_box_slots = Self::jarvis_provider_box_slots_snapshot(
            &pty_manager,
            &default_slot,
            &jarvis_intent_author,
            &jarvis_key_judgment_author,
            &jarvis_plan_author,
        )
        .await;
        let slot_log = default_slot_status
            .as_ref()
            .map(|info| info.log_file.clone())
            .unwrap_or_else(|| home.join(format!("logs/pty-{}.log", default_slot)));
        let slot_screen_available = pty_manager
            .get_screen(&default_slot)
            .await
            .map(|screen| !screen.trim().is_empty())
            .unwrap_or(false);

        let mcp_config_text = std::fs::read_to_string(&mcp_config).unwrap_or_default();
        let mcp_config_ok = mcp_config.exists() && mcp_config_text.contains("\"missiond\"");

        let slot_log_check = if slot_log.exists() {
            Self::file_check("default-slot-log", "Default slot PTY log", slot_log)
        } else {
            serde_json::json!({
                "id": "default-slot-log",
                "label": "Default slot PTY log",
                "ok": slot_screen_available,
                "status": if slot_screen_available { "live_screen_available" } else { "missing" },
                "path": slot_log,
                "reason": if slot_screen_available {
                    "PTY log file is missing, but the default slot live screen is readable."
                } else {
                    "PTY log file is missing and no live screen is available."
                },
            })
        };

        let mut checks = vec![
            serde_json::json!({
                "id": "public-entry",
                "label": "HTTP request reached MissionD daemon",
                "ok": true,
                "status": "ok",
            }),
            serde_json::json!({
                "id": "default-slot-readiness",
                "label": "Default Jarvis slot readiness",
                "ok": readiness_status == "ready",
                "status": readiness_status,
                "reason": readiness.get("reason"),
            }),
            serde_json::json!({
                "id": "mcp-config",
                "label": "Host-local MissionD MCP config",
                "ok": mcp_config_ok,
                "status": if mcp_config_ok { "ok" } else { "missing_or_invalid" },
                "path": mcp_config,
            }),
            Self::file_check("mission-mcp-binary", "MissionD MCP binary", mcp_binary),
            slot_log_check,
            compiled_abi_freshness,
            Self::file_check(
                "compiled-runtime-config",
                "Compiled V3 runtime config",
                compiled_runtime,
            ),
        ];
        checks.extend(Self::jarvis_topology_checks(&runtime_topology).await);
        checks.extend(Self::jarvis_provider_slot_checks(&provider_box_slots));
        if let Some(auth_checks) = auth_secret_readiness
            .get("checks")
            .and_then(|value| value.as_array())
        {
            checks.extend(auth_checks.iter().cloned());
        }

        let provider_slot_failures = checks
            .iter()
            .filter(|check| {
                check
                    .get("id")
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| id.starts_with("provider-slot-"))
            })
            .filter(|check| check.get("ok").and_then(|v| v.as_bool()) == Some(false))
            .filter(|check| check.get("critical").and_then(|v| v.as_bool()) == Some(true))
            .filter(|check| check.get("required_ready").and_then(|v| v.as_bool()) == Some(true))
            .count();
        let critical_failures = checks
            .iter()
            .filter(|check| check.get("ok").and_then(|v| v.as_bool()) == Some(false))
            .filter(|check| check.get("critical").and_then(|v| v.as_bool()) == Some(true))
            .count();
        let non_critical_failures = checks
            .iter()
            .filter(|check| check.get("ok").and_then(|v| v.as_bool()) == Some(false))
            .filter(|check| check.get("critical").and_then(|v| v.as_bool()) != Some(true))
            .count();
        let overall = match readiness_status {
            _ if provider_slot_failures > 0 => "provider_slot_unavailable",
            _ if critical_failures > 0 => "abi_freshness_mismatch",
            "ready" if non_critical_failures == 0 => "ready",
            "ready" => "degraded",
            "busy" => "busy",
            "stale_slot" => "stale_slot",
            "slot_unavailable" | "unavailable" => "unavailable",
            _ => "unknown",
        };
        let recommended_action = match overall {
            "ready" => "none",
            "degraded" => "check failed non-critical monitor rows before the next deploy",
            "busy" => "wait for default slot completion or choose another slot",
            "stale_slot" => "respawn default slot; MissionD spawn now cleans stale PTY sessions",
            "provider_slot_unavailable" => {
                "inspect provider_box_slots; restart or re-auth the blocked Jarvis phase slot before retrying iOS"
            }
            "abi_freshness_mismatch" => {
                "run node scripts/project-v3-contracts.mjs --write, node scripts/compile-v3-runtime.mjs --json, then redeploy MissionD"
            }
            "unavailable" => {
                "start the default Jarvis slot or inspect provider credentials/billing"
            }
            _ => "inspect /api/slots and daemon logs",
        };

        let route_graph = serde_json::json!({
            "schema": "missiond.jarvis-route-graph.v1",
            "public_domain": runtime_topology.get("edge_domain"),
            "edge_node": runtime_topology.get("edge_node"),
            "edge_public_ip": runtime_topology.get("edge_public_ip"),
            "edge_proxy": runtime_topology.get("edge_proxy"),
            "origin_node": runtime_topology.get("origin_node"),
            "origin": runtime_topology.get("origin"),
            "tunnel_server_url": runtime_topology.get("tunnel_server_url"),
            "tunnel_client_id": runtime_topology.get("tunnel_client_id"),
            "target_node": runtime_topology.get("target_node"),
            "target_service": runtime_topology.get("target_service"),
            "route_generation": runtime_topology.get("route_generation"),
            "streaming_policy": runtime_topology.get("streaming_policy"),
            "rule": "GCP direct tunnel mode may be disabled while Jarvis is healthy through the declared GCP Caddy -> BWG tunnel -> Mac mini route."
        });

        let body = crate::evidence_redactor::redact_json_value(&serde_json::json!({
            "schema": "missiond.jarvis-chain-monitor.v2",
            "legacy_schema": "missiond.jarvis-chain-monitor.v1",
            "overall": overall,
            "recommended_action": recommended_action,
            "checked_at": chrono::Utc::now().to_rfc3339(),
            "public_endpoint": "/jarvis",
            "chat_endpoint": "/v1/chat/completions",
            "runtime_topology": runtime_topology,
            "route_graph": route_graph,
            "auth_secret_readiness": auth_secret_readiness,
            "provider_box_slots": provider_box_slots,
            "readiness": readiness,
            "release": Self::release_snapshot(),
            "slots": {
                "default_slot": default_slot,
                "total": all_status.len(),
                "states": all_status.iter().fold(std::collections::BTreeMap::<String, usize>::new(), |mut acc, info| {
                    *acc.entry(format!("{:?}", info.state)).or_insert(0) += 1;
                    acc
                }),
            },
            "checks": checks.drain(..).collect::<Vec<_>>(),
        }))
        .to_string();

        let response = Self::http_json_response(body);
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await?;
        Ok(())
    }

    async fn handle_interaction_events(
        mut stream: TcpStream,
        request_line: &str,
        db: Option<Arc<dyn crate::db::traits::MissionStore>>,
    ) -> anyhow::Result<()> {
        let _ = Self::read_http_request(&mut stream).await;
        let interaction_id = request_line
            .split_whitespace()
            .nth(1)
            .and_then(|path| {
                path.trim_start_matches("/interactions/v1/")
                    .split('/')
                    .next()
            })
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("unknown");
        let sse_headers = "HTTP/1.1 200 OK\r\n\
            Content-Type: text/event-stream\r\n\
            Cache-Control: no-cache\r\n\
            Connection: close\r\n\
            Access-Control-Allow-Origin: *\r\n\
            \r\n";
        stream.write_all(sse_headers.as_bytes()).await?;
        let Some(db) = db else {
            Self::write_sse_event(
                &mut stream,
                "diagnostic",
                &serde_json::json!({
                    "schema": "missiond.interaction-event-stream.v1",
                    "interaction_id": interaction_id,
                    "phase": "replay_unavailable",
                    "error": {
                        "code": "MISSIOND_DB_UNAVAILABLE",
                        "message": "MissionD DB is unavailable; cannot replay interaction ledger."
                    }
                }),
            )
            .await?;
            Self::finish_sse(&mut stream).await?;
            return Ok(());
        };
        let events = db.get_interaction_events(interaction_id, 500).await?;
        Self::write_sse_event(
            &mut stream,
            "status",
            &serde_json::json!({
                "schema": "missiond.interaction-event-stream.v1",
                "interaction_id": interaction_id,
                "phase": "replay_ready",
                "event_count": events.len(),
            }),
        )
        .await?;
        for event in events {
            let payload = event
                .raw_data
                .as_deref()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                .unwrap_or_else(|| {
                    serde_json::json!({
                        "interaction_id": interaction_id,
                        "event_type": event.event_type,
                        "content": event.content,
                        "timestamp": event.timestamp,
                    })
                });
            let event_name = event
                .event_type
                .strip_prefix("interaction.")
                .unwrap_or(event.event_type.as_str());
            Self::write_sse_event(&mut stream, event_name, &payload).await?;
        }
        Self::finish_sse(&mut stream).await?;
        Ok(())
    }

    async fn handle_interaction_messages(
        mut stream: TcpStream,
        addr: SocketAddr,
        pty_manager: Option<Arc<PTYManager>>,
        jarvis_progress_bus: JarvisProgressBus,
        jarvis_intent_author: JarvisIntentAuthorConfig,
        jarvis_key_judgment_author: JarvisKeyJudgmentAuthorConfig,
        jarvis_plan_author: JarvisPlanAuthorConfig,
        jarvis_grounding: JarvisGroundingSlot,
        jarvis_artifact_writer: JarvisArtifactSlot,
        provider_box_http: ProviderBoxHttpSlot,
        db: Option<Arc<dyn crate::db::traits::MissionStore>>,
    ) -> anyhow::Result<()> {
        stream.set_nodelay(true)?;
        let (headers, body) = match Self::read_http_request(&mut stream).await {
            Ok(r) => r,
            Err(e) => {
                let err = serde_json::json!({"error": {"message": format!("Bad request: {}", e)}});
                Self::send_http_error(&mut stream, 400, "Bad Request", &err.to_string()).await?;
                return Ok(());
            }
        };

        let envelope: InteractionEnvelope = match serde_json::from_str(&body) {
            Ok(value) => value,
            Err(e) => {
                let err = serde_json::json!({"error": {"message": format!("Invalid InteractionEnvelope: {}", e)}});
                Self::send_http_error(&mut stream, 400, "Bad Request", &err.to_string()).await?;
                return Ok(());
            }
        };

        Self::handle_interaction_envelope(
            stream,
            addr,
            headers,
            envelope,
            pty_manager,
            jarvis_progress_bus,
            jarvis_intent_author,
            jarvis_key_judgment_author,
            jarvis_plan_author,
            jarvis_grounding,
            jarvis_artifact_writer,
            provider_box_http,
            db,
        )
        .await
    }

    async fn handle_chat_completions_interaction_adapter(
        mut stream: TcpStream,
        addr: SocketAddr,
        pty_manager: Option<Arc<PTYManager>>,
        default_chat_slot: String,
        jarvis_progress_bus: JarvisProgressBus,
        jarvis_intent_author: JarvisIntentAuthorConfig,
        jarvis_key_judgment_author: JarvisKeyJudgmentAuthorConfig,
        jarvis_plan_author: JarvisPlanAuthorConfig,
        jarvis_grounding: JarvisGroundingSlot,
        jarvis_artifact_writer: JarvisArtifactSlot,
        provider_box_http: ProviderBoxHttpSlot,
        db: Option<Arc<dyn crate::db::traits::MissionStore>>,
    ) -> anyhow::Result<()> {
        stream.set_nodelay(true)?;
        let (headers, body) = match Self::read_http_request(&mut stream).await {
            Ok(r) => r,
            Err(e) => {
                let err = serde_json::json!({"error": {"message": format!("Bad request: {}", e)}});
                Self::send_http_error(&mut stream, 400, "Bad Request", &err.to_string()).await?;
                return Ok(());
            }
        };
        let req: serde_json::Value = match serde_json::from_str(&body) {
            Ok(value) => value,
            Err(e) => {
                let err = serde_json::json!({"error": {"message": format!("Invalid JSON: {}", e)}});
                Self::send_http_error(&mut stream, 400, "Bad Request", &err.to_string()).await?;
                return Ok(());
            }
        };
        let follow_task_id = openai_request_follow_task_id(&req);
        if follow_task_id.is_none() {
            if let Err(err) =
                Self::ensure_jarvis_slot_ready_for_chat(pty_manager.as_ref(), &default_chat_slot)
                    .await
            {
                Self::send_http_error(&mut stream, 503, "Service Unavailable", &err.to_string())
                    .await?;
                return Ok(());
            }
        }
        let envelope = openai_request_to_interaction_envelope(&req);
        Self::handle_interaction_envelope(
            stream,
            addr,
            headers,
            envelope,
            pty_manager,
            jarvis_progress_bus,
            jarvis_intent_author,
            jarvis_key_judgment_author,
            jarvis_plan_author,
            jarvis_grounding,
            jarvis_artifact_writer,
            provider_box_http,
            db,
        )
        .await
    }

    async fn handle_interaction_envelope(
        mut stream: TcpStream,
        addr: SocketAddr,
        headers: String,
        mut envelope: InteractionEnvelope,
        _pty_manager: Option<Arc<PTYManager>>,
        jarvis_progress_bus: JarvisProgressBus,
        jarvis_intent_author: JarvisIntentAuthorConfig,
        jarvis_key_judgment_author: JarvisKeyJudgmentAuthorConfig,
        jarvis_plan_author: JarvisPlanAuthorConfig,
        jarvis_grounding: JarvisGroundingSlot,
        jarvis_artifact_writer: JarvisArtifactSlot,
        provider_box_http: ProviderBoxHttpSlot,
        db: Option<Arc<dyn crate::db::traits::MissionStore>>,
    ) -> anyhow::Result<()> {
        stream.set_nodelay(true)?;
        let channel = envelope.channel.trim().to_ascii_lowercase();
        let auth_resolution = match resolve_interaction_auth(&envelope, &headers).await {
            Ok(resolution) => resolution,
            Err((status, reason, body)) => {
                Self::send_http_error(&mut stream, status, reason, &body.to_string()).await?;
                return Ok(());
            }
        };
        let auth_token = auth_resolution.token;
        let permission_context = auth_resolution.permission_context;
        envelope.attachments =
            normalize_interaction_attachments(&envelope.attachments, "interaction-envelope");
        let media_context = interaction_media_context(&envelope.attachments);
        let accepted_attachment_count =
            interaction_accepted_attachment_count(&envelope.attachments);
        let mut raw_user_text = normalize_interaction_message(&envelope.message);
        if raw_user_text.is_empty() && accepted_attachment_count > 0 {
            raw_user_text = "请分析我上传的图片".to_string();
            envelope.message = serde_json::Value::String(raw_user_text.clone());
        }
        if raw_user_text.is_empty() {
            let err = serde_json::json!({
                "error": {
                    "code": "INTERACTION_MESSAGE_OR_MEDIA_REQUIRED",
                    "message": "InteractionEnvelope.message text or an accepted xjp-image-service media attachment is required"
                },
                "media_context": media_context
            });
            Self::send_http_error(&mut stream, 400, "Bad Request", &err.to_string()).await?;
            return Ok(());
        }
        let media_summary = interaction_media_summary_for_objective(&envelope.attachments);

        let interaction_id = interaction_metadata_string(&envelope, "interaction_id")
            .unwrap_or_else(|| format!("ix-{}", uuid::Uuid::new_v4().simple()));
        let chat_id = format!(
            "chatcmpl-interaction-{}",
            chrono::Utc::now().timestamp_millis()
        );
        let sse_headers = "HTTP/1.1 200 OK\r\n\
            Content-Type: text/event-stream\r\n\
            Cache-Control: no-cache\r\n\
            Connection: keep-alive\r\n\
            Access-Control-Allow-Origin: *\r\n\
            \r\n";
        stream.write_all(sse_headers.as_bytes()).await?;
        stream.flush().await?;

        let received = serde_json::json!({
            "schema": "missiond.interaction-envelope.v1",
            "interaction_id": interaction_id,
            "channel": channel,
            "external_user_id": envelope.external_user_id,
            "conversation_id": envelope.conversation_id,
            "message_chars": raw_user_text.chars().count(),
            "attachments_count": envelope.attachments.len(),
            "accepted_attachments_count": accepted_attachment_count,
            "media_context": media_context.clone(),
            "remote_addr": addr.to_string(),
        });
        let authenticated = serde_json::json!({
            "interaction_id": interaction_id,
            "channel": channel,
            "authenticated": auth_token.is_some(),
            "authority": "auth",
        });
        let permission_resolved = serde_json::json!({
            "interaction_id": interaction_id,
            "permission_context": permission_context.clone(),
        });
        Self::write_sse_event(&mut stream, "received", &received).await?;
        Self::write_sse_event(&mut stream, "authenticated", &authenticated).await?;
        Self::write_sse_event(&mut stream, "permission_resolved", &permission_resolved).await?;
        Self::write_jarvis_progress(
            &mut stream,
            &jarvis_progress_bus,
            &chat_id,
            Some(&interaction_id),
            "permission_resolved",
            "auth_permission",
            "completed",
            "已完成身份和权限解析，下一步收集 grounding context。",
            None,
            None,
            None,
        )
        .await?;

        if channel == "wechat" && auth_token.is_none() {
            Self::write_sse_event(
                &mut stream,
                "diagnostic",
                &serde_json::json!({
                    "interaction_id": interaction_id,
                    "phase": "identity_resolution",
                    "error": {
                        "code": "IDENTITY_BINDING_REQUIRED",
                        "message": "WeChat channel must bind openid/unionid to an Auth identity before MissionD can create work."
                    }
                }),
            )
            .await?;
            Self::write_sse_event(
                &mut stream,
                "final",
                &serde_json::json!({
                    "interaction_id": interaction_id,
                    "status": "blocked",
                    "reason": "identity_binding_required",
                }),
            )
            .await?;
            Self::finish_sse(&mut stream).await?;
            return Ok(());
        }

        let conversation_id = envelope.conversation_id.clone();
        let conversation_scope = conversation_scope_from_permission(
            &envelope,
            &permission_context,
            &channel,
            &raw_user_text,
        );
        if let Some(follow_task_id) =
            interaction_metadata_string(&envelope, "missiond_follow_task_id")
        {
            if let Some(ref db) = db {
                Self::write_sse_event(
                    &mut stream,
                    "status",
                    &serde_json::json!({
                        "interaction_id": interaction_id,
                        "phase": "result_followup",
                        "task_id": follow_task_id,
                    }),
                )
                .await?;
                Self::stream_jarvis_task_until_terminal(
                    db,
                    &jarvis_artifact_writer,
                    &provider_box_http,
                    &jarvis_progress_bus,
                    &mut stream,
                    &chat_id,
                    Some(&interaction_id),
                    &follow_task_id,
                    conversation_id.as_deref(),
                )
                .await?;
            } else {
                Self::write_sse_event(
                    &mut stream,
                    "diagnostic",
                    &serde_json::json!({
                        "interaction_id": interaction_id,
                        "phase": "result_followup",
                        "error": {
                            "code": "MISSIOND_DB_UNAVAILABLE",
                            "message": "MissionD DB unavailable; cannot follow BoardTask result."
                        }
                    }),
                )
                .await?;
            }
            Self::finish_sse(&mut stream).await?;
            return Ok(());
        }

        let jarvis_conv_id = if let Some(ref db) = db {
            match Self::resolve_jarvis_conversation_id(
                db,
                conversation_id.as_deref(),
                &raw_user_text,
                &conversation_scope,
            )
            .await
            {
                Ok(id) => {
                    let _ = db
                        .router_chat_append_messages(
                            &id,
                            &[(
                                "user".to_string(),
                                media_summary
                                    .as_ref()
                                    .map(|summary| format!("{raw_user_text}\n\n[media]\n{summary}"))
                                    .unwrap_or_else(|| raw_user_text.clone()),
                            )],
                        )
                        .await;
                    Some(id)
                }
                Err(e) => {
                    warn!(error = %e, "Interaction gateway cannot persist conversation");
                    None
                }
            }
        } else {
            None
        };
        if let Some(ref cid) = jarvis_conv_id {
            Self::persist_interaction_event(
                db.as_ref(),
                Some(cid),
                Some(&interaction_id),
                "received",
                &received,
            )
            .await;
            Self::persist_interaction_event(
                db.as_ref(),
                Some(cid),
                Some(&interaction_id),
                "authenticated",
                &authenticated,
            )
            .await;
            Self::persist_interaction_event(
                db.as_ref(),
                Some(cid),
                Some(&interaction_id),
                "permission_resolved",
                &permission_resolved,
            )
            .await;
            let meta = serde_json::json!({
                "interaction_id": interaction_id,
                "conversation_id": cid,
                "chat_id": chat_id
            });
            Self::write_sse_event(&mut stream, "meta", &meta).await?;
            Self::persist_interaction_event(
                db.as_ref(),
                Some(cid),
                Some(&interaction_id),
                "meta",
                &meta,
            )
            .await;
        }

        if !interaction_metadata_bool(&envelope, "missiond_intent_confirmed")
            && !interaction_metadata_bool(&envelope, "missiond_plan_confirmed")
            && Self::jarvis_text_confirms_pending_review(&raw_user_text)
        {
            if let (Some(ref db), Some(ref cid)) = (&db, &jarvis_conv_id) {
                match Self::load_pending_jarvis_confirmation(db, cid).await {
                    Ok(Some(confirm_payload)) => {
                        Self::inject_jarvis_confirm_payload(
                            &mut envelope.metadata,
                            confirm_payload,
                        );
                    }
                    Ok(None) => {}
                    Err(error) => {
                        warn!(conversation_id = %cid, error = %error, "failed to load pending Jarvis confirmation");
                    }
                }
            }
        }

        let intent_confirmed = interaction_metadata_bool(&envelope, "missiond_intent_confirmed");
        let plan_confirmed = interaction_metadata_bool(&envelope, "missiond_plan_confirmed");
        let mut objective_text = if intent_confirmed || plan_confirmed {
            match interaction_metadata_string(&envelope, "missiond_objective") {
                Some(value) => value,
                None => {
                    Self::fail_jarvis_gate_visible(
                        &mut stream,
                        &jarvis_progress_bus,
                        &chat_id,
                        Some(&interaction_id),
                        "Jarvis confirmation requires missiond_objective from the previous intent/plan payload; refusing to use the confirmation text as the task objective.".to_string(),
                        "confirmation_objective",
                        db.as_ref(),
                        jarvis_conv_id.as_deref(),
                    )
                    .await?;
                    return Ok(());
                }
            }
        } else {
            raw_user_text.clone()
        };

        let confirmed_intent_artifact_id = if intent_confirmed {
            match interaction_metadata_string(&envelope, "missiond_intent_artifact_id") {
                Some(value) => Some(value),
                None => {
                    Self::fail_jarvis_gate_visible(
                        &mut stream,
                        &jarvis_progress_bus,
                        &chat_id,
                        Some(&interaction_id),
                        "Jarvis intent confirmation requires missiond_intent_artifact_id from the previous intent payload; refusing to collect grounding without a confirmed intent.lisp.".to_string(),
                        "confirmation_intent_artifact",
                        db.as_ref(),
                        jarvis_conv_id.as_deref(),
                    )
                    .await?;
                    return Ok(());
                }
            }
        } else {
            None
        };
        let confirmed_plan_artifact_id = if plan_confirmed {
            match interaction_metadata_string(&envelope, "missiond_plan_artifact_id") {
                Some(value) => Some(value),
                None => {
                    Self::fail_jarvis_gate_visible(
                        &mut stream,
                        &jarvis_progress_bus,
                        &chat_id,
                        Some(&interaction_id),
                        "Jarvis plan confirmation requires missiond_plan_artifact_id from the previous plan payload; refusing to execute without a confirmed plan.lisp.".to_string(),
                        "confirmation_plan_artifact",
                        db.as_ref(),
                        jarvis_conv_id.as_deref(),
                    )
                    .await?;
                    return Ok(());
                }
            }
        } else {
            None
        };

        let grounding_was_collected = intent_confirmed && !plan_confirmed;
        let grounding = if plan_confirmed {
            match Self::jarvis_grounding_from_interaction_metadata(&envelope, &conversation_scope) {
                Ok(result) => result,
                Err(error) => {
                    Self::fail_jarvis_gate_visible(
                        &mut stream,
                        &jarvis_progress_bus,
                        &chat_id,
                        Some(&interaction_id),
                        error,
                        "confirmation_grounding",
                        db.as_ref(),
                        jarvis_conv_id.as_deref(),
                    )
                    .await?;
                    return Ok(());
                }
            }
        } else if intent_confirmed {
            Self::write_jarvis_progress(
                &mut stream,
                &jarvis_progress_bus,
                &chat_id,
                Some(&interaction_id),
                "grounding",
                "context_gather_start",
                "running",
                "intent.lisp 已确认，正在调用挂载 MissionD MCP 的 ClaudeCode 工位收集上下游全链上下文。",
                None,
                None,
                Some("claude-code-mcp-grounding"),
            )
            .await?;
            let result = match Self::gather_jarvis_grounding_with_progress(
                &mut stream,
                &jarvis_progress_bus,
                &chat_id,
                Some(&interaction_id),
                &jarvis_grounding,
                JarvisGroundingRequest {
                    query: objective_text.clone(),
                    confirmed_intent_artifact_id: confirmed_intent_artifact_id.clone(),
                    confirmed_intent_lisp: interaction_metadata_string(
                        &envelope,
                        "missiond_intent_artifact_body",
                    ),
                    conversation_id: jarvis_conv_id.clone(),
                    chat_id: chat_id.clone(),
                    user_id: conversation_scope.user_id.clone(),
                    tenant_id: conversation_scope.tenant_id.clone(),
                    application_id: conversation_scope.application_id.clone(),
                    channel: Some(conversation_scope.channel.clone()),
                    topic_id: conversation_scope.topic_id.clone(),
                    topic_label: conversation_scope.topic_label.clone(),
                    permission_context: permission_context.clone(),
                    media_context: media_context.clone(),
                    unknowns: vec![
                        "Collect MissionD upstream/downstream facts that affect this confirmed intent.".to_string(),
                        "Identify project registry, SSOT, runtime, skill, provider, infra, and permission evidence needed before plan.lisp.".to_string(),
                        "Write the grounded evidence report for the plan author instead of creating BoardTask or implementing changes.".to_string(),
                        "If media_context has accepted image attachments, include their xjp-image-service refs and any available dimensions/hash in the grounding report; never ask for or persist inline base64.".to_string(),
                    ],
                },
            )
            .await
            {
                Ok(result) => result,
                Err(error) => {
                    Self::fail_jarvis_gate_visible(
                        &mut stream,
                        &jarvis_progress_bus,
                        &chat_id,
                        Some(&interaction_id),
                        error,
                        "grounding",
                        db.as_ref(),
                        jarvis_conv_id.as_deref(),
                    )
                    .await?;
                    return Ok(());
                }
            };
            Self::write_jarvis_progress(
                &mut stream,
                &jarvis_progress_bus,
                &chat_id,
                Some(&interaction_id),
                "grounding",
                "context_gather_completed",
                "completed",
                &format!(
                    "grounding 已完成，context={}，下一步进入 plan gate。",
                    result.grounding_context_id
                ),
                None,
                result.grounding_worker_slot_id.as_deref(),
                Some("claude-code-mcp-grounding"),
            )
            .await?;
            result
        } else {
            Self::jarvis_pending_grounding_result(&conversation_scope)
        };
        let mut grounding_context_id = grounding.grounding_context_id.clone();
        let mut context_pack_path = grounding.context_pack_path.clone();
        let mut context_pack_file = grounding.context_pack_file.clone();
        let mut grounding_report_file = grounding.grounding_report_file.clone();
        let mut grounding_report_artifact_path = grounding.grounding_report_artifact_path.clone();
        let mut grounding_report_hash = grounding.grounding_report_hash.clone();
        let mut grounding_worker_slot_id = grounding.grounding_worker_slot_id.clone();
        let mut grounding_worker_turn_id = grounding.grounding_worker_turn_id.clone();
        let mut context_sufficiency = grounding.context_sufficiency.clone();
        let mut grounding_artifact_hash = grounding.artifact_hash.clone();
        let mut context_capsule_hash = grounding.context_capsule_hash.clone();
        let mut context_capsule_file = grounding.context_capsule_file.clone();
        let mut resolved_topic_id = grounding
            .topic_id
            .clone()
            .or_else(|| conversation_scope.topic_id.clone());
        let mut resolved_topic_label = grounding
            .topic_label
            .clone()
            .or_else(|| conversation_scope.topic_label.clone());
        let mut sources_used = grounding.sources_used.clone();
        let mut grounding_diagnostics = grounding.diagnostics.clone();
        if let (Some(ref db), Some(ref cid), Some(ref capsule_hash)) =
            (&db, &jarvis_conv_id, &context_capsule_hash)
        {
            let _ = db
                .bind_context_capsule(
                    cid,
                    capsule_hash,
                    resolved_topic_id.as_deref(),
                    resolved_topic_label.as_deref(),
                )
                .await;
        }
        if grounding_was_collected {
            let grounding_ledger_event = serde_json::json!({
                "interaction_id": interaction_id,
                "phase": "grounding",
                "grounding_context_id": grounding_context_id,
                "context_pack_path": context_pack_path,
                "context_pack_file": context_pack_file,
                "grounding_report_file": grounding_report_file,
                "grounding_report_artifact_path": grounding_report_artifact_path,
                "grounding_report_hash": grounding_report_hash,
                "grounding_worker_slot_id": grounding_worker_slot_id,
                "grounding_worker_turn_id": grounding_worker_turn_id,
                "context_sufficiency": context_sufficiency,
                "artifact_hash": grounding_artifact_hash,
                "context_capsule_hash": context_capsule_hash,
                "context_capsule_file": context_capsule_file,
                "topic_id": resolved_topic_id,
                "topic_label": resolved_topic_label,
                "sources_used": sources_used,
                "media_context": media_context.clone(),
                "diagnostics": grounding_diagnostics,
            });
            Self::write_sse_event(&mut stream, "grounding", &grounding_ledger_event).await?;
            Self::persist_interaction_event(
                db.as_ref(),
                jarvis_conv_id.as_deref(),
                Some(&interaction_id),
                "grounding",
                &grounding_ledger_event,
            )
            .await;
        }

        let intent_artifact_id = if !intent_confirmed {
            let authored_intent = match Self::author_jarvis_intent_draft_with_progress(
                &mut stream,
                &jarvis_progress_bus,
                &chat_id,
                Some(&interaction_id),
                &provider_box_http,
                &jarvis_intent_author,
                "missiond.interaction-intent-artifact.v1",
                &channel,
                &objective_text,
                &grounding_context_id,
                resolved_topic_id.as_deref(),
                resolved_topic_label.as_deref(),
                &sources_used,
                Some(&permission_context),
                &media_context,
            )
            .await
            {
                Ok(draft) => draft,
                Err(error) => {
                    Self::fail_jarvis_gate_visible(
                        &mut stream,
                        &jarvis_progress_bus,
                        &chat_id,
                        Some(&interaction_id),
                        format!(
                            "intent.lisp 生成失败：{error}。不会用 Rust fallback 代替你的意图识别。"
                        ),
                        "intent_authoring_failed",
                        db.as_ref(),
                        jarvis_conv_id.as_deref(),
                    )
                    .await?;
                    return Ok(());
                }
            };
            objective_text = authored_intent.objective.clone();
            let confirmation_required = jarvis_intent_plan_confirmation_required();
            let intent_payload = serde_json::json!({
                "schema": "missiond.interaction-intent-artifact.v1",
                "interaction_id": interaction_id,
                "channel": &channel,
                "phase": if confirmation_required { "intent_draft" } else { "intent_archived" },
                "author": "codex-cli-gpt-5.5-xhigh",
                "intent_author_slot_id": &jarvis_intent_author.slot_id,
                "intent_kind": authored_intent.intent_kind,
                "confidence": authored_intent.confidence,
                "grounding_context_id": grounding_context_id,
                "grounding_status": if confirmation_required { "pending_intent_confirmation" } else { "intent_archived_pending_grounding" },
                "context_pack_path": context_pack_path,
                "context_pack_file": context_pack_file,
                "grounding_report_file": grounding_report_file,
                "grounding_report_artifact_path": grounding_report_artifact_path,
                "grounding_report_hash": grounding_report_hash,
                "context_sufficiency": context_sufficiency,
                "context_capsule_hash": context_capsule_hash,
                "context_capsule_file": context_capsule_file,
                "topic_id": resolved_topic_id,
                "topic_label": resolved_topic_label,
                "permission_context": permission_context.clone(),
                "media_context": media_context.clone(),
                "understanding": authored_intent.understanding,
                "objective": objective_text,
                "original_user_message": &raw_user_text,
                "user_message_preview": raw_user_text.chars().take(240).collect::<String>(),
                "review_text": authored_intent.review_text,
                "artifact_language": "lisp",
                "artifact_body": authored_intent.artifact_body,
                "assumptions": authored_intent.assumptions,
                "non_goals": authored_intent.non_goals,
                "acceptance_signals": authored_intent.acceptance_signals,
                "sources_used": sources_used,
                "requires_confirmation": confirmation_required,
                "visibility": if confirmation_required { "review" } else { "archive_only" }
            });
            Self::write_jarvis_progress(
                &mut stream,
                &jarvis_progress_bus,
                &chat_id,
                Some(&interaction_id),
                "intent_artifact",
                "shared_artifact_put",
                "running",
                if confirmation_required {
                    "正在写入 intent.lisp artifact，写入完成后页面会显示可确认内容。"
                } else {
                    "正在归档 intent.lisp artifact；内容只用于存档和回放分析，不弹确认卡。"
                },
                None,
                None,
                None,
            )
            .await?;
            let intent_artifact = match Self::put_jarvis_artifact(
                &jarvis_artifact_writer,
                JarvisArtifactRequest {
                    kind: "interaction-intent-draft".to_string(),
                    project_id: None,
                    task_id: None,
                    payload: intent_payload.clone(),
                    metadata: serde_json::json!({
                        "schema": "missiond.interaction-intent-artifact.v1",
                        "interaction_id": interaction_id,
                        "channel": &channel,
                        "conversation_id": jarvis_conv_id,
                        "grounding_context_id": grounding_context_id,
                        "media_context": media_context.clone(),
                    }),
                },
            )
            .await
            {
                Ok(result) => result,
                Err(error) => {
                    Self::fail_jarvis_gate_visible(
                        &mut stream,
                        &jarvis_progress_bus,
                        &chat_id,
                        Some(&interaction_id),
                        error,
                        "intent_artifact",
                        db.as_ref(),
                        jarvis_conv_id.as_deref(),
                    )
                    .await?;
                    return Ok(());
                }
            };
            let intent_artifact_id = intent_artifact.artifact_id.clone();
            Self::write_jarvis_progress(
                &mut stream,
                &jarvis_progress_bus,
                &chat_id,
                Some(&interaction_id),
                "intent_artifact",
                "shared_artifact_put_completed",
                "completed",
                if confirmation_required {
                    "intent.lisp artifact 已写入，正在发送草案卡片和确认请求。"
                } else {
                    "intent.lisp artifact 已归档，下一步收集 MissionD grounding context。"
                },
                None,
                None,
                None,
            )
            .await?;
            let mut intent = intent_payload;
            if let Some(object) = intent.as_object_mut() {
                object.insert(
                    "intent_artifact_id".to_string(),
                    serde_json::Value::String(intent_artifact_id.clone()),
                );
                object.insert(
                    "intent_artifact_hash".to_string(),
                    serde_json::Value::String(intent_artifact.artifact_hash.clone()),
                );
                object.insert(
                    "intent_artifact_path".to_string(),
                    serde_json::Value::String(intent_artifact.path.clone()),
                );
            }
            let intent_event_name = if confirmation_required {
                "intent_draft"
            } else {
                "intent_archived"
            };
            Self::write_sse_event(&mut stream, intent_event_name, &intent).await?;
            Self::persist_interaction_event(
                db.as_ref(),
                jarvis_conv_id.as_deref(),
                Some(&interaction_id),
                intent_event_name,
                &intent,
            )
            .await;
            if confirmation_required {
                Self::write_sse_openai_missiond_projection(
                    &mut stream,
                    &chat_id,
                    "intent_draft",
                    &intent_artifact_id,
                    &intent_artifact.artifact_hash,
                    &intent_artifact.path,
                )
                .await?;
            }
            if !confirmation_required {
                Self::write_jarvis_progress(
                    &mut stream,
                    &jarvis_progress_bus,
                    &chat_id,
                    Some(&interaction_id),
                    "grounding",
                    "context_gather_start",
                    "running",
                    "intent.lisp 已归档，正在调用挂载 MissionD MCP 的 ClaudeCode 工位收集上下游全链上下文。",
                    None,
                    None,
                    Some("claude-code-mcp-grounding"),
                )
                .await?;
                let result = match Self::gather_jarvis_grounding_with_progress(
                    &mut stream,
                    &jarvis_progress_bus,
                    &chat_id,
                    Some(&interaction_id),
                    &jarvis_grounding,
                    JarvisGroundingRequest {
                        query: objective_text.clone(),
                        confirmed_intent_artifact_id: Some(intent_artifact_id.clone()),
                        confirmed_intent_lisp: intent
                            .get("artifact_body")
                            .and_then(|value| value.as_str())
                            .map(ToOwned::to_owned),
                        conversation_id: jarvis_conv_id.clone(),
                        chat_id: chat_id.clone(),
                        user_id: conversation_scope.user_id.clone(),
                        tenant_id: conversation_scope.tenant_id.clone(),
                        application_id: conversation_scope.application_id.clone(),
                        channel: Some(conversation_scope.channel.clone()),
                        topic_id: conversation_scope.topic_id.clone(),
                        topic_label: conversation_scope.topic_label.clone(),
                        permission_context: permission_context.clone(),
                        media_context: media_context.clone(),
                        unknowns: vec![
                            "Collect MissionD upstream/downstream facts that affect this archived intent.".to_string(),
                            "Identify project registry, SSOT, runtime, skill, provider, infra, and permission evidence needed before plan.lisp.".to_string(),
                            "Write the grounded evidence report for the plan author instead of creating BoardTask or implementing changes.".to_string(),
                            "If media_context has accepted image attachments, include their xjp-image-service refs and any available dimensions/hash in the grounding report; never ask for or persist inline base64.".to_string(),
                        ],
                    },
                )
                .await
                {
                    Ok(result) => result,
                    Err(error) => {
                        Self::fail_jarvis_gate_visible(
                            &mut stream,
                            &jarvis_progress_bus,
                            &chat_id,
                            Some(&interaction_id),
                            error,
                            "grounding",
                            db.as_ref(),
                            jarvis_conv_id.as_deref(),
                        )
                        .await?;
                        return Ok(());
                    }
                };
                Self::write_jarvis_progress(
                    &mut stream,
                    &jarvis_progress_bus,
                    &chat_id,
                    Some(&interaction_id),
                    "grounding",
                    "context_gather_completed",
                    "completed",
                    &format!(
                        "grounding 已完成，context={}，下一步进入关键判断和 plan authoring。",
                        result.grounding_context_id
                    ),
                    None,
                    result.grounding_worker_slot_id.as_deref(),
                    Some("claude-code-mcp-grounding"),
                )
                .await?;
                grounding_context_id = result.grounding_context_id.clone();
                context_pack_path = result.context_pack_path.clone();
                context_pack_file = result.context_pack_file.clone();
                grounding_report_file = result.grounding_report_file.clone();
                grounding_report_artifact_path = result.grounding_report_artifact_path.clone();
                grounding_report_hash = result.grounding_report_hash.clone();
                grounding_worker_slot_id = result.grounding_worker_slot_id.clone();
                grounding_worker_turn_id = result.grounding_worker_turn_id.clone();
                context_sufficiency = result.context_sufficiency.clone();
                grounding_artifact_hash = result.artifact_hash.clone();
                context_capsule_hash = result.context_capsule_hash.clone();
                context_capsule_file = result.context_capsule_file.clone();
                resolved_topic_id = result
                    .topic_id
                    .clone()
                    .or_else(|| conversation_scope.topic_id.clone());
                resolved_topic_label = result
                    .topic_label
                    .clone()
                    .or_else(|| conversation_scope.topic_label.clone());
                sources_used = result.sources_used.clone();
                grounding_diagnostics = result.diagnostics.clone();
                if let (Some(ref db), Some(ref cid), Some(ref capsule_hash)) =
                    (&db, &jarvis_conv_id, &context_capsule_hash)
                {
                    let _ = db
                        .bind_context_capsule(
                            cid,
                            capsule_hash,
                            resolved_topic_id.as_deref(),
                            resolved_topic_label.as_deref(),
                        )
                        .await;
                }
                let grounding_ledger_event = serde_json::json!({
                    "interaction_id": interaction_id,
                    "phase": "grounding",
                    "grounding_context_id": grounding_context_id,
                    "context_pack_path": context_pack_path,
                    "context_pack_file": context_pack_file,
                    "grounding_report_file": grounding_report_file,
                    "grounding_report_artifact_path": grounding_report_artifact_path,
                    "grounding_report_hash": grounding_report_hash,
                    "grounding_worker_slot_id": grounding_worker_slot_id,
                    "grounding_worker_turn_id": grounding_worker_turn_id,
                    "context_sufficiency": context_sufficiency,
                    "artifact_hash": grounding_artifact_hash,
                    "context_capsule_hash": context_capsule_hash,
                    "context_capsule_file": context_capsule_file,
                    "topic_id": resolved_topic_id,
                    "topic_label": resolved_topic_label,
                    "sources_used": sources_used,
                    "media_context": media_context.clone(),
                    "diagnostics": grounding_diagnostics,
                });
                Self::write_sse_event(&mut stream, "grounding", &grounding_ledger_event).await?;
                Self::persist_interaction_event(
                    db.as_ref(),
                    jarvis_conv_id.as_deref(),
                    Some(&interaction_id),
                    "grounding",
                    &grounding_ledger_event,
                )
                .await;
            }
            if !confirmation_required {
                intent_artifact_id
            } else {
                let confirm = serde_json::json!({
                    "interaction_id": interaction_id,
                    "phase": "awaiting_intent_confirmation",
                    "confirmation_type": "intent",
                    "confirm_payload": {
                        "missiond_intent_confirmed": true,
                        "missiond_objective": objective_text,
                        "missiond_grounding_context_id": grounding_context_id,
                        "missiond_intent_artifact_id": intent_artifact_id,
                    }
                });
                Self::write_sse_event(&mut stream, "confirm_required", &confirm).await?;
                Self::persist_interaction_event(
                    db.as_ref(),
                    jarvis_conv_id.as_deref(),
                    Some(&interaction_id),
                    "confirm_required",
                    &confirm,
                )
                .await;
                Self::persist_jarvis_pending_confirmation(
                    db.as_ref(),
                    jarvis_conv_id.as_deref(),
                    &confirm,
                )
                .await;
                Self::write_sse_openai_text_and_persist(
                    &mut stream,
                    &chat_id,
                    "我已生成 intent.lisp 草案，等待你确认意图。",
                    Some("stop"),
                    db.as_ref(),
                    jarvis_conv_id.as_deref(),
                )
                .await?;
                Self::finish_sse(&mut stream).await?;
                return Ok(());
            }
        } else {
            confirmed_intent_artifact_id.clone().unwrap_or_default()
        };

        if intent_confirmed && !plan_confirmed {
            Self::persist_jarvis_confirmation_fulfilled(
                db.as_ref(),
                jarvis_conv_id.as_deref(),
                "intent",
            )
            .await;
        }

        let key_judgment_ref = if plan_confirmed {
            match Self::jarvis_key_judgment_from_interaction_metadata(&envelope) {
                Ok(result) => result,
                Err(error) => {
                    Self::fail_jarvis_gate_visible(
                        &mut stream,
                        &jarvis_progress_bus,
                        &chat_id,
                        Some(&interaction_id),
                        error,
                        "confirmation_key_judgment",
                        db.as_ref(),
                        jarvis_conv_id.as_deref(),
                    )
                    .await?;
                    return Ok(());
                }
            }
        } else {
            let authored_key_judgment = match Self::author_jarvis_key_judgment_draft_with_progress(
                &mut stream,
                &jarvis_progress_bus,
                &chat_id,
                Some(&interaction_id),
                &provider_box_http,
                &jarvis_key_judgment_author,
                "missiond.interaction-key-judgment.v1",
                &channel,
                &objective_text,
                &grounding_context_id,
                &intent_artifact_id,
                resolved_topic_id.as_deref(),
                resolved_topic_label.as_deref(),
                &sources_used,
                Some(&permission_context),
                context_pack_path.as_deref(),
                context_pack_file.as_deref(),
                grounding_report_file.as_deref(),
                grounding_report_artifact_path.as_deref(),
                grounding_report_hash.as_deref(),
                context_sufficiency.as_deref(),
            )
            .await
            {
                Ok(draft) => draft,
                Err(error) => {
                    Self::fail_jarvis_gate_visible(
                        &mut stream,
                        &jarvis_progress_bus,
                        &chat_id,
                        Some(&interaction_id),
                        format!("关键判断生成失败：{error}。不会用 Rust fallback 代替判断。"),
                        "key_judgment_authoring_failed",
                        db.as_ref(),
                        jarvis_conv_id.as_deref(),
                    )
                    .await?;
                    return Ok(());
                }
            };
            let key_payload = serde_json::json!({
                "schema": "missiond.interaction-key-judgment.v1",
                "interaction_id": interaction_id,
                "channel": &channel,
                "phase": "key_judgment_draft",
                "author": "codex-cli-gpt-5.5-xhigh",
                "key_judgment_author_slot_id": &jarvis_key_judgment_author.slot_id,
                "confidence": authored_key_judgment.confidence,
                "grounding_context_id": grounding_context_id,
                "context_pack_path": context_pack_path,
                "context_pack_file": context_pack_file,
                "grounding_report_file": grounding_report_file,
                "grounding_report_artifact_path": grounding_report_artifact_path,
                "grounding_report_hash": grounding_report_hash,
                "grounding_worker_slot_id": grounding_worker_slot_id,
                "grounding_worker_turn_id": grounding_worker_turn_id,
                "context_sufficiency": context_sufficiency,
                "topic_id": resolved_topic_id,
                "topic_label": resolved_topic_label,
                "intent_artifact_id": intent_artifact_id,
                "media_context": media_context.clone(),
                "objective": objective_text,
                "judgment": authored_key_judgment.judgment,
                "review_text": authored_key_judgment.review_text,
                "rejected_hypotheses": authored_key_judgment.rejected_hypotheses,
                "evidence_refs": authored_key_judgment.evidence_refs,
                "planning_implications": authored_key_judgment.planning_implications,
                "acceptance_focus": authored_key_judgment.acceptance_focus,
                "artifact_language": "lisp",
                "artifact_body": authored_key_judgment.artifact_body,
                "sources_used": sources_used,
                "requires_confirmation": false
            });
            Self::write_jarvis_progress(
                &mut stream,
                &jarvis_progress_bus,
                &chat_id,
                Some(&interaction_id),
                "key_judgment_artifact",
                "shared_artifact_put",
                "running",
                "正在写入 key judgment artifact，写入完成后交给 plan author。",
                None,
                None,
                None,
            )
            .await?;
            let key_artifact = match Self::put_jarvis_artifact(
                &jarvis_artifact_writer,
                JarvisArtifactRequest {
                    kind: "interaction-key-judgment".to_string(),
                    project_id: None,
                    task_id: None,
                    payload: key_payload.clone(),
                    metadata: serde_json::json!({
                        "schema": "missiond.interaction-key-judgment.v1",
                        "interaction_id": interaction_id,
                        "channel": &channel,
                        "conversation_id": jarvis_conv_id,
                        "grounding_context_id": grounding_context_id,
                        "intent_artifact_id": intent_artifact_id,
                        "media_context": media_context.clone(),
                    }),
                },
            )
            .await
            {
                Ok(result) => result,
                Err(error) => {
                    Self::fail_jarvis_gate_visible(
                        &mut stream,
                        &jarvis_progress_bus,
                        &chat_id,
                        Some(&interaction_id),
                        error,
                        "key_judgment_artifact",
                        db.as_ref(),
                        jarvis_conv_id.as_deref(),
                    )
                    .await?;
                    return Ok(());
                }
            };
            Self::write_jarvis_progress(
                &mut stream,
                &jarvis_progress_bus,
                &chat_id,
                Some(&interaction_id),
                "key_judgment_artifact",
                "shared_artifact_put_completed",
                "completed",
                "key judgment artifact 已写入，下一步进入 plan authoring。",
                None,
                None,
                None,
            )
            .await?;
            let mut key_event = key_payload;
            if let Some(object) = key_event.as_object_mut() {
                object.insert(
                    "key_judgment_artifact_id".to_string(),
                    serde_json::Value::String(key_artifact.artifact_id.clone()),
                );
                object.insert(
                    "key_judgment_artifact_hash".to_string(),
                    serde_json::Value::String(key_artifact.artifact_hash.clone()),
                );
                object.insert(
                    "key_judgment_artifact_path".to_string(),
                    serde_json::Value::String(key_artifact.path.clone()),
                );
            }
            Self::write_sse_event(&mut stream, "key_judgment_draft", &key_event).await?;
            Self::persist_interaction_event(
                db.as_ref(),
                jarvis_conv_id.as_deref(),
                Some(&interaction_id),
                "key_judgment_draft",
                &key_event,
            )
            .await;
            Self::write_sse_openai_missiond_projection(
                &mut stream,
                &chat_id,
                "key_judgment_draft",
                &key_artifact.artifact_id,
                &key_artifact.artifact_hash,
                &key_artifact.path,
            )
            .await?;
            JarvisKeyJudgmentArtifactRef {
                artifact_id: key_artifact.artifact_id,
                artifact_hash: Some(key_artifact.artifact_hash),
                artifact_path: Some(key_artifact.path),
                judgment: key_event
                    .get("judgment")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string(),
                review_text: key_event
                    .get("review_text")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned),
                confidence: key_event
                    .get("confidence")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned),
                rejected_hypotheses: key_event
                    .get("rejected_hypotheses")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
                evidence_refs: key_event
                    .get("evidence_refs")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
                planning_implications: key_event
                    .get("planning_implications")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
                acceptance_focus: key_event
                    .get("acceptance_focus")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        };

        let mut generated_plan_atomization_graph: Option<serde_json::Value> = None;
        let mut generated_execution_mode: Option<String> = None;
        let mut generated_requires_board_task: Option<bool> = None;
        let mut generated_direct_answer_draft: Option<String> = None;
        let plan_artifact_id = if !plan_confirmed {
            let authored_plan = match Self::author_jarvis_plan_draft_with_progress(
                &mut stream,
                &jarvis_progress_bus,
                &chat_id,
                Some(&interaction_id),
                &provider_box_http,
                &jarvis_plan_author,
                "missiond.interaction-plan-artifact.v1",
                &channel,
                &objective_text,
                &grounding_context_id,
                &intent_artifact_id,
                &key_judgment_ref,
                resolved_topic_id.as_deref(),
                resolved_topic_label.as_deref(),
                &sources_used,
                Some(&permission_context),
                context_pack_path.as_deref(),
                context_pack_file.as_deref(),
                grounding_report_file.as_deref(),
                grounding_report_artifact_path.as_deref(),
                grounding_report_hash.as_deref(),
                grounding_worker_slot_id.as_deref(),
                grounding_worker_turn_id.as_deref(),
                context_sufficiency.as_deref(),
            )
            .await
            {
                Ok(draft) => draft,
                Err(error) => {
                    let diagnostic = serde_json::json!({
                        "phase": "plan_authoring_failed",
                        "error": {
                            "code": "JARVIS_PLAN_AUTHOR_FAILED",
                            "message": error.to_string()
                        }
                    });
                    Self::write_sse_event(&mut stream, "diagnostic", &diagnostic).await?;
                    Self::fail_jarvis_gate_visible(
                        &mut stream,
                        &jarvis_progress_bus,
                        &chat_id,
                        Some(&interaction_id),
                        format!("plan.lisp 生成失败：{error}。plan.lisp 需要 Codex CLI GPT-5.5 xhigh 工位生成；当前工位不可用或输出未通过校验，已停止，不会用 Rust fallback 代替你的计划生成。"),
                        "plan_authoring_failed",
                        db.as_ref(),
                        jarvis_conv_id.as_deref(),
                    )
                    .await?;
                    return Ok(());
                }
            };
            let plan_objective_text = authored_plan.objective.clone();
            let plan_review_text = authored_plan.review_text.clone();
            let plan_artifact_body = authored_plan.artifact_body.clone();
            let plan_steps = authored_plan.steps.clone();
            let confirmation_required = jarvis_intent_plan_confirmation_required();
            let plan_payload = serde_json::json!({
                "schema": "missiond.interaction-plan-artifact.v1",
                "interaction_id": interaction_id,
                "channel": &channel,
                "phase": if confirmation_required { "plan_draft" } else { "plan_archived" },
                "author": "codex-cli-gpt-5.5-xhigh",
                "plan_author_slot_id": &jarvis_plan_author.slot_id,
                "confidence": authored_plan.confidence,
                "grounding_context_id": grounding_context_id,
                "context_pack_path": context_pack_path,
                "context_pack_file": context_pack_file,
                "grounding_report_file": grounding_report_file,
                "grounding_report_artifact_path": grounding_report_artifact_path,
                "grounding_report_hash": grounding_report_hash,
                "grounding_worker_slot_id": grounding_worker_slot_id,
                "grounding_worker_turn_id": grounding_worker_turn_id,
                "context_sufficiency": context_sufficiency,
                "grounding_artifact_hash": grounding_artifact_hash,
                "context_capsule_hash": context_capsule_hash,
                "context_capsule_file": context_capsule_file,
                "topic_id": resolved_topic_id,
                "topic_label": resolved_topic_label,
                "intent_artifact_id": intent_artifact_id,
                "media_context": media_context.clone(),
                "key_judgment_artifact_id": key_judgment_ref.artifact_id,
                "key_judgment_artifact_hash": key_judgment_ref.artifact_hash,
                "key_judgment_artifact_path": key_judgment_ref.artifact_path,
                "key_judgment": key_judgment_ref.judgment,
                "key_judgment_review_text": key_judgment_ref.review_text,
                "key_judgment_confidence": key_judgment_ref.confidence,
                "key_judgment_rejected_hypotheses": key_judgment_ref.rejected_hypotheses,
                "key_judgment_evidence_refs": key_judgment_ref.evidence_refs,
                "key_judgment_planning_implications": key_judgment_ref.planning_implications,
                "key_judgment_acceptance_focus": key_judgment_ref.acceptance_focus,
                "objective": plan_objective_text,
                "review_text": plan_review_text,
                "execution_mode": authored_plan.execution_mode,
                "requires_board_task": authored_plan.requires_board_task,
                "answer_policy": authored_plan.answer_policy,
                "provider_hint": authored_plan.provider_hint,
                "plan_key_judgment": authored_plan.key_judgment,
                "artifact_language": "lisp",
                "artifact_body": plan_artifact_body,
                "steps": plan_steps,
                "direct_answer_draft": authored_plan.direct_answer_draft,
                "workstreams": authored_plan.workstreams,
                "atom_tasks": authored_plan.atom_tasks,
                "dependency_edges": authored_plan.dependency_edges,
                "serial_groups": authored_plan.serial_groups,
                "parallel_groups": authored_plan.parallel_groups,
                "assignment_policy": authored_plan.assignment_policy,
                "atomization_graph": authored_plan.atomization_graph,
                "boundary": authored_plan.boundary,
                "assumptions": authored_plan.assumptions,
                "non_goals": authored_plan.non_goals,
                "acceptance_signals": authored_plan.acceptance_signals,
                "sources_used": sources_used,
                "requires_confirmation": confirmation_required,
                "visibility": if confirmation_required { "review" } else { "archive_only" }
            });
            Self::write_jarvis_progress(
                &mut stream,
                &jarvis_progress_bus,
                &chat_id,
                Some(&interaction_id),
                "plan_artifact",
                "shared_artifact_put",
                "running",
                if confirmation_required {
                    "正在写入 plan.lisp artifact，写入完成后页面会显示可确认计划。"
                } else {
                    "正在归档 plan.lisp artifact；内容只用于存档和回放分析，不弹确认卡。"
                },
                None,
                None,
                None,
            )
            .await?;
            let plan_artifact = match Self::put_jarvis_artifact(
                &jarvis_artifact_writer,
                JarvisArtifactRequest {
                    kind: "interaction-plan-draft".to_string(),
                    project_id: None,
                    task_id: None,
                    payload: plan_payload.clone(),
                    metadata: serde_json::json!({
                        "schema": "missiond.interaction-plan-artifact.v1",
                        "interaction_id": interaction_id,
                        "channel": &channel,
                        "grounding_context_id": grounding_context_id,
                        "intent_artifact_id": intent_artifact_id,
                        "media_context": media_context.clone(),
                    }),
                },
            )
            .await
            {
                Ok(result) => result,
                Err(error) => {
                    Self::fail_jarvis_gate_visible(
                        &mut stream,
                        &jarvis_progress_bus,
                        &chat_id,
                        Some(&interaction_id),
                        error,
                        "plan_artifact",
                        db.as_ref(),
                        jarvis_conv_id.as_deref(),
                    )
                    .await?;
                    return Ok(());
                }
            };
            let plan_artifact_id = plan_artifact.artifact_id.clone();
            Self::write_jarvis_progress(
                &mut stream,
                &jarvis_progress_bus,
                &chat_id,
                Some(&interaction_id),
                "plan_artifact",
                "shared_artifact_put_completed",
                "completed",
                if confirmation_required {
                    "plan.lisp artifact 已写入，正在发送草案卡片和确认请求。"
                } else {
                    "plan.lisp artifact 已归档，下一步进入执行分工或直接回答。"
                },
                None,
                None,
                None,
            )
            .await?;
            let mut plan = plan_payload;
            if let Some(object) = plan.as_object_mut() {
                object.insert(
                    "plan_artifact_id".to_string(),
                    serde_json::Value::String(plan_artifact_id.clone()),
                );
                object.insert(
                    "plan_artifact_hash".to_string(),
                    serde_json::Value::String(plan_artifact.artifact_hash.clone()),
                );
                object.insert(
                    "plan_artifact_path".to_string(),
                    serde_json::Value::String(plan_artifact.path.clone()),
                );
            }
            generated_plan_atomization_graph = plan.get("atomization_graph").cloned();
            generated_execution_mode = plan
                .get("execution_mode")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned);
            generated_requires_board_task = plan
                .get("requires_board_task")
                .and_then(|value| value.as_bool());
            generated_direct_answer_draft = plan
                .get("direct_answer_draft")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            let plan_event_name = if confirmation_required {
                "plan_draft"
            } else {
                "plan_archived"
            };
            Self::write_sse_event(&mut stream, plan_event_name, &plan).await?;
            Self::persist_interaction_event(
                db.as_ref(),
                jarvis_conv_id.as_deref(),
                Some(&interaction_id),
                plan_event_name,
                &plan,
            )
            .await;
            if confirmation_required {
                Self::write_sse_openai_missiond_projection(
                    &mut stream,
                    &chat_id,
                    "plan_draft",
                    &plan_artifact_id,
                    &plan_artifact.artifact_hash,
                    &plan_artifact.path,
                )
                .await?;
            }
            if !confirmation_required {
                plan_artifact_id
            } else {
                let confirm = serde_json::json!({
                    "interaction_id": interaction_id,
                    "phase": "awaiting_plan_confirmation",
                    "confirmation_type": "plan",
                    "confirm_payload": {
                        "missiond_intent_confirmed": true,
                        "missiond_plan_confirmed": true,
                        "missiond_objective": plan_objective_text,
                        "missiond_grounding_context_id": grounding_context_id,
                        "missiond_context_pack_path": context_pack_path,
                        "missiond_context_pack_file": context_pack_file,
                        "missiond_grounding_report_file": grounding_report_file,
                        "missiond_grounding_report_artifact_path": grounding_report_artifact_path,
                        "missiond_grounding_report_hash": grounding_report_hash,
                        "missiond_grounding_worker_slot_id": grounding_worker_slot_id,
                        "missiond_grounding_worker_turn_id": grounding_worker_turn_id,
                        "missiond_context_sufficiency": context_sufficiency,
                        "missiond_grounding_artifact_hash": grounding_artifact_hash,
                        "missiond_context_capsule_hash": context_capsule_hash,
                        "missiond_context_capsule_file": context_capsule_file,
                        "missiond_topic_id": resolved_topic_id,
                        "missiond_topic_label": resolved_topic_label,
                        "missiond_sources_used": sources_used,
                        "missiond_intent_artifact_id": intent_artifact_id,
                        "missiond_key_judgment_artifact_id": key_judgment_ref.artifact_id,
                        "missiond_key_judgment_artifact_hash": key_judgment_ref.artifact_hash,
                        "missiond_key_judgment_artifact_path": key_judgment_ref.artifact_path,
                        "missiond_key_judgment": key_judgment_ref.judgment,
                        "missiond_key_judgment_review_text": key_judgment_ref.review_text,
                        "missiond_key_judgment_confidence": key_judgment_ref.confidence,
                        "missiond_key_judgment_rejected_hypotheses": key_judgment_ref.rejected_hypotheses,
                        "missiond_key_judgment_evidence_refs": key_judgment_ref.evidence_refs,
                        "missiond_key_judgment_planning_implications": key_judgment_ref.planning_implications,
                        "missiond_key_judgment_acceptance_focus": key_judgment_ref.acceptance_focus,
                        "missiond_plan_artifact_id": plan_artifact_id,
                        "missiond_direct_answer_draft": generated_direct_answer_draft.clone(),
                        "missiond_plan_atomization_graph_json": serde_json::to_string(
                            plan.get("atomization_graph").unwrap_or(&serde_json::Value::Null)
                        ).ok(),
                        "missiond_execution_mode": plan
                            .get("execution_mode")
                            .and_then(|value| value.as_str())
                            .unwrap_or("work_order"),
                        "missiond_requires_board_task": plan
                            .get("requires_board_task")
                            .and_then(|value| value.as_bool())
                            .unwrap_or(true),
                    }
                });
                Self::write_sse_event(&mut stream, "confirm_required", &confirm).await?;
                Self::persist_interaction_event(
                    db.as_ref(),
                    jarvis_conv_id.as_deref(),
                    Some(&interaction_id),
                    "confirm_required",
                    &confirm,
                )
                .await;
                Self::persist_jarvis_pending_confirmation(
                    db.as_ref(),
                    jarvis_conv_id.as_deref(),
                    &confirm,
                )
                .await;
                Self::write_sse_openai_text_and_persist(
                    &mut stream,
                    &chat_id,
                    "我已生成 plan.lisp 草案，等待你确认计划。",
                    Some("stop"),
                    db.as_ref(),
                    jarvis_conv_id.as_deref(),
                )
                .await?;
                Self::finish_sse(&mut stream).await?;
                return Ok(());
            }
        } else {
            confirmed_plan_artifact_id.clone().unwrap_or_default()
        };
        let plan_atomization_graph = generated_plan_atomization_graph.unwrap_or_else(|| {
            Self::jarvis_plan_atomization_graph_from_interaction_metadata(&envelope)
        });

        let execution_mode = generated_execution_mode
            .or_else(|| interaction_metadata_string(&envelope, "missiond_execution_mode"))
            .unwrap_or_else(|| "work_order".to_string())
            .to_ascii_lowercase();
        if execution_mode == "grounded_direct_answer" {
            let direct_answer_draft = generated_direct_answer_draft
                .or_else(|| interaction_metadata_string(&envelope, "missiond_direct_answer_draft"));
            let requires_board_task = generated_requires_board_task.unwrap_or_else(|| {
                interaction_metadata_bool(&envelope, "missiond_requires_board_task")
            });
            if requires_board_task {
                Self::fail_jarvis_gate_visible(
                    &mut stream,
                    &jarvis_progress_bus,
                    &chat_id,
                    Some(&interaction_id),
                    "plan.lisp declared grounded_direct_answer but missiond_requires_board_task=true; refusing ambiguous execution.",
                    "execution_mode",
                    db.as_ref(),
                    jarvis_conv_id.as_deref(),
                )
                .await?;
                return Ok(());
            }
            Self::persist_jarvis_confirmation_fulfilled(
                db.as_ref(),
                jarvis_conv_id.as_deref(),
                "plan",
            )
            .await;
            if let Err(error) = Self::stream_jarvis_grounded_direct_answer(
                &mut stream,
                &jarvis_progress_bus,
                &jarvis_artifact_writer,
                &chat_id,
                Some(&interaction_id),
                &objective_text,
                &grounding_context_id,
                context_pack_path.as_deref(),
                context_pack_file.as_deref(),
                grounding_report_file.as_deref(),
                grounding_report_artifact_path.as_deref(),
                grounding_report_hash.as_deref(),
                &intent_artifact_id,
                &plan_artifact_id,
                &key_judgment_ref,
                direct_answer_draft.as_deref(),
                &permission_context,
                &sources_used,
                &media_context,
                &provider_box_http,
                db.as_ref(),
                jarvis_conv_id.as_deref(),
            )
            .await
            {
                Self::fail_jarvis_gate_visible(
                    &mut stream,
                    &jarvis_progress_bus,
                    &chat_id,
                    Some(&interaction_id),
                    error.to_string(),
                    "grounded_direct_answer",
                    db.as_ref(),
                    jarvis_conv_id.as_deref(),
                )
                .await?;
                return Ok(());
            }
            Self::finish_sse(&mut stream).await?;
            return Ok(());
        }
        if !matches!(execution_mode.as_str(), "work_order" | "investigation_only") {
            Self::fail_jarvis_gate_visible(
                &mut stream,
                &jarvis_progress_bus,
                &chat_id,
                Some(&interaction_id),
                format!("Unsupported Jarvis execution_mode: {execution_mode}"),
                "execution_mode",
                db.as_ref(),
                jarvis_conv_id.as_deref(),
            )
            .await?;
            return Ok(());
        }

        let Some(ref db) = db else {
            Self::write_sse_event(
                &mut stream,
                "diagnostic",
                &serde_json::json!({
                    "interaction_id": interaction_id,
                    "phase": "board_task_create",
                    "phase_code": "board_dispatch",
                    "error": {
                        "code": "MISSIOND_DB_UNAVAILABLE",
                        "message": "MissionD DB unavailable; cannot create BoardTask."
                    }
                }),
            )
            .await?;
            Self::write_sse_openai_text(
                &mut stream,
                &chat_id,
                "失败在 board_task_create：MissionD DB 不可用，无法创建 BoardTask。\n",
                Some("stop"),
            )
            .await?;
            Self::finish_sse(&mut stream).await?;
            return Ok(());
        };

        let title = if objective_text.chars().count() > 80 {
            format!("{}...", objective_text.chars().take(77).collect::<String>())
        } else {
            objective_text.clone()
        };
        let mut dispatch_metadata = Self::derive_jarvis_dispatch_contract(
            &objective_text,
            &grounding_context_id,
            context_pack_path.as_deref(),
            context_pack_file.as_deref(),
            grounding_report_file.as_deref(),
            grounding_report_artifact_path.as_deref(),
            grounding_report_hash.as_deref(),
            &intent_artifact_id,
            &plan_artifact_id,
            &key_judgment_ref,
            &plan_atomization_graph,
            &Self::jarvis_runtime_read_scope_root(),
        );
        if let Some(object) = dispatch_metadata.as_object_mut() {
            object.insert("media_context".to_string(), media_context.clone());
        }
        let prompt_template = Self::build_jarvis_worker_prompt(&objective_text, &dispatch_metadata);
        let meta = serde_json::json!({
            "source": "interaction-gateway",
            "interaction_id": interaction_id,
            "channel": channel,
            "permission_context": permission_context.clone(),
            "grounding_context_id": grounding_context_id,
            "context_pack_path": context_pack_path,
            "context_pack_file": context_pack_file,
            "grounding_report_file": grounding_report_file,
            "grounding_report_artifact_path": grounding_report_artifact_path,
            "grounding_report_hash": grounding_report_hash,
            "grounding_worker_slot_id": grounding_worker_slot_id,
            "grounding_worker_turn_id": grounding_worker_turn_id,
            "context_sufficiency": context_sufficiency,
            "context_capsule_hash": context_capsule_hash,
            "context_capsule_file": context_capsule_file,
            "topic_id": resolved_topic_id,
            "topic_label": resolved_topic_label,
            "intent_artifact_id": intent_artifact_id,
            "plan_artifact_id": plan_artifact_id,
            "key_judgment_artifact_id": key_judgment_ref.artifact_id,
            "key_judgment_artifact_hash": key_judgment_ref.artifact_hash,
            "key_judgment": key_judgment_ref.judgment,
            "plan_atomization_graph": plan_atomization_graph,
            "media_context": media_context.clone(),
            "dispatch_metadata": dispatch_metadata,
            "user_message": raw_user_text,
            "objective": objective_text,
        });
        let context_intent = dispatch_metadata
            .get("task_class")
            .and_then(|v| v.as_str())
            .unwrap_or("interaction")
            .to_string();
        let task_input = crate::types::CreateBoardTaskInput {
            title,
            description: Some(format!(
                "Jarvis interaction task for conversation {}. See runtime_metadata for grounding, intent, plan, permission, and dispatch fields.",
                conversation_id.as_deref().unwrap_or("")
            )),
            priority: None,
            category: Some("interaction".to_string()),
            project: None,
            server: None,
            due_date: None,
            parent_id: None,
            assignee: None,
            auto_execute: Some(true),
            prompt_template: Some(prompt_template),
            hidden: Some(false),
            flow_template: None,
            depends_on: None,
            dedupe_key: None,
            timeout_secs: None,
            context_intent: Some(context_intent),
            runtime_metadata: Some(meta),
        };
        Self::write_jarvis_progress(
            &mut stream,
            &jarvis_progress_bus,
            &chat_id,
            Some(&interaction_id),
            "board_task_create",
            "create_board_task",
            "running",
            "计划已确认，正在创建可追踪 BoardTask 并准备异步派工。",
            None,
            None,
            None,
        )
        .await?;
        match Self::create_jarvis_atomized_board_tasks(
            db,
            task_input,
            &objective_text,
            &dispatch_metadata,
            &plan_atomization_graph,
        )
        .await
        {
            Ok(created) => {
                Self::persist_jarvis_confirmation_fulfilled(
                    Some(db),
                    jarvis_conv_id.as_deref(),
                    "plan",
                )
                .await;
                let atom_task_ids = created
                    .atom_tasks
                    .iter()
                    .map(|atom| atom.task.id.to_string())
                    .collect::<Vec<_>>();
                let planned_atom_task_ids = created
                    .atom_tasks
                    .iter()
                    .filter(|atom| !atom.synthetic)
                    .map(|atom| atom.task.id.to_string())
                    .collect::<Vec<_>>();
                let atom_task_contracts = created
                    .atom_tasks
                    .iter()
                    .map(|atom| {
                        serde_json::json!({
                            "atom_task_id": atom.atom_task_id.clone(),
                            "board_task_id": atom.task.id.to_string(),
                            "category": atom.category.clone(),
                            "assignee_engine": atom.assignee_engine.clone(),
                            "depends_on_atoms": atom.depends_on_atoms.clone(),
                            "parallel_group": atom.parallel_group.clone(),
                            "synthetic": atom.synthetic
                        })
                    })
                    .collect::<Vec<_>>();
                let follow_payload = serde_json::json!({
                    "missiond_follow_task_id": created.final_task_id.clone(),
                    "missiond_root_task_id": created.parent_task.id.to_string(),
                    "missiond_atom_task_ids": atom_task_ids.clone(),
                    "interaction_id": interaction_id,
                    "stream": true
                });
                let board_task_created = serde_json::json!({
                    "interaction_id": interaction_id,
                    "task_id": created.parent_task.id.to_string(),
                    "root_task_id": created.parent_task.id.to_string(),
                    "final_task_id": created.final_task_id.clone(),
                    "atom_task_ids": planned_atom_task_ids.clone(),
                    "atom_task_contracts": atom_task_contracts,
                    "title": created.parent_task.title.clone(),
                    "grounding_context_id": grounding_context_id,
                    "intent_artifact_id": intent_artifact_id,
                    "plan_artifact_id": plan_artifact_id,
                });
                Self::write_sse_event(&mut stream, "board_task_created", &board_task_created)
                    .await?;
                Self::persist_interaction_event(
                    Some(db),
                    jarvis_conv_id.as_deref(),
                    Some(&interaction_id),
                    "board_task_created",
                    &board_task_created,
                )
                .await;
                let worker_dispatched = serde_json::json!({
                    "interaction_id": interaction_id,
                    "phase": "workers_running",
                    "task_id": created.final_task_id.clone(),
                    "root_task_id": created.parent_task.id.to_string(),
                    "atom_task_ids": atom_task_ids.clone(),
                    "slot_id": serde_json::Value::Null,
                    "dispatch_state": "pending_autopilot_claim",
                    "status": created.parent_task.status.as_str(),
                    "terminal_task_result": false,
                    "follow_payload": follow_payload.clone(),
                    "message": "Atom-level BoardTasks are queued for Autopilot/provider claim; final acceptance task is the follow target."
                });
                Self::write_sse_event(&mut stream, "worker_dispatched", &worker_dispatched).await?;
                Self::persist_interaction_event(
                    Some(db),
                    jarvis_conv_id.as_deref(),
                    Some(&interaction_id),
                    "worker_dispatched",
                    &worker_dispatched,
                )
                .await;
                let worker_status = serde_json::json!({
                    "interaction_id": interaction_id,
                    "phase": "workers_running",
                    "task_id": created.final_task_id.clone(),
                    "root_task_id": created.parent_task.id.to_string(),
                    "atom_task_ids": atom_task_ids.clone(),
                    "status": created.parent_task.status.as_str(),
                    "terminal_task_result": false,
                });
                Self::write_sse_event(&mut stream, "worker_status", &worker_status).await?;
                Self::persist_interaction_event(
                    Some(db),
                    jarvis_conv_id.as_deref(),
                    Some(&interaction_id),
                    "worker_status",
                    &worker_status,
                )
                .await;
                let dispatch_accepted = serde_json::json!({
                    "interaction_id": interaction_id,
                    "phase": "board_tasks_created",
                    "task_id": created.final_task_id.clone(),
                    "root_task_id": created.parent_task.id.to_string(),
                    "atom_task_ids": atom_task_ids.clone(),
                    "terminal_task_result": false,
                    "follow_payload": follow_payload.clone(),
                    "message": "Atom-level BoardTasks were created and accepted for asynchronous worker dispatch; this is not a terminal task result."
                });
                Self::write_sse_event(&mut stream, "dispatch_accepted", &dispatch_accepted).await?;
                Self::persist_interaction_event(
                    Some(db),
                    jarvis_conv_id.as_deref(),
                    Some(&interaction_id),
                    "dispatch_accepted",
                    &dispatch_accepted,
                )
                .await;
                let result_pending = serde_json::json!({
                    "interaction_id": interaction_id,
                    "phase": "result_pending",
                    "task_id": created.final_task_id.clone(),
                    "root_task_id": created.parent_task.id.to_string(),
                    "atom_task_ids": atom_task_ids,
                    "terminal_task_result": false,
                    "follow_payload": follow_payload,
                });
                Self::write_sse_event(&mut stream, "result_pending", &result_pending).await?;
                Self::persist_interaction_event(
                    Some(db),
                    jarvis_conv_id.as_deref(),
                    Some(&interaction_id),
                    "result_pending",
                    &result_pending,
                )
                .await;
                let accepted_fallback = format!(
                    "plan.lisp 已归档，已创建 Jarvis 原子化 BoardTask 组；后续用 missiond_follow_task_id={} 读取最终验收 task-result-artifact。",
                    created.final_task_id
                );
                let accepted_text = match Self::materialize_jarvis_communication(
                    &mut stream,
                    &jarvis_progress_bus,
                    &jarvis_artifact_writer,
                    &chat_id,
                    Some(&interaction_id),
                    "plan_dispatched",
                    &objective_text,
                    serde_json::json!({
                        "execution_mode": execution_mode,
                        "terminal_task_result": false,
                        "intent_artifact_id": intent_artifact_id,
                        "plan_artifact_id": plan_artifact_id,
                        "key_judgment_artifact_id": key_judgment_ref.artifact_id,
                        "key_judgment": key_judgment_ref.judgment,
                        "root_task_id": created.parent_task.id.to_string(),
                        "final_task_id": created.final_task_id.clone(),
                        "atom_task_ids": atom_task_ids,
                        "planned_atom_task_ids": planned_atom_task_ids,
                        "follow_payload": result_pending.get("follow_payload").cloned(),
                        "dispatch_accepted": dispatch_accepted,
                        "media_context": media_context.clone(),
                    }),
                    &provider_box_http,
                    Some(db),
                    jarvis_conv_id.as_deref(),
                )
                .await
                {
                    Ok(text) => text,
                    Err(error) => {
                        let diagnostic = serde_json::json!({
                            "interaction_id": interaction_id,
                            "phase": "communicator",
                            "error": {
                                "code": "JARVIS_COMMUNICATOR_FAILED",
                                "message": error.to_string()
                            }
                        });
                        Self::write_sse_event(&mut stream, "diagnostic", &diagnostic).await?;
                        accepted_fallback
                    }
                };
                Self::write_sse_openai_text_and_persist(
                    &mut stream,
                    &chat_id,
                    &accepted_text,
                    Some("stop"),
                    Some(db),
                    jarvis_conv_id.as_deref(),
                )
                .await?;
            }
            Err(e) => {
                Self::write_sse_event(
                    &mut stream,
                    "diagnostic",
                    &serde_json::json!({
                        "interaction_id": interaction_id,
                        "phase": "board_task_create",
                        "error": {
                            "code": "BOARDTASK_CREATE_FAILED",
                            "message": e.to_string()
                        }
                    }),
                )
                .await?;
                Self::write_sse_openai_text_and_persist(
                    &mut stream,
                    &chat_id,
                    &format!("失败在 board_task_create：{}", e),
                    Some("stop"),
                    Some(db),
                    jarvis_conv_id.as_deref(),
                )
                .await?;
            }
        }
        Self::finish_sse(&mut stream).await?;
        Ok(())
    }

    /// Send an HTTP error response
    /// Extract text + images from OpenAI multimodal content array.
    /// Images (base64 data URLs) are saved to temp files; local paths are injected into the prompt.
    #[allow(dead_code)]
    async fn extract_multimodal_content(parts: &[serde_json::Value]) -> String {
        let media_dir = std::path::Path::new("/tmp/missiond_media");
        let mut text_parts: Vec<String> = Vec::new();
        let mut image_paths: Vec<String> = Vec::new();

        for part in parts {
            let part_type = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match part_type {
                "text" => {
                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        text_parts.push(text.to_string());
                    }
                }
                "image_url" => {
                    if let Some(url) = part
                        .get("image_url")
                        .and_then(|u| u.get("url"))
                        .and_then(|u| u.as_str())
                    {
                        if let Some(path) = Self::save_data_url_to_file(url, media_dir).await {
                            image_paths.push(path);
                        }
                    }
                }
                _ => {
                    // video_url, file_url etc — log and skip for now
                    debug!(
                        part_type,
                        "Multimodal: unsupported content part type, skipping"
                    );
                }
            }
        }

        // Build combined prompt: images first (as path references), then text
        let mut result = String::new();
        if !image_paths.is_empty() {
            result.push_str("[用户附带了图片，已保存到本机。请使用 Read 工具查看图片后回答：]\n");
            for path in &image_paths {
                result.push_str(&format!("- {}\n", path));
            }
            result.push('\n');
        }
        result.push_str(&text_parts.join("\n"));
        result
    }

    /// Decode a data URL (data:image/jpeg;base64,...) and save to a temp file.
    /// Returns the local file path on success.
    #[allow(dead_code)]
    async fn save_data_url_to_file(url: &str, media_dir: &std::path::Path) -> Option<String> {
        use base64::Engine;

        // Parse data URL: data:<mime>;base64,<data>
        let prefix = "data:";
        if !url.starts_with(prefix) {
            debug!(url_len = url.len(), "Multimodal: not a data URL, skipping");
            return None;
        }

        let rest = &url[prefix.len()..];
        let (mime, b64_data) = if let Some(pos) = rest.find(";base64,") {
            (&rest[..pos], &rest[pos + 8..])
        } else {
            debug!("Multimodal: data URL missing ;base64, marker");
            return None;
        };

        // Determine file extension from MIME type
        let ext = match mime {
            "image/jpeg" | "image/jpg" => "jpg",
            "image/png" => "png",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "image/heic" => "heic",
            _ => "bin",
        };

        // Decode base64
        let bytes = match base64::engine::general_purpose::STANDARD.decode(b64_data) {
            Ok(b) => b,
            Err(e) => {
                warn!(error = %e, "Multimodal: base64 decode failed");
                return None;
            }
        };

        // Ensure media directory exists
        if let Err(e) = tokio::fs::create_dir_all(media_dir).await {
            warn!(error = %e, "Multimodal: failed to create media dir");
            return None;
        }

        // Write to file with timestamp-based name
        let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S_%3f");
        let filename = format!("img_{}.{}", ts, ext);
        let filepath = media_dir.join(&filename);

        match tokio::fs::write(&filepath, &bytes).await {
            Ok(_) => {
                info!(
                    path = %filepath.display(),
                    size_kb = bytes.len() / 1024,
                    mime,
                    "Multimodal: saved image to temp file"
                );
                Some(filepath.to_string_lossy().to_string())
            }
            Err(e) => {
                warn!(error = %e, path = %filepath.display(), "Multimodal: failed to write image file");
                None
            }
        }
    }

    async fn send_http_error(
        stream: &mut TcpStream,
        status: u16,
        reason: &str,
        body: &str,
    ) -> anyhow::Result<()> {
        let response = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            status,
            reason,
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await?;
        Ok(())
    }

    async fn send_json_response(
        stream: &mut TcpStream,
        status: u16,
        reason: &str,
        body: &serde_json::Value,
    ) -> anyhow::Result<()> {
        let body = body.to_string();
        let response = format!(
            "HTTP/1.1 {} {}\r\n\
             Content-Type: application/json\r\n\
             Access-Control-Allow-Origin: *\r\n\
             Access-Control-Allow-Headers: Content-Type, Authorization\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n{}",
            status,
            reason,
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await?;
        Ok(())
    }

    async fn send_provider_box_response(
        stream: &mut TcpStream,
        response: ProviderBoxHttpResponse,
    ) -> anyhow::Result<()> {
        let body = response.body.to_string();
        let reason = http_reason(response.status);
        let content_type = if response.content_type.trim().is_empty() {
            "application/json"
        } else {
            response.content_type.trim()
        };
        let response_text = format!(
            "HTTP/1.1 {} {}\r\n\
             Content-Type: {}\r\n\
             Access-Control-Allow-Origin: *\r\n\
             Access-Control-Allow-Headers: Content-Type, Authorization, X-Slot-Id, X-Trace-Id\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n{}",
            response.status,
            reason,
            content_type,
            body.len(),
            body
        );
        stream.write_all(response_text.as_bytes()).await?;
        stream.shutdown().await?;
        Ok(())
    }

    async fn handle_provider_box_http(
        mut stream: TcpStream,
        method: &str,
        path: &str,
        provider_box_http: ProviderBoxHttpSlot,
    ) -> anyhow::Result<()> {
        let adapter = {
            let guard = provider_box_http.read().await;
            guard.clone()
        };
        let Some(adapter) = adapter else {
            let err = serde_json::json!({
                "error": {
                    "message": "Provider-box adapter is not configured"
                }
            });
            return Self::send_http_error(
                &mut stream,
                503,
                "Service Unavailable",
                &err.to_string(),
            )
            .await;
        };

        let (headers_str, body_text) = Self::read_http_request(&mut stream).await?;
        let headers = parse_http_headers(&headers_str);
        let body = if body_text.trim().is_empty() {
            serde_json::Value::Null
        } else {
            match serde_json::from_str::<serde_json::Value>(&body_text) {
                Ok(value) => value,
                Err(err) => {
                    let body = serde_json::json!({
                        "error": {
                            "message": format!("Invalid JSON body: {err}")
                        }
                    });
                    return Self::send_http_error(
                        &mut stream,
                        400,
                        "Bad Request",
                        &body.to_string(),
                    )
                    .await;
                }
            }
        };

        let request = ProviderBoxHttpRequest {
            method: method.to_string(),
            path: path.to_string(),
            headers,
            body,
        };

        match adapter(request).await {
            Ok(response) => Self::send_provider_box_response(&mut stream, response).await,
            Err(message) => {
                let body = serde_json::json!({
                    "error": {
                        "message": message
                    }
                });
                Self::send_http_error(&mut stream, 500, "Internal Server Error", &body.to_string())
                    .await
            }
        }
    }

    fn request_path_without_query(path: &str) -> &str {
        path.split('?').next().unwrap_or(path)
    }

    fn request_query_i64(path: &str, key: &str, default: i64, min: i64, max: i64) -> i64 {
        let Some(query) = path.split_once('?').map(|(_, query)| query) else {
            return default;
        };
        query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find_map(|(name, value)| {
                if name == key {
                    value.parse::<i64>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(default)
            .clamp(min, max)
    }

    /// Read full HTTP request from stream (headers + body)
    async fn read_http_request(stream: &mut TcpStream) -> anyhow::Result<(String, String)> {
        let mut buf = Vec::with_capacity(8192);
        let mut tmp = [0u8; 4096];

        // Read until we have full headers
        let header_end;
        loop {
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                anyhow::bail!("Connection closed before headers complete");
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                header_end = pos + 4;
                break;
            }
            if buf.len() > 65536 {
                anyhow::bail!("Headers too large");
            }
        }

        let headers_str = String::from_utf8_lossy(&buf[..header_end]).to_string();

        // Parse Content-Length
        let content_length: usize = headers_str
            .lines()
            .find_map(|line| {
                let lower = line.to_lowercase();
                if lower.starts_with("content-length:") {
                    lower
                        .trim_start_matches("content-length:")
                        .trim()
                        .parse()
                        .ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);

        // Read remaining body
        let _body_so_far = buf.len() - header_end;
        let mut body_buf = buf[header_end..].to_vec();
        while body_buf.len() < content_length {
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                break;
            }
            body_buf.extend_from_slice(&tmp[..n]);
        }

        let body =
            String::from_utf8_lossy(&body_buf[..content_length.min(body_buf.len())]).to_string();
        Ok((headers_str, body))
    }

    fn jarvis_history_include_legacy_unscoped(permission_context: &serde_json::Value) -> bool {
        if std::env::var("MISSIOND_JARVIS_HISTORY_LEGACY_UNSCOPED")
            .map(|value| value == "0" || value.eq_ignore_ascii_case("false"))
            .unwrap_or(false)
        {
            return false;
        }
        let roles = json_string_array_field(permission_context, &["roles"]);
        let scopes = json_string_array_field(permission_context, &["scope", "scopes"]);
        roles.iter().any(|role| {
            matches!(
                role.as_str(),
                "admin"
                    | "system_admin"
                    | "tenant_admin"
                    | "missiond_operator"
                    | "service"
                    | "user"
            )
        }) || scopes.iter().any(|scope| {
            matches!(
                scope.as_str(),
                "missiond:operator" | "missiond:admin" | "jarvis:history" | "jarvis:*"
            )
        })
    }

    async fn handle_jarvis_conversations(
        mut stream: TcpStream,
        normalized_request_line: &str,
        db: Option<Arc<dyn crate::db::traits::MissionStore>>,
    ) -> anyhow::Result<()> {
        let Some(db) = db else {
            return Self::send_json_response(
                &mut stream,
                503,
                "Service Unavailable",
                &serde_json::json!({
                    "error": {"code": "MISSIOND_DB_UNAVAILABLE", "message": "MissionD DB unavailable"}
                }),
            )
            .await;
        };
        let (headers, _body) = match Self::read_http_request(&mut stream).await {
            Ok(request) => request,
            Err(error) => {
                return Self::send_json_response(
                    &mut stream,
                    400,
                    "Bad Request",
                    &serde_json::json!({
                        "error": {"message": format!("Bad request: {}", error)}
                    }),
                )
                .await;
            }
        };

        let envelope = InteractionEnvelope {
            channel: "jarvis".to_string(),
            external_user_id: None,
            auth_token: None,
            conversation_id: None,
            message: serde_json::Value::String("jarvis history".to_string()),
            attachments: Vec::new(),
            metadata: serde_json::json!({"application_id": "missiond"}),
        };
        let auth_resolution = match resolve_interaction_auth(&envelope, &headers).await {
            Ok(resolution) => resolution,
            Err((status, reason, body)) => {
                return Self::send_json_response(&mut stream, status, reason, &body).await;
            }
        };
        let permission_context = auth_resolution.permission_context;
        let scope =
            conversation_scope_from_permission(&envelope, &permission_context, "jarvis", "");
        let include_legacy_unscoped =
            Self::jarvis_history_include_legacy_unscoped(&permission_context);

        let request_path = normalized_request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or("/api/jarvis/conversations");
        let path_only = Self::request_path_without_query(request_path);
        if path_only == "/api/jarvis/conversations" {
            let limit = Self::request_query_i64(request_path, "limit", 25, 1, 100);
            let conversations = db
                .jarvis_list_scoped(
                    scope.user_id.as_deref(),
                    scope.tenant_id.as_deref(),
                    scope.application_id.as_deref(),
                    Some(scope.channel.as_str()),
                    include_legacy_unscoped,
                    limit,
                )
                .await?;
            return Self::send_json_response(
                &mut stream,
                200,
                "OK",
                &serde_json::json!({
                    "schema": "missiond.jarvis-conversation-list.v1",
                    "permission_context": permission_context,
                    "scope": {
                        "user_id": scope.user_id,
                        "tenant_id": scope.tenant_id,
                        "application_id": scope.application_id,
                        "channel": scope.channel,
                        "include_legacy_unscoped": include_legacy_unscoped,
                    },
                    "conversations": conversations,
                }),
            )
            .await;
        }

        let Some(conversation_id) = path_only
            .strip_prefix("/api/jarvis/conversations/")
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Self::send_json_response(
                &mut stream,
                404,
                "Not Found",
                &serde_json::json!({"error": {"message": "Jarvis conversation route not found"}}),
            )
            .await;
        };

        let tail = Self::request_query_i64(request_path, "tail", 80, 1, 300);
        match db
            .jarvis_history_scoped(
                conversation_id,
                scope.user_id.as_deref(),
                scope.tenant_id.as_deref(),
                scope.application_id.as_deref(),
                Some(scope.channel.as_str()),
                include_legacy_unscoped,
                tail,
            )
            .await?
        {
            Some(history) => {
                let mut response = serde_json::json!({
                    "schema": "missiond.jarvis-conversation-history.v1",
                    "permission_context": permission_context,
                });
                if let Some(object) = response.as_object_mut() {
                    if let Some(history_object) = history.as_object() {
                        for (key, value) in history_object {
                            object.insert(key.clone(), value.clone());
                        }
                    }
                }
                Self::send_json_response(&mut stream, 200, "OK", &response).await
            }
            None => {
                Self::send_json_response(
                    &mut stream,
                    404,
                    "Not Found",
                    &serde_json::json!({
                        "error": {
                            "code": "JARVIS_CONVERSATION_NOT_FOUND",
                            "message": "Conversation not found in the caller's Jarvis scope"
                        }
                    }),
                )
                .await
            }
        }
    }

    /// Handle POST /webhooks/* — AIOps incident webhook receiver
    async fn handle_webhook(
        mut stream: TcpStream,
        request_line: &str,
        incident_tx: Option<tokio::sync::mpsc::Sender<crate::types::MissionIncident>>,
        system_event_tx: Option<tokio::sync::mpsc::Sender<SystemEvent>>,
    ) -> anyhow::Result<()> {
        let (headers, body) = Self::read_http_request(&mut stream).await?;

        // Extract path from request line (e.g. "POST /webhooks/deploy HTTP/1.1")
        let path = request_line.split_whitespace().nth(1).unwrap_or("");

        if path == "/webhooks/service-event"
            || path == "/webhooks/auth-event"
            || path == "/webhooks/deploy-center-event"
        {
            let expected_token = std::env::var("MISSIOND_EXTERNAL_WEBHOOK_TOKEN").ok();
            if !webhook_token_matches(&headers, expected_token.as_deref()) {
                Self::send_http_error(
                    &mut stream,
                    401,
                    "Unauthorized",
                    r#"{"error":"invalid webhook token"}"#,
                )
                .await?;
                return Ok(());
            }

            let tx = match system_event_tx {
                Some(tx) => tx,
                None => {
                    Self::send_http_error(
                        &mut stream,
                        503,
                        "Service Unavailable",
                        r#"{"error":"system event bus not configured"}"#,
                    )
                    .await?;
                    return Ok(());
                }
            };
            let default_service_id = match path {
                "/webhooks/auth-event" => "auth",
                "/webhooks/deploy-center-event" => "deploy-center",
                _ => "external",
            };
            let require_event_id = path == "/webhooks/deploy-center-event";
            let Some(event) =
                parse_external_service_webhook(&body, default_service_id, require_event_id)
            else {
                Self::send_http_error(
                    &mut stream,
                    400,
                    "Bad Request",
                    r#"{"error":"invalid service event envelope"}"#,
                )
                .await?;
                return Ok(());
            };
            let event_id = match &event {
                SystemEvent::ExternalServiceEvent { event_id, .. } => event_id.clone(),
                _ => "unknown".to_string(),
            };
            if let Err(e) = tx.try_send(event) {
                warn!("Webhook: system event channel full, dropping: {}", e);
                Self::send_http_error(
                    &mut stream,
                    503,
                    "Service Unavailable",
                    r#"{"error":"system event queue full"}"#,
                )
                .await?;
                return Ok(());
            }
            let resp_body = serde_json::json!({"ok": true, "event_id": event_id}).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                resp_body.len(),
                resp_body
            );
            stream.write_all(response.as_bytes()).await?;
            stream.shutdown().await?;
            return Ok(());
        }

        let tx = match incident_tx {
            Some(tx) => tx,
            None => {
                Self::send_http_error(
                    &mut stream,
                    503,
                    "Service Unavailable",
                    r#"{"error":"incident bus not configured"}"#,
                )
                .await?;
                return Ok(());
            }
        };

        let incident = match path {
            "/webhooks/deploy" => parse_deploy_webhook(&body),
            "/webhooks/test" => parse_test_webhook(&body),
            _ => {
                Self::send_http_error(
                    &mut stream,
                    404,
                    "Not Found",
                    r#"{"error":"unknown webhook path"}"#,
                )
                .await?;
                return Ok(());
            }
        };

        match incident {
            Some(inc) => {
                let inc_id = inc.id.clone();
                if let Err(e) = tx.try_send(inc) {
                    warn!("Webhook: incident channel full, dropping: {}", e);
                }
                let resp_body = serde_json::json!({"ok": true, "incident_id": inc_id}).to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    resp_body.len(),
                    resp_body
                );
                stream.write_all(response.as_bytes()).await?;
                stream.shutdown().await?;
            }
            None => {
                // Non-alert event (e.g. deploy success) → silent 200
                let resp_body = r#"{"ok":true,"action":"ignored"}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    resp_body.len(),
                    resp_body
                );
                stream.write_all(response.as_bytes()).await?;
                stream.shutdown().await?;
            }
        }

        Ok(())
    }

    async fn write_sse_event(
        stream: &mut TcpStream,
        event: &str,
        payload: &serde_json::Value,
    ) -> anyhow::Result<()> {
        Self::write_sse_bytes(
            stream,
            format!("event: {event}\ndata: {payload}\n\n").as_bytes(),
            "event",
            Some(event),
        )
        .await
    }

    async fn write_sse_bytes(
        stream: &mut TcpStream,
        bytes: &[u8],
        kind: &str,
        event: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Err(error) = stream.write_all(bytes).await {
            return Self::handle_sse_write_error(error, kind, event);
        }
        if let Err(error) = stream.flush().await {
            return Self::handle_sse_write_error(error, kind, event);
        }
        Ok(())
    }

    fn handle_sse_write_error(
        error: std::io::Error,
        kind: &str,
        event: Option<&str>,
    ) -> anyhow::Result<()> {
        if Self::is_client_disconnect_error(&error) {
            warn!(
                kind,
                event = event.unwrap_or(""),
                error = %error,
                "Jarvis SSE client disconnected; continuing durable interaction workflow"
            );
            Ok(())
        } else {
            Err(error.into())
        }
    }

    fn is_client_disconnect_error(error: &std::io::Error) -> bool {
        matches!(
            error.kind(),
            std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::NotConnected
                | std::io::ErrorKind::UnexpectedEof
        )
    }

    async fn persist_interaction_event(
        db: Option<&Arc<dyn crate::db::traits::MissionStore>>,
        conversation_id: Option<&str>,
        interaction_id: Option<&str>,
        event: &str,
        payload: &serde_json::Value,
    ) {
        let (Some(db), Some(conversation_id), Some(interaction_id)) =
            (db, conversation_id, interaction_id)
        else {
            return;
        };
        if conversation_id.trim().is_empty() || interaction_id.trim().is_empty() {
            return;
        }
        let mut raw_data = payload.clone();
        if let Some(object) = raw_data.as_object_mut() {
            object
                .entry("interaction_id".to_string())
                .or_insert_with(|| serde_json::Value::String(interaction_id.to_string()));
            object
                .entry("event_kind".to_string())
                .or_insert_with(|| serde_json::Value::String(event.to_string()));
        }
        let content = raw_data
            .get("message")
            .or_else(|| raw_data.get("text"))
            .or_else(|| raw_data.get("phase"))
            .and_then(|value| value.as_str())
            .map(|value| value.chars().take(400).collect::<String>());
        let row = crate::types::ConversationEvent {
            id: 0,
            session_id: conversation_id.to_string(),
            event_uuid: Some(format!(
                "interaction-{}-{}-{}",
                interaction_id,
                event,
                uuid::Uuid::new_v4().simple()
            )),
            event_type: format!("interaction.{event}"),
            content,
            raw_data: Some(raw_data.to_string()),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        if let Err(error) = db.insert_conversation_events_batch(&[row]).await {
            warn!(
                %conversation_id,
                %interaction_id,
                %event,
                error = %error,
                "failed to persist interaction event"
            );
        }
    }

    async fn write_sse_openai_text(
        stream: &mut TcpStream,
        chat_id: &str,
        text: &str,
        finish_reason: Option<&str>,
    ) -> anyhow::Result<()> {
        let finish_reason_value = finish_reason
            .map(|reason| serde_json::Value::String(reason.to_string()))
            .unwrap_or(serde_json::Value::Null);
        let chunk = serde_json::json!({
            "id": chat_id,
            "object": "chat.completion.chunk",
            "model": "jarvis-missiond",
            "choices": [{
                "index": 0,
                "delta": {"content": text},
                "finish_reason": finish_reason_value
            }]
        });
        Self::write_sse_bytes(
            stream,
            format!("data: {chunk}\n\n").as_bytes(),
            "openai_delta",
            None,
        )
        .await
    }

    async fn write_jarvis_progress(
        stream: &mut TcpStream,
        progress_bus: &JarvisProgressBus,
        chat_id: &str,
        interaction_id: Option<&str>,
        phase: &str,
        step: &str,
        status: &str,
        message: &str,
        elapsed_secs: Option<u64>,
        slot_id: Option<&str>,
        author: Option<&str>,
    ) -> anyhow::Result<()> {
        let event_id = format!(
            "jarvis-progress-{}-{}-{}-{}",
            interaction_id.unwrap_or("unknown"),
            phase,
            step,
            uuid::Uuid::new_v4().simple()
        );
        let mut payload = serde_json::json!({
            "schema": "missiond.jarvis-progress.v1",
            "event_id": event_id.clone(),
            "event_kind": "jarvis_progress",
            "event_bus_required": true,
            "interaction_id": interaction_id,
            "phase": phase,
            "step": step,
            "status": status,
            "message": message,
            "text": message,
            "visible": true,
            "ui_surface": "progress_timeline",
            "openai_delta": false,
        });
        if let Some(object) = payload.as_object_mut() {
            if let Some(elapsed_secs) = elapsed_secs {
                object.insert(
                    "elapsed_secs".to_string(),
                    serde_json::Value::Number(elapsed_secs.into()),
                );
            }
            if let Some(slot_id) = slot_id {
                object.insert(
                    "slot_id".to_string(),
                    serde_json::Value::String(slot_id.to_string()),
                );
            }
            if let Some(author) = author {
                object.insert(
                    "author".to_string(),
                    serde_json::Value::String(author.to_string()),
                );
            }
        }
        let mut bus_rx = progress_bus
            .frontend_events_tx
            .as_ref()
            .map(|tx| tx.subscribe());
        let bus_write_ok =
            Self::publish_jarvis_progress_to_event_bus(progress_bus, &payload, message).await;
        let mut event_payload = if bus_write_ok {
            Self::read_jarvis_progress_from_event_bus(bus_rx.as_mut(), &event_id)
                .await
                .unwrap_or_else(|| {
                    Self::stamp_jarvis_progress_bus_projection(
                        payload.clone(),
                        "direct_fallback_after_bus_timeout",
                    )
                })
        } else {
            Self::stamp_jarvis_progress_bus_projection(
                payload.clone(),
                "direct_fallback_bus_write_failed",
            )
        };
        if let Some(object) = event_payload.as_object_mut() {
            object.insert(
                "event_bus_write_ok".to_string(),
                serde_json::Value::Bool(bus_write_ok),
            );
        }
        Self::write_sse_event(stream, "status", &event_payload).await?;
        if std::env::var("MISSIOND_JARVIS_PROGRESS_OPENAI_DELTA")
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        {
            Self::write_sse_openai_text(stream, chat_id, &format!("{message}\n"), None).await?;
        }
        Ok(())
    }

    async fn publish_jarvis_progress_to_event_bus(
        progress_bus: &JarvisProgressBus,
        payload: &serde_json::Value,
        message: &str,
    ) -> bool {
        let Some(tx) = progress_bus.system_event_tx.as_ref() else {
            return false;
        };
        let event_id = payload
            .get("event_id")
            .and_then(|value| value.as_str())
            .unwrap_or("jarvis-progress-missing-event-id")
            .to_string();
        let trace_id = payload
            .get("interaction_id")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let event = SystemEvent::ExternalServiceEvent {
            service_id: "missiond-jarvis".to_string(),
            event_id,
            event_kind: "jarvis_progress".to_string(),
            summary: message.chars().take(240).collect::<String>(),
            trace_id,
            payload_json: serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string()),
        };
        match tx.send(event).await {
            Ok(()) => true,
            Err(error) => {
                warn!(error = %error, "failed to publish Jarvis progress to EventBus");
                false
            }
        }
    }

    async fn read_jarvis_progress_from_event_bus(
        rx: Option<&mut broadcast::Receiver<String>>,
        event_id: &str,
    ) -> Option<serde_json::Value> {
        let rx = rx?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(1200);
        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return None;
            }
            let remaining = deadline.saturating_duration_since(now);
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(json_str)) => {
                    if let Some(payload) =
                        Self::jarvis_progress_payload_from_event_bus_wire(&json_str, event_id)
                    {
                        return Some(payload);
                    }
                }
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(broadcast::error::RecvError::Closed)) | Err(_) => return None,
            }
        }
    }

    fn jarvis_progress_payload_from_event_bus_wire(
        json_str: &str,
        expected_event_id: &str,
    ) -> Option<serde_json::Value> {
        let wire = serde_json::from_str::<serde_json::Value>(json_str).ok()?;
        if wire.get("type").and_then(|value| value.as_str()) != Some("external_service_event") {
            return None;
        }
        let payload = wire.get("payload")?;
        if payload.get("service_id").and_then(|value| value.as_str()) != Some("missiond-jarvis") {
            return None;
        }
        if payload.get("event_kind").and_then(|value| value.as_str()) != Some("jarvis_progress") {
            return None;
        }
        if payload.get("event_id").and_then(|value| value.as_str()) != Some(expected_event_id) {
            return None;
        }
        let payload_json = payload
            .get("payload_json")
            .and_then(|value| value.as_str())?;
        let progress = serde_json::from_str::<serde_json::Value>(payload_json).ok()?;
        Some(Self::stamp_jarvis_progress_bus_projection(
            progress,
            "frontend_event_bus",
        ))
    }

    fn stamp_jarvis_progress_bus_projection(
        mut payload: serde_json::Value,
        projection: &str,
    ) -> serde_json::Value {
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "event_bus_projection".to_string(),
                serde_json::Value::String(projection.to_string()),
            );
        }
        payload
    }

    async fn persist_jarvis_visible_assistant_message(
        db: Option<&Arc<dyn crate::db::traits::MissionStore>>,
        conversation_id: Option<&str>,
        text: &str,
    ) {
        let text = text.trim();
        let (Some(db), Some(conversation_id)) = (db, conversation_id) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        if let Err(error) = db
            .router_chat_append_messages(
                conversation_id,
                &[("assistant".to_string(), text.to_string())],
            )
            .await
        {
            warn!(%conversation_id, error = %error, "failed to persist visible Jarvis assistant text");
        }
    }

    async fn write_sse_openai_text_and_persist(
        stream: &mut TcpStream,
        chat_id: &str,
        text: &str,
        finish_reason: Option<&str>,
        db: Option<&Arc<dyn crate::db::traits::MissionStore>>,
        conversation_id: Option<&str>,
    ) -> anyhow::Result<()> {
        Self::persist_jarvis_visible_assistant_message(db, conversation_id, text).await;
        Self::write_sse_openai_text(stream, chat_id, text, finish_reason).await
    }

    fn clamp_jarvis_author_timeout_secs(value: Option<u64>) -> u64 {
        const DEFAULT_TIMEOUT_SECS: u64 = 180;
        const MIN_TIMEOUT_SECS: u64 = 30;
        const MAX_TIMEOUT_SECS: u64 = 300;
        value
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
    }

    fn clamp_jarvis_intent_author_timeout_secs(value: Option<u64>) -> u64 {
        Self::clamp_jarvis_author_timeout_secs(value)
    }

    fn clamp_jarvis_plan_author_timeout_secs(value: Option<u64>) -> u64 {
        Self::clamp_jarvis_author_timeout_secs(value)
    }

    fn clamp_jarvis_key_judgment_author_timeout_secs(value: Option<u64>) -> u64 {
        Self::clamp_jarvis_author_timeout_secs(value)
    }

    fn jarvis_intent_author_timeout_secs(config: &JarvisIntentAuthorConfig) -> u64 {
        Self::clamp_jarvis_intent_author_timeout_secs(
            std::env::var("MISSIOND_JARVIS_INTENT_AUTHOR_TIMEOUT_SECS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .or(Some(config.timeout_secs)),
        )
    }

    fn jarvis_plan_author_timeout_secs(config: &JarvisPlanAuthorConfig) -> u64 {
        Self::clamp_jarvis_plan_author_timeout_secs(
            std::env::var("MISSIOND_JARVIS_PLAN_AUTHOR_TIMEOUT_SECS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .or(Some(config.timeout_secs)),
        )
    }

    fn jarvis_key_judgment_author_timeout_secs(config: &JarvisKeyJudgmentAuthorConfig) -> u64 {
        Self::clamp_jarvis_key_judgment_author_timeout_secs(
            std::env::var("MISSIOND_JARVIS_KEY_JUDGMENT_AUTHOR_TIMEOUT_SECS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .or(Some(config.timeout_secs)),
        )
    }

    fn jarvis_author_text_provider() -> String {
        let requested = std::env::var("MISSIOND_JARVIS_AUTHOR_TEXT_ONLY_PROVIDER")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "codex_cli".to_string());
        match requested.as_str() {
            "codex" | "codex_cli" | "codex-cli" => "codex_cli".to_string(),
            _ if env_flag("MISSIOND_JARVIS_ALLOW_NON_CODEX_AUTHORS") => requested,
            _ => "codex_cli".to_string(),
        }
    }

    fn jarvis_text_only_slot_id(
        provider: &str,
        explicit: Option<&str>,
        default_slot: &str,
    ) -> String {
        if let Some(slot_id) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
            if Self::jarvis_slot_matches_provider(provider, slot_id) {
                return slot_id.to_string();
            }
        }
        let default_slot = default_slot.trim();
        if !default_slot.is_empty() && Self::jarvis_slot_matches_provider(provider, default_slot) {
            return default_slot.to_string();
        }
        match provider {
            "agy" | "agy_cli" | "agy-cli" => "slot-agy-gemini-31-pro-high".to_string(),
            "codex" | "codex_cli" | "codex-cli" => "slot-codex-intent-author".to_string(),
            "claude_code" | "claude-code" | "claude" => "slot-claude-code-default".to_string(),
            "gemini" | "gemini_cli" | "gemini-cli" => "slot-gemini-fast-survey".to_string(),
            _ => default_slot.to_string(),
        }
    }

    fn jarvis_slot_matches_provider(provider: &str, slot_id: &str) -> bool {
        let provider = provider.trim().to_ascii_lowercase();
        let slot_id = slot_id.trim().to_ascii_lowercase();
        match provider.as_str() {
            "agy" | "agy_cli" | "agy-cli" => slot_id.starts_with("slot-agy-"),
            "codex" | "codex_cli" | "codex-cli" => slot_id.starts_with("slot-codex-"),
            "claude_code" | "claude-code" | "claude" => slot_id.starts_with("slot-claude-"),
            "gemini" | "gemini_cli" | "gemini-cli" => slot_id.starts_with("slot-gemini-"),
            _ => true,
        }
    }

    fn env_var_trimmed(name: &str) -> Option<String> {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    fn jarvis_author_text_slot_id(provider: &str, default_slot: &str) -> String {
        Self::jarvis_text_only_slot_id(
            provider,
            std::env::var("MISSIOND_JARVIS_AUTHOR_TEXT_ONLY_SLOT_ID")
                .ok()
                .as_deref(),
            default_slot,
        )
    }

    fn jarvis_author_text_slot_id_for_phase(
        provider: &str,
        phase_slot_env: &str,
        default_slot: &str,
    ) -> String {
        let phase_slot = Self::env_var_trimmed(phase_slot_env);
        if let Some(slot_id) = phase_slot.as_deref() {
            if Self::jarvis_slot_matches_provider(provider, slot_id) {
                return slot_id.to_string();
            }
        }
        Self::jarvis_author_text_slot_id(provider, default_slot)
    }

    fn jarvis_author_text_model(provider: &str, default_model: &str) -> Option<String> {
        if let Some(model) = std::env::var("MISSIOND_JARVIS_AUTHOR_TEXT_ONLY_MODEL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            return Some(model);
        }
        match provider {
            "codex" | "codex_cli" | "codex-cli" => Some(default_model.to_string()),
            _ => None,
        }
    }

    fn jarvis_author_text_authority(
        provider: &str,
        model: Option<&str>,
        reasoning_effort: &str,
    ) -> String {
        let provider_label = provider.replace('_', "-");
        match model.map(str::trim).filter(|value| !value.is_empty()) {
            Some(model) => format!("{provider_label}-{model}-{reasoning_effort}"),
            None => format!("{provider_label}-provider-current-{reasoning_effort}"),
        }
    }

    fn jarvis_author_progress_identity(
        default_slot: &str,
        default_model: &str,
        reasoning_effort: &str,
    ) -> (String, String, String) {
        let provider = Self::jarvis_author_text_provider();
        let slot_id = Self::jarvis_author_text_slot_id(provider.as_str(), default_slot);
        let model = Self::jarvis_author_text_model(provider.as_str(), default_model);
        let authority = Self::jarvis_author_text_authority(
            provider.as_str(),
            model.as_deref(),
            reasoning_effort,
        );
        (provider, slot_id, authority)
    }

    fn jarvis_codex_intent_prompt(
        config: &JarvisIntentAuthorConfig,
        schema: &str,
        channel: &str,
        objective: &str,
        grounding_context_id: &str,
        topic_id: Option<&str>,
        topic_label: Option<&str>,
        sources_used: &[String],
        permission_context: Option<&serde_json::Value>,
        media_context: &serde_json::Value,
    ) -> String {
        let provider = Self::jarvis_author_text_provider();
        let engine = Self::provider_box_engine_for_provider(provider.as_str()).unwrap_or("codex");
        let slot_id = Self::jarvis_author_text_slot_id(provider.as_str(), &config.slot_id);
        let model = Self::jarvis_author_text_model(provider.as_str(), &config.model);
        let input = serde_json::json!({
            "schema": schema,
            "channel": channel,
            "original_user_message": objective,
            "grounding_context_id": grounding_context_id,
            "topic_id": topic_id,
            "topic_label": topic_label,
            "sources_used": sources_used,
            "permission_context": permission_context.cloned().unwrap_or(serde_json::Value::Null),
            "media_context": media_context,
            "authoring_lane": {
                "provider": provider,
                "engine": engine,
                "slot_id": slot_id,
                "model": model,
                "reasoning_effort": config.reasoning_effort,
                "sandbox": config.sandbox,
                "approval_policy": config.approval_policy
            }
        });
        format!(
            "你是 MissionD 的 Jarvis intent.lisp 语义作者，运行在 provider_box 管理的纯文本语义作者工位。\n\
任务：识别用户真实意图，并输出一个可供用户确认的 intent draft。不要创建任务、不要改文件、不要派工位。\n\
只返回一个严格 JSON object，不要 Markdown，不要代码围栏，不要额外解释。JSON key 必须使用双引号。\n\
JSON 字段必须是：\n\
  recognized_objective: string，归一化后的用户目标，保留用户本意，不要扩大范围。\n\
  intent_kind: string，例如 greeting/question/review/design/implementation/ops/unknown。\n\
  understanding: string，用中文说明你理解的真实意图。\n\
  review_text: string，给用户看的审阅摘要，必须说明边界：只确认意图，确认后才生成 plan.lisp。\n\
  assumptions: string[]，你做出的假设。\n\
  non_goals: string[]，明确不做什么。\n\
  acceptance_signals: string[]，用户确认时实际接受的内容。\n\
  confidence: string，low/medium/high。\n\
\n\
输入 JSON：\n{}\n",
            serde_json::to_string_pretty(&input).unwrap_or_else(|_| "{}".to_string())
        )
    }

    fn jarvis_codex_key_judgment_prompt(
        config: &JarvisKeyJudgmentAuthorConfig,
        schema: &str,
        channel: &str,
        objective: &str,
        grounding_context_id: &str,
        intent_artifact_id: &str,
        topic_id: Option<&str>,
        topic_label: Option<&str>,
        sources_used: &[String],
        permission_context: Option<&serde_json::Value>,
        context_pack_path: Option<&str>,
        context_pack_file: Option<&str>,
        grounding_report_file: Option<&str>,
        grounding_report_artifact_path: Option<&str>,
        grounding_report_hash: Option<&str>,
        context_sufficiency: Option<&str>,
        grounding_report_preview: Option<&str>,
    ) -> String {
        let provider = Self::jarvis_author_text_provider();
        let engine = Self::provider_box_engine_for_provider(provider.as_str()).unwrap_or("codex");
        let slot_id = Self::jarvis_author_text_slot_id(provider.as_str(), &config.slot_id);
        let model = Self::jarvis_author_text_model(provider.as_str(), &config.model);
        let input = serde_json::json!({
            "schema": schema,
            "channel": channel,
            "confirmed_intent_objective": objective,
            "grounding_context_id": grounding_context_id,
            "intent_artifact_id": intent_artifact_id,
            "topic_id": topic_id,
            "topic_label": topic_label,
            "sources_used": sources_used,
            "permission_context": permission_context.cloned().unwrap_or(serde_json::Value::Null),
            "context_pack_path": context_pack_path,
            "context_pack_file": context_pack_file,
            "grounding_report_file": grounding_report_file,
            "grounding_report_artifact_path": grounding_report_artifact_path,
            "grounding_report_hash": grounding_report_hash,
            "context_sufficiency": context_sufficiency,
            "grounding_report_preview": grounding_report_preview,
            "authoring_lane": {
                "provider": provider,
                "engine": engine,
                "slot_id": slot_id,
                "model": model,
                "reasoning_effort": config.reasoning_effort,
                "sandbox": config.sandbox,
                "approval_policy": config.approval_policy
            }
        });
        format!(
            "你是 MissionD 的 Jarvis 关键判断作者，运行在 provider_box 管理的纯文本语义作者工位。\n\
任务：基于已确认 intent.lisp 目标和 ClaudeCode + MissionD MCP grounding report，给出一个【关键判断】，供下一步 plan.lisp 使用。不要创建 plan，不要创建 BoardTask，不要改文件，不要部署。\n\
关键判断必须是能改变后续计划路线的一句话，例如“不是算力差异，是用量差异”。你需要同时指出被排除的假设、证据引用、对计划拆分的影响和后续验收重点。\n\
只返回一个严格 JSON object，不要 Markdown，不要代码围栏，不要额外解释。JSON key 必须使用双引号。\n\
JSON 字段必须是：\n\
  judgment: string，一句话关键判断，中文，必须具体，不能是泛泛的“需要进一步调查”。\n\
  review_text: string，给用户看的审阅摘要，说明这是 plan 前的关键判断，不是执行结果。\n\
  confidence: string，low/medium/high。\n\
  rejected_hypotheses: string[]，被 grounding 证据排除或降级的假设。\n\
  evidence_refs: string[]，引用 grounding report/context 中的证据短标签或文件/段落描述。\n\
  planning_implications: string[]，这条判断如何影响 plan 的任务拆分、串并行和工位分配。\n\
  acceptance_focus: string[]，后续 Codex 验收需要重点判断什么。\n\
\n\
输入 JSON：\n{}\n",
            serde_json::to_string_pretty(&input).unwrap_or_else(|_| "{}".to_string())
        )
    }

    fn jarvis_codex_plan_prompt(
        config: &JarvisPlanAuthorConfig,
        schema: &str,
        channel: &str,
        objective: &str,
        grounding_context_id: &str,
        intent_artifact_id: &str,
        key_judgment: &JarvisKeyJudgmentArtifactRef,
        topic_id: Option<&str>,
        topic_label: Option<&str>,
        sources_used: &[String],
        permission_context: Option<&serde_json::Value>,
        context_pack_path: Option<&str>,
        context_pack_file: Option<&str>,
        grounding_report_file: Option<&str>,
        grounding_report_artifact_path: Option<&str>,
        grounding_report_hash: Option<&str>,
        grounding_worker_slot_id: Option<&str>,
        grounding_worker_turn_id: Option<&str>,
        context_sufficiency: Option<&str>,
        grounding_report_preview: Option<&str>,
    ) -> String {
        let provider = Self::jarvis_author_text_provider();
        let engine = Self::provider_box_engine_for_provider(provider.as_str()).unwrap_or("codex");
        let slot_id = Self::jarvis_author_text_slot_id(provider.as_str(), &config.slot_id);
        let model = Self::jarvis_author_text_model(provider.as_str(), &config.model);
        let input = serde_json::json!({
            "schema": schema,
            "channel": channel,
            "confirmed_intent_objective": objective,
            "grounding_context_id": grounding_context_id,
            "intent_artifact_id": intent_artifact_id,
            "key_judgment_artifact_id": key_judgment.artifact_id,
            "key_judgment_artifact_hash": key_judgment.artifact_hash,
            "key_judgment_artifact_path": key_judgment.artifact_path,
            "key_judgment": key_judgment.judgment,
            "key_judgment_review_text": key_judgment.review_text,
            "key_judgment_confidence": key_judgment.confidence,
            "rejected_hypotheses": key_judgment.rejected_hypotheses,
            "key_judgment_evidence_refs": key_judgment.evidence_refs,
            "planning_implications": key_judgment.planning_implications,
            "acceptance_focus": key_judgment.acceptance_focus,
            "topic_id": topic_id,
            "topic_label": topic_label,
            "sources_used": sources_used,
            "permission_context": permission_context.cloned().unwrap_or(serde_json::Value::Null),
            "context_pack_path": context_pack_path,
            "context_pack_file": context_pack_file,
            "grounding_report_file": grounding_report_file,
            "grounding_report_artifact_path": grounding_report_artifact_path,
            "grounding_report_hash": grounding_report_hash,
            "grounding_worker_slot_id": grounding_worker_slot_id,
            "grounding_worker_turn_id": grounding_worker_turn_id,
            "context_sufficiency": context_sufficiency,
            "grounding_report_preview": grounding_report_preview,
            "authoring_lane": {
                "provider": provider,
                "engine": engine,
                "slot_id": slot_id,
                "model": model,
                "reasoning_effort": config.reasoning_effort,
                "sandbox": config.sandbox,
                "approval_policy": config.approval_policy
            }
        });
        format!(
            "你是 MissionD 的 Jarvis plan.lisp 语义作者，运行在 provider_box 管理的纯文本语义作者工位。\n\
任务：基于已确认的 intent.lisp 目标、ClaudeCode + MissionD MCP grounding worker 写入的 Markdown 报告，以及 Codex key judgment artifact，生成可供用户确认的 plan draft。不要创建 BoardTask、不要派工位、不要执行实现、不要改文件。\n\
计划必须引用 key_judgment_artifact_id，并把 planning_implications 转化为可执行拆分。现阶段即使是聊天/问答也必须先经过 intent.lisp、grounding report、key judgment 和 plan.lisp；如果 grounding_report_preview + key_judgment 已足够直接回复用户，应选择 grounded_direct_answer，不创建 BoardTask。\n\
如果 grounding report 或 key judgment 证据不足，或者需要改代码/部署/长任务/进一步工位执行，选择 work_order 或 investigation_only，并说明缺口和边界。\n\
只返回一个严格 JSON object，不要 Markdown，不要代码围栏，不要额外解释。JSON key 必须使用双引号。\n\
JSON 字段必须是：\n\
  objective: string，必须等同或更保守地表达已确认意图，不要扩大范围。\n\
  review_text: string，给用户看的审阅摘要，必须说明边界：确认 plan 后才会进入 execution_mode 指定路径，结果以 artifact 为准。\n\
  execution_mode: string，只能是 grounded_direct_answer、work_order、investigation_only 三者之一。普通问答/解释/身份确认/状态说明选 grounded_direct_answer；需要改代码、部署、长期运行或多工位任务选 work_order；只读调查但需要工位证据选 investigation_only。\n\
  requires_board_task: boolean。grounded_direct_answer 必须是 false；work_order 和 investigation_only 必须是 true。\n\
  direct_answer_draft: string|null。grounded_direct_answer 时必须给出基于 grounding_report_preview、key_judgment 和 sources_used 的用户可见中文答案草稿，先用一到三行说明当前执行到哪一步，再直接回答用户；work_order/investigation_only 时必须为 null 或空字符串。\n\
  key_judgment: string，必须复述输入里的关键判断。\n\
  steps: string[]，2 到 6 个中文步骤，每步是可审阅的计划动作，不是执行结果。\n\
  answer_policy: string，说明直接回答或工位结果如何使用 grounding sources；grounded_direct_answer 时必须说明优先使用 plan direct_answer_draft 直接终止，必要时才使用 provider_box grounded-direct-answer fallback，且不创建 BoardTask。\n\
  provider_hint: string，例如 provider-box-codex、codex-review-worker、claude-code-default。\n\
  boundary: string，计划确认边界和不执行承诺。\n\
  assumptions: string[]，你做出的假设。\n\
  non_goals: string[]，明确不做什么。\n\
  acceptance_signals: string[]，用户确认 plan 时实际接受的内容。\n\
  confidence: string，low/medium/high。\n\
  workstreams: object[]。grounded_direct_answer 可为空；work_order/investigation_only 必须最多 10 个 workstream，每个 workstream 包含 id/title/objective/execution_order/depends_on/parallel_group/atoms。\n\
  atom_tasks: object[]。grounded_direct_answer 可为空；work_order/investigation_only 必须列出所有 atom，字段包含 atom_task_id/workstream_id/objective/category/assignee_engine/execution_order/depends_on/parallel_group/read_scope/write_scope/acceptance。category 只能是 query/code_change/deploy_ops/judgment/acceptance；query/code_change/deploy_ops 必须 assignee_engine=claude_code；judgment/acceptance 必须 assignee_engine=codex。\n\
  dependency_edges: object[]，串行依赖边。\n\
  serial_groups: object[]，串行组。\n\
  parallel_groups: object[]，可并行组。\n\
  assignment_policy: object，必须声明 query/code_change/deploy_ops -> claude_code，judgment/acceptance -> codex。\n\
\n\
输入 JSON：\n{}\n",
            serde_json::to_string_pretty(&input).unwrap_or_else(|_| "{}".to_string())
        )
    }

    fn jarvis_codex_output_sample(text: &str) -> String {
        text.chars().take(500).collect::<String>()
    }

    fn provider_box_internal_authorization_header() -> Option<String> {
        std::env::var("MISSIOND_PROVIDER_BOX_INTERNAL_TOKEN")
            .ok()
            .or_else(|| std::env::var("MISSIOND_AGY_INTERNAL_TOKEN").ok())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(|token| format!("Bearer {token}"))
    }

    async fn call_provider_box_turn(
        provider_box_http: &ProviderBoxHttpSlot,
        body: serde_json::Value,
        timeout_secs: u64,
        error_prefix: &str,
    ) -> anyhow::Result<String> {
        let callback = {
            let guard = provider_box_http.read().await;
            guard.clone()
        };
        let Some(callback) = callback else {
            anyhow::bail!("{error_prefix}_UNAVAILABLE: provider-box adapter is not configured");
        };
        let mut headers = HashMap::new();
        if let Some(authorization) = Self::provider_box_internal_authorization_header() {
            headers.insert("authorization".to_string(), authorization);
        }
        let request = ProviderBoxHttpRequest {
            method: "POST".to_string(),
            path: "/provider-box/v1/turns".to_string(),
            headers,
            body,
        };
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs.saturating_add(15)),
            callback(request),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!("{error_prefix}_TIMEOUT: provider-box turn exceeded {timeout_secs}s")
        })?
        .map_err(|err| anyhow::anyhow!("{error_prefix}_UNAVAILABLE: provider-box failed: {err}"))?;
        if !(200..300).contains(&response.status) {
            anyhow::bail!(
                "{}_FAILED: provider-box returned {}: {}",
                error_prefix,
                response.status,
                Self::jarvis_codex_output_sample(&response.body.to_string())
            );
        }
        response
            .body
            .get("final_text")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{}_FAILED: provider-box completed without final_text: {}",
                    error_prefix,
                    Self::jarvis_codex_output_sample(&response.body.to_string())
                )
            })
    }

    async fn run_jarvis_codex_author_turn(
        provider_box_http: &ProviderBoxHttpSlot,
        slot_id: &str,
        model: &str,
        reasoning_effort: &str,
        search_enabled: bool,
        sandbox: &str,
        approval_policy: &str,
        timeout_secs: u64,
        output_prefix: &str,
        slot_override_env: &str,
        env_marker: &str,
        error_prefix: &str,
        prompt: &str,
    ) -> anyhow::Result<String> {
        let cwd = Self::jarvis_slot_runtime_cwd();
        let correlation_id = format!("jarvis-{}-{}", output_prefix, uuid::Uuid::new_v4().simple());
        let provider = Self::jarvis_author_text_provider();
        let engine = Self::provider_box_engine_for_provider(provider.as_str())?;
        let slot_id = Self::jarvis_author_text_slot_id_for_phase(
            provider.as_str(),
            slot_override_env,
            slot_id,
        );
        let model = Self::jarvis_author_text_model(provider.as_str(), model);
        let pure_text_command = engine == "agy";
        let command = if pure_text_command {
            "pure-text-single-turn"
        } else {
            "semantic-authoring"
        };
        let output_contract = if pure_text_command {
            serde_json::json!({
                "media_type": "text/plain",
                "single_turn": true
            })
        } else {
            serde_json::json!({
                "media_type": "application/json",
                "artifact": output_prefix
            })
        };
        let body = serde_json::json!({
            "schema": "missiond.provider-interaction-request.v1",
            "command": command,
            "provider": provider,
            "engine": engine,
            "model": model,
            "model_profile": reasoning_effort,
            "cwd": cwd.display().to_string(),
            "project_root": cwd.display().to_string(),
            "prompt": prompt,
            "timeout_secs": timeout_secs,
            "correlation_id": correlation_id,
            "slot_id": slot_id,
            "no_tools": true,
            "no_mcp": true,
            "no_shell": true,
            "no_file_access": true,
            "tool_policy": {
                "sandbox": sandbox,
                "approval_policy": approval_policy,
                "search_enabled": search_enabled,
                "env_marker": env_marker,
                "started_at": chrono::Utc::now().to_rfc3339()
            },
            "output_contract": output_contract
        });
        Self::call_provider_box_turn(provider_box_http, body, timeout_secs, error_prefix).await
    }

    async fn run_jarvis_codex_intent_turn(
        provider_box_http: &ProviderBoxHttpSlot,
        config: &JarvisIntentAuthorConfig,
        prompt: &str,
    ) -> anyhow::Result<String> {
        Self::run_jarvis_codex_author_turn(
            provider_box_http,
            &config.slot_id,
            &config.model,
            &config.reasoning_effort,
            config.search_enabled,
            &config.sandbox,
            &config.approval_policy,
            Self::jarvis_intent_author_timeout_secs(config),
            "intent",
            "MISSIOND_JARVIS_INTENT_AUTHOR_SLOT_ID",
            "MISSIOND_JARVIS_INTENT_AUTHOR",
            "JARVIS_INTENT_AUTHOR",
            prompt,
        )
        .await
    }

    async fn run_jarvis_codex_key_judgment_turn(
        provider_box_http: &ProviderBoxHttpSlot,
        config: &JarvisKeyJudgmentAuthorConfig,
        prompt: &str,
    ) -> anyhow::Result<String> {
        Self::run_jarvis_codex_author_turn(
            provider_box_http,
            &config.slot_id,
            &config.model,
            &config.reasoning_effort,
            config.search_enabled,
            &config.sandbox,
            &config.approval_policy,
            Self::jarvis_key_judgment_author_timeout_secs(config),
            "key-judgment",
            "MISSIOND_JARVIS_KEY_JUDGMENT_AUTHOR_SLOT_ID",
            "MISSIOND_JARVIS_KEY_JUDGMENT_AUTHOR",
            "JARVIS_KEY_JUDGMENT_AUTHOR",
            prompt,
        )
        .await
    }

    async fn run_jarvis_codex_plan_turn(
        provider_box_http: &ProviderBoxHttpSlot,
        config: &JarvisPlanAuthorConfig,
        prompt: &str,
    ) -> anyhow::Result<String> {
        Self::run_jarvis_codex_author_turn(
            provider_box_http,
            &config.slot_id,
            &config.model,
            &config.reasoning_effort,
            config.search_enabled,
            &config.sandbox,
            &config.approval_policy,
            Self::jarvis_plan_author_timeout_secs(config),
            "plan",
            "MISSIOND_JARVIS_PLAN_AUTHOR_SLOT_ID",
            "MISSIOND_JARVIS_PLAN_AUTHOR",
            "JARVIS_PLAN_AUTHOR",
            prompt,
        )
        .await
    }

    fn extract_json_object(text: &str) -> Option<&str> {
        let bytes = text.as_bytes();
        let mut start = None;
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        for (idx, byte) in bytes.iter().enumerate() {
            let ch = *byte as char;
            if start.is_none() {
                if ch == '{' {
                    start = Some(idx);
                    depth = 1;
                }
                continue;
            }
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }
            match ch {
                '"' => in_string = true,
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return start.map(|s| &text[s..=idx]);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn parse_codex_intent_response(text: &str) -> anyhow::Result<JarvisCodexIntentResponse> {
        let json_text = Self::extract_json_object(text)
            .ok_or_else(|| anyhow::anyhow!("Codex intent author did not return a JSON object"))?;
        let parsed: JarvisCodexIntentResponse = serde_json::from_str(json_text)?;
        if parsed.recognized_objective.trim().is_empty()
            || parsed.intent_kind.trim().is_empty()
            || parsed.understanding.trim().is_empty()
            || parsed.review_text.trim().is_empty()
        {
            anyhow::bail!("Codex intent author returned an incomplete intent draft");
        }
        Ok(parsed)
    }

    fn parse_codex_key_judgment_response(
        text: &str,
    ) -> anyhow::Result<JarvisCodexKeyJudgmentResponse> {
        let json_text = Self::extract_json_object(text).ok_or_else(|| {
            anyhow::anyhow!("Codex key judgment author did not return a JSON object")
        })?;
        let mut parsed: JarvisCodexKeyJudgmentResponse = serde_json::from_str(json_text)?;
        parsed.judgment = parsed.judgment.trim().to_string();
        parsed.review_text = parsed.review_text.trim().to_string();
        parsed.rejected_hypotheses = parsed
            .rejected_hypotheses
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect();
        parsed.evidence_refs = parsed
            .evidence_refs
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect();
        parsed.planning_implications = parsed
            .planning_implications
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect();
        parsed.acceptance_focus = parsed
            .acceptance_focus
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect();
        if parsed.judgment.is_empty()
            || parsed.review_text.is_empty()
            || parsed.planning_implications.is_empty()
            || parsed.acceptance_focus.is_empty()
        {
            anyhow::bail!("Codex key judgment author returned an incomplete key judgment draft");
        }
        Ok(parsed)
    }

    fn jarvis_assignment_policy_default() -> serde_json::Value {
        serde_json::json!({
            "query": "claude_code",
            "code_change": "claude_code",
            "deploy_ops": "claude_code",
            "judgment": "codex",
            "acceptance": "codex"
        })
    }

    fn normalize_jarvis_plan_assignee(value: &str) -> String {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "claude" | "claudecode" | "claude_code" => "claude_code".to_string(),
            "codex_cli" | "codex" => "codex".to_string(),
            other => other.to_string(),
        }
    }

    fn jarvis_plan_atom_values(parsed: &JarvisCodexPlanResponse) -> Vec<&serde_json::Value> {
        if !parsed.atom_tasks.is_empty() {
            return parsed.atom_tasks.iter().collect::<Vec<_>>();
        }
        parsed
            .workstreams
            .iter()
            .flat_map(|workstream| {
                workstream
                    .get("atoms")
                    .and_then(|value| value.as_array())
                    .into_iter()
                    .flatten()
            })
            .collect()
    }

    fn jarvis_json_string_list(value: Option<&serde_json::Value>) -> Vec<String> {
        match value {
            Some(serde_json::Value::Array(items)) => items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
            Some(serde_json::Value::String(text)) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    Vec::new()
                } else if trimmed.contains(',') {
                    trimmed
                        .split(',')
                        .map(str::trim)
                        .filter(|item| !item.is_empty())
                        .map(ToOwned::to_owned)
                        .collect()
                } else {
                    vec![trimmed.to_string()]
                }
            }
            _ => Vec::new(),
        }
    }

    fn jarvis_atom_depends_on(atom: &serde_json::Value) -> Vec<String> {
        Self::jarvis_json_string_list(atom.get("depends_on").or_else(|| atom.get("dependsOn")))
    }

    fn jarvis_atom_id(atom: &serde_json::Value) -> Option<String> {
        json_string_field(atom, &["atom_task_id", "atomTaskId", "id"])
    }

    fn jarvis_plan_atom_specs(
        plan_atomization_graph: &serde_json::Value,
    ) -> anyhow::Result<Vec<JarvisPlanAtomTask>> {
        let atom_values: Vec<&serde_json::Value> = plan_atomization_graph
            .get("atom_tasks")
            .and_then(|value| value.as_array())
            .filter(|items| !items.is_empty())
            .map(|items| items.iter().collect())
            .unwrap_or_else(|| {
                plan_atomization_graph
                    .get("workstreams")
                    .and_then(|value| value.as_array())
                    .into_iter()
                    .flatten()
                    .flat_map(|workstream| {
                        workstream
                            .get("atoms")
                            .and_then(|value| value.as_array())
                            .into_iter()
                            .flatten()
                    })
                    .collect()
            });
        let mut seen = HashSet::new();
        let mut specs = Vec::with_capacity(atom_values.len());
        for atom in atom_values {
            let atom_task_id = Self::jarvis_atom_id(atom)
                .ok_or_else(|| anyhow::anyhow!("plan atom missing atom_task_id"))?;
            if !seen.insert(atom_task_id.clone()) {
                anyhow::bail!("plan atom id {atom_task_id} is duplicated");
            }
            let category = json_string_field(atom, &["category"])
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("plan atom {atom_task_id} missing category"))?;
            let assignee_engine =
                json_string_field(atom, &["assignee_engine", "assigneeEngine", "assignee"])
                    .map(|value| Self::normalize_jarvis_plan_assignee(&value))
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!("plan atom {atom_task_id} missing assignee_engine")
                    })?;
            let execution_order = json_string_field(atom, &["execution_order", "executionOrder"])
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("plan atom {atom_task_id} missing execution_order")
                })?;
            let objective = json_string_field(atom, &["objective", "title", "description"])
                .unwrap_or_else(|| atom_task_id.clone());
            let depends_on = Self::jarvis_atom_depends_on(atom);
            specs.push(JarvisPlanAtomTask {
                atom_task_id,
                workstream_id: json_string_field(atom, &["workstream_id", "workstreamId"]),
                objective,
                category,
                assignee_engine,
                execution_order,
                depends_on,
                parallel_group: json_string_field(atom, &["parallel_group", "parallelGroup"]),
                read_scope: Self::jarvis_json_string_list(
                    atom.get("read_scope").or_else(|| atom.get("readScope")),
                ),
                write_scope: Self::jarvis_json_string_list(
                    atom.get("write_scope").or_else(|| atom.get("writeScope")),
                ),
                acceptance: Self::jarvis_json_string_list(atom.get("acceptance")),
                raw: atom.clone(),
            });
        }
        let valid_ids = specs
            .iter()
            .map(|spec| spec.atom_task_id.as_str())
            .collect::<HashSet<_>>();
        for spec in &specs {
            for dep in &spec.depends_on {
                if !valid_ids.contains(dep.as_str()) {
                    anyhow::bail!(
                        "plan atom {} depends_on unknown atom {}",
                        spec.atom_task_id,
                        dep
                    );
                }
            }
        }
        Ok(specs)
    }

    fn validate_jarvis_plan_atomization(parsed: &JarvisCodexPlanResponse) -> anyhow::Result<()> {
        if parsed.execution_mode == "grounded_direct_answer" {
            return Ok(());
        }
        if parsed.workstreams.len() > 10 {
            anyhow::bail!(
                "{} plan has {} workstreams; maximum is 10",
                parsed.execution_mode,
                parsed.workstreams.len()
            );
        }
        for workstream in &parsed.workstreams {
            if let Some(items) = workstream.get("atoms").and_then(|value| value.as_array()) {
                if items.len() > 10 {
                    let id = json_string_field(workstream, &["id"])
                        .unwrap_or_else(|| "<missing-workstream-id>".to_string());
                    anyhow::bail!("workstream {id} has {} atoms; maximum is 10", items.len());
                }
            }
        }
        let atoms = Self::jarvis_plan_atom_values(parsed);
        if parsed.workstreams.is_empty() || atoms.is_empty() {
            anyhow::bail!(
                "{} plan must include workstreams and atom_tasks for MissionD atomized dispatch",
                parsed.execution_mode
            );
        }
        let mut atom_ids = HashSet::new();
        for atom in atoms {
            let id = Self::jarvis_atom_id(atom).unwrap_or_else(|| "<missing-atom-id>".to_string());
            if !atom_ids.insert(id.to_string()) {
                anyhow::bail!("plan atom id {id} is duplicated");
            }
            let category = atom
                .get("category")
                .and_then(|value| value.as_str())
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("plan atom {id} missing category"))?;
            let expected = match category.as_str() {
                "query" | "code_change" | "deploy_ops" => "claude_code",
                "judgment" | "acceptance" => "codex",
                other => {
                    anyhow::bail!(
                        "plan atom {id} has unsupported category {other}; expected query/code_change/deploy_ops/judgment/acceptance"
                    );
                }
            };
            let assignee = atom
                .get("assignee_engine")
                .or_else(|| atom.get("assignee"))
                .and_then(|value| value.as_str())
                .map(Self::normalize_jarvis_plan_assignee)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("plan atom {id} missing assignee_engine"))?;
            if assignee != expected {
                anyhow::bail!(
                    "plan atom {id} category {category} must assign to {expected}, got {assignee}"
                );
            }
            let execution_order = atom
                .get("execution_order")
                .and_then(|value| value.as_str())
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("plan atom {id} missing execution_order"))?;
            if !matches!(execution_order.as_str(), "serial" | "parallel") {
                anyhow::bail!(
                    "plan atom {id} execution_order must be serial or parallel, got {execution_order}"
                );
            }
        }
        for atom in Self::jarvis_plan_atom_values(parsed) {
            let id = Self::jarvis_atom_id(atom).unwrap_or_else(|| "<missing-atom-id>".to_string());
            for dep in Self::jarvis_atom_depends_on(atom) {
                if !atom_ids.contains(&dep) {
                    anyhow::bail!("plan atom {id} depends_on unknown atom {dep}");
                }
            }
        }
        Ok(())
    }

    fn parse_codex_plan_response(text: &str) -> anyhow::Result<JarvisCodexPlanResponse> {
        let json_text = Self::extract_json_object(text)
            .ok_or_else(|| anyhow::anyhow!("Codex plan author did not return a JSON object"))?;
        let mut parsed: JarvisCodexPlanResponse = serde_json::from_str(json_text)?;
        parsed.execution_mode = parsed.execution_mode.trim().to_ascii_lowercase();
        parsed.steps = parsed
            .steps
            .into_iter()
            .map(|step| step.trim().to_string())
            .filter(|step| !step.is_empty())
            .collect();
        if parsed.objective.trim().is_empty()
            || parsed.review_text.trim().is_empty()
            || parsed.execution_mode.trim().is_empty()
            || parsed.steps.is_empty()
        {
            anyhow::bail!("Codex plan author returned an incomplete plan draft");
        }
        match parsed.execution_mode.as_str() {
            "grounded_direct_answer" => {
                if parsed.requires_board_task {
                    anyhow::bail!("grounded_direct_answer plan must set requires_board_task=false");
                }
                parsed.direct_answer_draft = parsed
                    .direct_answer_draft
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty());
                if parsed.direct_answer_draft.is_none() {
                    anyhow::bail!("grounded_direct_answer plan must include direct_answer_draft");
                }
            }
            "work_order" | "investigation_only" => {
                if !parsed.requires_board_task {
                    anyhow::bail!(
                        "{} plan must set requires_board_task=true",
                        parsed.execution_mode
                    );
                }
                parsed.direct_answer_draft = None;
            }
            other => {
                anyhow::bail!(
                    "Codex plan author returned unsupported execution_mode: {}",
                    other
                );
            }
        }
        if parsed.assignment_policy.is_null() {
            parsed.assignment_policy = Self::jarvis_assignment_policy_default();
        }
        Self::validate_jarvis_plan_atomization(&parsed)?;
        Ok(parsed)
    }

    fn jarvis_authored_intent_lisp_body(
        schema: &str,
        config: &JarvisIntentAuthorConfig,
        channel: &str,
        original_message: &str,
        draft: &JarvisCodexIntentResponse,
        grounding_context_id: &str,
        topic_id: Option<&str>,
        topic_label: Option<&str>,
        sources_used: &[String],
        media_context: &serde_json::Value,
    ) -> String {
        let provider = Self::jarvis_author_text_provider();
        let engine = Self::provider_box_engine_for_provider(provider.as_str()).unwrap_or("codex");
        let slot_id = Self::jarvis_author_text_slot_id(provider.as_str(), &config.slot_id);
        let model = Self::jarvis_author_text_model(provider.as_str(), &config.model);
        let authority = Self::jarvis_author_text_authority(
            provider.as_str(),
            model.as_deref(),
            &config.reasoning_effort,
        );
        format!(
            "(intent-draft\n  :schema {}\n  :authority {}\n  :semantic-author (:provider {} :engine {} :slot-id {} :model {} :reasoning-effort {} :sandbox {} :approval-policy {})\n  :channel {}\n  :original-message {}\n  :objective {}\n  :intent-kind {}\n  :confidence {}\n  :understanding {}\n  :grounding-context-id {}\n  :topic-id {}\n  :topic-label {}\n  :sources-used {}\n  :media-context-json {}\n  :assumptions {}\n  :non-goals {}\n  :acceptance-signals {}\n  :approval (:state awaiting-intent-confirmation :required true)\n  :next-step \"confirm intent -> collect MissionD MCP grounding report -> generate plan.lisp -> confirm plan -> direct answer or BoardTask\")",
            Self::jarvis_lisp_string(schema),
            Self::jarvis_lisp_string(&authority),
            Self::jarvis_lisp_string(&provider),
            Self::jarvis_lisp_string(engine),
            Self::jarvis_lisp_string(&slot_id),
            Self::jarvis_lisp_optional(model.as_deref()),
            Self::jarvis_lisp_string(&config.reasoning_effort),
            Self::jarvis_lisp_string(&config.sandbox),
            Self::jarvis_lisp_string(&config.approval_policy),
            Self::jarvis_lisp_string(channel),
            Self::jarvis_lisp_string(original_message),
            Self::jarvis_lisp_string(draft.recognized_objective.trim()),
            Self::jarvis_lisp_string(draft.intent_kind.trim()),
            Self::jarvis_lisp_optional(draft.confidence.as_deref()),
            Self::jarvis_lisp_string(draft.understanding.trim()),
            Self::jarvis_lisp_string(grounding_context_id),
            Self::jarvis_lisp_optional(topic_id),
            Self::jarvis_lisp_optional(topic_label),
            Self::jarvis_lisp_string_list(sources_used),
            Self::jarvis_lisp_json(media_context),
            Self::jarvis_lisp_string_list(&draft.assumptions),
            Self::jarvis_lisp_string_list(&draft.non_goals),
            Self::jarvis_lisp_string_list(&draft.acceptance_signals),
        )
    }

    async fn author_jarvis_intent_draft(
        provider_box_http: &ProviderBoxHttpSlot,
        config: &JarvisIntentAuthorConfig,
        schema: &str,
        channel: &str,
        objective: &str,
        grounding_context_id: &str,
        topic_id: Option<&str>,
        topic_label: Option<&str>,
        sources_used: &[String],
        permission_context: Option<&serde_json::Value>,
        media_context: &serde_json::Value,
    ) -> anyhow::Result<JarvisAuthoredIntentDraft> {
        let prompt = Self::jarvis_codex_intent_prompt(
            config,
            schema,
            channel,
            objective,
            grounding_context_id,
            topic_id,
            topic_label,
            sources_used,
            permission_context,
            media_context,
        );
        let response =
            Self::run_jarvis_codex_intent_turn(provider_box_http, config, &prompt).await?;
        let parsed = Self::parse_codex_intent_response(&response)?;
        let artifact_body = Self::jarvis_authored_intent_lisp_body(
            schema,
            config,
            channel,
            objective,
            &parsed,
            grounding_context_id,
            topic_id,
            topic_label,
            sources_used,
            media_context,
        );
        Ok(JarvisAuthoredIntentDraft {
            objective: parsed.recognized_objective.trim().to_string(),
            intent_kind: parsed.intent_kind.trim().to_string(),
            understanding: parsed.understanding.trim().to_string(),
            review_text: parsed.review_text.trim().to_string(),
            artifact_body,
            assumptions: parsed.assumptions,
            non_goals: parsed.non_goals,
            acceptance_signals: parsed.acceptance_signals,
            confidence: parsed.confidence,
        })
    }

    async fn author_jarvis_intent_draft_with_progress(
        stream: &mut TcpStream,
        progress_bus: &JarvisProgressBus,
        chat_id: &str,
        interaction_id: Option<&str>,
        provider_box_http: &ProviderBoxHttpSlot,
        config: &JarvisIntentAuthorConfig,
        schema: &str,
        channel: &str,
        objective: &str,
        grounding_context_id: &str,
        topic_id: Option<&str>,
        topic_label: Option<&str>,
        sources_used: &[String],
        permission_context: Option<&serde_json::Value>,
        media_context: &serde_json::Value,
    ) -> anyhow::Result<JarvisAuthoredIntentDraft> {
        const HEARTBEAT_SECS: u64 = 8;
        let (provider, slot_id, author) = Self::jarvis_author_progress_identity(
            &config.slot_id,
            &config.model,
            &config.reasoning_effort,
        );
        Self::write_jarvis_progress(
            stream,
            progress_bus,
            chat_id,
            interaction_id,
            "intent_authoring",
            "provider_box_semantic_authoring_start",
            "running",
            &format!("正在调用 {provider} 文本作者工位生成 intent.lisp。"),
            None,
            Some(&slot_id),
            Some(author.as_str()),
        )
        .await?;
        let started = tokio::time::Instant::now();
        let mut authoring = Box::pin(Self::author_jarvis_intent_draft(
            provider_box_http,
            config,
            schema,
            channel,
            objective,
            grounding_context_id,
            topic_id,
            topic_label,
            sources_used,
            permission_context,
            media_context,
        ));
        let mut heartbeat = Box::pin(tokio::time::sleep(std::time::Duration::from_secs(
            HEARTBEAT_SECS,
        )));
        loop {
            tokio::select! {
                result = &mut authoring => {
                    return match result {
                        Ok(draft) => {
                            Self::write_jarvis_progress(
                                stream,
                                progress_bus,
                                chat_id,
                                interaction_id,
                                "intent_authoring",
                                "provider_box_semantic_authoring_completed",
                                "completed",
                                &format!("intent.lisp 已由 {provider} 生成并通过结构校验，正在写入 artifact。"),
                                Some(started.elapsed().as_secs()),
                                Some(&slot_id),
                                Some(author.as_str()),
                            )
                            .await?;
                            Ok(draft)
                        }
                        Err(error) => {
                            let error_message = error.to_string();
                            Self::write_jarvis_progress(
                                stream,
                                progress_bus,
                                chat_id,
                                interaction_id,
                                "intent_authoring_failed",
                                "provider_box_semantic_authoring_failed",
                                "failed",
                                &format!("失败在 intent.lisp 生成：{error_message}"),
                                Some(started.elapsed().as_secs()),
                                Some(&slot_id),
                                Some(author.as_str()),
                            )
                            .await?;
                            Err(error)
                        }
                    };
                }
                _ = &mut heartbeat => {
                    let elapsed = started.elapsed().as_secs();
                    Self::write_jarvis_progress(
                        stream,
                        progress_bus,
                        chat_id,
                        interaction_id,
                        "intent_authoring",
                        "provider_box_semantic_authoring_waiting",
                        "running",
                        &format!("{provider} intent author 仍在运行，已等待 {elapsed}s；当前步骤：生成并校验 intent.lisp。"),
                        Some(elapsed),
                        Some(&slot_id),
                        Some(author.as_str()),
                    )
                    .await?;
                    heartbeat.as_mut().reset(
                        tokio::time::Instant::now() + std::time::Duration::from_secs(HEARTBEAT_SECS),
                    );
                }
            }
        }
    }

    fn jarvis_lisp_string(value: &str) -> String {
        let mut escaped = String::with_capacity(value.len() + 2);
        escaped.push('"');
        for ch in value.chars() {
            match ch {
                '\\' => escaped.push_str("\\\\"),
                '"' => escaped.push_str("\\\""),
                '\n' => escaped.push_str("\\n"),
                '\r' => escaped.push_str("\\r"),
                '\t' => escaped.push_str("\\t"),
                _ => escaped.push(ch),
            }
        }
        escaped.push('"');
        escaped
    }

    fn jarvis_lisp_optional(value: Option<&str>) -> String {
        value
            .filter(|v| !v.trim().is_empty())
            .map(Self::jarvis_lisp_string)
            .unwrap_or_else(|| "nil".to_string())
    }

    fn jarvis_lisp_string_list(values: &[String]) -> String {
        if values.is_empty() {
            return "[]".to_string();
        }
        let joined = values
            .iter()
            .map(|value| Self::jarvis_lisp_string(value))
            .collect::<Vec<_>>()
            .join(" ");
        format!("[{}]", joined)
    }

    fn jarvis_lisp_json(value: &serde_json::Value) -> String {
        Self::jarvis_lisp_string(
            &serde_json::to_string(value).unwrap_or_else(|_| "null".to_string()),
        )
    }

    fn jarvis_authored_key_judgment_lisp_body(
        schema: &str,
        config: &JarvisKeyJudgmentAuthorConfig,
        channel: &str,
        objective: &str,
        draft: &JarvisCodexKeyJudgmentResponse,
        grounding_context_id: &str,
        intent_artifact_id: &str,
        topic_id: Option<&str>,
        topic_label: Option<&str>,
        sources_used: &[String],
        grounding_report_file: Option<&str>,
        grounding_report_hash: Option<&str>,
    ) -> String {
        let provider = Self::jarvis_author_text_provider();
        let engine = Self::provider_box_engine_for_provider(provider.as_str()).unwrap_or("codex");
        let slot_id = Self::jarvis_author_text_slot_id(provider.as_str(), &config.slot_id);
        let model = Self::jarvis_author_text_model(provider.as_str(), &config.model);
        let authority = Self::jarvis_author_text_authority(
            provider.as_str(),
            model.as_deref(),
            &config.reasoning_effort,
        );
        format!(
            "(key-judgment-draft\n  :schema {}\n  :authority {}\n  :semantic-author (:provider {} :engine {} :slot-id {} :model {} :reasoning-effort {} :sandbox {} :approval-policy {})\n  :channel {}\n  :objective {}\n  :confidence {}\n  :grounding-context-id {}\n  :intent-artifact-id {}\n  :grounding-report-file {}\n  :grounding-report-hash {}\n  :topic-id {}\n  :topic-label {}\n  :sources-used {}\n  :judgment {}\n  :rejected-hypotheses {}\n  :evidence-refs {}\n  :planning-implications {}\n  :acceptance-focus {}\n  :next-step \"feed key judgment into plan.lisp atomization\")",
            Self::jarvis_lisp_string(schema),
            Self::jarvis_lisp_string(&authority),
            Self::jarvis_lisp_string(&provider),
            Self::jarvis_lisp_string(engine),
            Self::jarvis_lisp_string(&slot_id),
            Self::jarvis_lisp_optional(model.as_deref()),
            Self::jarvis_lisp_string(&config.reasoning_effort),
            Self::jarvis_lisp_string(&config.sandbox),
            Self::jarvis_lisp_string(&config.approval_policy),
            Self::jarvis_lisp_string(channel),
            Self::jarvis_lisp_string(objective),
            Self::jarvis_lisp_optional(draft.confidence.as_deref()),
            Self::jarvis_lisp_string(grounding_context_id),
            Self::jarvis_lisp_string(intent_artifact_id),
            Self::jarvis_lisp_optional(grounding_report_file),
            Self::jarvis_lisp_optional(grounding_report_hash),
            Self::jarvis_lisp_optional(topic_id),
            Self::jarvis_lisp_optional(topic_label),
            Self::jarvis_lisp_string_list(sources_used),
            Self::jarvis_lisp_string(draft.judgment.trim()),
            Self::jarvis_lisp_string_list(&draft.rejected_hypotheses),
            Self::jarvis_lisp_string_list(&draft.evidence_refs),
            Self::jarvis_lisp_string_list(&draft.planning_implications),
            Self::jarvis_lisp_string_list(&draft.acceptance_focus),
        )
    }

    fn jarvis_plan_atomization_graph(
        draft: &JarvisCodexPlanResponse,
        grounding_context_id: &str,
        intent_artifact_id: &str,
        key_judgment: &JarvisKeyJudgmentArtifactRef,
    ) -> serde_json::Value {
        serde_json::json!({
            "schema": "missiond.plan-atomization-graph.v1",
            "grounding_context_id": grounding_context_id,
            "intent_artifact_id": intent_artifact_id,
            "key_judgment_artifact_id": key_judgment.artifact_id,
            "key_judgment_artifact_hash": key_judgment.artifact_hash,
            "key_judgment": key_judgment.judgment,
            "execution_mode": draft.execution_mode,
            "requires_board_task": draft.requires_board_task,
            "workstreams": draft.workstreams,
            "atom_tasks": draft.atom_tasks,
            "dependency_edges": draft.dependency_edges,
            "serial_groups": draft.serial_groups,
            "parallel_groups": draft.parallel_groups,
            "assignment_policy": if draft.assignment_policy.is_null() {
                Self::jarvis_assignment_policy_default()
            } else {
                draft.assignment_policy.clone()
            }
        })
    }

    fn jarvis_authored_plan_lisp_body(
        schema: &str,
        config: &JarvisPlanAuthorConfig,
        channel: &str,
        draft: &JarvisCodexPlanResponse,
        grounding_context_id: &str,
        intent_artifact_id: &str,
        key_judgment: &JarvisKeyJudgmentArtifactRef,
        atomization_graph: &serde_json::Value,
        topic_id: Option<&str>,
        topic_label: Option<&str>,
        sources_used: &[String],
    ) -> String {
        let provider = Self::jarvis_author_text_provider();
        let engine = Self::provider_box_engine_for_provider(provider.as_str()).unwrap_or("codex");
        let slot_id = Self::jarvis_author_text_slot_id(provider.as_str(), &config.slot_id);
        let model = Self::jarvis_author_text_model(provider.as_str(), &config.model);
        let authority = Self::jarvis_author_text_authority(
            provider.as_str(),
            model.as_deref(),
            &config.reasoning_effort,
        );
        let rendered_steps = draft
            .steps
            .iter()
            .enumerate()
            .map(|(idx, step)| {
                format!(
                    "    (step s{} :text {})",
                    idx + 1,
                    Self::jarvis_lisp_string(step)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "(plan-draft\n  :schema {}\n  :authority {}\n  :semantic-author (:provider {} :engine {} :slot-id {} :model {} :reasoning-effort {} :sandbox {} :approval-policy {})\n  :channel {}\n  :objective {}\n  :confidence {}\n  :grounding-context-id {}\n  :intent-artifact-id {}\n  :key-judgment-artifact-id {}\n  :key-judgment-artifact-hash {}\n  :key-judgment {}\n  :topic-id {}\n  :topic-label {}\n  :sources-used {}\n  :execution\n    (:mode {}\n     :requires-board-task {}\n     :answer-policy {}\n     :provider-hint {}\n     :direct-answer-provider provider-box\n     :direct-answer-draft {}\n     :completion-authority {})\n  :steps [\n{}\n  ]\n  :atomization-json {}\n  :assignment-policy {}\n  :boundary {}\n  :assumptions {}\n  :non-goals {}\n  :acceptance-signals {}\n  :approval (:state awaiting-plan-confirmation :required true))",
            Self::jarvis_lisp_string(schema),
            Self::jarvis_lisp_string(&authority),
            Self::jarvis_lisp_string(&provider),
            Self::jarvis_lisp_string(engine),
            Self::jarvis_lisp_string(&slot_id),
            Self::jarvis_lisp_optional(model.as_deref()),
            Self::jarvis_lisp_string(&config.reasoning_effort),
            Self::jarvis_lisp_string(&config.sandbox),
            Self::jarvis_lisp_string(&config.approval_policy),
            Self::jarvis_lisp_string(channel),
            Self::jarvis_lisp_string(draft.objective.trim()),
            Self::jarvis_lisp_optional(draft.confidence.as_deref()),
            Self::jarvis_lisp_string(grounding_context_id),
            Self::jarvis_lisp_string(intent_artifact_id),
            Self::jarvis_lisp_string(&key_judgment.artifact_id),
            Self::jarvis_lisp_optional(key_judgment.artifact_hash.as_deref()),
            Self::jarvis_lisp_string(&key_judgment.judgment),
            Self::jarvis_lisp_optional(topic_id),
            Self::jarvis_lisp_optional(topic_label),
            Self::jarvis_lisp_string_list(sources_used),
            draft.execution_mode.replace('_', "-"),
            if draft.requires_board_task {
                "true"
            } else {
                "false"
            },
            Self::jarvis_lisp_optional(draft.answer_policy.as_deref()),
            Self::jarvis_lisp_optional(draft.provider_hint.as_deref()),
            Self::jarvis_lisp_optional(draft.direct_answer_draft.as_deref()),
            if draft.requires_board_task {
                "task-result-artifact"
            } else {
                "interaction-result-artifact"
            },
            rendered_steps,
            Self::jarvis_lisp_json(atomization_graph),
            Self::jarvis_lisp_json(&draft.assignment_policy),
            Self::jarvis_lisp_optional(draft.boundary.as_deref()),
            Self::jarvis_lisp_string_list(&draft.assumptions),
            Self::jarvis_lisp_string_list(&draft.non_goals),
            Self::jarvis_lisp_string_list(&draft.acceptance_signals),
        )
    }

    async fn author_jarvis_key_judgment_draft(
        provider_box_http: &ProviderBoxHttpSlot,
        config: &JarvisKeyJudgmentAuthorConfig,
        schema: &str,
        channel: &str,
        objective: &str,
        grounding_context_id: &str,
        intent_artifact_id: &str,
        topic_id: Option<&str>,
        topic_label: Option<&str>,
        sources_used: &[String],
        permission_context: Option<&serde_json::Value>,
        context_pack_path: Option<&str>,
        context_pack_file: Option<&str>,
        grounding_report_file: Option<&str>,
        grounding_report_artifact_path: Option<&str>,
        grounding_report_hash: Option<&str>,
        context_sufficiency: Option<&str>,
    ) -> anyhow::Result<JarvisAuthoredKeyJudgmentDraft> {
        let grounding_report_preview =
            Self::read_jarvis_grounding_report_preview(grounding_report_file).await;
        let prompt = Self::jarvis_codex_key_judgment_prompt(
            config,
            schema,
            channel,
            objective,
            grounding_context_id,
            intent_artifact_id,
            topic_id,
            topic_label,
            sources_used,
            permission_context,
            context_pack_path,
            context_pack_file,
            grounding_report_file,
            grounding_report_artifact_path,
            grounding_report_hash,
            context_sufficiency,
            grounding_report_preview.as_deref(),
        );
        let response =
            Self::run_jarvis_codex_key_judgment_turn(provider_box_http, config, &prompt).await?;
        let parsed = Self::parse_codex_key_judgment_response(&response)?;
        let artifact_body = Self::jarvis_authored_key_judgment_lisp_body(
            schema,
            config,
            channel,
            objective,
            &parsed,
            grounding_context_id,
            intent_artifact_id,
            topic_id,
            topic_label,
            sources_used,
            grounding_report_file,
            grounding_report_hash,
        );
        Ok(JarvisAuthoredKeyJudgmentDraft {
            judgment: parsed.judgment.trim().to_string(),
            review_text: parsed.review_text.trim().to_string(),
            artifact_body,
            confidence: parsed.confidence,
            rejected_hypotheses: parsed.rejected_hypotheses,
            evidence_refs: parsed.evidence_refs,
            planning_implications: parsed.planning_implications,
            acceptance_focus: parsed.acceptance_focus,
        })
    }

    async fn author_jarvis_key_judgment_draft_with_progress(
        stream: &mut TcpStream,
        progress_bus: &JarvisProgressBus,
        chat_id: &str,
        interaction_id: Option<&str>,
        provider_box_http: &ProviderBoxHttpSlot,
        config: &JarvisKeyJudgmentAuthorConfig,
        schema: &str,
        channel: &str,
        objective: &str,
        grounding_context_id: &str,
        intent_artifact_id: &str,
        topic_id: Option<&str>,
        topic_label: Option<&str>,
        sources_used: &[String],
        permission_context: Option<&serde_json::Value>,
        context_pack_path: Option<&str>,
        context_pack_file: Option<&str>,
        grounding_report_file: Option<&str>,
        grounding_report_artifact_path: Option<&str>,
        grounding_report_hash: Option<&str>,
        context_sufficiency: Option<&str>,
    ) -> anyhow::Result<JarvisAuthoredKeyJudgmentDraft> {
        const HEARTBEAT_SECS: u64 = 8;
        let (provider, slot_id, author) = Self::jarvis_author_progress_identity(
            &config.slot_id,
            &config.model,
            &config.reasoning_effort,
        );
        Self::write_jarvis_progress(
            stream,
            progress_bus,
            chat_id,
            interaction_id,
            "key_judgment_authoring",
            "provider_box_semantic_authoring_start",
            "running",
            &format!("正在调用 {provider} 文本作者工位生成关键判断。"),
            None,
            Some(&slot_id),
            Some(author.as_str()),
        )
        .await?;
        let started = tokio::time::Instant::now();
        let mut authoring = Box::pin(Self::author_jarvis_key_judgment_draft(
            provider_box_http,
            config,
            schema,
            channel,
            objective,
            grounding_context_id,
            intent_artifact_id,
            topic_id,
            topic_label,
            sources_used,
            permission_context,
            context_pack_path,
            context_pack_file,
            grounding_report_file,
            grounding_report_artifact_path,
            grounding_report_hash,
            context_sufficiency,
        ));
        let mut heartbeat = Box::pin(tokio::time::sleep(std::time::Duration::from_secs(
            HEARTBEAT_SECS,
        )));
        loop {
            tokio::select! {
                result = &mut authoring => {
                    return match result {
                        Ok(draft) => {
                            Self::write_jarvis_progress(
                                stream,
                                progress_bus,
                                chat_id,
                                interaction_id,
                                "key_judgment_authoring",
                                "provider_box_semantic_authoring_completed",
                                "completed",
                                &format!("关键判断已由 {provider} 生成并通过结构校验，正在写入 artifact。"),
                                Some(started.elapsed().as_secs()),
                                Some(&slot_id),
                                Some(author.as_str()),
                            )
                            .await?;
                            Ok(draft)
                        }
                        Err(error) => {
                            let error_message = error.to_string();
                            Self::write_jarvis_progress(
                                stream,
                                progress_bus,
                                chat_id,
                                interaction_id,
                                "key_judgment_authoring_failed",
                                "provider_box_semantic_authoring_failed",
                                "failed",
                                &format!("失败在关键判断生成：{error_message}"),
                                Some(started.elapsed().as_secs()),
                                Some(&slot_id),
                                Some(author.as_str()),
                            )
                            .await?;
                            Err(error)
                        }
                    };
                }
                _ = &mut heartbeat => {
                    let elapsed = started.elapsed().as_secs();
                    Self::write_jarvis_progress(
                        stream,
                        progress_bus,
                        chat_id,
                        interaction_id,
                        "key_judgment_authoring",
                        "provider_box_semantic_authoring_waiting",
                        "running",
                        &format!("{provider} key judgment author 仍在运行，已等待 {elapsed}s；当前步骤：生成并校验关键判断 artifact。"),
                        Some(elapsed),
                        Some(&slot_id),
                        Some(author.as_str()),
                    )
                    .await?;
                    heartbeat.as_mut().reset(
                        tokio::time::Instant::now() + std::time::Duration::from_secs(HEARTBEAT_SECS),
                    );
                }
            }
        }
    }

    async fn author_jarvis_plan_draft(
        provider_box_http: &ProviderBoxHttpSlot,
        config: &JarvisPlanAuthorConfig,
        schema: &str,
        channel: &str,
        objective: &str,
        grounding_context_id: &str,
        intent_artifact_id: &str,
        key_judgment: &JarvisKeyJudgmentArtifactRef,
        topic_id: Option<&str>,
        topic_label: Option<&str>,
        sources_used: &[String],
        permission_context: Option<&serde_json::Value>,
        context_pack_path: Option<&str>,
        context_pack_file: Option<&str>,
        grounding_report_file: Option<&str>,
        grounding_report_artifact_path: Option<&str>,
        grounding_report_hash: Option<&str>,
        grounding_worker_slot_id: Option<&str>,
        grounding_worker_turn_id: Option<&str>,
        context_sufficiency: Option<&str>,
    ) -> anyhow::Result<JarvisAuthoredPlanDraft> {
        let grounding_report_preview =
            Self::read_jarvis_plan_grounding_report_preview(grounding_report_file).await;
        let prompt = Self::jarvis_codex_plan_prompt(
            config,
            schema,
            channel,
            objective,
            grounding_context_id,
            intent_artifact_id,
            key_judgment,
            topic_id,
            topic_label,
            sources_used,
            permission_context,
            context_pack_path,
            context_pack_file,
            grounding_report_file,
            grounding_report_artifact_path,
            grounding_report_hash,
            grounding_worker_slot_id,
            grounding_worker_turn_id,
            context_sufficiency,
            grounding_report_preview.as_deref(),
        );
        let response = Self::run_jarvis_codex_plan_turn(provider_box_http, config, &prompt).await?;
        let parsed = Self::parse_codex_plan_response(&response)?;
        let atomization_graph = Self::jarvis_plan_atomization_graph(
            &parsed,
            grounding_context_id,
            intent_artifact_id,
            key_judgment,
        );
        let artifact_body = Self::jarvis_authored_plan_lisp_body(
            schema,
            config,
            channel,
            &parsed,
            grounding_context_id,
            intent_artifact_id,
            key_judgment,
            &atomization_graph,
            topic_id,
            topic_label,
            sources_used,
        );
        Ok(JarvisAuthoredPlanDraft {
            objective: parsed.objective.trim().to_string(),
            review_text: parsed.review_text.trim().to_string(),
            execution_mode: parsed.execution_mode.clone(),
            requires_board_task: parsed.requires_board_task,
            artifact_body,
            steps: parsed.steps,
            direct_answer_draft: parsed.direct_answer_draft,
            answer_policy: parsed.answer_policy,
            provider_hint: parsed.provider_hint,
            boundary: parsed.boundary,
            assumptions: parsed.assumptions,
            non_goals: parsed.non_goals,
            acceptance_signals: parsed.acceptance_signals,
            confidence: parsed.confidence,
            key_judgment: parsed.key_judgment,
            atomization_graph,
            workstreams: parsed.workstreams,
            atom_tasks: parsed.atom_tasks,
            dependency_edges: parsed.dependency_edges,
            serial_groups: parsed.serial_groups,
            parallel_groups: parsed.parallel_groups,
            assignment_policy: parsed.assignment_policy,
        })
    }

    async fn author_jarvis_plan_draft_with_progress(
        stream: &mut TcpStream,
        progress_bus: &JarvisProgressBus,
        chat_id: &str,
        interaction_id: Option<&str>,
        provider_box_http: &ProviderBoxHttpSlot,
        config: &JarvisPlanAuthorConfig,
        schema: &str,
        channel: &str,
        objective: &str,
        grounding_context_id: &str,
        intent_artifact_id: &str,
        key_judgment: &JarvisKeyJudgmentArtifactRef,
        topic_id: Option<&str>,
        topic_label: Option<&str>,
        sources_used: &[String],
        permission_context: Option<&serde_json::Value>,
        context_pack_path: Option<&str>,
        context_pack_file: Option<&str>,
        grounding_report_file: Option<&str>,
        grounding_report_artifact_path: Option<&str>,
        grounding_report_hash: Option<&str>,
        grounding_worker_slot_id: Option<&str>,
        grounding_worker_turn_id: Option<&str>,
        context_sufficiency: Option<&str>,
    ) -> anyhow::Result<JarvisAuthoredPlanDraft> {
        const HEARTBEAT_SECS: u64 = 8;
        let (provider, slot_id, author) = Self::jarvis_author_progress_identity(
            &config.slot_id,
            &config.model,
            &config.reasoning_effort,
        );
        Self::write_jarvis_progress(
            stream,
            progress_bus,
            chat_id,
            interaction_id,
            "plan_authoring",
            "provider_box_semantic_authoring_start",
            "running",
            &format!("正在调用 {provider} 文本作者工位生成 plan.lisp。"),
            None,
            Some(&slot_id),
            Some(author.as_str()),
        )
        .await?;
        let started = tokio::time::Instant::now();
        let mut authoring = Box::pin(Self::author_jarvis_plan_draft(
            provider_box_http,
            config,
            schema,
            channel,
            objective,
            grounding_context_id,
            intent_artifact_id,
            key_judgment,
            topic_id,
            topic_label,
            sources_used,
            permission_context,
            context_pack_path,
            context_pack_file,
            grounding_report_file,
            grounding_report_artifact_path,
            grounding_report_hash,
            grounding_worker_slot_id,
            grounding_worker_turn_id,
            context_sufficiency,
        ));
        let mut heartbeat = Box::pin(tokio::time::sleep(std::time::Duration::from_secs(
            HEARTBEAT_SECS,
        )));
        loop {
            tokio::select! {
                result = &mut authoring => {
                    return match result {
                        Ok(draft) => {
                            Self::write_jarvis_progress(
                                stream,
                                progress_bus,
                                chat_id,
                                interaction_id,
                                "plan_authoring",
                                "provider_box_semantic_authoring_completed",
                                "completed",
                                &format!("plan.lisp 已由 {provider} 生成并通过结构校验，正在写入 artifact。"),
                                Some(started.elapsed().as_secs()),
                                Some(&slot_id),
                                Some(author.as_str()),
                            )
                            .await?;
                            Ok(draft)
                        }
                        Err(error) => {
                            let error_message = error.to_string();
                            Self::write_jarvis_progress(
                                stream,
                                progress_bus,
                                chat_id,
                                interaction_id,
                                "plan_authoring_failed",
                                "provider_box_semantic_authoring_failed",
                                "failed",
                                &format!("失败在 plan.lisp 生成：{error_message}"),
                                Some(started.elapsed().as_secs()),
                                Some(&slot_id),
                                Some(author.as_str()),
                            )
                            .await?;
                            Err(error)
                        }
                    };
                }
                _ = &mut heartbeat => {
                    let elapsed = started.elapsed().as_secs();
                    Self::write_jarvis_progress(
                        stream,
                        progress_bus,
                        chat_id,
                        interaction_id,
                        "plan_authoring",
                        "provider_box_semantic_authoring_waiting",
                        "running",
                        &format!("{provider} plan author 仍在运行，已等待 {elapsed}s；当前步骤：生成并校验 plan.lisp。"),
                        Some(elapsed),
                        Some(&slot_id),
                        Some(author.as_str()),
                    )
                    .await?;
                    heartbeat.as_mut().reset(
                        tokio::time::Instant::now() + std::time::Duration::from_secs(HEARTBEAT_SECS),
                    );
                }
            }
        }
    }

    fn jarvis_artifact_projection_text(
        event: &str,
        artifact_id: &str,
        artifact_hash: &str,
        artifact_path: &str,
    ) -> String {
        format!(
            "{event} artifact ready\nartifact_id: {artifact_id}\nartifact_hash: {artifact_hash}\nartifact_path: {artifact_path}"
        )
    }

    async fn write_sse_openai_missiond_projection(
        stream: &mut TcpStream,
        chat_id: &str,
        event: &str,
        artifact_id: &str,
        artifact_hash: &str,
        artifact_path: &str,
    ) -> anyhow::Result<()> {
        if !jarvis_artifact_projection_openai_delta_enabled() {
            return Ok(());
        }
        let content =
            Self::jarvis_artifact_projection_text(event, artifact_id, artifact_hash, artifact_path);
        let chunk = serde_json::json!({
            "id": chat_id,
            "object": "chat.completion.chunk",
            "model": "jarvis-missiond",
            "choices": [{
                "index": 0,
                "delta": {"content": content},
                "finish_reason": serde_json::Value::Null
            }],
            "missiond_projection": {
                "schema": "missiond.openai-artifact-projection.v1",
                "event": event,
                "artifact_id": artifact_id,
                "artifact_hash": artifact_hash,
                "artifact_path": artifact_path
            }
        });
        stream
            .write_all(format!("data: {chunk}\n\n").as_bytes())
            .await?;
        stream.flush().await?;
        Ok(())
    }

    fn jarvis_text_confirms_pending_review(text: &str) -> bool {
        let compact: String = text
            .trim()
            .to_lowercase()
            .chars()
            .filter(|c| {
                !c.is_whitespace()
                    && !matches!(
                        c,
                        ',' | '.' | '!' | '?' | ';' | ':' | '，' | '。' | '！' | '？' | '；' | '：'
                    )
            })
            .collect();
        if compact.is_empty() {
            return false;
        }
        if [
            "不确认",
            "不要确认",
            "别确认",
            "取消",
            "拒绝",
            "不同意",
            "no",
            "reject",
            "rejected",
            "cancel",
        ]
        .iter()
        .any(|deny| compact.contains(deny))
        {
            return false;
        }
        matches!(
            compact.as_str(),
            "确认"
                | "确认意图"
                | "确认intent"
                | "确认计划"
                | "确认plan"
                | "同意"
                | "可以"
                | "通过"
                | "批准"
                | "ok"
                | "okay"
                | "yes"
                | "y"
                | "confirm"
                | "confirmed"
                | "approve"
                | "approved"
        )
    }

    fn jarvis_pending_confirmation_marker(status: &str, confirm: &serde_json::Value) -> String {
        serde_json::json!({
            "schema": "missiond.jarvis-pending-confirmation.v1",
            "status": status,
            "confirmation_type": confirm.get("confirmation_type").cloned().unwrap_or(serde_json::Value::Null),
            "phase": confirm.get("phase").cloned().unwrap_or(serde_json::Value::Null),
            "confirm_payload": confirm.get("confirm_payload").cloned().unwrap_or(serde_json::json!({})),
        })
        .to_string()
    }

    fn latest_pending_jarvis_confirmation(
        history: &[serde_json::Value],
    ) -> Option<serde_json::Value> {
        for message in history.iter().rev() {
            let Some(content) = message.get("content").and_then(|value| value.as_str()) else {
                continue;
            };
            let Ok(marker) = serde_json::from_str::<serde_json::Value>(content) else {
                continue;
            };
            if marker.get("schema").and_then(|value| value.as_str())
                != Some("missiond.jarvis-pending-confirmation.v1")
            {
                continue;
            }
            match marker.get("status").and_then(|value| value.as_str()) {
                Some("pending") => return marker.get("confirm_payload").cloned(),
                Some("fulfilled") => return None,
                _ => continue,
            }
        }
        None
    }

    async fn load_pending_jarvis_confirmation(
        db: &Arc<dyn crate::db::traits::MissionStore>,
        conversation_id: &str,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        let history = db.router_chat_load_history(conversation_id).await?;
        Ok(Self::latest_pending_jarvis_confirmation(&history))
    }

    async fn resolve_jarvis_conversation_id(
        db: &Arc<dyn crate::db::traits::MissionStore>,
        requested_conversation_id: Option<&str>,
        raw_user_text: &str,
        scope: &ConversationSessionScope,
    ) -> anyhow::Result<String> {
        let requested = requested_conversation_id
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let resolved = db
            .jarvis_get_or_create_scoped(
                requested,
                scope.user_id.as_deref(),
                scope.tenant_id.as_deref(),
                scope.application_id.as_deref(),
                Some(scope.channel.as_str()),
                scope.topic_id.as_deref(),
                scope.topic_label.as_deref(),
            )
            .await?;

        if requested.is_none() && Self::jarvis_text_confirms_pending_review(raw_user_text) {
            if Self::load_pending_jarvis_confirmation(db, &resolved)
                .await?
                .is_some()
            {
                return Ok(resolved);
            }
        }

        Ok(resolved)
    }

    async fn persist_jarvis_pending_confirmation(
        db: Option<&Arc<dyn crate::db::traits::MissionStore>>,
        conversation_id: Option<&str>,
        confirm: &serde_json::Value,
    ) {
        let (Some(db), Some(conversation_id)) = (db, conversation_id) else {
            return;
        };
        let marker = Self::jarvis_pending_confirmation_marker("pending", confirm);
        if let Err(error) = db
            .router_chat_append_messages(conversation_id, &[("assistant".to_string(), marker)])
            .await
        {
            warn!(%conversation_id, error = %error, "failed to persist Jarvis pending confirmation");
        }
    }

    async fn persist_jarvis_confirmation_fulfilled(
        db: Option<&Arc<dyn crate::db::traits::MissionStore>>,
        conversation_id: Option<&str>,
        confirmation_type: &str,
    ) {
        let (Some(db), Some(conversation_id)) = (db, conversation_id) else {
            return;
        };
        let confirm = serde_json::json!({
            "phase": "fulfilled",
            "confirmation_type": confirmation_type,
            "confirm_payload": {}
        });
        let marker = Self::jarvis_pending_confirmation_marker("fulfilled", &confirm);
        if let Err(error) = db
            .router_chat_append_messages(conversation_id, &[("assistant".to_string(), marker)])
            .await
        {
            warn!(%conversation_id, error = %error, "failed to persist Jarvis fulfilled confirmation");
        }
    }

    fn inject_jarvis_confirm_payload(
        target: &mut serde_json::Value,
        confirm_payload: serde_json::Value,
    ) {
        let Some(object) = target.as_object_mut() else {
            return;
        };
        let confirm = object
            .entry("missiond_confirm".to_string())
            .or_insert_with(|| serde_json::json!({}));
        if !confirm.is_object() {
            *confirm = serde_json::json!({});
        }
        if let Some(confirm_object) = confirm.as_object_mut() {
            confirm_object
                .entry("confirm_payload".to_string())
                .or_insert(confirm_payload);
        }
    }

    async fn finish_sse(stream: &mut TcpStream) -> anyhow::Result<()> {
        Self::write_sse_bytes(stream, b"data: [DONE]\n\n", "done", None).await?;
        if let Err(error) = stream.shutdown().await {
            return Self::handle_sse_write_error(error, "shutdown", None);
        }
        Ok(())
    }

    async fn fail_jarvis_gate(
        stream: &mut TcpStream,
        message: impl Into<String>,
        stage: &str,
    ) -> anyhow::Result<()> {
        let message = message.into();
        let phase_code = Self::jarvis_phase_code(stage);
        let error_code = Self::jarvis_phase_error_code(stage);
        let diagnostic = serde_json::json!({
            "phase": stage,
            "phase_code": phase_code,
            "error": {"code": error_code, "message": message},
            "next_action": "Fix the missing runtime capability instead of falling back to direct PTY execution."
        });
        Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
        Self::finish_sse(stream).await
    }

    async fn fail_jarvis_gate_visible(
        stream: &mut TcpStream,
        progress_bus: &JarvisProgressBus,
        chat_id: &str,
        interaction_id: Option<&str>,
        message: impl Into<String>,
        stage: &str,
        db: Option<&Arc<dyn crate::db::traits::MissionStore>>,
        conversation_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let message = message.into();
        let phase_code = Self::jarvis_phase_code(stage);
        let error_code = Self::jarvis_phase_error_code(stage);
        let visible_message = format!("失败在 {stage}：{message}");
        Self::write_jarvis_progress(
            stream,
            progress_bus,
            chat_id,
            interaction_id,
            stage,
            stage,
            "failed",
            &visible_message,
            None,
            None,
            None,
        )
        .await?;
        let diagnostic = serde_json::json!({
            "schema": "missiond.jarvis-progress.v1",
            "interaction_id": interaction_id,
            "phase": stage,
            "phase_code": phase_code,
            "step": stage,
            "status": "failed",
            "visible": true,
            "openai_delta": true,
            "error": {"code": error_code, "message": message},
            "next_action": "Fix the missing runtime capability instead of falling back to direct PTY execution."
        });
        Self::persist_interaction_event(
            db,
            conversation_id,
            interaction_id,
            "diagnostic",
            &diagnostic,
        )
        .await;
        Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
        Self::write_sse_openai_text_and_persist(
            stream,
            chat_id,
            &visible_message,
            Some("stop"),
            db,
            conversation_id,
        )
        .await?;
        Self::finish_sse(stream).await
    }

    fn jarvis_phase_code(stage: &str) -> &'static str {
        match stage {
            "intent" | "intent_artifact" | "intent_authoring_failed" => "intent",
            "grounding" | "confirmation_grounding" => "grounding",
            "key_judgment"
            | "key_judgment_artifact"
            | "key_judgment_authoring_failed"
            | "confirmation_key_judgment" => "key_judgment",
            "plan" | "plan_artifact" | "plan_authoring_failed" | "execution_mode" => "plan",
            "communicator" | "plan_dispatched" | "result_terminal" => "communicator",
            "direct_answer" | "grounded_direct_answer" => "direct_answer",
            "board_dispatch" | "board_task_create" => "board_dispatch",
            "result_followup" | "workers_running" => "board_dispatch",
            _ => "unknown",
        }
    }

    fn jarvis_phase_error_code(stage: &str) -> &'static str {
        match stage {
            "intent_authoring_failed" => return "JARVIS_INTENT_AUTHOR_FAILED",
            "key_judgment_authoring_failed" => return "JARVIS_KEY_JUDGMENT_AUTHOR_FAILED",
            "plan_authoring_failed" => return "JARVIS_PLAN_AUTHOR_FAILED",
            _ => {}
        }
        match Self::jarvis_phase_code(stage) {
            "intent" => "JARVIS_INTENT_FAILED",
            "grounding" => "JARVIS_GROUNDING_FAILED",
            "key_judgment" => "JARVIS_KEY_JUDGMENT_FAILED",
            "plan" => "JARVIS_PLAN_FAILED",
            "communicator" => "JARVIS_COMMUNICATOR_FAILED",
            "direct_answer" => "JARVIS_DIRECT_ANSWER_FAILED",
            "board_dispatch" => "JARVIS_BOARD_DISPATCH_FAILED",
            _ => "JARVIS_PHASE_FAILED",
        }
    }

    fn jarvis_pending_grounding_result(scope: &ConversationSessionScope) -> JarvisGroundingResult {
        JarvisGroundingResult {
            grounding_context_id: "intent-stage:pending-grounding".to_string(),
            topic_id: scope.topic_id.clone(),
            topic_label: scope.topic_label.clone(),
            sources_used: Vec::new(),
            diagnostics: serde_json::json!({
                "status": "pending_intent_confirmation",
                "reason": "Jarvis collects full MissionD grounding only after intent.lisp is confirmed."
            }),
            ..Default::default()
        }
    }

    fn jarvis_grounding_from_interaction_metadata(
        envelope: &InteractionEnvelope,
        scope: &ConversationSessionScope,
    ) -> Result<JarvisGroundingResult, String> {
        let grounding_context_id =
            interaction_metadata_string(envelope, "missiond_grounding_context_id")
                .ok_or_else(|| {
                    "Jarvis plan confirmation requires missiond_grounding_context_id from the previous plan payload.".to_string()
                })?;
        Ok(JarvisGroundingResult {
            grounding_context_id,
            context_pack_path: interaction_metadata_string(envelope, "missiond_context_pack_path"),
            context_pack_file: interaction_metadata_string(envelope, "missiond_context_pack_file"),
            grounding_report_file: interaction_metadata_string(
                envelope,
                "missiond_grounding_report_file",
            ),
            grounding_report_artifact_path: interaction_metadata_string(
                envelope,
                "missiond_grounding_report_artifact_path",
            ),
            grounding_report_hash: interaction_metadata_string(
                envelope,
                "missiond_grounding_report_hash",
            ),
            grounding_worker_slot_id: interaction_metadata_string(
                envelope,
                "missiond_grounding_worker_slot_id",
            ),
            grounding_worker_turn_id: interaction_metadata_string(
                envelope,
                "missiond_grounding_worker_turn_id",
            ),
            context_sufficiency: interaction_metadata_string(
                envelope,
                "missiond_context_sufficiency",
            ),
            artifact_hash: interaction_metadata_string(
                envelope,
                "missiond_grounding_artifact_hash",
            ),
            context_capsule_hash: interaction_metadata_string(
                envelope,
                "missiond_context_capsule_hash",
            ),
            context_capsule_file: interaction_metadata_string(
                envelope,
                "missiond_context_capsule_file",
            ),
            topic_id: interaction_metadata_string(envelope, "missiond_topic_id")
                .or_else(|| scope.topic_id.clone()),
            topic_label: interaction_metadata_string(envelope, "missiond_topic_label")
                .or_else(|| scope.topic_label.clone()),
            sources_used: interaction_metadata_string_vec(envelope, "missiond_sources_used"),
            diagnostics: serde_json::json!({
                "status": "reused_from_plan_confirmation",
                "source": "missiond_confirm.confirm_payload"
            }),
        })
    }

    fn jarvis_grounding_from_confirm_value(
        req: &serde_json::Value,
        scope: &ConversationSessionScope,
    ) -> Result<JarvisGroundingResult, String> {
        let grounding_context_id = jarvis_confirm_string(req, "missiond_grounding_context_id")
            .ok_or_else(|| {
                "Jarvis plan confirmation requires missiond_grounding_context_id from the previous plan payload.".to_string()
            })?;
        Ok(JarvisGroundingResult {
            grounding_context_id,
            context_pack_path: jarvis_confirm_string(req, "missiond_context_pack_path"),
            context_pack_file: jarvis_confirm_string(req, "missiond_context_pack_file"),
            grounding_report_file: jarvis_confirm_string(req, "missiond_grounding_report_file"),
            grounding_report_artifact_path: jarvis_confirm_string(
                req,
                "missiond_grounding_report_artifact_path",
            ),
            grounding_report_hash: jarvis_confirm_string(req, "missiond_grounding_report_hash"),
            grounding_worker_slot_id: jarvis_confirm_string(
                req,
                "missiond_grounding_worker_slot_id",
            ),
            grounding_worker_turn_id: jarvis_confirm_string(
                req,
                "missiond_grounding_worker_turn_id",
            ),
            context_sufficiency: jarvis_confirm_string(req, "missiond_context_sufficiency"),
            artifact_hash: jarvis_confirm_string(req, "missiond_grounding_artifact_hash"),
            context_capsule_hash: jarvis_confirm_string(req, "missiond_context_capsule_hash"),
            context_capsule_file: jarvis_confirm_string(req, "missiond_context_capsule_file"),
            topic_id: jarvis_confirm_string(req, "missiond_topic_id")
                .or_else(|| scope.topic_id.clone()),
            topic_label: jarvis_confirm_string(req, "missiond_topic_label")
                .or_else(|| scope.topic_label.clone()),
            sources_used: jarvis_confirm_string_vec(req, "missiond_sources_used"),
            diagnostics: serde_json::json!({
                "status": "reused_from_plan_confirmation",
                "source": "missiond_confirm.confirm_payload"
            }),
        })
    }

    fn jarvis_key_judgment_from_interaction_metadata(
        envelope: &InteractionEnvelope,
    ) -> Result<JarvisKeyJudgmentArtifactRef, String> {
        let artifact_id = interaction_metadata_string(envelope, "missiond_key_judgment_artifact_id")
            .ok_or_else(|| {
                "Jarvis plan confirmation requires missiond_key_judgment_artifact_id from the previous plan payload.".to_string()
            })?;
        let judgment = interaction_metadata_string(envelope, "missiond_key_judgment")
            .ok_or_else(|| {
                "Jarvis plan confirmation requires missiond_key_judgment from the previous plan payload.".to_string()
            })?;
        Ok(JarvisKeyJudgmentArtifactRef {
            artifact_id,
            artifact_hash: interaction_metadata_string(
                envelope,
                "missiond_key_judgment_artifact_hash",
            ),
            artifact_path: interaction_metadata_string(
                envelope,
                "missiond_key_judgment_artifact_path",
            ),
            judgment,
            review_text: interaction_metadata_string(envelope, "missiond_key_judgment_review_text"),
            confidence: interaction_metadata_string(envelope, "missiond_key_judgment_confidence"),
            rejected_hypotheses: interaction_metadata_string_vec(
                envelope,
                "missiond_key_judgment_rejected_hypotheses",
            ),
            evidence_refs: interaction_metadata_string_vec(
                envelope,
                "missiond_key_judgment_evidence_refs",
            ),
            planning_implications: interaction_metadata_string_vec(
                envelope,
                "missiond_key_judgment_planning_implications",
            ),
            acceptance_focus: interaction_metadata_string_vec(
                envelope,
                "missiond_key_judgment_acceptance_focus",
            ),
        })
    }

    fn jarvis_key_judgment_from_confirm_value(
        req: &serde_json::Value,
    ) -> Result<JarvisKeyJudgmentArtifactRef, String> {
        let artifact_id = jarvis_confirm_string(req, "missiond_key_judgment_artifact_id")
            .ok_or_else(|| {
                "Jarvis plan confirmation requires missiond_key_judgment_artifact_id from the previous plan payload.".to_string()
            })?;
        let judgment = jarvis_confirm_string(req, "missiond_key_judgment").ok_or_else(|| {
            "Jarvis plan confirmation requires missiond_key_judgment from the previous plan payload.".to_string()
        })?;
        Ok(JarvisKeyJudgmentArtifactRef {
            artifact_id,
            artifact_hash: jarvis_confirm_string(req, "missiond_key_judgment_artifact_hash"),
            artifact_path: jarvis_confirm_string(req, "missiond_key_judgment_artifact_path"),
            judgment,
            review_text: jarvis_confirm_string(req, "missiond_key_judgment_review_text"),
            confidence: jarvis_confirm_string(req, "missiond_key_judgment_confidence"),
            rejected_hypotheses: jarvis_confirm_string_vec(
                req,
                "missiond_key_judgment_rejected_hypotheses",
            ),
            evidence_refs: jarvis_confirm_string_vec(req, "missiond_key_judgment_evidence_refs"),
            planning_implications: jarvis_confirm_string_vec(
                req,
                "missiond_key_judgment_planning_implications",
            ),
            acceptance_focus: jarvis_confirm_string_vec(
                req,
                "missiond_key_judgment_acceptance_focus",
            ),
        })
    }

    fn jarvis_plan_atomization_graph_from_interaction_metadata(
        envelope: &InteractionEnvelope,
    ) -> serde_json::Value {
        interaction_metadata_string(envelope, "missiond_plan_atomization_graph_json")
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .unwrap_or_else(|| {
                serde_json::json!({
                    "schema": "missiond.plan-atomization-graph.v1",
                    "status": "missing_from_confirmation_payload"
                })
            })
    }

    fn jarvis_plan_atomization_graph_from_confirm_value(
        req: &serde_json::Value,
    ) -> serde_json::Value {
        jarvis_confirm_string(req, "missiond_plan_atomization_graph_json")
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .unwrap_or_else(|| {
                serde_json::json!({
                    "schema": "missiond.plan-atomization-graph.v1",
                    "status": "missing_from_confirmation_payload"
                })
            })
    }

    async fn gather_jarvis_grounding(
        slot: &JarvisGroundingSlot,
        req: JarvisGroundingRequest,
    ) -> Result<JarvisGroundingResult, String> {
        let guard = slot.read().await;
        let Some(ref grounder) = *guard else {
            return Err("Jarvis grounding runtime is not configured".to_string());
        };
        let grounder = Arc::clone(grounder);
        drop(guard);
        grounder(req).await.and_then(|result| {
            if result.grounding_context_id.trim().is_empty() {
                Err("Jarvis grounding returned an empty grounding_context_id".to_string())
            } else {
                Ok(result)
            }
        })
    }

    async fn gather_jarvis_grounding_with_progress(
        stream: &mut TcpStream,
        progress_bus: &JarvisProgressBus,
        chat_id: &str,
        interaction_id: Option<&str>,
        slot: &JarvisGroundingSlot,
        req: JarvisGroundingRequest,
    ) -> Result<JarvisGroundingResult, String> {
        let started_at = std::time::Instant::now();
        let heartbeat = tokio::time::Duration::from_secs(jarvis_visible_heartbeat_secs());
        let mut interval = tokio::time::interval(heartbeat);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let grounding = Self::gather_jarvis_grounding(slot, req);
        tokio::pin!(grounding);
        let mut first_tick = true;

        loop {
            tokio::select! {
                result = &mut grounding => return result,
                _ = interval.tick() => {
                    if first_tick {
                        first_tick = false;
                        continue;
                    }
                    let elapsed_secs = started_at.elapsed().as_secs();
                    Self::write_jarvis_progress(
                        stream,
                        progress_bus,
                        chat_id,
                        interaction_id,
                        "grounding",
                        "context_gather_running",
                        "running",
                        &format!(
                            "ClaudeCode grounding 仍在运行 {elapsed_secs}s，MissionD 正在等待 grounding_report_file；不会静默挂起。"
                        ),
                        Some(elapsed_secs),
                        None,
                        Some("claude-code-mcp-grounding"),
                    )
                    .await
                    .map_err(|err| format!("JARVIS_GROUNDING_PROGRESS_WRITE_FAILED: {err}"))?;
                }
            }
        }
    }

    async fn put_jarvis_artifact(
        slot: &JarvisArtifactSlot,
        mut req: JarvisArtifactRequest,
    ) -> Result<JarvisArtifactResult, String> {
        let guard = slot.read().await;
        let Some(ref writer) = *guard else {
            return Err("Jarvis artifact writer is not configured".to_string());
        };
        let writer = Arc::clone(writer);
        drop(guard);
        req.payload = crate::evidence_redactor::redact_json_value(&req.payload);
        req.metadata = crate::evidence_redactor::redact_json_value(&req.metadata);
        writer(req).await.and_then(|result| {
            if result.artifact_id.trim().is_empty() || result.artifact_hash.trim().is_empty() {
                Err("Jarvis artifact writer returned an empty artifact id/hash".to_string())
            } else {
                Ok(result)
            }
        })
    }

    fn jarvis_direct_answer_provider() -> String {
        Self::jarvis_direct_answer_provider_override()
            .unwrap_or_else(|| Self::jarvis_communicator_provider())
    }

    fn jarvis_direct_answer_provider_override() -> Option<String> {
        std::env::var("MISSIOND_JARVIS_DIRECT_ANSWER_PROVIDER")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    fn provider_box_engine_for_provider(provider: &str) -> anyhow::Result<&'static str> {
        match provider {
            "codex" | "codex_cli" | "codex-cli" => Ok("codex"),
            "agy" | "agy_cli" | "agy-cli" => Ok("agy"),
            "claude_code" | "claude-code" | "claude" => Ok("claude_code"),
            "gemini" | "gemini_cli" | "gemini-cli" => Ok("gemini"),
            other => {
                anyhow::bail!("unsupported provider_box provider for Jarvis direct answer: {other}")
            }
        }
    }

    fn jarvis_communicator_provider() -> String {
        std::env::var("MISSIOND_JARVIS_COMMUNICATOR_PROVIDER")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "agy".to_string())
    }

    fn jarvis_communicator_slot_id(provider: &str) -> String {
        Self::jarvis_text_only_slot_id(
            provider,
            std::env::var("MISSIOND_JARVIS_COMMUNICATOR_SLOT_ID")
                .ok()
                .as_deref(),
            "slot-agy-gemini-31-pro-high-jarvis-communicator-a",
        )
    }

    fn jarvis_default_text_only_model(provider: &str) -> Option<String> {
        match provider {
            "agy" | "agy_cli" | "agy-cli" => Some("Gemini 3.1 Pro (High)".to_string()),
            "codex" | "codex_cli" | "codex-cli" => Some("gpt-5.5".to_string()),
            _ => None,
        }
    }

    fn jarvis_communicator_model_for_provider(provider: &str) -> Option<String> {
        std::env::var("MISSIOND_JARVIS_COMMUNICATOR_MODEL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| Self::jarvis_default_text_only_model(provider))
    }

    fn jarvis_direct_answer_model(provider: &str) -> Option<String> {
        std::env::var("MISSIOND_JARVIS_DIRECT_ANSWER_MODEL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| Self::jarvis_communicator_model_for_provider(provider))
    }

    fn jarvis_communicator_timeout_secs() -> u64 {
        std::env::var("MISSIOND_JARVIS_COMMUNICATOR_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(120)
            .clamp(10, 600)
    }

    fn jarvis_direct_answer_timeout_secs() -> u64 {
        std::env::var("MISSIOND_JARVIS_DIRECT_ANSWER_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or_else(Self::jarvis_communicator_timeout_secs)
            .clamp(10, 240)
    }

    fn jarvis_direct_answer_stream_budget_secs(timeout_secs: u64) -> u64 {
        std::env::var("MISSIOND_JARVIS_DIRECT_ANSWER_STREAM_BUDGET_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or_else(|| timeout_secs.min(90))
            .clamp(10, timeout_secs.max(10).min(180))
    }

    fn jarvis_runtime_root_dir() -> std::path::PathBuf {
        std::env::var("MISSIOND_RUNTIME_DIR")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
                    .join(".missiond")
                    .join("runtime")
                    .join("missiond")
            })
    }

    fn jarvis_communication_preferences_path() -> std::path::PathBuf {
        if let Some(path) = Self::env_var_trimmed("MISSIOND_JARVIS_COMMUNICATION_PREFERENCES_FILE")
        {
            return Self::expand_home_path(path.as_str())
                .unwrap_or_else(|| std::path::PathBuf::from(path));
        }
        Self::jarvis_runtime_root_dir()
            .join("jarvis")
            .join("communication-preferences.lisp")
    }

    fn default_jarvis_communication_preferences_lisp(path: &Path) -> String {
        format!(
            "(jarvis-communication-preferences\n  :schema {}\n  :owner jarvis-communication-officer\n  :provider agy\n  :model {}\n  :path {}\n  :base-style [zh-CN concise evidence-bound current-step-first]\n  :maintenance \"MissionD appends redacted preference-observation forms; provider-box AGY turns receive this file as read-only prompt context and must not write it.\"\n  :observations-append-only true\n)\n",
            Self::jarvis_lisp_string("missiond.jarvis-communication-preferences.v1"),
            Self::jarvis_lisp_string(
                Self::jarvis_default_text_only_model("agy")
                    .as_deref()
                    .unwrap_or("Gemini 3.1 Pro (High)")
            ),
            Self::jarvis_lisp_string(&path.display().to_string())
        )
    }

    async fn read_jarvis_communication_preferences_lisp() -> (std::path::PathBuf, String, bool) {
        const MAX_PREFERENCE_CHARS: usize = 12_000;
        let path = Self::jarvis_communication_preferences_path();
        let (content, existed) = match tokio::fs::read_to_string(&path).await {
            Ok(content) => (content, true),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let default = Self::default_jarvis_communication_preferences_lisp(&path);
                if let Some(parent) = path.parent() {
                    if let Err(create_err) = tokio::fs::create_dir_all(parent).await {
                        warn!(
                            "Jarvis communication preferences directory could not be created: {}",
                            create_err
                        );
                    }
                }
                if let Err(write_err) = tokio::fs::write(&path, default.as_bytes()).await {
                    warn!(
                        "Jarvis communication preferences file could not be initialized: {}",
                        write_err
                    );
                }
                (default, false)
            }
            Err(err) => {
                warn!(
                    "Jarvis communication preferences file could not be read: {}",
                    err
                );
                (
                    Self::default_jarvis_communication_preferences_lisp(&path),
                    false,
                )
            }
        };
        let redacted = crate::evidence_redactor::redact_text(&content).text;
        let preview = redacted.chars().take(MAX_PREFERENCE_CHARS).collect();
        (path, preview, existed)
    }

    fn jarvis_short_hash(input: &str) -> String {
        Sha256::digest(input.as_bytes())
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    }

    fn jarvis_communication_preference_signal(text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() {
            return false;
        }
        let lower = text.to_ascii_lowercase();
        let style_context = [
            "沟通风格",
            "说话风格",
            "回复风格",
            "表达风格",
            "沟通偏好",
            "语气",
            "口吻",
            "话术",
            "沟通官",
        ]
        .iter()
        .any(|marker| text.contains(marker));
        let durable_intent = ["以后", "下次", "记住", "偏好", "要求", "应该", "以后都"]
            .iter()
            .any(|marker| text.contains(marker));
        let concrete_style = [
            "简洁",
            "详细",
            "直接",
            "分步骤",
            "列表",
            "不要废话",
            "别废话",
            "少说",
            "多说",
            "少一点",
            "多一点",
            "中文",
            "英文",
            "markdown",
            "bullet",
            "tone",
            "style",
        ]
        .iter()
        .any(|marker| text.contains(marker) || lower.contains(marker));
        let negative_style = ["不要", "别", "少"]
            .iter()
            .any(|marker| text.contains(marker));

        (style_context && (durable_intent || concrete_style || negative_style))
            || (durable_intent && concrete_style)
    }

    async fn persist_jarvis_communication_preference_observation(
        phase: &str,
        objective: &str,
        interaction_id: Option<&str>,
        chat_id: &str,
        provider: &str,
        slot_id: &str,
        model: Option<&str>,
    ) -> anyhow::Result<Option<String>> {
        if !Self::jarvis_communication_preference_signal(objective) {
            return Ok(None);
        }
        let path = Self::jarvis_communication_preferences_path();
        let redacted = crate::evidence_redactor::redact_text(objective).text;
        let preference_text = redacted.trim().chars().take(1_200).collect::<String>();
        if preference_text.is_empty() {
            return Ok(None);
        }
        let observed_at = chrono::Utc::now().to_rfc3339();
        let observation_id = format!(
            "comm-pref-{}",
            Self::jarvis_short_hash(&format!(
                "{}|{}|{}|{}",
                phase,
                interaction_id.unwrap_or(""),
                chat_id,
                preference_text
            ))
        );
        let entry = format!(
            "(preference-observation\n  :schema {}\n  :observation-id {}\n  :observed-at {}\n  :source jarvis-user-message\n  :status candidate\n  :phase {}\n  :interaction-id {}\n  :chat-id {}\n  :provider {}\n  :slot-id {}\n  :model {}\n  :source-text-hash {}\n  :preference-text {}\n)\n",
            Self::jarvis_lisp_string("missiond.jarvis-communication-preferences.v1"),
            Self::jarvis_lisp_string(&observation_id),
            Self::jarvis_lisp_string(&observed_at),
            Self::jarvis_lisp_string(phase),
            Self::jarvis_lisp_optional(interaction_id),
            Self::jarvis_lisp_string(chat_id),
            Self::jarvis_lisp_string(provider),
            Self::jarvis_lisp_string(slot_id),
            Self::jarvis_lisp_optional(model),
            Self::jarvis_lisp_string(&Self::jarvis_short_hash(&preference_text)),
            Self::jarvis_lisp_string(&preference_text),
        );

        let mut content = match tokio::fs::read_to_string(&path).await {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Self::default_jarvis_communication_preferences_lisp(&path)
            }
            Err(err) => return Err(err.into()),
        };
        if content.contains(observation_id.as_str()) {
            return Ok(Some(observation_id));
        }
        if !content.ends_with('\n') {
            content.push('\n');
        }
        content.push('\n');
        content.push_str(&entry);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp_path = path.with_extension("lisp.tmp");
        tokio::fs::write(&tmp_path, content.as_bytes()).await?;
        tokio::fs::rename(&tmp_path, &path).await?;
        Ok(Some(observation_id))
    }

    async fn read_jarvis_file_preview(path: Option<&str>, max_chars: usize) -> Option<String> {
        let path = path?.trim();
        if path.is_empty() || path.starts_with("shared-artifact://") {
            return None;
        }
        let content = tokio::fs::read_to_string(path).await.ok()?;
        let preview = content.chars().take(max_chars).collect::<String>();
        Some(crate::evidence_redactor::redact_text(&preview).text)
    }

    async fn read_jarvis_context_preview(context_pack_file: Option<&str>) -> Option<String> {
        Self::read_jarvis_file_preview(context_pack_file, 16_000).await
    }

    async fn read_jarvis_grounding_report_preview(
        grounding_report_file: Option<&str>,
    ) -> Option<String> {
        Self::read_jarvis_file_preview(grounding_report_file, 24_000).await
    }

    async fn read_jarvis_plan_grounding_report_preview(
        grounding_report_file: Option<&str>,
    ) -> Option<String> {
        Self::read_jarvis_file_preview(grounding_report_file, 12_000).await
    }

    fn build_jarvis_direct_answer_prompt(
        objective: &str,
        grounding_context_id: &str,
        context_pack_path: Option<&str>,
        context_pack_file: Option<&str>,
        grounding_report_file: Option<&str>,
        grounding_report_artifact_path: Option<&str>,
        grounding_report_hash: Option<&str>,
        grounding_report_preview: Option<&str>,
        context_preview: Option<&str>,
        intent_artifact_id: &str,
        plan_artifact_id: &str,
        key_judgment: &JarvisKeyJudgmentArtifactRef,
        plan_direct_answer_draft: Option<&str>,
        permission_context: &serde_json::Value,
        sources_used: &[String],
        media_context: &serde_json::Value,
        communication_preferences_file: Option<&Path>,
        communication_preferences_existed: bool,
        communication_preferences_lisp: &str,
    ) -> (String, String) {
        let system_prompt = "你是 MissionD Jarvis 的 grounded direct-answer materializer。你只能基于随请求提供的 grounding report、key judgment、grounding context、PermissionContext、sources_used、intent/plan artifact 生成自然语言回答。你会收到 MissionD 维护的 communication_preferences_lisp，只能把它作为用户沟通偏好的只读风格上下文；不要调用工具，不要读取文件，不要声称已创建 BoardTask，不要编造没有证据的事实。若证据不足，直接说明证据不足并列出还缺什么。".to_string();
        let payload = serde_json::json!({
            "schema": "missiond.jarvis-grounded-direct-answer-input.v1",
            "objective": objective,
            "grounding_context_id": grounding_context_id,
            "context_pack_path": context_pack_path,
            "context_pack_file": context_pack_file,
            "grounding_report_file": grounding_report_file,
            "grounding_report_artifact_path": grounding_report_artifact_path,
            "grounding_report_hash": grounding_report_hash,
            "grounding_report_preview": grounding_report_preview,
            "context_preview": context_preview,
            "plan_direct_answer_draft": plan_direct_answer_draft,
            "intent_artifact_id": intent_artifact_id,
            "plan_artifact_id": plan_artifact_id,
            "key_judgment_artifact_id": key_judgment.artifact_id,
            "key_judgment_artifact_hash": key_judgment.artifact_hash,
            "key_judgment": key_judgment.judgment,
            "key_judgment_evidence_refs": key_judgment.evidence_refs,
            "planning_implications": key_judgment.planning_implications,
            "acceptance_focus": key_judgment.acceptance_focus,
            "permission_context": permission_context,
            "sources_used": sources_used,
            "media_context": media_context,
            "communication_preferences_file": communication_preferences_file
                .map(|path| path.display().to_string()),
            "communication_preferences_existed": communication_preferences_existed,
            "communication_preferences_schema": "missiond.jarvis-communication-preferences.v1",
            "communication_preferences_lisp": communication_preferences_lisp,
            "answer_contract": {
                "must_use_grounding": true,
                "must_not_create_board_task": true,
                "must_not_claim_tool_execution": true,
                "must_apply_communication_preferences_when_safe": true,
                "must_report_evidence": true
            }
        });
        let prompt = format!(
            "请基于以下 MissionD grounding payload 给用户一个直接回答。优先使用 grounding_report_preview，其次使用 context_preview。回答要简洁，但必须说明依据来自哪些 MissionD sources；如果用户问身份/你是谁，要分别说明 Jarvis/MissionD 的身份和 PermissionContext 中可确认的用户信息。\n\n{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
        );
        (system_prompt, prompt)
    }

    fn build_jarvis_communicator_prompt(
        phase: &str,
        objective: &str,
        context: &serde_json::Value,
        communication_preferences_file: Option<&Path>,
        communication_preferences_existed: bool,
        communication_preferences_lisp: &str,
    ) -> String {
        let payload = serde_json::json!({
            "schema": "missiond.jarvis-communication-officer-input.v1",
            "phase": phase,
            "objective": objective,
            "context": context,
            "communication_preferences_file": communication_preferences_file
                .map(|path| path.display().to_string()),
            "communication_preferences_existed": communication_preferences_existed,
            "communication_preferences_schema": "missiond.jarvis-communication-preferences.v1",
            "communication_preferences_lisp": communication_preferences_lisp,
            "communication_preferences_contract": {
                "maintainer": "missiond-runtime",
                "provider_access": "read-only-prompt-context",
                "observation_status": "candidate",
                "must_not_write_file": true
            },
            "output_contract": {
                "language": "zh-CN",
                "style": "concise",
                "must_be_evidence_bound": true,
                "must_apply_communication_preferences_when_safe": true,
                "must_not_show_lisp_body": true,
                "must_not_ask_user_to_confirm_intent_or_plan": true,
                "must_not_claim_terminal_result_unless_context_has_terminal_task_result": true,
                "must_say_current_step_before_user_message": true
            }
        });
        format!(
            "你是 MissionD Jarvis 的沟通官，由 AGY Gemini 3.1 Pro 工位承担。\
             你只负责把当前计划、派工状态或执行结果用中文告诉用户。\
             你不能修改 intent.lisp、key judgment、plan.lisp、BoardTask，也不能声称没有证据的执行结果。\
             你会收到 communication_preferences_lisp；它是 MissionD 维护的候选沟通偏好，只能作为风格参考，不能展示正文或声称已经改写偏好文件。\
             不要展示 Lisp 正文，不要让用户点击确认 intent/plan。\
             回复必须先用一到三行说明当前执行到哪一步，然后给出对用户可读的简短说明。\n\n{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
        )
    }

    async fn materialize_jarvis_communication(
        stream: &mut TcpStream,
        progress_bus: &JarvisProgressBus,
        artifact_writer: &JarvisArtifactSlot,
        chat_id: &str,
        interaction_id: Option<&str>,
        phase: &str,
        objective: &str,
        context: serde_json::Value,
        provider_box_http: &ProviderBoxHttpSlot,
        db: Option<&Arc<dyn crate::db::traits::MissionStore>>,
        conversation_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let provider = Self::jarvis_communicator_provider();
        let engine = Self::provider_box_engine_for_provider(provider.as_str())?;
        let timeout_secs = Self::jarvis_communicator_timeout_secs();
        let slot_id = Self::jarvis_communicator_slot_id(provider.as_str());
        let model = Self::jarvis_communicator_model_for_provider(provider.as_str());
        let preference_observation_id =
            match Self::persist_jarvis_communication_preference_observation(
                phase,
                objective,
                interaction_id,
                chat_id,
                provider.as_str(),
                slot_id.as_str(),
                model.as_deref(),
            )
            .await
            {
                Ok(id) => id,
                Err(err) => {
                    warn!(
                        "Jarvis communication preference observation could not be persisted: {}",
                        err
                    );
                    None
                }
            };
        let (
            communication_preferences_file,
            communication_preferences_lisp,
            communication_preferences_existed,
        ) = Self::read_jarvis_communication_preferences_lisp().await;
        Self::write_jarvis_progress(
            stream,
            progress_bus,
            chat_id,
            interaction_id,
            "communicator",
            "communication_officer_start",
            "running",
            "正在让 AGY Gemini 3.1 Pro 沟通官整理当前步骤和用户可见说明。",
            None,
            Some(&slot_id),
            Some("jarvis-communication-officer"),
        )
        .await?;

        let prompt = Self::build_jarvis_communicator_prompt(
            phase,
            objective,
            &context,
            Some(communication_preferences_file.as_path()),
            communication_preferences_existed,
            communication_preferences_lisp.as_str(),
        );
        let correlation_id = format!("jarvis-communicator-{}", uuid::Uuid::new_v4().simple());
        let agy_text_lane = engine == "agy";
        let body = serde_json::json!({
            "schema": "missiond.provider-interaction-request.v1",
            "command": "pure-text-single-turn",
            "provider": &provider,
            "engine": engine,
            "prompt": prompt,
            "model": model.clone(),
            "timeout_secs": timeout_secs,
            "correlation_id": correlation_id,
            "slot_id": slot_id,
            "provider_box_lane": "jarvis-communication-officer",
            "xjp_request_stage": phase,
            "dangerously_bypass_approvals_and_sandbox": agy_text_lane,
            "allow_model_switch": agy_text_lane,
            "allow_respawn": true,
            "require_verification": true,
            "model_switch_policy": {
                "target_model": model.clone(),
                "allow_respawn": true,
                "require_verification": true
            },
            "no_tools": true,
            "no_mcp": true,
            "no_shell": true,
            "no_file_access": true,
            "output_contract": {
                "media_type": "text/plain",
                "single_turn": true
            },
            "tool_policy": {
                "sandbox": "read-only",
                "approval_policy": "never"
            }
        });
        let text = Self::call_provider_box_turn(
            provider_box_http,
            body,
            timeout_secs,
            "JARVIS_COMMUNICATOR",
        )
        .await?;
        let text = text.trim().to_string();
        if text.is_empty() {
            anyhow::bail!("JARVIS_COMMUNICATOR_EMPTY: provider-box returned no visible answer");
        }

        let event_name = if phase.contains("final") || phase.contains("result") {
            "communicator_final"
        } else {
            "communicator_status"
        };
        let event = serde_json::json!({
            "phase": phase,
            "interaction_id": interaction_id,
            "provider": provider,
            "engine": engine,
            "slot_id": Self::jarvis_communicator_slot_id(Self::jarvis_communicator_provider().as_str()),
            "model": model,
            "content": text,
            "communication_preferences_file": communication_preferences_file.display().to_string(),
            "communication_preferences_schema": "missiond.jarvis-communication-preferences.v1",
            "preference_observation_ids": preference_observation_id
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
        });
        Self::write_sse_event(stream, event_name, &event).await?;
        Self::persist_interaction_event(db, conversation_id, interaction_id, event_name, &event)
            .await;
        let _ = Self::put_jarvis_artifact(
            artifact_writer,
            JarvisArtifactRequest {
                kind: "interaction-communication".to_string(),
                project_id: None,
                task_id: context
                    .get("task_id")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned),
                payload: serde_json::json!({
                    "schema": "missiond.interaction-communication.v1",
                    "interaction_id": interaction_id,
                    "phase": phase,
                    "communicator_provider": event.get("provider").cloned(),
                    "communicator_engine": engine,
                    "communicator_slot_id": event.get("slot_id").cloned(),
                    "communicator_model": event.get("model").cloned(),
                    "communication_preferences_file": event.get("communication_preferences_file").cloned(),
                    "communication_preferences_schema": "missiond.jarvis-communication-preferences.v1",
                    "preference_observation_ids": event.get("preference_observation_ids").cloned(),
                    "objective": objective,
                    "context": context,
                    "content": event.get("content").cloned(),
                }),
                metadata: serde_json::json!({
                    "schema": "missiond.interaction-communication.v1",
                    "interaction_id": interaction_id,
                    "phase": phase,
                }),
            },
        )
        .await;
        Self::write_jarvis_progress(
            stream,
            progress_bus,
            chat_id,
            interaction_id,
            "communicator",
            "communication_officer_completed",
            "completed",
            "沟通官已生成用户可见说明，准备发送给用户。",
            None,
            event.get("slot_id").and_then(|value| value.as_str()),
            Some("jarvis-communication-officer"),
        )
        .await?;
        Ok(event
            .get("content")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string())
    }

    async fn stream_jarvis_grounded_direct_answer(
        stream: &mut TcpStream,
        progress_bus: &JarvisProgressBus,
        artifact_writer: &JarvisArtifactSlot,
        chat_id: &str,
        interaction_id: Option<&str>,
        objective: &str,
        grounding_context_id: &str,
        context_pack_path: Option<&str>,
        context_pack_file: Option<&str>,
        grounding_report_file: Option<&str>,
        grounding_report_artifact_path: Option<&str>,
        grounding_report_hash: Option<&str>,
        intent_artifact_id: &str,
        plan_artifact_id: &str,
        key_judgment: &JarvisKeyJudgmentArtifactRef,
        plan_direct_answer_draft: Option<&str>,
        permission_context: &serde_json::Value,
        sources_used: &[String],
        media_context: &serde_json::Value,
        provider_box_http: &ProviderBoxHttpSlot,
        db: Option<&Arc<dyn crate::db::traits::MissionStore>>,
        conversation_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let provider = Self::jarvis_direct_answer_provider();
        let timeout_secs = Self::jarvis_direct_answer_timeout_secs();
        let direct_answer_slot_id = Self::jarvis_text_only_slot_id(
            provider.as_str(),
            std::env::var("MISSIOND_JARVIS_DIRECT_ANSWER_SLOT_ID")
                .ok()
                .or_else(|| std::env::var("MISSIOND_JARVIS_COMMUNICATOR_SLOT_ID").ok())
                .as_deref(),
            "slot-agy-gemini-31-pro-high-jarvis-communicator-a",
        );
        let direct_answer_model = Self::jarvis_direct_answer_model(provider.as_str());
        let direct_answer_preference_observation_id =
            match Self::persist_jarvis_communication_preference_observation(
                "grounded_direct_answer",
                objective,
                interaction_id,
                chat_id,
                provider.as_str(),
                direct_answer_slot_id.as_str(),
                direct_answer_model.as_deref(),
            )
            .await
            {
                Ok(id) => id,
                Err(err) => {
                    warn!(
                        "Jarvis direct-answer communication preference observation could not be persisted: {}",
                        err
                    );
                    None
                }
            };
        Self::write_jarvis_progress(
            stream,
            progress_bus,
            chat_id,
            interaction_id,
            "direct_answer",
            "provider_box_grounded_direct_answer_start",
            "running",
            "plan 已确认为 grounded_direct_answer，正在准备直接回答；优先使用 plan.lisp 草稿，必要时再进入 provider_box，不创建 BoardTask。",
            None,
            None,
            Some("provider-box"),
        )
        .await?;

        let mut answer_provider = provider.clone();
        let mut answer_source = "provider_box_grounded_direct_answer".to_string();
        let answer = if let Some(draft) = plan_direct_answer_draft
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            answer_provider = "codex_cli_plan_author".to_string();
            answer_source = "plan_direct_answer_draft".to_string();
            Self::write_jarvis_progress(
                stream,
                progress_bus,
                chat_id,
                interaction_id,
                "direct_answer",
                "plan_direct_answer_draft_selected",
                "completed",
                "plan.lisp 已包含 grounded direct answer 草稿，直接归档为本次终态回答，不再额外等待慢 provider 回合。",
                None,
                None,
                Some("codex-plan-author"),
            )
            .await?;
            draft.to_string()
        } else {
            let engine = Self::provider_box_engine_for_provider(provider.as_str())?;
            let grounding_report_preview =
                Self::read_jarvis_grounding_report_preview(grounding_report_file).await;
            let context_preview = Self::read_jarvis_context_preview(context_pack_file).await;
            let (
                communication_preferences_file,
                communication_preferences_lisp,
                communication_preferences_existed,
            ) = Self::read_jarvis_communication_preferences_lisp().await;
            let (system_prompt, prompt) = Self::build_jarvis_direct_answer_prompt(
                objective,
                grounding_context_id,
                context_pack_path,
                context_pack_file,
                grounding_report_file,
                grounding_report_artifact_path,
                grounding_report_hash,
                grounding_report_preview.as_deref(),
                context_preview.as_deref(),
                intent_artifact_id,
                plan_artifact_id,
                key_judgment,
                plan_direct_answer_draft,
                permission_context,
                sources_used,
                media_context,
                Some(communication_preferences_file.as_path()),
                communication_preferences_existed,
                communication_preferences_lisp.as_str(),
            );
            let prompt = format!("{system_prompt}\n\n{prompt}");
            let correlation_id = format!("jarvis-direct-answer-{}", uuid::Uuid::new_v4().simple());
            let pure_text_command = engine == "agy";
            let command = if pure_text_command {
                "pure-text-single-turn"
            } else {
                "grounded-direct-answer"
            };
            let body = serde_json::json!({
                "schema": "missiond.provider-interaction-request.v1",
                "command": command,
                "provider": &provider,
                "engine": engine,
                "prompt": prompt,
                "model": direct_answer_model.clone(),
                "timeout_secs": timeout_secs,
                "correlation_id": correlation_id,
                "slot_id": direct_answer_slot_id,
                "provider_box_lane": "jarvis-communication-officer",
                "xjp_request_stage": "grounded_direct_answer",
                "dangerously_bypass_approvals_and_sandbox": pure_text_command && engine == "agy",
                "allow_model_switch": pure_text_command && engine == "agy",
                "allow_respawn": true,
                "require_verification": true,
                "model_switch_policy": {
                    "target_model": direct_answer_model,
                    "allow_respawn": true,
                    "require_verification": true
                },
                "no_tools": true,
                "no_mcp": true,
                "no_shell": true,
                "no_file_access": true,
                "output_contract": {
                    "media_type": "text/plain",
                    "single_turn": true
                },
                "tool_policy": {
                    "sandbox": "read-only",
                    "approval_policy": "never"
                }
            });
            let stream_budget_secs = Self::jarvis_direct_answer_stream_budget_secs(timeout_secs);
            match tokio::time::timeout(
                std::time::Duration::from_secs(stream_budget_secs),
                Self::call_provider_box_turn(
                    provider_box_http,
                    body,
                    timeout_secs,
                    "JARVIS_DIRECT_ANSWER",
                ),
            )
            .await
            {
                Ok(answer) => answer?,
                Err(_) => {
                    let diagnostic = serde_json::json!({
                        "interaction_id": interaction_id,
                        "phase": "direct_answer",
                        "phase_code": "direct_answer",
                        "error": {
                            "code": "JARVIS_DIRECT_ANSWER_STREAM_BUDGET_EXCEEDED",
                            "message": format!("provider-box direct answer did not finish within the public stream budget of {stream_budget_secs}s")
                        },
                        "timeout_secs": timeout_secs,
                        "stream_budget_secs": stream_budget_secs,
                        "terminal_task_result": false,
                        "next_action": "Replay/follow the interaction ledger or retry after inspecting provider-box lane state; do not keep the public SSE stream open past edge timeout."
                    });
                    Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
                    Self::persist_interaction_event(
                        db,
                        conversation_id,
                        interaction_id,
                        "diagnostic",
                        &diagnostic,
                    )
                    .await;
                    let pending_event = serde_json::json!({
                        "interaction_id": interaction_id,
                        "phase": "direct_answer",
                        "status": "provider_box_running_or_cancelled",
                        "terminal_task_result": false,
                        "follow_payload": {
                            "missiond_follow_interaction_id": interaction_id,
                            "grounding_context_id": grounding_context_id,
                            "intent_artifact_id": intent_artifact_id,
                            "plan_artifact_id": plan_artifact_id,
                            "stream": true
                        }
                    });
                    Self::write_sse_event(stream, "result_pending", &pending_event).await?;
                    Self::persist_interaction_event(
                        db,
                        conversation_id,
                        interaction_id,
                        "result_pending",
                        &pending_event,
                    )
                    .await;
                    Self::write_sse_openai_text_and_persist(
                        stream,
                        chat_id,
                        "plan.lisp 已归档，但直接回答 provider 未在公网流预算内完成；我先结束本次流，避免 iOS/edge 超时。请稍后通过 interaction follow/replay 查看结果。",
                        Some("stop"),
                        db,
                        conversation_id,
                    )
                    .await?;
                    return Ok(());
                }
            }
        };
        if answer.trim().is_empty() {
            anyhow::bail!("JARVIS_DIRECT_ANSWER_EMPTY: provider-box returned no visible answer");
        }
        Self::write_sse_event(
            stream,
            "worker_status",
            &serde_json::json!({
                "phase": "direct_answer",
                "provider": &answer_provider,
                "answer_source": &answer_source,
                "status": "completed",
                "terminal_task_result": false,
            }),
        )
        .await?;
        Self::write_sse_event(
            stream,
            "answer_delta",
            &serde_json::json!({
                "phase": "direct_answer",
                "provider": &answer_provider,
                "answer_source": &answer_source,
                "content": answer.clone(),
            }),
        )
        .await?;
        Self::write_sse_openai_text(stream, chat_id, &answer, None).await?;

        Self::persist_jarvis_visible_assistant_message(db, conversation_id, &answer).await;
        let payload = serde_json::json!({
            "schema": "missiond.interaction-result-artifact.v1",
            "kind": "grounded-direct-answer",
            "interaction_id": interaction_id,
            "objective": objective,
            "grounding_context_id": grounding_context_id,
            "context_pack_path": context_pack_path,
            "context_pack_file": context_pack_file,
            "grounding_report_file": grounding_report_file,
            "grounding_report_artifact_path": grounding_report_artifact_path,
            "grounding_report_hash": grounding_report_hash,
            "intent_artifact_id": intent_artifact_id,
            "plan_artifact_id": plan_artifact_id,
            "key_judgment_artifact_id": key_judgment.artifact_id,
            "key_judgment_artifact_hash": key_judgment.artifact_hash,
            "key_judgment": key_judgment.judgment,
            "provider": &answer_provider,
            "answer_source": &answer_source,
            "answer_text": answer,
            "sources_used": sources_used,
            "media_context": media_context,
            "communication_preferences_file": Self::jarvis_communication_preferences_path()
                .display()
                .to_string(),
            "communication_preferences_schema": "missiond.jarvis-communication-preferences.v1",
            "preference_observation_ids": direct_answer_preference_observation_id
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
            "terminal_task_result": true,
            "board_task_created": false,
        });
        let artifact = Self::put_jarvis_artifact(
            artifact_writer,
            JarvisArtifactRequest {
                kind: "interaction-direct-answer".to_string(),
                project_id: None,
                task_id: None,
                payload: payload.clone(),
                metadata: serde_json::json!({
                    "schema": "missiond.interaction-result-artifact.v1",
                    "interaction_id": interaction_id,
                    "grounding_context_id": grounding_context_id,
                    "grounding_report_file": grounding_report_file,
                    "grounding_report_artifact_path": grounding_report_artifact_path,
                    "grounding_report_hash": grounding_report_hash,
                    "intent_artifact_id": intent_artifact_id,
                    "plan_artifact_id": plan_artifact_id,
                    "key_judgment_artifact_id": key_judgment.artifact_id,
                    "key_judgment_artifact_hash": key_judgment.artifact_hash,
                    "execution_mode": "grounded_direct_answer",
                    "answer_source": &answer_source,
                }),
            },
        )
        .await
        .map_err(|err| anyhow::anyhow!("JARVIS_DIRECT_ANSWER_ARTIFACT_FAILED: {err}"))?;
        Self::write_sse_event(
            stream,
            "result_artifact",
            &serde_json::json!({
                "phase": "result_ready",
                "interaction_id": interaction_id,
                "artifact_id": &artifact.artifact_id,
                "artifact_hash": &artifact.artifact_hash,
                "artifact_path": &artifact.path,
                "execution_mode": "grounded_direct_answer",
                "answer_source": &answer_source,
                "terminal_task_result": true,
                "board_task_created": false,
            }),
        )
        .await?;
        let result_artifact_event = serde_json::json!({
            "phase": "result_ready",
            "interaction_id": interaction_id,
            "artifact_id": &artifact.artifact_id,
            "artifact_hash": &artifact.artifact_hash,
            "artifact_path": &artifact.path,
            "execution_mode": "grounded_direct_answer",
            "answer_source": &answer_source,
            "terminal_task_result": true,
            "board_task_created": false,
        });
        Self::persist_interaction_event(
            db,
            conversation_id,
            interaction_id,
            "result_artifact",
            &result_artifact_event,
        )
        .await;
        Self::write_sse_event(
            stream,
            "final",
            &serde_json::json!({
                "phase": "done",
                "interaction_id": interaction_id,
                "status": "done",
                "execution_mode": "grounded_direct_answer",
                "answer_source": &answer_source,
                "terminal_task_result": true,
                "result_artifact_id": &artifact.artifact_id,
                "result_artifact_hash": &artifact.artifact_hash,
            }),
        )
        .await?;
        let final_event = serde_json::json!({
            "phase": "done",
            "interaction_id": interaction_id,
            "status": "done",
            "execution_mode": "grounded_direct_answer",
            "answer_source": &answer_source,
            "terminal_task_result": true,
            "result_artifact_id": &artifact.artifact_id,
            "result_artifact_hash": &artifact.artifact_hash,
        });
        Self::persist_interaction_event(db, conversation_id, interaction_id, "final", &final_event)
            .await;
        Self::write_sse_openai_text(stream, chat_id, "", Some("stop")).await?;
        Ok(())
    }

    async fn stream_jarvis_task_until_terminal(
        db: &Arc<dyn crate::db::traits::MissionStore>,
        jarvis_artifact_writer: &JarvisArtifactSlot,
        provider_box_http: &ProviderBoxHttpSlot,
        progress_bus: &JarvisProgressBus,
        stream: &mut TcpStream,
        chat_id: &str,
        interaction_id: Option<&str>,
        task_id: &str,
        conversation_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let wait_secs = jarvis_task_wait_secs();
        let public_stream_budget_secs = jarvis_public_stream_budget_secs();
        let db_poll_timeout = tokio::time::Duration::from_secs(jarvis_db_poll_timeout_secs());
        let visible_heartbeat = tokio::time::Duration::from_secs(jarvis_visible_heartbeat_secs());
        let started_at = std::time::Instant::now();
        let mut last_visible_heartbeat = started_at;
        let mut last_status = String::new();
        let mut last_slot: Option<String> = None;
        let mut seen_note_ids: Vec<String> = Vec::new();
        let mut latest_summary: Option<String> = None;
        let mut latest_artifact_hash: Option<String> = None;

        loop {
            if started_at.elapsed() > tokio::time::Duration::from_secs(wait_secs) {
                let diagnostic = serde_json::json!({
                    "phase": "workers_running",
                    "task_id": task_id,
                    "error": {"code": "JARVIS_WORKER_TIMEOUT", "message": format!("BoardTask did not reach a terminal state within {wait_secs}s")},
                    "wait_secs": wait_secs,
                    "next_action": "Inspect the BoardTask, slot state, and task-result-artifact before retrying."
                });
                Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
                let pending_event = serde_json::json!({
                    "phase": "timeout",
                    "task_id": task_id,
                    "status": "worker_timeout",
                    "wait_secs": wait_secs,
                    "terminal_task_result": false,
                    "follow_payload": {
                        "missiond_follow_task_id": task_id,
                        "stream": true
                    }
                });
                Self::write_sse_event(stream, "result_pending", &pending_event).await?;
                Self::write_sse_openai_text_and_persist(
                    stream,
                    chat_id,
                    "任务尚未在等待窗口内完成，我不会伪造结果。请检查 BoardTask、工位和 task-result-artifact。",
                    Some("stop"),
                    Some(db),
                    conversation_id,
                )
                .await?;
                return Ok(());
            }

            let task_result = match tokio::time::timeout(
                db_poll_timeout,
                db.get_board_task(task_id),
            )
            .await
            {
                Ok(result) => result?,
                Err(_) => {
                    let diagnostic = serde_json::json!({
                        "phase": "workers_running",
                        "task_id": task_id,
                        "error": {
                            "code": "BOARD_TASK_POLL_TIMEOUT",
                            "message": format!("BoardTask polling did not finish within {}s", db_poll_timeout.as_secs())
                        },
                        "next_action": "Investigate MissionD DB/event wait path; do not keep the public SSE stream open silently."
                    });
                    Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
                    let pending_event = serde_json::json!({
                        "phase": "poll_timeout",
                        "task_id": task_id,
                        "status": "poll_timeout",
                        "terminal_task_result": false,
                        "follow_payload": {
                            "missiond_follow_task_id": task_id,
                            "stream": true
                        }
                    });
                    Self::write_sse_event(stream, "result_pending", &pending_event).await?;
                    Self::write_sse_openai_text_and_persist(
                        stream,
                        chat_id,
                        "任务监督链路读取 BoardTask 超时；我不会继续静默等待。请检查 MissionD DB / EventBus / worker completion 链路。",
                        Some("stop"),
                        Some(db),
                        conversation_id,
                    )
                    .await?;
                    return Ok(());
                }
            };

            let Some(task) = task_result else {
                let diagnostic = serde_json::json!({
                    "phase": "workers_running",
                    "task_id": task_id,
                    "error": {"code": "BOARD_TASK_NOT_FOUND", "message": "Created BoardTask disappeared before completion"}
                });
                Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
                Self::write_sse_openai_text_and_persist(
                    stream,
                    chat_id,
                    "任务记录丢失，无法继续监督执行。",
                    Some("stop"),
                    Some(db),
                    conversation_id,
                )
                .await?;
                return Ok(());
            };

            let status = task.status.as_str().to_string();
            match tokio::time::timeout(
                db_poll_timeout,
                db.get_board_task_projection_artifact_hash(task_id),
            )
            .await
            {
                Ok(Ok(Some(artifact_hash))) if !artifact_hash.trim().is_empty() => {
                    latest_artifact_hash = Some(artifact_hash.trim().to_string());
                }
                Ok(Ok(_)) => {}
                Ok(Err(error)) => {
                    let diagnostic = serde_json::json!({
                        "phase": "workers_running",
                        "task_id": task_id,
                        "error": {
                            "code": "TASK_RESULT_ARTIFACT_LOOKUP_FAILED",
                            "message": error.to_string()
                        },
                        "next_action": "Inspect board_task_views and task_result_artifacts; do not parse Board notes for artifact hashes."
                    });
                    Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
                }
                Err(_) => {
                    let diagnostic = serde_json::json!({
                        "phase": "workers_running",
                        "task_id": task_id,
                        "error": {
                            "code": "TASK_RESULT_ARTIFACT_LOOKUP_TIMEOUT",
                            "message": format!("task-result-artifact lookup did not finish within {}s", db_poll_timeout.as_secs())
                        },
                        "next_action": "Inspect board_task_views/task_result_artifacts query latency; Board notes remain projection only."
                    });
                    Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
                }
            }
            let current_slot = task
                .claim_executor_id
                .clone()
                .or_else(|| task.assignee.clone());
            if last_status != status || last_slot != current_slot {
                let event = serde_json::json!({
                    "task_id": task.id,
                    "status": status,
                    "slot_id": current_slot,
                    "claim_executor_type": task.claim_executor_type,
                    "phase": "workers_running",
                });
                Self::write_sse_event(stream, "worker_status", &event).await?;
                if last_slot != current_slot && current_slot.is_some() {
                    Self::write_sse_event(stream, "worker_dispatched", &event).await?;
                }
                last_status = status.clone();
                last_slot = current_slot;
            }

            match tokio::time::timeout(db_poll_timeout, db.get_board_task_notes(task_id)).await {
                Ok(Ok(notes)) => {
                    for note in notes {
                        if seen_note_ids.iter().any(|id| id == &note.id) {
                            continue;
                        }
                        seen_note_ids.push(note.id.clone());
                        let is_summary = note.note_type.as_str() == "summary";
                        let content = note.content.clone();
                        if is_summary {
                            latest_summary = Some(content.clone());
                        }
                        let artifact_hash = latest_artifact_hash.clone();
                        let max_chars = if is_summary { 12_000 } else { 1_200 };
                        let content_preview = content.chars().take(max_chars).collect::<String>();
                        let event = serde_json::json!({
                            "task_id": task_id,
                            "note_id": note.id,
                            "note_type": note.note_type.as_str(),
                            "author": note.author,
                            "created_at": note.created_at,
                            "artifact_hash": artifact_hash,
                            "content": content_preview,
                            "truncated": content.chars().count() > max_chars,
                        });
                        if artifact_hash.is_some() {
                            Self::write_sse_event(stream, "result_artifact", &event).await?;
                        } else {
                            Self::write_sse_event(stream, "worker_status", &event).await?;
                        }
                    }
                }
                Ok(Err(error)) => {
                    let diagnostic = serde_json::json!({
                        "phase": "workers_running",
                        "task_id": task_id,
                        "error": {
                            "code": "BOARD_TASK_NOTES_POLL_FAILED",
                            "message": error.to_string()
                        }
                    });
                    Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
                }
                Err(_) => {
                    let diagnostic = serde_json::json!({
                        "phase": "workers_running",
                        "task_id": task_id,
                        "error": {
                            "code": "BOARD_TASK_NOTES_POLL_TIMEOUT",
                            "message": format!("BoardTask notes polling did not finish within {}s", db_poll_timeout.as_secs())
                        }
                    });
                    Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
                }
            }

            match task.status {
                crate::types::BoardTaskStatus::Done => {
                    if latest_summary.is_none() || latest_artifact_hash.is_none() {
                        match tokio::time::timeout(
                            db_poll_timeout,
                            db.get_board_task_notes(task_id),
                        )
                        .await
                        {
                            Ok(Ok(notes)) => {
                                if let Some(note) = notes
                                    .iter()
                                    .rev()
                                    .find(|note| note.note_type.as_str() == "summary")
                                {
                                    let content = note.content.clone();
                                    latest_summary = Some(content.clone());
                                    let event = serde_json::json!({
                                        "task_id": task_id,
                                        "note_id": note.id,
                                        "note_type": note.note_type.as_str(),
                                        "author": note.author,
                                        "created_at": note.created_at,
                                        "artifact_hash": latest_artifact_hash.clone(),
                                        "content": content.chars().take(12_000).collect::<String>(),
                                        "truncated": content.chars().count() > 12_000,
                                        "source": "jarvis-follow-board-projection",
                                    });
                                    Self::write_sse_event(stream, "worker_status", &event).await?;
                                }
                            }
                            Ok(Err(error)) => {
                                let diagnostic = serde_json::json!({
                                    "phase": "done",
                                    "task_id": task_id,
                                    "error": {
                                        "code": "BOARD_TASK_NOTES_REVALIDATE_FAILED",
                                        "message": error.to_string()
                                    }
                                });
                                Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
                            }
                            Err(_) => {
                                let diagnostic = serde_json::json!({
                                    "phase": "done",
                                    "task_id": task_id,
                                    "error": {
                                        "code": "BOARD_TASK_NOTES_REVALIDATE_TIMEOUT",
                                        "message": format!("BoardTask is done but notes revalidation did not finish within {}s", db_poll_timeout.as_secs())
                                    },
                                    "next_action": "Inspect DB pool/EventBus completion path before retrying; do not keep mobile SSE waiting silently."
                                });
                                Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
                                Self::write_sse_openai_text_and_persist(
                                    stream,
                                    chat_id,
                                    "任务已完成，但结果 notes/task-result-artifact 读取超时；我不会伪造结果。请检查结果落盘链路后重试 follow。",
                                    Some("stop"),
                                    Some(db),
                                    conversation_id,
                                )
                                .await?;
                                return Ok(());
                            }
                        }
                    }
                    if latest_artifact_hash.is_none() {
                        let diagnostic = serde_json::json!({
                            "phase": "done",
                            "task_id": task_id,
                            "error": {
                                "code": "TASK_RESULT_ARTIFACT_REQUIRED",
                                "message": "BoardTask is done but no task-result-artifact was durably written"
                            },
                            "next_action": "Inspect task-result-artifact writer before retrying; do not treat Board note as final authority."
                        });
                        Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
                        Self::write_sse_openai_text_and_persist(
                            stream,
                            chat_id,
                            "任务已完成但 task-result-artifact 未落盘；我不会把 Board note 当作最终结果返回。请先修复结果落盘链路。",
                            Some("stop"),
                            Some(db),
                            conversation_id,
                        )
                        .await?;
                        return Ok(());
                    }
                    let final_text = latest_summary.unwrap_or_else(|| {
                        "任务已完成，但没有找到 summary note；请检查 task-result-artifact。"
                            .to_string()
                    });
                    let user_text = match Self::materialize_jarvis_communication(
                        stream,
                        progress_bus,
                        jarvis_artifact_writer,
                        chat_id,
                        interaction_id,
                        "result_final",
                        task_id,
                        serde_json::json!({
                            "task_id": task_id,
                            "status": "done",
                            "terminal_task_result": true,
                            "task_result_artifact_hash": latest_artifact_hash,
                            "raw_summary": final_text,
                        }),
                        provider_box_http,
                        Some(db),
                        conversation_id,
                    )
                    .await
                    {
                        Ok(text) => text,
                        Err(error) => {
                            let diagnostic = serde_json::json!({
                                "phase": "communicator",
                                "task_id": task_id,
                                "error": {
                                    "code": "JARVIS_COMMUNICATOR_FAILED",
                                    "message": error.to_string()
                                }
                            });
                            Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
                            final_text
                        }
                    };
                    let final_event = serde_json::json!({
                        "phase": "done",
                        "task_id": task_id,
                        "artifact_hash": latest_artifact_hash,
                    });
                    Self::write_sse_event(stream, "final", &final_event).await?;
                    Self::write_sse_openai_text_and_persist(
                        stream,
                        chat_id,
                        &user_text,
                        Some("stop"),
                        Some(db),
                        conversation_id,
                    )
                    .await?;
                    return Ok(());
                }
                crate::types::BoardTaskStatus::Failed
                | crate::types::BoardTaskStatus::Blocked
                | crate::types::BoardTaskStatus::Skipped => {
                    let final_text = latest_summary.unwrap_or_else(|| {
                        format!(
                            "任务进入终态 `{}`，但没有可用 summary。",
                            task.status.as_str()
                        )
                    });
                    let user_text = match Self::materialize_jarvis_communication(
                        stream,
                        progress_bus,
                        jarvis_artifact_writer,
                        chat_id,
                        interaction_id,
                        "result_terminal_diagnostic",
                        task_id,
                        serde_json::json!({
                            "task_id": task_id,
                            "status": task.status.as_str(),
                            "terminal_task_result": false,
                            "task_result_artifact_hash": latest_artifact_hash,
                            "raw_summary": final_text,
                        }),
                        provider_box_http,
                        Some(db),
                        conversation_id,
                    )
                    .await
                    {
                        Ok(text) => text,
                        Err(error) => {
                            let diagnostic = serde_json::json!({
                                "phase": "communicator",
                                "task_id": task_id,
                                "error": {
                                    "code": "JARVIS_COMMUNICATOR_FAILED",
                                    "message": error.to_string()
                                }
                            });
                            Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
                            final_text
                        }
                    };
                    let diagnostic = serde_json::json!({
                        "phase": task.status.as_str(),
                        "task_id": task_id,
                        "artifact_hash": latest_artifact_hash,
                        "message": user_text,
                    });
                    Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
                    Self::write_sse_openai_text_and_persist(
                        stream,
                        chat_id,
                        &user_text,
                        Some("stop"),
                        Some(db),
                        conversation_id,
                    )
                    .await?;
                    return Ok(());
                }
                _ => {
                    if started_at.elapsed()
                        > tokio::time::Duration::from_secs(public_stream_budget_secs)
                    {
                        let follow_payload = serde_json::json!({
                            "missiond_follow_task_id": task_id,
                            "stream": true
                        });
                        let diagnostic = serde_json::json!({
                            "phase": "result_pending",
                            "task_id": task_id,
                            "status": task.status.as_str(),
                            "slot_id": task.claim_executor_id.clone().or_else(|| task.assignee.clone()),
                            "public_stream_budget_secs": public_stream_budget_secs,
                            "terminal_task_result": false,
                            "follow_payload": follow_payload.clone(),
                            "message": "Worker task is still running; return a resumable pending result before the public SSE route times out."
                        });
                        Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
                        let pending_event = serde_json::json!({
                            "phase": "result_pending",
                            "task_id": task_id,
                            "status": "result_pending",
                            "terminal_task_result": false,
                            "public_stream_budget_secs": public_stream_budget_secs,
                            "follow_payload": follow_payload
                        });
                        Self::write_sse_event(stream, "result_pending", &pending_event).await?;
                        let pending_text = format!(
                            "任务仍在运行，我已返回可续接状态而不是伪造结果。后续请求携带 missiond_follow_task_id={} 即可继续等待或读取最终 task-result-artifact。",
                            task_id
                        );
                        Self::write_sse_openai_text_and_persist(
                            stream,
                            chat_id,
                            &pending_text,
                            Some("stop"),
                            Some(db),
                            conversation_id,
                        )
                        .await?;
                        return Ok(());
                    }
                    if last_visible_heartbeat.elapsed() >= visible_heartbeat {
                        let elapsed_secs = started_at.elapsed().as_secs();
                        let next_visible_heartbeat_secs = visible_heartbeat.as_secs();
                        let heartbeat = serde_json::json!({
                            "phase": "workers_running",
                            "task_id": task_id,
                            "status": task.status.as_str(),
                            "slot_id": task.claim_executor_id.clone().or_else(|| task.assignee.clone()),
                            "heartbeat": true,
                            "elapsed_secs": elapsed_secs,
                            "next_visible_heartbeat_secs": next_visible_heartbeat_secs,
                            "terminal_task_result": false,
                            "message": format!(
                                "Worker task is still running after {elapsed_secs}s; MissionD is still observing durable evidence and will return result_pending before the public stream budget."
                            )
                        });
                        Self::write_sse_event(stream, "worker_status", &heartbeat).await?;
                        last_visible_heartbeat = std::time::Instant::now();
                    }
                    stream.write_all(b":\n\n").await?;
                    stream.flush().await?;
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            }
        }
    }

    fn classify_jarvis_dispatch_verb(text: &str) -> (&'static str, &'static str) {
        let lower = text.to_ascii_lowercase();
        const READ_ONLY_MARKERS: &[&str] = &[
            "read-only",
            "readonly",
            "no file edits",
            "no file changes",
            "do not modify",
            "do not edit",
            "不要修改",
            "不要改文件",
            "不修改文件",
            "不改文件",
            "不要写文件",
            "不写文件",
            "只读",
        ];
        if READ_ONLY_MARKERS
            .iter()
            .any(|marker| lower.contains(marker))
        {
            return ("review", "read-only");
        }
        const INVESTIGATION_FIRST_MARKERS: &[&str] = &[
            "investigate",
            "survey",
            "review",
            "research",
            "design",
            "plan",
            "调查",
            "审视",
            "研究",
            "分析",
            "设计",
            "规划",
            "方案",
        ];
        const CODE_NOW_MARKERS: &[&str] = &[
            "implement now",
            "fix now",
            "do it now",
            "exact shard",
            "accepted_shard",
            "accepted shard",
            "立刻实现",
            "立即实现",
            "现在实现",
            "直接实现",
            "立刻修复",
            "立即修复",
            "现在修复",
            "直接修复",
            "代码补丁",
            "改代码",
        ];
        if INVESTIGATION_FIRST_MARKERS
            .iter()
            .any(|marker| lower.contains(marker))
            && !CODE_NOW_MARKERS.iter().any(|marker| lower.contains(marker))
        {
            return ("review", "read-only");
        }
        let action_text = lower
            .replace("do not commit", "")
            .replace("don't commit", "")
            .replace("no commit", "")
            .replace("不要提交", "")
            .replace("不提交", "")
            .replace("不要推送", "")
            .replace("不推送", "");
        const IMPL_VERBS: &[&str] = &[
            "implement",
            "fix",
            "create",
            "build",
            "add",
            "refactor",
            "write code",
            "develop",
            "migrate",
            "补齐",
            "接入",
            "实现",
            "修复",
            "重构",
            "提交",
            "推送",
        ];
        for verb in IMPL_VERBS {
            if action_text.contains(verb) {
                return ("code", "scoped");
            }
        }
        ("review", "read-only")
    }

    fn derive_jarvis_dispatch_contract(
        raw_user_text: &str,
        grounding_context_id: &str,
        context_pack_path: Option<&str>,
        context_pack_file: Option<&str>,
        grounding_report_file: Option<&str>,
        grounding_report_artifact_path: Option<&str>,
        grounding_report_hash: Option<&str>,
        intent_artifact_id: &str,
        plan_artifact_id: &str,
        key_judgment: &JarvisKeyJudgmentArtifactRef,
        plan_atomization_graph: &serde_json::Value,
        read_scope_root: &str,
    ) -> serde_json::Value {
        let lower = raw_user_text.to_ascii_lowercase();
        let mentions_agy = lower.contains("agy")
            || raw_user_text.contains("反重力")
            || lower.contains("antigravity");
        let mentions_codex = lower.contains("codex");

        let (verb_class, verb_write_policy) = Self::classify_jarvis_dispatch_verb(raw_user_text);

        let (task_class, engine_hint, pool_hint, task_kind, write_policy, write_scope) =
            if mentions_agy {
                (
                    "research",
                    "agy",
                    "agy-research",
                    "agy-cli-readonly-validation",
                    "read-only",
                    serde_json::json!([]),
                )
            } else if verb_class == "code" && mentions_codex {
                // Codex implementation objectives project engine_hint=codex and pool_hint=codex-code-worker.
                (
                    "code",
                    "codex",
                    "codex-code-worker",
                    "codex-grounded-implementation",
                    verb_write_policy,
                    serde_json::json!([read_scope_root]),
                )
            } else if verb_class == "code" {
                (
                    "code",
                    "claude_code",
                    "claude-code-default",
                    "jarvis-grounded-implementation",
                    verb_write_policy,
                    serde_json::json!([read_scope_root]),
                )
            } else if mentions_codex {
                (
                    "review",
                    "codex",
                    "codex-review-worker",
                    "codex-worker-readonly-review",
                    "read-only",
                    serde_json::json!([]),
                )
            } else {
                (
                    "review",
                    "codex",
                    "codex-review-worker",
                    "jarvis-grounded-review",
                    "read-only",
                    serde_json::json!([]),
                )
            };

        let must_not_touch = if write_policy == "read-only" {
            serde_json::json!([
                "Do not modify files",
                "Do not stage",
                "Do not commit",
                "Do not spawn sub-workers from inside the worker"
            ])
        } else {
            serde_json::json!(["Do not spawn sub-workers from inside the worker"])
        };
        let read_scope = Self::jarvis_dispatch_read_scope(
            read_scope_root,
            context_pack_file,
            grounding_report_file,
        );

        serde_json::json!({
            "schema": "missiond.jarvis-dispatch-metadata.v1",
            "task_class": task_class,
            "task_kind": task_kind,
            "engine_hint": engine_hint,
            "pool_hint": pool_hint,
            "write_policy": write_policy,
            "read_scope": read_scope,
            "write_scope": write_scope,
            "must_not_touch": must_not_touch,
            "completion_materialization_policy": if write_policy == "read-only" {
                serde_json::json!("autopilot_readonly_ok")
            } else {
                serde_json::json!("worker_artifact_required")
            },
            "acceptance": [
                "Return a structured artifact with Findings / Evidence / Recommendations / Verification",
                "Use the grounding context and cited evidence instead of rediscovering broad context",
                "If the requested provider/tool is unavailable, fail fast with a diagnostic"
            ],
            "output_contract": "Findings / Evidence / Recommendations / Verification",
            "grounding_context_id": grounding_context_id,
            "context_pack_path": context_pack_path,
            "context_pack_file": context_pack_file,
            "grounding_report_file": grounding_report_file,
            "grounding_report_artifact_path": grounding_report_artifact_path,
            "grounding_report_hash": grounding_report_hash,
            "intent_artifact_id": intent_artifact_id,
            "plan_artifact_id": plan_artifact_id,
            "key_judgment_artifact_id": key_judgment.artifact_id,
            "key_judgment_artifact_hash": key_judgment.artifact_hash,
            "key_judgment": key_judgment.judgment,
            "key_judgment_evidence_refs": key_judgment.evidence_refs,
            "planning_implications": key_judgment.planning_implications,
            "acceptance_focus": key_judgment.acceptance_focus,
            "plan_atomization_graph": plan_atomization_graph,
            "assignment_policy": plan_atomization_graph
                .get("assignment_policy")
                .cloned()
                .unwrap_or_else(Self::jarvis_assignment_policy_default),
            "worker_may_delegate": false
        })
    }

    fn jarvis_dispatch_read_scope(
        read_scope_root: &str,
        context_pack_file: Option<&str>,
        grounding_report_file: Option<&str>,
    ) -> Vec<String> {
        let root = read_scope_root.trim();
        let mut scopes = Vec::new();
        if !root.is_empty() {
            scopes.push(root.to_string());
        }
        for file in [context_pack_file, grounding_report_file]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with("shared-artifact://"))
        {
            let path = Path::new(file);
            let scope = path.parent().unwrap_or(path);
            let scope_display = scope.display().to_string();
            if !scope_display.is_empty()
                && !Self::path_is_within_scope(scope, root)
                && !scopes.iter().any(|existing| existing == &scope_display)
            {
                scopes.push(scope_display);
            }
        }
        scopes
    }

    fn path_is_within_scope(path: &Path, scope: &str) -> bool {
        let scope = scope.trim();
        if scope.is_empty() {
            return false;
        }
        let scope_path = Path::new(scope);
        path == scope_path || path.starts_with(scope_path)
    }

    fn jarvis_runtime_read_scope_root() -> String {
        for key in [
            "MISSIOND_PROJECT_ROOT",
            "MISSIOND_REPO_ROOT",
            "MISSIOND_WORKSPACE_ROOT",
        ] {
            if let Ok(value) = std::env::var(key) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
        std::env::current_dir()
            .ok()
            .map(|path| path.display().to_string())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| ".".to_string())
    }

    fn jarvis_compact_board_title(prefix: &str, text: &str, max_chars: usize) -> String {
        let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let base = if compact.is_empty() {
            "Jarvis atom task".to_string()
        } else {
            compact
        };
        let remaining = max_chars.saturating_sub(prefix.chars().count());
        let title = if base.chars().count() > remaining {
            format!(
                "{}...",
                base.chars()
                    .take(remaining.saturating_sub(3))
                    .collect::<String>()
            )
        } else {
            base
        };
        format!("{prefix}{title}")
    }

    fn jarvis_dispatch_string_array(
        dispatch_metadata: &serde_json::Value,
        key: &str,
    ) -> Vec<String> {
        dispatch_metadata
            .get(key)
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn jarvis_atom_dispatch_defaults(
        atom: &JarvisPlanAtomTask,
    ) -> (String, String, String, String, String) {
        let values = match atom.category.as_str() {
            "query" => (
                "review",
                "jarvis-atom-query",
                "claude_code",
                "claude-code-default",
                "read-only",
            ),
            "code_change" => (
                "code",
                "jarvis-atom-code-change",
                "claude_code",
                "claude-code-default",
                "scoped",
            ),
            "deploy_ops" => (
                "deploy-ops",
                "jarvis-atom-deploy-ops",
                "claude_code",
                "claude-code-deploy-ops",
                "read-only",
            ),
            "judgment" => (
                "review",
                "jarvis-atom-judgment",
                "codex",
                "codex-review-worker",
                "read-only",
            ),
            "acceptance" => (
                "review",
                "jarvis-atom-acceptance",
                "codex",
                "codex-review-worker",
                "read-only",
            ),
            _ => (
                "review",
                "jarvis-atom-review",
                atom.assignee_engine.as_str(),
                "codex-review-worker",
                "read-only",
            ),
        };
        (
            values.0.to_string(),
            values.1.to_string(),
            values.2.to_string(),
            values.3.to_string(),
            values.4.to_string(),
        )
    }

    fn jarvis_plan_atom_scope_json(values: &[String], fallback: Vec<String>) -> serde_json::Value {
        if values.is_empty() {
            serde_json::json!(fallback)
        } else {
            serde_json::json!(values)
        }
    }

    fn jarvis_atom_dispatch_metadata(
        atom: &JarvisPlanAtomTask,
        base_dispatch_metadata: &serde_json::Value,
        plan_atomization_graph: &serde_json::Value,
        parent_task_id: &str,
        depends_on_task_ids: &[String],
        synthetic: bool,
    ) -> serde_json::Value {
        let (task_class, task_kind, engine_hint, pool_hint, write_policy) =
            Self::jarvis_atom_dispatch_defaults(atom);
        let base_read_scope =
            Self::jarvis_dispatch_string_array(base_dispatch_metadata, "read_scope");
        let base_write_scope =
            Self::jarvis_dispatch_string_array(base_dispatch_metadata, "write_scope");
        let write_scope = if write_policy == "read-only" {
            serde_json::json!([])
        } else {
            Self::jarvis_plan_atom_scope_json(&atom.write_scope, base_write_scope)
        };
        let read_scope = Self::jarvis_plan_atom_scope_json(&atom.read_scope, base_read_scope);
        let must_not_touch = if write_policy == "read-only" {
            serde_json::json!([
                "Do not modify files",
                "Do not stage",
                "Do not commit",
                "Do not spawn sub-workers from inside the worker"
            ])
        } else {
            serde_json::json!(["Do not spawn sub-workers from inside the worker"])
        };
        let mut dispatch = if base_dispatch_metadata.is_object() {
            base_dispatch_metadata.clone()
        } else {
            serde_json::json!({})
        };
        if let Some(fields) = dispatch.as_object_mut() {
            fields.insert("task_class".to_string(), serde_json::json!(task_class));
            fields.insert("task_kind".to_string(), serde_json::json!(task_kind));
            fields.insert("engine_hint".to_string(), serde_json::json!(engine_hint));
            fields.insert("pool_hint".to_string(), serde_json::json!(pool_hint));
            fields.insert("write_policy".to_string(), serde_json::json!(write_policy));
            fields.insert("read_scope".to_string(), read_scope);
            fields.insert("write_scope".to_string(), write_scope);
            fields.insert("must_not_touch".to_string(), must_not_touch);
            fields.insert(
                "parent_board_task_id".to_string(),
                serde_json::json!(parent_task_id),
            );
            fields.insert(
                "atom_task_id".to_string(),
                serde_json::json!(atom.atom_task_id),
            );
            fields.insert(
                "workstream_id".to_string(),
                serde_json::json!(atom.workstream_id),
            );
            fields.insert(
                "atom_category".to_string(),
                serde_json::json!(atom.category),
            );
            fields.insert(
                "assignee_engine".to_string(),
                serde_json::json!(atom.assignee_engine),
            );
            fields.insert(
                "execution_order".to_string(),
                serde_json::json!(atom.execution_order),
            );
            fields.insert(
                "depends_on_atoms".to_string(),
                serde_json::json!(atom.depends_on),
            );
            fields.insert(
                "depends_on_task_ids".to_string(),
                serde_json::json!(depends_on_task_ids),
            );
            fields.insert(
                "parallel_group".to_string(),
                serde_json::json!(atom.parallel_group),
            );
            fields.insert(
                "accepted_atom_objective".to_string(),
                serde_json::json!(atom.objective),
            );
            fields.insert("accepted_plan_atom".to_string(), atom.raw.clone());
            fields.insert(
                "synthetic_acceptance".to_string(),
                serde_json::json!(synthetic),
            );
            fields.insert(
                "plan_atomization_graph".to_string(),
                plan_atomization_graph.clone(),
            );
            fields.insert("worker_may_delegate".to_string(), serde_json::json!(false));
            fields.insert(
                "completion_materialization_policy".to_string(),
                serde_json::json!("worker_artifact_required"),
            );
            if !atom.acceptance.is_empty() {
                fields.insert("acceptance".to_string(), serde_json::json!(atom.acceptance));
            }
        }
        dispatch
    }

    fn jarvis_atom_runtime_metadata(
        parent_runtime_metadata: Option<&serde_json::Value>,
        parent_task_id: &str,
        atom: &JarvisPlanAtomTask,
        dispatch_metadata: serde_json::Value,
        plan_atomization_graph: &serde_json::Value,
        depends_on_task_ids: &[String],
        synthetic: bool,
    ) -> serde_json::Value {
        let mut metadata = parent_runtime_metadata
            .cloned()
            .filter(|value| value.is_object())
            .unwrap_or_else(|| serde_json::json!({}));
        let read_scope = dispatch_metadata
            .get("read_scope")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let write_scope = dispatch_metadata
            .get("write_scope")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let must_not_touch = dispatch_metadata
            .get("must_not_touch")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        if let Some(fields) = metadata.as_object_mut() {
            fields.insert(
                "source".to_string(),
                serde_json::json!("jarvis-atomized-dispatch"),
            );
            fields.insert("dispatch_mode".to_string(), serde_json::json!("atom_task"));
            fields.insert(
                "parent_board_task_id".to_string(),
                serde_json::json!(parent_task_id),
            );
            fields.insert(
                "atom_task_id".to_string(),
                serde_json::json!(atom.atom_task_id),
            );
            fields.insert(
                "workstream_id".to_string(),
                serde_json::json!(atom.workstream_id),
            );
            fields.insert(
                "atom_category".to_string(),
                serde_json::json!(atom.category),
            );
            fields.insert(
                "assignee_engine".to_string(),
                serde_json::json!(atom.assignee_engine),
            );
            fields.insert(
                "execution_order".to_string(),
                serde_json::json!(atom.execution_order),
            );
            fields.insert(
                "depends_on_atoms".to_string(),
                serde_json::json!(atom.depends_on),
            );
            fields.insert(
                "depends_on_task_ids".to_string(),
                serde_json::json!(depends_on_task_ids),
            );
            fields.insert(
                "parallel_group".to_string(),
                serde_json::json!(atom.parallel_group),
            );
            fields.insert("accepted_plan_atom".to_string(), atom.raw.clone());
            fields.insert(
                "synthetic_acceptance".to_string(),
                serde_json::json!(synthetic),
            );
            fields.insert("read_scope".to_string(), read_scope);
            fields.insert("write_scope".to_string(), write_scope);
            fields.insert("must_not_touch".to_string(), must_not_touch);
            fields.insert("dispatch_metadata".to_string(), dispatch_metadata);
            fields.insert(
                "plan_atomization_graph".to_string(),
                plan_atomization_graph.clone(),
            );
        }
        metadata
    }

    async fn create_jarvis_atom_board_task(
        db: &Arc<dyn crate::db::traits::MissionStore>,
        parent_task: &crate::types::BoardTask,
        parent_input: &crate::types::CreateBoardTaskInput,
        raw_user_text: &str,
        base_dispatch_metadata: &serde_json::Value,
        plan_atomization_graph: &serde_json::Value,
        atom: &JarvisPlanAtomTask,
        depends_on_task_ids: Vec<String>,
        synthetic: bool,
    ) -> anyhow::Result<JarvisCreatedAtomBoardTask> {
        let dispatch_metadata = Self::jarvis_atom_dispatch_metadata(
            atom,
            base_dispatch_metadata,
            plan_atomization_graph,
            parent_task.id.as_str(),
            &depends_on_task_ids,
            synthetic,
        );
        let prompt_template = Self::build_jarvis_worker_prompt(raw_user_text, &dispatch_metadata);
        let runtime_metadata = Self::jarvis_atom_runtime_metadata(
            parent_input.runtime_metadata.as_ref(),
            parent_task.id.as_str(),
            atom,
            dispatch_metadata,
            plan_atomization_graph,
            &depends_on_task_ids,
            synthetic,
        );
        let (task_class, _, _, _, _) = Self::jarvis_atom_dispatch_defaults(atom);
        let title_prefix = if synthetic {
            "[Jarvis 验收] "
        } else {
            match atom.category.as_str() {
                "query" => "[Jarvis 查询] ",
                "code_change" => "[Jarvis 代码] ",
                "deploy_ops" => "[Jarvis 部署] ",
                "judgment" => "[Jarvis 判断] ",
                "acceptance" => "[Jarvis 验收] ",
                _ => "[Jarvis Atom] ",
            }
        };
        let input = crate::types::CreateBoardTaskInput {
            title: Self::jarvis_compact_board_title(title_prefix, &atom.objective, 120),
            description: Some(format!(
                "Jarvis atom task {} under parent {}. Category={}, assignee_engine={}, synthetic_acceptance={}.",
                atom.atom_task_id, parent_task.id, atom.category, atom.assignee_engine, synthetic
            )),
            priority: parent_input.priority.clone(),
            category: Some("jarvis_atom".to_string()),
            project: parent_input.project.clone(),
            server: parent_input.server.clone(),
            due_date: parent_input.due_date.clone(),
            parent_id: Some(parent_task.id.to_string()),
            assignee: None,
            auto_execute: Some(true),
            prompt_template: Some(prompt_template),
            hidden: Some(false),
            flow_template: parent_input.flow_template.clone(),
            depends_on: if depends_on_task_ids.is_empty() {
                None
            } else {
                Some(depends_on_task_ids)
            },
            dedupe_key: None,
            timeout_secs: parent_input.timeout_secs,
            context_intent: Some(task_class.to_string()),
            runtime_metadata: Some(runtime_metadata),
        };
        let task = db.create_board_task(&input).await.map_err(|error| {
            anyhow::anyhow!(
                "create Jarvis atom task {} failed: {}",
                atom.atom_task_id,
                error
            )
        })?;
        Ok(JarvisCreatedAtomBoardTask {
            atom_task_id: atom.atom_task_id.clone(),
            task,
            category: atom.category.clone(),
            assignee_engine: atom.assignee_engine.clone(),
            depends_on_atoms: atom.depends_on.clone(),
            parallel_group: atom.parallel_group.clone(),
            synthetic,
        })
    }

    fn build_jarvis_parent_board_task_prompt(
        raw_user_text: &str,
        dispatch_metadata: &serde_json::Value,
    ) -> String {
        let plan_atomization_graph = dispatch_metadata
            .get("plan_atomization_graph")
            .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string()))
            .unwrap_or_else(|| "{}".to_string());
        format!(
            "Jarvis parent BoardTask for an atomized plan.\n\n\
             用户目标：\n{raw_user_text}\n\n\
             This parent task is a tracking container. Do not implement, deploy, or create more BoardTasks from this task.\n\
             MissionD has already expanded the confirmed plan into atom-level child BoardTasks with explicit dependencies and worker assignment.\n\n\
             Plan atomization graph:\n{plan_atomization_graph}\n"
        )
    }

    async fn create_jarvis_atomized_board_tasks(
        db: &Arc<dyn crate::db::traits::MissionStore>,
        mut parent_input: crate::types::CreateBoardTaskInput,
        raw_user_text: &str,
        base_dispatch_metadata: &serde_json::Value,
        plan_atomization_graph: &serde_json::Value,
    ) -> anyhow::Result<JarvisCreatedBoardTasks> {
        let specs = Self::jarvis_plan_atom_specs(plan_atomization_graph)?;
        if specs.is_empty() {
            anyhow::bail!("confirmed plan has no atom_tasks for BoardTask fanout");
        }
        let atom_ids = specs
            .iter()
            .map(|spec| spec.atom_task_id.clone())
            .collect::<Vec<_>>();
        if let Some(metadata) = parent_input.runtime_metadata.as_mut() {
            if let Some(fields) = metadata.as_object_mut() {
                fields.insert(
                    "dispatch_mode".to_string(),
                    serde_json::json!("atomized_parent"),
                );
                fields.insert(
                    "plan_atom_task_ids".to_string(),
                    serde_json::json!(atom_ids),
                );
                fields.insert(
                    "plan_atom_task_count".to_string(),
                    serde_json::json!(specs.len()),
                );
            }
        }
        parent_input.auto_execute = Some(false);
        parent_input.context_intent = Some("jarvis-plan".to_string());
        parent_input.prompt_template = Some(Self::build_jarvis_parent_board_task_prompt(
            raw_user_text,
            base_dispatch_metadata,
        ));
        let parent_task = db
            .create_board_task(&parent_input)
            .await
            .map_err(|error| anyhow::anyhow!("create Jarvis parent BoardTask failed: {}", error))?;

        let mut pending = specs;
        let mut created_by_atom_id: HashMap<String, crate::types::BoardTask> = HashMap::new();
        let mut created_atoms = Vec::new();
        while !pending.is_empty() {
            let mut progressed = false;
            let mut idx = 0;
            while idx < pending.len() {
                let ready = pending[idx]
                    .depends_on
                    .iter()
                    .all(|dep| created_by_atom_id.contains_key(dep));
                if !ready {
                    idx += 1;
                    continue;
                }
                let atom = pending.remove(idx);
                let depends_on_task_ids = atom
                    .depends_on
                    .iter()
                    .filter_map(|dep| created_by_atom_id.get(dep))
                    .map(|task| task.id.to_string())
                    .collect::<Vec<_>>();
                let created = Self::create_jarvis_atom_board_task(
                    db,
                    &parent_task,
                    &parent_input,
                    raw_user_text,
                    base_dispatch_metadata,
                    plan_atomization_graph,
                    &atom,
                    depends_on_task_ids,
                    false,
                )
                .await?;
                created_by_atom_id.insert(atom.atom_task_id.clone(), created.task.clone());
                created_atoms.push(created);
                progressed = true;
            }
            if !progressed {
                let blocked = pending
                    .iter()
                    .map(|atom| format!("{}<-{:?}", atom.atom_task_id, atom.depends_on))
                    .collect::<Vec<_>>()
                    .join(", ");
                anyhow::bail!("plan atom dependency graph is cyclic or unresolved: {blocked}");
            }
        }

        let depended_on = created_atoms
            .iter()
            .flat_map(|atom| atom.depends_on_atoms.iter().cloned())
            .collect::<HashSet<_>>();
        let leaves = created_atoms
            .iter()
            .filter(|atom| !depended_on.contains(&atom.atom_task_id))
            .collect::<Vec<_>>();
        let mut final_task_id = if leaves.len() == 1 && leaves[0].category == "acceptance" {
            leaves[0].task.id.to_string()
        } else {
            String::new()
        };
        if final_task_id.is_empty() {
            let leaf_atom_ids = leaves
                .iter()
                .map(|atom| atom.atom_task_id.clone())
                .collect::<Vec<_>>();
            let leaf_task_ids = leaves
                .iter()
                .map(|atom| atom.task.id.to_string())
                .collect::<Vec<_>>();
            let mut synthetic_id = "jarvis-acceptance-aggregate".to_string();
            if created_by_atom_id.contains_key(&synthetic_id) {
                synthetic_id = format!("{}-{}", synthetic_id, created_atoms.len() + 1);
            }
            let synthetic_atom = JarvisPlanAtomTask {
                atom_task_id: synthetic_id,
                workstream_id: Some("jarvis-acceptance".to_string()),
                objective: "汇总并验收所有 Jarvis atom task 结果，输出最终 Findings / Evidence / Recommendations / Verification。".to_string(),
                category: "acceptance".to_string(),
                assignee_engine: "codex".to_string(),
                execution_order: "serial".to_string(),
                depends_on: leaf_atom_ids,
                parallel_group: None,
                read_scope: Vec::new(),
                write_scope: Vec::new(),
                acceptance: vec![
                    "核对每个 atom task-result-artifact 是否满足 plan acceptance".to_string(),
                    "给出最终验收判断和剩余风险".to_string(),
                    "不得补做 ClaudeCode 查询、代码修改或部署工作".to_string(),
                ],
                raw: serde_json::json!({
                    "synthetic": true,
                    "category": "acceptance",
                    "assignee_engine": "codex"
                }),
            };
            let created = Self::create_jarvis_atom_board_task(
                db,
                &parent_task,
                &parent_input,
                raw_user_text,
                base_dispatch_metadata,
                plan_atomization_graph,
                &synthetic_atom,
                leaf_task_ids,
                true,
            )
            .await?;
            final_task_id = created.task.id.to_string();
            created_atoms.push(created);
        }

        Ok(JarvisCreatedBoardTasks {
            parent_task,
            atom_tasks: created_atoms,
            final_task_id,
        })
    }

    fn build_jarvis_worker_prompt(
        raw_user_text: &str,
        dispatch_metadata: &serde_json::Value,
    ) -> String {
        let grounding_context_id = dispatch_metadata
            .get("grounding_context_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let context_pack_path = dispatch_metadata
            .get("context_pack_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let context_pack_file = dispatch_metadata
            .get("context_pack_file")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let grounding_report_file = dispatch_metadata
            .get("grounding_report_file")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let grounding_report_hash = dispatch_metadata
            .get("grounding_report_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let task_kind = dispatch_metadata
            .get("task_kind")
            .and_then(|v| v.as_str())
            .unwrap_or("jarvis-grounded-review");
        let task_class = dispatch_metadata
            .get("task_class")
            .and_then(|v| v.as_str())
            .unwrap_or("review");
        let engine_hint = dispatch_metadata
            .get("engine_hint")
            .and_then(|v| v.as_str())
            .unwrap_or("codex");
        let pool_hint = dispatch_metadata
            .get("pool_hint")
            .and_then(|v| v.as_str())
            .unwrap_or("codex-review-worker");
        let write_policy = dispatch_metadata
            .get("write_policy")
            .and_then(|v| v.as_str())
            .unwrap_or("read-only");
        let intent_artifact_id = dispatch_metadata
            .get("intent_artifact_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let plan_artifact_id = dispatch_metadata
            .get("plan_artifact_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let key_judgment_artifact_id = dispatch_metadata
            .get("key_judgment_artifact_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let key_judgment = dispatch_metadata
            .get("key_judgment")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let atom_task_id = dispatch_metadata
            .get("atom_task_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let accepted_atom_objective = dispatch_metadata
            .get("accepted_atom_objective")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let accepted_plan_atom = dispatch_metadata
            .get("accepted_plan_atom")
            .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string()))
            .unwrap_or_else(|| "{}".to_string());
        let plan_atomization_graph = dispatch_metadata
            .get("plan_atomization_graph")
            .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string()))
            .unwrap_or_else(|| "{}".to_string());
        let assignment_policy = dispatch_metadata
            .get("assignment_policy")
            .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string()))
            .unwrap_or_else(|| "{}".to_string());
        let media_context = dispatch_metadata
            .get("media_context")
            .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string()))
            .unwrap_or_else(|| {
                serde_json::to_string_pretty(&interaction_media_context(&[]))
                    .unwrap_or_else(|_| "{}".to_string())
            });
        let read_scope = dispatch_metadata
            .get("read_scope")
            .and_then(|v| v.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let write_scope = dispatch_metadata
            .get("write_scope")
            .and_then(|v| v.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let acceptance = dispatch_metadata
            .get("acceptance")
            .and_then(|v| v.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str())
                    .map(|value| format!("- {}", value))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let context_pack_hash = context_pack_path
            .strip_prefix("shared-artifact://")
            .unwrap_or(context_pack_path);
        let task_mode_label = if write_policy == "read-only" {
            "只读工位任务"
        } else {
            "工位实现任务"
        };
        let write_constraint = if write_policy == "read-only" {
            "- 不要修改文件、不要 stage、不要 commit、不要在工位内部创建子任务或再派其他工位。\n"
        } else {
            "- 只在 write_scope 范围内修改文件；不要在工位内部创建子任务或再派其他工位。\n"
        };
        format!(
            "请基于已确认的 Jarvis intent.lisp / plan.lisp 执行一个{task_mode}。\n\n\
             用户目标：\n{raw_text}\n\n\
             任务类型：{kind}\n\
             任务类别：{class}\n\
             目标工位：engine_hint={engine} pool_hint={pool}\n\
             write_policy: {wp}\n\
             read_scope: [{rs}]\n\
             write_scope: [{ws}]\n\
             grounding_context_id: {gci}\n\
             context_pack_path: {cpp}\n\
             context_pack_file: {cpf}\n\
             grounding_report_file: {grf}\n\
             grounding_report_hash: {grh}\n\
             intent_artifact_id: {iai}\n\
             plan_artifact_id: {pai}\n\
             key_judgment_artifact_id: {kjai}\n\
             key_judgment: {kj}\n\
             atom_task_id: {ati}\n\
             accepted_atom_objective: {aao}\n\n\
             Accepted plan atom:\n{apa}\n\n\
             Plan atomization graph:\n{pag}\n\n\
	             Fixed assignment policy:\n{ap}\n\n\
	             Media attachments:\n{mc}\n\n\
	             已接受执行切片：\n\
             - 这个任务已经通过 Jarvis intent 确认和 plan 确认。\n\
             - 关键判断是 plan 的前提；不要推翻它，除非你在证据中明确报告冲突并停止执行。\n\
             - plan atomization graph 已经标记串行/并行和工位分配；你只执行属于当前 BoardTask/worker 的切片，不要重新一拆十或创建隐藏子任务。\n\
             - query/code_change/deploy_ops 归 ClaudeCode 工位，judgment/acceptance 归 Codex CLI GPT-5.5 xhigh 工位；不要自行改派。\n\
             - 你不是主控；不要重新拆任务、不要创建 BoardTask、不要派子工位。\n\
             - 禁止调用 ClaudeCode 内部 Task/Explore/TaskCreate/TaskUpdate/TaskList/TaskOutput 类子代理工具；如果这些工具出现在可用工具列表中，也不要使用。\n\
             - 你只需要按 task_kind 和 acceptance 验证当前工位能力，并返回结构化结果。\n\
             - acceptance:\n{acc}\n\n\
             工作方式：\n\
             - 这是已经过 Jarvis 意图确认和计划确认的 grounded dispatch，不要重新扮演主控。\n\
             - 先读取 grounding_report_file；这是 ClaudeCode + MissionD MCP grounding worker 写入的上下游全链 Markdown 报告。\n\
             - 再读取 context_pack_file；这是 MissionD 为没有 MCP 的工位物化的 bounded context slice。\n\
             - 如果文件不可读，且 context_pack_path 是 shared-artifact://，再用 MissionD MCP 调 mission_shared_memory(action=\"artifact_get\", hash=\"{cph}\") 或 mission_context_slice 读取上下文切片。\n\
             - 如果 grounding report、context 文件和 MCP 都不可用，不要自行大范围搜索代码；请快速失败并输出 Diagnostic / Evidence / Verification，说明 context unavailable。\n\
             {wc}\
             - 输出必须是结构化 artifact，严格包含以下四个 Markdown 二级标题：\n\
               ## Findings\n\
               ## Evidence\n\
               ## Recommendations\n\
               ## Verification\n\
             - 不要把 Board note 当作最终结果；MissionD 会在 durable final 后写 task-result-artifact 并关闭任务。",
            task_mode = task_mode_label,
            raw_text = raw_user_text,
            kind = task_kind,
            class = task_class,
            engine = engine_hint,
            pool = pool_hint,
            wp = write_policy,
            rs = read_scope,
            ws = write_scope,
            gci = grounding_context_id,
            cpp = context_pack_path,
            cpf = context_pack_file,
            grf = grounding_report_file,
            grh = grounding_report_hash,
            iai = intent_artifact_id,
            pai = plan_artifact_id,
            kjai = key_judgment_artifact_id,
            kj = key_judgment,
            ati = atom_task_id,
            aao = accepted_atom_objective,
            apa = accepted_plan_atom,
            pag = plan_atomization_graph,
            ap = assignment_policy,
            mc = media_context,
            acc = acceptance,
            cph = context_pack_hash,
            wc = write_constraint,
        )
    }

    /// Handle POST /v1/chat/completions — OpenAI-compatible SSE endpoint
    #[allow(dead_code)]
    async fn handle_chat_completions(
        mut stream: TcpStream,
        addr: SocketAddr,
        pty_manager: Arc<PTYManager>,
        trace_store: JarvisTraceStore,
        context_enricher: ContextEnricherSlot,
        jarvis_grounding: JarvisGroundingSlot,
        jarvis_artifact_writer: JarvisArtifactSlot,
        provider_box_http: ProviderBoxHttpSlot,
        db: Option<Arc<dyn crate::db::traits::MissionStore>>,
        cc_tasks_watcher: Option<Arc<Mutex<CCTasksWatcher>>>,
        tool_count: usize,
        default_chat_slot: String,
    ) -> anyhow::Result<()> {
        // Disable Nagle — SSE needs every chunk sent immediately
        stream.set_nodelay(true)?;
        let jarvis_progress_bus = JarvisProgressBus {
            system_event_tx: None,
            frontend_events_tx: None,
        };

        // Read full HTTP request
        let (headers, body) = match Self::read_http_request(&mut stream).await {
            Ok(r) => r,
            Err(e) => {
                let err = serde_json::json!({"error": {"message": format!("Bad request: {}", e)}});
                Self::send_http_error(&mut stream, 400, "Bad Request", &err.to_string()).await?;
                return Ok(());
            }
        };

        // Auth check: extract Bearer token
        let auth_token = headers.lines().find_map(|line| {
            let lower = line.to_lowercase();
            if lower.starts_with("authorization:") {
                let val = line.splitn(2, ':').nth(1)?.trim();
                val.strip_prefix("Bearer ").map(|t| t.to_string())
            } else {
                None
            }
        });

        if let Err((status, reason, err)) =
            validate_missiond_legacy_chat_bearer(auth_token.as_deref())
        {
            Self::send_http_error(&mut stream, status, reason, &err.to_string()).await?;
            return Ok(());
        }

        // Parse request body
        let req: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                let err = serde_json::json!({"error": {"message": format!("Invalid JSON: {}", e)}});
                Self::send_http_error(&mut stream, 400, "Bad Request", &err.to_string()).await?;
                return Ok(());
            }
        };

        // Parse conversation_id for Jarvis UI history persistence
        let conversation_id = req
            .get("conversation_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let request_scope = conversation_scope_from_request(
            &req,
            "jarvis_sse",
            req.get("query")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
        );

        if let Some(follow_task_id) = req
            .get("missiond_follow_task_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let chat_id = format!(
                "chatcmpl-jarvis-follow-{}",
                chrono::Utc::now().timestamp_millis()
            );
            let sse_headers = "HTTP/1.1 200 OK\r\n\
                Content-Type: text/event-stream\r\n\
                Cache-Control: no-cache\r\n\
                Connection: keep-alive\r\n\
                Access-Control-Allow-Origin: *\r\n\
                \r\n";
            stream.write_all(sse_headers.as_bytes()).await?;
            stream.flush().await?;

            if let Some(ref db) = db {
                match db
                    .jarvis_get_or_create_scoped(
                        conversation_id.as_deref(),
                        request_scope.user_id.as_deref(),
                        request_scope.tenant_id.as_deref(),
                        request_scope.application_id.as_deref(),
                        Some(request_scope.channel.as_str()),
                        request_scope.topic_id.as_deref(),
                        request_scope.topic_label.as_deref(),
                    )
                    .await
                {
                    Ok(id) => {
                        let meta_evt = serde_json::json!({
                            "conversation_id": id,
                            "chat_id": chat_id,
                            "follow_task_id": follow_task_id,
                            "mode": "missiond_follow_task"
                        });
                        Self::write_sse_event(&mut stream, "meta", &meta_evt).await?;
                        let status_evt = serde_json::json!({
                            "phase": "result_followup",
                            "task_id": follow_task_id,
                            "message": "Continuing an existing BoardTask result stream; intent/plan is not regenerated."
                        });
                        Self::write_sse_event(&mut stream, "status", &status_evt).await?;
                        Self::stream_jarvis_task_until_terminal(
                            db,
                            &jarvis_artifact_writer,
                            &provider_box_http,
                            &jarvis_progress_bus,
                            &mut stream,
                            &chat_id,
                            None,
                            follow_task_id,
                            Some(id.as_str()),
                        )
                        .await?;
                    }
                    Err(error) => {
                        let diagnostic = serde_json::json!({
                            "phase": "result_followup",
                            "task_id": follow_task_id,
                            "error": {
                                "code": "JARVIS_CONVERSATION_OPEN_FAILED",
                                "message": error.to_string()
                            }
                        });
                        Self::write_sse_event(&mut stream, "diagnostic", &diagnostic).await?;
                    }
                }
            } else {
                let diagnostic = serde_json::json!({
                    "phase": "result_followup",
                    "task_id": follow_task_id,
                    "error": {
                        "code": "MISSIOND_DB_UNAVAILABLE",
                        "message": "MissionD DB unavailable; cannot follow BoardTask result"
                    }
                });
                Self::write_sse_event(&mut stream, "diagnostic", &diagnostic).await?;
            }
            Self::finish_sse(&mut stream).await?;
            return Ok(());
        }

        let messages = req.get("messages").and_then(|m| m.as_array());

        // Extract raw user text for DB persistence (before system prompt wrapping)
        let raw_user_text: String = messages
            .as_ref()
            .and_then(|msgs| {
                msgs.iter()
                    .rev()
                    .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
                    .and_then(|m| match m.get("content") {
                        Some(serde_json::Value::String(s)) => Some(s.clone()),
                        _ => None,
                    })
            })
            .unwrap_or_default();
        if messages.is_none() || messages.unwrap().is_empty() {
            let err = serde_json::json!({"error": {"message": "messages array is required"}});
            Self::send_http_error(&mut stream, 400, "Bad Request", &err.to_string()).await?;
            return Ok(());
        }
        let messages = messages.unwrap();

        // Stateless proxy mode: format all messages into a single prompt
        // (MISSIOND_DISABLE_CONTEXT_ENRICHMENT implies proxy mode)
        let proxy_mode = std::env::var("MISSIOND_DISABLE_CONTEXT_ENRICHMENT")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let user_message = if proxy_mode {
            // Proxy mode: format full messages array as structured prompt
            let mut parts: Vec<String> = Vec::new();
            for msg in messages {
                let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                let content = match msg.get("content") {
                    Some(serde_json::Value::String(text)) => text.clone(),
                    Some(serde_json::Value::Array(arr)) => {
                        Self::extract_multimodal_content(arr).await
                    }
                    _ => continue,
                };
                if content.is_empty() {
                    continue;
                }
                match role {
                    "system" => parts.push(format!(
                        "[System Instructions]\n{}\n[End System Instructions]",
                        content
                    )),
                    "assistant" => parts.push(format!(
                        "[Previous Assistant Response]\n{}\n[End Previous Response]",
                        content
                    )),
                    _ => parts.push(content), // user messages as-is
                }
            }
            parts.join("\n\n")
        } else {
            // Legacy mode: extract last user message + system prefix
            let system_message: Option<String> = messages
                .iter()
                .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
                .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                .reduce(|a, b| {
                    let _ = a;
                    b
                })
                .map(|s| s.to_string());

            let last_user_msg = messages
                .iter()
                .rev()
                .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"));

            let raw_user_message = match last_user_msg {
                Some(msg) => match msg.get("content") {
                    Some(serde_json::Value::String(text)) => text.clone(),
                    Some(serde_json::Value::Array(parts)) => {
                        Self::extract_multimodal_content(parts).await
                    }
                    _ => String::new(),
                },
                None => String::new(),
            };

            match &system_message {
                Some(sys) if !sys.is_empty() => {
                    format!(
                        "[Instructions from caller — follow these for your response]\n{}\n[End instructions]\n\n{}",
                        sys, raw_user_message
                    )
                }
                _ => raw_user_message,
            }
        };

        if user_message.is_empty() {
            let err = serde_json::json!({"error": {"message": "No user message found"}});
            Self::send_http_error(&mut stream, 400, "Bad Request", &err.to_string()).await?;
            return Ok(());
        }
        let auth_envelope = openai_request_to_interaction_envelope(&req);
        let auth_resolution = match resolve_interaction_auth(&auth_envelope, &headers).await {
            Ok(resolution) => resolution,
            Err((status, reason, body)) => {
                Self::send_http_error(&mut stream, status, reason, &body.to_string()).await?;
                return Ok(());
            }
        };
        let permission_context = auth_resolution.permission_context;
        let media_context = interaction_media_context(&auth_envelope.attachments);
        let conversation_scope = conversation_scope_from_permission(
            &auth_envelope,
            &permission_context,
            "jarvis",
            &raw_user_text,
        );

        let exact_shard_ready = req
            .get("exact_shard_ready")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let strict_jarvis_gate = std::env::var("MISSIOND_JARVIS_INTENT_PLAN_GATE")
            .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
            .unwrap_or(true);
        if strict_jarvis_gate && !proxy_mode && !exact_shard_ready {
            let chat_id = format!(
                "chatcmpl-jarvis-gate-{}",
                chrono::Utc::now().timestamp_millis()
            );
            let sse_headers = "HTTP/1.1 200 OK\r\n\
                Content-Type: text/event-stream\r\n\
                Cache-Control: no-cache\r\n\
                Connection: keep-alive\r\n\
                Access-Control-Allow-Origin: *\r\n\
                \r\n";
            stream.write_all(sse_headers.as_bytes()).await?;
            stream.flush().await?;

            let jarvis_conv_id = if let Some(ref db) = db {
                match Self::resolve_jarvis_conversation_id(
                    db,
                    conversation_id.as_deref(),
                    &raw_user_text,
                    &conversation_scope,
                )
                .await
                {
                    Ok(id) => {
                        if !raw_user_text.is_empty() {
                            let _ = db
                                .router_chat_append_messages(
                                    &id,
                                    &[("user".to_string(), raw_user_text.clone())],
                                )
                                .await;
                        }
                        Some(id)
                    }
                    Err(e) => {
                        warn!(error = %e, "Jarvis intent/plan gate cannot persist conversation");
                        None
                    }
                }
            } else {
                None
            };

            if let Some(ref cid) = jarvis_conv_id {
                let meta_evt = serde_json::json!({"conversation_id": cid, "chat_id": chat_id});
                let _ = stream
                    .write_all(format!("event: meta\ndata: {}\n\n", meta_evt).as_bytes())
                    .await;
            }

            let mut effective_req = req.clone();
            if !jarvis_confirm_bool(&effective_req, "missiond_intent_confirmed")
                && !jarvis_confirm_bool(&effective_req, "missiond_plan_confirmed")
                && Self::jarvis_text_confirms_pending_review(&raw_user_text)
            {
                if let (Some(ref db), Some(ref cid)) = (&db, &jarvis_conv_id) {
                    match Self::load_pending_jarvis_confirmation(db, cid).await {
                        Ok(Some(confirm_payload)) => {
                            Self::inject_jarvis_confirm_payload(
                                &mut effective_req,
                                confirm_payload,
                            );
                        }
                        Ok(None) => {}
                        Err(error) => {
                            warn!(conversation_id = %cid, error = %error, "failed to load pending Jarvis confirmation");
                        }
                    }
                }
            }

            let intent_confirmed = jarvis_confirm_bool(&effective_req, "missiond_intent_confirmed");
            let plan_confirmed = jarvis_confirm_bool(&effective_req, "missiond_plan_confirmed");
            let mut objective_text = if intent_confirmed || plan_confirmed {
                match jarvis_confirm_string(&effective_req, "missiond_objective") {
                    Some(value) => value,
                    None => {
                        Self::fail_jarvis_gate(
                            &mut stream,
                            "Jarvis confirmation requires missiond_objective from the previous intent/plan payload; refusing to use the confirmation text as the task objective.".to_string(),
                            "confirmation_objective",
                        )
                        .await?;
                        return Ok(());
                    }
                }
            } else {
                raw_user_text.clone()
            };

            let confirmed_intent_artifact_id = if intent_confirmed {
                match jarvis_confirm_string(&effective_req, "missiond_intent_artifact_id") {
                    Some(value) => Some(value),
                    None => {
                        Self::fail_jarvis_gate(
                            &mut stream,
                            "Jarvis intent confirmation requires missiond_intent_artifact_id from the previous intent payload; refusing to collect grounding without a confirmed intent.lisp.",
                            "confirmation_intent_artifact",
                        )
                        .await?;
                        return Ok(());
                    }
                }
            } else {
                None
            };
            let confirmed_plan_artifact_id = if plan_confirmed {
                match jarvis_confirm_string(&effective_req, "missiond_plan_artifact_id") {
                    Some(value) => Some(value),
                    None => {
                        Self::fail_jarvis_gate(
                            &mut stream,
                            "Jarvis plan confirmation requires missiond_plan_artifact_id from the previous plan payload; refusing to execute without a confirmed plan.lisp.",
                            "confirmation_plan_artifact",
                        )
                        .await?;
                        return Ok(());
                    }
                }
            } else {
                None
            };
            let grounding_was_collected = intent_confirmed && !plan_confirmed;
            let grounding = if plan_confirmed {
                match Self::jarvis_grounding_from_confirm_value(&effective_req, &conversation_scope)
                {
                    Ok(result) => result,
                    Err(error) => {
                        Self::fail_jarvis_gate(&mut stream, error, "confirmation_grounding")
                            .await?;
                        return Ok(());
                    }
                }
            } else if intent_confirmed {
                match Self::gather_jarvis_grounding_with_progress(
                    &mut stream,
                    &jarvis_progress_bus,
                    &chat_id,
                    None,
                    &jarvis_grounding,
                    JarvisGroundingRequest {
                        query: objective_text.clone(),
                        confirmed_intent_artifact_id: confirmed_intent_artifact_id.clone(),
                        confirmed_intent_lisp: jarvis_confirm_string(
                            &effective_req,
                            "missiond_intent_artifact_body",
                        ),
                        conversation_id: jarvis_conv_id.clone(),
                        chat_id: chat_id.clone(),
                        user_id: conversation_scope.user_id.clone(),
                        tenant_id: conversation_scope.tenant_id.clone(),
                        application_id: conversation_scope.application_id.clone(),
                        channel: Some(conversation_scope.channel.clone()),
                        topic_id: conversation_scope.topic_id.clone(),
                        topic_label: conversation_scope.topic_label.clone(),
                        permission_context: permission_context.clone(),
                        media_context: media_context.clone(),
                        unknowns: vec![
                            "Collect MissionD upstream/downstream facts that affect this confirmed intent.".to_string(),
                            "Identify project registry, SSOT, runtime, skill, provider, infra, and permission evidence needed before plan.lisp.".to_string(),
                            "Write the grounded evidence report for the plan author instead of creating BoardTask or implementing changes.".to_string(),
                        ],
                    },
                )
                .await
                {
                    Ok(result) => result,
                    Err(error) => {
                        Self::fail_jarvis_gate(&mut stream, error, "grounding").await?;
                        return Ok(());
                    }
                }
            } else {
                Self::jarvis_pending_grounding_result(&conversation_scope)
            };
            let mut grounding_context_id = grounding.grounding_context_id.clone();
            let mut context_pack_path = grounding.context_pack_path.clone();
            let mut context_pack_file = grounding.context_pack_file.clone();
            let mut grounding_report_file = grounding.grounding_report_file.clone();
            let mut grounding_report_artifact_path =
                grounding.grounding_report_artifact_path.clone();
            let mut grounding_report_hash = grounding.grounding_report_hash.clone();
            let mut grounding_worker_slot_id = grounding.grounding_worker_slot_id.clone();
            let mut grounding_worker_turn_id = grounding.grounding_worker_turn_id.clone();
            let mut context_sufficiency = grounding.context_sufficiency.clone();
            let mut grounding_artifact_hash = grounding.artifact_hash.clone();
            let mut context_capsule_hash = grounding.context_capsule_hash.clone();
            let mut context_capsule_file = grounding.context_capsule_file.clone();
            let mut resolved_topic_id = grounding
                .topic_id
                .clone()
                .or_else(|| conversation_scope.topic_id.clone());
            let mut resolved_topic_label = grounding
                .topic_label
                .clone()
                .or_else(|| conversation_scope.topic_label.clone());
            let mut sources_used = grounding.sources_used.clone();
            let mut grounding_diagnostics = grounding.diagnostics.clone();
            if let (Some(ref db), Some(ref cid), Some(ref capsule_hash)) =
                (&db, &jarvis_conv_id, &context_capsule_hash)
            {
                let _ = db
                    .bind_context_capsule(
                        cid,
                        capsule_hash,
                        resolved_topic_id.as_deref(),
                        resolved_topic_label.as_deref(),
                    )
                    .await;
            }
            if grounding_was_collected {
                let grounding_event = serde_json::json!({
                    "phase": "grounding",
                    "grounding_context_id": grounding_context_id,
                    "context_pack_path": context_pack_path,
                    "context_pack_file": context_pack_file,
                    "grounding_report_file": grounding_report_file,
                    "grounding_report_artifact_path": grounding_report_artifact_path,
                    "grounding_report_hash": grounding_report_hash,
                    "grounding_worker_slot_id": grounding_worker_slot_id,
                    "grounding_worker_turn_id": grounding_worker_turn_id,
                    "context_sufficiency": context_sufficiency,
                    "artifact_hash": grounding_artifact_hash,
                    "context_capsule_hash": context_capsule_hash,
                    "context_capsule_file": context_capsule_file,
                    "topic_id": resolved_topic_id,
                    "topic_label": resolved_topic_label,
                    "sources_used": sources_used,
                    "diagnostics": grounding_diagnostics,
                });
                Self::write_sse_event(&mut stream, "status", &grounding_event).await?;
            }

            let intent_artifact_id = if !intent_confirmed {
                let jarvis_intent_author = JarvisIntentAuthorConfig::default();
                let authored_intent = match Self::author_jarvis_intent_draft_with_progress(
                    &mut stream,
                    &jarvis_progress_bus,
                    &chat_id,
                    None,
                    &provider_box_http,
                    &jarvis_intent_author,
                    "missiond.jarvis-intent-artifact.v1",
                    "jarvis",
                    &objective_text,
                    &grounding_context_id,
                    resolved_topic_id.as_deref(),
                    resolved_topic_label.as_deref(),
                    &sources_used,
                    Some(&permission_context),
                    &media_context,
                )
                .await
                {
                    Ok(draft) => draft,
                    Err(error) => {
                        Self::fail_jarvis_gate_visible(
                            &mut stream,
                            &jarvis_progress_bus,
                            &chat_id,
                            None,
                            format!(
                                "intent.lisp 生成失败：{error}。不会用 Rust fallback 代替你的意图识别。"
                            ),
                            "intent_authoring_failed",
                            db.as_ref(),
                            jarvis_conv_id.as_deref(),
                        )
                        .await?;
                        return Ok(());
                    }
                };
                objective_text = authored_intent.objective.clone();
                let confirmation_required = jarvis_intent_plan_confirmation_required();
                let intent_payload = serde_json::json!({
                "schema": "missiond.jarvis-intent-artifact.v1",
                "phase": if confirmation_required { "intent_draft" } else { "intent_archived" },
                "author": "codex-cli-gpt-5.5-xhigh",
                "intent_author_slot_id": &jarvis_intent_author.slot_id,
                "intent_kind": authored_intent.intent_kind,
                "confidence": authored_intent.confidence,
                "grounding_context_id": grounding_context_id,
                "context_pack_path": context_pack_path,
                "context_pack_file": context_pack_file,
                "grounding_report_file": grounding_report_file,
                "grounding_report_artifact_path": grounding_report_artifact_path,
                "grounding_report_hash": grounding_report_hash,
                "grounding_worker_slot_id": grounding_worker_slot_id,
                "grounding_worker_turn_id": grounding_worker_turn_id,
                "context_sufficiency": context_sufficiency,
                "context_capsule_hash": context_capsule_hash,
                "context_capsule_file": context_capsule_file,
                "topic_id": resolved_topic_id,
                "topic_label": resolved_topic_label,
                "understanding": authored_intent.understanding,
                "objective": objective_text,
                "original_user_message": &raw_user_text,
                "user_message_preview": raw_user_text.chars().take(240).collect::<String>(),
                "review_text": authored_intent.review_text,
                "artifact_language": "lisp",
                "artifact_body": authored_intent.artifact_body,
                "assumptions": authored_intent.assumptions,
                "non_goals": authored_intent.non_goals,
                "acceptance_signals": authored_intent.acceptance_signals,
                "sources_used": sources_used,
                "requires_confirmation": confirmation_required,
                "visibility": if confirmation_required { "review" } else { "archive_only" }
                });
                let intent_artifact = match Self::put_jarvis_artifact(
                    &jarvis_artifact_writer,
                    JarvisArtifactRequest {
                        kind: "jarvis-intent-draft".to_string(),
                        project_id: None,
                        task_id: None,
                        payload: intent_payload.clone(),
                        metadata: serde_json::json!({
                            "schema": "missiond.jarvis-intent-artifact.v1",
                            "chat_id": chat_id.clone(),
                            "conversation_id": jarvis_conv_id.clone(),
                            "grounding_context_id": grounding_context_id,
                        }),
                    },
                )
                .await
                {
                    Ok(result) => result,
                    Err(error) => {
                        Self::fail_jarvis_gate(&mut stream, error, "intent_artifact").await?;
                        return Ok(());
                    }
                };
                let intent_artifact_id = intent_artifact.artifact_id.clone();
                let intent_artifact_hash = intent_artifact.artifact_hash.clone();
                let intent_artifact_path = intent_artifact.path.clone();
                let mut intent = intent_payload;
                if let Some(object) = intent.as_object_mut() {
                    object.insert(
                        "intent_artifact_id".to_string(),
                        serde_json::Value::String(intent_artifact_id.clone()),
                    );
                    object.insert(
                        "intent_artifact_hash".to_string(),
                        serde_json::Value::String(intent_artifact_hash.clone()),
                    );
                    object.insert(
                        "intent_artifact_path".to_string(),
                        serde_json::Value::String(intent_artifact_path.clone()),
                    );
                }
                let intent_event_name = if confirmation_required {
                    "intent_draft"
                } else {
                    "intent_archived"
                };
                Self::write_sse_event(&mut stream, intent_event_name, &intent).await?;
                if confirmation_required {
                    Self::write_sse_openai_missiond_projection(
                        &mut stream,
                        &chat_id,
                        "intent_draft",
                        &intent_artifact_id,
                        &intent_artifact_hash,
                        &intent_artifact_path,
                    )
                    .await?;
                }
                if !confirmation_required {
                    Self::write_jarvis_progress(
                        &mut stream,
                        &jarvis_progress_bus,
                        &chat_id,
                        None,
                        "grounding",
                        "context_gather_start",
                        "running",
                        "intent.lisp 已归档，正在调用挂载 MissionD MCP 的 ClaudeCode 工位收集上下游全链上下文。",
                        None,
                        None,
                        Some("claude-code-mcp-grounding"),
                    )
                    .await?;
                    let result = match Self::gather_jarvis_grounding_with_progress(
                        &mut stream,
                        &jarvis_progress_bus,
                        &chat_id,
                        None,
                        &jarvis_grounding,
                        JarvisGroundingRequest {
                            query: objective_text.clone(),
                            confirmed_intent_artifact_id: Some(intent_artifact_id.clone()),
                            confirmed_intent_lisp: intent
                                .get("artifact_body")
                                .and_then(|value| value.as_str())
                                .map(ToOwned::to_owned),
                            conversation_id: jarvis_conv_id.clone(),
                            chat_id: chat_id.clone(),
                            user_id: conversation_scope.user_id.clone(),
                            tenant_id: conversation_scope.tenant_id.clone(),
                            application_id: conversation_scope.application_id.clone(),
                            channel: Some(conversation_scope.channel.clone()),
                            topic_id: conversation_scope.topic_id.clone(),
                            topic_label: conversation_scope.topic_label.clone(),
                            permission_context: permission_context.clone(),
                            media_context: media_context.clone(),
                            unknowns: vec![
                                "Collect MissionD upstream/downstream facts that affect this archived intent.".to_string(),
                                "Identify project registry, SSOT, runtime, skill, provider, infra, and permission evidence needed before plan.lisp.".to_string(),
                                "Write the grounded evidence report for the plan author instead of creating BoardTask or implementing changes.".to_string(),
                            ],
                        },
                    )
                    .await
                    {
                        Ok(result) => result,
                        Err(error) => {
                            Self::fail_jarvis_gate(&mut stream, error, "grounding").await?;
                            return Ok(());
                        }
                    };
                    Self::write_jarvis_progress(
                        &mut stream,
                        &jarvis_progress_bus,
                        &chat_id,
                        None,
                        "grounding",
                        "context_gather_completed",
                        "completed",
                        &format!(
                            "grounding 已完成，context={}，下一步进入关键判断和 plan authoring。",
                            result.grounding_context_id
                        ),
                        None,
                        result.grounding_worker_slot_id.as_deref(),
                        Some("claude-code-mcp-grounding"),
                    )
                    .await?;
                    grounding_context_id = result.grounding_context_id.clone();
                    context_pack_path = result.context_pack_path.clone();
                    context_pack_file = result.context_pack_file.clone();
                    grounding_report_file = result.grounding_report_file.clone();
                    grounding_report_artifact_path = result.grounding_report_artifact_path.clone();
                    grounding_report_hash = result.grounding_report_hash.clone();
                    grounding_worker_slot_id = result.grounding_worker_slot_id.clone();
                    grounding_worker_turn_id = result.grounding_worker_turn_id.clone();
                    context_sufficiency = result.context_sufficiency.clone();
                    grounding_artifact_hash = result.artifact_hash.clone();
                    context_capsule_hash = result.context_capsule_hash.clone();
                    context_capsule_file = result.context_capsule_file.clone();
                    resolved_topic_id = result
                        .topic_id
                        .clone()
                        .or_else(|| conversation_scope.topic_id.clone());
                    resolved_topic_label = result
                        .topic_label
                        .clone()
                        .or_else(|| conversation_scope.topic_label.clone());
                    sources_used = result.sources_used.clone();
                    grounding_diagnostics = result.diagnostics.clone();
                    let grounding_event = serde_json::json!({
                        "phase": "grounding",
                        "grounding_context_id": grounding_context_id,
                        "context_pack_path": context_pack_path,
                        "context_pack_file": context_pack_file,
                        "grounding_report_file": grounding_report_file,
                        "grounding_report_artifact_path": grounding_report_artifact_path,
                        "grounding_report_hash": grounding_report_hash,
                        "grounding_worker_slot_id": grounding_worker_slot_id,
                        "grounding_worker_turn_id": grounding_worker_turn_id,
                        "context_sufficiency": context_sufficiency,
                        "artifact_hash": grounding_artifact_hash,
                        "context_capsule_hash": context_capsule_hash,
                        "context_capsule_file": context_capsule_file,
                        "topic_id": resolved_topic_id,
                        "topic_label": resolved_topic_label,
                        "sources_used": sources_used,
                        "diagnostics": grounding_diagnostics,
                    });
                    Self::write_sse_event(&mut stream, "status", &grounding_event).await?;
                }
                if !confirmation_required {
                    intent_artifact_id
                } else {
                    let confirm = serde_json::json!({
                        "phase": "awaiting_intent_confirmation",
                        "message": "请确认：我的意图理解是否正确？确认后我会先收集 MissionD grounding，再生成 plan.lisp；plan 确认后可能直接回答或创建 BoardTask。",
                        "confirm_payload": {
                            "missiond_intent_confirmed": true,
                            "missiond_objective": objective_text,
                            "missiond_grounding_context_id": grounding_context_id,
                            "missiond_intent_artifact_id": intent_artifact_id
                        }
                    });
                    Self::write_sse_event(&mut stream, "confirm_required", &confirm).await?;
                    Self::persist_jarvis_pending_confirmation(
                        db.as_ref(),
                        jarvis_conv_id.as_deref(),
                        &confirm,
                    )
                    .await;
                    Self::write_sse_openai_text_and_persist(
                        &mut stream,
                        &chat_id,
                        "我已生成 intent.lisp 草案，等待你确认意图。",
                        Some("stop"),
                        db.as_ref(),
                        jarvis_conv_id.as_deref(),
                    )
                    .await?;
                    Self::finish_sse(&mut stream).await?;
                    return Ok(());
                }
            } else {
                confirmed_intent_artifact_id.clone().unwrap_or_default()
            };

            if intent_confirmed && !plan_confirmed {
                Self::persist_jarvis_confirmation_fulfilled(
                    db.as_ref(),
                    jarvis_conv_id.as_deref(),
                    "intent",
                )
                .await;
            }

            let key_judgment_ref = if plan_confirmed {
                match Self::jarvis_key_judgment_from_confirm_value(&effective_req) {
                    Ok(result) => result,
                    Err(error) => {
                        Self::fail_jarvis_gate(&mut stream, error, "confirmation_key_judgment")
                            .await?;
                        return Ok(());
                    }
                }
            } else {
                let jarvis_key_judgment_author = JarvisKeyJudgmentAuthorConfig::default();
                let authored_key_judgment =
                    match Self::author_jarvis_key_judgment_draft_with_progress(
                        &mut stream,
                        &jarvis_progress_bus,
                        &chat_id,
                        None,
                        &provider_box_http,
                        &jarvis_key_judgment_author,
                        "missiond.interaction-key-judgment.v1",
                        "jarvis",
                        &objective_text,
                        &grounding_context_id,
                        &intent_artifact_id,
                        resolved_topic_id.as_deref(),
                        resolved_topic_label.as_deref(),
                        &sources_used,
                        Some(&permission_context),
                        context_pack_path.as_deref(),
                        context_pack_file.as_deref(),
                        grounding_report_file.as_deref(),
                        grounding_report_artifact_path.as_deref(),
                        grounding_report_hash.as_deref(),
                        context_sufficiency.as_deref(),
                    )
                    .await
                    {
                        Ok(draft) => draft,
                        Err(error) => {
                            Self::fail_jarvis_gate_visible(
                                &mut stream,
                                &jarvis_progress_bus,
                                &chat_id,
                                None,
                                format!(
                                    "关键判断生成失败：{error}。不会用 Rust fallback 代替判断。"
                                ),
                                "key_judgment_authoring_failed",
                                db.as_ref(),
                                jarvis_conv_id.as_deref(),
                            )
                            .await?;
                            return Ok(());
                        }
                    };
                let key_judgment = authored_key_judgment.judgment.clone();
                let key_review_text = authored_key_judgment.review_text.clone();
                let key_confidence = authored_key_judgment.confidence.clone();
                let key_rejected_hypotheses = authored_key_judgment.rejected_hypotheses.clone();
                let key_evidence_refs = authored_key_judgment.evidence_refs.clone();
                let key_planning_implications = authored_key_judgment.planning_implications.clone();
                let key_acceptance_focus = authored_key_judgment.acceptance_focus.clone();
                let key_payload = serde_json::json!({
                    "schema": "missiond.interaction-key-judgment.v1",
                    "phase": "key_judgment_draft",
                    "author": "codex-cli-gpt-5.5-xhigh",
                    "key_judgment_author_slot_id": &jarvis_key_judgment_author.slot_id,
                    "confidence": key_confidence,
                    "grounding_context_id": grounding_context_id,
                    "context_pack_path": context_pack_path,
                    "context_pack_file": context_pack_file,
                    "grounding_report_file": grounding_report_file,
                    "grounding_report_artifact_path": grounding_report_artifact_path,
                    "grounding_report_hash": grounding_report_hash,
                    "intent_artifact_id": intent_artifact_id,
                    "objective": objective_text,
                    "judgment": key_judgment,
                    "review_text": key_review_text,
                    "rejected_hypotheses": key_rejected_hypotheses,
                    "evidence_refs": key_evidence_refs,
                    "planning_implications": key_planning_implications,
                    "acceptance_focus": key_acceptance_focus,
                    "artifact_language": "lisp",
                    "artifact_body": authored_key_judgment.artifact_body,
                    "sources_used": sources_used,
                    "requires_confirmation": false
                });
                let key_artifact = match Self::put_jarvis_artifact(
                    &jarvis_artifact_writer,
                    JarvisArtifactRequest {
                        kind: "interaction-key-judgment".to_string(),
                        project_id: None,
                        task_id: None,
                        payload: key_payload.clone(),
                        metadata: serde_json::json!({
                            "schema": "missiond.interaction-key-judgment.v1",
                            "chat_id": chat_id.clone(),
                            "conversation_id": jarvis_conv_id.clone(),
                            "grounding_context_id": grounding_context_id,
                            "intent_artifact_id": intent_artifact_id,
                        }),
                    },
                )
                .await
                {
                    Ok(result) => result,
                    Err(error) => {
                        Self::fail_jarvis_gate(&mut stream, error, "key_judgment_artifact").await?;
                        return Ok(());
                    }
                };
                let mut key_event = key_payload;
                if let Some(object) = key_event.as_object_mut() {
                    object.insert(
                        "key_judgment_artifact_id".to_string(),
                        serde_json::Value::String(key_artifact.artifact_id.clone()),
                    );
                    object.insert(
                        "key_judgment_artifact_hash".to_string(),
                        serde_json::Value::String(key_artifact.artifact_hash.clone()),
                    );
                    object.insert(
                        "key_judgment_artifact_path".to_string(),
                        serde_json::Value::String(key_artifact.path.clone()),
                    );
                }
                Self::write_sse_event(&mut stream, "key_judgment_draft", &key_event).await?;
                Self::write_sse_openai_missiond_projection(
                    &mut stream,
                    &chat_id,
                    "key_judgment_draft",
                    &key_artifact.artifact_id,
                    &key_artifact.artifact_hash,
                    &key_artifact.path,
                )
                .await?;
                JarvisKeyJudgmentArtifactRef {
                    artifact_id: key_artifact.artifact_id,
                    artifact_hash: Some(key_artifact.artifact_hash),
                    artifact_path: Some(key_artifact.path),
                    judgment: key_judgment,
                    review_text: Some(key_review_text),
                    confidence: key_confidence,
                    rejected_hypotheses: key_rejected_hypotheses,
                    evidence_refs: key_evidence_refs,
                    planning_implications: key_planning_implications,
                    acceptance_focus: key_acceptance_focus,
                }
            };

            let mut generated_plan_atomization_graph: Option<serde_json::Value> = None;
            let mut generated_execution_mode: Option<String> = None;
            let mut generated_requires_board_task: Option<bool> = None;
            let mut generated_direct_answer_draft: Option<String> = None;
            let plan_artifact_id = if !plan_confirmed {
                let jarvis_plan_author = JarvisPlanAuthorConfig::default();
                let authored_plan = match Self::author_jarvis_plan_draft_with_progress(
                    &mut stream,
                    &jarvis_progress_bus,
                    &chat_id,
                    None,
                    &provider_box_http,
                    &jarvis_plan_author,
                    "missiond.jarvis-plan-artifact.v1",
                    "jarvis",
                    &objective_text,
                    &grounding_context_id,
                    &intent_artifact_id,
                    &key_judgment_ref,
                    resolved_topic_id.as_deref(),
                    resolved_topic_label.as_deref(),
                    &sources_used,
                    Some(&permission_context),
                    context_pack_path.as_deref(),
                    context_pack_file.as_deref(),
                    grounding_report_file.as_deref(),
                    grounding_report_artifact_path.as_deref(),
                    grounding_report_hash.as_deref(),
                    grounding_worker_slot_id.as_deref(),
                    grounding_worker_turn_id.as_deref(),
                    context_sufficiency.as_deref(),
                )
                .await
                {
                    Ok(draft) => draft,
                    Err(error) => {
                        let diagnostic = serde_json::json!({
                            "phase": "plan_authoring_failed",
                            "error": {
                                "code": "JARVIS_PLAN_AUTHOR_FAILED",
                                "message": error.to_string()
                            }
                        });
                        Self::write_sse_event(&mut stream, "diagnostic", &diagnostic).await?;
                        Self::fail_jarvis_gate_visible(
                            &mut stream,
                            &jarvis_progress_bus,
                            &chat_id,
                            None,
                            format!("plan.lisp 生成失败：{error}。plan.lisp 需要 Codex CLI GPT-5.5 xhigh 工位生成；当前工位不可用或输出未通过校验，已停止，不会用 Rust fallback 代替你的计划生成。"),
                            "plan_authoring_failed",
                            db.as_ref(),
                            jarvis_conv_id.as_deref(),
                        )
                        .await?;
                        return Ok(());
                    }
                };
                objective_text = authored_plan.objective.clone();
                let plan_review_text = authored_plan.review_text.clone();
                let plan_artifact_body = authored_plan.artifact_body.clone();
                let plan_steps = authored_plan.steps.clone();
                let confirmation_required = jarvis_intent_plan_confirmation_required();
                let plan_payload = serde_json::json!({
                "schema": "missiond.jarvis-plan-artifact.v1",
                "phase": if confirmation_required { "plan_draft" } else { "plan_archived" },
                "author": "codex-cli-gpt-5.5-xhigh",
                "plan_author_slot_id": &jarvis_plan_author.slot_id,
                "confidence": authored_plan.confidence,
                "grounding_context_id": grounding_context_id,
                "context_pack_path": context_pack_path,
                "context_pack_file": context_pack_file,
                "grounding_report_file": grounding_report_file,
                "grounding_report_artifact_path": grounding_report_artifact_path,
                "grounding_report_hash": grounding_report_hash,
                "grounding_worker_slot_id": grounding_worker_slot_id,
                "grounding_worker_turn_id": grounding_worker_turn_id,
                "context_sufficiency": context_sufficiency,
                "grounding_artifact_hash": grounding_artifact_hash,
                "context_capsule_hash": context_capsule_hash,
                "context_capsule_file": context_capsule_file,
                "topic_id": resolved_topic_id,
                "topic_label": resolved_topic_label,
                "intent_artifact_id": intent_artifact_id,
                "key_judgment_artifact_id": key_judgment_ref.artifact_id,
                "key_judgment_artifact_hash": key_judgment_ref.artifact_hash,
                "key_judgment_artifact_path": key_judgment_ref.artifact_path,
                "key_judgment": key_judgment_ref.judgment,
                "key_judgment_review_text": key_judgment_ref.review_text,
                "key_judgment_confidence": key_judgment_ref.confidence,
                "key_judgment_rejected_hypotheses": key_judgment_ref.rejected_hypotheses,
                "key_judgment_evidence_refs": key_judgment_ref.evidence_refs,
                "key_judgment_planning_implications": key_judgment_ref.planning_implications,
                "key_judgment_acceptance_focus": key_judgment_ref.acceptance_focus,
                "objective": objective_text,
                "review_text": plan_review_text,
                "execution_mode": authored_plan.execution_mode,
                "requires_board_task": authored_plan.requires_board_task,
                "answer_policy": authored_plan.answer_policy,
                "provider_hint": authored_plan.provider_hint,
                "plan_key_judgment": authored_plan.key_judgment,
                "artifact_language": "lisp",
                "artifact_body": plan_artifact_body,
                "steps": plan_steps,
                "direct_answer_draft": authored_plan.direct_answer_draft,
                "workstreams": authored_plan.workstreams,
                "atom_tasks": authored_plan.atom_tasks,
                "dependency_edges": authored_plan.dependency_edges,
                "serial_groups": authored_plan.serial_groups,
                "parallel_groups": authored_plan.parallel_groups,
                "assignment_policy": authored_plan.assignment_policy,
                "atomization_graph": authored_plan.atomization_graph,
                "boundary": authored_plan.boundary,
                "assumptions": authored_plan.assumptions,
                "non_goals": authored_plan.non_goals,
                "acceptance_signals": authored_plan.acceptance_signals,
                "sources_used": sources_used,
                "requires_confirmation": confirmation_required,
                "visibility": if confirmation_required { "review" } else { "archive_only" }
                });
                let plan_artifact = match Self::put_jarvis_artifact(
                    &jarvis_artifact_writer,
                    JarvisArtifactRequest {
                        kind: "jarvis-plan-draft".to_string(),
                        project_id: None,
                        task_id: None,
                        payload: plan_payload.clone(),
                        metadata: serde_json::json!({
                            "schema": "missiond.jarvis-plan-artifact.v1",
                            "chat_id": chat_id.clone(),
                            "conversation_id": jarvis_conv_id.clone(),
                            "grounding_context_id": grounding_context_id,
                            "intent_artifact_id": intent_artifact_id,
                        }),
                    },
                )
                .await
                {
                    Ok(result) => result,
                    Err(error) => {
                        Self::fail_jarvis_gate(&mut stream, error, "plan_artifact").await?;
                        return Ok(());
                    }
                };
                let plan_artifact_id = plan_artifact.artifact_id.clone();
                let plan_artifact_hash = plan_artifact.artifact_hash.clone();
                let plan_artifact_path = plan_artifact.path.clone();
                let mut plan = plan_payload;
                if let Some(object) = plan.as_object_mut() {
                    object.insert(
                        "plan_artifact_id".to_string(),
                        serde_json::Value::String(plan_artifact_id.clone()),
                    );
                    object.insert(
                        "plan_artifact_hash".to_string(),
                        serde_json::Value::String(plan_artifact_hash.clone()),
                    );
                    object.insert(
                        "plan_artifact_path".to_string(),
                        serde_json::Value::String(plan_artifact_path.clone()),
                    );
                }
                generated_plan_atomization_graph = plan.get("atomization_graph").cloned();
                generated_execution_mode = plan
                    .get("execution_mode")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned);
                generated_requires_board_task = plan
                    .get("requires_board_task")
                    .and_then(|value| value.as_bool());
                generated_direct_answer_draft = plan
                    .get("direct_answer_draft")
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned);
                let plan_event_name = if confirmation_required {
                    "plan_draft"
                } else {
                    "plan_archived"
                };
                Self::write_sse_event(&mut stream, plan_event_name, &plan).await?;
                if confirmation_required {
                    Self::write_sse_openai_missiond_projection(
                        &mut stream,
                        &chat_id,
                        "plan_draft",
                        &plan_artifact_id,
                        &plan_artifact_hash,
                        &plan_artifact_path,
                    )
                    .await?;
                }
                if !confirmation_required {
                    plan_artifact_id
                } else {
                    let plan_confirm_message = if authored_plan
                        .execution_mode
                        .eq_ignore_ascii_case("grounded_direct_answer")
                        && !authored_plan.requires_board_task
                    {
                        "请确认 plan.lisp。确认后我会基于 grounding 和权限上下文生成直接回答，不创建 BoardTask。"
                    } else {
                        "请确认 plan.lisp。确认后我会创建 BoardTask 并派工位，不会让主控直接做实现。"
                    };
                    let confirm = serde_json::json!({
                        "phase": "awaiting_plan_confirmation",
                        "message": plan_confirm_message,
                        "confirm_payload": {
                            "missiond_intent_confirmed": true,
                            "missiond_plan_confirmed": true,
                            "missiond_objective": objective_text,
                            "missiond_grounding_context_id": grounding_context_id,
                            "missiond_context_pack_path": context_pack_path,
                            "missiond_context_pack_file": context_pack_file,
                            "missiond_grounding_report_file": grounding_report_file,
                            "missiond_grounding_report_artifact_path": grounding_report_artifact_path,
                            "missiond_grounding_report_hash": grounding_report_hash,
                            "missiond_grounding_worker_slot_id": grounding_worker_slot_id,
                            "missiond_grounding_worker_turn_id": grounding_worker_turn_id,
                            "missiond_context_sufficiency": context_sufficiency,
                            "missiond_grounding_artifact_hash": grounding_artifact_hash,
                            "missiond_context_capsule_hash": context_capsule_hash,
                            "missiond_context_capsule_file": context_capsule_file,
                            "missiond_topic_id": resolved_topic_id,
                            "missiond_topic_label": resolved_topic_label,
                            "missiond_sources_used": sources_used,
                            "missiond_intent_artifact_id": intent_artifact_id,
                            "missiond_key_judgment_artifact_id": key_judgment_ref.artifact_id,
                            "missiond_key_judgment_artifact_hash": key_judgment_ref.artifact_hash,
                            "missiond_key_judgment_artifact_path": key_judgment_ref.artifact_path,
                            "missiond_key_judgment": key_judgment_ref.judgment,
                            "missiond_key_judgment_review_text": key_judgment_ref.review_text,
                            "missiond_key_judgment_confidence": key_judgment_ref.confidence,
                            "missiond_key_judgment_rejected_hypotheses": key_judgment_ref.rejected_hypotheses,
                            "missiond_key_judgment_evidence_refs": key_judgment_ref.evidence_refs,
                            "missiond_key_judgment_planning_implications": key_judgment_ref.planning_implications,
                            "missiond_key_judgment_acceptance_focus": key_judgment_ref.acceptance_focus,
                            "missiond_plan_artifact_id": plan_artifact_id,
                            "missiond_direct_answer_draft": generated_direct_answer_draft.clone(),
                            "missiond_plan_atomization_graph_json": serde_json::to_string(
                                plan.get("atomization_graph").unwrap_or(&serde_json::Value::Null)
                            ).ok(),
                            "missiond_execution_mode": plan
                                .get("execution_mode")
                                .and_then(|value| value.as_str())
                                .unwrap_or("work_order"),
                            "missiond_requires_board_task": plan
                                .get("requires_board_task")
                                .and_then(|value| value.as_bool())
                                .unwrap_or(true)
                        }
                    });
                    Self::write_sse_event(&mut stream, "confirm_required", &confirm).await?;
                    Self::persist_jarvis_pending_confirmation(
                        db.as_ref(),
                        jarvis_conv_id.as_deref(),
                        &confirm,
                    )
                    .await;
                    Self::write_sse_openai_text_and_persist(
                        &mut stream,
                        &chat_id,
                        "我已生成 plan.lisp 草案，等待你确认计划。",
                        Some("stop"),
                        db.as_ref(),
                        jarvis_conv_id.as_deref(),
                    )
                    .await?;
                    Self::finish_sse(&mut stream).await?;
                    return Ok(());
                }
            } else {
                confirmed_plan_artifact_id.clone().unwrap_or_default()
            };
            let plan_atomization_graph = generated_plan_atomization_graph.unwrap_or_else(|| {
                Self::jarvis_plan_atomization_graph_from_confirm_value(&effective_req)
            });

            let execution_mode = generated_execution_mode
                .or_else(|| jarvis_confirm_string(&effective_req, "missiond_execution_mode"))
                .unwrap_or_else(|| "work_order".to_string())
                .to_ascii_lowercase();
            if execution_mode == "grounded_direct_answer" {
                let direct_answer_draft = generated_direct_answer_draft.or_else(|| {
                    jarvis_confirm_string(&effective_req, "missiond_direct_answer_draft")
                });
                let requires_board_task = generated_requires_board_task.unwrap_or_else(|| {
                    jarvis_confirm_bool(&effective_req, "missiond_requires_board_task")
                });
                if requires_board_task {
                    Self::fail_jarvis_gate(
                        &mut stream,
                        "plan.lisp declared grounded_direct_answer but missiond_requires_board_task=true; refusing ambiguous execution.",
                        "execution_mode",
                    )
                    .await?;
                    return Ok(());
                }
                let legacy_progress_bus = JarvisProgressBus::default();
                if let Err(error) = Self::stream_jarvis_grounded_direct_answer(
                    &mut stream,
                    &legacy_progress_bus,
                    &jarvis_artifact_writer,
                    &chat_id,
                    None,
                    &objective_text,
                    &grounding_context_id,
                    context_pack_path.as_deref(),
                    context_pack_file.as_deref(),
                    grounding_report_file.as_deref(),
                    grounding_report_artifact_path.as_deref(),
                    grounding_report_hash.as_deref(),
                    &intent_artifact_id,
                    &plan_artifact_id,
                    &key_judgment_ref,
                    direct_answer_draft.as_deref(),
                    &permission_context,
                    &sources_used,
                    &media_context,
                    &provider_box_http,
                    db.as_ref(),
                    jarvis_conv_id.as_deref(),
                )
                .await
                {
                    Self::fail_jarvis_gate(
                        &mut stream,
                        error.to_string(),
                        "grounded_direct_answer",
                    )
                    .await?;
                    return Ok(());
                }
                Self::finish_sse(&mut stream).await?;
                return Ok(());
            }
            if !matches!(execution_mode.as_str(), "work_order" | "investigation_only") {
                Self::fail_jarvis_gate(
                    &mut stream,
                    format!("Unsupported Jarvis execution_mode: {execution_mode}"),
                    "execution_mode",
                )
                .await?;
                return Ok(());
            }

            let Some(ref db) = db else {
                let err = serde_json::json!({
                    "phase": "board_dispatch",
                    "phase_code": "board_dispatch",
                    "error": {
                        "code": "MISSIOND_DB_UNAVAILABLE",
                        "message": "MissionD DB unavailable; cannot create grounded BoardTask"
                    }
                });
                let _ = stream
                    .write_all(format!("event: diagnostic\ndata: {}\n\n", err).as_bytes())
                    .await;
                let _ = stream.write_all(b"data: [DONE]\n\n").await;
                let _ = stream.flush().await;
                stream.shutdown().await?;
                return Ok(());
            };

            let task_title: String = if objective_text.chars().count() > 80 {
                format!("{}...", objective_text.chars().take(77).collect::<String>())
            } else {
                objective_text.clone()
            };
            let dispatch_metadata = Self::derive_jarvis_dispatch_contract(
                &objective_text,
                &grounding_context_id,
                context_pack_path.as_deref(),
                context_pack_file.as_deref(),
                grounding_report_file.as_deref(),
                grounding_report_artifact_path.as_deref(),
                grounding_report_hash.as_deref(),
                &intent_artifact_id,
                &plan_artifact_id,
                &key_judgment_ref,
                &plan_atomization_graph,
                &Self::jarvis_runtime_read_scope_root(),
            );
            let prompt_template =
                Self::build_jarvis_worker_prompt(&objective_text, &dispatch_metadata);
            let context_intent = dispatch_metadata
                .get("task_class")
                .and_then(|v| v.as_str())
                .unwrap_or("review")
                .to_string();
            let meta = serde_json::json!({
                "source": "jarvis-intent-plan-gate",
                "conversation_id": jarvis_conv_id.as_deref().unwrap_or(""),
                "grounding_context_id": grounding_context_id,
                "context_pack_path": context_pack_path,
                "context_pack_file": context_pack_file,
                "context_capsule_hash": context_capsule_hash,
                "context_capsule_file": context_capsule_file,
                "topic_id": resolved_topic_id,
                "topic_label": resolved_topic_label,
                "intent_artifact_id": intent_artifact_id,
                "plan_artifact_id": plan_artifact_id,
                "key_judgment_artifact_id": key_judgment_ref.artifact_id,
                "key_judgment_artifact_hash": key_judgment_ref.artifact_hash,
                "key_judgment": key_judgment_ref.judgment,
                "plan_atomization_graph": plan_atomization_graph,
                "dispatch_metadata": dispatch_metadata,
                "user_message": raw_user_text,
                "objective": objective_text,
            });
            let task_input = crate::types::CreateBoardTaskInput {
                title: task_title,
                description: Some(format!(
                    "Jarvis interaction task for conversation {}. See runtime_metadata for grounding, intent, plan, and dispatch fields.",
                    jarvis_conv_id.as_deref().unwrap_or("")
                )),
                priority: None,
                category: Some("jarvis".to_string()),
                project: None,
                server: None,
                due_date: None,
                parent_id: None,
                assignee: None,
                auto_execute: Some(true),
                prompt_template: Some(prompt_template),
                hidden: Some(false),
                flow_template: None,
                depends_on: None,
                dedupe_key: None,
                timeout_secs: None,
                context_intent: Some(context_intent),
                runtime_metadata: Some(meta),
            };
            match Self::create_jarvis_atomized_board_tasks(
                db,
                task_input,
                &objective_text,
                &dispatch_metadata,
                &plan_atomization_graph,
            )
            .await
            {
                Ok(created) => {
                    Self::persist_jarvis_confirmation_fulfilled(
                        Some(db),
                        jarvis_conv_id.as_deref(),
                        "plan",
                    )
                    .await;
                    let atom_task_ids = created
                        .atom_tasks
                        .iter()
                        .map(|atom| atom.task.id.to_string())
                        .collect::<Vec<_>>();
                    let planned_atom_task_ids = created
                        .atom_tasks
                        .iter()
                        .filter(|atom| !atom.synthetic)
                        .map(|atom| atom.task.id.to_string())
                        .collect::<Vec<_>>();
                    let atom_task_contracts = created
                        .atom_tasks
                        .iter()
                        .map(|atom| {
                            serde_json::json!({
                                "atom_task_id": atom.atom_task_id.clone(),
                                "board_task_id": atom.task.id.to_string(),
                                "category": atom.category.clone(),
                                "assignee_engine": atom.assignee_engine.clone(),
                                "depends_on_atoms": atom.depends_on_atoms.clone(),
                                "parallel_group": atom.parallel_group.clone(),
                                "synthetic": atom.synthetic
                            })
                        })
                        .collect::<Vec<_>>();
                    let event = serde_json::json!({
                        "task_id": created.parent_task.id.to_string(),
                        "root_task_id": created.parent_task.id.to_string(),
                        "final_task_id": created.final_task_id.clone(),
                        "atom_task_ids": planned_atom_task_ids,
                        "atom_task_contracts": atom_task_contracts,
                        "title": created.parent_task.title.clone(),
                        "grounding_context_id": grounding_context_id,
                        "intent_artifact_id": intent_artifact_id,
                        "plan_artifact_id": plan_artifact_id,
                    });
                    Self::write_sse_event(&mut stream, "board_task_created", &event).await?;
                    let follow_payload = serde_json::json!({
                        "missiond_follow_task_id": created.final_task_id.clone(),
                        "missiond_root_task_id": created.parent_task.id.to_string(),
                        "missiond_atom_task_ids": atom_task_ids.clone(),
                        "stream": true
                    });
                    Self::write_sse_event(
                        &mut stream,
                        "worker_dispatched",
                        &serde_json::json!({
                            "phase": "workers_running",
                            "task_id": created.final_task_id.clone(),
                            "root_task_id": created.parent_task.id.to_string(),
                            "atom_task_ids": atom_task_ids.clone(),
                            "slot_id": serde_json::Value::Null,
                            "dispatch_state": "pending_autopilot_claim",
                            "status": created.parent_task.status.as_str(),
                            "terminal_task_result": false,
                            "follow_payload": follow_payload.clone(),
                            "message": "Atom-level BoardTasks are queued for Autopilot/provider claim; final acceptance task is the follow target."
                        }),
                    )
                    .await?;
                    let worker_status = serde_json::json!({
                        "phase": "board_tasks_created",
                        "task_id": created.final_task_id.clone(),
                        "root_task_id": created.parent_task.id.to_string(),
                        "atom_task_ids": atom_task_ids.clone(),
                        "status": created.parent_task.status.as_str(),
                        "terminal_task_result": false,
                        "follow_payload": follow_payload.clone(),
                        "message": "Atom-level BoardTasks created; worker execution continues asynchronously and final result must be read through follow-up supervision."
                    });
                    Self::write_sse_event(&mut stream, "worker_status", &worker_status).await?;
                    Self::write_sse_event(
                        &mut stream,
                        "dispatch_accepted",
                        &serde_json::json!({
                            "phase": "board_tasks_created",
                            "task_id": created.final_task_id.clone(),
                            "root_task_id": created.parent_task.id.to_string(),
                            "atom_task_ids": atom_task_ids.clone(),
                            "status": created.parent_task.status.as_str(),
                            "terminal_task_result": false,
                            "follow_payload": follow_payload.clone(),
                            "message": "Atom-level BoardTasks were created and accepted for asynchronous worker dispatch; this is not a terminal task result."
                        }),
                    )
                    .await?;
                    let pending_event = serde_json::json!({
                        "phase": "result_pending",
                        "task_id": created.final_task_id.clone(),
                        "root_task_id": created.parent_task.id.to_string(),
                        "atom_task_ids": atom_task_ids,
                        "status": "result_pending",
                        "terminal_task_result": false,
                        "follow_payload": follow_payload
                    });
                    Self::write_sse_event(&mut stream, "result_pending", &pending_event).await?;
                    let pending_fallback = format!(
                        "plan.lisp 已归档，Jarvis 原子化 BoardTask 组已创建。后续请求携带 missiond_follow_task_id={} 读取最终验收 task-result-artifact；初始手机请求不会等待长任务完成。",
                        created.final_task_id
                    );
                    let pending_text = match Self::materialize_jarvis_communication(
                        &mut stream,
                        &jarvis_progress_bus,
                        &jarvis_artifact_writer,
                        &chat_id,
                        None,
                        "plan_dispatched",
                        &objective_text,
                        serde_json::json!({
                            "execution_mode": execution_mode,
                            "terminal_task_result": false,
                            "intent_artifact_id": intent_artifact_id,
                            "plan_artifact_id": plan_artifact_id,
                            "key_judgment_artifact_id": key_judgment_ref.artifact_id,
                            "key_judgment": key_judgment_ref.judgment,
                            "root_task_id": created.parent_task.id.to_string(),
                            "final_task_id": created.final_task_id.clone(),
                            "atom_task_ids": pending_event.get("atom_task_ids").cloned(),
                            "follow_payload": pending_event.get("follow_payload").cloned(),
                        }),
                        &provider_box_http,
                        Some(db),
                        jarvis_conv_id.as_deref(),
                    )
                    .await
                    {
                        Ok(text) => text,
                        Err(error) => {
                            let diagnostic = serde_json::json!({
                                "phase": "communicator",
                                "error": {
                                    "code": "JARVIS_COMMUNICATOR_FAILED",
                                    "message": error.to_string()
                                }
                            });
                            Self::write_sse_event(&mut stream, "diagnostic", &diagnostic).await?;
                            pending_fallback
                        }
                    };
                    Self::write_sse_openai_text_and_persist(
                        &mut stream,
                        &chat_id,
                        &pending_text,
                        Some("stop"),
                        Some(db),
                        jarvis_conv_id.as_deref(),
                    )
                    .await?;
                }
                Err(e) => {
                    let err = serde_json::json!({
                        "phase": "board_dispatch",
                        "phase_code": "board_dispatch",
                        "error": {
                            "code": "BOARDTASK_CREATE_FAILED",
                            "message": format!("Failed to create BoardTask: {}", e)
                        }
                    });
                    Self::write_sse_event(&mut stream, "diagnostic", &err).await?;
                }
            }
            Self::finish_sse(&mut stream).await?;
            return Ok(());
        }

        // Slot selection: X-Slot-Id header > V3-projected default slot.
        let slot_id = headers
            .lines()
            .find_map(|line| {
                let lower = line.to_lowercase();
                if lower.starts_with("x-slot-id:") {
                    Some(line.splitn(2, ':').nth(1)?.trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or(default_chat_slot);
        let chat_id = format!(
            "chatcmpl-{}-{}",
            &slot_id,
            chrono::Utc::now().timestamp_millis()
        );

        // Extract Router's trace_id from X-Trace-Id header
        let router_trace_id = headers.lines().find_map(|line| {
            let lower = line.to_lowercase();
            if lower.starts_with("x-trace-id:") {
                Some(line.splitn(2, ':').nth(1)?.trim().to_string())
            } else {
                None
            }
        });

        info!(?addr, slot_id, msg_len = user_message.len(), trace_id = %chat_id, "Chat completions request");

        // Check slot status. If enabled, Jarvis may make one bounded restart
        // attempt for the default slot; failure remains a typed diagnostic.
        let mut status = pty_manager.get_status(&slot_id).await;
        let mut state = status.as_ref().map(|s| s.state.clone());
        let mut auto_heal = serde_json::json!({"status": "not_attempted"});
        if matches!(
            state,
            None | Some(SessionState::Exited | SessionState::Error)
        ) {
            auto_heal = Self::maybe_auto_heal_jarvis_slot(&pty_manager, &slot_id).await;
            if auto_heal.get("status").and_then(|value| value.as_str()) == Some("healed") {
                status = pty_manager.get_status(&slot_id).await;
                state = status.as_ref().map(|s| s.state.clone());
            }
        }

        match &state {
            None | Some(SessionState::Exited) => {
                let error_msg = format!("Slot {} not running.", slot_id);
                trace_store
                    .unavailable_trace(
                        chat_id,
                        addr,
                        &slot_id,
                        &user_message,
                        &error_msg,
                        router_trace_id,
                    )
                    .await;
                let err = serde_json::json!({
                    "error": {
                        "code": "JARVIS_SLOT_UNAVAILABLE",
                        "message": &error_msg,
                    },
                    "auto_heal": auto_heal,
                });
                Self::send_http_error(&mut stream, 503, "Service Unavailable", &err.to_string())
                    .await?;
                return Ok(());
            }
            Some(s) if *s != SessionState::Idle => {
                let error_msg = format!("{} is busy (state: {:?}). Try again later.", slot_id, s);
                trace_store
                    .unavailable_trace(
                        chat_id,
                        addr,
                        &slot_id,
                        &user_message,
                        &error_msg,
                        router_trace_id,
                    )
                    .await;
                let err = serde_json::json!({
                    "error": {
                        "code": "JARVIS_SLOT_BUSY",
                        "message": &error_msg,
                    },
                    "auto_heal": auto_heal,
                    "retry_after": 5
                });
                let response = format!(
                    "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nRetry-After: 5\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    err.to_string().len(),
                    err
                );
                stream.write_all(response.as_bytes()).await?;
                stream.shutdown().await?;
                return Ok(());
            }
            _ => {} // Idle — good to go
        }

        // Proxy mode carries the full transcript in the request body. Do not send
        // `/clear` to the shared PTY: it mutates a user-visible interactive
        // session and creates races with durable conversation capture.
        if proxy_mode {
            debug!(
                slot_id,
                "Proxy mode: using direct transcript prompt without clearing PTY"
            );
        }

        // Start trace
        trace_store
            .start_trace(
                chat_id.clone(),
                addr,
                &slot_id,
                &user_message,
                router_trace_id,
            )
            .await;

        // Create or reuse Jarvis UI conversation for persistence
        let jarvis_conv_id = if let Some(ref db) = db {
            match db
                .jarvis_get_or_create_scoped(
                    conversation_id.as_deref(),
                    conversation_scope.user_id.as_deref(),
                    conversation_scope.tenant_id.as_deref(),
                    conversation_scope.application_id.as_deref(),
                    Some(conversation_scope.channel.as_str()),
                    conversation_scope.topic_id.as_deref(),
                    conversation_scope.topic_label.as_deref(),
                )
                .await
            {
                Ok(id) => Some(id),
                Err(e) => {
                    warn!(error = %e, "Failed to create jarvis conversation");
                    None
                }
            }
        } else {
            None
        };

        // Write SSE response headers immediately — flush for curl to see
        let sse_headers = "HTTP/1.1 200 OK\r\n\
            Content-Type: text/event-stream\r\n\
            Cache-Control: no-cache\r\n\
            Connection: keep-alive\r\n\
            Access-Control-Allow-Origin: *\r\n\
            \r\n";
        stream.write_all(sse_headers.as_bytes()).await?;
        stream.flush().await?;

        // Send conversation_id as first SSE event so frontend can track it
        if let Some(ref cid) = jarvis_conv_id {
            let meta_evt = serde_json::json!({"conversation_id": cid});
            let sse = format!("event: meta\ndata: {}\n\n", meta_evt);
            let _ = stream.write_all(sse.as_bytes()).await;
            let _ = stream.flush().await;
        }

        // Context enrichment: inject KB/Skill/Code context before sending to PTY
        // Disabled when MISSIOND_DISABLE_CONTEXT_ENRICHMENT=1 (e.g. VDS transparent API proxy mode)
        let context_enrichment_disabled = std::env::var("MISSIOND_DISABLE_CONTEXT_ENRICHMENT")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let (enriched_message, enrich_intent) = if context_enrichment_disabled {
            (user_message.clone(), None)
        } else {
            let enricher_guard = context_enricher.read().await;
            if let Some(ref enricher) = *enricher_guard {
                let enricher = Arc::clone(enricher);
                drop(enricher_guard); // release lock before async call
                let enrich_result = enricher(user_message.clone()).await;
                let intent = enrich_result.intent.clone();
                let sys_prompt = jarvis_system_prompt(tool_count);
                let msg = if enrich_result.assembled.is_empty() {
                    format!("{}\n\n{}", sys_prompt, user_message)
                } else {
                    debug!(
                        slot_id,
                        ctx_len = enrich_result.assembled.len(),
                        "Jarvis: context enrichment injected"
                    );
                    format!(
                        "{}\n\n{}\n\n{}",
                        sys_prompt, enrich_result.assembled, user_message
                    )
                };
                (msg, intent)
            } else {
                (
                    format!("{}\n\n{}", jarvis_system_prompt(tool_count), user_message),
                    None,
                )
            }
        };

        // ── Async dispatch (opt-in only) ──
        // Default: synchronous response (normal Claude Code experience).
        // Only dispatch async when intent is explicitly marked "async:" (e.g. long-running
        // deploy/build operations). This gives the user a conversational Jarvis instead of
        // one-line ack stubs.
        let needs_async_dispatch = enrich_intent
            .as_deref()
            .map(|i| i.starts_with("async:"))
            .unwrap_or(false);

        if needs_async_dispatch && !proxy_mode {
            if let Some(ref db) = db {
                // Build Board Task metadata
                let conv_id_str = jarvis_conv_id.as_deref().unwrap_or("");
                let meta = serde_json::json!({
                    "conversation_id": conv_id_str,
                    "user_message": raw_user_text,
                });
                let task_title: String = if raw_user_text.chars().count() > 80 {
                    let truncated: String = raw_user_text.chars().take(77).collect();
                    format!("{}...", truncated)
                } else {
                    raw_user_text.clone()
                };

                let task_input = crate::types::CreateBoardTaskInput {
                    title: task_title.clone(),
                    description: Some(
                        "Legacy Jarvis direct task. See runtime_metadata for original message."
                            .to_string(),
                    ),
                    priority: None,
                    category: Some("jarvis".to_string()),
                    project: None,
                    server: None,
                    due_date: None,
                    parent_id: None,
                    assignee: None, // Dynamic: autopilot picks idle coder slot
                    auto_execute: Some(true),
                    prompt_template: Some(user_message.clone()),
                    hidden: Some(true),
                    flow_template: None,
                    depends_on: None,
                    dedupe_key: None,
                    timeout_secs: None,
                    context_intent: None,
                    runtime_metadata: Some(meta),
                };

                match db.create_board_task(&task_input).await {
                    Ok(task) => {
                        let task_id = task.id.clone();
                        let task_short_id = &task_id.as_str()[..8.min(task_id.as_str().len())];
                        info!(?addr, slot_id, task_id = %task_id, intent = ?enrich_intent, "Jarvis async → Board Task created, SSE bridge active");

                        // Save user message only (autopilot saves assistant response on completion)
                        if let Some(ref cid) = jarvis_conv_id {
                            if !raw_user_text.is_empty() {
                                let _ = db
                                    .router_chat_append_messages(
                                        cid,
                                        &[("user".to_string(), raw_user_text.clone())],
                                    )
                                    .await;
                                if conversation_id.is_none() {
                                    let _ = db.jarvis_update_title(cid, &task_title).await;
                                }
                            }
                        }

                        // Send async_dispatch marker (frontend tracks pending task)
                        let dispatch_event = serde_json::json!({
                            "conversation_id": conv_id_str,
                            "task_id": &task_id,
                        });
                        let _ = stream
                            .write_all(
                                format!("event: async_dispatch\ndata: {}\n\n", dispatch_event)
                                    .as_bytes(),
                            )
                            .await;

                        // Send status: waiting for dispatch
                        let status_evt = serde_json::json!({"phase": "dispatching", "text": format!("等待调度 #{}...", task_short_id)});
                        let _ = stream
                            .write_all(
                                format!("event: status\ndata: {}\n\n", status_evt).as_bytes(),
                            )
                            .await;
                        let _ = stream.flush().await;

                        let bridge_start = std::time::Instant::now();

                        // ── Phase 1: Wait for task to be claimed by a slot ──
                        let dispatch_timeout = tokio::time::Duration::from_secs(120);
                        let poll_interval = tokio::time::Duration::from_secs(1);
                        let mut target_slot: Option<String> = None;
                        let mut task_already_done = false;

                        loop {
                            if bridge_start.elapsed() > dispatch_timeout {
                                let err_msg = "任务调度超时，120秒内未被分配到工位";
                                warn!(?addr, task_id = %task_id, err_msg);
                                let err = serde_json::json!({"error": {"message": err_msg}});
                                let _ = stream
                                    .write_all(format!("data: {}\n\n", err).as_bytes())
                                    .await;
                                let _ = stream.write_all(b"data: [DONE]\n\n").await;
                                let _ = stream.flush().await;
                                trace_store.error_trace(&chat_id, err_msg, None).await;
                                stream.shutdown().await?;
                                return Ok(());
                            }

                            tokio::time::sleep(poll_interval).await;

                            // Check task status in DB
                            if let Ok(Some(t)) = db.get_board_task(task_id.as_str()).await {
                                match t.status {
                                    crate::types::BoardTaskStatus::Running => {
                                        if let Some(ref executor) = t.claim_executor_id {
                                            target_slot = Some(executor.clone());
                                            break;
                                        }
                                    }
                                    crate::types::BoardTaskStatus::Done
                                    | crate::types::BoardTaskStatus::Failed
                                    | crate::types::BoardTaskStatus::Blocked => {
                                        // Task completed/failed before we could subscribe
                                        task_already_done = true;
                                        break;
                                    }
                                    _ => {} // Still open, keep waiting
                                }
                            }

                            // Heartbeat to keep SSE alive
                            let _ = stream.write_all(b":\n\n").await;
                            let _ = stream.flush().await;
                        }

                        // ── Phase 2: Subscribe to executing slot's PTY events ──
                        let mut streamed_response = String::new();

                        if let Some(ref exec_slot) = target_slot {
                            let status_evt = serde_json::json!({"phase": "thinking", "text": format!("已分配到 {}", exec_slot)});
                            let _ = stream
                                .write_all(
                                    format!("event: status\ndata: {}\n\n", status_evt).as_bytes(),
                                )
                                .await;
                            let _ = stream.flush().await;

                            match pty_manager.subscribe_session(exec_slot).await {
                                Ok(mut rx) => {
                                    // Check current slot state: if already past echo phase, enable text forwarding
                                    let mut past_first_thinking = false;
                                    if let Some(current_status) =
                                        pty_manager.get_status(exec_slot).await
                                    {
                                        match current_status.state {
                                            SessionState::Thinking
                                            | SessionState::Responding
                                            | SessionState::ToolRunning => {
                                                past_first_thinking = true;
                                            }
                                            _ => {}
                                        }
                                    }

                                    let mut tool_seq: u32 = 0;
                                    let mut had_activity = false;
                                    let mut last_status_phase = String::new();
                                    let mut last_status_sent = std::time::Instant::now()
                                        - std::time::Duration::from_secs(1);
                                    let status_throttle = std::time::Duration::from_millis(500);
                                    let heartbeat_interval = tokio::time::Duration::from_secs(15);
                                    let bridge_timeout = tokio::time::Duration::from_secs(600); // 10 min max
                                    let mut last_event_time = std::time::Instant::now();

                                    loop {
                                        if bridge_start.elapsed() > bridge_timeout {
                                            warn!(task_id = %task_id, "SSE bridge timeout (10 min)");
                                            break;
                                        }

                                        let recv_timeout = heartbeat_interval
                                            .saturating_sub(last_event_time.elapsed());
                                        match tokio::time::timeout(recv_timeout, rx.recv()).await {
                                            Ok(Ok(event)) => {
                                                last_event_time = std::time::Instant::now();
                                                match event {
                                                    SessionEvent::StatusUpdate(ref status) => {
                                                        let phase = format!("{}", status.phase);
                                                        let phase_changed =
                                                            phase != last_status_phase;
                                                        let throttle_elapsed = last_status_sent
                                                            .elapsed()
                                                            >= status_throttle;
                                                        if phase_changed || throttle_elapsed {
                                                            last_status_phase = phase.clone();
                                                            last_status_sent =
                                                                std::time::Instant::now();
                                                            let evt = serde_json::json!({
                                                                "phase": phase,
                                                                "text": status.status_text,
                                                            });
                                                            let sse = format!(
                                                                "event: status\ndata: {}\n\n",
                                                                evt
                                                            );
                                                            let _ = stream
                                                                .write_all(sse.as_bytes())
                                                                .await;
                                                            let _ = stream.flush().await;
                                                        }
                                                    }
                                                    SessionEvent::ToolOutput(ref tool_output) => {
                                                        had_activity = true;
                                                        use crate::semantic::ToolStatus;
                                                        match tool_output.status {
                                                            ToolStatus::Running => {
                                                                tool_seq += 1;
                                                                let id = format!("t{}", tool_seq);
                                                                let params_json = serde_json::json!(
                                                                    tool_output.params
                                                                );
                                                                let evt = serde_json::json!({
                                                                    "id": id,
                                                                    "tool": tool_output.tool_name,
                                                                    "params": params_json,
                                                                });
                                                                let sse = format!(
                                                                    "event: tool_start\ndata: {}\n\n",
                                                                    evt
                                                                );
                                                                let _ = stream
                                                                    .write_all(sse.as_bytes())
                                                                    .await;
                                                                let _ = stream.flush().await;
                                                            }
                                                            ToolStatus::Completed => {
                                                                let id = format!("t{}", tool_seq);
                                                                let output = tool_output.output.as_deref().map(|o| {
                                                                    if o.len() > 4096 {
                                                                        format!("{}...\n[truncated, {} bytes total]", &o[..4096], o.len())
                                                                    } else {
                                                                        o.to_string()
                                                                    }
                                                                });
                                                                let evt = serde_json::json!({
                                                                    "id": id,
                                                                    "tool": tool_output.tool_name,
                                                                    "duration_ms": tool_output.duration_ms,
                                                                    "output": output,
                                                                });
                                                                let sse = format!(
                                                                    "event: tool_end\ndata: {}\n\n",
                                                                    evt
                                                                );
                                                                let _ = stream
                                                                    .write_all(sse.as_bytes())
                                                                    .await;
                                                                let _ = stream.flush().await;
                                                            }
                                                        }
                                                    }
                                                    SessionEvent::StateChange {
                                                        new_state, ..
                                                    } => {
                                                        match new_state {
                                                            SessionState::Thinking => {
                                                                if !past_first_thinking {
                                                                    past_first_thinking = true;
                                                                    debug!(task_id = %task_id, "SSE bridge: past first thinking, text forwarding enabled");
                                                                }
                                                                let evt = serde_json::json!({"phase": "thinking", "text": "Thinking..."});
                                                                let sse = format!(
                                                                    "event: status\ndata: {}\n\n",
                                                                    evt
                                                                );
                                                                let _ = stream
                                                                    .write_all(sse.as_bytes())
                                                                    .await;
                                                                let _ = stream.flush().await;
                                                            }
                                                            SessionState::ToolRunning => {
                                                                let evt = serde_json::json!({"phase": "tool_running", "text": ""});
                                                                let sse = format!(
                                                                    "event: status\ndata: {}\n\n",
                                                                    evt
                                                                );
                                                                let _ = stream
                                                                    .write_all(sse.as_bytes())
                                                                    .await;
                                                                let _ = stream.flush().await;
                                                            }
                                                            SessionState::Idle => {
                                                                // Slot returned to idle — task likely done
                                                                if had_activity {
                                                                    debug!(task_id = %task_id, "SSE bridge: slot returned to idle after activity");
                                                                    break;
                                                                }
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                    SessionEvent::TextOutput(ref text_event) => {
                                                        use crate::TextOutputEvent;
                                                        match text_event {
                                                            TextOutputEvent::Stream {
                                                                content,
                                                                ..
                                                            } => {
                                                                if past_first_thinking
                                                                    && !content.is_empty()
                                                                {
                                                                    had_activity = true;
                                                                    streamed_response
                                                                        .push_str(content);
                                                                    let chunk = serde_json::json!({
                                                                        "id": &chat_id,
                                                                        "object": "chat.completion.chunk",
                                                                        "model": "jarvis-missiond",
                                                                        "choices": [{
                                                                            "index": 0,
                                                                            "delta": { "content": content },
                                                                            "finish_reason": serde_json::Value::Null,
                                                                        }]
                                                                    });
                                                                    let _ = stream
                                                                        .write_all(
                                                                            format!(
                                                                                "data: {}\n\n",
                                                                                chunk
                                                                            )
                                                                            .as_bytes(),
                                                                        )
                                                                        .await;
                                                                    let _ = stream.flush().await;
                                                                }
                                                            }
                                                            TextOutputEvent::Complete {
                                                                ..
                                                            } => {}
                                                        }
                                                    }
                                                    SessionEvent::ConfirmRequired {
                                                        ref prompt,
                                                        ref info,
                                                    } => {
                                                        let evt = serde_json::json!({
                                                            "action_id": format!("confirm-{}", tool_seq),
                                                            "prompt": prompt,
                                                            "target_slot": exec_slot,
                                                            "info": info.as_ref().map(|i| {
                                                                let structured_options: Vec<serde_json::Value> = i.options.iter()
                                                                    .enumerate()
                                                                    .map(|(idx, label)| serde_json::json!({
                                                                        "key": idx + 1,
                                                                        "label": label,
                                                                        "is_default": idx == 0,
                                                                    }))
                                                                    .collect();
                                                                serde_json::json!({
                                                                    "type": i.confirm_type,
                                                                    "tool": i.tool.as_ref().map(|t| &t.name),
                                                                    "options": structured_options,
                                                                })
                                                            }),
                                                        });
                                                        let sse = format!(
                                                            "event: confirm_required\ndata: {}\n\n",
                                                            evt
                                                        );
                                                        let _ =
                                                            stream.write_all(sse.as_bytes()).await;
                                                        let _ = stream.flush().await;
                                                    }
                                                    SessionEvent::Exit(code) => {
                                                        warn!(task_id = %task_id, code, "SSE bridge: PTY exited");
                                                        break;
                                                    }
                                                    _ => {} // Ignore Data, ScreenText, TitleChange
                                                }
                                            }
                                            Ok(Err(
                                                tokio::sync::broadcast::error::RecvError::Lagged(n),
                                            )) => {
                                                warn!(task_id = %task_id, lagged = n, "SSE bridge broadcast lagged");
                                            }
                                            Ok(Err(
                                                tokio::sync::broadcast::error::RecvError::Closed,
                                            )) => {
                                                break;
                                            }
                                            Err(_) => {
                                                // Heartbeat + DB failsafe check
                                                let _ = stream.write_all(b":\n\n").await;
                                                let _ = stream.flush().await;
                                                last_event_time = std::time::Instant::now();
                                                if let Ok(Some(t)) =
                                                    db.get_board_task(task_id.as_str()).await
                                                {
                                                    match t.status {
                                                        crate::types::BoardTaskStatus::Done
                                                        | crate::types::BoardTaskStatus::Failed => {
                                                            debug!(task_id = %task_id, status = ?t.status, "SSE bridge: task completed (DB failsafe)");
                                                            break;
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(task_id = %task_id, slot = %exec_slot, error = %e, "SSE bridge: failed to subscribe to slot PTY");
                                }
                            }
                        }

                        // ── Phase 3: Wait for DB write + close SSE ──
                        // After slot goes idle, autopilot needs time to save the response to DB.
                        // Wait briefly then verify task is done before closing SSE.
                        if !task_already_done {
                            tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
                        }

                        let duration_ms = bridge_start.elapsed().as_millis() as u64;

                        // Fallback: if text wasn't streamed via PTY events, fetch from DB
                        if streamed_response.is_empty() {
                            if let Some(ref cid) = jarvis_conv_id {
                                // Retry a few times in case autopilot hasn't saved yet
                                for attempt in 0..3 {
                                    if attempt > 0 {
                                        tokio::time::sleep(tokio::time::Duration::from_secs(1))
                                            .await;
                                    }
                                    if let Ok(msgs) = db.router_chat_load_history(cid).await {
                                        // Find last assistant message
                                        if let Some(last_asst) = msgs.iter().rev().find(|m| {
                                            m.get("role").and_then(|v| v.as_str())
                                                == Some("assistant")
                                        }) {
                                            if let Some(content) =
                                                last_asst.get("content").and_then(|v| v.as_str())
                                            {
                                                if !content.is_empty() {
                                                    let chunk = serde_json::json!({
                                                        "id": &chat_id,
                                                        "object": "chat.completion.chunk",
                                                        "model": "jarvis-missiond",
                                                        "choices": [{
                                                            "index": 0,
                                                            "delta": { "content": content },
                                                            "finish_reason": serde_json::Value::Null,
                                                        }]
                                                    });
                                                    let _ = stream
                                                        .write_all(
                                                            format!("data: {}\n\n", chunk)
                                                                .as_bytes(),
                                                        )
                                                        .await;
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Close SSE stream
                        let stop = serde_json::json!({
                            "id": &chat_id,
                            "object": "chat.completion.chunk",
                            "model": "jarvis-missiond",
                            "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }]
                        });
                        let _ = stream
                            .write_all(format!("data: {}\n\n", stop).as_bytes())
                            .await;
                        let _ = stream.write_all(b"data: [DONE]\n\n").await;
                        let _ = stream.flush().await;

                        trace_store
                            .complete_trace(&chat_id, "[async SSE bridge]", duration_ms)
                            .await;
                        info!(?addr, slot_id, task_id = %task_id, duration_ms, "Jarvis SSE bridge completed");
                        stream.shutdown().await?;
                        return Ok(());
                    }
                    Err(e) => {
                        warn!(?addr, error = %e, "Board Task creation failed, falling back to sync path");
                        // Fall through to synchronous path
                    }
                }
            }
        }

        // ── Synchronous path (chat intents / proxy mode / Board Task fallback) ──
        // Dual-source streaming:
        // 1. PTY events → status/tool/confirm/exit events as SSE (activity view)
        // 2. JSONL watcher → structured assistant messages as SSE data chunks (content)
        // PTY send() manages turn lifecycle (paste message, detect completion).
        let mut rx = match pty_manager.subscribe_session(&slot_id).await {
            Ok(rx) => rx,
            Err(e) => {
                let err =
                    serde_json::json!({"error": {"message": format!("Subscribe failed: {}", e)}});
                let event = format!("data: {}\n\n", err);
                let _ = stream.write_all(event.as_bytes()).await;
                let _ = stream.write_all(b"data: [DONE]\n\n").await;
                stream.shutdown().await?;
                return Ok(());
            }
        };

        // Subscribe to JSONL watcher for structured message content
        let mut target_session_id: Option<String> = match db.as_ref() {
            Some(db) => db.get_slot_session(&slot_id).await.ok().flatten(),
            None => None,
        };
        let mut jsonl_rx: Option<broadcast::Receiver<WatcherEvent>> = match &cc_tasks_watcher {
            Some(watcher) => {
                let w = watcher.lock().await;
                Some(w.subscribe())
            }
            None => None,
        };
        if target_session_id.is_some() {
            debug!(slot_id, session_id = ?target_session_id, "JSONL watcher subscribed for chat content");
        }

        // Spawn blocking send() in a background task — it manages turn lifecycle.
        let pty_for_send = Arc::clone(&pty_manager);
        let send_msg = enriched_message.clone();
        let send_slot = slot_id.to_string();
        let send_timeout_ms = jarvis_sync_timeout_ms();
        let send_handle = tokio::spawn(async move {
            pty_for_send
                .send(&send_slot, &send_msg, send_timeout_ms)
                .await
        });

        // Forward activity events via SSE while send() is running
        let start_time = std::time::Instant::now();
        let heartbeat_interval = tokio::time::Duration::from_secs(15);
        let mut tool_seq: u32 = 0;
        let mut seen_uuids = std::collections::HashSet::<String>::new();
        let mut last_event_time = std::time::Instant::now();
        let mut last_status_phase = String::new();
        let mut last_status_sent = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let status_throttle = std::time::Duration::from_millis(500);
        let idle_without_final_grace =
            tokio::time::Duration::from_millis(jarvis_idle_without_final_grace_ms());
        // Buffer: each new assistant message REPLACES the buffer (not appends).
        // A turn produces multiple assistant messages (intermediate tool-calling ones + final response).
        // Only the last one is the actual user-facing answer.
        let mut last_assistant_text: Option<String> = None;
        let mut past_first_thinking = false;
        let mut had_activity = false;
        let mut completed_by_idle = false;
        let mut idle_without_final = false;
        let mut last_provider_event_time = std::time::Instant::now();
        let sent_prompt_for_match = enriched_message.clone();

        let messages_match_current_turn = |messages: &[CCMessageLine]| -> bool {
            messages.iter().any(|m| {
                if m.message.role != "user" {
                    return false;
                }

                let text = match &m.message.content {
                    serde_json::Value::String(s) => Some(s.as_str()),
                    serde_json::Value::Array(blocks) => blocks.iter().find_map(|block| {
                        block
                            .get("text")
                            .and_then(|t| t.as_str())
                            .filter(|t| !t.is_empty())
                    }),
                    _ => None,
                };

                match text {
                    Some(user_text) => {
                        user_text.contains(&sent_prompt_for_match)
                            || sent_prompt_for_match.contains(user_text)
                    }
                    None => false,
                }
            })
        };

        // Helper: extract text from JSONL messages, buffering the latest assistant text
        macro_rules! process_jsonl_messages {
            ($messages:expr) => {
                for msg in $messages {
                    if !seen_uuids.insert(msg.uuid.clone()) {
                        continue;
                    }
                    if msg.message.role != "assistant" {
                        continue;
                    }
                    // Extract text from content (string or array of blocks)
                    let text = match &msg.message.content {
                        serde_json::Value::String(s) => {
                            if s.is_empty() {
                                None
                            } else {
                                Some(s.clone())
                            }
                        }
                        serde_json::Value::Array(blocks) => {
                            let mut texts = Vec::new();
                            let mut has_tool_use = false;
                            for block in blocks {
                                match block.get("type").and_then(|t| t.as_str()) {
                                    Some("text") => {
                                        if let Some(t) = block.get("text").and_then(|t| t.as_str())
                                        {
                                            if !t.is_empty() {
                                                texts.push(t.to_string());
                                            }
                                        }
                                    }
                                    Some("tool_use") => {
                                        has_tool_use = true;
                                    }
                                    _ => {}
                                }
                            }
                            // Tool-use messages invalidate any immediately preceding
                            // assistant text candidate. Claude Code often writes
                            // "I'll check..." as a separate assistant text record
                            // immediately before the tool_use record. That text is
                            // progress, not a final answer.
                            if has_tool_use {
                                last_assistant_text = None;
                                None
                            } else if texts.is_empty() {
                                None
                            } else {
                                Some(texts.join("\n"))
                            }
                        }
                        _ => None,
                    };
                    if let Some(text) = text {
                        // Replace (not append) — the final assistant message wins
                        last_assistant_text = Some(text);
                    }
                }
            };
        }

        loop {
            // Check if send() has completed
            if send_handle.is_finished() {
                break;
            }

            let recv_timeout = heartbeat_interval.saturating_sub(last_event_time.elapsed());
            tokio::select! {
                // PTY events: status, tools, confirm, exit (text content from JSONL watcher)
                pty_event = rx.recv() => {
                    last_event_time = std::time::Instant::now();
                    last_provider_event_time = std::time::Instant::now();
                    match pty_event {
                        Ok(SessionEvent::StatusUpdate(status)) => {
                            let phase = format!("{}", status.phase);
                            let phase_changed = phase != last_status_phase;
                            let throttle_elapsed = last_status_sent.elapsed() >= status_throttle;
                            if phase_changed || throttle_elapsed {
                                last_status_phase = phase.clone();
                                last_status_sent = std::time::Instant::now();
                                let evt = serde_json::json!({"phase": phase, "text": status.status_text});
                                let sse = format!("event: status\ndata: {}\n\n", evt);
                                let _ = stream.write_all(sse.as_bytes()).await;
                                let _ = stream.flush().await;
                            }
                        }
                        Ok(SessionEvent::ToolOutput(tool_output)) => {
                            use crate::semantic::ToolStatus;
                            // TODO: Migrate to JSONL tool activity layer (Option D) — see Board 151a1373
                            // Filter out meta-orchestration tools that flood the SSE stream.
                            // These generate dozens of repeated events when Claude Code uses Agent subprocesses.
                            const META_TOOLS: &[&str] = &["Agent", "Skill", "Explore"];
                            if META_TOOLS.iter().any(|t| tool_output.tool_name.eq_ignore_ascii_case(t)) {
                                // silently skip — meta-tool activity not useful in chat UI
                                continue;
                            }
                            match tool_output.status {
                                ToolStatus::Running => {
                                    had_activity = true;
                                    tool_seq += 1;
                                    let id = format!("t{}", tool_seq);
                                    let evt = serde_json::json!({
                                        "id": id, "tool": tool_output.tool_name,
                                        "params": serde_json::json!(tool_output.params),
                                    });
                                    let sse = format!("event: tool_start\ndata: {}\n\n", evt);
                                    let _ = stream.write_all(sse.as_bytes()).await;
                                    let _ = stream.flush().await;
                                }
                                ToolStatus::Completed => {
                                    had_activity = true;
                                    let id = format!("t{}", tool_seq);
                                    let output = tool_output.output.as_deref().map(|o| {
                                        if o.len() > 4096 {
                                            format!("{}...\n[truncated, {} bytes total]", &o[..4096], o.len())
                                        } else { o.to_string() }
                                    });
                                    let evt = serde_json::json!({
                                        "id": id, "tool": tool_output.tool_name,
                                        "duration_ms": tool_output.duration_ms, "output": output,
                                    });
                                    let sse = format!("event: tool_end\ndata: {}\n\n", evt);
                                    let _ = stream.write_all(sse.as_bytes()).await;
                                    let _ = stream.flush().await;
                                }
                            }
                        }
                        Ok(SessionEvent::StateChange { new_state, .. }) => {
                            match new_state {
                                SessionState::Thinking => {
                                    if !past_first_thinking {
                                        past_first_thinking = true;
                                    }
                                    let evt = serde_json::json!({"phase": "thinking", "text": "Thinking..."});
                                    let sse = format!("event: status\ndata: {}\n\n", evt);
                                    let _ = stream.write_all(sse.as_bytes()).await;
                                    let _ = stream.flush().await;
                                }
                                SessionState::ToolRunning => {
                                    had_activity = true;
                                    let evt = serde_json::json!({"phase": "tool_running", "text": ""});
                                    let sse = format!("event: status\ndata: {}\n\n", evt);
                                    let _ = stream.write_all(sse.as_bytes()).await;
                                    let _ = stream.flush().await;
                                }
                                SessionState::Idle => {
                                    if had_activity || last_assistant_text.is_some() {
                                        completed_by_idle = true;
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        Ok(SessionEvent::Exit(code)) => {
                            let err_msg = format!("PTY session exited with code {}", code);
                            trace_store.error_trace(&chat_id, &err_msg, None).await;
                            let err = serde_json::json!({"error": {"message": err_msg}});
                            let _ = stream.write_all(format!("data: {}\n\n", err).as_bytes()).await;
                            let _ = stream.write_all(b"data: [DONE]\n\n").await;
                            warn!(?addr, slot_id, code, trace_id = %chat_id, "PTY exited during streaming");
                            let _ = stream.shutdown().await;
                            return Ok(());
                        }
                        Ok(SessionEvent::ConfirmRequired { prompt, info }) => {
                            let evt = serde_json::json!({
                                "action_id": format!("confirm-{}", tool_seq),
                                "prompt": prompt,
                                "info": info.as_ref().map(|i| {
                                    let structured_options: Vec<serde_json::Value> = i.options.iter()
                                        .enumerate()
                                        .map(|(idx, label)| serde_json::json!({
                                            "key": idx + 1, "label": label, "is_default": idx == 0,
                                        }))
                                        .collect();
                                    serde_json::json!({
                                        "type": i.confirm_type,
                                        "tool": i.tool.as_ref().map(|t| &t.name),
                                        "options": structured_options,
                                    })
                                }),
                            });
                            let sse = format!("event: confirm_required\ndata: {}\n\n", evt);
                            let _ = stream.write_all(sse.as_bytes()).await;
                            let _ = stream.flush().await;
                        }
                        Ok(SessionEvent::TextOutput(ref text_event)) => {
                            use crate::TextOutputEvent;
                            match text_event {
                                TextOutputEvent::Stream { content, .. } => {
                                    if past_first_thinking && !content.is_empty() {
                                        // PTY text extraction is diagnostic only on the
                                        // Jarvis HTTP/SSE path. Live tests showed Claude
                                        // Code can classify pasted user prompt echo as
                                        // assistant text. User-visible content must come
                                        // from durable provider JSONL assistant messages.
                                        had_activity = true;
                                    }
                                }
                                TextOutputEvent::Complete { content, .. } => {
                                    if !content.is_empty() {
                                        // Keep as diagnostic evidence only; do not promote
                                        // PTY screen text to final chat content, and do
                                        // not use it as a completion signal. Claude Code
                                        // can emit a screen "complete" before the durable
                                        // assistant final is written to JSONL.
                                    }
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!(slot_id, lagged = n, "PTY broadcast lagged");
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }

                // JSONL watcher: structured assistant messages from .jsonl file
                jsonl_event = async {
                    match jsonl_rx.as_mut() {
                        Some(jrx) => jrx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Ok(WatcherEvent::NewMessages { session_id, messages, .. }) = jsonl_event {
                        if messages_match_current_turn(&messages) {
                            if target_session_id.as_deref() != Some(session_id.as_str()) {
                                debug!(
                                    slot_id,
                                    session_id = %session_id,
                                    previous_session_id = ?target_session_id,
                                    "Jarvis JSONL watcher bound current turn session"
                                );
                                target_session_id = Some(session_id.clone());
                            }
                        } else if let Some(db) = db.as_ref() {
                            if let Ok(Some(current_session_id)) = db.get_slot_session(&slot_id).await {
                                if current_session_id == session_id
                                    && target_session_id.as_deref() != Some(session_id.as_str())
                                {
                                    debug!(
                                        slot_id,
                                        session_id = %session_id,
                                        previous_session_id = ?target_session_id,
                                        "Jarvis JSONL watcher refreshed slot session binding"
                                    );
                                    target_session_id = Some(session_id.clone());
                                }
                            }
                        }

                        if target_session_id.as_deref() == Some(session_id.as_str()) {
                            last_provider_event_time = std::time::Instant::now();
                            process_jsonl_messages!(messages);
                        }
                    }
                }

                // Heartbeat timeout
                _ = tokio::time::sleep(recv_timeout) => {
                    if (past_first_thinking || had_activity)
                        && last_assistant_text.is_none()
                        && last_provider_event_time.elapsed() >= idle_without_final_grace
                    {
                        if let Some(status) = pty_manager.get_status(&slot_id).await {
                            if let Ok(lines) = pty_manager.get_last_lines(&slot_id, 40).await {
                                let snapshot = crate::pty::recognize_screen(
                                    status.engine,
                                    &lines,
                                    status.state,
                                );
                                if matches!(
                                    snapshot.state,
                                    crate::pty::PtyCanonicalState::Idle
                                        | crate::pty::PtyCanonicalState::Complete
                                ) {
                                    completed_by_idle = true;
                                    idle_without_final = true;
                                    warn!(
                                        ?addr,
                                        slot_id,
                                        trace_id = %chat_id,
                                        reason = %snapshot.reason,
                                        "Jarvis provider returned to prompt without durable final"
                                    );
                                    break;
                                }
                            }
                        }
                    }
                    let _ = stream.write_all(b":\n\n").await;
                    let _ = stream.flush().await;
                    last_event_time = std::time::Instant::now();
                }
            }
        }

        // ── Drain: after send() completes, catch final JSONL messages (500ms window) ──
        if let Some(ref mut jrx) = jsonl_rx {
            let drain_deadline =
                tokio::time::Instant::now() + tokio::time::Duration::from_millis(500);
            loop {
                match tokio::time::timeout_at(drain_deadline, jrx.recv()).await {
                    Ok(Ok(WatcherEvent::NewMessages {
                        session_id,
                        messages,
                        ..
                    })) => {
                        if messages_match_current_turn(&messages) {
                            if target_session_id.as_deref() != Some(session_id.as_str()) {
                                target_session_id = Some(session_id.clone());
                            }
                        } else if let Some(db) = db.as_ref() {
                            if let Ok(Some(current_session_id)) =
                                db.get_slot_session(&slot_id).await
                            {
                                if current_session_id == session_id
                                    && target_session_id.as_deref() != Some(session_id.as_str())
                                {
                                    target_session_id = Some(session_id.clone());
                                }
                            }
                        }

                        if target_session_id.as_deref() == Some(session_id.as_str()) {
                            process_jsonl_messages!(messages);
                        }
                    }
                    Ok(Ok(_)) => {}  // other watcher events
                    Ok(Err(_)) => {} // lagged/closed
                    Err(_) => break, // drain timeout reached
                }
            }
        }

        // ── send() result: emit buffered response, persist, close ──
        let send_result = if completed_by_idle && !send_handle.is_finished() {
            let mut send_handle = send_handle;
            tokio::select! {
                result = &mut send_handle => Some(result),
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {
                    send_handle.abort();
                    None
                }
            }
        } else {
            Some(send_handle.await)
        };

        let mut fatal_error: Option<String> = None;
        match send_result {
            Some(Ok(Ok(result))) => {
                debug!(
                    ?addr,
                    slot_id,
                    response_len = result.response.len(),
                    trace_id = %chat_id,
                    "PTY send() returned diagnostic screen response"
                );
            }
            Some(Ok(Err(e))) => {
                if last_assistant_text.is_none() {
                    fatal_error = Some(format!("Claude Code error: {}", e));
                } else {
                    warn!(?addr, slot_id, error = %e, trace_id = %chat_id, "Chat completions send() ended with error after durable output");
                }
            }
            Some(Err(e)) => {
                if last_assistant_text.is_none() {
                    fatal_error = Some(format!("Internal error: {}", e));
                } else {
                    warn!(?addr, slot_id, error = %e, trace_id = %chat_id, "Chat completions send task ended after durable output");
                }
            }
            None => {
                warn!(?addr, slot_id, trace_id = %chat_id, "Chat completions closed by idle completion before send() returned");
            }
        }

        if fatal_error.is_none() && last_assistant_text.is_none() {
            if let Some(ref mut jrx) = jsonl_rx {
                let settle_deadline = tokio::time::Instant::now()
                    + tokio::time::Duration::from_millis(jarvis_final_settle_ms());
                loop {
                    if last_assistant_text.is_some() {
                        break;
                    }
                    match tokio::time::timeout_at(settle_deadline, jrx.recv()).await {
                        Ok(Ok(WatcherEvent::NewMessages {
                            session_id,
                            messages,
                            ..
                        })) => {
                            if messages_match_current_turn(&messages) {
                                if target_session_id.as_deref() != Some(session_id.as_str()) {
                                    target_session_id = Some(session_id.clone());
                                }
                            } else if let Some(db) = db.as_ref() {
                                if let Ok(Some(current_session_id)) =
                                    db.get_slot_session(&slot_id).await
                                {
                                    if current_session_id == session_id
                                        && target_session_id.as_deref() != Some(session_id.as_str())
                                    {
                                        target_session_id = Some(session_id.clone());
                                    }
                                }
                            }

                            if target_session_id.as_deref() == Some(session_id.as_str()) {
                                process_jsonl_messages!(messages);
                            }
                        }
                        Ok(Ok(_)) => {}
                        Ok(Err(_)) => {}
                        Err(_) => break,
                    }
                }
            }
        }

        if idle_without_final && last_assistant_text.is_none() {
            fatal_error = Some(
                "Claude Code returned to the input prompt without producing a final answer. The request was closed to avoid an infinite iOS/Jarvis wait; please retry or inspect the worker transcript."
                    .to_string(),
            );
        }

        if let Some(error_message) = fatal_error {
            trace_store
                .error_trace(&chat_id, &error_message, None)
                .await;
            let err = serde_json::json!({"error": {"message": error_message}});
            let _ = stream
                .write_all(format!("data: {}\n\n", err).as_bytes())
                .await;
            let _ = stream.write_all(b"data: [DONE]\n\n").await;
            let _ = stream.flush().await;
            warn!(?addr, slot_id, trace_id = %chat_id, "Chat completions error");
        } else {
            let duration_ms = start_time.elapsed().as_millis() as u64;

            // User-visible content must come from the durable JSONL assistant final.
            // PTY screen text is diagnostic only and must never become chat content.
            let final_response = if let Some(ref text) = last_assistant_text {
                text.clone()
            } else {
                String::new()
            };

            if final_response.is_empty() {
                let error_message =
                    "Claude Code did not produce a durable assistant final message. PTY screen text is diagnostic only and will not be used as chat content."
                        .to_string();
                trace_store
                    .error_trace(&chat_id, &error_message, None)
                    .await;
                let err = serde_json::json!({"error": {"message": error_message}});
                let _ = stream
                    .write_all(format!("data: {}\n\n", err).as_bytes())
                    .await;
                let _ = stream.write_all(b"data: [DONE]\n\n").await;
                let _ = stream.flush().await;
                warn!(?addr, slot_id, trace_id = %chat_id, "Chat completions missing durable final");
                let _ = stream.shutdown().await;
                return Ok(());
            }
            trace_store
                .complete_trace(&chat_id, &final_response, duration_ms)
                .await;

            // Persist conversation to DB
            if let (Some(ref db), Some(ref cid)) = (&db, &jarvis_conv_id) {
                if !raw_user_text.is_empty() {
                    if let Err(e) = db
                        .jarvis_save_exchange(cid, &raw_user_text, &final_response)
                        .await
                    {
                        warn!(error = %e, conv_id = %cid, "Failed to save jarvis exchange");
                    } else if conversation_id.is_none() {
                        let title = if raw_user_text.chars().count() > 80 {
                            let truncated: String = raw_user_text.chars().take(77).collect();
                            format!("{}...", truncated)
                        } else {
                            raw_user_text.clone()
                        };
                        let _ = db.jarvis_update_title(cid, &title).await;
                    }
                }
            }

            // Emit the final response as a single SSE chunk.
            if !final_response.is_empty() {
                let chunk = serde_json::json!({
                    "id": &chat_id,
                    "object": "chat.completion.chunk",
                    "model": "jarvis-missiond",
                    "choices": [{"index": 0, "delta": {"content": final_response}, "finish_reason": serde_json::Value::Null}]
                });
                let _ = stream
                    .write_all(format!("data: {}\n\n", chunk).as_bytes())
                    .await;
            }

            let stop = serde_json::json!({
                "id": &chat_id,
                "object": "chat.completion.chunk",
                "model": "jarvis-missiond",
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
            });
            let _ = stream
                .write_all(format!("data: {}\n\n", stop).as_bytes())
                .await;
            let _ = stream.write_all(b"data: [DONE]\n\n").await;
            info!(?addr, slot_id, response_len = final_response.len(), duration_ms, trace_id = %chat_id, "Chat completions done (JSONL+PTY)");
        }

        let _ = stream.shutdown().await;
        Ok(())
    }

    async fn handle_connection(
        stream: TcpStream,
        addr: SocketAddr,
        pty_manager: Option<Arc<PTYManager>>,
        cc_tasks_watcher: Option<Arc<Mutex<CCTasksWatcher>>>,
        screenshot_broker: Option<Arc<super::ScreenshotBroker>>,
        _jarvis_trace: JarvisTraceStore,
        incident_tx: Option<tokio::sync::mpsc::Sender<crate::types::MissionIncident>>,
        system_event_tx: Option<tokio::sync::mpsc::Sender<SystemEvent>>,
        frontend_events_tx: Option<broadcast::Sender<String>>,
        db: Option<Arc<dyn crate::db::traits::MissionStore>>,
        _context_enricher: ContextEnricherSlot,
        jarvis_grounding: JarvisGroundingSlot,
        jarvis_artifact_writer: JarvisArtifactSlot,
        provider_box_http: ProviderBoxHttpSlot,
        _tool_count: usize,
        default_chat_slot: String,
        jarvis_intent_author: JarvisIntentAuthorConfig,
        jarvis_key_judgment_author: JarvisKeyJudgmentAuthorConfig,
        jarvis_plan_author: JarvisPlanAuthorConfig,
    ) -> anyhow::Result<()> {
        // Peek at first bytes to detect non-WebSocket HTTP requests
        let mut peek_buf = [0u8; 512];
        let n = stream.peek(&mut peek_buf).await.unwrap_or(0);
        if n > 0 {
            let request_line = String::from_utf8_lossy(&peek_buf[..n]);
            let first_line = request_line.lines().next().unwrap_or_default();
            let mut request_parts = first_line.split_whitespace();
            let method = request_parts.next().unwrap_or_default();
            let path = request_parts.next().unwrap_or_default();
            let version = request_parts.next().unwrap_or("HTTP/1.1");
            let normalized_path = normalize_public_jarvis_path(path);
            let normalized_request_line = format!("{method} {normalized_path} {version}");
            let is_upgrade = request_line.to_ascii_lowercase().contains("upgrade:");
            let jarvis_progress_bus = JarvisProgressBus {
                system_event_tx: system_event_tx.clone(),
                frontend_events_tx: frontend_events_tx.clone(),
            };
            // Local deploy-center smoke may restore Jarvis provider slots after a
            // blue/green restart. This is intentionally not a normalized
            // `/jarvis/*` public route.
            if method == "POST" && path == "/internal/jarvis/slot/ensure" && !is_upgrade {
                return match pty_manager {
                    Some(pm) => {
                        Self::handle_jarvis_slot_ensure(
                            stream,
                            addr,
                            pm,
                            default_chat_slot.clone(),
                            jarvis_intent_author.clone(),
                            jarvis_key_judgment_author.clone(),
                            jarvis_plan_author.clone(),
                        )
                        .await
                    }
                    None => {
                        let mut s = stream;
                        let err = serde_json::json!({
                            "schema": "missiond.jarvis-slot-ensure.v1",
                            "overall": "unavailable",
                            "error": {"message": "PTY manager not available"}
                        });
                        Self::send_http_error(&mut s, 503, "Service Unavailable", &err.to_string())
                            .await
                    }
                };
            }
            // Health check
            if method == "GET" && normalized_path == "/health" && !is_upgrade {
                return Self::handle_health(stream).await;
            }
            if method == "GET" && normalized_path == "/v1/project-universe" && !is_upgrade {
                return Self::handle_project_universe(stream).await;
            }
            if method == "OPTIONS" && normalized_path.starts_with("/provider-box/v1/") {
                let mut s = stream;
                let response = "HTTP/1.1 204 No Content\r\n\
                    Access-Control-Allow-Origin: *\r\n\
                    Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
                    Access-Control-Allow-Headers: Content-Type, Authorization, X-Slot-Id, X-Trace-Id\r\n\
                    Access-Control-Max-Age: 86400\r\n\
                    Content-Length: 0\r\n\
                    Connection: close\r\n\
                    \r\n";
                s.write_all(response.as_bytes()).await?;
                s.shutdown().await?;
                return Ok(());
            }
            if !is_upgrade
                && normalized_path.starts_with("/provider-box/v1/")
                && matches!(method, "GET" | "POST")
            {
                return Self::handle_provider_box_http(
                    stream,
                    method,
                    normalized_path.as_ref(),
                    provider_box_http,
                )
                .await;
            }
            // Jarvis readiness: daemon/proxy health plus default slot availability.
            if method == "GET" && normalized_path == "/api/readiness" && !is_upgrade {
                return match pty_manager {
                    Some(pm) => Self::handle_readiness(stream, pm, default_chat_slot.clone()).await,
                    None => {
                        let mut s = stream;
                        let err = serde_json::json!({
                            "status": "slot_unavailable",
                            "error": {"message": "PTY manager not available"}
                        });
                        Self::send_http_error(&mut s, 503, "Service Unavailable", &err.to_string())
                            .await
                    }
                };
            }
            // Jarvis chain monitor: one endpoint for public proxy, daemon,
            // default slot, MCP config, release, and diagnostic files.
            if method == "GET" && normalized_path == "/api/monitor/jarvis" && !is_upgrade {
                return match pty_manager {
                    Some(pm) => {
                        Self::handle_jarvis_monitor(
                            stream,
                            pm,
                            default_chat_slot.clone(),
                            jarvis_intent_author.clone(),
                            jarvis_key_judgment_author.clone(),
                            jarvis_plan_author.clone(),
                        )
                        .await
                    }
                    None => {
                        let mut s = stream;
                        let err = serde_json::json!({
                            "schema": "missiond.jarvis-chain-monitor.v2",
                            "legacy_schema": "missiond.jarvis-chain-monitor.v1",
                            "overall": "unavailable",
                            "error": {"message": "PTY manager not available"}
                        });
                        Self::send_http_error(&mut s, 503, "Service Unavailable", &err.to_string())
                            .await
                    }
                };
            }
            // AIOps webhook endpoint
            if method == "POST" && normalized_path.starts_with("/webhooks/") {
                return Self::handle_webhook(
                    stream,
                    &normalized_request_line,
                    incident_tx,
                    system_event_tx,
                )
                .await;
            }
            // POST /interactions/v1/messages (and public /jarvis/interactions/v1/messages)
            if method == "POST" && normalized_path == "/interactions/v1/messages" {
                return Self::handle_interaction_messages(
                    stream,
                    addr,
                    pty_manager,
                    jarvis_progress_bus,
                    jarvis_intent_author.clone(),
                    jarvis_key_judgment_author.clone(),
                    jarvis_plan_author.clone(),
                    jarvis_grounding,
                    jarvis_artifact_writer,
                    provider_box_http.clone(),
                    db,
                )
                .await;
            }
            // GET /interactions/v1/{interaction_id}/events
            if method == "GET" && normalized_path.starts_with("/interactions/v1/") && !is_upgrade {
                return Self::handle_interaction_events(
                    stream,
                    &normalized_request_line,
                    db.clone(),
                )
                .await;
            }
            // Jarvis durable conversation history for authenticated mobile clients.
            if method == "GET"
                && normalized_path.starts_with("/api/jarvis/conversations")
                && !is_upgrade
            {
                return Self::handle_jarvis_conversations(
                    stream,
                    &normalized_request_line,
                    db.clone(),
                )
                .await;
            }
            // Chat completions SSE endpoint
            // POST /v1/chat/completions (and public /jarvis/v1/chat/completions)
            if method == "POST" && normalized_path == "/v1/chat/completions" {
                return Self::handle_chat_completions_interaction_adapter(
                    stream,
                    addr,
                    pty_manager.clone(),
                    default_chat_slot.clone(),
                    jarvis_progress_bus,
                    jarvis_intent_author.clone(),
                    jarvis_key_judgment_author.clone(),
                    jarvis_plan_author.clone(),
                    jarvis_grounding,
                    jarvis_artifact_writer,
                    provider_box_http.clone(),
                    db,
                )
                .await;
            }
            // Slot status API
            if method == "GET" && normalized_path == "/api/slots" && !is_upgrade {
                return match pty_manager {
                    Some(pm) => Self::handle_slot_status(stream, pm, db.clone()).await,
                    None => {
                        let mut s = stream;
                        let err =
                            serde_json::json!({"error": {"message": "PTY manager not available"}});
                        Self::send_http_error(&mut s, 503, "Service Unavailable", &err.to_string())
                            .await
                    }
                };
            }
            // CORS preflight for chat completions
            if method == "OPTIONS" && normalized_path == "/v1/chat/completions" {
                let mut s = stream;
                let response = "HTTP/1.1 204 No Content\r\n\
                    Access-Control-Allow-Origin: *\r\n\
                    Access-Control-Allow-Methods: POST, OPTIONS\r\n\
                    Access-Control-Allow-Headers: Content-Type, Authorization, X-Slot-Id, X-Trace-Id\r\n\
                    Access-Control-Max-Age: 86400\r\n\
                    Content-Length: 0\r\n\
                    Connection: close\r\n\
                    \r\n";
                s.write_all(response.as_bytes()).await?;
                s.shutdown().await?;
                return Ok(());
            }
            if method == "OPTIONS" && normalized_path.starts_with("/api/jarvis/conversations") {
                let mut s = stream;
                let response = "HTTP/1.1 204 No Content\r\n\
                    Access-Control-Allow-Origin: *\r\n\
                    Access-Control-Allow-Methods: GET, OPTIONS\r\n\
                    Access-Control-Allow-Headers: Content-Type, Authorization\r\n\
                    Access-Control-Max-Age: 86400\r\n\
                    Content-Length: 0\r\n\
                    Connection: close\r\n\
                    \r\n";
                s.write_all(response.as_bytes()).await?;
                s.shutdown().await?;
                return Ok(());
            }
            if method == "OPTIONS" && normalized_path.starts_with("/interactions/v1/") {
                let mut s = stream;
                let response = "HTTP/1.1 204 No Content\r\n\
                    Access-Control-Allow-Origin: *\r\n\
                    Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
                    Access-Control-Allow-Headers: Content-Type, Authorization, X-Trace-Id\r\n\
                    Access-Control-Max-Age: 86400\r\n\
                    Content-Length: 0\r\n\
                    Connection: close\r\n\
                    \r\n";
                s.write_all(response.as_bytes()).await?;
                s.shutdown().await?;
                return Ok(());
            }
        }

        // Capture path from handshake
        let path_cell = Arc::new(StdMutex::new(String::new()));
        let path_cell2 = Arc::clone(&path_cell);

        let ws_stream = accept_hdr_async(stream, move |req: &WsRequest, resp: WsResponse| {
            if let Ok(mut path) = path_cell2.lock() {
                *path = req.uri().path().to_string();
            }
            Ok(resp)
        })
        .await?;

        let path = path_cell
            .lock()
            .map(|p| p.clone())
            .unwrap_or_else(|_| "/".to_string());
        let path = normalize_public_jarvis_path(&path).into_owned();

        match parse_route(&path) {
            Route::Tasks => {
                Self::handle_tasks_subscription(addr, ws_stream, cc_tasks_watcher).await
            }
            Route::Events => {
                Self::handle_events_subscription(addr, ws_stream, frontend_events_tx, db).await
            }
            Route::Pty { slot_id } => {
                Self::handle_pty_subscription(
                    addr,
                    ws_stream,
                    pty_manager,
                    screenshot_broker,
                    slot_id,
                )
                .await
            }
            Route::Invalid => {
                let (mut ws_tx, _ws_rx) = ws_stream.split();
                let _ = ws_tx
                    .send(Message::Close(Some(close_frame(
                        4000,
                        "Invalid URL. Use /pty/<slotId>, /tasks, or /events",
                    ))))
                    .await;
                warn!(?addr, %path, "Invalid WebSocket URL");
                Ok(())
            }
        }
    }

    async fn handle_pty_subscription(
        addr: SocketAddr,
        ws_stream: tokio_tungstenite::WebSocketStream<TcpStream>,
        pty_manager: Option<Arc<PTYManager>>,
        screenshot_broker: Option<Arc<super::ScreenshotBroker>>,
        slot_id: &str,
    ) -> anyhow::Result<()> {
        let pty_manager = match pty_manager {
            Some(pm) => pm,
            None => {
                let (mut ws_tx, _ws_rx) = ws_stream.split();
                let _ = ws_tx
                    .send(Message::Close(Some(close_frame(
                        4000,
                        "PTY manager not available",
                    ))))
                    .await;
                warn!(?addr, "PTY manager not available");
                return Ok(());
            }
        };

        // Check if PTY session exists
        let status = pty_manager.get_status(slot_id).await;
        if status
            .as_ref()
            .map(|s| s.state == SessionState::Exited)
            .unwrap_or(true)
        {
            let (mut ws_tx, _ws_rx) = ws_stream.split();
            let _ = ws_tx
                .send(Message::Close(Some(close_frame(
                    4001,
                    format!("PTY session not found: {}", slot_id),
                ))))
                .await;
            warn!(?addr, slot_id, "PTY session not found");
            return Ok(());
        }

        let (mut ws_tx, mut ws_rx) = ws_stream.split();

        info!(?addr, slot_id, "Client attached to PTY");

        // Send replay buffer (raw PTY output history) for late-joining clients
        if let Ok(replay) = pty_manager.get_replay_buffer(slot_id).await {
            if !replay.is_empty() {
                let data = String::from_utf8_lossy(&replay).to_string();
                let msg = PtyOutMessage::Screen { data };
                let _ = send_json(&mut ws_tx, &msg).await;
            }
        }

        // Send current state so new connections see the correct status immediately
        if let Some(status) = pty_manager.get_status(slot_id).await {
            let msg = PtyOutMessage::State {
                state: status.state.clone(),
                prev_state: status.state,
                status_text: status.status_text.clone(),
            };
            let _ = send_json(&mut ws_tx, &msg).await;
        }

        // Subscribe to session events
        let mut session_rx = match pty_manager.subscribe_session(slot_id).await {
            Ok(rx) => rx,
            Err(e) => {
                warn!(?addr, slot_id, error = %e, "Cannot subscribe to PTY events");
                let _ = ws_tx
                    .send(Message::Close(Some(close_frame(
                        4002,
                        format!("Cannot attach to PTY: {}", slot_id),
                    ))))
                    .await;
                return Ok(());
            }
        };

        // Subscribe to screenshot requests (if broker available)
        let mut screenshot_rx = screenshot_broker.as_ref().map(|b| b.subscribe());

        // State heartbeat: periodically send current state to prevent stale UI.
        // If a StateChange event is missed (broadcast lag), the client would
        // be stuck showing the old state forever. This 5s heartbeat fixes that.
        let mut state_heartbeat = tokio::time::interval(std::time::Duration::from_secs(5));
        state_heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_sent_state: Option<SessionState> = None;

        loop {
            tokio::select! {
                // State heartbeat -> send current state if changed since last heartbeat
                _ = state_heartbeat.tick() => {
                    if let Some(status) = pty_manager.get_status(slot_id).await {
                        let current = status.state.clone();
                        if last_sent_state.as_ref() != Some(&current) {
                            let prev = last_sent_state.replace(current.clone()).unwrap_or(current.clone());
                            let msg = PtyOutMessage::State { state: current, prev_state: prev, status_text: status.status_text.clone() };
                            if send_json(&mut ws_tx, &msg).await.is_err() {
                                break;
                            }
                        }
                    }
                }

                // PTY -> client
                evt = session_rx.recv() => {
                    let evt = match evt {
                        Ok(e) => e,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!(slot_id, lagged = n, "PTY broadcast lagged, continuing");
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    };

                    match evt {
                        SessionEvent::Data(bytes) => {
                            let data = String::from_utf8_lossy(&bytes).to_string();
                            let msg = PtyOutMessage::Data { data };
                            if send_json(&mut ws_tx, &msg).await.is_err() {
                                break;
                            }
                        }
                        SessionEvent::StateChange { new_state, prev_state } => {
                            last_sent_state = Some(new_state.clone());
                            // Get status_text from agent_info
                            let status_text = pty_manager.get_status(slot_id).await
                                .and_then(|s| s.status_text);
                            let msg = PtyOutMessage::State { state: new_state, prev_state, status_text };
                            if send_json(&mut ws_tx, &msg).await.is_err() {
                                break;
                            }
                        }
                        SessionEvent::Exit(code) => {
                            let msg = PtyOutMessage::Exit { code };
                            let _ = send_json(&mut ws_tx, &msg).await;
                            let _ = ws_tx.send(Message::Close(Some(close_frame(4003, format!("PTY exited with code {}", code))))).await;
                            break;
                        }
                        SessionEvent::StatusUpdate(status) => {
                            // Push status_text to client in real-time
                            let text = format!("{} {}", status.spinner, status.status_text);
                            let current_state = last_sent_state.clone().unwrap_or(SessionState::Starting);
                            let msg = PtyOutMessage::State {
                                state: current_state.clone(),
                                prev_state: current_state,
                                status_text: Some(text),
                            };
                            if send_json(&mut ws_tx, &msg).await.is_err() {
                                break;
                            }
                        }
                        _ => {}
                    }
                }

                // Screenshot request from broker -> forward to browser client
                screenshot_req = async {
                    match screenshot_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Ok((req_slot, request_id)) = screenshot_req {
                        if req_slot == slot_id {
                            let msg = PtyOutMessage::ScreenshotRequest { request_id };
                            let _ = send_json(&mut ws_tx, &msg).await;
                        }
                    }
                }

                // client -> PTY
                msg = ws_rx.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(input) = serde_json::from_str::<PtyInMessage>(&text) {
                                match input {
                                    PtyInMessage::Input { data } => {
                                        let _ = pty_manager.write(slot_id, &data).await;
                                    }
                                    PtyInMessage::ScreenshotResponse { request_id, data, width, height, error } => {
                                        if let Some(ref broker) = screenshot_broker {
                                            if let Some(err) = error {
                                                broker.resolve(&request_id, Err(err)).await;
                                            } else if let Some(b64) = data {
                                                match base64::Engine::decode(
                                                    &base64::engine::general_purpose::STANDARD,
                                                    &b64,
                                                ) {
                                                    Ok(png_bytes) => {
                                                        broker.resolve(&request_id, Ok(super::ScreenshotResult {
                                                            png_data: png_bytes,
                                                            width: width.unwrap_or(0),
                                                            height: height.unwrap_or(0),
                                                        })).await;
                                                    }
                                                    Err(e) => {
                                                        broker.resolve(&request_id, Err(format!("base64 decode: {e}"))).await;
                                                    }
                                                }
                                            } else {
                                                broker.resolve(&request_id, Err("No data in screenshot response".into())).await;
                                            }
                                        }
                                    }
                                }
                            } else {
                                // Raw input fallback
                                let _ = pty_manager.write(slot_id, &text).await;
                            }
                        }
                        Some(Ok(Message::Binary(data))) => {
                            let text = String::from_utf8_lossy(&data).to_string();
                            let _ = pty_manager.write(slot_id, &text).await;
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Err(e)) => {
                            warn!(?addr, slot_id, error = %e, "WebSocket error");
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        info!(?addr, slot_id, "Client disconnected from PTY");
        Ok(())
    }

    /// Handle /events WebSocket subscription — frontend EventBus bridge.
    ///
    /// Receives pre-serialized JSON strings from the daemon's frontend_event_consumer
    /// and forwards them to the connected browser client.
    async fn handle_events_subscription(
        addr: SocketAddr,
        ws_stream: tokio_tungstenite::WebSocketStream<TcpStream>,
        frontend_events_tx: Option<broadcast::Sender<String>>,
        db: Option<Arc<dyn crate::db::traits::MissionStore>>,
    ) -> anyhow::Result<()> {
        let tx = match frontend_events_tx {
            Some(tx) => tx,
            None => {
                let (mut ws_tx, _ws_rx) = ws_stream.split();
                let _ = ws_tx
                    .send(Message::Close(Some(close_frame(
                        4000,
                        "Frontend event stream not configured",
                    ))))
                    .await;
                warn!(?addr, "Frontend events not available");
                return Ok(());
            }
        };

        let mut rx = tx.subscribe();
        let (mut ws_tx, mut ws_rx) = ws_stream.split();

        let subscriber_id = format!(
            "frontend-events-{}-{}",
            addr,
            chrono::Utc::now().timestamp_millis()
        );
        info!(?addr, subscriber_id = %subscriber_id, "Client subscribing to EventBus stream");

        // Send connected message with latest seq from DB (for catch-up protocol)
        let latest_seq = match db.as_ref() {
            Some(d) => d.timeline_latest_seq().await.unwrap_or(0),
            None => 0,
        };
        let connected = serde_json::json!({
            "type": "connected",
            "ts": chrono::Utc::now().timestamp_millis(),
            "seq": latest_seq,
        });
        let _ = send_json(&mut ws_tx, &connected).await;

        // Ping keepalive every 15 seconds
        let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(15));
        ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut consecutive_lags: u32 = 0;
        let mut last_client_seq = latest_seq;

        loop {
            tokio::select! {
                event = rx.recv() => {
                    match event {
                        Ok(json_str) => {
                            consecutive_lags = 0;
                            if let Ok(event) = serde_json::from_str::<serde_json::Value>(&json_str) {
                                if let Some(seq) = event.get("seq").and_then(|v| v.as_i64()) {
                                    if seq >= 0 {
                                        last_client_seq = last_client_seq.max(seq);
                                    }
                                }
                            }
                            if ws_tx.send(Message::Text(json_str)).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            consecutive_lags += 1;
                            let latest_seq = match db.as_ref() {
                                Some(d) => d.timeline_latest_seq().await.unwrap_or(0),
                                None => 0,
                            };
                            let lag_class = if consecutive_lags == 1 && latest_seq <= n as i64 {
                                "startup_catchup"
                            } else if consecutive_lags >= 3 {
                                "slow_subscriber"
                            } else {
                                "event_burst"
                            };
                            let cursor_lag = latest_seq.saturating_sub(last_client_seq);
                            let resync = serde_json::json!({
                                "type": "resync",
                                "schema": "missiond.eventbus-live-lag-diagnostic.v1",
                                "ts": chrono::Utc::now().timestamp_millis(),
                                "seq": latest_seq,
                                "subscriber_id": subscriber_id,
                                "missed": n,
                                "latest_seq": latest_seq,
                                "last_client_seq": last_client_seq,
                                "cursor_lag": cursor_lag,
                                "lag_class": lag_class,
                                "consecutive_lags": consecutive_lags,
                                "classification": {
                                    "startup_catchup": lag_class == "startup_catchup",
                                    "slow_subscriber": lag_class == "slow_subscriber",
                                    "event_burst": lag_class == "event_burst"
                                },
                                "diagnostic": "Frontend EventBus subscriber lagged; client should request bounded sync and MissionD should surface this as observability, not silent log noise.",
                            });
                            let _ = send_json(&mut ws_tx, &resync).await;
                            if consecutive_lags >= 3 {
                                warn!(?addr, subscriber_id = %subscriber_id, missed = n, latest_seq, "Events client too slow, disconnecting");
                                let _ = ws_tx
                                    .send(Message::Close(Some(close_frame(4008, "Too slow"))))
                                    .await;
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }

                _ = ping_interval.tick() => {
                    if ws_tx.send(Message::Ping(vec![])).await.is_err() {
                        break;
                    }
                }

                msg = ws_rx.next() => {
                    match msg {
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Ok(Message::Pong(_))) => {} // keepalive ack
                        Some(Ok(Message::Text(text))) => {
                            // Handle client sync request: { "action": "sync", "since_seq": N }
                            if let Ok(req) = serde_json::from_str::<serde_json::Value>(&text) {
                                if req.get("action").and_then(|v| v.as_str()) == Some("sync") {
                                    let since_seq = req.get("since_seq").and_then(|v| v.as_i64()).unwrap_or(0);
                                    last_client_seq = last_client_seq.max(since_seq);
                                    if let Some(ref db) = db {
                                        Self::handle_catch_up(&mut ws_tx, db, since_seq).await;
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => {
                            warn!(?addr, error = %e, "Events WS error");
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        info!(?addr, "Client unsubscribed from EventBus stream");
        Ok(())
    }

    /// Handle catch-up: replay historical events from DB since a given seq.
    async fn handle_catch_up(
        ws_tx: &mut futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<TcpStream>,
            Message,
        >,
        db: &Arc<dyn crate::db::traits::MissionStore>,
        since_seq: i64,
    ) {
        let latest = db.timeline_latest_seq().await.unwrap_or(0);
        let gap = latest - since_seq;

        if gap > 1000 {
            let msg = serde_json::json!({
                "type": "too_far_behind",
                "ts": chrono::Utc::now().timestamp_millis(),
                "gap": gap,
                "latest_seq": latest,
            });
            let _ = send_json(ws_tx, &msg).await;
            return;
        }

        if gap <= 0 {
            let msg = serde_json::json!({
                "type": "caught_up",
                "ts": chrono::Utc::now().timestamp_millis(),
                "seq": latest,
            });
            let _ = send_json(ws_tx, &msg).await;
            return;
        }

        match db.query_timeline_since(since_seq, 1000).await {
            Ok(rows) => {
                for row in &rows {
                    let payload: serde_json::Value =
                        serde_json::from_str(&row.payload).unwrap_or(serde_json::json!({}));
                    let ts =
                        chrono::NaiveDateTime::parse_from_str(&row.created_at, "%Y-%m-%d %H:%M:%S")
                            .map(|dt| dt.and_utc().timestamp_millis())
                            .unwrap_or(0);
                    let event = serde_json::json!({
                        "type": row.event_type,
                        "ts": ts,
                        "seq": row.seq,
                        "trace_id": row.trace_id,
                        "span_id": row.span_id,
                        "parent_span_id": row.parent_span_id,
                        "payload": payload,
                    });
                    if ws_tx.send(Message::Text(event.to_string())).await.is_err() {
                        return;
                    }
                }
                let caught_up = serde_json::json!({
                    "type": "caught_up",
                    "ts": chrono::Utc::now().timestamp_millis(),
                    "seq": rows.last().map(|r| r.seq).unwrap_or(latest),
                });
                let _ = send_json(ws_tx, &caught_up).await;
            }
            Err(e) => {
                warn!(error = %e, "Timeline catch-up query failed");
                let msg = serde_json::json!({
                    "type": "caught_up",
                    "ts": chrono::Utc::now().timestamp_millis(),
                    "seq": latest,
                });
                let _ = send_json(ws_tx, &msg).await;
            }
        }
    }

    async fn handle_tasks_subscription(
        addr: SocketAddr,
        ws_stream: tokio_tungstenite::WebSocketStream<TcpStream>,
        cc_tasks_watcher: Option<Arc<Mutex<CCTasksWatcher>>>,
    ) -> anyhow::Result<()> {
        let watcher = match cc_tasks_watcher {
            Some(w) => w,
            None => {
                let (mut ws_tx, _ws_rx) = ws_stream.split();
                let _ = ws_tx
                    .send(Message::Close(Some(close_frame(
                        4000,
                        "CC Tasks watcher not available",
                    ))))
                    .await;
                warn!(?addr, "CC Tasks watcher not available");
                return Ok(());
            }
        };

        let (mut ws_tx, mut ws_rx) = ws_stream.split();

        info!(?addr, "Client subscribing to Tasks events");

        // Send snapshot + overview on connect (supports both legacy and current dashboard protocols).
        let (sessions, overview) = {
            let guard = watcher.lock().await;
            let sessions = guard.get_active_sessions().await;
            let overview = guard.get_overview().await;
            (sessions, overview)
        };
        let snapshot_msg = TasksEventMessage::CcTasksSnapshot { sessions };
        let _ = send_json(&mut ws_tx, &snapshot_msg).await;
        let overview_msg = TasksEventMessage::CcTasksOverview { payload: overview };
        let _ = send_json(&mut ws_tx, &overview_msg).await;

        // Subscribe to watcher events
        let mut events_rx = watcher.lock().await.subscribe();

        loop {
            tokio::select! {
                event = events_rx.recv() => {
                    let event = match event {
                        Ok(e) => e,
                        Err(_) => break,
                    };

                    let msg = match event {
                        WatcherEvent::TasksChanged(e) => {
                            let changed_msg = TasksEventMessage::CcTasksChanged { payload: e };
                            if send_json(&mut ws_tx, &changed_msg).await.is_err() {
                                break;
                            }

                            // Keep legacy clients in sync without requiring protocol awareness.
                            let sessions = {
                                let guard = watcher.lock().await;
                                guard.get_active_sessions().await
                            };
                            let snapshot_msg = TasksEventMessage::CcTasksSnapshot { sessions };
                            if send_json(&mut ws_tx, &snapshot_msg).await.is_err() {
                                break;
                            }
                            continue;
                        }
                        WatcherEvent::TaskStarted { session, task } => TasksEventMessage::CcTaskStarted {
                            payload: TaskEventPayload {
                                session_id: session.session_id,
                                project_name: session.project_name,
                                task,
                            }
                        },
                        WatcherEvent::TaskCompleted { session, task } => TasksEventMessage::CcTaskCompleted {
                            payload: TaskEventPayload {
                                session_id: session.session_id,
                                project_name: session.project_name,
                                task,
                            }
                        },
                        WatcherEvent::SessionActive(session) => TasksEventMessage::CcSessionActive {
                            payload: SessionEventPayload {
                                session_id: session.session_id,
                                project_name: session.project_name,
                                summary: Some(session.summary),
                            }
                        },
                        WatcherEvent::SessionInactive(session) => TasksEventMessage::CcSessionInactive {
                            payload: SessionEventPayload {
                                session_id: session.session_id,
                                project_name: session.project_name,
                                summary: None,
                            }
                        },
                        // NewMessages/NewEvents are handled by the daemon, not the WS server
                        WatcherEvent::NewMessages { .. } | WatcherEvent::NewEvents { .. } => continue,
                    };

                    if send_json(&mut ws_tx, &msg).await.is_err() {
                        break;
                    }
                }

                msg = ws_rx.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if matches!(
                                serde_json::from_str::<TasksInMessage>(&text),
                                Ok(TasksInMessage::GetTasks)
                            ) {
                                let (sessions, overview) = {
                                    let guard = watcher.lock().await;
                                    let sessions = guard.get_active_sessions().await;
                                    let overview = guard.get_overview().await;
                                    (sessions, overview)
                                };
                                let snapshot_msg = TasksEventMessage::CcTasksSnapshot { sessions };
                                if send_json(&mut ws_tx, &snapshot_msg).await.is_err() {
                                    break;
                                }
                                let overview_msg = TasksEventMessage::CcTasksOverview { payload: overview };
                                if send_json(&mut ws_tx, &overview_msg).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Err(e)) => {
                            warn!(?addr, error = %e, "WebSocket error");
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        info!(?addr, "Client unsubscribed from Tasks events");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn jarvis_intent_plan_confirmation_is_opt_in() {
        let _guard = ENV_TEST_LOCK.lock().unwrap();
        std::env::remove_var("MISSIOND_JARVIS_REQUIRE_CONFIRMATION");
        std::env::remove_var("MISSIOND_JARVIS_INTENT_PLAN_CONFIRMATION");
        assert!(!jarvis_intent_plan_confirmation_required());

        std::env::set_var("MISSIOND_JARVIS_REQUIRE_CONFIRMATION", "1");
        assert!(jarvis_intent_plan_confirmation_required());
        std::env::remove_var("MISSIOND_JARVIS_REQUIRE_CONFIRMATION");
    }

    #[test]
    fn jarvis_artifact_projection_delta_is_opt_in() {
        let _guard = ENV_TEST_LOCK.lock().unwrap();
        std::env::remove_var("MISSIOND_JARVIS_ARTIFACT_PROJECTION_OPENAI_DELTA");
        assert!(!jarvis_artifact_projection_openai_delta_enabled());

        std::env::set_var("MISSIOND_JARVIS_ARTIFACT_PROJECTION_OPENAI_DELTA", "true");
        assert!(jarvis_artifact_projection_openai_delta_enabled());
        std::env::remove_var("MISSIOND_JARVIS_ARTIFACT_PROJECTION_OPENAI_DELTA");
    }

    #[test]
    fn jarvis_workspace_trust_selection_requires_selected_option() {
        assert_eq!(
            PTYWebSocketServer::jarvis_workspace_trust_selection(
                "  > Yes, I trust this folder\n    No, exit"
            ),
            "trust"
        );
        assert_eq!(
            PTYWebSocketServer::jarvis_workspace_trust_selection(
                "    Yes, I trust this folder\n  > No, exit"
            ),
            "exit"
        );
        assert_eq!(
            PTYWebSocketServer::jarvis_workspace_trust_selection(
                "    Yes, I trust this folder\n    No, exit"
            ),
            "unknown"
        );
    }

    fn test_key_judgment_ref() -> JarvisKeyJudgmentArtifactRef {
        JarvisKeyJudgmentArtifactRef {
            artifact_id: "key-judgment-test".to_string(),
            artifact_hash: Some("hash-key-judgment-test".to_string()),
            artifact_path: Some("shared-artifact://key-judgment-test".to_string()),
            judgment: "不是算力差异，是用量差异".to_string(),
            review_text: Some("关键判断用于 plan 拆分。".to_string()),
            confidence: Some("high".to_string()),
            rejected_hypotheses: vec!["算力不足是主因".to_string()],
            evidence_refs: vec!["grounding-report#usage".to_string()],
            planning_implications: vec!["先查用量与配额，再安排代码修改。".to_string()],
            acceptance_focus: vec!["验收用量判断是否被证据支持。".to_string()],
        }
    }

    fn test_plan_atomization_graph() -> serde_json::Value {
        serde_json::json!({
            "schema": "missiond.plan-atomization-graph.v1",
            "assignment_policy": PTYWebSocketServer::jarvis_assignment_policy_default(),
            "workstreams": [{
                "id": "ws1",
                "title": "调查",
                "objective": "查询用量证据",
                "execution_order": "serial",
                "depends_on": [],
                "parallel_group": null,
                "atoms": [{
                    "atom_task_id": "a1",
                    "workstream_id": "ws1",
                    "objective": "查询用量证据",
                    "category": "query",
                    "assignee_engine": "claude_code",
                    "execution_order": "serial",
                    "depends_on": [],
                    "parallel_group": null,
                    "read_scope": [],
                    "write_scope": [],
                    "acceptance": ["证据可追溯"]
                }]
            }],
            "atom_tasks": [{
                "atom_task_id": "a1",
                "workstream_id": "ws1",
                "objective": "查询用量证据",
                "category": "query",
                "assignee_engine": "claude_code",
                "execution_order": "serial",
                "depends_on": [],
                "parallel_group": null,
                "read_scope": [],
                "write_scope": [],
                "acceptance": ["证据可追溯"]
            }],
            "dependency_edges": [],
            "serial_groups": [{"id": "sg1", "atoms": ["a1"]}],
            "parallel_groups": []
        })
    }

    #[test]
    fn auth_event_webhook_accepts_default_auth_service_id() {
        let event = parse_external_service_webhook(
            r#"{"event_id":"auth_evt_1","event_kind":"login_succeeded","summary":"login ok"}"#,
            "auth",
            false,
        )
        .unwrap();
        match event {
            SystemEvent::ExternalServiceEvent {
                service_id,
                event_id,
                event_kind,
                ..
            } => {
                assert_eq!(service_id, "auth");
                assert_eq!(event_id, "auth_evt_1");
                assert_eq!(event_kind, "login_succeeded");
            }
            _ => panic!("unexpected event"),
        }
    }

    #[test]
    fn deploy_center_event_requires_stable_event_id_and_preserves_envelope() {
        assert!(parse_external_service_webhook(
            r#"{"event_kind":"deploy_succeeded"}"#,
            "deploy-center",
            true
        )
        .is_none());

        let event = parse_external_service_webhook(
            r#"{
              "schema_version":"missiond.event-envelope.v1",
              "event_id":"deploy-center:deploy_events:42",
              "source":"deploy-center",
              "project_id":"auth",
              "service_id":"deploy-center",
              "event_kind":"deploy_succeeded",
              "correlation_id":"session-1",
              "payload":{"deploy_event_id":42}
            }"#,
            "deploy-center",
            true,
        )
        .unwrap();
        match event {
            SystemEvent::ExternalServiceEvent {
                service_id,
                event_id,
                payload_json,
                ..
            } => {
                assert_eq!(service_id, "deploy-center");
                assert_eq!(event_id, "deploy-center:deploy_events:42");
                let payload: serde_json::Value = serde_json::from_str(&payload_json).unwrap();
                assert_eq!(payload["deploy_event_id"], 42);
                assert_eq!(payload["_envelope"]["project_id"], "auth");
                assert_eq!(payload["_envelope"]["correlation_id"], "session-1");
            }
            _ => panic!("unexpected event"),
        }
    }

    #[test]
    fn webhook_token_optional_or_required() {
        let headers =
            "POST /webhooks/auth-event HTTP/1.1\r\nX-MissionD-Webhook-Token: secret\r\n\r\n";
        assert!(webhook_token_matches(headers, None));
        assert!(webhook_token_matches(headers, Some("secret")));
        assert!(!webhook_token_matches(headers, Some("wrong")));
        assert!(!webhook_token_matches(
            "POST / HTTP/1.1\r\n\r\n",
            Some("secret")
        ));
    }

    #[test]
    fn compiled_abi_freshness_accepts_matching_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("compiled-contract-abi.json"),
            serde_json::json!({
                "schema_version": crate::v3_contracts::SCHEMA_VERSION,
                "source_hash": crate::v3_contracts::SOURCE_HASH,
                "diagnostics": [],
                "payload": {}
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("compiled-runtime-config.json"),
            serde_json::json!({
                "schema_version": "missiond.compiled-runtime-config.v1",
                "source_hash": crate::v3_contracts::SOURCE_HASH,
                "diagnostics": [],
                "payload": {}
            })
            .to_string(),
        )
        .unwrap();

        let check = PTYWebSocketServer::compiled_abi_freshness_check_in_dir(dir.path());
        assert_eq!(check["ok"], true);
        assert_eq!(check["status"], "ok");
    }

    #[test]
    fn compiled_abi_freshness_fails_closed_on_hash_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("compiled-contract-abi.json"),
            serde_json::json!({
                "schema_version": crate::v3_contracts::SCHEMA_VERSION,
                "source_hash": crate::v3_contracts::SOURCE_HASH,
                "diagnostics": [],
                "payload": {}
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("compiled-runtime-config.json"),
            serde_json::json!({
                "schema_version": "missiond.compiled-runtime-config.v1",
                "source_hash": "stale-runtime-config-hash",
                "diagnostics": [],
                "payload": {}
            })
            .to_string(),
        )
        .unwrap();

        let check = PTYWebSocketServer::compiled_abi_freshness_check_in_dir(dir.path());
        assert_eq!(check["ok"], false);
        assert_eq!(check["status"], "abi_freshness_mismatch");
        assert_eq!(
            check["failed"][0],
            "compiled-runtime-config:source_hash_mismatch"
        );
    }

    #[test]
    fn jarvis_task_wait_budget_is_bounded_for_mobile_clients() {
        assert_eq!(clamp_jarvis_task_wait_secs(None), 180);
        assert_eq!(clamp_jarvis_task_wait_secs(Some(1)), 15);
        assert_eq!(clamp_jarvis_task_wait_secs(Some(120)), 120);
        assert_eq!(clamp_jarvis_task_wait_secs(Some(900)), 300);
    }

    #[test]
    fn jarvis_public_stream_budget_is_bounded_for_proxy_routes() {
        assert_eq!(clamp_jarvis_public_stream_budget_secs(None), 6);
        assert_eq!(clamp_jarvis_public_stream_budget_secs(Some(1)), 2);
        assert_eq!(clamp_jarvis_public_stream_budget_secs(Some(90)), 90);
        assert_eq!(clamp_jarvis_public_stream_budget_secs(Some(900)), 240);
    }

    #[test]
    fn jarvis_db_poll_timeout_budget_is_bounded() {
        assert_eq!(clamp_jarvis_db_poll_timeout_secs(None), 8);
        assert_eq!(clamp_jarvis_db_poll_timeout_secs(Some(1)), 2);
        assert_eq!(clamp_jarvis_db_poll_timeout_secs(Some(12)), 12);
        assert_eq!(clamp_jarvis_db_poll_timeout_secs(Some(90)), 30);
    }

    #[test]
    fn jarvis_visible_heartbeat_budget_is_bounded() {
        assert_eq!(clamp_jarvis_visible_heartbeat_secs(None), 10);
        assert_eq!(clamp_jarvis_visible_heartbeat_secs(Some(1)), 3);
        assert_eq!(clamp_jarvis_visible_heartbeat_secs(Some(12)), 12);
        assert_eq!(clamp_jarvis_visible_heartbeat_secs(Some(90)), 30);
    }

    #[test]
    fn jarvis_communication_preference_signal_detects_style_requests() {
        assert!(PTYWebSocketServer::jarvis_communication_preference_signal(
            "以后沟通风格更直接一点，不要废话"
        ));
        assert!(PTYWebSocketServer::jarvis_communication_preference_signal(
            "沟通官应该记住我的偏好：先给结论，再给依据"
        ));
        assert!(!PTYWebSocketServer::jarvis_communication_preference_signal(
            "现在查一下 MissionD 的状态"
        ));
    }

    #[test]
    fn jarvis_communicator_prompt_includes_preference_lisp_as_read_only_context() {
        let prompt = PTYWebSocketServer::build_jarvis_communicator_prompt(
            "plan_archived",
            "告诉用户当前计划",
            &serde_json::json!({"plan_artifact_id": "plan-1"}),
            Some(Path::new("/tmp/communication-preferences.lisp")),
            true,
            "(jarvis-communication-preferences :schema \"missiond.jarvis-communication-preferences.v1\")",
        );
        assert!(prompt.contains("communication_preferences_lisp"));
        assert!(prompt.contains("missiond.jarvis-communication-preferences.v1"));
        assert!(prompt.contains("must_not_write_file"));
        assert!(prompt.contains("read-only-prompt-context"));
        assert!(prompt.contains("风格参考"));
    }

    #[test]
    fn jarvis_communication_preference_observation_is_redacted_and_appended() {
        let _guard = ENV_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prefs_path = dir.path().join("communication-preferences.lisp");
        std::env::set_var(
            "MISSIOND_JARVIS_COMMUNICATION_PREFERENCES_FILE",
            &prefs_path,
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let observation_id = runtime
            .block_on(
                PTYWebSocketServer::persist_jarvis_communication_preference_observation(
                    "communicator",
                    "以后沟通风格更简洁，token=sk_test_1234567890 不要展示",
                    Some("interaction-1"),
                    "chat-1",
                    "agy",
                    "slot-agy-gemini-31-pro-high-jarvis-communicator-a",
                    Some("Gemini 3.1 Pro (High)"),
                ),
            )
            .unwrap()
            .unwrap();
        std::env::remove_var("MISSIOND_JARVIS_COMMUNICATION_PREFERENCES_FILE");

        let content = std::fs::read_to_string(&prefs_path).unwrap();
        assert!(content.contains(&observation_id));
        assert!(content.contains("preference-observation"));
        assert!(content.contains("<redacted:"));
        assert!(!content.contains("sk_test_1234567890"));
    }

    #[test]
    fn jarvis_stage_errors_have_standard_phase_codes() {
        assert_eq!(
            PTYWebSocketServer::jarvis_phase_error_code("intent_artifact"),
            "JARVIS_INTENT_FAILED"
        );
        assert_eq!(
            PTYWebSocketServer::jarvis_phase_error_code("confirmation_key_judgment"),
            "JARVIS_KEY_JUDGMENT_FAILED"
        );
        assert_eq!(
            PTYWebSocketServer::jarvis_phase_error_code("key_judgment_authoring_failed"),
            "JARVIS_KEY_JUDGMENT_AUTHOR_FAILED"
        );
        assert_eq!(
            PTYWebSocketServer::jarvis_phase_error_code("plan_authoring_failed"),
            "JARVIS_PLAN_AUTHOR_FAILED"
        );
        assert_eq!(
            PTYWebSocketServer::jarvis_phase_error_code("grounded_direct_answer"),
            "JARVIS_DIRECT_ANSWER_FAILED"
        );
        assert_eq!(
            PTYWebSocketServer::jarvis_phase_error_code("board_task_create"),
            "JARVIS_BOARD_DISPATCH_FAILED"
        );
    }

    #[test]
    fn jarvis_intent_author_timeout_budget_is_bounded() {
        assert_eq!(
            PTYWebSocketServer::clamp_jarvis_intent_author_timeout_secs(None),
            180
        );
        assert_eq!(
            PTYWebSocketServer::clamp_jarvis_intent_author_timeout_secs(Some(1)),
            30
        );
        assert_eq!(
            PTYWebSocketServer::clamp_jarvis_intent_author_timeout_secs(Some(90)),
            90
        );
        assert_eq!(
            PTYWebSocketServer::clamp_jarvis_intent_author_timeout_secs(Some(900)),
            300
        );
    }

    #[test]
    fn jarvis_plan_author_timeout_budget_is_bounded() {
        assert_eq!(
            PTYWebSocketServer::clamp_jarvis_plan_author_timeout_secs(None),
            180
        );
        assert_eq!(
            PTYWebSocketServer::clamp_jarvis_plan_author_timeout_secs(Some(1)),
            30
        );
        assert_eq!(
            PTYWebSocketServer::clamp_jarvis_plan_author_timeout_secs(Some(90)),
            90
        );
        assert_eq!(
            PTYWebSocketServer::clamp_jarvis_plan_author_timeout_secs(Some(900)),
            300
        );
    }

    #[test]
    fn jarvis_codex_intent_response_parses_from_wrapped_output() {
        let output = r#"Thinking...
{"recognized_objective":"修复 iOS intent 确认体验","intent_kind":"implementation","understanding":"用户要把 intent.lisp 改成可审阅且由 LLM 识别。","review_text":"目标: 修复确认体验\n边界: 只确认意图。","assumptions":["用户希望 Codex 参与"],"non_goals":["确认前创建 BoardTask"],"acceptance_signals":["看到完整 intent.lisp"],"confidence":"high"}
done"#;
        let parsed = PTYWebSocketServer::parse_codex_intent_response(output).unwrap();
        assert_eq!(parsed.intent_kind, "implementation");
        assert!(parsed.review_text.contains("目标"));
        assert_eq!(parsed.assumptions, vec!["用户希望 Codex 参与"]);
    }

    #[test]
    fn jarvis_author_response_accepts_numeric_confidence() {
        let output = r#"```json
{"recognized_objective":"确认 ROUTER 部署方式","intent_kind":"question","understanding":"用户要查明 ROUTER 当前部署链路。","review_text":"确认意图后继续 grounding。","assumptions":[],"non_goals":[],"acceptance_signals":["回答部署方式"],"confidence":0.85}
```"#;
        let parsed = PTYWebSocketServer::parse_codex_intent_response(output).unwrap();
        assert_eq!(parsed.confidence.as_deref(), Some("high"));

        let low = r#"{"judgment":"证据不足","review_text":"需要继续查证","confidence":0.2,"rejected_hypotheses":[],"evidence_refs":["grounding"],"planning_implications":["先查证"],"acceptance_focus":["证据闭环"]}"#;
        let parsed = PTYWebSocketServer::parse_codex_key_judgment_response(low).unwrap();
        assert_eq!(parsed.confidence.as_deref(), Some("low"));
    }

    #[test]
    fn jarvis_codex_key_judgment_response_parses_from_wrapped_output() {
        let output = r#"noise
{"judgment":"不是算力差异，是用量差异","review_text":"关键判断用于 plan 前提。","confidence":"high","rejected_hypotheses":["算力不足是主因"],"evidence_refs":["grounding-report#usage"],"planning_implications":["查询任务先于代码修改"],"acceptance_focus":["验收用量证据是否支撑判断"]}
done"#;
        let parsed = PTYWebSocketServer::parse_codex_key_judgment_response(output).unwrap();
        assert_eq!(parsed.judgment, "不是算力差异，是用量差异");
        assert_eq!(parsed.planning_implications.len(), 1);

        let incomplete = r#"{"judgment":"泛化判断","review_text":"缺少关键字段"}"#;
        assert!(PTYWebSocketServer::parse_codex_key_judgment_response(incomplete).is_err());
    }

    #[test]
    fn jarvis_codex_plan_response_parses_from_wrapped_output() {
        let output = r#"noise
{"objective":"修复 plan.lisp 作者链路","review_text":"目标: 修复 plan.lisp 作者链路\n边界: 确认 plan 后才创建 BoardTask。","execution_mode":"work_order","requires_board_task":true,"key_judgment":"不是算力差异，是用量差异","steps":["确认已批准 intent","生成 Codex-authored plan.lisp","等待用户确认 plan"],"answer_policy":"结果来自 task-result-artifact。","provider_hint":"codex-review-worker","boundary":"不创建 BoardTask，直到用户确认 plan。","assumptions":["intent 已确认"],"non_goals":["Rust deterministic plan"],"acceptance_signals":["plan 标出 Codex author"],"confidence":"high","workstreams":[{"id":"ws1","title":"查询","objective":"查询用量证据","execution_order":"serial","depends_on":[],"parallel_group":null,"atoms":[{"atom_task_id":"a1","workstream_id":"ws1","objective":"查询用量证据","category":"query","assignee_engine":"claude_code","execution_order":"serial","depends_on":[],"parallel_group":null,"read_scope":[],"write_scope":[],"acceptance":["证据可追溯"]}]}],"atom_tasks":[{"atom_task_id":"a1","workstream_id":"ws1","objective":"查询用量证据","category":"query","assignee_engine":"claude_code","execution_order":"serial","depends_on":[],"parallel_group":null,"read_scope":[],"write_scope":[],"acceptance":["证据可追溯"]}],"dependency_edges":[],"serial_groups":[{"id":"sg1","atoms":["a1"]}],"parallel_groups":[],"assignment_policy":{"query":"claude_code","code_change":"claude_code","deploy_ops":"claude_code","judgment":"codex","acceptance":"codex"}}
done"#;
        let parsed = PTYWebSocketServer::parse_codex_plan_response(output).unwrap();
        assert_eq!(parsed.objective, "修复 plan.lisp 作者链路");
        assert_eq!(parsed.execution_mode, "work_order");
        assert!(parsed.requires_board_task);
        assert_eq!(parsed.steps.len(), 3);
        assert_eq!(parsed.non_goals, vec!["Rust deterministic plan"]);
        assert_eq!(parsed.atom_tasks.len(), 1);
    }

    #[test]
    fn jarvis_codex_plan_response_rejects_inconsistent_execution_mode() {
        let direct_answer_with_task = r#"{
            "objective":"回答身份问题",
            "review_text":"直接回答",
            "execution_mode":"grounded_direct_answer",
            "requires_board_task":true,
            "steps":["基于 grounding 回答"],
            "answer_policy":"provider_box grounded-direct-answer",
            "provider_hint":"provider-box-codex",
            "boundary":"不改代码",
            "assumptions":[],
            "non_goals":[],
            "acceptance_signals":["回答完整"],
            "confidence":"high"
        }"#;
        assert!(PTYWebSocketServer::parse_codex_plan_response(direct_answer_with_task).is_err());

        let work_order_without_task = r#"{
            "objective":"修改代码",
            "review_text":"需要工位执行",
            "execution_mode":"work_order",
            "requires_board_task":false,
            "steps":["创建任务"],
            "answer_policy":"task-result-artifact",
            "provider_hint":"codex-code-worker",
            "boundary":"需要 accepted shard",
            "assumptions":[],
            "non_goals":[],
            "acceptance_signals":["测试通过"],
            "confidence":"high"
        }"#;
        assert!(PTYWebSocketServer::parse_codex_plan_response(work_order_without_task).is_err());
    }

    #[test]
    fn jarvis_codex_plan_response_rejects_unknown_atom_dependency() {
        let plan = r#"{
            "objective":"拆分任务",
            "review_text":"需要原子化派工",
            "execution_mode":"work_order",
            "requires_board_task":true,
            "key_judgment":"不是算力差异，是用量差异",
            "steps":["查询","修改","验收"],
            "answer_policy":"task-result-artifact",
            "provider_hint":"codex-review-worker",
            "boundary":"只按 atom 图派工",
            "assumptions":[],
            "non_goals":[],
            "acceptance_signals":["依赖有效"],
            "confidence":"high",
            "workstreams":[{"id":"ws1","title":"实现","objective":"实现","execution_order":"serial","depends_on":[],"parallel_group":null,"atoms":[]}],
            "atom_tasks":[
                {"atom_task_id":"a1","workstream_id":"ws1","objective":"查询","category":"query","assignee_engine":"claude_code","execution_order":"serial","depends_on":["missing"],"parallel_group":null,"read_scope":[],"write_scope":[],"acceptance":["证据可追溯"]}
            ],
            "dependency_edges":[],
            "serial_groups":[],
            "parallel_groups":[],
            "assignment_policy":{"query":"claude_code","code_change":"claude_code","deploy_ops":"claude_code","judgment":"codex","acceptance":"codex"}
        }"#;
        let err = PTYWebSocketServer::parse_codex_plan_response(plan)
            .expect_err("unknown atom dependency must be rejected")
            .to_string();
        assert!(err.contains("depends_on unknown atom missing"));
    }

    #[test]
    fn jarvis_authored_intent_lisp_records_codex_authority() {
        let config = JarvisIntentAuthorConfig::default();
        let draft = JarvisCodexIntentResponse {
            recognized_objective: "识别真实意图".to_string(),
            intent_kind: "design".to_string(),
            understanding: "用户要求 Codex CLI 参与 intent authoring。".to_string(),
            review_text: "目标: 识别真实意图".to_string(),
            assumptions: vec!["使用 GPT-5.5".to_string()],
            non_goals: vec!["Rust 自行猜测意图".to_string()],
            acceptance_signals: vec!["intent.lisp 标出 Codex author".to_string()],
            confidence: Some("high".to_string()),
        };
        let body = PTYWebSocketServer::jarvis_authored_intent_lisp_body(
            "missiond.interaction-intent-artifact.v1",
            &config,
            "jarvis",
            "原始消息",
            &draft,
            "context-gather:test",
            Some("topic-1"),
            Some("Jarvis intent"),
            &["source-a".to_string()],
            &interaction_media_context(&[]),
        );
        assert!(body.contains(":authority \"codex-cli-gpt-5.5-xhigh\""));
        assert!(body.contains(":semantic-author"));
        assert!(body.contains(":objective \"识别真实意图\""));
        assert!(body.contains(":non-goals [\"Rust 自行猜测意图\"]"));
    }

    #[test]
    fn jarvis_author_slot_ignores_stale_cross_provider_env_slot() {
        assert_eq!(
            PTYWebSocketServer::jarvis_text_only_slot_id(
                "codex_cli",
                Some("slot-agy-gemini-31-pro-high"),
                "slot-codex-intent-author",
            ),
            "slot-codex-intent-author"
        );
        assert_eq!(
            PTYWebSocketServer::jarvis_text_only_slot_id(
                "codex_cli",
                Some("slot-agy-gemini-31-pro-high"),
                "slot-agy-gemini-31-pro-high",
            ),
            "slot-codex-intent-author"
        );
        assert_eq!(
            PTYWebSocketServer::jarvis_text_only_slot_id(
                "codex_cli",
                Some("slot-codex-custom-author"),
                "slot-codex-intent-author",
            ),
            "slot-codex-custom-author"
        );
        assert_eq!(
            PTYWebSocketServer::jarvis_text_only_slot_id(
                "agy",
                Some("slot-agy-gemini-31-pro-high-jarvis-communicator-a"),
                "slot-agy-gemini-31-pro-high",
            ),
            "slot-agy-gemini-31-pro-high-jarvis-communicator-a"
        );
    }

    #[test]
    fn jarvis_authored_plan_lisp_records_codex_authority() {
        let config = JarvisPlanAuthorConfig::default();
        let draft = JarvisCodexPlanResponse {
            objective: "生成 Codex-authored plan".to_string(),
            review_text: "目标: 生成 Codex-authored plan".to_string(),
            execution_mode: "grounded_direct_answer".to_string(),
            requires_board_task: false,
            steps: vec![
                "确认 intent artifact".to_string(),
                "创建可审阅 plan draft".to_string(),
                "等待用户确认 plan".to_string(),
            ],
            direct_answer_draft: Some(
                "当前步骤：plan.lisp 已归档。\n结论：这是基于 grounding 的直接回答。".to_string(),
            ),
            answer_policy: Some("使用 provider_box 基于 grounding 直接回答。".to_string()),
            provider_hint: Some("provider-box-codex".to_string()),
            boundary: Some("确认 plan 后才创建 BoardTask。".to_string()),
            assumptions: vec!["intent 已确认".to_string()],
            non_goals: vec!["Rust 自行拼接 plan".to_string()],
            acceptance_signals: vec!["plan.lisp 标出 Codex author".to_string()],
            confidence: Some("high".to_string()),
            key_judgment: Some("不是算力差异，是用量差异".to_string()),
            workstreams: Vec::new(),
            atom_tasks: Vec::new(),
            dependency_edges: Vec::new(),
            serial_groups: Vec::new(),
            parallel_groups: Vec::new(),
            assignment_policy: PTYWebSocketServer::jarvis_assignment_policy_default(),
        };
        let key_judgment = test_key_judgment_ref();
        let atomization_graph = PTYWebSocketServer::jarvis_plan_atomization_graph(
            &draft,
            "context-gather:test",
            "interaction-intent-draft:test",
            &key_judgment,
        );
        let body = PTYWebSocketServer::jarvis_authored_plan_lisp_body(
            "missiond.interaction-plan-artifact.v1",
            &config,
            "jarvis",
            &draft,
            "context-gather:test",
            "interaction-intent-draft:test",
            &key_judgment,
            &atomization_graph,
            Some("topic-1"),
            Some("Jarvis plan"),
            &["source-a".to_string()],
        );
        assert!(body.contains(":authority \"codex-cli-gpt-5.5-xhigh\""));
        assert!(body.contains(":semantic-author"));
        assert!(body.contains(":slot-id \"slot-codex-plan-author\""));
        assert!(body.contains(":objective \"生成 Codex-authored plan\""));
        assert!(body.contains(":mode grounded-direct-answer"));
        assert!(body.contains(":requires-board-task false"));
        assert!(body.contains(":direct-answer-draft"));
        assert!(body.contains(":key-judgment-artifact-id \"key-judgment-test\""));
        assert!(body.contains(":atomization-json"));
        assert!(body.contains(":completion-authority interaction-result-artifact"));
        assert!(body.contains(":non-goals [\"Rust 自行拼接 plan\"]"));
    }

    #[test]
    fn jarvis_confirmation_accepts_top_level_and_wrapped_payloads() {
        let top_level = serde_json::json!({
            "missiond_intent_confirmed": true,
            "missiond_plan_confirmed": false
        });
        assert!(jarvis_confirm_bool(&top_level, "missiond_intent_confirmed"));
        assert!(!jarvis_confirm_bool(&top_level, "missiond_plan_confirmed"));

        let wrapped = serde_json::json!({
            "missiond_confirm": {
                "missiond_intent_confirmed": true
            }
        });
        assert!(jarvis_confirm_bool(&wrapped, "missiond_intent_confirmed"));

        let nested_payload = serde_json::json!({
            "missiond_confirm": {
                "confirm_payload": {
                    "missiond_plan_confirmed": true
                }
            }
        });
        assert!(jarvis_confirm_bool(
            &nested_payload,
            "missiond_plan_confirmed"
        ));
        assert_eq!(
            jarvis_confirm_string(
                &serde_json::json!({
                    "missiond_confirm": {
                        "confirm_payload": {
                            "missiond_objective": "  original objective  "
                        }
                    }
                }),
                "missiond_objective"
            )
            .as_deref(),
            Some("original objective")
        );
    }

    #[test]
    fn jarvis_confirmation_text_is_conservative() {
        assert!(PTYWebSocketServer::jarvis_text_confirms_pending_review(
            "确认意图"
        ));
        assert!(PTYWebSocketServer::jarvis_text_confirms_pending_review(
            "确认 plan"
        ));
        assert!(PTYWebSocketServer::jarvis_text_confirms_pending_review(
            "OK"
        ));
        assert!(!PTYWebSocketServer::jarvis_text_confirms_pending_review(
            "草案在哪里"
        ));
        assert!(!PTYWebSocketServer::jarvis_text_confirms_pending_review(
            "不要确认"
        ));
    }

    #[test]
    fn jarvis_pending_confirmation_marker_round_trips_latest_payload() {
        let confirm = serde_json::json!({
            "phase": "awaiting_intent_confirmation",
            "confirmation_type": "intent",
            "confirm_payload": {
                "missiond_intent_confirmed": true,
                "missiond_objective": "修复 iOS 草案确认"
            }
        });
        let marker = PTYWebSocketServer::jarvis_pending_confirmation_marker("pending", &confirm);
        let history = vec![serde_json::json!({
            "role": "assistant",
            "content": marker
        })];
        let payload = PTYWebSocketServer::latest_pending_jarvis_confirmation(&history).unwrap();
        assert!(payload["missiond_intent_confirmed"].as_bool().unwrap());
        assert_eq!(payload["missiond_objective"], "修复 iOS 草案确认");

        let fulfilled =
            PTYWebSocketServer::jarvis_pending_confirmation_marker("fulfilled", &confirm);
        let history = vec![
            serde_json::json!({"role": "assistant", "content": history[0]["content"]}),
            serde_json::json!({"role": "assistant", "content": fulfilled}),
        ];
        assert!(PTYWebSocketServer::latest_pending_jarvis_confirmation(&history).is_none());
    }

    #[test]
    fn jarvis_pending_confirmation_injects_nested_confirm_payload() {
        let mut req = serde_json::json!({"messages": []});
        PTYWebSocketServer::inject_jarvis_confirm_payload(
            &mut req,
            serde_json::json!({
                "missiond_intent_confirmed": true,
                "missiond_objective": "原始目标"
            }),
        );
        assert!(jarvis_confirm_bool(&req, "missiond_intent_confirmed"));
        assert_eq!(
            jarvis_confirm_string(&req, "missiond_objective").as_deref(),
            Some("原始目标")
        );
    }

    #[test]
    fn jarvis_follow_request_is_detected_before_slot_readiness() {
        let top_level = serde_json::json!({
            "missiond_follow_task_id": " task-1 ",
            "messages": [{"role": "user", "content": "follow"}]
        });
        assert_eq!(
            openai_request_follow_task_id(&top_level).as_deref(),
            Some("task-1")
        );

        let nested = serde_json::json!({
            "missiond_follow": {"task_id": " task-2 "},
            "messages": [{"role": "user", "content": "follow"}]
        });
        assert_eq!(
            openai_request_follow_task_id(&nested).as_deref(),
            Some("task-2")
        );

        let metadata = serde_json::json!({
            "metadata": {"missiond_follow": {"missiond_follow_task_id": "task-3"}},
            "messages": [{"role": "user", "content": "follow"}]
        });
        assert_eq!(
            openai_request_follow_task_id(&metadata).as_deref(),
            Some("task-3")
        );
    }

    #[test]
    fn interaction_message_normalizes_string_and_object_payloads() {
        assert_eq!(
            normalize_interaction_message(&serde_json::json!("  hello missiond  ")),
            "hello missiond"
        );
        assert_eq!(
            normalize_interaction_message(&serde_json::json!({"text": "  from ios  "})),
            "from ios"
        );
        assert_eq!(
            normalize_interaction_message(&serde_json::json!({"content": "from wechat"})),
            "from wechat"
        );
    }

    #[test]
    fn interaction_confirmation_accepts_nested_confirm_payload() {
        let envelope = InteractionEnvelope {
            metadata: serde_json::json!({
                "missiond_confirm": {
                    "confirm_payload": {
                        "missiond_intent_confirmed": true,
                        "missiond_plan_confirmed": true,
                        "missiond_objective": "  original interaction objective  "
                    }
                }
            }),
            ..Default::default()
        };
        assert!(interaction_metadata_bool(
            &envelope,
            "missiond_intent_confirmed"
        ));
        assert!(interaction_metadata_bool(
            &envelope,
            "missiond_plan_confirmed"
        ));
        assert_eq!(
            interaction_metadata_string(&envelope, "missiond_objective").as_deref(),
            Some("original interaction objective")
        );
    }

    #[test]
    fn interaction_auth_requires_token_except_wechat_binding_path() {
        let web = InteractionEnvelope {
            channel: "ios".to_string(),
            ..Default::default()
        };
        assert!(verify_interaction_auth(&web, "POST / HTTP/1.1\r\n\r\n").is_err());

        let wechat = InteractionEnvelope {
            channel: "wechat".to_string(),
            ..Default::default()
        };
        assert_eq!(
            verify_interaction_auth(&wechat, "POST / HTTP/1.1\r\n\r\n")
                .unwrap()
                .as_deref(),
            None
        );
    }

    #[test]
    fn interaction_permission_context_uses_auth_userinfo_claims() {
        let envelope = InteractionEnvelope {
            channel: "ios".to_string(),
            external_user_id: Some("local-user".to_string()),
            metadata: serde_json::json!({
                "tenant_id": "metadata-tenant",
                "roles": ["metadata-admin"]
            }),
            ..Default::default()
        };
        let ctx = interaction_permission_context_from_userinfo(
            &envelope,
            &serde_json::json!({
                "sub": "auth-user",
                "tenant_id": "auth-tenant",
                "application_id": "auth-app",
                "product_id": "auth-product",
                "product_groups": ["operators"],
                "roles": ["tenant_admin"],
                "scope": "openid profile workflow:execute",
                "email": "user@example.com",
                "email_verified": true
            }),
            "https://auth.xiaojinpro.com/oidc/userinfo",
        );
        assert_eq!(ctx["resolution"], "auth-userinfo");
        assert_eq!(ctx["user_id"], "auth-user");
        assert_eq!(ctx["tenant_id"], "auth-tenant");
        assert_eq!(ctx["application_id"], "auth-app");
        assert_eq!(ctx["product_id"], "auth-product");
        assert_eq!(ctx["groups"][0], "operators");
        assert_eq!(ctx["subject"]["email"], "user@example.com");
        assert!(ctx["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "worker:dispatch"));
    }

    #[test]
    fn openai_chat_request_normalizes_to_interaction_envelope() {
        let req = serde_json::json!({
            "conversation_id": "conv-1",
            "user": "ios-user",
            "missiond_follow": {"task_id": "follow-task-1"},
            "metadata": {
                "missiond_confirm": {
                    "confirm_payload": {
                        "missiond_intent_confirmed": true,
                        "missiond_objective": "原始目标",
                        "missiond_intent_artifact_id": "intent-artifact-1",
                        "missiond_plan_artifact_id": "plan-artifact-1"
                    }
                }
            },
            "messages": [
                {"role": "system", "content": "ignore"},
                {"role": "assistant", "content": "old"},
                {"role": "user", "content": [
                    {"type": "text", "text": "请测试 MissionD"},
                    {"type": "text", "text": "并返回计划"},
                    {"type": "image_url", "image_url": {"url": "https://images.xiaojins.com/v1/images/img_test/content?token=secret-token", "detail": "auto"}}
                ]}
            ]
        });
        let envelope = openai_request_to_interaction_envelope(&req);
        assert_eq!(envelope.channel, "jarvis");
        assert_eq!(envelope.external_user_id.as_deref(), Some("ios-user"));
        assert_eq!(envelope.conversation_id.as_deref(), Some("conv-1"));
        assert!(normalize_interaction_message(&envelope.message).contains("请测试 MissionD"));
        assert_eq!(envelope.metadata["wire_format"], "openai-chat-completions");
        assert!(interaction_metadata_bool(
            &envelope,
            "missiond_intent_confirmed"
        ));
        assert_eq!(
            interaction_metadata_string(&envelope, "missiond_objective").as_deref(),
            Some("原始目标")
        );
        assert_eq!(
            interaction_metadata_string(&envelope, "missiond_intent_artifact_id").as_deref(),
            Some("intent-artifact-1")
        );
        assert_eq!(
            interaction_metadata_string(&envelope, "missiond_plan_artifact_id").as_deref(),
            Some("plan-artifact-1")
        );
        assert_eq!(
            interaction_metadata_string(&envelope, "missiond_follow_task_id").as_deref(),
            Some("follow-task-1")
        );
        assert_eq!(envelope.attachments.len(), 1);
        assert_eq!(
            envelope.attachments[0]["schema"],
            "missiond.interaction-media-attachment.v1"
        );
        assert_eq!(envelope.attachments[0]["status"], "accepted");
        assert_eq!(
            envelope.attachments[0]["transport"],
            "xjp-image-service-ref"
        );
        assert_eq!(envelope.attachments[0]["signed_url_present"], true);
        let serialized = serde_json::to_string(&envelope.attachments).unwrap();
        assert!(!serialized.contains("secret-token"));
        assert!(serialized.contains("?<redacted>"));
    }

    #[test]
    fn openai_inline_data_image_is_rejected_without_persisting_base64() {
        let req = serde_json::json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "看看这张图"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,VERY_SECRET_BASE64"}}
                ]}
            ]
        });
        let envelope = openai_request_to_interaction_envelope(&req);
        assert_eq!(envelope.attachments.len(), 1);
        assert_eq!(envelope.attachments[0]["status"], "rejected");
        assert_eq!(
            envelope.attachments[0]["transport"],
            "inline-data-url-rejected"
        );
        assert_eq!(
            envelope.attachments[0]["diagnostics"][0]["code"],
            "IMAGE_INLINE_DATA_URL_REQUIRES_XJP_UPLOAD"
        );
        let serialized = serde_json::to_string(&envelope.attachments).unwrap();
        assert!(!serialized.contains("VERY_SECRET_BASE64"));
        assert!(serialized.contains("data:<redacted>"));
    }

    #[test]
    fn public_jarvis_prefix_normalizes_to_daemon_routes() {
        assert_eq!(normalize_public_jarvis_path("/jarvis"), "/");
        assert_eq!(
            normalize_public_jarvis_path("/jarvis/api/monitor/jarvis"),
            "/api/monitor/jarvis"
        );
        assert_eq!(
            normalize_public_jarvis_path("/jarvis/v1/chat/completions"),
            "/v1/chat/completions"
        );
        assert_eq!(normalize_public_jarvis_path("/api/slots"), "/api/slots");
    }

    #[test]
    fn jarvis_sse_disconnect_errors_are_non_terminal() {
        for kind in [
            std::io::ErrorKind::BrokenPipe,
            std::io::ErrorKind::ConnectionReset,
            std::io::ErrorKind::ConnectionAborted,
            std::io::ErrorKind::NotConnected,
            std::io::ErrorKind::UnexpectedEof,
        ] {
            let error = std::io::Error::new(kind, "client went away");
            assert!(PTYWebSocketServer::is_client_disconnect_error(&error));
        }
        let error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "bad fd");
        assert!(!PTYWebSocketServer::is_client_disconnect_error(&error));
    }

    #[test]
    fn jarvis_follow_does_not_synthesize_task_result_from_board_summary() {
        let source = include_str!("./server.rs");
        let follow_body = source
            .split("async fn stream_jarvis_task_until_terminal")
            .nth(1)
            .and_then(|tail| tail.split("fn classify_jarvis_dispatch_verb").next())
            .expect("Jarvis follow body should remain present");
        assert!(
            !follow_body.contains(&format!("{}{}", "jarvis-board", "-summary-projection")),
            "Board summary notes must never be promoted to canonical task-result-artifacts"
        );
        assert!(
            !follow_body.contains(&format!("kind: \"{}\".to_string()", "task-result-artifact")),
            "Jarvis follow may read artifact hashes but must not create task-result-artifacts"
        );
        assert!(
            follow_body.contains("\"TASK_RESULT_ARTIFACT_REQUIRED\""),
            "Done-without-artifact must be surfaced as a typed diagnostic"
        );
    }

    #[test]
    fn jarvis_worker_prompt_prefers_materialized_context_file() {
        let metadata = PTYWebSocketServer::derive_jarvis_dispatch_contract(
            "请接入并验证 agy CLI",
            "context-gather:abc",
            Some("shared-artifact://abc"),
            Some("/tmp/missiond/context-gather/abc.json"),
            None,
            None,
            None,
            "intent-abc",
            "plan-abc",
            &test_key_judgment_ref(),
            &test_plan_atomization_graph(),
            "/repo",
        );
        let prompt =
            PTYWebSocketServer::build_jarvis_worker_prompt("请接入并验证 agy CLI", &metadata);
        assert!(prompt.contains("context_pack_file: /tmp/missiond/context-gather/abc.json"));
        assert!(prompt.contains("目标工位：engine_hint=agy pool_hint=agy-research"));
        assert!(prompt.contains("intent_artifact_id: intent-abc"));
        assert!(prompt.contains("plan_artifact_id: plan-abc"));
        assert!(prompt.contains("key_judgment_artifact_id: key-judgment-test"));
        assert!(prompt.contains("Plan atomization graph"));
        assert!(prompt.contains("Fixed assignment policy"));
        assert!(prompt.contains("已接受执行切片"));
        assert!(prompt.contains("先读取 grounding_report_file"));
        assert!(prompt.contains("再读取 context_pack_file"));
        assert!(prompt.contains("context unavailable"));
        assert!(prompt.contains("mission_shared_memory(action=\"artifact_get\", hash=\"abc\")"));
        assert!(prompt.contains("Task/Explore/TaskCreate/TaskUpdate/TaskList/TaskOutput"));
        assert!(prompt.contains("## Findings"));
        assert!(prompt.contains("## Evidence"));
        assert!(prompt.contains("## Recommendations"));
        assert!(prompt.contains("## Verification"));
        assert!(prompt.contains("task-result-artifact"));
        assert!(!prompt.contains("mission_board_update"));
    }

    #[test]
    fn jarvis_atom_worker_prompt_is_scoped_to_accepted_atom() {
        let graph = serde_json::json!({
            "schema": "missiond.plan-atomization-graph.v1",
            "assignment_policy": PTYWebSocketServer::jarvis_assignment_policy_default(),
            "workstreams": [{
                "id": "ws1",
                "title": "实现",
                "objective": "实现修复",
                "execution_order": "serial",
                "depends_on": [],
                "parallel_group": null,
                "atoms": []
            }],
            "atom_tasks": [
                {"atom_task_id":"a1","workstream_id":"ws1","objective":"查询证据","category":"query","assignee_engine":"claude_code","execution_order":"serial","depends_on":[],"parallel_group":null,"read_scope":[],"write_scope":[],"acceptance":["证据可追溯"]},
                {"atom_task_id":"a2","workstream_id":"ws1","objective":"修改 Jarvis gate","category":"code_change","assignee_engine":"claude_code","execution_order":"serial","depends_on":["a1"],"parallel_group":null,"read_scope":["/repo/crates"],"write_scope":["/repo/crates/missiond-core"],"acceptance":["测试通过"]}
            ],
            "dependency_edges": [{"from":"a1","to":"a2"}],
            "serial_groups": [{"id":"sg1","atoms":["a1","a2"]}],
            "parallel_groups": []
        });
        let base = PTYWebSocketServer::derive_jarvis_dispatch_contract(
            "立刻实施开发",
            "context-gather:abc",
            Some("shared-artifact://abc"),
            Some("/tmp/missiond/context-gather/abc.json"),
            Some("/tmp/missiond/context-gather/report.md"),
            None,
            Some("hash-report"),
            "intent-abc",
            "plan-abc",
            &test_key_judgment_ref(),
            &graph,
            "/repo",
        );
        let specs = PTYWebSocketServer::jarvis_plan_atom_specs(&graph).unwrap();
        let code_atom = specs
            .iter()
            .find(|atom| atom.atom_task_id == "a2")
            .expect("a2 atom");
        let child = PTYWebSocketServer::jarvis_atom_dispatch_metadata(
            code_atom,
            &base,
            &graph,
            "parent-task",
            &["task-a1".to_string()],
            false,
        );
        let prompt = PTYWebSocketServer::build_jarvis_worker_prompt("立刻实施开发", &child);
        assert!(prompt.contains("目标工位：engine_hint=claude_code pool_hint=claude-code-default"));
        assert!(prompt.contains("write_policy: scoped"));
        assert!(prompt.contains("atom_task_id: a2"));
        assert!(prompt.contains("accepted_atom_objective: 修改 Jarvis gate"));
        assert!(prompt.contains("\"depends_on\": [\n    \"a1\""));
        assert!(prompt.contains("Accepted plan atom"));
        assert!(prompt.contains("你只执行属于当前 BoardTask/worker 的切片"));
    }

    #[test]
    fn jarvis_dispatch_context_pack_parent_enters_read_scope() {
        let metadata = PTYWebSocketServer::derive_jarvis_dispatch_contract(
            "请接入并验证 agy CLI",
            "context-gather:abc",
            Some("shared-artifact://abc"),
            Some("/tmp/missiond/context-gather/abc.json"),
            None,
            None,
            None,
            "intent-abc",
            "plan-abc",
            &test_key_judgment_ref(),
            &test_plan_atomization_graph(),
            "/repo",
        );
        let read_scope = metadata["read_scope"].as_array().unwrap();
        assert_eq!(read_scope[0], "/repo");
        assert_eq!(read_scope[1], "/tmp/missiond/context-gather");
    }

    #[test]
    fn jarvis_dispatch_classifies_implementation_verbs_as_code() {
        let metadata = PTYWebSocketServer::derive_jarvis_dispatch_contract(
            "implement the new auth middleware",
            "context-gather:xyz",
            Some("shared-artifact://xyz"),
            Some("/tmp/ctx.json"),
            None,
            None,
            None,
            "intent-xyz",
            "plan-xyz",
            &test_key_judgment_ref(),
            &test_plan_atomization_graph(),
            "/repo",
        );
        assert_eq!(metadata["task_class"], "code");
        assert_eq!(metadata["write_policy"], "scoped");
        assert_eq!(metadata["engine_hint"], "claude_code");
        assert_eq!(metadata["pool_hint"], "claude-code-default");
        assert_eq!(metadata["task_kind"], "jarvis-grounded-implementation");
        let ws = metadata["write_scope"].as_array().unwrap();
        assert!(!ws.is_empty());
        assert_eq!(ws[0], "/repo");
    }

    #[test]
    fn jarvis_dispatch_codex_implementation_uses_codex_code_worker() {
        let metadata = PTYWebSocketServer::derive_jarvis_dispatch_contract(
            "请让 Codex 补齐 Autopilot final 选择并提交推送",
            "context-gather:codex",
            Some("shared-artifact://codex"),
            Some("/tmp/ctx.json"),
            None,
            None,
            None,
            "intent-codex",
            "plan-codex",
            &test_key_judgment_ref(),
            &test_plan_atomization_graph(),
            "/repo",
        );
        assert_eq!(metadata["task_class"], "code");
        assert_eq!(metadata["write_policy"], "scoped");
        assert_eq!(metadata["engine_hint"], "codex");
        assert_eq!(metadata["pool_hint"], "codex-code-worker");
        assert_eq!(metadata["task_kind"], "codex-grounded-implementation");
        assert_eq!(metadata["write_scope"][0], "/repo");
    }

    #[test]
    fn jarvis_dispatch_classifies_review_verbs_as_readonly() {
        let metadata = PTYWebSocketServer::derive_jarvis_dispatch_contract(
            "review the deployment pipeline",
            "context-gather:abc",
            None,
            None,
            None,
            None,
            None,
            "intent-abc",
            "plan-abc",
            &test_key_judgment_ref(),
            &test_plan_atomization_graph(),
            "/repo",
        );
        assert_eq!(metadata["task_class"], "review");
        assert_eq!(metadata["write_policy"], "read-only");
        assert_eq!(metadata["pool_hint"], "codex-review-worker");
        assert_eq!(
            metadata["completion_materialization_policy"],
            "autopilot_readonly_ok"
        );
        let ws = metadata["write_scope"].as_array().unwrap();
        assert!(ws.is_empty());
    }

    #[test]
    fn jarvis_dispatch_investigation_plan_stays_readonly_even_with_followup_fix_words() {
        let metadata = PTYWebSocketServer::derive_jarvis_dispatch_contract(
            "请调查当前实现并设计系统级实施方案，尽可能补齐缺失的 SSOT/checker/runtime 小闭环。",
            "context-gather:survey",
            Some("shared-artifact://survey"),
            Some("/tmp/ctx.json"),
            None,
            None,
            None,
            "intent-survey",
            "plan-survey",
            &test_key_judgment_ref(),
            &test_plan_atomization_graph(),
            "/repo",
        );
        assert_eq!(metadata["task_class"], "review");
        assert_eq!(metadata["write_policy"], "read-only");
        assert_eq!(metadata["engine_hint"], "codex");
        assert_eq!(metadata["pool_hint"], "codex-review-worker");
        let ws = metadata["write_scope"].as_array().unwrap();
        assert!(ws.is_empty());
    }

    #[test]
    fn jarvis_dispatch_readonly_constraints_override_commit_words() {
        let metadata = PTYWebSocketServer::derive_jarvis_dispatch_contract(
            "只读验证任务：请报告 Mac mini MissionD 状态。不要修改文件，不要提交。",
            "context-gather:readonly",
            Some("shared-artifact://readonly"),
            Some("/tmp/ctx.json"),
            None,
            None,
            None,
            "intent-readonly",
            "plan-readonly",
            &test_key_judgment_ref(),
            &test_plan_atomization_graph(),
            "/repo",
        );
        assert_eq!(metadata["task_class"], "review");
        assert_eq!(metadata["write_policy"], "read-only");
        assert_eq!(metadata["engine_hint"], "codex");
        assert_eq!(metadata["pool_hint"], "codex-review-worker");
        let ws = metadata["write_scope"].as_array().unwrap();
        assert!(ws.is_empty());
        let must_not_touch = metadata["must_not_touch"].as_array().unwrap();
        assert!(must_not_touch
            .iter()
            .any(|item| item.as_str() == Some("Do not modify files")));
    }

    #[test]
    fn jarvis_dispatch_no_commit_alone_does_not_hide_implementation_intent() {
        let metadata = PTYWebSocketServer::derive_jarvis_dispatch_contract(
            "请实现这个小修复，但不要提交。",
            "context-gather:code",
            Some("shared-artifact://code"),
            Some("/tmp/ctx.json"),
            None,
            None,
            None,
            "intent-code",
            "plan-code",
            &test_key_judgment_ref(),
            &test_plan_atomization_graph(),
            "/repo",
        );
        assert_eq!(metadata["task_class"], "code");
        assert_eq!(metadata["write_policy"], "scoped");
        assert_eq!(metadata["pool_hint"], "claude-code-default");
        assert_eq!(
            metadata["completion_materialization_policy"],
            "worker_artifact_required"
        );
        assert_eq!(metadata["write_scope"][0], "/repo");
    }

    #[test]
    fn jarvis_dispatch_implementation_prompt_uses_scoped_write_constraint() {
        let metadata = PTYWebSocketServer::derive_jarvis_dispatch_contract(
            "fix the broken auth flow",
            "ctx:abc",
            None,
            None,
            None,
            None,
            None,
            "i",
            "p",
            &test_key_judgment_ref(),
            &test_plan_atomization_graph(),
            "/repo",
        );
        let prompt =
            PTYWebSocketServer::build_jarvis_worker_prompt("fix the broken auth flow", &metadata);
        assert!(prompt.contains("工位实现任务"));
        assert!(prompt.contains("只在 write_scope 范围内修改文件"));
        assert!(!prompt.contains("不要修改文件"));
    }
}
