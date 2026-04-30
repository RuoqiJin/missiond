use anyhow::Result;
use chrono::Utc;
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};

use crate::state::AppState;

use super::claim_lease::{find_claim_node, parse_claims, parse_iso};
use super::completion_indexes::rebuild_derived_indexes;
use super::log_counters::{insert_id_counters_block, scan_max_id, Counter};
use super::log_dispatch::read_dispatch_metadata_from_log;
use super::log_store::{
    companion_path, lisp_quote_string, project_or_target_project, require_str,
    resolve_project_root, touch_last_updated, update_kv_in_node, write_log_file, LogFile,
};
use super::log_surface::emit_execution_event;

// ───────────────────────────────────────────────────────────────────────
// action: repair — dry-run by default; structural fixes only
// ───────────────────────────────────────────────────────────────────────

pub(super) async fn action_repair(state: &AppState, args: &Value) -> Result<ToolResult> {
    let execution_id = match require_str(args, "execution_id") {
        Ok(s) => s,
        Err(r) => return Ok(r),
    };
    let mode = args
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("dry_run");
    if mode != "dry_run" && mode != "apply" {
        return Ok(ToolResult::structured_error(ToolError::new(
            error_codes::INVALID_PARAM,
            format!("repair mode must be `dry_run` or `apply`, got `{}`", mode),
        )));
    }

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let raw = std::fs::read_to_string(&path)?;
    let mut file = LogFile::parse(raw)?;

    let mut actions: Vec<Value> = Vec::new();

    // 1) Synthesize missing id-counters with values derived from scan_max_id.
    if file.find_block("id-counters").is_none() {
        let claim_n = scan_max_id(&file, Counter::Claim) + 1;
        let dev_n = scan_max_id(&file, Counter::Deviation) + 1;
        let dec_n = scan_max_id(&file, Counter::Decision) + 1;
        let issue_n = scan_max_id(&file, Counter::Issue) + 1;
        let comp_n = scan_max_id(&file, Counter::Completion) + 1;
        actions.push(json!({
            "kind": "synthesize-id-counters",
            "next_claim_id": claim_n,
            "next_deviation_id": dev_n,
            "next_decision_id": dec_n,
            "next_issue_id": issue_n,
            "next_completion_id": comp_n,
        }));
        if mode == "apply" {
            insert_id_counters_block(&mut file, claim_n, dev_n, dec_n, issue_n, comp_n)?;
        }
    }

    // 2) Mark stale claims (lease expired, no release).
    let claims = parse_claims(&file);
    let now = Utc::now();
    let mut stale_ids = Vec::new();
    for c in &claims {
        if c.status != "active" {
            continue;
        }
        if let Some(exp) = c.lease_expires_at.as_deref().and_then(parse_iso) {
            if exp < now {
                stale_ids.push(c.id.clone());
            }
        }
    }
    for id in &stale_ids {
        actions.push(json!({
            "kind": "mark-stale-claim",
            "claim_id": id,
        }));
        if mode == "apply" {
            if let Some(node) = find_claim_node(&file, id).cloned() {
                update_kv_in_node(&mut file, &node, "status", &lisp_quote_string("stale"))?;
            }
        }
    }

    // 3) Rebuild derived-indexes if it exists; otherwise leave alone (the
    //    block is cache, not truth — status action recomputes anyway).
    if file.find_block("derived-indexes").is_some() {
        actions.push(json!({
            "kind": "rebuild-derived-indexes",
            "note": "regenerating from durable slots"
        }));
        if mode == "apply" {
            rebuild_derived_indexes(&mut file)?;
        }
    }

    if mode == "apply" && !actions.is_empty() {
        touch_last_updated(&mut file)?;
        write_log_file(&path, &file)?;
    }

    // Wave 20 / Task 09 — surface the workstation-dispatch trio on
    // repair events. The same `file` handle is current after any
    // apply-mode mutations above, so the meta block we observe is the
    // post-write authoritative state.
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Repaired {
            execution_id: execution_id.to_string(),
            applied: mode == "apply",
            action_count: actions.len() as u32,
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    Ok(ToolResult::json_pretty(&json!({
        "execution_id": execution_id,
        "path": path.display().to_string(),
        "mode": mode,
        "actions": actions,
        "applied": mode == "apply",
    })))
}
