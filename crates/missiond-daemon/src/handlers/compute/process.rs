use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::Value;
use missiond_mcp::tools::ToolResult;
use missiond_core::PTYSpawnOptions;

use crate::state::AppState;
use crate::lenient;
use crate::slot_env;

#[derive(Deserialize)]
struct SpawnArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    #[serde(rename = "autoRestart", default, deserialize_with = "lenient::option_bool")]
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
    #[serde(rename = "autoRestart", default, deserialize_with = "lenient::option_bool")]
    auto_restart: Option<bool>,
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Consolidated tool: mission_agent with action parameter
    if name == "mission_agent" {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("list");
        return match action {
            "spawn" => handle_inner(state, "mission_spawn", args).await,
            "kill" => handle_inner(state, "mission_kill", args).await,
            "restart" => handle_inner(state, "mission_restart", args).await,
            "list" => handle_inner(state, "mission_agents", args).await,
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    handle_inner(state, name, args).await
}

async fn handle_inner(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Process control (all via PTYManager) =====
        "mission_spawn" => {
            let SpawnArgs {
                slot_id,
                auto_restart,
            } = serde_json::from_value(args)?;
            let slot = state
                .mission
                .get_slot(&slot_id)
                .ok_or_else(|| anyhow!("Slot not found: {}", slot_id))?;
            let pty_slot = missiond_core::PTYSlot {
                id: slot.config.id.clone(),
                role: slot.config.role.clone(),
                cwd: slot.config.cwd.as_deref().map(std::path::PathBuf::from),
                engine: slot.config.engine,
            };
            let mcp_config = slot.config.mcp_config.clone().map(std::path::PathBuf::from);

            let info = crate::slot_orchestrator::spawner::spawn_tracked_slot(
                &state.pty,
                &state.store,
                &state.pty_session_uuids,
                &pty_slot,
                PTYSpawnOptions {
                    auto_restart: auto_restart.unwrap_or(false),
                    wait_for_idle: true,
                    timeout_secs: Some(30),
                    mcp_config,
                    dangerously_skip_permissions: slot
                        .config
                        .dangerously_skip_permissions
                        .unwrap_or(false),
                    model: slot.config.model.clone(),
                    extra_env: std::collections::HashMap::new(),
                },
                slot.config.env.as_ref()
            ).await?;
            Ok(ToolResult::json(&info))
        }
        "mission_kill" => {
            let KillArgs { slot_id } = serde_json::from_value(args)?;
            state.pty.kill(&slot_id).await?;
            Ok(ToolResult::json(
                &serde_json::json!({ "success": true, "slotId": slot_id }),
            ))
        }
        "mission_restart" => {
            let RestartArgs {
                slot_id,
                auto_restart,
            } = serde_json::from_value(args)?;
            let slot = state
                .mission
                .get_slot(&slot_id)
                .ok_or_else(|| anyhow!("Slot not found: {}", slot_id))?;
            let pty_slot = missiond_core::PTYSlot {
                id: slot.config.id.clone(),
                role: slot.config.role.clone(),
                cwd: slot.config.cwd.as_deref().map(std::path::PathBuf::from),
                engine: slot.config.engine,
            };
            let mcp_config = slot.config.mcp_config.clone().map(std::path::PathBuf::from);

            // Replicate pty.restart behavior but with tracking
            let _ = state.pty.kill(&slot_id).await;

            let info = crate::slot_orchestrator::spawner::spawn_tracked_slot(
                &state.pty,
                &state.store,
                &state.pty_session_uuids,
                &pty_slot,
                PTYSpawnOptions {
                    auto_restart: auto_restart.unwrap_or(false),
                    wait_for_idle: true,
                    timeout_secs: Some(30),
                    mcp_config,
                    dangerously_skip_permissions: slot
                        .config
                        .dangerously_skip_permissions
                        .unwrap_or(false),
                    model: slot.config.model.clone(),
                    extra_env: std::collections::HashMap::new(),
                },
                slot.config.env.as_ref()
            ).await?;
            Ok(ToolResult::json(&info))
        }
        "mission_agents" => {
            let slots = state.mission.list_slots();
            let mut agents = Vec::new();
            for slot in &slots {
                let info = state.pty.get_status(&slot.config.id).await;
                agents.push(match info {
                    Some(info) => serde_json::to_value(&info).unwrap_or_default(),
                    None => serde_json::json!({
                        "slotId": slot.config.id,
                        "role": slot.config.role,
                        "state": "no_session",
                    }),
                });
            }
            Ok(ToolResult::json(&agents))
        }

        _ => Err(anyhow!("Unknown process tool: {name}")),
    }
}
