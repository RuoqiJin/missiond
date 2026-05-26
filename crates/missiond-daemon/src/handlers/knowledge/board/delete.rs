use super::*;

#[derive(Deserialize)]
struct BoardIdArgs {
    id: String,
}

pub(super) async fn handle_delete(state: &AppState, args: Value) -> Result<ToolResult> {
    let BoardIdArgs { id } = serde_json::from_value(args)?;
    let storage = state.storage_plane();
    // Fetch task info before deletion for event.
    let task_title = storage
        .ports
        .get_board_task(&id)
        .await
        .ok()
        .flatten()
        .map(|t| t.title.clone())
        .unwrap_or_default();
    let deleted = storage
        .ports
        .delete_board_task(&id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    if deleted > 0 {
        let ev = BoardEvent::Deleted {
            task_id: id.clone(),
            title: task_title.clone(),
        };
        crate::engine::master_control::notify_board_event_direct(&ev);
        let event_plane = state.event_plane();
        let _ = event_plane.bus.publish_board_event(ev).await;
    }
    Ok(ToolResult::json(&serde_json::json!({
        "deleted": deleted,
        "id": id,
    })))
}
