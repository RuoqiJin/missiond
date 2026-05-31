use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use missiond_core::types::CliEngine;
use serde_json::json;

use super::artifact::ProviderBoxArtifactWriter;
use super::driver::{ProviderDriver, UnsupportedProviderDriver};
use super::types::{
    BoxCommand, ProviderBoxDiagnostic, ProviderBoxResult, ProviderInteractionRequest,
};

pub(crate) struct ProviderInteractionBox {
    drivers: HashMap<CliEngine, Arc<dyn ProviderDriver>>,
    artifact_writer: Option<ProviderBoxArtifactWriter>,
}

impl ProviderInteractionBox {
    pub(crate) fn new(artifact_writer: ProviderBoxArtifactWriter) -> Self {
        Self::with_artifact_writer(Some(artifact_writer))
    }

    pub(crate) fn without_artifacts() -> Self {
        Self::with_artifact_writer(None)
    }

    fn with_artifact_writer(artifact_writer: Option<ProviderBoxArtifactWriter>) -> Self {
        let mut boxed = Self {
            drivers: HashMap::new(),
            artifact_writer,
        };
        boxed.install_unsupported_defaults();
        boxed
    }

    pub(crate) fn register_driver(&mut self, driver: Arc<dyn ProviderDriver>) {
        self.drivers.insert(driver.engine(), driver);
    }

    pub(crate) async fn execute(
        &self,
        request: ProviderInteractionRequest,
    ) -> Result<ProviderBoxResult> {
        let driver = self.driver_for(request.engine);
        let mut result = match request.command {
            BoxCommand::WorkerTurn
            | BoxCommand::SemanticAuthoring
            | BoxCommand::GroundedDirectAnswer
            | BoxCommand::RunnerOneShot
            | BoxCommand::Vision => driver.submit_turn(&request).await,
            BoxCommand::ModelSwitch => driver.switch_model(&request).await,
            BoxCommand::UsageProbe => driver.probe_usage(&request).await,
            BoxCommand::ModelCatalogExport => driver.discover_models(&request).await,
            BoxCommand::PureTextSingleTurn => driver.pure_text_single_turn(&request).await,
            BoxCommand::ControlAction => driver.control_action(&request).await,
            BoxCommand::PtyStep => driver.pty_step(&request).await,
            BoxCommand::Status => driver.status(&request).await,
        };

        if let Some(writer) = &self.artifact_writer {
            match writer.persist_turn(&request, &result).await {
                Ok(hash) => {
                    result.artifact_hash = Some(hash);
                }
                Err(err) => {
                    result.add_diagnostic(ProviderBoxDiagnostic::warning(
                        "PROVIDER_BOX_ARTIFACT_WRITE_FAILED",
                        "Provider interaction turn could not be persisted",
                        json!({
                            "error": err.to_string(),
                            "turn_id": result.turn_id,
                            "correlation_id": result.correlation_id,
                        }),
                    ));
                }
            }
        }

        Ok(result)
    }

    fn driver_for(&self, engine: CliEngine) -> Arc<dyn ProviderDriver> {
        self.drivers
            .get(&engine)
            .cloned()
            .unwrap_or_else(|| Arc::new(UnsupportedProviderDriver::new(engine)))
    }

    fn install_unsupported_defaults(&mut self) {
        for engine in [
            CliEngine::ClaudeCode,
            CliEngine::Gemini,
            CliEngine::Codex,
            CliEngine::Agy,
        ] {
            self.register_driver(Arc::new(UnsupportedProviderDriver::new(engine)));
        }
    }
}

#[cfg(test)]
mod tests {
    use missiond_core::types::CliEngine;

    use super::ProviderInteractionBox;
    use crate::provider_box::types::{
        BoxCommand, ProviderBoxStatus, ProviderInteractionRequest,
        DIAG_AGY_MODEL_CATALOG_UNSUPPORTED, DIAG_MODEL_SWITCH_UNSUPPORTED,
        DIAG_PROVIDER_CONTROL_ACTION_UNSUPPORTED, DIAG_PROVIDER_PTY_STEP_UNSUPPORTED,
        DIAG_PURE_TEXT_GUARD_UNSUPPORTED, DIAG_USAGE_UNKNOWN,
    };

