use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use missiond_core::db::traits::MissionStore;
use missiond_core::pty::{recognize_screen, recognize_styled_screen, PtyCanonicalState};
use missiond_core::types::{CliEngine, SharedProjectRegistry};
use missiond_core::{LearnedPermissions, PTYManager, PTYSlot, PTYSpawnOptions, SessionState};
use serde_json::{json, Value};
use tokio::sync::{Mutex, RwLock, Semaphore};

use super::driver::{ProviderDriver, ProviderDriverCapabilities};
use super::types::{
    ModelSwitchResult, ModelSwitchStatus, ProviderBoxDiagnostic, ProviderBoxResult,
    ProviderBoxStatus, ProviderControlAction, ProviderInteractionRequest, ProviderSessionIdentity,
    PtyObservation, PtyStepAction, PtyStepRecord, PtyStepVerificationStatus,
    DIAG_MODEL_SWITCH_UNVERIFIED, DIAG_PROVIDER_BOX_INVALID_REQUEST,
    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE, DIAG_PROVIDER_CONTROL_ACTION_UNSUPPORTED,
    DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED, DIAG_PROVIDER_DURABLE_FINAL_MISSING,
    DIAG_PROVIDER_TEXT_ONLY_VIOLATION,
};

const DEFAULT_CLAUDE_CODE_SLOT: &str = "slot-claude-code-default";
const CLAUDE_CODE_TEXT_PROVIDER: &str = "claude_code_text";
const CLAUDE_CODE_TEXT_RUNTIME_DIR: &str = ".missiond/runtime/claude-code-text-only";
const CLAUDE_CODE_TEXT_EMPTY_MCP_CONFIG: &str = "empty-mcp-config.json";
const CLAUDE_CODE_TEXT_MAX_CONCURRENT: usize = 1;
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
    text_lane_locks: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    claude_home: PathBuf,
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
struct ClaudeCodeTextSourceDef {
    model_id: &'static str,
    source_id: &'static str,
    display_name: &'static str,
    provider_model_id: &'static str,
    effort: &'static str,
}

#[derive(Debug, Clone)]
struct ClaudeCodeJsonlCursor {
    path: Option<PathBuf>,
    offset: u64,
}

