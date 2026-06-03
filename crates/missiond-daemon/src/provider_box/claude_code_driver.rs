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
    DIAG_PROVIDER_MCP_RECONNECT_UNVERIFIED, DIAG_PROVIDER_MCP_STATUS_UNAVAILABLE,
    DIAG_PROVIDER_TEXT_ONLY_VIOLATION, DIAG_PROVIDER_UPSTREAM_UNAVAILABLE,
};

const DEFAULT_CLAUDE_CODE_SLOT: &str = "slot-claude-code-default";
const CLAUDE_CODE_TEXT_PROVIDER: &str = "claude_code_text";
const CLAUDE_CODE_DEEP_RESEARCH_PROVIDER: &str = "claude_code_deep_research";
const CLAUDE_CODE_DEEP_RESEARCH_MODEL_ID: &str = "claude-code-deep-research-opus-4-8-xhigh";
const CLAUDE_CODE_DEEP_RESEARCH_PROVIDER_MODEL: &str = "claude-opus-4-8";
const CLAUDE_CODE_DEEP_RESEARCH_MODEL_PROFILE: &str = "xhigh";
const CLAUDE_CODE_TEXT_RUNTIME_DIR: &str = ".missiond/runtime/claude-code-text-only";
const CLAUDE_CODE_DEEP_RESEARCH_RUNTIME_DIR: &str = ".missiond/runtime/claude-code-deep-research";
const CLAUDE_CODE_TEXT_EMPTY_MCP_CONFIG: &str = "empty-mcp-config.json";
const CLAUDE_CODE_TEXT_MAX_CONCURRENT: usize = 1;
const CLAUDE_CODE_DEEP_RESEARCH_MAX_CONCURRENT: usize = 1;
const OBSERVE_SETTLE_MS: u64 = 350;
const OBSERVE_STABLE_POLL_MS: u64 = 120;
const OBSERVE_STABLE_MAX_MS: u64 = 1_000;
const DURABLE_FINAL_IDLE_GRACE_SECS: u64 = 4;
const PROMPT_SUBMISSION_IDLE_GRACE_SECS: u64 = 30;
const OUTPUT_CONTRACT_FILE_MIN_BYTES: u64 = 40;
const OUTPUT_CONTRACT_FILE_STABLE_SECS: u64 = 2;
const OUTPUT_CONTRACT_FILE_IDLE_CONTINUE_GRACE_SECS: u64 = 4;
const OUTPUT_CONTRACT_FILE_MAX_CONTINUE_ATTEMPTS: usize = 1;

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
    session_state: SessionState,
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
    upstream_error: Option<Value>,
}

