//! missiond - singleton daemon for missiond
//!
//! Responsibilities:
//! - Own the global state (DB, slot/process/task/inbox, PTY sessions, CC tasks watcher)
//! - Provide a stable WebSocket endpoint for attach + tasks events
//! - Expose an IPC JSON-RPC endpoint for MCP proxy processes

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use missiond_core::{
    CorePermissionDecision, InfraConfig, MissionControl, MissionControlOptions, PermissionPolicy,
    PermissionRule, PTYManager, PTYSpawnOptions, PTYWebSocketServer, WSServerOptions, SkillIndex,
};
use missiond_core::{CCTasksWatcher, CCTasksWatcherOptions, WatcherEvent};
use missiond_mcp::protocol::{self, Request, RequestId, Response, RpcError};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use missiond_core::ipc::{self, IpcListener, IpcStream};

#[derive(Clone)]
struct AppState {
    mission: Arc<MissionControl>,
    permission: Arc<PermissionPolicy>,
    pty: Arc<PTYManager>,
    cc_tasks: Arc<Mutex<CCTasksWatcher>>,
    skills: Arc<SkillIndex>,
    infra: Arc<InfraConfig>,
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
    query: String,
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

    async fn call_tool_inner(&self, name: &str, args: Value) -> Result<ToolResult> {
        match name {
            // ===== Task operations =====
            "mission_submit" => {
                let SubmitArgs { role, prompt } = serde_json::from_value(args)?;
                let task_id = self.mission.submit(&role, &prompt)?;
                Ok(ToolResult::json(&serde_json::json!({ "taskId": task_id })))
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

                let info = self
                    .pty
                    .spawn(
                        &pty_slot,
                        PTYSpawnOptions {
                            auto_restart: auto_restart.unwrap_or(false),
                            wait_for_idle: wait_for_idle.unwrap_or(false),
                            timeout_secs,
                            mcp_config,
                        },
                    )
                    .await?;
                Ok(ToolResult::json(&info))
            }
            "mission_pty_send" => {
                let PTYSendArgs {
                    slot_id,
                    message,
                    timeout_ms,
                } = serde_json::from_value(args)?;
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
            }
            "mission_pty_kill" => {
                let PTYKillArgs { slot_id } = serde_json::from_value(args)?;
                self.pty.kill(&slot_id).await?;
                Ok(ToolResult::json(
                    &serde_json::json!({ "success": true, "slotId": slot_id }),
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
                    Ok(ToolResult::json(&status))
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
                #[cfg(unix)]
                let hint = format!("tail -f {}", status.log_file.display());
                #[cfg(windows)]
                let hint = format!("Get-Content -Path \"{}\" -Wait -Tail 50", status.log_file.display());

                Ok(ToolResult::json(&serde_json::json!({
                    "slotId": slot_id,
                    "logFile": status.log_file,
                    "hint": hint,
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
            "mission_kb_search" => {
                let KBSearchArgs { query, category } = serde_json::from_value(args)?;
                let results = self.mission.db()
                    .kb_search(&query, category.as_deref())
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json_pretty(&results))
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
                            "pairs": dups.iter().map(|(a, b)| serde_json::json!({
                                "a": {"category": a.category, "key": a.key, "summary": a.summary},
                                "b": {"category": b.category, "key": b.key, "summary": b.summary},
                            })).collect::<Vec<_>>(),
                        })))
                    }
                    _ => Ok(ToolResult::error(format!("Unknown gc action: {}. Use: stats, stale, duplicates", action))),
                }
            }

            // ===== Conversation Logs =====
            "mission_conversation_list" => {
                #[derive(Deserialize)]
                struct Args {
                    status: Option<String>,
                    limit: Option<i64>,
                }
                let Args { status, limit } =
                    serde_json::from_value(args).unwrap_or(Args { status: None, limit: None });
                let convs = self.mission.db()
                    .list_conversations(status.as_deref(), limit.unwrap_or(20))
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
                }
                let Args { session_id, tail, since_id } = serde_json::from_value(args)?;
                let conv = self.mission.db()
                    .get_conversation(&session_id)
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                let msgs = self.mission.db()
                    .get_conversation_messages(&session_id, since_id, tail.unwrap_or(50))
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json(&serde_json::json!({
                    "conversation": conv,
                    "messages": msgs,
                    "count": msgs.len(),
                })))
            }
            "mission_conversation_search" => {
                #[derive(Deserialize)]
                struct Args {
                    query: String,
                    limit: Option<i64>,
                }
                let Args { query, limit } = serde_json::from_value(args)?;
                let msgs = self.mission.db()
                    .search_conversation_messages(&query, limit.unwrap_or(20))
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                Ok(ToolResult::json(&serde_json::json!({
                    "results": msgs,
                    "count": msgs.len(),
                    "query": query,
                })))
            }

