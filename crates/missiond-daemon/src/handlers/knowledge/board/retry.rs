use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RetryArgs {
    task_id: String,
    #[serde(default = "default_true")]
    reset_downstream: bool,
}

fn default_true() -> bool {
    true
}

pub(super) async fn handle_retry(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: RetryArgs = serde_json::from_value(args)?;
    let storage = state.storage_plane();

    // Verify task exists.
    let task = storage
        .ports
        .get_board_task(&args.task_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
        .ok_or_else(|| anyhow!("Task not found: {}", args.task_id))?;

    let reset_ids = storage
        .ports
        .retry_board_task(task.id.as_str(), args.reset_downstream)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    // Write note.
    let _ = storage
        .ports
        .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
            task_id: task.id.to_string(),
            content: format!(
                "🔄 任务重试\n- 重置任务数: {}\n- 级联下游: {}",
                reset_ids.len(),
                if args.reset_downstream { "是" } else { "否" }
            ),
            note_type: Some("progress".to_string()),
            author: Some("retry".to_string()),
        })
        .await;

    Ok(ToolResult::text(format!(
        "✅ 已重试任务 '{}'\n- 重置任务数: {}\n- 重置的任务 ID: {:?}",
        task.title,
        reset_ids.len(),
        reset_ids
    )))
}
