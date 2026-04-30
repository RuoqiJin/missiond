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
    let rows = state
        .store
        .workflow_list_top_n(limit)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&json!({
        "workflows": rows,
        "count": rows.len(),
        "limit": limit,
        "note": "ranked by executions desc, success_count desc, last_used_at desc",
    })))
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
            )))
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
