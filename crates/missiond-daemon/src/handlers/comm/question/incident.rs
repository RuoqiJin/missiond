use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};
use tracing::warn;

use crate::state::AppState;

pub(super) async fn handle_consolidated(state: &AppState, args: Value) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("list");
    match action {
        "test" => incident_test(state, args).await,
        "list" => incident_list(state, args).await,
        "get" => incident_get(state, args).await,
        "remediate" => incident_remediate(state, args).await,
        "status" => incident_status(state, args).await,
        "close" => incident_close(state, args).await,
        _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
    }
}

pub(super) async fn handle_legacy(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_incident_test" => incident_test(state, args).await,
        "mission_incident_list" => incident_list(state, args).await,
        "mission_incident_get" => incident_get(state, args).await,
        "mission_incident_remediate" => incident_remediate(state, args).await,
        "mission_incident_status" => incident_status(state, args).await,
        "mission_incident_close" => incident_close(state, args).await,
        _ => Ok(ToolResult::error(format!(
            "Unknown incident tool: {}",
            name
        ))),
    }
}

async fn incident_test(state: &AppState, args: Value) -> Result<ToolResult> {
    let severity_str = args
        .get("severity")
        .and_then(|v| v.as_str())
        .unwrap_or("warning");
    let severity = match severity_str {
        "critical" => missiond_core::types::IncidentSeverity::Critical,
        "high" => missiond_core::types::IncidentSeverity::High,
        _ => missiond_core::types::IncidentSeverity::Warning,
    };
    let source_str = args
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("manual");
    let source = match source_str {
        "health_check" => missiond_core::types::IncidentSource::HealthCheck,
        "deploy_center" => missiond_core::types::IncidentSource::DeployCenter,
        "sentry" => missiond_core::types::IncidentSource::Sentry,
        _ => missiond_core::types::IncidentSource::Manual,
    };
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Test incident")
        .to_string();
    let server_id = args
        .get("server_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let incident = missiond_core::types::MissionIncident {
        id: format!("inc-{}", uuid::Uuid::new_v4()),
        severity,
        source,
        title: title.clone(),
        description: format!("Manual test incident: {}", title),
        server_id,
        raw_payload: json!({"test": true, "injected_at": chrono::Utc::now().to_rfc3339()}),
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    match state
        .bus
        .publish_incident(missiond_core::event::events::IncidentEvent::Reported {
            incident: incident.clone(),
        })
        .await
    {
        Err(e) => {
            warn!("Failed to publish incident: {}", e);
            Ok(ToolResult::error(format!(
                "Failed to publish incident: {}",
                e
            )))
        }
        Ok(_) => Ok(ToolResult::json_pretty(&json!({
            "status": "injected",
            "incident_id": incident.id,
            "severity": severity_str,
            "title": incident.title,
        }))),
    }
}

async fn incident_list(state: &AppState, args: Value) -> Result<ToolResult> {
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as i64;
    let limit = limit.min(100);
    let incidents = state
        .store
        .list_incidents(limit)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    Ok(ToolResult::json_pretty(&incidents))
}

async fn incident_get(state: &AppState, args: Value) -> Result<ToolResult> {
    let id = match args.get("id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return Ok(ToolResult::error("id is required")),
    };
    let incident = state
        .store
        .get_incident_by_id(&id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let incident = match incident {
        Some(row) => row,
        None => return Ok(ToolResult::error(format!("Incident {} not found", id))),
    };

    let board_task = match incident.board_task_id.as_deref() {
        Some(tid) => state
            .store
            .get_board_task(tid)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?,
        None => None,
    };

    Ok(ToolResult::json_pretty(&json!({
        "incident": incident,
        "board_task": board_task,
    })))
}

async fn incident_remediate(state: &AppState, args: Value) -> Result<ToolResult> {
    if let Some(id) = args
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        let row = state
            .store
            .get_incident_by_id(id)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        let row = match row {
            Some(r) => r,
            None => {
                return Ok(ToolResult::error(format!("Incident {} not found", id)));
            }
        };
        if let Some(tid) = row.board_task_id.clone() {
            return Ok(ToolResult::json_pretty(&json!({
                "incident_id": row.id,
                "board_task_id": tid,
                "remediation_status": "already_linked",
                "next_action": "monitor board task; use mission_incident(action=status) to track",
                "agent_dispatched": false,
            })));
        }
        let severity = crate::aiops::parse_severity(&row.severity);
        let source = crate::aiops::parse_source(&row.source);
        let incident = missiond_core::types::MissionIncident {
            id: row.id.clone(),
            severity,
            source,
            title: row.title.clone(),
            description: row.description.clone(),
            server_id: row.server_id.clone(),
            raw_payload: json!({"replayed_from": row.id, "via": "mission_incident.remediate"}),
            created_at: row.created_at.clone(),
        };
        let board_task_id = crate::aiops::triage_incident(state, incident).await;
        if let Some(ref tid) = board_task_id {
            let _ = state
                .store
                .update_incident_board_task_id(&row.id, tid)
                .await;
        }
        let status = if board_task_id.is_some() {
            "linked"
        } else {
            "no_task"
        };
        return Ok(ToolResult::json_pretty(&json!({
            "incident_id": row.id,
            "board_task_id": board_task_id,
            "remediation_status": status,
            "next_action": if board_task_id.is_some() {
                "monitor board task; close via mission_incident(action=close, id, reason, actor)"
            } else {
                "no remediation task created; inspect logs"
            },
            "agent_dispatched": false,
        })));
    }

    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("either id or title is required"))?;
    let severity_str = args
        .get("severity")
        .and_then(|v| v.as_str())
        .unwrap_or("warning");
    let source_str = args
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("manual");
    let description = args
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or(title.as_str())
        .to_string();
    let server_id = args
        .get("server_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let incident = missiond_core::types::MissionIncident {
        id: format!("inc-{}", uuid::Uuid::new_v4()),
        severity: crate::aiops::parse_severity(severity_str),
        source: crate::aiops::parse_source(source_str),
        title: title.clone(),
        description,
        server_id,
        raw_payload: json!({"via": "mission_incident.remediate", "synthetic": true}),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let incident_id = incident.id.clone();
    let board_task_id = crate::aiops::triage_incident(state, incident).await;
    let status = if board_task_id.is_some() {
        "linked"
    } else {
        "no_task"
    };
    Ok(ToolResult::json_pretty(&json!({
        "incident_id": incident_id,
        "board_task_id": board_task_id,
        "remediation_status": status,
        "next_action": if board_task_id.is_some() {
            "monitor board task; close via mission_incident(action=close, id, reason, actor)"
        } else {
            "no remediation task created; inspect logs"
        },
        "agent_dispatched": false,
    })))
}

async fn incident_status(state: &AppState, args: Value) -> Result<ToolResult> {
    let id = match args.get("id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return Ok(ToolResult::error("id is required")),
    };
    let row = state
        .store
        .get_incident_by_id(&id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let row = match row {
        Some(r) => r,
        None => return Ok(ToolResult::error(format!("Incident {} not found", id))),
    };

    let (board_task, recent_notes) = match row.board_task_id.as_deref() {
        Some(tid) => {
            let task = state
                .store
                .get_board_task(tid)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            let notes = state
                .store
                .get_board_task_notes(tid)
                .await
                .unwrap_or_default();
            let mut notes = notes;
            if notes.len() > 5 {
                let drop = notes.len() - 5;
                notes.drain(..drop);
            }
            (task, notes)
        }
        None => (None, Vec::new()),
    };

    let remediation_status = match &board_task {
        None => "no_remediation_task",
        Some(t) if matches!(t.status, missiond_core::types::BoardTaskStatus::Done) => "resolved",
        Some(t)
            if matches!(
                t.status,
                missiond_core::types::BoardTaskStatus::Failed
                    | missiond_core::types::BoardTaskStatus::Blocked
            ) =>
        {
            "needs_attention"
        }
        Some(_) => "in_progress",
    };

    let next_action = match remediation_status {
        "resolved" => "incident already resolved; nothing to do",
        "needs_attention" => "remediation task is failed/blocked; inspect notes",
        "no_remediation_task" => "call mission_incident(action=remediate, id) to create one",
        _ => "monitor board task; close via mission_incident(action=close, id, reason, actor)",
    };

    Ok(ToolResult::json_pretty(&json!({
        "incident": row,
        "board_task": board_task,
        "recent_notes": recent_notes,
        "remediation_status": remediation_status,
        "next_action": next_action,
    })))
}

async fn incident_close(state: &AppState, args: Value) -> Result<ToolResult> {
    let id = match args.get("id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return Ok(ToolResult::error("id is required")),
    };
    let reason = match args.get("reason").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => {
            return Ok(ToolResult::error(
                "reason is required for close (free-form, e.g. 'service recovered after restart')",
            ));
        }
    };
    let actor = match args.get("actor").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => {
            return Ok(ToolResult::error(
                "actor is required for close (e.g. 'commander', 'slot-ops', 'oncall')",
            ));
        }
    };

    let row = state
        .store
        .get_incident_by_id(&id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let row = match row {
        Some(r) => r,
        None => return Ok(ToolResult::error(format!("Incident {} not found", id))),
    };

    let board_task_id = row.board_task_id.clone();
    let mut closed_task_id: Option<String> = None;
    if let Some(ref tid) = board_task_id {
        let task = state
            .store
            .get_board_task(tid)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        let task = match task {
            Some(t) => t,
            None => {
                return Ok(ToolResult::error(format!(
                    "Linked board task {} not found",
                    tid
                )));
            }
        };
        if let Err(reason_msg) = crate::aiops::is_safe_to_close_task(&task) {
            return Ok(ToolResult::json_pretty(&json!({
                "incident_id": row.id,
                "board_task_id": board_task_id,
                "remediation_status": "refused",
                "refusal_reason": reason_msg,
                "next_action": "ask the task owner to close manually, or pass action=close on a different incident",
            })));
        }

        let note = format!(
            "✅ Incident closed by {} ({}): {}",
            actor,
            chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
            reason
        );
        crate::engine::control_plane_kernel::ControlPlaneKernel::new(state)
            .complete_system_task(
                crate::engine::control_plane_kernel::SystemTaskCompletionInput {
                    task_id: task.id.to_string(),
                    project_id: task.project.clone(),
                    producer_id: "incident_close".to_string(),
                    summary: format!("Incident {} closed: {}", row.id, reason),
                    content: Some(note.clone()),
                    raw_evidence: json!({
                        "kind": "incident_close",
                        "incident_id": row.id,
                        "reason": reason,
                        "actor": actor
                    }),
                    evidence_refs: vec![json!({
                        "kind": "mission_incident",
                        "incident_id": row.id
                    })],
                    result_status: "completed".to_string(),
                    metadata: json!({
                        "incident_id": row.id,
                        "source": "incident_close"
                    }),
                },
            )
            .await
            .map_err(|e| anyhow!("control-plane settle failed: {}", e))?;
        let _ = state
            .store
            .add_board_task_note(&missiond_core::types::AddBoardTaskNoteInput {
                task_id: task.id.to_string(),
                content: note,
                note_type: Some("progress".to_string()),
                author: Some(format!("aiops:{}", actor)),
            })
            .await;
        closed_task_id = Some(task.id.to_string());
    }

    let _ = state
        .bus
        .publish_incident(missiond_core::event::events::IncidentEvent::Resolved {
            incident_id: row.id.clone(),
            reason: reason.clone(),
        })
        .await;

    Ok(ToolResult::json_pretty(&json!({
        "incident_id": row.id,
        "board_task_id": board_task_id,
        "closed_board_task_id": closed_task_id,
        "remediation_status": "closed",
        "actor": actor,
        "reason": reason,
        "next_action": "incident resolved; review history with mission_incident(action=get, id)",
    })))
}
