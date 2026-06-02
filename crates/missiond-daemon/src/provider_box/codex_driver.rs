use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use missiond_core::db::traits::MissionStore;
use missiond_core::pty::{recognize_screen, recognize_styled_screen, PtyCanonicalState};
use missiond_core::types::{CliEngine, SharedProjectRegistry};
use missiond_core::{LearnedPermissions, PTYManager, PTYSlot, PTYSpawnOptions, SessionState};
use serde_json::{json, Value};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::{Mutex, RwLock, Semaphore};

use super::driver::{ProviderDriver, ProviderDriverCapabilities};
use super::types::{
    BoxCommand, ProviderBoxDiagnostic, ProviderBoxResult, ProviderBoxStatus, ProviderControlAction,
    ProviderInteractionRequest, ProviderModelUsage, ProviderSessionIdentity, ProviderUsageSnapshot,
    ProviderUsageStatus, PtyObservation, PtyStepAction, PtyStepRecord, PtyStepVerificationStatus,
    DIAG_PROVIDER_BOX_INVALID_REQUEST, DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
    DIAG_PROVIDER_CONTROL_ACTION_UNSUPPORTED, DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
    DIAG_PROVIDER_DURABLE_FINAL_MISSING, DIAG_PROVIDER_MCP_RECONNECT_UNSUPPORTED,
    DIAG_PROVIDER_MCP_STATUS_UNAVAILABLE, DIAG_PROVIDER_SESSION_ID_UNKNOWN,
    DIAG_PROVIDER_TEXT_ONLY_VIOLATION, DIAG_PROVIDER_TURN_TIMEOUT_CANCELLED,
    DIAG_PROVIDER_TURN_TIMEOUT_CANCEL_FAILED, DIAG_USAGE_UNKNOWN,
};

const DEFAULT_CODEX_SLOT: &str = "slot-codex-provider-box";
const OBSERVE_SETTLE_MS: u64 = 350;
const OBSERVE_STABLE_POLL_MS: u64 = 120;
const OBSERVE_STABLE_MAX_MS: u64 = 1_000;
const CODEX_EXEC_TEXT_PROVIDER: &str = "codex_exec_text";
const CODEX_RESEARCH_PROVIDER: &str = "codex_research";
const CODEX_IMAGE_PROVIDER: &str = "codex_image_generation";
const CODEX_EXEC_TEXT_MODEL: &str = "gpt-5.5";
const CODEX_EXEC_TEXT_DEFAULT_MAX_CONCURRENT: usize = 4;
const CODEX_EXEC_TEXT_XHIGH_MAX_CONCURRENT: usize = 2;
const CODEX_EXEC_TASK_MAX_CONCURRENT: usize = 1;
const CODEX_STARTUP_READY_WAIT_SECS: u64 = 60;
const CODEX_TRUST_READY_WAIT_SECS: u64 = 12;
const CODEX_MANUAL_TEXT_LIMIT: usize = 4096;
const CODEX_MANUAL_KEY_NAMES: &[&str] = &[
    "enter",
    "escape",
    "up",
    "down",
    "left",
    "right",
    "tab",
    "shift-tab",
    "shift+tab",
    "backtab",
    "backspace",
    "delete",
    "ctrl+c",
    "pageup",
    "pagedown",
    "home",
    "end",
];

#[derive(Clone)]
pub(crate) struct CodexProviderDriver {
    pty: Arc<PTYManager>,
    store: Arc<dyn MissionStore>,
    pty_session_uuids: Arc<RwLock<HashSet<String>>>,
    project_registry: SharedProjectRegistry,
    learned: Option<Arc<LearnedPermissions>>,
    slot_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    exec_lane_locks: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    codex_home: PathBuf,
}

#[derive(Debug, Clone)]
struct CodexObservation {
    lines: Vec<String>,
    text: String,
    snapshot: missiond_core::pty::PtyRecognitionSnapshot,
}

#[derive(Debug, Clone)]
struct CodexTurnFinal {
    session_id: String,
    rollout_path: String,
    final_text: String,
}

#[derive(Debug, Clone)]
struct CodexImageGenerationEvidence {
    session_id: String,
    rollout_path: String,
    image_paths: Vec<String>,
    revised_prompts: Vec<String>,
    final_text: Option<String>,
    image_event_count: usize,
}

#[derive(Debug, Clone)]
struct CodexImageUploadResult {
    artifact: Value,
    final_text: String,
}

#[derive(Debug, Clone)]
struct CodexExecJsonlAnalysis {
    event_count: usize,
    allowed_tool_event_count: usize,
    thread_id: Option<String>,
    final_text: Option<String>,
    violation: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexExecTaskKind {
    TextOnly,
    Research,
    ImageGeneration,
}

impl CodexExecTaskKind {
    fn provider(self) -> &'static str {
        match self {
            Self::TextOnly => CODEX_EXEC_TEXT_PROVIDER,
            Self::Research => CODEX_RESEARCH_PROVIDER,
            Self::ImageGeneration => CODEX_IMAGE_PROVIDER,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::TextOnly => "text-only",
            Self::Research => "research",
            Self::ImageGeneration => "image-generation",
        }
    }

