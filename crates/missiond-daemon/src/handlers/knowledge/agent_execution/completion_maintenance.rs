use anyhow::Result;
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};

use crate::state::AppState;

use super::completion_audit_findings::collect_audit_findings;
use super::log_dispatch::read_dispatch_metadata_from_log;
use super::log_store::{
    companion_path, project_or_target_project, require_str, resolve_project_root,
};
use super::log_surface::emit_execution_event;

pub(super) use super::completion_repair::action_repair;

// ───────────────────────────────────────────────────────────────────────
// action: audit — paren balance + ID monotonic + claim overlap + stale +
//                 completion coverage + open-issue owners
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_audit(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let raw = std::fs::read_to_string(&path)?;
    let audit = match collect_audit_findings(raw, &path, execution_id) {
        Ok(audit) => audit,
        Err(result) => return Ok(result),
    };
    let ok = audit.ok();
    let error_count = audit.error_count();
    // Wave 20 / Task 09 — surface the workstation-dispatch trio on the
    // audit + stale-claim events. Audit is read-only so we don't write
    // back to the file; the meta block we observe is whatever the latest
    // writer left there.
    let meta = read_dispatch_metadata_from_log(&audit.file);
    emit_execution_event(
        state,
        ExecutionEvent::Audited {
            execution_id: execution_id.to_string(),
            ok,
            findings_count: audit.findings.len() as u32,
            error_count,
            dispatch_strategy: meta.dispatch_strategy.clone(),
            target_project: meta.target_project.clone(),
            requested_cwd: meta.requested_cwd.clone(),
        },
    )
    .await;
    for f in &audit.findings {
        if f.get("kind").and_then(|v| v.as_str()) != Some("stale-claim") {
            continue;
        }
        let claim_id = f
            .get("claim_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let claimer = f
            .get("claimer")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let lease_expires_at = f
            .get("lease_expires_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        emit_execution_event(
            state,
            ExecutionEvent::StaleClaim {
                execution_id: execution_id.to_string(),
                claim_id,
                claimer,
                lease_expires_at,
                dispatch_strategy: meta.dispatch_strategy.clone(),
                target_project: meta.target_project.clone(),
                requested_cwd: meta.requested_cwd.clone(),
            },
        )
        .await;
    }

    Ok(ToolResult::json_pretty(&json!({
        "execution_id": execution_id,
        "path": path.display().to_string(),
        "ok": ok,
        "findings": audit.findings,
    })))
}
