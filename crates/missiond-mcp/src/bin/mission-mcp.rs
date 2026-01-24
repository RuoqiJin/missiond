//! mission-mcp - MCP stdio server proxy for missiond daemon
//!
//! This binary is intended to be launched by Claude Code as an MCP server.
//! It forwards tool calls to a singleton `missiond` daemon over a Unix socket.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use missiond_mcp::protocol::{Request, RequestId, Response, JSONRPC_VERSION};
use missiond_mcp::server::{McpServer, ToolHandler};
use missiond_mcp::tools::ToolResult;

static NEXT_ID: AtomicI64 = AtomicI64::new(1);

#[derive(Clone)]
struct IpcClient {
    socket_path: PathBuf,
}

impl IpcClient {
    async fn call_tool(&self, name: &str, arguments: Value) -> Result<ToolResult> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

        let request = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": name,
                "arguments": arguments,
            })),
            id: RequestId::Number(id),
        };

        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .with_context(|| format!("Failed to connect to daemon socket: {}", self.socket_path.display()))?;

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

#[async_trait::async_trait]
impl ToolHandler for ProxyHandler {
    async fn call(&self, name: &str, arguments: Value) -> ToolResult {
        match self.client.call_tool(name, arguments).await {
            Ok(res) => res,
            Err(e) => {
                error!(tool = %name, error = %e, "IPC tool call failed");
                ToolResult::error(e.to_string())
            }
        }
    }
}

fn default_mission_home() -> PathBuf {
    if let Ok(home) = std::env::var("XJP_MISSION_HOME") {
        return PathBuf::from(home);
    }
    dirs::home_dir()
        .map(|h| h.join(".xjp-mission"))
        .unwrap_or_else(|| PathBuf::from(".xjp-mission"))
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

fn socket_path_from_env() -> PathBuf {
    if let Ok(sock) = std::env::var("MISSION_IPC_SOCKET") {
        return PathBuf::from(sock);
    }
    default_mission_home().join("missiond.sock")
}

fn missiond_binary_path() -> PathBuf {
    // Prefer a sibling `missiond` binary next to the current executable (dev / cargo build).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("missiond");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from("missiond")
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

async fn ensure_daemon(socket_path: &Path) -> Result<()> {
    if UnixStream::connect(socket_path).await.is_ok() {
        return Ok(());
    }

    warn!(
        socket = %socket_path.display(),
        "Daemon socket not reachable, starting daemon"
    );
    spawn_daemon()?;

    // Wait for daemon to come up
    for _ in 0..50 {
        if UnixStream::connect(socket_path).await.is_ok() {
            info!("Daemon is ready");
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }

    Err(anyhow!(
        "Timed out waiting for daemon socket: {}",
        socket_path.display()
    ))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(log_filter())
        .with_writer(std::io::stderr)
        .init();

    let socket_path = socket_path_from_env();
    ensure_daemon(&socket_path).await?;

    let handler = ProxyHandler {
        client: IpcClient { socket_path },
    };
    let mut server = McpServer::new(handler);
    server.run().await?;
    Ok(())
}