    fn output_media_type(self) -> &'static str {
        match self {
            Self::TextOnly | Self::Research => "text/markdown",
            Self::ImageGeneration => "text/markdown+image",
        }
    }

    fn required_tool_family(self) -> Option<&'static str> {
        match self {
            Self::TextOnly => None,
            Self::Research => Some("web_search"),
            Self::ImageGeneration => Some("image_generation"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexStartupOutcome {
    Ready,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexTrustSelection {
    Continue,
    Quit,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexPermissionMode {
    Default,
    AutoReview,
    FullAccess,
}

impl CodexPermissionMode {
    fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::AutoReview => "Auto-review",
            Self::FullAccess => "Full Access",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexFastMode {
    Enabled,
    Disabled,
}

impl CodexFastMode {
    fn label(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
        }
    }

    fn service_tier(self) -> &'static str {
        match self {
            Self::Enabled => "priority",
            Self::Disabled => "default",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexManualPtyStep {
    action_type: String,
    key: Option<String>,
    text: Option<String>,
    expected_change: Option<String>,
    redacted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexUsageLines {
    five_hour: String,
    weekly: String,
}

impl CodexProviderDriver {
    #[allow(clippy::too_many_arguments)]
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
            exec_lane_locks: Arc::new(Mutex::new(HashMap::new())),
            codex_home: default_codex_home(),
        }
    }

    async fn slot_lock(&self, slot_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.slot_locks.lock().await;
        locks
            .entry(slot_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    async fn exec_lane_semaphore(&self, queue_key: &str, max_concurrent: usize) -> Arc<Semaphore> {
        let mut locks = self.exec_lane_locks.lock().await;
        locks
            .entry(queue_key.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(max_concurrent)))
            .clone()
    }

    fn request_slot_id(request: &ProviderInteractionRequest) -> String {
        request
            .slot_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_CODEX_SLOT.to_string())
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
        request.dangerously_bypass_approvals_and_sandbox
            || bool_any(
                request.tool_policy.as_ref(),
                &[
                    "dangerously_bypass_approvals_and_sandbox",
                    "dangerously_skip_permissions",
                    "dangerously_bypass",
                    "bypass_approvals_and_sandbox",
                    "bypass_mode",
                    "bypass",
                ],
            )
            || bool_any(
                request.desired_worker.as_ref(),
                &[
                    "dangerously_bypass_approvals_and_sandbox",
                    "dangerously_skip_permissions",
                    "dangerously_bypass",
                    "bypass_approvals_and_sandbox",
                    "bypass_mode",
                    "bypass",
                ],
            )
    }

    fn request_search_enabled(request: &ProviderInteractionRequest) -> bool {
        request
            .tool_policy
            .as_ref()
            .and_then(|policy| policy.get("search_enabled").and_then(Value::as_bool))
            .unwrap_or(true)
    }

    fn request_sandbox(request: &ProviderInteractionRequest) -> Option<String> {
        request
            .tool_policy
            .as_ref()
            .and_then(|policy| policy.get("sandbox").and_then(Value::as_str))
            .map(str::to_string)
            .or_else(|| Some("read-only".to_string()))
    }

    fn request_approval_policy(request: &ProviderInteractionRequest) -> Option<String> {
        Some(
            request
                .tool_policy
                .as_ref()
                .and_then(|policy| policy.get("approval_policy").and_then(Value::as_str))
                .unwrap_or("never")
                .to_string(),
        )
    }

    fn request_submit_input(request: &ProviderInteractionRequest) -> bool {
        request
            .desired_worker
            .as_ref()
            .and_then(|worker| {
                worker
                    .get("submit")
                    .or_else(|| worker.get("enter"))
                    .or_else(|| worker.get("append_enter"))
                    .and_then(Value::as_bool)
            })
            .unwrap_or(false)
    }

    fn request_mcp_server(request: &ProviderInteractionRequest) -> String {
        request
            .desired_worker
            .as_ref()
            .and_then(|worker| {
                worker
                    .get("mcp_server")
                    .or_else(|| worker.get("server"))
                    .and_then(Value::as_str)
            })
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("missiond")
            .to_string()
    }

    fn request_permission_mode(
        request: &ProviderInteractionRequest,
    ) -> Result<CodexPermissionMode, ProviderBoxDiagnostic> {
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
                    "Codex permissions control action requires permission_mode",
                    json!({
                        "slot_id": request.slot_id,
                        "allowed_permission_modes": ["Default", "Auto-review", "Full Access"],
                    }),
                )
            })?;
        normalize_codex_permission_mode(raw).ok_or_else(|| {
            ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Codex permissions control action uses an unsupported permission_mode",
                json!({
                    "slot_id": request.slot_id,
                    "permission_mode": raw,
                    "allowed_permission_modes": ["Default", "Auto-review", "Full Access"],
                }),
            )
        })
    }

    fn request_fast_mode(
        request: &ProviderInteractionRequest,
    ) -> Result<CodexFastMode, ProviderBoxDiagnostic> {
        let raw = request
            .desired_worker
            .as_ref()
            .and_then(|worker| {
                worker
                    .get("fast_mode")
                    .or_else(|| worker.get("target_fast_mode"))
                    .or_else(|| worker.get("service_tier"))
                    .or_else(|| worker.get("fast_enabled"))
                    .or_else(|| worker.get("enabled"))
                    .or_else(|| worker.get("fast"))
                    .or_else(|| worker.get("mode"))
            })
            .ok_or_else(|| {
                ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "Codex fast-mode control action requires fast_mode or enabled",
                    json!({
                        "slot_id": request.slot_id,
                        "allowed_fast_modes": ["enabled", "disabled"],
                    }),
                )
            })?;
        normalize_codex_fast_mode(raw).ok_or_else(|| {
            ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Codex fast-mode control action uses an unsupported fast_mode",
                json!({
                    "slot_id": request.slot_id,
                    "fast_mode": raw,
                    "allowed_fast_modes": ["enabled", "disabled"],
                }),
            )
        })
    }

    fn request_manual_pty_step(
        request: &ProviderInteractionRequest,
    ) -> Result<CodexManualPtyStep, ProviderBoxDiagnostic> {
        let step = request
            .desired_worker
            .as_ref()
            .and_then(|worker| worker.get("pty_step").or(Some(worker)))
            .ok_or_else(|| {
                ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "Codex PTY step requires desired_worker.pty_step",
                    json!({
                        "slot_id": request.slot_id,
                        "command": request.command,
                    }),
                )
            })?;
        let action_type = step
            .get("action_type")
            .or_else(|| step.get("type"))
            .and_then(Value::as_str)
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .or_else(|| {
                if step.get("key").is_some() {
                    Some("key".to_string())
                } else if step.get("text").is_some() {
                    Some("text".to_string())
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "Codex PTY step requires action_type=key or action_type=text",
                    json!({
                        "slot_id": request.slot_id,
                        "allowed_action_types": ["key", "text"],
                    }),
                )
            })?;
        let redacted = step
            .get("redacted")
            .and_then(Value::as_bool)
            .unwrap_or(action_type == "text" || action_type == "paste");
        let expected_change = step
            .get("expected_change")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        match action_type.as_str() {
            "key" => {
                let key = step
                    .get("key")
                    .or_else(|| step.get("human_input"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .ok_or_else(|| {
                        ProviderBoxDiagnostic::error(
                            DIAG_PROVIDER_BOX_INVALID_REQUEST,
                            "Codex key PTY step requires a key name",
                            json!({
                                "slot_id": request.slot_id,
                                "allowed_keys": CODEX_MANUAL_KEY_NAMES,
                            }),
                        )
                    })?;
                if codex_manual_key_bytes(&key).is_none() {
                    return Err(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_BOX_INVALID_REQUEST,
                        "Codex key PTY step uses an unsupported key",
                        json!({
                            "slot_id": request.slot_id,
                            "key": key,
                            "allowed_keys": CODEX_MANUAL_KEY_NAMES,
                        }),
                    ));
                }
                Ok(CodexManualPtyStep {
                    action_type,
                    key: Some(key),
                    text: None,
                    expected_change,
                    redacted: false,
                })
            }
            "text" | "paste" => {
                let text = step
                    .get("text")
                    .or_else(|| step.get("human_input"))
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .ok_or_else(|| {
                        ProviderBoxDiagnostic::error(
                            DIAG_PROVIDER_BOX_INVALID_REQUEST,
                            "Codex text PTY step requires text",
                            json!({
                                "slot_id": request.slot_id,
                            }),
                        )
                    })?;
                validate_codex_manual_text_step(request.slot_id.as_deref(), &text)?;
                Ok(CodexManualPtyStep {
                    action_type: "text".to_string(),
                    key: None,
                    text: Some(text),
                    expected_change,
                    redacted,
                })
            }
            _ => Err(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Codex PTY step action_type is unsupported",
                json!({
                    "slot_id": request.slot_id,
                    "action_type": action_type,
                    "allowed_action_types": ["key", "text"],
                }),
            )),
        }
    }

    async fn existing_slot_matches_request(
        &self,
        slot_id: &str,
        request: &ProviderInteractionRequest,
    ) -> bool {
        let Some(options) = self.pty.get_spawn_options(slot_id).await else {
            return true;
        };
        if let Some(model) = request.model.as_deref() {
            if options.model.as_deref() != Some(model) {
                return false;
            }
            if options.reasoning_effort.as_deref() != request.model_profile.as_deref() {
                return false;
            }
        } else if let Some(profile) = request.model_profile.as_deref() {
            if options.reasoning_effort.as_deref() != Some(profile) {
                return false;
            }
        }
        if request_mentions_bypass_policy(request)
            && options.dangerously_skip_permissions != Self::request_dangerous_bypass(request)
        {
            return false;
        }
        true
    }

    async fn ensure_codex_binary(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> bool {
        self.locate_codex_binary(request, result).await.is_some()
    }

    async fn locate_codex_binary(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> Option<PathBuf> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let output = Command::new(shell)
            .arg("-l")
            .arg("-i")
            .arg("-c")
            .arg("command -v codex")
            .output()
            .await;
        match output {
            Ok(output) if output.status.success() => {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if path.is_empty() {
                    result.status = ProviderBoxStatus::Failed;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                        "required CLI codex resolved to an empty path",
                        json!({
                            "engine": request.engine.to_string(),
                            "rule": "fast-fail; no fallback provider path is allowed"
                        }),
                    ));
                    None
                } else {
                    Some(PathBuf::from(path))
                }
            }
            Ok(output) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "required CLI codex is not available in the MissionD worker login shell",
                    json!({
                        "engine": request.engine.to_string(),
                        "slot_id": request.slot_id,
                        "stdout": String::from_utf8_lossy(&output.stdout).trim(),
                        "stderr": String::from_utf8_lossy(&output.stderr).trim(),
                        "rule": "fast-fail; no fallback provider path is allowed"
                    }),
                ));
                None
            }
            Err(err) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "could not preflight required CLI codex",
                    json!({
                        "engine": request.engine.to_string(),
                        "slot_id": request.slot_id,
                        "error": err.to_string(),
                        "rule": "fast-fail; no fallback provider path is allowed"
                    }),
                ));
                None
            }
        }
    }

    async fn ensure_slot(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> Option<String> {
        let slot_id = Self::request_slot_id(request);
        if let Some(status) = self.pty.get_status(&slot_id).await {
            if status.engine != CliEngine::Codex {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Requested provider-box slot is not a Codex slot",
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
                if self.existing_slot_matches_request(&slot_id, request).await {
                    let observation = self.observe(&slot_id).await;
                    if should_resolve_codex_startup_surface(&observation) {
                        let lock = self.slot_lock(&slot_id).await;
                        let _guard = lock.lock().await;
                        match self.ensure_startup_ready_locked(result, &slot_id).await {
                            CodexStartupOutcome::Ready => {}
                            CodexStartupOutcome::Failed => return None,
                        }
                    }
                    return Some(slot_id);
                }
                let _ = self.pty.kill(&slot_id).await;
            }
        }

        if !self.ensure_codex_binary(request, result).await {
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
            role: "provider-box-codex".to_string(),
            cwd: Some(cwd),
            engine: CliEngine::Codex,
        };
        self.pty.init_slot(&slot).await;

        let dangerous_bypass = Self::request_dangerous_bypass(request);
        let options = PTYSpawnOptions {
            auto_restart: true,
            wait_for_idle: false,
            timeout_secs: Some(90),
            mcp_config: None,
            dangerously_skip_permissions: dangerous_bypass,
            model: request.model.clone(),
            reasoning_effort: request.model_profile.clone(),
            search_enabled: Self::request_search_enabled(request),
            sandbox: if dangerous_bypass {
                None
            } else {
                Self::request_sandbox(request)
            },
            approval_policy: if dangerous_bypass {
                None
            } else {
                Self::request_approval_policy(request)
            },
            tool_policy_path: None,
            extra_env: HashMap::new(),
            initial_prompt: None,
            command_override: None,
            ..Default::default()
        };
        self.spawn_slot_with_bootstrap(slot, options, result).await
    }

    async fn spawn_slot_with_bootstrap(
        &self,
        slot: PTYSlot,
        options: PTYSpawnOptions,
        result: &mut ProviderBoxResult,
    ) -> Option<String> {
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
                let lock = self.slot_lock(&slot.id).await;
                let _guard = lock.lock().await;
                match self.ensure_startup_ready_locked(result, &slot.id).await {
                    CodexStartupOutcome::Ready => Some(slot.id),
                    CodexStartupOutcome::Failed => None,
                }
            }
            Err(err) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Codex PTY slot could not be spawned by provider-box",
                    json!({
                        "slot_id": slot.id,
                        "error": err.to_string(),
                        "rule": "fast-fail; no direct GenericCli fallback is allowed"
                    }),
                ));
                None
            }
        }
    }

    async fn attach_status_observation(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        expected_change: impl Into<Option<String>>,
    ) -> CodexObservation {
        let observation = self.observe(slot_id).await;
        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
        self.attach_session_identity(result, slot_id, status.as_ref(), &observation)
            .await;
        let pty_observation = Self::pty_observation(slot_id, &observation);
        result.record_step(PtyStepRecord::new(
            pty_observation.clone(),
            PtyStepAction::key("observe"),
            pty_observation,
            expected_change.into(),
            PtyStepVerificationStatus::Skipped,
        ));
        observation
    }

    async fn attach_session_identity(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        status: Option<&missiond_core::PTYAgentInfo>,
        observation: &CodexObservation,
    ) {
        let identity = self
            .resolve_session_identity(slot_id, status, observation)
            .await;
        if let Some(session_id) = identity.provider_session_id.clone() {
            result.provider_conversation_id = Some(session_id);
        }
        if result.durable_source.is_none() {
            result.durable_source = identity.durable_source.clone();
        }
        if let Some(slot_status) = result.slot_status.as_mut() {
            slot_status["provider_session_identity"] = json!(identity.clone());
        }
        result.provider_session_identity = Some(identity);
    }

    async fn resolve_session_identity(
        &self,
        slot_id: &str,
        status: Option<&missiond_core::PTYAgentInfo>,
        observation: &CodexObservation,
    ) -> ProviderSessionIdentity {
        let workspace = codex_workspace_candidates(observation)
            .into_iter()
            .next()
            .map(|path| path.display().to_string());
        if let Some(meta) =
            find_latest_codex_session_identity(&self.codex_home, observation, status).await
        {
            return ProviderSessionIdentity::resolved(
                Some("codex_cli".to_string()),
                CliEngine::Codex,
                Some(slot_id.to_string()),
                meta.session_id,
                meta.identity_source,
                Some(meta.rollout_path),
                meta.cwd,
                meta.confidence,
            );
        }
        ProviderSessionIdentity::unknown(
            Some("codex_cli".to_string()),
            CliEngine::Codex,
            Some(slot_id.to_string()),
            "codex_rollout_session_meta_probe",
            workspace,
            ProviderBoxDiagnostic::warning(
                DIAG_PROVIDER_SESSION_ID_UNKNOWN,
                "Codex session id is not visible yet for this slot",
                json!({
                    "slot_id": slot_id,
                    "sources_checked": [
                        "Codex rollout session_meta.payload.id",
                        "Codex rollout filename suffix"
                    ],
                    "rule": "slot_id is operational identity; provider_session_id must come from Codex durable session metadata"
                }),
            ),
        )
    }

    async fn observe(&self, slot_id: &str) -> CodexObservation {
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
            recognize_styled_screen(CliEngine::Codex, screen, state)
        } else {
            recognize_screen(CliEngine::Codex, &lines, state)
        };
        let text = lines.join("\n");
        CodexObservation {
            lines,
            text,
            snapshot,
        }
    }

    fn observations_equivalent(left: &CodexObservation, right: &CodexObservation) -> bool {
        left.text == right.text
            && left.snapshot.state == right.snapshot.state
            && left.snapshot.reason == right.snapshot.reason
            && left.snapshot.blocked_kind == right.snapshot.blocked_kind
    }

    fn observations_changed(before: &CodexObservation, after: &CodexObservation) -> bool {
        !Self::observations_equivalent(before, after)
    }

    async fn observe_after_action(&self, slot_id: &str) -> CodexObservation {
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

    fn pty_observation(slot_id: &str, observation: &CodexObservation) -> PtyObservation {
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
        expected_change: impl Into<Option<String>>,
    ) -> CodexObservation {
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
            expected_change.into(),
            status,
        );
        if let Err(err) = write_result {
            step.diagnostics.push(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "Codex PTY write failed",
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
    ) -> CodexObservation
    where
        F: FnMut(&CodexObservation) -> bool,
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
        expected_change: impl Into<Option<String>>,
        mut predicate: F,
    ) -> CodexObservation
    where
        F: FnMut(&CodexObservation) -> bool,
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
            PtyStepAction {
                action_type: "wait".to_string(),
                human_input: format!("wait {}s", timeout.as_secs()),
                redacted: false,
            },
            Self::pty_observation(slot_id, &after),
            expected_change.into(),
            status,
        ));
        after
    }

    async fn ensure_startup_ready_locked(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) -> CodexStartupOutcome {
        let mut observation = self
            .wait_step_until(
                result,
                slot_id,
                Duration::from_secs(CODEX_STARTUP_READY_WAIT_SECS),
                Some("wait for Codex startup surface".to_string()),
                |obs| is_ready_for_codex_text(obs) || is_codex_workspace_trust_prompt(obs),
            )
            .await;

        if is_codex_workspace_trust_prompt(&observation) {
            observation = match self
                .accept_workspace_trust_locked(result, slot_id, observation)
                .await
            {
                Some(observation) => observation,
                None => return CodexStartupOutcome::Failed,
            };
        }

        if is_ready_for_codex_text(&observation) {
            CodexStartupOutcome::Ready
        } else {
            result.status = ProviderBoxStatus::Blocked;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "Codex startup did not reach a ready composer",
                json!({
                    "slot_id": slot_id,
                    "state": observation.snapshot.state,
                    "reason": observation.snapshot.reason,
                    "blocked_kind": observation.snapshot.blocked_kind,
                }),
            ));
            CodexStartupOutcome::Failed
        }
    }

    async fn accept_workspace_trust_locked(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        mut observation: CodexObservation,
    ) -> Option<CodexObservation> {
        match selected_codex_workspace_trust_option(&observation) {
            CodexTrustSelection::Continue => {}
            CodexTrustSelection::Quit => {
                let mut selected_continue = false;
                for (key_name, bytes) in [("up", "\x1b[A"), ("down", "\x1b[B")] {
                    observation = self
                        .write_step(
                            result,
                            slot_id,
                            PtyStepAction::key(key_name),
                            bytes,
                            Some("move Codex workspace trust selection to Yes".to_string()),
                        )
                        .await;
                    if selected_codex_workspace_trust_option(&observation)
                        == CodexTrustSelection::Continue
                    {
                        selected_continue = true;
                        break;
                    }
                }
                if !selected_continue {
                    result.status = ProviderBoxStatus::Failed;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                        "Codex workspace trust prompt could not be moved to Yes",
                        json!({
                            "slot_id": slot_id,
                            "reason": observation.snapshot.reason,
                        }),
                    ));
                    return None;
                }
            }
            CodexTrustSelection::Unknown => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                    "Codex workspace trust prompt selection was not recognizable",
                    json!({
                        "slot_id": slot_id,
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
                Some("confirm Codex workspace trust selection".to_string()),
            )
            .await;
        if !is_ready_for_codex_text(&observation) {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(CODEX_TRUST_READY_WAIT_SECS),
                    Some("wait for Codex composer after workspace trust confirmation".to_string()),
                    is_ready_for_codex_text,
                )
                .await;
        }
        if is_ready_for_codex_text(&observation) {
            Some(observation)
        } else {
            result.status = ProviderBoxStatus::Blocked;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "Codex workspace trust confirmation did not reach the composer",
                json!({
                    "slot_id": slot_id,
                    "state": observation.snapshot.state,
                    "reason": observation.snapshot.reason,
                }),
            ));
            None
        }
    }

    async fn ensure_ready_for_prompt(&self, result: &mut ProviderBoxResult, slot_id: &str) -> bool {
        let started = Instant::now();
        loop {
            let mut observation = self.observe(slot_id).await;
            if is_codex_workspace_trust_prompt(&observation) {
                observation = match self
                    .accept_workspace_trust_locked(result, slot_id, observation)
                    .await
                {
                    Some(observation) => observation,
                    None => return false,
                };
            }
            if is_ready_for_codex_text(&observation) {
                return true;
            }
            if started.elapsed() > Duration::from_secs(8) {
                result.status = ProviderBoxStatus::Blocked;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Codex slot is not ready for provider-box prompt submission",
                    json!({
                        "slot_id": slot_id,
                        "state": observation.snapshot.state,
                        "reason": observation.snapshot.reason,
                        "blocked_kind": observation.snapshot.blocked_kind,
                    }),
                ));
                return false;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    async fn submit_prompt_step(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        prompt: &str,
    ) -> bool {
        let before = self.observe(slot_id).await;
        let send_result = self.pty.send_fire_and_forget(slot_id, prompt).await;
        let after = self.observe_after_action(slot_id).await;
        let mut action = PtyStepAction::text("<codex prompt paste + enter>");
        action.redacted = true;
        let status = if send_result.is_err() {
            PtyStepVerificationStatus::Failed
        } else if after.snapshot.state == PtyCanonicalState::Running
            || Self::observations_changed(&before, &after)
        {
            PtyStepVerificationStatus::Verified
        } else {
            PtyStepVerificationStatus::Ambiguous
        };
        let mut step = PtyStepRecord::new(
            Self::pty_observation(slot_id, &before),
            action,
            Self::pty_observation(slot_id, &after),
            Some("Codex accepts prompt and starts a provider turn".to_string()),
            status,
        );
        if let Err(err) = send_result {
            step.diagnostics.push(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "Codex provider-box prompt submission failed",
                json!({
                    "slot_id": slot_id,
                    "error": err.to_string(),
                }),
            ));
        }
        let ok = step.verification_status != PtyStepVerificationStatus::Failed;
        result.record_step(step);
        ok
    }

    async fn monitor_turn(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) -> ProviderBoxResult {
        let timeout_secs = request.timeout_secs.unwrap_or(180).clamp(10, 7_200);
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let mut idle_seen_at: Option<Instant> = None;

        loop {
            if let Some(final_turn) = self
                .extract_turn_from_rollouts(&request.correlation_id)
                .await
            {
                result.status = ProviderBoxStatus::Completed;
                result.provider_conversation_id = Some(final_turn.session_id.clone());
                result.provider_session_identity = Some(ProviderSessionIdentity::resolved(
                    request
                        .provider
                        .clone()
                        .or_else(|| Some("codex_cli".to_string())),
                    CliEngine::Codex,
                    request.slot_id.clone(),
                    final_turn.session_id.clone(),
                    "codex_correlated_rollout_jsonl",
                    Some(final_turn.rollout_path.clone()),
                    None,
                    "durable_final",
                ));
                result.durable_source = Some(final_turn.rollout_path);
                result.final_text = Some(final_turn.final_text);
                return result.clone();
            }

            let observation = self.observe(slot_id).await;
            if observation.snapshot.state == PtyCanonicalState::Blocked {
                let mut failed = ProviderBoxResult::base(request, ProviderBoxStatus::Blocked);
                failed.slot_id = Some(slot_id.to_string());
                failed.step_records = result.step_records.clone();
                failed.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Codex provider-box turn entered a blocked provider surface",
                    json!({
                        "slot_id": slot_id,
                        "reason": observation.snapshot.reason,
                        "blocked_kind": observation.snapshot.blocked_kind,
                    }),
                ));
                return failed;
            }

            if matches!(
                observation.snapshot.state,
                PtyCanonicalState::Idle | PtyCanonicalState::Complete
            ) {
                if let Some(seen_at) = idle_seen_at {
                    if seen_at.elapsed() >= Duration::from_secs(3) {
                        let mut failed =
                            ProviderBoxResult::base(request, ProviderBoxStatus::Failed);
                        failed.slot_id = Some(slot_id.to_string());
                        failed.step_records = result.step_records.clone();
                        failed.add_diagnostic(ProviderBoxDiagnostic::error(
                            DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                            "Codex returned to input but no matching durable rollout final was found",
                            json!({
                                "slot_id": slot_id,
                                "correlation_id": request.correlation_id,
                                "codex_home": self.codex_home.display().to_string(),
                                "rule": "PTY screen text is diagnostic only; no fallback final was synthesized"
                            }),
                        ));
                        return failed;
                    }
                } else {
                    idle_seen_at = Some(Instant::now());
                }
            } else {
                idle_seen_at = None;
            }

            if Instant::now() >= deadline {
                let _ = self.pty.write(slot_id, "\x1b").await;
                tokio::time::sleep(Duration::from_secs(1)).await;
                let mut failed = ProviderBoxResult::base(request, ProviderBoxStatus::Failed);
                failed.slot_id = Some(slot_id.to_string());
                failed.step_records = result.step_records.clone();
                failed.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_TURN_TIMEOUT_CANCEL_FAILED,
                    "Codex provider-box turn timed out before a matching durable final appeared",
                    json!({
                        "slot_id": slot_id,
                        "correlation_id": request.correlation_id,
                        "timeout_secs": timeout_secs,
                        "cancel": DIAG_PROVIDER_TURN_TIMEOUT_CANCELLED,
                    }),
                ));
                return failed;
            }

            tokio::time::sleep(Duration::from_millis(750)).await;
        }
    }

    async fn input_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) {
        let Some(input) = request
            .prompt
            .as_ref()
            .map(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
        else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Codex input control action requires prompt or text",
                json!({
                    "slot_id": slot_id,
                    "control_action": "input",
                }),
            ));
            return;
        };

        if !self.ensure_ready_for_prompt(result, slot_id).await {
            return;
        }

        let submit = Self::request_submit_input(request);
        let mut action = PtyStepAction::text("<input text>");
        action.redacted = true;
        let mut after = self
            .write_step(
                result,
                slot_id,
                action,
                input,
                Some("write text into Codex composer".to_string()),
            )
            .await;
        if submit {
            after = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key("enter"),
                    "\r",
                    Some("press Enter to submit Codex input".to_string()),
                )
                .await;
        }
        let failed = result
            .step_records
            .last()
            .is_some_and(|step| step.verification_status == PtyStepVerificationStatus::Failed);
        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &after));
        result.status = if failed {
            ProviderBoxStatus::Failed
        } else {
            ProviderBoxStatus::Completed
        };
    }

    async fn clear_input_locked(&self, result: &mut ProviderBoxResult, slot_id: &str) {
        let before = self.observe(slot_id).await;
        if before.snapshot.state == PtyCanonicalState::Running {
            result.status = ProviderBoxStatus::Blocked;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "Codex clear-input refused because the slot is currently running",
                json!({
                    "slot_id": slot_id,
                    "reason": before.snapshot.reason,
                    "rule": "clear_input sends Ctrl-C only when Codex is not actively working"
                }),
            ));
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &before));
            return;
        }
        if is_codex_empty_composer(&before) {
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &before));
            result.status = ProviderBoxStatus::Completed;
            return;
        }

        let mut after = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("ctrl+c"),
                "\x03",
                Some("clear Codex composer input with Ctrl-C".to_string()),
            )
            .await;
        if !is_ready_for_codex_text(&after) {
            after = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(2),
                    Some("wait for Codex composer after Ctrl-C".to_string()),
                    is_ready_for_codex_text,
                )
                .await;
        }
        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &after));
        if is_ready_for_codex_text(&after) {
            result.status = ProviderBoxStatus::Completed;
        } else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "Codex Ctrl-C clear-input did not return to a ready composer",
                json!({
                    "slot_id": slot_id,
                    "reason": after.snapshot.reason,
                }),
            ));
        }
    }

    async fn clear_screen_locked(&self, result: &mut ProviderBoxResult, slot_id: &str) {
        if !self.ensure_ready_for_prompt(result, slot_id).await {
            return;
        }
        self.clear_input_locked(result, slot_id).await;
        if result.status == ProviderBoxStatus::Blocked || result.status == ProviderBoxStatus::Failed
        {
            return;
        }

        let _ = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::text("/clear"),
                "/clear",
                Some("type Codex /clear command".to_string()),
            )
            .await;
        let mut observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("execute Codex /clear command".to_string()),
            )
            .await;
        if !is_ready_for_codex_text(&observation) {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(5),
                    Some("wait for Codex composer after /clear".to_string()),
                    is_ready_for_codex_text,
                )
                .await;
        }
        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
        if is_ready_for_codex_text(&observation) {
            result.status = ProviderBoxStatus::Completed;
        } else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "Codex /clear execution did not return to a ready composer",
                json!({
                    "slot_id": slot_id,
                    "reason": observation.snapshot.reason,
                }),
            ));
        }
    }

    async fn exit_locked(&self, result: &mut ProviderBoxResult, slot_id: &str) {
        if !self.ensure_ready_for_prompt(result, slot_id).await {
            return;
        }
        self.clear_input_locked(result, slot_id).await;
        if matches!(
            result.status,
            ProviderBoxStatus::Blocked | ProviderBoxStatus::Failed
        ) {
            return;
        }

        let before_status = self.pty.get_status(slot_id).await;
        let before_pid = before_status.as_ref().and_then(|status| status.pid);
        let before_started_at = before_status.as_ref().and_then(|status| status.started_at);

        let _ = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::text("/exit"),
                "/exit",
                Some("type Codex /exit command".to_string()),
            )
            .await;
        let _ = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("execute Codex /exit command".to_string()),
            )
            .await;

        let started = Instant::now();
        let mut last_status = self.pty.get_status(slot_id).await;
        while started.elapsed() < Duration::from_secs(8) {
            let exited_or_restarted = match last_status.as_ref() {
                None => true,
                Some(status) => {
                    matches!(status.state, SessionState::Exited | SessionState::Error)
                        || (before_pid.is_some() && status.pid != before_pid)
                        || (before_started_at.is_some() && status.started_at != before_started_at)
                }
            };
            if exited_or_restarted {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            last_status = self.pty.get_status(slot_id).await;
        }

        let observation = self.observe(slot_id).await;
        let exited_or_restarted = match last_status.as_ref() {
            None => true,
            Some(status) => {
                matches!(status.state, SessionState::Exited | SessionState::Error)
                    || (before_pid.is_some() && status.pid != before_pid)
                    || (before_started_at.is_some() && status.started_at != before_started_at)
            }
        };
        result.slot_status = Some(slot_status_value(
            slot_id,
            last_status.as_ref(),
            &observation,
        ));
        if exited_or_restarted {
            result.status = ProviderBoxStatus::Completed;
            result.final_text = Some(match last_status.as_ref() {
                Some(status)
                    if status.pid.is_some()
                        && (status.pid != before_pid || status.started_at != before_started_at) =>
                {
                    "Codex /exit completed; the slot was restarted by the PTY supervisor"
                        .to_string()
                }
                _ => "Codex /exit completed".to_string(),
            });
            result.durable_source = Some("codex_exit_command".to_string());
        } else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "Codex /exit did not verify that the previous process exited or restarted",
                json!({
                    "slot_id": slot_id,
                    "before_pid": before_pid,
                    "after_pid": last_status.as_ref().and_then(|status| status.pid),
                    "before_started_at": before_started_at,
                    "after_started_at": last_status.as_ref().and_then(|status| status.started_at),
                    "reason": observation.snapshot.reason,
                }),
            ));
        }
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

        let mut observation = self.observe(slot_id).await;
        if !is_codex_permission_picker_observation(&observation) {
            if !self.ensure_ready_for_prompt(result, slot_id).await {
                return;
            }
            self.clear_input_locked(result, slot_id).await;
            if matches!(
                result.status,
                ProviderBoxStatus::Blocked | ProviderBoxStatus::Failed
            ) {
                return;
            }

            let _ = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::text("/permissions"),
                    "/permissions",
                    Some("type Codex /permissions command".to_string()),
                )
                .await;
            observation = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key("enter"),
                    "\r",
                    Some("open Codex permissions picker".to_string()),
                )
                .await;
            if !is_codex_permission_picker_observation(&observation) {
                observation = self
                    .wait_step_until(
                        result,
                        slot_id,
                        Duration::from_secs(5),
                        Some("wait for Codex permissions picker".to_string()),
                        is_codex_permission_picker_observation,
                    )
                    .await;
            }
        }

        let Some((modes, mut selected)) = codex_permission_picker_modes(&observation) else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "Codex permissions picker was not recognized",
                json!({
                    "slot_id": slot_id,
                    "target_permission_mode": target.label(),
                    "reason": observation.snapshot.reason,
                }),
            ));
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            return;
        };

        let selected_index = modes.iter().position(|mode| *mode == selected);
        let target_index = modes.iter().position(|mode| *mode == target);
        let (Some(selected_index), Some(target_index)) = (selected_index, target_index) else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "Codex permissions picker did not include the target or selected mode",
                json!({
                    "slot_id": slot_id,
                    "target_permission_mode": target.label(),
                    "selected_permission_mode": selected.label(),
                    "visible_permission_modes": modes.iter().map(|mode| mode.label()).collect::<Vec<_>>(),
                }),
            ));
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            return;
        };

        let (key_name, bytes, steps) = if target_index >= selected_index {
            ("down", "\x1b[B", target_index - selected_index)
        } else {
            ("up", "\x1b[A", selected_index - target_index)
        };

        for step_index in 0..steps {
            observation = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key(key_name),
                    bytes,
                    Some(format!(
                        "move Codex permissions selection toward {}",
                        target.label()
                    )),
                )
                .await;
            let Some((_visible, next_selected)) = codex_permission_picker_modes(&observation)
            else {
                result.status = ProviderBoxStatus::Unverified;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                    "Codex permissions picker disappeared during navigation",
                    json!({
                        "slot_id": slot_id,
                        "target_permission_mode": target.label(),
                        "step_index": step_index,
                    }),
                ));
                let status = self.pty.get_status(slot_id).await;
                result.slot_status =
                    Some(slot_status_value(slot_id, status.as_ref(), &observation));
                return;
            };
            if next_selected == selected {
                result.status = ProviderBoxStatus::Unverified;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                    "Codex permissions selection did not move after arrow key",
                    json!({
                        "slot_id": slot_id,
                        "target_permission_mode": target.label(),
                        "selected_permission_mode": selected.label(),
                        "key": key_name,
                        "step_index": step_index,
                    }),
                ));
                let status = self.pty.get_status(slot_id).await;
                result.slot_status =
                    Some(slot_status_value(slot_id, status.as_ref(), &observation));
                return;
            }
            selected = next_selected;
        }

        if selected != target {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "Codex permissions navigation ended on the wrong mode",
                json!({
                    "slot_id": slot_id,
                    "target_permission_mode": target.label(),
                    "selected_permission_mode": selected.label(),
                }),
            ));
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            return;
        }

        observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some(format!("confirm Codex permissions mode {}", target.label())),
            )
            .await;
        if !is_ready_for_codex_text(&observation) {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(5),
                    Some("wait for Codex composer after permissions confirmation".to_string()),
                    is_ready_for_codex_text,
                )
                .await;
        }

        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
        if is_ready_for_codex_text(&observation) {
            result.status = ProviderBoxStatus::Completed;
            result.final_text = Some(format!("Codex permissions set to {}", target.label()));
            result.durable_source =
                Some("codex_permission_picker_selection_before_confirm".to_string());
        } else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "Codex permissions confirmation did not return to a ready composer",
                json!({
                    "slot_id": slot_id,
                    "target_permission_mode": target.label(),
                    "reason": observation.snapshot.reason,
                }),
            ));
        }
    }

    async fn set_fast_mode_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) {
        let target = match Self::request_fast_mode(request) {
            Ok(mode) => mode,
            Err(diagnostic) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(diagnostic);
                return;
            }
        };

        if !self.ensure_ready_for_prompt(result, slot_id).await {
            return;
        }
        self.clear_input_locked(result, slot_id).await;
        if matches!(
            result.status,
            ProviderBoxStatus::Blocked | ProviderBoxStatus::Failed
        ) {
            return;
        }

        let mut observation = self.observe(slot_id).await;
        let Some(current) = codex_fast_mode(&observation) else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "Codex current fast-mode state could not be read before /fast toggle",
                json!({
                    "slot_id": slot_id,
                    "target_fast_mode": target.label(),
                    "current_model": codex_current_model(&observation),
                    "reason": observation.snapshot.reason,
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
            result.final_text = Some(format!("Codex fast mode already {}", target.label()));
            result.durable_source = Some("codex_screen_identity_current_model".to_string());
            return;
        }

        let _ = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::text("/fast"),
                "/fast",
                Some("type Codex /fast command".to_string()),
            )
            .await;
        observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some(format!(
                    "toggle Codex /fast service tier to {}",
                    target.service_tier()
                )),
            )
            .await;
        if codex_fast_mode(&observation) != Some(target) {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(5),
                    Some(format!(
                        "wait for Codex fast mode to become {}",
                        target.label()
                    )),
                    |obs| is_ready_for_codex_text(obs) && codex_fast_mode(obs) == Some(target),
                )
                .await;
        }

        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
        if is_ready_for_codex_text(&observation) && codex_fast_mode(&observation) == Some(target) {
            result.status = ProviderBoxStatus::Completed;
            result.final_text = Some(format!("Codex fast mode {}", target.label()));
            result.durable_source = Some("codex_fast_toggle_screen_identity".to_string());
        } else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "Codex /fast toggle did not verify the requested service tier",
                json!({
                    "slot_id": slot_id,
                    "target_fast_mode": target.label(),
                    "observed_fast_mode": codex_fast_mode(&observation).map(|mode| mode.label()),
                    "current_model": codex_current_model(&observation),
                    "reason": observation.snapshot.reason,
                }),
            ));
        }
    }

    async fn refresh_mcp_status_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) -> CodexObservation {
        if !self.ensure_ready_for_prompt(result, slot_id).await {
            let observation = self.observe(slot_id).await;
            result.mcp_status = Some(codex_mcp_status_value(request, slot_id, &observation));
            return observation;
        }
        self.clear_input_locked(result, slot_id).await;

        let _ = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::text("/mcp"),
                "/mcp",
                Some("type Codex /mcp command".to_string()),
            )
            .await;
        let mut observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("execute Codex /mcp command".to_string()),
            )
            .await;
        if observation.snapshot.screen_mcp.is_none() {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(8),
                    Some("wait for Codex MCP status output".to_string()),
                    |obs| obs.snapshot.screen_mcp.is_some(),
                )
                .await;
        }
        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
        result.mcp_status = Some(codex_mcp_status_value(request, slot_id, &observation));
        if observation.snapshot.screen_mcp.is_some() {
            result.status = ProviderBoxStatus::Completed;
        } else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_MCP_STATUS_UNAVAILABLE,
                "Codex /mcp output was not recognized as an MCP status screen",
                json!({
                    "slot_id": slot_id,
                    "reason": observation.snapshot.reason,
                }),
            ));
        }
        observation
    }

    async fn refresh_usage_status_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) -> CodexObservation {
        if !self.ensure_ready_for_prompt(result, slot_id).await {
            let observation = self.observe(slot_id).await;
            result.usage_snapshot = Some(codex_unknown_usage_snapshot(
                request,
                slot_id,
                Some(&observation),
                "Codex slot was not ready for /status usage probing",
            ));
            return observation;
        }
        self.clear_input_locked(result, slot_id).await;

        let _ = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::text("/status"),
                "/status",
                Some("type Codex /status command".to_string()),
            )
            .await;
        let mut observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("execute Codex /status command".to_string()),
            )
            .await;
        if extract_codex_usage_lines(&observation.text).is_none() {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(8),
                    Some("wait for Codex /status quota lines".to_string()),
                    |obs| extract_codex_usage_lines(&obs.text).is_some(),
                )
                .await;
        }
        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
        if let Some(usage) = extract_codex_usage_lines(&observation.text) {
            result.status = ProviderBoxStatus::Completed;
            result.final_text = Some(usage.as_text());
            result.usage_snapshot =
                Some(codex_usage_snapshot(request, slot_id, &observation, &usage));
        } else {
            result.status = ProviderBoxStatus::Unverified;
            result.usage_snapshot = Some(codex_unknown_usage_snapshot(
                request,
                slot_id,
                Some(&observation),
                "Codex /status output did not include recognizable 5h and Weekly limit lines",
            ));
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_USAGE_UNKNOWN,
                "Codex /status usage lines were not recognized",
                json!({
                    "slot_id": slot_id,
                    "reason": observation.snapshot.reason,
                }),
            ));
        }
        observation
    }

    fn validate_prompt_turn(
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> bool {
        let prompt_ok = request
            .prompt
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty());
        let attachments_ok = request.attachments.is_empty();
        if prompt_ok && attachments_ok {
            return true;
        }
        result.status = ProviderBoxStatus::Failed;
        result.add_diagnostic(ProviderBoxDiagnostic::error(
            DIAG_PROVIDER_BOX_INVALID_REQUEST,
            "Codex provider-box turn requires a non-empty prompt and currently supports no attachments",
            json!({
                "prompt_present": prompt_ok,
                "attachments": request.attachments.len(),
            }),
        ));
        false
    }

    fn validate_pure_text_exec_request(
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> Option<(String, Option<String>, String)> {
        if !Self::validate_prompt_turn(request, result) {
            return None;
        }
        if request.dangerously_bypass_approvals_and_sandbox {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Codex exec text-only source does not allow bypassing approvals and sandbox",
                json!({
                    "provider": CODEX_EXEC_TEXT_PROVIDER,
                    "rule": "pure text lanes run in an isolated cwd with read-only sandbox and fail closed on tool events"
                }),
            ));
            return None;
        }
        if !(request.no_tools && request.no_mcp && request.no_shell && request.no_file_access) {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Codex exec text-only source requires no_tools/no_mcp/no_shell/no_file_access guards",
                json!({
                    "no_tools": request.no_tools,
                    "no_mcp": request.no_mcp,
                    "no_shell": request.no_shell,
                    "no_file_access": request.no_file_access,
                }),
            ));
            return None;
        }

        let Some(model) = request
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
        else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Codex exec text-only source requires an explicit model",
                json!({
                    "examples": [
                        {"model": "gpt-5.5", "model_profile": "xhigh"},
                        {"model": "gpt-5.5"}
                    ]
                }),
            ));
            return None;
        };
        let model = match normalize_codex_exec_model(&model) {
            Ok(value) => value,
            Err(value) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "Codex exec text-only source only exports GPT-5.5",
                    json!({
                        "model": value,
                        "allowed_models": [CODEX_EXEC_TEXT_MODEL],
                        "profiles": ["default", "xhigh"]
                    }),
                ));
                return None;
            }
        };

        let reasoning = match normalize_codex_reasoning_effort(request.model_profile.as_deref()) {
            Ok(value) => value,
            Err(value) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "Codex exec text-only source received an unsupported reasoning effort",
                    json!({
                        "model_profile": value,
                        "allowed": ["minimal", "low", "medium", "high", "xhigh", "default"]
                    }),
                ));
                return None;
            }
        };

        let prompt = request.prompt.clone().unwrap_or_default();
        Some((model, reasoning, prompt))
    }

    fn request_exec_task_kind(request: &ProviderInteractionRequest) -> Option<CodexExecTaskKind> {
        match request.command {
            BoxCommand::Research => return Some(CodexExecTaskKind::Research),
            BoxCommand::ImageGeneration => return Some(CodexExecTaskKind::ImageGeneration),
            _ => {}
        }

        match request.provider.as_deref().map(str::trim) {
            Some(value)
                if value.eq_ignore_ascii_case(CODEX_RESEARCH_PROVIDER)
                    || value.eq_ignore_ascii_case("codex-research") =>
            {
                Some(CodexExecTaskKind::Research)
            }
            Some(value)
                if value.eq_ignore_ascii_case(CODEX_IMAGE_PROVIDER)
                    || value.eq_ignore_ascii_case("codex-image-generation")
                    || value.eq_ignore_ascii_case("codex_image")
                    || value.eq_ignore_ascii_case("codex-image") =>
            {
                Some(CodexExecTaskKind::ImageGeneration)
            }
            _ => None,
        }
    }

    fn validate_codex_exec_task_request(
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        kind: CodexExecTaskKind,
    ) -> Option<(String, Option<String>, String)> {
        if !Self::validate_prompt_turn(request, result) {
            return None;
        }
        if request.dangerously_bypass_approvals_and_sandbox {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Codex task source does not allow bypassing approvals and sandbox",
                json!({
                    "provider": kind.provider(),
                    "kind": kind.label(),
                    "rule": "task sources run in isolated read-only workspaces with explicit tool allowlists"
                }),
            ));
            return None;
        }
        if !(request.no_mcp && request.no_shell && request.no_file_access) {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Codex task source requires no_mcp/no_shell/no_file_access guards",
                json!({
                    "provider": kind.provider(),
                    "kind": kind.label(),
                    "no_mcp": request.no_mcp,
                    "no_shell": request.no_shell,
                    "no_file_access": request.no_file_access,
                }),
            ));
            return None;
        }
        if request.no_tools {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Codex task source must use the provider-box task tool allowlist instead of no_tools=true",
                json!({
                    "provider": kind.provider(),
                    "kind": kind.label(),
                    "allowed_tool_policy": match kind {
                        CodexExecTaskKind::Research => json!(["web_search"]),
                        CodexExecTaskKind::ImageGeneration => json!(["image_generation"]),
                        CodexExecTaskKind::TextOnly => json!([]),
                    },
                }),
            ));
            return None;
        }

        let Some(model) = request
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
        else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Codex task source requires an explicit model",
                json!({
                    "provider": kind.provider(),
                    "allowed_models": [CODEX_EXEC_TEXT_MODEL],
                }),
            ));
            return None;
        };
        let model = match normalize_codex_exec_model(&model) {
            Ok(value) => value,
            Err(value) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "Codex task source only exports GPT-5.5",
                    json!({
                        "provider": kind.provider(),
                        "kind": kind.label(),
                        "model": value,
                        "allowed_models": [CODEX_EXEC_TEXT_MODEL],
                    }),
                ));
                return None;
            }
        };

        let reasoning = match normalize_codex_reasoning_effort(request.model_profile.as_deref()) {
            Ok(value) => value,
            Err(value) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "Codex task source received an unsupported reasoning effort",
                    json!({
                        "provider": kind.provider(),
                        "kind": kind.label(),
                        "model_profile": value,
                        "allowed": ["minimal", "low", "medium", "high", "xhigh", "default"]
                    }),
                ));
                return None;
            }
        };

        let prompt = request.prompt.clone().unwrap_or_default();
        Some((model, reasoning, prompt))
    }

    async fn run_codex_exec_text(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        model: &str,
        reasoning: Option<&str>,
        prompt: &str,
    ) -> bool {
        self.run_codex_exec_task(
            request,
            result,
            CodexExecTaskKind::TextOnly,
            model,
            reasoning,
            prompt,
        )
        .await
    }

    async fn run_codex_exec_task(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        kind: CodexExecTaskKind,
        model: &str,
        reasoning: Option<&str>,
        prompt: &str,
    ) -> bool {
        let queue_key = codex_exec_queue_key(kind, model, reasoning);
        let queue_max_concurrent = codex_exec_queue_max_concurrent(kind, reasoning);
        let queue_semaphore = self
            .exec_lane_semaphore(&queue_key, queue_max_concurrent)
            .await;
        let queued_at = Instant::now();
        let Ok(_queue_guard) = queue_semaphore.acquire_owned().await else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                format!("Codex exec {} queue is unavailable", kind.label()),
                json!({
                    "provider": kind.provider(),
                    "kind": kind.label(),
                    "queue": {
                        "key": queue_key,
                        "max_concurrent": queue_max_concurrent,
                    },
                }),
            ));
            return false;
        };
        let queue_wait_ms = u64::try_from(queued_at.elapsed().as_millis()).unwrap_or(u64::MAX);

        let Some(codex_binary) = self.locate_codex_binary(request, result).await else {
            return false;
        };
        let workspace = codex_exec_workspace_for_kind(kind, &request.correlation_id);
        if let Err(err) = tokio::fs::create_dir_all(&workspace).await {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                format!("Codex exec {} workspace could not be created", kind.label()),
                json!({
                    "provider": kind.provider(),
                    "kind": kind.label(),
                    "workspace": workspace.display().to_string(),
                    "error": err.to_string(),
                }),
            ));
            return false;
        }
        let output_file = workspace.join("last-message.txt");
        let events_file = workspace.join("events.jsonl");
        let stderr_file = workspace.join("stderr.log");
        let args = codex_exec_args_for_kind(kind, &output_file, model, reasoning);
        let timeout_secs = request.timeout_secs.unwrap_or(180).clamp(10, 7_200);

        let mut command = Command::new(&codex_binary);
        command
            .args(&args)
            .arg(prompt)
            .current_dir(&workspace)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    format!("Codex exec {} process could not be started", kind.label()),
                    json!({
                        "provider": kind.provider(),
                        "kind": kind.label(),
                        "codex_binary": codex_binary.display().to_string(),
                        "error": err.to_string(),
                    }),
                ));
                return false;
            }
        };

        let stdout_task = child.stdout.take().map(|mut stdout| {
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let _ = stdout.read_to_end(&mut buf).await;
                buf
            })
        });
        let stderr_task = child.stderr.take().map(|mut stderr| {
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let _ = stderr.read_to_end(&mut buf).await;
                buf
            })
        });

        let status =
            match tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait()).await {
                Ok(Ok(status)) => status,
                Ok(Err(err)) => {
                    result.status = ProviderBoxStatus::Failed;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                        format!("Codex exec {} process wait failed", kind.label()),
                        json!({
                            "provider": kind.provider(),
                            "kind": kind.label(),
                            "error": err.to_string(),
                        }),
                    ));
                    return false;
                }
                Err(_) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    result.status = ProviderBoxStatus::Failed;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_TURN_TIMEOUT_CANCEL_FAILED,
                        format!(
                            "Codex exec {} process timed out and was killed",
                            kind.label()
                        ),
                        json!({
                            "provider": kind.provider(),
                            "kind": kind.label(),
                            "timeout_secs": timeout_secs,
                            "cancel": DIAG_PROVIDER_TURN_TIMEOUT_CANCELLED,
                        }),
                    ));
                    return false;
                }
            };

        let stdout = join_output_task(stdout_task).await;
        let stderr = join_output_task(stderr_task).await;
        let stdout_text = String::from_utf8_lossy(&stdout).to_string();
        let stderr_text = String::from_utf8_lossy(&stderr).to_string();
        let _ = tokio::fs::write(&events_file, stdout_text.as_bytes()).await;
        let _ = tokio::fs::write(&stderr_file, stderr_text.as_bytes()).await;
        let analysis = analyze_codex_exec_jsonl_for_kind(&stdout_text, kind);
        let image_evidence = if kind == CodexExecTaskKind::ImageGeneration {
            if let Some(thread_id) = analysis.thread_id.as_deref() {
                self.extract_image_generation_from_rollouts(thread_id).await
            } else {
                None
            }
        } else {
            None
        };
        let allowed_tool_event_count = image_evidence
            .as_ref()
            .map(|evidence| evidence.image_event_count)
            .unwrap_or(analysis.allowed_tool_event_count);

        if let Some(violation) = analysis.violation {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_TEXT_ONLY_VIOLATION,
                format!(
                    "Codex exec {} turn attempted a disallowed tool, shell, file, MCP, or approval action",
                    kind.label()
                ),
                json!({
                    "provider": kind.provider(),
                    "kind": kind.label(),
                    "event": violation,
                    "stdout_event_count": analysis.event_count,
                    "allowed_tool_event_count": allowed_tool_event_count,
                    "rule": "provider-box task results are not returned after disallowed provider tool activity"
                }),
            ));
            return false;
        }

        if !status.success() {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                format!("Codex exec {} process exited unsuccessfully", kind.label()),
                json!({
                    "provider": kind.provider(),
                    "kind": kind.label(),
                    "exit_status": status.code(),
                    "stdout_excerpt": stdout_text.chars().take(1200).collect::<String>(),
                    "stderr_excerpt": stderr_text.chars().take(1200).collect::<String>(),
                    "stdout_event_count": analysis.event_count,
                    "allowed_tool_event_count": allowed_tool_event_count,
                }),
            ));
            return false;
        }

        if kind != CodexExecTaskKind::TextOnly && allowed_tool_event_count == 0 {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                format!(
                    "Codex exec {} completed without required tool evidence",
                    kind.label()
                ),
                json!({
                    "provider": kind.provider(),
                    "kind": kind.label(),
                    "expected_tool_family": kind.required_tool_family(),
                    "output_last_message": output_file.display().to_string(),
                    "events_jsonl": events_file.display().to_string(),
                    "codex_thread_id": analysis.thread_id.as_deref(),
                    "stdout_event_count": analysis.event_count,
                    "allowed_tool_event_count": allowed_tool_event_count,
                    "rule": "provider-box task sources require durable JSONL evidence that the lane-specific tool actually ran"
                }),
            ));
            return false;
        }

        let image_upload = if kind == CodexExecTaskKind::ImageGeneration {
            let Some(evidence) = image_evidence.as_ref() else {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                    "Codex image generation completed without validated image evidence",
                    json!({
                        "provider": kind.provider(),
                        "kind": kind.label(),
                        "rule": "image generation results must have rollout JSONL evidence and validated PNG before media upload"
                    }),
                ));
                return false;
            };
            match self
                .upload_image_generation_artifact(request, evidence)
                .await
            {
                Ok(upload) => Some(upload),
                Err(diagnostic) => {
                    result.status = ProviderBoxStatus::Failed;
                    result.add_diagnostic(diagnostic);
                    return false;
                }
            }
        } else {
            None
        };

        let final_text = if kind == CodexExecTaskKind::TextOnly {
            tokio::fs::read_to_string(&output_file)
                .await
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .or_else(|| analysis.final_text.map(|value| value.trim().to_string()))
                .filter(|value| !value.is_empty())
        } else if kind == CodexExecTaskKind::ImageGeneration {
            image_upload
                .as_ref()
                .map(|upload| upload.final_text.clone())
        } else {
            analysis
                .final_text
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };

        let Some(final_text) = final_text else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                format!(
                    "Codex exec {} completed without a usable JSONL assistant final",
                    kind.label()
                ),
                json!({
                    "provider": kind.provider(),
                    "kind": kind.label(),
                    "output_last_message": output_file.display().to_string(),
                    "events_jsonl": events_file.display().to_string(),
                    "codex_thread_id": analysis.thread_id.as_deref(),
                    "stdout_event_count": analysis.event_count,
                    "allowed_tool_event_count": allowed_tool_event_count,
                }),
            ));
            return false;
        };

        result.status = ProviderBoxStatus::Completed;
        result.provider = Some(kind.provider().to_string());
        result.provider_conversation_id = analysis
            .thread_id
            .clone()
            .or_else(|| Some(request.correlation_id.clone()));
        result.durable_source = Some(
            if let Some(evidence) = image_evidence.as_ref() {
                Path::new(&evidence.rollout_path)
            } else if kind == CodexExecTaskKind::TextOnly {
                &output_file
            } else {
                &events_file
            }
            .display()
            .to_string(),
        );
        if let Some(session_id) = result.provider_conversation_id.clone() {
            result.provider_session_identity = Some(ProviderSessionIdentity::resolved(
                Some(kind.provider().to_string()),
                CliEngine::Codex,
                None,
                session_id,
                if image_evidence.is_some() {
                    "codex_rollout_jsonl_image_generation_end"
                } else {
                    "codex_exec_jsonl_thread_id"
                },
                result.durable_source.clone(),
                Some(workspace.display().to_string()),
                "durable_exec_result",
            ));
        }
        result.slot_status = Some(json!({
            "kind": "codex_exec_task",
            "task_kind": kind.label(),
            "workspace": workspace.display().to_string(),
            "output_last_message": output_file.display().to_string(),
            "events_jsonl": events_file.display().to_string(),
            "stderr_log": stderr_file.display().to_string(),
            "model": model,
            "reasoning_effort": reasoning,
            "queue": {
                "owner": "provider-box",
                "key": queue_key.as_str(),
                "max_concurrent": queue_max_concurrent,
                "wait_ms": queue_wait_ms,
                "policy": "per_logical_codex_exec_source"
            },
            "output_media_type": kind.output_media_type(),
            "stdout_event_count": analysis.event_count,
            "allowed_tool_event_count": allowed_tool_event_count,
            "codex_thread_id": analysis.thread_id.as_deref(),
            "image_generation_evidence": image_evidence.as_ref().map(|evidence| json!({
                "session_id": evidence.session_id,
                "rollout_path": evidence.rollout_path,
                "validated_image_count": evidence.image_paths.len(),
                "revised_prompts": evidence.revised_prompts,
                "image_event_count": evidence.image_event_count,
                "assistant_final": evidence.final_text,
            })),
            "media_artifact": image_upload.as_ref().map(|upload| upload.artifact.clone()),
            "exit_status": status.code(),
        }));
        result.final_text = Some(final_text);
        true
    }

    async fn extract_turn_from_rollouts(&self, correlation_id: &str) -> Option<CodexTurnFinal> {
        let roots = [
            self.codex_home.join("sessions"),
            self.codex_home.join("archived_sessions"),
        ];
        let correlation_id = correlation_id.to_string();
        tokio::task::spawn_blocking(move || {
            let mut files = Vec::new();
            for root in roots {
                collect_jsonl_files(&root, &mut files);
            }
            files.sort_by(|a, b| modified_at(b).cmp(&modified_at(a)));
            files.truncate(80);
            files
                .iter()
                .filter_map(|path| extract_correlated_rollout(path, &correlation_id))
                .max_by_key(|turn| modified_at(Path::new(&turn.rollout_path)))
        })
        .await
        .ok()
        .flatten()
    }

    async fn extract_image_generation_from_rollouts(
        &self,
        thread_id: &str,
    ) -> Option<CodexImageGenerationEvidence> {
        let roots = [
            self.codex_home.join("sessions"),
            self.codex_home.join("archived_sessions"),
        ];
        let codex_home = self.codex_home.clone();
        let thread_id = thread_id.to_string();
        tokio::task::spawn_blocking(move || {
            let mut files = Vec::new();
            for root in roots {
                collect_jsonl_files(&root, &mut files);
            }
            files.sort_by(|a, b| modified_at(b).cmp(&modified_at(a)));
            files.truncate(120);
            files
                .iter()
                .filter_map(|path| extract_image_generation_rollout(path, &thread_id, &codex_home))
                .max_by_key(|turn| modified_at(Path::new(&turn.rollout_path)))
        })
        .await
        .ok()
        .flatten()
    }

    async fn upload_image_generation_artifact(
        &self,
        request: &ProviderInteractionRequest,
        evidence: &CodexImageGenerationEvidence,
    ) -> Result<CodexImageUploadResult, ProviderBoxDiagnostic> {
        let base_url = std::env::var("XJP_IMAGE_SERVICE_URL")
            .or_else(|_| std::env::var("XJP_MEDIA_IMAGE_SERVICE_URL"))
            .map_err(|_| {
                ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "XJP image service URL is required for codex_image_generation",
                    json!({
                        "required_env": ["XJP_IMAGE_SERVICE_URL", "XJP_MEDIA_IMAGE_SERVICE_URL"],
                        "rule": "codex_image_generation must upload to xjp-image-service and must not return local paths"
                    }),
                )
            })?;
        let internal_token = std::env::var("XJP_MEDIA_INTERNAL_TOKEN")
            .or_else(|_| std::env::var("XJP_IMAGE_SERVICE_INTERNAL_TOKEN"))
            .map_err(|_| {
                ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "XJP media internal token is required for codex_image_generation",
                    json!({
                        "required_env": ["XJP_MEDIA_INTERNAL_TOKEN", "XJP_IMAGE_SERVICE_INTERNAL_TOKEN"],
                        "rule": "provider-box must authenticate to xjp-image-service import endpoint"
                    }),
                )
            })?;
        let tenant_id = media_tenant_id_from_request(request).ok_or_else(|| {
            ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "tenant_id is required for codex_image_generation media import",
                json!({
                    "accepted_sources": [
                        "output_contract.tenant_id",
                        "write_scope.tenant_id",
                        "desired_worker.tenant_id",
                        "XJP_MEDIA_DEFAULT_TENANT_ID"
                    ],
                    "rule": "media artifacts are tenant-scoped and cannot be uploaded to a default implicit tenant"
                }),
            )
        })?;
        let image_path = evidence.image_paths.first().ok_or_else(|| {
            ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                "Codex image generation evidence has no validated PNG path",
                json!({ "image_event_count": evidence.image_event_count }),
            )
        })?;
        let bytes = tokio::fs::read(image_path).await.map_err(|err| {
            ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                "Failed to read validated Codex generated image for upload",
                json!({
                    "error": err.to_string(),
                    "rule": "validated local PNG must be readable before xjp-image-service import"
                }),
            )
        })?;
        let url = image_import_url(&base_url);
        let body = json!({
            "tenant_id": tenant_id,
            "owner_user_id": media_owner_user_id_from_request(request),
            "content_type": "image/png",
            "bytes_base64": BASE64_STANDARD.encode(&bytes),
            "prompt": evidence.revised_prompts.first().or(evidence.final_text.as_ref()),
            "metadata": {
                "source": "missiond_provider_box",
                "provider": CODEX_IMAGE_PROVIDER,
                "codex_session_id": evidence.session_id,
                "codex_rollout_path": evidence.rollout_path,
                "correlation_id": request.correlation_id,
                "project_id": request.project_id,
                "validated_image_count": evidence.image_paths.len()
            }
        });
        let response = reqwest::Client::new()
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("x-internal-token", internal_token)
            .json(&body)
            .send()
            .await
            .map_err(|err| {
                ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "xjp-image-service import request failed",
                    json!({
                        "url": url,
                        "error": err.to_string(),
                    }),
                )
            })?;
        let status = response.status();
        let artifact = response.json::<Value>().await.map_err(|err| {
            ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                "xjp-image-service import response was not valid JSON",
                json!({
                    "status": status.as_u16(),
                    "error": err.to_string(),
                }),
            )
        })?;
        if !status.is_success() {
            return Err(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "xjp-image-service rejected generated image import",
                json!({
                    "status": status.as_u16(),
                    "response": artifact,
                }),
            ));
        }
        let signed_url = artifact
            .pointer("/signed_url/url")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                    "xjp-image-service import response did not include signed_url.url",
                    json!({
                        "artifact_id": artifact.get("artifact_id"),
                        "rule": "image-generation final_text must point to the platform media URL, not a local path"
                    }),
                )
            })?;
        Ok(CodexImageUploadResult {
            artifact,
            final_text: format!("IMAGE_DONE {signed_url}"),
        })
    }
}

