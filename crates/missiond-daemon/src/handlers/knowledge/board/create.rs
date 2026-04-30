use super::*;

pub(super) async fn handle_create(state: &AppState, args: Value) -> Result<ToolResult> {
    let input: missiond_core::types::CreateBoardTaskInput = serde_json::from_value(args)?;
    let mut task = state
        .store
        .create_board_task(&input)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    // If flowTemplate is set, initialize flow fields.
    if input.flow_template.is_some() {
        let flow_phase = "investigate".to_string();
        let flow_ctx = serde_json::to_string(&missiond_core::types::FlowContext::default())
            .unwrap_or_else(|_| "{}".to_string());
        let updated = state
            .store
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
            .map_err(|e| anyhow!("DB error setting flow: {}", e))?;
        if let Some(t) = updated {
            task = t;
        }
    }

    super::publish_board_created(state, &task);
    Ok(ToolResult::json_pretty(&task))
}
