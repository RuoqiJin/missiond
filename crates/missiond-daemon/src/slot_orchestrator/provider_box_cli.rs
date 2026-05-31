//! ProviderBoxSlotManager — routes provider CLI slot tasks through provider_box.
//!
//! This keeps Codex worker turns behind the single provider interaction boundary
//! instead of letting AgentSlotManager callers reach GenericCliSlotManager.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;

use missiond_core::types::CliEngine;

use super::types::{EngineSlotManager, EngineStatus, SlotTaskRequest};

pub struct ProviderBoxSlotManager {
    engine: CliEngine,
    boxed: Arc<crate::provider_box::ProviderInteractionBox>,
}

impl ProviderBoxSlotManager {
    pub fn new(engine: CliEngine, boxed: Arc<crate::provider_box::ProviderInteractionBox>) -> Self {
        Self { engine, boxed }
    }
}

#[async_trait]
impl EngineSlotManager for ProviderBoxSlotManager {
    fn engine_type(&self) -> CliEngine {
        self.engine
    }

    async fn execute(&self, task: &SlotTaskRequest) -> Result<String> {
        let mut request = crate::provider_box::ProviderInteractionRequest::new(
            crate::provider_box::BoxCommand::WorkerTurn,
            self.engine,
        );
        request.provider = Some(super::canonical_source_for_engine(self.engine).to_string());
        request.prompt = Some(task.prompt.clone());
        request.timeout_secs = Some(task.timeout.as_secs().max(1));
        request.slot_id = task.slot_id.clone();
        request.cwd = Some(task.cwd.display().to_string());
        request.project_root = Some(task.cwd.display().to_string());
        request.model = task.model.clone();
        request.model_profile = task.reasoning_effort.clone();
        request.tool_policy = Some(serde_json::json!({
            "sandbox": task.sandbox.clone().unwrap_or_else(|| "read-only".to_string()),
            "approval_policy": task.approval_policy.clone().unwrap_or_else(|| "never".to_string()),
            "search_enabled": task.search_enabled,
            "source": "agent-slot-manager"
        }));

        let result = self.boxed.execute(request).await?;
        if result.status != crate::provider_box::ProviderBoxStatus::Completed {
            let diagnostics = result
                .diagnostics
                .iter()
                .map(|diag| format!("{}: {}", diag.code, diag.message))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(anyhow!(
                "provider_box {:?} turn failed with status {:?}: {}",
                self.engine,
                result.status,
                diagnostics
            ));
        }
        result
            .final_text
            .filter(|text| !text.trim().is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "provider_box {:?} turn completed without final_text",
                    self.engine
                )
            })
    }

    async fn status(&self) -> EngineStatus {
        EngineStatus {
            engine: self.engine,
            // Provider-box drivers serialize per slot internally. Expose one
            // logical persistent lane so AgentSlotManager health no longer
            // reports this routed manager as zero-capacity.
            persistent_slots: 1,
            ephemeral_active: 0,
            ephemeral_capacity: 1,
        }
    }
}
