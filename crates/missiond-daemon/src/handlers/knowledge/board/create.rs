use super::*;

pub(super) async fn handle_create(state: &AppState, args: Value) -> Result<ToolResult> {
    let input: missiond_core::types::CreateBoardTaskInput =
        match serde_json::from_value(super::normalize_board_args(args)) {
            Ok(input) => input,
            Err(err) => return Ok(super::invalid_board_args("mission_board_create", err)),
        };
    if input.title.trim().is_empty() {
        return Ok(ToolResult::structured_error(
            missiond_mcp::tools::ToolError::new(
                missiond_mcp::tools::error_codes::INVALID_PARAM,
                "mission_board_create invalid arguments: title must be non-empty",
            )
            .with_suggestion("supply a concise title and put long details in description"),
        ));
    }
    let storage = state.storage_plane();
    let mut task = match storage.ports.create_board_task(&input).await {
        Ok(task) => task,
        Err(err) => return Ok(super::board_store_error("mission_board_create", err)),
    };
    let runtime_metadata = ensure_board_create_capabilities(state, &task).await?;
    if let Some(runtime_metadata) = runtime_metadata {
        match storage
            .ports
            .update_board_task(
                task.id.as_str(),
                &missiond_core::types::UpdateBoardTaskInput {
                    runtime_metadata: Some(runtime_metadata),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(Some(updated)) => task = updated,
            Ok(None) => {}
            Err(err) => return Ok(super::board_store_error("mission_board_create grants", err)),
        }
    }
    if let Err(err) = state
        .shared_memory
        .upsert_task_contract_from_metadata(
            task.id.as_str(),
            task.project.as_deref(),
            &task.runtime_metadata,
        )
        .await
    {
        return Ok(ToolResult::structured_error(
            missiond_mcp::tools::ToolError::new(
                missiond_mcp::tools::error_codes::DB_ERROR,
                format!("mission_board_create task contract failed: {err}"),
            )
            .with_suggestion(
                "retry after migrations are applied; task_contracts is required for control-plane tasks",
            ),
        ));
    }

    // If flowTemplate is set, initialize flow fields.
    if input.flow_template.is_some() {
        let flow_phase = "investigate".to_string();
        let flow_ctx = serde_json::to_string(&missiond_core::types::FlowContext::default())
            .unwrap_or_else(|_| "{}".to_string());
        let updated = match storage
            .ports
            .update_board_task(
                task.id.as_str(),
                &missiond_core::types::UpdateBoardTaskInput {
                    flow_phase: Some(flow_phase),
                    flow_context: Some(flow_ctx),
                    flow_template: input.flow_template.clone(),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(updated) => updated,
            Err(err) => {
                return Ok(super::board_store_error(
                    "mission_board_create flow init",
                    err,
                ));
            }
        };
        if let Some(t) = updated {
            task = t;
        }
    }

    super::publish_board_created(state, &task);
    Ok(ToolResult::json_pretty(&task))
}

async fn ensure_board_create_capabilities(
    state: &AppState,
    task: &missiond_core::types::BoardTask,
) -> Result<Option<serde_json::Value>> {
    let mut metadata = task.runtime_metadata.clone();
    if !metadata.is_object() {
        metadata = serde_json::json!({});
    }
    let existing_grants = metadata_string_list(metadata.get("capability_grant_ids"));
    if !existing_grants.is_empty() {
        return Ok(None);
    }
    let Some(subject_id) = task
        .assignee
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let read_scope = metadata_string_list(metadata.get("read_scope"));
    let write_scope = metadata_string_list(metadata.get("write_scope"));
    let must_not_touch = metadata_string_list(metadata.get("must_not_touch"));
    let grant_ids = state
        .shared_memory
        .grant_task_capabilities(
            task.project.as_deref(),
            task.id.as_str(),
            "worker",
            subject_id,
            &read_scope,
            &write_scope,
            &must_not_touch,
            "mission_board_create",
        )
        .await
        .map_err(|err| anyhow!("capability grant error: {}", err))?;
    if let Some(fields) = metadata.as_object_mut() {
        fields.insert(
            "capability_grant_ids".to_string(),
            serde_json::Value::Array(
                grant_ids
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
    }
    Ok(Some(metadata))
}

fn metadata_string_list(value: Option<&serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .filter_map(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        Some(serde_json::Value::String(value)) => value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "[]")
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}
