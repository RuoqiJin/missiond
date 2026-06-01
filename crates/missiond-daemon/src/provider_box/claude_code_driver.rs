use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use missiond_core::db::traits::MissionStore;
use missiond_core::pty::{recognize_screen, recognize_styled_screen, PtyCanonicalState};
use missiond_core::types::{CliEngine, SharedProjectRegistry};
use missiond_core::{LearnedPermissions, PTYManager, PTYSlot, PTYSpawnOptions, SessionState};
use serde_json::{json, Value};
use tokio::sync::{Mutex, RwLock};

use super::driver::{ProviderDriver, ProviderDriverCapabilities};
use super::types::{
    ModelSwitchResult, ModelSwitchStatus, ProviderBoxDiagnostic, ProviderBoxResult,
    ProviderBoxStatus, ProviderControlAction, ProviderInteractionRequest, PtyObservation,
    PtyStepAction, PtyStepRecord, PtyStepVerificationStatus, DIAG_MODEL_SWITCH_UNVERIFIED,
    DIAG_PROVIDER_BOX_INVALID_REQUEST, DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
    DIAG_PROVIDER_CONTROL_ACTION_UNSUPPORTED, DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
};

const DEFAULT_CLAUDE_CODE_SLOT: &str = "slot-claude-code-default";
const OBSERVE_SETTLE_MS: u64 = 350;
const OBSERVE_STABLE_POLL_MS: u64 = 120;
const OBSERVE_STABLE_MAX_MS: u64 = 1_000;

#[derive(Clone)]
pub(crate) struct ClaudeCodeProviderDriver {
    pty: Arc<PTYManager>,
    store: Arc<dyn MissionStore>,
    pty_session_uuids: Arc<RwLock<HashSet<String>>>,
    project_registry: SharedProjectRegistry,
    learned: Option<Arc<LearnedPermissions>>,
    slot_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

#[derive(Debug, Clone)]
struct ClaudeCodeObservation {
    lines: Vec<String>,
    text: String,
    snapshot: missiond_core::pty::PtyRecognitionSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClaudeCodeModelTarget {
    command_id: &'static str,
    display_name: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeCodePermissionMode {
    Auto,
    Default,
    AcceptEdits,
    Plan,
    BypassPermissions,
}

impl ClaudeCodePermissionMode {
    fn value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Default => "default",
            Self::AcceptEdits => "accept_edits",
            Self::Plan => "plan",
            Self::BypassPermissions => "bypass_permissions",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto mode",
            Self::Default => "default mode",
            Self::AcceptEdits => "accept edits mode",
            Self::Plan => "plan mode",
            Self::BypassPermissions => "bypass permissions mode",
        }
    }

    fn shift_tab_cycle() -> &'static [Self] {
        &[Self::Auto, Self::Default, Self::AcceptEdits, Self::Plan]
    }
}

impl ClaudeCodeProviderDriver {
    pub(crate) fn new(
        pty: Arc<PTYManager>,
        store: Arc<dyn MissionStore>,
        pty_session_uuids: Arc<RwLock<HashSet<String>>>,
        project_registry: SharedProjectRegistry,
        learned: Option<Arc<LearnedPermissions>>,
    ) -> Self {
        Self {
            pty,
            store,
            pty_session_uuids,
            project_registry,
            learned,
            slot_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn slot_lock(&self, slot_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.slot_locks.lock().await;
        locks
            .entry(slot_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    fn request_slot_id(request: &ProviderInteractionRequest) -> String {
        request
            .slot_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_CLAUDE_CODE_SLOT.to_string())
    }

    fn request_spawn_if_missing(request: &ProviderInteractionRequest) -> bool {
        request
            .desired_worker
            .as_ref()
            .and_then(|worker| {
                worker
                    .get("spawn_if_missing")
                    .or_else(|| worker.get("spawn"))
                    .and_then(Value::as_bool)
            })
            .unwrap_or(false)
            || request
                .model_switch_policy
                .as_ref()
                .is_some_and(|policy| policy.allow_respawn)
    }

    fn request_force_restart(request: &ProviderInteractionRequest) -> bool {
        request
            .desired_worker
            .as_ref()
            .and_then(|worker| {
                worker
                    .get("force_restart")
                    .or_else(|| worker.get("restart"))
                    .or_else(|| worker.get("respawn"))
                    .and_then(Value::as_bool)
            })
            .unwrap_or(false)
    }

    fn request_dangerous_bypass(request: &ProviderInteractionRequest) -> bool {
        const KEYS: &[&str] = &[
            "dangerously_bypass_approvals_and_sandbox",
            "dangerously_skip_permissions",
            "dangerously_bypass",
            "bypass_approvals_and_sandbox",
            "bypass_mode",
            "bypass",
        ];
        request.dangerously_bypass_approvals_and_sandbox
            || bool_any(request.tool_policy.as_ref(), KEYS)
            || bool_any(request.desired_worker.as_ref(), KEYS)
    }

    fn request_launch_model(request: &ProviderInteractionRequest) -> Option<String> {
        request.model.as_deref().map(|model| {
            normalize_claude_code_model_target(model)
                .map(|target| target.command_id.to_string())
                .unwrap_or_else(|| model.to_string())
        })
    }

    fn request_target_model(
        request: &ProviderInteractionRequest,
    ) -> Result<ClaudeCodeModelTarget, ProviderBoxDiagnostic> {
        let raw = request
            .model_switch_policy
            .as_ref()
            .and_then(|policy| policy.target_model.as_deref())
            .or(request.model.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "ClaudeCode model switch requires model or target_model",
                    json!({
                        "allowed_model_ids": Self::allowed_model_ids(),
                    }),
                )
            })?;

        normalize_claude_code_model_target(raw).ok_or_else(|| {
            ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode model switch uses an unsupported model id",
                json!({
                    "requested_model": raw,
                    "allowed_model_ids": Self::allowed_model_ids(),
                }),
            )
        })
    }

