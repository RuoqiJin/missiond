use crate::state::AppState;
use anyhow::Result;
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};

use super::log_counters::{allocate_id, Counter};
use super::log_dispatch::read_dispatch_metadata_from_log;
use super::log_store::{
    append_to_block, companion_path, lisp_quote_string, now_iso, project_or_target_project,
    read_log_file, require_str, resolve_project_root, touch_last_updated, write_log_file,
};
use super::log_surface::emit_execution_event;

pub(super) async fn action_decide(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let context = match require_str(args, "context") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let chosen = match require_str(args, "chosen") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let rationale = match require_str(args, "rationale") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let decided_by = match require_str(args, "decided_by") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let options = args.get("options").and_then(|v| v.as_str()).unwrap_or("");

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;
    let id = allocate_id(&mut file, Counter::Decision)?;
    let date = now_iso();
    let entry = format!(
        "    ({id}\n      :context {context}\n      :options {options}\n      :chosen {chosen}\n      :rationale {rationale}\n      :decided-by {decided_by}\n      :at {date})",
        id = id,
        context = lisp_quote_string(context),
        options = lisp_quote_string(options),
        chosen = lisp_quote_string(chosen),
        rationale = lisp_quote_string(rationale),
        decided_by = lisp_quote_string(decided_by),
        date = lisp_quote_string(&date),
    );
    append_to_block(&mut file, "decisions", &entry)?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::DecisionRecorded {
            execution_id: execution_id.to_string(),
            decision_id: id.clone(),
            decided_by: decided_by.to_string(),
            at: date.clone(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "recorded",
        "decision_id": id,
        "decided_by": decided_by,
        "at": date,
    })))
}
