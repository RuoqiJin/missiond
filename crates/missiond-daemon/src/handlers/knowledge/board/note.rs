use super::*;

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

pub(super) async fn handle_note_add(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: BoardNoteAddArgs = serde_json::from_value(args)?;
    let task_id = args.task_id.clone();
    let content_preview: String = args.content.chars().take(80).collect();
    let is_master_control_note =
        args.author.as_deref() == Some(crate::engine::master_control::MASTER_WORKER_ID);
    let input = missiond_core::types::AddBoardTaskNoteInput {
        task_id: args.task_id,
        content: args.content,
        note_type: args.note_type,
        author: args.author,
    };
    let note = state
        .store
        .add_board_task_note(&input)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    // Refresh binding: session is actively updating this task.
    if let Ok(Some(task)) = state.store.get_board_task(&task_id).await {
        super::record_session_task_binding(state, task.id.as_str(), &task.title);
    }
    let ev = BoardEvent::NoteAdded {
        task_id: task_id.clone(),
        note_id: note.id.clone(),
        content_preview: content_preview.clone(),
    };
    if !is_master_control_note {
        crate::engine::master_control::notify_board_event_direct(&ev);
    }
    let _ = state.bus.publish_board(ev).await;
    Ok(ToolResult::json_pretty(&note))
}