    fn allowed_model_ids() -> Vec<&'static str> {
        vec!["claude-opus-4-6", "claude-sonnet-4-6"]
    }

    fn request_permission_mode(
        request: &ProviderInteractionRequest,
    ) -> Result<ClaudeCodePermissionMode, ProviderBoxDiagnostic> {
        let raw = request
            .desired_worker
            .as_ref()
            .and_then(|worker| {
                worker
                    .get("permission_mode")
                    .or_else(|| worker.get("mode"))
                    .or_else(|| worker.get("target_permission_mode"))
                    .and_then(Value::as_str)
            })
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "ClaudeCode permissions control action requires permission_mode",
                    json!({
                        "slot_id": request.slot_id,
                        "allowed_permission_modes": Self::allowed_permission_modes(),
                    }),
                )
            })?;
        normalize_claude_code_permission_mode(raw).ok_or_else(|| {
            ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode permissions control action uses an unsupported permission_mode",
                json!({
                    "slot_id": request.slot_id,
                    "permission_mode": raw,
                    "allowed_permission_modes": Self::allowed_permission_modes(),
                }),
            )
        })
    }

    fn allowed_permission_modes() -> Vec<&'static str> {
        vec!["auto", "default", "accept_edits", "plan"]
    }

    async fn ensure_slot(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> Option<String> {
        let slot_id = Self::request_slot_id(request);
        if let Some(status) = self.pty.get_status(&slot_id).await {
            if status.engine != CliEngine::ClaudeCode {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Requested provider-box slot is not a ClaudeCode slot",
                    json!({
                        "slot_id": slot_id,
                        "engine": status.engine.to_string(),
                    }),
                ));
                return None;
            }
            if Self::request_force_restart(request) {
                let _ = self.pty.kill(&slot_id).await;
            } else if !matches!(status.state, SessionState::Exited | SessionState::Error) {
                return Some(slot_id);
            }
        }

        if !Self::request_spawn_if_missing(request) && !Self::request_force_restart(request) {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "ClaudeCode slot status is unavailable",
                json!({
                    "slot_id": slot_id,
                    "rule": "set spawn_if_missing=true or call /provider-box/v1/slots/<slot_id>/spawn to launch a ClaudeCode PTY through provider-box"
                }),
            ));
            return None;
        }

        let cwd = request
            .cwd
            .as_ref()
            .or(request.project_root.as_ref())
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
        let slot = PTYSlot {
            id: slot_id.clone(),
            role: "provider-box-claude-code".to_string(),
            cwd: Some(cwd),
            engine: CliEngine::ClaudeCode,
        };
        self.pty.init_slot(&slot).await;

        let dangerous_bypass = Self::request_dangerous_bypass(request);
        let mut extra_env = HashMap::new();
        if dangerous_bypass {
            extra_env.insert(
                "MISSIOND_ALLOW_BROAD_SKIP_PERMISSIONS".to_string(),
                "true".to_string(),
            );
        }
        let options = PTYSpawnOptions {
            auto_restart: true,
            wait_for_idle: false,
            timeout_secs: Some(90),
            mcp_config: None,
            dangerously_skip_permissions: dangerous_bypass,
            model: Self::request_launch_model(request),
            reasoning_effort: None,
            search_enabled: false,
            sandbox: None,
            approval_policy: None,
            tool_policy_path: None,
            extra_env,
            initial_prompt: None,
            command_override: None,
        };

        match crate::slot_orchestrator::spawner::spawn_tracked_slot(
            &self.pty,
            &self.store,
            &self.pty_session_uuids,
            &self.project_registry,
            self.learned.as_ref(),
            &slot,
            options,
            None,
        )
        .await
        {
            Ok(_) => {
                self.wait_step_until(
                    result,
                    &slot_id,
                    Duration::from_secs(12),
                    Some("wait for ClaudeCode startup, trust, or ready surface".to_string()),
                    |obs| {
                        !obs.text.trim().is_empty()
                            && obs.snapshot.reason != "session_state:Starting"
                    },
                )
                .await;
                Some(slot_id)
            }
            Err(err) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "ClaudeCode PTY slot could not be spawned by provider-box",
                    json!({
                        "slot_id": slot_id,
                        "error": err.to_string(),
                        "dangerously_skip_permissions_requested": dangerous_bypass,
                    }),
                ));
                None
            }
        }
    }

    async fn observe(&self, slot_id: &str) -> ClaudeCodeObservation {
        let styled_screen = self.pty.get_styled_screen(slot_id).await.ok();
        let lines = if let Some(screen) = styled_screen.as_ref() {
            screen
                .lines
                .iter()
                .map(|line| line.text.clone())
                .collect::<Vec<_>>()
        } else {
            self.pty
                .get_last_lines(slot_id, 180)
                .await
                .unwrap_or_else(|_| Vec::new())
        };
        let status = self.pty.get_status(slot_id).await;
        let state = status
            .as_ref()
            .map(|info| info.state)
            .unwrap_or(SessionState::Idle);
        let snapshot = if let Some(screen) = styled_screen.as_ref() {
            recognize_styled_screen(CliEngine::ClaudeCode, screen, state)
        } else {
            recognize_screen(CliEngine::ClaudeCode, &lines, state)
        };
        let text = lines.join("\n");
        ClaudeCodeObservation {
            lines,
            text,
            snapshot,
        }
    }

    fn observations_equivalent(
        left: &ClaudeCodeObservation,
        right: &ClaudeCodeObservation,
    ) -> bool {
        left.text == right.text
            && left.snapshot.state == right.snapshot.state
            && left.snapshot.reason == right.snapshot.reason
            && left.snapshot.blocked_kind == right.snapshot.blocked_kind
    }

    fn observations_changed(before: &ClaudeCodeObservation, after: &ClaudeCodeObservation) -> bool {
        !Self::observations_equivalent(before, after)
    }

    async fn observe_after_action(&self, slot_id: &str) -> ClaudeCodeObservation {
        let started = Instant::now();
        tokio::time::sleep(Duration::from_millis(OBSERVE_SETTLE_MS)).await;
        let mut previous = self.observe(slot_id).await;

        loop {
            if started.elapsed() >= Duration::from_millis(OBSERVE_STABLE_MAX_MS) {
                return previous;
            }

            tokio::time::sleep(Duration::from_millis(OBSERVE_STABLE_POLL_MS)).await;
            let current = self.observe(slot_id).await;
            if Self::observations_equivalent(&previous, &current) {
                return current;
            }
            previous = current;
        }
    }

    fn pty_observation(slot_id: &str, observation: &ClaudeCodeObservation) -> PtyObservation {
        PtyObservation::structured(
            format!("pty:{slot_id}"),
            observation.text.clone(),
            serde_json::to_value(&observation.snapshot).unwrap_or_else(|_| json!({})),
        )
    }

    async fn write_step(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        action: PtyStepAction,
        bytes: &str,
        expected_change: Option<String>,
    ) -> ClaudeCodeObservation {
        let before = self.observe(slot_id).await;
        let write_result = self.pty.write(slot_id, bytes).await;
        let after = self.observe_after_action(slot_id).await;
        let status = if write_result.is_err() {
            PtyStepVerificationStatus::Failed
        } else if Self::observations_changed(&before, &after) {
            PtyStepVerificationStatus::Verified
        } else {
            PtyStepVerificationStatus::Unchanged
        };
        let mut step = PtyStepRecord::new(
            Self::pty_observation(slot_id, &before),
            action,
            Self::pty_observation(slot_id, &after),
            expected_change,
            status,
        );
        if let Err(err) = write_result {
            step.diagnostics.push(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "ClaudeCode PTY write failed",
                json!({
                    "slot_id": slot_id,
                    "error": err.to_string(),
                }),
            ));
        }
        result.record_step(step);
        after
    }

    async fn wait_until<F>(
        &self,
        slot_id: &str,
        timeout: Duration,
        mut predicate: F,
    ) -> ClaudeCodeObservation
    where
        F: FnMut(&ClaudeCodeObservation) -> bool,
    {
        let started = Instant::now();
        loop {
            let observation = self.observe(slot_id).await;
            if predicate(&observation) || started.elapsed() >= timeout {
                return observation;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    async fn wait_step_until<F>(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        timeout: Duration,
        expected_change: Option<String>,
        mut predicate: F,
    ) -> ClaudeCodeObservation
    where
        F: FnMut(&ClaudeCodeObservation) -> bool,
    {
        let before = self.observe(slot_id).await;
        let after = self
            .wait_until(slot_id, timeout, |obs| predicate(obs))
            .await;
        let status = if predicate(&after) {
            PtyStepVerificationStatus::Verified
        } else if before.text != after.text {
            PtyStepVerificationStatus::Ambiguous
        } else {
            PtyStepVerificationStatus::Unchanged
        };
        result.record_step(PtyStepRecord::new(
            Self::pty_observation(slot_id, &before),
            PtyStepAction::key("wait"),
            Self::pty_observation(slot_id, &after),
            expected_change,
            status,
        ));
        after
    }

    async fn attach_status_observation(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        expected_change: Option<String>,
    ) -> ClaudeCodeObservation {
        let observation = self.observe(slot_id).await;
        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
        let pty_observation = Self::pty_observation(slot_id, &observation);
        result.record_step(PtyStepRecord::new(
            pty_observation.clone(),
            PtyStepAction::key("observe"),
            pty_observation,
            expected_change,
            PtyStepVerificationStatus::Skipped,
        ));
        observation
    }

    async fn ensure_ready_for_model_command(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) -> Option<ClaudeCodeObservation> {
        let mut observation = self.observe(slot_id).await;
        if !is_ready_for_claude_code_text(&observation) {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(8),
                    Some("wait for ClaudeCode prompt idle before model switch".to_string()),
                    is_ready_for_claude_code_text,
                )
                .await;
        }
        if !is_ready_for_claude_code_text(&observation) {
            result.status = ProviderBoxStatus::Blocked;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_MODEL_SWITCH_UNVERIFIED,
                "ClaudeCode model switch requires an idle composer",
                json!({
                    "slot_id": slot_id,
                    "reason": observation.snapshot.reason,
                    "state": observation.snapshot.state,
                    "blocked_kind": observation.snapshot.blocked_kind,
                }),
            ));
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            return None;
        }
        if let Some(text) = claude_code_composer_text(&observation) {
            if !text.trim().is_empty() {
                result.status = ProviderBoxStatus::Blocked;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_MODEL_SWITCH_UNVERIFIED,
                    "ClaudeCode composer is not empty; refusing to append /model command",
                    json!({
                        "slot_id": slot_id,
                        "composer_text_preview": text.chars().take(120).collect::<String>(),
                        "safe_alternative": "clear the composer through a taught provider-box control before switching models",
                    }),
                ));
                let status = self.pty.get_status(slot_id).await;
                result.slot_status =
                    Some(slot_status_value(slot_id, status.as_ref(), &observation));
                return None;
            }
        }
        Some(observation)
    }

    async fn ensure_ready_for_permission_cycle(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) -> Option<ClaudeCodeObservation> {
        let mut observation = self.observe(slot_id).await;
        if !is_ready_for_claude_code_text(&observation) {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(8),
                    Some(
                        "wait for ClaudeCode prompt idle before permission mode switch".to_string(),
                    ),
                    is_ready_for_claude_code_text,
                )
                .await;
        }
        if !is_ready_for_claude_code_text(&observation) {
            result.status = ProviderBoxStatus::Blocked;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "ClaudeCode permissions mode switch requires an idle composer",
                json!({
                    "slot_id": slot_id,
                    "reason": observation.snapshot.reason,
                    "state": observation.snapshot.state,
                    "blocked_kind": observation.snapshot.blocked_kind,
                }),
            ));
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            return None;
        }
        if let Some(text) = claude_code_composer_text(&observation) {
            if !text.trim().is_empty() {
                result.status = ProviderBoxStatus::Blocked;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                    "ClaudeCode composer is not empty; refusing to cycle permission mode",
                    json!({
                        "slot_id": slot_id,
                        "composer_text_preview": text.chars().take(120).collect::<String>(),
                        "safe_alternative": "clear the composer through a taught provider-box control before switching permission modes",
                    }),
                ));
                let status = self.pty.get_status(slot_id).await;
                result.slot_status =
                    Some(slot_status_value(slot_id, status.as_ref(), &observation));
                return None;
            }
        }
        Some(observation)
    }

    async fn set_permissions_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) {
        let target = match Self::request_permission_mode(request) {
            Ok(mode) => mode,
            Err(diagnostic) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(diagnostic);
                return;
            }
        };

        if target == ClaudeCodePermissionMode::BypassPermissions {
            result.status = ProviderBoxStatus::Unsupported;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNSUPPORTED,
                "ClaudeCode bypass permissions is a launch-time policy, not a taught Shift+Tab target",
                json!({
                    "slot_id": slot_id,
                    "requested_permission_mode": target.value(),
                    "safe_alternative": "restart or spawn the ClaudeCode slot with dangerously_skip_permissions=true after confirming context loss is acceptable",
                }),
            ));
            let observation = self.observe(slot_id).await;
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            return;
        }

        let Some(mut observation) = self
            .ensure_ready_for_permission_cycle(result, slot_id)
            .await
        else {
            return;
        };

        let Some(current) = claude_code_permission_mode(&observation) else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "ClaudeCode current permission mode was not recognized from the footer",
                json!({
                    "slot_id": slot_id,
                    "target_permission_mode": target.value(),
                    "reason": observation.snapshot.reason,
                    "screen_identity": observation.snapshot.screen_identity.clone(),
                }),
            ));
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            return;
        };

        if current == target {
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            result.status = ProviderBoxStatus::Completed;
            result.final_text = Some(format!(
                "ClaudeCode permission mode already {}",
                target.value()
            ));
            result.durable_source = Some("claude_code_screen_identity_permission_mode".to_string());
            return;
        }

        let Some(expected_modes) = claude_code_permission_cycle_steps(current, target) else {
            result.status = ProviderBoxStatus::Unsupported;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNSUPPORTED,
                "ClaudeCode permission mode transition is not in the taught Shift+Tab cycle",
                json!({
                    "slot_id": slot_id,
                    "current_permission_mode": current.value(),
                    "target_permission_mode": target.value(),
                    "taught_cycle": ClaudeCodePermissionMode::shift_tab_cycle()
                        .iter()
                        .map(|mode| mode.value())
                        .collect::<Vec<_>>(),
                }),
            ));
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            return;
        };

        for (step_index, expected_mode) in expected_modes.iter().copied().enumerate() {
            observation = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key("shift-tab"),
                    "\x1b[Z",
                    Some(format!(
                        "cycle ClaudeCode permission mode toward {}",
                        target.value()
                    )),
                )
                .await;

            if claude_code_permission_mode(&observation) != Some(expected_mode) {
                observation = self
                    .wait_step_until(
                        result,
                        slot_id,
                        Duration::from_secs(3),
                        Some(format!(
                            "wait for ClaudeCode permission footer to become {}",
                            expected_mode.value()
                        )),
                        |obs| claude_code_permission_mode(obs) == Some(expected_mode),
                    )
                    .await;
            }

            if claude_code_permission_mode(&observation) != Some(expected_mode) {
                result.status = ProviderBoxStatus::Unverified;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                    "ClaudeCode permission mode did not advance to the expected footer state",
                    json!({
                        "slot_id": slot_id,
                        "target_permission_mode": target.value(),
                        "expected_permission_mode": expected_mode.value(),
                        "observed_permission_mode": claude_code_permission_mode(&observation).map(|mode| mode.value()),
                        "step_index": step_index,
                    }),
                ));
                let status = self.pty.get_status(slot_id).await;
                result.slot_status =
                    Some(slot_status_value(slot_id, status.as_ref(), &observation));
                return;
            }
        }

        let verified = claude_code_permission_mode(&observation);
        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
        if verified == Some(target) {
            result.status = ProviderBoxStatus::Completed;
            result.final_text = Some(format!(
                "ClaudeCode permission mode switched to {}",
                target.value()
            ));
            result.durable_source = Some("claude_code_screen_identity_permission_mode".to_string());
        } else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "ClaudeCode permission mode switch ended on the wrong mode",
                json!({
                    "slot_id": slot_id,
                    "target_permission_mode": target.value(),
                    "observed_permission_mode": verified.map(|mode| mode.value()),
                }),
            ));
        }
    }
}

