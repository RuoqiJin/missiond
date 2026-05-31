use missiond_core::types::CliEngine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub(crate) const DIAG_SUBMIT_TURN_UNSUPPORTED: &str = "SUBMIT_TURN_UNSUPPORTED";
pub(crate) const DIAG_MODEL_SWITCH_UNSUPPORTED: &str = "MODEL_SWITCH_UNSUPPORTED";
pub(crate) const DIAG_MODEL_SWITCH_UNVERIFIED: &str = "MODEL_SWITCH_UNVERIFIED";
pub(crate) const DIAG_USAGE_UNKNOWN: &str = "USAGE_UNKNOWN";
pub(crate) const DIAG_AGY_MODEL_CATALOG_UNSUPPORTED: &str = "AGY_MODEL_CATALOG_UNSUPPORTED";
pub(crate) const DIAG_PURE_TEXT_GUARD_UNSUPPORTED: &str = "PURE_TEXT_GUARD_UNSUPPORTED";
pub(crate) const DIAG_PROVIDER_TURN_STALLED: &str = "PROVIDER_TURN_STALLED";
pub(crate) const DIAG_PROVIDER_TURN_TIMEOUT_CANCELLED: &str = "PROVIDER_TURN_TIMEOUT_CANCELLED";
pub(crate) const DIAG_PROVIDER_TURN_TIMEOUT_CANCEL_FAILED: &str =
    "PROVIDER_TURN_TIMEOUT_CANCEL_FAILED";
pub(crate) const DIAG_PROVIDER_BOX_AUTH_REQUIRED: &str = "PROVIDER_BOX_AUTH_REQUIRED";
pub(crate) const DIAG_PROVIDER_BOX_INVALID_REQUEST: &str = "PROVIDER_BOX_INVALID_REQUEST";
pub(crate) const DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE: &str = "PROVIDER_BOX_SLOT_UNAVAILABLE";
pub(crate) const DIAG_PROVIDER_TEXT_ONLY_VIOLATION: &str = "PROVIDER_TEXT_ONLY_VIOLATION";
pub(crate) const DIAG_PROVIDER_DURABLE_FINAL_MISSING: &str = "PROVIDER_DURABLE_FINAL_MISSING";
pub(crate) const DIAG_PROVIDER_CONTROL_ACTION_UNSUPPORTED: &str =
    "PROVIDER_CONTROL_ACTION_UNSUPPORTED";
pub(crate) const DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED: &str =
    "PROVIDER_CONTROL_ACTION_UNVERIFIED";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum BoxCommand {
    WorkerTurn,
    ModelSwitch,
    UsageProbe,
    ModelCatalogExport,
    PureTextSingleTurn,
    SemanticAuthoring,
    GroundedDirectAnswer,
    RunnerOneShot,
    Vision,
    ControlAction,
}