    #[tokio::test]
    async fn model_switch_is_typed_unsupported_until_driver_is_taught() {
        let boxed = ProviderInteractionBox::without_artifacts();
        let request = ProviderInteractionRequest::new(BoxCommand::ModelSwitch, CliEngine::Agy);

        let result = boxed.execute(request).await.expect("box result");

        assert_eq!(result.status, ProviderBoxStatus::Unsupported);
        assert!(result
            .diagnostics
            .iter()
            .any(|diag| diag.code == DIAG_MODEL_SWITCH_UNSUPPORTED));
        assert!(result.model_switch_result.is_some());
        assert!(result.artifact_hash.is_none());
    }

    #[tokio::test]
    async fn usage_probe_returns_unknown_not_fake_remaining_quota() {
        let boxed = ProviderInteractionBox::without_artifacts();
        let request = ProviderInteractionRequest::new(BoxCommand::UsageProbe, CliEngine::Codex);

        let result = boxed.execute(request).await.expect("box result");

        assert_eq!(result.status, ProviderBoxStatus::Unknown);
        let snapshot = result.usage_snapshot.expect("usage snapshot");
        assert!(snapshot.remaining.is_none());
        assert!(snapshot
            .diagnostics
            .iter()
            .any(|diag| diag.code == DIAG_USAGE_UNKNOWN));
    }

    #[tokio::test]
    async fn agy_model_catalog_export_is_explicitly_unsupported() {
        let boxed = ProviderInteractionBox::without_artifacts();
        let request =
            ProviderInteractionRequest::new(BoxCommand::ModelCatalogExport, CliEngine::Agy);

        let result = boxed.execute(request).await.expect("box result");

        assert_eq!(result.status, ProviderBoxStatus::Unsupported);
        assert!(result
            .diagnostics
            .iter()
            .any(|diag| diag.code == DIAG_AGY_MODEL_CATALOG_UNSUPPORTED));
        assert!(result.model_catalog.expect("catalog").entries.is_empty());
    }

    #[tokio::test]
    async fn pure_text_turn_fails_closed_until_guard_exists() {
        let boxed = ProviderInteractionBox::without_artifacts();
        let request = ProviderInteractionRequest::pure_text(CliEngine::Codex, "hello");

        let result = boxed.execute(request).await.expect("box result");

        assert_eq!(result.status, ProviderBoxStatus::Unsupported);
        assert!(result
            .diagnostics
            .iter()
            .any(|diag| diag.code == DIAG_PURE_TEXT_GUARD_UNSUPPORTED));
    }

    #[tokio::test]
    async fn control_action_fails_closed_until_driver_is_taught() {
        let boxed = ProviderInteractionBox::without_artifacts();
        let request = ProviderInteractionRequest::new(BoxCommand::ControlAction, CliEngine::Codex);

        let result = boxed.execute(request).await.expect("box result");

        assert_eq!(result.status, ProviderBoxStatus::Unsupported);
        assert!(result
            .diagnostics
            .iter()
            .any(|diag| diag.code == DIAG_PROVIDER_CONTROL_ACTION_UNSUPPORTED));
    }

    #[tokio::test]
    async fn pty_step_fails_closed_until_driver_is_taught() {
        let boxed = ProviderInteractionBox::without_artifacts();
        let request = ProviderInteractionRequest::new(BoxCommand::PtyStep, CliEngine::Codex);

        let result = boxed.execute(request).await.expect("box result");

        assert_eq!(result.status, ProviderBoxStatus::Unsupported);
        assert!(result
            .diagnostics
            .iter()
            .any(|diag| diag.code == DIAG_PROVIDER_PTY_STEP_UNSUPPORTED));
    }
}
