use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use crate::lenient;
use crate::state::AppState;

#[derive(Deserialize)]
struct CCSessionsArgs {
    #[serde(rename = "projectPath")]
    project_path: Option<String>,
    #[serde(
        rename = "activeOnly",
        default,
        deserialize_with = "lenient::option_bool"
    )]
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
pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Consolidated tools
    if name == "mission_cc_query" {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("overview");
        return match action {
            "sessions" => handle_inner(state, "mission_cc_sessions", args).await,
            "tasks" => handle_inner(state, "mission_cc_tasks", args).await,
            "overview" => handle_inner(state, "mission_cc_overview", args).await,
            "in_progress" => handle_inner(state, "mission_cc_in_progress", args).await,
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    if name == "mission_cc_swarm" {
        return handle_inner(state, "mission_cc_trigger_swarm", args).await;
    }
    handle_inner(state, name, args).await
}

async fn handle_inner(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
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
                let cc = state.cc_tasks.lock().await;
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
                    .filter(|s| {
                        s.project_path.contains(&filter) || s.project_name.contains(&filter)
                    })
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
                    let cc = state.cc_tasks.lock().await;
                    cc.get_session_tasks(&session_id).await
                };
                if let Some(tasks) = tasks {
                    return Ok(ToolResult::json_pretty(&tasks));
                }
                return Ok(ToolResult::error("Session not found"));
            }

            if let Some(project_path) = project_path {
                let sessions = {
                    let cc = state.cc_tasks.lock().await;
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
            let overview = { state.cc_tasks.lock().await.get_overview().await };
            Ok(ToolResult::json_pretty(&overview))
        }
        "mission_cc_in_progress" => {
            let in_progress = { state.cc_tasks.lock().await.get_in_progress_tasks().await };
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

            let res = state.pty.send(&slot_id, &prompt, timeout_ms).await?;
            Ok(ToolResult::text(res.response))
        }

        _ => Err(anyhow!("Unknown cc_tasks tool: {name}")),
    }
}