impl BoxCommand {
    pub(crate) fn is_prompt_turn(self) -> bool {
        matches!(
            self,
            Self::WorkerTurn
                | Self::PureTextSingleTurn
                | Self::SemanticAuthoring
                | Self::GroundedDirectAnswer
                | Self::RunnerOneShot
                | Self::Vision
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderControlAction {
    ClearScreen,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderBoxStatus {
    Accepted,
    Completed,
    Blocked,
    Unsupported,
    Unknown,
    Failed,
    Unverified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderUsageStatus {
    Exact,
    Estimated,
    Blocked,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModelSwitchStatus {
    Verified,
    Applied,
    Unsupported,
    Unverified,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PtyStepVerificationStatus {
    Verified,
    Unchanged,
    Ambiguous,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ProviderAttachmentRef {
    pub(crate) id: String,
    pub(crate) media_type: Option<String>,
    pub(crate) uri: Option<String>,
    pub(crate) artifact_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ModelSwitchPolicy {
    pub(crate) target_model: Option<String>,
    pub(crate) target_model_profile: Option<String>,
    pub(crate) allow_respawn: bool,
    pub(crate) require_verification: bool,
}

impl Default for ModelSwitchPolicy {
    fn default() -> Self {
        Self {
            target_model: None,
            target_model_profile: None,
            allow_respawn: true,
            require_verification: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct SingleTurnPolicy {
    pub(crate) require_plain_text: bool,
    pub(crate) no_tools: bool,
    pub(crate) no_mcp: bool,
    pub(crate) no_shell: bool,
    pub(crate) no_file_access: bool,
    pub(crate) max_provider_steps: Option<u32>,
}

impl Default for SingleTurnPolicy {
    fn default() -> Self {
        Self {
            require_plain_text: true,
            no_tools: true,
            no_mcp: true,
            no_shell: true,
            no_file_access: true,
            max_provider_steps: Some(1),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct TimeoutCancelPolicy {
    pub(crate) running_timeout_secs: u64,
    pub(crate) no_progress_grace_secs: u64,
    pub(crate) cancel_key: String,
    pub(crate) cancel_grace_secs: u64,
    pub(crate) max_cancel_attempts: u8,
    pub(crate) max_retries: u8,
    pub(crate) retry_after_cancel: bool,
    pub(crate) require_ready_after_cancel: bool,
}

impl Default for TimeoutCancelPolicy {
    fn default() -> Self {
        Self {
            running_timeout_secs: 120,
            no_progress_grace_secs: 20,
            cancel_key: "escape".to_string(),
            cancel_grace_secs: 5,
            max_cancel_attempts: 1,
            max_retries: 1,
            retry_after_cancel: true,
            require_ready_after_cancel: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ProviderInteractionRequest {
    pub(crate) schema: String,
    pub(crate) command: BoxCommand,
    pub(crate) provider: Option<String>,
    pub(crate) engine: CliEngine,
    pub(crate) model: Option<String>,
    pub(crate) model_profile: Option<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) project_root: Option<String>,
    pub(crate) prompt: Option<String>,
    #[serde(default)]
    pub(crate) attachments: Vec<ProviderAttachmentRef>,
    pub(crate) timeout_secs: Option<u64>,
    pub(crate) timeout_cancel_policy: Option<TimeoutCancelPolicy>,
    pub(crate) correlation_id: String,
    pub(crate) interaction_id: Option<String>,
    pub(crate) task_id: Option<String>,
    pub(crate) project_id: Option<String>,
    pub(crate) lease_id: Option<String>,
    pub(crate) slot_id: Option<String>,
    pub(crate) artifact_contract: Option<Value>,
    pub(crate) output_contract: Option<Value>,
    pub(crate) read_scope: Option<Value>,
    pub(crate) write_scope: Option<Value>,
    pub(crate) no_tools: bool,
    pub(crate) no_mcp: bool,
    pub(crate) no_shell: bool,
    pub(crate) no_file_access: bool,
    pub(crate) tool_policy: Option<Value>,
    pub(crate) desired_worker: Option<Value>,
    pub(crate) model_switch_policy: Option<ModelSwitchPolicy>,
    pub(crate) single_turn_policy: Option<SingleTurnPolicy>,
    pub(crate) router_export_policy: Option<Value>,
    pub(crate) control_action: Option<ProviderControlAction>,
}

impl ProviderInteractionRequest {
    pub(crate) fn new(command: BoxCommand, engine: CliEngine) -> Self {
        Self {
            schema: "missiond.provider-interaction-request.v1".to_string(),
            command,
            provider: None,
            engine,
            model: None,
            model_profile: None,
            cwd: None,
            project_root: None,
            prompt: None,
            attachments: Vec::new(),
            timeout_secs: None,
            timeout_cancel_policy: None,
            correlation_id: format!("corr-{}", uuid::Uuid::new_v4().simple()),
            interaction_id: None,
            task_id: None,
            project_id: None,
            lease_id: None,
            slot_id: None,
            artifact_contract: None,
            output_contract: None,
            read_scope: None,
            write_scope: None,
            no_tools: false,
            no_mcp: false,
            no_shell: false,
            no_file_access: false,
            tool_policy: None,
            desired_worker: None,
            model_switch_policy: None,
            single_turn_policy: None,
            router_export_policy: None,
            control_action: None,
        }
    }

    pub(crate) fn pure_text(engine: CliEngine, prompt: impl Into<String>) -> Self {
        let mut request = Self::new(BoxCommand::PureTextSingleTurn, engine);
        request.prompt = Some(prompt.into());
        request.no_tools = true;
        request.no_mcp = true;
        request.no_shell = true;
        request.no_file_access = true;
        request.single_turn_policy = Some(SingleTurnPolicy::default());
        request.timeout_cancel_policy = Some(TimeoutCancelPolicy::default());
        request.output_contract = Some(json!({
            "media_type": "text/plain",
            "single_turn": true
        }));
        request
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ProviderBoxDiagnostic {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) severity: String,
    pub(crate) details: Value,
}

impl ProviderBoxDiagnostic {
    pub(crate) fn error(
        code: impl Into<String>,
        message: impl Into<String>,
        details: Value,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            severity: "error".to_string(),
            details,
        }
    }

    pub(crate) fn unsupported(
        code: impl Into<String>,
        message: impl Into<String>,
        details: Value,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            severity: "error".to_string(),
            details,
        }
    }

    pub(crate) fn warning(
        code: impl Into<String>,
        message: impl Into<String>,
        details: Value,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            severity: "warning".to_string(),
            details,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ProviderModelUsage {
    pub(crate) model: String,
    pub(crate) percent: Option<u8>,
    pub(crate) status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ProviderUsageSnapshot {
    pub(crate) schema: String,
    pub(crate) snapshot_id: String,
    pub(crate) provider: Option<String>,
    pub(crate) engine: CliEngine,
    pub(crate) slot_id: Option<String>,
    pub(crate) account_ref: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) observed_at: String,
    pub(crate) status: ProviderUsageStatus,
    pub(crate) remaining: Option<i64>,
    pub(crate) limit: Option<i64>,
    pub(crate) reset_at: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) confidence: f32,
    pub(crate) block_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) model_quotas: Vec<ProviderModelUsage>,
    #[serde(default)]
    pub(crate) diagnostics: Vec<ProviderBoxDiagnostic>,
}

impl ProviderUsageSnapshot {
    pub(crate) fn unknown(request: &ProviderInteractionRequest) -> Self {
        Self {
            schema: "missiond.provider-usage-snapshot.v1".to_string(),
            snapshot_id: format!("usage-{}", uuid::Uuid::new_v4().simple()),
            provider: request.provider.clone(),
            engine: request.engine,
            slot_id: request.slot_id.clone(),
            account_ref: None,
            model: request.model.clone(),
            observed_at: chrono::Utc::now().to_rfc3339(),
            status: ProviderUsageStatus::Unknown,
            remaining: None,
            limit: None,
            reset_at: None,
            source: None,
            confidence: 0.0,
            block_kind: None,
            model_quotas: Vec::new(),
            diagnostics: vec![ProviderBoxDiagnostic::warning(
                DIAG_USAGE_UNKNOWN,
                "Provider driver has no reliable remaining-usage surface yet",
                json!({
                    "engine": request.engine.to_string(),
                    "slot_id": request.slot_id,
                }),
            )],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ProviderModelCatalogEntry {
    pub(crate) provider_model_id: String,
    pub(crate) display_name: String,
    pub(crate) family: Option<String>,
    pub(crate) routeable_default: bool,
    pub(crate) switch_capability: String,
    pub(crate) usage_probe_capability: String,
    pub(crate) confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ProviderModelCatalog {
    pub(crate) schema: String,
    pub(crate) catalog_id: String,
    pub(crate) provider: Option<String>,
    pub(crate) engine: CliEngine,
    pub(crate) account_ref: Option<String>,
    pub(crate) discovered_at: String,
    pub(crate) source: Option<String>,
    #[serde(default)]
    pub(crate) entries: Vec<ProviderModelCatalogEntry>,
    #[serde(default)]
    pub(crate) diagnostics: Vec<ProviderBoxDiagnostic>,
}

impl ProviderModelCatalog {
    pub(crate) fn unsupported(request: &ProviderInteractionRequest, code: &str) -> Self {
        Self {
            schema: "missiond.provider-model-catalog.v1".to_string(),
            catalog_id: format!("catalog-{}", uuid::Uuid::new_v4().simple()),
            provider: request.provider.clone(),
            engine: request.engine,
            account_ref: None,
            discovered_at: chrono::Utc::now().to_rfc3339(),
            source: None,
            entries: Vec::new(),
            diagnostics: vec![ProviderBoxDiagnostic::unsupported(
                code,
                "Provider driver has no stable model catalog discovery source yet",
                json!({
                    "engine": request.engine.to_string(),
                    "rule": "do not infer catalog entries from provider transcript text",
                }),
            )],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ProviderRouterExport {
    pub(crate) schema: String,
    pub(crate) export_id: String,
    pub(crate) catalog_id: Option<String>,
    pub(crate) provider: Option<String>,
    pub(crate) engine: CliEngine,
    #[serde(default)]
    pub(crate) router_backend_ids: Vec<String>,
    #[serde(default)]
    pub(crate) routeable_entries: Vec<Value>,
    #[serde(default)]
    pub(crate) blocked_entries: Vec<Value>,
    pub(crate) policy_ref: Option<String>,
    #[serde(default)]
    pub(crate) diagnostics: Vec<ProviderBoxDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ModelSwitchResult {
    pub(crate) status: ModelSwitchStatus,
    pub(crate) requested_model: Option<String>,
    pub(crate) requested_model_profile: Option<String>,
    pub(crate) verified_model: Option<String>,
    pub(crate) verification_source: Option<String>,
}

impl ModelSwitchResult {
    pub(crate) fn unsupported(request: &ProviderInteractionRequest) -> Self {
        let policy = request.model_switch_policy.clone().unwrap_or_default();
        Self {
            status: ModelSwitchStatus::Unsupported,
            requested_model: policy.target_model.or_else(|| request.model.clone()),
            requested_model_profile: policy
                .target_model_profile
                .or_else(|| request.model_profile.clone()),
            verified_model: None,
            verification_source: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct PtyObservation {
    pub(crate) observed_at: String,
    pub(crate) source: String,
    pub(crate) screen_hash: Option<String>,
    pub(crate) text_excerpt: Option<String>,
    pub(crate) structured_state: Option<Value>,
}

impl PtyObservation {
    pub(crate) fn text(source: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            observed_at: chrono::Utc::now().to_rfc3339(),
            source: source.into(),
            screen_hash: Some(screen_hash(&text)),
            text_excerpt: Some(text.chars().take(1200).collect()),
            structured_state: None,
        }
    }

    pub(crate) fn structured(
        source: impl Into<String>,
        text: impl Into<String>,
        structured_state: Value,
    ) -> Self {
        let text = text.into();
        Self {
            observed_at: chrono::Utc::now().to_rfc3339(),
            source: source.into(),
            screen_hash: Some(screen_hash(&text)),
            text_excerpt: Some(text.chars().take(1200).collect()),
            structured_state: Some(structured_state),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct PtyStepAction {
    pub(crate) action_type: String,
    pub(crate) human_input: String,
    pub(crate) redacted: bool,
}

impl PtyStepAction {
    pub(crate) fn key(key: impl Into<String>) -> Self {
        Self {
            action_type: "key".to_string(),
            human_input: key.into(),
            redacted: false,
        }
    }

    pub(crate) fn text(text: impl Into<String>) -> Self {
        Self {
            action_type: "text".to_string(),
            human_input: text.into(),
            redacted: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct PtyStepRecord {
    pub(crate) step_id: String,
    pub(crate) before: PtyObservation,
    pub(crate) action: PtyStepAction,
    pub(crate) after: PtyObservation,
    pub(crate) expected_change: Option<String>,
    pub(crate) verification_status: PtyStepVerificationStatus,
    #[serde(default)]
    pub(crate) diagnostics: Vec<ProviderBoxDiagnostic>,
}

impl PtyStepRecord {
    pub(crate) fn new(
        before: PtyObservation,
        action: PtyStepAction,
        after: PtyObservation,
        expected_change: Option<String>,
        verification_status: PtyStepVerificationStatus,
    ) -> Self {
        Self {
            step_id: format!("step-{}", uuid::Uuid::new_v4().simple()),
            before,
            action,
            after,
            expected_change,
            verification_status,
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn is_verified(&self) -> bool {
        self.verification_status == PtyStepVerificationStatus::Verified
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ProviderBoxResult {
    pub(crate) schema: String,
    pub(crate) turn_id: String,
    pub(crate) command: BoxCommand,
    pub(crate) status: ProviderBoxStatus,
    pub(crate) provider: Option<String>,
    pub(crate) engine: CliEngine,
    pub(crate) lease_id: Option<String>,
    pub(crate) slot_id: Option<String>,
    pub(crate) provider_conversation_id: Option<String>,
    pub(crate) durable_source: Option<String>,
    pub(crate) final_text: Option<String>,
    pub(crate) artifact_hash: Option<String>,
    pub(crate) correlation_id: String,
    #[serde(default)]
    pub(crate) diagnostics: Vec<ProviderBoxDiagnostic>,
    #[serde(default)]
    pub(crate) step_records: Vec<PtyStepRecord>,
    pub(crate) usage_snapshot: Option<ProviderUsageSnapshot>,
    pub(crate) model_catalog: Option<ProviderModelCatalog>,
    pub(crate) router_export: Option<ProviderRouterExport>,
    pub(crate) model_switch_result: Option<ModelSwitchResult>,
}

impl ProviderBoxResult {
    pub(crate) fn base(request: &ProviderInteractionRequest, status: ProviderBoxStatus) -> Self {
        Self {
            schema: "missiond.provider-interaction-turn.v1".to_string(),
            turn_id: format!("turn-{}", uuid::Uuid::new_v4().simple()),
            command: request.command,
            status,
            provider: request.provider.clone(),
            engine: request.engine,
            lease_id: request.lease_id.clone(),
            slot_id: request.slot_id.clone(),
            provider_conversation_id: None,
            durable_source: None,
            final_text: None,
            artifact_hash: None,
            correlation_id: request.correlation_id.clone(),
            diagnostics: Vec::new(),
            step_records: Vec::new(),
            usage_snapshot: None,
            model_catalog: None,
            router_export: None,
            model_switch_result: None,
        }
    }

    pub(crate) fn unsupported(
        request: &ProviderInteractionRequest,
        code: &str,
        message: impl Into<String>,
    ) -> Self {
        let mut result = Self::base(request, ProviderBoxStatus::Unsupported);
        result.diagnostics.push(ProviderBoxDiagnostic::unsupported(
            code,
            message,
            json!({
                "engine": request.engine.to_string(),
                "command": request.command,
                "slot_id": request.slot_id,
                "correlation_id": request.correlation_id,
            }),
        ));
        result
    }

    pub(crate) fn add_diagnostic(&mut self, diagnostic: ProviderBoxDiagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub(crate) fn record_step(&mut self, step: PtyStepRecord) {
        self.step_records.push(step);
    }
}

fn screen_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod step_record_tests {
    use super::{
        PtyObservation, PtyStepAction, PtyStepRecord, PtyStepVerificationStatus,
        TimeoutCancelPolicy,
    };

    #[test]
    fn pty_step_record_keeps_before_after_and_verified_status() {
        let before = PtyObservation::text("pty-screen", "one");
        let after = PtyObservation::text("pty-screen", "two");
        let step = PtyStepRecord::new(
            before,
            PtyStepAction::key("down"),
            after,
            Some("selection moved down".to_string()),
            PtyStepVerificationStatus::Verified,
        );

        assert!(step.is_verified());
        assert_eq!(step.action.human_input, "down");
        assert!(step.before.screen_hash.is_some());
        assert!(step.after.screen_hash.is_some());
        assert_ne!(step.before.screen_hash, step.after.screen_hash);
    }

    #[test]
    fn timeout_cancel_policy_defaults_to_escape_cancel_and_one_retry() {
        let policy = TimeoutCancelPolicy::default();

        assert_eq!(policy.cancel_key, "escape");
        assert_eq!(policy.max_cancel_attempts, 1);
        assert_eq!(policy.max_retries, 1);
        assert!(policy.retry_after_cancel);
        assert!(policy.require_ready_after_cancel);
        assert!(policy.running_timeout_secs > policy.cancel_grace_secs);
    }
}
