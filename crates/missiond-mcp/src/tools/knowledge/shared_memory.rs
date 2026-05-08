use crate::ToolDefinition;
use serde_json::{json, Value};

fn shared_memory_properties() -> Value {
    let mut props = json!({
        "action": {
            "type": "string",
            "enum": ["query", "append", "artifact_put", "artifact_get", "claim", "release", "heartbeat", "cursor"]
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
            "MissionD durable shared memory: append/query events, put/get artifacts, claim/release/heartbeat write leases, and manage agent cursors.",
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