#[derive(Debug, Clone, Default)]
struct ClaudeCodeDeepResearchAnalysis {
    jsonl_path: Option<PathBuf>,
    line_count: usize,
    launch: Option<ClaudeCodeWorkflowLaunch>,
    upstream_error: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClaudeCodeWorkflowLaunch {
    task_id: Option<String>,
    run_id: String,
    transcript_dir: PathBuf,
    script_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
struct ClaudeCodeWorkflowJournalReport {
    journal_path: Option<PathBuf>,
    line_count: usize,
    report: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClaudeCodeMcpServerEntry {
    name: String,
    selected: bool,
    status: String,
    connected: bool,
    auth: Option<String>,
    tools_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClaudeCodePromptAuthorization {
    kind: String,
    target: Option<String>,
    selected_option_index: Option<usize>,
    selected_option: Option<String>,
    allow_option_index: Option<usize>,
    allow_option: Option<String>,
    visible_options: Vec<String>,
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
            .or_else(|| {
                request.tool_policy.as_ref().and_then(|policy| {
                    policy
                        .get("mcp_server")
                        .or_else(|| policy.get("server"))
                        .and_then(Value::as_str)
                })
            })
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("missiond")
            .to_string()
    }

    fn request_requires_mcp(request: &ProviderInteractionRequest) -> bool {
        !request.no_mcp
            && (bool_any(
                request.tool_policy.as_ref(),
                &["require_mcp", "require_mcp_server"],
            ) || request.tool_policy.as_ref().is_some_and(|policy| {
                policy.get("mcp_server").is_some() || policy.get("server").is_some()
            }) || request.desired_worker.as_ref().is_some_and(|worker| {
                worker.get("mcp_server").is_some() || worker.get("server").is_some()
            }))
    }

    fn request_provider_session_id(request: &ProviderInteractionRequest) -> Option<String> {
        request
            .desired_worker
            .as_ref()
            .and_then(|worker| {
                worker
                    .get("provider_session_id")
                    .or_else(|| worker.get("session_id"))
                    .or_else(|| worker.get("claude_session_id"))
                    .and_then(Value::as_str)
            })
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
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

    fn request_is_deep_research(request: &ProviderInteractionRequest) -> bool {
        request.command == super::types::BoxCommand::Research
            && request.provider.as_deref().is_some_and(|provider| {
                provider.eq_ignore_ascii_case(CLAUDE_CODE_DEEP_RESEARCH_PROVIDER)
                    || provider.eq_ignore_ascii_case("claude-code-deep-research")
            })
    }

    fn validate_deep_research_request(
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> bool {
        let provider = request
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(CLAUDE_CODE_DEEP_RESEARCH_PROVIDER);
        if !provider.eq_ignore_ascii_case(CLAUDE_CODE_DEEP_RESEARCH_PROVIDER)
            && !provider.eq_ignore_ascii_case("claude-code-deep-research")
        {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode deep-research export requires provider=claude_code_deep_research",
                json!({
                    "provider": provider,
                    "allowed_provider": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER,
                }),
            ));
            return false;
        }
        if request.slot_id.is_some() {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode deep-research export does not accept external slot_id",
                json!({
                    "rule": "provider-box generates a private ephemeral PTY slot and hides it from callers",
                }),
            ));
            return false;
        }
        if request
            .prompt
            .as_ref()
            .is_none_or(|prompt| prompt.trim().is_empty())
        {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode deep-research export requires a non-empty research prompt",
                json!({
                    "provider": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER,
                }),
            ));
            return false;
        }
        if !request.attachments.is_empty() {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode deep-research export does not accept attachments",
                json!({
                    "attachments": request.attachments.len(),
                }),
            ));
            return false;
        }
        true
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
            provider_session_id: Self::request_provider_session_id(request),
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
            session_state: state,
            snapshot,
        }
    }

    fn observations_equivalent(
        left: &ClaudeCodeObservation,
        right: &ClaudeCodeObservation,
    ) -> bool {
        left.text == right.text
            && left.session_state == right.session_state
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
        if let Some(text) = claude_code_composer_blocking_text(&observation) {
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
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            return None;
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
        if let Some(text) = claude_code_composer_blocking_text(&observation) {
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
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            return None;
        }
        Some(observation)
    }

    async fn ensure_ready_for_mcp_command(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) -> Option<ClaudeCodeObservation> {
        let mut observation = self.observe(slot_id).await;
        if is_claude_code_prompt_authorization_prompt(&observation) {
            observation = self
                .accept_prompt_authorization_locked(result, slot_id, observation)
                .await?;
        }
        for _ in 0..2 {
            if !is_claude_code_mcp_surface(&observation) {
                break;
            }
            observation = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key("escape"),
                    "\x1b",
                    Some("close ClaudeCode MCP page before reopening /mcp".to_string()),
                )
                .await;
        }

        if !is_ready_for_claude_code_text(&observation) {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(8),
                    Some("wait for ClaudeCode prompt idle before /mcp".to_string()),
                    is_ready_for_claude_code_text,
                )
                .await;
        }
        if is_claude_code_prompt_authorization_prompt(&observation) {
            observation = self
                .accept_prompt_authorization_locked(result, slot_id, observation)
                .await?;
        }
        if !is_ready_for_claude_code_text(&observation) {
            result.status = ProviderBoxStatus::Blocked;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_MCP_STATUS_UNAVAILABLE,
                "ClaudeCode MCP status requires an idle composer or an MCP page that can be closed",
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
        if let Some(text) = claude_code_composer_blocking_text(&observation) {
            result.status = ProviderBoxStatus::Blocked;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_MCP_STATUS_UNAVAILABLE,
                "ClaudeCode composer is not empty; refusing to append /mcp command",
                json!({
                    "slot_id": slot_id,
                    "composer_text_preview": text.chars().take(120).collect::<String>(),
                    "safe_alternative": "clear the composer before probing MCP status",
                }),
            ));
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            return None;
        }
        Some(observation)
    }

    async fn open_mcp_servers_locked(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) -> Option<ClaudeCodeObservation> {
        let _ = self.ensure_ready_for_mcp_command(result, slot_id).await?;
        let _ = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::text("/mcp"),
                "/mcp",
                Some("type ClaudeCode /mcp command".to_string()),
            )
            .await;
        let mut observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("execute ClaudeCode /mcp command".to_string()),
            )
            .await;
        if !is_claude_code_mcp_list_screen(&observation)
            && claude_code_staged_command_matches(&observation, "/mcp")
        {
            observation = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key("enter"),
                    "\r",
                    Some("execute staged ClaudeCode /mcp command".to_string()),
                )
                .await;
        }
        if !is_claude_code_mcp_list_screen(&observation) {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(8),
                    Some("wait for ClaudeCode MCP server list".to_string()),
                    is_claude_code_mcp_list_screen,
                )
                .await;
        }
        if is_claude_code_mcp_list_screen(&observation) {
            Some(observation)
        } else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_MCP_STATUS_UNAVAILABLE,
                "ClaudeCode /mcp did not render a recognizable MCP server list",
                json!({
                    "slot_id": slot_id,
                    "reason": observation.snapshot.reason,
                    "state": observation.snapshot.state,
                    "screen_excerpt": bounded_text_excerpt(&observation.text, 1200),
                }),
            ));
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            None
        }
    }

    async fn collect_mcp_status_locked(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        mut observation: ClaudeCodeObservation,
    ) -> (Vec<ClaudeCodeMcpServerEntry>, ClaudeCodeObservation) {
        let mut servers: Vec<ClaudeCodeMcpServerEntry> = Vec::new();
        let mut seen = HashSet::new();
        for _ in 0..=36 {
            for entry in extract_claude_code_mcp_server_entries_from_screen(&observation.text) {
                let key = entry.name.to_ascii_lowercase();
                if seen.insert(key) {
                    servers.push(entry);
                }
            }
            if !is_claude_code_mcp_list_screen(&observation)
                || !claude_code_mcp_has_more_below(&observation)
            {
                break;
            }
            observation = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key("down"),
                    "\x1b[B",
                    Some("scroll ClaudeCode MCP server list".to_string()),
                )
                .await;
        }
        (servers, observation)
    }

    async fn refresh_mcp_status_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) {
        let target = Self::request_mcp_server(request);
        let Some(observation) = self.open_mcp_servers_locked(result, slot_id).await else {
            return;
        };
        let (servers, observation) = self
            .collect_mcp_status_locked(result, slot_id, observation)
            .await;
        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
        result.mcp_status = Some(claude_code_mcp_status_value(
            request,
            slot_id,
            &target,
            &servers,
            &observation,
        ));
        if servers.is_empty() {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_MCP_STATUS_UNAVAILABLE,
                "ClaudeCode MCP status page was recognized but no server rows were parsed",
                json!({
                    "slot_id": slot_id,
                    "target_server": target,
                    "screen_excerpt": bounded_text_excerpt(&observation.text, 1200),
                }),
            ));
        } else {
            result.status = ProviderBoxStatus::Completed;
            result.durable_source = Some("claude_code:/mcp".to_string());
        }
    }

    async fn select_mcp_server_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        mut observation: ClaudeCodeObservation,
        target: &str,
    ) -> Option<ClaudeCodeObservation> {
        for _ in 0..=48 {
            let entries = extract_claude_code_mcp_server_entries_from_screen(&observation.text);
            if entries
                .iter()
                .any(|entry| entry.selected && claude_code_mcp_server_name_eq(&entry.name, target))
            {
                return Some(observation);
            }
            if let Some(target_index) = entries
                .iter()
                .position(|entry| claude_code_mcp_server_name_eq(&entry.name, target))
            {
                let selected_index = entries.iter().position(|entry| entry.selected).unwrap_or(0);
                let (key_name, key_bytes, steps) = if target_index >= selected_index {
                    ("down", "\x1b[B", target_index - selected_index)
                } else {
                    ("up", "\x1b[A", selected_index - target_index)
                };
                for _ in 0..steps {
                    observation = self
                        .write_step(
                            result,
                            slot_id,
                            PtyStepAction::key(key_name),
                            key_bytes,
                            Some(format!("navigate ClaudeCode MCP list to {target}")),
                        )
                        .await;
                }
                continue;
            }

            if !claude_code_mcp_has_more_below(&observation) {
                result.mcp_status = Some(claude_code_mcp_status_value(
                    request,
                    slot_id,
                    target,
                    &entries,
                    &observation,
                ));
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_MCP_STATUS_UNAVAILABLE,
                    "ClaudeCode MCP server was not found in the /mcp list",
                    json!({
                        "slot_id": slot_id,
                        "target_server": target,
                        "visible_servers": entries.iter().map(|entry| entry.name.clone()).collect::<Vec<_>>(),
                    }),
                ));
                return None;
            }

            observation = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key("down"),
                    "\x1b[B",
                    Some(format!("scan ClaudeCode MCP list for {target}")),
                )
                .await;
        }

        result.status = ProviderBoxStatus::Unverified;
        result.add_diagnostic(ProviderBoxDiagnostic::error(
            DIAG_PROVIDER_MCP_RECONNECT_UNVERIFIED,
            "ClaudeCode MCP target could not be selected within the bounded navigation budget",
            json!({
                "slot_id": slot_id,
                "target_server": target,
                "screen_excerpt": bounded_text_excerpt(&observation.text, 1200),
            }),
        ));
        None
    }

    async fn reconnect_mcp_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) {
        let target = Self::request_mcp_server(request);
        let Some(observation) = self.open_mcp_servers_locked(result, slot_id).await else {
            return;
        };
        let Some(_) = self
            .select_mcp_server_locked(request, result, slot_id, observation, &target)
            .await
        else {
            return;
        };

        let mut observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some(format!("open ClaudeCode MCP details for {target}")),
            )
            .await;
        if !claude_code_mcp_detail_screen_for(&observation, &target) {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(6),
                    Some(format!("wait for ClaudeCode MCP details for {target}")),
                    |obs| claude_code_mcp_detail_screen_for(obs, &target),
                )
                .await;
        }
        if !claude_code_mcp_reconnect_action_selected(&observation) {
            let Some((selected_index, target_index)) =
                claude_code_mcp_detail_action_positions(&observation, "reconnect")
            else {
                result.status = ProviderBoxStatus::Unverified;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_MCP_RECONNECT_UNVERIFIED,
                    "ClaudeCode MCP details page did not expose Reconnect as an action",
                    json!({
                        "slot_id": slot_id,
                        "target_server": target,
                        "screen_excerpt": bounded_text_excerpt(&observation.text, 1200),
                    }),
                ));
                let status = self.pty.get_status(slot_id).await;
                result.slot_status =
                    Some(slot_status_value(slot_id, status.as_ref(), &observation));
                result.mcp_status = Some(claude_code_mcp_status_value(
                    request,
                    slot_id,
                    &target,
                    &extract_claude_code_mcp_server_entries_from_screen(&observation.text),
                    &observation,
                ));
                return;
            };

            let (key_name, key_bytes, steps) = if selected_index <= target_index {
                ("down", "\x1b[B", target_index - selected_index)
            } else {
                ("up", "\x1b[A", selected_index - target_index)
            };
            for _ in 0..steps {
                observation = self
                    .write_step(
                        result,
                        slot_id,
                        PtyStepAction::key(key_name),
                        key_bytes,
                        Some(format!(
                            "navigate ClaudeCode MCP details action to Reconnect for {target}"
                        )),
                    )
                    .await;
            }

            if !claude_code_mcp_reconnect_action_selected(&observation) {
                observation = self
                    .wait_step_until(
                        result,
                        slot_id,
                        Duration::from_secs(3),
                        Some(format!(
                            "wait for ClaudeCode MCP Reconnect action to become selected for {target}"
                        )),
                        claude_code_mcp_reconnect_action_selected,
                    )
                    .await;
            }
        }

        if !claude_code_mcp_reconnect_action_selected(&observation) {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_MCP_RECONNECT_UNVERIFIED,
                "ClaudeCode MCP details page did not select Reconnect after navigation",
                json!({
                    "slot_id": slot_id,
                    "target_server": target,
                    "screen_excerpt": bounded_text_excerpt(&observation.text, 1200),
                }),
            ));
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            result.mcp_status = Some(claude_code_mcp_status_value(
                request,
                slot_id,
                &target,
                &extract_claude_code_mcp_server_entries_from_screen(&observation.text),
                &observation,
            ));
            return;
        }

        let _ = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some(format!("execute ClaudeCode MCP reconnect for {target}")),
            )
            .await;
        observation = self
            .wait_step_until(
                result,
                slot_id,
                Duration::from_secs(12),
                Some(format!(
                    "wait for ClaudeCode MCP reconnect outcome for {target}"
                )),
                |obs| {
                    is_ready_for_claude_code_text(obs)
                        && claude_code_mcp_reconnect_outcome(&obs.text, &target).is_some()
                },
            )
            .await;

        let outcome = claude_code_mcp_reconnect_outcome(&observation.text, &target);
        if outcome.as_deref() == Some("failed") {
            result.status = ProviderBoxStatus::Failed;
            result.final_text = claude_code_mcp_reconnect_line(&observation.text, &target);
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_MCP_RECONNECT_UNVERIFIED,
                "ClaudeCode MCP reconnect reported failure",
                json!({
                    "slot_id": slot_id,
                    "target_server": target,
                    "outcome_line": result.final_text.clone(),
                }),
            ));
            self.refresh_mcp_status_locked(request, result, slot_id)
                .await;
            result.status = ProviderBoxStatus::Failed;
            return;
        }

        self.refresh_mcp_status_locked(request, result, slot_id)
            .await;
        let connected = result
            .mcp_status
            .as_ref()
            .and_then(|status| status.get("target_connected"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if connected {
            result.status = ProviderBoxStatus::Completed;
            result.final_text = Some(format!("ClaudeCode MCP server {target} is connected"));
        } else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_MCP_RECONNECT_UNVERIFIED,
                "ClaudeCode MCP reconnect did not verify the target server as connected",
                json!({
                    "slot_id": slot_id,
                    "target_server": target,
                    "outcome": outcome,
                    "mcp_status": result.mcp_status,
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
        if let Some(text) = claude_code_composer_blocking_text(&observation) {
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
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
            return;
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

    async fn ensure_deep_research_runtime(
        &self,
        result: &mut ProviderBoxResult,
    ) -> Option<PathBuf> {
        let runtime_dir = claude_code_deep_research_runtime_dir();
        if let Err(err) = tokio::fs::create_dir_all(&runtime_dir).await {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "ClaudeCode deep-research runtime directory could not be created",
                json!({
                    "runtime_dir": runtime_dir.display().to_string(),
                    "error": err.to_string(),
                }),
            ));
            return None;
        }

        let readme = runtime_dir.join("README.missiond-claude-code-deep-research.txt");
        let _ = tokio::fs::write(
            &readme,
            "MissionD provider-box ClaudeCode deep-research runtime.\nThis directory is fixed and pre-trusted; each request uses an independent ClaudeCode --session-id and a private ephemeral PTY.\n",
        )
        .await;
        Some(runtime_dir)
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

    async fn accept_prompt_authorization_locked(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        mut observation: ClaudeCodeObservation,
    ) -> Option<ClaudeCodeObservation> {
        let allowlist = claude_code_prompt_authorization_allowlist();
        let mut surface = claude_code_prompt_authorization_surface(&observation)?;
        if !claude_code_prompt_authorization_allowed(&surface, &allowlist) {
            result.status = ProviderBoxStatus::Blocked;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "ClaudeCode provider authorization prompt is not allowlisted",
                json!({
                    "slot_id": slot_id,
                    "kind": surface.kind,
                    "target": surface.target,
                    "selected_option": surface.selected_option,
                    "visible_options": surface.visible_options,
                    "rule": "provider-box only confirms provider authorization prompts when the normalized target is in the prompt authorization allowlist"
                }),
            ));
            return None;
        }

        let Some(target_index) = surface.allow_option_index else {
            result.status = ProviderBoxStatus::Blocked;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "ClaudeCode provider authorization prompt has no recognizable allow option",
                json!({
                    "slot_id": slot_id,
                    "kind": surface.kind,
                    "target": surface.target,
                    "visible_options": surface.visible_options,
                }),
            ));
            return None;
        };

        let Some(mut selected_index) = surface.selected_option_index else {
            result.status = ProviderBoxStatus::Blocked;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "ClaudeCode provider authorization prompt selection was not recognizable",
                json!({
                    "slot_id": slot_id,
                    "kind": surface.kind,
                    "target": surface.target,
                    "rule": "provider-box must observe the selected option before sending Enter"
                }),
            ));
            return None;
        };

        while selected_index != target_index {
            let (key, bytes, expected) = if selected_index < target_index {
                (
                    "down",
                    "\x1b[B",
                    "move ClaudeCode provider authorization selection down",
                )
            } else {
                (
                    "up",
                    "\x1b[A",
                    "move ClaudeCode provider authorization selection up",
                )
            };
            observation = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key(key),
                    bytes,
                    Some(expected.to_string()),
                )
                .await;
            surface = match claude_code_prompt_authorization_surface(&observation) {
                Some(surface) => surface,
                None => break,
            };
            selected_index = match surface.selected_option_index {
                Some(index) => index,
                None => {
                    result.status = ProviderBoxStatus::Blocked;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                        "ClaudeCode provider authorization selection disappeared before confirmation",
                        json!({
                            "slot_id": slot_id,
                            "kind": surface.kind,
                            "target": surface.target,
                        }),
                    ));
                    return None;
                }
            };
        }

        surface = match claude_code_prompt_authorization_surface(&observation) {
            Some(surface) => surface,
            None => {
                result.status = ProviderBoxStatus::Blocked;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                    "ClaudeCode provider authorization prompt disappeared before Enter verification",
                    json!({
                        "slot_id": slot_id,
                    }),
                ));
                return None;
            }
        };
        if surface.selected_option_index != surface.allow_option_index {
            result.status = ProviderBoxStatus::Blocked;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "ClaudeCode provider authorization allow option could not be selected",
                json!({
                    "slot_id": slot_id,
                    "kind": surface.kind,
                    "target": surface.target,
                    "selected_option": surface.selected_option,
                    "allow_option": surface.allow_option,
                }),
            ));
            return None;
        }

        let authorized_target = surface.target.clone();
        observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("confirm allowlisted ClaudeCode provider authorization".to_string()),
            )
            .await;
        result.add_diagnostic(ProviderBoxDiagnostic::warning(
            "PROVIDER_PROMPT_AUTHORIZATION_CONFIRMED",
            "ClaudeCode provider authorization prompt was confirmed from allowlist",
            json!({
                "slot_id": slot_id,
                "kind": surface.kind,
                "target": authorized_target,
                "selected_option": surface.selected_option,
                "rule": "observe-act-observe verified the allow option before Enter"
            }),
        ));

        if is_claude_code_prompt_authorization_prompt(&observation) {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(8),
                    Some(
                        "wait for ClaudeCode provider authorization prompt to advance".to_string(),
                    ),
                    |obs| !is_claude_code_prompt_authorization_prompt(obs),
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
            if is_claude_code_prompt_authorization_prompt(&observation) {
                let accepted = self
                    .accept_prompt_authorization_locked(result, slot_id, observation)
                    .await;
                match accepted {
                    Some(next) => {
                        observation = next;
                    }
                    None => return false,
                }
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

    fn validate_prompt_turn(
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> bool {
        if request
            .prompt
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode provider-box prompt turn requires a non-empty prompt",
                json!({
                    "slot_id": request.slot_id,
                    "command": request.command,
                }),
            ));
            return false;
        }
        if Self::request_provider_session_id(request).is_none() {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode provider-box prompt turn requires desired_worker.provider_session_id for durable JSONL final extraction",
                json!({
                    "slot_id": request.slot_id,
                    "command": request.command,
                    "rule": "callers that submit ClaudeCode prompt turns must allocate a request-scoped session id and pass it through desired_worker.provider_session_id"
                }),
            ));
            return false;
        }
        true
    }

    async fn ensure_ready_for_prompt_turn(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        timeout: Duration,
    ) -> bool {
        let started = Instant::now();
        loop {
            let mut observation = self.observe(slot_id).await;
            for _ in 0..2 {
                if !is_claude_code_mcp_surface(&observation) {
                    break;
                }
                observation = self
                    .write_step(
                        result,
                        slot_id,
                        PtyStepAction::key("escape"),
                        "\x1b",
                        Some("close ClaudeCode MCP page before prompt submission".to_string()),
                    )
                    .await;
            }
            if is_claude_code_workspace_trust_prompt(&observation) {
                observation = match self
                    .accept_workspace_trust_locked(result, slot_id, observation)
                    .await
                {
                    Some(observation) => observation,
                    None => return false,
                };
            }
            if is_claude_code_prompt_authorization_prompt(&observation) {
                let accepted = self
                    .accept_prompt_authorization_locked(result, slot_id, observation)
                    .await;
                match accepted {
                    Some(next) => {
                        observation = next;
                    }
                    None => return false,
                }
            }

            if is_ready_for_claude_code_text(&observation) {
                if let Some(text) = claude_code_composer_blocking_text(&observation) {
                    result.status = ProviderBoxStatus::Blocked;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                        "ClaudeCode composer is not empty; refusing to append a provider-box prompt turn",
                        json!({
                            "slot_id": slot_id,
                            "composer_text_preview": text.chars().take(120).collect::<String>(),
                            "safe_alternative": "clear the composer through a taught provider-box control before submitting a prompt turn"
                        }),
                    ));
                    let status = self.pty.get_status(slot_id).await;
                    result.slot_status =
                        Some(slot_status_value(slot_id, status.as_ref(), &observation));
                    return false;
                }
                return true;
            }

            if observation.snapshot.state == PtyCanonicalState::Blocked {
                result.status = ProviderBoxStatus::Blocked;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "ClaudeCode prompt turn slot is blocked before submission",
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

            if started.elapsed() >= timeout {
                result.status = ProviderBoxStatus::Blocked;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "ClaudeCode prompt turn slot did not reach an idle composer before timeout",
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

    async fn ensure_mcp_ready_for_prompt_turn(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) -> bool {
        let target = Self::request_mcp_server(request);
        self.refresh_mcp_status_locked(request, result, slot_id)
            .await;
        let connected = result
            .mcp_status
            .as_ref()
            .and_then(|status| status.get("target_connected"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if connected {
            return true;
        }

        self.reconnect_mcp_locked(request, result, slot_id).await;
        let connected = result
            .mcp_status
            .as_ref()
            .and_then(|status| status.get("target_connected"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if connected {
            true
        } else {
            if !matches!(
                result.status,
                ProviderBoxStatus::Failed
                    | ProviderBoxStatus::Blocked
                    | ProviderBoxStatus::Unverified
            ) {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_MCP_STATUS_UNAVAILABLE,
                    "ClaudeCode required MCP server was not connected before prompt submission",
                    json!({
                        "slot_id": slot_id,
                        "target_server": target,
                        "mcp_status": result.mcp_status,
                    }),
                ));
            }
            false
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
        let mut action = PtyStepAction::text("<claude-code prompt paste + enter>");
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
            Some("ClaudeCode accepts prompt and starts a provider turn".to_string()),
            status,
        );
        if let Err(err) = send_result {
            step.diagnostics.push(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "ClaudeCode provider-box prompt submission failed",
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

    async fn monitor_prompt_turn(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        session_id: &str,
        cursor: ClaudeCodeJsonlCursor,
    ) -> ProviderBoxResult {
        let timeout_secs = request.timeout_secs.unwrap_or(300).clamp(10, 7_200);
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let monitor_started_at = Instant::now();
        let mut idle_seen_at: Option<Instant> = None;
        let output_contract_file = output_contract_must_write_file(request);
        let mut output_contract_file_seen: Option<(u64, Instant)> = None;
        let mut output_contract_file_idle_seen_at: Option<Instant> = None;
        let mut output_contract_file_continue_attempts: usize = 0;

        loop {
            if let Some(path) = output_contract_file.as_ref() {
                match output_contract_file_completion_state(
                    path,
                    output_contract_file_seen.as_ref(),
                ) {
                    OutputContractFileState::Pending { len } => {
                        output_contract_file_seen = Some((len, Instant::now()));
                    }
                    OutputContractFileState::Stable { len } => {
                        let durable_source = path.display().to_string();
                        result.status = ProviderBoxStatus::Completed;
                        result.provider = request
                            .provider
                            .clone()
                            .or_else(|| Some("claude_code".to_string()));
                        result.model = request.model.clone();
                        result.model_profile = request.model_profile.clone();
                        result.provider_conversation_id = Some(session_id.to_string());
                        result.provider_session_identity = Some(ProviderSessionIdentity::resolved(
                            result.provider.clone(),
                            CliEngine::ClaudeCode,
                            Some(slot_id.to_string()),
                            session_id.to_string(),
                            "output_contract_file",
                            Some(durable_source.clone()),
                            request.cwd.clone().or_else(|| request.project_root.clone()),
                            "must_write_file_stable",
                        ));
                        result.durable_source = Some(durable_source.clone());
                        result.slot_status = Some(json!({
                            "slot_id": slot_id,
                            "completion_source": "output_contract_file",
                            "observation_skipped": true,
                            "reason": "The requested output_contract.must_write_file is stable; PTY screen observation is diagnostic only."
                        }));
                        result.final_text =
                            Some(format!("Output contract file written: {}", path.display()));
                        result.add_diagnostic(ProviderBoxDiagnostic::warning(
                            "PROVIDER_OUTPUT_CONTRACT_FILE_COMPLETED",
                            "ClaudeCode worker-turn completed from stable output_contract.must_write_file",
                            json!({
                                "slot_id": slot_id,
                                "session_id": session_id,
                                "must_write_file": path.display().to_string(),
                                "bytes": len,
                                "rule": "The requested artifact file is the canonical output; PTY screen text and JSONL final extraction were not required"
                            }),
                        ));
                        return result.clone();
                    }
                    OutputContractFileState::MissingOrTooSmall => {
                        output_contract_file_seen = None;
                    }
                }
            }

            let analysis =
                analyze_claude_code_jsonl_after_cursor(&self.claude_home, session_id, &cursor);
            if claude_code_analysis_line_count_advanced(
                analysis.line_count,
                &mut last_analysis_line_count,
            ) {
                idle_seen_at = None;
            }
            if let Some(upstream_error) = analysis.upstream_error {
                let status = upstream_error
                    .pointer("/error/status")
                    .or_else(|| upstream_error.get("status"))
                    .and_then(Value::as_i64);
                let formatted = upstream_error
                    .pointer("/error/formatted")
                    .or_else(|| upstream_error.get("formatted"))
                    .and_then(Value::as_str);
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_UPSTREAM_UNAVAILABLE,
                    "ClaudeCode upstream provider returned an API error before durable final",
                    json!({
                        "provider": request.provider.clone().unwrap_or_else(|| "claude_code".to_string()),
                        "slot_id": slot_id,
                        "session_id": session_id,
                        "status": status,
                        "formatted": formatted,
                        "event": upstream_error,
                        "line_count": analysis.line_count,
                        "rule": "provider-box fails closed on ClaudeCode API errors and does not synthesize a final from PTY screen text"
                    }),
                ));
                return result.clone();
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
                let status = self.pty.get_status(slot_id).await;
                let observation = self.observe(slot_id).await;
                result.status = ProviderBoxStatus::Completed;
                result.provider = request
                    .provider
                    .clone()
                    .or_else(|| Some("claude_code".to_string()));
                result.model = request.model.clone();
                result.model_profile = request.model_profile.clone();
                result.provider_conversation_id = Some(session_id.to_string());
                result.provider_session_identity = Some(ProviderSessionIdentity::resolved(
                    result.provider.clone(),
                    CliEngine::ClaudeCode,
                    Some(slot_id.to_string()),
                    session_id.to_string(),
                    "claude_code_session_jsonl",
                    Some(durable_source.clone()),
                    request.cwd.clone().or_else(|| request.project_root.clone()),
                    "durable_final",
                ));
                result.durable_source = Some(durable_source.clone());
                result.slot_status =
                    Some(slot_status_value(slot_id, status.as_ref(), &observation));
                result.final_text = Some(final_text.to_string());
                return result.clone();
            }

            if let Some(path) = output_contract_file.as_ref() {
                match output_contract_file_completion_state(
                    path,
                    output_contract_file_seen.as_ref(),
                ) {
                    OutputContractFileState::Pending { len } => {
                        output_contract_file_seen = Some((len, Instant::now()));
                    }
                    OutputContractFileState::Stable { len } => {
                        let _ = self.pty.write(slot_id, "\x1b").await;
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        let status = self.pty.get_status(slot_id).await;
                        let observation = self.observe(slot_id).await;
                        let durable_source = path.display().to_string();
                        result.status = ProviderBoxStatus::Completed;
                        result.provider = request
                            .provider
                            .clone()
                            .or_else(|| Some("claude_code".to_string()));
                        result.model = request.model.clone();
                        result.model_profile = request.model_profile.clone();
                        result.provider_conversation_id = Some(session_id.to_string());
                        result.provider_session_identity = Some(ProviderSessionIdentity::resolved(
                            result.provider.clone(),
                            CliEngine::ClaudeCode,
                            Some(slot_id.to_string()),
                            session_id.to_string(),
                            "output_contract_file",
                            Some(durable_source.clone()),
                            request.cwd.clone().or_else(|| request.project_root.clone()),
                            "must_write_file_stable",
                        ));
                        result.durable_source = Some(durable_source.clone());
                        result.slot_status =
                            Some(slot_status_value(slot_id, status.as_ref(), &observation));
                        result.final_text =
                            Some(format!("Output contract file written: {}", path.display()));
                        result.add_diagnostic(ProviderBoxDiagnostic::warning(
                            "PROVIDER_OUTPUT_CONTRACT_FILE_COMPLETED",
                            "ClaudeCode worker-turn completed from stable output_contract.must_write_file",
                            json!({
                                "slot_id": slot_id,
                                "session_id": session_id,
                                "must_write_file": path.display().to_string(),
                                "bytes": len,
                                "line_count": analysis.line_count,
                                "rule": "The requested artifact file is the canonical output; PTY screen text was not used as a semantic final"
                            }),
                        ));
                        return result.clone();
                    }
                    OutputContractFileState::MissingOrTooSmall => {
                        output_contract_file_seen = None;
                    }
                }
            }

            let observation = self.observe(slot_id).await;
            if observation.snapshot.state == PtyCanonicalState::Blocked {
                if is_claude_code_prompt_authorization_prompt(&observation) {
                    if self
                        .accept_prompt_authorization_locked(result, slot_id, observation)
                        .await
                        .is_some()
                    {
                        idle_seen_at = None;
                        continue;
                    }
                    return result.clone();
                }
                result.status = ProviderBoxStatus::Blocked;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "ClaudeCode provider-box turn entered a blocked provider surface",
                    json!({
                        "slot_id": slot_id,
                        "reason": observation.snapshot.reason,
                        "blocked_kind": observation.snapshot.blocked_kind,
                        "screen_excerpt": bounded_text_excerpt(&observation.text, 1200),
                    }),
                ));
                let status = self.pty.get_status(slot_id).await;
                result.slot_status =
                    Some(slot_status_value(slot_id, status.as_ref(), &observation));
                return result.clone();
            }

            if matches!(
                observation.snapshot.state,
                PtyCanonicalState::Idle | PtyCanonicalState::Complete
            ) {
                if should_fail_missing_durable_final_on_idle(
                    output_contract_file.is_some(),
                    idle_seen_at.as_ref().map(Instant::elapsed),
                    monitor_started_at.elapsed(),
                ) {
                    if idle_seen_at.is_some() {
                        result.status = ProviderBoxStatus::Failed;
                        result.add_diagnostic(ProviderBoxDiagnostic::error(
                            DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                            "ClaudeCode returned to input but no durable assistant end_turn final was found",
                            json!({
                                "slot_id": slot_id,
                                "session_id": session_id,
                                "line_count": analysis.line_count,
                                "rule": "PTY screen text is diagnostic only; no fallback final was synthesized"
                            }),
                        ));
                        return result.clone();
                    }
                } else if output_contract_file.is_some() {
                    if let Some(path) = output_contract_file.as_ref() {
                        if output_contract_file_idle_seen_at.is_none() {
                            output_contract_file_idle_seen_at = Some(Instant::now());
                        } else if should_continue_file_contract_worker_on_idle(
                            output_contract_file_idle_seen_at
                                .as_ref()
                                .map(Instant::elapsed),
                            monitor_started_at.elapsed(),
                            output_contract_file_continue_attempts,
                        ) {
                            output_contract_file_continue_attempts += 1;
                            output_contract_file_idle_seen_at = None;
                            idle_seen_at = None;
                            output_contract_file_seen = None;
                            if !self
                                .submit_redacted_prompt(
                                    result,
                                    slot_id,
                                    &file_contract_continue_prompt(path),
                                    "<claude-code output-contract continue prompt>",
                                )
                                .await
                            {
                                result.status = ProviderBoxStatus::Failed;
                                result.add_diagnostic(ProviderBoxDiagnostic::error(
                                    DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                                    "ClaudeCode file-contract worker was idle without the required artifact, and provider-box could not submit a continuation prompt",
                                    json!({
                                        "slot_id": slot_id,
                                        "session_id": session_id,
                                        "must_write_file": path.display().to_string(),
                                        "continue_attempts": output_contract_file_continue_attempts,
                                    }),
                                ));
                                return result.clone();
                            }
                            result.add_diagnostic(ProviderBoxDiagnostic::warning(
                                "PROVIDER_OUTPUT_CONTRACT_FILE_CONTINUED",
                                "ClaudeCode file-contract worker returned to input before writing output_contract.must_write_file; provider-box submitted a bounded continuation prompt",
                                json!({
                                    "slot_id": slot_id,
                                    "session_id": session_id,
                                    "must_write_file": path.display().to_string(),
                                    "continue_attempts": output_contract_file_continue_attempts,
                                    "max_continue_attempts": OUTPUT_CONTRACT_FILE_MAX_CONTINUE_ATTEMPTS,
                                    "rule": "For file-writing workers, the requested file is canonical completion; idle without the file triggers bounded continuation before a typed failure"
                                }),
                            ));
                            continue;
                        }
                    }
                    idle_seen_at = None;
                } else if idle_seen_at.is_none() {
                    idle_seen_at = Some(Instant::now());
                }
            } else {
                idle_seen_at = None;
                output_contract_file_idle_seen_at = None;
            }

            if Instant::now() >= deadline {
                let _ = self.pty.write(slot_id, "\x1b").await;
                tokio::time::sleep(Duration::from_millis(500)).await;
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                    "ClaudeCode provider-box turn timed out before durable final appeared",
                    json!({
                        "slot_id": slot_id,
                        "session_id": session_id,
                        "timeout_secs": timeout_secs,
                    }),
                ));
                return result.clone();
            }

            tokio::time::sleep(Duration::from_millis(750)).await;
        }
    }

    async fn submit_redacted_prompt(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        prompt: &str,
        label: &str,
    ) -> bool {
        let mut action = PtyStepAction::text(label.to_string());
        action.redacted = true;
        let _ = self
            .write_step(
                result,
                slot_id,
                action,
                prompt,
                Some("write ClaudeCode prompt into composer".to_string()),
            )
            .await;
        let _ = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("submit ClaudeCode prompt".to_string()),
            )
            .await;
        !result
            .step_records
            .last()
            .is_some_and(|step| step.verification_status == PtyStepVerificationStatus::Failed)
    }

    async fn submit_text_only_prompt(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        prompt: &str,
    ) -> bool {
        self.submit_redacted_prompt(result, slot_id, prompt, "<claude-code text-only prompt>")
            .await
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
        let monitor_started_at = Instant::now();
        let mut idle_seen_at: Option<Instant> = None;
        let mut last_analysis_line_count = 0usize;

        loop {
            let analysis =
                analyze_claude_code_jsonl_after_cursor(&self.claude_home, session_id, &cursor);
            if claude_code_analysis_line_count_advanced(
                analysis.line_count,
                &mut last_analysis_line_count,
            ) {
                idle_seen_at = None;
            }
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
            if let Some(upstream_error) = analysis.upstream_error {
                let status = upstream_error
                    .pointer("/error/status")
                    .or_else(|| upstream_error.get("status"))
                    .and_then(Value::as_i64);
                let formatted = upstream_error
                    .pointer("/error/formatted")
                    .or_else(|| upstream_error.get("formatted"))
                    .and_then(Value::as_str);
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_UPSTREAM_UNAVAILABLE,
                    "ClaudeCode upstream provider returned an API error before durable final",
                    json!({
                        "provider": CLAUDE_CODE_TEXT_PROVIDER,
                        "model_id": source.model_id,
                        "provider_model_id": source.provider_model_id,
                        "session_id": session_id,
                        "status": status,
                        "formatted": formatted,
                        "event": upstream_error,
                        "line_count": analysis.line_count,
                        "rule": "provider-box fails closed on ClaudeCode API errors and does not synthesize a final from PTY screen text"
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
                if is_claude_code_prompt_authorization_prompt(&observation) {
                    if self
                        .accept_prompt_authorization_locked(result, slot_id, observation)
                        .await
                        .is_some()
                    {
                        idle_seen_at = None;
                        continue;
                    }
                    return false;
                }
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

            if claude_code_observation_is_terminal_idle(&observation) {
                if let Some(seen_at) = idle_seen_at {
                    if durable_final_missing_idle_grace_elapsed(
                        seen_at.elapsed(),
                        monitor_started_at.elapsed(),
                    ) {
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
            let _ = self.pty.force_kill(&slot_id).await;
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
        let _ = self.pty.force_kill(&slot_id).await;
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

    async fn run_deep_research_source(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> bool {
        if !Self::validate_deep_research_request(request, result) {
            return false;
        }

        let queue_key =
            format!("{CLAUDE_CODE_DEEP_RESEARCH_PROVIDER}:{CLAUDE_CODE_DEEP_RESEARCH_MODEL_ID}");
        let queue_semaphore = self.text_lane_semaphore(&queue_key).await;
        let queued_at = Instant::now();
        let Ok(_queue_guard) = queue_semaphore.acquire_owned().await else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "ClaudeCode deep-research queue is unavailable",
                json!({
                    "provider": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER,
                    "queue": {
                        "key": queue_key,
                        "max_concurrent": CLAUDE_CODE_DEEP_RESEARCH_MAX_CONCURRENT,
                    },
                }),
            ));
            return false;
        };
        let queue_wait_ms = u64::try_from(queued_at.elapsed().as_millis()).unwrap_or(u64::MAX);

        let Some(runtime_dir) = self.ensure_deep_research_runtime(result).await else {
            return false;
        };

        let session_id = Self::request_provider_session_id(request)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let slot_id = format!("slot-claude-code-deep-research-{}", &session_id[..8]);
        let slot = PTYSlot {
            id: slot_id.clone(),
            role: "provider-box-claude-code-deep-research".to_string(),
            cwd: Some(runtime_dir.clone()),
            engine: CliEngine::ClaudeCode,
        };
        self.pty.init_slot(&slot).await;

        let mut extra_env = HashMap::new();
        extra_env.insert(
            "MISSIOND_PROVIDER_BOX_TASK".to_string(),
            "deep_research".to_string(),
        );
        extra_env.insert(
            "MISSIOND_PROVIDER_BOX_TASK_PROVIDER".to_string(),
            CLAUDE_CODE_DEEP_RESEARCH_PROVIDER.to_string(),
        );

        let options = PTYSpawnOptions {
            auto_restart: false,
            wait_for_idle: false,
            timeout_secs: Some(90),
            mcp_config: None,
            dangerously_skip_permissions: false,
            model: Some(CLAUDE_CODE_DEEP_RESEARCH_PROVIDER_MODEL.to_string()),
            reasoning_effort: Some(CLAUDE_CODE_DEEP_RESEARCH_MODEL_PROFILE.to_string()),
            search_enabled: true,
            sandbox: None,
            approval_policy: None,
            tool_policy_path: None,
            claude_code_tools: None,
            claude_code_strict_mcp_config: false,
            claude_code_disable_slash_commands: false,
            provider_session_id: Some(session_id.clone()),
            extra_env,
            initial_prompt: None,
            command_override: None,
        };

        if let Err(err) = self.pty.spawn(&slot, options).await {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "ClaudeCode deep-research PTY could not be spawned",
                json!({
                    "provider": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER,
                    "error": err.to_string(),
                }),
            ));
            let _ = self.pty.force_kill(&slot_id).await;
            return false;
        }

        let ok = self
            .run_spawned_deep_research_source(
                request,
                result,
                &slot_id,
                &session_id,
                &queue_key,
                queue_wait_ms,
            )
            .await;
        let _ = self.pty.force_kill(&slot_id).await;
        ok
    }

    async fn run_spawned_deep_research_source(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        session_id: &str,
        queue_key: &str,
        queue_wait_ms: u64,
    ) -> bool {
        if !self
            .ensure_ready_for_prompt_turn(result, slot_id, Duration::from_secs(90))
            .await
        {
            return false;
        }

        let cursor = claude_code_jsonl_cursor_for_session(&self.claude_home, session_id);
        let prompt = request.prompt.as_deref().unwrap_or_default();
        if !self
            .submit_redacted_prompt(
                result,
                slot_id,
                prompt,
                "<claude-code deep-research prompt>",
            )
            .await
        {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "ClaudeCode deep-research prompt submission failed",
                json!({
                    "provider": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER,
                }),
            ));
            return false;
        }

        self.monitor_deep_research_turn(
            request,
            result,
            slot_id,
            session_id,
            cursor,
            queue_key,
            queue_wait_ms,
        )
        .await
    }

    async fn monitor_deep_research_turn(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        session_id: &str,
        cursor: ClaudeCodeJsonlCursor,
        queue_key: &str,
        queue_wait_ms: u64,
    ) -> bool {
        let timeout_secs = request.timeout_secs.unwrap_or(1_800).clamp(60, 14_400);
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let mut launch: Option<ClaudeCodeWorkflowLaunch> = None;

        loop {
            if launch.is_none() {
                let analysis = analyze_claude_code_deep_research_after_cursor(
                    &self.claude_home,
                    session_id,
                    &cursor,
                );
                if let Some(upstream_error) = analysis.upstream_error {
                    result.status = ProviderBoxStatus::Failed;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_UPSTREAM_UNAVAILABLE,
                        "ClaudeCode upstream provider returned an API error before launching deep-research workflow",
                        json!({
                            "provider": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER,
                            "session_id": session_id,
                            "event": upstream_error,
                            "line_count": analysis.line_count,
                        }),
                    ));
                    return false;
                }
                launch = analysis.launch;
                if let Some(path) = analysis.jsonl_path {
                    result.durable_source = Some(path.display().to_string());
                }
            }

            if let Some(workflow) = launch.as_ref() {
                let report = read_claude_code_workflow_journal_report(&workflow.transcript_dir);
                if let Some(report_json) = report.report {
                    let final_text = claude_code_deep_research_report_to_markdown(&report_json);
                    result.status = ProviderBoxStatus::Completed;
                    result.provider = Some(CLAUDE_CODE_DEEP_RESEARCH_PROVIDER.to_string());
                    result.model = Some(CLAUDE_CODE_DEEP_RESEARCH_MODEL_ID.to_string());
                    result.model_profile =
                        Some(CLAUDE_CODE_DEEP_RESEARCH_MODEL_PROFILE.to_string());
                    result.provider_conversation_id = Some(session_id.to_string());
                    result.provider_session_identity = Some(ProviderSessionIdentity::resolved(
                        Some(CLAUDE_CODE_DEEP_RESEARCH_PROVIDER.to_string()),
                        CliEngine::ClaudeCode,
                        Some(slot_id.to_string()),
                        session_id.to_string(),
                        "claude_code_deep_research_workflow_journal",
                        report
                            .journal_path
                            .as_ref()
                            .map(|path| path.display().to_string()),
                        Some(
                            claude_code_deep_research_runtime_dir()
                                .display()
                                .to_string(),
                        ),
                        "durable_workflow_report",
                    ));
                    result.durable_source = report
                        .journal_path
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .or_else(|| Some("claude_code_deep_research_workflow_journal".to_string()));
                    result.slot_status = Some(json!({
                        "kind": "claude_code_deep_research",
                        "provider": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER,
                        "private_slot_id": slot_id,
                        "session_id": session_id,
                        "workflow": "deep-research",
                        "task_id": workflow.task_id,
                        "run_id": workflow.run_id,
                        "journal_line_count": report.line_count,
                        "queue": {
                            "owner": "provider-box",
                            "key": queue_key,
                            "max_concurrent": CLAUDE_CODE_DEEP_RESEARCH_MAX_CONCURRENT,
                            "wait_ms": queue_wait_ms,
                            "policy": "per_logical_claude_code_deep_research_source"
                        }
                    }));
                    result.final_text = Some(final_text);
                    return true;
                }
            }

            let observation = self.observe(slot_id).await;
            if observation.snapshot.state == PtyCanonicalState::Blocked {
                if is_claude_code_prompt_authorization_prompt(&observation) {
                    if self
                        .accept_prompt_authorization_locked(result, slot_id, observation)
                        .await
                        .is_some()
                    {
                        continue;
                    }
                    return false;
                }
                result.status = ProviderBoxStatus::Blocked;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "ClaudeCode deep-research turn entered a blocked provider surface",
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

            if Instant::now() >= deadline {
                let _ = self.pty.write(slot_id, "\x1b").await;
                tokio::time::sleep(Duration::from_millis(500)).await;
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                    "ClaudeCode deep-research workflow timed out before durable journal report appeared",
                    json!({
                        "provider": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER,
                        "session_id": session_id,
                        "workflow_launch": launch.as_ref().map(|workflow| json!({
                            "task_id": workflow.task_id,
                            "run_id": workflow.run_id,
                        })),
                        "timeout_secs": timeout_secs,
                    }),
                ));
                return false;
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
}

#[async_trait]
impl ProviderDriver for ClaudeCodeProviderDriver {
    fn engine(&self) -> CliEngine {
        CliEngine::ClaudeCode
    }

    fn capabilities(&self) -> ProviderDriverCapabilities {
        claude_code_provider_capabilities()
    }

    async fn submit_turn(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        if Self::request_is_deep_research(request) {
            result.provider = Some(CLAUDE_CODE_DEEP_RESEARCH_PROVIDER.to_string());
            result.model = Some("claude-code-deep-research-opus-4-8-xhigh".to_string());
            result.model_profile = Some("xhigh".to_string());
            self.run_deep_research_source(request, &mut result).await;
            return result;
        }
        if !Self::validate_prompt_turn(request, &mut result) {
            return result;
        }
        let Some(session_id) = Self::request_provider_session_id(request) else {
            return result;
        };
        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;

        if Self::request_requires_mcp(request)
            && !self
                .ensure_mcp_ready_for_prompt_turn(request, &mut result, &slot_id)
                .await
        {
            return result;
        }
        if !self
            .ensure_ready_for_prompt_turn(&mut result, &slot_id, Duration::from_secs(90))
            .await
        {
            return result;
        }
        let cursor = claude_code_jsonl_cursor_for_session(&self.claude_home, &session_id);
        let prompt = request.prompt.as_deref().unwrap_or_default();
        if !self.submit_prompt_step(&mut result, &slot_id, prompt).await {
            result.status = ProviderBoxStatus::Failed;
            return result;
        }
        self.monitor_prompt_turn(request, &mut result, &slot_id, &session_id, cursor)
            .await
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
        self.reconnect_mcp_locked(request, &mut result, &slot_id)
            .await;
        result
    }
}

fn claude_code_provider_capabilities() -> ProviderDriverCapabilities {
    ProviderDriverCapabilities {
        submit_turn: true,
        switch_model: true,
        usage_probe: false,
        model_catalog: false,
        pure_text_guard: true,
        control_action: true,
        pty_step: false,
        status: true,
        mcp_status: true,
        mcp_reconnect: true,
        prompt_authorization: true,
    }
}

fn is_claude_code_mcp_list_screen(observation: &ClaudeCodeObservation) -> bool {
    observation
        .text
        .to_ascii_lowercase()
        .contains("manage mcp servers")
}

fn is_claude_code_mcp_detail_screen(observation: &ClaudeCodeObservation) -> bool {
    let lower = observation.text.to_ascii_lowercase();
    lower.contains("mcp server")
        && lower.contains("status:")
        && lower.contains("config location:")
        && lower.contains("reconnect")
}

fn is_claude_code_mcp_surface(observation: &ClaudeCodeObservation) -> bool {
    is_claude_code_mcp_list_screen(observation) || is_claude_code_mcp_detail_screen(observation)
}

fn claude_code_mcp_has_more_below(observation: &ClaudeCodeObservation) -> bool {
    observation
        .text
        .lines()
        .map(|line| line.trim())
        .any(|line| line.starts_with('↓') && line.to_ascii_lowercase().contains("more below"))
}

fn extract_claude_code_mcp_server_entries_from_screen(text: &str) -> Vec<ClaudeCodeMcpServerEntry> {
    text.lines()
        .filter_map(parse_claude_code_mcp_server_entry)
        .collect()
}

fn parse_claude_code_mcp_server_entry(line: &str) -> Option<ClaudeCodeMcpServerEntry> {
    let normalized = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = normalized.trim_start();
    let selected = trimmed.starts_with('❯') || trimmed.starts_with('>') || trimmed.starts_with('›');
    let row = trimmed
        .trim_start_matches(|c: char| matches!(c, '❯' | '>' | '›' | '●'))
        .trim_start();
    if !row.contains('·') {
        return None;
    }
    let parts = row.split('·').map(str::trim).collect::<Vec<_>>();
    let name = parts.first()?.trim();
    let lower_name = name.to_ascii_lowercase();
    if name.is_empty()
        || name.starts_with('↑')
        || name.starts_with('↓')
        || name.starts_with('⚠')
        || name.eq_ignore_ascii_case("claude.ai")
        || name.ends_with("MCPs")
        || name.contains("MCPs (")
        || lower_name.contains("setup issue")
    {
        return None;
    }
    let marker = parts.get(1).copied().unwrap_or_default();
    let (status, connected, auth) = claude_code_mcp_marker_status(marker);
    Some(ClaudeCodeMcpServerEntry {
        name: name.to_string(),
        selected,
        status,
        connected,
        auth,
        tools_summary: parts.get(2).map(|value| value.to_string()),
    })
}

fn claude_code_mcp_marker_status(marker: &str) -> (String, bool, Option<String>) {
    let lower = marker.to_ascii_lowercase();
    if lower.contains("connected") || marker.contains('✔') || marker.contains('✓') {
        ("connected".to_string(), true, None)
    } else if lower.contains("failed") || marker.contains('✘') || marker.contains('✗') {
        ("failed".to_string(), false, None)
    } else if lower.contains("needs authentication") || marker.contains('△') {
        (
            "needs_authentication".to_string(),
            false,
            Some("needs authentication".to_string()),
        )
    } else if lower.contains("disabled") || marker.contains('◯') {
        ("disabled".to_string(), false, None)
    } else {
        ("unknown".to_string(), false, None)
    }
}

fn claude_code_mcp_status_value(
    request: &ProviderInteractionRequest,
    slot_id: &str,
    target: &str,
    servers: &[ClaudeCodeMcpServerEntry],
    observation: &ClaudeCodeObservation,
) -> Value {
    let selected_server = servers
        .iter()
        .find(|entry| entry.selected)
        .map(|entry| entry.name.clone());
    let target_entry = servers
        .iter()
        .find(|entry| claude_code_mcp_server_name_eq(&entry.name, target));
    let aggregate_status = if servers.is_empty() {
        "unknown"
    } else if target_entry
        .as_ref()
        .is_some_and(|entry| entry.status == "failed")
    {
        "target_failed"
    } else if target_entry.as_ref().is_some_and(|entry| entry.connected) {
        "target_connected"
    } else if servers.iter().all(|entry| entry.connected) {
        "connected"
    } else {
        "degraded"
    };
    json!({
        "schema": "missiond.provider-box.claude-code-mcp-status.v1",
        "provider": request.provider.clone().or_else(|| Some("claude_code".to_string())),
        "engine": CliEngine::ClaudeCode,
        "slot_id": slot_id,
        "source": "claude_code:/mcp",
        "observed_at": chrono::Utc::now().to_rfc3339(),
        "status": aggregate_status,
        "target_server": target,
        "target_status": target_entry.map(|entry| entry.status.clone()),
        "target_connected": target_entry.is_some_and(|entry| entry.connected),
        "selected_server": selected_server,
        "reconnect_action_visible": claude_code_mcp_reconnect_action_visible(observation),
        "servers": servers
            .iter()
            .map(|entry| {
                json!({
                    "name": entry.name,
                    "selected": entry.selected,
                    "status": entry.status,
                    "connected": entry.connected,
                    "auth": entry.auth,
                    "tools_summary": entry.tools_summary,
                })
            })
            .collect::<Vec<_>>(),
    })
}

fn claude_code_mcp_server_name_eq(observed: &str, target: &str) -> bool {
    observed.trim().eq_ignore_ascii_case(target.trim())
}

fn claude_code_mcp_detail_screen_for(observation: &ClaudeCodeObservation, target: &str) -> bool {
    if !is_claude_code_mcp_detail_screen(observation) {
        return false;
    }
    let normalized_target = target.replace('-', " ").to_ascii_lowercase();
    observation.text.lines().any(|line| {
        let lower = line
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase();
        lower.ends_with(" mcp server") && lower.replace('-', " ").contains(&normalized_target)
    })
}

fn claude_code_mcp_reconnect_action_selected(observation: &ClaudeCodeObservation) -> bool {
    observation.text.lines().any(|line| {
        let normalized = line.split_whitespace().collect::<Vec<_>>().join(" ");
        let lower = normalized.to_ascii_lowercase();
        (normalized.trim_start().starts_with('❯')
            || normalized.trim_start().starts_with('>')
            || normalized.trim_start().starts_with('›'))
            && lower.contains("reconnect")
    })
}

fn claude_code_mcp_reconnect_action_visible(observation: &ClaudeCodeObservation) -> bool {
    claude_code_mcp_detail_action_positions(observation, "reconnect").is_some()
}

fn claude_code_mcp_detail_action_positions(
    observation: &ClaudeCodeObservation,
    target_action: &str,
) -> Option<(usize, usize)> {
    let target_action = target_action.trim().to_ascii_lowercase();
    if target_action.is_empty() {
        return None;
    }

    let mut selected_index = None;
    let mut target_index = None;
    let mut action_index = 0usize;

    for line in observation.text.lines() {
        let trimmed = line.trim_start();
        let selected =
            trimmed.starts_with('❯') || trimmed.starts_with('>') || trimmed.starts_with('›');
        let candidate = if selected {
            trimmed
                .chars()
                .skip(1)
                .collect::<String>()
                .trim_start()
                .to_string()
        } else {
            trimmed.to_string()
        };
        let Some((number, label)) = candidate.split_once('.') else {
            continue;
        };
        if !number.trim().chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let label = label.trim();
        if label.is_empty() {
            continue;
        }

        if selected {
            selected_index = Some(action_index);
        }
        if label.to_ascii_lowercase().contains(&target_action) {
            target_index = Some(action_index);
        }
        action_index += 1;
    }

    selected_index.zip(target_index)
}

fn claude_code_mcp_reconnect_outcome(text: &str, target: &str) -> Option<String> {
    let target = target.to_ascii_lowercase();
    for line in text.lines().rev().take(40) {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("reconnect") || !lower.contains(&target) {
            continue;
        }
        if lower.contains("failed") || lower.contains("error") {
            return Some("failed".to_string());
        }
        if lower.contains("reconnected") || lower.contains("connected") || lower.contains("success")
        {
            return Some("connected".to_string());
        }
    }
    None
}

fn claude_code_mcp_reconnect_line(text: &str, target: &str) -> Option<String> {
    let target = target.to_ascii_lowercase();
    text.lines().rev().find_map(|line| {
        let lower = line.to_ascii_lowercase();
        if lower.contains("reconnect") && lower.contains(&target) {
            Some(line.trim().to_string())
        } else {
            None
        }
    })
}

fn bounded_text_excerpt(text: &str, max_chars: usize) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        text.to_string()
    } else {
        chars[chars.len() - max_chars..].iter().collect()
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

fn claude_code_observation_is_terminal_idle(observation: &ClaudeCodeObservation) -> bool {
    matches!(
        observation.snapshot.state,
        PtyCanonicalState::Idle | PtyCanonicalState::Complete
    ) && matches!(
        observation.session_state,
        SessionState::Idle | SessionState::Exited
    )
}

fn claude_code_observation_is_auto_confirming(observation: &ClaudeCodeObservation) -> bool {
    observation.snapshot.state == PtyCanonicalState::Blocked
        && observation.session_state == SessionState::Confirming
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
            .or_else(|| trimmed.strip_prefix('›'))
            .or_else(|| trimmed.strip_prefix('>'))?;
        let text = rest.trim();
        if is_claude_code_placeholder_text(text) {
            Some(String::new())
        } else {
            Some(text.to_string())
        }
    })
}

