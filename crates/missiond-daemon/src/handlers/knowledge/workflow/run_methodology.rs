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
    let project_id = methodology_run_project_id(args);
    let runtime_metadata = methodology_run_runtime_metadata(
        args,
        project_root.as_path(),
        flow.id.as_str(),
        resolved.path.as_path(),
        &record_intent,
    );
    let input = missiond_core::types::CreateBoardTaskInput {
        title,
        project: project_id.clone(),
        category: Some("methodology".to_string()),
        description: Some(format!(
            "compiled methodology flow `{}` — source: {}",
            flow.id,
            resolved.path.display()
        )),
        flow_template: Some(flow.id.clone()),
        runtime_metadata: Some(runtime_metadata),
        ..Default::default()
    };
    let task = state
        .store
        .create_board_task(&input)
        .await
        .map_err(|e| anyhow!("DB: {}", e))?;
    let task_id = task.id.to_string();
    crate::engine::control_plane_kernel::ControlPlaneKernel::new(state)
        .upsert_task_contract_command(
            crate::engine::control_plane_kernel::UpsertTaskContractCommand {
                task_id: task_id.clone(),
                project_id: task.project.clone().or(project_id),
                runtime_metadata: task.runtime_metadata.clone(),
            },
        )
        .await
        .map_err(|err| anyhow!("control-plane task_contracts upsert failed: {}", err))?;

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

fn methodology_run_project_id(args: &Value) -> Option<String> {
    for key in ["project_id", "projectId"] {
        if let Some(value) = string_arg(args, key) {
            return Some(value.to_string());
        }
    }
    for key in ["project", "target_project", "targetProject"] {
        let Some(value) = string_arg(args, key) else {
            continue;
        };
        if !Path::new(value).is_absolute() {
            return Some(value.to_string());
        }
    }
    None
}

fn methodology_run_runtime_metadata(
    args: &Value,
    project_root: &Path,
    flow_id: &str,
    flow_path: &Path,
    record_intent: &RunMethodologyRecordIntent,
) -> Value {
    json!({
        "schema": "missiond.board-task-runtime-metadata.v1",
        "source": "run_methodology",
        "control_state": "task_contracts",
        "dispatch_metadata": {
            "task_class": "methodology-flow-run",
            "flow_id": flow_id,
            "flow_path": flow_path.display().to_string(),
            "project_id": methodology_run_project_id(args),
            "project_root": project_root.display().to_string(),
            "workflow_id": record_intent.workflow_id,
            "cost_usd": record_intent.cost_usd,
            "output_contract": "methodology flow runner writes canonical system completion artifact through ControlPlaneKernel"
        },
        "read_scope": [
            flow_path.display().to_string(),
            project_root.display().to_string()
        ],
        "write_scope": [],
        "must_not_touch": [],
        "capability_grant_ids": [],
        "sandbox_profile": "system-methodology-flow-orchestrator",
        "projection_policy": "description_notes_are_projection_only"
    })
}

fn string_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn methodology_run_metadata_declares_task_contract_authority() {
        let workflow_id = uuid::Uuid::new_v4();
        let intent = RunMethodologyRecordIntent {
            workflow_id: Some(workflow_id),
            cost_usd: Some(0.25),
        };
        let metadata = methodology_run_runtime_metadata(
            &json!({ "project": "missiond" }),
            Path::new("/tmp/missiond-project"),
            "methodology-smoke",
            Path::new("/tmp/missiond-project/.missiond/generated/flows/methodology.yml"),
            &intent,
        );
        assert_eq!(
            metadata["schema"],
            "missiond.board-task-runtime-metadata.v1"
        );
        assert_eq!(metadata["source"], "run_methodology");
        assert_eq!(metadata["control_state"], "task_contracts");
        assert_eq!(
            metadata["dispatch_metadata"]["task_class"],
            "methodology-flow-run"
        );
        let workflow_id_text = workflow_id.to_string();
        assert_eq!(
            metadata["dispatch_metadata"]["workflow_id"].as_str(),
            Some(workflow_id_text.as_str())
        );
        assert_eq!(metadata["write_scope"].as_array().unwrap().len(), 0);
        assert_eq!(
            metadata["sandbox_profile"],
            "system-methodology-flow-orchestrator"
        );
    }

    #[test]
    fn methodology_run_project_id_does_not_store_path_as_project() {
        assert_eq!(
            methodology_run_project_id(&json!({ "project": "/tmp/project-root" })),
            None
        );
        assert_eq!(
            methodology_run_project_id(&json!({ "project_id": "missiond" })).as_deref(),
            Some("missiond")
        );
        assert_eq!(
            methodology_run_project_id(&json!({ "target_project": "alpha" })).as_deref(),
            Some("alpha")
        );
    }
}
