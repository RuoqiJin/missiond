use super::*;

#[derive(Deserialize)]
struct BoardIdArgs {
    id: String,
}

pub(super) async fn handle_update(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Detect batch mode: explicit ids array or old batch_update tool name.
    let has_ids = args
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);

    if has_ids {
        handle_batch_update(state, args).await
    } else if name == "mission_board_toggle" {
        handle_toggle(state, args).await
    } else {
        handle_single_update(state, args).await
    }
}

async fn handle_batch_update(state: &AppState, args: Value) -> Result<ToolResult> {
    let ids = args
        .get("ids")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect::<Vec<_>>();
    let is_marking_done = args
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s == "done")
        .unwrap_or(false);
    let is_status_change = args.get("status").and_then(|v| v.as_str()).is_some();
    let update_template: missiond_core::types::UpdateBoardTaskInput = serde_json::from_value(args)?;
    let mut results = Vec::new();
    let (mut success_count, mut fail_count) = (0u32, 0u32);
    for id in &ids {
        let old_status = if is_status_change {
            state
                .store
                .get_board_task(id)
                .await
                .ok()
                .flatten()
                .map(|t| format!("{:?}", t.status))
        } else {
            None
        };
        match state.store.update_board_task(id, &update_template).await {
            Ok(Some(t)) => {
                if let Some(old) = old_status {
                    super::publish_board_status_changed(state, &t, &old);
                } else {
                    super::publish_board_update(state, &t);
                }
                if is_marking_done {
                    let state = state.clone();
                    let task_id = t.id.to_string();
                    let task_title = t.title.clone();
                    tokio::spawn(async move {
                        harvest_decisions_for_task(&state, &task_id, &task_title).await;
                    });
                }
                success_count += 1;
                results.push(serde_json::json!({"id": t.id, "title": t.title, "status": format!("{:?}", t.status), "ok": true}));
            }
            Ok(None) => {
                fail_count += 1;
                results.push(serde_json::json!({"id": id, "ok": false, "error": "not found"}));
            }
            Err(e) => {
                fail_count += 1;
                results.push(serde_json::json!({"id": id, "ok": false, "error": e.to_string()}));
            }
        }
    }
    Ok(ToolResult::json(
        &serde_json::json!({"total": ids.len(), "success": success_count, "failed": fail_count, "results": results}),
    ))
}

async fn handle_toggle(state: &AppState, args: Value) -> Result<ToolResult> {
    let BoardIdArgs { id } = serde_json::from_value(args)?;
    let old_status = state
        .store
        .get_board_task(&id)
        .await
        .ok()
        .flatten()
        .map(|t| format!("{:?}", t.status))
        .unwrap_or_else(|| "unknown".to_string());
    let task = state
        .store
        .toggle_board_task(&id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    match task {
        Some(t) => {
            super::publish_board_status_changed(state, &t, &old_status);
            if t.status == missiond_core::types::BoardTaskStatus::Done {
                let state = state.clone();
                let task_id = t.id.to_string();
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

async fn handle_single_update(state: &AppState, args: Value) -> Result<ToolResult> {
    let args_val: Value = args;
    let id = args_val
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Either 'id' or 'ids' is required"))?
        .to_string();
    let is_status_change = args_val.get("status").and_then(|v| v.as_str()).is_some();
    let is_marking_done = args_val
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s == "done")
        .unwrap_or(false);
    let old_status = if is_status_change {
        state
            .store
            .get_board_task(&id)
            .await
            .ok()
            .flatten()
            .map(|t| format!("{:?}", t.status))
    } else {
        None
    };
    let update: missiond_core::types::UpdateBoardTaskInput = serde_json::from_value(args_val)?;
    let task = state
        .store
        .update_board_task(&id, &update)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    match task {
        Some(t) => {
            super::record_session_task_binding(state, t.id.as_str(), &t.title);
            if let Some(old) = old_status {
                super::publish_board_status_changed(state, &t, &old);
            } else {
                super::publish_board_update(state, &t);
            }
            if is_marking_done {
                let state = state.clone();
                let task_id = t.id.to_string();
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