#[derive(Debug, Clone, Default)]
struct ClaudeCodeJsonlAnalysis {
    jsonl_path: Option<PathBuf>,
    line_count: usize,
    final_text: Option<String>,
    violation: Option<Value>,
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
            text_lane_locks: Arc::new(Mutex::new(HashMap::new())),
            claude_home: default_claude_home(),
        }
    }

    async fn slot_lock(&self, slot_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.slot_locks.lock().await;
        locks
            .entry(slot_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    async fn text_lane_semaphore(&self, queue_key: &str) -> Arc<Semaphore> {
        let mut locks = self.text_lane_locks.lock().await;
        locks
            .entry(queue_key.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(CLAUDE_CODE_TEXT_MAX_CONCURRENT)))
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

    fn request_allow_hidden_logout(request: &ProviderInteractionRequest) -> bool {
        request
            .desired_worker
            .as_ref()
            .and_then(|worker| worker.get("allow_hidden_logout").and_then(Value::as_bool))
            .unwrap_or(false)
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
        vec!["claude-opus-4-8", "claude-opus-4-6", "claude-sonnet-4-6"]
    }

    fn text_source_defs() -> &'static [ClaudeCodeTextSourceDef] {
        &[
            ClaudeCodeTextSourceDef {
                model_id: "claude-code-opus-4-8-xhigh",
                source_id: "missiond/claude-code-text/opus-4-8-xhigh",
                display_name: "ClaudeCode Opus 4.8 (xhigh)",
                provider_model_id: "claude-opus-4-8",
                effort: "xhigh",
            },
            ClaudeCodeTextSourceDef {
                model_id: "claude-code-opus-4-6-high",
                source_id: "missiond/claude-code-text/opus-4-6-high",
                display_name: "ClaudeCode Opus 4.6 (high)",
                provider_model_id: "claude-opus-4-6",
                effort: "high",
            },
            ClaudeCodeTextSourceDef {
                model_id: "claude-code-sonnet-4-6-high",
                source_id: "missiond/claude-code-text/sonnet-4-6-high",
                display_name: "ClaudeCode Sonnet 4.6 (high)",
                provider_model_id: "claude-sonnet-4-6",
                effort: "high",
            },
        ]
    }

    fn request_text_source(
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> Option<ClaudeCodeTextSourceDef> {
        let provider = request
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(CLAUDE_CODE_TEXT_PROVIDER);
        if !provider.eq_ignore_ascii_case(CLAUDE_CODE_TEXT_PROVIDER)
            && !provider.eq_ignore_ascii_case("claude-code-text")
        {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode text-only export requires provider=claude_code_text",
                json!({
                    "provider": provider,
                    "allowed_provider": CLAUDE_CODE_TEXT_PROVIDER,
                }),
            ));
            return None;
        }

        if request.slot_id.is_some() {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode text-only export does not accept external slot_id",
                json!({
                    "rule": "provider-box generates a private ephemeral PTY slot per request and hides it from callers",
                }),
            ));
            return None;
        }

        if request.dangerously_bypass_approvals_and_sandbox {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode text-only export does not use dangerously-skip-permissions",
                json!({
                    "provider": CLAUDE_CODE_TEXT_PROVIDER,
                    "rule": "text-only lanes run without bypass and with --tools '' plus durable JSONL violation checks",
                }),
            ));
            return None;
        }

        if !(request.no_tools && request.no_mcp && request.no_shell && request.no_file_access) {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode text-only export requires no_tools/no_mcp/no_shell/no_file_access guards",
                json!({
                    "no_tools": request.no_tools,
                    "no_mcp": request.no_mcp,
                    "no_shell": request.no_shell,
                    "no_file_access": request.no_file_access,
                }),
            ));
            return None;
        }

        if !request
            .prompt
            .as_ref()
            .is_some_and(|prompt| !prompt.trim().is_empty())
        {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode text-only export requires a non-empty prompt",
                json!({
                    "provider": CLAUDE_CODE_TEXT_PROVIDER,
                }),
            ));
            return None;
        }

        if !request.attachments.is_empty() {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode text-only export does not accept attachments",
                json!({
                    "attachments": request.attachments.len(),
                }),
            ));
            return None;
        }

        let Some(model) = request
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode text-only export requires one of the explicit exported model ids",
                json!({
                    "allowed_model_ids": Self::text_source_defs()
                        .iter()
                        .map(|def| def.model_id)
                        .collect::<Vec<_>>(),
                    "default_model_exported": false,
                }),
            ));
            return None;
        };

        let requested_profile = request
            .model_profile
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let Some(def) = Self::text_source_defs().iter().copied().find(|def| {
            claude_code_text_model_ref_matches(model, *def)
                && requested_profile
                    .map(|profile| profile.eq_ignore_ascii_case(def.effort))
                    .unwrap_or(true)
        }) else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Requested ClaudeCode text-only model is not exported",
                json!({
                    "requested_model": model,
                    "requested_model_profile": requested_profile,
                    "allowed_sources": Self::text_source_defs()
                        .iter()
                        .map(|def| json!({
                            "model_id": def.model_id,
                            "provider_model_id": def.provider_model_id,
                            "model_profile": def.effort,
                        }))
                        .collect::<Vec<_>>(),
                    "default_model_exported": false,
                }),
            ));
            return None;
        };

        Some(def)
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
            ..Default::default()
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

    async fn logout_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) {
        let mut observation = self.observe(slot_id).await;
        if is_claude_code_logout_success(&observation) {
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            result.status = ProviderBoxStatus::Completed;
            result.final_text =
                Some("ClaudeCode slot is already logged out or in first-run setup".to_string());
            result.durable_source = Some("claude_code_logout_state".to_string());
            return;
        }

        if !is_ready_for_claude_code_text(&observation) {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(8),
                    Some("wait for ClaudeCode prompt idle before /logout".to_string()),
                    is_ready_for_claude_code_text,
                )
                .await;
        }
        if !is_ready_for_claude_code_text(&observation) {
            result.status = ProviderBoxStatus::Blocked;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "ClaudeCode /logout requires an idle composer or an already logged-out startup/auth screen",
                json!({
                    "slot_id": slot_id,
                    "reason": observation.snapshot.reason,
                    "state": observation.snapshot.state,
                    "blocked_kind": observation.snapshot.blocked_kind,
                }),
            ));
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            return;
        }
        if let Some(text) = claude_code_composer_text(&observation) {
            if !text.trim().is_empty() {
                result.status = ProviderBoxStatus::Blocked;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                    "ClaudeCode composer is not empty; refusing to append /logout command",
                    json!({
                        "slot_id": slot_id,
                        "composer_text_preview": text.chars().take(120).collect::<String>(),
                        "safe_alternative": "clear the composer before logging out",
                    }),
                ));
                let status = self.pty.get_status(slot_id).await;
                result.slot_status =
                    Some(slot_status_value(slot_id, status.as_ref(), &observation));
                return;
            }
        }

        let command = "/logout";
        let _ = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::text(command.to_string()),
                command,
                Some("type ClaudeCode /logout command".to_string()),
            )
            .await;
        observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("execute ClaudeCode /logout command".to_string()),
            )
            .await;

        if !is_claude_code_logout_success(&observation) {
            let timeout = Duration::from_secs(request.timeout_secs.unwrap_or(45).clamp(5, 180));
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    timeout,
                    Some(
                        "wait for ClaudeCode /logout to reach auth or startup setup screen"
                            .to_string(),
                    ),
                    is_claude_code_logout_success,
                )
                .await;
        }

        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
        if is_claude_code_logout_success(&observation) {
            result.status = ProviderBoxStatus::Completed;
            result.final_text =
                Some("ClaudeCode /logout reached auth or startup setup screen".to_string());
            result.durable_source = Some("claude_code_logout_state".to_string());
        } else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "ClaudeCode /logout did not verify an auth/startup screen",
                json!({
                    "slot_id": slot_id,
                    "reason": observation.snapshot.reason,
                    "state": observation.snapshot.state,
                    "blocked_kind": observation.snapshot.blocked_kind,
                    "success_condition": "blocked_kind is auth_missing or startup_config; ordinary composer idle is not success",
                }),
            ));
        }
    }

    async fn ensure_text_only_runtime(
        &self,
        result: &mut ProviderBoxResult,
    ) -> Option<(PathBuf, PathBuf)> {
        let runtime_dir = claude_code_text_runtime_dir();
        if let Err(err) = tokio::fs::create_dir_all(&runtime_dir).await {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "ClaudeCode text-only runtime directory could not be created",
                json!({
                    "runtime_dir": runtime_dir.display().to_string(),
                    "error": err.to_string(),
                }),
            ));
            return None;
        }

        let readme = runtime_dir.join("README.missiond-claude-code-text-only.txt");
        let _ = tokio::fs::write(
            &readme,
            "MissionD provider-box ClaudeCode text-only runtime.\nThis directory is fixed and pre-trusted; each request uses an independent ClaudeCode --session-id.\n",
        )
        .await;

        let mcp_config = runtime_dir.join(CLAUDE_CODE_TEXT_EMPTY_MCP_CONFIG);
        if let Err(err) = tokio::fs::write(&mcp_config, r#"{"mcpServers":{}}"#).await {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "ClaudeCode text-only empty MCP config could not be written",
                json!({
                    "mcp_config": mcp_config.display().to_string(),
                    "error": err.to_string(),
                }),
            ));
            return None;
        }
        Some((runtime_dir, mcp_config))
    }

    async fn accept_workspace_trust_locked(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        mut observation: ClaudeCodeObservation,
    ) -> Option<ClaudeCodeObservation> {
        let selected = selected_claude_code_workspace_trust_option(&observation);
        match selected.as_deref() {
            Some("yes") => {}
            Some("no") => {
                observation = self
                    .write_step(
                        result,
                        slot_id,
                        PtyStepAction::key("up"),
                        "\x1b[A",
                        Some("move ClaudeCode workspace trust selection to Yes".to_string()),
                    )
                    .await;
                if selected_claude_code_workspace_trust_option(&observation).as_deref()
                    != Some("yes")
                {
                    result.status = ProviderBoxStatus::Blocked;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                        "ClaudeCode workspace trust prompt could not be moved to Yes",
                        json!({
                            "slot_id": slot_id,
                            "reason": observation.snapshot.reason,
                        }),
                    ));
                    return None;
                }
            }
            _ => {
                result.status = ProviderBoxStatus::Blocked;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                    "ClaudeCode workspace trust prompt selection was not recognizable",
                    json!({
                        "slot_id": slot_id,
                        "rule": "provider-box only confirms trust after observing the selected option",
                    }),
                ));
                return None;
            }
        }

        observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("confirm ClaudeCode workspace trust selection".to_string()),
            )
            .await;
        if !is_ready_for_claude_code_text(&observation) {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(12),
                    Some(
                        "wait for ClaudeCode composer after workspace trust confirmation"
                            .to_string(),
                    ),
                    is_ready_for_claude_code_text,
                )
                .await;
        }
        Some(observation)
    }

    async fn ensure_ready_for_text_only_prompt(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        timeout: Duration,
    ) -> bool {
        let started = Instant::now();
        loop {
            let mut observation = self.observe(slot_id).await;
            if is_claude_code_workspace_trust_prompt(&observation) {
                observation = match self
                    .accept_workspace_trust_locked(result, slot_id, observation)
                    .await
                {
                    Some(observation) => observation,
                    None => return false,
                };
            }

            if is_ready_for_claude_code_text(&observation) {
                return true;
            }

            if observation.snapshot.state == PtyCanonicalState::Blocked {
                result.status = ProviderBoxStatus::Blocked;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "ClaudeCode text-only slot is blocked during startup",
                    json!({
                        "slot_id": slot_id,
                        "reason": observation.snapshot.reason,
                        "blocked_kind": observation.snapshot.blocked_kind,
                        "rule": "auth and first-run setup must be handled before router-facing text-only export"
                    }),
                ));
                let status = self.pty.get_status(slot_id).await;
                result.slot_status =
                    Some(slot_status_value(slot_id, status.as_ref(), &observation));
                return false;
            }

            if started.elapsed() >= timeout {
                result.status = ProviderBoxStatus::Blocked;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "ClaudeCode text-only slot did not reach an idle composer before timeout",
                    json!({
                        "slot_id": slot_id,
                        "reason": observation.snapshot.reason,
                        "state": observation.snapshot.state,
                        "blocked_kind": observation.snapshot.blocked_kind,
                    }),
                ));
                let status = self.pty.get_status(slot_id).await;
                result.slot_status =
                    Some(slot_status_value(slot_id, status.as_ref(), &observation));
                return false;
            }

            tokio::time::sleep(Duration::from_millis(350)).await;
        }
    }

    async fn submit_text_only_prompt(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        prompt: &str,
    ) -> bool {
        let mut action = PtyStepAction::text("<claude-code text-only prompt>");
        action.redacted = true;
        let _ = self
            .write_step(
                result,
                slot_id,
                action,
                prompt,
                Some("write ClaudeCode text-only prompt into composer".to_string()),
            )
            .await;
        let _ = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("submit ClaudeCode text-only prompt".to_string()),
            )
            .await;
        !result
            .step_records
            .last()
            .is_some_and(|step| step.verification_status == PtyStepVerificationStatus::Failed)
    }

    async fn monitor_text_only_turn(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        session_id: &str,
        cursor: ClaudeCodeJsonlCursor,
        source: ClaudeCodeTextSourceDef,
        queue_key: &str,
        queue_wait_ms: u64,
    ) -> bool {
        let timeout_secs = request.timeout_secs.unwrap_or(180).clamp(10, 7_200);
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let mut idle_seen_at: Option<Instant> = None;

        loop {
            let analysis =
                analyze_claude_code_jsonl_after_cursor(&self.claude_home, session_id, &cursor);
            if let Some(violation) = analysis.violation {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_TEXT_ONLY_VIOLATION,
                    "ClaudeCode text-only turn attempted a disallowed provider action",
                    json!({
                        "provider": CLAUDE_CODE_TEXT_PROVIDER,
                        "model_id": source.model_id,
                        "provider_model_id": source.provider_model_id,
                        "event": violation,
                        "line_count": analysis.line_count,
                        "rule": "provider-box returns no final after tool/MCP/shell/file/search evidence appears in durable ClaudeCode JSONL"
                    }),
                ));
                return false;
            }

            if let Some(final_text) = analysis
                .final_text
                .as_ref()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
            {
                let durable_source = analysis
                    .jsonl_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "claude_code_session_jsonl".to_string());
                result.status = ProviderBoxStatus::Completed;
                result.provider = Some(CLAUDE_CODE_TEXT_PROVIDER.to_string());
                result.model = Some(source.model_id.to_string());
                result.model_profile = Some(source.effort.to_string());
                result.provider_conversation_id = Some(session_id.to_string());
                result.provider_session_identity = Some(ProviderSessionIdentity::resolved(
                    Some(CLAUDE_CODE_TEXT_PROVIDER.to_string()),
                    CliEngine::ClaudeCode,
                    Some(slot_id.to_string()),
                    session_id.to_string(),
                    "claude_code_session_jsonl",
                    Some(durable_source.clone()),
                    Some(claude_code_text_runtime_dir().display().to_string()),
                    "durable_final",
                ));
                result.durable_source = Some(durable_source.clone());
                result.slot_status = Some(json!({
                    "kind": "claude_code_text_only",
                    "provider": CLAUDE_CODE_TEXT_PROVIDER,
                    "private_slot_id": slot_id,
                    "session_id": session_id,
                    "model_id": source.model_id,
                    "provider_model_id": source.provider_model_id,
                    "model_profile": source.effort,
                    "durable_jsonl": durable_source,
                    "line_count": analysis.line_count,
                    "queue": {
                        "owner": "provider-box",
                        "key": queue_key,
                        "max_concurrent": CLAUDE_CODE_TEXT_MAX_CONCURRENT,
                        "wait_ms": queue_wait_ms,
                        "policy": "per_logical_claude_code_text_source"
                    }
                }));
                result.final_text = Some(final_text.to_string());
                return true;
            }

            let observation = self.observe(slot_id).await;
            if observation.snapshot.state == PtyCanonicalState::Blocked {
                result.status = ProviderBoxStatus::Blocked;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "ClaudeCode text-only turn entered a blocked provider surface",
                    json!({
                        "slot_id": slot_id,
                        "reason": observation.snapshot.reason,
                        "blocked_kind": observation.snapshot.blocked_kind,
                    }),
                ));
                let status = self.pty.get_status(slot_id).await;
                result.slot_status =
                    Some(slot_status_value(slot_id, status.as_ref(), &observation));
                return false;
            }

            if matches!(
                observation.snapshot.state,
                PtyCanonicalState::Idle | PtyCanonicalState::Complete
            ) {
                if let Some(seen_at) = idle_seen_at {
                    if seen_at.elapsed() >= Duration::from_secs(4) {
                        result.status = ProviderBoxStatus::Failed;
                        result.add_diagnostic(ProviderBoxDiagnostic::error(
                            DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                            "ClaudeCode returned to input but no durable assistant end_turn final was found",
                            json!({
                                "provider": CLAUDE_CODE_TEXT_PROVIDER,
                                "model_id": source.model_id,
                                "provider_model_id": source.provider_model_id,
                                "session_id": session_id,
                                "line_count": analysis.line_count,
                                "rule": "PTY screen text is diagnostic only; no fallback final was synthesized"
                            }),
                        ));
                        return false;
                    }
                } else {
                    idle_seen_at = Some(Instant::now());
                }
            } else {
                idle_seen_at = None;
            }

            if Instant::now() >= deadline {
                let _ = self.pty.write(slot_id, "\x1b").await;
                tokio::time::sleep(Duration::from_millis(500)).await;
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                    "ClaudeCode text-only turn timed out before durable final appeared",
                    json!({
                        "provider": CLAUDE_CODE_TEXT_PROVIDER,
                        "model_id": source.model_id,
                        "provider_model_id": source.provider_model_id,
                        "session_id": session_id,
                        "timeout_secs": timeout_secs,
                    }),
                ));
                return false;
            }

            tokio::time::sleep(Duration::from_millis(750)).await;
        }
    }

    async fn run_text_only_source(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        source: ClaudeCodeTextSourceDef,
    ) -> bool {
        let queue_key = format!("{}:{}", CLAUDE_CODE_TEXT_PROVIDER, source.model_id);
        let queue_semaphore = self.text_lane_semaphore(&queue_key).await;
        let queued_at = Instant::now();
        let Ok(_queue_guard) = queue_semaphore.acquire_owned().await else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "ClaudeCode text-only queue is unavailable",
                json!({
                    "provider": CLAUDE_CODE_TEXT_PROVIDER,
                    "queue": {
                        "key": queue_key,
                        "max_concurrent": CLAUDE_CODE_TEXT_MAX_CONCURRENT,
                    },
                }),
            ));
            return false;
        };
        let queue_wait_ms = u64::try_from(queued_at.elapsed().as_millis()).unwrap_or(u64::MAX);

        let Some((runtime_dir, mcp_config)) = self.ensure_text_only_runtime(result).await else {
            return false;
        };

        let session_id = uuid::Uuid::new_v4().to_string();
        let slot_id = format!(
            "slot-claude-code-text-{}-{}",
            source
                .model_id
                .strip_prefix("claude-code-")
                .unwrap_or(source.model_id),
            &session_id[..8]
        );
        let slot = PTYSlot {
            id: slot_id.clone(),
            role: "provider-box-claude-code-text-only".to_string(),
            cwd: Some(runtime_dir.clone()),
            engine: CliEngine::ClaudeCode,
        };
        self.pty.init_slot(&slot).await;

        let mut extra_env = HashMap::new();
        extra_env.insert(
            "MISSIOND_PROVIDER_BOX_TEXT_ONLY".to_string(),
            "1".to_string(),
        );
        extra_env.insert(
            "MISSIOND_PROVIDER_BOX_TEXT_PROVIDER".to_string(),
            CLAUDE_CODE_TEXT_PROVIDER.to_string(),
        );

        let options = PTYSpawnOptions {
            auto_restart: false,
            wait_for_idle: false,
            timeout_secs: Some(90),
            mcp_config: Some(mcp_config),
            dangerously_skip_permissions: false,
            model: Some(source.provider_model_id.to_string()),
            reasoning_effort: Some(source.effort.to_string()),
            search_enabled: false,
            sandbox: None,
            approval_policy: None,
            tool_policy_path: None,
            claude_code_tools: Some(String::new()),
            claude_code_strict_mcp_config: true,
            claude_code_disable_slash_commands: true,
            provider_session_id: Some(session_id.clone()),
            extra_env,
            initial_prompt: None,
            command_override: None,
        };

        if let Err(err) = self.pty.spawn(&slot, options).await {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "ClaudeCode text-only PTY could not be spawned",
                json!({
                    "provider": CLAUDE_CODE_TEXT_PROVIDER,
                    "model_id": source.model_id,
                    "provider_model_id": source.provider_model_id,
                    "error": err.to_string(),
                }),
            ));
            let _ = self.pty.kill(&slot_id).await;
            return false;
        }

        let ok = self
            .run_spawned_text_only_source(
                request,
                result,
                source,
                &slot_id,
                &session_id,
                &queue_key,
                queue_wait_ms,
            )
            .await;
        let _ = self.pty.kill(&slot_id).await;
        ok
    }

    async fn run_spawned_text_only_source(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        source: ClaudeCodeTextSourceDef,
        slot_id: &str,
        session_id: &str,
        queue_key: &str,
        queue_wait_ms: u64,
    ) -> bool {
        if !self
            .ensure_ready_for_text_only_prompt(result, slot_id, Duration::from_secs(90))
            .await
        {
            return false;
        }

        let cursor = claude_code_jsonl_cursor_for_session(&self.claude_home, session_id);
        let prompt = claude_code_text_prompt(request.prompt.as_deref().unwrap_or_default());
        if !self.submit_text_only_prompt(result, slot_id, &prompt).await {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "ClaudeCode text-only prompt submission failed",
                json!({
                    "provider": CLAUDE_CODE_TEXT_PROVIDER,
                    "model_id": source.model_id,
                }),
            ));
            return false;
        }

        self.monitor_text_only_turn(
            request,
            result,
            slot_id,
            session_id,
            cursor,
            source,
            queue_key,
            queue_wait_ms,
        )
        .await
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
            pure_text_guard: true,
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
        if !claude_code_model_matches(&observation, target)
            && claude_code_staged_command_matches(&observation, &command)
        {
            observation = self
                .write_step(
                    &mut result,
                    &slot_id,
                    PtyStepAction::key("enter"),
                    "\r",
                    Some(format!(
                        "execute staged ClaudeCode {command} command after slash completion"
                    )),
                )
                .await;
        }
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

    async fn pure_text_single_turn(
        &self,
        request: &ProviderInteractionRequest,
    ) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let Some(source) = Self::request_text_source(request, &mut result) else {
            return result;
        };
        result.provider = Some(CLAUDE_CODE_TEXT_PROVIDER.to_string());
        result.model = Some(source.model_id.to_string());
        result.model_profile = Some(source.effort.to_string());
        self.run_text_only_source(request, &mut result, source)
            .await;
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
            ProviderControlAction::Logout => {
                if Self::request_allow_hidden_logout(request) {
                    self.logout_locked(request, &mut result, &slot_id).await;
                } else {
                    result.status = ProviderBoxStatus::Unsupported;
                    result.add_diagnostic(ProviderBoxDiagnostic::unsupported(
                        DIAG_PROVIDER_CONTROL_ACTION_UNSUPPORTED,
                        "ClaudeCode logout control is implemented but hidden because re-login is sensitive",
                        json!({
                            "slot_id": slot_id,
                            "control_action": "logout",
                            "exposed": false,
                            "safe_alternative": "operate /logout manually through the teaching PTY only when the human explicitly wants to log out",
                        }),
                    ));
                }
            }
            _ => {
                result.status = ProviderBoxStatus::Unsupported;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_CONTROL_ACTION_UNSUPPORTED,
                    "ClaudeCode control action has not been taught yet",
                    json!({
                        "slot_id": slot_id,
                        "action": action,
                        "supported_actions": ["set_permissions", "logout"],
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

fn is_claude_code_logout_success(observation: &ClaudeCodeObservation) -> bool {
    matches!(
        observation.snapshot.blocked_kind.as_deref(),
        Some("auth_missing" | "startup_config")
    ) || matches!(
        observation.snapshot.reason.as_str(),
        "provider:auth_missing" | "claude_code:first_run_theme_prompt"
    )
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

fn claude_code_staged_command_matches(observation: &ClaudeCodeObservation, command: &str) -> bool {
    claude_code_composer_text(observation).is_some_and(|text| text == command)
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
        "claude-opus-4-8" | "opus-4-8" | "opus4-8" | "claude-opus-48" | "opus-48" => {
            Some(ClaudeCodeModelTarget {
                command_id: "claude-opus-4-8",
                display_name: "Opus 4.8",
            })
        }
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

fn claude_code_text_model_ref_matches(model: &str, def: ClaudeCodeTextSourceDef) -> bool {
    let normalized = model.trim().to_ascii_lowercase().replace('_', "-");
    normalized == def.model_id
        || normalized == def.provider_model_id
        || normalized == def.display_name.to_ascii_lowercase().replace('_', "-")
}

fn claude_code_text_prompt(prompt: &str) -> String {
    format!(
        "请直接用纯文本回答，不要调用工具、命令、MCP、网页搜索或读写文件。\n\n{}",
        prompt.trim()
    )
}

fn claude_code_text_runtime_dir() -> PathBuf {
    if let Ok(root) = std::env::var("MISSIOND_RUNTIME_DIR") {
        let root = root.trim();
        if !root.is_empty() {
            return PathBuf::from(root).join("claude-code-text-only");
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(CLAUDE_CODE_TEXT_RUNTIME_DIR)
}

fn default_claude_home() -> PathBuf {
    if let Ok(root) = std::env::var("CLAUDE_CONFIG_DIR") {
        let root = root.trim();
        if !root.is_empty() {
            return PathBuf::from(root);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
}

fn is_claude_code_workspace_trust_prompt(observation: &ClaudeCodeObservation) -> bool {
    observation
        .text
        .contains("Do you trust the contents of this directory?")
        || observation
            .text
            .contains("Do you trust the contents of this project?")
}

fn selected_claude_code_workspace_trust_option(
    observation: &ClaudeCodeObservation,
) -> Option<String> {
    observation.lines.iter().find_map(|line| {
        let trimmed = line.trim_start();
        if !(trimmed.starts_with('❯') || trimmed.starts_with('>')) {
            return None;
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.contains("yes") || lower.contains("trust") || lower.contains("continue") {
            Some("yes".to_string())
        } else if lower.contains("no") || lower.contains("quit") || lower.contains("exit") {
            Some("no".to_string())
        } else {
            None
        }
    })
}

fn claude_code_jsonl_cursor_for_session(
    claude_home: &Path,
    session_id: &str,
) -> ClaudeCodeJsonlCursor {
    let path = find_claude_code_session_jsonl(claude_home, session_id);
    let offset = path
        .as_ref()
        .and_then(|path| fs::metadata(path).ok())
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    ClaudeCodeJsonlCursor { path, offset }
}

fn analyze_claude_code_jsonl_after_cursor(
    claude_home: &Path,
    session_id: &str,
    cursor: &ClaudeCodeJsonlCursor,
) -> ClaudeCodeJsonlAnalysis {
    let path = cursor
        .path
        .clone()
        .filter(|path| path.exists())
        .or_else(|| find_claude_code_session_jsonl(claude_home, session_id));
    let Some(path) = path else {
        return ClaudeCodeJsonlAnalysis::default();
    };

    let mut file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(_) => return ClaudeCodeJsonlAnalysis::default(),
    };
    let len = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    let offset = if cursor
        .path
        .as_ref()
        .is_some_and(|cursor_path| cursor_path == &path)
        && cursor.offset <= len
    {
        cursor.offset
    } else {
        0
    };
    if offset > 0 {
        let _ = file.seek(SeekFrom::Start(offset));
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return ClaudeCodeJsonlAnalysis::default();
    }

    let mut analysis = ClaudeCodeJsonlAnalysis {
        jsonl_path: Some(path),
        ..Default::default()
    };
    let text = String::from_utf8_lossy(&bytes);
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        analysis.line_count += 1;
        if analysis.violation.is_none() && claude_code_jsonl_event_is_text_only_violation(&event) {
            analysis.violation = Some(event.clone());
            continue;
        }
        if let Some(final_text) = claude_code_assistant_end_turn_text(&event) {
            if !final_text.trim().is_empty() {
                analysis.final_text = Some(final_text.trim().to_string());
            }
        }
    }
    analysis
}

fn find_claude_code_session_jsonl(claude_home: &Path, session_id: &str) -> Option<PathBuf> {
    let filename = format!("{session_id}.jsonl");
    let roots = [claude_home.join("projects"), claude_home.to_path_buf()];
    roots
        .iter()
        .filter(|root| root.exists())
        .filter_map(|root| find_named_jsonl(root, &filename, 0, 5))
        .max_by_key(|path| {
            fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .ok()
        })
}

fn find_named_jsonl(
    root: &Path,
    filename: &str,
    depth: usize,
    max_depth: usize,
) -> Option<PathBuf> {
    if depth > max_depth {
        return None;
    }
    let mut matches = Vec::new();
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|value| value.to_str()) == Some(filename) {
            matches.push(path);
            continue;
        }
        if path.is_dir() {
            if let Some(path) = find_named_jsonl(&path, filename, depth + 1, max_depth) {
                matches.push(path);
            }
        }
    }
    matches.into_iter().max_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    })
}

fn claude_code_jsonl_event_is_text_only_violation(event: &Value) -> bool {
    if event
        .pointer("/message/stop_reason")
        .and_then(Value::as_str)
        == Some("tool_use")
    {
        return true;
    }
    json_value_has_disallowed_claude_code_tool_evidence(event)
}

fn json_value_has_disallowed_claude_code_tool_evidence(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map.get("toolUseResult").is_some() {
                return true;
            }
            if map
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|value| {
                    matches!(
                        value,
                        "tool_use"
                            | "tool_result"
                            | "toolUseResult"
                            | "web_search"
                            | "web_search_result"
                            | "web_search_request"
                            | "mcp_tool_call"
                    )
                })
            {
                return true;
            }
            if map
                .get("web_search_requests")
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
            {
                return true;
            }
            map.iter().any(|(key, value)| {
                let key_lower = key.to_ascii_lowercase();
                (key_lower.contains("mcp") && json_value_has_signal(value))
                    || json_value_has_disallowed_claude_code_tool_evidence(value)
            })
        }
        Value::Array(items) => items
            .iter()
            .any(json_value_has_disallowed_claude_code_tool_evidence),
        _ => false,
    }
}

fn json_value_has_signal(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(_) => true,
        Value::String(value) => !value.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn claude_code_assistant_end_turn_text(event: &Value) -> Option<String> {
    let message = event.get("message")?;
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    if message.get("stop_reason").and_then(Value::as_str) != Some("end_turn") {
        return None;
    }
    claude_code_message_content_text(message.get("content")?)
}

fn claude_code_message_content_text(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| {
                    if part.get("type").and_then(Value::as_str) == Some("text") {
                        part.get("text").and_then(Value::as_str)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            Some(text).filter(|text| !text.trim().is_empty())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;

    use missiond_core::pty::recognize_screen;
    use missiond_core::types::CliEngine;
    use missiond_core::SessionState;

    use super::{
        analyze_claude_code_jsonl_after_cursor, claude_code_jsonl_cursor_for_session,
        claude_code_jsonl_event_is_text_only_violation, claude_code_permission_cycle_steps,
        claude_code_staged_command_matches, find_claude_code_session_jsonl,
        is_claude_code_logout_success, normalize_claude_code_model_target,
        normalize_claude_code_permission_mode, ClaudeCodeJsonlCursor, ClaudeCodeModelTarget,
        ClaudeCodeObservation, ClaudeCodePermissionMode,
    };

    fn observation(lines: &[&str]) -> ClaudeCodeObservation {
        let owned = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        ClaudeCodeObservation {
            lines: owned.clone(),
            text: owned.join("\n"),
            snapshot: recognize_screen(CliEngine::ClaudeCode, &owned, SessionState::Idle),
        }
    }

    #[test]
    fn normalizes_claude_code_opus_model_command_aliases() {
        let target = normalize_claude_code_model_target("/model claude-opus-4-8")
            .expect("opus 4.8 command target");
        assert_eq!(
            target,
            ClaudeCodeModelTarget {
                command_id: "claude-opus-4-8",
                display_name: "Opus 4.8"
            }
        );

        let target =
            normalize_claude_code_model_target("Opus 4.8").expect("opus 4.8 display target");
        assert_eq!(target.command_id, "claude-opus-4-8");
        assert_eq!(target.display_name, "Opus 4.8");

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
        assert!(normalize_claude_code_model_target("claude-opus-4-9").is_none());
    }

    #[test]
    fn recognizes_exact_staged_claude_code_model_command() {
        let obs = observation(&[
            " ▐▛███▜▌   Claude Code v2.1.160",
            "▝▜█████▛▘  Sonnet 4.6 with high effort · Claude Max",
            "  ▘▘ ▝▝    ~/Projects/missiond",
            "────────────────────────────────────────────────────────────────",
            "❯ /model claude-opus-4-8",
            "────────────────────────────────────────────────────────────────",
            "  ⏵⏵ auto mode on (shift+tab to cycle)",
        ]);
        assert!(claude_code_staged_command_matches(
            &obs,
            "/model claude-opus-4-8"
        ));
        assert!(!claude_code_staged_command_matches(
            &obs,
            "/model claude-opus-4-6"
        ));
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

    #[test]
    fn claude_code_logout_success_requires_auth_or_startup_screen() {
        let startup = observation(&[
            "Welcome to Claude Code v2.1.159",
            "Let's get started.",
            "Choose the text style that looks best with your terminal",
            "  1. Auto (match terminal)",
            "❯ 2. Dark mode ✔",
        ]);
        assert!(is_claude_code_logout_success(&startup));

        let auth_missing = observation(&[
            "Credentials file not found — Claude Code may require interactive login",
            "Please log in to continue.",
        ]);
        assert!(is_claude_code_logout_success(&auth_missing));

        let idle = observation(&[
            "Claude Code v2.1.159",
            "Sonnet 4.6 with high effort · Claude Max",
            "~/Projects/missiond",
            "❯",
            "⏵⏵ auto mode on (shift+tab to cycle)",
        ]);
        assert!(!is_claude_code_logout_success(&idle));
    }

    #[test]
    fn claude_code_jsonl_scanner_extracts_assistant_end_turn_after_cursor() {
        let temp = tempfile::tempdir().expect("tempdir");
        let session_id = "019e82f0-0000-7000-8000-000000000001";
        let project_dir = temp
            .path()
            .join("projects")
            .join("-Users-jinchen-Projects-missiond");
        fs::create_dir_all(&project_dir).expect("project dir");
        let jsonl = project_dir.join(format!("{session_id}.jsonl"));
        fs::write(
            &jsonl,
            concat!(
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"stop_reason\":\"end_turn\",\"content\":[{\"type\":\"text\",\"text\":\"old final\"}]}}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}}\n"
            ),
        )
        .expect("initial jsonl");
        let cursor = claude_code_jsonl_cursor_for_session(temp.path(), session_id);
        assert_eq!(cursor.path.as_deref(), Some(jsonl.as_path()));
        assert!(cursor.offset > 0);
        fs::OpenOptions::new()
            .append(true)
            .open(&jsonl)
            .expect("open append")
            .write_all(
                br#"{"type":"assistant","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"marker final"}]}}"#,
            )
            .expect("append final");
        fs::OpenOptions::new()
            .append(true)
            .open(&jsonl)
            .expect("open newline")
            .write_all(b"\n")
            .expect("append newline");

        let analysis = analyze_claude_code_jsonl_after_cursor(temp.path(), session_id, &cursor);

        assert_eq!(analysis.final_text.as_deref(), Some("marker final"));
        assert!(analysis.violation.is_none());
        assert_eq!(analysis.line_count, 1);
    }

    #[test]
    fn claude_code_jsonl_scanner_finds_session_file_without_cwd_encoding() {
        let temp = tempfile::tempdir().expect("tempdir");
        let session_id = "019e82f0-0000-7000-8000-000000000002";
        let nested = temp
            .path()
            .join("projects")
            .join("arbitrary")
            .join("encoded")
            .join("path");
        fs::create_dir_all(&nested).expect("nested");
        let jsonl = nested.join(format!("{session_id}.jsonl"));
        fs::write(&jsonl, "{}\n").expect("jsonl");

        assert_eq!(
            find_claude_code_session_jsonl(temp.path(), session_id).as_deref(),
            Some(jsonl.as_path())
        );
    }

    #[test]
    fn claude_code_jsonl_scanner_detects_tool_and_web_mcp_violations() {
        for event in [
            serde_json::json!({
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "stop_reason": "tool_use",
                    "content": [{"type": "tool_use", "name": "Read"}]
                }
            }),
            serde_json::json!({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": [{"type": "tool_result", "tool_use_id": "toolu_1"}]
                }
            }),
            serde_json::json!({
                "toolUseResult": {"stdout": "ok"}
            }),
            serde_json::json!({
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "stop_reason": "end_turn",
                    "content": [{"type": "text", "text": "done"}]
                },
                "web_search_requests": [{"query": "weather"}]
            }),
            serde_json::json!({
                "mcpServer": "missiond"
            }),
        ] {
            assert!(
                claude_code_jsonl_event_is_text_only_violation(&event),
                "expected violation for {event}"
            );
        }
    }

    #[test]
    fn claude_code_jsonl_scanner_allows_empty_mcp_metadata() {
        let event = serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "stop_reason": "end_turn",
                "content": [{"type": "text", "text": "plain final"}]
            },
            "mcpServers": [],
            "mcp": {}
        });

        assert!(!claude_code_jsonl_event_is_text_only_violation(&event));
    }

    #[test]
    fn claude_code_jsonl_scanner_reports_violation_before_returning_final() {
        let temp = tempfile::tempdir().expect("tempdir");
        let session_id = "019e82f0-0000-7000-8000-000000000003";
        let project_dir = temp.path().join("projects").join("missiond");
        fs::create_dir_all(&project_dir).expect("project dir");
        let jsonl = project_dir.join(format!("{session_id}.jsonl"));
        fs::write(
            &jsonl,
            concat!(
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"stop_reason\":\"tool_use\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\"}]}}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"stop_reason\":\"end_turn\",\"content\":[{\"type\":\"text\",\"text\":\"should not be trusted\"}]}}\n"
            ),
        )
        .expect("jsonl");

        let analysis = analyze_claude_code_jsonl_after_cursor(
            temp.path(),
            session_id,
            &ClaudeCodeJsonlCursor {
                path: None,
                offset: 0,
            },
        );

        assert!(analysis.violation.is_some());
        assert_eq!(
            analysis.final_text.as_deref(),
            Some("should not be trusted")
        );
    }
}
