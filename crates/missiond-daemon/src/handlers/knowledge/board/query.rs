use super::*;

#[derive(Deserialize)]
struct BoardListArgs {
    #[serde(default)]
    status: Option<String>,
    #[serde(
        default,
        rename = "includeHidden",
        deserialize_with = "lenient::option_bool"
    )]
    include_hidden: Option<bool>,
    #[serde(default)]
    project: Option<String>,
}

/// Extracted board_get logic (used by query action="get").
async fn board_get(state: &AppState, args: Value) -> Result<ToolResult> {
    let include_children = args
        .get("includeChildren")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let ids: Vec<String> = if let Some(id_val) = args.get("ids").and_then(|v| v.as_array()) {
        id_val
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    } else if let Some(id) = args.get("id").and_then(|v| v.as_str()) {
        vec![id.to_string()]
    } else {
        return Ok(ToolResult::error("Either 'id' or 'ids' is required"));
    };
    let single_mode = args.get("ids").is_none() && args.get("id").is_some();
    if include_children || ids.len() > 1 {
        let results = state
            .store
            .get_board_tasks_with_context(&ids, include_children)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        if results.is_empty() {
            return Ok(ToolResult::error("Task not found"));
        }
        if single_mode {
            Ok(ToolResult::json_pretty(&results[0]))
        } else {
            Ok(ToolResult::json_pretty(&results))
        }
    } else {
        let task = state
            .store
            .get_board_task_with_notes(&ids[0])
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        match task {
            Some(ref t) => {
                super::record_session_task_binding(state, t.task.id.as_str(), &t.task.title);
                Ok(ToolResult::json_pretty(&t))
            }
            None => Ok(ToolResult::error("Task not found")),
        }
    }
}

pub(super) async fn handle_query(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Determine action from tool name or action parameter.
    let action = if name != "mission_board_query" {
        // Backward compat: old tool names map to actions.
        name.trim_start_matches("mission_board_").to_string()
    } else {
        args.get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list")
            .to_string()
    };
    match action.as_str() {
        "list" => {
            let BoardListArgs {
                status,
                include_hidden,
                project,
            } = serde_json::from_value(args).unwrap_or(BoardListArgs {
                status: None,
                include_hidden: None,
                project: None,
            });
            let mut tasks = state
                .store
                .list_board_tasks(status.as_deref(), include_hidden.unwrap_or(false))
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            if let Some(ref proj) = project {
                tasks
                    .retain(|t| t.project.as_deref() == Some(proj.as_str()) || t.project.is_none());
            }
            Ok(ToolResult::json_pretty(&tasks))
        }
        "get" => board_get(state, args).await,
        "search" => {
            let input: missiond_core::types::BoardSearchInput =
                serde_json::from_value(args).unwrap_or_default();
            let result = state
                .store
                .search_board_tasks(&input)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&result))
        }
        "summary" => {
            #[derive(Deserialize)]
            struct SummaryArgs {
                since: Option<String>,
            }
            let a: SummaryArgs = serde_json::from_value(args)?;
            let summary = state
                .store
                .board_summary(a.since.as_deref())
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&summary))
        }
        "clear_done" => {
            let deleted = state
                .store
                .clear_done_board_tasks()
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::text(format!(
                "Cleared {} completed tasks",
                deleted
            )))
        }
        _ => Ok(ToolResult::error(format!(
            "Unknown board_query action: {action}. Use: list, get, search, summary, clear_done"
        ))),
    }
}
