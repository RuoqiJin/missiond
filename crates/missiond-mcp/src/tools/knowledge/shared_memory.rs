use crate::ToolDefinition;
use serde_json::{json, Value};

fn shared_memory_properties() -> Value {
    let mut props = json!({
        "action": {
            "type": "string",
            "enum": [
                "query",
                "append",
                "artifact_put",
                "artifact_get",
                "task_result_put",
                "task_result_get",
                "workflow_start",
                "workflow_checkpoint",
                "workflow_status",
                "evidence_view",
                "worker_settle",
                "claim",
                "release",
                "heartbeat",
                "cursor"
            ]
        },
        "stream_id": {"type": "string"},
        "streamId": {"type": "string"},
        "project_id": {"type": "string"},
        "projectId": {"type": "string"},
        "task_id": {"type": "string"},
        "taskId": {"type": "string"},
        "agent_id": {"type": "string"},
        "agentId": {"type": "string"},
        "event_kind": {"type": "string"},
        "eventKind": {"type": "string"},
        "payload": {"type": "object"},
        "idempotency_key": {"type": "string"},
        "idempotencyKey": {"type": "string"},
        "correlation_id": {"type": "string"},
        "correlationId": {"type": "string"}
    });
    if let Some(map) = props.as_object_mut() {
        for (key, value) in json!({
            "parent_event_ids": {"type": "array", "items": {"type": "string"}},
            "parentEventIds": {"type": "array", "items": {"type": "string"}},
            "kind": {"type": "string"},
            "media_type": {"type": "string"},
            "mediaType": {"type": "string"},
            "content": {"type": "string"},
            "json": {},
            "hash": {"type": "string"},
            "artifact_hash": {"type": "string"},
            "artifactHash": {"type": "string"},
            "workflow_run_id": {"type": "string"},
            "workflowRunId": {"type": "string"},
            "workflow_id": {"type": "string"},
            "workflowId": {"type": "string"},
            "workflow_path": {"type": "string"},
            "workflowPath": {"type": "string"},
            "parent_task_id": {"type": "string"},
            "parentTaskId": {"type": "string"},
            "slot_id": {"type": "string"},
            "slotId": {"type": "string"},
            "conversation_id": {"type": "string"},
            "conversationId": {"type": "string"},
            "provider": {"type": "string"},
            "summary": {"type": "string"},
            "result_status": {"type": "string"},
            "resultStatus": {"type": "string"},
            "status": {"type": "string"},
            "cursor": {"type": "object"},
            "checkpoint": {"type": "object"},
            "active_task_ids": {"type": "array", "items": {"type": "string"}},
            "activeTaskIds": {"type": "array", "items": {"type": "string"}},
            "artifact_hashes": {"type": "array", "items": {"type": "string"}},
            "artifactHashes": {"type": "array", "items": {"type": "string"}},
            "max_inflight": {"type": "integer"},
            "maxInflight": {"type": "integer"},
            "owner_id": {"type": "string"},
            "ownerId": {"type": "string"},
            "scope_kind": {"type": "string"},
            "scopeKind": {"type": "string"}
        })
        .as_object()
        .unwrap()
        {
            map.insert(key.clone(), value.clone());
        }
        for (key, value) in json!({
            "scope_key": {"type": "string"},
            "scopeKey": {"type": "string"},
            "lease_secs": {"type": "integer", "default": 1800},
            "leaseSecs": {"type": "integer"},
            "claim_id": {"type": "string"},
            "claimId": {"type": "string"},
            "last_seq": {"type": "integer"},
            "lastSeq": {"type": "integer"},
            "since_seq": {"type": "integer"},
            "sinceSeq": {"type": "integer"},
            "metadata": {"type": "object"},
            "limit": {"type": "integer", "default": 50}
        })
        .as_object()
        .unwrap()
        {
            map.insert(key.clone(), value.clone());
        }
    }
    props
}

fn shared_memory_schema() -> Value {
    json!({
        "type": "object",
        "required": ["action"],
        "properties": shared_memory_properties()
    })
}

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "mission_shared_memory",
            "MissionD durable shared memory and execution control plane: append/query events, put/get artifacts, create task-result artifacts, start/checkpoint workflow runs, query the unified evidence-governance view, settle worker completion, claim/release/heartbeat write leases, and manage agent cursors.",
            shared_memory_schema(),
        ),
        ToolDefinition::new(
            "mission_context_slice",
            "Return a compact agent context slice from compiled semantic IR plus shared artifacts for a task/shard.",
            json!({
                "type": "object",
                "properties": {
                    "project": {"type": "string"},
                    "project_id": {"type": "string"},
                    "projectId": {"type": "string"},
                    "task_id": {"type": "string"},
                    "taskId": {"type": "string"},
                    "accepted_shard_id": {"type": "string"},
                    "acceptedShardId": {"type": "string"}
                }
            }),
        ),
        ToolDefinition::new(
            "mission_claim_status",
            "Inspect active/released/expired shared-memory write leases by project or scope.",
            json!({
                "type": "object",
                "properties": {
                    "project_id": {"type": "string"},
                    "projectId": {"type": "string"},
                    "scope_kind": {"type": "string"},
                    "scopeKind": {"type": "string"},
                    "scope_key": {"type": "string"},
                    "scopeKey": {"type": "string"},
                    "limit": {"type": "integer", "default": 50}
                }
            }),
        ),
    ]
}
