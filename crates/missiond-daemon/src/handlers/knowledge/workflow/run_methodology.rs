use super::*;

// ───────────────────────────────────────────────────────────────────────
// run_methodology — resolve compiled YAML, dispatch into flow engine
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_run_methodology(state: &AppState, args: &Value) -> Result<ToolResult> {
    let project_root = match super::project_root::resolve_project_root_from_args(state, args).await
    {
        Ok(p) => p,
        Err(reason) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::INVALID_PARAM, reason).with_suggestion(
                    "supply `project` (registered id) or absolute `cwd`; \
                     run_methodology refuses process-cwd fallback so the compiled YAML \
                     resolves against the registered project root.",
                ),
            ))
        }
    };
    let dry_run = args
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let flow_id_arg = args.get("flow_id").and_then(|v| v.as_str());
    let flow_path_arg = args.get("flow_path").and_then(|v| v.as_str());
    let name_arg = args.get("name").and_then(|v| v.as_str());

    let resolved = match resolve_compiled_flow(&project_root, flow_id_arg, flow_path_arg, name_arg)
    {
        Ok(r) => r,
        Err(CompiledFlowError::MissingArgs) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::MISSING_PARAM,
                    "run_methodology requires `flow_id`, `flow_path`, or `name`",
                ),
            ))
        }
        Err(CompiledFlowError::Missing { flow_id, expected }) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    "MISSING_COMPILED_FLOW: no compiled YAML on disk for the requested methodology",
                )
                .with_suggestion(format!(
                    "call mission_workflow(action=compile_methodology, compile_mode=\"deterministic\", persist=true, name=<methodology>, output_flow_id=\"{}\") to generate {}",
                    flow_id,
                    expected.display()
                )),
            ))
        }
    };

    let raw = std::fs::read_to_string(&resolved.path)
        .map_err(|e| anyhow!("read {}: {}", resolved.path.display(), e))?;
    let flow: crate::engine::flow::FlowDefinition = serde_yaml::from_str(&raw)
        .map_err(|e| anyhow!("parse {}: {}", resolved.path.display(), e))?;

    if dry_run {
        return Ok(ToolResult::json_pretty(&json!({
            "status": "would_run",
            "flow_ref": "F-methodology-to-executable-compile :: s6 dry-run-or-run (dry_run)",
            "flow_id": flow.id,
            "flow_path": resolved.path.display().to_string(),
            "node_count": flow.nodes.len(),
            "node_ids": flow.nodes.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
            "params_echo": args.get("params").cloned().unwrap_or(Value::Null),
            "next_step": "pass dry_run=false to dispatch into mission_flow_run on this compiled YAML",
        })));
    }

    // dry_run=false → dispatch through flow engine.
    let title = format!("Methodology: {}", flow.name);
    let input = missiond_core::types::CreateBoardTaskInput {
        title,
        category: Some("methodology".to_string()),
        description: Some(format!(
            "compiled methodology flow `{}` — source: {}",
            flow.id,
            resolved.path.display()
        )),
        flow_template: Some(flow.id.clone()),
        ..Default::default()
    };
    let task = state
        .store
        .create_board_task(&input)
        .await
        .map_err(|e| anyhow!("DB: {}", e))?;
    let task_id = task.id.to_string();

    let mut ctx = crate::engine::flow::FlowContext::new();
    if let Some(params) = args.get("params").and_then(|v| v.as_object()) {
        for (k, v) in params {
            let value = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            ctx.set(k.clone(), value);
        }
    }

    let _ = state
        .store
        .update_board_task(
            &task_id,
            &missiond_core::types::UpdateBoardTaskInput {
                flow_phase: Some("running".to_string()),
                flow_context: Some(serde_json::to_string(&ctx).unwrap_or_default()),
                status: Some("running".to_string()),
                ..Default::default()
            },
        )
        .await;

    let run_result = crate::engine::flow::runner::run_flow(state, &flow, &mut ctx, &task_id).await;
    match run_result {
        Ok(()) => {
            let _ = state
                .store
                .update_board_task(
                    &task_id,
                    &missiond_core::types::UpdateBoardTaskInput {
                        flow_phase: Some("completed".to_string()),
                        status: Some("done".to_string()),
                        ..Default::default()
                    },
                )
                .await;
            Ok(ToolResult::json_pretty(&json!({
                "status": "dispatched",
                "flow_ref": "F-methodology-to-executable-compile :: s6 dry-run-or-run (run)",
                "flow_id": flow.id,
                "flow_path": resolved.path.display().to_string(),
                "task_id": task_id,
                "completed_nodes": ctx.completed_nodes,
                "record_execution_status": "TODO_external — methodology compile flows are not yet linked to a workflow row; call mission_workflow(action=record_execution) manually with the matching workflow_id once distilled",
            })))
        }
        Err(e) => {
            let _ = state
                .store
                .update_board_task(
                    &task_id,
                    &missiond_core::types::UpdateBoardTaskInput {
                        flow_phase: Some("failed".to_string()),
                        status: Some("failed".to_string()),
                        ..Default::default()
                    },
                )
                .await;
            Err(e)
        }
    }
}
