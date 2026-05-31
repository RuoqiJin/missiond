use async_trait::async_trait;
use missiond_core::types::CliEngine;
use serde::{Deserialize, Serialize};

use super::types::{
    BoxCommand, ModelSwitchResult, ProviderBoxResult, ProviderBoxStatus,
    ProviderInteractionRequest, ProviderModelCatalog, ProviderUsageSnapshot,
    DIAG_AGY_MODEL_CATALOG_UNSUPPORTED, DIAG_MODEL_SWITCH_UNSUPPORTED,
    DIAG_PROVIDER_CONTROL_ACTION_UNSUPPORTED, DIAG_PROVIDER_MCP_RECONNECT_UNSUPPORTED,
    DIAG_PROVIDER_MCP_STATUS_UNSUPPORTED, DIAG_PROVIDER_PTY_STEP_UNSUPPORTED,
    DIAG_PURE_TEXT_GUARD_UNSUPPORTED, DIAG_SUBMIT_TURN_UNSUPPORTED,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ProviderDriverCapabilities {
    pub(crate) submit_turn: bool,
    pub(crate) switch_model: bool,
    pub(crate) usage_probe: bool,
    pub(crate) model_catalog: bool,
    pub(crate) pure_text_guard: bool,
    pub(crate) control_action: bool,
    pub(crate) pty_step: bool,
    pub(crate) status: bool,
    pub(crate) mcp_status: bool,
    pub(crate) mcp_reconnect: bool,
}

#[async_trait]
pub(crate) trait ProviderDriver: Send + Sync {
    fn engine(&self) -> CliEngine;

    fn capabilities(&self) -> ProviderDriverCapabilities {
        ProviderDriverCapabilities::default()
    }

    async fn submit_turn(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        ProviderBoxResult::unsupported(
            request,
            DIAG_SUBMIT_TURN_UNSUPPORTED,
            "Provider driver has not been taught interactive prompt submission yet",
        )
    }

    async fn switch_model(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::unsupported(
            request,
            DIAG_MODEL_SWITCH_UNSUPPORTED,
            "Provider driver has not been taught verified model switching yet",
        );
        result.model_switch_result = Some(ModelSwitchResult::unsupported(request));
        result
    }

    async fn probe_usage(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        result.usage_snapshot = Some(ProviderUsageSnapshot::unknown(request));
        result
    }

    async fn discover_models(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let code = if request.engine == CliEngine::Agy {
            DIAG_AGY_MODEL_CATALOG_UNSUPPORTED
        } else {
            "MODEL_CATALOG_UNSUPPORTED"
        };
        let catalog = ProviderModelCatalog::unsupported(request, code);
        let mut result = ProviderBoxResult::unsupported(
            request,
            code,
            "Provider driver has not been taught stable model catalog export yet",
        );
        result.model_catalog = Some(catalog);
        result
    }

    async fn pure_text_single_turn(
        &self,
        request: &ProviderInteractionRequest,
    ) -> ProviderBoxResult {
        ProviderBoxResult::unsupported(
            request,
            DIAG_PURE_TEXT_GUARD_UNSUPPORTED,
            "Provider driver has not been taught enforceable pure-text guard semantics yet",
        )
    }

    async fn control_action(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        ProviderBoxResult::unsupported(
            request,
            DIAG_PROVIDER_CONTROL_ACTION_UNSUPPORTED,
            "Provider driver has not been taught interactive control actions yet",
        )
    }

    async fn pty_step(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        ProviderBoxResult::unsupported(
            request,
            DIAG_PROVIDER_PTY_STEP_UNSUPPORTED,
            "Provider driver has not been taught guarded PTY single-step control yet",
        )
    }

    async fn status(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        ProviderBoxResult::base(request, ProviderBoxStatus::Unknown)
    }

    async fn mcp_status(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        ProviderBoxResult::unsupported(
            request,
            DIAG_PROVIDER_MCP_STATUS_UNSUPPORTED,
            "Provider driver has not been taught interactive MCP status probing yet",
        )
    }

    async fn mcp_reconnect(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        ProviderBoxResult::unsupported(
            request,
            DIAG_PROVIDER_MCP_RECONNECT_UNSUPPORTED,
            "Provider driver has not been taught interactive MCP restart/reconnect yet",
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct UnsupportedProviderDriver {
    engine: CliEngine,
}

impl UnsupportedProviderDriver {
    pub(crate) fn new(engine: CliEngine) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl ProviderDriver for UnsupportedProviderDriver {
    fn engine(&self) -> CliEngine {
        self.engine
    }
}

pub(crate) fn command_requires_driver(command: BoxCommand) -> bool {
    command.is_prompt_turn()
        || matches!(
            command,
            BoxCommand::ModelSwitch
                | BoxCommand::UsageProbe
                | BoxCommand::ModelCatalogExport
                | BoxCommand::ControlAction
                | BoxCommand::PtyStep
                | BoxCommand::Status
                | BoxCommand::McpStatus
                | BoxCommand::McpReconnect
        )
}
