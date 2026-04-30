use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DecomposeArgs {
    task_id: String,
    slot_id: Option<String>,
    hints: Option<String>,
}

pub(super) async fn handle_decompose(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: DecomposeArgs = serde_json::from_value(args)?;
    let slot_id = args.slot_id.unwrap_or_else(|| "slot-coder-1".to_string());

    // Get the task.
    let task = state
        .store
        .get_board_task(&args.task_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
        .ok_or_else(|| anyhow!("Task not found: {}", args.task_id))?;

    // Validate: must be open.
    if task.status != missiond_core::types::BoardTaskStatus::Open {
        return Ok(ToolResult::text(format!(
            "Error: 任务状态为 {:?}，只能拆分 open 状态的任务",
            task.status
        )));
    }

    // Check if already has subtasks.
    let subtasks = state
        .store
        .list_board_tasks(None, true)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
        .into_iter()
        .filter(|t| t.parent_id.as_ref() == Some(&task.id))
        .count();
    if subtasks > 0 {
        return Ok(ToolResult::text(format!(
            "Error: 任务已有 {} 个子任务，不能重复拆分。如需重新拆分，请先删除现有子任务。",
            subtasks
        )));
    }

    // Build decompose prompt.
    let hints_section = args
        .hints
        .map(|h| format!("\n### 用户提示\n{}", h))
        .unwrap_or_default();

    // Inject context from Skills + KB.
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

    // Submit as a task to the slot.
    let submit_task_id =
        crate::state::submit_task(state.store.as_ref(), "coder", &decompose_prompt).await?;

    // Store target slot.
    let _ = state
        .store
        .update_task(
            &submit_task_id,
            &missiond_core::types::TaskUpdate {
                slot_id: Some(slot_id.clone()),
                ..Default::default()
            },
        )
        .await;

    // Try immediate dispatch.
    if let Some(status) = state.pty.get_status(&slot_id).await {
        if status.state == missiond_core::pty::SessionState::Idle {
            if let Ok(()) = state
                .pty
                .send_fire_and_forget(&slot_id, &decompose_prompt)
                .await
            {
                let now = chrono::Utc::now().timestamp_millis();
                let slot_session = state.store.get_slot_session(&slot_id).await.ok().flatten();
                let _ = state
                    .store
                    .update_task(
                        &submit_task_id,
                        &missiond_core::types::TaskUpdate {
                            status: Some(missiond_core::types::TaskStatus::Running),
                            slot_id: Some(slot_id.clone()),
                            session_id: slot_session,
                            started_at: Some(now),
                            ..Default::default()
                        },
                    )
                    .await;
                // Emit dispatch event for timeline visibility.
                let preview = if decompose_prompt.len() > 200 {
                    let mut end = 200;
                    while end > 0 && !decompose_prompt.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}...", &decompose_prompt[..end])
                } else {
                    decompose_prompt.clone()
                };
                let ev = SlotEvent::TaskDispatched {
                    slot_id: slot_id.clone(),
                    task_id: Some(submit_task_id.clone()),
                    purpose: "decompose".to_string(),
                    prompt_chars: decompose_prompt.len(),
                    preview,
                    cited_kb_ids: vec![],
                };
                let _ = state.bus.publish_slot(ev).await;
            }
        }
    }

    // Write note on parent task.
    let _ = state
        .store
        .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
            task_id: task.id.to_string(),
            content: format!(
                "🔀 任务拆分已启动 → submit task {} → slot {}",
                submit_task_id, slot_id
            ),
            note_type: Some("progress".to_string()),
            author: Some("decompose".to_string()),
        })
        .await;

    Ok(ToolResult::text(format!(
        "✅ 拆分任务已派发\n- Submit Task: {}\n- 工位: {}\n- 父任务: {} ({})\n\n工位将调查代码后自动创建子任务。用 mission_task_track(taskId: \"{}\") 追踪进度。",
        submit_task_id, slot_id, task.title, task.id, submit_task_id
    )))
}