#[async_trait]
impl ProviderDriver for ClaudeCodeProviderDriver {
    fn engine(&self) -> CliEngine {
        CliEngine::ClaudeCode
    }

    fn capabilities(&self) -> ProviderDriverCapabilities {
        ProviderDriverCapabilities {
            submit_turn: false,
            switch_model: true,
            usage_probe: false,
            model_catalog: false,
            pure_text_guard: false,
            control_action: true,
            pty_step: false,
            status: true,
            mcp_status: false,
            mcp_reconnect: false,
        }
    }

    async fn status(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        self.attach_status_observation(
            &mut result,
            &slot_id,
            Some("observe current ClaudeCode CLI state".to_string()),
        )
        .await;
        result.status = ProviderBoxStatus::Completed;
        result
    }

    async fn switch_model(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let target = match Self::request_target_model(request) {
            Ok(target) => target,
            Err(diagnostic) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(diagnostic);
                result.model_switch_result = Some(ModelSwitchResult {
                    status: ModelSwitchStatus::Unknown,
                    requested_model: request.model.clone(),
                    requested_model_profile: request.model_profile.clone(),
                    verified_model: None,
                    verification_source: None,
                });
                return result;
            }
        };
        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            result.model_switch_result = Some(ModelSwitchResult {
                status: ModelSwitchStatus::Unknown,
                requested_model: Some(target.command_id.to_string()),
                requested_model_profile: request.model_profile.clone(),
                verified_model: None,
                verification_source: None,
            });
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;

