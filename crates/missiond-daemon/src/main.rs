//! missiond - singleton daemon for missiond
//!
//! Responsibilities:
//! - Own the global state (DB, slot/process/task/inbox, PTY sessions, CC tasks watcher)
//! - Provide a stable WebSocket endpoint for attach + tasks events
//! - Expose an IPC JSON-RPC endpoint for MCP proxy processes

mod events_sync;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use missiond_core::{
    CorePermissionDecision, InfraConfig, MissionControl, MissionControlOptions, PermissionPolicy,
    PermissionRule, PTYManager, PTYSpawnOptions, PTYWebSocketServer, WSServerOptions, SkillIndex,
};
use missiond_core::SessionState;
use missiond_core::{CCTasksWatcher, CCTasksWatcherOptions, WatcherEvent};
use missiond_mcp::protocol::{self, Request, RequestId, Response, RpcError};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use missiond_core::ipc::{self, IpcListener, IpcStream};

/// Extraction phase state machine. Replaces rigid 120s cooldown with
/// event-driven completion detection.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ExtractionPhase {
    /// Ready for next extraction trigger.
    Idle,
    /// send() is in flight (waiting for TextComplete).
    Sending,
    /// send() returned but slot is still processing MCP calls.
    /// Will transition to Idle when slot's SessionState becomes Idle.
    WaitingForSlotIdle,
}

struct ExtractionState {
    phase: ExtractionPhase,
    /// Which extraction type is active: "realtime", "deep_analysis", "kb_consolidation"
    active_type: Option<&'static str>,
    /// When current phase started (epoch secs), for timeout detection.
    phase_started_at: i64,
    /// Conversation ID currently being deep-analyzed (for marking complete on Idle).
    current_deep_conv_id: Option<String>,
    /// Watermark targets for realtime extraction: (session_id, max_timestamp).
    /// On completion, update realtime_forwarded_at for each session.
    watermark_targets: Vec<(String, String)>,
    /// Task ID for the current extraction job (survives compaction).
    current_task_id: Option<String>,
    /// slot_tasks table row ID for the current extraction (for history tracking).
    current_slot_task_id: Option<String>,
    /// Whether current deep analysis is a checkpoint (active session) vs full (completed session).
    is_checkpoint: bool,
    /// Max message ID in the batch being analyzed (for advancing checkpoint watermark on completion).
    checkpoint_message_id: Option<i64>,
}

/// Deep analysis schema version. Bump this when the analysis prompt changes
/// to trigger re-analysis of all previously analyzed conversations.
const CURRENT_ANALYSIS_VERSION: i32 = 1;
/// Max retries for a single conversation's deep analysis before giving up.
const MAX_ANALYSIS_RETRIES: i32 = 2;

/// Safety valve: max time to wait for slot to return to Idle after send() returns.
const MAX_WAIT_FOR_IDLE_SECS: i64 = 900;

/// Per-slot JSONL progress tracking: tool call counts, current tool, etc.
/// Populated from tool_use/tool_result events, queried by mission_pty_status.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SlotProgress {
    session_id: String,
    tool_counts: HashMap<String, u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_tool: Option<CurrentToolInfo>,
    total_calls: u32,
    total_results: u32,
    error_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_activity: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CurrentToolInfo {
    name: String,
    started_at: String,
}

/// Persistent MCP client for xjp-mcp server (stdio-based, lazy-initialized).
/// Uses Arc-wrapped shared state so the outer lock can be released before awaiting responses.
struct McpProcessClient {
    state: tokio::sync::Mutex<Option<McpClientState>>,
    config_path: PathBuf,
}

#[derive(Clone)]
struct McpClientState {
    stdin: Arc<tokio::sync::Mutex<tokio::process::ChildStdin>>,
    pending: Arc<tokio::sync::Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Value>>>>,
    next_id: Arc<std::sync::atomic::AtomicU64>,
    reader_alive: Arc<std::sync::atomic::AtomicBool>,
    /// Call counter for max-uses recycling
    call_count: Arc<std::sync::atomic::AtomicU64>,
    /// Keep child alive — kill_on_drop fires when this Arc reaches zero.
    _child: Arc<tokio::sync::Mutex<tokio::process::Child>>,
}

/// Max calls before recycling the MCP process (prevents state pollution)
const MCP_MAX_USES: u64 = 200;

impl McpProcessClient {
    fn new(config_path: PathBuf) -> Self {
        Self { state: tokio::sync::Mutex::new(None), config_path }
    }

    async fn call_tool(&self, tool_name: &str, tool_args: Value) -> Result<ToolResult> {
        // Phase 1: Get or create client state (short lock)
        let client = {
            let mut guard = self.state.lock().await;
            let needs_respawn = match guard.as_ref() {
                None => true,
                Some(s) => {
                    !s.reader_alive.load(std::sync::atomic::Ordering::Relaxed)
                        || s.call_count.load(std::sync::atomic::Ordering::Relaxed) >= MCP_MAX_USES
                }
            };
            if needs_respawn {
                if let Some(ref old) = *guard {
                    let reason = if !old.reader_alive.load(std::sync::atomic::Ordering::Relaxed) {
                        "process died"
                    } else {
                        "max-uses reached"
                    };
                    warn!("xjp-mcp recycling: {}", reason);
                }
                *guard = Some(Self::spawn(&self.config_path).await?);
            }
            guard.as_ref().unwrap().clone()
        }; // Lock released here!

        client.call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Phase 2: Send request (no outer lock held)
        let id = client.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (tx, rx) = tokio::sync::oneshot::channel();
        client.pending.lock().await.insert(id, tx);

        let send_result = {
            let mut stdin = client.stdin.lock().await;
            let req = serde_json::json!({
                "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": {"name": tool_name, "arguments": tool_args}
            });
            let r1 = stdin.write_all(req.to_string().as_bytes()).await;
            let r2 = stdin.write_all(b"\n").await;
            let r3 = stdin.flush().await;
            r1.and(r2).and(r3)
        }; // stdin lock released

        // Handle Broken Pipe: mark process dead for next call to respawn
        if let Err(e) = send_result {
            client.reader_alive.store(false, std::sync::atomic::Ordering::Relaxed);
            return Err(anyhow!("xjp-mcp stdin write failed (Broken Pipe?): {}", e));
        }

        // Phase 3: Wait for response (completely lock-free)
        let resp = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            rx,
        ).await
            .map_err(|_| anyhow!("xjp-mcp tool '{}' timed out after 30s", tool_name))?
            .map_err(|_| anyhow!("xjp-mcp response channel closed (process may have died)"))?;

        if let Some(result) = resp.get("result") {
            let tool_result: ToolResult = serde_json::from_value(result.clone())
                .unwrap_or_else(|_| ToolResult::text(result.to_string()));
            Ok(tool_result)
        } else if let Some(error) = resp.get("error") {
            let msg = error.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
            let mut res = ToolResult::text(msg.to_string());
            res.is_error = Some(true);
            Ok(res)
        } else {
            Ok(ToolResult::text(format!("Unexpected xjp-mcp response: {}", resp)))
        }
    }

    async fn spawn(config_path: &Path) -> Result<McpClientState> {
        let config_str = tokio::fs::read_to_string(config_path).await
            .map_err(|e| anyhow!("Failed to read xjp-mcp config: {}", e))?;
        let config: Value = serde_json::from_str(&config_str)?;
        let mcp_config = config.get("mcpServers").and_then(|s| s.get("xjp-mcp"))
            .ok_or_else(|| anyhow!("xjp-mcp not found in config"))?;

        let command = mcp_config.get("command").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing command"))?;
        let args: Vec<String> = mcp_config.get("args")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let env_map: std::collections::HashMap<String, String> = mcp_config.get("env")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let mut child = tokio::process::Command::new(command)
            .args(&args)
            .envs(&env_map)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| anyhow!("Failed to spawn xjp-mcp: {}", e))?;

        let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("No stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("No stdout"))?;

        // Background stderr reader for debugging
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut buf = String::new();
                loop {
                    buf.clear();
                    match reader.read_line(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            debug!("xjp-mcp stderr: {}", buf.trim());
                        }
                    }
                }
            });
        }

        // MCP handshake
        let init_req = serde_json::json!({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "missiond", "version": "0.1.0"}}
        });
        stdin.write_all(init_req.to_string().as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;

        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let notif = serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        stdin.write_all(notif.to_string().as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;

        let pending: Arc<tokio::sync::Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Value>>>> =
            Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let reader_alive = Arc::new(std::sync::atomic::AtomicBool::new(true));

        let pending_clone = pending.clone();
        let alive_clone = reader_alive.clone();

        // Background reader task
        tokio::spawn(async move {
            let mut buf = String::new();
            loop {
                buf.clear();
                match reader.read_line(&mut buf).await {
                    Ok(0) => {
                        warn!("xjp-mcp reader: EOF (process exited)");
                        alive_clone.store(false, std::sync::atomic::Ordering::Relaxed);
                        break;
                    }
                    Err(e) => {
                        warn!("xjp-mcp reader: error: {}", e);
                        alive_clone.store(false, std::sync::atomic::Ordering::Relaxed);
                        break;
                    }
                    Ok(_) => {
                        if let Ok(resp) = serde_json::from_str::<Value>(buf.trim()) {
                            if let Some(id) = resp.get("id").and_then(|v| v.as_u64()) {
                                if let Some(tx) = pending_clone.lock().await.remove(&id) {
                                    let _ = tx.send(resp);
                                }
                            }
                        }
                    }
                }
            }
        });

        info!("xjp-mcp persistent client spawned");
        Ok(McpClientState {
            stdin: Arc::new(tokio::sync::Mutex::new(stdin)),
            pending,
            next_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            reader_alive,
            call_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            _child: Arc::new(tokio::sync::Mutex::new(child)),
        })
    }
}

#[derive(Clone)]
struct AppState {
    mission: Arc<MissionControl>,
    permission: Arc<PermissionPolicy>,
    pty: Arc<PTYManager>,
    cc_tasks: Arc<Mutex<CCTasksWatcher>>,
    skills: Arc<SkillIndex>,
    infra: Arc<InfraConfig>,
    /// JSONL session UUIDs belonging to PTY-managed slots.
    /// White-list: any session_id NOT in this set is a user CLI session.
    pty_session_uuids: Arc<tokio::sync::RwLock<HashSet<String>>>,
    /// Fast lane state machine (realtime extraction on slot-memory).
    extraction_state: Arc<tokio::sync::RwLock<ExtractionState>>,
    /// Slow lane state machine (deep analysis + kb consolidation on slot-memory-slow).
    slow_extraction_state: Arc<tokio::sync::RwLock<ExtractionState>>,
    /// Timestamp when slot-memory entered its current non-Idle state. 0 = idle.
    memory_slot_busy_since: Arc<std::sync::atomic::AtomicI64>,
    /// Timestamp when slot-memory-slow entered its current non-Idle state. 0 = idle.
    slow_slot_busy_since: Arc<std::sync::atomic::AtomicI64>,
    /// Hash of last synced CLAUDE.md managed section (to avoid unnecessary writes).
    claude_md_hash: Arc<std::sync::atomic::AtomicU64>,
    /// Signal to re-check fast lane extractions immediately.
    extraction_notify: Arc<tokio::sync::Notify>,
    /// Signal to re-check slow lane extractions immediately.
    slow_extraction_notify: Arc<tokio::sync::Notify>,
    /// Signal to re-dispatch queued submit tasks (fired on task creation/completion).
    submit_notify: Arc<tokio::sync::Notify>,
    /// Last KB auto-GC timestamp (epoch secs). 0 = never run.
    last_auto_gc_at: Arc<std::sync::atomic::AtomicI64>,
    /// Last KB consolidation timestamp (epoch secs). 0 = never run.
    last_kb_consolidation_at: Arc<std::sync::atomic::AtomicI64>,
    /// Pause switch for memory extraction tasks (realtime, deep_analysis, sync, GC).
    memory_paused: Arc<std::sync::atomic::AtomicBool>,
    /// Epoch secs when memory was paused. 0 = not paused. Used for TTL auto-resume.
    memory_paused_at: Arc<std::sync::atomic::AtomicI64>,
    /// Per-slot consecutive failure count for autopilot throttling.
    slot_fail_counts: Arc<std::sync::Mutex<HashMap<String, (i32, i64)>>>,  // (count, last_fail_at)
    /// Screenshot broker for browser-based PTY screenshots.
    screenshot_broker: Arc<missiond_core::ws::ScreenshotBroker>,
    /// Jarvis request trace store for debugging.
    jarvis_trace: missiond_core::ws::JarvisTraceStore,
    /// Last complete response per slot (for submit task result tracking).
    slot_last_responses: Arc<tokio::sync::RwLock<HashMap<String, String>>>,
    /// Per-slot JSONL progress tracking (tool call counts, current tool, etc.).
    slot_progress: Arc<tokio::sync::RwLock<HashMap<String, SlotProgress>>>,
    /// Shared HTTP client for Router API calls (connection pool reuse).
    http_client: reqwest::Client,
    /// Persistent xjp-mcp client (lazy-initialized, auto-reconnect on crash).
    xjp_mcp: Arc<McpProcessClient>,
}

fn default_mission_home() -> PathBuf {
    ipc::default_mission_home()
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var(var).ok().map(PathBuf::from)
}

fn ipc_endpoint_from_env() -> String {
    if let Ok(endpoint) = std::env::var("MISSION_IPC_ENDPOINT") {
        return endpoint;
    }
    // Legacy support for MISSION_IPC_SOCKET on Unix
    #[cfg(unix)]
    if let Ok(socket) = std::env::var("MISSION_IPC_SOCKET") {
        return socket;
    }
    ipc::default_ipc_endpoint()
}

fn db_path() -> PathBuf {
    env_path("MISSION_DB_PATH").unwrap_or_else(|| default_mission_home().join("mission.db"))
}

fn slots_config_path() -> PathBuf {
    env_path("MISSION_SLOTS_CONFIG").unwrap_or_else(|| default_mission_home().join("slots.yaml"))
}

fn ws_port() -> u16 {
    std::env::var("MISSION_WS_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(9120)
}

fn logs_dir(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("logs")
}

/// Ensure sensitive config files have restrictive permissions (owner-only read/write).
/// Automatically fixes permissions and logs a warning if they're too open.
#[cfg(unix)]
fn ensure_config_permissions(home: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let files = [
        "servers.yaml",
        "slots.yaml",
        "mcp-config.json",
        "xjp-mcp-config.json", // legacy name
        "config/permissions.yaml",
        "mission.db",
    ];

    for name in &files {
        let path = home.join(name);
        if !path.exists() {
            continue;
        }
        if let Ok(meta) = std::fs::metadata(&path) {
            let mode = meta.permissions().mode();
            if mode & 0o077 != 0 {
                warn!(
                    file = %path.display(),
                    old_mode = format!("{:o}", mode),
                    "Config file too permissive, fixing to 600"
                );
                std::fs::set_permissions(
                    &path,
                    std::fs::Permissions::from_mode(0o600),
                )
                .ok();
            }
        }
    }
}

fn log_filter() -> tracing_subscriber::EnvFilter {
    let level = if let Ok(v) = std::env::var("RUST_LOG") {
        v
    } else if let Ok(v) = std::env::var("MISSION_LOG_LEVEL") {
        match v.as_str() {
            "silent" => "off".to_string(),
            "fatal" => "error".to_string(),
            other => other.to_string(),
        }
    } else {
        // Daemon default: info (need visibility into PTY spawns, IPC, etc.)
        "info".to_string()
    };

    tracing_subscriber::EnvFilter::try_new(level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"))
}

struct PermissionAdapter {
    permission: Arc<PermissionPolicy>,
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

// =========================
// Tool dispatch (daemon side)
// =========================

#[derive(Deserialize)]
struct SubmitArgs {
    role: String,
    prompt: String,
    #[serde(rename = "slotId")]
    slot_id: Option<String>,
}

#[derive(Deserialize)]
struct AskArgs {
    role: String,
    question: String,
    #[serde(rename = "timeoutMs", default)]
    timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
struct StatusArgs {
    #[serde(rename = "taskId")]
    task_id: String,
}

#[derive(Deserialize)]
struct CancelArgs {
    #[serde(rename = "taskId")]
    task_id: String,
}

#[derive(Deserialize)]
struct SpawnArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    #[serde(default)]
    visible: Option<bool>,
    #[serde(rename = "autoRestart", default)]
    auto_restart: Option<bool>,
}

#[derive(Deserialize)]
struct KillArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
}

#[derive(Deserialize)]
struct RestartArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    #[serde(default)]
    visible: Option<bool>,
}

#[derive(Deserialize)]
struct InboxArgs {
    #[serde(rename = "unreadOnly", default)]
    unread_only: Option<bool>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct PTYSpawnArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    #[serde(rename = "waitForIdle", default)]
    wait_for_idle: Option<bool>,
    #[serde(rename = "timeoutSecs", default)]
    timeout_secs: Option<u64>,
    #[serde(rename = "autoRestart", default)]
    auto_restart: Option<bool>,
    #[serde(rename = "mcpConfigPath", default)]
    mcp_config_path: Option<String>,
}

#[derive(Deserialize)]
struct PTYSendArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    message: String,
    #[serde(rename = "waitForResponse", default)]
    wait_for_response: Option<bool>,
    #[serde(rename = "timeoutMs", default)]
    timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
struct PTYKillArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
}

#[derive(Deserialize)]
struct PTYScreenArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    #[serde(default)]
    lines: Option<usize>,
}

#[derive(Deserialize)]
struct PTYHistoryArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
}

#[derive(Deserialize)]
struct PTYStatusArgs {
    #[serde(rename = "slotId")]
    slot_id: Option<String>,
}

#[derive(Deserialize)]
struct PTYConfirmArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    response: Value,
}

#[derive(Deserialize)]
struct PTYInterruptArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
}

#[derive(Deserialize)]
struct PTYLogsArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
}

#[derive(Deserialize)]
struct PTYScreenshotArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
}

#[derive(Deserialize)]
struct SetRolePermissionArgs {
    role: String,
    rule: PermissionRule,
}

#[derive(Deserialize)]
struct SetSlotPermissionArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    rule: PermissionRule,
}

#[derive(Deserialize)]
struct AddAutoAllowArgs {
    role: Option<String>,
    #[serde(rename = "slotId")]
    slot_id: Option<String>,
    pattern: String,
}

#[derive(Deserialize)]
struct CCSessionsArgs {
    #[serde(rename = "projectPath")]
    project_path: Option<String>,
    #[serde(rename = "activeOnly", default)]
    active_only: Option<bool>,
}

#[derive(Deserialize)]
struct CCTasksArgs {
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    #[serde(rename = "projectPath")]
    project_path: Option<String>,
}

#[derive(Deserialize)]
struct CCTriggerSwarmArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    tasks: Vec<String>,
    #[serde(rename = "teammateCount", default)]
    teammate_count: Option<usize>,
    #[serde(rename = "timeoutMs", default)]
    timeout_ms: Option<u64>,
}

// Board tasks args
#[derive(Deserialize)]
struct BoardListArgs {
    #[serde(default)]
    status: Option<String>,
    #[serde(default, rename = "includeHidden")]
    include_hidden: Option<bool>,
}

#[derive(Deserialize)]
struct BoardIdArgs {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoardNoteAddArgs {
    task_id: String,
    content: String,
    #[serde(default)]
    note_type: Option<String>,
    #[serde(default)]
    author: Option<String>,
}

// Agent questions args
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuestionCreateArgs {
    question: String,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    slot_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Deserialize)]
struct QuestionListArgs {
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize)]
struct QuestionAnswerArgs {
    id: String,
    answer: String,
}

#[derive(Deserialize)]
struct QuestionIdArgs {
    id: String,
}

#[derive(Deserialize)]
struct SkillSearchArgs {
    query: String,
}

#[derive(Deserialize)]
struct ContextBuildArgs {
    query: String,
}

#[derive(Deserialize)]
struct InfraListArgs {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    provider: Option<String>,
}

#[derive(Deserialize)]
struct InfraGetArgs {
    id: String,
}

#[derive(Deserialize)]
struct ReachabilityArgs {
    target: String,
    #[serde(default)]
    channels: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct OsDiagnoseArgs {
    target: String,
    #[serde(default)]
    checks: Option<Vec<String>>,
}

// Knowledge Base args
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KBRememberArgs {
    category: String,
    key: String,
    summary: String,
    #[serde(default)]
    detail: Option<Value>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    confidence: Option<f64>,
}

#[derive(Deserialize)]
struct KBKeyArgs {
    key: String,
}

#[derive(Deserialize)]
struct KBSearchArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    category: Option<String>,
}

#[derive(Deserialize)]
struct KBListArgs {
    #[serde(default)]
    category: Option<String>,
}

#[derive(Deserialize)]
struct KBImportArgs {
    format: String,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Deserialize)]
struct KBDiscoverArgs {
    host: String,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    password: Option<String>,
}

#[derive(Deserialize)]
struct KBGCArgs {
    action: String,
    #[serde(default)]
    days: Option<i64>,
}

impl AppState {
    async fn call_tool(&self, name: &str, args: Value) -> ToolResult {
        match self.call_tool_inner(name, args).await {
            Ok(res) => res,
            Err(e) => {
                error!(tool = %name, error = %e, "Tool call failed");
                ToolResult::error(e.to_string())
            }
        }
    }

    /// Execute a skill workflow: load workflow block, run MCP tools sequentially
    fn execute_workflow<'a>(
        &'a self,
        skill_name: &'a str,
        action_id: &'a str,
        dry_run: bool,
        param_overrides: Option<Value>,
        depth: u32,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<missiond_core::WorkflowResult>> + Send + 'a>> {
        const MAX_DEPTH: u32 = 3;
        const STEP_TIMEOUT_SECS: u64 = 30;