fn claude_code_composer_blocking_text(observation: &ClaudeCodeObservation) -> Option<String> {
    let text = claude_code_composer_text(observation)?;
    if text.trim().is_empty() || claude_code_composer_text_is_placeholder(observation, &text) {
        None
    } else {
        Some(text)
    }
}

fn claude_code_composer_text_is_placeholder(
    observation: &ClaudeCodeObservation,
    text: &str,
) -> bool {
    let normalized = normalize_claude_code_placeholder_text(text);
    if normalized.is_empty() {
        return false;
    }
    if observation
        .snapshot
        .screen_signals
        .as_ref()
        .is_some_and(|signals| {
            signals.placeholder_visible
                && signals
                    .placeholder_text
                    .as_deref()
                    .is_some_and(|placeholder| {
                        normalize_claude_code_placeholder_text(placeholder) == normalized
                    })
        })
    {
        return true;
    }
    is_known_claude_code_placeholder_text(&normalized)
        && is_ready_for_claude_code_text(observation)
        && observation
            .snapshot
            .screen_identity
            .as_ref()
            .and_then(|identity| identity.permission_mode.as_deref())
            .is_some()
}

fn normalize_claude_code_placeholder_text(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn is_known_claude_code_placeholder_text(normalized: &str) -> bool {
    matches!(
        normalized,
        "try \"fix lint errors\"" | "try \"how does <filepath> work?\""
    )
}

fn claude_code_staged_command_matches(observation: &ClaudeCodeObservation, command: &str) -> bool {
    claude_code_composer_text(observation).is_some_and(|text| {
        text == command && !claude_code_composer_text_is_placeholder(observation, &text)
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

fn claude_code_deep_research_runtime_dir() -> PathBuf {
    if let Ok(root) = std::env::var("MISSIOND_RUNTIME_DIR") {
        let root = root.trim();
        if !root.is_empty() {
            return PathBuf::from(root).join("claude-code-deep-research");
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(CLAUDE_CODE_DEEP_RESEARCH_RUNTIME_DIR)
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

fn claude_code_prompt_authorization_allowlist() -> HashSet<String> {
    let mut allowlist = HashSet::from(["mcp:missiond".to_string(), "mcp:supabase".to_string()]);
    add_claude_code_authorization_allowlist_from_runtime(&mut allowlist);
    for key in [
        "MISSIOND_PROVIDER_BOX_CLAUDE_CODE_AUTH_ALLOWLIST",
        "MISSIOND_PROVIDER_BOX_CLAUDE_CODE_MCP_AUTH_ALLOWLIST",
    ] {
        let Ok(value) = std::env::var(key) else {
            continue;
        };
        for item in value.split([',', ';', '\n']) {
            let normalized = normalize_claude_code_authorization_token(item);
            if !normalized.is_empty() {
                allowlist.insert(normalized);
            }
        }
    }
    allowlist
}

fn add_claude_code_authorization_allowlist_from_runtime(allowlist: &mut HashSet<String>) {
    for path in [
        Path::new(".missiond/v3/runtime/compiled/compiled-runtime-workstation.json"),
        Path::new(".missiond/v3/runtime/compiled/compiled-runtime-config.json"),
    ] {
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&content) else {
            continue;
        };
        let pools = [
            value.pointer("/payload/config/workstation_pool"),
            value.pointer("/payload/workstation/workstation_pool"),
        ];
        for pool in pools.into_iter().flatten() {
            let Some(workers) = pool.as_array() else {
                continue;
            };
            for worker in workers {
                if !worker
                    .get("engine")
                    .and_then(Value::as_str)
                    .is_some_and(|engine| engine.eq_ignore_ascii_case("claude-code"))
                {
                    continue;
                }
                let Some(items) = worker
                    .get("provider_authorization_allowlist")
                    .and_then(Value::as_array)
                else {
                    continue;
                };
                for item in items {
                    let Some(item) = item.as_str() else {
                        continue;
                    };
                    let normalized = normalize_claude_code_authorization_token(item);
                    if !normalized.is_empty() {
                        allowlist.insert(normalized);
                    }
                }
            }
        }
    }
}

fn normalize_claude_code_authorization_token(value: &str) -> String {
    value
        .trim()
        .trim_matches(|ch: char| {
            ch.is_ascii_whitespace()
                || matches!(ch, '"' | '\'' | '`' | ':' | ',' | ';' | '.' | '，' | '。')
        })
        .to_ascii_lowercase()
        .replace('_', "-")
}

fn extract_claude_code_new_mcp_server_target(text: &str) -> Option<String> {
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        let Some(pos) = lower.find("new mcp server found in this project") else {
            continue;
        };
        let rest = &line[pos + "new mcp server found in this project".len()..];
        let target = rest.trim().strip_prefix(':').unwrap_or(rest.trim()).trim();
        let normalized = normalize_claude_code_authorization_token(target);
        if !normalized.is_empty() {
            return Some(normalized);
        }
    }
    None
}

fn claude_code_prompt_authorization_surface(
    observation: &ClaudeCodeObservation,
) -> Option<ClaudeCodePromptAuthorization> {
    let signals = observation.snapshot.screen_signals.as_ref()?;
    let kind = signals.startup_prompt_kind.as_deref()?;
    if kind != "mcp_server_authorization" {
        return None;
    }
    let visible_options = signals.visible_startup_options.clone();
    let allow_option_index = visible_options
        .iter()
        .position(|option| claude_code_authorization_option_is_allow(option))
        .map(|index| index + 1);
    let allow_option =
        allow_option_index.and_then(|index| visible_options.get(index.saturating_sub(1)).cloned());

    Some(ClaudeCodePromptAuthorization {
        kind: kind.to_string(),
        target: extract_claude_code_new_mcp_server_target(&observation.text),
        selected_option_index: signals.selected_startup_option_index.map(usize::from),
        selected_option: signals.selected_startup_option.clone(),
        allow_option_index,
        allow_option,
        visible_options,
    })
}

fn claude_code_authorization_option_is_allow(option: &str) -> bool {
    let lower = option.to_ascii_lowercase();
    let denyish = lower.contains("without")
        || lower.contains("don't")
        || lower.contains("do not")
        || lower.contains("deny")
        || lower.contains("reject")
        || lower.contains("no ");
    !denyish
        && (lower.contains("use this mcp server")
            || lower.contains("allow")
            || lower.contains("approve")
            || lower.contains("yes")
            || lower.contains("trust"))
}

fn claude_code_prompt_authorization_allowed(
    surface: &ClaudeCodePromptAuthorization,
    allowlist: &HashSet<String>,
) -> bool {
    let Some(target) = surface.target.as_deref() else {
        return false;
    };
    let scoped = format!("mcp:{target}");
    allowlist.contains("*")
        || allowlist.contains("mcp:*")
        || allowlist.contains(target)
        || allowlist.contains(&scoped)
}

fn is_claude_code_prompt_authorization_prompt(observation: &ClaudeCodeObservation) -> bool {
    claude_code_prompt_authorization_surface(observation).is_some()
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
        if analysis.upstream_error.is_none() {
            if let Some(upstream_error) = claude_code_jsonl_event_upstream_error(&event) {
                analysis.upstream_error = Some(upstream_error);
                continue;
            }
        }
        if let Some(final_text) = claude_code_assistant_end_turn_text(&event) {
            if !final_text.trim().is_empty() {
                analysis.final_text = Some(final_text.trim().to_string());
            }
        }
    }
    analysis
}

enum OutputContractFileState {
    MissingOrTooSmall,
    Pending { len: u64 },
    Stable { len: u64 },
}

fn output_contract_must_write_file(request: &ProviderInteractionRequest) -> Option<PathBuf> {
    request
        .output_contract
        .as_ref()
        .and_then(|contract| contract.get("must_write_file"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn output_contract_file_completion_state(
    path: &Path,
    previous: Option<&(u64, Instant)>,
) -> OutputContractFileState {
    let Ok(metadata) = fs::metadata(path) else {
        return OutputContractFileState::MissingOrTooSmall;
    };
    if !metadata.is_file() || metadata.len() < OUTPUT_CONTRACT_FILE_MIN_BYTES {
        return OutputContractFileState::MissingOrTooSmall;
    }
    let len = metadata.len();
    if let Some((previous_len, seen_at)) = previous {
        if *previous_len == len
            && seen_at.elapsed() >= Duration::from_secs(OUTPUT_CONTRACT_FILE_STABLE_SECS)
        {
            return OutputContractFileState::Stable { len };
        }
    }
    OutputContractFileState::Pending { len }
}

fn analyze_claude_code_deep_research_after_cursor(
    claude_home: &Path,
    session_id: &str,
    cursor: &ClaudeCodeJsonlCursor,
) -> ClaudeCodeDeepResearchAnalysis {
    let path = cursor
        .path
        .clone()
        .filter(|path| path.exists())
        .or_else(|| find_claude_code_session_jsonl(claude_home, session_id));
    let Some(path) = path else {
        return ClaudeCodeDeepResearchAnalysis::default();
    };

    let mut file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(_) => return ClaudeCodeDeepResearchAnalysis::default(),
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
        return ClaudeCodeDeepResearchAnalysis::default();
    }

    let mut analysis = ClaudeCodeDeepResearchAnalysis {
        jsonl_path: Some(path),
        ..Default::default()
    };
    let text = String::from_utf8_lossy(&bytes);
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        analysis.line_count += 1;
        if analysis.upstream_error.is_none() {
            if let Some(upstream_error) = claude_code_jsonl_event_upstream_error(&event) {
                analysis.upstream_error = Some(upstream_error);
                continue;
            }
        }
        if analysis.launch.is_none() {
            analysis.launch = claude_code_workflow_launch_from_event(&event);
        }
    }
    analysis
}

fn claude_code_workflow_launch_from_event(event: &Value) -> Option<ClaudeCodeWorkflowLaunch> {
    let launch = find_workflow_launch_value(event)?;
    let status = launch.get("status").and_then(Value::as_str)?;
    if status != "async_launched" {
        return None;
    }
    if !claude_code_workflow_launch_is_deep_research(launch) {
        return None;
    }
    let run_id = launch
        .get("runId")
        .or_else(|| launch.get("run_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let transcript_dir = launch
        .get("transcriptDir")
        .or_else(|| launch.get("transcript_dir"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    let task_id = launch
        .get("taskId")
        .or_else(|| launch.get("task_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let script_path = launch
        .get("scriptPath")
        .or_else(|| launch.get("script_path"))
        .and_then(Value::as_str)
        .map(PathBuf::from);
    Some(ClaudeCodeWorkflowLaunch {
        task_id,
        run_id,
        transcript_dir,
        script_path,
    })
}

fn claude_code_workflow_launch_is_deep_research(launch: &Value) -> bool {
    let mut haystack = String::new();
    for key in [
        "name",
        "workflow",
        "summary",
        "description",
        "scriptPath",
        "script_path",
    ] {
        if let Some(value) = launch.get(key).and_then(Value::as_str) {
            haystack.push_str(value);
            haystack.push('\n');
        }
    }
    let normalized = haystack.to_ascii_lowercase();
    normalized.contains("deep-research") || normalized.contains("deep research")
}

fn find_workflow_launch_value(value: &Value) -> Option<&Value> {
    match value {
        Value::Object(map) => {
            if map
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|status| status == "async_launched")
                && (map.get("runId").is_some() || map.get("run_id").is_some())
                && (map.get("transcriptDir").is_some() || map.get("transcript_dir").is_some())
            {
                return Some(value);
            }
            for (key, child) in map {
                if key == "toolUseResult" {
                    if let Some(found) = find_workflow_launch_value(child) {
                        return Some(found);
                    }
                }
            }
            map.values().find_map(find_workflow_launch_value)
        }
        Value::Array(items) => items.iter().find_map(find_workflow_launch_value),
        _ => None,
    }
}

fn read_claude_code_workflow_journal_report(
    transcript_dir: &Path,
) -> ClaudeCodeWorkflowJournalReport {
    let journal_path = transcript_dir.join("journal.jsonl");
    let Ok(text) = fs::read_to_string(&journal_path) else {
        return ClaudeCodeWorkflowJournalReport::default();
    };
    let mut report = ClaudeCodeWorkflowJournalReport {
        journal_path: Some(journal_path),
        ..Default::default()
    };
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        report.line_count += 1;
        if event.get("type").and_then(Value::as_str) != Some("result") {
            continue;
        }
        let Some(result) = event.get("result") else {
            continue;
        };
        let has_report_shape = result.get("summary").is_some()
            && result
                .get("findings")
                .and_then(Value::as_array)
                .is_some_and(|findings| !findings.is_empty())
            && result.get("caveats").is_some();
        if has_report_shape {
            report.report = Some(result.clone());
        }
    }
    report
}

fn claude_code_deep_research_report_to_markdown(report: &Value) -> String {
    let mut rendered = Vec::new();
    if let Some(summary) = report.get("summary").and_then(Value::as_str) {
        rendered.push("## Summary".to_string());
        rendered.push(summary.trim().to_string());
    }

    if let Some(findings) = report.get("findings").and_then(Value::as_array) {
        rendered.push("## Findings".to_string());
        for (index, finding) in findings.iter().enumerate() {
            let claim = finding
                .get("claim")
                .and_then(Value::as_str)
                .unwrap_or("Untitled finding")
                .trim();
            let confidence = finding
                .get("confidence")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .trim();
            rendered.push(format!("{}. **{}** ({})", index + 1, claim, confidence));
            if let Some(evidence) = finding.get("evidence").and_then(Value::as_str) {
                rendered.push(format!("Evidence: {}", evidence.trim()));
            }
            if let Some(vote) = finding.get("vote").and_then(Value::as_str) {
                rendered.push(format!("Vote: {}", vote.trim()));
            }
            if let Some(sources) = finding.get("sources").and_then(Value::as_array) {
                let urls = sources
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>();
                if !urls.is_empty() {
                    rendered.push(format!("Sources: {}", urls.join(", ")));
                }
            }
        }
    }

    if let Some(caveats) = report.get("caveats").and_then(Value::as_str) {
        rendered.push("## Caveats".to_string());
        rendered.push(caveats.trim().to_string());
    }

    if let Some(open_questions) = report.get("openQuestions").and_then(Value::as_array) {
        let questions = open_questions
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if !questions.is_empty() {
            rendered.push("## Open Questions".to_string());
            for question in questions {
                rendered.push(format!("- {question}"));
            }
        }
    }

    rendered.join("\n\n")
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

fn claude_code_jsonl_event_upstream_error(event: &Value) -> Option<Value> {
    if event
        .get("isApiErrorMessage")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Some(event.clone());
    }

    let error = event.get("error")?;
    match error {
        Value::Object(map) => {
            if map
                .get("status")
                .and_then(Value::as_i64)
                .is_some_and(|status| status >= 400)
            {
                return Some(event.clone());
            }
            if map
                .get("formatted")
                .and_then(Value::as_str)
                .is_some_and(|formatted| {
                    let formatted = formatted.to_ascii_lowercase();
                    formatted.contains("overloaded")
                        || formatted.contains("rate limit")
                        || formatted.contains("server error")
                        || formatted.contains("api error")
                })
            {
                return Some(event.clone());
            }
        }
        Value::String(error) => {
            let error = error.to_ascii_lowercase();
            if error.contains("server_error")
                || error.contains("overloaded")
                || error.contains("rate limit")
                || error.contains("api error")
            {
                return Some(event.clone());
            }
        }
        _ => {}
    }
    None
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

fn durable_final_missing_idle_grace_elapsed(
    idle_elapsed: Duration,
    monitor_elapsed: Duration,
) -> bool {
    idle_elapsed >= Duration::from_secs(DURABLE_FINAL_IDLE_GRACE_SECS)
        && monitor_elapsed >= Duration::from_secs(PROMPT_SUBMISSION_IDLE_GRACE_SECS)
}

fn should_fail_missing_durable_final_on_idle(
    output_contract_file_required: bool,
    idle_elapsed: Option<Duration>,
    monitor_elapsed: Duration,
) -> bool {
    !output_contract_file_required
        && idle_elapsed.is_some_and(|elapsed| {
            durable_final_missing_idle_grace_elapsed(elapsed, monitor_elapsed)
        })
}

fn output_contract_file_idle_grace_elapsed(
    idle_elapsed: Option<Duration>,
    monitor_elapsed: Duration,
) -> bool {
    idle_elapsed.is_some_and(|elapsed| {
        elapsed >= Duration::from_secs(OUTPUT_CONTRACT_FILE_IDLE_CONTINUE_GRACE_SECS)
            && monitor_elapsed >= Duration::from_secs(PROMPT_SUBMISSION_IDLE_GRACE_SECS)
    })
}

fn should_continue_file_contract_worker_on_idle(
    idle_elapsed: Option<Duration>,
    monitor_elapsed: Duration,
    continue_attempts: usize,
) -> bool {
    continue_attempts < OUTPUT_CONTRACT_FILE_MAX_CONTINUE_ATTEMPTS
        && output_contract_file_idle_grace_elapsed(idle_elapsed, monitor_elapsed)
}

fn file_contract_continue_prompt(path: &Path) -> String {
    format!(
        "你还没有完成 output_contract.must_write_file。请不要等待用户确认，继续完成当前任务，并把结果写入这个同一个 Markdown 文件：{}\n\
只收集和用户问题直接相关的证据；一旦证据足以回答，就立即写文件。文件至少包含：结论、关键证据、来源/路径、仍不确定的点。不要只在对话里总结，不要写到其他文件。",
        path.display()
    )
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
    use std::collections::HashSet;
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use missiond_core::pty::{recognize_screen, PtyCanonicalState};
    use missiond_core::types::CliEngine;
    use missiond_core::SessionState;
    use serde_json::json;

    use crate::provider_box::types::{BoxCommand, ProviderInteractionRequest};

    use super::{
        analyze_claude_code_jsonl_after_cursor, claude_code_composer_blocking_text,
        claude_code_deep_research_report_to_markdown, claude_code_jsonl_cursor_for_session,
        claude_code_jsonl_event_is_text_only_violation, claude_code_mcp_detail_action_positions,
        claude_code_mcp_reconnect_action_selected, claude_code_mcp_reconnect_action_visible,
        claude_code_mcp_reconnect_line, claude_code_mcp_reconnect_outcome,
        claude_code_mcp_status_value, claude_code_permission_cycle_steps,
        claude_code_prompt_authorization_allowed, claude_code_prompt_authorization_surface,
        claude_code_provider_capabilities, claude_code_staged_command_matches,
        claude_code_workflow_launch_from_event, durable_final_missing_idle_grace_elapsed,
        extract_claude_code_mcp_server_entries_from_screen, file_contract_continue_prompt,
        find_claude_code_session_jsonl, is_claude_code_logout_success,
        normalize_claude_code_model_target, normalize_claude_code_permission_mode,
        output_contract_file_completion_state, output_contract_must_write_file,
        read_claude_code_workflow_journal_report, should_continue_file_contract_worker_on_idle,
        should_fail_missing_durable_final_on_idle, ClaudeCodeJsonlCursor, ClaudeCodeModelTarget,
        ClaudeCodeObservation, ClaudeCodePermissionMode, OutputContractFileState,
        DURABLE_FINAL_IDLE_GRACE_SECS, OUTPUT_CONTRACT_FILE_IDLE_CONTINUE_GRACE_SECS,
        OUTPUT_CONTRACT_FILE_MAX_CONTINUE_ATTEMPTS, OUTPUT_CONTRACT_FILE_STABLE_SECS,
        PROMPT_SUBMISSION_IDLE_GRACE_SECS,
    };

    fn observation(lines: &[&str]) -> ClaudeCodeObservation {
        observation_with_session_state(lines, SessionState::Idle)
    }

    fn observation_with_session_state(
        lines: &[&str],
        session_state: SessionState,
    ) -> ClaudeCodeObservation {
        let owned = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        ClaudeCodeObservation {
            lines: owned.clone(),
            text: owned.join("\n"),
            session_state,
            snapshot: recognize_screen(CliEngine::ClaudeCode, &owned, session_state),
        }
    }

    #[test]
    fn claude_code_placeholder_composer_is_treated_as_empty_input() {
        let observation = observation(&[
            "⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents",
            "────────────────────────────────────────────────────────────────",
            "❯ Try \"edit <filepath> to...\"",
            "────────────────────────────────────────────────────────────────",
            "  ▘▘ ▝▝    ~/.xjp-mission/releases/current/source",
            "▝▜█████▛▘  Opus 4.8 (1M context) · Claude Max",
            " ▐▛███▜▌   Claude Code v2.1.161",
        ]);

        assert_eq!(claude_code_composer_text(&observation).as_deref(), Some(""));
        assert!(!claude_code_staged_command_matches(&observation, "/mcp"));
    }

    #[test]
    fn claude_code_real_composer_text_is_preserved() {
        let observation = observation(&[
            " ▐▛███▜▌   Claude Code v2.1.161",
            "▝▜█████▛▘  Opus 4.8 (1M context) · Claude Max",
            "  ▘▘ ▝▝    ~/Projects/missiond",
            "❯ /mcp",
            "⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents",
        ]);

        assert_eq!(
            claude_code_composer_text(&observation).as_deref(),
            Some("/mcp")
        );
        assert!(claude_code_staged_command_matches(&observation, "/mcp"));
    }

    #[test]
    fn claude_code_driver_capabilities_expose_mcp_status_and_reconnect() {
        let caps = claude_code_provider_capabilities();

        assert!(caps.submit_turn);
        assert!(caps.mcp_status);
        assert!(caps.mcp_reconnect);
        assert!(caps.status);
        assert!(caps.switch_model);
        assert!(caps.control_action);
    }

    #[test]
    fn must_write_file_output_contract_resolves_path() {
        let mut request =
            ProviderInteractionRequest::new(BoxCommand::WorkerTurn, CliEngine::ClaudeCode);
        request.output_contract = Some(json!({
            "media_type": "text/markdown",
            "must_write_file": "/tmp/missiond-grounding-report.md"
        }));

        assert_eq!(
            output_contract_must_write_file(&request),
            Some(PathBuf::from("/tmp/missiond-grounding-report.md"))
        );
    }

    #[test]
    fn output_contract_file_completion_requires_stable_size() {
        let path = std::env::temp_dir().join(format!(
            "missiond-output-contract-{}.md",
            uuid::Uuid::new_v4()
        ));
        assert!(matches!(
            output_contract_file_completion_state(&path, None),
            OutputContractFileState::MissingOrTooSmall
        ));

        std::fs::write(&path, "short").expect("write short output contract file");
        assert!(matches!(
            output_contract_file_completion_state(&path, None),
            OutputContractFileState::MissingOrTooSmall
        ));

        std::fs::write(
            &path,
            "## Evidence\nThis grounding report has enough bytes to satisfy the contract.\n",
        )
        .expect("write output contract file");
        let pending = output_contract_file_completion_state(&path, None);
        let len = match pending {
            OutputContractFileState::Pending { len } => len,
            _ => panic!("expected pending file state"),
        };
        assert!(matches!(
            output_contract_file_completion_state(
                &path,
                Some(&(len, Instant::now() - Duration::from_secs(OUTPUT_CONTRACT_FILE_STABLE_SECS + 1)))
            ),
            OutputContractFileState::Stable { len: stable_len } if stable_len == len
        ));
        std::fs::write(
            &path,
            "## Evidence\nThis grounding report changed size and must be observed again before completion.\n",
        )
        .expect("rewrite output contract file");
        assert!(matches!(
            output_contract_file_completion_state(
                &path,
                Some(&(
                    len,
                    Instant::now() - Duration::from_secs(OUTPUT_CONTRACT_FILE_STABLE_SECS + 1)
                ))
            ),
            OutputContractFileState::Pending { .. }
        ));
        let _ = std::fs::remove_file(&path);
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
    fn claude_code_placeholder_composer_does_not_block_provider_controls() {
        let obs = observation(&[
            " ▐▛███▜▌   Claude Code v2.1.160",
            "▝▜█████▛▘  Sonnet 4.6 with high effort · Claude Max",
            "  ▘▘ ▝▝    ~/Projects/missiond",
            "────────────────────────────────────────────────────────────────",
            "❯ Try \"fix lint errors\"",
            "────────────────────────────────────────────────────────────────",
            "  ⏵⏵ auto mode on (shift+tab to cycle)",
        ]);

        assert_eq!(obs.snapshot.reason, "claude_code:prompt_idle");
        assert!(claude_code_composer_blocking_text(&obs).is_none());
        assert!(!claude_code_staged_command_matches(
            &obs,
            "Try \"fix lint errors\""
        ));
    }

    #[test]
    fn claude_code_filepath_placeholder_composer_does_not_block_provider_controls() {
        let obs = observation(&[
            " ▐▛███▜▌   Claude Code v2.1.161",
            "▝▜█████▛▘  Opus 4.8 with xhigh effort · Claude Max",
            "  ▘▘ ▝▝    ~/Projects/missiond",
            "────────────────────────────────────────────────────────────────",
            "❯ Try \"how does <filepath> work?\"",
            "────────────────────────────────────────────────────────────────",
            "  ⏵⏵ auto mode on (shift+tab to cycle)",
        ]);

        assert_eq!(obs.snapshot.reason, "claude_code:prompt_idle");
        assert!(claude_code_composer_blocking_text(&obs).is_none());
        assert!(!claude_code_staged_command_matches(
            &obs,
            "Try \"how does <filepath> work?\""
        ));
    }

    #[test]
    fn claude_code_real_composer_text_still_blocks_provider_controls() {
        let obs = observation(&[
            " ▐▛███▜▌   Claude Code v2.1.160",
            "▝▜█████▛▘  Sonnet 4.6 with high effort · Claude Max",
            "  ▘▘ ▝▝    ~/Projects/missiond",
            "────────────────────────────────────────────────────────────────",
            "❯ please keep this draft",
            "────────────────────────────────────────────────────────────────",
            "  ⏵⏵ auto mode on (shift+tab to cycle)",
        ]);

        assert_eq!(
            claude_code_composer_blocking_text(&obs).as_deref(),
            Some("please keep this draft")
        );
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
    fn claude_code_mcp_parser_extracts_list_rows() {
        let obs = observation(&[
            "  Manage MCP servers",
            "  12 servers",
            "  ⚠ 2 setup issues: MCP · /doctor",
            "    Local MCPs (/Users/jinchen/.claude.json [project: /private/tmp/missiond-search-noise-fix])",
            "  ❯ chrome-devtools · ✔ connected · 29 tools",
            "    missiond-fail-demo · ✘ failed",
            "    claude.ai Gmail · △ needs authentication",
            "    computer-use · ◯ disabled",
            "    missiond · ✔ connected · 100 tools",
            "  ※ Run claude --debug to see error logs",
            "  https://code.claude.com/docs/en/mcp for help",
        ]);
        let entries = extract_claude_code_mcp_server_entries_from_screen(&obs.text);
        assert_eq!(entries.len(), 5);
        assert!(!entries
            .iter()
            .any(|entry| entry.name.to_ascii_lowercase().contains("setup issue")));
        assert_eq!(entries[0].name, "chrome-devtools");
        assert!(entries[0].selected);
        assert!(entries[0].connected);
        assert_eq!(entries[1].status, "failed");
        assert_eq!(entries[2].status, "needs_authentication");
        assert_eq!(entries[3].status, "disabled");
        assert_eq!(entries[4].tools_summary.as_deref(), Some("100 tools"));
    }

    #[test]
    fn claude_code_mcp_status_value_marks_target_failed() {
        let obs = observation(&[
            "  Manage MCP servers",
            "  ❯ missiond-fail-demo · ✘ failed",
            "    missiond · ✔ connected · 100 tools",
            "  https://code.claude.com/docs/en/mcp for help",
        ]);
        let entries = extract_claude_code_mcp_server_entries_from_screen(&obs.text);
        let request = ProviderInteractionRequest::new(BoxCommand::McpStatus, CliEngine::ClaudeCode);
        let value = claude_code_mcp_status_value(
            &request,
            "slot-claude-code-default",
            "missiond-fail-demo",
            &entries,
            &obs,
        );
        assert_eq!(
            value.pointer("/status").and_then(|v| v.as_str()),
            Some("target_failed")
        );
        assert_eq!(
            value.pointer("/target_status").and_then(|v| v.as_str()),
            Some("failed")
        );
        assert_eq!(
            value.pointer("/target_connected").and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn claude_code_prompt_authorization_extracts_allowlisted_mcp_target() {
        let obs = observation(&[
            "New MCP server found in this project: supabase",
            "MCP servers may execute code or access system resources.",
            "❯ 1. Use this MCP server",
            "  2. Continue without using this MCP server",
        ]);
        let surface =
            claude_code_prompt_authorization_surface(&obs).expect("provider authorization surface");

        assert_eq!(surface.kind, "mcp_server_authorization");
        assert_eq!(surface.target.as_deref(), Some("supabase"));
        assert_eq!(surface.selected_option_index, Some(1));
        assert_eq!(surface.allow_option_index, Some(1));
        assert_eq!(surface.allow_option.as_deref(), Some("Use this MCP server"));

        let allowlist = HashSet::from(["mcp:supabase".to_string()]);
        assert!(claude_code_prompt_authorization_allowed(
            &surface, &allowlist
        ));
    }

    #[test]
    fn claude_code_prompt_authorization_rejects_unknown_mcp_target() {
        let obs = observation(&[
            "New MCP server found in this project: unknown-prod",
            "MCP servers may execute code or access system resources.",
            "  1. Use this MCP server",
            "❯ 2. Continue without using this MCP server",
        ]);
        let surface =
            claude_code_prompt_authorization_surface(&obs).expect("provider authorization surface");

        assert_eq!(surface.target.as_deref(), Some("unknown-prod"));
        assert_eq!(surface.selected_option_index, Some(2));
        assert_eq!(surface.allow_option_index, Some(1));

        let allowlist = HashSet::from(["mcp:supabase".to_string()]);
        assert!(!claude_code_prompt_authorization_allowed(
            &surface, &allowlist
        ));
    }

    #[test]
    fn claude_code_mcp_reconnect_outcome_parses_failure_banner() {
        let text = "❯ /mcp\n  ⎿  Failed to reconnect to missiond-fail-demo: -32000\n❯";
        assert_eq!(
            claude_code_mcp_reconnect_outcome(text, "missiond-fail-demo").as_deref(),
            Some("failed")
        );
        assert_eq!(
            claude_code_mcp_reconnect_line(text, "missiond-fail-demo").as_deref(),
            Some("⎿  Failed to reconnect to missiond-fail-demo: -32000")
        );
    }

    #[test]
    fn claude_code_mcp_detail_actions_detect_reconnect_position() {
        let obs = observation(&[
            "  Missiond MCP Server",
            "",
            "  Status:           ✔ connected",
            "  Command:          /Users/jinchen/.xjp-mission/mission-mcp",
            "  Tools: 100 tools",
            "",
            "  ❯ 1. View tools",
            "    2. Reconnect",
            "    3. Disable",
            "",
            "  ↑/↓ to navigate · Enter to select · Esc to back",
        ]);

        assert!(claude_code_mcp_reconnect_action_visible(&obs));
        assert!(!claude_code_mcp_reconnect_action_selected(&obs));
        assert_eq!(
            claude_code_mcp_detail_action_positions(&obs, "reconnect"),
            Some((0, 1))
        );

        let obs = observation(&[
            "  Missiond MCP Server",
            "    1. View tools",
            "  ❯ 2. Reconnect",
            "    3. Disable",
        ]);
        assert!(claude_code_mcp_reconnect_action_visible(&obs));
        assert!(claude_code_mcp_reconnect_action_selected(&obs));
        assert_eq!(
            claude_code_mcp_detail_action_positions(&obs, "reconnect"),
            Some((1, 1))
        );
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
    fn claude_code_workflow_launch_is_extracted_from_tool_result_event() {
        let event = serde_json::json!({
            "type": "user",
            "toolUseResult": {
                "status": "async_launched",
                "taskId": "w82yur346",
                "runId": "wf_7d30c912-5c6",
                "summary": "Deep research harness — fan-out web searches, fetch sources, adversarially verify claims, synthesize a cited report.",
                "transcriptDir": "/tmp/session/subagents/workflows/wf_7d30c912-5c6",
                "scriptPath": "/tmp/session/workflows/scripts/deep-research.js"
            }
        });

        let launch = claude_code_workflow_launch_from_event(&event).expect("launch");

        assert_eq!(launch.task_id.as_deref(), Some("w82yur346"));
        assert_eq!(launch.run_id, "wf_7d30c912-5c6");
        assert_eq!(
            launch.transcript_dir,
            std::path::PathBuf::from("/tmp/session/subagents/workflows/wf_7d30c912-5c6")
        );
        assert_eq!(
            launch.script_path.as_deref(),
            Some(std::path::Path::new(
                "/tmp/session/workflows/scripts/deep-research.js"
            ))
        );
    }

    #[test]
    fn claude_code_workflow_launch_ignores_non_deep_research_workflow() {
        let event = serde_json::json!({
            "type": "user",
            "toolUseResult": {
                "status": "async_launched",
                "taskId": "w-other",
                "runId": "wf_other",
                "summary": "Generic implementation workflow",
                "transcriptDir": "/tmp/session/subagents/workflows/wf_other",
                "scriptPath": "/tmp/session/workflows/scripts/implementation.js"
            }
        });

        assert!(claude_code_workflow_launch_from_event(&event).is_none());
    }

    #[test]
    fn claude_code_workflow_journal_report_extracts_final_synthesis_report() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workflow_dir = temp.path().join("wf_test");
        fs::create_dir_all(&workflow_dir).expect("workflow dir");
        fs::write(
            workflow_dir.join("journal.jsonl"),
            concat!(
                "{\"type\":\"result\",\"agentId\":\"scope\",\"result\":{\"summary\":\"scope only\",\"findings\":[]}}\n",
                "{\"type\":\"result\",\"agentId\":\"synth\",\"result\":{\"summary\":\"final summary\",\"findings\":[{\"claim\":\"claim one\",\"confidence\":\"high\",\"evidence\":\"evidence one\",\"sources\":[\"https://example.com/a\"],\"vote\":\"3-0\"}],\"caveats\":\"mind caveats\",\"openQuestions\":[\"what next?\"]}}\n"
            ),
        )
        .expect("journal");

        let report = read_claude_code_workflow_journal_report(&workflow_dir);
        let report_json = report.report.expect("report json");
        let markdown = claude_code_deep_research_report_to_markdown(&report_json);

        assert_eq!(report.line_count, 2);
        assert!(markdown.contains("## Summary"));
        assert!(markdown.contains("final summary"));
        assert!(markdown.contains("claim one"));
        assert!(markdown.contains("https://example.com/a"));
        assert!(markdown.contains("## Caveats"));
        assert!(markdown.contains("what next?"));
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
    fn durable_final_missing_idle_grace_ignores_initial_submission_idle() {
        assert!(!durable_final_missing_idle_grace_elapsed(
            Duration::from_secs(DURABLE_FINAL_IDLE_GRACE_SECS),
            Duration::from_secs(PROMPT_SUBMISSION_IDLE_GRACE_SECS - 1),
        ));
        assert!(durable_final_missing_idle_grace_elapsed(
            Duration::from_secs(DURABLE_FINAL_IDLE_GRACE_SECS),
            Duration::from_secs(PROMPT_SUBMISSION_IDLE_GRACE_SECS),
        ));
    }

    #[test]
    fn durable_final_missing_idle_grace_does_not_fail_file_contract_workers() {
        let idle_elapsed = Some(Duration::from_secs(DURABLE_FINAL_IDLE_GRACE_SECS + 10));
        let monitor_elapsed = Duration::from_secs(PROMPT_SUBMISSION_IDLE_GRACE_SECS + 10);

        assert!(!should_fail_missing_durable_final_on_idle(
            true,
            idle_elapsed,
            monitor_elapsed,
        ));
        assert!(should_fail_missing_durable_final_on_idle(
            false,
            idle_elapsed,
            monitor_elapsed,
        ));
    }

    #[test]
    fn file_contract_idle_grace_uses_bounded_continue_budget() {
        let idle_elapsed = Some(Duration::from_secs(
            OUTPUT_CONTRACT_FILE_IDLE_CONTINUE_GRACE_SECS,
        ));
        let monitor_elapsed = Duration::from_secs(PROMPT_SUBMISSION_IDLE_GRACE_SECS);

        assert!(should_continue_file_contract_worker_on_idle(
            idle_elapsed,
            monitor_elapsed,
            0,
        ));
        assert!(should_continue_file_contract_worker_on_idle(
            idle_elapsed,
            monitor_elapsed,
            OUTPUT_CONTRACT_FILE_MAX_CONTINUE_ATTEMPTS - 1,
        ));
        assert!(!should_continue_file_contract_worker_on_idle(
            idle_elapsed,
            monitor_elapsed,
            OUTPUT_CONTRACT_FILE_MAX_CONTINUE_ATTEMPTS,
        ));
    }

    #[test]
    fn file_contract_continue_prompt_preserves_target_path() {
        let target = PathBuf::from("/tmp/missiond-grounding-report.md");
        let prompt = file_contract_continue_prompt(&target);

        assert!(prompt.contains("/tmp/missiond-grounding-report.md"));
        assert!(prompt.contains("output_contract.must_write_file"));
        assert!(prompt.contains("不要只在对话里总结"));
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

    #[test]
    fn claude_code_jsonl_scanner_detects_upstream_api_errors() {
        let temp = tempfile::tempdir().expect("tempdir");
        let session_id = "019e82f0-0000-7000-8000-000000000004";
        let project_dir = temp.path().join("projects").join("missiond");
        fs::create_dir_all(&project_dir).expect("project dir");
        let jsonl = project_dir.join(format!("{session_id}.jsonl"));
        fs::write(
            &jsonl,
            concat!(
                "{\"type\":\"system\",\"error\":{\"status\":529,\"formatted\":\"529 Overloaded\",\"requestId\":\"req_1\"}}\n",
                "{\"type\":\"assistant\",\"isApiErrorMessage\":true,\"error\":\"server_error\",\"message\":{\"role\":\"assistant\",\"stop_reason\":\"stop_sequence\",\"content\":[{\"type\":\"text\",\"text\":\"API Error: 529 Overloaded\"}]}}\n"
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

        let upstream = analysis
            .upstream_error
            .expect("upstream API error should be detected");
        assert_eq!(
            upstream.pointer("/error/status").and_then(|v| v.as_i64()),
            Some(529)
        );
        assert!(analysis.violation.is_none());
        assert!(analysis.final_text.is_none());
    }
}
