use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::Value;
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;
use crate::event_bus::DaemonEvent;
use crate::lenient;
use crate::decision_harvest::harvest_decisions_for_task;

/// Publish BoardTaskUpdated event.
fn publish_board_update(state: &AppState, task: &missiond_core::types::BoardTask) {
    state.event_bus.publish(DaemonEvent::BoardTaskUpdated {
        task_id: task.id.clone(),
        status: format!("{:?}", task.status),
        category: task.category.clone(),
    });
}

#[derive(Deserialize)]
struct BoardListArgs {
    #[serde(default)]
    status: Option<String>,
    #[serde(default, rename = "includeHidden", deserialize_with = "lenient::option_bool")]
    include_hidden: Option<bool>,
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

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Board Tasks (Personal Task Board) =====
        "mission_board_list" => {
            let BoardListArgs { status, include_hidden } =
                serde_json::from_value(args).unwrap_or(BoardListArgs { status: None, include_hidden: None });
            let tasks = state
                .mission
                .db()
                .list_board_tasks(status.as_deref(), include_hidden.unwrap_or(false))
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&tasks))
        }
        "mission_board_create" => {
            let input: missiond_core::types::CreateBoardTaskInput =
                serde_json::from_value(args)?;
            let mut task = state
                .mission
                .db()
                .create_board_task(&input)
                .map_err(|e| anyhow!("DB error: {}", e))?;

            // If flowTemplate is set, initialize flow fields
            if input.flow_template.is_some() {
                let flow_phase = "investigate".to_string();
                let flow_ctx = serde_json::to_string(&missiond_core::types::FlowContext::default())
                    .unwrap_or_else(|_| "{}".to_string());
                let updated = state.mission.db().update_board_task(
                    &task.id,
                    &missiond_core::types::UpdateBoardTaskInput {
                        flow_phase: Some(flow_phase),
                        flow_context: Some(flow_ctx),
                        flow_template: input.flow_template.clone(),
                        ..Default::default()
                    },
                ).map_err(|e| anyhow!("DB error setting flow: {}", e))?;
                if let Some(t) = updated {
                    task = t;
                }
            }

            publish_board_update(state, &task);
            Ok(ToolResult::json_pretty(&task))
        }
        "mission_board_update" => {
            let args_val: Value = args;
            let id = args_val
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing 'id' field"))?
                .to_string();
            let is_marking_done = args_val.get("status")
                .and_then(|v| v.as_str())
                .map(|s| s == "done")
                .unwrap_or(false);
            let update: missiond_core::types::UpdateBoardTaskInput =
                serde_json::from_value(args_val)?;
            let task = state
                .mission
                .db()
                .update_board_task(&id, &update)
                .map_err(|e| anyhow!("DB error: {}", e))?;
            match task {
                Some(t) => {
                    publish_board_update(state, &t);
                    // Decision Engine: harvest decisions when task marked done
                    if is_marking_done {
                        let state = state.clone();
                        let task_id = t.id.clone();
                        let task_title = t.title.clone();
                        tokio::spawn(async move {
                            harvest_decisions_for_task(&state, &task_id, &task_title).await;
                        });
                    }
                    Ok(ToolResult::json_pretty(&t))
                }
                None => Ok(ToolResult::error("Task not found")),
            }
        }
        "mission_board_get" => {
            let BoardIdArgs { id } = serde_json::from_value(args)?;
            let task = state
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
            let deleted = state
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
            let task = state
                .mission
                .db()
                .toggle_board_task(&id)
                .map_err(|e| anyhow!("DB error: {}", e))?;
            match task {
                Some(t) => {
                    publish_board_update(state, &t);
                    // Decision Engine: harvest decisions when toggled to done
                    if t.status == missiond_core::types::BoardTaskStatus::Done {
                        let state = state.clone();
                        let task_id = t.id.clone();
                        let task_title = t.title.clone();
                        tokio::spawn(async move {
                            harvest_decisions_for_task(&state, &task_id, &task_title).await;
                        });
                    }
                    Ok(ToolResult::json_pretty(&t))
                }
                None => Ok(ToolResult::error("Task not found")),
            }
        }
        "mission_board_claim" => {
            let task_id = args.get("taskId").and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("taskId is required"))?;
            let executor_type = args.get("executorType").and_then(|v| v.as_str())
                .unwrap_or("manual_session");
            // Use explicit executorId or fall back to a generated session identifier
            let executor_id = args.get("executorId").and_then(|v| v.as_str())
                .unwrap_or("claude-code-session");
            match state.mission.db().claim_board_task(task_id, executor_id, executor_type) {
                Ok(Some(task)) => Ok(ToolResult::json_pretty(&task)),
                Ok(None) => {
                    // Check why it failed: task not found vs already claimed
                    match state.mission.db().get_board_task(task_id) {
                        Ok(Some(existing)) => {
                            let msg = if let Some(ref claimer) = existing.claim_executor_id {
                                format!("Task already claimed by {} ({})",
                                    claimer,
                                    existing.claim_executor_type.as_deref().unwrap_or("unknown"))
                            } else {
                                format!("Task cannot be claimed (status: {})", existing.status.as_str())
                            };
                            Ok(ToolResult::error(msg))
                        }
                        _ => Ok(ToolResult::error("Task not found")),
                    }
                }
                Err(e) => Ok(ToolResult::error(format!("DB error: {}", e))),
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
            let note = state
                .mission
                .db()
                .add_board_task_note(&input)
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&note))
        }

        "mission_board_summary" => {
            #[derive(Deserialize)]
            struct SummaryArgs {
                since: Option<String>,
            }
            let args: SummaryArgs = serde_json::from_value(args)?;
            let summary = state
                .mission
                .db()
                .board_summary(args.since.as_deref())
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&summary))
        }

        // ===== Task Decompose =====
        "mission_board_decompose" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct DecomposeArgs {
                task_id: String,
                slot_id: Option<String>,
                hints: Option<String>,
            }
            let args: DecomposeArgs = serde_json::from_value(args)?;
            let slot_id = args.slot_id.unwrap_or_else(|| "slot-coder-1".to_string());

            // Get the task
            let task = state.mission.db()
                .get_board_task(&args.task_id)
                .map_err(|e| anyhow!("DB error: {}", e))?
                .ok_or_else(|| anyhow!("Task not found: {}", args.task_id))?;

            // Validate: must be open
            if task.status != missiond_core::types::BoardTaskStatus::Open {
                return Ok(ToolResult::text(format!(
                    "Error: 任务状态为 {:?}，只能拆分 open 状态的任务", task.status
                )));
            }

            // Check if already has subtasks
            let subtasks = state.mission.db()
                .list_board_tasks(None, true)
                .map_err(|e| anyhow!("DB error: {}", e))?
                .into_iter()
                .filter(|t| t.parent_id.as_deref() == Some(&task.id))
                .count();
            if subtasks > 0 {
                return Ok(ToolResult::text(format!(
                    "Error: 任务已有 {} 个子任务，不能重复拆分。如需重新拆分，请先删除现有子任务。", subtasks
                )));
            }

            // Build decompose prompt
            let hints_section = args.hints
                .map(|h| format!("\n### 用户提示\n{}", h))
                .unwrap_or_default();

            // Inject context from Skills + KB
            let context = state.skills.build_context(&task.title);
            let context_section = if context.contains("No matching skills") {
                String::new()
            } else {
                format!("\n### 相关知识\n{}", context)
            };

            let decompose_prompt = format!(
                r#"## 任务拆分指令

你需要将以下 Board 任务拆分为可执行的子任务序列。

### 父任务
- ID: {task_id}
- 标题: {title}
- 描述: {description}
{hints}{context}

### 拆分规范

1. **调查先行**: 第一个子任务必须是调查/分析相关代码和基建
2. **检查点**: 在"方案确定"后插入一个 user_review 子任务（让用户审批方案再继续）
3. **原子化**: 每个子任务应该是一个独立的、可验证的工作单元
4. **依赖链**: 用 dependsOn 串联，确保执行顺序

### 操作步骤

对每个子任务，调用 `mission_board_create`:
- title: 清晰的动作描述
- description: 包含具体文件路径、预期产出
- parentId: "{task_id}"
- dependsOn: [前置任务ID]（第一个子任务不需要）
- category: 继承父任务 "{category}"
- priority: 继承父任务 "{priority}"
- project: "{project}"

**执行型子任务** (工位干活):
- assignee: "slot-coder-1"
- autoExecute: true

**审批型子任务** (用户检查点):
- autoExecute: false
- title 以 "[Review]" 开头
- description 写明: 需要审批什么、审批后用 mission_board_toggle 放行下游

### 完成后

在父任务添加一条备注 (mission_board_note_add):
- taskId: "{task_id}"
- noteType: "summary"
- content: 列出所有子任务 ID + 标题 + 依赖链"#,
                task_id = task.id,
                title = task.title,
                description = task.description,
                hints = hints_section,
                context = context_section,
                category = task.category,
                priority = task.priority,
                project = task.project.as_deref().unwrap_or(""),
            );

            // Submit as a task to the slot
            let submit_task_id = state.mission.submit("coder", &decompose_prompt)?;

            // Store target slot
            let _ = state.mission.db().update_task(
                &submit_task_id,
                &missiond_core::types::TaskUpdate {
                    slot_id: Some(slot_id.clone()),
                    ..Default::default()
                },
            );

            // Try immediate dispatch
            if let Some(status) = state.pty.get_status(&slot_id).await {
                if status.state == missiond_core::pty::SessionState::Idle {
                    if let Ok(()) = state.pty.send_fire_and_forget(&slot_id, &decompose_prompt).await {
                        let now = chrono::Utc::now().timestamp_millis();
                        let slot_session = state.mission.db().get_slot_session(&slot_id).ok().flatten();
                        let _ = state.mission.db().update_task(
                            &submit_task_id,
                            &missiond_core::types::TaskUpdate {
                                status: Some(missiond_core::types::TaskStatus::Running),
                                slot_id: Some(slot_id.clone()),
                                session_id: slot_session,
                                started_at: Some(now),
                                ..Default::default()
                            },
                        );
                    }
                }
            }

            // Write note on parent task
            let _ = state.mission.db().add_board_task_note(
                &missiond_core::types::AddBoardTaskNoteInput {
                    task_id: task.id.clone(),
                    content: format!("🔀 任务拆分已启动 → submit task {} → slot {}", submit_task_id, slot_id),
                    note_type: Some("progress".to_string()),
                    author: Some("decompose".to_string()),
                },
            );

            Ok(ToolResult::text(format!(
                "✅ 拆分任务已派发\n- Submit Task: {}\n- 工位: {}\n- 父任务: {} ({})\n\n工位将调查代码后自动创建子任务。用 mission_task_track(taskId: \"{}\") 追踪进度。",
                submit_task_id, slot_id, task.title, task.id, submit_task_id
            )))
        }

        "mission_board_retry" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct RetryArgs {
                task_id: String,
                #[serde(default = "default_true")]
                reset_downstream: bool,
            }
            fn default_true() -> bool { true }

            let args: RetryArgs = serde_json::from_value(args)?;

            // Verify task exists
            let task = state.mission.db()
                .get_board_task(&args.task_id)
                .map_err(|e| anyhow!("DB error: {}", e))?
                .ok_or_else(|| anyhow!("Task not found: {}", args.task_id))?;

            let reset_ids = state.mission.db()
                .retry_board_task(&args.task_id, args.reset_downstream)
                .map_err(|e| anyhow!("DB error: {}", e))?;

            // Write note
            let _ = state.mission.db().add_board_task_note(
                &missiond_core::types::AddBoardTaskNoteInput {
                    task_id: task.id.clone(),
                    content: format!(
                        "🔄 任务重试\n- 重置任务数: {}\n- 级联下游: {}",
                        reset_ids.len(),
                        if args.reset_downstream { "是" } else { "否" }
                    ),
                    note_type: Some("progress".to_string()),
                    author: Some("retry".to_string()),
                },
            );

            Ok(ToolResult::text(format!(
                "✅ 已重试任务 '{}'\n- 重置任务数: {}\n- 重置的任务 ID: {:?}",
                task.title, reset_ids.len(), reset_ids
            )))
        }

        _ => Err(anyhow!("Unknown board tool: {name}")),
    }
}
