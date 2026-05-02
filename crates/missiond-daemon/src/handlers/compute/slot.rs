use anyhow::{anyhow, Result};
use missiond_core::types::{CliEngine, Slot};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use tracing::{info, warn};

use crate::context::v3_blueprint_runtime::WorkstationRuntimeConfig;
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
        "mission_slots" => Ok(ToolResult::json(&projected_mission_slots(state).await)),
        "mission_master_status" => Ok(ToolResult::json(&mission_master_status(state).await)),
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

async fn mission_master_status(state: &AppState) -> Value {
    let config = WorkstationRuntimeConfig::load_for_current_dir().ok();
    let worker = config.as_ref().and_then(|config| {
        config
            .workstation_pool()
            .iter()
            .find(|worker| worker.id == "codex-master-control")
    });
    let slot_id = worker
        .map(|worker| worker.slot_id.as_str())
        .unwrap_or("slot-codex-master-control");
    let pty_status = state.pty.get_status(slot_id).await;
    let mission_slot_record = state
        .mission
        .list_slots()
        .into_iter()
        .find(|slot| slot.config.id == slot_id);
    let checkpoint_root = mission_slot_record
        .as_ref()
        .and_then(|slot| slot.config.project_root.clone().or(slot.config.cwd.clone()))
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let mission_slot = mission_slot_record.and_then(|slot| serde_json::to_value(slot).ok());
    let checkpoint_path =
        checkpoint_root.join(".missiond/v3/runtime/master-control-checkpoint.lisp");
    let checkpoint_text = std::fs::read_to_string(&checkpoint_path).ok();

    json!({
        "schema": "missiond.master-status.v1",
        "worker": "codex-master-control",
        "slotId": slot_id,
        "configured": worker.is_some(),
        "acceptsBoardTask": worker.map(|worker| worker.accepts_boardtask).unwrap_or(false),
        "writeAllowed": worker.map(|worker| worker.write_allowed).unwrap_or(false),
        "modelProfile": worker.and_then(|worker| worker.model_profile.clone()),
        "reasoningEffort": worker.and_then(|worker| worker.reasoning_effort.clone()),
        "searchEnabled": worker.map(|worker| worker.search_enabled).unwrap_or(false),
        "sandbox": worker.and_then(|worker| worker.sandbox.clone()),
        "approvalPolicy": worker.and_then(|worker| worker.approval_policy.clone()),
        "pty": pty_status.and_then(|status| serde_json::to_value(status).ok()),
        "slot": mission_slot,
        "checkpoint": {
            "path": checkpoint_path.display().to_string(),
            "exists": checkpoint_text.is_some(),
            "preview": checkpoint_text
                .as_ref()
                .map(|text| text.chars().take(1600).collect::<String>()),
        },
        "authority": {
            "primary": ["provider_jsonl", "codex_sqlite", "claude_jsonl", "gemini_chat_file"],
            "secondary": ["missiond_event_bus", "board_task_lifecycle"],
            "diagnostic": ["pty_recognition_snapshot"]
        }
    })
}

async fn projected_mission_slots(state: &AppState) -> Vec<Value> {
    let slots = state.mission.list_slots();
    let Ok(config) = WorkstationRuntimeConfig::load_for_current_dir() else {
        return slots
            .into_iter()
            .filter_map(|slot| serde_json::to_value(slot).ok())
            .collect();
    };
    let v3_slot_ids: HashSet<String> = config
        .startup_slots()
        .iter()
        .filter_map(|slot| slot.slot_id.clone())
        .chain(
            config
                .workstation_pool()
                .iter()
                .map(|worker| worker.slot_id.clone()),
        )
        .collect();

    let projected: Vec<Slot> = slots
        .into_iter()
        .filter(|slot| !is_stopped_legacy_sonnet_residual(slot, &v3_slot_ids))
        .collect();

    let mut out = Vec::with_capacity(projected.len());
    for slot in projected {
        let mut value = serde_json::to_value(&slot).unwrap_or_else(|_| json!({}));
        if let Some(info) = state.pty.get_status(&slot.config.id).await {
            value["ptyState"] = json!(serde_json::to_value(&info.state)
                .ok()
                .and_then(|v| v.as_str().map(ToString::to_string))
                .unwrap_or_else(|| format!("{:?}", info.state)));
            if let Some(recognition) = info.recognition {
                if let Ok(recognition) = serde_json::to_value(recognition) {
                    value["ptyRecognition"] = recognition;
                }
            }
        }

        if let Ok(Some(session_id)) = state.store.get_slot_session(&slot.config.id).await {
            value["sessionId"] = json!(session_id);
            if let Ok(Some(conv)) = state.store.get_conversation(&session_id).await {
                if conversation_source_matches_engine(slot.config.engine, &conv.source) {
                    value["latestConversation"] = json!({
                        "id": conv.id,
                        "source": conv.source,
                        "chatType": conv.chat_type,
                        "conversationType": conv.conversation_type,
                        "messageCount": conv.message_count,
                        "status": conv.status,
                        "updatedAt": conv.updated_at,
                    });
                } else {
                    value["latestConversationMismatch"] = json!({
                        "id": conv.id,
                        "source": conv.source,
                        "expectedSource": canonical_source_for_engine(slot.config.engine),
                    });
                }
            }
        }
        out.push(value);
    }

    out
}

fn canonical_source_for_engine(engine: CliEngine) -> &'static str {
    match engine {
        CliEngine::ClaudeCode => "claude_code",
        CliEngine::Gemini => "gemini_cli",
        CliEngine::Codex => "codex_cli",
    }
}

fn conversation_source_matches_engine(engine: CliEngine, source: &str) -> bool {
    source == canonical_source_for_engine(engine)
}

fn is_stopped_legacy_sonnet_residual(slot: &Slot, v3_slot_ids: &HashSet<String>) -> bool {
    if v3_slot_ids.contains(&slot.config.id) {
        return false;
    }
    slot.session_id.is_none()
        && slot.config.engine == CliEngine::ClaudeCode
        && slot.config.project_root.is_none()
        && slot
            .config
            .model
            .as_deref()
            .is_some_and(|model| model.contains("sonnet"))
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