#[async_trait]
impl ProviderDriver for CodexProviderDriver {
    fn engine(&self) -> CliEngine {
        CliEngine::Codex
    }

    fn capabilities(&self) -> ProviderDriverCapabilities {
        ProviderDriverCapabilities {
            submit_turn: true,
            switch_model: false,
            usage_probe: true,
            model_catalog: false,
            pure_text_guard: true,
            control_action: true,
            pty_step: true,
            status: true,
            mcp_status: true,
            mcp_reconnect: false,
        }
    }

    async fn status(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let slot_id = Self::request_slot_id(request);
        result.slot_id = Some(slot_id.clone());

        if Self::request_spawn_if_missing(request) || Self::request_force_restart(request) {
            let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
                return result;
            };
            result.slot_id = Some(slot_id.clone());
        } else {
            let Some(status) = self.pty.get_status(&slot_id).await else {
                result.status = ProviderBoxStatus::Unknown;
                result.add_diagnostic(ProviderBoxDiagnostic::warning(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Codex slot status is unavailable",
                    json!({
                        "slot_id": slot_id,
                        "spawn_if_missing": false,
                    }),
                ));
                return result;
            };
            if status.engine != CliEngine::Codex {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Requested slot is not a Codex slot",
                    json!({
                        "slot_id": slot_id,
                        "engine": status.engine.to_string(),
                    }),
                ));
                return result;
            }
        }

        self.attach_status_observation(
            &mut result,
            &slot_id,
            Some("observe current Codex CLI state".to_string()),
        )
        .await;
        result.status = ProviderBoxStatus::Completed;
        result
    }

    async fn probe_usage(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;
        self.refresh_usage_status_locked(request, &mut result, &slot_id)
            .await;
        result
    }

    async fn submit_turn(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        if let Some(kind) = Self::request_exec_task_kind(request) {
            result.provider = Some(kind.provider().to_string());
            result.slot_id = None;
            let Some((model, reasoning, prompt)) =
                Self::validate_codex_exec_task_request(request, &mut result, kind)
            else {
                return result;
            };
            self.run_codex_exec_task(
                request,
                &mut result,
                kind,
                &model,
                reasoning.as_deref(),
                &prompt,
            )
            .await;
            return result;
        }

        if !Self::validate_prompt_turn(request, &mut result) {
            return result;
        }
        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;

        if !self.ensure_ready_for_prompt(&mut result, &slot_id).await {
            return result;
        }
        let prompt = correlate_prompt(request);
        if !self
            .submit_prompt_step(&mut result, &slot_id, prompt.as_str())
            .await
        {
            result.status = ProviderBoxStatus::Failed;
            return result;
        }
        self.monitor_turn(request, &mut result, &slot_id).await
    }

    async fn control_action(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let Some(action) = request.control_action else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Codex control action request requires control_action",
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
            ProviderControlAction::Input => {
                self.input_locked(request, &mut result, &slot_id).await;
            }
            ProviderControlAction::ClearInput => {
                self.clear_input_locked(&mut result, &slot_id).await;
            }
            ProviderControlAction::ClearScreen => {
                self.clear_screen_locked(&mut result, &slot_id).await;
            }
            ProviderControlAction::SetPermissions => {
                self.set_permissions_locked(request, &mut result, &slot_id)
                    .await;
            }
            ProviderControlAction::SetFastMode => {
                self.set_fast_mode_locked(request, &mut result, &slot_id)
                    .await;
            }
            ProviderControlAction::Exit => {
                self.exit_locked(&mut result, &slot_id).await;
            }
            ProviderControlAction::Logout => {
                result.status = ProviderBoxStatus::Unsupported;
                result.add_diagnostic(ProviderBoxDiagnostic::unsupported(
                    DIAG_PROVIDER_CONTROL_ACTION_UNSUPPORTED,
                    "Codex logout is not a taught provider-box control action",
                    json!({
                        "slot_id": slot_id,
                        "control_action": "logout",
                    }),
                ));
            }
        }
        if result.slot_status.is_none() {
            self.attach_status_observation(
                &mut result,
                &slot_id,
                Some("observe Codex state after control action".to_string()),
            )
            .await;
        }
        result
    }

    async fn pty_step(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let step = match Self::request_manual_pty_step(request) {
            Ok(step) => step,
            Err(diagnostic) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(diagnostic);
                return result;
            }
        };

        let slot_id = if Self::request_spawn_if_missing(request) {
            let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
                return result;
            };
            slot_id
        } else {
            let slot_id = Self::request_slot_id(request);
            result.slot_id = Some(slot_id.clone());
            let Some(status) = self.pty.get_status(&slot_id).await else {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Cannot send a PTY step to an unavailable Codex slot",
                    json!({
                        "slot_id": slot_id,
                        "spawn_if_missing": false,
                    }),
                ));
                return result;
            };
            if status.engine != CliEngine::Codex {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Requested slot is not a Codex slot",
                    json!({
                        "slot_id": slot_id,
                        "engine": status.engine.to_string(),
                    }),
                ));
                return result;
            }
            slot_id
        };
        result.slot_id = Some(slot_id.clone());

        let (action, bytes) = if step.action_type == "key" {
            let key = step.key.as_deref().unwrap_or_default();
            let Some((canonical, bytes)) = codex_manual_key_bytes(key) else {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "Codex key PTY step uses an unsupported key",
                    json!({
                        "slot_id": slot_id,
                        "key": key,
                        "allowed_keys": CODEX_MANUAL_KEY_NAMES,
                    }),
                ));
                return result;
            };
            (PtyStepAction::key(canonical), bytes.to_string())
        } else {
            let text = step.text.as_deref().unwrap_or_default();
            if let Err(diagnostic) = validate_codex_manual_text_step(Some(&slot_id), text) {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(diagnostic);
                return result;
            }
            let mut action = if step.redacted {
                PtyStepAction::text("<manual text>")
            } else {
                PtyStepAction::text(text)
            };
            action.redacted = step.redacted;
            (action, text.to_string())
        };

        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;
        let after = self
            .write_step(
                &mut result,
                &slot_id,
                action,
                &bytes,
                step.expected_change.clone(),
            )
            .await;
        let status = self.pty.get_status(&slot_id).await;
        result.slot_status = Some(slot_status_value(&slot_id, status.as_ref(), &after));
        let failed = result
            .step_records
            .last()
            .is_some_and(|step| step.verification_status == PtyStepVerificationStatus::Failed);
        result.status = if failed {
            ProviderBoxStatus::Failed
        } else {
            ProviderBoxStatus::Completed
        };
        result
    }

    async fn mcp_status(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;
        self.refresh_mcp_status_locked(request, &mut result, &slot_id)
            .await;
        result
    }

    async fn mcp_reconnect(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;
        self.refresh_mcp_status_locked(request, &mut result, &slot_id)
            .await;
        result.status = ProviderBoxStatus::Unsupported;
        result.add_diagnostic(ProviderBoxDiagnostic::unsupported(
            DIAG_PROVIDER_MCP_RECONNECT_UNSUPPORTED,
            "Codex CLI does not support hot MCP reload through /mcp; restart the PTY slot after fixing MCP config",
            json!({
                "slot_id": slot_id,
                "server": Self::request_mcp_server(request),
                "destructive_restart_performed": false,
                "operator_hint": "Provider-box intentionally does not restart Codex from mcp/reconnect because the PTY conversation context may be valuable. External LLM callers should ask for an explicit slot restart if losing context is acceptable."
            }),
        ));
        result
    }

    async fn pure_text_single_turn(
        &self,
        request: &ProviderInteractionRequest,
    ) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        result.provider = Some(CODEX_EXEC_TEXT_PROVIDER.to_string());
        result.slot_id = None;
        let Some((model, reasoning, prompt)) =
            Self::validate_pure_text_exec_request(request, &mut result)
        else {
            return result;
        };

        self.run_codex_exec_text(request, &mut result, &model, reasoning.as_deref(), &prompt)
            .await;
        result
    }
}