            // ===== Board Tasks (Personal Task Board) =====
            "mission_board_list" => {
                let BoardListArgs { status } =
                    serde_json::from_value(args).unwrap_or(BoardListArgs { status: None });
                let tasks = self
                    .mission
                    .db()
                    .list_board_tasks(status.as_deref())
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
                let results: Vec<Value> = self
                    .skills
                    .search(&query)
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
                Ok(ToolResult::json_pretty(&results))
            }
            "mission_context_build" => {
                let ContextBuildArgs { query } = serde_json::from_value(args)?;
                let mut context = self.skills.build_context(&query);

                // Also search KB for matching knowledge
                let db = self.mission.db();
                if let Ok(entries) = db.kb_search(&query, None) {
                    let entries: Vec<_> = entries.into_iter().take(5).collect();
                    if !entries.is_empty() {
                        context.push_str("\n[Knowledge Base]\n");
                        for entry in &entries {
                            context.push_str(&format!(
                                "- [{}] {}: {}\n",
                                entry.category, entry.key, entry.summary
                            ));
                        }
                    }
                }

                Ok(ToolResult::text(context))
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

            _ => {
                let mut res = ToolResult::text(format!("Unknown tool: {}", name));
                res.is_error = Some(true);
                Ok(res)
            }
        }
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

async fn autopilot_tick(state: &AppState) -> Result<()> {
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

        info!(task_id = %task.id, slot_id = %slot_id, title = %task.title, "Autopilot: executing task");

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
                let mcp_config = slot.config.mcp_config.map(PathBuf::from);
                if let Err(e) = state.pty.spawn(&pty_slot, PTYSpawnOptions {
                    auto_restart: false,
                    wait_for_idle: true,
                    timeout_secs: Some(120),
                    mcp_config,
                }).await {
                    warn!(task_id = %task.id, slot_id = %slot_id, error = %e, "Autopilot: failed to spawn PTY");
                    continue;
                }
            } else {
                warn!(task_id = %task.id, slot_id = %slot_id, "Autopilot: slot not found, skipping");
                continue;
            }
        }

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
                info!(task_id = %task.id, duration_ms = res.duration_ms, "Autopilot: task completed");
            }
            Err(e) => {
                // Record failure as a note
                let note_content = format!("**Autopilot 执行失败**\n\n{}", e);
                let _ = state.mission.db().add_board_task_note(
                    &missiond_core::types::AddBoardTaskNoteInput {
                        task_id: task.id.clone(),
                        content: note_content,
                        note_type: Some("note".to_string()),
                        author: Some("autopilot".to_string()),
                    },
                );
                warn!(task_id = %task.id, error = %e, "Autopilot: task execution failed");
            }
        }
    }

    Ok(())
}