        let Some(mut observation) = self
            .ensure_ready_for_model_command(&mut result, &slot_id)
            .await
        else {
            result.model_switch_result = Some(ModelSwitchResult {
                status: ModelSwitchStatus::Unverified,
                requested_model: Some(target.command_id.to_string()),
                requested_model_profile: request.model_profile.clone(),
                verified_model: claude_code_current_model_from_result(&result),
                verification_source: Some("claude_code_screen_identity_current_model".to_string()),
            });
            return result;
        };

        if claude_code_model_matches(&observation, target) {
            let verified_model = claude_code_current_model(&observation);
            let status = self.pty.get_status(&slot_id).await;
            result.slot_status = Some(slot_status_value(&slot_id, status.as_ref(), &observation));
            result.status = ProviderBoxStatus::Completed;
            result.final_text = Some(format!("ClaudeCode model already {}", target.display_name));
            result.durable_source = Some("claude_code_screen_identity_current_model".to_string());
            result.model_switch_result = Some(ModelSwitchResult {
                status: ModelSwitchStatus::Verified,
                requested_model: Some(target.command_id.to_string()),
                requested_model_profile: request.model_profile.clone(),
                verified_model,
                verification_source: Some("claude_code_screen_identity_current_model".to_string()),
            });
            return result;
        }

