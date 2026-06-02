//! missiond-cli - shell-native adapter for MissionD daemon tools.
//!
//! The CLI reuses missiond-mcp tool definitions and forwards tool calls to the
//! daemon through the same IPC `tools/call` method used by `mission-mcp`.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use missiond_core::{default_ipc_endpoint, IpcStream};
use missiond_mcp::protocol::{Request, RequestId, Response, JSONRPC_VERSION};
use missiond_mcp::{all_tools, get_tool, ToolDefinition, ToolResult};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::sleep;
use tracing::{debug, info, warn};

static NEXT_ID: AtomicI64 = AtomicI64::new(1);

#[derive(Parser, Debug)]
#[command(name = "missiond-cli")]
#[command(about = "Shell-native MissionD tool adapter")]
#[command(version)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Inspect or call MissionD daemon tools.
    Tools {
        #[command(subcommand)]
        command: ToolsCommand,
    },
}

#[derive(Subcommand, Debug)]
enum ToolsCommand {
    /// List available MissionD tools.
    List {
        /// Emit JSON instead of a compact text list.
        #[arg(long)]
        json: bool,
    },
    /// Print the schema for one tool.
    Schema {
        /// Tool name, for example mission_context_boot.
        tool_name: String,
    },
    /// Call one MissionD tool through daemon IPC.
    Call {
        /// Tool name, for example mission_context_boot.
        tool_name: String,
        /// JSON object passed as the tool arguments.
        #[arg(long)]
        args_json: String,
        /// Emit compact JSON instead of pretty JSON.
        #[arg(long)]
        compact: bool,
    },
}

#[derive(Clone)]
struct IpcClient {
    endpoint: String,
    session_id: String,
}

impl IpcClient {
    async fn call_tool(&self, name: &str, arguments: Value) -> Result<ToolResult> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let request = Request {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": name,
                "arguments": arguments,
                "_meta": { "session_id": self.session_id },
            })),
            id: Some(RequestId::Number(id)),
        };

        let mut stream = IpcStream::connect(&self.endpoint)
            .await
            .with_context(|| format!("failed to connect to daemon: {}", self.endpoint))?;
        let request_json = serde_json::to_string(&request)?;
        debug!(tool = %name, "IPC -> {}", request_json);
        stream.write_all(request_json.as_bytes()).await?;
        stream.write_all(b"\n").await?;
        stream.flush().await?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            return Err(anyhow!("daemon closed connection without response"));
        }

        let response: Response = serde_json::from_str(line.trim())?;
        if let Some(err) = response.error {
            return Ok(ToolResult::error(err.message));
        }

        let result = response
            .result
            .ok_or_else(|| anyhow!("missing result in daemon response"))?;
        Ok(serde_json::from_value(result)?)
    }
}

fn get_session_id() -> String {
    std::env::var("CODEX_SESSION_ID")
        .or_else(|_| std::env::var("SESSION_ID"))
        .unwrap_or_else(|_| format!("cli-{}", std::process::id()))
}

fn ipc_endpoint_from_env() -> String {
    if let Ok(endpoint) = std::env::var("MISSION_IPC_ENDPOINT") {
        return endpoint;
    }
    #[cfg(unix)]
    if let Ok(socket) = std::env::var("MISSION_IPC_SOCKET") {
        return socket;
    }
    default_ipc_endpoint()
}

fn missiond_binary_path() -> PathBuf {
    #[cfg(windows)]
    const BINARY_NAME: &str = "missiond.exe";
    #[cfg(not(windows))]
    const BINARY_NAME: &str = "missiond";

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
    std::process::Command::new(&bin)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .envs(std::env::vars())
        .spawn()
        .with_context(|| format!("failed to spawn daemon: {}", bin.display()))?;
    Ok(())
}

async fn ensure_daemon(endpoint: &str) -> Result<()> {
    if IpcStream::can_connect(endpoint).await {
        return Ok(());
    }

    warn!(endpoint = %endpoint, "daemon not reachable, starting daemon");
    spawn_daemon()?;
    for _ in 0..50 {
        if IpcStream::can_connect(endpoint).await {
            info!("daemon is ready");
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }

    Err(anyhow!("timed out waiting for daemon: {}", endpoint))
}

fn log_filter() -> tracing_subscriber::EnvFilter {
    let level = if let Ok(value) = std::env::var("RUST_LOG") {
        value
    } else if let Ok(value) = std::env::var("MISSION_LOG_LEVEL") {
        match value.as_str() {
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

fn print_json<T: Serialize>(value: &T, compact: bool) -> Result<()> {
    if compact {
        println!("{}", serde_json::to_string(value)?);
    } else {
        println!("{}", serde_json::to_string_pretty(value)?);
    }
    Ok(())
}

fn parse_args_json(text: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(text).context("--args-json must be valid JSON")?;
    if !value.is_object() {
        return Err(anyhow!("--args-json must be a JSON object"));
    }
    Ok(value)
}

fn tool_list() -> Vec<ToolDefinition> {
    let mut tools = all_tools();
    tools.sort_by(|left, right| left.name.cmp(&right.name));
    tools
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(log_filter())
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();
    match args.command {
        Command::Tools { command } => handle_tools(command).await,
    }
}

async fn handle_tools(command: ToolsCommand) -> Result<()> {
    match command {
        ToolsCommand::List { json } => {
            let tools = tool_list();
            if json {
                print_json(&tools, false)?;
            } else {
                for tool in tools {
                    println!("{}\t{}", tool.name, tool.description);
                }
            }
        }
        ToolsCommand::Schema { tool_name } => {
            let tool =
                get_tool(&tool_name).ok_or_else(|| anyhow!("unknown tool: {}", tool_name))?;
            print_json(&tool, false)?;
        }
        ToolsCommand::Call {
            tool_name,
            args_json,
            compact,
        } => {
            if get_tool(&tool_name).is_none() {
                return Err(anyhow!("unknown tool: {}", tool_name));
            }
            let arguments = parse_args_json(&args_json)?;
            let endpoint = ipc_endpoint_from_env();
            ensure_daemon(&endpoint).await?;
            let client = IpcClient {
                endpoint,
                session_id: get_session_id(),
            };
            let result = client.call_tool(&tool_name, arguments).await?;
            print_json(&result, compact)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{LazyLock, Mutex};

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn codex_session_id_precedes_generic_session_id() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("CODEX_SESSION_ID", "codex-session");
        std::env::set_var("SESSION_ID", "generic-session");
        assert_eq!(get_session_id(), "codex-session");
        std::env::remove_var("CODEX_SESSION_ID");
        std::env::remove_var("SESSION_ID");
    }

    #[test]
    fn generic_session_id_precedes_pid_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("CODEX_SESSION_ID");
        std::env::set_var("SESSION_ID", "generic-session");
        assert_eq!(get_session_id(), "generic-session");
        std::env::remove_var("SESSION_ID");
    }

    #[test]
    fn args_json_requires_object() {
        assert!(parse_args_json("{}").is_ok());
        assert!(parse_args_json("[]").is_err());
    }

    #[test]
    fn tool_list_contains_context_boot_schema() {
        let tool = get_tool("mission_context_boot").expect("mission_context_boot schema");
        assert_eq!(tool.name, "mission_context_boot");
    }
}
