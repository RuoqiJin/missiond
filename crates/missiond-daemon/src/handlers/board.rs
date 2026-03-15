use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::Value;
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;
use crate::event_bus::{DaemonEvent, TraceContext};
use crate::lenient;
use crate::decision_harvest::harvest_decisions_for_task;

/// Extracted board_get logic (used by query action="get")
async fn board_get(state: &AppState, args: Value) -> Result<ToolResult> {
    let include_children = args.get("includeChildren")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let ids: Vec<String> = if let Some(id_val) = args.get("ids").and_then(|v| v.as_array()) {
        id_val.iter().filter_map(|v| v.as_str().map(String::from)).collect()
    } else if let Some(id) = args.get("id").and_then(|v| v.as_str()) {
        vec![id.to_string()]
    } else {
        return Ok(ToolResult::error("Either 'id' or 'ids' is required"));
    };
    let single_mode = args.get("ids").is_none() && args.get("id").is_some();
    if include_children || ids.len() > 1 {
        let results = state.mission.db()
            .get_board_tasks_with_context(&ids, include_children)
            .map_err(|e| anyhow!("DB error: {}", e))?;
        if results.is_empty() { return Ok(ToolResult::error("Task not found")); }
        if single_mode { Ok(ToolResult::json_pretty(&results[0])) }
        else { Ok(ToolResult::json_pretty(&results)) }
    } else {
        let task = state.mission.db()
            .get_board_task_with_notes(&ids[0])
            .map_err(|e| anyhow!("DB error: {}", e))?;
        match task {
            Some(t) => Ok(ToolResult::json_pretty(&t)),
            None => Ok(ToolResult::error("Task not found")),
        }
    }
}

/// Publish BoardTaskCreated event.
fn publish_board_created(state: &AppState, task: &missiond_core::types::BoardTask) {
    state.event_bus.publish_traced(
        DaemonEvent::BoardTaskCreated {
            task_id: task.id.clone(),
            title: task.title.clone(),
            category: task.category.clone(),
        },
        TraceContext {
            trace_id: Some(task.id.clone()),
            summary: Some(format!("board: created {}", task.title)),
            ..Default::default()
        },
    );
}

/// Publish BoardTaskUpdated event (generic field update).
fn publish_board_update(state: &AppState, task: &missiond_core::types::BoardTask) {
    state.event_bus.publish_traced(
        DaemonEvent::BoardTaskUpdated {
            task_id: task.id.clone(),
            status: format!("{:?}", task.status),
            category: task.category.clone(),
        },
        TraceContext {
            trace_id: Some(task.id.clone()),
            summary: Some(format!("board: {} → {:?}", task.title, task.status)),
            ..Default::default()
        },
    );
}