        Box::pin(async move {
        use missiond_core::{WorkflowStepPreview, WorkflowStepResult, WorkflowResult, parse_workflow_blocks, resolve_vars};

        // Guard: prevent recursive workflow bombs
        if depth > MAX_DEPTH {
            return Err(anyhow!("Workflow recursion depth exceeded (max {}). Skill '{}' action '{}'", MAX_DEPTH, skill_name, action_id));
        }
        let db = self.mission.db();

        // Guard: prevent concurrent execution of same action
        if !dry_run {
            if let Ok(true) = db.skill_execution_is_running(skill_name, action_id) {
                return Err(anyhow!("Action '{}' on skill '{}' is already running", action_id, skill_name));
            }
        }

        // Step 1: Load skill content from file
        let topic = db.skill_topic_get(skill_name)
            .map_err(|e| anyhow!("DB: {}", e))?
            .ok_or_else(|| anyhow!("Skill '{}' not found", skill_name))?;

        let content = std::fs::read_to_string(&topic.file_path)
            .map_err(|e| anyhow!("Failed to read skill file {}: {}", topic.file_path, e))?;

        // Step 2: Parse workflow blocks from skill content
        let workflows = parse_workflow_blocks(&content);
        let workflow = workflows.iter()
            .find(|w| w.id == action_id)
            .ok_or_else(|| anyhow!("Workflow '{}' not found in skill '{}'", action_id, skill_name))?;

        // Step 3: Check requires_approval from frontmatter actions
        let actions_json = topic.actions_json.as_deref().unwrap_or("[]");
        let actions: Vec<missiond_core::SkillAction> = serde_json::from_str(actions_json).unwrap_or_default();
        let action_meta = actions.iter().find(|a| a.id == action_id);
        let requires_approval = action_meta.map(|a| a.requires_approval).unwrap_or(false);

        if requires_approval && !dry_run {
            return Ok(WorkflowResult::PendingApproval {
                action_id: action_id.to_string(),
                skill: skill_name.to_string(),
            });
        }

        // Step 4: Dry-run → return preview only
        if dry_run {
            let steps: Vec<WorkflowStepPreview> = workflow.steps.iter().map(|s| {
                WorkflowStepPreview {
                    name: s.name.clone(),
                    tool: s.tool.clone(),
                    params: s.params.clone(),
                }
            }).collect();
            return Ok(WorkflowResult::Preview { steps });
        }

        // Step 5: Create execution log
        let exec_id = uuid::Uuid::new_v4().to_string();
        let _ = db.skill_execution_insert(
            &exec_id, skill_name, action_id,
            workflow.steps.len() as i32, "manual",
        );
        let exec_start = std::time::Instant::now();

        // Step 5b: Execute context hooks (pre-flight probes, best-effort)
        let mut context: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        if let Some(ref hooks_json) = topic.context_hooks_json {
            if let Ok(hooks) = serde_json::from_str::<Vec<missiond_core::ContextHook>>(hooks_json) {
                for hook in &hooks {
                    let hook_result = tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        self.call_tool(&hook.tool, hook.params.clone()),
                    ).await;
                    match hook_result {
                        Ok(result) => {
                            let output = result.content.first()
                                .map(|c| match c { missiond_mcp::ToolContent::Text { text } => text.clone() })
                                .unwrap_or_default();
                            // Escape ${...} in hook output to prevent injection into resolve_vars()
                            let safe_output = output.replace("${", "$\\{");
                            context.insert(hook.save_as.clone(), safe_output);
                            debug!(hook = %hook.tool, save_as = %hook.save_as, "Context hook completed");
                        }
                        Err(_) => {
                            warn!(hook = %hook.tool, "Context hook timed out (10s), skipping");
                        }
                    }
                }
            }
        }

        // Step 6: Sequential execution

        // Apply param_overrides to context
        if let Some(overrides) = param_overrides {
            if let Value::Object(map) = overrides {
                for (k, v) in map {
                    context.insert(k, v.as_str().unwrap_or(&v.to_string()).to_string());
                }
            }
        }

        let mut results: Vec<WorkflowStepResult> = Vec::new();
        let mut i = 0usize;
        let mut visit_counts: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
        const MAX_STEP_VISITS: u32 = 5; // absolute ceiling per step

        while i < workflow.steps.len() {
            let step = &workflow.steps[i];

            // Guard: prevent infinite fallback loops
            let visits = visit_counts.entry(i).or_insert(0);
            *visits += 1;
            if *visits > MAX_STEP_VISITS {
                let duration_ms = exec_start.elapsed().as_millis() as i64;
                let err_msg = format!("Step {} ('{}') visited {} times — infinite loop detected", i, step.tool, visits);
                warn!(%err_msg);
                let _ = db.skill_execution_update_with_duration(
                    &exec_id, "failed", (i + 1) as i32,
                    Some(&serde_json::to_string(&context).unwrap_or_default()),
                    Some(&err_msg),
                    Some(duration_ms),
                );
                return Ok(WorkflowResult::Failed {
                    steps_completed: i,
                    error_step: i,
                    error: err_msg,
                    results,
                });
            }

            info!(exec_id = %exec_id, step = i, tool = %step.tool, "Executing workflow step");

            // Resolve ${var} references in params
            let resolved_params = resolve_vars(&step.params, &context);

            // Call the MCP tool with timeout
            let tool_result = match tokio::time::timeout(
                std::time::Duration::from_secs(STEP_TIMEOUT_SECS),
                self.call_tool(&step.tool, resolved_params),
            ).await {
                Ok(result) => result,
                Err(_) => {
                    let mut res = ToolResult::text(format!("Step timed out after {}s: {}", STEP_TIMEOUT_SECS, step.tool));
                    res.is_error = Some(true);
                    res
                }
            };
            let is_error = tool_result.is_error.unwrap_or(false);
            let output = tool_result.content.first()
                .map(|c| match c { missiond_mcp::ToolContent::Text { text } => text.clone() })
                .unwrap_or_default();

            // Save result to context if save_as is specified
            if let Some(ref key) = step.save_as {
                context.insert(key.clone(), output.clone());
            }

            results.push(WorkflowStepResult {
                name: step.name.clone(),
                tool: step.tool.clone(),
                success: !is_error,
                output: output.clone(),
            });

            // Update progress
            let _ = db.skill_execution_update(
                &exec_id, "running", (i + 1) as i32, None, None,
            );

            // Error handling
            if is_error {
                let on_error = step.on_error.as_str();
                match on_error {
                    "skip" => {
                        warn!(step = i, tool = %step.tool, "Step failed, skipping");
                    }
                    "retry" => {
                        let max = step.max_retries.max(1);
                        let mut succeeded = false;
                        for attempt in 1..=max {
                            let backoff_secs = 1u64 << (attempt - 1).min(4);
                            warn!(step = i, tool = %step.tool, attempt, max, backoff_secs, "Retrying step");
                            tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                            let retry_params = resolve_vars(&step.params, &context);
                            let retry_result = match tokio::time::timeout(
                                std::time::Duration::from_secs(STEP_TIMEOUT_SECS),
                                self.call_tool(&step.tool, retry_params),
                            ).await {
                                Ok(r) => r,
                                Err(_) => {
                                    let mut r = ToolResult::text("Retry timed out".to_string());
                                    r.is_error = Some(true);
                                    r
                                }
                            };
                            if !retry_result.is_error.unwrap_or(false) {
                                let retry_output = retry_result.content.first()
                                    .map(|c| match c { missiond_mcp::ToolContent::Text { text } => text.clone() })
                                    .unwrap_or_default();
                                if let Some(ref key) = step.save_as {
                                    context.insert(key.clone(), retry_output.clone());
                                }
                                if let Some(last) = results.last_mut() {
                                    last.success = true;
                                    last.output = retry_output;
                                }
                                succeeded = true;
                                break;
                            }
                        }
                        if !succeeded {
                            let duration_ms = exec_start.elapsed().as_millis() as i64;
                            let _ = db.skill_execution_update_with_duration(
                                &exec_id, "failed", (i + 1) as i32,
                                Some(&serde_json::to_string(&context).unwrap_or_default()),
                                Some(&format!("Failed after {} retries: {}", max, output)),
                                Some(duration_ms),
                            );
                            return Ok(WorkflowResult::Failed {
                                steps_completed: i + 1,
                                error_step: i,
                                error: format!("Failed after {} retries: {}", max, output),
                                results,
                            });
                        }
                    }
                    s if s.starts_with("fallback:") => {
                        let target_id = &s["fallback:".len()..];
                        if let Some(target_idx) = workflow.steps.iter().position(|st| st.id.as_deref() == Some(target_id)) {
                            warn!(step = i, tool = %step.tool, target = target_id, target_idx, "Falling back");
                            i = target_idx;
                            continue; // Jump without incrementing
                        } else {
                            let duration_ms = exec_start.elapsed().as_millis() as i64;
                            let err_msg = format!("Fallback target '{}' not found", target_id);
                            let _ = db.skill_execution_update_with_duration(
                                &exec_id, "failed", (i + 1) as i32,
                                Some(&serde_json::to_string(&context).unwrap_or_default()),
                                Some(&err_msg),
                                Some(duration_ms),
                            );
                            return Ok(WorkflowResult::Failed {
                                steps_completed: i + 1,
                                error_step: i,
                                error: err_msg,
                                results,
                            });
                        }
                    }
                    _ => {
                        // "stop" (default)
                        let duration_ms = exec_start.elapsed().as_millis() as i64;
                        let _ = db.skill_execution_update_with_duration(
                            &exec_id, "failed", (i + 1) as i32,
                            Some(&serde_json::to_string(&context).unwrap_or_default()),
                            Some(&output),
                            Some(duration_ms),
                        );
                        return Ok(WorkflowResult::Failed {
                            steps_completed: i + 1,
                            error_step: i,
                            error: output,
                            results,
                        });
                    }
                }
            }
            i += 1;
        }

        // Success
        let duration_ms = exec_start.elapsed().as_millis() as i64;
        let _ = db.skill_execution_update_with_duration(
            &exec_id, "success", workflow.steps.len() as i32,
            Some(&serde_json::to_string(&context).unwrap_or_default()),
            None,
            Some(duration_ms),
        );

        Ok(WorkflowResult::Success {
            steps_completed: workflow.steps.len(),
            results,
        })
        }) // Box::pin(async move)
    }

    async fn call_tool_inner(&self, name: &str, args: Value) -> Result<ToolResult> {
        match name {
            // ===== Task operations =====
            "mission_submit" => {
                let SubmitArgs { role, prompt, slot_id: target_slot } = serde_json::from_value(args)?;
                let task_id = self.mission.submit(&role, &prompt)?;

                // If slotId specified, store it on the task for autopilot fallback
                if let Some(ref target) = target_slot {
                    let _ = self.mission.db().update_task(
                        &task_id,
                        &missiond_core::types::TaskUpdate {
                            slot_id: Some(target.clone()),
                            ..Default::default()
                        },
                    );
                }

                // Try immediate dispatch
                let mut dispatched_to: Option<String> = None;
                let slots = self.mission.list_slots();

                // Build candidate list: if slotId specified, only that slot; otherwise all matching role
                let candidates: Vec<&str> = if let Some(ref target) = target_slot {
                    vec![target.as_str()]
                } else {
                    slots.iter()
                        .filter(|s| s.config.role == role)
                        .map(|s| s.config.id.as_str())
                        .collect()
                };

                for candidate_id in &candidates {
                    if let Some(status) = self.pty.get_status(candidate_id).await {
                        if status.state == missiond_core::pty::SessionState::Idle {
                            match self.pty.send_fire_and_forget(candidate_id, &prompt).await {
                                Ok(()) => {
                                    let now = chrono::Utc::now().timestamp_millis();
                                    let slot_session = self.mission.db().get_slot_session(candidate_id).ok().flatten();
                                    let _ = self.mission.db().update_task(
                                        &task_id,
                                        &missiond_core::types::TaskUpdate {
                                            status: Some(missiond_core::types::TaskStatus::Running),
                                            slot_id: Some(candidate_id.to_string()),
                                            session_id: slot_session,
                                            started_at: Some(now),
                                            ..Default::default()
                                        },
                                    );
                                    dispatched_to = Some(candidate_id.to_string());
                                    info!(task_id = %task_id, slot_id = %candidate_id, "mission_submit: dispatched to idle slot");
                                    break;
                                }
                                Err(e) => {
                                    warn!(task_id = %task_id, slot_id = %candidate_id, error = %e, "mission_submit: dispatch failed");
                                }
                            }
                        }
                    }
                }

                // Phase 2: No idle slot — auto-spawn an exited/no-session slot then dispatch
                if dispatched_to.is_none() {
                    for candidate_id in &candidates {
                        let status = self.pty.get_status(candidate_id).await;
                        let is_spawnable = match &status {
                            Some(s) => s.state == missiond_core::pty::SessionState::Exited,
                            None => true, // no session at all
                        };
                        if !is_spawnable { continue; }

                        // Find slot config for spawn
                        let slot = slots.iter().find(|s| s.config.id == *candidate_id);
                        let slot = match slot {
                            Some(s) => s,
                            None => continue,
                        };

                        let pty_slot = missiond_core::PTYSlot {
                            id: slot.config.id.clone(),
                            role: slot.config.role.clone(),
                            cwd: slot.config.cwd.as_deref().map(std::path::PathBuf::from),
                        };
                        let mcp_config = slot.config.mcp_config.clone().map(std::path::PathBuf::from);
                        let (extra_env, session_file) = build_slot_tracking_env(candidate_id, slot.config.env.as_ref()).await;

                        info!(task_id = %task_id, slot_id = %candidate_id, "mission_submit: auto-spawning exited slot");
                        match self.pty.spawn(&pty_slot, PTYSpawnOptions {
                            auto_restart: false,
                            wait_for_idle: true,
                            timeout_secs: Some(30),
                            mcp_config,
                            dangerously_skip_permissions: slot.config.dangerously_skip_permissions.unwrap_or(false),
                            extra_env,
                        }).await {
                            Ok(_info) => {
                                capture_slot_session_uuid(self, candidate_id, &session_file).await;
                                match self.pty.send_fire_and_forget(candidate_id, &prompt).await {
                                    Ok(()) => {
                                        let now = chrono::Utc::now().timestamp_millis();
                                        let slot_session = self.mission.db().get_slot_session(candidate_id).ok().flatten();
                                        let _ = self.mission.db().update_task(
                                            &task_id,
                                            &missiond_core::types::TaskUpdate {
                                                status: Some(missiond_core::types::TaskStatus::Running),
                                                slot_id: Some(candidate_id.to_string()),
                                                session_id: slot_session,
                                                started_at: Some(now),
                                                ..Default::default()
                                            },
                                        );
                                        dispatched_to = Some(candidate_id.to_string());
                                        info!(task_id = %task_id, slot_id = %candidate_id, "mission_submit: spawned + dispatched");
                                        break;
                                    }
                                    Err(e) => warn!(task_id = %task_id, slot_id = %candidate_id, error = %e, "mission_submit: send after spawn failed"),
                                }
                            }
                            Err(e) => warn!(task_id = %task_id, slot_id = %candidate_id, error = %e, "mission_submit: auto-spawn failed"),
                        }
                    }
                }

                let mut result = serde_json::json!({ "taskId": task_id });
                if let Some(slot_id) = dispatched_to {
                    result["dispatched"] = serde_json::json!(true);
                    result["slotId"] = serde_json::json!(slot_id);
                } else {
                    result["dispatched"] = serde_json::json!(false);
                    result["hint"] = serde_json::json!("No idle slot found, task queued for autopilot dispatch");
                    // Signal unified scheduler to dispatch immediately
                    self.submit_notify.notify_one();
                }
                Ok(ToolResult::json(&result))
            }
            "mission_ask" => {
                let AskArgs {
                    role,
                    question,
                    timeout_ms,
                } = serde_json::from_value(args)?;
                let timeout_ms = timeout_ms.unwrap_or(120_000);
                let result = self.mission.ask_expert(&role, &question, timeout_ms).await?;
                Ok(ToolResult::text(result))
            }
            "mission_status" => {
                let StatusArgs { task_id } = serde_json::from_value(args)?;
                if let Some(task) = self.mission.get_status(&task_id) {
                    Ok(ToolResult::json(&task))
                } else {
                    Ok(ToolResult::error("Task not found"))
                }
            }
            "mission_cancel" => {
                let CancelArgs { task_id } = serde_json::from_value(args)?;
                let cancelled = self.mission.cancel(&task_id).await?;
                Ok(ToolResult::json(&serde_json::json!({ "cancelled": cancelled })))
            }
            "mission_task" => {
                #[derive(Deserialize)]
                struct Args {
                    status: Option<String>,
                    limit: Option<i64>,
                }
                let args: Args = serde_json::from_value(args).unwrap_or(Args { status: None, limit: None });
                let limit = args.limit.unwrap_or(20);
                let tasks = if let Some(ref status_str) = args.status {
                    if let Some(status) = missiond_core::types::TaskStatus::from_str(status_str) {
                        self.mission.db().get_tasks_by_status(status)?
                    } else {
                        return Ok(ToolResult::error(format!("Invalid status: {}. Use: queued, running, done, failed", status_str)));
                    }
                } else {
                    self.mission.db().get_all_tasks(limit)?
                };
                Ok(ToolResult::json(&tasks))
            }

            "mission_task_ack" => {
                let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
                let since = args_val.get("since").and_then(|v| v.as_i64());
                let tasks = self.mission.db().ack_completed_tasks(since)?;
                Ok(ToolResult::json(&tasks))
            }

            "mission_task_track" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Args { task_id: String }
                let Args { task_id } = serde_json::from_value(args)?;

                // 1. Task status
                let task = self.mission.db().get_task(&task_id)
                    .map_err(|e| anyhow!("DB error: {}", e))?
                    .ok_or_else(|| anyhow!("Task not found: {}", task_id))?;
                let mut result = serde_json::json!({
                    "task": {
                        "id": task.id,
                        "role": task.role,
                        "status": format!("{:?}", task.status),
                        "slotId": task.slot_id,
                        "createdAt": task.created_at,
                        "startedAt": task.started_at,
                        "finishedAt": task.finished_at,
                        "result": task.result,
                        "error": task.error,
                    }
                });

                // 2. Slot PTY status + progress + lastResponse (if assigned)
                if let Some(ref slot_id) = task.slot_id {
                    if let Some(info) = self.pty.get_status(slot_id).await {
                        let mut slot_obj = serde_json::json!({
                            "state": format!("{:?}", info.state),
                            "statusText": info.status_text,
                        });
                        // Session & activity
                        if let Ok(Some(session_uuid)) = self.mission.db().get_slot_session(slot_id) {
                            slot_obj["sessionId"] = json!(session_uuid);
                            if let Ok(Some(conv)) = self.mission.db().get_conversation(&session_uuid) {
                                if let Some(ref jp) = conv.jsonl_path {
                                    if let Ok(md) = std::fs::metadata(jp) {
                                        if let Ok(m) = md.modified() {
                                            slot_obj["lastActivitySecsAgo"] = json!(m.elapsed().unwrap_or_default().as_secs());
                                        }
                                    }
                                }
                            }
                        }
                        // Progress
                        {
                            let progress = self.slot_progress.read().await;
                            if let Some(sp) = progress.get(slot_id) {
                                if sp.total_calls > 0 {
                                    slot_obj["progress"] = serde_json::to_value(sp).unwrap_or_default();
                                }
                            }
                        }
                        // Last response
                        {
                            let responses = self.slot_last_responses.read().await;
                            if let Some(resp) = responses.get(slot_id) {
                                let truncated = if resp.len() > 2048 {
                                    let mut end = 2048;
                                    while end > 0 && !resp.is_char_boundary(end) { end -= 1; }
                                    format!("{}...(truncated)", &resp[..end])
                                } else {
                                    resp.clone()
                                };
                                slot_obj["lastResponse"] = json!(truncated);
                            }
                        }
                        result["slot"] = slot_obj;
                    }
                }

                Ok(ToolResult::json_pretty(&result))
            }

            // ===== Process control =====
            "mission_spawn" => {
                let SpawnArgs {
                    slot_id,
                    visible,
                    auto_restart,
                } = serde_json::from_value(args)?;
                let agent = self
                    .mission
                    .spawn_agent(
                        &slot_id,
                        Some(missiond_core::SpawnOptions {
                            visible: visible.unwrap_or(false),
                            auto_restart: auto_restart.unwrap_or(false),
                        }),
                    )
                    .await?;
                Ok(ToolResult::json(&agent))
            }
            "mission_kill" => {
                let KillArgs { slot_id } = serde_json::from_value(args)?;
                self.mission.kill_agent(&slot_id).await?;
                Ok(ToolResult::json(
                    &serde_json::json!({ "success": true, "slotId": slot_id }),
                ))
            }
            "mission_restart" => {
                let RestartArgs { slot_id, visible } = serde_json::from_value(args)?;
                let agent = self
                    .mission
                    .restart_agent(
                        &slot_id,
                        Some(missiond_core::SpawnOptions {
                            visible: visible.unwrap_or(false),
                            auto_restart: false,
                        }),
                    )
                    .await?;
                Ok(ToolResult::json(&agent))
            }
            "mission_agents" => {
                let agents = self.mission.get_agents();
                Ok(ToolResult::json(&agents))
            }

            // ===== Info =====
            "mission_slots" => Ok(ToolResult::json(&self.mission.list_slots())),
            "mission_inbox" => {
                let InboxArgs {
                    unread_only,
                    limit,
                } = serde_json::from_value(args).unwrap_or(InboxArgs {
                    unread_only: None,
                    limit: None,
                });
                let messages = self
                    .mission
                    .get_inbox(unread_only.unwrap_or(true), limit.unwrap_or(10));
                Ok(ToolResult::json(&messages))
            }

            // ===== PTY =====
            "mission_pty_spawn" => {
                let PTYSpawnArgs {
                    slot_id,
                    wait_for_idle,
                    timeout_secs,
                    auto_restart,
                    mcp_config_path,
                } = serde_json::from_value(args)?;
                let slot = self
                    .mission
                    .list_slots()
                    .into_iter()
                    .find(|s| s.config.id == slot_id)
                    .ok_or_else(|| anyhow!("Slot not found: {}", slot_id))?;

                let pty_slot = missiond_core::PTYSlot {
                    id: slot.config.id.clone(),
                    role: slot.config.role.clone(),
                    cwd: slot.config.cwd.as_deref().map(PathBuf::from),
                };

                // Resolve MCP config: arg > slot config > None
                let mcp_config = mcp_config_path
                    .or(slot.config.mcp_config.clone())
                    .map(PathBuf::from);

                let (extra_env, session_file) = build_slot_tracking_env(&slot_id, slot.config.env.as_ref()).await;
                let wait = wait_for_idle.unwrap_or(false);
                let info = self
                    .pty
                    .spawn(
                        &pty_slot,
                        PTYSpawnOptions {
                            auto_restart: auto_restart.unwrap_or(false),
                            wait_for_idle: wait,
                            timeout_secs,
                            mcp_config,
                            dangerously_skip_permissions: slot.config.dangerously_skip_permissions.unwrap_or(false),
                            extra_env,
                        },
                    )
                    .await?;

                // Capture UUID after spawn (only reliable when we waited for idle)
                if wait {
                    capture_slot_session_uuid(self, &slot_id, &session_file).await;
                }
                Ok(ToolResult::json(&info))
            }
            "mission_pty_send" => {
                let PTYSendArgs {
                    slot_id,
                    message,
                    wait_for_response,
                    timeout_ms,
                } = serde_json::from_value(args)?;

                if wait_for_response.unwrap_or(false) {
                    // Blocking mode: send and wait for response
                    let timeout_ms = timeout_ms.unwrap_or(300_000);
                    let res = self.pty.send(&slot_id, &message, timeout_ms).await?;
                    let state = self
                        .pty
                        .get_status(&slot_id)
                        .await
                        .map(|s| format!("{:?}", s.state))
                        .unwrap_or_else(|| "unknown".to_string());
                    Ok(ToolResult::json(&serde_json::json!({
                        "delivered": true,
                        "response": res.response,
                        "durationMs": res.duration_ms,
                        "state": state,
                    })))
                } else {
                    // Fire-and-forget: send and return immediately
                    self.pty.send_fire_and_forget(&slot_id, &message).await?;
                    Ok(ToolResult::json(&serde_json::json!({
                        "delivered": true,
                        "mode": "fire-and-forget",
                        "hint": "Use pty_status to poll for completion (state returns to Idle when done)",
                    })))
                }
            }
            "mission_pty_kill" => {
                let PTYKillArgs { slot_id } = serde_json::from_value(args)?;
                self.pty.kill(&slot_id).await?;
                // Requeue any Running submit tasks assigned to this slot
                let requeued = self.mission.db().requeue_running_tasks_for_slot(&slot_id).unwrap_or(0);
                // Release any board task claims held by this slot
                let claims_released = self.mission.db().release_board_claims_by_executor(&slot_id).unwrap_or(0);
                Ok(ToolResult::json(
                    &serde_json::json!({ "success": true, "slotId": slot_id, "requeuedTasks": requeued, "claimsReleased": claims_released }),
                ))
            }
            "mission_pty_screen" => {
                let PTYScreenArgs { slot_id, lines } = serde_json::from_value(args)?;
                if let Some(n) = lines {
                    let last = self.pty.get_last_lines(&slot_id, n).await?;
                    Ok(ToolResult::text(last.join("\n")))
                } else {
                    Ok(ToolResult::text(self.pty.get_screen(&slot_id).await?))
                }
            }
            "mission_pty_history" => {
                let PTYHistoryArgs { slot_id } = serde_json::from_value(args)?;
                let history = self.pty.get_history(&slot_id).await;
                Ok(ToolResult::json(&history))
            }
            "mission_pty_status" => {
                let PTYStatusArgs { slot_id } = serde_json::from_value(args).unwrap_or(PTYStatusArgs {
                    slot_id: None,
                });
                if let Some(slot_id) = slot_id {
                    let status = self.pty.get_status(&slot_id).await;
                    match status {
                        Some(info) => {
                            let mut obj = serde_json::to_value(&info).unwrap_or_default();
                            // Enrich with session_uuid and JSONL activity
                            if let Ok(Some(session_uuid)) = self.mission.db().get_slot_session(&slot_id) {
                                obj["sessionId"] = json!(session_uuid);
                                if let Ok(Some(conv)) = self.mission.db().get_conversation(&session_uuid) {
                                    if let Some(ref jsonl_path) = conv.jsonl_path {
                                        if let Ok(metadata) = std::fs::metadata(jsonl_path) {
                                            if let Ok(modified) = metadata.modified() {
                                                let secs_ago = modified.elapsed().unwrap_or_default().as_secs();
                                                obj["lastActivitySecsAgo"] = json!(secs_ago);
                                            }
                                        }
                                    }
                                }
                            }
                            // Attach JSONL progress stats (tool call counts, current tool)
                            {
                                let progress = self.slot_progress.read().await;
                                if let Some(sp) = progress.get(&slot_id) {
                                    if sp.total_calls > 0 {
                                        obj["progress"] = serde_json::to_value(sp).unwrap_or_default();
                                    }
                                }
                            }
                            // Attach last response (eliminates screen-blank misjudgment)
                            {
                                let responses = self.slot_last_responses.read().await;
                                if let Some(resp) = responses.get(&slot_id) {
                                    // Truncate to 2KB for status response
                                    let truncated = if resp.len() > 2048 {
                                        let mut end = 2048;
                                        while end > 0 && !resp.is_char_boundary(end) { end -= 1; }
                                        format!("{}...(truncated)", &resp[..end])
                                    } else {
                                        resp.clone()
                                    };
                                    obj["lastResponse"] = json!(truncated);
                                }
                            }
                            Ok(ToolResult::json(&obj))
                        }
                        None => Ok(ToolResult::json(&serde_json::Value::Null)),
                    }
                } else {
                    let all = self.pty.get_all_status().await;
                    Ok(ToolResult::json(&all))
                }
            }
            "mission_pty_confirm" => {
                let PTYConfirmArgs { slot_id, response } = serde_json::from_value(args)?;
                let response_echo = response.clone();

                // Capture pending confirm info before confirming (for auto-allow recording)
                let pending = self.pty.get_pending_confirm(&slot_id).await;

                // Map Node-style response (boolean/number/string) to PTY confirm input.
                let resp = match response {
                    Value::Bool(true) => missiond_core::ConfirmResponse::Yes,
                    Value::Bool(false) => missiond_core::ConfirmResponse::No,
                    Value::Number(n) => {
                        let n = n.as_u64().unwrap_or(1) as usize;
                        if n == 1 {
                            missiond_core::ConfirmResponse::Yes
                        } else if n == 3 {
                            missiond_core::ConfirmResponse::No
                        } else {
                            missiond_core::ConfirmResponse::Option(n)
                        }
                    }
                    Value::String(s) => {
                        if s == "y" || s == "Y" || s == "1" {
                            missiond_core::ConfirmResponse::Yes
                        } else if s == "n" || s == "N" || s == "3" {
                            missiond_core::ConfirmResponse::No
                        } else if let Ok(n) = s.parse::<usize>() {
                            if n == 1 {
                                missiond_core::ConfirmResponse::Yes
                            } else if n == 3 {
                                missiond_core::ConfirmResponse::No
                            } else {
                                missiond_core::ConfirmResponse::Option(n)
                            }
                        } else {
                            // Fallback: write raw input + enter
                            let response_text = s.clone();
                            let input = format!("{}\r", s);
                            self.pty.write(&slot_id, &input).await?;
                            return Ok(ToolResult::json(&serde_json::json!({
                                "success": true,
                                "slotId": slot_id,
                                "response": response_text,
                            })));
                        }
                    }
                    _ => missiond_core::ConfirmResponse::Yes,
                };

                // Determine if this is an approval (Yes, Option(1), Option(2))
                let is_approval = matches!(
                    resp,
                    missiond_core::ConfirmResponse::Yes
                        | missiond_core::ConfirmResponse::Option(1)
                        | missiond_core::ConfirmResponse::Option(2)
                );

                self.pty.confirm(&slot_id, resp).await?;

                // Auto-record permission for approved tool confirmations
                if is_approval {
                    if let Some(ref info) = pending {
                        if let Some(ref tool) = info.tool {
                            if let Some(status) = self.pty.get_status(&slot_id).await {
                                self.permission.add_role_auto_allow(&status.role, &tool.name);
                                info!(
                                    role = %status.role,
                                    pattern = %tool.name,
                                    slot_id = %slot_id,
                                    "Auto-allow recorded after confirm approval"
                                );
                            }
                        }
                    }
                }

                Ok(ToolResult::json(&serde_json::json!({
                    "success": true,
                    "slotId": slot_id,
                    "response": response_echo,
                })))
            }
            "mission_pty_interrupt" => {
                let PTYInterruptArgs { slot_id } = serde_json::from_value(args)?;
                self.pty.interrupt(&slot_id).await?;
                Ok(ToolResult::json(
                    &serde_json::json!({ "success": true, "slotId": slot_id }),
                ))
            }
            "mission_pty_logs" => {
                let PTYLogsArgs { slot_id } = serde_json::from_value(args)?;
                let status = self.pty.get_status(&slot_id).await;
                let status = status.ok_or_else(|| anyhow!("PTY session not found"))?;
                // Find JSONL path from slot→session DB mapping
                let jsonl_path = self.mission.db().get_all_slot_sessions().ok()
                    .and_then(|sessions| sessions.into_iter().find(|(sid, _)| sid == &slot_id))
                    .and_then(|(_, session_uuid)| {
                        self.mission.db().get_conversation(&session_uuid).ok().flatten()
                            .and_then(|c| c.jsonl_path)
                    });
                #[cfg(unix)]
                let hint = if let Some(ref jp) = jsonl_path {
                    format!("tail -f {}", jp)
                } else {
                    format!("tail -f {}", status.log_file.display())
                };
                #[cfg(windows)]
                let hint = if let Some(ref jp) = jsonl_path {
                    format!("Get-Content -Path \"{}\" -Wait -Tail 50", jp)
                } else {
                    format!("Get-Content -Path \"{}\" -Wait -Tail 50", status.log_file.display())
                };

                Ok(ToolResult::json(&serde_json::json!({
                    "slotId": slot_id,
                    "logFile": status.log_file,
                    "jsonlFile": jsonl_path,
                    "hint": hint,
                })))
            }

            "mission_pty_screenshot" => {
                let PTYScreenshotArgs { slot_id } = serde_json::from_value(args)?;
                let screenshots_dir = default_mission_home().join("screenshots");
                std::fs::create_dir_all(&screenshots_dir)?;

                // Try browser-based screenshot first (real xterm.js canvas)
                let (request_id, rx) = self.screenshot_broker.request(&slot_id).await;
                let _ = &request_id; // request is already broadcast by broker

                let browser_result = tokio::time::timeout(
                    self.screenshot_broker.timeout,
                    rx,
                ).await;

                let (path, source) = match browser_result {
                    Ok(Ok(Ok(result))) => {
                        let ts = chrono::Utc::now().timestamp_millis();
                        let filename = format!("{}-{}.png", slot_id, ts);
                        let path = screenshots_dir.join(&filename);
                        std::fs::write(&path, &result.png_data)?;
                        (path, "browser")
                    }
                    _ => {
                        // Fallback: backend rendering via alacritty grid
                        info!(slot_id = %slot_id, "Browser screenshot unavailable, using backend rendering");
                        let path = self.pty.screenshot(&slot_id, &screenshots_dir).await?;
                        (path, "backend")
                    }
                };

                Ok(ToolResult::json(&serde_json::json!({
                    "path": path.to_string_lossy(),
                    "source": source,
                    "hint": "Use the Read tool to view this PNG image"
                })))
            }

            // ===== Permission =====
            "mission_permission_get" => Ok(ToolResult::json_pretty(
                &self.permission.get_config(),
            )),
            "mission_permission_set_role" => {
                let SetRolePermissionArgs { role, rule } = serde_json::from_value(args)?;
                self.permission.set_role_rule(&role, rule.clone());
                Ok(ToolResult::json(&serde_json::json!({
                    "success": true,
                    "role": role,
                    "rule": rule,
                })))
            }
            "mission_permission_set_slot" => {
                let SetSlotPermissionArgs { slot_id, rule } = serde_json::from_value(args)?;
                self.permission.set_slot_rule(&slot_id, rule.clone());
                Ok(ToolResult::json(&serde_json::json!({
                    "success": true,
                    "slotId": slot_id,
                    "rule": rule,
                })))
            }
            "mission_permission_add_auto_allow" => {
                let AddAutoAllowArgs {
                    role,
                    slot_id,
                    pattern,
                } = serde_json::from_value(args)?;
                if let Some(role) = role {
                    self.permission.add_role_auto_allow(&role, &pattern);
                    Ok(ToolResult::json(&serde_json::json!({
                        "success": true,
                        "role": role,
                        "pattern": pattern,
                    })))
                } else if let Some(slot_id) = slot_id {
                    self.permission.add_slot_auto_allow(&slot_id, &pattern);
                    Ok(ToolResult::json(&serde_json::json!({
                        "success": true,
                        "slotId": slot_id,
                        "pattern": pattern,
                    })))
                } else {
                    Ok(ToolResult::error("Must specify role or slotId"))
                }
            }
            "mission_permission_reload" => {
                self.permission.reload();
                Ok(ToolResult::json(&serde_json::json!({ "success": true })))
            }

            // ===== Claude Code Tasks =====
            "mission_cc_sessions" => {
                let CCSessionsArgs {
                    project_path,
                    active_only,
                } = serde_json::from_value(args).unwrap_or(CCSessionsArgs {
                    project_path: None,
                    active_only: None,
                });
                let active_only = active_only.unwrap_or(true);

                let sessions = {
                    let cc = self.cc_tasks.lock().await;
                    if active_only {
                        cc.get_active_sessions().await
                    } else {
                        cc.get_all_sessions().await
                    }
                };

                let mut sessions = sessions;
                if let Some(filter) = project_path {
                    sessions = sessions
                        .into_iter()
                        .filter(|s| s.project_path.contains(&filter) || s.project_name.contains(&filter))
                        .collect();
                }

                let result: Vec<Value> = sessions
                    .into_iter()
                    .map(|s| {
                        let mut pending = 0;
                        let mut in_progress = 0;
                        let mut completed = 0;
                        for t in &s.tasks {
                            match t.status {
                                missiond_core::CCTaskStatus::Pending => pending += 1,
                                missiond_core::CCTaskStatus::InProgress => in_progress += 1,
                                missiond_core::CCTaskStatus::Completed => completed += 1,
                            }
                        }

                        serde_json::json!({
                            "sessionId": s.session_id,
                            "project": s.project_name,
                            "summary": s.summary,
                            "tasks": s.tasks.len(),
                            "inProgress": in_progress,
                            "pending": pending,
                            "completed": completed,
                            "modified": s.modified,
                            "isActive": s.is_active,
                        })
                    })
                    .collect();

                Ok(ToolResult::json_pretty(&result))
            }
            "mission_cc_tasks" => {
                let CCTasksArgs {
                    session_id,
                    project_path,
                } = serde_json::from_value(args).unwrap_or(CCTasksArgs {
                    session_id: None,
                    project_path: None,
                });

                if let Some(session_id) = session_id {
                    let tasks = {
                        let cc = self.cc_tasks.lock().await;
                        cc.get_session_tasks(&session_id).await
                    };
                    if let Some(tasks) = tasks {
                        return Ok(ToolResult::json_pretty(&tasks));
                    }
                    return Ok(ToolResult::error("Session not found"));
                }

                if let Some(project_path) = project_path {
                    let sessions = {
                        let cc = self.cc_tasks.lock().await;
                        cc.get_sessions_by_project(&project_path).await
                    };
                    let result: Vec<Value> = sessions
                        .into_iter()
                        .map(|s| {
                            serde_json::json!({
                                "sessionId": s.session_id,
                                "summary": s.summary,
                                "tasks": s.tasks,
                            })
                        })
                        .collect();
                    return Ok(ToolResult::json_pretty(&result));
                }

                Ok(ToolResult::error("Provide sessionId or projectPath"))
            }
            "mission_cc_overview" => {
                let overview = { self.cc_tasks.lock().await.get_overview().await };
                Ok(ToolResult::json_pretty(&overview))
            }
            "mission_cc_in_progress" => {
                let in_progress = { self.cc_tasks.lock().await.get_in_progress_tasks().await };
                let result: Vec<Value> = in_progress
                    .into_iter()
                    .map(|item| {
                        serde_json::json!({
                            "sessionId": item.session_id,
                            "project": item.project_name,
                            "summary": item.summary,
                            "task": item.task.content,
                            "activeForm": item.task.active_form,
                            "modified": item.modified,
                        })
                    })
                    .collect();
                Ok(ToolResult::json_pretty(&result))
            }
            "mission_cc_trigger_swarm" => {
                let CCTriggerSwarmArgs {
                    slot_id,
                    tasks,
                    teammate_count,
                    timeout_ms,
                } = serde_json::from_value(args)?;
                let teammate_count = teammate_count.unwrap_or(3);
                let timeout_ms = timeout_ms.unwrap_or(600_000);

                let prompt = format!(
                    "请进入 Plan 模式，创建以下任务，然后用 {} 个 teammate 并行执行：\n\n{}\n\n完成后汇报结果。",
                    teammate_count,
                    tasks
                        .iter()
                        .enumerate()
                        .map(|(i, t)| format!("{}. {}", i + 1, t))
                        .collect::<Vec<_>>()
                        .join("\n")
                );

                let res = self.pty.send(&slot_id, &prompt, timeout_ms).await?;
                Ok(ToolResult::text(res.response))
            }

            // ===== Health =====
            "mission_health" => {
                let agents = self.pty.get_all_status().await;
                let pty_status: Vec<Value> = agents
                    .iter()
                    .map(|a| {
                        serde_json::json!({
                            "slotId": a.slot_id,
                            "state": a.state,
                            "pid": a.pid,
                        })
                    })
                    .collect();

                Ok(ToolResult::json(&serde_json::json!({
                    "status": "ok",
                    "ipc": "connected",
                    "wsPort": ws_port(),
                    "pty": pty_status,
                    "uptime": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                })))
            }

            // ===== Knowledge Base (Jarvis Memory) =====
            "mission_kb_remember" => {
                let args: KBRememberArgs = serde_json::from_value(args)?;
                let input = missiond_core::types::KBRememberInput {
                    category: args.category,
                    key: args.key,
                    summary: args.summary,
                    detail: args.detail,
                    source: args.source,
                    confidence: args.confidence,
                };
                let entry = self.mission.db()
                    .kb_remember(&input)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json_pretty(&entry))
            }
            "mission_kb_forget" => {
                let KBKeyArgs { key } = serde_json::from_value(args)?;
                let deleted = self.mission.db()
                    .kb_forget(&key)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json(&serde_json::json!({
                    "deleted": deleted,
                    "key": key,
                })))
            }
            "mission_kb_batch_forget" => {
                let keys: Vec<String> = serde_json::from_value(
                    args.get("keys").cloned().unwrap_or(serde_json::Value::Array(vec![]))
                ).map_err(|e| anyhow!("Invalid keys: {}", e))?;
                if keys.is_empty() {
                    return Ok(ToolResult::error("keys array is empty"));
                }
                let count = self.mission.db()
                    .kb_batch_forget(&keys)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json(&serde_json::json!({
                    "deleted_count": count,
                    "requested_keys": keys.len(),
                })))
            }
            "mission_kb_search" => {
                let KBSearchArgs { query, category } = serde_json::from_value(args)
                    .unwrap_or(KBSearchArgs { query: None, category: None });
                let query = query.unwrap_or_default();
                if query.is_empty() && category.is_none() {
                    // No query: return recent entries
                    let entries = self.mission.db()
                        .kb_list(None)
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json_pretty(&entries))
                } else {
                    let results = self.mission.db()
                        .kb_search(&query, category.as_deref())
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json_pretty(&results))
                }
            }
            "mission_kb_get" => {
                let KBKeyArgs { key } = serde_json::from_value(args)?;
                let entry = self.mission.db()
                    .kb_get(&key)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                match entry {
                    Some(e) => Ok(ToolResult::json_pretty(&e)),
                    None => Ok(ToolResult::error(format!("Key not found: {}", key))),
                }
            }
            "mission_kb_list" => {
                let KBListArgs { category } =
                    serde_json::from_value(args).unwrap_or(KBListArgs { category: None });
                let entries = self.mission.db()
                    .kb_list(category.as_deref())
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json_pretty(&entries))
            }
            "mission_kb_import" => {
                let KBImportArgs { format, path } = serde_json::from_value(args)?;
                match format.as_str() {
                    "servers_yaml" => {
                        let yaml_path = path
                            .map(std::path::PathBuf::from)
                            .unwrap_or_else(|| default_mission_home().join("servers.yaml"));
                        let infra = missiond_core::InfraConfig::load(&yaml_path);
                        let mut imported = 0;
                        for server in &infra.servers {
                            let detail = serde_json::to_value(server).ok();
                            let summary = format!(
                                "{} ({}) — {}",
                                server.name,
                                server.provider,
                                server.roles.join(", ")
                            );
                            let input = missiond_core::types::KBRememberInput {
                                category: "infra".to_string(),
                                key: server.id.clone(),
                                summary,
                                detail,
                                source: Some("import".to_string()),
                                confidence: Some(1.0),
                            };
                            self.mission.db()
                                .kb_remember(&input)
                                .map_err(|e| anyhow!("DB error: {}", e))?;
                            imported += 1;
                        }
                        Ok(ToolResult::json(&serde_json::json!({
                            "imported": imported,
                            "source": yaml_path.display().to_string(),
                        })))
                    }
                    _ => Ok(ToolResult::error(format!("Unsupported import format: {}", format))),
                }
            }

            "mission_kb_discover" => {
                let KBDiscoverArgs { host, port, password } = serde_json::from_value(args)?;

                // Resolve host: if it looks like an infra key (no @ or .), try infra registry
                let (ssh_user, ssh_host, ssh_port, ssh_pass) = if !host.contains('@') && !host.contains('.') {
                    // Try infra registry lookup
                    let server = self.infra.get(&host);
                    let ip = server.and_then(|s| s.host.as_deref()).unwrap_or(&host);
                    // Look up credentials from KB
                    let db = self.mission.db();
                    let cred_pass = db.kb_search(&format!("{} password", host), Some("credential"))
                        .ok()
                        .and_then(|entries| entries.into_iter().next())
                        .and_then(|e| e.detail.as_ref().and_then(|d| d.get("password").and_then(|v| v.as_str().map(String::from))));
                    ("root".to_string(), ip.to_string(), port.unwrap_or(22), password.or(cred_pass))
                } else if host.contains('@') {
                    let parts: Vec<&str> = host.splitn(2, '@').collect();
                    (parts[0].to_string(), parts[1].to_string(), port.unwrap_or(22), password)
                } else {
                    ("root".to_string(), host.clone(), port.unwrap_or(22), password)
                };

                // Build probe script (piped to remote bash to avoid quoting issues)
                let probe_script = concat!(
                    "echo \"HOSTNAME=$(hostname)\"\n",
                    "echo \"UNAME=$(uname -a)\"\n",
                    "echo \"OS=$(. /etc/os-release 2>/dev/null && echo \"$PRETTY_NAME\" || echo unknown)\"\n",
                    "echo \"CPU=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo unknown)\"\n",
                    "MEM=$(LANG=C free -h 2>/dev/null | awk '/Mem:/{print $2}'); echo \"MEM=${MEM:-unknown}\"\n",
                    "DISK=$(LANG=C df -h / 2>/dev/null | awk 'NR==2{print $2}'); echo \"DISK=${DISK:-unknown}\"\n",
                    "echo \"UPTIME=$(uptime -p 2>/dev/null || uptime || echo unknown)\"\n",
                    "DOCKER=$(docker ps --format '{{.Names}}:{{.Image}}' 2>/dev/null | tr '\\n' ','); echo \"DOCKER=${DOCKER:-none}\"\n",
                    "LISTEN=$(LANG=C ss -tlnp 2>/dev/null | awk 'NR>1{print $4}' | tr '\\n' ','); echo \"LISTEN=${LISTEN:-unknown}\"\n",
                );

                // Build SSH command args (pipe probe_script to stdin)
                let mut ssh_args: Vec<String> = Vec::new();
                if let Some(ref pass) = ssh_pass {
                    ssh_args.extend(["sshpass".into(), "-p".into(), pass.clone(), "ssh".into()]);
                } else {
                    ssh_args.push("ssh".into());
                    ssh_args.extend(["-o".into(), "BatchMode=yes".into()]);
                }
                ssh_args.extend([
                    "-o".into(), "StrictHostKeyChecking=no".into(),
                    "-o".into(), "ConnectTimeout=10".into(),
                    "-p".into(), ssh_port.to_string(),
                    format!("{}@{}", ssh_user, ssh_host),
                    "bash".into(),
                ]);

                let program = ssh_args.remove(0);
                let mut cmd = tokio::process::Command::new(&program);
                cmd.args(&ssh_args);
                cmd.stdin(std::process::Stdio::piped());
                cmd.stdout(std::process::Stdio::piped());
                cmd.stderr(std::process::Stdio::piped());

                let mut child = cmd.spawn()
                    .map_err(|e| anyhow!("Failed to spawn SSH: {}", e))?;

                // Write probe script to stdin
                if let Some(mut stdin) = child.stdin.take() {
                    use tokio::io::AsyncWriteExt;
                    stdin.write_all(probe_script.as_bytes()).await.ok();
                    drop(stdin);
                }

                let output = child.wait_with_output().await
                    .map_err(|e| anyhow!("SSH failed: {}", e))?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Ok(ToolResult::error(format!("SSH probe failed: {}", stderr.trim())));
                }

                // Parse key=value output
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut detail = serde_json::Map::new();
                for line in stdout.lines() {
                    if let Some((k, v)) = line.split_once('=') {
                        let key = k.trim().to_lowercase();
                        let val = v.trim().to_string();
                        if !val.is_empty() && val != "unknown" && val != "none" {
                            detail.insert(key, serde_json::Value::String(val));
                        }
                    }
                }

                // Add connection info
                detail.insert("ssh_user".to_string(), serde_json::Value::String(ssh_user.clone()));
                detail.insert("ssh_host".to_string(), serde_json::Value::String(ssh_host.clone()));
                if ssh_port != 22 {
                    detail.insert("ssh_port".to_string(), serde_json::Value::Number(ssh_port.into()));
                }

                // Build summary
                let hostname = detail.get("hostname").and_then(|v| v.as_str()).unwrap_or("unknown");
                let os = detail.get("os").and_then(|v| v.as_str()).unwrap_or("unknown");
                let cpu = detail.get("cpu").and_then(|v| v.as_str()).unwrap_or("?");
                let mem = detail.get("mem").and_then(|v| v.as_str()).unwrap_or("?");
                let summary = format!("{} — {} ({}C, {})", hostname, os, cpu, mem);

                // Derive a key from hostname or host
                let key = hostname.to_lowercase().replace(' ', "-");

                let input = missiond_core::types::KBRememberInput {
                    category: "infra".to_string(),
                    key: key.clone(),
                    summary: summary.clone(),
                    detail: Some(serde_json::Value::Object(detail.clone())),
                    source: Some("discovery".to_string()),
                    confidence: Some(1.0),
                };
                self.mission.db()
                    .kb_remember(&input)
                    .map_err(|e| anyhow!("DB error: {}", e))?;

                Ok(ToolResult::json(&serde_json::json!({
                    "status": "discovered",
                    "key": key,
                    "summary": summary,
                    "detail": detail,
                })))
            }

            "mission_kb_gc" => {
                let KBGCArgs { action, days } = serde_json::from_value(args)?;
                let db = self.mission.db();
                match action.as_str() {
                    "stats" => {
                        let stats = db.kb_stats()
                            .map_err(|e| anyhow!("DB error: {}", e))?;
                        Ok(ToolResult::json_pretty(&stats))
                    }
                    "stale" => {
                        let threshold = days.unwrap_or(30);
                        let stale = db.kb_find_stale(threshold)
                            .map_err(|e| anyhow!("DB error: {}", e))?;
                        Ok(ToolResult::json(&serde_json::json!({
                            "threshold_days": threshold,
                            "count": stale.len(),
                            "entries": stale.iter().map(|e| serde_json::json!({
                                "category": e.category,
                                "key": e.key,
                                "summary": e.summary,
                                "updatedAt": e.updated_at,
                            })).collect::<Vec<_>>(),
                        })))
                    }
                    "duplicates" => {
                        let dups = db.kb_find_duplicates()
                            .map_err(|e| anyhow!("DB error: {}", e))?;
                        Ok(ToolResult::json(&serde_json::json!({
                            "count": dups.len(),
                            "pairs": dups.iter().map(|(a, b, sim)| serde_json::json!({
                                "similarity": format!("{:.2}", sim),
                                "a": {"category": a.category, "key": a.key, "summary": a.summary, "accessCount": a.access_count},
                                "b": {"category": b.category, "key": b.key, "summary": b.summary, "accessCount": b.access_count},
                            })).collect::<Vec<_>>(),
                        })))
                    }
                    "clean_stale" => {
                        let threshold = days.unwrap_or(30);
                        let stale = db.kb_find_stale(threshold)
                            .map_err(|e| anyhow!("DB error: {}", e))?;
                        let keys: Vec<String> = stale.iter().map(|e| e.key.clone()).collect();
                        let count = db.kb_batch_forget(&keys)
                            .map_err(|e| anyhow!("DB error: {}", e))?;
                        Ok(ToolResult::json(&serde_json::json!({
                            "action": "clean_stale",
                            "threshold_days": threshold,
                            "deleted": count,
                            "keys": keys,
                        })))
                    }
                    "clean_duplicates" => {
                        let dups = db.kb_find_duplicates()
                            .map_err(|e| anyhow!("DB error: {}", e))?;
                        // Keep entry with higher access_count (or newer updated_at), delete the other
                        let mut to_delete = Vec::new();
                        let mut seen = std::collections::HashSet::new();
                        for (a, b, sim) in &dups {
                            // Skip if either already marked for deletion
                            if seen.contains(&a.key) || seen.contains(&b.key) { continue; }
                            let loser = if a.access_count > b.access_count {
                                &b.key
                            } else if b.access_count > a.access_count {
                                &a.key
                            } else if a.updated_at >= b.updated_at {
                                &b.key
                            } else {
                                &a.key
                            };
                            to_delete.push(serde_json::json!({
                                "deleted_key": loser,
                                "kept_key": if loser == &a.key { &b.key } else { &a.key },
                                "similarity": format!("{:.2}", sim),
                            }));
                            seen.insert(loser.clone());
                        }
                        let keys: Vec<String> = to_delete.iter()
                            .filter_map(|d| d["deleted_key"].as_str().map(String::from))
                            .collect();
                        let count = db.kb_batch_forget(&keys)
                            .map_err(|e| anyhow!("DB error: {}", e))?;
                        Ok(ToolResult::json(&serde_json::json!({
                            "action": "clean_duplicates",
                            "deleted": count,
                            "details": to_delete,
                        })))
                    }
                    _ => Ok(ToolResult::error(format!("Unknown gc action: {}. Use: stats, stale, duplicates, clean_stale, clean_duplicates", action))),
                }
            }

            // ===== KB Analysis (via external AI) =====
            "mission_kb_analyze" => {
                // Parse parameters
                let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
                let mode = args_val.get("mode").and_then(|v| v.as_str()).unwrap_or("overview");
                let target_category = args_val.get("target_category").and_then(|v| v.as_str());
                let limit = args_val.get("limit").and_then(|v| v.as_u64()).unwrap_or(500) as u32;
                let offset = args_val.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let custom_prompt = args_val.get("custom_prompt").and_then(|v| v.as_str());
                let include_board_context = args_val.get("include_board_context")
                    .and_then(|v| v.as_bool()).unwrap_or(false);
                let model: String = args_val.get("model").and_then(|v| v.as_str())
                    .unwrap_or("gemini-3.1-pro").to_string();
                let max_tokens: u32 = args_val.get("max_tokens").and_then(|v| v.as_u64())
                    .unwrap_or(16384) as u32;

                // 1. Read KB entries with pagination
                let entries = self.mission.db()
                    .kb_list_paginated(target_category, limit, offset)
                    .map_err(|e| anyhow!("DB error: {}", e))?;

                if entries.is_empty() {
                    return Ok(ToolResult::error("No KB entries found for the given filter."));
                }

                // Also get total count for pagination info
                let total_count = self.mission.db()
                    .kb_list(target_category.map(|s| s))
                    .map(|v| v.len())
                    .unwrap_or(0);

                // 2. Build JSONL with metadata (compact format for LLM)
                let now = chrono::Utc::now();
                let mut jsonl_lines = Vec::with_capacity(entries.len());
                let include_detail = mode == "consolidation_plan";

                for e in &entries {
                    if e.category == "credential" { continue; }
                    let sanitized_summary = missiond_core::db::MissionDB::redact_sensitive(&e.summary);

                    // Calculate age in days
                    let age_days = chrono::DateTime::parse_from_rfc3339(&e.updated_at)
                        .map(|dt| (now - dt.with_timezone(&chrono::Utc)).num_days())
                        .unwrap_or(0);

                    let mut item = serde_json::json!({
                        "category": e.category,
                        "key": e.key,
                        "summary": sanitized_summary,
                        "access_count": e.access_count,
                        "age_days": age_days,
                        "confidence": e.confidence,
                    });

                    if include_detail {
                        if let Some(detail) = &e.detail {
                            let detail_str = match detail {
                                serde_json::Value::String(s) => s.clone(),
                                other => other.to_string(),
                            };
                            let sanitized_detail = missiond_core::db::MissionDB::redact_sensitive(&detail_str);
                            item["detail"] = serde_json::Value::String(sanitized_detail);
                        }
                    }

                    jsonl_lines.push(serde_json::to_string(&item).unwrap_or_default());
                }
                let kb_jsonl = jsonl_lines.join("\n");

                // 2b. Build Board context if requested
                let board_context = if include_board_context {
                    let tasks = self.mission.db().list_board_tasks(None, false)
                        .unwrap_or_default();
                    let mut open_lines = Vec::new();
                    let mut done_lines = Vec::new();
                    for t in &tasks {
                        let line = format!("{} {}", &t.id[..8], t.title);
                        match t.status {
                            missiond_core::types::BoardTaskStatus::Done => done_lines.push(line),
                            _ => open_lines.push(line),
                        }
                    }
                    Some(format!(
                        "[Board Tasks Context]\n<open>\n{}\n<done>\n{}",
                        open_lines.join("\n"),
                        done_lines.join("\n")
                    ))
                } else {
                    None
                };

                // 3. Build prompt and optional response_format based on mode
                let mut response_format: Option<serde_json::Value> = None;
                let analysis_prompt = match mode {
                    "consolidation_plan" => {
                        response_format = Some(serde_json::json!({
                            "type": "json_schema",
                            "json_schema": {
                                "name": "kb_consolidation_actions",
                                "strict": false,
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "actions": {
                                            "type": "array",
                                            "items": {
                                                "type": "object",
                                                "properties": {
                                                    "action_type": { "type": "string", "enum": ["merge", "delete", "update", "distill"] },
                                                    "target_keys": {
                                                        "type": "array",
                                                        "items": { "type": "string" },
                                                        "description": "Keys of entries to delete or merge"
                                                    },
                                                    "new_entry": {
                                                        "type": "object",
                                                        "properties": {
                                                            "category": { "type": "string" },
                                                            "key": { "type": "string" },
                                                            "summary": { "type": "string" },
                                                            "confidence": { "type": "number" }
                                                        },
                                                        "required": ["category", "key", "summary"]
                                                    },
                                                    "linked_task_id": {
                                                        "type": "string",
                                                        "description": "Matched Board Task ID (short, 8-char prefix)"
                                                    },
                                                    "reason": { "type": "string" }
                                                },
                                                "required": ["action_type", "target_keys", "reason"]
                                            }
                                        }
                                    },
                                    "required": ["actions"]
                                }
                            }
                        }));

                        let board_section = if let Some(ref ctx) = board_context {
                            format!("\n\n=== BOARD TASKS CONTEXT ===\n{}\n===========================\n\n\
                                === 任务感知整理规则 ===\n\
                                1. 关联 Open 任务的 KB → 保护，不删不合并，仅用 update 补 linked_task_id\n\
                                2. 关联 Done 任务的 KB → distill 蒸馏收敛（提取最终结论升维到 architecture/feature，删流水账）\n\
                                3. 无关联 Orphan → preference/ops/platform 保留；老旧 debug/bugfix 可删除\n\
                                4. 每个 action 的 reason 中说明关联了哪个任务（或标注 orphan）\n", ctx)
                        } else {
                            String::new()
                        };

                        format!(
                            "你是 MissionD 的知识库自治整理引擎。分析传入的 JSONL 条目，生成可执行的清理计划。\n\n\
                            规则：\n\
                            1. 重复/相似合并 (merge)：相同主题不同措辞 → 合并，保留最高 confidence，汇总 summary\n\
                            2. 碎片整合 (update)：松散相关条目 → 整合成连贯大条目\n\
                            3. 过时清理 (delete)：基于 age_days 和 access_count，旧策略已被覆盖的条目\n\
                            4. 类别修正 (update)：category 放错的条目移到正确分类\n\
                            5. 蒸馏收敛 (distill)：已完成项目的多条流水账 → 提取最终结论为一条精华\n\n\
                            保守原则：不确定就不动。每个 action 必须有 reason。\
                            {}\n\n共 {} 条（第 {} 到 {} 条）：\n{}",
                            board_section,
                            entries.len(), offset + 1, offset + entries.len() as u32, kb_jsonl
                        )
                    }
                    "custom" => {
                        format!(
                            "{}\n\n知识库数据（共 {} 条，JSONL格式）：\n{}",
                            custom_prompt.unwrap_or("请分析以下知识库数据。"),
                            entries.len(), kb_jsonl
                        )
                    }
                    _ => { // overview - 查重+升维版
                        format!(
                            "作为 MissionD 核心系统的知识管理专家，请深度审查以下知识库（KB）条目。\n\n
请务必重点完成以下两项任务，并给出具体的、可操作的建议：\n\n
1. 【冗余与合并分析】\n
- 识别同一问题的多阶段记录、重复的调试流水账（尤其是频繁触发的遗留 bug 记录）。\n
- 明确列出建议合并保留的主 key 和建议删除的冗余 key 列表。\n\n
2. 【类别纠偏与升维建议】\n
- 严格对照系统的生命周期与分类约束：\n
  * preference: 用户偏好/纠正/否定 (长期保留保护)\n
  * memory:decision / memory:architecture: 架构决策/核心技术事实 (长期保留保护)\n
  * project: 项目专属上下文 (长期保留保护)\n
  * memory:bugfix: 已修复 bug 的根因分析 (30天 GC)\n
  * memory:debug: 调试弯路/临时排查经验 (短周期 GC)\n
  * memory:ops: 运维基建/CI脚本/痛点信号\n
- 找出被埋没或错放的条目。例如：误记在 debug/bugfix 中但实际是高价值架构决策的条目（面临被误删风险，应升维至 decision）；或属于用户习惯却混入普通 memory 的条目（应移至 preference）。
- 列出需要修改 category 的条目 key，并给出建议的新 category 及简短理由。\n\n
附带的知识库数据如下（JSONL格式，共 {} 条）：\n{}",
                            entries.len(), kb_jsonl
                        )
                    }
                };

                // 4. Resolve LLM credentials
                let (base_url, jwt) = resolve_llm_credentials().await?;

                // 5. Apply context budget
                let mut analysis_messages: Vec<Value> = vec![
                    serde_json::json!({"role": "user", "content": analysis_prompt})
                ];
                let budget_result = apply_context_budget(&mut analysis_messages, MAX_ROUTER_PAYLOAD_BYTES);
                if budget_result.trimmed {
                    info!("KB analyze: context budget applied — {}", budget_result.note.as_deref().unwrap_or("trimmed"));
                }

                // 6. Build request body with optional response_format
                let url = format!("{}/v1/chat/completions", base_url);
                let mut body = serde_json::json!({
                    "model": model,
                    "messages": analysis_messages,
                    "max_tokens": max_tokens,
                });
                if let Some(fmt) = &response_format {
                    body.as_object_mut().unwrap().insert("response_format".to_string(), fmt.clone());
                }

                info!("KB analyze [{}]: sending {} entries ({} chars) to {} via {}",
                    mode, entries.len(), kb_jsonl.len(), model, url);

                // 7. Call router API (shared client for connection pool reuse)
                let resp = self.http_client.post(&url)
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {}", jwt))
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| anyhow!("Router request failed: {}", e))?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let err_body = resp.text().await.unwrap_or_default();
                    return Ok(ToolResult::error(format!("Router returned {}: {}", status, err_body)));
                }

                let result: serde_json::Value = resp.json().await
                    .map_err(|e| anyhow!("Failed to parse router response: {}", e))?;

                let content = result
                    .pointer("/choices/0/message/content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(empty response)");
                let finish_reason = result
                    .pointer("/choices/0/finish_reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let usage = result.get("usage");
                let resp_model = result.get("model").and_then(|v| v.as_str()).unwrap_or(&model);

                // 8. Build response with pagination metadata
                let mut resp = serde_json::json!({
                    "model": resp_model,
                    "mode": mode,
                    "entries_in_request": entries.len(),
                    "total_entries": total_count,
                    "offset": offset,
                    "has_more": (offset as usize + entries.len()) < total_count,
                    "usage": usage,
                });

                // For consolidation_plan, try to parse as JSON and auto-save to queue
                let save_plan = args_val.get("save_plan").and_then(|v| v.as_bool()).unwrap_or(true);
                if mode == "consolidation_plan" {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
                        resp["plan"] = parsed.clone();
                        // Auto-save plan to operation queue
                        if save_plan {
                            if let Some(actions) = parsed.get("actions").and_then(|a| a.as_array()) {
                                let plan_id = uuid::Uuid::new_v4().to_string();
                                let task_id_param = args_val.get("task_id").and_then(|v| v.as_str());
                                let ops: Vec<missiond_core::types::KBOperation> = actions.iter().filter_map(|a| {
                                    let operation = a.get("action_type").and_then(|v| v.as_str())
                                        .or_else(|| a.get("action").and_then(|v| v.as_str()))
                                        .or_else(|| a.get("operation").and_then(|v| v.as_str()))?;
                                    let keys: Vec<String> = a.get("target_keys").and_then(|v| v.as_array())
                                        .or_else(|| a.get("keys").and_then(|v| v.as_array()))
                                        .map(|arr| arr.iter().filter_map(|k| k.as_str().map(|s| s.to_string())).collect())
                                        .or_else(|| a.get("key").and_then(|v| v.as_str()).map(|k| vec![k.to_string()]))?;
                                    Some(missiond_core::types::KBOperation {
                                        operation: operation.to_string(),
                                        target_keys: keys,
                                        rationale: {
                                            let reason = a.get("reason").and_then(|v| v.as_str())
                                                .or_else(|| a.get("rationale").and_then(|v| v.as_str()));
                                            // For update ops, embed new_entry in rationale JSON
                                            if operation == "update" || operation == "recategorize" || operation == "category_fix" {
                                                let mut meta = serde_json::Map::new();
                                                if let Some(r) = reason {
                                                    meta.insert("reason".into(), serde_json::json!(r));
                                                }
                                                if let Some(ne) = a.get("new_entry") {
                                                    meta.insert("new_entry".into(), ne.clone());
                                                }
                                                Some(serde_json::to_string(&meta).unwrap_or_default())
                                            } else {
                                                reason.map(|s| s.to_string())
                                            }
                                        },
                                    })
                                }).collect();
                                if !ops.is_empty() {
                                    match self.mission.db().kb_ops_save_plan(&plan_id, task_id_param, &ops) {
                                        Ok(n) => {
                                            resp["plan_id"] = serde_json::json!(plan_id);
                                            resp["operations_saved"] = serde_json::json!(n);
                                            info!(plan_id = %plan_id, ops = n, "KB consolidation plan saved to queue");
                                        }
                                        Err(e) => {
                                            resp["save_error"] = serde_json::json!(format!("Failed to save plan: {}", e));
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        resp["analysis"] = serde_json::Value::String(content.to_string());
                        resp["parse_warning"] = serde_json::json!("Response was not valid JSON. Returned as text.");
                    }
                } else {
                    resp["analysis"] = serde_json::Value::String(content.to_string());
                }

                if finish_reason == "length" || finish_reason == "max_tokens" {
                    resp["warning"] = serde_json::json!("⚠️ 输出被截断：LLM 达到 max_tokens 限制。可增大 max_tokens 参数重试。");
                    resp["finish_reason"] = serde_json::json!(finish_reason);
                }
                if let Some(note) = budget_result.note {
                    resp["context_budget"] = serde_json::json!(note);
                }
                Ok(ToolResult::json_pretty(&resp))
            }

            // ===== KB Operation Queue =====
            "mission_kb_queue_status" => {
                let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
                let plan_id = args_val.get("plan_id").and_then(|v| v.as_str());
                let status_filter = args_val.get("status").and_then(|v| v.as_str());

                let ops = self.mission.db().kb_ops_list(plan_id, status_filter)
                    .map_err(|e| anyhow!("DB error: {}", e))?;

                // If plan_id given, also get summary
                let summary = if let Some(pid) = plan_id {
                    self.mission.db().kb_ops_plan_summary(pid).ok()
                } else {
                    None
                };

                let mut resp = serde_json::json!({
                    "operations": ops,
                    "count": ops.len(),
                });
                if let Some(s) = summary {
                    resp["summary"] = s;
                }
                Ok(ToolResult::json_pretty(&resp))
            }

            "mission_kb_execute_plan" => {
                let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
                let plan_id = args_val.get("plan_id").and_then(|v| v.as_str());
                let limit = args_val.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

                // Expire stale pending ops (>24h)
                let expired = self.mission.db().kb_ops_expire_stale(86400).unwrap_or(0);
                if expired > 0 {
                    info!(expired, "kb_execute_plan: expired stale pending ops");
                }

                let plan_id = plan_id.ok_or_else(|| anyhow!("plan_id is required"))?;

                // Get pending operations
                let ops = self.mission.db().kb_ops_list(Some(plan_id), Some("pending"))
                    .map_err(|e| anyhow!("DB error: {}", e))?;

                if ops.is_empty() {
                    return Ok(ToolResult::text("No pending operations in queue."));
                }

                let batch: Vec<_> = ops.into_iter().take(limit).collect();
                let mut results = Vec::new();

                for op in &batch {
                    // Mark as running
                    let _ = self.mission.db().kb_ops_update_status(&op.id, "running", None, None);

                    let target_keys: Vec<String> = serde_json::from_str(&op.target_keys).unwrap_or_default();
                    let outcome = match op.operation.as_str() {
                        "delete" => {
                            let mut deleted = 0usize;
                            for key in &target_keys {
                                if self.mission.db().kb_forget(key).unwrap_or(false) {
                                    deleted += 1;
                                }
                            }
                            Ok(format!("Deleted {}/{} keys", deleted, target_keys.len()))
                        }
                        "update" | "category_fix" | "recategorize" => {
                            // Update category/summary directly from rationale (contains new_entry JSON)
                            let meta: serde_json::Value = op.rationale.as_deref()
                                .and_then(|r| serde_json::from_str(r).ok())
                                .unwrap_or_default();
                            let new_entry = meta.get("new_entry");
                            let key = target_keys.first().map(|k| k.as_str())
                                .or_else(|| new_entry.and_then(|ne| ne.get("key").and_then(|v| v.as_str())));
                            let category = new_entry.and_then(|ne| ne.get("category").and_then(|v| v.as_str()));
                            let summary = new_entry.and_then(|ne| ne.get("summary").and_then(|v| v.as_str()));

                            match (key, category) {
                                (Some(key), Some(cat)) => {
                                    let input = missiond_core::types::KBRememberInput {
                                        category: cat.to_string(),
                                        key: key.to_string(),
                                        summary: summary.unwrap_or("").to_string(),
                                        detail: new_entry.and_then(|ne| ne.get("detail").cloned()),
                                        source: Some("consolidation".to_string()),
                                        confidence: new_entry.and_then(|ne| ne.get("confidence").and_then(|v| v.as_f64())),
                                    };
                                    match self.mission.db().kb_remember(&input) {
                                        Ok(r) => Ok(format!("Updated key={} category={} action={}", key, cat, r.action)),
                                        Err(e) => Err(format!("Failed to update: {}", e)),
                                    }
                                }
                                _ => Err("update operation requires new_entry with key and category in rationale".to_string()),
                            }
                        }
                        "merge" | "distill" => {
                            // Auto-dispatch: fetch entries, build prompt, submit to slot-memory-slow
                            let mut entries_text = String::new();
                            for key in &target_keys {
                                if let Ok(Some(entry)) = self.mission.db().kb_get(key) {
                                    entries_text.push_str(&format!(
                                        "---\nKey: {}\nCategory: {}\nSummary: {}\nDetail: {}\n",
                                        entry.key, entry.category, entry.summary,
                                        entry.detail.as_ref().map(|d| d.to_string()).unwrap_or_default(),
                                    ));
                                }
                            }
                            if entries_text.is_empty() {
                                Err(format!("No KB entries found for keys: {:?}", target_keys))
                            } else {
                                let rationale = op.rationale.as_deref().unwrap_or("");
                                let prompt = if op.operation == "merge" {
                                    format!(
                                        "KB整理任务(merge):\n\n原因: {}\n\n以下KB条目内容重叠,请合并为一条。\
                                        保留最完整的key,用 mission_kb_remember 写入合并后的内容(category/summary/detail),\
                                        然后用 mission_kb_forget 删除多余的key。\n\n{}", rationale, entries_text
                                    )
                                } else {
                                    format!(
                                        "KB整理任务(distill):\n\n原因: {}\n\n以下KB条目需要精炼。\
                                        用 mission_kb_remember 更新每条的 summary(更简洁)和 detail(保留关键信息,删除冗余)。\n\n{}",
                                        rationale, entries_text
                                    )
                                };
                                match self.mission.submit("memory", &prompt) {
                                    Ok(task_id) => Ok(format!("dispatched:task_id={}", task_id)),
                                    Err(e) => Err(format!("submit failed: {}", e)),
                                }
                            }
                        }
                        other => {
                            Err(format!("Unknown operation: {}", other))
                        }
                    };

                    match outcome {
                        Ok(msg) => {
                            let (status_str, result_json) = if msg.starts_with("dispatched:") {
                                // Extract task_id from "dispatched:task_id=xxx"
                                let task_id = msg.strip_prefix("dispatched:task_id=").unwrap_or(&msg);
                                ("dispatched", serde_json::json!({
                                    "id": op.id,
                                    "operation": op.operation,
                                    "status": "dispatched",
                                    "taskId": task_id,
                                }))
                            } else {
                                ("done", serde_json::json!({
                                    "id": op.id,
                                    "operation": op.operation,
                                    "status": "done",
                                    "result": msg,
                                }))
                            };
                            let _ = self.mission.db().kb_ops_update_status(&op.id, status_str, Some(&msg), None);
                            results.push(result_json);
                        }
                        Err(msg) => {
                            let _ = self.mission.db().kb_ops_update_status(&op.id, "failed", None, Some(&msg));
                            results.push(serde_json::json!({
                                "id": op.id,
                                "operation": op.operation,
                                "status": "failed",
                                "error": msg,
                            }));
                        }
                    }
                }

                // Signal unified scheduler to dispatch any newly created submit tasks
                if results.iter().any(|r| r.get("status").and_then(|s| s.as_str()) == Some("dispatched")) {
                    self.submit_notify.notify_one();
                }

                // Get remaining count
                let remaining = self.mission.db().kb_ops_list(Some(plan_id), Some("pending"))
                    .map(|v| v.len()).unwrap_or(0);

                Ok(ToolResult::json_pretty(&serde_json::json!({
                    "executed": results.len(),
                    "results": results,
                    "remaining": remaining,
                })))
            }

            // ===== Router Chat =====
            "mission_router_chat" => {
                let params: serde_json::Value = serde_json::from_value(args)
                    .map_err(|e| anyhow!("Invalid params: {}", e))?;

                let mut messages: Vec<serde_json::Value> = params.get("messages")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .ok_or_else(|| anyhow!("'messages' array is required"))?;

                let context_mode = params.get("context")
                    .and_then(|v| v.as_str())
                    .unwrap_or("none");
                let model = params.get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("gemini-3.1-pro")
                    .to_string();
                let max_tokens: u32 = params.get("max_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32)
                    .unwrap_or(16384);
                let search_enabled = params.get("search")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let task_id = params.get("task_id").and_then(|v| v.as_str()).map(|s| s.to_string());

                // If task_id provided, load conversation history and prepend
                let conv_id = if let Some(ref tid) = task_id {
                    let cid = self.mission.db().router_chat_get_or_create(tid, &model)
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    let history = self.mission.db().router_chat_load_history(&cid)
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    if !history.is_empty() {
                        // Prepend history before new messages
                        let mut combined = history;
                        combined.extend(messages);
                        messages = combined;
                        info!(task_id = %tid, history_msgs = messages.len(), "Router chat: loaded history for task");
                    }
                    Some(cid)
                } else {
                    None
                };

                // Auto-inject context into first user message if requested
                if context_mode != "none" {
                    let mut context_parts: Vec<String> = Vec::new();

                    if context_mode == "kb" || context_mode == "both" {
                        let entries = self.mission.db()
                            .kb_list(None)
                            .map_err(|e| anyhow!("DB error: {}", e))?;
                        let kb_lines: Vec<String> = entries.iter()
                            .filter(|e| e.category != "credential")
                            .map(|e| format!("[{}] {}: {}", e.category, e.key, e.summary.replace('\n', " ")))
                            .collect();
                        context_parts.push(format!("\n\n[Knowledge Base ({} entries)]\n{}", entries.len(), kb_lines.join("\n")));
                    }

                    if context_mode == "board" || context_mode == "both" {
                        let tasks = self.mission.db()
                            .list_board_tasks(None, false)
                            .map_err(|e| anyhow!("DB error: {}", e))?;
                        let task_lines: Vec<String> = tasks.iter()
                            .map(|t| {
                                let desc_preview: String = t.description.chars().take(2000).collect();
                                let project = t.project.as_deref().unwrap_or("");
                                let mut line = format!("[{}|{}|{}] {}",
                                    t.status.as_str(), t.priority, t.category, t.title);
                                if !project.is_empty() { line.push_str(&format!(" (project: {})", project)); }
                                if !desc_preview.is_empty() { line.push_str(&format!(" -- {}", desc_preview)); }
                                line
                            })
                            .collect();
                        context_parts.push(format!("\n\n[Mission Board ({} tasks)]\n{}", tasks.len(), task_lines.join("\n")));
                    }

                    if !context_parts.is_empty() {
                        // Append context to the first user message
                        if let Some(first_user) = messages.iter_mut().find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user")) {
                            let original = first_user.get("content").and_then(|c| c.as_str()).unwrap_or("");
                            let enriched = format!("{}{}", original, context_parts.join(""));
                            first_user["content"] = serde_json::Value::String(enriched);
                        }
                    }
                }

                // Apply context budget before sending
                let budget_result = apply_context_budget(&mut messages, MAX_ROUTER_PAYLOAD_BYTES);
                if budget_result.trimmed {
                    info!("Router chat: context budget applied — {}", budget_result.note.as_deref().unwrap_or("trimmed"));
                }

                // Resolve LLM credentials
                let (base_url, jwt) = resolve_llm_credentials().await?;

                // Call router API
                let url = format!("{}/v1/chat/completions", base_url);
                let mut body = serde_json::json!({
                    "model": model,
                    "messages": messages,
                    "max_tokens": max_tokens,
                });
                if search_enabled {
                    body["tools"] = serde_json::json!([{"type": "google_search"}]);
                }

                let total_chars: usize = messages.iter()
                    .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                    .map(|s| s.len())
                    .sum();
                info!("Router chat: {} messages ({} chars) to {} via {}", messages.len(), total_chars, model, url);

                let resp = self.http_client.post(&url)
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {}", jwt))
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| anyhow!("Router request failed: {}", e))?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let err_body = resp.text().await.unwrap_or_default();
                    return Ok(ToolResult::error(format!("Router returned {}: {}", status, err_body)));
                }

                let result: serde_json::Value = resp.json().await
                    .map_err(|e| anyhow!("Failed to parse router response: {}", e))?;

                let content = result
                    .pointer("/choices/0/message/content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(empty response)");
                let finish_reason = result
                    .pointer("/choices/0/finish_reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let usage = result.get("usage");
                let resp_model = result.get("model").and_then(|v| v.as_str()).unwrap_or(&model);

                let mut resp = serde_json::json!({
                    "model": resp_model,
                    "response": content,
                    "usage": usage,
                });
                if finish_reason == "length" || finish_reason == "max_tokens" {
                    resp["warning"] = serde_json::json!("⚠️ 输出被截断：LLM 达到 max_tokens 限制，返回内容不完整。可增大 max_tokens 参数重试。");
                    resp["finish_reason"] = serde_json::json!(finish_reason);
                }
                if let Some(note) = budget_result.note {
                    resp["context_budget"] = serde_json::json!(note);
                }

                // Save new messages + assistant response to conversation history
                if let Some(ref cid) = conv_id {
                    let mut new_msgs: Vec<(String, String)> = Vec::new();
                    // Count how many history messages were prepended
                    let history_count = self.mission.db().router_chat_load_history(cid)
                        .map(|h| h.len()).unwrap_or(0);
                    // messages array = history + new_user_messages; skip history portion
                    for msg in messages.iter().skip(history_count) {
                        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                        let msg_content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
                        new_msgs.push((role.to_string(), msg_content.to_string()));
                    }
                    // Add assistant response
                    new_msgs.push(("assistant".to_string(), content.to_string()));

                    if let Err(e) = self.mission.db().router_chat_append_messages(cid, &new_msgs) {
                        warn!("Failed to save router chat history: {}", e);
                    } else {
                        info!(conv_id = %cid, saved = new_msgs.len(), "Router chat: saved messages to history");
                    }
                    resp["conversation_id"] = serde_json::json!(cid);
                }

                Ok(ToolResult::json_pretty(&resp))
            }

            "mission_router_chat_history" => {
                let args_val: serde_json::Value = serde_json::from_value(args).unwrap_or_default();
                let task_id = args_val.get("task_id").and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("task_id is required"))?;
                let model = "gemini-3.1-pro"; // default model for lookup
                let conv_id = self.mission.db().router_chat_get_or_create(task_id, model)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                let history = self.mission.db().router_chat_load_history(&conv_id)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                if history.is_empty() {
                    return Ok(ToolResult::text(format!("任务 {} 暂无 Gemini 对话记录", task_id)));
                }
                let resp = serde_json::json!({
                    "task_id": task_id,
                    "conversation_id": conv_id,
                    "message_count": history.len(),
                    "messages": history,
                });
                Ok(ToolResult::json_pretty(&resp))
            }

            // ===== Memory Extraction =====
            // Message-level pipeline tracking: returns pending messages with IDs.
            // State auto-committed by Daemon on extraction completion — no manual done() needed.
            "mission_memory_pending" | "mission_memory_pending_user" => {
                let db = self.mission.db();
                const PENDING_MSG_LIMIT: usize = 50;
                let pending = db.get_pending_realtime_messages_with_limit(PENDING_MSG_LIMIT)
                    .map_err(|e| anyhow!("DB error: {}", e))?;

                if pending.is_empty() {
                    return Ok(ToolResult::text("没有待分析的新对话内容。"));
                }

                let mut output = String::new();
                let mut all_msg_ids: Vec<i64> = Vec::new();
                let mut user_count = 0usize;
                for (session_id, project, msgs) in &pending {
                    output.push_str(&format!("## session: {} (project: {})\n\n", session_id, project));
                    for msg in msgs {
                        all_msg_ids.push(msg.id);
                        if msg.role == "user" {
                            user_count += 1;
                            output.push_str(&format!("[#{}][{}] ★ user: {}\n\n", msg.id, msg.timestamp, msg.content));
                        } else {
                            // Assistant messages: truncate to reduce payload
                            let content = if msg.content.len() > 500 {
                                let mut end = 500;
                                while !msg.content.is_char_boundary(end) && end > 0 { end -= 1; }
                                format!("{}…", &msg.content[..end])
                            } else {
                                msg.content.clone()
                            };
                            output.push_str(&format!("[#{}][{}] assistant: {}\n\n", msg.id, msg.timestamp, content));
                        }
                    }
                }

                let session_count = pending.len();
                let msg_count = all_msg_ids.len();
                let truncated_note = if msg_count >= PENDING_MSG_LIMIT {
                    format!(" ⚠️ 已达上限 {}，可能还有更多未显示的消息。处理完当前批次后系统将自动推送下一批。", PENDING_MSG_LIMIT)
                } else {
                    String::new()
                };
                let header = format!(
                    "[realtime-extract] {} 个会话, {} 条消息 (其中 {} 条用户消息){}\n\n\
                     ★ = 用户原话，优先级最高。每句用户消息都是刻意的。\n\
                     assistant 消息仅提供上下文，不需逐条分析。\n\n\
                     提取规则:\n\
                     - 用户偏好/纠正/否定 → category: preference (最高优先)\n\
                     - 架构决策/技术事实 → category: memory 或子分类\n\
                     - 「好」「行」= 用户认可 AI 方案，记录为决策\n\
                     - 「别...」「不要...」= 高价值偏好\n\
                     - 运维痛点/调试弯路 → category: memory:ops / memory:debug\n\
                     - 不存: 纯任务指令、当天工作日志、代码提交记录\n\
                     - 存入前用 mission_kb_search 检查去重\n\n",
                    session_count, msg_count, user_count, truncated_note,
                );

                Ok(ToolResult::text(&format!("{}{}", header, output)))
            }

            // Deprecated: pipeline state table has been removed. Both realtime and deep_analysis
            // now use conversation-level watermarks. Kept for backward compat with old agent prompts.
            "mission_memory_done" => {
                Ok(ToolResult::text("已废弃。系统现在使用会话级水位线自动管理状态，无需手动调用。"))
            }

            "mission_memory_pause" => {
                #[derive(Deserialize)]
                struct Args {
                    paused: Option<bool>,
                }
                let args: Args = serde_json::from_value(args).unwrap_or(Args { paused: None });
                let current = self.memory_paused.load(std::sync::atomic::Ordering::Relaxed);
                let new_val = args.paused.unwrap_or(!current); // toggle if not specified
                self.memory_paused.store(new_val, std::sync::atomic::Ordering::Relaxed);
                // Persist to flag file so pause survives daemon restart
                let flag = default_mission_home().join("memory_paused");
                if new_val {
                    let now = chrono::Utc::now().timestamp();
                    let _ = std::fs::write(&flag, now.to_string());
                    self.memory_paused_at.store(now, std::sync::atomic::Ordering::Relaxed);
                    info!("Memory extraction PAUSED by user");
                } else {
                    let _ = std::fs::remove_file(&flag);
                    self.memory_paused_at.store(0, std::sync::atomic::Ordering::Relaxed);
                    info!("Memory extraction RESUMED by user");
                }
                Ok(ToolResult::text(if new_val {
                    "记忆任务已暂停（2 小时后自动恢复）。调用 mission_memory_pause(paused: false) 手动恢复。"
                } else {
                    "记忆任务已恢复。"
                }))
            }

            "mission_memory_status" => {
                let paused = self.memory_paused.load(std::sync::atomic::Ordering::Relaxed);
                let now = chrono::Utc::now().timestamp();

                // Fast lane state
                let fast_es = self.extraction_state.read().await;
                let fast_busy = self.memory_slot_busy_since.load(std::sync::atomic::Ordering::Relaxed);
                let fast_lane = serde_json::json!({
                    "slotId": MEMORY_SLOT_ID,
                    "phase": format!("{:?}", fast_es.phase),
                    "activeType": fast_es.active_type,
                    "phaseAge": if fast_es.phase_started_at > 0 { now - fast_es.phase_started_at } else { 0 },
                    "busySince": fast_busy,
                    "busyDuration": if fast_busy > 0 { now - fast_busy } else { 0 },
                    "currentTargets": fast_es.watermark_targets.iter()
                        .map(|(sid, _)| sid.clone()).collect::<Vec<_>>(),
                    "currentTaskId": fast_es.current_task_id,
                });
                drop(fast_es);

                // Slow lane state
                let slow_es = self.slow_extraction_state.read().await;
                let slow_busy = self.slow_slot_busy_since.load(std::sync::atomic::Ordering::Relaxed);
                let slow_lane = serde_json::json!({
                    "slotId": MEMORY_SLOW_SLOT_ID,
                    "phase": format!("{:?}", slow_es.phase),
                    "activeType": slow_es.active_type,
                    "phaseAge": if slow_es.phase_started_at > 0 { now - slow_es.phase_started_at } else { 0 },
                    "busySince": slow_busy,
                    "busyDuration": if slow_busy > 0 { now - slow_busy } else { 0 },
                    "currentConvId": slow_es.current_deep_conv_id,
                    "currentTaskId": slow_es.current_task_id,
                });
                drop(slow_es);

                // Pending counts
                let db = self.mission.db();
                let pending_realtime = db.count_pending_realtime().unwrap_or(0);
                let pending_deep = db.count_pending_deep_analysis(
                    CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES
                ).unwrap_or(0);

                // Timestamps
                let last_consolidation = self.last_kb_consolidation_at.load(std::sync::atomic::Ordering::Relaxed);
                let last_gc = self.last_auto_gc_at.load(std::sync::atomic::Ordering::Relaxed);

                // KB stats (full — includes mostAccessed, oldest, subcategories)
                let kb_stats = db.kb_stats()
                    .map(|s| serde_json::json!({
                        "total": s["total"],
                        "categories": s["categoryRollup"],
                        "subcategories": s["categories"],
                        "neverAccessed": s["neverAccessed"],
                        "mostAccessed": s["mostAccessed"],
                        "oldest": s["oldest"],
                    }))
                    .unwrap_or(serde_json::json!(null));

                // Recent memory slot tasks (last 15 across both slots)
                let mut recent: Vec<serde_json::Value> = Vec::new();
                for sid in &[MEMORY_SLOT_ID, MEMORY_SLOW_SLOT_ID] {
                    if let Ok(tasks) = db.list_slot_tasks(Some(sid), None, None, 10) {
                        for t in tasks {
                            recent.push(serde_json::json!({
                                "id": t.id,
                                "slotId": t.slot_id,
                                "taskType": t.task_type,
                                "status": t.status,
                                "durationMs": t.duration_ms,
                                "createdAt": t.created_at,
                                "error": t.error,
                                "outputCount": t.output_count,
                                "sourceSessions": t.source_sessions,
                                "conversationId": t.conversation_id,
                            }));
                        }
                    }
                }
                recent.sort_by(|a, b| {
                    let ta = a["createdAt"].as_str().unwrap_or("");
                    let tb = b["createdAt"].as_str().unwrap_or("");
                    tb.cmp(ta)
                });
                recent.truncate(15);

                // Queue detail (per-session / per-conversation)
                let realtime_detail: Vec<serde_json::Value> = db.pending_realtime_detail()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(sid, cnt, oldest)| serde_json::json!({"sessionId": sid, "msgCount": cnt, "oldest": oldest}))
                    .collect();
                let deep_detail: Vec<serde_json::Value> = db.pending_deep_detail(
                    CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES
                ).unwrap_or_default()
                    .into_iter()
                    .map(|(id, ended, retries)| serde_json::json!({"conversationId": id, "endedAt": ended, "retries": retries}))
                    .collect();

                Ok(ToolResult::json(&serde_json::json!({
                    "paused": paused,
                    "fastLane": fast_lane,
                    "slowLane": slow_lane,
                    "pendingRealtime": pending_realtime,
                    "pendingDeep": pending_deep,
                    "realtimeDetail": realtime_detail,
                    "deepDetail": deep_detail,
                    "lastKbConsolidation": if last_consolidation > 0 {
                        chrono::DateTime::from_timestamp(last_consolidation, 0)
                            .map(|d| d.to_rfc3339()).unwrap_or_default()
                    } else { String::new() },
                    "lastAutoGc": if last_gc > 0 {
                        chrono::DateTime::from_timestamp(last_gc, 0)
                            .map(|d| d.to_rfc3339()).unwrap_or_default()
                    } else { String::new() },
                    "kbStats": kb_stats,
                    "recentTasks": recent,
                })))
            }

            // ===== Token Stats =====
            "mission_token_stats" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Args {
                    session_id: Option<String>,
                    slot_id: Option<String>,
                    since: Option<String>,
                    group_by: Option<String>,
                }
                let Args { session_id, slot_id, since, group_by } =
                    serde_json::from_value(args).unwrap_or(Args {
                        session_id: None, slot_id: None, since: None, group_by: None,
                    });
                let rows = self.mission.db().token_stats(
                    session_id.as_deref(),
                    slot_id.as_deref(),
                    since.as_deref(),
                    group_by.as_deref(),
                ).map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json_pretty(&rows))
            }

            // ===== Conversation Logs =====
            "mission_conversation_list" => {
                #[derive(Deserialize)]
                struct Args {
                    status: Option<String>,
                    limit: Option<i64>,
                    conversation_type: Option<String>,
                }
                let Args { status, limit, conversation_type } =
                    serde_json::from_value(args).unwrap_or(Args { status: None, limit: None, conversation_type: None });
                let convs = self.mission.db()
                    .list_conversations(status.as_deref(), limit.unwrap_or(20), conversation_type.as_deref())
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json_pretty(&convs))
            }
            "mission_conversation_get" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Args {
                    session_id: String,
                    tail: Option<i64>,
                    since_id: Option<i64>,
                    include_raw: Option<bool>,
                }
                let Args { session_id, tail, since_id, include_raw } = serde_json::from_value(args)?;
                let db = self.mission.db();
                let conv = db.get_conversation(&session_id)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                let msgs = db.get_conversation_messages(&session_id, since_id, tail.unwrap_or(50))
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                let messages: Vec<serde_json::Value> = if include_raw.unwrap_or(false) {
                    // Full messages for frontend (includes rawContent/model/metadata for image rendering)
                    msgs.iter().map(|m| {
                        serde_json::json!({
                            "id": m.id,
                            "sessionId": m.session_id,
                            "role": m.role,
                            "content": m.content,
                            "rawContent": m.raw_content,
                            "messageUuid": m.message_uuid,
                            "parentUuid": m.parent_uuid,
                            "model": m.model,
                            "timestamp": m.timestamp,
                            "metadata": m.metadata,
                        })
                    }).collect()
                } else {
                    // Lite messages for LLM consumption (strip base64 images to protect context)
                    msgs.iter().map(|m| {
                        serde_json::json!({
                            "id": m.id,
                            "role": m.role,
                            "content": m.content,
                            "timestamp": m.timestamp,
                            "messageUuid": m.message_uuid,
                            "parentUuid": m.parent_uuid,
                        })
                    }).collect()
                };
                // Include child (subagent) conversations summary
                let children = db.get_child_conversations(&session_id)
                    .unwrap_or_default();
                let mut result = serde_json::json!({
                    "conversation": conv,
                    "messages": messages,
                    "count": messages.len(),
                });
                if !children.is_empty() {
                    let child_summaries: Vec<serde_json::Value> = children.iter().map(|c| {
                        serde_json::json!({
                            "id": c.id,
                            "messageCount": c.message_count,
                            "status": c.status,
                            "startedAt": c.started_at,
                        })
                    }).collect();
                    result["subagents"] = serde_json::json!(child_summaries);
                }
                Ok(ToolResult::json(&result))
            }
            "mission_conversation_search" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Args {
                    query: String,
                    limit: Option<i64>,
                    role: Option<String>,
                    session_id: Option<String>,
                    exclude_session_id: Option<String>,
                }
                let Args { query, limit, role, session_id, exclude_session_id } = serde_json::from_value(args)?;
                let mut msgs = self.mission.db()
                    .search_conversation_messages(&query, limit.unwrap_or(20) * 3) // over-fetch for post-filter
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                let limit = limit.unwrap_or(20) as usize;
                // Post-filter by role/session
                if let Some(ref r) = role {
                    msgs.retain(|m| m.role == *r);
                }
                if let Some(ref sid) = session_id {
                    msgs.retain(|m| m.session_id == *sid);
                }
                if let Some(ref ex_sid) = exclude_session_id {
                    msgs.retain(|m| m.session_id != *ex_sid);
                }
                msgs.truncate(limit);
                let msgs_lite: Vec<serde_json::Value> = msgs.iter().map(|m| {
                    serde_json::json!({
                        "id": m.id,
                        "sessionId": m.session_id,
                        "role": m.role,
                        "content": m.content,
                        "timestamp": m.timestamp,
                    })
                }).collect();
                Ok(ToolResult::json(&serde_json::json!({
                    "results": msgs_lite,
                    "count": msgs_lite.len(),
                    "query": query,
                })))
            }

            // ===== Conversation Events & Agent Trajectory =====
            "mission_conversation_events" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Args {
                    session_id: Option<String>,
                    event_type: Option<String>,
                    limit: Option<i64>,
                }
                let Args { session_id, event_type, limit } =
                    serde_json::from_value(args).unwrap_or(Args { session_id: None, event_type: None, limit: None });
                let db = self.mission.db();
                if let Some(sid) = &session_id {
                    let events = db.get_conversation_events(sid, event_type.as_deref(), limit.unwrap_or(100))
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json(&serde_json::json!({
                        "sessionId": sid,
                        "events": events,
                        "count": events.len(),
                    })))
                } else {
                    // No sessionId → return event type summary
                    let summary = db.get_event_type_summary(None)
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    let summary_obj: Vec<serde_json::Value> = summary.iter().map(|(t, c)| {
                        serde_json::json!({ "eventType": t, "count": c })
                    }).collect();
                    Ok(ToolResult::json(&serde_json::json!({
                        "summary": summary_obj,
                        "totalTypes": summary.len(),
                    })))
                }
            }
            "mission_agent_trajectory" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Args {
                    tool_use_id: String,
                    limit: Option<i64>,
                }
                let Args { tool_use_id, limit } = serde_json::from_value(args)?;
                let msgs = self.mission.db()
                    .get_agent_trajectory(&tool_use_id, limit.unwrap_or(200))
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                // Extract agentId from first message metadata
                let agent_id = msgs.first()
                    .and_then(|m| m.metadata.as_ref())
                    .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                    .and_then(|v| v.get("agentId").and_then(|a| a.as_str()).map(|s| s.to_string()));
                // Strip raw_content for LLM consumption
                let msgs_lite: Vec<serde_json::Value> = msgs.iter().map(|m| {
                    serde_json::json!({
                        "id": m.id,
                        "role": m.role,
                        "content": m.content,
                        "timestamp": m.timestamp,
                    })
                }).collect();
                Ok(ToolResult::json(&serde_json::json!({
                    "toolUseId": tool_use_id,
                    "agentId": agent_id,
                    "messages": msgs_lite,
                    "count": msgs_lite.len(),
                })))
            }

            // ===== Audit (Conversation Tool Call Analysis) =====
            "mission_audit_trace" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Args {
                    session_id: String,
                    tool_filter: Option<Vec<String>>,
                    include_reasoning: Option<bool>,
                }
                let Args { session_id, tool_filter, include_reasoning } = serde_json::from_value(args)?;
                let db = self.mission.db();
                let conv = db.get_conversation(&session_id)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                let calls = db.get_tool_calls_by_session(
                    &session_id,
                    tool_filter.as_deref(),
                    10000,
                ).map_err(|e| anyhow!("DB error: {}", e))?;

                // Build Markdown trace
                let mut md = String::new();
                let slot_id = conv.as_ref().and_then(|c| c.slot_id.as_deref()).unwrap_or("N/A");
                let model = conv.as_ref().and_then(|c| c.model.as_deref()).unwrap_or("N/A");
                let success_count = calls.iter().filter(|c| c.status == "success").count();
                let error_count = calls.iter().filter(|c| c.status == "error").count();
                let pending_count = calls.iter().filter(|c| c.status == "pending").count();

                md.push_str(&format!("# Audit Trace: {session_id}\n"));
                md.push_str(&format!("Slot: {slot_id} | Model: {model}\n"));
                md.push_str(&format!("Tool Calls: {} (Success: {success_count}, Error: {error_count}, Pending: {pending_count})\n\n---\n\n", calls.len()));

                // Optionally interleave reasoning from conversation_messages
                if include_reasoning.unwrap_or(false) {
                    let msgs = db.get_conversation_messages(&session_id, None, 10000)
                        .unwrap_or_default();
                    // Build timeline: messages + tool calls sorted by timestamp
                    let mut msg_idx = 0;
                    for tc in &calls {
                        // Print assistant reasoning before this tool call
                        while msg_idx < msgs.len() && msgs[msg_idx].timestamp <= tc.timestamp {
                            let m = &msgs[msg_idx];
                            if m.role == "assistant" && !m.content.starts_with("[Tool:") && !m.content.starts_with("[thinking]") {
                                let content = if m.content.len() > 200 {
                                    format!("{}...", &m.content[..m.content.char_indices().nth(200).map(|(i,_)| i).unwrap_or(m.content.len())])
                                } else {
                                    m.content.clone()
                                };
                                md.push_str(&format!("[{}] 💭 {}\n\n", &tc.timestamp[11..19.min(tc.timestamp.len())], content));
                            }
                            msg_idx += 1;
                        }
                        format_tool_call_trace(&mut md, tc);
                    }
                } else {
                    for tc in &calls {
                        format_tool_call_trace(&mut md, tc);
                    }
                }

                if !calls.is_empty() {
                    md.push_str("\n💡 Use mission_audit_detail(toolId: \"<id>\") for full I/O payload.\n");
                }

                Ok(ToolResult::text(&md))
            }
            "mission_audit_detail" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Args {
                    tool_id: String,
                }
                let Args { tool_id } = serde_json::from_value(args)?;
                let db = self.mission.db();
                let tc = db.get_tool_call_by_id(&tool_id)
                    .map_err(|e| anyhow!("DB error: {}", e))?
                    .ok_or_else(|| anyhow!("Tool call not found: {}", tool_id))?;

                // Parse raw_input/raw_output back to JSON for clean display
                let input: serde_json::Value = tc.raw_input.as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or(serde_json::Value::Null);
                let output: serde_json::Value = tc.raw_output.as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or(serde_json::Value::Null);

                Ok(ToolResult::json(&serde_json::json!({
                    "id": tc.id,
                    "toolName": tc.tool_name,
                    "timestamp": tc.timestamp,
                    "status": tc.status,
                    "input": input,
                    "output": output,
                    "inputSummary": tc.input_summary,
                    "outputSummary": tc.output_summary,
                })))
            }
            "mission_audit_stats" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Args {
                    session_id: String,
                }
                let Args { session_id } = serde_json::from_value(args)?;
                let db = self.mission.db();
                let stats = db.get_tool_call_stats(&session_id)
                    .map_err(|e| anyhow!("DB error: {}", e))?;

                let mut by_tool = serde_json::Map::new();
                let mut total = 0i64;
                let mut total_success = 0i64;
                let mut total_error = 0i64;
                for (name, count, success, error) in &stats {
                    by_tool.insert(name.clone(), serde_json::json!({
                        "count": count,
                        "success": success,
                        "error": error,
                    }));
                    total += count;
                    total_success += success;
                    total_error += error;
                }

                // Get first/last timestamps
                let calls = db.get_tool_calls_by_session(&session_id, None, 10000)
                    .unwrap_or_default();
                let first_ts = calls.first().map(|c| c.timestamp.as_str()).unwrap_or("N/A");
                let last_ts = calls.last().map(|c| c.timestamp.as_str()).unwrap_or("N/A");

                Ok(ToolResult::json(&serde_json::json!({
                    "sessionId": session_id,
                    "totalCalls": total,
                    "byTool": by_tool,
                    "byStatus": {
                        "success": total_success,
                        "error": total_error,
                        "pending": total - total_success - total_error,
                    },
                    "firstCall": first_ts,
                    "lastCall": last_ts,
                })))
            }

            // ===== Board Tasks (Personal Task Board) =====
            "mission_board_list" => {
                let BoardListArgs { status, include_hidden } =
                    serde_json::from_value(args).unwrap_or(BoardListArgs { status: None, include_hidden: None });
                let tasks = self
                    .mission
                    .db()
                    .list_board_tasks(status.as_deref(), include_hidden.unwrap_or(false))
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json_pretty(&tasks))
            }
            "mission_board_create" => {
                let input: missiond_core::types::CreateBoardTaskInput =
                    serde_json::from_value(args)?;
                let task = self
                    .mission
                    .db()
                    .create_board_task(&input)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json_pretty(&task))
            }
            "mission_board_update" => {
                let args_val: Value = args;
                let id = args_val
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing 'id' field"))?
                    .to_string();
                let update: missiond_core::types::UpdateBoardTaskInput =
                    serde_json::from_value(args_val)?;
                let task = self
                    .mission
                    .db()
                    .update_board_task(&id, &update)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                match task {
                    Some(t) => Ok(ToolResult::json_pretty(&t)),
                    None => Ok(ToolResult::error("Task not found")),
                }
            }
            "mission_board_get" => {
                let BoardIdArgs { id } = serde_json::from_value(args)?;
                let task = self
                    .mission
                    .db()
                    .get_board_task_with_notes(&id)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                match task {
                    Some(t) => Ok(ToolResult::json_pretty(&t)),
                    None => Ok(ToolResult::error("Task not found")),
                }
            }
            "mission_board_delete" => {
                let BoardIdArgs { id } = serde_json::from_value(args)?;
                let deleted = self
                    .mission
                    .db()
                    .delete_board_task(&id)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json(&serde_json::json!({
                    "deleted": deleted,
                    "id": id,
                })))
            }
            "mission_board_toggle" => {
                let BoardIdArgs { id } = serde_json::from_value(args)?;
                let task = self
                    .mission
                    .db()
                    .toggle_board_task(&id)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                match task {
                    Some(t) => Ok(ToolResult::json_pretty(&t)),
                    None => Ok(ToolResult::error("Task not found")),
                }
            }
            "mission_board_claim" => {
                let task_id = args.get("taskId").and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("taskId is required"))?;
                let executor_type = args.get("executorType").and_then(|v| v.as_str())
                    .unwrap_or("manual_session");
                // Use explicit executorId or fall back to a generated session identifier
                let executor_id = args.get("executorId").and_then(|v| v.as_str())
                    .unwrap_or("claude-code-session");
                match self.mission.db().claim_board_task(task_id, executor_id, executor_type) {
                    Ok(Some(task)) => Ok(ToolResult::json_pretty(&task)),
                    Ok(None) => {
                        // Check why it failed: task not found vs already claimed
                        match self.mission.db().get_board_task(task_id) {
                            Ok(Some(existing)) => {
                                let msg = if let Some(ref claimer) = existing.claim_executor_id {
                                    format!("Task already claimed by {} ({})",
                                        claimer,
                                        existing.claim_executor_type.as_deref().unwrap_or("unknown"))
                                } else {
                                    format!("Task cannot be claimed (status: {})", existing.status.as_str())
                                };
                                Ok(ToolResult::error(msg))
                            }
                            _ => Ok(ToolResult::error("Task not found")),
                        }
                    }
                    Err(e) => Ok(ToolResult::error(format!("DB error: {}", e))),
                }
            }
            // ===== Skill Knowledge Hub =====
            "mission_skill_list" => {
                let skills: Vec<Value> = self
                    .skills
                    .list()
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "name": s.name,
                            "description": s.description,
                            "aka": s.aka,
                            "path": s.path,
                        })
                    })
                    .collect();
                Ok(ToolResult::json_pretty(&skills))
            }
            "mission_skill_search" => {
                let SkillSearchArgs { query } = serde_json::from_value(args)?;

                // Primary: FTS5 full-text search (covers content, not just name/aka)
                let db = self.mission.db();
                let mut seen_topics = std::collections::HashSet::new();
                let mut results: Vec<Value> = Vec::new();

                // 1. Exact name/aka match from in-memory index (highest priority)
                for s in self.skills.search(&query).iter().take(5) {
                    if seen_topics.insert(s.name.clone()) {
                        results.push(serde_json::json!({
                            "name": s.name,
                            "description": s.description,
                            "aka": s.aka,
                            "path": s.path,
                            "match_type": "name/aka",
                        }));
                    }
                }

                // 2. FTS5 full-text search (content-level matching)
                if let Ok(fts_results) = db.skill_search_fts(&query) {
                    for r in fts_results.iter().take(10) {
                        if seen_topics.insert(r.topic.clone()) {
                            results.push(serde_json::json!({
                                "name": r.topic,
                                "description": r.description,
                                "path": r.file_path,
                                "match_type": "content",
                                "matched_section": r.section_title,
                                "snippet": r.snippet,
                            }));
                        }
                    }
                }

                // 3. Track hits for matched topics
                for r in &results {
                    if let Some(name) = r.get("name").and_then(|v| v.as_str()) {
                        let _ = db.skill_topic_hit(name);
                    }
                }

                Ok(ToolResult::json_pretty(&results))
            }
            "mission_context_build" => {
                let ContextBuildArgs { query } = serde_json::from_value(args)?;
                let mut context = self.skills.build_context(&query);

                // Also search KB for matching knowledge (multi-factor sort + token budget)
                let db = self.mission.db();
                if let Ok(mut entries) = db.kb_search(&query, None) {
                    // Sort by confidence × log(access_count + 1) descending
                    entries.sort_by(|a, b| {
                        let score_a = a.confidence * (a.access_count as f64 + 1.0).ln();
                        let score_b = b.confidence * (b.access_count as f64 + 1.0).ln();
                        score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
                    });

                    // Token budget: ~800 chars instead of fixed top-5
                    let mut budget: i32 = 800;
                    let mut kb_block = String::new();
                    for entry in &entries {
                        let line = format!("- [{}] {}: {}\n", entry.category, entry.key, entry.summary);
                        budget -= line.len() as i32;
                        if budget < 0 { break; }
                        kb_block.push_str(&line);
                    }
                    if !kb_block.is_empty() {
                        context.push_str("\n[Knowledge Base]\n");
                        context.push_str(&kb_block);
                    }
                }

                Ok(ToolResult::text(context))
            }
            "mission_context_resolve" => {
                #[derive(Deserialize)]
                struct ContextResolveArgs {
                    query: String,
                    skill: Option<String>,
                    include_board: Option<bool>,
                }
                let args: ContextResolveArgs = serde_json::from_value(args)?;
                let db = self.mission.db();
                let include_board = args.include_board.unwrap_or(false);

                // Step 1: Find primary skills
                let mut primary_topics: Vec<String> = Vec::new();
                if let Some(ref name) = args.skill {
                    primary_topics.push(name.clone());
                } else {
                    // FTS search + name/aka match
                    for s in self.skills.search(&args.query).iter().take(3) {
                        primary_topics.push(s.name.clone());
                    }
                }

                // Step 2: Recursive dependency resolution (max 2 layers)
                let mut all_skill_names: Vec<String> = Vec::new();
                let mut seen = std::collections::HashSet::new();
                let mut infra_ids = std::collections::HashSet::new();
                let mut kb_categories = std::collections::HashSet::new();

                let mut skill_results: Vec<Value> = Vec::new();

                for topic_name in &primary_topics {
                    if !seen.insert(topic_name.clone()) { continue; }
                    all_skill_names.push(topic_name.clone());

                    if let Ok(Some(topic)) = db.skill_topic_get(topic_name) {
                        skill_results.push(json!({
                            "name": topic.topic,
                            "path": topic.file_path,
                            "description": topic.description,
                            "matched_by": if args.skill.is_some() { "direct" } else { "query" },
                        }));

                        // Parse requires from DB
                        if let Some(ref rj) = topic.requires_json {
                            if let Ok(req) = serde_json::from_str::<missiond_core::SkillRequires>(rj) {
                                // Layer 1 dependencies
                                for dep_name in &req.skills {
                                    if seen.insert(dep_name.clone()) {
                                        all_skill_names.push(dep_name.clone());
                                        if let Ok(Some(dep_topic)) = db.skill_topic_get(dep_name) {
                                            skill_results.push(json!({
                                                "name": dep_topic.topic,
                                                "path": dep_topic.file_path,
                                                "description": dep_topic.description,
                                                "matched_by": "dependency",
                                            }));
                                            // Layer 2 dependencies (no further recursion)
                                            if let Some(ref drj) = dep_topic.requires_json {
                                                if let Ok(dreq) = serde_json::from_str::<missiond_core::SkillRequires>(drj) {
                                                    for dep2_name in &dreq.skills {
                                                        if seen.insert(dep2_name.clone()) {
                                                            if let Ok(Some(dep2)) = db.skill_topic_get(dep2_name) {
                                                                skill_results.push(json!({
                                                                    "name": dep2.topic,
                                                                    "path": dep2.file_path,
                                                                    "description": dep2.description,
                                                                    "matched_by": "dependency_l2",
                                                                }));
                                                            }
                                                        }
                                                    }
                                                    infra_ids.extend(dreq.infra);
                                                    kb_categories.extend(dreq.kb);
                                                }
                                            }
                                        }
                                    }
                                }
                                infra_ids.extend(req.infra);
                                kb_categories.extend(req.kb);
                            }
                        }
                    } else if let Some(skill_meta) = self.skills.get(topic_name) {
                        // Fallback: found in memory index but not in DB
                        skill_results.push(json!({
                            "name": skill_meta.name,
                            "path": skill_meta.path,
                            "description": skill_meta.description,
                            "matched_by": if args.skill.is_some() { "direct" } else { "query" },
                        }));
                    }
                }

                // Step 3: Aggregate Infra
                let mut infra_results: Vec<Value> = Vec::new();
                for id in &infra_ids {
                    if let Some(server) = self.infra.get(id) {
                        infra_results.push(json!({
                            "id": server.id,
                            "name": server.name,
                            "host": server.host,
                            "roles": server.roles,
                            "matched_by": "dependency",
                        }));
                    }
                }

                // Step 4: Aggregate KB (by category + query joint search)
                let mut kb_results: Vec<Value> = Vec::new();
                let mut kb_seen = std::collections::HashSet::new();
                for cat in &kb_categories {
                    if let Ok(entries) = db.kb_search(&args.query, Some(cat)) {
                        for entry in entries.iter().take(5) {
                            if kb_seen.insert(entry.key.clone()) {
                                kb_results.push(json!({
                                    "key": entry.key,
                                    "category": entry.category,
                                    "summary": entry.summary,
                                    "matched_by": "category_filter",
                                }));
                            }
                        }
                    }
                }
                // Also do a general KB search if no category filter yielded results
                if kb_results.is_empty() {
                    if let Ok(entries) = db.kb_search(&args.query, None) {
                        for entry in entries.iter().take(5) {
                            if kb_seen.insert(entry.key.clone()) {
                                kb_results.push(json!({
                                    "key": entry.key,
                                    "category": entry.category,
                                    "summary": entry.summary,
                                    "matched_by": "query",
                                }));
                            }
                        }
                    }
                }

                // Step 5: Optional Board search
                let mut board_results: Vec<Value> = Vec::new();
                if include_board {
                    if let Ok(tasks) = db.list_board_tasks(None, false) {
                        let query_lower = args.query.to_lowercase();
                        for task in tasks.iter().take(100) {
                            if task.title.to_lowercase().contains(&query_lower)
                                || task.description.to_lowercase().contains(&query_lower) {
                                board_results.push(json!({
                                    "id": task.id,
                                    "title": task.title,
                                    "status": task.status,
                                }));
                                if board_results.len() >= 5 { break; }
                            }
                        }
                    }
                }

                let result = json!({
                    "skills": skill_results,
                    "infra": infra_results,
                    "kb": kb_results,
                    "board": board_results,
                });

                Ok(ToolResult::json_pretty(&result))
            }

            // ===== Skill Engine (CQRS write tools) =====
            "mission_skill_upsert" => {
                #[derive(Deserialize)]
                struct SkillUpsertArgs {
                    topic: String,
                    section_title: String,
                    content: String,
                    sort_order: Option<i32>,
                }
                let args: SkillUpsertArgs = serde_json::from_value(args)?;
                let db = self.mission.db();

                // Ensure topic exists
                if db.skill_topic_get(&args.topic).map_err(|e| anyhow!("DB: {}", e))?.is_none() {
                    // Auto-create topic with default path
                    let skills_dir = dirs::home_dir()
                        .unwrap_or_else(|| std::path::PathBuf::from("."))
                        .join(".claude/skills");
                    let file_path = skills_dir.join(&args.topic).join("SKILL.md");
                    db.skill_topic_upsert(
                        &args.topic, None, None, None,
                        &file_path.to_string_lossy(), None, None,
                    ).map_err(|e| anyhow!("DB: {}", e))?;
                }

                // Find existing block with same title, or create new
                let blocks = db.skill_blocks_for_topic(&args.topic)
                    .map_err(|e| anyhow!("DB: {}", e))?;
                let existing = blocks.iter().find(|b| b.title.as_deref() == Some(&args.section_title));

                let action;
                if let Some(block) = existing {
                    db.skill_block_update(&block.id, &args.content)
                        .map_err(|e| anyhow!("DB: {}", e))?;
                    action = "updated";
                } else {
                    let sort = args.sort_order.unwrap_or(blocks.len() as i32);
                    db.skill_block_insert(&args.topic, "section", Some(&args.section_title), &args.content, sort)
                        .map_err(|e| anyhow!("DB: {}", e))?;
                    action = "created";
                }

                // Materialize to file
                match missiond_core::skill::materialize_topic(db, &args.topic) {
                    Ok(_) => Ok(ToolResult::text(format!("Section '{}' {} in topic '{}', file regenerated", args.section_title, action, args.topic))),
                    Err(e) => Ok(ToolResult::text(format!("Section {} but materialize failed: {}", action, e))),
                }
            }
            "mission_skill_record" => {
                #[derive(Deserialize)]
                struct SkillRecordArgs {
                    topic: String,
                    content: String,
                }
                let args: SkillRecordArgs = serde_json::from_value(args)?;
                let db = self.mission.db();

                // Ensure topic exists
                if db.skill_topic_get(&args.topic).map_err(|e| anyhow!("DB: {}", e))?.is_none() {
                    let skills_dir = dirs::home_dir()
                        .unwrap_or_else(|| std::path::PathBuf::from("."))
                        .join(".claude/skills");
                    let file_path = skills_dir.join(&args.topic).join("SKILL.md");
                    db.skill_topic_upsert(
                        &args.topic, None, None, None,
                        &file_path.to_string_lossy(), None, None,
                    ).map_err(|e| anyhow!("DB: {}", e))?;
                }

                db.skill_block_insert(&args.topic, "fragment", None, &args.content, 0)
                    .map_err(|e| anyhow!("DB: {}", e))?;

                let topic_meta = db.skill_topic_get(&args.topic)
                    .map_err(|e| anyhow!("DB: {}", e))?;
                let frag_count = topic_meta.map(|t| t.fragment_count).unwrap_or(0);

                // Materialize
                let _ = missiond_core::skill::materialize_topic(db, &args.topic);

                let mut msg = format!("Fragment recorded for '{}' ({} fragments)", args.topic, frag_count);
                if frag_count >= 5 {
                    msg.push_str(". Recommend running mission_skill_optimize to consolidate.");
                }
                Ok(ToolResult::text(msg))
            }
            "mission_skill_render" => {
                #[derive(Deserialize)]
                struct SkillRenderArgs {
                    topic: Option<String>,
                }
                let args: SkillRenderArgs = serde_json::from_value(args)
                    .unwrap_or(SkillRenderArgs { topic: None });
                let db = self.mission.db();

                if let Some(topic) = args.topic {
                    match missiond_core::skill::materialize_topic(db, &topic) {
                        Ok(output) => Ok(ToolResult::text(format!("Rendered '{}' ({} lines)", topic, output.lines().count()))),
                        Err(e) => Ok(ToolResult::error(format!("Render failed: {}", e))),
                    }
                } else {
                    match missiond_core::skill::materialize_all(db) {
                        Ok(count) => Ok(ToolResult::text(format!("Rendered all {} skills", count))),
                        Err(e) => Ok(ToolResult::error(format!("Render all failed: {}", e))),
                    }
                }
            }
            "mission_skill_topics" => {
                let db = self.mission.db();
                let topics = db.skill_topic_list()
                    .map_err(|e| anyhow!("DB: {}", e))?;
                Ok(ToolResult::json_pretty(&topics))
            }

            // ===== Skill Execution (Phase 3) =====
            "mission_skill_exec" => {
                #[derive(Deserialize)]
                struct SkillExecArgs {
                    skill: String,
                    action: String,
                    #[serde(default)]
                    dry_run: bool,
                    params: Option<Value>,
                }
                let args: SkillExecArgs = serde_json::from_value(args)?;

                match self.execute_workflow(&args.skill, &args.action, args.dry_run, args.params, 0).await {
                    Ok(result) => Ok(ToolResult::json_pretty(&result)),
                    Err(e) => Ok(ToolResult::error(format!("Workflow execution failed: {}", e))),
                }
            }
            "mission_skill_actions" => {
                #[derive(Deserialize)]
                struct SkillActionsArgs {
                    skill: Option<String>,
                }
                let args: SkillActionsArgs = serde_json::from_value(args)
                    .unwrap_or(SkillActionsArgs { skill: None });
                let db = self.mission.db();

                let topics = if let Some(ref name) = args.skill {
                    db.skill_topic_get(name)
                        .map_err(|e| anyhow!("DB: {}", e))?
                        .into_iter().collect::<Vec<_>>()
                } else {
                    db.skill_topic_list()
                        .map_err(|e| anyhow!("DB: {}", e))?
                };

                let mut all_actions: Vec<Value> = Vec::new();
                for topic in &topics {
                    if let Some(ref json_str) = topic.actions_json {
                        if let Ok(actions) = serde_json::from_str::<Vec<missiond_core::SkillAction>>(json_str) {
                            // Also count workflow steps from file
                            let step_counts = if let Ok(content) = std::fs::read_to_string(&topic.file_path) {
                                let workflows = missiond_core::parse_workflow_blocks(&content);
                                workflows.iter().map(|w| (w.id.clone(), w.steps.len())).collect::<std::collections::HashMap<_, _>>()
                            } else {
                                std::collections::HashMap::new()
                            };

                            for action in actions {
                                all_actions.push(serde_json::json!({
                                    "skill": topic.topic,
                                    "action_id": action.id,
                                    "name": action.name,
                                    "requires_approval": action.requires_approval,
                                    "step_count": step_counts.get(&action.id).unwrap_or(&0),
                                }));
                            }
                        }
                    }
                }

                Ok(ToolResult::json_pretty(&all_actions))
            }

            // ===== Skill Execution Stats (Phase 4) =====
            "mission_skill_stats" => {
                #[derive(Deserialize)]
                struct StatsArgs {
                    skill: Option<String>,
                }
                let args: StatsArgs = serde_json::from_value(args)
                    .unwrap_or(StatsArgs { skill: None });
                let db = self.mission.db();
                let stats = db.skill_execution_stats(args.skill.as_deref())
                    .map_err(|e| anyhow!("DB: {}", e))?;
                Ok(ToolResult::json_pretty(&stats))
            }

            // ===== Skill Version Rollback (Phase 4) =====
            "mission_skill_rollback" => {
                #[derive(Deserialize)]
                struct RollbackArgs {
                    skill: String,
                    version_id: Option<i64>,
                }
                let args: RollbackArgs = serde_json::from_value(args)?;
                let db = self.mission.db();

                if let Some(vid) = args.version_id {
                    // Rollback to specific version
                    let version = db.skill_version_get(vid)
                        .map_err(|e| anyhow!("DB: {}", e))?
                        .ok_or_else(|| anyhow!("Version {} not found", vid))?;
                    if version.topic != args.skill {
                        return Ok(ToolResult::error(format!("Version {} belongs to '{}', not '{}'", vid, version.topic, args.skill)));
                    }
                    let topic = db.skill_topic_get(&args.skill)
                        .map_err(|e| anyhow!("DB: {}", e))?
                        .ok_or_else(|| anyhow!("Skill '{}' not found", args.skill))?;
                    std::fs::write(&topic.file_path, &version.content)
                        .map_err(|e| anyhow!("Write error: {}", e))?;
                    // Re-ingest the skill
                    let skills_dir = std::path::Path::new(&topic.file_path).parent()
                        .and_then(|p| p.parent())
                        .unwrap_or(std::path::Path::new("."));
                    missiond_core::skill::ingest_skills(db, skills_dir);
                    Ok(ToolResult::text(format!("Rolled back '{}' to version {} ({})", args.skill, vid, version.created_at)))
                } else {
                    // List available versions
                    let versions = db.skill_version_list(&args.skill, 10)
                        .map_err(|e| anyhow!("DB: {}", e))?;
                    if versions.is_empty() {
                        return Ok(ToolResult::text(format!("No version history for '{}'", args.skill)));
                    }
                    let list: Vec<Value> = versions.iter().map(|v| {
                        serde_json::json!({
                            "version_id": v.id,
                            "checksum": v.checksum,
                            "created_at": v.created_at,
                            "content_lines": v.content.lines().count(),
                        })
                    }).collect();
                    Ok(ToolResult::json_pretty(&list))
                }
            }

            // ===== Infrastructure Registry =====
            "mission_infra_list" => {
                let InfraListArgs { role, provider } =
                    serde_json::from_value(args).unwrap_or(InfraListArgs { role: None, provider: None });
                let servers: Vec<&missiond_core::InfraServer> = if let Some(role) = role {
                    self.infra.by_role(&role)
                } else if let Some(provider) = provider {
                    self.infra.by_provider(&provider)
                } else {
                    self.infra.servers.iter().collect()
                };
                Ok(ToolResult::json_pretty(&servers))
            }
            "mission_infra_get" => {
                let InfraGetArgs { id } = serde_json::from_value(args)?;
                match self.infra.get(&id) {
                    Some(server) => Ok(ToolResult::json_pretty(&server)),
                    None => Ok(ToolResult::error(format!("Server not found: {}", id))),
                }
            }

            "mission_reachability" => {
                let ReachabilityArgs { target, channels } = serde_json::from_value(args)?;

                let server = self.infra.get(&target);
                let public_ip = server.and_then(|s| s.host.as_deref());
                let lan_ip = server.and_then(|s| s.lan.as_deref());
                let server_name = server.map(|s| s.name.as_str()).unwrap_or(&target);

                // Parse Tailscale IP from description (e.g. "ssh user@100.x.x.x")
                let ts_ip = server.and_then(|s| {
                    let targets = s.parse_ssh_targets();
                    targets.iter().find(|t| t.via == "tailscale").map(|t| t.host.clone())
                });

                let should_probe = |ch: &str| -> bool {
                    channels.as_ref().map_or(true, |chs| chs.iter().any(|c| c == ch))
                };

                // Probe 1: LAN ping
                let lan_ip_owned = lan_ip.map(String::from);
                let do_lan = should_probe("lan_ping") && lan_ip_owned.is_some();
                let lan_ping_fut = async {
                    if !do_lan { return None; }
                    let ip = lan_ip_owned.as_ref().unwrap();
                    let output = tokio::process::Command::new("ping")
                        .args(["-c", "3", "-W", "2", ip])
                        .output().await.ok()?;
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let latency = stdout.lines()
                        .find(|l| l.contains("avg"))
                        .and_then(|l| l.split('=').nth(1))
                        .and_then(|v| v.split('/').nth(1))
                        .and_then(|v| v.trim().parse::<f64>().ok());
                    Some(serde_json::json!({
                        "reachable": output.status.success(),
                        "ip": ip,
                        "latency_ms": latency,
                        "error": if !output.status.success() { Some(stderr.trim().to_string()) } else { None::<String> }
                    }))
                };

                // Probe 2: Public ping
                let public_ip_owned = public_ip.map(String::from);
                let do_pub = should_probe("public_ping") && public_ip_owned.is_some();
                let public_ping_fut = async {
                    if !do_pub { return None; }
                    let ip = public_ip_owned.as_ref().unwrap();
                    let output = tokio::process::Command::new("ping")
                        .args(["-c", "3", "-W", "2", ip])
                        .output().await.ok()?;
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let latency = stdout.lines()
                        .find(|l| l.contains("avg"))
                        .and_then(|l| l.split('=').nth(1))
                        .and_then(|v| v.split('/').nth(1))
                        .and_then(|v| v.trim().parse::<f64>().ok());
                    Some(serde_json::json!({
                        "reachable": output.status.success(),
                        "ip": ip,
                        "latency_ms": latency,
                        "error": if !output.status.success() { Some(stderr.trim().to_string()) } else { None::<String> }
                    }))
                };

                // Probe 3: Tailscale status
                let ts_ip_owned = ts_ip.clone();
                let do_ts = should_probe("tailscale");
                let tailscale_fut = async {
                    if !do_ts { return None; }
                    let output = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        tokio::process::Command::new("tailscale")
                            .args(["status", "--json"])
                            .output()
                    ).await.ok()?.ok()?;
                    if !output.status.success() { return None; }
                    let status_json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
                    let peers = status_json.get("Peer")?.as_object()?;
                    for (_key, peer) in peers {
                        let ips = peer.get("TailscaleIPs")?.as_array()?;
                        let ip_strs: Vec<&str> = ips.iter().filter_map(|v| v.as_str()).collect();
                        // Match by Tailscale IP from description
                        let matched = ts_ip_owned.as_ref().map_or(false, |tip| ip_strs.contains(&tip.as_str()));
                        if matched {
                            let online = peer.get("Online").and_then(|v| v.as_bool()).unwrap_or(false);
                            let last_seen = peer.get("LastSeen").and_then(|v| v.as_str()).unwrap_or("unknown");
                            let dns_name = peer.get("DNSName").and_then(|v| v.as_str()).unwrap_or("");
                            let hostname = dns_name.split('.').next().unwrap_or(
                                peer.get("HostName").and_then(|v| v.as_str()).unwrap_or("unknown")
                            );
                            let ip = ip_strs.first().unwrap_or(&"unknown");
                            return Some(serde_json::json!({
                                "status": if online { "online" } else { "offline" },
                                "hostname": hostname,
                                "ip": ip,
                                "last_seen": if online { "now".to_string() } else { last_seen.to_string() },
                                "os": peer.get("OS").and_then(|v| v.as_str()),
                            }));
                        }
                    }
                    Some(serde_json::json!({ "status": "not_found", "error": "Node not found in Tailscale peers" }))
                };

                // Probe 4: SSH TCP port
                let ssh_targets_owned: Vec<(String, u16, String)> = server
                    .map(|s| s.parse_ssh_targets().into_iter().map(|t| (t.host, t.port, t.via)).collect())
                    .unwrap_or_default();
                let do_ssh = should_probe("ssh") && !ssh_targets_owned.is_empty();
                let ssh_fut = async {
                    if !do_ssh { return None; }
                    for (host, port, via) in &ssh_targets_owned {
                        let addr = format!("{}:{}", host, port);
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(5),
                            tokio::net::TcpStream::connect(&addr)
                        ).await {
                            Ok(Ok(_)) => return Some(serde_json::json!({
                                "reachable": true, "ip": host, "port": port, "via": via,
                            })),
                            Ok(Err(e)) => return Some(serde_json::json!({
                                "reachable": false, "ip": host, "port": port, "via": via,
                                "error": e.to_string(),
                            })),
                            Err(_) => continue, // timeout, try next
                        }
                    }
                    Some(serde_json::json!({ "reachable": false, "error": "All SSH targets timed out" }))
                };

                // Probe 5: Deploy agent HTTP (reads health_endpoint from servers.yaml)
                let health_url = server.and_then(|s| s.health_endpoint.clone());
                let do_agent = should_probe("deploy_agent") && health_url.is_some();
                let agent_fut = async {
                    if !do_agent { return None; }
                    let url = health_url.as_ref().unwrap();
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(5))
                        .danger_accept_invalid_certs(true)
                        .build().ok()?;
                    match client.get(url).send().await {
                        Ok(resp) => {
                            let status = resp.status().as_u16();
                            Some(serde_json::json!({
                                "reachable": status == 200,
                                "http_status": status,
                            }))
                        }
                        Err(e) => Some(serde_json::json!({
                            "reachable": false,
                            "error": e.to_string(),
                        })),
                    }
                };

                // Run all in parallel
                let (lan_ping, public_ping, tailscale, ssh, agent) = tokio::join!(
                    lan_ping_fut, public_ping_fut, tailscale_fut, ssh_fut, agent_fut
                );

                let mut channels_result = serde_json::Map::new();
                if let Some(v) = lan_ping { channels_result.insert("lan_ping".into(), v); }
                if let Some(v) = public_ping { channels_result.insert("public_ping".into(), v); }
                if let Some(v) = tailscale { channels_result.insert("tailscale".into(), v); }
                if let Some(v) = ssh { channels_result.insert("ssh".into(), v); }
                if let Some(v) = agent { channels_result.insert("deploy_agent".into(), v); }

                let total = channels_result.len();
                let reachable = channels_result.values().filter(|v| {
                    v.get("reachable").and_then(|r| r.as_bool()).unwrap_or(false)
                        || v.get("status").and_then(|s| s.as_str()) == Some("online")
                }).count();

                let severity = if reachable == 0 { "red" }
                    else if reachable < total { "yellow" }
                    else { "green" };

                Ok(ToolResult::json_pretty(&serde_json::json!({
                    "target": target,
                    "name": server_name,
                    "channels": channels_result,
                    "summary": format!("{}/{} channels reachable", reachable, total),
                    "severity": severity,
                })))
            }

            "mission_os_diagnose" => {
                let OsDiagnoseArgs { target, checks } = serde_json::from_value(args)?;

                let server = self.infra.get(&target);
                if server.is_none() && !target.contains('@') && !target.contains('.') {
                    return Ok(ToolResult::error(format!("Server not found in infra registry: {}", target)));
                }

                // Resolve SSH targets
                let ssh_targets = if let Some(srv) = server {
                    let mut targets = srv.parse_ssh_targets();
                    if targets.is_empty() {
                        if let Some(ip) = srv.host.as_deref() {
                            targets.push(missiond_core::types::SshTarget {
                                user: "root".to_string(),
                                host: ip.to_string(),
                                port: 22,
                                password: None,
                                via: "public".to_string(),
                            });
                        }
                    }
                    targets
                } else if target.contains('@') {
                    let parts: Vec<&str> = target.splitn(2, '@').collect();
                    vec![missiond_core::types::SshTarget {
                        user: parts[0].to_string(),
                        host: parts[1].to_string(),
                        port: 22,
                        password: None,
                        via: "direct".to_string(),
                    }]
                } else {
                    vec![missiond_core::types::SshTarget {
                        user: "root".to_string(),
                        host: target.clone(),
                        port: 22,
                        password: None,
                        via: "direct".to_string(),
                    }]
                };

                if ssh_targets.is_empty() {
                    return Ok(ToolResult::error(format!("No SSH targets for {}", target)));
                }

                // KB credential fallback
                let kb_pass = self.mission.db()
                    .kb_search(&format!("{} password", target), Some("credential"))
                    .ok()
                    .and_then(|entries| entries.into_iter().next())
                    .and_then(|e| e.detail.as_ref()
                        .and_then(|d| d.get("password").and_then(|v| v.as_str().map(String::from))));

                // Build probe script based on checks filter
                let all_checks = ["system", "crashes", "top_cpu", "temperatures", "journal_errors", "docker", "network", "gpu"];
                let active: Vec<&str> = if let Some(ref chs) = checks {
                    chs.iter().map(|s| s.as_str()).filter(|c| all_checks.contains(c)).collect()
                } else {
                    all_checks.to_vec()
                };

                let mut script = String::from("#!/bin/bash\n");

                if active.contains(&"system") {
                    script.push_str(concat!(
                        "echo 'SECTION=system'\n",
                        "echo \"HOSTNAME=$(hostname)\"\n",
                        "echo \"KERNEL=$(uname -r)\"\n",
                        "echo \"UPTIME=$(uptime)\"\n",
                        "LOAD=$(cat /proc/loadavg 2>/dev/null || echo unknown); echo \"LOAD=$LOAD\"\n",
                        "FREE=$(LANG=C free -h 2>/dev/null | awk '/Mem:/{printf \"%s/%s\", $3, $2}'); echo \"MEMORY=${FREE:-unknown}\"\n",
                        "DISK=$(LANG=C df -h / 2>/dev/null | awk 'NR==2{printf \"%s/%s (%s)\", $3, $2, $5}'); echo \"DISK=${DISK:-unknown}\"\n",
                        "DISK_PCT=$(LANG=C df / 2>/dev/null | awk 'NR==2{print $5}' | tr -d '%'); echo \"DISK_PCT=${DISK_PCT:-0}\"\n",
                        "echo \"NPROC=$(nproc 2>/dev/null || echo 1)\"\n",
                    ));
                }

                if active.contains(&"crashes") {
                    script.push_str(concat!(
                        "echo 'SECTION=crashes'\n",
                        "if [ -d /var/crash ]; then\n",
                        "  for f in /var/crash/*.crash; do\n",
                        "    [ -f \"$f\" ] || continue\n",
                        "    echo \"CRASH_FILE=$f\"\n",
                        "    grep -E '^(ProblemType|Date|ExecutablePath|Signal|SignalName|Package)' \"$f\" 2>/dev/null || true\n",
                        "    echo '---'\n",
                        "  done\n",
                        "else\n",
                        "  echo 'NO_CRASH_DIR'\n",
                        "fi\n",
                    ));
                }

                if active.contains(&"top_cpu") {
                    script.push_str(concat!(
                        "echo 'SECTION=top_cpu'\n",
                        "ps aux --sort=-%cpu 2>/dev/null | head -11 || echo 'PS_FAILED'\n",
                    ));
                }

                if active.contains(&"temperatures") {
                    script.push_str(concat!(
                        "echo 'SECTION=temperatures'\n",
                        "sensors 2>/dev/null || echo 'NO_SENSORS'\n",
                    ));
                }

                if active.contains(&"journal_errors") {
                    script.push_str(concat!(
                        "echo 'SECTION=journal_errors'\n",
                        "journalctl --since '1 hour ago' -p err -n 20 --no-pager 2>/dev/null || echo 'NO_JOURNALCTL'\n",
                    ));
                }

                if active.contains(&"docker") {
                    script.push_str(concat!(
                        "echo 'SECTION=docker'\n",
                        "docker ps --format 'table {{.Names}}\\t{{.Image}}\\t{{.Status}}' 2>/dev/null || echo 'NO_DOCKER'\n",
                    ));
                }

                if active.contains(&"network") {
                    script.push_str(concat!(
                        "echo 'SECTION=network'\n",
                        "ss -tlnp 2>/dev/null | head -30 || echo 'NO_SS'\n",
                    ));
                }

                if active.contains(&"gpu") {
                    script.push_str(concat!(
                        "echo 'SECTION=gpu'\n",
                        "nvidia-smi --query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu --format=csv,noheader 2>/dev/null || ",
                        "cat /sys/class/drm/card*/device/gpu_busy_percent 2>/dev/null || ",
                        "echo 'NO_GPU'\n",
                    ));
                }

                // Try SSH targets in order
                let mut last_error = String::new();
                let mut connected_via = String::new();
                let mut raw_output = String::new();

                for st in &ssh_targets {
                    let pass = st.password.as_ref().or(kb_pass.as_ref());
                    let mut ssh_args: Vec<String> = Vec::new();
                    if let Some(p) = pass {
                        ssh_args.extend(["sshpass".into(), "-p".into(), p.clone(), "ssh".into()]);
                    } else {
                        ssh_args.push("ssh".into());
                        ssh_args.extend(["-o".into(), "BatchMode=yes".into()]);
                    }
                    ssh_args.extend([
                        "-o".into(), "StrictHostKeyChecking=no".into(),
                        "-o".into(), "ConnectTimeout=10".into(),
                        "-p".into(), st.port.to_string(),
                        format!("{}@{}", st.user, st.host),
                        "bash".into(),
                    ]);

                    let program = ssh_args.remove(0);
                    let mut cmd = tokio::process::Command::new(&program);
                    cmd.args(&ssh_args);
                    cmd.stdin(std::process::Stdio::piped());
                    cmd.stdout(std::process::Stdio::piped());
                    cmd.stderr(std::process::Stdio::piped());

                    match cmd.spawn() {
                        Ok(mut child) => {
                            if let Some(mut stdin) = child.stdin.take() {
                                use tokio::io::AsyncWriteExt;
                                stdin.write_all(script.as_bytes()).await.ok();
                                drop(stdin);
                            }
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                child.wait_with_output()
                            ).await {
                                Ok(Ok(output)) if output.status.success() => {
                                    connected_via = format!("{} ({}:{})", st.via, st.host, st.port);
                                    raw_output = String::from_utf8_lossy(&output.stdout).to_string();
                                    break;
                                }
                                Ok(Ok(output)) => {
                                    // Non-zero exit but may still have partial output
                                    let stdout = String::from_utf8_lossy(&output.stdout);
                                    if !stdout.trim().is_empty() {
                                        connected_via = format!("{} ({}:{})", st.via, st.host, st.port);
                                        raw_output = stdout.to_string();
                                        break;
                                    }
                                    last_error = String::from_utf8_lossy(&output.stderr).trim().to_string();
                                }
                                Ok(Err(e)) => { last_error = e.to_string(); }
                                Err(_) => { last_error = format!("SSH timed out (30s) via {} {}:{}", st.via, st.host, st.port); }
                            }
                        }
                        Err(e) => { last_error = e.to_string(); }
                    }
                }

                if raw_output.is_empty() {
                    return Ok(ToolResult::error(format!(
                        "All SSH channels failed for '{}'. Last error: {}", target, last_error
                    )));
                }

                // Parse SECTION-based output
                let mut result = serde_json::Map::new();
                result.insert("target".into(), serde_json::json!(target));
                result.insert("connected_via".into(), serde_json::json!(connected_via));

                let mut current_section = String::new();
                let mut section_lines: Vec<String> = Vec::new();

                let parse_section = |name: &str, lines: &[String]| -> serde_json::Value {
                    match name {
                        "system" => {
                            let mut obj = serde_json::Map::new();
                            for line in lines {
                                if let Some((k, v)) = line.split_once('=') {
                                    let key = k.trim().to_lowercase();
                                    let val = v.trim().to_string();
                                    if !val.is_empty() && val != "unknown" {
                                        obj.insert(key, serde_json::Value::String(val));
                                    }
                                }
                            }
                            serde_json::Value::Object(obj)
                        }
                        "crashes" => {
                            let mut crashes = Vec::new();
                            let mut current: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
                            for line in lines {
                                if line == "NO_CRASH_DIR" {
                                    return serde_json::json!([]);
                                }
                                if line.starts_with("CRASH_FILE=") {
                                    if !current.is_empty() { crashes.push(serde_json::Value::Object(current.clone())); }
                                    current = serde_json::Map::new();
                                    current.insert("file".into(), serde_json::json!(line.strip_prefix("CRASH_FILE=").unwrap_or("")));
                                } else if line == "---" {
                                    if !current.is_empty() { crashes.push(serde_json::Value::Object(current.clone())); }
                                    current = serde_json::Map::new();
                                } else if let Some((k, v)) = line.split_once(": ") {
                                    current.insert(k.to_lowercase().replace(' ', "_"), serde_json::json!(v.trim()));
                                }
                            }
                            serde_json::json!(crashes)
                        }
                        _ => {
                            // Raw text for top_cpu, temperatures, journal_errors, docker, network, gpu
                            let text = lines.join("\n");
                            serde_json::Value::String(text)
                        }
                    }
                };

                for line in raw_output.lines() {
                    if let Some(section_name) = line.strip_prefix("SECTION=") {
                        if !current_section.is_empty() {
                            result.insert(current_section.clone(), parse_section(&current_section, &section_lines));
                        }
                        current_section = section_name.to_string();
                        section_lines.clear();
                    } else {
                        section_lines.push(line.to_string());
                    }
                }
                if !current_section.is_empty() {
                    result.insert(current_section.clone(), parse_section(&current_section, &section_lines));
                }

                // Compute severity
                let mut severity = "green";
                if let Some(sys) = result.get("system").and_then(|v| v.as_object()) {
                    // Check disk usage
                    if let Some(pct) = sys.get("disk_pct").and_then(|v| v.as_str()).and_then(|v| v.parse::<u32>().ok()) {
                        if pct > 90 { severity = "red"; }
                        else if pct > 80 { severity = "yellow"; }
                    }
                    // Check load vs nproc
                    if let Some(load_str) = sys.get("load").and_then(|v| v.as_str()) {
                        if let Some(nproc) = sys.get("nproc").and_then(|v| v.as_str()).and_then(|v| v.parse::<f64>().ok()) {
                            if let Some(load1) = load_str.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()) {
                                if load1 > nproc { severity = "red"; }
                                else if load1 > nproc * 0.8 && severity != "red" { severity = "yellow"; }
                            }
                        }
                    }
                }
                if let Some(crashes) = result.get("crashes").and_then(|v| v.as_array()) {
                    if !crashes.is_empty() && severity != "red" { severity = "yellow"; }
                }

                result.insert("severity".into(), serde_json::json!(severity));

                Ok(ToolResult::json_pretty(&serde_json::Value::Object(result)))
            }

            "mission_board_note_add" => {
                let args: BoardNoteAddArgs = serde_json::from_value(args)?;
                let input = missiond_core::types::AddBoardTaskNoteInput {
                    task_id: args.task_id,
                    content: args.content,
                    note_type: args.note_type,
                    author: args.author,
                };
                let note = self
                    .mission
                    .db()
                    .add_board_task_note(&input)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json_pretty(&note))
            }

            "mission_board_summary" => {
                #[derive(Deserialize)]
                struct SummaryArgs {
                    since: Option<String>,
                }
                let args: SummaryArgs = serde_json::from_value(args)?;
                let summary = self
                    .mission
                    .db()
                    .board_summary(args.since.as_deref())
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json_pretty(&summary))
            }

            // ===== Slot Task History =====
            "mission_slot_history" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct SlotHistoryArgs {
                    slot_id: Option<String>,
                    task_type: Option<String>,
                    status: Option<String>,
                    limit: Option<i64>,
                    stats: Option<bool>,
                }
                let args: SlotHistoryArgs = serde_json::from_value(args)?;
                if args.stats.unwrap_or(false) {
                    let stats = self.mission.db()
                        .slot_task_stats(args.slot_id.as_deref())
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json_pretty(&stats))
                } else {
                    let tasks = self.mission.db()
                        .list_slot_tasks(
                            args.slot_id.as_deref(),
                            args.task_type.as_deref(),
                            args.status.as_deref(),
                            args.limit.unwrap_or(20),
                        )
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json_pretty(&tasks))
                }
            }

            // ===== Agent Questions (Pending Decisions) =====
            "mission_question_create" => {
                let args: QuestionCreateArgs = serde_json::from_value(args)?;
                let input = missiond_core::types::CreateAgentQuestionInput {
                    question: args.question,
                    context: args.context,
                    task_id: args.task_id,
                    slot_id: args.slot_id,
                    session_id: args.session_id,
                };
                let q = self
                    .mission
                    .db()
                    .create_agent_question(&input)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json_pretty(&q))
            }
            "mission_question_list" => {
                let QuestionListArgs { status } =
                    serde_json::from_value(args).unwrap_or(QuestionListArgs { status: None });
                let questions = self
                    .mission
                    .db()
                    .list_agent_questions(status.as_deref())
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json_pretty(&questions))
            }
            "mission_question_get" => {
                let QuestionIdArgs { id } = serde_json::from_value(args)?;
                match self
                    .mission
                    .db()
                    .get_agent_question(&id)
                    .map_err(|e| anyhow!("DB error: {}", e))?
                {
                    Some(q) => Ok(ToolResult::json_pretty(&q)),
                    None => Ok(ToolResult::error("Question not found")),
                }
            }
            "mission_question_answer" => {
                let QuestionAnswerArgs { id, answer } = serde_json::from_value(args)?;
                match self
                    .mission
                    .db()
                    .answer_agent_question(&id, &answer)
                    .map_err(|e| anyhow!("DB error: {}", e))?
                {
                    Some(q) => Ok(ToolResult::json_pretty(&q)),
                    None => Ok(ToolResult::error("Question not found")),
                }
            }
            "mission_question_dismiss" => {
                let QuestionIdArgs { id } = serde_json::from_value(args)?;
                match self
                    .mission
                    .db()
                    .dismiss_agent_question(&id)
                    .map_err(|e| anyhow!("DB error: {}", e))?
                {
                    Some(q) => Ok(ToolResult::json_pretty(&q)),
                    None => Ok(ToolResult::error("Question not found")),
                }
            }

            // ── Jarvis Trace ──
            "mission_jarvis_logs" => {
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
                let limit = limit.min(100);
                let status_filter = args.get("status").and_then(|v| v.as_str()).map(|s| s.to_string());
                let mut traces = self.jarvis_trace.list_traces(limit).await;
                if let Some(ref sf) = status_filter {
                    traces.retain(|t| t.status.to_string() == *sf);
                }
                Ok(ToolResult::json_pretty(&traces))
            }
            "mission_jarvis_trace" => {
                let trace_id = args.get("trace_id").and_then(|v| v.as_str());
                let trace = if let Some(id) = trace_id {
                    self.jarvis_trace.get_trace(id).await
                } else {
                    self.jarvis_trace.latest_trace().await
                };
                match trace {
                    Some(t) => Ok(ToolResult::json_pretty(&t)),
                    None => Ok(ToolResult::error("Trace not found")),
                }
            }

            _ => {
                // Proxy xjp_* tools to xjp-mcp server
                if name.starts_with("xjp_") {
                    match self.call_xjp_mcp_tool(name, args).await {
                        Ok(result) => Ok(result),
                        Err(e) => {
                            let mut res = ToolResult::text(format!("xjp-mcp proxy error: {}", e));
                            res.is_error = Some(true);
                            Ok(res)
                        }
                    }
                } else {
                    let mut res = ToolResult::text(format!("Unknown tool: {}", name));
                    res.is_error = Some(true);
                    Ok(res)
                }
            }
        }
    }

    /// Proxy a tool call to the xjp-mcp server via persistent client
    async fn call_xjp_mcp_tool(&self, tool_name: &str, tool_args: Value) -> Result<ToolResult> {
        self.xjp_mcp.call_tool(tool_name, tool_args).await
    }
}

