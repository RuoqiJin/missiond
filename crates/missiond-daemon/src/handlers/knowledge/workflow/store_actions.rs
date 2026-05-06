use super::*;

// ───────────────────────────────────────────────────────────────────────
// list / get / match — store-backed reads
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_list(state: &AppState, args: &Value) -> Result<ToolResult> {
    let limit = args
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT);
    let compiled_contracts = compiled_workflow_contracts_for_args(state, args).await;
    let rows = state
        .store
        .workflow_list_top_n(limit)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&json!({
        "workflows": rows,
        "count": rows.len(),
        "limit": limit,
        "compiledContracts": compiled_contracts,
        "note": "ranked by executions desc, success_count desc, last_used_at desc",
    })))
}

async fn compiled_workflow_contracts_for_args(state: &AppState, args: &Value) -> Value {
    let project_root = match resolve_project_root_from_args(state, args).await {
        Ok(root) => root,
        Err(reason) => {
            return json!({
                "ok": false,
                "skipped": true,
                "reason": reason,
            });
        }
    };
    let loaded = load_compiled_workflow_contracts(&project_root, None);
    let workflows = loaded.payload.as_ref().map(|payload| {
        payload
            .workflows
            .iter()
            .map(|workflow| {
                json!({
                    "file": &workflow.file,
                    "name": &workflow.name,
                    "workflowId": &workflow.workflow_id,
                    "status": &workflow.status,
                    "owner": &workflow.owner,
                    "authority": &workflow.authority,
                    "sourcePlans": &workflow.source_plans,
                    "steps": &workflow.steps,
                    "riskGateCount": workflow.risk_gate_count,
                    "completionCriteriaCount": workflow.completion_criteria_count,
                })
            })
            .collect::<Vec<_>>()
    });
    json!({
        "ok": loaded.payload.is_some() && loaded.diagnostics.is_empty(),
        "source": "compiled-workflows",
        "projectRoot": project_root.display().to_string(),
        "snapshot": loaded.snapshot.as_ref().map(|snapshot| json!({
            "kind": snapshot.kind,
            "path": snapshot.path.display().to_string(),
            "schemaVersion": snapshot.schema_version,
            "sourceHash": snapshot.source_hash,
        })),
        "workflowCount": workflows.as_ref().map(|items| items.len()).unwrap_or(0),
        "workflows": workflows.unwrap_or_default(),
        "diagnostics": loaded.diagnostics,
    })
}

pub(super) async fn action_get(state: &AppState, args: &Value) -> Result<ToolResult> {
    let row = if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
        state
            .store
            .workflow_get_by_name(name)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
    } else if let Some(raw) = args.get("workflow_id").and_then(|v| v.as_str()) {
        let id = uuid::Uuid::parse_str(raw).map_err(|e| anyhow!("workflow_id not UUID: {}", e))?;
        state
            .store
            .workflow_get_by_id(id)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
    } else {
        return Ok(ToolResult::structured_error(ToolError::new(
            error_codes::MISSING_PARAM,
            "get requires `name` or `workflow_id`",
        )));
    };
    match row {
        Some(w) => Ok(ToolResult::json_pretty(&w)),
        None => Ok(ToolResult::structured_error(
            ToolError::new(error_codes::NOT_FOUND, "workflow not found")
                .with_suggestion("use action=list or action=match"),
        )),
    }
}

pub(super) async fn action_match(state: &AppState, args: &Value) -> Result<ToolResult> {
    let utterance = match args.get("utterance").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::MISSING_PARAM,
                "match requires `utterance` (or `query`)",
            )));
        }
    };
    let rows = state
        .store
        .workflow_find_by_match(utterance)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&json!({
        "query": utterance,
        "matches": rows,
        "count": rows.len(),
        "note": "current matcher is substring over match_rules JSONB text; refine by parsing keys after actor lands",
    })))
}

// ───────────────────────────────────────────────────────────────────────
// apply — read-only candidate, no execution
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_apply(state: &AppState, args: &Value) -> Result<ToolResult> {
    let row = if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
        state
            .store
            .workflow_get_by_name(name)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
    } else if let Some(raw) = args.get("workflow_id").and_then(|v| v.as_str()) {
        let id = uuid::Uuid::parse_str(raw).map_err(|e| anyhow!("workflow_id not UUID: {}", e))?;
        state
            .store
            .workflow_get_by_id(id)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
    } else {
        return Ok(ToolResult::structured_error(ToolError::new(
            error_codes::MISSING_PARAM,
            "apply requires `name` or `workflow_id`",
        )));
    };
    match row {
        Some(w) => Ok(ToolResult::json_pretty(&json!({
            "status": "candidate_returned",
            "workflow": w,
            "note": "apply returns the template. Execution requires action=run_methodology or mission_flow_run on a compiled YAML.",
        }))),
        None => Ok(ToolResult::structured_error(ToolError::new(
            error_codes::NOT_FOUND,
            "workflow not found",
        ))),
    }
}

// ───────────────────────────────────────────────────────────────────────
// record_execution — full
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_record_execution(state: &AppState, args: &Value) -> Result<ToolResult> {
    let id = parse_id_arg(args, "workflow_id")?;
    let success = args
        .get("success")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| anyhow!("`success` required (boolean)"))?;
    let cost_usd = args.get("cost_usd").and_then(|v| v.as_f64());
    state
        .store
        .workflow_record_execution(id, success, cost_usd)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&json!({
        "status": "recorded",
        "workflow_id": id,
        "success": success,
        "cost_usd": cost_usd,
    })))
}

pub(super) fn parse_id_arg(args: &Value, key: &str) -> Result<uuid::Uuid> {
    let raw = args
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("`{}` required", key))?;
    uuid::Uuid::parse_str(raw).map_err(|e| anyhow!("`{}` is not a UUID: {}", key, e))
}
