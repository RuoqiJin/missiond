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