        let command = format!("/model {}", target.command_id);
        let _ = self
            .write_step(
                &mut result,
                &slot_id,
                PtyStepAction::text(command.clone()),
                &command,
                Some(format!("type ClaudeCode {command} command")),
            )
            .await;
        observation = self
            .write_step(
                &mut result,
                &slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some(format!("execute ClaudeCode {command} command")),
            )
            .await;
        if !claude_code_model_matches(&observation, target) {
            observation = self
                .wait_step_until(
                    &mut result,
                    &slot_id,
                    Duration::from_secs(10),
                    Some(format!(
                        "wait for ClaudeCode current model to become {}",
                        target.display_name
                    )),
                    |obs| {
                        is_ready_for_claude_code_text(obs) && claude_code_model_matches(obs, target)
                    },
                )
                .await;
        }

        let verified_model = claude_code_current_model(&observation);
        let status = self.pty.get_status(&slot_id).await;
        result.slot_status = Some(slot_status_value(&slot_id, status.as_ref(), &observation));
        if is_ready_for_claude_code_text(&observation)
            && verified_model.as_deref() == Some(target.display_name)
        {
            result.status = ProviderBoxStatus::Completed;
            result.final_text = Some(format!(
                "ClaudeCode model switched to {}",
                target.display_name
            ));
            result.durable_source = Some("claude_code_screen_identity_current_model".to_string());
            result.model_switch_result = Some(ModelSwitchResult {
                status: ModelSwitchStatus::Verified,
                requested_model: Some(target.command_id.to_string()),
                requested_model_profile: request.model_profile.clone(),
                verified_model,
                verification_source: Some("claude_code_screen_identity_current_model".to_string()),
            });
        } else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_MODEL_SWITCH_UNVERIFIED,
                "ClaudeCode /model command did not verify the requested model",
                json!({
                    "slot_id": slot_id,
                    "requested_model": target.command_id,
                    "requested_display_name": target.display_name,
                    "observed_model": verified_model,
                    "reason": observation.snapshot.reason,
                    "state": observation.snapshot.state,
                }),
            ));
            result.model_switch_result = Some(ModelSwitchResult {
                status: ModelSwitchStatus::Unverified,
                requested_model: Some(target.command_id.to_string()),
                requested_model_profile: request.model_profile.clone(),
                verified_model,
                verification_source: Some("claude_code_screen_identity_current_model".to_string()),
            });
        }
        result
    }

    async fn control_action(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let Some(action) = request.control_action else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode control action request requires control_action",
                json!({
                    "slot_id": request.slot_id,
                    "command": request.command,
                }),
            ));
            return result;
        };
        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;

        match action {
            ProviderControlAction::SetPermissions => {
                self.set_permissions_locked(request, &mut result, &slot_id)
                    .await;
            }
            _ => {
                result.status = ProviderBoxStatus::Unsupported;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_CONTROL_ACTION_UNSUPPORTED,
                    "ClaudeCode control action has not been taught yet",
                    json!({
                        "slot_id": slot_id,
                        "action": action,
                        "supported_actions": ["set_permissions"],
                    }),
                ));
            }
        }

        if result.slot_status.is_none() {
            self.attach_status_observation(
                &mut result,
                &slot_id,
                Some("observe ClaudeCode state after control action".to_string()),
            )
            .await;
        }
        result
    }
}

