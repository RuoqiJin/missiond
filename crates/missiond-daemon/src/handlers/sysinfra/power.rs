use anyhow::Result;
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};
use tracing::info;

use crate::state::AppState;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_power_control" => handle_power_control(state, args).await,
        _ => Ok(ToolResult::error(format!("Unknown power tool: {}", name))),
    }
}

async fn handle_power_control(state: &AppState, args: Value) -> Result<ToolResult> {
    let target = args.get("target").and_then(|v| v.as_str()).unwrap_or("");
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");

    if target.is_empty() || action.is_empty() {
        return Ok(ToolResult::error("target and action are required"));
    }

    let server = state.infra.read().unwrap().get(target).cloned();
    let server_info = server
        .as_ref()
        .map(|s| json!({ "id": s.id, "host": s.host, "roles": s.roles }));

    match action {
        "status" => {
            let host = server
                .as_ref()
                .and_then(|s| s.host.as_deref())
                .unwrap_or(target);
            let port: u16 = 22;
            let addr = format!("{}:{}", host, port);
            let reachable = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                tokio::net::TcpStream::connect(&addr),
            )
            .await
            .is_ok();
            Ok(ToolResult::json_pretty(&json!({
                "target": target,
                "action": "status",
                "reachable": reachable,
                "probe": addr,
                "server": server_info,
            })))
        }
        "wake" => {
            info!(target, "Power control: wake requested");
            Ok(ToolResult::json_pretty(&json!({
                "target": target,
                "action": "wake",
                "status": "requested",
                "note": "Wake-on-LAN / cloud API 唤醒已记录，具体执行需按 infra 配置补充",
                "server": server_info,
            })))
        }
        "suspend" => {
            info!(target, "Power control: suspend requested");
            Ok(ToolResult::json_pretty(&json!({
                "target": target,
                "action": "suspend",
                "status": "requested",
                "note": "休眠指令已记录，具体执行需按 infra 配置补充",
                "server": server_info,
            })))
        }
        _ => Ok(ToolResult::error(format!(
            "Unknown action: {}. Use wake/suspend/status",
            action
        ))),
    }
}
