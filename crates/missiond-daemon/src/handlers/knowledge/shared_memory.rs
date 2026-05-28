use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::engine::control_plane_kernel::{
    ClaimLeaseCommand, ControlPlaneKernel, HeartbeatLeaseCommand, ReleaseLeaseCommand,
};
use crate::state::AppState;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};

const DEFAULT_LEASE_SECS: i64 = 1800;

fn shared_memory_error(err: anyhow::Error) -> ToolError {
    if let Some(control) =
        err.downcast_ref::<crate::engine::shared_memory::StructuredControlError>()
    {
        let mut tool_error = ToolError::new(control.code, control.message.clone())
            .with_details(control.details.clone());
        if let Some(suggestion) = &control.suggestion {
            tool_error = tool_error.with_suggestion(suggestion.clone());
        }
        return tool_error;
    }
    ToolError::new(error_codes::INVALID_PARAM, err.to_string()).with_suggestion(
        "use mission_shared_memory(action=\"task_result_put\"|\"worker_settle\"|\"claim\") with typed fields; Board notes and PTY text are projections only",
    )
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_shared_memory" => {
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("query")
                .trim();
            let result = match action {
                "task_result_put" | "put_task_result" => {
                    state.shared_memory.task_result_put_typed(&args).await
                }
                "worker_settle" | "completion_settle" | "settle_worker" => {
                    state.shared_memory.settle_worker_typed(args.clone()).await
                }
                "claim" => {
                    ControlPlaneKernel::new(state)
                        .claim_lease_command(claim_lease_command_from_args(&args)?)
                        .await
                }
                "release" => {
                    ControlPlaneKernel::new(state)
                        .release_lease_command(release_lease_command_from_args(&args)?)
                        .await
                }
                "heartbeat" => {
                    ControlPlaneKernel::new(state)
                        .heartbeat_lease_command(heartbeat_lease_command_from_args(&args)?)
                        .await
                }
                "capability_check" | "check_capability" => {
                    state.shared_memory.capability_check_typed(&args).await
                }
                "job_event" | "record_job_event" => {
                    state.shared_memory.job_event_typed(args.clone()).await
                }
                _ => state.shared_memory.handle_action(&args).await,
            };
            match result {
                Ok(value) => Ok(ToolResult::json_pretty(&value)),
                Err(err) => Ok(ToolResult::structured_error(shared_memory_error(err))),
            }
        }
        "mission_context_slice" => match state.shared_memory.context_slice(&args).await {
            Ok(value) => Ok(ToolResult::json_pretty(&value)),
            Err(err) => Ok(ToolResult::structured_error(shared_memory_error(err))),
        },
        "mission_claim_status" => match state.shared_memory.claim_status(&args).await {
            Ok(value) => Ok(ToolResult::json_pretty(&value)),
            Err(err) => Ok(ToolResult::structured_error(shared_memory_error(err))),
        },
        _ => Ok(ToolResult::structured_error(ToolError::new(
            error_codes::UNKNOWN_TOOL,
            format!("Unknown shared-memory tool: {name}"),
        ))),
    }
}

fn claim_lease_command_from_args(args: &Value) -> Result<ClaimLeaseCommand> {
    Ok(ClaimLeaseCommand {
        project_id: string_arg(args, "project_id")
            .or_else(|| string_arg(args, "projectId"))
            .map(str::to_string),
        task_id: string_arg(args, "task_id")
            .or_else(|| string_arg(args, "taskId"))
            .map(str::to_string),
        owner_id: string_arg(args, "owner_id")
            .or_else(|| string_arg(args, "ownerId"))
            .unwrap_or("unknown")
            .to_string(),
        grant_id: grant_id_arg(args).map(str::to_string),
        subject_kind: string_arg(args, "subject_kind")
            .or_else(|| string_arg(args, "subjectKind"))
            .unwrap_or("worker")
            .to_string(),
        subject_id: string_arg(args, "subject_id")
            .or_else(|| string_arg(args, "subjectId"))
            .or_else(|| string_arg(args, "owner_id"))
            .or_else(|| string_arg(args, "ownerId"))
            .unwrap_or("unknown")
            .to_string(),
        scope_kind: string_arg(args, "scope_kind")
            .or_else(|| string_arg(args, "scopeKind"))
            .unwrap_or("write_scope")
            .to_string(),
        scope_key: string_arg(args, "scope_key")
            .or_else(|| string_arg(args, "scopeKey"))
            .ok_or_else(|| anyhow!("scope_key is required"))?
            .to_string(),
        lease_secs: args
            .get("lease_secs")
            .or_else(|| args.get("leaseSecs"))
            .and_then(Value::as_i64)
            .unwrap_or(DEFAULT_LEASE_SECS),
        metadata: args
            .get("metadata")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
        allow_system_bypass: system_or_operator_bypass_allowed(args),
        bypass_reason: Some("mission_shared_memory claim system/operator authority".to_string()),
    })
}

