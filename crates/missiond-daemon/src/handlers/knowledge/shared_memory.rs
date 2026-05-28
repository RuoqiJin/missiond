use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::engine::control_plane_kernel::{
    CapabilityGrantCommand, ClaimLeaseCommand, ControlPlaneKernel, HeartbeatLeaseCommand,
    JobEventCommand, ReleaseLeaseCommand, RequireCapabilityCommand,
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

fn legacy_projection_action(action: &str) -> bool {
    matches!(
        action,
        "append"
            | "query"
            | "artifact_put"
            | "put_artifact"
            | "artifact_get"
            | "get_artifact"
            | "task_result_get"
            | "get_task_result"
            | "task_evidence_summary"
            | "evidence_summary"
            | "workflow_start"
            | "start_workflow"
            | "workflow_checkpoint"
            | "checkpoint_workflow"
            | "workflow_status"
            | "get_workflow_status"
            | "workflow_summary"
            | "workflow_runs_summary"
            | "runtime_artifact_index"
            | "index_runtime_artifact"
            | "runtime_artifact_list"
            | "list_runtime_artifacts"
            | "runtime_artifact_prune"
            | "prune_runtime_artifacts"
            | "evidence_view"
            | "evidence_governance_view"
            | "get_evidence_view"
            | "model_route_outcome_put"
            | "record_model_route_outcome"
            | "cursor"
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
                    let request = state
                        .shared_memory
                        .task_result_put_request_from_args(&args)?;
                    ControlPlaneKernel::new(state)
                        .task_result_put_command(request)
                        .await
                }
                "worker_settle" | "completion_settle" | "settle_worker" => {
                    let request = state.shared_memory.worker_settle_request_from_args(&args)?;
                    ControlPlaneKernel::new(state)
                        .worker_settle_command(request)
                        .await
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
                    ControlPlaneKernel::new(state)
                        .capability_check_command(capability_check_command_from_args(&args)?)
                        .await
                }
                "capability_grant" | "grant_capability" => {
                    ControlPlaneKernel::new(state)
                        .capability_grant_command(capability_grant_command_from_args(&args)?)
                        .await
                }
                "job_event" | "record_job_event" => {
                    ControlPlaneKernel::new(state)
                        .job_event_command(job_event_command_from_args(&args)?)
                        .await
                }
                _ if legacy_projection_action(action) => {
                    state.shared_memory.handle_action(&args).await
                }
                other => Err(anyhow!("unknown shared memory action: {other}")),
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

fn capability_check_command_from_args(args: &Value) -> Result<RequireCapabilityCommand> {
    let task_id = string_arg(args, "task_id")
        .or_else(|| string_arg(args, "taskId"))
        .ok_or_else(|| anyhow!("task_id is required"))?
        .to_string();
    let operation = string_arg(args, "operation")
        .ok_or_else(|| anyhow!("operation is required"))?
        .to_string();
    let scope_kind = string_arg(args, "scope_kind")
        .or_else(|| string_arg(args, "scopeKind"))
        .unwrap_or("task")
        .to_string();
    let scope_key = string_arg(args, "scope_key")
        .or_else(|| string_arg(args, "scopeKey"))
        .unwrap_or(task_id.as_str())
        .to_string();
    Ok(RequireCapabilityCommand {
        grant_id: grant_id_arg(args).map(str::to_string),
        subject_kind: string_arg(args, "subject_kind")
            .or_else(|| string_arg(args, "subjectKind"))
            .unwrap_or("task")
            .to_string(),
        subject_id: string_arg(args, "subject_id")
            .or_else(|| string_arg(args, "subjectId"))
            .unwrap_or(task_id.as_str())
            .to_string(),
        operation,
        scope_kind,
        scope_key,
        task_id: Some(task_id),
        allow_system_bypass: system_or_operator_bypass_allowed(args),
        bypass_reason: Some("mission_shared_memory capability_check bypass".to_string()),
        details: args
            .get("details")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    })
}

fn capability_grant_command_from_args(args: &Value) -> Result<CapabilityGrantCommand> {
    let authority_subject_kind = string_arg(args, "authority_subject_kind")
        .or_else(|| string_arg(args, "authoritySubjectKind"))
        .or_else(|| string_arg(args, "issuer_subject_kind"))
        .or_else(|| string_arg(args, "issuerSubjectKind"))
        .or_else(|| string_arg(args, "actor_subject_kind"))
        .or_else(|| string_arg(args, "actorSubjectKind"))
        .unwrap_or("operator")
        .to_string();
    let authority_subject_id = string_arg(args, "authority_subject_id")
        .or_else(|| string_arg(args, "authoritySubjectId"))
        .or_else(|| string_arg(args, "issuer_subject_id"))
        .or_else(|| string_arg(args, "issuerSubjectId"))
        .or_else(|| string_arg(args, "actor_subject_id"))
        .or_else(|| string_arg(args, "actorSubjectId"))
        .unwrap_or("operator")
        .to_string();
    let allow_system_bypass = match authority_subject_kind.as_str() {
        "system" | "daemon" => true,
        "operator" => bool_arg_any(
            args,
            &[
                "confirm",
                "operator_confirm",
                "operatorConfirm",
                "operator_confirmed",
                "operatorConfirmed",
            ],
        ),
        _ => false,
    };
    Ok(CapabilityGrantCommand {
        authority_grant_id: string_arg(args, "authority_grant_id")
            .or_else(|| string_arg(args, "authorityGrantId"))
            .or_else(|| string_arg(args, "issuer_grant_id"))
            .or_else(|| string_arg(args, "issuerGrantId"))
            .or_else(|| string_arg(args, "delegate_grant_id"))
            .or_else(|| string_arg(args, "delegateGrantId"))
            .map(str::to_string),
        authority_subject_kind,
        authority_subject_id,
        subject_kind: string_arg(args, "subject_kind")
            .or_else(|| string_arg(args, "subjectKind"))
            .unwrap_or("task")
            .to_string(),
        subject_id: string_arg(args, "subject_id")
            .or_else(|| string_arg(args, "subjectId"))
            .ok_or_else(|| anyhow!("subject_id is required"))?
            .to_string(),
        operation: string_arg(args, "operation")
            .ok_or_else(|| anyhow!("operation is required"))?
            .to_string(),
        scope_kind: string_arg(args, "scope_kind")
            .or_else(|| string_arg(args, "scopeKind"))
            .ok_or_else(|| anyhow!("scope_kind is required"))?
            .to_string(),
        scope_key: string_arg(args, "scope_key")
            .or_else(|| string_arg(args, "scopeKey"))
            .ok_or_else(|| anyhow!("scope_key is required"))?
            .to_string(),
        project_id: string_arg(args, "project_id")
            .or_else(|| string_arg(args, "projectId"))
            .map(str::to_string),
        task_id: string_arg(args, "task_id")
            .or_else(|| string_arg(args, "taskId"))
            .map(str::to_string),
        issuer: string_arg(args, "issuer").unwrap_or("missiond").to_string(),
        evidence_requirement: string_arg(args, "evidence_requirement")
            .or_else(|| string_arg(args, "evidenceRequirement"))
            .map(str::to_string),
        details: args
            .get("details")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
        allow_system_bypass,
        bypass_reason: string_arg(args, "bypass_reason")
            .or_else(|| string_arg(args, "bypassReason"))
            .map(str::to_string),
    })
}

fn job_event_command_from_args(args: &Value) -> Result<JobEventCommand> {
    let task_id = string_arg(args, "task_id")
        .or_else(|| string_arg(args, "taskId"))
        .ok_or_else(|| anyhow!("task_id is required"))?
        .to_string();
    Ok(JobEventCommand {
        task_id,
        project_id: string_arg(args, "project_id")
            .or_else(|| string_arg(args, "projectId"))
            .map(str::to_string),
        agent_id: string_arg(args, "agent_id")
            .or_else(|| string_arg(args, "agentId"))
            .unwrap_or("missiond")
            .to_string(),
        event_kind: string_arg(args, "event_kind")
            .or_else(|| string_arg(args, "eventKind"))
            .unwrap_or("observation.recorded")
            .to_string(),
        attempt_id: string_arg(args, "attempt_id")
            .or_else(|| string_arg(args, "attemptId"))
            .map(str::to_string),
        worker_id: string_arg(args, "worker_id")
            .or_else(|| string_arg(args, "workerId"))
            .map(str::to_string),
        conversation_id: string_arg(args, "conversation_id")
            .or_else(|| string_arg(args, "conversationId"))
            .map(str::to_string),
        runtime_metadata: args
            .get("runtime_metadata")
            .or_else(|| args.get("runtimeMetadata"))
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
        payload: args
            .get("payload")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
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
        allow_system_bypass: system_or_operator_bypass_allowed(args),
        bypass_reason: Some("mission_shared_memory release system/operator authority".to_string()),
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
        allow_system_bypass: system_or_operator_bypass_allowed(args),
        bypass_reason: Some(
            "mission_shared_memory heartbeat system/operator authority".to_string(),
        ),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_memory_adapter_allowlists_legacy_projection_actions() {
        for action in [
            "append",
            "query",
            "artifact_put",
            "task_result_get",
            "workflow_start",
            "evidence_view",
            "model_route_outcome_put",
            "cursor",
        ] {
            assert!(legacy_projection_action(action), "{action}");
        }
    }

    #[test]
    fn shared_memory_adapter_does_not_legacy_route_control_actions() {
        for action in [
            "task_result_put",
            "worker_settle",
            "claim",
            "release",
            "heartbeat",
            "capability_check",
            "capability_grant",
            "job_event",
            "unknown_future_control_action",
        ] {
            assert!(!legacy_projection_action(action), "{action}");
        }
    }

    #[test]
    fn capability_grant_command_separates_authority_from_target_subject() {
        let command = capability_grant_command_from_args(&serde_json::json!({
            "subject_kind": "worker",
            "subject_id": "slot-1",
            "operation": "settle",
            "scope_kind": "task",
            "scope_key": "task-1",
            "task_id": "task-1"
        }))
        .expect("command");
        assert_eq!(command.subject_kind, "worker");
        assert_eq!(command.subject_id, "slot-1");
        assert_eq!(command.authority_subject_kind, "operator");
        assert_eq!(command.authority_subject_id, "operator");
        assert!(!command.allow_system_bypass);
    }

    #[test]
    fn capability_grant_command_requires_confirmed_operator_bypass() {
        let command = capability_grant_command_from_args(&serde_json::json!({
            "authority_subject_kind": "operator",
            "authority_subject_id": "jinchen",
            "confirm": true,
            "subject_kind": "worker",
            "subject_id": "slot-1",
            "operation": "settle",
            "scope_kind": "task",
            "scope_key": "task-1",
            "task_id": "task-1"
        }))
        .expect("command");
        assert_eq!(command.authority_subject_kind, "operator");
        assert_eq!(command.authority_subject_id, "jinchen");
        assert!(command.allow_system_bypass);
    }
}
