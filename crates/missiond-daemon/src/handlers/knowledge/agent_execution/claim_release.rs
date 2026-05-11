use anyhow::{anyhow, Result};
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};

use crate::state::AppState;

use super::claim_records::find_claim_node;
use super::log_dispatch::read_dispatch_metadata_from_log;
use super::log_store::{
    companion_path, lisp_quote_string, now_iso, parse_kv_pairs, project_or_target_project,
    read_log_file, require_str, resolve_project_root, touch_last_updated, update_kv_in_node,
    write_log_file,
};
use super::log_surface::emit_execution_event;

pub(super) async fn action_release(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let claim_id = match require_str(args, "claim_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let claimer = match require_str(args, "claimer_name") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let summary = args.get("summary").and_then(|v| v.as_str()).unwrap_or("");

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;

    let claim_node = match find_claim_node(&file, claim_id) {
        Some(n) => n.clone(),
        None => {
            return Ok(ToolResult::structured_error(
                ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("claim {} not found", claim_id),
                )
                .with_suggestion("use action=status to list active claims"),
            ));
        }
    };

    let kvs = parse_kv_pairs(&file.src, claim_node.children());
    let owner = kvs
        .get("claimer")
        .or_else(|| kvs.get("agent"))
        .cloned()
        .unwrap_or_default();
    if owner.trim_matches('"') != claimer {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                "CLAIM_WRONG_OWNER",
                format!("claim {} owned by `{}`, not `{}`", claim_id, owner, claimer),
            )
            .with_suggestion("use the original claimer_name or run action=audit"),
        ));
    }

    let now = now_iso();
    update_kv_in_node(
        &mut file,
        &claim_node,
        "released-at",
        &lisp_quote_string(&now),
    )?;
    let claim_node2 = find_claim_node(&file, claim_id)
        .cloned()
        .ok_or_else(|| anyhow!("claim node vanished after release update"))?;
    update_kv_in_node(
        &mut file,
        &claim_node2,
        "status",
        &lisp_quote_string("released"),
    )?;
    if !summary.is_empty() {
        let claim_node3 = find_claim_node(&file, claim_id)
            .cloned()
            .ok_or_else(|| anyhow!("claim node vanished after status update"))?;
        update_kv_in_node(
            &mut file,
            &claim_node3,
            "summary",
            &lisp_quote_string(summary),
        )?;
    }
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Released {
            execution_id: execution_id.to_string(),
            claim_id: claim_id.to_string(),
            claimer: claimer.to_string(),
            released_at: now.clone(),
            summary: if summary.is_empty() {
                None
            } else {
                Some(summary.to_string())
            },
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "released",
        "claim_id": claim_id,
        "released_at": now,
        "summary": summary,
    })))
}