/// Publish BoardTaskStatusChanged event.
fn publish_board_status_changed(state: &AppState, task: &missiond_core::types::BoardTask, old_status: &str) {
    state.event_bus.publish_traced(
        DaemonEvent::BoardTaskStatusChanged {
            task_id: task.id.clone(),
            old_status: old_status.to_string(),
            new_status: format!("{:?}", task.status),
        },
        TraceContext {
            trace_id: Some(task.id.clone()),
            summary: Some(format!("board: {} → {:?}", task.title, task.status)),
            ..Default::default()
        },
    );
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

// @beacon: board
pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Consolidated Query =====
        "mission_board_query" | "mission_board_list" | "mission_board_get"
            | "mission_board_search" | "mission_board_summary" => {
            // Determine action from tool name or action parameter
            let action = if name != "mission_board_query" {
                // Backward compat: old tool names map to actions
                name.trim_start_matches("mission_board_").to_string()
            } else {
                args.get("action").and_then(|v| v.as_str()).unwrap_or("list").to_string()
            };
            match action.as_str() {
                "list" => {
                    let BoardListArgs { status, include_hidden } =
                        serde_json::from_value(args).unwrap_or(BoardListArgs { status: None, include_hidden: None });
                    let tasks = state.mission.db()
                        .list_board_tasks(status.as_deref(), include_hidden.unwrap_or(false))
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json_pretty(&tasks))
                }
                "get" => {
                    board_get(state, args).await
                }
                "search" => {
                    let input: missiond_core::types::BoardSearchInput =
                        serde_json::from_value(args).unwrap_or_default();
                    let result = state.mission.db()
                        .search_board_tasks(&input)
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json_pretty(&result))
                }
                "summary" => {
                    #[derive(Deserialize)]
                    struct SummaryArgs { since: Option<String> }
                    let a: SummaryArgs = serde_json::from_value(args)?;
                    let summary = state.mission.db()
                        .board_summary(a.since.as_deref())
                        .map_err(|e| anyhow!("DB error: {}", e))?;
                    Ok(ToolResult::json_pretty(&summary))
                }
                _ => Ok(ToolResult::error(format!("Unknown board_query action: {action}. Use: list, get, search, summary"))),
            }
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

            publish_board_created(state, &task);
            Ok(ToolResult::json_pretty(&task))
        }
        // ===== Unified Update (single + batch, absorbs toggle) =====
        "mission_board_update" | "mission_board_batch_update" | "mission_board_toggle" => {
            // Detect batch mode: explicit ids array or old batch_update tool name
            let has_ids = args.get("ids").and_then(|v| v.as_array()).map(|a| !a.is_empty()).unwrap_or(false);

            if has_ids {
                // === Batch update path ===
                let ids = args.get("ids").unwrap().as_array().unwrap()
                    .iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>();
                let is_marking_done = args.get("status").and_then(|v| v.as_str()).map(|s| s == "done").unwrap_or(false);
                let is_status_change = args.get("status").and_then(|v| v.as_str()).is_some();
                let update_template: missiond_core::types::UpdateBoardTaskInput = serde_json::from_value(args)?;
                let mut results = Vec::new();
                let (mut success_count, mut fail_count) = (0u32, 0u32);
                for id in &ids {
                    let old_status = if is_status_change {
                        state.mission.db().get_board_task(id).ok().flatten().map(|t| format!("{:?}", t.status))
                    } else { None };
                    match state.mission.db().update_board_task(id, &update_template) {
                        Ok(Some(t)) => {
                            if let Some(old) = old_status { publish_board_status_changed(state, &t, &old); }
                            else { publish_board_update(state, &t); }
                            if is_marking_done {
                                let state = state.clone(); let task_id = t.id.clone(); let task_title = t.title.clone();
                                tokio::spawn(async move { harvest_decisions_for_task(&state, &task_id, &task_title).await; });
                            }
                            success_count += 1;
                            results.push(serde_json::json!({"id": t.id, "title": t.title, "status": format!("{:?}", t.status), "ok": true}));
                        }
                        Ok(None) => { fail_count += 1; results.push(serde_json::json!({"id": id, "ok": false, "error": "not found"})); }
                        Err(e) => { fail_count += 1; results.push(serde_json::json!({"id": id, "ok": false, "error": e.to_string()})); }
                    }
                }
                Ok(ToolResult::json(&serde_json::json!({"total": ids.len(), "success": success_count, "failed": fail_count, "results": results})))
            } else if name == "mission_board_toggle" {
                // === Toggle path (backward compat) ===
                let BoardIdArgs { id } = serde_json::from_value(args)?;
                let old_status = state.mission.db().get_board_task(&id).ok().flatten()
                    .map(|t| format!("{:?}", t.status)).unwrap_or_else(|| "unknown".to_string());
                let task = state.mission.db().toggle_board_task(&id).map_err(|e| anyhow!("DB error: {}", e))?;
                match task {
                    Some(t) => {
                        publish_board_status_changed(state, &t, &old_status);
                        if t.status == missiond_core::types::BoardTaskStatus::Done {
                            let state = state.clone(); let task_id = t.id.clone(); let task_title = t.title.clone();
                            tokio::spawn(async move { harvest_decisions_for_task(&state, &task_id, &task_title).await; });
                        }
                        Ok(ToolResult::json_pretty(&t))
                    }
                    None => Ok(ToolResult::error("Task not found")),
                }
            } else {
                // === Single update path ===
                let args_val: Value = args;
                let id = args_val.get("id").and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Either 'id' or 'ids' is required"))?.to_string();
                let is_status_change = args_val.get("status").and_then(|v| v.as_str()).is_some();
                let is_marking_done = args_val.get("status").and_then(|v| v.as_str()).map(|s| s == "done").unwrap_or(false);
                let old_status = if is_status_change {
                    state.mission.db().get_board_task(&id).ok().flatten().map(|t| format!("{:?}", t.status))
                } else { None };
                let update: missiond_core::types::UpdateBoardTaskInput = serde_json::from_value(args_val)?;
                let task = state.mission.db().update_board_task(&id, &update).map_err(|e| anyhow!("DB error: {}", e))?;
                match task {
                    Some(t) => {
                        if let Some(old) = old_status { publish_board_status_changed(state, &t, &old); }
                        else { publish_board_update(state, &t); }
                        if is_marking_done {
                            let state = state.clone(); let task_id = t.id.clone(); let task_title = t.title.clone();
                            tokio::spawn(async move { harvest_decisions_for_task(&state, &task_id, &task_title).await; });
                        }
                        Ok(ToolResult::json_pretty(&t))
                    }
                    None => Ok(ToolResult::error("Task not found")),
                }
            }
        }
        "mission_board_delete" => {
            let BoardIdArgs { id } = serde_json::from_value(args)?;
            // Fetch task info before deletion for event
            let task_title = state.mission.db().get_board_task(&id).ok().flatten()
                .map(|t| t.title.clone()).unwrap_or_default();
            let deleted = state
                .mission
                .db()
                .delete_board_task(&id)
                .map_err(|e| anyhow!("DB error: {}", e))?;
            if deleted > 0 {
                state.event_bus.publish_traced(
                    DaemonEvent::BoardTaskDeleted {
                        task_id: id.clone(),
                        title: task_title.clone(),
                    },
                    TraceContext {
                        trace_id: Some(id.clone()),
                        summary: Some(format!("board: deleted {}", task_title)),
                        ..Default::default()
                    },
                );
            }
            Ok(ToolResult::json(&serde_json::json!({
                "deleted": deleted,
                "id": id,
            })))
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
                Ok(Some(task)) => {
                    state.event_bus.publish_traced(
                        DaemonEvent::BoardTaskClaimed {
                            task_id: task.id.clone(),
                            slot_id: executor_id.to_string(),
                        },
                        TraceContext {
                            trace_id: Some(task.id.clone()),
                            summary: Some(format!("board: {} claimed by {}", task.title, executor_id)),
                            ..Default::default()
                        },
                    );
                    Ok(ToolResult::json_pretty(&task))
                }
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
            let task_id = args.task_id.clone();
            let content_preview: String = args.content.chars().take(80).collect();
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
            state.event_bus.publish_traced(
                DaemonEvent::BoardTaskNoteAdded {
                    task_id: task_id.clone(),
                    note_id: note.id.clone(),
                    content_preview: content_preview.clone(),
                },
                TraceContext {
                    trace_id: Some(task_id),
                    summary: Some(format!("board note: {}", content_preview)),
                    ..Default::default()
                },
            );
            Ok(ToolResult::json_pretty(&note))
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
                        // Emit dispatch event for timeline visibility
                        let preview = if decompose_prompt.len() > 200 {
                            let mut end = 200;
                            while end > 0 && !decompose_prompt.is_char_boundary(end) { end -= 1; }
                            format!("{}...", &decompose_prompt[..end])
                        } else { decompose_prompt.clone() };
                        state.event_bus.publish(crate::event_bus::DaemonEvent::SlotTaskDispatched {
                            slot_id: slot_id.clone(),
                            task_id: Some(submit_task_id.clone()),
                            purpose: "decompose".to_string(),
                            prompt_chars: decompose_prompt.len(),
                            preview,
                            cited_kb_ids: vec![],
                        });
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
                .retry_board_task(&task.id, args.reset_downstream)
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
