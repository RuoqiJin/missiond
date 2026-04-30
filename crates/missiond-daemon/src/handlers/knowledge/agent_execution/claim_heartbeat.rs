use anyhow::{anyhow, Result};
use chrono::{SecondsFormat, Utc};
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};

use crate::state::AppState;

use super::claim_lease::{DEFAULT_LEASE_SECS, MAX_LEASE_SECS};
use super::claim_records::find_claim_node;
use super::log_store::{
    companion_path, lisp_quote_string, parse_kv_pairs, project_or_target_project, read_log_file,
    require_str, resolve_project_root, touch_last_updated, update_kv_in_node, write_log_file,
};
use super::log_surface::{emit_execution_event, read_dispatch_metadata_from_log};

pub(super) async fn action_heartbeat(state: &AppState, args: &Value) -> Result<ToolResult> {
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
    let lease_secs = args
        .get("lease_secs")
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_LEASE_SECS)
        .clamp(60, MAX_LEASE_SECS);

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
            ))
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

    let now = Utc::now();
    let now_s = now.to_rfc3339_opts(SecondsFormat::Secs, true);
    let expires =
        (now + chrono::Duration::seconds(lease_secs)).to_rfc3339_opts(SecondsFormat::Secs, true);

    update_kv_in_node(
        &mut file,
        &claim_node,
        "heartbeat-at",
        &lisp_quote_string(&now_s),
    )?;
    let claim_node2 = find_claim_node(&file, claim_id)
        .cloned()
        .ok_or_else(|| anyhow!("claim node vanished after heartbeat update"))?;
    update_kv_in_node(
        &mut file,
        &claim_node2,
        "lease-expires-at",
        &lisp_quote_string(&expires),
    )?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Heartbeat {
            execution_id: execution_id.to_string(),
            claim_id: claim_id.to_string(),
            claimer: claimer.to_string(),
            heartbeat_at: now_s.clone(),
            lease_expires_at: expires.clone(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "status": "heartbeat",
        "claim_id": claim_id,
        "heartbeat_at": now_s,
        "lease_expires_at": expires,
    })))
}