fn slot_status_value(
    slot_id: &str,
    status: Option<&missiond_core::PTYAgentInfo>,
    observation: &ClaudeCodeObservation,
) -> Value {
    json!({
        "slot_id": slot_id,
        "engine": status.map(|info| info.engine.to_string()),
        "session_state": status.map(|info| info.state),
        "running": status
            .map(|info| !matches!(info.state, SessionState::Exited | SessionState::Error))
            .unwrap_or(false),
        "pid": status.and_then(|info| info.pid),
        "started_at": status.and_then(|info| info.started_at),
        "status_text": status.and_then(|info| info.status_text.clone()),
        "current_task_id": status.and_then(|info| info.current_task_id.clone()),
        "log_file": status.map(|info| info.log_file.display().to_string()),
        "pty_canonical_state": observation.snapshot.state,
        "reason": observation.snapshot.reason.clone(),
        "phase": observation.snapshot.phase.clone(),
        "blocked_kind": observation.snapshot.blocked_kind.clone(),
        "screen_identity": observation.snapshot.screen_identity.clone(),
        "screen_usage": observation.snapshot.screen_usage.clone(),
        "screen_mcp": observation.snapshot.screen_mcp.clone(),
        "screen_hash": PtyObservation::text("pty-screen", &observation.text).screen_hash,
    })
}

