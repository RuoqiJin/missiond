use super::*;

// ───────────────────────────────────────────────────────────────────────
// run_methodology — resolve compiled YAML, dispatch into flow engine
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub(super) struct RunMethodologyRecordIntent {
    pub workflow_id: Option<uuid::Uuid>,
    pub cost_usd: Option<f64>,
}

pub(super) fn parse_run_methodology_record_intent(
    args: &Value,
) -> Result<RunMethodologyRecordIntent> {
    let workflow_id = match args.get("workflow_id").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => Some(parse_id_arg(args, "workflow_id")?),
        _ => None,
    };
    Ok(RunMethodologyRecordIntent {
        workflow_id,
        cost_usd: args.get("cost_usd").and_then(|v| v.as_f64()),
    })
}

pub(super) fn methodology_execution_record_payload(
    flow_id: &str,
    intent: &RunMethodologyRecordIntent,
) -> Value {
    match intent.workflow_id {
        Some(workflow_id) => json!({
            "status": "recorded",
            "mode": "workflow_row",
            "workflow_id": workflow_id,
            "success": true,
            "cost_usd": intent.cost_usd,
        }),
        None => json!({
            "status": "artifact_only_no_workflow_row",
            "mode": "methodology_flow",
            "flow_id": flow_id,
            "success": true,
            "note": "run_methodology executed the compiled methodology YAML and recorded the BoardTask result; pass workflow_id when this methodology flow is linked to a persisted Workflow row and MissionD should update workflow execution statistics.",
        }),
    }
}

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
            ));
        }
    };
    let dry_run = args
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let flow_id_arg = args.get("flow_id").and_then(|v| v.as_str());
    let flow_path_arg = args.get("flow_path").and_then(|v| v.as_str());
    let name_arg = args.get("name").and_then(|v| v.as_str());
    let record_intent = parse_run_methodology_record_intent(args)?;

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
            "record_execution_preview": methodology_execution_record_payload(&flow.id, &record_intent),
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
                        ..Default::default()
                    },
                )
                .await;
            crate::engine::control_plane_kernel::ControlPlaneKernel::new(state)
                .complete_system_task(
                    crate::engine::control_plane_kernel::SystemTaskCompletionInput {
                        task_id: task_id.clone(),
                        project_id: None,
                        producer_id: "workflow_run_methodology".to_string(),
                        summary: format!("Workflow methodology `{}` completed.", flow.id),
                        content: Some(format!(
                            "Workflow methodology `{}` completed through the flow runner.",
                            flow.id
                        )),
                        raw_evidence: json!({
                            "kind": "workflow_methodology_run",
                            "flow_id": flow.id,
                            "workflow_id": record_intent.workflow_id,
                            "cost_usd": record_intent.cost_usd
                        }),
                        evidence_refs: vec![json!({
                            "kind": "workflow_methodology_run",
                            "flow_id": flow.id
                        })],
                        result_status: "completed".to_string(),
                        metadata: json!({
                            "flow_id": flow.id,
                            "workflow_id": record_intent.workflow_id,
                            "cost_usd": record_intent.cost_usd
                        }),
                    },
                )
                .await
                .map_err(|err| anyhow!("control-plane settle failed: {}", err))?;
            if let Some(workflow_id) = record_intent.workflow_id {
                state
                    .store
                    .workflow_record_execution(workflow_id, true, record_intent.cost_usd)
                    .await
                    .map_err(|e| anyhow!("DB record_execution: {}", e))?;
            }
            let record_execution = methodology_execution_record_payload(&flow.id, &record_intent);
            Ok(ToolResult::json_pretty(&json!({
                "status": "dispatched",
                "flow_ref": "F-methodology-to-executable-compile :: s6 dry-run-or-run (run)",
                "flow_id": flow.id,
                "flow_path": resolved.path.display().to_string(),
                "task_id": task_id,
                "completed_nodes": ctx.completed_nodes,
                "record_execution_status": record_execution["status"].clone(),
                "record_execution": record_execution,
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
