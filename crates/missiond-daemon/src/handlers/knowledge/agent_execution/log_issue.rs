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

pub(super) async fn action_issue(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let severity = args
        .get("severity")
        .and_then(|v| v.as_str())
        .unwrap_or("medium");
    let desc = match require_str(args, "desc") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let resolution_path = args
        .get("resolution_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let owner = args.get("owner").and_then(|v| v.as_str()).unwrap_or("");

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;
    let id = allocate_id(&mut file, Counter::Issue)?;
    let date = now_iso();
    let entry = format!(
        "    ({id}\n      :severity {severity}\n      :desc {desc}\n      :resolution-path {res}\n      :owner {owner}\n      :at {date}\n      :status \"open\")",
        id = id,
        severity = lisp_quote_string(severity),
        desc = lisp_quote_string(desc),
        res = lisp_quote_string(resolution_path),
        owner = lisp_quote_string(owner),
        date = lisp_quote_string(&date),
    );
    append_to_block(&mut file, "issues", &entry)?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::IssueRecorded {
            execution_id: execution_id.to_string(),
            issue_id: id.clone(),
            severity: severity.to_string(),
            owner: owner.to_string(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "recorded",
        "issue_id": id,
        "severity": severity,
        "owner": owner,
    })))
}
