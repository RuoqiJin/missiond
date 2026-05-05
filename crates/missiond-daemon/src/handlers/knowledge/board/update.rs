use super::*;

#[derive(Deserialize)]
struct BoardIdArgs {
    id: String,
}

pub(super) async fn handle_update(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    let args = super::normalize_board_args(args);
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
    if let Some(status) = args.get("status").and_then(|v| v.as_str()) {
        if missiond_core::types::BoardTaskStatus::from_str(status).is_none() {
            return Ok(invalid_status_result(status));
        }
    }
    let update_template: missiond_core::types::UpdateBoardTaskInput =
        match serde_json::from_value(args) {
            Ok(update) => update,
            Err(err) => return Ok(super::invalid_board_args("mission_board_update", err)),
        };
    if is_marking_done {
        if let Some(blocked) = guard_done_close_against_code_drift(state).await? {
            return Ok(blocked);
        }
    }
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
    let BoardIdArgs { id } = match serde_json::from_value(args) {
        Ok(args) => args,
        Err(err) => return Ok(super::invalid_board_args("mission_board_toggle", err)),
    };
    let Some(existing) = state
        .store
        .get_board_task(&id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    else {
        return Ok(not_found_result("mission_board_toggle", &id));
    };
    if existing.status != missiond_core::types::BoardTaskStatus::Done {
        if let Some(blocked) = guard_done_close_against_code_drift(state).await? {
            return Ok(blocked);
        }
    }
    let old_status = format!("{:?}", existing.status);
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
        None => Ok(not_found_result("mission_board_toggle", &id)),
    }
}

async fn handle_single_update(state: &AppState, args: Value) -> Result<ToolResult> {
    let args_val: Value = args;
    let Some(id) = args_val
        .get("id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
    else {
        return Ok(ToolResult::structured_error(
            missiond_mcp::tools::ToolError::new(
                missiond_mcp::tools::error_codes::MISSING_PARAM,
                "mission_board_update requires either id or non-empty ids",
            )
            .with_suggestion("pass id for a single task update or ids for a batch update"),
        ));
    };
    let is_status_change = args_val.get("status").and_then(|v| v.as_str()).is_some();
    let is_marking_done = args_val
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s == "done")
        .unwrap_or(false);
    if is_marking_done {
        if let Some(blocked) = guard_done_close_against_code_drift(state).await? {
            return Ok(blocked);
        }
    }
    if let Some(status) = args_val.get("status").and_then(|v| v.as_str()) {
        if missiond_core::types::BoardTaskStatus::from_str(status).is_none() {
            return Ok(invalid_status_result(status));
        }
    }
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
    let update: missiond_core::types::UpdateBoardTaskInput = match serde_json::from_value(args_val)
    {
        Ok(update) => update,
        Err(err) => return Ok(super::invalid_board_args("mission_board_update", err)),
    };
    let task = match state.store.update_board_task(&id, &update).await {
        Ok(task) => task,
        Err(err) => return Ok(super::board_store_error("mission_board_update", err)),
    };
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
        None => Ok(not_found_result("mission_board_update", &id)),
    }
}

fn invalid_status_result(status: &str) -> ToolResult {
    ToolResult::structured_error(
        missiond_mcp::tools::ToolError::new(
            missiond_mcp::tools::error_codes::INVALID_PARAM,
            format!("mission_board_update invalid status: {status}"),
        )
        .with_suggestion("use one of: open, running, verifying, done, blocked, failed, skipped"),
    )
}

fn not_found_result(tool: &str, id: &str) -> ToolResult {
    ToolResult::structured_error(
        missiond_mcp::tools::ToolError::new(
            missiond_mcp::tools::error_codes::NOT_FOUND,
            format!("{tool} task not found: {id}"),
        )
        .with_suggestion(
            "verify the BoardTask id; short ids are accepted only when they resolve uniquely",
        ),
    )
}

async fn guard_done_close_against_code_drift(state: &AppState) -> Result<Option<ToolResult>> {
    let backfill_task_id =
        crate::engine::master_control::ensure_code_drift_backfill_task_for_state(state)
            .await
            .map_err(|e| anyhow!("code drift guard failed: {}", e))?;
    let Some(backfill_task_id) = backfill_task_id else {
        return Ok(None);
    };
    Ok(Some(ToolResult::error(format!(
        "BoardTask close blocked by Lisp/code drift. Code changes under crates/packages/scripts have no same-diff Lisp or evidence update. Backfill task created or reused: {backfill_task_id}. Complete the backfill before marking this task done."
    ))))
}