// =========================
// IPC server (daemon)
// =========================

async fn handle_ipc_connection(state: AppState, mut reader: BufReader<IpcStream>) -> Result<()> {
    let mut line = String::new();
    let bytes = reader.read_line(&mut line).await?;
    if bytes == 0 {
        return Ok(());
    }

    let message = line.trim();
    let request = match protocol::parse_request_str(message) {
        Ok(r) => r,
        Err(e) => {
            let resp = Response::from_error(RequestId::Null, e);
            let json = protocol::serialize_response_string(&resp)?;
            reader.get_mut().write_all(json.as_bytes()).await?;
            reader.get_mut().write_all(b"\n").await?;
            reader.get_mut().flush().await?;
            return Ok(());
        }
    };

    let resp = handle_ipc_request(state, request).await;
    let json = protocol::serialize_response_string(&resp)?;
    reader.get_mut().write_all(json.as_bytes()).await?;
    reader.get_mut().write_all(b"\n").await?;
    reader.get_mut().flush().await?;
    Ok(())
}

async fn handle_ipc_request(state: AppState, request: Request) -> Response {
    let id = request.id.clone().unwrap_or(RequestId::Null);
    let method = request.method.as_str();
    let params = request.params.unwrap_or(Value::Null);

    match method {
        "ping" => Response::success(id, serde_json::json!({})),
        "kb/summary" => {
            let db = state.mission.db();
            let instructions = match db.kb_summary() {
                Ok(counts) => {
                    if counts.is_empty() {
                        "MissionD KB is empty. Use mission_kb_remember when you learn new facts. Use mission_kb_search before guessing.".to_string()
                    } else {
                        let parts: Vec<String> = counts
                            .iter()
                            .map(|(cat, n)| format!("{} {}", n, cat))
                            .collect();
                        // Preferences + hot topics are synced to CLAUDE.md (always visible).
                        // Instructions only carry KB stats + behavioral nudges.
                        format!(
                            "[MissionD] KB: {}. Use mission_kb_search before guessing. Use mission_kb_remember when learning.",
                            parts.join(", ")
                        )
                    }
                }
                Err(_) => String::new(),
            };
            Response::success(id, serde_json::json!({ "instructions": instructions }))
        }
        "tools/call" => {
            let name = match params.get("name").and_then(|v| v.as_str()) {
                Some(n) => n.to_string(),
                None => {
                    return Response::from_error(
                        id,
                        RpcError::InvalidParams("Missing 'name' field".to_string()),
                    );
                }
            };
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or(Value::Object(serde_json::Map::new()));

            let tool_res = state.call_tool(&name, arguments).await;
            Response::success(id, serde_json::to_value(tool_res).unwrap_or(Value::Null))
        }
        _ => Response::from_error(id, RpcError::MethodNotFound(method.to_string())),
    }
}

