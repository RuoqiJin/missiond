use serde::Deserialize;

use super::*;

#[derive(Clone, Debug, Deserialize)]
pub(super) struct CompiledRuntimeConfigPayload {
    #[serde(default)]
    pub(super) source_units: Vec<CompiledSourceUnit>,
    #[serde(default)]
    pub(super) source_domains: Vec<CompiledSourceDomain>,
    pub(super) workstation: WorkstationRuntimeConfig,
    pub(super) flow: FlowRuntimeConfig,
    pub(super) compute: ComputePrimitivesRuntimeConfig,
    pub(super) minimax: MinimaxRuntimeConfig,
    pub(super) router: RouterRuntimeConfig,
    pub(super) cascade: CascadeRuntimeConfig,
    #[serde(rename = "projectRegistry")]
    pub(super) project_registry: ProjectRegistryRuntimeConfig,
    #[serde(rename = "capabilityGovernance")]
    pub(super) capability_governance: CapabilityGovernanceRuntimeConfig,
    #[serde(rename = "memoryKb")]
    pub(super) memory_kb: MemoryKbRuntimeConfig,
    #[serde(rename = "evidenceLanePolicy", default)]
    pub(super) evidence_lane_policy: EvidenceLaneRuntimeConfig,
    #[serde(rename = "conversationIngestion")]
    pub(super) conversation_ingestion: ConversationIngestionRuntimeConfig,
    pub(super) autopilot: AutopilotRuntimeConfig,
    #[serde(rename = "learningEngine")]
    pub(super) learning_engine: LearningEngineRuntimeConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct CompiledRuntimeDomainPayload<T> {
    #[serde(default)]
    pub(super) source_units: Vec<CompiledSourceUnit>,
    #[serde(default)]
    pub(super) source_domains: Vec<CompiledSourceDomain>,
    pub(super) domain: String,
    #[serde(default)]
    pub(super) payload_key: String,
    pub(super) config: T,
}

pub(super) fn generated_default_runtime_config() -> CompiledRuntimeConfigPayload {
    serde_json::from_str(v3_runtime_defaults::generated::DEFAULT_RUNTIME_CONFIG_JSON)
        .expect("generated V3 runtime defaults must deserialize")
}
