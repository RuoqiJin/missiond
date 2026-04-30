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

pub(super) async fn action_deviate(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let lisp_said = match require_str(args, "lisp_said") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let actually_found = match require_str(args, "actually_found") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let reason = match require_str(args, "reason") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let approved_by = args
        .get("approved_by")
        .and_then(|v| v.as_str())
        .unwrap_or("auto");
    let phase = args.get("phase").and_then(|v| v.as_str()).unwrap_or("");

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;
    let id = allocate_id(&mut file, Counter::Deviation)?;
    let date = now_iso();
    let entry = format!(
        "    ({id}\n      :phase {phase}\n      :date {date}\n      :lisp-said {lisp_said}\n      :actually-found {actually_found}\n      :reason {reason}\n      :approved-by {approved_by}\n      :status \"open\")",
        id = id,
        phase = lisp_quote_string(phase),
        date = lisp_quote_string(&date),
        lisp_said = lisp_quote_string(lisp_said),
        actually_found = lisp_quote_string(actually_found),
        reason = lisp_quote_string(reason),
        approved_by = lisp_quote_string(approved_by),
    );
    append_to_block(&mut file, "deviations", &entry)?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::DeviationRecorded {
            execution_id: execution_id.to_string(),
            deviation_id: id.clone(),
            phase: phase.to_string(),
            approved_by: approved_by.to_string(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "recorded",
        "deviation_id": id,
        "phase": phase,
        "approved_by": approved_by,
    })))
}
