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
use std::collections::HashMap;
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
    pub artifact_hash: Option<String>,
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
    pub conversation_id: Option<String>,
    pub chat_id: String,
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
    /// Number of native MCP tools (injected into Jarvis system prompt)
    pub tool_count: usize,
    /// V3-projected default slot for OpenAI-compatible chat completions.
    pub default_chat_slot: String,
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
    tool_count: usize,
    default_chat_slot: String,
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
    req.get("messages")
        .and_then(|messages| messages.as_array())
        .and_then(|messages| {
            messages.iter().rev().find_map(|message| {
                let role = message.get("role").and_then(|value| value.as_str())?;
                if role != "user" {
                    return None;
                }
                let text = openai_content_to_text(message.get("content")?);
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            })
        })
        .unwrap_or_else(|| {
            req.get("prompt")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .unwrap_or_default()
                .to_string()
        })
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
        attachments: Vec::new(),
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

fn interaction_service_permission_context(envelope: &InteractionEnvelope) -> serde_json::Value {
    let channel = envelope.channel.trim().to_ascii_lowercase();
    serde_json::json!({
        "schema": "missiond.permission-context.v1",
        "authority": "auth",
        "resolution": "service-token",
        "user_id": envelope.external_user_id,
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
        "user_id": envelope.external_user_id,
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
            tool_count: options.tool_count,
            default_chat_slot: options.default_chat_slot,
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
        let tool_count = self.tool_count;
        let default_chat_slot = self.default_chat_slot.clone();

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
                                let tool_count = tool_count;
                                let default_chat_slot = default_chat_slot.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = Self::handle_connection(stream, addr, pty_manager, cc_tasks_watcher, screenshot_broker, jarvis_trace, incident_tx, system_event_tx, frontend_events_tx, db, context_enricher, jarvis_grounding, jarvis_artifact_writer, tool_count, default_chat_slot).await {
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
        let session_running = pty_manager.is_running(default_slot).await;
        let status = pty_manager.get_status(default_slot).await;
        let (readiness, reason, slot_state, recognition) = match status {
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

        serde_json::json!({
            "status": readiness,
            "default_slot": default_slot,
            "slot_state": slot_state,
            "session_running": session_running,
            "reason": reason,
            "recognition": recognition,
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

    /// Local-only control surface for deploy-center post-deploy smoke.
    ///
    /// Public `/jarvis/*` monitor paths stay read-only. The self-update lane calls
    /// this endpoint from Mac mini localhost after blue/green restart to restore
    /// the default Jarvis slot before judging readiness.
    async fn handle_jarvis_slot_ensure(
        mut stream: TcpStream,
        addr: SocketAddr,
        pty_manager: Arc<PTYManager>,
        default_slot: String,
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
        if before_status == "ready" {
            let body = serde_json::json!({
                "schema": "missiond.jarvis-slot-ensure.v1",
                "overall": "ready",
                "default_slot": default_slot,
                "readiness_before": before,
                "auto_heal": {
                    "status": "skipped",
                    "reason": "default slot already ready"
                },
                "checked_at": chrono::Utc::now().to_rfc3339(),
            });
            let response = Self::http_json_response(body.to_string());
            stream.write_all(response.as_bytes()).await?;
            stream.shutdown().await?;
            return Ok(());
        }
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

        let auto_heal = Self::maybe_auto_heal_jarvis_slot(&pty_manager, &default_slot).await;
        let after = Self::build_jarvis_readiness(&pty_manager, &default_slot).await;
        let after_status = after
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        let ok = after_status == "ready";
        let body = serde_json::json!({
            "schema": "missiond.jarvis-slot-ensure.v1",
            "overall": if ok { "ready" } else { "unavailable" },
            "default_slot": default_slot,
            "readiness_before": before,
            "readiness_after": after,
            "auto_heal": auto_heal,
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
            "status": if metadata.is_some() { "ok" } else { "missing" },
            "path": path,
            "size_bytes": metadata.as_ref().map(|m| m.len()),
            "modified_unix_secs": metadata
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs()),
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

    async fn handle_jarvis_monitor(
        mut stream: TcpStream,
        pty_manager: Arc<PTYManager>,
        default_slot: String,
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
        let compiled_runtime = std::env::var("MISSIOND_COMPILED_RUNTIME_DIR")
            .map(|dir| std::path::PathBuf::from(dir).join("compiled-runtime-config.json"))
            .or_else(|_| {
                std::env::var("MISSIOND_RUNTIME_DIR").map(|dir| {
                    std::path::PathBuf::from(dir)
                        .join("compiled")
                        .join("compiled-runtime-config.json")
                })
            })
            .unwrap_or_else(|_| {
                std::path::PathBuf::from(
                    ".missiond/v3/runtime/compiled/compiled-runtime-config.json",
                )
            });
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
            Self::file_check(
                "compiled-runtime-config",
                "Compiled V3 runtime config",
                compiled_runtime,
            ),
        ];

        let non_critical_failures = checks
            .iter()
            .filter(|check| check.get("ok").and_then(|v| v.as_bool()) == Some(false))
            .count();
        let overall = match readiness_status {
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
            "unavailable" => {
                "start the default Jarvis slot or inspect provider credentials/billing"
            }
            _ => "inspect /api/slots and daemon logs",
        };

        let body = serde_json::json!({
            "schema": "missiond.jarvis-chain-monitor.v1",
            "overall": overall,
            "recommended_action": recommended_action,
            "checked_at": chrono::Utc::now().to_rfc3339(),
            "public_endpoint": "/jarvis",
            "chat_endpoint": "/v1/chat/completions",
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
        })
        .to_string();

        let response = Self::http_json_response(body);
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await?;
        Ok(())
    }

    async fn handle_interaction_events(
        mut stream: TcpStream,
        request_line: &str,
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
        Self::write_sse_event(
            &mut stream,
            "status",
            &serde_json::json!({
                "schema": "missiond.interaction-event-stream.v1",
                "interaction_id": interaction_id,
                "phase": "event_stream_ready",
                "message": "Live interaction receive streams are authoritative in this release; durable replay is reached through BoardTask/result-artifact ids returned by the interaction."
            }),
        )
        .await?;
        Self::finish_sse(&mut stream).await?;
        Ok(())
    }

    async fn handle_interaction_messages(
        mut stream: TcpStream,
        addr: SocketAddr,
        jarvis_grounding: JarvisGroundingSlot,
        jarvis_artifact_writer: JarvisArtifactSlot,
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
            jarvis_grounding,
            jarvis_artifact_writer,
            db,
        )
        .await
    }

    async fn handle_chat_completions_interaction_adapter(
        mut stream: TcpStream,
        addr: SocketAddr,
        pty_manager: Option<Arc<PTYManager>>,
        default_chat_slot: String,
        jarvis_grounding: JarvisGroundingSlot,
        jarvis_artifact_writer: JarvisArtifactSlot,
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
            jarvis_grounding,
            jarvis_artifact_writer,
            db,
        )
        .await
    }

    async fn handle_interaction_envelope(
        mut stream: TcpStream,
        addr: SocketAddr,
        headers: String,
        envelope: InteractionEnvelope,
        jarvis_grounding: JarvisGroundingSlot,
        jarvis_artifact_writer: JarvisArtifactSlot,
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
        let raw_user_text = normalize_interaction_message(&envelope.message);
        if raw_user_text.is_empty() {
            let err = serde_json::json!({"error": {"message": "InteractionEnvelope.message text is required"}});
            Self::send_http_error(&mut stream, 400, "Bad Request", &err.to_string()).await?;
            return Ok(());
        }

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
            "remote_addr": addr.to_string(),
        });
        Self::write_sse_event(&mut stream, "received", &received).await?;
        Self::write_sse_event(
            &mut stream,
            "authenticated",
            &serde_json::json!({
                "interaction_id": interaction_id,
                "channel": channel,
                "authenticated": auth_token.is_some(),
                "authority": "auth",
            }),
        )
        .await?;
        Self::write_sse_event(
            &mut stream,
            "permission_resolved",
            &serde_json::json!({
                "interaction_id": interaction_id,
                "permission_context": permission_context.clone(),
            }),
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
                    &mut stream,
                    &chat_id,
                    &follow_task_id,
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
            match db.jarvis_get_or_create(conversation_id.as_deref()).await {
                Ok(id) => {
                    let _ = db
                        .router_chat_append_messages(
                            &id,
                            &[("user".to_string(), raw_user_text.clone())],
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
            Self::write_sse_event(
                &mut stream,
                "meta",
                &serde_json::json!({
                    "interaction_id": interaction_id,
                    "conversation_id": cid,
                    "chat_id": chat_id
                }),
            )
            .await?;
        }

        let intent_confirmed = interaction_metadata_bool(&envelope, "missiond_intent_confirmed");
        let plan_confirmed = interaction_metadata_bool(&envelope, "missiond_plan_confirmed");
        let objective_text = if intent_confirmed || plan_confirmed {
            match interaction_metadata_string(&envelope, "missiond_objective") {
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
        let grounding = match Self::gather_jarvis_grounding(
            &jarvis_grounding,
            JarvisGroundingRequest {
                query: objective_text.clone(),
                conversation_id: jarvis_conv_id.clone(),
                chat_id: chat_id.clone(),
                unknowns: vec![
                    format!("What does this {channel} channel user intend MissionD to do?"),
                    "Which project registry, SSOT, skill, infra, or tool facts are required before planning?".to_string(),
                    "What permissions and capabilities should this channel identity have?".to_string(),
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
        let grounding_context_id = grounding.grounding_context_id.clone();
        let context_pack_path = grounding.context_pack_path.clone();
        let context_pack_file = grounding.context_pack_file.clone();
        let sources_used = grounding.sources_used.clone();
        Self::write_sse_event(
            &mut stream,
            "grounding",
            &serde_json::json!({
                "interaction_id": interaction_id,
                "phase": "grounding",
                "grounding_context_id": grounding_context_id,
                "context_pack_path": context_pack_path,
                "context_pack_file": context_pack_file,
                "sources_used": sources_used,
                "diagnostics": grounding.diagnostics,
            }),
        )
        .await?;

        let intent_payload = serde_json::json!({
            "schema": "missiond.interaction-intent-artifact.v1",
            "interaction_id": interaction_id,
            "channel": channel,
            "phase": "intent_draft",
            "grounding_context_id": grounding_context_id,
            "permission_context": permission_context.clone(),
            "understanding": "我理解这是一个外部渠道请求，需要先确认 intent.lisp，再确认 plan.lisp，之后才创建 BoardTask 并派工位。",
            "objective": objective_text,
            "user_message_preview": objective_text.chars().take(240).collect::<String>(),
            "sources_used": sources_used,
            "requires_confirmation": true
        });
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
                    "channel": channel,
                    "conversation_id": jarvis_conv_id,
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
        if !intent_confirmed {
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
            Self::write_sse_event(&mut stream, "intent_draft", &intent).await?;
            Self::write_sse_event(
                &mut stream,
                "confirm_required",
                &serde_json::json!({
                    "interaction_id": interaction_id,
                    "phase": "awaiting_intent_confirmation",
                    "confirmation_type": "intent",
                    "confirm_payload": {
                        "missiond_intent_confirmed": true,
                        "missiond_objective": objective_text,
                        "missiond_grounding_context_id": grounding_context_id,
                        "missiond_intent_artifact_id": intent_artifact_id,
                    }
                }),
            )
            .await?;
            Self::write_sse_openai_text(
                &mut stream,
                &chat_id,
                "我已生成 intent.lisp 草案，等待你确认意图。",
                Some("stop"),
            )
            .await?;
            Self::finish_sse(&mut stream).await?;
            return Ok(());
        }

        let plan_payload = serde_json::json!({
            "schema": "missiond.interaction-plan-artifact.v1",
            "interaction_id": interaction_id,
            "channel": channel,
            "phase": "plan_draft",
            "grounding_context_id": grounding_context_id,
            "context_pack_path": context_pack_path,
            "context_pack_file": context_pack_file,
            "intent_artifact_id": intent_artifact_id,
            "objective": objective_text,
            "steps": [
                "按 PermissionContext 和 grounding evidence 确认可执行范围",
                "创建可追踪 BoardTask，并写入 grounding / intent / plan artifact ids",
                "由主控选择合适工位，不直接执行实现任务",
                "等待 task-result-artifact 后通过对应 channel response sink 返回结果"
            ],
            "requires_confirmation": true
        });
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
                    "channel": channel,
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
        if !plan_confirmed {
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
            Self::write_sse_event(&mut stream, "plan_draft", &plan).await?;
            Self::write_sse_event(
                &mut stream,
                "confirm_required",
                &serde_json::json!({
                    "interaction_id": interaction_id,
                    "phase": "awaiting_plan_confirmation",
                    "confirmation_type": "plan",
                    "confirm_payload": {
                        "missiond_intent_confirmed": true,
                        "missiond_plan_confirmed": true,
                        "missiond_objective": objective_text,
                        "missiond_grounding_context_id": grounding_context_id,
                        "missiond_intent_artifact_id": intent_artifact_id,
                        "missiond_plan_artifact_id": plan_artifact_id,
                    }
                }),
            )
            .await?;
            Self::write_sse_openai_text(
                &mut stream,
                &chat_id,
                "我已生成 plan.lisp 草案，等待你确认计划。",
                Some("stop"),
            )
            .await?;
            Self::finish_sse(&mut stream).await?;
            return Ok(());
        }

        let Some(ref db) = db else {
            Self::write_sse_event(
                &mut stream,
                "diagnostic",
                &serde_json::json!({
                    "interaction_id": interaction_id,
                    "phase": "board_task_create",
                    "error": {
                        "code": "MISSIOND_DB_UNAVAILABLE",
                        "message": "MissionD DB unavailable; cannot create BoardTask."
                    }
                }),
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
        let dispatch_metadata = Self::derive_jarvis_dispatch_contract(
            &objective_text,
            &grounding_context_id,
            context_pack_path.as_deref(),
            context_pack_file.as_deref(),
            &intent_artifact_id,
            &plan_artifact_id,
            &Self::jarvis_runtime_read_scope_root(),
        );
        let prompt_template = Self::build_jarvis_worker_prompt(&objective_text, &dispatch_metadata);
        let meta = serde_json::json!({
            "source": "interaction-gateway",
            "interaction_id": interaction_id,
            "channel": channel,
            "permission_context": permission_context.clone(),
            "grounding_context_id": grounding_context_id,
            "context_pack_path": context_pack_path,
            "context_pack_file": context_pack_file,
            "intent_artifact_id": intent_artifact_id,
            "plan_artifact_id": plan_artifact_id,
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
        match db.create_board_task(&task_input).await {
            Ok(task) => {
                let follow_payload = serde_json::json!({
                    "missiond_follow_task_id": task.id,
                    "interaction_id": interaction_id,
                    "stream": true
                });
                Self::write_sse_event(
                    &mut stream,
                    "board_task_created",
                    &serde_json::json!({
                        "interaction_id": interaction_id,
                        "task_id": task.id,
                        "title": task.title,
                        "grounding_context_id": grounding_context_id,
                        "intent_artifact_id": intent_artifact_id,
                        "plan_artifact_id": plan_artifact_id,
                    }),
                )
                .await?;
                Self::write_sse_event(
                    &mut stream,
                    "worker_dispatched",
                    &serde_json::json!({
                        "interaction_id": interaction_id,
                        "phase": "workers_running",
                        "task_id": task.id,
                        "slot_id": serde_json::Value::Null,
                        "dispatch_state": "pending_autopilot_claim",
                        "status": task.status.as_str(),
                        "terminal_task_result": false,
                        "follow_payload": follow_payload.clone(),
                        "message": "BoardTask is queued for Autopilot/provider claim; concrete slot attribution will arrive through follow-up supervision."
                    }),
                )
                .await?;
                Self::write_sse_event(
                    &mut stream,
                    "worker_status",
                    &serde_json::json!({
                        "interaction_id": interaction_id,
                        "phase": "workers_running",
                        "task_id": task.id,
                        "status": task.status.as_str(),
                        "terminal_task_result": false,
                    }),
                )
                .await?;
                Self::write_sse_event(
                    &mut stream,
                    "dispatch_accepted",
                    &serde_json::json!({
                        "interaction_id": interaction_id,
                        "phase": "board_tasks_created",
                        "task_id": task.id,
                        "terminal_task_result": false,
                        "follow_payload": follow_payload.clone(),
                        "message": "BoardTask was created and accepted for asynchronous worker dispatch; this is not a terminal task result."
                    }),
                )
                .await?;
                Self::write_sse_event(
                    &mut stream,
                    "result_pending",
                    &serde_json::json!({
                        "interaction_id": interaction_id,
                        "phase": "result_pending",
                        "task_id": task.id,
                        "terminal_task_result": false,
                        "follow_payload": follow_payload,
                    }),
                )
                .await?;
                Self::write_sse_openai_text(
                    &mut stream,
                    &chat_id,
                    &format!(
                        "计划已确认，BoardTask 已创建。后续用 missiond_follow_task_id={} 读取 task-result-artifact。",
                        task.id
                    ),
                    Some("stop"),
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
        stream
            .write_all(format!("event: {event}\ndata: {payload}\n\n").as_bytes())
            .await?;
        stream.flush().await?;
        Ok(())
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
        stream
            .write_all(format!("data: {chunk}\n\n").as_bytes())
            .await?;
        stream.flush().await?;
        Ok(())
    }

    async fn finish_sse(stream: &mut TcpStream) -> anyhow::Result<()> {
        stream.write_all(b"data: [DONE]\n\n").await?;
        stream.flush().await?;
        stream.shutdown().await?;
        Ok(())
    }

    async fn fail_jarvis_gate(
        stream: &mut TcpStream,
        message: impl Into<String>,
        stage: &str,
    ) -> anyhow::Result<()> {
        let message = message.into();
        let diagnostic = serde_json::json!({
            "phase": stage,
            "error": {"message": message},
            "next_action": "Fix the missing runtime capability instead of falling back to direct PTY execution."
        });
        Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
        Self::finish_sse(stream).await
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

    async fn put_jarvis_artifact(
        slot: &JarvisArtifactSlot,
        req: JarvisArtifactRequest,
    ) -> Result<JarvisArtifactResult, String> {
        let guard = slot.read().await;
        let Some(ref writer) = *guard else {
            return Err("Jarvis artifact writer is not configured".to_string());
        };
        let writer = Arc::clone(writer);
        drop(guard);
        writer(req).await.and_then(|result| {
            if result.artifact_id.trim().is_empty() || result.artifact_hash.trim().is_empty() {
                Err("Jarvis artifact writer returned an empty artifact id/hash".to_string())
            } else {
                Ok(result)
            }
        })
    }

    fn extract_task_result_artifact_hash(text: &str) -> Option<String> {
        let marker = "task_result_artifact:";
        let tail = text.split(marker).nth(1)?.trim();
        let trimmed = tail
            .trim_start_matches('`')
            .split(|c: char| c == '`' || c.is_whitespace())
            .next()
            .unwrap_or("")
            .trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    async fn stream_jarvis_task_until_terminal(
        db: &Arc<dyn crate::db::traits::MissionStore>,
        _jarvis_artifact_writer: &JarvisArtifactSlot,
        stream: &mut TcpStream,
        chat_id: &str,
        task_id: &str,
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
                Self::write_sse_openai_text(
                    stream,
                    chat_id,
                    "任务尚未在等待窗口内完成，我不会伪造结果。请检查 BoardTask、工位和 task-result-artifact。",
                    Some("stop"),
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
                    Self::write_sse_openai_text(
                            stream,
                            chat_id,
                            "任务监督链路读取 BoardTask 超时；我不会继续静默等待。请检查 MissionD DB / EventBus / worker completion 链路。",
                            Some("stop"),
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
                Self::write_sse_openai_text(
                    stream,
                    chat_id,
                    "任务记录丢失，无法继续监督执行。",
                    Some("stop"),
                )
                .await?;
                return Ok(());
            };

            let status = task.status.as_str().to_string();
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
                        let artifact_hash = Self::extract_task_result_artifact_hash(&content);
                        if is_summary {
                            latest_summary = Some(content.clone());
                        }
                        let artifact_hash = artifact_hash;
                        if artifact_hash.is_some() {
                            latest_artifact_hash = artifact_hash.clone();
                        }
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
                                    let artifact_hash =
                                        Self::extract_task_result_artifact_hash(&content);
                                    if artifact_hash.is_some() {
                                        latest_artifact_hash = artifact_hash.clone();
                                        let event = serde_json::json!({
                                            "task_id": task_id,
                                            "note_id": note.id,
                                            "note_type": note.note_type.as_str(),
                                            "author": note.author,
                                            "created_at": note.created_at,
                                            "artifact_hash": artifact_hash,
                                            "content": content.chars().take(12_000).collect::<String>(),
                                            "truncated": content.chars().count() > 12_000,
                                            "source": "jarvis-follow-artifact-reference",
                                        });
                                        Self::write_sse_event(stream, "result_artifact", &event)
                                            .await?;
                                    }
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
                                Self::write_sse_openai_text(
                                    stream,
                                    chat_id,
                                    "任务已完成，但结果 notes/task-result-artifact 读取超时；我不会伪造结果。请检查结果落盘链路后重试 follow。",
                                    Some("stop"),
                                )
                                .await?;
                                return Ok(());
                            }
                        }
                    }
                    if latest_summary.is_some() && latest_artifact_hash.is_none() {
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
                        Self::write_sse_openai_text(
                            stream,
                            chat_id,
                            "任务已完成但 task-result-artifact 未落盘；我不会把 Board note 当作最终结果返回。请先修复结果落盘链路。",
                            Some("stop"),
                        )
                        .await?;
                        return Ok(());
                    }
                    let final_text = latest_summary.unwrap_or_else(|| {
                        "任务已完成，但没有找到 summary note；请检查 task-result-artifact。"
                            .to_string()
                    });
                    let final_event = serde_json::json!({
                        "phase": "done",
                        "task_id": task_id,
                        "artifact_hash": latest_artifact_hash,
                    });
                    Self::write_sse_event(stream, "final", &final_event).await?;
                    Self::write_sse_openai_text(stream, chat_id, &final_text, Some("stop")).await?;
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
                    let diagnostic = serde_json::json!({
                        "phase": task.status.as_str(),
                        "task_id": task_id,
                        "artifact_hash": latest_artifact_hash,
                        "message": final_text,
                    });
                    Self::write_sse_event(stream, "diagnostic", &diagnostic).await?;
                    Self::write_sse_openai_text(stream, chat_id, &final_text, Some("stop")).await?;
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
                        Self::write_sse_openai_text(
                            stream,
                            chat_id,
                            &format!(
                                "任务仍在运行，我已返回可续接状态而不是伪造结果。后续请求携带 missiond_follow_task_id={} 即可继续等待或读取最终 task-result-artifact。",
                                task_id
                            ),
                            Some("stop"),
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
        intent_artifact_id: &str,
        plan_artifact_id: &str,
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
        let read_scope = Self::jarvis_dispatch_read_scope(read_scope_root, context_pack_file);

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
            "acceptance": [
                "Return a structured artifact with Findings / Evidence / Recommendations / Verification",
                "Use the grounding context and cited evidence instead of rediscovering broad context",
                "If the requested provider/tool is unavailable, fail fast with a diagnostic"
            ],
            "output_contract": "Findings / Evidence / Recommendations / Verification",
            "grounding_context_id": grounding_context_id,
            "context_pack_path": context_pack_path,
            "context_pack_file": context_pack_file,
            "intent_artifact_id": intent_artifact_id,
            "plan_artifact_id": plan_artifact_id,
            "worker_may_delegate": false
        })
    }

    fn jarvis_dispatch_read_scope(
        read_scope_root: &str,
        context_pack_file: Option<&str>,
    ) -> Vec<String> {
        let root = read_scope_root.trim();
        let mut scopes = Vec::new();
        if !root.is_empty() {
            scopes.push(root.to_string());
        }
        let Some(context_pack_file) = context_pack_file.map(str::trim).filter(|s| !s.is_empty())
        else {
            return scopes;
        };
        let context_path = Path::new(context_pack_file);
        let context_scope = context_path.parent().unwrap_or(context_path);
        let context_scope_display = context_scope.display().to_string();
        if !context_scope_display.is_empty()
            && !Self::path_is_within_scope(context_scope, root)
            && !scopes.iter().any(|scope| scope == &context_scope_display)
        {
            scopes.push(context_scope_display);
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
             intent_artifact_id: {iai}\n\
             plan_artifact_id: {pai}\n\n\
             已接受执行切片：\n\
             - 这个任务已经通过 Jarvis intent 确认和 plan 确认。\n\
             - 你不是主控；不要重新拆任务、不要创建 BoardTask、不要派子工位。\n\
             - 你只需要按 task_kind 和 acceptance 验证当前工位能力，并返回结构化结果。\n\
             - acceptance:\n{acc}\n\n\
             工作方式：\n\
             - 这是已经过 Jarvis 意图确认和计划确认的 grounded dispatch，不要重新扮演主控。\n\
             - 先读取 context_pack_file；这是 MissionD 为没有 MCP 的工位物化的 bounded context slice。\n\
             - 如果 context_pack_file 不可读，且 context_pack_path 是 shared-artifact://，再用 MissionD MCP 调 mission_shared_memory(action=\"artifact_get\", hash=\"{cph}\") 或 mission_context_slice 读取上下文切片。\n\
             - 如果文件和 MCP 都不可用，不要自行大范围搜索代码；请快速失败并输出 Diagnostic / Evidence / Verification，说明 context unavailable。\n\
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
            iai = intent_artifact_id,
            pai = plan_artifact_id,
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
        db: Option<Arc<dyn crate::db::traits::MissionStore>>,
        cc_tasks_watcher: Option<Arc<Mutex<CCTasksWatcher>>>,
        tool_count: usize,
        default_chat_slot: String,
    ) -> anyhow::Result<()> {
        // Disable Nagle — SSE needs every chunk sent immediately
        stream.set_nodelay(true)?;

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

        // Validate Bearer token: require match against MISSIOND_API_TOKEN if set.
        // If env var is unset, accept any non-empty token (backward compatible).
        match &auth_token {
            None => {
                let err = serde_json::json!({"error": {"message": "Missing Authorization header"}});
                Self::send_http_error(&mut stream, 401, "Unauthorized", &err.to_string()).await?;
                return Ok(());
            }
            Some(token) => {
                if let Ok(expected) = std::env::var("MISSIOND_API_TOKEN") {
                    if token != &expected {
                        let err = serde_json::json!({"error": {"message": "Invalid API token"}});
                        Self::send_http_error(&mut stream, 401, "Unauthorized", &err.to_string())
                            .await?;
                        return Ok(());
                    }
                }
            }
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
                match db.jarvis_get_or_create(conversation_id.as_deref()).await {
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
                            &mut stream,
                            &chat_id,
                            follow_task_id,
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

        let intent_confirmed = jarvis_confirm_bool(&req, "missiond_intent_confirmed");
        let plan_confirmed = jarvis_confirm_bool(&req, "missiond_plan_confirmed");
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
                match db.jarvis_get_or_create(conversation_id.as_deref()).await {
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

            let objective_text = if intent_confirmed || plan_confirmed {
                match jarvis_confirm_string(&req, "missiond_objective") {
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

            let grounding = match Self::gather_jarvis_grounding(
                &jarvis_grounding,
                JarvisGroundingRequest {
                    query: objective_text.clone(),
                    conversation_id: jarvis_conv_id.clone(),
                    chat_id: chat_id.clone(),
                    unknowns: vec![
                        "What project, skill, deploy fact, or tool context is needed before dispatch?"
                            .to_string(),
                        "Is this broad request ready for intent/plan confirmation or already an exact shard?"
                            .to_string(),
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
            let grounding_context_id = grounding.grounding_context_id.clone();
            let context_pack_path = grounding.context_pack_path.clone();
            let context_pack_file = grounding.context_pack_file.clone();
            let grounding_artifact_hash = grounding.artifact_hash.clone();
            let sources_used = grounding.sources_used.clone();
            let grounding_diagnostics = grounding.diagnostics.clone();
            let grounding_event = serde_json::json!({
                "phase": "grounding",
                "grounding_context_id": grounding_context_id,
                "context_pack_path": context_pack_path,
                "context_pack_file": context_pack_file,
                "artifact_hash": grounding_artifact_hash,
                "sources_used": sources_used,
                "diagnostics": grounding_diagnostics,
            });
            Self::write_sse_event(&mut stream, "status", &grounding_event).await?;

            let intent_payload = serde_json::json!({
                "phase": "intent_draft",
                "grounding_context_id": grounding_context_id,
                "context_pack_path": context_pack_path,
                "context_pack_file": context_pack_file,
                "understanding": "我理解这是一个需要先确认意图、再拆 plan.lisp、再派工位执行的 Jarvis 请求。",
                "objective": objective_text,
                "user_message_preview": objective_text.chars().take(240).collect::<String>(),
                "sources_used": sources_used,
                "requires_confirmation": true
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
            if !intent_confirmed {
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
                Self::write_sse_event(&mut stream, "intent_draft", &intent).await?;
                let confirm = serde_json::json!({
                    "phase": "awaiting_intent_confirmation",
                    "message": "请确认：我的意图理解是否正确？确认后我会生成 plan.lisp，再等你确认后创建 BoardTask 并派工位。",
                    "confirm_payload": {
                        "missiond_intent_confirmed": true,
                        "missiond_objective": objective_text,
                        "missiond_grounding_context_id": grounding_context_id,
                        "missiond_intent_artifact_id": intent_artifact_id
                    }
                });
                Self::write_sse_event(&mut stream, "confirm_required", &confirm).await?;
                Self::write_sse_openai_text(
                    &mut stream,
                    &chat_id,
                    "我已生成 intent.lisp 草案，等待你确认意图。",
                    Some("stop"),
                )
                .await?;
                Self::finish_sse(&mut stream).await?;
                return Ok(());
            }

            let plan_payload = serde_json::json!({
                "phase": "plan_draft",
                "grounding_context_id": grounding_context_id,
                "context_pack_path": context_pack_path,
                "context_pack_file": context_pack_file,
                "intent_artifact_id": intent_artifact_id,
                "objective": objective_text,
                "steps": [
                    "确认 project_id / read_scope / skill evidence / deploy facts 等 grounding 证据",
                    "把用户目标拆成可验收的 BoardTask / accepted shard",
                    "按任务类型选择 Codex / ClaudeCode / Agy 工位",
                    "等待 task-result-artifact，再由主控返回结果"
                ],
                "requires_confirmation": true
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
            if !plan_confirmed {
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
                Self::write_sse_event(&mut stream, "plan_draft", &plan).await?;
                let confirm = serde_json::json!({
                    "phase": "awaiting_plan_confirmation",
                    "message": "请确认 plan.lisp。确认后我会创建 BoardTask 并派工位，不会让主控直接做实现。",
                    "confirm_payload": {
                        "missiond_intent_confirmed": true,
                        "missiond_plan_confirmed": true,
                        "missiond_objective": objective_text,
                        "missiond_grounding_context_id": grounding_context_id,
                        "missiond_intent_artifact_id": intent_artifact_id,
                        "missiond_plan_artifact_id": plan_artifact_id
                    }
                });
                Self::write_sse_event(&mut stream, "confirm_required", &confirm).await?;
                Self::write_sse_openai_text(
                    &mut stream,
                    &chat_id,
                    "我已生成 plan.lisp 草案，等待你确认计划。",
                    Some("stop"),
                )
                .await?;
                Self::finish_sse(&mut stream).await?;
                return Ok(());
            }

            let Some(ref db) = db else {
                let err = serde_json::json!({"error": {"message": "MissionD DB unavailable; cannot create grounded BoardTask"}});
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
                &intent_artifact_id,
                &plan_artifact_id,
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
                "intent_artifact_id": intent_artifact_id,
                "plan_artifact_id": plan_artifact_id,
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
            match db.create_board_task(&task_input).await {
                Ok(task) => {
                    let event = serde_json::json!({
                        "task_id": task.id,
                        "title": task.title,
                        "grounding_context_id": grounding_context_id,
                        "intent_artifact_id": intent_artifact_id,
                        "plan_artifact_id": plan_artifact_id,
                    });
                    Self::write_sse_event(&mut stream, "board_task_created", &event).await?;
                    Self::write_sse_event(
                        &mut stream,
                        "worker_dispatched",
                        &serde_json::json!({
                            "phase": "workers_running",
                            "task_id": task.id,
                            "slot_id": serde_json::Value::Null,
                            "dispatch_state": "pending_autopilot_claim",
                            "status": task.status.as_str(),
                            "terminal_task_result": false,
                            "follow_payload": {
                                "missiond_follow_task_id": task.id,
                                "stream": true
                            },
                            "message": "BoardTask is queued for Autopilot/provider claim; concrete slot attribution will arrive through follow-up supervision."
                        }),
                    )
                    .await?;
                    Self::write_sse_openai_text(
                        &mut stream,
                        &chat_id,
                        "计划已确认，我已创建 BoardTask；这次请求会返回 follow handle，不会等待长任务占住手机连接。",
                        None,
                    )
                    .await?;
                    let follow_payload = serde_json::json!({
                        "missiond_follow_task_id": task.id,
                        "stream": true
                    });
                    let worker_status = serde_json::json!({
                        "phase": "board_tasks_created",
                        "task_id": task.id,
                        "status": task.status.as_str(),
                        "terminal_task_result": false,
                        "follow_payload": follow_payload.clone(),
                        "message": "BoardTask created; worker execution continues asynchronously and results must be read through follow-up supervision."
                    });
                    Self::write_sse_event(&mut stream, "worker_status", &worker_status).await?;
                    Self::write_sse_event(
                        &mut stream,
                        "dispatch_accepted",
                        &serde_json::json!({
                            "phase": "board_tasks_created",
                            "task_id": task.id,
                            "status": task.status.as_str(),
                            "terminal_task_result": false,
                            "follow_payload": follow_payload.clone(),
                            "message": "BoardTask was created and accepted for asynchronous worker dispatch; this is not a terminal task result."
                        }),
                    )
                    .await?;
                    let pending_event = serde_json::json!({
                        "phase": "result_pending",
                        "task_id": task.id,
                        "status": "result_pending",
                        "terminal_task_result": false,
                        "follow_payload": follow_payload
                    });
                    Self::write_sse_event(&mut stream, "result_pending", &pending_event).await?;
                    Self::write_sse_openai_text(
                        &mut stream,
                        &chat_id,
                        &format!(
                            "BoardTask 已创建。后续请求携带 missiond_follow_task_id={} 读取 task-result-artifact；初始手机请求不会等待长任务完成。",
                            task.id
                        ),
                        Some("stop"),
                    )
                    .await?;
                }
                Err(e) => {
                    let err = serde_json::json!({"error": {"message": format!("Failed to create BoardTask: {}", e)}});
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
            match db.jarvis_get_or_create(conversation_id.as_deref()).await {
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
        _tool_count: usize,
        default_chat_slot: String,
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
            // Local deploy-center smoke may restore the default Jarvis slot after
            // a blue/green restart. This is intentionally not a normalized
            // `/jarvis/*` public route.
            if method == "POST" && path == "/internal/jarvis/slot/ensure" && !is_upgrade {
                return match pty_manager {
                    Some(pm) => {
                        Self::handle_jarvis_slot_ensure(stream, addr, pm, default_chat_slot.clone())
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
                        Self::handle_jarvis_monitor(stream, pm, default_chat_slot.clone()).await
                    }
                    None => {
                        let mut s = stream;
                        let err = serde_json::json!({
                            "schema": "missiond.jarvis-chain-monitor.v1",
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
                    jarvis_grounding,
                    jarvis_artifact_writer,
                    db,
                )
                .await;
            }
            // GET /interactions/v1/{interaction_id}/events
            if method == "GET" && normalized_path.starts_with("/interactions/v1/") && !is_upgrade {
                return Self::handle_interaction_events(stream, &normalized_request_line).await;
            }
            // Chat completions SSE endpoint
            // POST /v1/chat/completions (and public /jarvis/v1/chat/completions)
            if method == "POST" && normalized_path == "/v1/chat/completions" {
                return Self::handle_chat_completions_interaction_adapter(
                    stream,
                    addr,
                    pty_manager.clone(),
                    default_chat_slot.clone(),
                    jarvis_grounding,
                    jarvis_artifact_writer,
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
                        "missiond_objective": "原始目标"
                    }
                }
            },
            "messages": [
                {"role": "system", "content": "ignore"},
                {"role": "assistant", "content": "old"},
                {"role": "user", "content": [
                    {"type": "text", "text": "请测试 MissionD"},
                    {"type": "text", "text": "并返回计划"}
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
            interaction_metadata_string(&envelope, "missiond_follow_task_id").as_deref(),
            Some("follow-task-1")
        );
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
            "intent-abc",
            "plan-abc",
            "/repo",
        );
        let prompt =
            PTYWebSocketServer::build_jarvis_worker_prompt("请接入并验证 agy CLI", &metadata);
        assert!(prompt.contains("context_pack_file: /tmp/missiond/context-gather/abc.json"));
        assert!(prompt.contains("目标工位：engine_hint=agy pool_hint=agy-research"));
        assert!(prompt.contains("intent_artifact_id: intent-abc"));
        assert!(prompt.contains("plan_artifact_id: plan-abc"));
        assert!(prompt.contains("已接受执行切片"));
        assert!(prompt.contains("先读取 context_pack_file"));
        assert!(prompt.contains("context unavailable"));
        assert!(prompt.contains("mission_shared_memory(action=\"artifact_get\", hash=\"abc\")"));
        assert!(prompt.contains("## Findings"));
        assert!(prompt.contains("## Evidence"));
        assert!(prompt.contains("## Recommendations"));
        assert!(prompt.contains("## Verification"));
        assert!(prompt.contains("task-result-artifact"));
        assert!(!prompt.contains("mission_board_update"));
    }

    #[test]
    fn jarvis_dispatch_context_pack_parent_enters_read_scope() {
        let metadata = PTYWebSocketServer::derive_jarvis_dispatch_contract(
            "请接入并验证 agy CLI",
            "context-gather:abc",
            Some("shared-artifact://abc"),
            Some("/tmp/missiond/context-gather/abc.json"),
            "intent-abc",
            "plan-abc",
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
            "intent-xyz",
            "plan-xyz",
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
            "intent-codex",
            "plan-codex",
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
            "intent-abc",
            "plan-abc",
            "/repo",
        );
        assert_eq!(metadata["task_class"], "review");
        assert_eq!(metadata["write_policy"], "read-only");
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
            "intent-readonly",
            "plan-readonly",
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
            "intent-code",
            "plan-code",
            "/repo",
        );
        assert_eq!(metadata["task_class"], "code");
        assert_eq!(metadata["write_policy"], "scoped");
        assert_eq!(metadata["pool_hint"], "claude-code-default");
        assert_eq!(metadata["write_scope"][0], "/repo");
    }

    #[test]
    fn jarvis_dispatch_implementation_prompt_uses_scoped_write_constraint() {
        let metadata = PTYWebSocketServer::derive_jarvis_dispatch_contract(
            "fix the broken auth flow",
            "ctx:abc",
            None,
            None,
            "i",
            "p",
            "/repo",
        );
        let prompt =
            PTYWebSocketServer::build_jarvis_worker_prompt("fix the broken auth flow", &metadata);
        assert!(prompt.contains("工位实现任务"));
        assert!(prompt.contains("只在 write_scope 范围内修改文件"));
        assert!(!prompt.contains("不要修改文件"));
    }
}