// =========================
// Autopilot Scheduler
// =========================

/// Check all running PTY slots for low context warnings.
/// When "Context left until auto-compact: X%" is detected and X < 10,
/// kill and restart the slot to avoid degraded performance from context compression.
async fn check_slot_context_levels(state: &AppState) {
    let slots = state.mission.list_slots();
    for slot in &slots {
        let status = state.pty.get_status(&slot.config.id).await;
        let Some(info) = status else { continue };
        // Only check running slots
        if info.state == SessionState::Exited {
            continue;
        }

        let screen = match state.pty.get_screen(&slot.config.id).await {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Look for "Context left until auto-compact: XX%"
        if let Some(pct) = extract_context_percentage(&screen) {
            if pct < 10 {
                warn!(
                    slot_id = %slot.config.id,
                    context_pct = pct,
                    "Slot context critically low, restarting"
                );
                // Kill the slot
                if let Err(e) = state.pty.kill(&slot.config.id).await {
                    warn!(error = %e, slot_id = %slot.config.id, "Failed to kill low-context slot");
                    continue;
                }
                // Requeue any Running submit tasks assigned to this slot
                match state.mission.db().requeue_running_tasks_for_slot(&slot.config.id) {
                    Ok(n) if n > 0 => warn!(slot_id = %slot.config.id, count = n, "Requeued running submit tasks after low-context restart"),
                    Err(e) => warn!(slot_id = %slot.config.id, error = %e, "Failed to requeue tasks after low-context restart"),
                    _ => {}
                }
                // Release any board task claims held by this slot
                let _ = state.mission.db().release_board_claims_by_executor(&slot.config.id);
                // Wait for exit
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;

                // Respawn
                let pty_slot = missiond_core::PTYSlot {
                    id: slot.config.id.clone(),
                    role: slot.config.role.clone(),
                    cwd: slot.config.cwd.as_deref().map(PathBuf::from),
                };
                let mcp_config = slot.config.mcp_config.clone().map(PathBuf::from);
                let (extra_env, session_file) = build_slot_tracking_env(&slot.config.id, slot.config.env.as_ref()).await;
                match state.pty.spawn(&pty_slot, PTYSpawnOptions {
                    auto_restart: false,
                    wait_for_idle: true,
                    timeout_secs: Some(120),
                    mcp_config,
                    dangerously_skip_permissions: slot.config.dangerously_skip_permissions.unwrap_or(false),
                    extra_env,
                }).await {
                    Ok(_) => {
                        capture_slot_session_uuid(state, &slot.config.id, &session_file).await;
                        info!(slot_id = %slot.config.id, "Slot restarted due to low context");
                    }
                    Err(e) => {
                        warn!(error = %e, slot_id = %slot.config.id, "Failed to respawn slot after context kill");
                    }
                }
            }
        }
    }
}

/// Extract context percentage from PTY screen text.
/// Looks for "Context left until auto-compact: XX%" pattern.
fn extract_context_percentage(screen: &str) -> Option<u32> {
    // The status bar text: "Context left until auto-compact: 12%"
    for line in screen.lines().rev() {
        if let Some(pos) = line.find("Context left until auto-compact:") {
            let after = &line[pos + "Context left until auto-compact:".len()..];
            let trimmed = after.trim().trim_end_matches('%');
            if let Ok(pct) = trimmed.parse::<u32>() {
                return Some(pct);
            }
        }
    }
    None
}

async fn autopilot_tick(state: &AppState) -> Result<()> {
    // Check PTY slots for low context — restart if < 10%
    check_slot_context_levels(state).await;

    // Complete stale active conversations (no messages for > 10 minutes)
    let cutoff = (chrono::Utc::now() - chrono::TimeDelta::minutes(10))
        .to_rfc3339();
    match state.mission.db().complete_stale_conversations(&cutoff) {
        Ok(n) if n > 0 => info!(count = n, "Completed stale conversations"),
        Err(e) => warn!(error = %e, "Failed to complete stale conversations"),
        _ => {}
    }

    let mut memory_paused = state.memory_paused.load(std::sync::atomic::Ordering::Relaxed);

    // TTL auto-resume: if paused for > 2 hours, auto-resume
    if memory_paused {
        const PAUSE_TTL_SECS: i64 = 2 * 60 * 60; // 2 hours
        let paused_at = state.memory_paused_at.load(std::sync::atomic::Ordering::Relaxed);
        if paused_at > 0 {
            let now = chrono::Utc::now().timestamp();
            if now - paused_at > PAUSE_TTL_SECS {
                warn!(paused_secs = now - paused_at, "Memory pause TTL expired, auto-resuming");
                state.memory_paused.store(false, std::sync::atomic::Ordering::Relaxed);
                state.memory_paused_at.store(0, std::sync::atomic::Ordering::Relaxed);
                let _ = std::fs::remove_file(default_mission_home().join("memory_paused"));
                memory_paused = false;
            }
        }
    }

    // Submit task dispatch — always runs, not gated by memory_paused
    dispatch_queued_submit_tasks(state).await;

    if !memory_paused {
        // Check if memory slots are stuck in non-Idle state for too long
        check_slot_stuck(state, MEMORY_SLOT_ID, &state.memory_slot_busy_since, &state.extraction_state).await;
        check_slot_stuck(state, MEMORY_SLOW_SLOT_ID, &state.slow_slot_busy_since, &state.slow_extraction_state).await;

        // Memory scheduler: realtime > deep > consolidation
        schedule_memory_tasks(state).await;
    }

    // FTS dirty flag rebuild: after kb_forget sets dirty, rebuild here
    match state.mission.db().kb_rebuild_fts_if_dirty() {
        Ok(true) => info!("autopilot: FTS index rebuilt (dirty flag)"),
        Err(e) => warn!(error = %e, "FTS dirty rebuild failed"),
        _ => {}
    }

    // Sync KB preferences + hot topics into CLAUDE.md
    sync_claude_md(state);

    // KB auto-GC: every hour
    check_kb_auto_gc(state);

    // Reaper: force-fail stale slot tasks (pending/running > 30 min)
    match state.mission.db().reap_stale_slot_tasks(1800) {
        Ok(n) if n > 0 => warn!(count = n, "Reaped stale slot tasks"),
        Err(e) => warn!(error = %e, "Slot task reaper failed"),
        _ => {}
    }

    // Reaper: timeout Running submit tasks after 15 minutes
    reap_stale_submit_tasks(state).await;

    // Extraction status summary (debug)
    {
        let now = chrono::Utc::now().timestamp();
        let fast_es = state.extraction_state.read().await;
        let fast_slot = state.pty.get_status(MEMORY_SLOT_ID).await
            .map(|s| format!("{:?}", s.state))
            .unwrap_or_else(|| "not_spawned".to_string());
        let slow_es = state.slow_extraction_state.read().await;
        let slow_slot = state.pty.get_status(MEMORY_SLOW_SLOT_ID).await
            .map(|s| format!("{:?}", s.state))
            .unwrap_or_else(|| "not_spawned".to_string());
        debug!(
            fast_slot = %fast_slot,
            fast_phase = ?fast_es.phase,
            fast_type = ?fast_es.active_type,
            fast_age = now - fast_es.phase_started_at,
            slow_slot = %slow_slot,
            slow_phase = ?slow_es.phase,
            slow_type = ?slow_es.active_type,
            slow_age = now - slow_es.phase_started_at,
            "autopilot: extraction status"
        );
    }

    // Recover stale running tasks (daemon restart or PTY crash)
    let _ = state.mission.db().recover_stale_running_tasks(15);

    let tasks = state.mission.db().list_autopilot_tasks()
        .map_err(|e| anyhow!("DB error: {}", e))?;

    if tasks.is_empty() {
        return Ok(());
    }

    info!(count = tasks.len(), "Autopilot: found executable tasks");

    for task in tasks {
        let slot_id = match &task.assignee {
            Some(id) => id.clone(),
            None => continue,
        };

        // Build prompt: template > "title\n\ndescription"
        let prompt = if let Some(ref tmpl) = task.prompt_template {
            tmpl.clone()
        } else {
            let mut p = task.title.clone();
            if !task.description.is_empty() {
                p.push_str("\n\n");
                p.push_str(&task.description);
            }
            p
        };

        // Inject context from Phase B skills
        let context = state.skills.build_context(&task.title);
        let full_prompt = if context.contains("No matching skills") {
            prompt
        } else {
            format!("{}\n\n{}", context, prompt)
        };

        // Slot throttling: skip if slot has 3+ consecutive failures within 30 min
        {
            let fail_map = state.slot_fail_counts.lock().unwrap();
            if let Some(&(count, last_fail)) = fail_map.get(&slot_id) {
                let now = chrono::Utc::now().timestamp();
                if count >= 3 && now - last_fail < 1800 {
                    debug!(slot_id = %slot_id, failures = count, "Autopilot: slot throttled, skipping");
                    continue;
                }
            }
        }

        info!(task_id = %task.id, slot_id = %slot_id, title = %task.title, "Autopilot: executing task");

        // Atomically claim the task (CAS: only succeeds if open + unclaimed)
        match state.mission.db().claim_board_task(&task.id, &slot_id, "pty_slot") {
            Ok(Some(_)) => {} // Successfully claimed
            Ok(None) => {
                debug!(task_id = %task.id, slot_id = %slot_id, "Autopilot: task already claimed, skipping");
                continue;
            }
            Err(e) => {
                warn!(task_id = %task.id, error = %e, "Autopilot: failed to claim task");
                continue;
            }
        }

        // Check if PTY session exists, spawn if needed
        let pty_status = state.pty.get_status(&slot_id).await;
        if pty_status.is_none() {
            // Try to find slot config and spawn
            let slot = state.mission.list_slots().into_iter().find(|s| s.config.id == slot_id);
            if let Some(slot) = slot {
                let pty_slot = missiond_core::PTYSlot {
                    id: slot.config.id.clone(),
                    role: slot.config.role.clone(),
                    cwd: slot.config.cwd.as_deref().map(PathBuf::from),
                };
                let slot_env = slot.config.env.as_ref();
                let mcp_config = slot.config.mcp_config.map(PathBuf::from);
                let (extra_env, session_file) = build_slot_tracking_env(&slot_id, slot_env).await;
                if let Err(e) = state.pty.spawn(&pty_slot, PTYSpawnOptions {
                    auto_restart: false,
                    wait_for_idle: true,
                    timeout_secs: Some(120),
                    mcp_config,
                    dangerously_skip_permissions: slot.config.dangerously_skip_permissions.unwrap_or(false),
                    extra_env,
                }).await {
                    warn!(task_id = %task.id, slot_id = %slot_id, error = %e, "Autopilot: failed to spawn PTY");
                    // Revert to open for retry
                    let _ = state.mission.db().update_board_task(
                        &task.id,
                        &missiond_core::types::UpdateBoardTaskInput {
                            status: Some("open".to_string()),
                            ..Default::default()
                        },
                    );
                    continue;
                }
                capture_slot_session_uuid(state, &slot_id, &session_file).await;
            } else {
                warn!(task_id = %task.id, slot_id = %slot_id, "Autopilot: slot not found, skipping");
                let _ = state.mission.db().update_board_task(
                    &task.id,
                    &missiond_core::types::UpdateBoardTaskInput {
                        status: Some("open".to_string()),
                        ..Default::default()
                    },
                );
                continue;
            }
        }

        // Inject answered questions as context (Phase 2 linkage)
        let full_prompt = {
            let answered = state.mission.db().list_questions_for_task(&task.id).unwrap_or_default();
            if answered.is_empty() {
                full_prompt
            } else {
                let qa_block: String = answered.iter()
                    .filter(|q| q.answer.is_some())
                    .map(|q| format!("Q: {}\nA: {}", q.question, q.answer.as_deref().unwrap_or("")))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                if qa_block.is_empty() {
                    full_prompt
                } else {
                    format!("[用户已回答的问题]\n{}\n\n{}", qa_block, full_prompt)
                }
            }
        };

        // Send prompt and wait for response
        let timeout_ms = 600_000; // 10 minutes
        match state.pty.send(&slot_id, &full_prompt, timeout_ms).await {
            Ok(res) => {
                // Record result as a board note
                let note_content = format!("**Autopilot 执行完成** ({}ms)\n\n{}", res.duration_ms, res.response);
                let _ = state.mission.db().add_board_task_note(
                    &missiond_core::types::AddBoardTaskNoteInput {
                        task_id: task.id.clone(),
                        content: note_content,
                        note_type: Some("summary".to_string()),
                        author: Some("autopilot".to_string()),
                    },
                );
                // Mark task as done
                let _ = state.mission.db().update_board_task(
                    &task.id,
                    &missiond_core::types::UpdateBoardTaskInput {
                        status: Some("done".to_string()),
                        ..Default::default()
                    },
                );
                // Reset slot failure count on success
                {
                    let mut fail_map = state.slot_fail_counts.lock().unwrap();
                    fail_map.remove(&slot_id);
                }
                info!(task_id = %task.id, duration_ms = res.duration_ms, "Autopilot: task completed");
            }
            Err(e) => {
                // Record failure as a note
                let note_content = format!("**Autopilot 执行失败** (retry {}/{})\n\n{}", task.retry_count + 1, task.max_retries, e);
                let _ = state.mission.db().add_board_task_note(
                    &missiond_core::types::AddBoardTaskNoteInput {
                        task_id: task.id.clone(),
                        content: note_content,
                        note_type: Some("note".to_string()),
                        author: Some("autopilot".to_string()),
                    },
                );

                // Track slot consecutive failures
                {
                    let mut fail_map = state.slot_fail_counts.lock().unwrap();
                    let entry = fail_map.entry(slot_id.clone()).or_insert((0, 0));
                    entry.0 += 1;
                    entry.1 = chrono::Utc::now().timestamp();
                    if entry.0 >= 3 {
                        warn!(slot_id = %slot_id, failures = entry.0, "Slot throttled for 30 min due to consecutive failures");
                    }
                }

                // Retry logic: increment count, mark failed if exhausted
                let new_retry = task.retry_count + 1;
                if new_retry >= task.max_retries {
                    let _ = state.mission.db().update_board_task(
                        &task.id,
                        &missiond_core::types::UpdateBoardTaskInput {
                            status: Some("failed".to_string()),
                            ..Default::default()
                        },
                    );
                    warn!(task_id = %task.id, retries = new_retry, "Autopilot: task failed after max retries");
                } else {
                    // Back to open for retry, increment retry_count
                    let _ = state.mission.db().increment_board_task_retry(&task.id, new_retry);
                    warn!(task_id = %task.id, retry = new_retry, max = task.max_retries, error = %e, "Autopilot: task failed, will retry");
                }
            }
        }
    }

    Ok(())
}

const MEMORY_SLOT_ID: &str = "slot-memory";           // Fast lane (realtime)
const MEMORY_SLOW_SLOT_ID: &str = "slot-memory-slow";  // Slow lane (deep + consolidation)

/// Detect if a new unknown session is a compacted replacement for an active slot session.
///
/// When Claude Code runs out of context, it compacts into a new session (new JSONL file).
/// The old session stops being written to, but the PTY process continues.
/// We detect this by checking if any active slot has a session in the same project directory.
///
/// Returns (slot_id, old_session_id, old_task_id) if compaction is detected.
fn detect_compaction(
    state: &AppState,
    new_session_id: &str,
    new_project: &str,
) -> Option<(String, String, Option<String>)> {
    let db = state.mission.db();
    let all_slot_sessions = db.get_all_slot_sessions().ok()?;

    for (slot_id, old_uuid) in &all_slot_sessions {
        if old_uuid == new_session_id {
            continue; // Same session, not compaction
        }
        let old_conv = db.get_conversation(old_uuid).ok()??;
        // Must be same project and still active
        if old_conv.project.as_deref() != Some(new_project) || old_conv.status != "active" {
            continue;
        }
        // The old session should have been written to recently (within 10 min)
        // to avoid false positives with stale slot sessions
        if let Some(ref started) = Some(&old_conv.started_at) {
            if let Ok(start_time) = chrono::DateTime::parse_from_rfc3339(started) {
                let age = chrono::Utc::now().signed_duration_since(start_time);
                if age > chrono::Duration::hours(2) {
                    continue; // Too old to be a live compaction
                }
            }
        }
        return Some((
            slot_id.clone(),
            old_uuid.clone(),
            old_conv.task_id.clone(),
        ));
    }
    None
}

/// LLM API configuration for Router / KB Analyze calls.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LlmConfig {
    base_url: String,
    #[serde(default)]
    auth: LlmAuth,
    #[serde(default = "LlmConfig::default_model")]
    default_model: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
enum LlmAuth {
    /// Read bearer token from an environment variable.
    #[serde(rename = "bearer_env")]
    BearerEnv { env: String },
    /// Read bearer token from a JSON file (extract a specific key).
    #[serde(rename = "bearer_file")]
    BearerFile { path: String, key: String },
    /// No authentication.
    #[serde(rename = "none")]
    None,
}

impl Default for LlmAuth {
    fn default() -> Self {
        LlmAuth::None
    }
}

impl LlmConfig {
    fn default_model() -> String {
        "gpt-4o".to_string()
    }
}

/// Resolve LLM credentials (base_url, bearer_token) for Router API calls.
///
/// Resolution order:
/// 1. `$MISSIOND_HOME/llm.yaml` — explicit config (preferred)
/// 2. `~/.xjp/credentials.json` — legacy fallback (jwt_token + auth_url)
///
/// Returns (base_url, bearer_token).
async fn resolve_llm_credentials() -> Result<(String, String)> {
    // 1. Try llm.yaml in mission home
    let llm_yaml = default_mission_home().join("llm.yaml");
    if llm_yaml.exists() {
        let content = tokio::fs::read_to_string(&llm_yaml).await
            .map_err(|e| anyhow!("Failed to read {}: {}", llm_yaml.display(), e))?;
        let config: LlmConfig = serde_yaml::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse {}: {}", llm_yaml.display(), e))?;

        let token = match &config.auth {
            LlmAuth::BearerEnv { env } => {
                std::env::var(env)
                    .map_err(|_| anyhow!("Env var '{}' not set (required by llm.yaml)", env))?
            }
            LlmAuth::BearerFile { path, key } => {
                let expanded = if path.starts_with("~/") {
                    dirs::home_dir()
                        .ok_or_else(|| anyhow!("Cannot determine home directory"))?
                        .join(&path[2..])
                } else {
                    PathBuf::from(path)
                };
                let file_content = tokio::fs::read_to_string(&expanded).await
                    .map_err(|e| anyhow!("Failed to read {}: {}", expanded.display(), e))?;
                let json: serde_json::Value = serde_json::from_str(&file_content)
                    .map_err(|e| anyhow!("Failed to parse {}: {}", expanded.display(), e))?;
                json.get(key)
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Key '{}' not found in {}", key, expanded.display()))?
                    .to_string()
            }
            LlmAuth::None => String::new(),
        };

        info!(base_url = %config.base_url, "LLM credentials resolved from llm.yaml");
        return Ok((config.base_url, token));
    }

    // 2. Legacy fallback: ~/.xjp/credentials.json
    let cred_path = dirs::home_dir()
        .ok_or_else(|| anyhow!("Cannot determine home directory"))?
        .join(".xjp")
        .join("credentials.json");

    if cred_path.exists() {
        let cred_content = tokio::fs::read_to_string(&cred_path).await
            .map_err(|e| anyhow!("Failed to read credentials: {}", e))?;
        let creds: serde_json::Value = serde_json::from_str(&cred_content)
            .map_err(|e| anyhow!("Failed to parse credentials: {}", e))?;
        let jwt = creds.get("jwt_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("No jwt_token in credentials.json. Configure LLM credentials first."))?;
        let base_url = creds.get("auth_url")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if base_url.is_empty() {
            return Err(anyhow!("No auth_url in credentials.json. Configure LLM credentials in llm.yaml or credentials.json."));
        }

        info!("LLM credentials resolved from legacy credentials.json");
        return Ok((base_url.to_string(), jwt.to_string()));
    }

    Err(anyhow!("No LLM credentials found. Create llm.yaml in mission home or ~/.xjp/credentials.json."))
}

/// Build env vars for PTY spawn that enable UUID capture via SessionStart hook.
/// Merges slot-level custom env (from slots.yaml `env:` field) with tracking vars.
/// Supports dynamic references: `${env:VAR}`, `${file:path}`, `${cmd:...}`, `${secret:path}`.
/// Returns (extra_env, session_file_path).
async fn build_slot_tracking_env(slot_id: &str, slot_env: Option<&HashMap<String, String>>) -> (HashMap<String, String>, PathBuf) {
    let session_file = std::env::temp_dir().join(format!("missiond-session-{}.txt", slot_id));
    // Remove stale file from previous spawn
    let _ = std::fs::remove_file(&session_file);

    let mut extra_env = HashMap::new();

    // 1. Merge slot-level custom env (model provider config, etc.)
    if let Some(env_map) = slot_env {
        info!(slot_id, env_count = env_map.len(), "Injecting slot-level custom env");
        for (key, value) in env_map {
            let resolved = resolve_env_value(value).await;
            let is_sensitive = value.starts_with("${secret:") || value.starts_with("${cmd:");
            if is_sensitive {
                info!(slot_id, key, "Resolved sensitive env var");
            } else {
                info!(slot_id, key, %resolved, "Set env var");
            }
            extra_env.insert(key.clone(), resolved);
        }
    } else {
        debug!(slot_id, "No custom env for slot");
    }

    // 2. Add tracking vars (always override custom env to prevent collision)
    extra_env.insert("MISSIOND_SLOT_ID".to_string(), slot_id.to_string());
    extra_env.insert(
        "MISSIOND_SESSION_FILE".to_string(),
        session_file.to_string_lossy().to_string(),
    );

    (extra_env, session_file)
}

/// Resolve dynamic references in env values.
///
/// Supported providers:
/// - `${env:VAR}` — read from environment variable
/// - `${file:path}` — read file contents (trimmed)
/// - `${cmd:program args...}` — execute command, use stdout
/// - `${secret:path}` — backward compat, translates to `${cmd:xjp secret get --raw path}`
/// - bare string — returned as-is (plaintext fallback)
///
/// Falls back to the raw value on any error.
async fn resolve_env_value(value: &str) -> String {
    // Must match ${provider:content} pattern
    if !value.starts_with("${") || !value.ends_with('}') {
        return value.to_string();
    }

    let inner = &value[2..value.len() - 1]; // strip ${ and }
    let Some((provider, content)) = inner.split_once(':') else {
        return value.to_string();
    };

    match provider {
        "env" => {
            match std::env::var(content) {
                Ok(val) => {
                    info!(var = content, "Resolved env var");
                    val
                }
                Err(_) => {
                    warn!(var = content, "Env var not set, using raw value");
                    value.to_string()
                }
            }
        }
        "file" => {
            match tokio::fs::read_to_string(content).await {
                Ok(val) => {
                    let trimmed = val.trim().to_string();
                    info!(path = content, "Resolved file value");
                    trimmed
                }
                Err(e) => {
                    warn!(path = content, error = %e, "Failed to read file, using raw value");
                    value.to_string()
                }
            }
        }
        "cmd" => {
            resolve_cmd_value(value, content).await
        }
        "secret" => {
            // Backward compat: translate to xjp secret get --raw
            let cmd_str = format!("xjp secret get --raw {}", content);
            resolve_cmd_value(value, &cmd_str).await
        }
        _ => {
            warn!(provider, "Unknown env value provider, using raw value");
            value.to_string()
        }
    }
}

/// Execute a command string and return its stdout. Helper for ${cmd:} and ${secret:}.
async fn resolve_cmd_value(raw_value: &str, cmd_str: &str) -> String {
    let parts: Vec<&str> = cmd_str.split_whitespace().collect();
    if parts.is_empty() {
        warn!("Empty command in env value, using raw value");
        return raw_value.to_string();
    }

    let program = parts[0];
    let args = &parts[1..];

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(program)
            .args(args)
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if out.is_empty() {
                warn!(cmd = cmd_str, "Command returned empty, using raw value");
                raw_value.to_string()
            } else {
                info!(cmd = program, "Resolved command value for slot env");
                out
            }
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(cmd = cmd_str, %stderr, "Command failed, using raw value");
            raw_value.to_string()
        }
        Ok(Err(e)) => {
            warn!(cmd = cmd_str, error = %e, "Failed to run command, using raw value");
            raw_value.to_string()
        }
        Err(_) => {
            warn!(cmd = cmd_str, "Command timed out (10s), using raw value");
            raw_value.to_string()
        }
    }
}

