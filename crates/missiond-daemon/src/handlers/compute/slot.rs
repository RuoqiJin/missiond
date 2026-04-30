use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;
use tracing::{info, warn};

use crate::lenient;
use crate::state::AppState;

#[derive(Deserialize)]
struct InboxArgs {
    #[serde(
        rename = "unreadOnly",
        default,
        deserialize_with = "lenient::option_bool"
    )]
    unread_only: Option<bool>,
    #[serde(default)]
    limit: Option<usize>,
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_slots" => Ok(ToolResult::json(&state.mission.list_slots())),
        "mission_inbox" => {
            let InboxArgs { unread_only, limit } =
                serde_json::from_value(args).unwrap_or(InboxArgs {
                    unread_only: None,
                    limit: None,
                });
            let messages = state
                .store
                .get_inbox_messages(unread_only.unwrap_or(true), limit.unwrap_or(10) as i64)
                .await?;
            Ok(ToolResult::json(&messages))
        }
        "mission_pause" => handle_pause(state, args),
        "mission_slot_history" => handle_slot_history(state, args).await,
        _ => Err(anyhow!("Unknown compute slot tool: {name}")),
    }
}

fn handle_pause(state: &AppState, args: Value) -> Result<ToolResult> {
    use std::sync::atomic::Ordering;

    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("status");
    let home = crate::helpers::default_mission_home();
    let flag = home.join("global_paused");

    match action {
        "pause" => {
            state.global_paused.store(true, Ordering::Relaxed);
            let now = chrono::Utc::now().timestamp();
            state.global_paused_at.store(now, Ordering::Relaxed);
            let _ = std::fs::write(&flag, now.to_string());
            warn!("Global pause activated via MCP tool");
            Ok(ToolResult::text(
                "✅ 全局暂停已激活。所有工位任务分派已停止。",
            ))
        }
        "resume" => {
            state.global_paused.store(false, Ordering::Relaxed);
            state.global_paused_at.store(0, Ordering::Relaxed);
            let _ = std::fs::remove_file(&flag);
            info!("Global pause deactivated via MCP tool");
            Ok(ToolResult::text("▶️ 全局暂停已解除。工位恢复正常工作。"))
        }
        _ => {
            let paused = state.control_manager.current().global_paused;
            let since = state.global_paused_at.load(Ordering::Relaxed);
            let msg = if paused {
                format!(
                    "⏸ 当前处于全局暂停状态 (始于 {})",
                    chrono::DateTime::from_timestamp(since, 0)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_else(|| "未知时间".to_string())
                )
            } else {
                "🟢 当前工作正常，未暂停。".to_string()
            };
            Ok(ToolResult::text(msg))
        }
    }
}

async fn handle_slot_history(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SlotHistoryArgs {
        slot_id: Option<String>,
        task_type: Option<String>,
        status: Option<String>,
        #[serde(default, deserialize_with = "lenient::option_i64")]
        limit: Option<i64>,
        #[serde(default, deserialize_with = "lenient::option_bool")]
        stats: Option<bool>,
    }
    let args: SlotHistoryArgs = serde_json::from_value(args)?;
    if args.stats.unwrap_or(false) {
        let stats = state
            .store
            .slot_task_stats(args.slot_id.as_deref())
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        Ok(ToolResult::json_pretty(&stats))
    } else {
        let tasks = state
            .store
            .list_slot_tasks(
                args.slot_id.as_deref(),
                args.task_type.as_deref(),
                args.status.as_deref(),
                args.limit.unwrap_or(20),
            )
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        Ok(ToolResult::json_pretty(&tasks))
    }
}