fn correlate_prompt(request: &ProviderInteractionRequest) -> String {
    let prompt = request.prompt.clone().unwrap_or_default();
    if prompt.contains(&request.correlation_id) {
        return prompt;
    }
    format!(
        "MissionD provider-box correlation_id: {}\n\
         MissionD provider-box turn_id_hint: {}\n\n{}",
        request.correlation_id,
        request.task_id.as_deref().unwrap_or("none"),
        prompt
    )
}

fn bool_any(value: Option<&Value>, keys: &[&str]) -> bool {
    let Some(value) = value else {
        return false;
    };
    keys.iter()
        .any(|key| value.get(*key).and_then(Value::as_bool).unwrap_or(false))
}

fn request_mentions_bypass_policy(request: &ProviderInteractionRequest) -> bool {
    const KEYS: &[&str] = &[
        "dangerously_bypass_approvals_and_sandbox",
        "dangerously_skip_permissions",
        "dangerously_bypass",
        "bypass_approvals_and_sandbox",
        "bypass_mode",
        "bypass",
    ];
    request.dangerously_bypass_approvals_and_sandbox
        || [
            request.tool_policy.as_ref(),
            request.desired_worker.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|value| KEYS.iter().any(|key| value.get(*key).is_some()))
}

fn slot_status_value(
    slot_id: &str,
    status: Option<&missiond_core::PTYAgentInfo>,
    observation: &CodexObservation,
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

fn codex_mcp_status_value(
    request: &ProviderInteractionRequest,
    slot_id: &str,
    observation: &CodexObservation,
) -> Value {
    let target = CodexProviderDriver::request_mcp_server(request);
    let screen = observation.snapshot.screen_mcp.clone();
    let target_status = screen.as_ref().and_then(|screen| {
        screen
            .servers
            .iter()
            .find(|server| server.name.eq_ignore_ascii_case(&target))
            .map(|server| server.status.clone())
    });
    json!({
        "schema": "missiond.provider-box.codex-mcp-status.v1",
        "provider": request.provider.clone().or_else(|| Some("codex_cli".to_string())),
        "engine": CliEngine::Codex,
        "slot_id": slot_id,
        "source": "codex:/mcp",
        "observed_at": chrono::Utc::now().to_rfc3339(),
        "status": screen
            .as_ref()
            .map(|screen| screen.status.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        "target_server": target,
        "target_status": target_status,
        "hot_reconnect_supported": false,
        "restart_required_for_reconnect": true,
        "screen_mcp": screen,
    })
}

impl CodexUsageLines {
    fn as_text(&self) -> String {
        format!("{}\n{}", self.five_hour, self.weekly)
    }

    fn model_quotas(&self) -> Vec<ProviderModelUsage> {
        vec![
            ProviderModelUsage {
                model: "5h limit".to_string(),
                percent: codex_usage_percent(&self.five_hour),
                status: Some(self.five_hour.clone()),
            },
            ProviderModelUsage {
                model: "Weekly limit".to_string(),
                percent: codex_usage_percent(&self.weekly),
                status: Some(self.weekly.clone()),
            },
        ]
    }
}

fn codex_usage_snapshot(
    request: &ProviderInteractionRequest,
    slot_id: &str,
    observation: &CodexObservation,
    usage: &CodexUsageLines,
) -> ProviderUsageSnapshot {
    ProviderUsageSnapshot {
        schema: "missiond.provider-usage-snapshot.v1".to_string(),
        snapshot_id: format!("usage-{}", uuid::Uuid::new_v4().simple()),
        provider: request
            .provider
            .clone()
            .or_else(|| Some("codex_cli".to_string())),
        engine: CliEngine::Codex,
        slot_id: Some(slot_id.to_string()),
        account_ref: codex_status_field(&observation.text, "Account"),
        model: codex_status_field(&observation.text, "Model")
            .or_else(|| {
                observation
                    .snapshot
                    .screen_identity
                    .as_ref()
                    .and_then(|identity| identity.current_model.clone())
            })
            .or_else(|| request.model.clone()),
        observed_at: chrono::Utc::now().to_rfc3339(),
        status: ProviderUsageStatus::Exact,
        remaining: None,
        limit: None,
        reset_at: None,
        source: Some("codex:/status".to_string()),
        confidence: observation.snapshot.confidence as f32,
        block_kind: None,
        model_quotas: usage.model_quotas(),
        diagnostics: Vec::new(),
    }
}

fn codex_unknown_usage_snapshot(
    request: &ProviderInteractionRequest,
    slot_id: &str,
    observation: Option<&CodexObservation>,
    message: &str,
) -> ProviderUsageSnapshot {
    ProviderUsageSnapshot {
        schema: "missiond.provider-usage-snapshot.v1".to_string(),
        snapshot_id: format!("usage-{}", uuid::Uuid::new_v4().simple()),
        provider: request
            .provider
            .clone()
            .or_else(|| Some("codex_cli".to_string())),
        engine: CliEngine::Codex,
        slot_id: Some(slot_id.to_string()),
        account_ref: None,
        model: request.model.clone(),
        observed_at: chrono::Utc::now().to_rfc3339(),
        status: ProviderUsageStatus::Unknown,
        remaining: None,
        limit: None,
        reset_at: None,
        source: Some("codex:/status".to_string()),
        confidence: observation
            .map(|observation| observation.snapshot.confidence as f32)
            .unwrap_or(0.0),
        block_kind: observation.and_then(|observation| observation.snapshot.blocked_kind.clone()),
        model_quotas: Vec::new(),
        diagnostics: vec![ProviderBoxDiagnostic::warning(
            DIAG_USAGE_UNKNOWN,
            message,
            json!({
                "slot_id": slot_id,
                "reason": observation.map(|observation| observation.snapshot.reason.clone()),
            }),
        )],
    }
}

fn extract_codex_usage_lines(text: &str) -> Option<CodexUsageLines> {
    let status_slice = text
        .rfind("/status")
        .map(|index| &text[index..])
        .unwrap_or(text);
    let mut five_hour = None;
    let mut weekly = None;

    for raw_line in status_slice.lines() {
        let line = clean_codex_status_line(raw_line);
        if line.starts_with("GPT-") && line.contains("limit:") && five_hour.is_some() {
            break;
        }
        if line.starts_with("5h limit:") && five_hour.is_none() {
            five_hour = Some(line);
            continue;
        }
        if line.starts_with("Weekly limit:") && weekly.is_none() {
            weekly = Some(line);
            continue;
        }
        if five_hour.is_some() && weekly.is_some() {
            break;
        }
    }

    Some(CodexUsageLines {
        five_hour: five_hour?,
        weekly: weekly?,
    })
}

fn clean_codex_status_line(line: &str) -> String {
    line.trim().trim_matches('│').trim().to_string()
}

fn codex_usage_percent(line: &str) -> Option<u8> {
    let prefix = line.split("% left").next()?;
    let digits = prefix
        .chars()
        .rev()
        .skip_while(|ch| ch.is_whitespace())
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    let digits = digits.chars().rev().collect::<String>();
    digits.parse::<u8>().ok().map(|value| value.min(100))
}

fn codex_status_field(text: &str, field: &str) -> Option<String> {
    let prefix = format!("{field}:");
    let status_slice = text
        .rfind("/status")
        .map(|index| &text[index..])
        .unwrap_or(text);
    status_slice.lines().find_map(|line| {
        let line = clean_codex_status_line(line);
        line.strip_prefix(&prefix)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn should_resolve_codex_startup_surface(observation: &CodexObservation) -> bool {
    is_codex_workspace_trust_prompt(observation)
        || observation.snapshot.reason == "codex:mcp_startup_running"
        || observation.snapshot.state == PtyCanonicalState::Unknown
}

fn is_ready_for_codex_text(observation: &CodexObservation) -> bool {
    observation.snapshot.state == PtyCanonicalState::Idle
        && observation.snapshot.blocked_kind.is_none()
        && observation.snapshot.reason != "session_state:Exited"
}

fn is_codex_empty_composer(observation: &CodexObservation) -> bool {
    observation
        .snapshot
        .screen_signals
        .as_ref()
        .is_some_and(|signals| signals.placeholder_visible)
}

fn is_codex_workspace_trust_prompt(observation: &CodexObservation) -> bool {
    observation.snapshot.reason == "codex:workspace_trust_prompt"
        || observation.snapshot.blocked_kind.as_deref() == Some("workspace_trust")
        || {
            let lower = observation.text.to_ascii_lowercase();
            lower.contains("do you trust the contents of this directory")
                && lower.contains("yes, continue")
                && lower.contains("no, quit")
                && lower.contains("press enter to continue")
        }
}

fn selected_codex_workspace_trust_option(observation: &CodexObservation) -> CodexTrustSelection {
    for line in &observation.lines {
        let trimmed = line.trim_start();
        let selected =
            trimmed.starts_with('›') || trimmed.starts_with('>') || trimmed.starts_with('❯');
        if !selected {
            continue;
        }
        let body = trimmed
            .trim_start_matches(|ch| matches!(ch, '›' | '>' | '❯'))
            .trim_start()
            .to_ascii_lowercase();
        if body.starts_with("1. yes, continue") || body.starts_with("yes, continue") {
            return CodexTrustSelection::Continue;
        }
        if body.starts_with("2. no, quit") || body.starts_with("no, quit") {
            return CodexTrustSelection::Quit;
        }
    }
    CodexTrustSelection::Unknown
}

fn normalize_codex_permission_mode(value: &str) -> Option<CodexPermissionMode> {
    let normalized = value
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-")
        .replace(' ', "-");
    match normalized.as_str() {
        "default" => Some(CodexPermissionMode::Default),
        "auto-review" | "autoreview" | "auto" => Some(CodexPermissionMode::AutoReview),
        "full-access" | "fullaccess" | "full" => Some(CodexPermissionMode::FullAccess),
        _ => None,
    }
}

fn normalize_codex_fast_mode(value: &Value) -> Option<CodexFastMode> {
    if let Some(enabled) = value.as_bool() {
        return Some(if enabled {
            CodexFastMode::Enabled
        } else {
            CodexFastMode::Disabled
        });
    }
    let normalized = value
        .as_str()?
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-")
        .replace(' ', "-");
    match normalized.as_str() {
        "enable" | "enabled" | "on" | "true" | "fast" | "priority" => Some(CodexFastMode::Enabled),
        "disable" | "disabled" | "off" | "false" | "default" | "normal" | "standard" => {
            Some(CodexFastMode::Disabled)
        }
        _ => None,
    }
}

fn codex_current_model(observation: &CodexObservation) -> Option<String> {
    observation
        .snapshot
        .screen_identity
        .as_ref()
        .and_then(|identity| identity.current_model.clone())
}

fn codex_fast_mode_from_current_model(current_model: Option<&str>) -> Option<CodexFastMode> {
    let current_model = current_model?.trim();
    if current_model.is_empty() {
        return None;
    }
    let has_fast_token = current_model
        .split_whitespace()
        .any(|token| token.eq_ignore_ascii_case("fast"));
    Some(if has_fast_token {
        CodexFastMode::Enabled
    } else {
        CodexFastMode::Disabled
    })
}

fn codex_fast_mode(observation: &CodexObservation) -> Option<CodexFastMode> {
    codex_fast_mode_from_current_model(codex_current_model(observation).as_deref())
}

fn codex_permission_picker_modes(
    observation: &CodexObservation,
) -> Option<(Vec<CodexPermissionMode>, CodexPermissionMode)> {
    let signals = observation.snapshot.screen_signals.as_ref()?;
    if !signals.permission_picker_visible {
        return None;
    }
    let modes = signals
        .visible_permission_modes
        .iter()
        .filter_map(|mode| normalize_codex_permission_mode(mode))
        .collect::<Vec<_>>();
    let selected = signals
        .selected_permission_mode
        .as_deref()
        .and_then(normalize_codex_permission_mode)?;
    if modes.is_empty() {
        None
    } else {
        Some((modes, selected))
    }
}

fn is_codex_permission_picker_observation(observation: &CodexObservation) -> bool {
    codex_permission_picker_modes(observation).is_some()
        || observation.snapshot.blocked_kind.as_deref() == Some("permission_picker")
        || observation.snapshot.reason == "codex:permission_picker"
}

fn codex_manual_key_bytes(key: &str) -> Option<(&'static str, &'static str)> {
    let normalized = key
        .trim()
        .to_ascii_lowercase()
        .replace(['_', '-'], "")
        .replace(['+', '/'], "")
        .replace("control", "ctrl")
        .replace(' ', "");
    match normalized.as_str() {
        "enter" | "return" => Some(("enter", "\r")),
        "esc" | "escape" => Some(("escape", "\x1b")),
        "up" | "arrowup" => Some(("up", "\x1b[A")),
        "down" | "arrowdown" => Some(("down", "\x1b[B")),
        "right" | "arrowright" => Some(("right", "\x1b[C")),
        "left" | "arrowleft" => Some(("left", "\x1b[D")),
        "tab" => Some(("tab", "\t")),
        "shifttab" | "backtab" => Some(("shift-tab", "\x1b[Z")),
        "backspace" => Some(("backspace", "\x7f")),
        "delete" | "del" => Some(("delete", "\x1b[3~")),
        "ctrlc" => Some(("ctrl+c", "\x03")),
        "pageup" => Some(("pageup", "\x1b[5~")),
        "pagedown" => Some(("pagedown", "\x1b[6~")),
        "home" => Some(("home", "\x1b[H")),
        "end" => Some(("end", "\x1b[F")),
        _ => None,
    }
}

fn validate_codex_manual_text_step(
    slot_id: Option<&str>,
    text: &str,
) -> Result<(), ProviderBoxDiagnostic> {
    if text.chars().count() > CODEX_MANUAL_TEXT_LIMIT {
        return Err(ProviderBoxDiagnostic::error(
            DIAG_PROVIDER_BOX_INVALID_REQUEST,
            "Codex text PTY step exceeds the maximum length",
            json!({
                "slot_id": slot_id,
                "max_chars": CODEX_MANUAL_TEXT_LIMIT,
            }),
        ));
    }
    if text.contains('\n') || text.contains('\r') {
        return Err(ProviderBoxDiagnostic::error(
            DIAG_PROVIDER_BOX_INVALID_REQUEST,
            "Codex text PTY step must not include Enter; send text and Enter as separate steps",
            json!({
                "slot_id": slot_id,
                "rule": "text_and_enter_are_separate_observe_act_observe_steps",
            }),
        ));
    }
    Ok(())
}

fn default_codex_home() -> PathBuf {
    std::env::var("CODEX_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::var("MISSIOND_CODEX_HOME").ok().map(PathBuf::from))
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join(".codex"))
        })
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

fn codex_exec_text_args(output_file: &Path, model: &str, reasoning: Option<&str>) -> Vec<String> {
    codex_exec_args_for_kind(CodexExecTaskKind::TextOnly, output_file, model, reasoning)
}

fn codex_exec_queue_key(kind: CodexExecTaskKind, model: &str, reasoning: Option<&str>) -> String {
    format!(
        "{}:{}:{}",
        kind.provider(),
        model.trim(),
        reasoning
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("default")
    )
}

fn codex_exec_queue_max_concurrent(kind: CodexExecTaskKind, reasoning: Option<&str>) -> usize {
    if kind != CodexExecTaskKind::TextOnly {
        return CODEX_EXEC_TASK_MAX_CONCURRENT;
    }
    let reasoning = reasoning
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("default");
    if reasoning.eq_ignore_ascii_case("xhigh") {
        CODEX_EXEC_TEXT_XHIGH_MAX_CONCURRENT
    } else {
        CODEX_EXEC_TEXT_DEFAULT_MAX_CONCURRENT
    }
}

fn codex_exec_args_for_kind(
    kind: CodexExecTaskKind,
    output_file: &Path,
    model: &str,
    reasoning: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
        "exec".to_string(),
        "--json".to_string(),
        "--output-last-message".to_string(),
        output_file.display().to_string(),
        "--color".to_string(),
        "never".to_string(),
    ];
    if kind != CodexExecTaskKind::ImageGeneration {
        args.extend([
            "--ephemeral".to_string(),
            "--ignore-user-config".to_string(),
            "--ignore-rules".to_string(),
        ]);
    }
    args.extend([
        "--skip-git-repo-check".to_string(),
        "--sandbox".to_string(),
        "read-only".to_string(),
        "--model".to_string(),
        model.to_string(),
        "-c".to_string(),
        "approval_policy=\"never\"".to_string(),
        "-c".to_string(),
        "features.shell_tool=false".to_string(),
    ]);
    match kind {
        CodexExecTaskKind::TextOnly => {
            args.extend([
                "-c".to_string(),
                "web_search=\"disabled\"".to_string(),
                "-c".to_string(),
                "tools.web_search=false".to_string(),
                "-c".to_string(),
                "apps._default.enabled=false".to_string(),
                "-c".to_string(),
                "apps._default.default_tools_enabled=false".to_string(),
                "-c".to_string(),
                "tools.view_image=false".to_string(),
            ]);
        }
        CodexExecTaskKind::Research => {
            args.extend([
                "-c".to_string(),
                "web_search=\"live\"".to_string(),
                "-c".to_string(),
                "tools.web_search=true".to_string(),
                "-c".to_string(),
                "apps._default.enabled=false".to_string(),
                "-c".to_string(),
                "apps._default.default_tools_enabled=false".to_string(),
                "-c".to_string(),
                "tools.view_image=false".to_string(),
            ]);
        }
        CodexExecTaskKind::ImageGeneration => {
            args.extend([
                "-c".to_string(),
                "web_search=\"disabled\"".to_string(),
                "-c".to_string(),
                "tools.web_search=false".to_string(),
                "-c".to_string(),
                "features.image_generation=true".to_string(),
                "-c".to_string(),
                "tools.image_generation=true".to_string(),
                "-c".to_string(),
                "tools.view_image=false".to_string(),
            ]);
        }
    }
    if let Some(reasoning) = reasoning {
        args.push("-c".to_string());
        args.push(format!("model_reasoning_effort=\"{reasoning}\""));
    }
    args
}

fn normalize_codex_reasoning_effort(value: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let normalized = value.to_ascii_lowercase();
    if normalized == "default" {
        return Ok(None);
    }
    match normalized.as_str() {
        "minimal" | "low" | "medium" | "high" | "xhigh" => Ok(Some(normalized)),
        _ => Err(value.to_string()),
    }
}

fn normalize_codex_exec_model(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == CODEX_EXEC_TEXT_MODEL {
        Ok(CODEX_EXEC_TEXT_MODEL.to_string())
    } else {
        Err(value.trim().to_string())
    }
}

fn codex_exec_workspace(correlation_id: &str) -> PathBuf {
    let root = std::env::var("MISSIOND_PROVIDER_BOX_CODEX_EXEC_ROOT")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("MISSIOND_RUNTIME_DIR")
                .ok()
                .map(|root| PathBuf::from(root).join("provider-box/codex-exec-text"))
        })
        .or_else(|| {
            std::env::var("HOME").ok().map(|home| {
                PathBuf::from(home).join(".missiond/runtime/missiond/provider-box/codex-exec-text")
            })
        })
        .unwrap_or_else(|| PathBuf::from(".missiond/runtime/provider-box/codex-exec-text"));
    root.join(sanitize_path_segment(correlation_id))
}

fn codex_exec_workspace_for_kind(kind: CodexExecTaskKind, correlation_id: &str) -> PathBuf {
    if kind == CodexExecTaskKind::TextOnly {
        return codex_exec_workspace(correlation_id);
    }
    let root = std::env::var("MISSIOND_PROVIDER_BOX_CODEX_EXEC_ROOT")
        .ok()
        .map(PathBuf::from)
        .map(|root| root.join(kind.label()))
        .or_else(|| {
            std::env::var("MISSIOND_RUNTIME_DIR").ok().map(|root| {
                PathBuf::from(root)
                    .join("provider-box/codex-exec")
                    .join(kind.label())
            })
        })
        .or_else(|| {
            std::env::var("HOME").ok().map(|home| {
                PathBuf::from(home)
                    .join(".missiond/runtime/missiond/provider-box/codex-exec")
                    .join(kind.label())
            })
        })
        .unwrap_or_else(|| {
            PathBuf::from(".missiond/runtime/provider-box/codex-exec").join(kind.label())
        });
    root.join(sanitize_path_segment(correlation_id))
}

fn sanitize_path_segment(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    if sanitized.is_empty() {
        format!("corr-{}", uuid::Uuid::new_v4().simple())
    } else {
        sanitized
    }
}

async fn join_output_task(task: Option<tokio::task::JoinHandle<Vec<u8>>>) -> Vec<u8> {
    match task {
        Some(task) => task.await.unwrap_or_default(),
        None => Vec::new(),
    }
}

fn analyze_codex_exec_jsonl(stdout: &str) -> CodexExecJsonlAnalysis {
    analyze_codex_exec_jsonl_for_kind(stdout, CodexExecTaskKind::TextOnly)
}

fn analyze_codex_exec_jsonl_for_kind(
    stdout: &str,
    kind: CodexExecTaskKind,
) -> CodexExecJsonlAnalysis {
    let mut analysis = CodexExecJsonlAnalysis {
        event_count: 0,
        allowed_tool_event_count: 0,
        thread_id: None,
        final_text: None,
        violation: None,
    };
    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        analysis.event_count += 1;
        if analysis.thread_id.is_none() {
            if let Some(thread_id) = event.get("thread_id").and_then(Value::as_str) {
                analysis.thread_id = Some(thread_id.to_string());
            }
        }
        if analysis.violation.is_none() && codex_exec_event_is_tool_violation_for_kind(&event, kind)
        {
            analysis.violation = Some(event.clone());
        } else if codex_exec_event_is_allowed_tool_event_for_kind(&event, kind) {
            analysis.allowed_tool_event_count += 1;
        }
        if let Some(text) = codex_assistant_text(&event) {
            if !text.trim().is_empty() {
                analysis.final_text = Some(text);
            }
        }
    }
    analysis
}

