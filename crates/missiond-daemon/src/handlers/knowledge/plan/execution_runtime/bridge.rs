use super::*;

/// Splice the `plan_field_inference` block onto a successful response.
/// No-op when the inference block is absent (mode=`off`) or the response
/// already carries one (DAG / resume paths emit their own future hooks).
/// Errors propagate untouched — we never mask a failure with the
/// inference metadata.
pub(in crate::handlers::knowledge::plan) fn attach_inference_block(
    mut result: ToolResult,
    block: Option<Value>,
) -> ToolResult {
    let Some(block) = block else {
        return result;
    };
    if result.is_error.unwrap_or(false) {
        // Don't decorate structured errors with inference metadata —
        // the caller needs the error path uncluttered.
        return result;
    }
    let text = match result.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => return result,
    };
    let mut payload: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return result,
    };
    if let Some(map) = payload.as_object_mut() {
        // Preserve any pre-existing inference block (DAG / resume paths
        // may attach their own in the future) by NEVER overwriting.
        map.entry("plan_field_inference".to_string())
            .or_insert(block);
    }
    result.content = vec![ToolContent::Text {
        text: serde_json::to_string_pretty(&payload).unwrap_or(text),
    }];
    result
}

pub(in crate::handlers::knowledge::plan) fn action_execute_bridge(
    plan: &Plan,
    resolved: &ResolvedExec,
) -> ToolResult {
    let next_call = match resolved.target {
        "mission_execution" => json!({
            "tool": "mission_execution",
            "action": "open",
            "execution_id": format!("plan-{}", plan.id),
            "scope": format!("plan {}", plan.id),
        }),
        "mission_task_delegate" => json!({
            "tool": "mission_task_delegate",
            "board_task_id": plan.board_task_id,
            "plan_id": plan.id,
        }),
        "mission_flow_run" => json!({
            "tool": "mission_flow_run",
            "action": "run",
            "hint": "supply flow_id; plan.sexp_text 暂未自动编译为 flow YAML",
        }),
        _ => Value::Null,
    };

    ToolResult::json_pretty(&json!({
        "status": "bridge_ready",
        "execute_mode": "bridge",
        "runner_status": "bridge_only",
        "plan_id": plan.id,
        "board_task_id": plan.board_task_id,
        "target_tool": resolved.target,
        "target_source": resolved.target_source,
        "dispatch_strategy": resolved.dispatch_strategy,
        "dispatch_strategy_source": resolved.dispatch_strategy_source,
        "plan_hint_summary": resolved.plan_hint_summary,
        "next_call": next_call,
        "note": "manager returns the next-call descriptor; caller invokes the target tool directly. \
                 Pass execute_mode=\"internal\" to have MissionD dispatch the target inside the daemon.",
    }))
}