fn is_ready_for_claude_code_text(observation: &ClaudeCodeObservation) -> bool {
    observation.snapshot.state == PtyCanonicalState::Idle
        && observation.snapshot.blocked_kind.is_none()
        && observation.snapshot.reason != "session_state:Exited"
}

fn claude_code_current_model(observation: &ClaudeCodeObservation) -> Option<String> {
    observation
        .snapshot
        .screen_identity
        .as_ref()
        .and_then(|identity| identity.current_model.clone())
}

fn claude_code_permission_mode(
    observation: &ClaudeCodeObservation,
) -> Option<ClaudeCodePermissionMode> {
    observation
        .snapshot
        .screen_identity
        .as_ref()
        .and_then(|identity| identity.permission_mode.as_deref())
        .and_then(normalize_claude_code_permission_mode)
}

fn claude_code_permission_cycle_steps(
    current: ClaudeCodePermissionMode,
    target: ClaudeCodePermissionMode,
) -> Option<Vec<ClaudeCodePermissionMode>> {
    let cycle = ClaudeCodePermissionMode::shift_tab_cycle();
    let current_index = cycle.iter().position(|mode| *mode == current)?;
    let target_index = cycle.iter().position(|mode| *mode == target)?;
    let mut steps = Vec::new();
    let mut index = current_index;
    while index != target_index {
        index = (index + 1) % cycle.len();
        steps.push(cycle[index]);
    }
    Some(steps)
}

fn claude_code_model_matches(
    observation: &ClaudeCodeObservation,
    target: ClaudeCodeModelTarget,
) -> bool {
    claude_code_current_model(observation)
        .as_deref()
        .is_some_and(|model| model.trim().eq_ignore_ascii_case(target.display_name))
}

fn claude_code_composer_text(observation: &ClaudeCodeObservation) -> Option<String> {
    observation.lines.iter().rev().find_map(|line| {
        let trimmed = line.trim_start();
        let rest = trimmed
            .strip_prefix('❯')
            .or_else(|| trimmed.strip_prefix('>'))?;
        Some(rest.trim().to_string())
    })
}

