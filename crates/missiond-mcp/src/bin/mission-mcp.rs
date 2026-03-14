//! mission-mcp - MCP stdio server proxy for missiond daemon
//!
//! This binary is intended to be launched by Claude Code as an MCP server.
//! It forwards tool calls to a singleton `missiond` daemon over IPC.
//! - Unix: Uses Unix domain sockets
//! - Windows: Uses TCP loopback

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use missiond_core::ipc::{self, IpcStream};
use missiond_mcp::protocol::{Request, RequestId, Response, JSONRPC_VERSION};
use missiond_mcp::server::{McpServer, ToolHandler};
use missiond_mcp::tools::ToolResult;

static NEXT_ID: AtomicI64 = AtomicI64::new(1);

/// Session ID for this MCP proxy process (one per Claude Code session).
/// Sourced from CLAUDE_SESSION_ID env var, falling back to pid-based ID.
fn get_session_id() -> String {
    std::env::var("CLAUDE_SESSION_ID")
        .or_else(|_| std::env::var("SESSION_ID"))
        .unwrap_or_else(|_| format!("mcp-{}", std::process::id()))
}

#[derive(Clone)]
struct IpcClient {
    endpoint: String,
    session_id: String,
}

impl IpcClient {
    /// Send a raw IPC request and return the result value
    async fn call_method(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let request = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.to_string(),
            params,
            id: Some(RequestId::Number(id)),
        };

        let mut stream = IpcStream::connect(&self.endpoint)
            .await
            .with_context(|| format!("Failed to connect to daemon: {}", self.endpoint))?;

        let request_json = serde_json::to_string(&request)?;
        debug!(%method, "IPC -> {}", request_json);
        stream.write_all(request_json.as_bytes()).await?;
        stream.write_all(b"\n").await?;
        stream.flush().await?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            return Err(anyhow!("Daemon closed connection without response"));
        }
        let line = line.trim();
        debug!(%method, "IPC <- {}", line);

        let response: Response = serde_json::from_str(line)?;
        if let Some(err) = response.error {
            return Err(anyhow!("IPC error: {}", err.message));
        }
        response.result.ok_or_else(|| anyhow!("Missing result"))
    }

    async fn call_tool(&self, name: &str, arguments: Value) -> Result<ToolResult> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

        let request = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": name,
                "arguments": arguments,
                "_meta": { "session_id": self.session_id },
            })),
            id: Some(RequestId::Number(id)),
        };

        let mut stream = IpcStream::connect(&self.endpoint)
            .await
            .with_context(|| format!("Failed to connect to daemon: {}", self.endpoint))?;

        let request_json = serde_json::to_string(&request)?;
        debug!(%name, "IPC -> {}", request_json);
        stream.write_all(request_json.as_bytes()).await?;
        stream.write_all(b"\n").await?;
        stream.flush().await?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            return Err(anyhow!("Daemon closed connection without response"));
        }
        let line = line.trim();
        debug!(%name, "IPC <- {}", line);

        let response: Response = serde_json::from_str(line)?;
        if let Some(err) = response.error {
            return Ok(ToolResult::error(err.message));
        }

        let result = response
            .result
            .ok_or_else(|| anyhow!("Missing result in daemon response"))?;
        let tool_result: ToolResult = serde_json::from_value(result)?;
        Ok(tool_result)
    }
}

struct ProxyHandler {
    client: IpcClient,
}

/// Classify whether an IPC error is transient (worth retrying) or deterministic.
/// Transient: connection refused, socket gone, broken pipe, daemon restart.
/// Deterministic: IPC protocol errors, deserialization failures, daemon-side errors.
///
/// NOTE: Post-connect failures (broken pipe, connection reset) mean the request
/// may have been sent to daemon. Retrying non-idempotent tools could cause
/// duplicate execution. Accepted risk for v1 — most MCP tools are read-heavy.
fn is_transient_error(err: &anyhow::Error) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("failed to connect")
        || msg.contains("connection refused")
        || msg.contains("broken pipe")
        || msg.contains("connection reset")
        || msg.contains("daemon closed connection")
        || msg.contains("no such file or directory")
        || msg.contains("timed out")
        || msg.contains("unexpected eof")
}

#[async_trait::async_trait]
impl ToolHandler for ProxyHandler {
    async fn call(&self, name: &str, arguments: Value) -> ToolResult {
        const MAX_RETRIES: u32 = 2;
        const BASE_DELAY_MS: u64 = 200;

        let mut last_error = String::new();
        for attempt in 0..=MAX_RETRIES {
            match self.client.call_tool(name, arguments.clone()).await {
                Ok(res) => return res,
                Err(e) => {
                    if attempt < MAX_RETRIES && is_transient_error(&e) {
                        let delay = BASE_DELAY_MS * (1 << attempt);
                        warn!(tool = %name, attempt, delay_ms = delay, error = %e,
                              "IPC transient error, retrying");
                        sleep(Duration::from_millis(delay)).await;
                        last_error = e.to_string();
                        continue;
                    }
                    error!(tool = %name, attempt, error = %e, "IPC tool call failed");
                    return ToolResult::error(e.to_string());
                }
            }
        }
        error!(tool = %name, retries = MAX_RETRIES, "IPC tool call exhausted retries");
        ToolResult::error(format!("IPC connection failed after retries: {}", last_error))
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
        "warn".to_string()
    };

    tracing_subscriber::EnvFilter::try_new(level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"))
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

fn missiond_binary_path() -> PathBuf {
    #[cfg(windows)]
    const BINARY_NAME: &str = "missiond.exe";
    #[cfg(not(windows))]
    const BINARY_NAME: &str = "missiond";

    // Prefer a sibling binary next to the current executable (dev / cargo build).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(BINARY_NAME);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from(BINARY_NAME)
}

fn spawn_daemon() -> Result<()> {
    let bin = missiond_binary_path();
    let mut cmd = std::process::Command::new(&bin);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .envs(std::env::vars());

    cmd.spawn()
        .with_context(|| format!("Failed to spawn daemon: {}", bin.display()))?;
    Ok(())
}

async fn ensure_daemon(endpoint: &str) -> Result<()> {
    if IpcStream::can_connect(endpoint).await {
        return Ok(());
    }

    warn!(
        endpoint = %endpoint,
        "Daemon not reachable, starting daemon"
    );
    spawn_daemon()?;

    // Wait for daemon to come up
    for _ in 0..50 {
        if IpcStream::can_connect(endpoint).await {
            info!("Daemon is ready");
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }

    Err(anyhow!(
        "Timed out waiting for daemon: {}",
        endpoint
    ))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(log_filter())
        .with_writer(std::io::stderr)
        .init();

    let endpoint = ipc_endpoint_from_env();
    ensure_daemon(&endpoint).await?;

    let session_id = get_session_id();
    let client = IpcClient { endpoint, session_id };

    // Fetch KB summary from daemon for instructions injection
    let instructions = match client.call_method("kb/summary", None).await {
        Ok(val) => val.get("instructions").and_then(|v| v.as_str()).map(String::from),
        Err(e) => {
            warn!("Failed to fetch KB summary: {}", e);
            None
        }
    };

    let handler = ProxyHandler { client };
    let mut server = McpServer::new(handler);
    if let Some(instructions) = instructions {
        if !instructions.is_empty() {
            server.set_instructions(instructions);
        }
    }
    server.run().await?;
    Ok(())
}
