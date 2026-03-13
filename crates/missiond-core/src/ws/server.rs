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
    CCSession, CCTask, CCTaskChangeEvent, CCTasksOverview, CCTasksWatcher, WatcherEvent,
};
use crate::pty::{PTYManager, SessionEvent, SessionState, TextOutputEvent};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
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

/// Callback type for context enrichment before sending to PTY.
/// Implemented by daemon to inject KB/Skill/Code context into Jarvis messages.
/// Takes user query, returns enriched context string (empty = no context).
pub type ContextEnricherFn = Arc<
    dyn Fn(String) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send>>
        + Send
        + Sync,
>;

/// Late-bound container for context enricher (set after AppState is constructed).
pub type ContextEnricherSlot = Arc<tokio::sync::RwLock<Option<ContextEnricherFn>>>;

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
    /// Frontend event stream (pre-serialized JSON from daemon EventBus)
    pub frontend_events_tx: Option<broadcast::Sender<String>>,
    /// Database for timeline catch-up queries (Phase 6)
    pub db: Option<Arc<crate::db::MissionDB>>,
    /// Context enricher for Jarvis chat completions (late-bound by daemon)
    pub context_enricher: ContextEnricherSlot,
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
    frontend_events_tx: Option<broadcast::Sender<String>>,
    db: Option<Arc<crate::db::MissionDB>>,
    context_enricher: ContextEnricherSlot,
}

// ── AIOps Webhook Parsers ──