/// After a PTY spawn with wait_for_idle, read the session UUID
/// written by the SessionStart hook and register it in DB + cache.
async fn capture_slot_session_uuid(
    state: &AppState,
    slot_id: &str,
    session_file: &Path,
) {
    let mut uuid = None;

    // The hook writes during Claude's SessionStart, which happens
    // before idle. So by the time wait_for_idle returns, the file
    // should exist. Retry as safety net.
    for attempt in 0..5u32 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        match tokio::fs::read_to_string(session_file).await {
            Ok(content) => {
                let trimmed = content.trim().to_string();
                if !trimmed.is_empty() {
                    uuid = Some(trimmed);
                    break;
                }
            }
            Err(_) => continue,
        }
    }

    // Clean up temp file
    let _ = tokio::fs::remove_file(session_file).await;

    match uuid {
        Some(session_uuid) => {
            info!(
                slot_id = %slot_id,
                session_uuid = %session_uuid,
                "Captured PTY session UUID via hook"
            );

            // Persist in DB (activates the previously-orphaned slot_sessions table)
            if let Err(e) = state.mission.db().set_slot_session(slot_id, &session_uuid) {
                warn!(slot_id = %slot_id, error = %e, "Failed to persist slot session mapping");
            }

            // Update in-memory cache
            state.pty_session_uuids.write().await.insert(session_uuid.clone());

            // Retroactive fix: if conversation already exists with slot_id=None, tag it
            if let Ok(Some(conv)) = state.mission.db().get_conversation(&session_uuid) {
                if conv.slot_id.is_none() {
                    let mut updated = conv;
                    updated.slot_id = Some(slot_id.to_string());
                    updated.source = "pty_jsonl".to_string();
                    updated.conversation_type = missiond_core::db::derive_conversation_type(
                        Some(slot_id), &session_uuid
                    );
                    let _ = state.mission.db().upsert_conversation(&updated);
                    info!(session = %session_uuid, slot_id = %slot_id, "Retroactively tagged conversation with slot_id and conversation_type");
                }
            }
        }
        None => {
            warn!(
                slot_id = %slot_id,
                "Failed to capture session UUID - hook may not be installed"
            );
        }
    }
}