fn codex_exec_event_is_tool_violation(event: &Value) -> bool {
    codex_exec_event_is_tool_violation_for_kind(event, CodexExecTaskKind::TextOnly)
}

fn codex_exec_event_is_tool_violation_for_kind(event: &Value, kind: CodexExecTaskKind) -> bool {
    if !codex_exec_event_is_tool_like(event) {
        return false;
    }
    let type_fields = [
        event.get("type").and_then(Value::as_str),
        event.pointer("/payload/type").and_then(Value::as_str),
        event.pointer("/item/type").and_then(Value::as_str),
    ];
    if type_fields
        .iter()
        .flatten()
        .any(|value| matches!(*value, "mcp_tool_call" | "approval_request" | "approval"))
    {
        if kind == CodexExecTaskKind::ImageGeneration
            && codex_exec_event_is_imagegen_skill_bootstrap(event)
        {
            return false;
        }
        return true;
    }
    if type_fields
        .iter()
        .flatten()
        .any(|value| matches!(*value, "command_execution"))
    {
        return true;
    }
    if kind == CodexExecTaskKind::TextOnly {
        return true;
    }

    let type_name = type_fields
        .into_iter()
        .flatten()
        .map(str::to_ascii_lowercase)
        .find(|value| !value.is_empty());
    let tool_name = codex_exec_tool_name(event).map(|value| value.to_ascii_lowercase());

    if type_name
        .as_deref()
        .is_some_and(|value| matches!(value, "command_execution"))
    {
        return true;
    }
    if tool_name
        .as_deref()
        .is_some_and(codex_exec_tool_name_is_always_forbidden)
    {
        return true;
    }
    if tool_name.is_none()
        && type_name
            .as_deref()
            .is_some_and(|value| matches!(value, "function_call" | "tool_call"))
    {
        return true;
    }

    match kind {
        CodexExecTaskKind::Research => {
            if type_name
                .as_deref()
                .is_some_and(codex_exec_type_is_research_tool)
            {
                return false;
            }
            if let Some(name) = tool_name.as_deref() {
                return !codex_exec_tool_name_is_research_allowed(name);
            }
            false
        }
        CodexExecTaskKind::ImageGeneration => {
            if type_name
                .as_deref()
                .is_some_and(codex_exec_type_is_image_tool)
            {
                return false;
            }
            if let Some(name) = tool_name.as_deref() {
                return !codex_exec_tool_name_is_image_allowed(name);
            }
            false
        }
        CodexExecTaskKind::TextOnly => true,
    }
}

