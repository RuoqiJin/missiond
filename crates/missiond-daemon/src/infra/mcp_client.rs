use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info, warn};

/// Persistent MCP client for xjp-mcp server (stdio-based, lazy-initialized).
/// Uses Arc-wrapped shared state so the outer lock can be released before awaiting responses.
pub(crate) struct McpProcessClient {
    state: tokio::sync::Mutex<Option<McpClientState>>,
    config_path: PathBuf,
}

#[derive(Clone)]
pub(crate) struct McpClientState {
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
pub(crate) const MCP_MAX_USES: u64 = 200;

impl McpProcessClient {
    pub(crate) fn new(config_path: PathBuf) -> Self {
        Self {
            state: tokio::sync::Mutex::new(None),
            config_path,
        }
    }

    pub(crate) async fn call_tool(&self, tool_name: &str, tool_args: Value) -> Result<ToolResult> {
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

        client
            .call_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Phase 2: Send request (no outer lock held)
        let id = client
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
            client
                .reader_alive
                .store(false, std::sync::atomic::Ordering::Relaxed);
            return Err(anyhow!("xjp-mcp stdin write failed (Broken Pipe?): {}", e));
        }

        // Phase 3: Wait for response (completely lock-free)
        let resp = tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .map_err(|_| anyhow!("xjp-mcp tool '{}' timed out after 30s", tool_name))?
            .map_err(|_| anyhow!("xjp-mcp response channel closed (process may have died)"))?;

        if let Some(result) = resp.get("result") {
            let tool_result: ToolResult = serde_json::from_value(result.clone())
                .unwrap_or_else(|_| ToolResult::text(result.to_string()));
            Ok(tool_result)
        } else if let Some(error) = resp.get("error") {
            let msg = error
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            let mut res = ToolResult::text(msg.to_string());
            res.is_error = Some(true);
            Ok(res)
        } else {
            Ok(ToolResult::text(format!(
                "Unexpected xjp-mcp response: {}",
                resp
            )))
        }
    }

    async fn spawn(config_path: &Path) -> Result<McpClientState> {
        let config_str = tokio::fs::read_to_string(config_path)
            .await
            .map_err(|e| anyhow!("Failed to read xjp-mcp config: {}", e))?;
        let config: Value = serde_json::from_str(&config_str)?;
        let mcp_config = config
            .get("mcpServers")
            .and_then(|s| s.get("xjp-mcp"))
            .ok_or_else(|| anyhow!("xjp-mcp not found in config"))?;

        let command = mcp_config
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing command"))?;
        let args: Vec<String> = mcp_config
            .get("args")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let env_map: std::collections::HashMap<String, String> = mcp_config
            .get("env")
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