/// Ensure a memory-role PTY session is running, spawn if needed.
/// Returns true if the session is available.
async fn ensure_memory_slot_by_id(state: &AppState, slot_id: &str) -> bool {
    // Check if session is actually running (not just initialized/exited)
    if let Some(info) = state.pty.get_status(slot_id).await {
        if info.state != SessionState::Exited {
            return true;
        }
    }
    let slot = state
        .mission
        .list_slots()
        .into_iter()
        .find(|s| s.config.id == slot_id);
    let Some(slot) = slot else {
        warn!(slot_id, "Memory slot not configured in slots.yaml");
        return false;
    };
    let pty_slot = missiond_core::PTYSlot {
        id: slot.config.id.clone(),
        role: slot.config.role.clone(),
        cwd: slot.config.cwd.as_deref().map(PathBuf::from),
    };
    let slot_env = slot.config.env.as_ref();
    let mcp_config = slot.config.mcp_config.map(PathBuf::from);
    let (extra_env, session_file) = build_slot_tracking_env(slot_id, slot_env).await;
    match state.pty.spawn(&pty_slot, PTYSpawnOptions {
        auto_restart: true,
        wait_for_idle: true,
        timeout_secs: Some(120),
        mcp_config,
        dangerously_skip_permissions: slot.config.dangerously_skip_permissions.unwrap_or(false),
        extra_env,
    }).await {
        Ok(_) => {
            capture_slot_session_uuid(state, slot_id, &session_file).await;
            info!(slot_id, "Memory slot spawned (auto_restart=true)");
            true
        }
        Err(e) => {
            warn!(slot_id, error = %e, "Failed to spawn memory slot");
            false
        }
    }
}

/// Convenience wrapper for fast-lane memory slot.
async fn ensure_memory_slot(state: &AppState) -> bool {
    ensure_memory_slot_by_id(state, MEMORY_SLOT_ID).await
}

/// Unified priority scheduler for memory slots.
/// Enforces strict priority: Submit Tasks > Realtime Extraction > Deep Analysis > KB Consolidation.
/// Called from autopilot_tick (60s fallback) and event-driven paths (immediate).
async fn schedule_memory_tasks(state: &AppState) {
    // P1: Submit tasks — highest priority, dispatch to any idle memory slot
    dispatch_queued_submit_tasks(state).await;

    // P2: Fast lane — realtime extraction on slot-memory
    // (only runs if slot-memory wasn't grabbed by P1)
    check_realtime_extraction(state).await;

    // P3: Slow lane — deep analysis + consolidation on slot-memory-slow
    // (independent of fast lane, only blocked if P1 grabbed slot-memory-slow)
    check_deep_analysis(state).await;
    check_kb_consolidation(state).await;
}

/// Dispatch queued tasks from the `tasks` table (created by mission_submit).
/// Returns true if at least one task was dispatched.
/// Part of the unified priority scheduler — called before extraction checks.
async fn dispatch_queued_submit_tasks(state: &AppState) -> bool {
    let queued = match state.mission.db().get_tasks_by_status(missiond_core::types::TaskStatus::Queued) {
        Ok(tasks) => tasks,
        Err(e) => {
            warn!(error = %e, "Failed to query queued submit tasks");
            return false;
        }
    };

    if queued.is_empty() {
        return false;
    }

    info!(count = queued.len(), "Autopilot: found queued submit tasks");

    let mut any_dispatched = false;
    // Track slots used in this dispatch round to avoid sending multiple tasks to the same slot
    let mut used_slots: HashSet<String> = HashSet::new();

    for task in &queued {
        let slots = state.mission.list_slots();
        let mut dispatched = false;

        let candidates: Vec<String> = if let Some(ref target) = task.slot_id {
            vec![target.clone()]
        } else {
            slots.iter()
                .filter(|s| s.config.role == task.role)
                .map(|s| s.config.id.clone())
                .collect()
        };

        // Phase 1: Try idle slots (skip slots already used in this round)
        for slot_id in &candidates {
            if used_slots.contains(slot_id) {
                continue;
            }
            let status = match state.pty.get_status(slot_id).await {
                Some(s) => s,
                None => continue,
            };
            if status.state != missiond_core::pty::SessionState::Idle {
                continue;
            }

            match state.pty.send_fire_and_forget(slot_id, &task.prompt).await {
                Ok(()) => {
                    let now = chrono::Utc::now().timestamp_millis();
                    let _ = state.mission.db().update_task(
                        &task.id,
                        &missiond_core::types::TaskUpdate {
                            status: Some(missiond_core::types::TaskStatus::Running),
                            slot_id: Some(slot_id.clone()),
                            started_at: Some(now),
                            ..Default::default()
                        },
                    );
                    info!(task_id = %task.id, slot_id = %slot_id, role = %task.role, "Autopilot: dispatched queued submit task");
                    used_slots.insert(slot_id.clone());
                    dispatched = true;
                    any_dispatched = true;
                    break;
                }
                Err(e) => {
                    warn!(task_id = %task.id, slot_id = %slot_id, error = %e, "Autopilot: dispatch to slot failed");
                }
            }
        }

        if !dispatched {
            debug!(task_id = %task.id, role = %task.role, "Autopilot: no idle slot for queued task");
        }
    }

    // Phase 2: Wake-on-Demand — auto-spawn stopped slots for roles that still have queued tasks
    {
        let remaining = match state.mission.db().get_tasks_by_status(missiond_core::types::TaskStatus::Queued) {
            Ok(t) => t,
            Err(_) => vec![],
        };
        let remaining_roles: HashSet<String> = remaining.iter().map(|t| t.role.clone()).collect();
        if !remaining_roles.is_empty() {
            let slots = state.mission.list_slots();
            for role in &remaining_roles {
                for slot in slots.iter().filter(|s| s.config.role == *role) {
                    let slot_id = &slot.config.id;
                    if used_slots.contains(slot_id) { continue; }
                    let status = state.pty.get_status(slot_id).await;
                    let is_spawnable = match &status {
                        Some(s) => s.state == missiond_core::pty::SessionState::Exited,
                        None => true,
                    };
                    if !is_spawnable { continue; }

                    let state_clone = state.clone();
                    let slot_id_clone = slot_id.clone();
                    info!(slot_id = %slot_id, role = %role, "Autopilot: auto-spawning slot for queued tasks (Wake-on-Demand)");
                    tokio::spawn(async move {
                        if ensure_memory_slot_by_id(&state_clone, &slot_id_clone).await {
                            state_clone.submit_notify.notify_one();
                        }
                    });
                    break; // One spawn attempt per role
                }
            }
        }
    }

    any_dispatched
}

/// Reap submit tasks stuck in Running state for too long (15 min).
/// If the slot is Idle, mark Done; otherwise mark Failed after timeout.
async fn reap_stale_submit_tasks(state: &AppState) {
    let running = match state.mission.db().get_tasks_by_status(missiond_core::types::TaskStatus::Running) {
        Ok(t) => t,
        Err(_) => return,
    };
    if running.is_empty() {
        return;
    }

    let now = chrono::Utc::now().timestamp_millis();
    const JSONL_CHECK_THRESHOLD_MS: i64 = 2 * 60 * 1000; // 2 minutes: start checking JSONL
    const SUBMIT_TASK_TIMEOUT_MS: i64 = 15 * 60 * 1000; // 15 minutes: hard timeout

    for task in &running {
        let started = task.started_at.unwrap_or(task.created_at);
        let elapsed = now - started;

        if elapsed < JSONL_CHECK_THRESHOLD_MS {
            continue;
        }

        // --- JSONL completion detection (compensating path for missed PTY Idle events) ---
        if elapsed < SUBMIT_TASK_TIMEOUT_MS {
            if let Some(ref slot_id) = task.slot_id {
                // Guard 1: slot's current session must match task's session
                let session_matches = match (
                    state.mission.db().get_slot_session(slot_id),
                    &task.session_id,
                ) {
                    (Ok(Some(current_session)), Some(task_session)) => current_session == *task_session,
                    // No session tracking → can't verify, skip JSONL check
                    _ => false,
                };

                if session_matches {
                    if let Some(jsonl_path) = get_task_jsonl_path(state, task) {
                        let path = std::path::Path::new(&jsonl_path);
                        if missiond_core::jsonl_has_completed_turn(path).await {
                            // JSONL confirms turn completed — extract result and close
                            let result_text = missiond_core::extract_last_assistant_text(path).await
                                .unwrap_or_else(|| "completed (JSONL turn_duration)".to_string());
                            // Safe UTF-8 truncation to 4KB
                            let result_text = if result_text.len() > 4096 {
                                let mut end = 4096;
                                while !result_text.is_char_boundary(end) && end > 0 { end -= 1; }
                                format!("{}...(truncated)", &result_text[..end])
                            } else {
                                result_text
                            };
                            let _ = state.mission.db().update_task(
                                &task.id,
                                &missiond_core::types::TaskUpdate {
                                    status: Some(missiond_core::types::TaskStatus::Done),
                                    finished_at: Some(now),
                                    result: Some(result_text.clone()),
                                    ..Default::default()
                                },
                            );
                            // Update associated kb_operation
                            let _ = state.mission.db().kb_ops_complete_by_task_id(&task.id, "done", Some(&result_text));
                            info!(
                                task_id = %task.id, slot_id = %slot_id,
                                age_min = elapsed / 60000,
                                "Submit task closed via JSONL turn_duration compensation"
                            );
                            continue;
                        }
                    }
                }
            }
            // Not yet at hard timeout and JSONL didn't confirm — wait
            continue;
        }

        // --- Hard timeout (15 min) ---
        let slot_idle = if let Some(ref sid) = task.slot_id {
            state.pty.get_status(sid).await
                .map(|s| s.state == missiond_core::pty::SessionState::Idle)
                .unwrap_or(true) // no session = treat as idle
        } else {
            true
        };

        let (new_status, result_msg) = if slot_idle {
            // Try JSONL result even at timeout
            let jsonl_result = if task.slot_id.is_some() {
                if let Some(jsonl_path) = get_task_jsonl_path(state, task) {
                    missiond_core::extract_last_assistant_text(std::path::Path::new(&jsonl_path)).await
                } else { None }
            } else { None };
            (missiond_core::types::TaskStatus::Done,
             jsonl_result.unwrap_or_else(|| "completed (timeout reaper)".to_string()))
        } else {
            (missiond_core::types::TaskStatus::Failed, "timed out after 15 minutes".to_string())
        };

        let _ = state.mission.db().update_task(
            &task.id,
            &missiond_core::types::TaskUpdate {
                status: Some(new_status),
                finished_at: Some(now),
                result: Some(result_msg.clone()),
                ..Default::default()
            },
        );
        // Update associated kb_operation (done or failed depending on task status)
        let kb_status = if new_status == missiond_core::types::TaskStatus::Done { "done" } else { "failed" };
        if let Ok(true) = state.mission.db().kb_ops_complete_by_task_id(&task.id, kb_status, Some(&result_msg)) {
            info!(task_id = %task.id, kb_status = kb_status, "KB operation updated via reaper");
        }
        warn!(
            task_id = %task.id,
            slot_id = ?task.slot_id,
            status = ?new_status,
            age_min = elapsed / 60000,
            "Reaped stale submit task"
        );
    }
}

/// Get the JSONL path for a task's slot session
fn get_task_jsonl_path(state: &AppState, task: &missiond_core::types::Task) -> Option<String> {
    let slot_id = task.slot_id.as_ref()?;
    let session_uuid = state.mission.db().get_slot_session(slot_id).ok()??;
    let conv = state.mission.db().get_conversation(&session_uuid).ok()??;
    conv.jsonl_path
}

/// Threshold for considering the memory slot stuck (10 minutes).
const STUCK_THRESHOLD_SECS: i64 = 600;