/// Extract text content from a Claude Code message content field.
/// Content can be a plain string or an array of content blocks.
fn extract_text_content(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|item| {
                if item.get("type")?.as_str()? == "text" {
                    item.get("text")?.as_str().map(String::from)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Handle a NewMessages watcher event: write conversation messages to DB.
fn handle_new_messages(
    state: &AppState,
    session_id: String,
    project_path: String,
    jsonl_path: String,
    messages: Vec<missiond_core::CCMessageLine>,
) {
    let db = state.mission.db();

    // Ensure conversation exists
    if db.get_conversation(&session_id).unwrap_or(None).is_none() {
        let first = messages.first();
        let conv = missiond_core::types::Conversation {
            id: session_id.clone(),
            project: Some(first.map(|m| m.cwd.clone()).unwrap_or(project_path)),
            slot_id: None,
            source: "claude_cli".to_string(),
            model: first.and_then(|m| m.message.model.clone()),
            git_branch: first.and_then(|m| m.git_branch.clone()),
            jsonl_path: Some(jsonl_path),
            message_count: 0,
            started_at: first
                .map(|m| m.timestamp.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            ended_at: None,
            status: "active".to_string(),
            analyzed_at: None,
        };
        if let Err(e) = db.upsert_conversation(&conv) {
            error!(session = %session_id, error = %e, "Failed to create conversation");
            return;
        }
    }

    let mut inserted = 0u32;
    for msg in &messages {
        // Dedup by message UUID
        if db.conversation_message_exists(&msg.uuid).unwrap_or(true) {
            continue;
        }

        let text_content = extract_text_content(&msg.message.content);
        if text_content.is_empty() {
            continue;
        }

        let raw_content = serde_json::to_string(&msg.message.content).ok();

        let conv_msg = missiond_core::types::ConversationMessage {
            id: 0, // auto-increment
            session_id: session_id.clone(),
            role: msg.message.role.clone(),
            content: text_content,
            raw_content,
            message_uuid: Some(msg.uuid.clone()),
            parent_uuid: msg.parent_uuid.clone(),
            model: msg.message.model.clone(),
            timestamp: msg.timestamp.clone(),
            metadata: None,
        };

        match db.insert_conversation_message(&conv_msg) {
            Ok(_) => inserted += 1,
            Err(e) => {
                error!(uuid = %msg.uuid, error = %e, "Failed to insert conversation message");
            }
        }
    }

    if inserted > 0 {
        info!(
            session = %session_id,
            count = inserted,
            "Logged conversation messages"
        );
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
            message_count: 0,
            started_at: ts,
            ended_at: None,
            status: "active".to_string(),
            analyzed_at: None,
        };
        if let Err(e) = db.upsert_conversation(&conv) {
            error!(slot = %slot_id, error = %e, "Failed to create PTY conversation");
            return;
        }
    }

    let msg_uuid = format!("pty-{}-turn-{}", slot_id, turn_id);
    if db.conversation_message_exists(&msg_uuid).unwrap_or(true) {
        return;
    }

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

    // WebSocket server (PTY attach + Tasks events)
    let ws_port = ws_port();
    let mut ws_server = PTYWebSocketServer::new(WSServerOptions {
        port: ws_port,
        pty_manager: Some(Arc::clone(&pty)),
        cc_tasks_watcher: Some(Arc::clone(&cc_tasks)),
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

    let state = AppState {
        mission,
        permission,
        pty,
        cc_tasks,
        skills,
        infra,
    };

    // Autopilot scheduler + IPC server via select
    let mut autopilot_interval = tokio::time::interval(std::time::Duration::from_secs(60));
    info!("Autopilot scheduler started (60s interval)");

    loop {
        tokio::select! {
            result = listener.accept() => {
                let stream = result?;
                let reader = BufReader::new(stream);
                if let Err(e) = handle_ipc_connection(state.clone(), reader).await {
                    warn!(error = %e, "IPC connection error");
                }
            }
            _ = autopilot_interval.tick() => {
                if let Err(e) = autopilot_tick(&state).await {
                    warn!(error = %e, "Autopilot tick failed");
                }
            }
            event = conv_logger_rx.recv() => {
                match event {
                    Ok(WatcherEvent::NewMessages { session_id, project_path, jsonl_path, messages }) => {
                        handle_new_messages(&state, session_id, project_path, jsonl_path, messages);
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
                        handle_pty_text_complete(&state, slot_id, turn_id, content, timestamp);
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