fn release_lease_command_from_args(args: &Value) -> Result<ReleaseLeaseCommand> {
    Ok(ReleaseLeaseCommand {
        claim_id: claim_id_arg(args)?.to_string(),
        owner_id: string_arg(args, "owner_id")
            .or_else(|| string_arg(args, "ownerId"))
            .map(str::to_string),
        grant_id: grant_id_arg(args).map(str::to_string),
        subject_kind: string_arg(args, "subject_kind")
            .or_else(|| string_arg(args, "subjectKind"))
            .unwrap_or("")
            .to_string(),
        subject_id: string_arg(args, "subject_id")
            .or_else(|| string_arg(args, "subjectId"))
            .unwrap_or("")
            .to_string(),
        details: args
            .get("details")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    })
}

fn heartbeat_lease_command_from_args(args: &Value) -> Result<HeartbeatLeaseCommand> {
    Ok(HeartbeatLeaseCommand {
        claim_id: claim_id_arg(args)?.to_string(),
        owner_id: string_arg(args, "owner_id")
            .or_else(|| string_arg(args, "ownerId"))
            .map(str::to_string),
        grant_id: grant_id_arg(args).map(str::to_string),
        subject_kind: string_arg(args, "subject_kind")
            .or_else(|| string_arg(args, "subjectKind"))
            .unwrap_or("")
            .to_string(),
        subject_id: string_arg(args, "subject_id")
            .or_else(|| string_arg(args, "subjectId"))
            .unwrap_or("")
            .to_string(),
        lease_secs: args
            .get("lease_secs")
            .or_else(|| args.get("leaseSecs"))
            .and_then(Value::as_i64)
            .unwrap_or(DEFAULT_LEASE_SECS),
        details: args
            .get("details")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    })
}

fn claim_id_arg(args: &Value) -> Result<&str> {
    string_arg(args, "claim_id")
        .or_else(|| string_arg(args, "claimId"))
        .or_else(|| string_arg(args, "id"))
        .ok_or_else(|| anyhow!("claim_id is required"))
}

fn grant_id_arg(args: &Value) -> Option<&str> {
    string_arg(args, "grant_id")
        .or_else(|| string_arg(args, "grantId"))
        .or_else(|| string_arg(args, "capability_grant_id"))
        .or_else(|| string_arg(args, "capabilityGrantId"))
}

fn string_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn bool_arg_any(args: &Value, keys: &[&str]) -> bool {
    keys.iter().any(|key| {
        let Some(value) = args.get(*key) else {
            return false;
        };
        value.as_bool().unwrap_or_else(|| {
            value.as_str().is_some_and(|text| {
                matches!(
                    text.trim().to_ascii_lowercase().as_str(),
                    "true" | "1" | "yes" | "on"
                )
            })
        })
    })
}

fn system_or_operator_bypass_allowed(args: &Value) -> bool {
    let subject_kind = string_arg(args, "subject_kind")
        .or_else(|| string_arg(args, "subjectKind"))
        .unwrap_or("");
    matches!(subject_kind, "system" | "daemon")
        || (subject_kind == "operator"
            && bool_arg_any(
                args,
                &[
                    "confirm",
                    "operator_confirm",
                    "operatorConfirm",
                    "operator_confirmed",
                    "operatorConfirmed",
                ],
            ))
}
