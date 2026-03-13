use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::info;
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;
use crate::slot_env::capture_slot_session_uuid;
use crate::helpers::default_mission_home;
use std::path::PathBuf;
use missiond_core::PTYSpawnOptions;
use crate::lenient;
use crate::slot_env::build_slot_tracking_env;

#[derive(Deserialize)]
struct PTYSpawnArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    #[serde(rename = "waitForIdle", default, deserialize_with = "lenient::option_bool")]
    wait_for_idle: Option<bool>,
    #[serde(rename = "timeoutSecs", default)]
    timeout_secs: Option<u64>,
    #[serde(rename = "autoRestart", default, deserialize_with = "lenient::option_bool")]
    auto_restart: Option<bool>,
    #[serde(rename = "mcpConfigPath", default)]
    mcp_config_path: Option<String>,
}

#[derive(Deserialize)]
struct PTYSendArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    message: String,
    #[serde(rename = "waitForResponse", default, deserialize_with = "lenient::option_bool")]
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

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== PTY =====
        "mission_pty_spawn" => {
            let PTYSpawnArgs {
                slot_id,
                wait_for_idle,
                timeout_secs,
                auto_restart,
                mcp_config_path,
            } = serde_json::from_value(args)?;
            let slot = state
                .mission
                .list_slots()
                .into_iter()
                .find(|s| s.config.id == slot_id)
                .ok_or_else(|| anyhow!("Slot not found: {}", slot_id))?;

            let pty_slot = missiond_core::PTYSlot {
                id: slot.config.id.clone(),
                role: slot.config.role.clone(),
                cwd: slot.config.cwd.as_deref().map(PathBuf::from),
                engine: slot.config.engine,
            };

            // Resolve MCP config: arg > slot config > None
            let mcp_config = mcp_config_path
                .or(slot.config.mcp_config.clone())
                .map(PathBuf::from);

            let (extra_env, session_file) = build_slot_tracking_env(&slot_id, slot.config.env.as_ref()).await;
            let wait = wait_for_idle.unwrap_or(false);
            let info = state
                .pty
                .spawn(
                    &pty_slot,
                    PTYSpawnOptions {
                        auto_restart: auto_restart.unwrap_or(false),
                        wait_for_idle: wait,
                        timeout_secs,
                        mcp_config,
                        dangerously_skip_permissions: slot.config.dangerously_skip_permissions.unwrap_or(false),
                        model: slot.config.model.clone(),
                        extra_env,
                    },
                )
                .await?;

            // Capture UUID after spawn (only reliable when we waited for idle)
            if wait {
                capture_slot_session_uuid(state, &slot_id, &session_file).await;
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
                let res = state.pty.send(&slot_id, &message, timeout_ms).await?;
                let state = state
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
                state.pty.send_fire_and_forget(&slot_id, &message).await?;
                Ok(ToolResult::json(&serde_json::json!({
                    "delivered": true,
                    "mode": "fire-and-forget",
                    "hint": "Use pty_status to poll for completion (state returns to Idle when done)",
                })))
            }
        }
        "mission_pty_kill" => {
            let PTYKillArgs { slot_id } = serde_json::from_value(args)?;
            state.pty.kill(&slot_id).await?;
            // Requeue any Running submit tasks assigned to this slot
            let requeued = state.mission.db().requeue_running_tasks_for_slot(&slot_id).unwrap_or(0);
            // Release any board task claims held by this slot
            let claims_released = state.mission.db().release_board_claims_by_executor(&slot_id).unwrap_or(0);
            Ok(ToolResult::json(
                &serde_json::json!({ "success": true, "slotId": slot_id, "requeuedTasks": requeued, "claimsReleased": claims_released }),
            ))
        }
        "mission_pty_screen" => {
            let PTYScreenArgs { slot_id, lines } = serde_json::from_value(args)?;
            if let Some(n) = lines {
                let last = state.pty.get_last_lines(&slot_id, n).await?;
                Ok(ToolResult::text(last.join("\n")))
            } else {
                Ok(ToolResult::text(state.pty.get_screen(&slot_id).await?))
            }
        }
        "mission_pty_history" => {
            let PTYHistoryArgs { slot_id } = serde_json::from_value(args)?;
            let history = state.pty.get_history(&slot_id).await;
            Ok(ToolResult::json(&history))
        }
        "mission_pty_status" => {
            let PTYStatusArgs { slot_id } = serde_json::from_value(args).unwrap_or(PTYStatusArgs {
                slot_id: None,
            });
            if let Some(slot_id) = slot_id {
                let status = state.pty.get_status(&slot_id).await;
                match status {
                    Some(info) => {
                        let mut obj = serde_json::to_value(&info).unwrap_or_default();
                        // Enrich with session_uuid and JSONL activity
                        if let Ok(Some(session_uuid)) = state.mission.db().get_slot_session(&slot_id) {
                            obj["sessionId"] = json!(session_uuid);
                            if let Ok(Some(conv)) = state.mission.db().get_conversation(&session_uuid) {
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
                            let progress = state.slot_progress.read().await;
                            if let Some(sp) = progress.get(&slot_id) {
                                if sp.total_calls > 0 {
                                    obj["progress"] = serde_json::to_value(sp).unwrap_or_default();
                                }
                            }
                        }
                        // Attach last response (eliminates screen-blank misjudgment)
                        {
                            let responses = state.slot_last_responses.read().await;
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
                let all = state.pty.get_all_status().await;
                Ok(ToolResult::json(&all))
            }
        }
        "mission_pty_confirm" => {
            let PTYConfirmArgs { slot_id, response } = serde_json::from_value(args)?;
            let response_echo = response.clone();

            // Capture pending confirm info before confirming (for auto-allow recording)
            let pending = state.pty.get_pending_confirm(&slot_id).await;

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
                        state.pty.write(&slot_id, &input).await?;
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

            state.pty.confirm(&slot_id, resp).await?;

            // Auto-record permission for approved tool confirmations
            if is_approval {
                if let Some(ref info) = pending {
                    if let Some(ref tool) = info.tool {
                        if let Some(status) = state.pty.get_status(&slot_id).await {
                            state.permission.add_role_auto_allow(&status.role, &tool.name);
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
            state.pty.interrupt(&slot_id).await?;
            Ok(ToolResult::json(
                &serde_json::json!({ "success": true, "slotId": slot_id }),
            ))
        }
        "mission_pty_logs" => {
            let PTYLogsArgs { slot_id } = serde_json::from_value(args)?;
            let status = state.pty.get_status(&slot_id).await;
            let status = status.ok_or_else(|| anyhow!("PTY session not found"))?;
            // Find JSONL path from slot→session DB mapping
            let jsonl_path = state.mission.db().get_all_slot_sessions().ok()
                .and_then(|sessions| sessions.into_iter().find(|(sid, _)| sid == &slot_id))
                .and_then(|(_, session_uuid)| {
                    state.mission.db().get_conversation(&session_uuid).ok().flatten()
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
            let (request_id, rx) = state.screenshot_broker.request(&slot_id).await;
            let _ = &request_id; // request is already broadcast by broker

            let browser_result = tokio::time::timeout(
                state.screenshot_broker.timeout,
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
                    let path = state.pty.screenshot(&slot_id, &screenshots_dir).await?;
                    (path, "backend")
                }
            };

            Ok(ToolResult::json(&serde_json::json!({
                "path": path.to_string_lossy(),
                "source": source,
                "hint": "Use the Read tool to view this PNG image"
            })))
        }


        _ => Err(anyhow!("Unknown pty tool: {name}")),
    }
}