/// Clean Jarvis PTY response using UUID boundary marker.
///
/// The injected message has format: `{context}\n\n{user_message}\n<<<BOUNDARY_{id}>>>`
/// PTY echoes back the entire input, then Claude's response follows.
/// We find the LAST occurrence of the boundary marker and take everything after it.
fn clean_jarvis_response(raw: &str, boundary_id: &str) -> String {
    let marker = format!("<<<BOUNDARY_{}>>>", boundary_id);

    // 1. Find boundary marker — take everything after the last occurrence
    let text = if let Some(pos) = raw.rfind(&marker) {
        &raw[pos + marker.len()..]
    } else {
        // Fallback: boundary not found (PTY buffer overflow or truncation).
        // Try stripping from last ⏺ block as best-effort.
        tracing::warn!(boundary_id, "Boundary marker not found in PTY output, using fallback");
        raw
    };

    // 2. Strip Claude Code TUI bullet markers (⏺)
    let cleaned: String = text
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if let Some(after) = trimmed.strip_prefix('⏺') {
                after.strip_prefix(' ').unwrap_or(after)
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    // 3. Trim whitespace
    cleaned.trim().to_string()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route<'a> {
    Pty { slot_id: &'a str },
    Tasks,
    Events,
    Invalid,
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
    ws_tx: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<TcpStream>, Message>,
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
    Screen { data: String },
    Data { data: String },
    State {
        state: SessionState,
        #[serde(rename = "prevState")]
        prev_state: SessionState,
        #[serde(rename = "statusText", skip_serializing_if = "Option::is_none")]
        status_text: Option<String>,
    },
    Exit { code: i32 },
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
    Input { data: String },
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
            frontend_events_tx: options.frontend_events_tx,
            db: options.db,
            context_enricher: Arc::clone(&options.context_enricher),
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

    /// Start the server
    pub async fn start(&mut self) -> anyhow::Result<()> {
        let bind_addr = std::env::var("MISSION_WS_BIND")
            .unwrap_or_else(|_| "0.0.0.0".to_string());
        let addr = format!("{}:{}", bind_addr, self.port);
        let listener = TcpListener::bind(&addr).await?;

        info!(port = self.port, bind = %bind_addr, "PTY WebSocket server started");

        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx.clone());

        let pty_manager = self.pty_manager.clone();
        let cc_tasks_watcher = self.cc_tasks_watcher.clone();
        let screenshot_broker = self.screenshot_broker.clone();
        let jarvis_trace = self.jarvis_trace.clone();
        let incident_tx = self.incident_tx.clone();
        let frontend_events_tx = self.frontend_events_tx.clone();
        let db = self.db.clone();
        let context_enricher = self.context_enricher.clone();

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
                                let frontend_events_tx = frontend_events_tx.clone();
                                let db = db.clone();
                                let context_enricher = context_enricher.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = Self::handle_connection(stream, addr, pty_manager, cc_tasks_watcher, screenshot_broker, jarvis_trace, incident_tx, frontend_events_tx, db, context_enricher).await {
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

    /// Send an HTTP error response
    /// Extract text + images from OpenAI multimodal content array.
    /// Images (base64 data URLs) are saved to temp files; local paths are injected into the prompt.
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
                    debug!(part_type, "Multimodal: unsupported content part type, skipping");
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
    async fn save_data_url_to_file(
        url: &str,
        media_dir: &std::path::Path,
    ) -> Option<String> {
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
            status, reason, body.len(), body
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
            if let Some(pos) = buf
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
            {
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
                    lower.trim_start_matches("content-length:").trim().parse().ok()
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

        let body = String::from_utf8_lossy(&body_buf[..content_length.min(body_buf.len())]).to_string();
        Ok((headers_str, body))
    }

    /// Handle POST /webhooks/* — AIOps incident webhook receiver
    async fn handle_webhook(
        mut stream: TcpStream,
        request_line: &str,
        incident_tx: Option<tokio::sync::mpsc::Sender<crate::types::MissionIncident>>,
    ) -> anyhow::Result<()> {
        let tx = match incident_tx {
            Some(tx) => tx,
            None => {
                Self::send_http_error(&mut stream, 503, "Service Unavailable", r#"{"error":"incident bus not configured"}"#).await?;
                return Ok(());
            }
        };

        let (_headers, body) = Self::read_http_request(&mut stream).await?;

        // Extract path from request line (e.g. "POST /webhooks/deploy HTTP/1.1")
        let path = request_line.split_whitespace().nth(1).unwrap_or("");

        let incident = match path {
            "/webhooks/deploy" => parse_deploy_webhook(&body),
            "/webhooks/test" => parse_test_webhook(&body),
            _ => {
                Self::send_http_error(&mut stream, 404, "Not Found", r#"{"error":"unknown webhook path"}"#).await?;
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
                    resp_body.len(), resp_body
                );
                stream.write_all(response.as_bytes()).await?;
                stream.shutdown().await?;
            }
            None => {
                // Non-alert event (e.g. deploy success) → silent 200
                let resp_body = r#"{"ok":true,"action":"ignored"}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    resp_body.len(), resp_body
                );
                stream.write_all(response.as_bytes()).await?;
                stream.shutdown().await?;
            }
        }

        Ok(())
    }

    /// Handle POST /v1/chat/completions — OpenAI-compatible SSE endpoint
    async fn handle_chat_completions(
        mut stream: TcpStream,
        addr: SocketAddr,
        pty_manager: Arc<PTYManager>,
        trace_store: JarvisTraceStore,
        context_enricher: ContextEnricherSlot,
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
        let auth_token = headers
            .lines()
            .find_map(|line| {
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
                        Self::send_http_error(&mut stream, 401, "Unauthorized", &err.to_string()).await?;
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

        let messages = req.get("messages").and_then(|m| m.as_array());
        if messages.is_none() || messages.unwrap().is_empty() {
            let err = serde_json::json!({"error": {"message": "messages array is required"}});
            Self::send_http_error(&mut stream, 400, "Bad Request", &err.to_string()).await?;
            return Ok(());
        }
        let messages = messages.unwrap();

        // Extract the last user message (supports both string and multimodal array content)
        let last_user_msg = messages
            .iter()
            .rev()
            .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"));

        let user_message = match last_user_msg {
            Some(msg) => {
                match msg.get("content") {
                    Some(serde_json::Value::String(text)) => text.clone(),
                    Some(serde_json::Value::Array(parts)) => {
                        // Multimodal content: extract text + save images to temp files
                        Self::extract_multimodal_content(parts).await
                    }
                    _ => String::new(),
                }
            }
            None => String::new(),
        };

        if user_message.is_empty() {
            let err = serde_json::json!({"error": {"message": "No user message found"}});
            Self::send_http_error(&mut stream, 400, "Bad Request", &err.to_string()).await?;
            return Ok(());
        }

        // Slot selection: X-Slot-Id header > default "slot-jarvis"
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
            .unwrap_or_else(|| "slot-jarvis".to_string());
        let chat_id = format!("chatcmpl-{}-{}", &slot_id, chrono::Utc::now().timestamp_millis());
        let boundary_id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();

        // Extract Router's trace_id from X-Trace-Id header
        let router_trace_id = headers
            .lines()
            .find_map(|line| {
                let lower = line.to_lowercase();
                if lower.starts_with("x-trace-id:") {
                    Some(line.splitn(2, ':').nth(1)?.trim().to_string())
                } else {
                    None
                }
            });

        info!(?addr, slot_id, msg_len = user_message.len(), trace_id = %chat_id, "Chat completions request");

        // Check slot status
        let status = pty_manager.get_status(&slot_id).await;
        let state = status.as_ref().map(|s| s.state.clone());

        match &state {
            None | Some(SessionState::Exited) => {
                let error_msg = format!("Slot {} not running.", slot_id);
                trace_store.unavailable_trace(
                    chat_id, addr, &slot_id, &user_message, &error_msg, router_trace_id,
                ).await;
                let err = serde_json::json!({"error": {"message": &error_msg}});
                Self::send_http_error(&mut stream, 503, "Service Unavailable", &err.to_string()).await?;
                return Ok(());
            }
            Some(s) if *s != SessionState::Idle => {
                let error_msg = format!("{} is busy (state: {:?}). Try again later.", slot_id, s);
                trace_store.unavailable_trace(
                    chat_id, addr, &slot_id, &user_message, &error_msg, router_trace_id,
                ).await;
                let err = serde_json::json!({"error": {"message": &error_msg}, "retry_after": 5});
                let response = format!(
                    "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nRetry-After: 5\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    err.to_string().len(), err
                );
                stream.write_all(response.as_bytes()).await?;
                stream.shutdown().await?;
                return Ok(());
            }
            _ => {} // Idle — good to go
        }

        // Start trace
        trace_store.start_trace(
            chat_id.clone(), addr, &slot_id, &user_message, router_trace_id,
        ).await;

        // Write SSE response headers immediately — flush for curl to see
        let sse_headers = "HTTP/1.1 200 OK\r\n\
            Content-Type: text/event-stream\r\n\
            Cache-Control: no-cache\r\n\
            Connection: keep-alive\r\n\
            Access-Control-Allow-Origin: *\r\n\
            \r\n";
        stream.write_all(sse_headers.as_bytes()).await?;
        stream.flush().await?;

        // Context enrichment: inject KB/Skill/Code context before sending to PTY
        let enriched_message = {
            let enricher_guard = context_enricher.read().await;
            if let Some(ref enricher) = *enricher_guard {
                let enricher = Arc::clone(enricher);
                drop(enricher_guard); // release lock before async call
                let ctx = enricher(user_message.clone()).await;
                if ctx.is_empty() {
                    format!("{}\n<<<BOUNDARY_{}>>>", user_message, boundary_id)
                } else {
                    debug!(slot_id, ctx_len = ctx.len(), "Jarvis: context enrichment injected");
                    format!("{}\n\n{}\n<<<BOUNDARY_{}>>>", ctx, user_message, boundary_id)
                }
            } else {
                user_message.clone()
            }
        };

        // Dual-track streaming:
        // 1. Subscribe to session events → forward status/tool events as SSE (activity view)
        // 2. Use blocking send() → get clean final response (handles turn management correctly)
        //
        // send_fire_and_forget causes spurious Complete events from paste notification.
        // send() properly waits for the real Complete, so we use it for the response.
        let mut rx = match pty_manager.subscribe_session(&slot_id).await {
            Ok(rx) => rx,
            Err(e) => {
                let err = serde_json::json!({"error": {"message": format!("Subscribe failed: {}", e)}});
                let event = format!("data: {}\n\n", err);
                let _ = stream.write_all(event.as_bytes()).await;
                let _ = stream.write_all(b"data: [DONE]\n\n").await;
                stream.shutdown().await?;
                return Ok(());
            }
        };

        // Spawn blocking send() in a background task — it returns the final clean response.
        let pty_for_send = Arc::clone(&pty_manager);
        let send_msg = enriched_message.clone();
        let send_slot = slot_id.to_string();
        let send_handle = tokio::spawn(async move {
            let timeout_ms = 300_000u64; // 5 min
            pty_for_send.send(&send_slot, &send_msg, timeout_ms).await
        });

        // Forward activity events via SSE while send() is running
        let start_time = std::time::Instant::now();
        let heartbeat_interval = tokio::time::Duration::from_secs(15);
        let mut tool_seq: u32 = 0;
        let mut last_event_time = std::time::Instant::now();
        let mut last_status_phase = String::new();
        let mut last_status_sent = std::time::Instant::now() - std::time::Duration::from_secs(1); // Allow first status immediately
        let status_throttle = std::time::Duration::from_millis(500);

        loop {
            // Check if send() has completed
            if send_handle.is_finished() {
                break;
            }

            // Poll for events with heartbeat timeout
            let recv_timeout = heartbeat_interval.saturating_sub(last_event_time.elapsed());
            match tokio::time::timeout(recv_timeout, rx.recv()).await {
                Ok(Ok(event)) => {
                    last_event_time = std::time::Instant::now();
                    match event {
                        SessionEvent::StatusUpdate(status) => {
                            let phase = format!("{}", status.phase);
                            let phase_changed = phase != last_status_phase;
                            let throttle_elapsed = last_status_sent.elapsed() >= status_throttle;
                            // Send immediately on phase change, or throttle within same phase
                            if phase_changed || throttle_elapsed {
                                last_status_phase = phase.clone();
                                last_status_sent = std::time::Instant::now();
                                let evt = serde_json::json!({
                                    "phase": phase,
                                    "text": status.status_text,
                                });
                                let sse = format!("event: status\ndata: {}\n\n", evt);
                                let _ = stream.write_all(sse.as_bytes()).await;
                                let _ = stream.flush().await;
                            }
                        }
                        SessionEvent::ToolOutput(tool_output) => {
                            use crate::semantic::ToolStatus;
                            match tool_output.status {
                                ToolStatus::Running => {
                                    tool_seq += 1;
                                    let id = format!("t{}", tool_seq);
                                    let param_summary = tool_output.params.values().next()
                                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                                        .unwrap_or_default();
                                    let evt = serde_json::json!({
                                        "id": id,
                                        "tool": tool_output.tool_name,
                                        "params": param_summary,
                                    });
                                    let sse = format!("event: tool_start\ndata: {}\n\n", evt);
                                    let _ = stream.write_all(sse.as_bytes()).await;
                                    let _ = stream.flush().await;
                                }
                                ToolStatus::Completed => {
                                    let id = format!("t{}", tool_seq);
                                    let evt = serde_json::json!({
                                        "id": id,
                                        "tool": tool_output.tool_name,
                                        "duration_ms": tool_output.duration_ms,
                                    });
                                    let sse = format!("event: tool_end\ndata: {}\n\n", evt);
                                    let _ = stream.write_all(sse.as_bytes()).await;
                                    let _ = stream.flush().await;
                                }
                            }
                        }
                        SessionEvent::StateChange { new_state, .. } => {
                            match new_state {
                                SessionState::Thinking => {
                                    let evt = serde_json::json!({"phase": "thinking", "text": "Thinking..."});
                                    let sse = format!("event: status\ndata: {}\n\n", evt);
                                    let _ = stream.write_all(sse.as_bytes()).await;
                                    let _ = stream.flush().await;
                                }
                                SessionState::ToolRunning => {
                                    let evt = serde_json::json!({"phase": "tool_running", "text": ""});
                                    let sse = format!("event: status\ndata: {}\n\n", evt);
                                    let _ = stream.write_all(sse.as_bytes()).await;
                                    let _ = stream.flush().await;
                                }
                                _ => {}
                            }
                        }
                        SessionEvent::Exit(code) => {
                            let err_msg = format!("PTY session exited with code {}", code);
                            trace_store.error_trace(&chat_id, &err_msg, None).await;
                            let err = serde_json::json!({"error": {"message": err_msg}});
                            let _ = stream.write_all(format!("data: {}\n\n", err).as_bytes()).await;
                            let _ = stream.write_all(b"data: [DONE]\n\n").await;
                            warn!(?addr, slot_id, code, trace_id = %chat_id, "PTY exited during streaming");
                            let _ = stream.shutdown().await;
                            return Ok(());
                        }
                        _ => {} // Ignore Data, ScreenText, TitleChange, TextOutput::Stream/Complete
                    }
                }
                Ok(Err(broadcast::error::RecvError::Lagged(n))) => {
                    warn!(slot_id, lagged = n, "Broadcast lagged, continuing");
                }
                Ok(Err(broadcast::error::RecvError::Closed)) => {
                    break;
                }
                Err(_) => {
                    // Heartbeat — send SSE comment to keep connection alive
                    let _ = stream.write_all(b":\n\n").await;
                    let _ = stream.flush().await;
                    last_event_time = std::time::Instant::now();
                }
            }
        }

        // send() has completed — get the result and emit final response
        match send_handle.await {
            Ok(Ok(result)) => {
                let duration_ms = start_time.elapsed().as_millis() as u64;
                trace_store.complete_trace(&chat_id, &result.response, duration_ms).await;

                if !result.response.is_empty() {
                    let cleaned = clean_jarvis_response(&result.response, &boundary_id);
                    let chunk = serde_json::json!({
                        "id": &chat_id,
                        "object": "chat.completion.chunk",
                        "model": "jarvis-missiond",
                        "choices": [{
                            "index": 0,
                            "delta": { "content": cleaned },
                            "finish_reason": serde_json::Value::Null,
                        }]
                    });
                    let _ = stream.write_all(format!("data: {}\n\n", chunk).as_bytes()).await;
                }
                let stop = serde_json::json!({
                    "id": &chat_id,
                    "object": "chat.completion.chunk",
                    "model": "jarvis-missiond",
                    "choices": [{
                        "index": 0,
                        "delta": {},
                        "finish_reason": "stop",
                    }]
                });
                let _ = stream.write_all(format!("data: {}\n\n", stop).as_bytes()).await;
                let _ = stream.write_all(b"data: [DONE]\n\n").await;
                info!(?addr, slot_id, response_len = result.response.len(), duration_ms, trace_id = %chat_id, "Chat completions done (streaming)");
            }
            Ok(Err(e)) => {
                trace_store.error_trace(&chat_id, &e.to_string(), None).await;
                let err = serde_json::json!({"error": {"message": format!("Claude Code error: {}", e)}});
                let _ = stream.write_all(format!("data: {}\n\n", err).as_bytes()).await;
                let _ = stream.write_all(b"data: [DONE]\n\n").await;
                warn!(?addr, slot_id, error = %e, trace_id = %chat_id, "Chat completions error");
            }
            Err(e) => {
                trace_store.error_trace(&chat_id, &e.to_string(), None).await;
                let err = serde_json::json!({"error": {"message": format!("Internal error: {}", e)}});
                let _ = stream.write_all(format!("data: {}\n\n", err).as_bytes()).await;
                let _ = stream.write_all(b"data: [DONE]\n\n").await;
            }
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
        jarvis_trace: JarvisTraceStore,
        incident_tx: Option<tokio::sync::mpsc::Sender<crate::types::MissionIncident>>,
        frontend_events_tx: Option<broadcast::Sender<String>>,
        db: Option<Arc<crate::db::MissionDB>>,
        context_enricher: ContextEnricherSlot,
    ) -> anyhow::Result<()> {
        // Peek at first bytes to detect non-WebSocket HTTP requests
        let mut peek_buf = [0u8; 512];
        let n = stream.peek(&mut peek_buf).await.unwrap_or(0);
        if n > 0 {
            let request_line = String::from_utf8_lossy(&peek_buf[..n]);
            // Health check
            if request_line.starts_with("GET /health") && !request_line.contains("Upgrade:") {
                return Self::handle_health(stream).await;
            }
            // AIOps webhook endpoint
            if request_line.starts_with("POST /webhooks/") {
                return Self::handle_webhook(stream, &request_line, incident_tx).await;
            }
            // Chat completions SSE endpoint
            if request_line.starts_with("POST /v1/chat/completions") {
                return match pty_manager {
                    Some(pm) => Self::handle_chat_completions(stream, addr, pm, jarvis_trace, context_enricher).await,
                    None => {
                        let mut s = stream;
                        let err = serde_json::json!({"error": {"message": "PTY manager not available"}});
                        Self::send_http_error(&mut s, 503, "Service Unavailable", &err.to_string()).await
                    }
                };
            }
            // CORS preflight for chat completions
            if request_line.starts_with("OPTIONS /v1/chat/completions") {
                let mut s = stream;
                let response = "HTTP/1.1 204 No Content\r\n\
                    Access-Control-Allow-Origin: *\r\n\
                    Access-Control-Allow-Methods: POST, OPTIONS\r\n\
                    Access-Control-Allow-Headers: Content-Type, Authorization\r\n\
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

        match parse_route(&path) {
            Route::Tasks => Self::handle_tasks_subscription(addr, ws_stream, cc_tasks_watcher).await,
            Route::Events => Self::handle_events_subscription(addr, ws_stream, frontend_events_tx, db).await,
            Route::Pty { slot_id } => {
                Self::handle_pty_subscription(addr, ws_stream, pty_manager, screenshot_broker, slot_id).await
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
                        Err(_) => break,
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
        db: Option<Arc<crate::db::MissionDB>>,
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

        info!(?addr, "Client subscribing to EventBus stream");

        // Send connected message with latest seq from DB (for catch-up protocol)
        let latest_seq = db.as_ref()
            .and_then(|d| d.timeline_latest_seq().ok())
            .unwrap_or(0);
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

        loop {
            tokio::select! {
                event = rx.recv() => {
                    match event {
                        Ok(json_str) => {
                            consecutive_lags = 0;
                            if ws_tx.send(Message::Text(json_str)).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            consecutive_lags += 1;
                            let resync = serde_json::json!({
                                "type": "resync",
                                "ts": chrono::Utc::now().timestamp_millis(),
                                "missed": n,
                            });
                            let _ = send_json(&mut ws_tx, &resync).await;
                            if consecutive_lags >= 3 {
                                warn!(?addr, "Events client too slow, disconnecting");
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
        ws_tx: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<TcpStream>, Message>,
        db: &Arc<crate::db::MissionDB>,
        since_seq: i64,
    ) {
        let latest = db.timeline_latest_seq().unwrap_or(0);
        let gap = latest - since_seq;

        if gap > 1000 {
            // Too far behind — client should do full HTTP refresh
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

        // Replay historical events from SQLite
        let db_clone = Arc::clone(db);
        let rows = tokio::task::spawn_blocking(move || {
            db_clone.query_timeline_since(since_seq, 1000)
        }).await;

        match rows {
            Ok(Ok(rows)) => {
                for row in &rows {
                    // Reconstruct the wire-format JSON from TimelineRow
                    let payload: serde_json::Value = serde_json::from_str(&row.payload)
                        .unwrap_or(serde_json::json!({}));
                    // Parse SQLite datetime "YYYY-MM-DD HH:MM:SS" as UTC millis
                    let ts = chrono::NaiveDateTime::parse_from_str(&row.created_at, "%Y-%m-%d %H:%M:%S")
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
            _ => {
                warn!("Timeline catch-up query failed");
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