fn codex_exec_event_is_allowed_tool_event_for_kind(event: &Value, kind: CodexExecTaskKind) -> bool {
    kind != CodexExecTaskKind::TextOnly
        && codex_exec_event_is_tool_like(event)
        && !codex_exec_event_is_imagegen_skill_bootstrap(event)
        && !codex_exec_event_is_tool_violation_for_kind(event, kind)
}

fn codex_exec_event_is_tool_like(event: &Value) -> bool {
    let type_fields = [
        event.get("type").and_then(Value::as_str),
        event.pointer("/payload/type").and_then(Value::as_str),
        event.pointer("/item/type").and_then(Value::as_str),
    ];
    type_fields.into_iter().flatten().any(|value| {
        matches!(
            value,
            "function_call"
                | "function_call_output"
                | "tool_call"
                | "tool_result"
                | "mcp_tool_call"
                | "approval_request"
                | "approval"
                | "command_execution"
                | "web_search_call"
                | "web_search_result"
                | "search_query"
                | "web_search"
                | "image_generation_call"
                | "image_generation_result"
                | "image_generation"
                | "image_generation_end"
        )
    }) || codex_exec_tool_name(event).is_some()
}

fn codex_exec_tool_name(event: &Value) -> Option<&str> {
    event
        .get("name")
        .or_else(|| event.pointer("/payload/name"))
        .or_else(|| event.pointer("/payload/tool_name"))
        .or_else(|| event.pointer("/payload/tool/name"))
        .or_else(|| event.pointer("/item/name"))
        .or_else(|| event.pointer("/item/tool_name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn codex_exec_tool_name_is_always_forbidden(name: &str) -> bool {
    matches!(
        name,
        "exec_command"
            | "shell_command"
            | "apply_patch"
            | "read_file"
            | "write_file"
            | "edit_file"
            | "list_files"
            | "grep"
            | "view_image"
            | "mcp"
            | "mcp_tool_call"
            | "approval_request"
            | "command_execution"
    )
}

fn codex_exec_event_is_imagegen_skill_bootstrap(event: &Value) -> bool {
    let is_node_repl = event.pointer("/item/type").and_then(Value::as_str) == Some("mcp_tool_call")
        && event.pointer("/item/server").and_then(Value::as_str) == Some("node_repl")
        && event.pointer("/item/tool").and_then(Value::as_str) == Some("js");
    if !is_node_repl {
        return false;
    }
    let title = event
        .pointer("/item/arguments/title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let code = event
        .pointer("/item/arguments/code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    title.contains("imagegen")
        && (codex_exec_imagegen_skill_read_code(code)
            || codex_exec_imagegen_skill_continuation_code(code))
}

fn codex_exec_imagegen_skill_read_code(code: &str) -> bool {
    code.contains("/.codex/skills/.system/imagegen/SKILL.md") && code.contains("readFile")
}

fn codex_exec_imagegen_skill_continuation_code(code: &str) -> bool {
    let normalized = code.split_whitespace().collect::<String>();
    let Some(inner) = normalized
        .strip_prefix("nodeRepl.write(")
        .and_then(|value| value.strip_suffix(");"))
    else {
        return false;
    };
    let Some((var_name, range)) = inner.split_once(".slice(") else {
        return false;
    };
    if !var_name.starts_with("skillText")
        || !var_name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return false;
    }
    let Some(range) = range.strip_suffix(')') else {
        return false;
    };
    let Some((start, end)) = range.split_once(',') else {
        return false;
    };
    !start.is_empty()
        && !end.is_empty()
        && start.chars().all(|ch| ch.is_ascii_digit())
        && end.chars().all(|ch| ch.is_ascii_digit())
}

fn codex_exec_tool_name_is_research_allowed(name: &str) -> bool {
    matches!(
        name,
        "web_search" | "web.run" | "web_search_call" | "search_query" | "search" | "browser.search"
    )
}

fn codex_exec_type_is_research_tool(value: &str) -> bool {
    matches!(
        value,
        "web_search" | "web_search_call" | "web_search_result" | "search_query"
    )
}

fn codex_exec_tool_name_is_image_allowed(name: &str) -> bool {
    matches!(
        name,
        "image_gen"
            | "image_generation"
            | "image_generation_call"
            | "generate_image"
            | "create_image"
            | "openai_image_generation"
            | "imagegen"
            | "images.generate"
    )
}

fn codex_exec_type_is_image_tool(value: &str) -> bool {
    matches!(
        value,
        "image_generation"
            | "image_generation_call"
            | "image_generation_result"
            | "image_generation_end"
            | "image_gen_call"
    )
}

#[derive(Debug, Clone)]
struct CodexSessionIdentityCandidate {
    session_id: String,
    rollout_path: String,
    cwd: Option<String>,
    identity_source: String,
    confidence: String,
    modified_ms: i64,
}

async fn find_latest_codex_session_identity(
    codex_home: &Path,
    observation: &CodexObservation,
    status: Option<&missiond_core::PTYAgentInfo>,
) -> Option<CodexSessionIdentityCandidate> {
    let candidates = codex_workspace_candidate_strings(observation);
    if candidates.is_empty() {
        return None;
    }
    let roots = [
        codex_home.join("sessions"),
        codex_home.join("archived_sessions"),
    ];
    let started_at = status.and_then(|info| info.started_at);
    tokio::task::spawn_blocking(move || {
        let mut files = Vec::new();
        for root in roots {
            collect_jsonl_files(&root, &mut files);
        }
        files.sort_by(|a, b| modified_at(b).cmp(&modified_at(a)));
        files.truncate(200);
        let mut matches = files
            .iter()
            .filter_map(|path| extract_codex_session_identity_candidate(path))
            .filter(|candidate| {
                candidate
                    .cwd
                    .as_ref()
                    .is_some_and(|cwd| candidates.iter().any(|candidate| candidate == cwd))
            })
            .collect::<Vec<_>>();
        if matches.is_empty() {
            return None;
        }
        matches.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
        if let Some(started_at) = started_at {
            if let Some(candidate) = matches
                .iter()
                .find(|candidate| candidate.modified_ms + 120_000 >= started_at)
                .cloned()
            {
                return Some(CodexSessionIdentityCandidate {
                    confidence: "slot_started_workspace_rollout".to_string(),
                    ..candidate
                });
            }
        }
        matches
            .into_iter()
            .next()
            .map(|candidate| CodexSessionIdentityCandidate {
                confidence: "workspace_latest_rollout".to_string(),
                ..candidate
            })
    })
    .await
    .ok()
    .flatten()
}

fn extract_codex_session_identity_candidate(path: &Path) -> Option<CodexSessionIdentityCandidate> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut fallback_session_id = codex_session_id_from_rollout_filename(path);
    for line in reader.lines().take(20).map_while(Result::ok) {
        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if event.get("type").and_then(Value::as_str) != Some("session_meta") {
            continue;
        }
        let session_id = event
            .pointer("/payload/id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| fallback_session_id.take())?;
        let cwd = event
            .pointer("/payload/cwd")
            .and_then(Value::as_str)
            .map(str::to_string);
        return Some(CodexSessionIdentityCandidate {
            session_id,
            rollout_path: path.display().to_string(),
            cwd,
            identity_source: "codex_rollout_session_meta_jsonl".to_string(),
            confidence: "workspace_rollout".to_string(),
            modified_ms: system_time_ms(modified_at(path)),
        });
    }
    let session_id = fallback_session_id?;
    Some(CodexSessionIdentityCandidate {
        session_id,
        rollout_path: path.display().to_string(),
        cwd: None,
        identity_source: "codex_rollout_filename".to_string(),
        confidence: "filename_only".to_string(),
        modified_ms: system_time_ms(modified_at(path)),
    })
}

fn codex_session_id_from_rollout_filename(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let tail = stem
        .chars()
        .rev()
        .take(36)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    if uuid::Uuid::parse_str(&tail).is_ok() {
        Some(tail)
    } else {
        None
    }
}

fn codex_workspace_candidate_strings(observation: &CodexObservation) -> Vec<String> {
    let mut values = Vec::new();
    for candidate in codex_workspace_candidates(observation) {
        push_unique_workspace_string(&mut values, candidate.display().to_string());
        if let Ok(canonical) = candidate.canonicalize() {
            push_unique_workspace_string(&mut values, canonical.display().to_string());
        }
    }
    values
}

fn codex_workspace_candidates(observation: &CodexObservation) -> Vec<PathBuf> {
    let mut values = Vec::new();
    if let Some(cwd) = observation
        .snapshot
        .screen_identity
        .as_ref()
        .and_then(|identity| identity.cwd.as_deref())
    {
        push_unique_workspace_path(&mut values, expand_provider_cwd(cwd));
    }
    for cwd in codex_cwd_candidates_from_lines(&observation.lines) {
        push_unique_workspace_path(&mut values, expand_provider_cwd(&cwd));
    }
    values
}

fn codex_cwd_candidates_from_lines(lines: &[String]) -> Vec<String> {
    let mut values = Vec::new();
    for line in lines {
        let cleaned = line.trim();
        if let Some(rest) = cleaned.strip_prefix("directory:") {
            push_unique_workspace_string(&mut values, rest.trim().to_string());
            continue;
        }
        if cleaned.contains('·') {
            let parts = cleaned
                .split('·')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            if parts.len() >= 2 {
                let cwd = parts[1].to_string();
                if cwd.starts_with('/')
                    || cwd.starts_with("~/")
                    || cwd == "~"
                    || cwd.contains("/Projects/")
                {
                    push_unique_workspace_string(&mut values, cwd);
                }
            }
        }
    }
    values
}

fn expand_provider_cwd(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(trimmed));
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn push_unique_workspace_path(values: &mut Vec<PathBuf>, path: PathBuf) {
    if !values.iter().any(|existing| existing == &path) {
        values.push(path);
    }
}

fn push_unique_workspace_string(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn system_time_ms(value: std::time::SystemTime) -> i64 {
    value
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn collect_jsonl_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, out);
        } else if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

fn modified_at(path: &Path) -> std::time::SystemTime {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
}

fn extract_correlated_rollout(path: &Path, correlation_id: &str) -> Option<CodexTurnFinal> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut session_id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("codex-rollout")
        .to_string();
    let mut after_marker = false;
    let mut final_text = None;

    for line in reader.lines().map_while(Result::ok) {
        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if event.get("type").and_then(Value::as_str) == Some("session_meta") {
            if let Some(id) = event.pointer("/payload/id").and_then(Value::as_str) {
                session_id = id.to_string();
            }
        }
        if event_contains_text(&event, correlation_id) {
            after_marker = true;
            final_text = None;
            continue;
        }
        if !after_marker {
            continue;
        }
        if event.pointer("/payload/type").and_then(Value::as_str) == Some("user_message") {
            if !event_contains_text(&event, correlation_id) {
                after_marker = false;
                final_text = None;
            }
            continue;
        }
        if let Some(text) = codex_assistant_text(&event) {
            if !text.trim().is_empty() {
                final_text = Some(text);
            }
        }
    }

    final_text.map(|text| CodexTurnFinal {
        session_id,
        rollout_path: path.display().to_string(),
        final_text: text,
    })
}

fn event_contains_text(event: &Value, needle: &str) -> bool {
    value_text(event).contains(needle)
}

fn codex_assistant_text(event: &Value) -> Option<String> {
    if event.pointer("/item/type").and_then(Value::as_str) == Some("agent_message") {
        return event
            .pointer("/item/text")
            .and_then(Value::as_str)
            .or_else(|| event.pointer("/item/message").and_then(Value::as_str))
            .map(str::to_string);
    }
    match event.pointer("/payload/type").and_then(Value::as_str) {
        Some("task_complete") => event
            .pointer("/payload/last_agent_message")
            .and_then(Value::as_str)
            .or_else(|| event.pointer("/payload/message").and_then(Value::as_str))
            .map(str::to_string),
        Some("agent_message") => event
            .pointer("/payload/message")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Array(items) => items.iter().map(value_text).collect::<Vec<_>>().join("\n"),
        Value::Object(map) => map.values().map(value_text).collect::<Vec<_>>().join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn extract_image_generation_rollout(
    path: &Path,
    thread_id: &str,
    codex_home: &Path,
) -> Option<CodexImageGenerationEvidence> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut session_id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("codex-rollout")
        .to_string();
    let mut matched = path
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.contains(thread_id));
    let mut final_text = None;
    let mut image_paths = Vec::new();
    let mut revised_prompts = Vec::new();
    let mut image_event_count = 0;

    for line in reader.lines().map_while(Result::ok) {
        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if event.get("type").and_then(Value::as_str) == Some("session_meta") {
            if let Some(id) = event.pointer("/payload/id").and_then(Value::as_str) {
                session_id = id.to_string();
                if id == thread_id {
                    matched = true;
                }
            }
        }
        match event.pointer("/payload/type").and_then(Value::as_str) {
            Some("image_generation_end") | Some("image_generation_call") => {
                image_event_count += 1;
                if let Some(path) = event.pointer("/payload/saved_path").and_then(Value::as_str) {
                    if codex_generated_png_is_valid(codex_home, path) {
                        image_paths.push(path.to_string());
                    }
                }
                if let Some(prompt) = event
                    .pointer("/payload/revised_prompt")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                {
                    revised_prompts.push(prompt.to_string());
                }
            }
            _ => {}
        }
        if let Some(text) = codex_assistant_text(&event) {
            if !text.trim().is_empty() {
                final_text = Some(text);
            }
        }
    }

    image_paths.sort();
    image_paths.dedup();
    revised_prompts.dedup();
    if matched && !image_paths.is_empty() {
        Some(CodexImageGenerationEvidence {
            session_id,
            rollout_path: path.display().to_string(),
            image_paths,
            revised_prompts,
            final_text,
            image_event_count,
        })
    } else {
        None
    }
}

fn codex_generated_png_is_valid(codex_home: &Path, value: &str) -> bool {
    let path = Path::new(value);
    if path.extension().and_then(|value| value.to_str()) != Some("png") {
        return false;
    }
    let Ok(canonical) = fs::canonicalize(path) else {
        return false;
    };
    let Ok(root) = fs::canonicalize(codex_home.join("generated_images")) else {
        return false;
    };
    if !canonical.starts_with(root) {
        return false;
    }
    let Ok(metadata) = fs::metadata(&canonical) else {
        return false;
    };
    if metadata.len() < 1024 {
        return false;
    }
    let Ok(mut file) = fs::File::open(&canonical) else {
        return false;
    };
    let mut signature = [0_u8; 8];
    file.read_exact(&mut signature).is_ok() && signature == *b"\x89PNG\r\n\x1a\n"
}

fn image_import_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1/images") {
        format!("{base}/import")
    } else {
        format!("{base}/v1/images/import")
    }
}