fn claude_code_current_model_from_result(result: &ProviderBoxResult) -> Option<String> {
    result
        .slot_status
        .as_ref()
        .and_then(|status| status.get("screen_identity"))
        .and_then(|identity| {
            identity
                .get("currentModel")
                .or_else(|| identity.get("current_model"))
        })
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn normalize_claude_code_model_target(value: &str) -> Option<ClaudeCodeModelTarget> {
    let without_command = value
        .trim()
        .strip_prefix("/model")
        .map(str::trim)
        .unwrap_or_else(|| value.trim());
    let normalized = without_command
        .to_ascii_lowercase()
        .replace('_', "-")
        .replace('.', "-")
        .replace(' ', "-");
    match normalized.as_str() {
        "claude-opus-4-6" | "opus-4-6" | "opus4-6" | "claude-opus-46" | "opus-46" => {
            Some(ClaudeCodeModelTarget {
                command_id: "claude-opus-4-6",
                display_name: "Opus 4.6",
            })
        }
        "claude-sonnet-4-6" | "sonnet-4-6" | "sonnet4-6" | "claude-sonnet-46" | "sonnet-46" => {
            Some(ClaudeCodeModelTarget {
                command_id: "claude-sonnet-4-6",
                display_name: "Sonnet 4.6",
            })
        }
        _ => None,
    }
}

fn normalize_claude_code_permission_mode(value: &str) -> Option<ClaudeCodePermissionMode> {
    let normalized = value
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-")
        .replace(' ', "-");
    match normalized.as_str() {
        "auto" | "auto-mode" | "automode" | "auto-review" => Some(ClaudeCodePermissionMode::Auto),
        "default" | "ask" | "ask-first" | "normal" => Some(ClaudeCodePermissionMode::Default),
        "accept-edits" | "accept-edits-mode" | "acceptedits" | "accept" | "edits" => {
            Some(ClaudeCodePermissionMode::AcceptEdits)
        }
        "plan" | "plan-mode" => Some(ClaudeCodePermissionMode::Plan),
        "bypass" | "bypass-permissions" | "bypasspermissions" | "dangerously-skip-permissions" => {
            Some(ClaudeCodePermissionMode::BypassPermissions)
        }
        _ => None,
    }
}

fn bool_any(value: Option<&Value>, keys: &[&str]) -> bool {
    let Some(value) = value else {
        return false;
    };
    keys.iter()
        .any(|key| value.get(*key).and_then(Value::as_bool).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::{
        claude_code_permission_cycle_steps, normalize_claude_code_model_target,
        normalize_claude_code_permission_mode, ClaudeCodeModelTarget, ClaudeCodePermissionMode,
    };

    #[test]
    fn normalizes_claude_code_opus_model_command_aliases() {
        let target = normalize_claude_code_model_target("/model claude-opus-4-6")
            .expect("opus command target");
        assert_eq!(
            target,
            ClaudeCodeModelTarget {
                command_id: "claude-opus-4-6",
                display_name: "Opus 4.6"
            }
        );

        let target = normalize_claude_code_model_target("Opus 4.6").expect("opus display target");
        assert_eq!(target.command_id, "claude-opus-4-6");
        assert_eq!(target.display_name, "Opus 4.6");
    }

    #[test]
    fn normalizes_claude_code_sonnet_model_command_aliases() {
        let target =
            normalize_claude_code_model_target("claude-sonnet-4-6").expect("sonnet command target");
        assert_eq!(
            target,
            ClaudeCodeModelTarget {
                command_id: "claude-sonnet-4-6",
                display_name: "Sonnet 4.6"
            }
        );

        let target =
            normalize_claude_code_model_target("/model Sonnet 4.6").expect("sonnet display target");
        assert_eq!(target.command_id, "claude-sonnet-4-6");
        assert_eq!(target.display_name, "Sonnet 4.6");
    }

    #[test]
    fn rejects_unknown_claude_code_model_alias() {
        assert!(normalize_claude_code_model_target("claude-opus-4-8").is_none());
    }

    #[test]
    fn normalizes_claude_code_permission_mode_aliases() {
        assert_eq!(
            normalize_claude_code_permission_mode("auto"),
            Some(ClaudeCodePermissionMode::Auto)
        );
        assert_eq!(
            normalize_claude_code_permission_mode("accept edits"),
            Some(ClaudeCodePermissionMode::AcceptEdits)
        );
        assert_eq!(
            normalize_claude_code_permission_mode("plan-mode"),
            Some(ClaudeCodePermissionMode::Plan)
        );
        assert_eq!(
            normalize_claude_code_permission_mode("dangerously-skip-permissions"),
            Some(ClaudeCodePermissionMode::BypassPermissions)
        );
    }

    #[test]
    fn claude_code_permission_cycle_uses_taught_shift_tab_order() {
        assert_eq!(
            claude_code_permission_cycle_steps(
                ClaudeCodePermissionMode::Auto,
                ClaudeCodePermissionMode::Plan
            ),
            Some(vec![
                ClaudeCodePermissionMode::Default,
                ClaudeCodePermissionMode::AcceptEdits,
                ClaudeCodePermissionMode::Plan
            ])
        );
        assert_eq!(
            claude_code_permission_cycle_steps(
                ClaudeCodePermissionMode::Plan,
                ClaudeCodePermissionMode::Auto
            ),
            Some(vec![ClaudeCodePermissionMode::Auto])
        );
        assert_eq!(
            claude_code_permission_cycle_steps(
                ClaudeCodePermissionMode::BypassPermissions,
                ClaudeCodePermissionMode::Auto
            ),
            None
        );
    }
}
