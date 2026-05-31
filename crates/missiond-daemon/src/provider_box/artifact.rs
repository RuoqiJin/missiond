use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use super::types::{ProviderBoxResult, ProviderInteractionRequest};

#[derive(Clone)]
pub(crate) struct ProviderBoxArtifactWriter {
    shared_memory: Arc<crate::engine::shared_memory::SharedMemoryService>,
}

impl ProviderBoxArtifactWriter {
    pub(crate) fn new(
        shared_memory: Arc<crate::engine::shared_memory::SharedMemoryService>,
    ) -> Self {
        Self { shared_memory }
    }

    pub(crate) async fn persist_turn(
        &self,
        request: &ProviderInteractionRequest,
        result: &ProviderBoxResult,
    ) -> Result<String> {
        let body = serde_json::to_value(result)?;
        let stored = self
            .shared_memory
            .put_json_artifact(
                "provider-interaction-turn",
                request.project_id.as_deref(),
                request.task_id.as_deref(),
                &body,
                json!({
                    "schema": "missiond.provider-interaction-turn.v1",
                    "turn_id": result.turn_id,
                    "command": result.command,
                    "engine": result.engine.to_string(),
                    "slot_id": result.slot_id,
                    "lease_id": result.lease_id,
                    "correlation_id": result.correlation_id,
                }),
            )
            .await?;

        artifact_hash(&stored)
    }
}

fn artifact_hash(value: &Value) -> Result<String> {
    value
        .get("hash")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("shared artifact write did not return hash"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::artifact_hash;

    #[test]
    fn artifact_hash_requires_hash_field() {
        assert_eq!(artifact_hash(&json!({"hash": "abc"})).expect("hash"), "abc");
        assert!(artifact_hash(&json!({})).is_err());
    }
}