/// Detect and recover from a memory slot stuck in non-Idle state.
async fn check_slot_stuck(
    state: &AppState,
    slot_id: &str,
    busy_since_atomic: &std::sync::atomic::AtomicI64,
    extraction_state: &tokio::sync::RwLock<ExtractionState>,
) {
    let now = chrono::Utc::now().timestamp();
    let busy_since = busy_since_atomic.load(std::sync::atomic::Ordering::SeqCst);

    // Poll actual slot state: if slot is non-Idle but busy_since is 0 (lost track),
    // re-initialize busy_since so stuck detection can work.
    if busy_since == 0 {
        let is_busy = state.pty.get_status(slot_id).await
            .map(|s| s.state != SessionState::Idle)
            .unwrap_or(false);
        if is_busy {
            busy_since_atomic.store(now, std::sync::atomic::Ordering::SeqCst);
            debug!(slot_id, "slot_stuck: re-initialized busy_since (slot is non-Idle but was untracked)");
        }
        return;
    }

    let stuck_duration = now - busy_since;
    if stuck_duration < STUCK_THRESHOLD_SECS {
        return;
    }

    let status = state.pty.get_status(slot_id).await;
    let current_state = status.as_ref()
        .map(|s| format!("{:?}", s.state))
        .unwrap_or_else(|| "unknown".to_string());

    // If slot is actually Idle, just clear the counter
    if status.as_ref().map(|s| s.state == SessionState::Idle).unwrap_or(false) {
        busy_since_atomic.store(0, std::sync::atomic::Ordering::SeqCst);
        return;
    }

    // P2: Check JSONL activity before killing — slot may be doing bulk MCP tool_use
    // which shows as Thinking the entire time (especially with MiniMax M2.5)
    if let Ok(Some(session_uuid)) = state.mission.db().get_slot_session(slot_id) {
        if let Ok(Some(conv)) = state.mission.db().get_conversation(&session_uuid) {
            if let Some(ref jsonl_path) = conv.jsonl_path {
                if let Ok(metadata) = std::fs::metadata(jsonl_path) {
                    if let Ok(modified) = metadata.modified() {
                        let elapsed = modified.elapsed().unwrap_or_default();
                        if elapsed.as_secs() < 120 {
                            info!(
                                slot_id,
                                stuck_secs = stuck_duration,
                                jsonl_activity_secs_ago = elapsed.as_secs(),
                                "Slot appears stuck but JSONL is active, deferring kill"
                            );
                            return;
                        }
                    }
                }
            }
        }
    }

    warn!(
        slot_id,
        state = %current_state,
        stuck_secs = stuck_duration,
        "Memory slot stuck, killing and respawning"
    );

    // Kill and respawn instead of just Ctrl+C (Ctrl+C often doesn't recover)
    if let Err(e) = state.pty.kill(slot_id).await {
        warn!(slot_id, error = %e, "Failed to kill stuck memory slot");
    }

    // Requeue any Running submit tasks assigned to this slot — they were lost with the old session
    match state.mission.db().requeue_running_tasks_for_slot(slot_id) {
        Ok(n) if n > 0 => warn!(slot_id, count = n, "Requeued running submit tasks after slot restart"),
        Err(e) => warn!(slot_id, error = %e, "Failed to requeue tasks after slot restart"),
        _ => {}
    }
    // Release board task claims held by this slot
    let _ = state.mission.db().release_board_claims_by_executor(slot_id);

    // Allow next tick to respawn
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let _ = ensure_memory_slot_by_id(state, slot_id).await;

    // Reset extraction state so next tick can trigger
    {
        let mut es = extraction_state.write().await;
        // Mark deep analysis as failed so it increments retry count (not silently lost)
        if let Some(conv_id) = es.current_deep_conv_id.take() {
            let _ = state.mission.db().mark_analysis_failed(&conv_id);
            warn!(conv_id = %conv_id, "Marked stuck deep analysis as failed");
        }
        // Mark slot task as failed (stuck)
        if let Some(ref st_id) = es.current_slot_task_id {
            let _ = state.mission.db().slot_task_set_failed(st_id, "slot stuck, force reset");
        }
        es.phase = ExtractionPhase::Idle;
        es.active_type = None;
        es.current_task_id = None;
        es.current_slot_task_id = None;
        es.is_checkpoint = false;
        es.checkpoint_message_id = None;
        // Advance realtime watermarks before clearing — same fix as check_extraction_gate.
        if matches!(es.active_type, Some("realtime")) && !es.watermark_targets.is_empty() {
            let db = state.mission.db();
            for (session_id, timestamp) in &es.watermark_targets {
                let _ = db.update_realtime_forwarded_at(session_id, timestamp);
            }
            warn!(
                slot_id,
                sessions = es.watermark_targets.len(),
                "Advanced watermarks before slot kill (preventing stall)"
            );
        }
        es.watermark_targets.clear();
    }
    // Don't reset to 0 — set to now so we can detect if respawn also gets stuck
    busy_since_atomic.store(now, std::sync::atomic::Ordering::SeqCst);
}

/// Check extraction state gate: returns true if extraction is allowed.
/// Handles safety-valve timeout for WaitingForSlotIdle phase.
async fn check_extraction_gate(
    extraction_state: &tokio::sync::RwLock<ExtractionState>,
    mission: &MissionControl,
    label: &str,
) -> bool {
    let now = chrono::Utc::now().timestamp();
    let es = extraction_state.read().await;
    if es.phase == ExtractionPhase::Idle {
        return true;
    }
    let age = now - es.phase_started_at;
    // Safety valve: force reset if WaitingForSlotIdle exceeds MAX_WAIT_FOR_IDLE_SECS
    if es.phase == ExtractionPhase::WaitingForSlotIdle && age > MAX_WAIT_FOR_IDLE_SECS {
        drop(es);
        let mut es = extraction_state.write().await;
        if es.phase == ExtractionPhase::WaitingForSlotIdle {
            warn!(age_secs = age, "{}: extraction stuck in WaitingForSlotIdle, forcing reset", label);
            // Mark deep analysis as failed (increments retry count) instead of silently losing
            if let Some(conv_id) = es.current_deep_conv_id.take() {
                let _ = mission.db().mark_analysis_failed(&conv_id);
            }
            // Mark slot task as failed (timeout)
            if let Some(ref st_id) = es.current_slot_task_id {
                let _ = mission.db().slot_task_set_failed(st_id, "WaitingForSlotIdle timeout");
            }
            es.phase = ExtractionPhase::Idle;
            es.active_type = None;
            es.current_task_id = None;
            es.current_slot_task_id = None;
            es.is_checkpoint = false;
            es.checkpoint_message_id = None;
            // Advance realtime watermarks before clearing — prevents permanent
            // watermark stall. Messages were already sent to slot; even if
            // processing was incomplete, not advancing causes 100% message leak.
            if matches!(es.active_type, Some("realtime")) && !es.watermark_targets.is_empty() {
                let db = mission.db();
                for (session_id, timestamp) in &es.watermark_targets {
                    let _ = db.update_realtime_forwarded_at(session_id, timestamp);
                }
                warn!(
                    sessions = es.watermark_targets.len(),
                    "{}: advanced watermarks on timeout (preventing stall)", label
                );
            }
            es.watermark_targets.clear();
            return true;
        }
    } else {
        debug!(
            phase = ?es.phase,
            active_type = ?es.active_type,
            age_secs = age,
            "{}: skipped (extraction in progress)", label
        );
    }
    false
}

/// Unified realtime extraction on fast lane (slot-memory).
async fn check_realtime_extraction(state: &AppState) {
    if !check_extraction_gate(&state.extraction_state, &state.mission, "realtime").await {
        return;
    }

    // Priority enforcement: unified scheduler guarantees submit tasks run before this.
    // Skip if slot-memory is occupied by a running submit task.
    if let Ok(running) = state.mission.db().get_tasks_by_status(missiond_core::types::TaskStatus::Running) {
        if running.iter().any(|t| t.slot_id.as_deref() == Some(MEMORY_SLOT_ID)) {
            debug!("realtime: skipping, running submit task on memory slot");
            return;
        }
    }

    // Watermark-based check: any conversations with messages beyond their realtime_forwarded_at?
    let raw_pending = match state.mission.db().get_pending_realtime_messages() {
        Ok(p) if !p.is_empty() => p,
        Ok(_) => {
            debug!("realtime: no pending messages (watermark)");
            return;
        }
        Err(_) => return,
    };

    // Triage: skip sessions with zero user messages, auto-advance their watermarks
    let db = state.mission.db();
    let mut pending = Vec::new();
    for (session_id, project, msgs) in raw_pending {
        if msgs.iter().any(|m| m.role == "user") {
            pending.push((session_id, project, msgs));
        } else {
            if let Some(last) = msgs.last() {
                let _ = db.update_realtime_forwarded_at(&session_id, &last.timestamp);
            }
        }
    }
    if pending.is_empty() {
        debug!("realtime: all sessions filtered (no user messages)");
        return;
    }

    // Capture watermark targets: (session_id, max_timestamp) for advancing on completion
    let watermark_targets: Vec<(String, String)> = pending
        .iter()
        .filter_map(|(session_id, _, msgs)| {
            msgs.last().map(|m| (session_id.clone(), m.timestamp.clone()))
        })
        .collect();

    let session_count = pending.len();
    let msg_count: usize = pending.iter().map(|(_, _, msgs)| msgs.len()).sum();

    // Ensure memory slot is spawned, then check it's idle
    if !ensure_memory_slot(state).await {
        debug!("realtime: memory slot not available");
        return;
    }
    let status = state.pty.get_status(MEMORY_SLOT_ID).await;
    match status {
        Some(s) if s.state == SessionState::Idle => {}
        Some(s) => {
            debug!(state = ?s.state, "realtime: slot not idle");
            return;
        }
        None => {
            debug!("realtime: slot status unavailable");
            return;
        }
    }

    info!(sessions = session_count, messages = msg_count, "Realtime extraction: locking batch (watermark)");

    // Generate task_id and tag the memory slot's current session
    let task_id = uuid::Uuid::new_v4().to_string();
    if let Ok(Some(current_session)) = state.mission.db().get_slot_session(MEMORY_SLOT_ID) {
        let _ = state.mission.db().set_conversation_task_id(&current_session, &task_id);
    }

    // Record slot task for history tracking
    let source_session_ids: Vec<String> = pending.iter().map(|(sid, _, _)| sid.clone()).collect();
    let slot_task_id = uuid::Uuid::new_v4().to_string();
    let slot_task = missiond_core::types::SlotTask {
        id: slot_task_id.clone(),
        slot_id: MEMORY_SLOT_ID.to_string(),
        task_type: "realtime_extract".to_string(),
        status: "pending".to_string(),
        prompt_summary: Some(format!("{} 个会话, {} 条消息", session_count, msg_count)),
        source_sessions: Some(serde_json::to_string(&source_session_ids).unwrap_or_default()),
        output_count: 0,
        created_at: chrono::Utc::now().to_rfc3339(),
        started_at: None,
        completed_at: None,
        duration_ms: None,
        error: None,
        conversation_id: None,
    };
    let _ = state.mission.db().insert_slot_task(&slot_task);

    // Store watermark targets for advancing on completion
    {
        let now = chrono::Utc::now().timestamp();
        let mut es = state.extraction_state.write().await;
        es.phase = ExtractionPhase::Sending;
        es.active_type = Some("realtime");
        es.phase_started_at = now;
        es.watermark_targets = watermark_targets;
        es.current_task_id = Some(task_id);
        es.current_slot_task_id = Some(slot_task_id.clone());
    }

    let prompt = "有新的对话内容待分析。调用 mission_memory_pending 获取待分析内容，提取知识存入 KB。\n\n\
         ⚠️ 作业边界:\n\
         - 只处理 mission_memory_pending 返回的内容\n\
         - 禁止调用 mission_conversation_search / mission_conversation_get\n\
         - 跨会话分析是 deep-analysis 的职责，不在此处执行\n\n\
         提取目标（按优先级）:\n\
         - 用户偏好/原则/纠正 → category: preference\n\
         - 架构决策/技术事实 → category: memory 或 memory:architecture/memory:decision\n\
         - 已修 bug 根因 → category: memory:bugfix\n\
         - 运维痛点信号 → category: memory:ops\n\
         - 调试弯路经验 → category: memory:debug\n\
         禁止提取: 基础设施信息/API细节/版本号/通用技术知识/当天日志\n\
         去重: 提取前 mission_kb_search 检查。\n\
         问题上报: 发现 bug → mission_board_create。";

    info!("Triggering realtime extraction via MCP pull");

    let pty = Arc::clone(&state.pty);
    let extraction_state = Arc::clone(&state.extraction_state);
    let mission = Arc::clone(&state.mission);
    let slot_task_id_clone = slot_task_id;
    tokio::spawn(async move {
        match pty.send(MEMORY_SLOT_ID, prompt, 300_000).await {
            Ok(res) => {
                info!(duration_ms = res.duration_ms, "realtime extraction send() returned");
                // send() blocks until slot finishes and returns to Idle.
                // Complete extraction directly — don't enter WaitingForSlotIdle
                // (race condition: Idle transition may have already fired and been
                // ignored due to phase_age < 3s guard).
                let mut es = extraction_state.write().await;
                if es.phase == ExtractionPhase::Sending || es.phase == ExtractionPhase::WaitingForSlotIdle {
                    // Advance watermarks for processed sessions
                    if !es.watermark_targets.is_empty() {
                        let db = mission.db();
                        for (session_id, timestamp) in &es.watermark_targets {
                            let _ = db.update_realtime_forwarded_at(session_id, timestamp);
                        }
                        info!(sessions = es.watermark_targets.len(), "Realtime: advanced watermarks (send-complete)");
                        es.watermark_targets.clear();
                    }
                    // Mark slot task completed
                    let _ = mission.db().slot_task_set_completed(&slot_task_id_clone, 0);
                    info!(duration_ms = res.duration_ms, "Realtime extraction complete (send-path)");
                    es.phase = ExtractionPhase::Idle;
                    es.active_type = None;
                    es.current_task_id = None;
                    es.current_slot_task_id = None;
                    es.is_checkpoint = false;
                    es.checkpoint_message_id = None;
                }
            }
            Err(e) => {
                warn!(error = %e, "realtime extraction trigger failed");
                let _ = mission.db().slot_task_set_failed(&slot_task_id_clone, &e.to_string());
                let mut es = extraction_state.write().await;
                es.phase = ExtractionPhase::Idle;
                es.active_type = None;
                es.current_task_id = None;
                es.current_slot_task_id = None;
                es.is_checkpoint = false;
                es.checkpoint_message_id = None;
                es.watermark_targets.clear();
            }
        }
    });
}

/// Deep analysis on slow lane (slot-memory-slow).
/// Reviews completed conversations using conversation-level watermark.
async fn check_deep_analysis(state: &AppState) {
    if !check_extraction_gate(&state.slow_extraction_state, &state.mission, "deep_analysis").await {
        return;
    }

    // Priority enforcement: unified scheduler guarantees submit tasks run before this.
    // Skip if slow slot is occupied by a running submit task.
    if let Ok(running) = state.mission.db().get_tasks_by_status(missiond_core::types::TaskStatus::Running) {
        if running.iter().any(|t| t.slot_id.as_deref() == Some(MEMORY_SLOW_SLOT_ID)) {
            debug!("deep_analysis: skipping, running submit task on slow slot");
            return;
        }
    }

    // Ensure slow slot is idle before proceeding
    let status = state.pty.get_status(MEMORY_SLOW_SLOT_ID).await;
    match status {
        Some(s) if s.state == SessionState::Idle => {}
        Some(s) => {
            debug!(state = ?s.state, "deep_analysis: slow slot not idle");
            return;
        }
        None => {
            // Slot not spawned yet — will be spawned below by ensure_memory_slot_by_id
        }
    }

    let db = state.mission.db();
    let pending_convs = match db.get_pending_deep_analysis(CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES) {
        Ok(convs) => convs,
        Err(_) => return,
    };

    if pending_convs.is_empty() {
        return;
    }

    for conv in &pending_convs {
        let is_checkpoint = conv.status == "active";
        let since_id = if is_checkpoint && conv.deep_analyzed_message_id > 0 {
            Some(conv.deep_analyzed_message_id)
        } else {
            None
        };

        let msgs = db
            .get_conversation_messages(&conv.id, since_id, 1000)
            .unwrap_or_default();
        let msg_count = msgs.len();

        // Skip conversations with too few messages — mark analyzed and move on
        // (only for completed sessions; checkpoints already passed the 100-msg threshold)
        if !is_checkpoint && msg_count < 6 {
            let _ = db.mark_analysis_complete(&conv.id, CURRENT_ANALYSIS_VERSION);
            continue;
        }

        // Meta-sessions excluded at SQL level via conversation_type = 'user'.

        if !ensure_memory_slot_by_id(state, MEMORY_SLOW_SLOT_ID).await {
            break;
        }

        // Record the max message ID for checkpoint watermark advancement
        let max_message_id = msgs.last().map(|m| m.id);

        let checkpoint_hint = if is_checkpoint {
            format!(
                "\n⚠️ 这是增量 checkpoint 分析（活跃会话）。\n\
                 仅分析 since_id={} 之后的 {} 条新消息。\n\
                 调用 mission_conversation_get(sessionId: \"{}\", sinceId: {}) 获取新消息。\n",
                since_id.unwrap_or(0), msg_count, conv.id, since_id.unwrap_or(0)
            )
        } else if conv.deep_analyzed_message_id > 0 {
            // Completed session with previous checkpoint — only analyze remaining
            format!(
                "\n⚠️ 此会话已有 checkpoint，仅分析 since_id={} 之后的 {} 条剩余消息。\n\
                 调用 mission_conversation_get(sessionId: \"{}\", sinceId: {}) 获取剩余消息。\n",
                conv.deep_analyzed_message_id, msg_count, conv.id, conv.deep_analyzed_message_id
            )
        } else {
            format!(
                "\n调用 mission_conversation_get(sessionId: \"{}\") 获取完整会话内容。\n\
                 返回中如有 subagents 字段，表示该会话产生了子任务(Task tool)，可按需获取子会话内容。\n",
                conv.id
            )
        };

        let prompt = format!(
            "[deep-analysis]\n\
             session_id: {session_id}\n\
             project: {project}\n\
             消息数: {msg_count}\n\
             模式: {mode}\n\
             {checkpoint_hint}\n\
             ⚠️ 重要: 消息级知识（偏好/决策/事实）已由 realtime 管道提取，不要重复提取。\n\
             你的任务仅限于:\n\
             1. 跨会话模式 — 用 mission_conversation_search 搜索相关会话，发现反复出现的主题\n\
             2. 工作流抽象 — 可以固化为工具/服务的重复操作\n\
             3. 知识关联 — 不同会话之间的隐含联系\n\
             4. 趋势发现 — 用户行为/需求的演变方向\n\
             5. 问题上报 — 发现 bug/资源浪费/反复出错等需要代码修复的问题时，调用 mission_board_create 创建任务\n\
             6. 运维链路审计 — 会话中是否有重复的多步手动操作（SSH→查日志→重启→再查）？\
             这些操作链可以封装为 MCP 工具一步完成。记录具体步骤序列和建议的工具名，存 category: memory:ops\n\
             7. 调试经验提炼 — 调试过程中走了哪些弯路（错误假设→验证失败→换方向）？\
             根因最终是什么？总结「正确排查路径」供下次遇到类似问题时参考，存 category: memory:debug\n\n\
             不要提取: 单条消息的偏好/决策/事实（realtime 已处理）、当天工作日志、版本细节。\n\
             绝对禁止写入 category: infra（基础设施由 servers.yaml 管理）。",
            session_id = conv.id,
            project = conv.project.as_deref().unwrap_or("unknown"),
            msg_count = msg_count,
            mode = if is_checkpoint { "checkpoint (活跃会话增量)" } else { "full (已完成会话)" },
            checkpoint_hint = checkpoint_hint,
        );

        let task_type = if is_checkpoint { "deep_checkpoint" } else { "deep_analysis" };
        info!(conv_id = %conv.id, msg_count, retries = conv.analysis_retries, is_checkpoint, "Deep analysis: sending to slow lane");

        // Generate task_id and tag the slow slot's current session
        let task_id = uuid::Uuid::new_v4().to_string();
        if let Ok(Some(current_session)) = state.mission.db().get_slot_session(MEMORY_SLOW_SLOT_ID) {
            let _ = state.mission.db().set_conversation_task_id(&current_session, &task_id);
        }

        // Record slot task for history tracking
        let slot_task_id = uuid::Uuid::new_v4().to_string();
        let slot_task = missiond_core::types::SlotTask {
            id: slot_task_id.clone(),
            slot_id: MEMORY_SLOW_SLOT_ID.to_string(),
            task_type: task_type.to_string(),
            status: "pending".to_string(),
            prompt_summary: Some(format!("session: {}, {} msgs{}", &conv.id[..8.min(conv.id.len())], msg_count, if is_checkpoint { " [checkpoint]" } else { "" })),
            source_sessions: Some(serde_json::to_string(&[&conv.id]).unwrap_or_default()),
            output_count: 0,
            created_at: chrono::Utc::now().to_rfc3339(),
            started_at: None,
            completed_at: None,
            duration_ms: None,
            error: None,
            conversation_id: None,
        };
        let _ = state.mission.db().insert_slot_task(&slot_task);

        // Set slow extraction state with conv_id for marking complete on Idle
        {
            let now = chrono::Utc::now().timestamp();
            let mut es = state.slow_extraction_state.write().await;
            es.phase = ExtractionPhase::Sending;
            es.active_type = Some("deep_analysis");
            es.phase_started_at = now;
            es.current_deep_conv_id = Some(conv.id.clone());
            es.current_task_id = Some(task_id);
            es.current_slot_task_id = Some(slot_task_id.clone());
            es.is_checkpoint = is_checkpoint;
            es.checkpoint_message_id = max_message_id;
        }

        let conv_id = conv.id.clone();
        let pty = Arc::clone(&state.pty);
        let extraction_state = Arc::clone(&state.slow_extraction_state);
        let mission = Arc::clone(&state.mission);
        tokio::spawn(async move {
            match pty.send(MEMORY_SLOW_SLOT_ID, &prompt, 900_000).await {
                Ok(res) => {
                    info!(conv_id = %conv_id, duration_ms = res.duration_ms, "Deep analysis send() returned");
                    // send() blocks until slot finishes — complete directly
                    let mut es = extraction_state.write().await;
                    if es.phase == ExtractionPhase::Sending || es.phase == ExtractionPhase::WaitingForSlotIdle {
                        // Mark deep analysis conversation as analyzed
                        let deep_cid = es.current_deep_conv_id.clone();
                        let is_ckpt = es.is_checkpoint;
                        let ckpt_msg_id = es.checkpoint_message_id.take();
                        if let Some(cid) = &deep_cid {
                            if is_ckpt {
                                if let Some(msg_id) = ckpt_msg_id {
                                    let _ = mission.db().update_deep_checkpoint(cid, msg_id);
                                    info!(conv_id = %cid, msg_id, "Deep checkpoint: advanced watermark (send-path)");
                                }
                            } else {
                                let _ = mission.db().mark_analysis_complete(cid, CURRENT_ANALYSIS_VERSION);
                                info!(conv_id = %cid, "Deep analysis: marked complete (send-path)");
                            }
                        }
                        let _ = mission.db().slot_task_set_completed(&slot_task_id, 0);
                        info!(conv_id = %conv_id, duration_ms = res.duration_ms, "Deep analysis complete (send-path)");
                        es.phase = ExtractionPhase::Idle;
                        es.active_type = None;
                        es.current_task_id = None;
                        es.current_slot_task_id = None;
                        es.current_deep_conv_id = None;
                        es.is_checkpoint = false;
                        es.checkpoint_message_id = None;
                    }
                }
                Err(e) => {
                    warn!(conv_id = %conv_id, error = %e, "Deep analysis send() failed");
                    let _ = mission.db().mark_analysis_failed(&conv_id);
                    let _ = mission.db().slot_task_set_failed(&slot_task_id, &e.to_string());
                    let mut es = extraction_state.write().await;
                    es.phase = ExtractionPhase::Idle;
                    es.active_type = None;
                    es.current_task_id = None;
                    es.current_slot_task_id = None;
                    es.current_deep_conv_id = None;
                    es.is_checkpoint = false;
                    es.checkpoint_message_id = None;
                }
            }
        });
        break;
    }
}

/// KB consolidation on slow lane (slot-memory-slow).
/// Periodic (every 24h) KB dedup, merge, and cleanup.
async fn check_kb_consolidation(state: &AppState) {
    // Only run once per 24 hours
    let now = chrono::Utc::now().timestamp();
    let last = state.last_kb_consolidation_at.load(std::sync::atomic::Ordering::Relaxed);
    if last > 0 && now - last < 86400 {
        return;
    }

    // Yield to deep analysis if there's pending work
    let has_deep_pending = state.mission.db()
        .has_pending_deep_analysis(CURRENT_ANALYSIS_VERSION, MAX_ANALYSIS_RETRIES)
        .unwrap_or(false);
    if has_deep_pending {
        return;
    }

    // Gate: slow lane must be idle
    if !check_extraction_gate(&state.slow_extraction_state, &state.mission, "kb_consolidation").await {
        return;
    }

    // Ensure slow slot is spawned and idle
    if !ensure_memory_slot_by_id(state, MEMORY_SLOW_SLOT_ID).await {
        return;
    }
    let status = state.pty.get_status(MEMORY_SLOW_SLOT_ID).await;
    match status {
        Some(s) if s.state == SessionState::Idle => {}
        _ => return,
    }

    info!("KB consolidation: starting periodic cleanup on slow lane");

    let prompt = "[kb-consolidation] 知识库自治整理任务（Board-Aware 模式）。\n\n\
        执行流：\n\
        1. 对 memory 分类调用 mission_kb_analyze(mode=\"consolidation_plan\", target_category=\"memory\", include_board_context=true)\n\
        2. 预检: 遍历 plan.actions，用 mission_kb_get 验证每个 target_key 确实存在，跳过不存在的 key\n\
        3. 遍历 plan.actions 执行：\n\
           - merge/distill: 先 mission_kb_remember 写入 new_entry，再 mission_kb_batch_forget(keys: target_keys) 批量删除\n\
           - update: 如有 linked_task_id，记录关联（暂存到 reason 中即可）\n\
           - delete: 直接 mission_kb_batch_forget(keys: target_keys) 批量删除\n\
        4. 再对 preference 分类执行同样流程（include_board_context=true）\n\
        5. 完成后回复 '[DONE] 整理了 N 个分类，合并 M 条，蒸馏 K 条，删除 J 条'\n\n\
        规则:\n\
        - 不操作 memory:bugfix（有 30 天 Auto-GC）\n\
        - 坚决执行 mission_kb_analyze 返回的 actions，无需二次判断\n\
        - distill = 从已完成项目的多条流水账提取精华，升维到 architecture/feature\n\
        - 如果 plan.actions 为空数组，说明该分类不需要整理，跳过\n\
        - 如果 mission_kb_analyze 返回了 parse_warning（非 JSON），降级为手动分析\n\
        - 删除操作必须使用 mission_kb_batch_forget 批量删除，禁止逐条调用 mission_kb_forget";

    // Generate task_id and tag session
    let task_id = uuid::Uuid::new_v4().to_string();
    if let Ok(Some(current_session)) = state.mission.db().get_slot_session(MEMORY_SLOW_SLOT_ID) {
        let _ = state.mission.db().set_conversation_task_id(&current_session, &task_id);
    }

    // Record slot task for history tracking
    let slot_task_id = uuid::Uuid::new_v4().to_string();
    let slot_task = missiond_core::types::SlotTask {
        id: slot_task_id.clone(),
        slot_id: MEMORY_SLOW_SLOT_ID.to_string(),
        task_type: "kb_consolidation".to_string(),
        status: "pending".to_string(),
        prompt_summary: Some("periodic KB dedup/merge/cleanup".to_string()),
        source_sessions: None,
        output_count: 0,
        created_at: chrono::Utc::now().to_rfc3339(),
        started_at: None,
        completed_at: None,
        duration_ms: None,
        error: None,
        conversation_id: None,
    };
    let _ = state.mission.db().insert_slot_task(&slot_task);

    // Set slow extraction state
    {
        let mut es = state.slow_extraction_state.write().await;
        es.phase = ExtractionPhase::Sending;
        es.active_type = Some("kb_consolidation");
        es.phase_started_at = now;
        es.current_task_id = Some(task_id);
        es.current_slot_task_id = Some(slot_task_id.clone());
    }

    // Update last consolidation timestamp
    state.last_kb_consolidation_at.store(now, std::sync::atomic::Ordering::Relaxed);

    let pty = Arc::clone(&state.pty);
    let extraction_state = Arc::clone(&state.slow_extraction_state);
    let mission = Arc::clone(&state.mission);
    tokio::spawn(async move {
        match pty.send(MEMORY_SLOW_SLOT_ID, prompt, 900_000).await {
            Ok(res) => {
                info!(duration_ms = res.duration_ms, "KB consolidation send() returned");
                // send() blocks until slot finishes — complete directly
                let mut es = extraction_state.write().await;
                if es.phase == ExtractionPhase::Sending || es.phase == ExtractionPhase::WaitingForSlotIdle {
                    let _ = mission.db().slot_task_set_completed(&slot_task_id, 0);
                    info!(duration_ms = res.duration_ms, "KB consolidation complete (send-path)");
                    es.phase = ExtractionPhase::Idle;
                    es.active_type = None;
                    es.current_task_id = None;
                    es.current_slot_task_id = None;
                    es.is_checkpoint = false;
                    es.checkpoint_message_id = None;
                }
            }
            Err(e) => {
                warn!(error = %e, "KB consolidation send() failed");
                let _ = mission.db().slot_task_set_failed(&slot_task_id, &e.to_string());
                let mut es = extraction_state.write().await;
                es.phase = ExtractionPhase::Idle;
                es.active_type = None;
                es.current_task_id = None;
                es.current_slot_task_id = None;
                es.is_checkpoint = false;
                es.checkpoint_message_id = None;
            }
        }
    });
}

// ===== CLAUDE.md sync =====

const MANAGED_START: &str = "<!-- missiond:managed:start -->";
const MANAGED_END: &str = "<!-- missiond:managed:end -->";

/// KB auto-GC: delete infra, expired bugfix, stale zero-access entries. Runs hourly.
fn check_kb_auto_gc(state: &AppState) {
    use std::sync::atomic::Ordering;
    let now = chrono::Utc::now().timestamp();
    let last = state.last_auto_gc_at.load(Ordering::Relaxed);
    if now - last < 3600 {
        return;
    }

    match state.mission.db().kb_auto_gc() {
        Ok(n) if n > 0 => info!(deleted = n, "KB auto-GC completed"),
        Ok(_) => debug!("KB auto-GC: nothing to clean"),
        Err(e) => warn!(error = %e, "KB auto-GC failed"),
    }
    state.last_auto_gc_at.store(now, Ordering::Relaxed);
}

/// Sync KB preferences + hot topics into ~/.claude/CLAUDE.md managed section.
/// Only writes when content actually changes (hash-based detection).
fn sync_claude_md(state: &AppState) {
    let db = state.mission.db();

    let preferences = db.kb_list(Some("preference")).unwrap_or_default();
    let hot_keys = db.kb_hot_keys(20).unwrap_or_default();

    // Nothing to sync
    if preferences.is_empty() && hot_keys.is_empty() {
        return;
    }

    // Hash-based change detection
    let new_hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for p in &preferences { p.summary.hash(&mut hasher); }
        for k in &hot_keys { k.hash(&mut hasher); }
        hasher.finish()
    };
    let last_hash = state.claude_md_hash.load(std::sync::atomic::Ordering::Relaxed);
    if new_hash == last_hash && last_hash != 0 {
        return;
    }

    // Build managed section
    let mut managed = String::new();
    managed.push_str(MANAGED_START);
    managed.push_str("\n# MissionD Managed\n");

    if !preferences.is_empty() {
        managed.push_str("\n## Preferences\n");
        for p in &preferences {
            managed.push_str(&format!("- {}\n", p.summary));
        }
    }

    if !hot_keys.is_empty() {
        managed.push_str(&format!("\n## Hot Topics\n{}\n", hot_keys.join(", ")));
    }

    managed.push_str(MANAGED_END);

    // Read existing file
    let claude_md_path = match dirs::home_dir() {
        Some(home) => home.join(".claude/CLAUDE.md"),
        None => {
            warn!("Cannot determine home directory for CLAUDE.md sync");
            return;
        }
    };

    let existing = std::fs::read_to_string(&claude_md_path).unwrap_or_default();

    // Replace or append managed section
    let new_content = if let (Some(start), Some(end_pos)) = (
        existing.find(MANAGED_START),
        existing.find(MANAGED_END),
    ) {
        let before = &existing[..start];
        let after_marker = end_pos + MANAGED_END.len();
        let after = &existing[after_marker..];
        format!("{}{}{}", before, managed, after)
    } else {
        // Append to end
        if existing.trim().is_empty() {
            managed
        } else {
            format!("{}\n\n{}\n", existing.trim_end(), managed)
        }
    };

    // Only write if content actually differs
    if new_content == existing {
        state.claude_md_hash.store(new_hash, std::sync::atomic::Ordering::Relaxed);
        return;
    }

    match std::fs::write(&claude_md_path, &new_content) {
        Ok(_) => {
            info!(
                prefs = preferences.len(),
                topics = hot_keys.len(),
                "CLAUDE.md managed section synced"
            );
            state.claude_md_hash.store(new_hash, std::sync::atomic::Ordering::Relaxed);
        }
        Err(e) => warn!(error = %e, "Failed to write CLAUDE.md"),
    }
}

// ===== Context Budget Manager =====
// Prevents 502/413 errors by ensuring messages don't exceed upstream payload limits.
// Architecture: Storage layer stores everything; this is the Compute/Transport layer.

/// Max HTTP payload content size for Router API calls.
/// Conservative limit to avoid 502 from upstream proxies (Caddy/Nginx/Cloudflare).
const MAX_ROUTER_PAYLOAD_BYTES: usize = 6 * 1024 * 1024; // 6 MB

/// Result of applying context budget to a messages array.
struct ContextBudgetResult {
    /// Whether any trimming was applied.
    trimmed: bool,
    /// Human-readable note about what was trimmed (None if within budget).
    note: Option<String>,
}

/// Apply context budget to a messages array for Router API calls.
///
/// Strategy when over budget:
/// 1. Keep first message (system prompt / context) + last N recent turns
/// 2. Drop middle messages, insert a note about omitted context
/// 3. Progressively reduce N until within budget
/// 4. If even 2 messages exceed budget, truncate the longest message
fn apply_context_budget(messages: &mut Vec<Value>, max_bytes: usize) -> ContextBudgetResult {
    let calc_size = |msgs: &[Value]| -> usize {
        msgs.iter()
            .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
            .map(|s| s.len())
            .sum()
    };

    let total_bytes = calc_size(messages);
    if total_bytes <= max_bytes {
        return ContextBudgetResult { trimmed: false, note: None };
    }

    let original_count = messages.len();

    // Edge case: 0-1 messages, can only truncate content
    if messages.len() <= 1 {
        if let Some(msg) = messages.first_mut() {
            truncate_message_content(msg, max_bytes);
        }
        return ContextBudgetResult {
            trimmed: true,
            note: Some(format!(
                "单条消息超出预算({:.1}MB > {:.1}MB)，已截断内容。",
                total_bytes as f64 / 1_048_576.0,
                max_bytes as f64 / 1_048_576.0
            )),
        };
    }

    // Sliding window: keep first message + last N turns
    let first = messages[0].clone();
    let mut keep_tail = messages.len() - 1;

    loop {
        let mut candidate: Vec<Value> = Vec::new();
        candidate.push(first.clone());

        let dropped = original_count - 1 - keep_tail;
        if dropped > 0 {
            candidate.push(serde_json::json!({
                "role": "system",
                "content": format!(
                    "[上下文管理] 为适应上下文窗口，已省略中间 {} 轮对话。如需回溯历史，请使用 mission_conversation_get 工具查询完整记录。",
                    dropped
                )
            }));
        }

        let tail_start = original_count - keep_tail;
        for msg in &messages[tail_start..] {
            candidate.push(msg.clone());
        }

        let size = calc_size(&candidate);
        if size <= max_bytes {
            let note = format!(
                "上下文超出预算({:.1}MB > {:.1}MB)，保留首条 + 最近 {} 轮，省略 {} 轮中间对话。",
                total_bytes as f64 / 1_048_576.0,
                max_bytes as f64 / 1_048_576.0,
                keep_tail,
                dropped
            );
            *messages = candidate;
            return ContextBudgetResult { trimmed: true, note: Some(note) };
        }

        if keep_tail <= 1 {
            // Even first + last message exceeds budget; truncate the longest
            let longest_idx = candidate.iter().enumerate()
                .max_by_key(|(_, m)| {
                    m.get("content").and_then(|c| c.as_str()).map(|s| s.len()).unwrap_or(0)
                })
                .map(|(i, _)| i)
                .unwrap_or(0);
            truncate_message_content(&mut candidate[longest_idx], max_bytes / 2);
            let note = format!(
                "上下文严重超出预算({:.1}MB > {:.1}MB)，仅保留首尾消息并截断最长内容。",
                total_bytes as f64 / 1_048_576.0,
                max_bytes as f64 / 1_048_576.0,
            );
            *messages = candidate;
            return ContextBudgetResult { trimmed: true, note: Some(note) };
        }

        keep_tail -= 1;
    }
}

