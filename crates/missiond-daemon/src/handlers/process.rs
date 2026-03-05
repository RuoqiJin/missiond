use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::Value;
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;
use crate::lenient;

#[derive(Deserialize)]
struct SpawnArgs {
    #[serde(rename = "slotId")]
    slot_id: String,
    #[serde(default, deserialize_with = "lenient::option_bool")]
    visible: Option<bool>,
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
    #[serde(default, deserialize_with = "lenient::option_bool")]
    visible: Option<bool>,
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Process control =====
        "mission_spawn" => {
            let SpawnArgs {
                slot_id,
                visible,
                auto_restart,
            } = serde_json::from_value(args)?;
            let agent = state
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
            state.mission.kill_agent(&slot_id).await?;
            Ok(ToolResult::json(
                &serde_json::json!({ "success": true, "slotId": slot_id }),
            ))
        }
        "mission_restart" => {
            let RestartArgs { slot_id, visible } = serde_json::from_value(args)?;
            let agent = state
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
            let agents = state.mission.get_agents();
            Ok(ToolResult::json(&agents))
        }


        _ => Err(anyhow!("Unknown process tool: {name}")),
    }
}