fn media_tenant_id_from_request(request: &ProviderInteractionRequest) -> Option<String> {
    json_string_at(
        request.output_contract.as_ref(),
        &["/tenant_id", "/owner/tenant_id"],
    )
    .or_else(|| {
        json_string_at(
            request.write_scope.as_ref(),
            &["/tenant_id", "/owner/tenant_id"],
        )
    })
    .or_else(|| {
        json_string_at(
            request.desired_worker.as_ref(),
            &["/tenant_id", "/owner/tenant_id"],
        )
    })
    .or_else(|| std::env::var("XJP_MEDIA_DEFAULT_TENANT_ID").ok())
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

fn media_owner_user_id_from_request(request: &ProviderInteractionRequest) -> Option<String> {
    json_string_at(
        request.output_contract.as_ref(),
        &["/owner_user_id", "/user_id", "/owner/user_id"],
    )
    .or_else(|| {
        json_string_at(
            request.write_scope.as_ref(),
            &["/owner_user_id", "/user_id", "/owner/user_id"],
        )
    })
    .or_else(|| {
        json_string_at(
            request.desired_worker.as_ref(),
            &["/owner_user_id", "/user_id", "/owner/user_id"],
        )
    })
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

fn json_string_at(value: Option<&Value>, paths: &[&str]) -> Option<String> {
    let value = value?;
    paths
        .iter()
        .find_map(|path| value.pointer(path).and_then(Value::as_str))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_permission_mode_aliases_normalize() {
        assert_eq!(
            normalize_codex_permission_mode("default"),
            Some(CodexPermissionMode::Default)
        );
        assert_eq!(
            normalize_codex_permission_mode("auto_review"),
            Some(CodexPermissionMode::AutoReview)
        );
        assert_eq!(
            normalize_codex_permission_mode("full access"),
            Some(CodexPermissionMode::FullAccess)
        );
        assert_eq!(normalize_codex_permission_mode("danger"), None);
    }

    #[test]
    fn codex_fast_mode_aliases_normalize() {
        assert_eq!(
            normalize_codex_fast_mode(&json!(true)),
            Some(CodexFastMode::Enabled)
        );
        assert_eq!(
            normalize_codex_fast_mode(&json!("priority")),
            Some(CodexFastMode::Enabled)
        );
        assert_eq!(
            normalize_codex_fast_mode(&json!("off")),
            Some(CodexFastMode::Disabled)
        );
        assert_eq!(normalize_codex_fast_mode(&json!("turbo")), None);
    }

    #[test]
    fn codex_fast_mode_reads_current_model_suffix() {
        assert_eq!(
            codex_fast_mode_from_current_model(Some("gpt-5.5 xhigh fast")),
            Some(CodexFastMode::Enabled)
        );
        assert_eq!(
            codex_fast_mode_from_current_model(Some("gpt-5.5 xhigh")),
            Some(CodexFastMode::Disabled)
        );
        assert_eq!(codex_fast_mode_from_current_model(None), None);
    }

    #[test]
    fn codex_provider_session_identity_reads_rollout_filename_suffix() {
        let path = PathBuf::from(
            "/tmp/rollout-2026-06-01T21-28-19-019e835e-f90d-7681-afc5-bc92092aeade.jsonl",
        );

        assert_eq!(
            codex_session_id_from_rollout_filename(&path).as_deref(),
            Some("019e835e-f90d-7681-afc5-bc92092aeade")
        );
    }

    #[tokio::test]
    async fn codex_provider_session_identity_matches_rollout_cwd() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("workspace");
        let sessions = dir
            .path()
            .join("sessions")
            .join("2026")
            .join("06")
            .join("01");
        std::fs::create_dir_all(&workspace).expect("workspace");
        std::fs::create_dir_all(&sessions).expect("sessions");
        let rollout =
            sessions.join("rollout-2026-06-01T21-28-19-019e835e-f90d-7681-afc5-bc92092aeade.jsonl");
        std::fs::write(
            &rollout,
            serde_json::json!({
                "type": "session_meta",
                "timestamp": "2026-06-01T13:28:19.000Z",
                "payload": {
                    "id": "019e835e-f90d-7681-afc5-bc92092aeade",
                    "cwd": workspace.display().to_string(),
                    "cli_version": "0.135.0-alpha.1"
                }
            })
            .to_string(),
        )
        .expect("rollout");
        let lines = vec![
            ">_ OpenAI Codex (v0.135.0-alpha.1)".to_string(),
            "model:     gpt-5.5 xhigh   /model to change".to_string(),
            format!("directory: {}", workspace.display()),
            format!("gpt-5.5 xhigh · {}", workspace.display()),
        ];
        let observation = CodexObservation {
            text: lines.join("\n"),
            snapshot: recognize_screen(CliEngine::Codex, &lines, SessionState::Idle),
            lines,
        };
        assert!(codex_workspace_candidate_strings(&observation)
            .iter()
            .any(|candidate| candidate == workspace.to_str().unwrap()));

        let identity = find_latest_codex_session_identity(dir.path(), &observation, None)
            .await
            .expect("identity");

        assert_eq!(identity.session_id, "019e835e-f90d-7681-afc5-bc92092aeade");
        assert_eq!(identity.cwd.as_deref(), Some(workspace.to_str().unwrap()));
        assert_eq!(identity.identity_source, "codex_rollout_session_meta_jsonl");
    }

    #[test]
    fn rollout_extractor_prefers_task_complete_after_correlation() {
        let dir = std::env::temp_dir().join(format!(
            "missiond-codex-provider-box-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("rollout.jsonl");
        let corr = "corr-test";
        let user = json!({
            "type": "event_msg",
            "payload": {"type": "user_message", "message": format!("hello {corr}")}
        });
        let final_event = json!({
            "type": "event_msg",
            "payload": {
                "type": "task_complete",
                "last_agent_message": "final from durable rollout"
            }
        });
        fs::write(&path, format!("{user}\n{final_event}\n")).expect("write");

        let turn = extract_correlated_rollout(&path, corr).expect("turn");

        assert_eq!(turn.final_text, "final from durable rollout");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn correlated_prompt_preserves_existing_correlation() {
        let mut request = ProviderInteractionRequest::new(
            super::super::types::BoxCommand::WorkerTurn,
            CliEngine::Codex,
        );
        request.correlation_id = "corr-existing".to_string();
        request.prompt = Some("contains corr-existing already".to_string());

        assert_eq!(
            correlate_prompt(&request),
            "contains corr-existing already".to_string()
        );
    }

    #[test]
    fn codex_bypass_can_be_requested_from_top_level_or_tool_policy() {
        let mut top_level = ProviderInteractionRequest::new(
            super::super::types::BoxCommand::Status,
            CliEngine::Codex,
        );
        top_level.dangerously_bypass_approvals_and_sandbox = true;
        assert!(CodexProviderDriver::request_dangerous_bypass(&top_level));

        let mut policy = ProviderInteractionRequest::new(
            super::super::types::BoxCommand::Status,
            CliEngine::Codex,
        );
        policy.tool_policy = Some(json!({
            "bypass_approvals_and_sandbox": true,
        }));
        assert!(CodexProviderDriver::request_dangerous_bypass(&policy));
    }

    #[test]
    fn codex_default_reasoning_is_absent_when_model_profile_is_omitted() {
        let request = ProviderInteractionRequest::new(
            super::super::types::BoxCommand::Status,
            CliEngine::Codex,
        );

        assert!(request.model_profile.is_none());
    }

    #[test]
    fn codex_exec_text_args_disable_agentic_tools() {
        let output = Path::new("/tmp/missiond-codex-last.txt");
        let args = codex_exec_text_args(output, "gpt-5.5", Some("xhigh"));

        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--json" && pair[1] == "--output-last-message"));
        assert!(args.contains(&"--ignore-user-config".to_string()));
        assert!(args.contains(&"--ignore-rules".to_string()));
        assert!(args.contains(&"approval_policy=\"never\"".to_string()));
        assert!(args.contains(&"features.shell_tool=false".to_string()));
        assert!(args.contains(&"web_search=\"disabled\"".to_string()));
        assert!(args.contains(&"tools.web_search=false".to_string()));
        assert!(args.contains(&"apps._default.enabled=false".to_string()));
        assert!(args.contains(&"apps._default.default_tools_enabled=false".to_string()));
        assert!(args.contains(&"tools.view_image=false".to_string()));
        assert!(args.contains(&"model_reasoning_effort=\"xhigh\"".to_string()));
    }

    #[test]
    fn codex_exec_research_args_allow_web_search_without_shell_or_apps() {
        let output = Path::new("/tmp/missiond-codex-last.txt");
        let args = codex_exec_args_for_kind(
            CodexExecTaskKind::Research,
            output,
            "gpt-5.5",
            Some("xhigh"),
        );

        assert!(args.contains(&"features.shell_tool=false".to_string()));
        assert!(args.contains(&"approval_policy=\"never\"".to_string()));
        assert!(args.contains(&"web_search=\"live\"".to_string()));
        assert!(args.contains(&"tools.web_search=true".to_string()));
        assert!(args.contains(&"apps._default.enabled=false".to_string()));
        assert!(args.contains(&"apps._default.default_tools_enabled=false".to_string()));
        assert!(args.contains(&"tools.view_image=false".to_string()));
    }

    #[test]
    fn codex_exec_image_args_allow_image_generation_without_shell_or_search() {
        let output = Path::new("/tmp/missiond-codex-last.txt");
        let args =
            codex_exec_args_for_kind(CodexExecTaskKind::ImageGeneration, output, "gpt-5.5", None);

        assert!(!args.contains(&"--ephemeral".to_string()));
        assert!(!args.contains(&"--ignore-user-config".to_string()));
        assert!(!args.contains(&"--ignore-rules".to_string()));
        assert!(args.contains(&"features.shell_tool=false".to_string()));
        assert!(args.contains(&"approval_policy=\"never\"".to_string()));
        assert!(args.contains(&"web_search=\"disabled\"".to_string()));
        assert!(args.contains(&"tools.web_search=false".to_string()));
        assert!(!args.contains(&"apps._default.enabled=false".to_string()));
        assert!(!args.contains(&"apps._default.default_tools_enabled=false".to_string()));
        assert!(args.contains(&"features.image_generation=true".to_string()));
        assert!(args.contains(&"tools.image_generation=true".to_string()));
        assert!(args.contains(&"tools.view_image=false".to_string()));
    }

    #[test]
    fn codex_exec_text_default_reasoning_omits_override() {
        let output = Path::new("/tmp/missiond-codex-last.txt");
        let args = codex_exec_text_args(output, "gpt-5.5", None);

        assert!(!args
            .iter()
            .any(|arg| arg.starts_with("model_reasoning_effort=")));
    }

    #[test]
    fn codex_exec_queue_key_distinguishes_exported_lanes() {
        assert_eq!(
            codex_exec_queue_key(CodexExecTaskKind::TextOnly, "gpt-5.5", None),
            "codex_exec_text:gpt-5.5:default"
        );
        assert_eq!(
            codex_exec_queue_key(CodexExecTaskKind::TextOnly, "gpt-5.5", Some("xhigh")),
            "codex_exec_text:gpt-5.5:xhigh"
        );
        assert_eq!(
            codex_exec_queue_key(CodexExecTaskKind::Research, "gpt-5.5", Some("xhigh")),
            "codex_research:gpt-5.5:xhigh"
        );
        assert_eq!(
            codex_exec_queue_key(CodexExecTaskKind::ImageGeneration, "gpt-5.5", None),
            "codex_image_generation:gpt-5.5:default"
        );
    }

    #[test]
    fn codex_exec_queue_max_concurrent_matches_export_policy() {
        assert_eq!(
            codex_exec_queue_max_concurrent(CodexExecTaskKind::TextOnly, Some("default")),
            4
        );
        assert_eq!(
            codex_exec_queue_max_concurrent(CodexExecTaskKind::TextOnly, None),
            4
        );
        assert_eq!(
            codex_exec_queue_max_concurrent(CodexExecTaskKind::TextOnly, Some("xhigh")),
            2
        );
        assert_eq!(
            codex_exec_queue_max_concurrent(CodexExecTaskKind::Research, Some("xhigh")),
            1
        );
        assert_eq!(
            codex_exec_queue_max_concurrent(CodexExecTaskKind::ImageGeneration, None),
            1
        );
    }

    #[test]
    fn codex_manual_key_bytes_are_allowlisted() {
        assert_eq!(codex_manual_key_bytes("enter"), Some(("enter", "\r")));
        assert_eq!(codex_manual_key_bytes("esc"), Some(("escape", "\x1b")));
        assert_eq!(codex_manual_key_bytes("ctrl-c"), Some(("ctrl+c", "\x03")));
        assert_eq!(codex_manual_key_bytes("down"), Some(("down", "\x1b[B")));
        assert!(codex_manual_key_bytes("f13").is_none());
    }

    #[test]
    fn codex_manual_text_step_rejects_enter() {
        let err = validate_codex_manual_text_step(Some("slot-codex-test"), "/clear\r")
            .expect_err("enter rejected");

        assert_eq!(err.code, DIAG_PROVIDER_BOX_INVALID_REQUEST);
    }

    #[test]
    fn codex_placeholder_composer_is_treated_as_empty_input() {
        use missiond_core::pty::{
            CapturedCellFlags, StyledScreenLine, StyledScreenSnapshot, StyledScreenSpan,
        };

        let styled_span = |text: &str, dim: bool, bold: bool| StyledScreenSpan {
            text: text.to_string(),
            fg: [205, 214, 244],
            bg: [30, 30, 46],
            fg_hex: "#cdd6f4".to_string(),
            bg_hex: "#1e1e2e".to_string(),
            flags: CapturedCellFlags {
                bold,
                dim,
                ..CapturedCellFlags::default()
            },
        };
        let styled_line = |spans: Vec<StyledScreenSpan>| StyledScreenLine {
            text: spans.iter().map(|span| span.text.as_str()).collect(),
            spans,
        };
        let styled_screen = StyledScreenSnapshot {
            rows: 5,
            cols: 120,
            lines: vec![
                styled_line(vec![styled_span(
                    "│ >_ OpenAI Codex (v0.135.0-alpha.1) │",
                    false,
                    false,
                )]),
                styled_line(vec![styled_span(
                    "│ model:     gpt-5.5 xhigh   /model to change │",
                    false,
                    false,
                )]),
                styled_line(vec![styled_span(
                    "│ directory: ~/Projects/missiond │",
                    false,
                    false,
                )]),
                styled_line(vec![
                    styled_span("›", false, true),
                    styled_span(" ", false, false),
                    styled_span("Improve documentation in @filename", true, false),
                ]),
                styled_line(vec![styled_span(
                    "  gpt-5.5 xhigh · ~/Projects/missiond",
                    false,
                    false,
                )]),
            ],
        };
        let lines = styled_screen
            .lines
            .iter()
            .map(|line| line.text.clone())
            .collect::<Vec<_>>();
        let snapshot =
            recognize_styled_screen(CliEngine::Codex, &styled_screen, SessionState::Idle);
        let observation = CodexObservation {
            text: lines.join("\n"),
            lines,
            snapshot,
        };

        assert!(is_ready_for_codex_text(&observation));
        assert!(is_codex_empty_composer(&observation));
    }

    #[test]
    fn codex_exited_session_is_not_ready_for_text_input() {
        let lines = vec!["/status".to_string()];
        let snapshot = recognize_screen(CliEngine::Codex, &lines, SessionState::Exited);
        let observation = CodexObservation {
            text: lines.join("\n"),
            lines,
            snapshot,
        };

        assert!(!is_ready_for_codex_text(&observation));
    }

    #[test]
    fn codex_mcp_status_value_marks_reconnect_as_restart_required() {
        let request = ProviderInteractionRequest::new(BoxCommand::McpStatus, CliEngine::Codex);
        let lines = vec![
            "🔌 MCP Tools".to_string(),
            "".to_string(),
            "  • missiond".to_string(),
            "    • Auth: Unsupported".to_string(),
            "    • Tools: mission_board_query".to_string(),
            "".to_string(),
            "› Use /skills to list available skills".to_string(),
            "  gpt-5.5 xhigh · ~/Projects/missiond".to_string(),
        ];
        let snapshot = recognize_screen(CliEngine::Codex, &lines, SessionState::Idle);
        let observation = CodexObservation {
            lines,
            text: "fixture".to_string(),
            snapshot,
        };

        let value = codex_mcp_status_value(&request, "slot-codex-test", &observation);

        assert_eq!(value["status"], "connected");
        assert_eq!(value["hot_reconnect_supported"], false);
        assert_eq!(value["restart_required_for_reconnect"], true);
    }

    #[test]
    fn codex_status_usage_extracts_current_limit_pair_only() {
        let text = r#"
› /status

Model: gpt-5.5 (reasoning xhigh, summaries auto)
Account: citrobridegroom967@gmail.com (Pro)

5h limit:                    [████░░░░░░░░░░░░░░░░] 18% left (resets 22:58)
Weekly limit:                [█████████████░░░░░░░] 66% left (resets 23:22 on 7 Jun)

GPT-5.3-Codex-Spark limit:
5h limit:                    [████████████████████] 0% left (resets tomorrow)
Weekly limit:                [████████████████████] 0% left (resets next week)
"#;

        let usage = extract_codex_usage_lines(text).expect("usage lines");

        assert_eq!(
            usage.five_hour,
            "5h limit:                    [████░░░░░░░░░░░░░░░░] 18% left (resets 22:58)"
        );
        assert_eq!(
            usage.weekly,
            "Weekly limit:                [█████████████░░░░░░░] 66% left (resets 23:22 on 7 Jun)"
        );
        assert_eq!(usage.model_quotas()[0].percent, Some(18));
        assert_eq!(usage.model_quotas()[1].percent, Some(66));
    }

    #[test]
    fn codex_exec_text_model_policy_only_allows_gpt_55() {
        assert_eq!(
            normalize_codex_exec_model(" gpt-5.5 ").expect("gpt-5.5"),
            "gpt-5.5"
        );
        assert!(normalize_codex_exec_model("gpt-5.4").is_err());
        assert!(normalize_codex_exec_model("gpt-5.5 xhigh").is_err());
    }

    #[test]
    fn codex_exec_text_validation_rejects_non_gpt_55_model() {
        let mut request = ProviderInteractionRequest::pure_text(CliEngine::Codex, "hello");
        request.command = super::super::types::BoxCommand::PureTextSingleTurn;
        request.provider = Some(CODEX_EXEC_TEXT_PROVIDER.to_string());
        request.model = Some("gpt-5.4".to_string());
        let mut result = ProviderBoxResult::base(&request, ProviderBoxStatus::Unknown);

        assert!(
            CodexProviderDriver::validate_pure_text_exec_request(&request, &mut result).is_none()
        );
        assert_eq!(result.status, ProviderBoxStatus::Failed);
        assert_eq!(
            result.diagnostics[0].message,
            "Codex exec text-only source only exports GPT-5.5"
        );
    }

    #[test]
    fn codex_exec_jsonl_analysis_detects_tool_violation() {
        let stdout = json!({
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "shell_command",
                "arguments": "{}"
            }
        })
        .to_string();

        let analysis = analyze_codex_exec_jsonl(&stdout);

        assert_eq!(analysis.event_count, 1);
        assert!(analysis.violation.is_some());
    }

    #[test]
    fn codex_exec_jsonl_analysis_detects_command_execution_violation() {
        let stdout = json!({
            "type": "item.completed",
            "item": {
                "type": "command_execution",
                "command": "pwd",
                "status": "completed"
            }
        })
        .to_string();

        let analysis = analyze_codex_exec_jsonl_for_kind(&stdout, CodexExecTaskKind::Research);

        assert!(analysis.violation.is_some());
    }

    #[test]
    fn codex_exec_image_analysis_allows_only_imagegen_skill_bootstrap_mcp() {
        let bootstrap = json!({
            "type": "item.completed",
            "item": {
                "type": "mcp_tool_call",
                "server": "node_repl",
                "tool": "js",
                "arguments": {
                    "title": "Read imagegen skill",
                    "code": "var fs = await import('node:fs/promises'); await fs.readFile('/Users/jinchen/.codex/skills/.system/imagegen/SKILL.md', 'utf8');"
                }
            }
        })
        .to_string();

        let image_analysis =
            analyze_codex_exec_jsonl_for_kind(&bootstrap, CodexExecTaskKind::ImageGeneration);
        assert!(image_analysis.violation.is_none());
        assert_eq!(image_analysis.allowed_tool_event_count, 0);

        let research_analysis =
            analyze_codex_exec_jsonl_for_kind(&bootstrap, CodexExecTaskKind::Research);
        assert!(research_analysis.violation.is_some());
    }

    #[test]
    fn codex_exec_image_analysis_allows_imagegen_skill_continuation_mcp() {
        let continuation = json!({
            "type": "item.started",
            "item": {
                "type": "mcp_tool_call",
                "server": "node_repl",
                "tool": "js",
                "arguments": {
                    "title": "Read imagegen skill continuation",
                    "code": "nodeRepl.write(skillText1.slice(4000, 8000));"
                }
            }
        })
        .to_string();

        let image_analysis =
            analyze_codex_exec_jsonl_for_kind(&continuation, CodexExecTaskKind::ImageGeneration);
        assert!(image_analysis.violation.is_none());
        assert_eq!(image_analysis.allowed_tool_event_count, 0);

        let research_analysis =
            analyze_codex_exec_jsonl_for_kind(&continuation, CodexExecTaskKind::Research);
        assert!(research_analysis.violation.is_some());
    }

    #[test]
    fn codex_exec_image_analysis_rejects_arbitrary_node_repl_imagegen_mcp() {
        let arbitrary = json!({
            "type": "item.started",
            "item": {
                "type": "mcp_tool_call",
                "server": "node_repl",
                "tool": "js",
                "arguments": {
                    "title": "Read imagegen skill continuation",
                    "code": "nodeRepl.write(process.env.HOME);"
                }
            }
        })
        .to_string();

        let image_analysis =
            analyze_codex_exec_jsonl_for_kind(&arbitrary, CodexExecTaskKind::ImageGeneration);
        assert!(image_analysis.violation.is_some());
    }

    #[test]
    fn codex_exec_research_analysis_allows_search_and_rejects_shell() {
        let search_event = json!({
            "type": "item.completed",
            "item": {
                "type": "web_search",
                "query": "weather: Beijing"
            }
        })
        .to_string();
        let analysis =
            analyze_codex_exec_jsonl_for_kind(&search_event, CodexExecTaskKind::Research);
        assert_eq!(analysis.allowed_tool_event_count, 1);
        assert!(analysis.violation.is_none());

        let shell_event = json!({
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "shell_command"
            }
        })
        .to_string();
        let analysis = analyze_codex_exec_jsonl_for_kind(&shell_event, CodexExecTaskKind::Research);
        assert!(analysis.violation.is_some());
    }

    #[test]
    fn codex_exec_image_analysis_allows_image_tool_and_rejects_file_read() {
        let image_event = json!({
            "type": "item.completed",
            "item": {
                "type": "image_generation",
                "prompt": "red circle"
            }
        })
        .to_string();
        let analysis =
            analyze_codex_exec_jsonl_for_kind(&image_event, CodexExecTaskKind::ImageGeneration);
        assert_eq!(analysis.allowed_tool_event_count, 1);
        assert!(analysis.violation.is_none());

        let read_event = json!({
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "read_file"
            }
        })
        .to_string();
        let analysis =
            analyze_codex_exec_jsonl_for_kind(&read_event, CodexExecTaskKind::ImageGeneration);
        assert!(analysis.violation.is_some());
    }

    #[test]
    fn codex_exec_jsonl_analysis_accepts_plain_final() {
        let stdout = json!({
            "type": "event_msg",
            "payload": {
                "type": "task_complete",
                "last_agent_message": "plain final"
            }
        })
        .to_string();

        let analysis = analyze_codex_exec_jsonl(&stdout);

        assert_eq!(analysis.final_text.as_deref(), Some("plain final"));
        assert!(analysis.violation.is_none());
    }

    #[test]
    fn codex_exec_jsonl_analysis_accepts_item_completed_agent_message_final() {
        let stdout = [
            json!({
                "type": "item.completed",
                "item": {
                    "id": "item_1",
                    "type": "agent_message",
                    "text": "thinking preface"
                }
            })
            .to_string(),
            json!({
                "type": "item.completed",
                "item": {
                    "id": "item_2",
                    "type": "web_search",
                    "query": "weather: Beijing"
                }
            })
            .to_string(),
            json!({
                "type": "item.completed",
                "item": {
                    "id": "item_3",
                    "type": "agent_message",
                    "text": "durable final"
                }
            })
            .to_string(),
        ]
        .join("\n");

        let analysis = analyze_codex_exec_jsonl_for_kind(&stdout, CodexExecTaskKind::Research);

        assert_eq!(analysis.event_count, 3);
        assert_eq!(analysis.allowed_tool_event_count, 1);
        assert_eq!(analysis.final_text.as_deref(), Some("durable final"));
        assert!(analysis.violation.is_none());
    }

    #[test]
    fn codex_image_generation_rollout_extracts_saved_png_without_returning_local_path() {
        let dir = std::env::temp_dir().join(format!(
            "missiond-codex-image-rollout-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let codex_home = dir.join("codex-home");
        let thread_id = "019e82df-test";
        let image_dir = codex_home.join("generated_images").join(thread_id);
        fs::create_dir_all(&image_dir).expect("image dir");
        let image_path = image_dir.join("ig_test.png");
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.resize(2048, 0);
        fs::write(&image_path, png).expect("png");
        let rollout_path = dir.join(format!("rollout-2026-06-01T19-08-54-{thread_id}.jsonl"));
        let session = json!({
            "type": "session_meta",
            "payload": {"id": thread_id}
        });
        let image_end = json!({
            "type": "event_msg",
            "payload": {
                "type": "image_generation_end",
                "saved_path": image_path.display().to_string(),
                "revised_prompt": "green apple"
            }
        });
        let bad_final = json!({
            "type": "event_msg",
            "payload": {
                "type": "agent_message",
                "message": "IMAGE_FAILED imagegen generated an image but did not expose a path"
            }
        });
        fs::write(
            &rollout_path,
            format!("{session}\n{image_end}\n{bad_final}\n"),
        )
        .expect("rollout");

        let evidence = extract_image_generation_rollout(&rollout_path, thread_id, &codex_home)
            .expect("evidence");

        assert_eq!(evidence.image_event_count, 1);
        assert_eq!(evidence.image_paths, vec![image_path.display().to_string()]);
        assert_eq!(
            image_import_url("https://images.xiaojins.com"),
            "https://images.xiaojins.com/v1/images/import"
        );
        assert_eq!(
            image_import_url("https://images.xiaojins.com/v1/images/"),
            "https://images.xiaojins.com/v1/images/import"
        );
        let _ = fs::remove_dir_all(dir);
    }
}