/// Truncate a single message's content to fit within max_bytes (char-safe).
fn truncate_message_content(msg: &mut Value, max_bytes: usize) {
    if let Some(content) = msg.get("content").and_then(|c| c.as_str()).map(String::from) {
        if content.len() > max_bytes {
            // Use char_indices for safe UTF-8 truncation
            let target_chars = max_bytes / 3; // conservative: assume ~3 bytes per char average
            let truncated: String = content.chars().take(target_chars).collect();
            msg["content"] = Value::String(format!(
                "{}\n\n[...内容因超出上下文预算被截断，原始长度 {:.1}MB...]",
                truncated,
                content.len() as f64 / 1_048_576.0
            ));
        }
    }
}

/// Extract displayable content from a Claude Code message content field.
/// Extract text content from a message's content field for the `content` DB column.
/// Content can be a plain string or an array of content blocks.
/// Storage layer: NO truncation. Full content preserved for analysis pipelines.
/// Truncation happens at the API/display layer only.
// extract_text_content, sanitize_raw_content, handle_new_events, backfill_conversation_events
// → moved to events_sync.rs

/// Format a single tool call entry for the audit trace Markdown.
fn format_tool_call_trace(md: &mut String, tc: &missiond_core::types::ToolCallRecord) {
    let time = if tc.timestamp.len() >= 19 {
        &tc.timestamp[11..19]
    } else {
        &tc.timestamp
    };
    let id_short = if tc.id.len() > 15 {
        format!("{}...", &tc.id[..12])
    } else {
        tc.id.clone()
    };
    let status_icon = match tc.status.as_str() {
        "success" => "✅",
        "error" => "❌",
        "pending" => "⏳",
        _ => "❓",
    };
    md.push_str(&format!("[{time}] 🛠️ {} ({id_short})\n", tc.tool_name));
    if let Some(ref summary) = tc.input_summary {
        md.push_str(&format!("  ├─ Input: {summary}\n"));
    }
    let output_display = tc
        .output_summary
        .as_deref()
        .unwrap_or(if tc.status == "pending" { "awaiting result" } else { "N/A" });
    md.push_str(&format!("  └─ Output: [{status_icon}] {output_display}\n\n"));
}

/// Handle a NewMessages watcher event: write conversation messages to DB.
fn handle_new_messages(
    state: &AppState,
    session_id: String,
    project_path: String,
    jsonl_path: String,
    messages: Vec<missiond_core::CCMessageLine>,
    is_pty: bool,
) {
    let db = state.mission.db();

    // Determine slot_id if this session belongs to a PTY slot
    let slot_id = if is_pty {
        db.get_slot_for_session(&session_id).unwrap_or(None)
    } else {
        None
    };
    let is_slot_session = slot_id.is_some();
    let source = if is_pty { "pty_jsonl" } else { "claude_cli" };

    // Ensure conversation exists; re-activate if completed
    let existing_conv = db.get_conversation(&session_id).unwrap_or(None);
    if let Some(ref conv) = existing_conv {
        if conv.status == "completed" {
            if let Err(e) = db.reactivate_conversation(&session_id) {
                warn!(session = %session_id, error = %e, "Failed to re-activate conversation");
            } else {
                info!(session = %session_id, "Re-activated completed conversation");
            }
        }
    }
    if existing_conv.is_none() {
        let first = messages.first();
        // Extract parent session ID for subagent conversations
        let parent_session_id = if session_id.starts_with("agent-") {
            missiond_core::db::extract_parent_session_id(&jsonl_path)
        } else {
            None
        };
        let conv = missiond_core::types::Conversation {
            id: session_id.clone(),
            project: Some(first.map(|m| m.cwd.clone()).unwrap_or(project_path)),
            slot_id: slot_id.clone(),
            source: source.to_string(),
            model: first.and_then(|m| m.message.model.clone()),
            git_branch: first.and_then(|m| m.git_branch.clone()),
            jsonl_path: Some(jsonl_path),
            parent_session_id,
            task_id: None,
            message_count: 0,
            started_at: first
                .map(|m| m.timestamp.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            ended_at: None,
            status: "active".to_string(),
            analyzed_at: None,
            analysis_version: 0,
            analysis_retries: 0,
            deep_analyzed_message_id: 0,
            chat_type: None,
            conversation_type: missiond_core::db::derive_conversation_type(slot_id.as_deref(), &session_id),
        };
        if let Err(e) = db.upsert_conversation(&conv) {
            error!(session = %session_id, error = %e, "Failed to create conversation");
            return;
        }
    }

    let batch: Vec<missiond_core::types::ConversationMessage> = messages.iter()
        .filter_map(|msg| {
            let text_content = events_sync::extract_text_content(&msg.message.content);
            // Detect special content types from JSONL content blocks
            let content_types: Vec<&str> = msg.message.content.as_array()
                .map(|arr| arr.iter()
                    .filter_map(|b| b.get("type").and_then(|t| t.as_str()))
                    .collect())
                .unwrap_or_default();
            let is_tool_result = !content_types.is_empty() && content_types.iter().all(|t| *t == "tool_result");
            let is_thinking = !content_types.is_empty() && content_types.iter().all(|t| *t == "thinking");
            // Keep tool_result and thinking messages even if content extraction is empty
            if text_content.is_empty() && !is_tool_result {
                return None;
            }
            let content = if text_content.is_empty() {
                "[tool_result]".to_string()
            } else {
                text_content
            };
            let role = if is_tool_result {
                "tool_result".to_string()
            } else if is_thinking {
                "thinking".to_string()
            } else if msg.message.role == "user" && is_slot_session {
                // Slot sessions: "user" messages are daemon-sent system prompts, not the human
                "system".to_string()
            } else {
                msg.message.role.clone()
            };
            let raw_content = events_sync::sanitize_raw_content(&msg.message.content);
            Some(missiond_core::types::ConversationMessage {
                id: 0,
                session_id: session_id.clone(),
                role,
                content,
                raw_content,
                message_uuid: Some(msg.uuid.clone()),
                parent_uuid: msg.parent_uuid.clone(),
                model: msg.message.model.clone(),
                timestamp: msg.timestamp.clone(),
                metadata: None,
            })
        })
        .collect();

    match db.insert_conversation_messages_batch(&batch) {
        Ok(inserted_ids) if !inserted_ids.is_empty() => {
            info!(session = %session_id, count = inserted_ids.len(), "Logged conversation messages");
            // Deep analysis uses conversation-level analyzed_at watermark (no per-message enqueue needed).
            // Realtime extraction uses realtime_forwarded_at watermark.
        }
        Err(e) => {
            error!(session = %session_id, error = %e, "Failed to insert conversation messages");
        }
        _ => {}
    }

    // ── Audit: extract tool_use/tool_result into conversation_tool_calls ──
    {
        let mut tool_calls = Vec::new();
        let mut tool_results = Vec::new();
        for msg in &messages {
            let role = &msg.message.role;
            let content = &msg.message.content;
            if role == "assistant" {
                tool_calls.extend(events_sync::extract_tool_calls_from_assistant(
                    &session_id,
                    &msg.timestamp,
                    content,
                ));
            } else if role == "user" {
                tool_results.extend(events_sync::extract_tool_results_from_user(content));
            }
        }
        if !tool_calls.is_empty() {
            match db.insert_tool_calls_batch(&tool_calls) {
                Ok(count) if count > 0 => {
                    info!(session = %session_id, count, "Extracted tool calls for audit");
                }
                Err(e) => {
                    warn!(session = %session_id, error = %e, "Failed to insert tool calls");
                }
                _ => {}
            }
        }
        for (tool_use_id, summary, raw, status) in &tool_results {
            if let Err(e) = db.update_tool_call_output(tool_use_id, summary, raw, status) {
                warn!(tool_use_id, error = %e, "Failed to update tool call output");
            }
        }
    }

    // ── Token usage ledger ─────────────────────────────────────────
    // Extract usage from assistant messages and write to append-only ledger.
    let slot_task_id = slot_id.as_deref()
        .and_then(|sid| db.get_running_slot_task(sid).ok().flatten());
    for msg in &messages {
        if let Some(ref usage) = msg.message.usage {
            let total = usage.input_tokens + usage.output_tokens
                + usage.cache_creation_input_tokens + usage.cache_read_input_tokens;
            if total == 0 {
                continue;
            }
            if let Err(e) = db.insert_token_usage(
                &session_id,
                slot_id.as_deref(),
                slot_task_id.as_deref(),
                msg.message.model.as_deref(),
                usage.input_tokens,
                usage.cache_creation_input_tokens,
                usage.cache_read_input_tokens,
                usage.output_tokens,
            ) {
                warn!(session = %session_id, error = %e, "Failed to insert token usage");
            }
        }
    }
}

fn handle_pty_text_complete(
    state: &AppState,
    slot_id: String,
    turn_id: u64,
    content: String,
    timestamp: i64,
) {
    let db = state.mission.db();

    // If this slot has a captured JSONL session UUID, JSONL provides richer data.
    // Skip inferior PTY TextComplete logging to avoid dual-write.
    if db.get_slot_session(&slot_id).unwrap_or(None).is_some() {
        return;
    }

    let session_id = format!("pty-{}", slot_id);

    // Ensure conversation exists for this PTY session
    if db.get_conversation(&session_id).unwrap_or(None).is_none() {
        let ts = chrono::DateTime::from_timestamp(timestamp, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| timestamp.to_string());
        let conv = missiond_core::types::Conversation {
            id: session_id.clone(),
            project: None,
            slot_id: Some(slot_id.clone()),
            source: "pty".to_string(),
            model: None,
            git_branch: None,
            jsonl_path: None,
            parent_session_id: None,
            task_id: None,
            message_count: 0,
            started_at: ts,
            ended_at: None,
            status: "active".to_string(),
            analyzed_at: None,
            analysis_version: 0,
            analysis_retries: 0,
            deep_analyzed_message_id: 0,
            chat_type: None,
            conversation_type: missiond_core::db::derive_conversation_type(Some(&slot_id), &session_id),
        };
        if let Err(e) = db.upsert_conversation(&conv) {
            error!(slot = %slot_id, error = %e, "Failed to create PTY conversation");
            return;
        }
    }

    let msg_uuid = format!("pty-{}-turn-{}", slot_id, turn_id);

    let ts = chrono::DateTime::from_timestamp(timestamp, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| timestamp.to_string());

    let conv_msg = missiond_core::types::ConversationMessage {
        id: 0,
        session_id: session_id.clone(),
        role: "assistant".to_string(),
        content,
        raw_content: None,
        message_uuid: Some(msg_uuid),
        parent_uuid: None,
        model: None,
        timestamp: ts,
        metadata: Some(format!("{{\"turn_id\":{}}}", turn_id)),
    };

    // INSERT OR IGNORE handles dedup via UNIQUE index on message_uuid
    match db.insert_conversation_message(&conv_msg) {
        Ok(_) => {
            info!(slot = %slot_id, turn = turn_id, "Logged PTY assistant output");
        }
        Err(e) => {
            error!(slot = %slot_id, turn = turn_id, error = %e, "Failed to insert PTY message");
        }
    }
}

async fn bind_ipc_listener(endpoint: &str) -> Result<IpcListener> {
    IpcListener::bind(endpoint).await
}

#[tokio::main]
async fn main() -> Result<()> {
    let home = default_mission_home();
    std::fs::create_dir_all(&home).ok();

    // Dual-layer logging: stderr + file (daily rotation)
    let log_dir = home.join("logs");
    std::fs::create_dir_all(&log_dir).ok();
    let file_appender = tracing_appender::rolling::daily(&log_dir, "missiond.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    tracing_subscriber::registry()
        .with(log_filter())
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false),
        )
        .init();

    // Panic hook: log panic info before process exits (normal panic output goes to stderr
    // which may not be captured; this ensures it's in the tracing log file too).
    std::panic::set_hook(Box::new(|info| {
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
        eprintln!("PANIC at {}: {}", location, payload);
        tracing::error!(location = %location, "DAEMON PANIC: {}", payload);
    }));

    // Ensure config files have restrictive permissions
    #[cfg(unix)]
    ensure_config_permissions(&home);

    let db_path = db_path();
    let slots_path = slots_config_path();
    if !slots_path.exists() {
        return Err(anyhow!(
            "Slots config not found: {} (set MISSION_SLOTS_CONFIG or create slots.yaml)",
            slots_path.display()
        ));
    }

    let logs_dir = logs_dir(&db_path);
    let permission_config_path = db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("config")
        .join("permissions.yaml");
    let permission = Arc::new(PermissionPolicy::new(&permission_config_path));

    let mission = Arc::new(MissionControl::new(MissionControlOptions {
        db_path: db_path.clone(),
        slots_config_path: slots_path.clone(),
        permission_config_path: None,
        logs_dir: Some(logs_dir.clone()),
        default_mode: None,
    })?);
    mission.start().await?;

    // Startup: clean orphan slot_tasks from previous daemon instance
    match mission.db().cleanup_orphan_slot_tasks() {
        Ok(n) if n > 0 => info!(count = n, "Cleaned up orphan slot tasks from previous run"),
        Err(e) => warn!(error = %e, "Failed to cleanup orphan slot tasks"),
        _ => {}
    }

    // PTY manager setup
    let pty = Arc::new(PTYManager::new(logs_dir.clone()));
    pty.set_permission_policy(Arc::new(PermissionAdapter {
        permission: Arc::clone(&permission),
    }))
    .await;

    // Init PTY slots
    for slot in mission.list_slots() {
        let pty_slot = missiond_core::PTYSlot {
            id: slot.config.id.clone(),
            role: slot.config.role.clone(),
            cwd: slot.config.cwd.as_deref().map(PathBuf::from),
        };
        pty.init_slot(&pty_slot).await;
    }

    // CC tasks watcher
    let mut cc = CCTasksWatcher::new(CCTasksWatcherOptions::default());
    cc.start().await?;
    let cc_tasks = Arc::new(Mutex::new(cc));

    // Conversation logger: subscribe to watcher events (processed in main select loop)
    let mut conv_logger_rx = cc_tasks.lock().await.subscribe();
    // PTY conversation logger: subscribe to manager events
    let mut pty_logger_rx = pty.subscribe();

    // Screenshot broker (coordinates browser-based PTY screenshots)
    let screenshot_broker = missiond_core::ws::ScreenshotBroker::new(
        std::time::Duration::from_secs(5),
    );

    // WebSocket server (PTY attach + Tasks events)
    let ws_port = ws_port();
    let mut ws_server = PTYWebSocketServer::new(WSServerOptions {
        port: ws_port,
        pty_manager: Some(Arc::clone(&pty)),
        cc_tasks_watcher: Some(Arc::clone(&cc_tasks)),
        screenshot_broker: Some(Arc::clone(&screenshot_broker)),
    });
    if let Err(e) = ws_server.start().await {
        // Match Node behavior: continue running even if WS is unavailable (e.g. port in use).
        warn!(port = ws_port, error = %e, "Failed to start WebSocket server");
    }

    // IPC server
    let endpoint = ipc_endpoint_from_env();
    let listener = bind_ipc_listener(&endpoint).await?;
    info!(endpoint = %endpoint, "missiond IPC listening");

    // Infrastructure registry
    let servers_path = home.join("servers.yaml");
    let infra = Arc::new(InfraConfig::load(&servers_path));
    info!(count = infra.servers.len(), path = %servers_path.display(), "Infra registry loaded");

    // Skill index (scan ~/.claude/skills/)
    let skills_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("skills");
    let skills = Arc::new(SkillIndex::build(&skills_dir));
    info!(count = skills.list().len(), "Skill index loaded");

    // Skill Engine: ingest SKILL.md files into DB for FTS5 search
    let ingested = missiond_core::skill::ingest_skills(mission.db(), &skills_dir);
    info!(count = ingested, "Skill engine: ingested skills into DB");

    // Warm PTY session UUID cache from DB (activates slot_sessions table)
    let existing_slot_sessions = mission.db().get_all_slot_sessions().unwrap_or_default();
    let pty_uuids: HashSet<String> = existing_slot_sessions
        .iter()
        .map(|(_, session_id)| session_id.clone())
        .collect();
    if !pty_uuids.is_empty() {
        info!(count = pty_uuids.len(), "Loaded PTY session UUIDs from DB");
    }

    let state = AppState {
        mission,
        permission,
        pty,
        cc_tasks,
        skills,
        infra,
        pty_session_uuids: Arc::new(tokio::sync::RwLock::new(pty_uuids)),
        extraction_state: Arc::new(tokio::sync::RwLock::new(ExtractionState {
            phase: ExtractionPhase::Idle,
            active_type: None,
            phase_started_at: 0,
            current_deep_conv_id: None,
            watermark_targets: Vec::new(),
            current_task_id: None,
            current_slot_task_id: None,
            is_checkpoint: false,
            checkpoint_message_id: None,
        })),
        slow_extraction_state: Arc::new(tokio::sync::RwLock::new(ExtractionState {
            phase: ExtractionPhase::Idle,
            active_type: None,
            phase_started_at: 0,
            current_deep_conv_id: None,
            watermark_targets: Vec::new(),
            current_task_id: None,
            current_slot_task_id: None,
            is_checkpoint: false,
            checkpoint_message_id: None,
        })),
        memory_slot_busy_since: Arc::new(std::sync::atomic::AtomicI64::new(0)),
        slow_slot_busy_since: Arc::new(std::sync::atomic::AtomicI64::new(0)),
        claude_md_hash: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        extraction_notify: Arc::new(tokio::sync::Notify::new()),
        slow_extraction_notify: Arc::new(tokio::sync::Notify::new()),
        submit_notify: Arc::new(tokio::sync::Notify::new()),
        last_auto_gc_at: Arc::new(std::sync::atomic::AtomicI64::new(0)),
        last_kb_consolidation_at: Arc::new(std::sync::atomic::AtomicI64::new(0)),
        memory_paused: Arc::new(std::sync::atomic::AtomicBool::new(
            home.join("memory_paused").exists()
        )),
        memory_paused_at: Arc::new(std::sync::atomic::AtomicI64::new({
            let flag = home.join("memory_paused");
            if flag.exists() {
                std::fs::read_to_string(&flag).ok()
                    .and_then(|s| s.trim().parse::<i64>().ok())
                    .unwrap_or_else(|| chrono::Utc::now().timestamp())
            } else {
                0
            }
        })),
        slot_fail_counts: Arc::new(std::sync::Mutex::new(HashMap::new())),
        screenshot_broker: Arc::clone(&screenshot_broker),
        jarvis_trace: ws_server.jarvis_trace_store().clone(),
        slot_last_responses: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        slot_progress: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        http_client: reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .expect("Failed to build HTTP client"),
        xjp_mcp: Arc::new(McpProcessClient::new(
            default_mission_home().join("xjp-mcp-config.json"),
        )),
    };

    // Auto-spawn slots with auto_start: true
    {
        let slots = state.mission.list_slots();
        for slot in &slots {
            if slot.config.auto_start == Some(true) {
                info!(slot_id = %slot.config.id, "Auto-starting slot on daemon boot");
                match state.mission.spawn_agent(
                    &slot.config.id,
                    Some(missiond_core::SpawnOptions {
                        visible: false,
                        auto_restart: true,
                    }),
                ).await {
                    Ok(_) => info!(slot_id = %slot.config.id, "Auto-started slot"),
                    Err(e) => warn!(slot_id = %slot.config.id, error = %e, "Failed to auto-start slot"),
                }
            }
        }
    }

    // One-time backfill: populate conversation_events from historical JSONL files
    {
        let backfill_state = state.clone();
        tokio::spawn(async move {
            events_sync::backfill_conversation_events(backfill_state.mission.db()).await;
        });
    }

    // One-time backfill: populate conversation_tool_calls from existing conversation_messages
    {
        let backfill_state = state.clone();
        tokio::spawn(async move {
            events_sync::backfill_tool_calls(backfill_state.mission.db()).await;
        });
    }

    // Autopilot scheduler + IPC server via select
    let mut autopilot_interval = tokio::time::interval(std::time::Duration::from_secs(60));
    info!("Autopilot scheduler started (60s interval)");

    loop {
        tokio::select! {
            result = listener.accept() => {
                let stream = result?;
                let reader = BufReader::new(stream);
                let conn_state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_ipc_connection(conn_state, reader).await {
                        warn!(error = %e, "IPC connection error");
                    }
                });
            }
            _ = autopilot_interval.tick() => {
                if let Err(e) = autopilot_tick(&state).await {
                    warn!(error = %e, "Autopilot tick failed");
                }
            }
            _ = state.extraction_notify.notified() => {
                // Fast lane extraction completed — run unified scheduler
                if !state.memory_paused.load(std::sync::atomic::Ordering::Relaxed) {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    schedule_memory_tasks(&state).await;
                }
            }
            _ = state.slow_extraction_notify.notified() => {
                // Slow lane extraction completed — run unified scheduler
                if !state.memory_paused.load(std::sync::atomic::Ordering::Relaxed) {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    schedule_memory_tasks(&state).await;
                }
            }
            _ = state.submit_notify.notified() => {
                // Submit task created/completed — always dispatch submit tasks (not gated by memory_paused)
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                dispatch_queued_submit_tasks(&state).await;
                // Also run memory scheduler if not paused
                if !state.memory_paused.load(std::sync::atomic::Ordering::Relaxed) {
                    schedule_memory_tasks(&state).await;
                }
            }
            event = conv_logger_rx.recv() => {
                match event {
                    Ok(WatcherEvent::NewMessages { session_id, project_path, jsonl_path, messages }) => {
                        let mut is_pty = state.pty_session_uuids.read().await.contains(&session_id);

                        // Compaction detection: if session is unknown, check if it replaced an active slot session
                        let mut compaction_task_id: Option<String> = None;
                        if !is_pty {
                            if let Some((slot_id, old_uuid, old_task_id)) = detect_compaction(&state, &session_id, &project_path) {
                                info!(
                                    slot_id = %slot_id,
                                    old_session = %old_uuid,
                                    new_session = %session_id,
                                    "Compaction detected: session replaced by context compaction"
                                );
                                let db = state.mission.db();
                                // Mark old session as compacted (not completed)
                                let _ = db.mark_conversation_compacted(&old_uuid);
                                // Update slot→session mapping
                                let _ = db.set_slot_session(&slot_id, &session_id);
                                state.pty_session_uuids.write().await.remove(&old_uuid);
                                state.pty_session_uuids.write().await.insert(session_id.clone());
                                compaction_task_id = old_task_id;
                                is_pty = true;
                            }
                        }

                        // --- Progress tracking: extract tool_use/tool_result for in-memory stats ---
                        if is_pty {
                            if let Ok(Some(slot_id)) = state.mission.db().get_slot_for_session(&session_id) {
                                let mut progress = state.slot_progress.write().await;
                                let sp = progress.entry(slot_id).or_default();
                                // Session switch detection: reset counters when session changes
                                if sp.session_id != session_id {
                                    *sp = SlotProgress { session_id: session_id.clone(), ..Default::default() };
                                }
                                for msg in &messages {
                                    if let Some(blocks) = msg.message.content.as_array() {
                                        for block in blocks {
                                            match block.get("type").and_then(|t| t.as_str()) {
                                                Some("tool_use") => {
                                                    let name = block.get("name")
                                                        .and_then(|n| n.as_str())
                                                        .unwrap_or("unknown")
                                                        .to_string();
                                                    *sp.tool_counts.entry(name.clone()).or_insert(0) += 1;
                                                    sp.total_calls += 1;
                                                    sp.current_tool = Some(CurrentToolInfo {
                                                        name,
                                                        started_at: msg.timestamp.clone(),
                                                    });
                                                    sp.last_activity = Some(msg.timestamp.clone());
                                                }
                                                Some("tool_result") => {
                                                    sp.total_results += 1;
                                                    sp.current_tool = None;
                                                    if block.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false) {
                                                        sp.error_count += 1;
                                                    }
                                                    sp.last_activity = Some(msg.timestamp.clone());
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Filter out tool_use before DB insertion (keep existing DB behavior)
                        let db_messages: Vec<_> = messages.into_iter()
                            .filter(|m| m.message_type != "tool_use")
                            .collect();
                        handle_new_messages(&state, session_id.clone(), project_path, jsonl_path, db_messages, is_pty);

                        // Transfer task_id AFTER conversation creation (so the row exists)
                        if let Some(tid) = compaction_task_id {
                            let _ = state.mission.db().set_conversation_task_id(&session_id, &tid);
                        }
                    }
                    Ok(WatcherEvent::NewEvents { session_id, events }) => {
                        events_sync::handle_new_events(state.mission.db(), session_id, events);
                    }
                    Ok(WatcherEvent::SessionInactive(session)) => {
                        // Skip already-compacted sessions (replaced by context compaction)
                        if let Ok(Some(conv)) = state.mission.db().get_conversation(&session.session_id) {
                            if conv.status == "compacted" {
                                debug!(session = %session.session_id, "Skipping inactive check for compacted session");
                                continue;
                            }
                        }
                        // Mark conversation as completed so deep analysis can pick it up
                        if let Err(e) = state.mission.db().complete_conversation(&session.session_id) {
                            warn!(session = %session.session_id, error = %e, "Failed to complete conversation");
                        } else {
                            info!(session = %session.session_id, "Conversation marked completed");
                        }
                    }
                    Ok(_) => {} // Other events handled by WS server
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "Conversation logger lagged");
                    }
                    Err(_) => {} // Channel closed, ignore
                }
            }
            pty_event = pty_logger_rx.recv() => {
                match pty_event {
                    Ok(missiond_core::ManagerEvent::TextComplete { slot_id, turn_id, content, timestamp }) => {
                        // Save last response for submit task result tracking
                        if !content.is_empty() {
                            state.slot_last_responses.write().await.insert(slot_id.clone(), content.clone());
                        }
                        handle_pty_text_complete(&state, slot_id, turn_id, content, timestamp);
                    }
                    Ok(missiond_core::ManagerEvent::Exited { slot_id, exit_code }) => {
                        info!(slot_id = %slot_id, exit_code = exit_code, "PTY session exited");
                        // Clean up UUID mapping for exited slot
                        let old_uuid = state.mission.db().get_slot_session(&slot_id).unwrap_or(None);
                        if let Some(ref uuid) = old_uuid {
                            let _ = state.mission.db().complete_conversation(uuid);
                            state.pty_session_uuids.write().await.remove(uuid);
                        }
                        state.mission.db().clear_slot_session(&slot_id);
                    }
                    Ok(missiond_core::ManagerEvent::StateChange { ref slot_id, new_state, prev_state }) => {
                        // Route memory slot state changes to the correct lane
                        let lane = if slot_id == MEMORY_SLOT_ID {
                            Some(("fast", &state.extraction_state, &state.memory_slot_busy_since, &state.extraction_notify))
                        } else if slot_id == MEMORY_SLOW_SLOT_ID {
                            Some(("slow", &state.slow_extraction_state, &state.slow_slot_busy_since, &state.slow_extraction_notify))
                        } else {
                            None
                        };
                        if let Some((lane_name, es_lock, busy_since, notify)) = lane {
                            if new_state == SessionState::Idle {
                                busy_since.store(0, std::sync::atomic::Ordering::SeqCst);
                                // Release extraction gate when slot returns to Idle
                                let mut es = es_lock.write().await;
                                if es.phase == ExtractionPhase::WaitingForSlotIdle || es.phase == ExtractionPhase::Sending {
                                    let phase_age = chrono::Utc::now().timestamp() - es.phase_started_at;
                                    // Ignore spurious Idle transitions from slot spawn (< 3s)
                                    if phase_age < 3 {
                                        debug!(lane = lane_name, phase_age, "Ignoring early Idle transition (likely spawn init)");
                                    } else {
                                        let is_realtime = matches!(es.active_type, Some("realtime"));
                                        info!(
                                            lane = lane_name,
                                            extraction_type = ?es.active_type,
                                            phase_age,
                                            "Extraction complete: slot returned to Idle"
                                        );
                                        // Fast lane: advance watermarks for processed sessions
                                        if is_realtime {
                                            if !es.watermark_targets.is_empty() {
                                                let db = state.mission.db();
                                                for (session_id, timestamp) in &es.watermark_targets {
                                                    let _ = db.update_realtime_forwarded_at(session_id, timestamp);
                                                }
                                                info!(sessions = es.watermark_targets.len(), "Realtime: advanced watermarks");
                                                es.watermark_targets.clear();
                                            }
                                        }
                                        // Slow lane: mark deep analysis conversation as analyzed
                                        if matches!(es.active_type, Some("deep_analysis")) {
                                            if let Some(conv_id) = es.current_deep_conv_id.take() {
                                                if es.is_checkpoint {
                                                    // Checkpoint: advance watermark, don't mark fully analyzed
                                                    if let Some(msg_id) = es.checkpoint_message_id.take() {
                                                        if let Err(e) = state.mission.db().update_deep_checkpoint(&conv_id, msg_id) {
                                                            warn!(conv_id = %conv_id, error = %e, "Failed to advance checkpoint watermark");
                                                        } else {
                                                            info!(conv_id = %conv_id, msg_id, "Deep analysis checkpoint: advanced watermark");
                                                        }
                                                    }
                                                } else {
                                                    // Full analysis (completed session)
                                                    if let Err(e) = state.mission.db().mark_analysis_complete(&conv_id, CURRENT_ANALYSIS_VERSION) {
                                                        warn!(conv_id = %conv_id, error = %e, "Failed to mark analysis complete");
                                                    } else {
                                                        info!(conv_id = %conv_id, version = CURRENT_ANALYSIS_VERSION, "Deep analysis: marked complete");
                                                    }
                                                }
                                            }
                                        }
                                        // Mark slot task completed in history
                                        if let Some(ref st_id) = es.current_slot_task_id {
                                            let _ = state.mission.db().slot_task_set_completed(st_id, 0);
                                        }
                                        es.phase = ExtractionPhase::Idle;
                                        es.active_type = None;
                                        es.current_task_id = None;
                                        es.current_slot_task_id = None;
                                        es.is_checkpoint = false;
                                        es.checkpoint_message_id = None;
                                        // Event-driven: signal main loop to re-check this lane
                                        notify.notify_one();
                                    }
                                }
                            } else if prev_state == SessionState::Idle {
                                busy_since.store(
                                    chrono::Utc::now().timestamp(),
                                    std::sync::atomic::Ordering::SeqCst,
                                );
                            }
                        }

                        // Close Running submit tasks assigned to this slot when it returns to Idle.
                        // Guard: tasks running < 5s are only closed if JSONL confirms turn_duration.
                        // This prevents premature closure on PTY startup noise while allowing
                        // genuinely fast tasks (e.g. simple KB operations) to close promptly.
                        if new_state == SessionState::Idle && prev_state != SessionState::Idle {
                            if let Ok(running_tasks) = state.mission.db().get_tasks_by_status(missiond_core::types::TaskStatus::Running) {
                                let now = chrono::Utc::now().timestamp_millis();
                                const MIN_EXECUTION_MS: i64 = 5_000; // 5 seconds: filter PTY startup noise
                                const MIN_JSONL_EXECUTION_MS: i64 = 3_000; // 3 seconds: even with JSONL, prevent stale turn confirmation
                                // Get PTY last response as fallback
                                let pty_resp = state.slot_last_responses.write().await.remove(slot_id.as_str());
                                // Try JSONL extraction for more accurate result
                                let jsonl_resp = match state.mission.db().get_slot_session(slot_id.as_str()) {
                                    Ok(Some(session_uuid)) => {
                                        match state.mission.db().get_conversation(&session_uuid) {
                                            Ok(Some(conv)) => {
                                                if let Some(ref jsonl_path) = conv.jsonl_path {
                                                    missiond_core::extract_last_assistant_text(std::path::Path::new(jsonl_path)).await
                                                } else { None }
                                            }
                                            _ => None,
                                        }
                                    }
                                    _ => None,
                                };
                                // Check JSONL turn_duration for fast tasks that complete under the guard
                                let jsonl_confirmed = if let Some(jsonl_path) = get_task_jsonl_path(&state, &missiond_core::types::Task {
                                    id: String::new(), role: String::new(), prompt: String::new(),
                                    status: missiond_core::types::TaskStatus::Running,
                                    slot_id: Some(slot_id.to_string()), session_id: None,
                                    result: None, error: None, created_at: 0, started_at: None, finished_at: None,
                                }) {
                                    missiond_core::jsonl_has_completed_turn(std::path::Path::new(&jsonl_path)).await
                                } else { false };
                                for task in &running_tasks {
                                    if task.slot_id.as_deref() == Some(slot_id.as_str()) {
                                        let started = task.started_at.unwrap_or(task.created_at);
                                        let elapsed = now - started;
                                        // Guard: prevent stale JSONL turn confirmation from closing tasks prematurely.
                                        // Even with JSONL confirmation, require 3s minimum to ensure the slot
                                        // has actually started processing THIS task (not a previous extraction).
                                        if elapsed < MIN_JSONL_EXECUTION_MS {
                                            debug!(
                                                task_id = %task.id, slot_id = %slot_id, elapsed_ms = elapsed,
                                                "Submit task NOT closed: too short even for JSONL ({elapsed}ms < {MIN_JSONL_EXECUTION_MS}ms)"
                                            );
                                            continue;
                                        }
                                        if elapsed < MIN_EXECUTION_MS && !jsonl_confirmed {
                                            debug!(
                                                task_id = %task.id, slot_id = %slot_id, elapsed_ms = elapsed,
                                                "Submit task NOT closed: execution too short ({elapsed}ms < {MIN_EXECUTION_MS}ms) and no JSONL confirmation"
                                            );
                                            continue;
                                        }
                                        // Prefer JSONL result > PTY result > default
                                        let result_text = jsonl_resp.clone()
                                            .or_else(|| {
                                                if pty_resp.is_some() {
                                                    warn!(task_id = %task.id, "JSONL result unavailable, falling back to PTY");
                                                }
                                                pty_resp.clone()
                                            })
                                            .unwrap_or_else(|| "completed".to_string());
                                        // Safe UTF-8 truncation to 4KB
                                        let result_text = if result_text.len() > 4096 {
                                            let mut end = 4096;
                                            while !result_text.is_char_boundary(end) && end > 0 { end -= 1; }
                                            format!("{}...(truncated)", &result_text[..end])
                                        } else {
                                            result_text
                                        };
                                        let _ = state.mission.db().update_task(
                                            &task.id,
                                            &missiond_core::types::TaskUpdate {
                                                status: Some(missiond_core::types::TaskStatus::Done),
                                                finished_at: Some(now),
                                                result: Some(result_text.clone()),
                                                ..Default::default()
                                            },
                                        );
                                        // Update associated kb_operation if this task was dispatched from kb_execute_plan
                                        if let Ok(true) = state.mission.db().kb_ops_complete_by_task_id(&task.id, "done", Some(&result_text)) {
                                            info!(task_id = %task.id, "KB operation marked done via task completion");
                                        }
                                        info!(task_id = %task.id, slot_id = %slot_id, elapsed_ms = elapsed,
                                            jsonl_result = jsonl_resp.is_some(),
                                            "Submit task closed: slot returned to Idle");
                                        // Signal unified scheduler to dispatch next queued task
                                        state.submit_notify.notify_one();
                                    }
                                }
                            }
                            // Always signal submit dispatcher when any slot becomes Idle,
                            // even if no running task was closed (e.g., after memory extraction).
                            // This ensures queued submit tasks get dispatched promptly.
                            state.submit_notify.notify_one();
                        }
                    }
                    Ok(_) => {} // Other PTY events not needed for logging
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "PTY logger lagged");
                    }
                    Err(_) => {}
                }
            }
        }
    }
}
