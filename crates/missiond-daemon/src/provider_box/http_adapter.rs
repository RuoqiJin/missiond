use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use missiond_core::types::CliEngine;
use missiond_core::{ProviderBoxHttpRequest, ProviderBoxHttpResponse};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio::time::timeout;

use super::driver::ProviderDriverCapabilities;
use super::runtime::ProviderInteractionBox;
use super::types::{
    BoxCommand, ModelSwitchPolicy, ProviderBoxDiagnostic, ProviderBoxResult, ProviderBoxStatus,
    ProviderControlAction, ProviderInteractionRequest, ProviderModelCatalog,
    ProviderModelCatalogEntry, ProviderRouterExport, TimeoutCancelPolicy,
    DIAG_PROVIDER_BOX_AUTH_REQUIRED, DIAG_PROVIDER_BOX_INVALID_REQUEST,
};

const PTY_STEP_TEXT_LIMIT: usize = 4096;
const AGY_USAGE_PROBE_SLOT: &str = "slot-agy-usage-probe";
const CODEX_USAGE_PROBE_SLOT: &str = "slot-codex-usage-probe";
const CODEX_RESEARCH_PROVIDER: &str = "codex_research";
const CODEX_IMAGE_PROVIDER: &str = "codex_image_generation";
const CLAUDE_CODE_TEXT_PROVIDER: &str = "claude_code_text";
const CLAUDE_CODE_DEEP_RESEARCH_PROVIDER: &str = "claude_code_deep_research";
const CODEX_RESEARCH_PROMPT_PREFIX: &str = "帮我在互联网上进行详细调研以下问题：";
const CODEX_IMAGE_PROMPT_PREFIX: &str = "帮我生成一张图片，要求如下：";
const CODEX_IMAGE_STABLE_PROMPT_SUFFIX: &str = "执行要求：请直接调用并使用 imagegen 生成图片，必须实际产出图片文件，不要只描述图片或给出提示词。图片是预览用途，保留在 Codex 默认 generated_images 路径即可；不要复制、移动或写入当前工作区。生成完成后只回复 IMAGE_DONE。";
const CLAUDE_CODE_DEEP_RESEARCH_PROMPT_PREFIX: &str =
    "请调用 deep-research skill，并运行 deep-research workflow。不要只做普通回答。调研问题如下：";
const CODEX_TASK_MODEL: &str = "gpt-5.5";
const CLAUDE_CODE_DEEP_RESEARCH_MODEL_ID: &str = "claude-code-deep-research-opus-4-8-xhigh";
const CLAUDE_CODE_DEEP_RESEARCH_PROVIDER_MODEL: &str = "claude-opus-4-8";
const CLAUDE_CODE_DEEP_RESEARCH_MODEL_PROFILE: &str = "xhigh";
const CODEX_EXEC_TEXT_DEFAULT_MAX_CONCURRENT: usize = 4;
const CODEX_EXEC_TEXT_XHIGH_MAX_CONCURRENT: usize = 2;
const CODEX_EXEC_TASK_MAX_CONCURRENT: usize = 1;
const CLAUDE_CODE_TEXT_MAX_CONCURRENT: usize = 1;
const CLAUDE_CODE_DEEP_RESEARCH_MAX_CONCURRENT: usize = 1;
const MODEL_CATALOG_LIVE_TIMEOUT_SECS: u64 = 20;
const STATIC_AGY_TEXT_MODELS: &[&str] = &[
    "Gemini 3.5 Flash (Medium)",
    "Gemini 3.5 Flash (High)",
    "Gemini 3.5 Flash (Low)",
    "Gemini 3.1 Pro (Low)",
    "Gemini 3.1 Pro (High)",
    "Claude Sonnet 4.6 (Thinking)",
    "Claude Opus 4.6 (Thinking)",
];
const PTY_STEP_ALLOWED_KEYS: &[&str] = &[
    "enter",
    "return",
    "esc",
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
    "ctrl+d",
    "pageup",
    "page_up",
    "pagedown",
    "page_down",
    "home",
    "end",
];

#[derive(Clone)]
pub(crate) struct ProviderBoxHttpAdapter {
    boxed: Arc<ProviderInteractionBox>,
    internal_token: Option<String>,
    agy_slot_pool_cursors: Arc<Mutex<HashMap<String, usize>>>,
    agy_usage_cache: Arc<Mutex<Option<Value>>>,
    codex_usage_cache: Arc<Mutex<Option<Value>>>,
}

impl ProviderBoxHttpAdapter {
    pub(crate) fn new(boxed: Arc<ProviderInteractionBox>) -> Self {
        Self {
            boxed,
            internal_token: provider_box_internal_token(),
            agy_slot_pool_cursors: Arc::new(Mutex::new(HashMap::new())),
            agy_usage_cache: Arc::new(Mutex::new(None)),
            codex_usage_cache: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) async fn handle(
        &self,
        request: ProviderBoxHttpRequest,
    ) -> Result<ProviderBoxHttpResponse, String> {
        if let Err(response) = self.authorize(&request) {
            return Ok(response);
        }

        let path = request
            .path
            .split('?')
            .next()
            .unwrap_or(request.path.as_str());
        if request.method == "POST" && path == "/provider-box/v1/slots/spawn" {
            return self.handle_slot_spawn(request).await;
        }
        if let Some((slot_id, suffix)) = parse_slot_endpoint(path) {
            return match (request.method.as_str(), suffix.as_str()) {
                ("GET", "status") | ("POST", "status") => {
                    self.handle_slot_status(request, slot_id, false).await
                }
                ("GET", "session")
                | ("POST", "session")
                | ("GET", "session/identity")
                | ("POST", "session/identity")
                | ("GET", "actions/session")
                | ("POST", "actions/session") => {
                    self.handle_slot_status(request, slot_id, false).await
                }
                ("GET", "screen")
                | ("POST", "screen")
                | ("GET", "observe")
                | ("POST", "observe")
                | ("GET", "actions/observe")
                | ("POST", "actions/observe") => {
                    self.handle_slot_status(request, slot_id, false).await
                }
                ("POST", "input") | ("POST", "actions/input") => {
                    self.handle_slot_input(request, slot_id).await
                }
                ("POST", "clear-input") | ("POST", "actions/clear-input") => {
                    self.handle_slot_control(request, slot_id, ProviderControlAction::ClearInput)
                        .await
                }
                ("POST", "pty-step")
                | ("POST", "actions/pty-step")
                | ("POST", "key")
                | ("POST", "actions/key") => self.handle_slot_pty_step(request, slot_id).await,
                ("GET", "capabilities")
                | ("POST", "capabilities")
                | ("GET", "actions/capabilities")
                | ("POST", "actions/capabilities") => {
                    self.handle_slot_capabilities(request, slot_id).await
                }
                ("POST", "restart")
                | ("POST", "respawn")
                | ("POST", "actions/restart")
                | ("POST", "actions/respawn") => self.handle_slot_restart(request, slot_id).await,
                ("POST", "clear")
                | ("POST", "clear-screen")
                | ("POST", "actions/clear")
                | ("POST", "actions/clear-screen") => {
                    self.handle_slot_control(request, slot_id, ProviderControlAction::ClearScreen)
                        .await
                }
                ("POST", "exit") | ("POST", "actions/exit") => {
                    self.handle_slot_control(request, slot_id, ProviderControlAction::Exit)
                        .await
                }
                ("POST", "permissions")
                | ("POST", "permission")
                | ("POST", "actions/permissions")
                | ("POST", "actions/permission") => {
                    self.handle_slot_permissions(request, slot_id, None).await
                }
                ("POST", "fast") | ("POST", "actions/fast") => {
                    self.handle_slot_fast_mode(request, slot_id, None).await
                }
                _ if request.method == "POST"
                    && slot_permission_mode_raw_from_suffix(&suffix).is_some() =>
                {
                    let engine = engine_from_body_or_slot(&request.body, &slot_id);
                    let mode = slot_permission_mode_raw_from_suffix(&suffix).and_then(|raw| {
                        normalize_provider_box_permission_mode_for_engine(engine, &raw)
                    });
                    self.handle_slot_permissions(request, slot_id, mode).await
                }
                _ if request.method == "POST" && slot_fast_mode_from_suffix(&suffix).is_some() => {
                    let mode = slot_fast_mode_from_suffix(&suffix);
                    self.handle_slot_fast_mode(request, slot_id, mode).await
                }
                ("POST", "switch-model") | ("POST", "actions/switch-model") => {
                    self.handle_slot_switch_model(request, slot_id).await
                }
                ("POST", "usage") | ("POST", "usage/refresh") | ("POST", "actions/usage") => {
                    self.handle_slot_usage_refresh(request, slot_id).await
                }
                ("GET", "mcp")
                | ("GET", "mcp/status")
                | ("POST", "mcp/status")
                | ("GET", "actions/mcp/status")
                | ("POST", "actions/mcp/status") => {
                    self.handle_slot_mcp_status(request, slot_id).await
                }
                ("POST", "mcp/reconnect")
                | ("POST", "mcp/restart")
                | ("POST", "actions/mcp/reconnect")
                | ("POST", "actions/mcp/restart") => {
                    self.handle_slot_mcp_reconnect(request, slot_id).await
                }
                ("POST", "completions") | ("POST", "text-only/completions") => {
                    self.handle_slot_text_only_completion(request, slot_id)
                        .await
                }
                _ => Ok(json_response(
                    404,
                    json!({
                        "error": {
                            "message": "Unknown provider-box slot endpoint"
                        }
                    }),
                )),
            };
        }
        match (request.method.as_str(), path) {
            ("GET", "/provider-box/v1/models") => self.handle_models(request).await,
            ("GET", "/provider-box/v1/usage") => self.handle_usage_cache(request).await,
            ("POST", "/provider-box/v1/usage/refresh") => self.handle_usage_refresh(request).await,
            ("POST", "/provider-box/v1/turns") => self.handle_turn(request).await,
            ("POST", "/provider-box/v1/text-only/completions") => {
                self.handle_text_only_completion(request).await
            }
            ("POST", "/provider-box/v1/research")
            | ("POST", "/provider-box/v1/research/completions")
            | ("POST", "/provider-box/v1/research/chat/completions") => {
                if is_claude_code_deep_research_body(&request.body) {
                    self.handle_claude_code_deep_research_completion(request)
                        .await
                } else {
                    self.handle_codex_task_completion(request, BoxCommand::Research)
                        .await
                }
            }
            ("POST", "/provider-box/v1/claude-code/deep-research")
            | ("POST", "/provider-box/v1/claude-code/deep-research/completions")
            | ("POST", "/provider-box/v1/claude-code/deep-research/chat/completions") => {
                self.handle_claude_code_deep_research_completion(request)
                    .await
            }
            ("POST", "/provider-box/v1/image")
            | ("POST", "/provider-box/v1/image-generation")
            | ("POST", "/provider-box/v1/image/completions")
            | ("POST", "/provider-box/v1/image-generation/completions") => {
                self.handle_codex_task_completion(request, BoxCommand::ImageGeneration)
                    .await
            }
            _ => Ok(json_response(
                404,
                json!({
                    "error": {
                        "message": "Unknown provider-box endpoint"
                    }
                }),
            )),
        }
    }

    fn authorize(&self, request: &ProviderBoxHttpRequest) -> Result<(), ProviderBoxHttpResponse> {
        if std::env::var("MISSIOND_PROVIDER_BOX_ALLOW_NO_AUTH")
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        {
            return Ok(());
        }

        let Some(expected) = self.internal_token.as_deref() else {
            return Err(json_response(
                503,
                json!({
                    "error": {
                        "code": DIAG_PROVIDER_BOX_AUTH_REQUIRED,
                        "message": "Provider-box internal token is not configured"
                    }
                }),
            ));
        };
        let authorization = request
            .headers
            .get("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
            .map(str::trim);
        if authorization == Some(expected) {
            Ok(())
        } else {
            Err(json_response(
                401,
                json!({
                    "error": {
                        "code": DIAG_PROVIDER_BOX_AUTH_REQUIRED,
                        "message": "Provider-box request is missing a valid bearer token"
                    }
                }),
            ))
        }
    }

    async fn handle_slot_spawn(
        &self,
        request: ProviderBoxHttpRequest,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let slot_id = header_slot_id(&request)
            .or_else(|| string_field(&request.body, "slot_id"))
            .unwrap_or_else(|| "slot-agy-provider-box".to_string());
        self.handle_slot_status(request, slot_id, true).await
    }

    async fn handle_slot_status(
        &self,
        request: ProviderBoxHttpRequest,
        slot_id: String,
        spawn_if_missing: bool,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let mut interaction = ProviderInteractionRequest::new(
            BoxCommand::Status,
            engine_from_body_or_slot(&request.body, &slot_id),
        );
        interaction.provider = string_field(&request.body, "provider").or_else(|| {
            if interaction.engine == CliEngine::Agy {
                Some("agy_cli".to_string())
            } else {
                None
            }
        });
        interaction.slot_id = Some(slot_id);
        interaction.model = string_field(&request.body, "model");
        interaction.model_profile = string_field(&request.body, "model_profile")
            .or_else(|| string_field(&request.body, "reasoning_effort"))
            .or_else(|| string_field(&request.body, "model_reasoning_effort"));
        interaction.dangerously_bypass_approvals_and_sandbox =
            bool_field(&request.body, "dangerously_bypass_approvals_and_sandbox")
                .or_else(|| bool_field(&request.body, "dangerously_skip_permissions"))
                .or_else(|| bool_field(&request.body, "dangerously_bypass"))
                .or_else(|| bool_field(&request.body, "bypass_approvals_and_sandbox"))
                .or_else(|| bool_field(&request.body, "bypass_mode"))
                .unwrap_or(false);
        interaction.cwd = string_field(&request.body, "cwd");
        interaction.project_root = string_field(&request.body, "project_root");
        interaction.tool_policy = tool_policy_from_body(&request.body);
        interaction.correlation_id = string_field(&request.body, "correlation_id")
            .unwrap_or_else(|| interaction.correlation_id.clone());
        let body_spawn = bool_field(&request.body, "spawn_if_missing")
            .or_else(|| bool_field(&request.body, "spawn"))
            .unwrap_or(spawn_if_missing);
        interaction.desired_worker = Some(json!({
            "spawn_if_missing": body_spawn,
            "force_restart": bool_field(&request.body, "force_restart")
                .or_else(|| bool_field(&request.body, "restart"))
                .or_else(|| bool_field(&request.body, "respawn"))
                .unwrap_or(false),
        }));
        let result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        let mut result = result;
        result.capabilities = Some(provider_slot_capabilities_value(
            result.engine,
            &self.boxed.driver_capabilities(result.engine),
        ));
        Ok(result_response(result))
    }

    async fn handle_slot_capabilities(
        &self,
        request: ProviderBoxHttpRequest,
        slot_id: String,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let engine = engine_from_body_or_slot(&request.body, &slot_id);
        Ok(json_response(
            200,
            json!({
                "schema": "missiond.provider-box.slot-capabilities.v1",
                "status": "completed",
                "slot_id": slot_id,
                "engine": engine,
                "capabilities": provider_slot_capabilities_value(
                    engine,
                    &self.boxed.driver_capabilities(engine),
                )
            }),
        ))
    }

    async fn handle_slot_restart(
        &self,
        request: ProviderBoxHttpRequest,
        slot_id: String,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let engine = engine_from_body_or_slot(&request.body, &slot_id);
        let mut interaction = ProviderInteractionRequest::new(BoxCommand::Status, engine);
        interaction.provider = string_field(&request.body, "provider").or_else(|| {
            Some(
                match interaction.engine {
                    CliEngine::Codex => "codex_cli",
                    CliEngine::Agy => "agy_cli",
                    CliEngine::ClaudeCode => "claude_code",
                    CliEngine::Gemini => "gemini_cli",
                }
                .to_string(),
            )
        });
        interaction.slot_id = Some(slot_id.clone());
        interaction.model = string_field(&request.body, "model");
        interaction.model_profile = string_field(&request.body, "model_profile")
            .or_else(|| string_field(&request.body, "reasoning_effort"))
            .or_else(|| string_field(&request.body, "model_reasoning_effort"));
        interaction.dangerously_bypass_approvals_and_sandbox =
            bool_field(&request.body, "dangerously_bypass_approvals_and_sandbox")
                .or_else(|| bool_field(&request.body, "dangerously_skip_permissions"))
                .or_else(|| bool_field(&request.body, "dangerously_bypass"))
                .or_else(|| bool_field(&request.body, "bypass_approvals_and_sandbox"))
                .or_else(|| bool_field(&request.body, "bypass_mode"))
                .unwrap_or(false);
        interaction.cwd = string_field(&request.body, "cwd");
        interaction.project_root = string_field(&request.body, "project_root");
        interaction.tool_policy = tool_policy_from_body(&request.body);
        interaction.correlation_id = string_field(&request.body, "correlation_id")
            .unwrap_or_else(|| interaction.correlation_id.clone());
        interaction.desired_worker = Some(json!({
            "spawn_if_missing": true,
            "force_restart": true,
        }));

        if !bool_field(&request.body, "confirm_destroy_context")
            .or_else(|| bool_field(&request.body, "confirm_restart"))
            .or_else(|| bool_field(&request.body, "destroy_context_confirmed"))
            .unwrap_or(false)
        {
            let mut result = ProviderBoxResult::base(&interaction, ProviderBoxStatus::Blocked);
            result.slot_id = Some(slot_id);
            result.capabilities = Some(provider_slot_capabilities_value(
                result.engine,
                &self.boxed.driver_capabilities(result.engine),
            ));
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Slot restart requires confirm_destroy_context=true because the provider context may be valuable",
                json!({
                    "required": {
                        "confirm_destroy_context": true
                    },
                    "safe_alternative": "Call status/session/capabilities first, or ask the operator/user before destroying context."
                }),
            ));
            return Ok(result_response(result));
        }

        let result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        let mut result = result;
        result.capabilities = Some(provider_slot_capabilities_value(
            result.engine,
            &self.boxed.driver_capabilities(result.engine),
        ));
        Ok(result_response(result))
    }

    async fn handle_slot_input(
        &self,
        request: ProviderBoxHttpRequest,
        slot_id: String,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let mut interaction = ProviderInteractionRequest::new(
            BoxCommand::ControlAction,
            engine_from_body_or_slot(&request.body, &slot_id),
        );
        interaction.provider = string_field(&request.body, "provider").or_else(|| {
            if interaction.engine == CliEngine::Agy {
                Some("agy_cli".to_string())
            } else {
                None
            }
        });
        interaction.slot_id = Some(slot_id);
        interaction.cwd = string_field(&request.body, "cwd");
        interaction.project_root = string_field(&request.body, "project_root");
        interaction.control_action = Some(ProviderControlAction::Input);
        interaction.prompt = string_field(&request.body, "text")
            .or_else(|| string_field(&request.body, "input"))
            .or_else(|| string_field(&request.body, "prompt"));
        interaction.correlation_id = string_field(&request.body, "correlation_id")
            .unwrap_or_else(|| interaction.correlation_id.clone());
        interaction.desired_worker = Some(json!({
            "submit": bool_field(&request.body, "submit")
                .or_else(|| bool_field(&request.body, "enter"))
                .or_else(|| bool_field(&request.body, "append_enter"))
                .unwrap_or(false),
        }));
        let result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        Ok(result_response(result))
    }

    async fn handle_slot_pty_step(
        &self,
        request: ProviderBoxHttpRequest,
        slot_id: String,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let mut interaction = ProviderInteractionRequest::new(
            BoxCommand::PtyStep,
            engine_from_body_or_slot(&request.body, &slot_id),
        );
        interaction.provider = string_field(&request.body, "provider").or_else(|| {
            if interaction.engine == CliEngine::Agy {
                Some("agy_cli".to_string())
            } else {
                None
            }
        });
        interaction.slot_id = Some(slot_id);
        interaction.cwd = string_field(&request.body, "cwd");
        interaction.project_root = string_field(&request.body, "project_root");
        interaction.correlation_id = string_field(&request.body, "correlation_id")
            .unwrap_or_else(|| interaction.correlation_id.clone());
        let pty_step = match pty_step_payload_from_body(&request.body) {
            Ok(value) => value,
            Err(diagnostic) => {
                let mut result = ProviderBoxResult::base(&interaction, ProviderBoxStatus::Failed);
                result.add_diagnostic(diagnostic);
                return Ok(result_response(result));
            }
        };
        interaction.desired_worker = Some(json!({
            "spawn_if_missing": bool_field(&request.body, "spawn_if_missing")
                .or_else(|| bool_field(&request.body, "spawn"))
                .unwrap_or(false),
            "pty_step": pty_step,
        }));
        let result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        Ok(result_response(result))
    }

    async fn handle_slot_control(
        &self,
        request: ProviderBoxHttpRequest,
        slot_id: String,
        action: ProviderControlAction,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let mut interaction = ProviderInteractionRequest::new(
            BoxCommand::ControlAction,
            engine_from_body_or_slot(&request.body, &slot_id),
        );
        interaction.provider = string_field(&request.body, "provider").or_else(|| {
            if interaction.engine == CliEngine::Agy {
                Some("agy_cli".to_string())
            } else {
                None
            }
        });
        interaction.slot_id = Some(slot_id);
        interaction.cwd = string_field(&request.body, "cwd");
        interaction.project_root = string_field(&request.body, "project_root");
        interaction.control_action = Some(action);
        interaction.correlation_id = string_field(&request.body, "correlation_id")
            .unwrap_or_else(|| interaction.correlation_id.clone());
        let result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        Ok(result_response(result))
    }

    async fn handle_slot_permissions(
        &self,
        request: ProviderBoxHttpRequest,
        slot_id: String,
        mode_from_path: Option<String>,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let mut interaction = ProviderInteractionRequest::new(
            BoxCommand::ControlAction,
            engine_from_body_or_slot(&request.body, &slot_id),
        );
        interaction.provider = string_field(&request.body, "provider").or_else(|| {
            Some(
                match interaction.engine {
                    CliEngine::Codex => "codex_cli",
                    CliEngine::Agy => "agy_cli",
                    CliEngine::ClaudeCode => "claude_code",
                    CliEngine::Gemini => "gemini_cli",
                }
                .to_string(),
            )
        });
        interaction.slot_id = Some(slot_id);
        interaction.cwd = string_field(&request.body, "cwd");
        interaction.project_root = string_field(&request.body, "project_root");
        interaction.control_action = Some(ProviderControlAction::SetPermissions);
        interaction.correlation_id = string_field(&request.body, "correlation_id")
            .unwrap_or_else(|| interaction.correlation_id.clone());
        let raw_permission_mode = mode_from_path
            .or_else(|| string_field(&request.body, "permission_mode"))
            .or_else(|| string_field(&request.body, "mode"))
            .or_else(|| string_field(&request.body, "target_permission_mode"));
        let permission_mode = raw_permission_mode
            .as_deref()
            .and_then(|value| {
                normalize_provider_box_permission_mode_for_engine(interaction.engine, value)
            })
            .or(raw_permission_mode);
        interaction.desired_worker = Some(json!({
            "permission_mode": permission_mode,
            "spawn_if_missing": bool_field(&request.body, "spawn_if_missing")
                .or_else(|| bool_field(&request.body, "spawn"))
                .unwrap_or(true),
            "force_restart": bool_field(&request.body, "force_restart")
                .or_else(|| bool_field(&request.body, "restart"))
                .or_else(|| bool_field(&request.body, "respawn"))
                .unwrap_or(false),
        }));
        let result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        Ok(result_response(result))
    }

    async fn handle_slot_fast_mode(
        &self,
        request: ProviderBoxHttpRequest,
        slot_id: String,
        mode_from_path: Option<String>,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let mut interaction = ProviderInteractionRequest::new(
            BoxCommand::ControlAction,
            engine_from_body_or_slot(&request.body, &slot_id),
        );
        interaction.provider = string_field(&request.body, "provider").or_else(|| {
            Some(
                match interaction.engine {
                    CliEngine::Codex => "codex_cli",
                    CliEngine::Agy => "agy_cli",
                    CliEngine::ClaudeCode => "claude_code",
                    CliEngine::Gemini => "gemini_cli",
                }
                .to_string(),
            )
        });
        interaction.slot_id = Some(slot_id);
        interaction.cwd = string_field(&request.body, "cwd");
        interaction.project_root = string_field(&request.body, "project_root");
        interaction.control_action = Some(ProviderControlAction::SetFastMode);
        interaction.correlation_id = string_field(&request.body, "correlation_id")
            .unwrap_or_else(|| interaction.correlation_id.clone());
        let fast_mode = mode_from_path
            .map(Value::String)
            .or_else(|| request.body.get("fast_mode").cloned())
            .or_else(|| request.body.get("target_fast_mode").cloned())
            .or_else(|| request.body.get("service_tier").cloned())
            .or_else(|| request.body.get("fast_enabled").cloned())
            .or_else(|| request.body.get("enabled").cloned())
            .or_else(|| request.body.get("fast").cloned())
            .or_else(|| request.body.get("mode").cloned());
        interaction.desired_worker = Some(json!({
            "fast_mode": fast_mode,
            "spawn_if_missing": bool_field(&request.body, "spawn_if_missing")
                .or_else(|| bool_field(&request.body, "spawn"))
                .unwrap_or(true),
            "force_restart": bool_field(&request.body, "force_restart")
                .or_else(|| bool_field(&request.body, "restart"))
                .or_else(|| bool_field(&request.body, "respawn"))
                .unwrap_or(false),
        }));
        let result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        Ok(result_response(result))
    }

    async fn handle_slot_switch_model(
        &self,
        request: ProviderBoxHttpRequest,
        slot_id: String,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let mut interaction = ProviderInteractionRequest::new(
            BoxCommand::ModelSwitch,
            engine_from_body_or_slot(&request.body, &slot_id),
        );
        interaction.provider = string_field(&request.body, "provider").or_else(|| {
            if interaction.engine == CliEngine::Agy {
                Some("agy_cli".to_string())
            } else {
                None
            }
        });
        interaction.slot_id = Some(slot_id);
        interaction.model = string_field(&request.body, "model")
            .or_else(|| string_field(&request.body, "target_model"));
        interaction.model_profile = string_field(&request.body, "model_profile");
        interaction.dangerously_bypass_approvals_and_sandbox =
            bool_field(&request.body, "dangerously_bypass_approvals_and_sandbox")
                .or_else(|| bool_field(&request.body, "dangerously_skip_permissions"))
                .or_else(|| bool_field(&request.body, "dangerously_bypass"))
                .or_else(|| bool_field(&request.body, "bypass_approvals_and_sandbox"))
                .or_else(|| bool_field(&request.body, "bypass_mode"))
                .unwrap_or(false);
        interaction.cwd = string_field(&request.body, "cwd");
        interaction.project_root = string_field(&request.body, "project_root");
        interaction.tool_policy = tool_policy_from_body(&request.body);
        interaction.correlation_id = string_field(&request.body, "correlation_id")
            .unwrap_or_else(|| interaction.correlation_id.clone());
        interaction.model_switch_policy = Some(ModelSwitchPolicy {
            target_model: string_field(&request.body, "target_model")
                .or_else(|| string_field(&request.body, "model")),
            target_model_profile: string_field(&request.body, "target_model_profile")
                .or_else(|| string_field(&request.body, "model_profile")),
            allow_respawn: bool_field(&request.body, "allow_respawn").unwrap_or(true),
            require_verification: bool_field(&request.body, "require_verification").unwrap_or(true),
        });
        interaction.desired_worker = Some(json!({
            "spawn_if_missing": bool_field(&request.body, "spawn_if_missing")
                .or_else(|| bool_field(&request.body, "spawn"))
                .or_else(|| bool_field(&request.body, "allow_respawn"))
                .unwrap_or(true),
            "force_restart": bool_field(&request.body, "force_restart")
                .or_else(|| bool_field(&request.body, "restart"))
                .or_else(|| bool_field(&request.body, "respawn"))
                .unwrap_or(false),
            "dangerously_skip_permissions": interaction.dangerously_bypass_approvals_and_sandbox,
        }));
        let result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        Ok(result_response(result))
    }

    async fn handle_slot_usage_refresh(
        &self,
        request: ProviderBoxHttpRequest,
        slot_id: String,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let mut request = request;
        request.body["slot_id"] = Value::String(slot_id);
        self.handle_usage_refresh(request).await
    }

    async fn handle_slot_mcp_status(
        &self,
        request: ProviderBoxHttpRequest,
        slot_id: String,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let mut interaction = ProviderInteractionRequest::new(
            BoxCommand::McpStatus,
            engine_from_body_or_slot(&request.body, &slot_id),
        );
        interaction.provider = string_field(&request.body, "provider").or_else(|| {
            if interaction.engine == CliEngine::Agy {
                Some("agy_cli".to_string())
            } else {
                None
            }
        });
        interaction.slot_id = Some(slot_id);
        interaction.cwd = string_field(&request.body, "cwd");
        interaction.project_root = string_field(&request.body, "project_root");
        interaction.correlation_id = string_field(&request.body, "correlation_id")
            .unwrap_or_else(|| interaction.correlation_id.clone());
        interaction.desired_worker = Some(json!({
            "spawn_if_missing": bool_field(&request.body, "spawn_if_missing")
                .or_else(|| bool_field(&request.body, "spawn"))
                .unwrap_or(true),
            "mcp_server": string_field(&request.body, "server")
                .or_else(|| string_field(&request.body, "mcp_server"))
                .unwrap_or_else(|| "missiond".to_string()),
        }));
        let result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        Ok(result_response(result))
    }

    async fn handle_slot_mcp_reconnect(
        &self,
        request: ProviderBoxHttpRequest,
        slot_id: String,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let mut interaction = ProviderInteractionRequest::new(
            BoxCommand::McpReconnect,
            engine_from_body_or_slot(&request.body, &slot_id),
        );
        interaction.provider = string_field(&request.body, "provider").or_else(|| {
            if interaction.engine == CliEngine::Agy {
                Some("agy_cli".to_string())
            } else {
                None
            }
        });
        interaction.slot_id = Some(slot_id);
        interaction.cwd = string_field(&request.body, "cwd");
        interaction.project_root = string_field(&request.body, "project_root");
        interaction.correlation_id = string_field(&request.body, "correlation_id")
            .unwrap_or_else(|| interaction.correlation_id.clone());
        interaction.desired_worker = Some(json!({
            "spawn_if_missing": bool_field(&request.body, "spawn_if_missing")
                .or_else(|| bool_field(&request.body, "spawn"))
                .unwrap_or(true),
            "mcp_server": string_field(&request.body, "server")
                .or_else(|| string_field(&request.body, "mcp_server"))
                .unwrap_or_else(|| "missiond".to_string()),
            "force": bool_field(&request.body, "force").unwrap_or(true),
        }));
        let result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        Ok(result_response(result))
    }

    async fn handle_slot_text_only_completion(
        &self,
        request: ProviderBoxHttpRequest,
        slot_id: String,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let mut request = request;
        request.body["slot_id"] = Value::String(slot_id);
        self.handle_text_only_completion(request).await
    }

    async fn handle_models(
        &self,
        request: ProviderBoxHttpRequest,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let mut interaction =
            ProviderInteractionRequest::new(BoxCommand::ModelCatalogExport, CliEngine::Agy);
        interaction.provider = Some("agy_cli".to_string());
        if let Some(slot_id) =
            header_slot_id(&request).or_else(|| string_field(&request.body, "slot_id"))
        {
            interaction.slot_id = Some(slot_id);
        }
        if let Some(policy) = request.body.get("router_export_policy").cloned() {
            interaction.router_export_policy = Some(policy);
        } else {
            interaction.router_export_policy = Some(json!({}));
        }
        if !models_live_catalog_requested(&request) {
            let result = static_models_export_result(
                &interaction,
                ProviderBoxDiagnostic::warning(
                    "MODEL_CATALOG_STATIC_EXPORT",
                    "Returning static provider-box model sources; live provider catalog discovery requires explicit opt-in",
                    json!({
                        "live_catalog": false,
                        "opt_in_fields": [
                            "live_catalog",
                            "refresh_live_catalog",
                            "live",
                            "use_live_catalog"
                        ]
                    }),
                ),
            );
            return Ok(result_response(result));
        }
        let timeout_secs = request
            .body
            .get("live_timeout_secs")
            .or_else(|| request.body.get("timeout_secs"))
            .and_then(Value::as_u64)
            .unwrap_or(MODEL_CATALOG_LIVE_TIMEOUT_SECS);
        let result = match timeout(
            Duration::from_secs(timeout_secs.max(1)),
            self.boxed.execute(interaction.clone()),
        )
        .await
        {
            Ok(Ok(result)) if result.status == ProviderBoxStatus::Completed => result,
            Ok(Ok(result)) => static_models_export_result(
                &interaction,
                ProviderBoxDiagnostic::warning(
                    "MODEL_CATALOG_LIVE_EXPORT_UNAVAILABLE",
                    "Live provider model catalog is unavailable; returning static provider-box sources",
                    json!({
                        "live_status": result.status,
                        "live_diagnostics": result.diagnostics,
                    }),
                ),
            ),
            Ok(Err(err)) => static_models_export_result(
                &interaction,
                ProviderBoxDiagnostic::warning(
                    "MODEL_CATALOG_LIVE_EXPORT_ERROR",
                    "Live provider model catalog failed; returning static provider-box sources",
                    json!({
                        "error": err.to_string(),
                    }),
                ),
            ),
            Err(_) => static_models_export_result(
                &interaction,
                ProviderBoxDiagnostic::warning(
                    "MODEL_CATALOG_LIVE_EXPORT_TIMEOUT",
                    "Live provider model catalog timed out; returning static provider-box sources",
                    json!({
                        "timeout_secs": timeout_secs.max(1),
                    }),
                ),
            ),
        };
        Ok(result_response(result))
    }

    async fn handle_usage_refresh(
        &self,
        request: ProviderBoxHttpRequest,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let engine = usage_request_engine(&request);
        let mut interaction = ProviderInteractionRequest::new(BoxCommand::UsageProbe, engine);
        interaction.provider = Some(
            match engine {
                CliEngine::Codex => "codex_cli",
                CliEngine::Agy => "agy_cli",
                CliEngine::ClaudeCode => "claude_code",
                CliEngine::Gemini => "gemini_cli",
            }
            .to_string(),
        );
        interaction.slot_id = Some(usage_refresh_slot_id(&request, engine));
        interaction.model = string_field(&request.body, "model");
        interaction.model_profile = string_field(&request.body, "model_profile")
            .or_else(|| string_field(&request.body, "reasoning_effort"));
        interaction.cwd = string_field(&request.body, "cwd");
        interaction.project_root = string_field(&request.body, "project_root");
        interaction.correlation_id = string_field(&request.body, "correlation_id")
            .unwrap_or_else(|| interaction.correlation_id.clone());
        interaction.desired_worker = Some(json!({
            "spawn_if_missing": bool_field(&request.body, "spawn_if_missing")
                .or_else(|| bool_field(&request.body, "spawn"))
                .unwrap_or(true),
            "force_restart": bool_field(&request.body, "force_restart")
                .or_else(|| bool_field(&request.body, "restart"))
                .unwrap_or(false),
        }));
        let result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        if let Some(snapshot) = result
            .usage_snapshot
            .as_ref()
            .and_then(|snapshot| serde_json::to_value(snapshot).ok())
        {
            match engine {
                CliEngine::Codex => *self.codex_usage_cache.lock().await = Some(snapshot),
                _ => *self.agy_usage_cache.lock().await = Some(snapshot),
            }
        }
        Ok(result_response(result))
    }

    async fn handle_usage_cache(
        &self,
        request: ProviderBoxHttpRequest,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let engine = usage_request_engine(&request);
        let cached = match engine {
            CliEngine::Codex => self.codex_usage_cache.lock().await.clone(),
            _ => self.agy_usage_cache.lock().await.clone(),
        };
        let cached_hit = cached.is_some();
        let (provider, engine_label, probe_slot, message_hit, message_miss) = match engine {
            CliEngine::Codex => (
                "codex_cli",
                "codex",
                CODEX_USAGE_PROBE_SLOT,
                "Returning the latest cached Codex usage snapshot.",
                "No cached Codex usage snapshot is available yet; call POST /provider-box/v1/usage/refresh with engine=codex.",
            ),
            _ => (
                "agy_cli",
                "agy",
                AGY_USAGE_PROBE_SLOT,
                "Returning the latest cached AGY usage snapshot.",
                "No cached AGY usage snapshot is available yet; call POST /provider-box/v1/usage/refresh.",
            ),
        };
        Ok(json_response(
            200,
            json!({
                "schema": "missiond.provider-box.usage-cache.v1",
                "status": if cached_hit { "completed" } else { "unknown" },
                "cached": cached_hit,
                "provider": provider,
                "engine": engine_label,
                "usage_snapshot": cached,
                "refresh_endpoint": "/provider-box/v1/usage/refresh",
                "probe_slot_policy": {
                    "slot_id": probe_slot,
                    "owned_by": "provider-box",
                    "interferes_with_text_only_slots": false
                },
                "message": if cached_hit {
                    message_hit
                } else {
                    message_miss
                }
            }),
        ))
    }

    async fn handle_text_only_completion(
        &self,
        request: ProviderBoxHttpRequest,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let requested_engine = engine_from_body(&request.body);
        let has_explicit_slot =
            header_slot_id(&request).is_some() || string_field(&request.body, "slot_id").is_some();
        let logical_model_request = requested_engine == CliEngine::Agy && !has_explicit_slot;
        let logical_claude_code_text_request = requested_engine == CliEngine::ClaudeCode;
        let requested_model = string_field(&request.body, "model");
        let Some(mut interaction) = text_only_interaction_from_body(&request.body) else {
            let mut result = ProviderBoxResult::base(
                &ProviderInteractionRequest::new(BoxCommand::PureTextSingleTurn, requested_engine),
                ProviderBoxStatus::Failed,
            );
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Invalid provider-box text-only completion request",
                json!({
                    "schema": request.body.get("schema"),
                    "required": [
                        "pure_text=true",
                        "engine=agy, engine=codex, or provider=claude_code_text",
                        "model",
                        "messages[]"
                    ]
                }),
            ));
            return Ok(result_response(result));
        };
        if logical_claude_code_text_request && has_explicit_slot {
            let mut result = ProviderBoxResult::base(
                &ProviderInteractionRequest::new(
                    BoxCommand::PureTextSingleTurn,
                    CliEngine::ClaudeCode,
                ),
                ProviderBoxStatus::Failed,
            );
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "ClaudeCode text-only provider does not accept external slot_id",
                json!({
                    "provider": CLAUDE_CODE_TEXT_PROVIDER,
                    "rule": "provider-box owns private ephemeral PTY slots for ClaudeCode text-only requests"
                }),
            ));
            return Ok(result_response(result));
        }
        if logical_model_request {
            if let Some(model) = requested_model.as_deref() {
                if !is_agy_text_model_exportable(model) {
                    let mut result = ProviderBoxResult::base(
                        &ProviderInteractionRequest::new(
                            BoxCommand::PureTextSingleTurn,
                            CliEngine::Agy,
                        ),
                        ProviderBoxStatus::Failed,
                    );
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_BOX_INVALID_REQUEST,
                        "Requested AGY model is not exported as a provider-box text-only source",
                        json!({
                            "model": model,
                            "export_policy": "not_exported",
                        }),
                    ));
                    return Ok(result_response(result));
                }
                let lane = provider_box_lane(&request.body);
                interaction.slot_id = Some(
                    self.next_private_agy_slot_for_model_and_lane(model, lane.as_deref())
                        .await,
                );
            }
        } else if has_explicit_slot && interaction.slot_id.is_none() {
            interaction.slot_id = header_slot_id(&request);
        }
        let mut result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        if logical_model_request {
            if let Some(model) = requested_model.as_deref() {
                redact_private_slot_details(&mut result, model);
            }
        } else if logical_claude_code_text_request {
            redact_private_claude_code_text_details(&mut result);
        }
        Ok(result_response(result))
    }

    async fn handle_codex_task_completion(
        &self,
        request: ProviderBoxHttpRequest,
        command: BoxCommand,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let Some(interaction) = codex_task_interaction_from_body(&request.body, command) else {
            let mut result = ProviderBoxResult::base(
                &ProviderInteractionRequest::new(command, CliEngine::Codex),
                ProviderBoxStatus::Failed,
            );
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Invalid provider-box Codex task completion request",
                json!({
                    "schema": request.body.get("schema"),
                    "required": [
                        "engine=codex",
                        "model=gpt-5.5",
                        "messages[] or prompt",
                        "no external tools/files/attachments"
                    ],
                    "command": command,
                }),
            ));
            return Ok(result_response(result));
        };

        let result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        Ok(result_response(result))
    }

    async fn handle_claude_code_deep_research_completion(
        &self,
        request: ProviderBoxHttpRequest,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let Some(interaction) = claude_code_deep_research_interaction_from_body(&request.body)
        else {
            let mut result = ProviderBoxResult::base(
                &ProviderInteractionRequest::new(BoxCommand::Research, CliEngine::ClaudeCode),
                ProviderBoxStatus::Failed,
            );
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Invalid provider-box ClaudeCode deep-research request",
                json!({
                    "schema": request.body.get("schema"),
                    "required": [
                        "provider=claude_code_deep_research",
                        "engine=claude_code",
                        "messages[] or prompt",
                        "no external tools/files/attachments"
                    ],
                    "route": "/provider-box/v1/claude-code/deep-research/completions",
                }),
            ));
            return Ok(result_response(result));
        };

        let mut result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        redact_private_claude_code_task_details(&mut result);
        Ok(result_response(result))
    }

    async fn next_private_agy_slot_for_model(&self, model: &str) -> String {
        self.next_private_agy_slot_for_model_and_lane(model, None)
            .await
    }

    async fn next_private_agy_slot_for_model_and_lane(
        &self,
        model: &str,
        lane: Option<&str>,
    ) -> String {
        let slot_ids = private_agy_slot_ids_for_model_and_lane(model, lane);
        if slot_ids.len() <= 1 {
            return slot_ids
                .into_iter()
                .next()
                .unwrap_or_else(|| format!("slot-agy-{}", slug_model(model)));
        }

        let pool_id = agy_slot_pool_id_for_lane(model, lane);
        let mut cursors = self.agy_slot_pool_cursors.lock().await;
        let cursor = cursors.entry(pool_id).or_insert(0);
        let slot_id = slot_ids[*cursor % slot_ids.len()].clone();
        *cursor = (*cursor + 1) % slot_ids.len();
        slot_id
    }

    async fn handle_turn(
        &self,
        request: ProviderBoxHttpRequest,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let mut interaction =
            match serde_json::from_value::<ProviderInteractionRequest>(request.body.clone()) {
                Ok(value) => value,
                Err(err) => {
                    let mut result = ProviderBoxResult::base(
                        &ProviderInteractionRequest::new(BoxCommand::WorkerTurn, CliEngine::Codex),
                        ProviderBoxStatus::Failed,
                    );
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_BOX_INVALID_REQUEST,
                        "Invalid provider-box turn request",
                        json!({
                            "error": err.to_string(),
                            "schema": request.body.get("schema"),
                            "required": [
                                "schema=missiond.provider-interaction-request.v1",
                                "command",
                                "engine",
                                "prompt",
                                "correlation_id",
                                "control_action when command=control-action",
                                "desired_worker.pty_step when command=pty-step"
                            ]
                        }),
                    ));
                    return Ok(result_response(result));
                }
            };
        if interaction.slot_id.is_none() {
            interaction.slot_id = header_slot_id(&request);
        }
        let result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        Ok(result_response(result))
    }
}

fn text_only_interaction_from_body(body: &Value) -> Option<ProviderInteractionRequest> {
    if body.get("pure_text").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let engine = engine_from_body(body);
    if !matches!(
        engine,
        CliEngine::Agy | CliEngine::Codex | CliEngine::ClaudeCode
    ) {
        return None;
    }
    if has_forbidden_text_only_fields(body) {
        return None;
    }
    let model = string_field(body, "model")?;
    let slot_id = string_field(body, "slot_id");
    let messages = body.get("messages")?.as_array()?;
    let correlation_id = string_field(body, "correlation_id")
        .unwrap_or_else(|| format!("router-{}", uuid::Uuid::new_v4().simple()));
    let prompt = build_pure_text_prompt(messages)?;
    let model_profile = string_field(body, "model_profile")
        .or_else(|| string_field(body, "reasoning_effort"))
        .or_else(|| string_field(body, "model_reasoning_effort"));
    if engine == CliEngine::ClaudeCode
        && !is_exported_claude_code_text_model(&model, model_profile.as_deref())
    {
        return None;
    }

    let mut interaction = ProviderInteractionRequest::pure_text(engine, prompt);
    interaction.schema = "missiond.provider-interaction-request.v1".to_string();
    interaction.provider = string_field(body, "provider").or_else(|| {
        Some(
            match engine {
                CliEngine::Codex => "codex_exec_text",
                CliEngine::ClaudeCode => CLAUDE_CODE_TEXT_PROVIDER,
                _ => "agy_cli",
            }
            .to_string(),
        )
    });
    interaction.model = Some(model.clone());
    interaction.model_profile = model_profile;
    interaction.provider_box_lane = provider_box_lane(body);
    interaction.xjp_request_stage = string_field(body, "xjp_request_stage")
        .or_else(|| string_field(body, "pipeline_stage"))
        .or_else(|| string_field(body, "stage"));
    interaction.slot_id = slot_id;
    interaction.dangerously_bypass_approvals_and_sandbox =
        bool_field(body, "dangerously_bypass_approvals_and_sandbox")
            .or_else(|| bool_field(body, "dangerously_bypass"))
            .or_else(|| bool_field(body, "bypass_approvals_and_sandbox"))
            .or_else(|| bool_field(body, "bypass_mode"))
            .unwrap_or(false);
    interaction.correlation_id = correlation_id;
    interaction.timeout_secs = body.get("timeout_secs").and_then(Value::as_u64);
    interaction.output_contract = body.get("output_contract").cloned().or_else(|| {
        Some(json!({
            "media_type": "text/plain",
            "single_turn": true
        }))
    });

    if let Some(policy) = body.get("timeout_cancel_policy").cloned() {
        if let Ok(policy) = serde_json::from_value::<TimeoutCancelPolicy>(policy) {
            interaction.timeout_cancel_policy = Some(policy);
        }
    }
    if engine == CliEngine::Agy {
        interaction.model_switch_policy = Some(ModelSwitchPolicy {
            target_model: Some(model),
            target_model_profile: string_field(body, "target_model_profile")
                .or_else(|| string_field(body, "model_profile")),
            allow_respawn: bool_field(body, "allow_model_switch")
                .or_else(|| bool_field(body, "allow_respawn"))
                .unwrap_or(false),
            require_verification: bool_field(body, "require_verification").unwrap_or(true),
        });
    }
    Some(interaction)
}

fn codex_task_interaction_from_body(
    body: &Value,
    command: BoxCommand,
) -> Option<ProviderInteractionRequest> {
    if !matches!(command, BoxCommand::Research | BoxCommand::ImageGeneration) {
        return None;
    }
    if matches_forbidden_codex_task_fields(body) {
        return None;
    }
    let engine = if body.get("engine").is_none() && body.get("provider").is_none() {
        CliEngine::Codex
    } else {
        engine_from_body(body)
    };
    if engine != CliEngine::Codex {
        return None;
    }
    let user_prompt = codex_task_user_prompt_from_body(body)?;
    let (provider, prompt_prefix, task_kind, media_type, allowed_tools) = match command {
        BoxCommand::Research => (
            CODEX_RESEARCH_PROVIDER,
            CODEX_RESEARCH_PROMPT_PREFIX,
            "research",
            "text/markdown",
            json!(["web_search"]),
        ),
        BoxCommand::ImageGeneration => (
            CODEX_IMAGE_PROVIDER,
            CODEX_IMAGE_PROMPT_PREFIX,
            "image_generation",
            "text/markdown+image",
            json!(["image_generation"]),
        ),
        _ => unreachable!("checked above"),
    };
    let correlation_id = string_field(body, "correlation_id")
        .unwrap_or_else(|| format!("router-{}", uuid::Uuid::new_v4().simple()));
    let mut interaction = ProviderInteractionRequest::new(command, CliEngine::Codex);
    interaction.schema = "missiond.provider-interaction-request.v1".to_string();
    interaction.provider = Some(provider.to_string());
    interaction.model =
        Some(string_field(body, "model").unwrap_or_else(|| CODEX_TASK_MODEL.to_string()));
    interaction.model_profile = string_field(body, "model_profile")
        .or_else(|| string_field(body, "reasoning_effort"))
        .or_else(|| string_field(body, "model_reasoning_effort"));
    interaction.prompt = Some(if command == BoxCommand::ImageGeneration {
        format!(
            "{prompt_prefix}\n\n{}\n\n{CODEX_IMAGE_STABLE_PROMPT_SUFFIX}",
            user_prompt.trim()
        )
    } else {
        format!("{prompt_prefix}\n\n{}", user_prompt.trim())
    });
    interaction.correlation_id = correlation_id;
    interaction.timeout_secs = body.get("timeout_secs").and_then(Value::as_u64);
    interaction.no_tools = false;
    interaction.no_mcp = true;
    interaction.no_shell = true;
    interaction.no_file_access = true;
    interaction.output_contract = Some(json!({
        "media_type": media_type,
        "single_turn": true,
        "durable_source": if command == BoxCommand::ImageGeneration {
            "codex_rollout_jsonl_image_generation_end"
        } else {
            "codex_exec_jsonl"
        }
    }));
    interaction.tool_policy = Some(json!({
        "provider_task_kind": task_kind,
        "allowed_tools": allowed_tools,
        "sandbox": "read-only",
        "approval_policy": "never",
        "search_enabled": command == BoxCommand::Research,
        "image_generation_enabled": command == BoxCommand::ImageGeneration,
        "no_mcp": true,
        "no_shell": true,
        "no_file_access": true,
    }));
    Some(interaction)
}

fn claude_code_deep_research_interaction_from_body(
    body: &Value,
) -> Option<ProviderInteractionRequest> {
    if matches_forbidden_codex_task_fields(body) {
        return None;
    }
    let engine = if body.get("engine").is_none() && body.get("provider").is_none() {
        CliEngine::ClaudeCode
    } else {
        engine_from_body(body)
    };
    if engine != CliEngine::ClaudeCode {
        return None;
    }
    let provider = string_field(body, "provider")
        .unwrap_or_else(|| CLAUDE_CODE_DEEP_RESEARCH_PROVIDER.to_string());
    if !provider.eq_ignore_ascii_case(CLAUDE_CODE_DEEP_RESEARCH_PROVIDER)
        && !provider.eq_ignore_ascii_case("claude-code-deep-research")
    {
        return None;
    }
    if string_field(body, "slot_id").is_some() {
        return None;
    }
    let user_prompt = codex_task_user_prompt_from_body(body)?;
    let correlation_id = string_field(body, "correlation_id")
        .unwrap_or_else(|| format!("router-{}", uuid::Uuid::new_v4().simple()));
    let requested_model = string_field(body, "model")
        .unwrap_or_else(|| CLAUDE_CODE_DEEP_RESEARCH_MODEL_ID.to_string());
    if !claude_code_deep_research_model_ref_matches(&requested_model) {
        return None;
    }
    let requested_profile = string_field(body, "model_profile")
        .or_else(|| string_field(body, "reasoning_effort"))
        .or_else(|| string_field(body, "model_reasoning_effort"));
    if requested_profile.as_deref().is_some_and(|profile| {
        !profile.eq_ignore_ascii_case(CLAUDE_CODE_DEEP_RESEARCH_MODEL_PROFILE)
    }) {
        return None;
    }

    let mut interaction =
        ProviderInteractionRequest::new(BoxCommand::Research, CliEngine::ClaudeCode);
    interaction.schema = "missiond.provider-interaction-request.v1".to_string();
    interaction.provider = Some(CLAUDE_CODE_DEEP_RESEARCH_PROVIDER.to_string());
    interaction.model = Some(CLAUDE_CODE_DEEP_RESEARCH_MODEL_ID.to_string());
    interaction.model_profile = Some(CLAUDE_CODE_DEEP_RESEARCH_MODEL_PROFILE.to_string());
    interaction.prompt = Some(format!(
        "{CLAUDE_CODE_DEEP_RESEARCH_PROMPT_PREFIX}\n\ncall deep-research:\n\n{}",
        user_prompt.trim()
    ));
    interaction.correlation_id = correlation_id;
    interaction.timeout_secs = body
        .get("timeout_secs")
        .and_then(Value::as_u64)
        .or(Some(1_800));
    interaction.no_tools = false;
    interaction.no_mcp = false;
    interaction.no_shell = false;
    interaction.no_file_access = false;
    interaction.output_contract = Some(json!({
        "media_type": "text/markdown",
        "single_turn": false,
        "durable_source": "claude_code_deep_research_workflow_journal",
        "workflow": "deep-research"
    }));
    interaction.tool_policy = Some(json!({
        "provider_task_kind": "deep_research",
        "allowed_tools": ["Skill", "Workflow"],
        "workflow": "deep-research",
        "workflow_required": true,
        "background_workflow_allowed": true,
        "durable_journal_required": true,
        "external_tool_schemas_allowed": false,
        "external_files_allowed": false
    }));
    interaction.desired_worker = Some(json!({
        "provider_session_id": uuid::Uuid::new_v4().to_string(),
        "spawn_if_missing": true,
        "private_ephemeral_slot": true,
        "provider_box_managed": true,
        "workflow": "deep-research"
    }));
    Some(interaction)
}

fn is_claude_code_deep_research_body(body: &Value) -> bool {
    string_field(body, "provider").is_some_and(|provider| {
        provider.eq_ignore_ascii_case(CLAUDE_CODE_DEEP_RESEARCH_PROVIDER)
            || provider.eq_ignore_ascii_case("claude-code-deep-research")
    }) || engine_from_body(body) == CliEngine::ClaudeCode
}

fn codex_task_user_prompt_from_body(body: &Value) -> Option<String> {
    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        return build_pure_text_prompt(messages);
    }
    raw_string_field(body, "prompt")
        .or_else(|| raw_string_field(body, "input"))
        .or_else(|| raw_string_field(body, "text"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn matches_forbidden_codex_task_fields(body: &Value) -> bool {
    body.get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty())
        || body
            .get("functions")
            .and_then(Value::as_array)
            .is_some_and(|functions| !functions.is_empty())
        || body
            .get("tool_choice")
            .is_some_and(|choice| !choice.is_null() && choice.as_str() != Some("none"))
        || body.get("attachments").is_some()
        || body.get("files").is_some()
}

fn pty_step_payload_from_body(body: &Value) -> Result<Value, ProviderBoxDiagnostic> {
    let action = body.get("action").unwrap_or(body);
    let action_type = action
        .get("action_type")
        .or_else(|| action.get("type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .or_else(|| {
            if action.get("key").is_some() || body.get("key").is_some() {
                Some("key".to_string())
            } else if action.get("text").is_some() || body.get("text").is_some() {
                Some("text".to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "PTY step requires action.type=key or action.type=text",
                json!({
                    "allowed_action_types": ["key", "text"],
                    "examples": [
                        {"action": {"type": "key", "key": "down"}},
                        {"action": {"type": "text", "text": "/model"}}
                    ]
                }),
            )
        })?;
    let expected_change = raw_string_field(action, "expected_change")
        .or_else(|| raw_string_field(body, "expected_change"));
    let redacted = bool_field(action, "redacted")
        .or_else(|| bool_field(body, "redacted"))
        .unwrap_or(action_type == "text" || action_type == "paste");

    match action_type.as_str() {
        "key" => {
            let key = string_field(action, "key")
                .or_else(|| string_field(body, "key"))
                .or_else(|| string_field(action, "human_input"))
                .ok_or_else(|| {
                    ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_BOX_INVALID_REQUEST,
                        "PTY key step requires a key name",
                        json!({
                            "allowed_keys": PTY_STEP_ALLOWED_KEYS,
                        }),
                    )
                })?;
            if !pty_step_key_allowed(&key) {
                return Err(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "PTY key step uses an unsupported key",
                    json!({
                        "key": key,
                        "allowed_keys": PTY_STEP_ALLOWED_KEYS,
                    }),
                ));
            }
            Ok(json!({
                "action_type": "key",
                "key": key,
                "redacted": false,
                "expected_change": expected_change,
            }))
        }
        "text" | "paste" => {
            let text = raw_string_field(action, "text")
                .or_else(|| raw_string_field(body, "text"))
                .or_else(|| raw_string_field(action, "human_input"))
                .ok_or_else(|| {
                    ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_BOX_INVALID_REQUEST,
                        "PTY text step requires text",
                        json!({}),
                    )
                })?;
            if text.chars().count() > PTY_STEP_TEXT_LIMIT {
                return Err(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "PTY text step exceeds the maximum length",
                    json!({
                        "max_chars": PTY_STEP_TEXT_LIMIT,
                    }),
                ));
            }
            if text.contains('\n') || text.contains('\r') {
                return Err(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "PTY text step must not include Enter; send text and Enter as separate API calls",
                    json!({
                        "rule": "text_and_enter_are_separate_observe_act_observe_steps",
                    }),
                ));
            }
            Ok(json!({
                "action_type": "text",
                "text": text,
                "redacted": redacted,
                "expected_change": expected_change,
            }))
        }
        _ => Err(ProviderBoxDiagnostic::error(
            DIAG_PROVIDER_BOX_INVALID_REQUEST,
            "PTY step action type is unsupported",
            json!({
                "action_type": action_type,
                "allowed_action_types": ["key", "text"],
            }),
        )),
    }
}

fn parse_slot_endpoint(path: &str) -> Option<(String, String)> {
    let rest = path.strip_prefix("/provider-box/v1/slots/")?;
    if rest == "spawn" {
        return None;
    }
    let mut parts = rest.split('/');
    let slot_id = parts.next()?.trim();
    if slot_id.is_empty() {
        return None;
    }
    let suffix = parts.collect::<Vec<_>>().join("/");
    if suffix.is_empty() {
        return None;
    }
    Some((slot_id.to_string(), suffix))
}

fn slot_permission_mode_from_suffix(suffix: &str) -> Option<String> {
    let mode = slot_permission_mode_raw_from_suffix(suffix)?;
    normalize_provider_box_permission_mode(&mode)
}

fn slot_permission_mode_raw_from_suffix(suffix: &str) -> Option<String> {
    let normalized = suffix.trim().trim_matches('/').to_ascii_lowercase();
    let mode = normalized
        .strip_prefix("permissions/")
        .or_else(|| normalized.strip_prefix("permission/"))
        .or_else(|| normalized.strip_prefix("actions/permissions/"))
        .or_else(|| normalized.strip_prefix("actions/permission/"))
        .or_else(|| {
            if matches!(
                normalized.as_str(),
                "permissions" | "permission" | "actions/permissions" | "actions/permission"
            ) {
                Some("")
            } else {
                None
            }
        })?;
    let mode = mode.trim();
    if mode.is_empty() {
        return None;
    }
    Some(mode.to_string())
}

fn slot_fast_mode_from_suffix(suffix: &str) -> Option<String> {
    let normalized = suffix.trim().trim_matches('/').to_ascii_lowercase();
    let mode = normalized
        .strip_prefix("fast/")
        .or_else(|| normalized.strip_prefix("actions/fast/"))
        .or_else(|| {
            if matches!(normalized.as_str(), "fast" | "actions/fast") {
                Some("")
            } else {
                None
            }
        })?;
    let mode = mode.trim();
    if mode.is_empty() {
        return None;
    }
    normalize_provider_box_fast_mode(mode)
}

fn normalize_provider_box_fast_mode(value: &str) -> Option<String> {
    let normalized = value
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-")
        .replace(' ', "-");
    match normalized.as_str() {
        "enable" | "enabled" | "on" | "true" | "fast" | "priority" => Some("enabled".to_string()),
        "disable" | "disabled" | "off" | "false" | "default" | "normal" | "standard" => {
            Some("disabled".to_string())
        }
        _ => None,
    }
}

fn normalize_provider_box_permission_mode(value: &str) -> Option<String> {
    let normalized = value
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-")
        .replace(' ', "-");
    match normalized.as_str() {
        "default" => Some("Default".to_string()),
        "auto-review" | "autoreview" | "auto" => Some("Auto-review".to_string()),
        "full-access" | "fullaccess" | "full" => Some("Full Access".to_string()),
        _ => None,
    }
}

fn normalize_provider_box_permission_mode_for_engine(
    engine: CliEngine,
    value: &str,
) -> Option<String> {
    match engine {
        CliEngine::ClaudeCode => normalize_provider_box_claude_code_permission_mode(value),
        CliEngine::Codex => normalize_provider_box_permission_mode(value),
        _ => normalize_provider_box_permission_mode(value)
            .or_else(|| normalize_provider_box_claude_code_permission_mode(value)),
    }
}

fn normalize_provider_box_claude_code_permission_mode(value: &str) -> Option<String> {
    let normalized = value
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-")
        .replace(' ', "-");
    match normalized.as_str() {
        "auto" | "auto-mode" | "automode" | "auto-review" => Some("auto".to_string()),
        "default" | "ask" | "ask-first" | "normal" => Some("default".to_string()),
        "accept-edits" | "accept-edits-mode" | "acceptedits" | "accept" | "edits" => {
            Some("accept_edits".to_string())
        }
        "plan" | "plan-mode" => Some("plan".to_string()),
        "bypass" | "bypass-permissions" | "bypasspermissions" | "dangerously-skip-permissions" => {
            Some("bypass_permissions".to_string())
        }
        _ => None,
    }
}

fn engine_from_body(body: &Value) -> CliEngine {
    let raw = body
        .get("engine")
        .and_then(Value::as_str)
        .or_else(|| body.get("provider").and_then(Value::as_str))
        .unwrap_or("agy");
    cli_engine_from_hint(raw)
}

fn cli_engine_from_hint(raw: &str) -> CliEngine {
    match raw.to_ascii_lowercase().as_str() {
        "claude-code" | "claude_code" | "claudecode" | "claude" | "claude_code_text"
        | "claude-code-text" => CliEngine::ClaudeCode,
        "codex"
        | "codex_cli"
        | "codex-cli"
        | "codex_exec_text"
        | "codex-exec-text"
        | "codex_research"
        | "codex-research"
        | "codex_image_generation"
        | "codex-image-generation"
        | "codex_image"
        | "codex-image" => CliEngine::Codex,
        "gemini" | "gemini_cli" | "gemini-cli" => CliEngine::Gemini,
        _ => CliEngine::Agy,
    }
}

fn usage_request_engine(request: &ProviderBoxHttpRequest) -> CliEngine {
    query_field(&request.path, "engine")
        .or_else(|| query_field(&request.path, "provider"))
        .map(|value| cli_engine_from_hint(&value))
        .unwrap_or_else(|| engine_from_body(&request.body))
}

fn engine_from_body_or_slot(body: &Value, slot_id: &str) -> CliEngine {
    if body.get("engine").is_some() || body.get("provider").is_some() {
        return engine_from_body(body);
    }
    let normalized = slot_id.trim().to_ascii_lowercase();
    if normalized.starts_with("slot-agy-") || normalized.contains("-agy-") {
        CliEngine::Agy
    } else if normalized.contains("codex") {
        CliEngine::Codex
    } else if normalized.contains("claude") || normalized.contains("cc-") {
        CliEngine::ClaudeCode
    } else if normalized.contains("gemini") {
        CliEngine::Gemini
    } else {
        CliEngine::Agy
    }
}

fn build_pure_text_prompt(messages: &[Value]) -> Option<String> {
    let mut rendered = Vec::new();
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        if role == "tool"
            || message.get("tool_calls").is_some()
            || message.get("tool_call_id").is_some()
        {
            return None;
        }
        let content = message.get("content")?;
        let text = if let Some(text) = content.as_str() {
            text.to_string()
        } else if let Some(parts) = content.as_array() {
            let mut part_text = Vec::new();
            for part in parts {
                if part.get("type").and_then(Value::as_str).unwrap_or("text") != "text" {
                    return None;
                }
                part_text.push(part.get("text")?.as_str()?.to_string());
            }
            part_text.join("\n")
        } else {
            return None;
        };
        if !text.trim().is_empty() {
            rendered.push((role.to_string(), text.trim().to_string()));
        }
    }
    if rendered.is_empty() {
        return None;
    }
    if rendered.len() == 1 && rendered[0].0 == "user" {
        return Some(rendered.remove(0).1);
    }
    Some(
        rendered
            .into_iter()
            .map(|(role, text)| format!("{role}:\n{text}"))
            .collect::<Vec<_>>()
            .join("\n\n"),
    )
}

fn has_forbidden_text_only_fields(body: &Value) -> bool {
    body.get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty())
        || body
            .get("functions")
            .and_then(Value::as_array)
            .is_some_and(|functions| !functions.is_empty())
        || body
            .get("tool_choice")
            .is_some_and(|choice| !choice.is_null() && choice.as_str() != Some("none"))
        || body
            .get("search_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || body
            .get("web_search")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || body.get("attachments").is_some()
        || body.get("files").is_some()
}

fn result_response(result: ProviderBoxResult) -> ProviderBoxHttpResponse {
    let status = match result.status {
        ProviderBoxStatus::Completed | ProviderBoxStatus::Accepted => 200,
        ProviderBoxStatus::Unknown | ProviderBoxStatus::Unverified => 422,
        ProviderBoxStatus::Unsupported => 501,
        ProviderBoxStatus::Blocked => 409,
        ProviderBoxStatus::Failed => 502,
    };
    let screen = latest_screen_observation(&result);
    let media_artifact = response_media_artifact(result.slot_status.as_ref());
    let media_kind = media_artifact
        .as_ref()
        .and_then(|artifact| artifact.get("kind"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let media_url = media_artifact
        .as_ref()
        .and_then(response_media_signed_url)
        .map(str::to_string);
    let media_content_parts = media_artifact
        .as_ref()
        .and_then(response_media_content_parts);
    let body = if status == 200 {
        let final_text = result.final_text.clone().unwrap_or_default();
        let mut body = json!({
            "schema": result.schema,
            "status": result.status,
            "turn_id": result.turn_id,
            "correlation_id": result.correlation_id,
            "provider": result.provider,
            "engine": result.engine,
            "slot_id": result.slot_id,
            "provider_conversation_id": result.provider_conversation_id,
            "provider_session_identity": result.provider_session_identity,
            "durable_source": result.durable_source,
            "final_text": final_text,
            "slot_status": result.slot_status,
            "screen": screen,
            "usage_snapshot": result.usage_snapshot,
            "model_catalog": result.model_catalog,
            "router_export": result.router_export,
            "model_switch_result": result.model_switch_result,
            "mcp_status": result.mcp_status,
            "capabilities": result.capabilities,
            "diagnostics": result.diagnostics,
            "step_records": result.step_records,
            "artifact_hash": result.artifact_hash
        });
        if let Some(artifact) = media_artifact.as_ref() {
            body["media_artifact"] = artifact.clone();
            match media_kind.as_deref() {
                Some("image") => {
                    body["image_artifact"] = artifact.clone();
                }
                Some("video") => {
                    body["video_artifact"] = artifact.clone();
                }
                _ => {}
            }
        }
        if let Some(url) = media_url.as_ref() {
            match media_kind.as_deref() {
                Some("image") => {
                    body["imageUrl"] = json!(url);
                }
                Some("video") => {
                    body["videoUrl"] = json!(url);
                }
                _ => {}
            }
        }
        if let Some(parts) = media_content_parts.as_ref() {
            body["content_parts"] = parts.clone();
        }
        body["model"] = json!(result.model);
        body["model_profile"] = json!(result.model_profile);
        body["dangerously_bypass_approvals_and_sandbox"] =
            json!(result.dangerously_bypass_approvals_and_sandbox);
        if result.final_text.is_some() {
            let mut message = json!({
                "role": "assistant",
                "content": final_text
            });
            if let Some(parts) = media_content_parts.as_ref() {
                message["content_parts"] = parts.clone();
            }
            if let Some(artifact) = media_artifact.as_ref() {
                match media_kind.as_deref() {
                    Some("image") => {
                        message["image_artifact"] = artifact.clone();
                    }
                    Some("video") => {
                        message["video_artifact"] = artifact.clone();
                    }
                    _ => {}
                }
            }
            body["choices"] = json!([{
                "index": 0,
                "message": message,
                "finish_reason": "stop"
            }]);
        }
        if let Some(catalog) = result.model_catalog.as_ref() {
            body["object"] = json!("list");
            body["data"] = openai_model_data(catalog);
            body["provider_text_only_sources"] = provider_text_only_sources(catalog);
            append_codex_exec_text_exports(&mut body);
            append_codex_task_exports(&mut body);
            append_claude_code_text_exports(&mut body);
            append_claude_code_task_exports(&mut body);
            body["model_export"] = json!({
                "schema": "missiond.provider-box.model-export.v1",
                "provider": catalog.provider.clone(),
                "engine": catalog.engine,
                "catalog_id": catalog.catalog_id.clone(),
                "models_count": catalog.entries.len(),
                "router_backend_ids": result
                    .router_export
                    .as_ref()
                    .map(|export| export.router_backend_ids.clone())
                    .unwrap_or_default(),
                "routeable_count": result
                    .router_export
                    .as_ref()
                    .map(|export| export.routeable_entries.len())
                    .unwrap_or_default(),
                "blocked_count": result
                    .router_export
                    .as_ref()
                    .map(|export| export.blocked_entries.len())
                    .unwrap_or_default(),
                "completion_endpoint": "/provider-box/v1/text-only/completions",
                "slot_scoped_apis": "internal_maintenance_only",
                "codex_exec_text_sources": "exported_guarded_static_sources",
                "codex_task_sources": "exported_guarded_static_sources",
                "claude_code_text_sources": "exported_guarded_static_sources",
                "claude_code_task_sources": "exported_guarded_static_sources",
                "pure_text_guard": {
                    "prompt_instruction": false,
                    "sidecar_correlation": true,
                    "transcript_cursor_guard": true,
                    "isolated_runtime_workspace": true,
                    "agy_sandbox_flag": true,
                    "durable_jsonl_guard": true,
                    "permission_profile": "documented_deny_policy_plus_fail_closed_transcript_gate",
                    "tools": false,
                    "mcp": false,
                    "shell": false,
                    "file_access": false,
                    "vision": false
                }
            });
        }
        if let (Some(catalog), Some(export)) =
            (result.model_catalog.as_ref(), result.router_export.as_ref())
        {
            body["router_model_sources"] = router_model_sources(catalog, export);
            append_codex_exec_router_sources(&mut body);
            append_codex_task_router_sources(&mut body);
            append_claude_code_text_router_sources(&mut body);
            append_claude_code_task_router_sources(&mut body);
        }
        body
    } else {
        let diagnostics = result.diagnostics.clone();
        let error_code = diagnostics.first().map(|diag| diag.code.clone());
        json!({
            "schema": result.schema,
            "status": result.status,
            "turn_id": result.turn_id,
            "correlation_id": result.correlation_id,
            "error": {
                "code": error_code,
                "message": result
                    .diagnostics
                    .first()
                    .map(|diag| diag.message.clone())
                    .unwrap_or_else(|| "Provider-box request failed".to_string()),
                "diagnostics": diagnostics.clone()
            },
            "diagnostics": diagnostics,
            "slot_status": result.slot_status,
            "provider_conversation_id": result.provider_conversation_id,
            "provider_session_identity": result.provider_session_identity,
            "durable_source": result.durable_source,
            "screen": screen,
            "step_records": result.step_records,
            "usage_snapshot": result.usage_snapshot,
            "model_catalog": result.model_catalog,
            "router_export": result.router_export,
            "model_switch_result": result.model_switch_result,
            "mcp_status": result.mcp_status,
            "capabilities": result.capabilities,
            "artifact_hash": result.artifact_hash
        })
    };
    json_response(status, body)
}

fn static_models_export_result(
    request: &ProviderInteractionRequest,
    diagnostic: ProviderBoxDiagnostic,
) -> ProviderBoxResult {
    let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Completed);
    let entries = static_agy_model_catalog_entries();
    result.model_catalog = Some(ProviderModelCatalog {
        schema: "missiond.provider-model-catalog.v1".to_string(),
        catalog_id: format!("static-catalog-{}", uuid::Uuid::new_v4().simple()),
        provider: Some("agy_cli".to_string()),
        engine: CliEngine::Agy,
        account_ref: None,
        discovered_at: chrono::Utc::now().to_rfc3339(),
        source: Some("provider-box:static-agy-text-only-sources".to_string()),
        entries,
        diagnostics: vec![diagnostic.clone()],
    });
    if let Some(catalog) = result.model_catalog.as_ref() {
        result.router_export = Some(static_agy_router_export(request, catalog, &diagnostic));
    }
    result.diagnostics.push(diagnostic);
    result
}

fn static_agy_model_catalog_entries() -> Vec<ProviderModelCatalogEntry> {
    STATIC_AGY_TEXT_MODELS
        .iter()
        .filter(|model| is_agy_text_model_exportable(model))
        .map(|model| ProviderModelCatalogEntry {
            provider_model_id: format!("agy:{}", slug_model(model)),
            display_name: (*model).to_string(),
            family: model.split_whitespace().next().map(str::to_string),
            routeable_default: true,
            switch_capability: "interactive_model_picker".to_string(),
            usage_probe_capability: "interactive_usage_screen".to_string(),
            confidence: 0.76,
        })
        .collect()
}

fn static_agy_router_export(
    request: &ProviderInteractionRequest,
    catalog: &ProviderModelCatalog,
    diagnostic: &ProviderBoxDiagnostic,
) -> ProviderRouterExport {
    let base_url = request
        .router_export_policy
        .as_ref()
        .and_then(|policy| {
            policy
                .get("provider_box_base_url")
                .and_then(Value::as_str)
                .or_else(|| policy.get("managed_proxy_base_url").and_then(Value::as_str))
        })
        .map(str::to_string)
        .or_else(|| std::env::var("MISSIOND_PROVIDER_BOX_PROXY_BASE_URL").ok())
        .or_else(|| std::env::var("MISSIOND_AGY_PROVIDER_BOX_BASE_URL").ok());

    let mut routeable_entries = Vec::new();
    let mut blocked_entries = Vec::new();
    for entry in &catalog.entries {
        if !is_agy_text_model_exportable(&entry.display_name) {
            continue;
        }
        let slug = slug_model(&entry.display_name);
        let slot_pool_id = agy_slot_pool_id(&entry.display_name);
        let route = json!({
            "model_id": format!("agy-{slug}"),
            "primary": {
                "provider": "MissionDAgy",
                "provider_model_id": base_url.clone().unwrap_or_default(),
                "billing_id": format!("missiond/agy/{slug}"),
                "timeouts_ms": 300000,
                "capabilities": {
                    "text": true,
                    "tools": false,
                    "vision": false,
                    "files": false,
                    "mcp": false,
                    "shell": false
                },
                "extra": {
                    "provider": "agy_cli",
                    "model": entry.display_name,
                    "slot_pool_id": slot_pool_id,
                    "slot_policy": {
                        "kind": "provider_box_managed_pool",
                        "public_max_concurrent": 1,
                        "replicas_hidden": true,
                        "queue_owner": "provider-box"
                    },
                    "pure_text": true,
                    "allow_model_switch": false,
                    "requires_current_model_verification": true,
                    "completion_endpoint": "/provider-box/v1/text-only/completions"
                }
            }
        });
        if base_url.is_some() {
            routeable_entries.push(route);
        } else {
            blocked_entries.push(json!({
                "entry": route,
                "reason": "managed proxy provider-box base URL missing"
            }));
        }
    }

    let mut diagnostics = vec![diagnostic.clone()];
    if base_url.is_none() {
        diagnostics.push(ProviderBoxDiagnostic::warning(
            "PROVIDER_ROUTER_EXPORT_PROXY_URL_MISSING",
            "AGY router export requires MissionD provider-box URL from the self-built proxy deployment program",
            json!({
                "env": [
                    "MISSIOND_PROVIDER_BOX_PROXY_BASE_URL",
                    "MISSIOND_AGY_PROVIDER_BOX_BASE_URL"
                ]
            }),
        ));
    }

    ProviderRouterExport {
        schema: "missiond.provider-router-export.v1".to_string(),
        export_id: format!("static-export-{}", uuid::Uuid::new_v4().simple()),
        catalog_id: Some(catalog.catalog_id.clone()),
        provider: Some("agy_cli".to_string()),
        engine: CliEngine::Agy,
        router_backend_ids: vec!["xjp-router:MissionDAgy".to_string()],
        routeable_entries,
        blocked_entries,
        policy_ref: Some("interactive-provider-box/MissionDAgy/text-only".to_string()),
        diagnostics,
    }
}

fn response_media_artifact(slot_status: Option<&Value>) -> Option<Value> {
    slot_status?
        .get("media_artifact")
        .filter(|artifact| !artifact.is_null())
        .cloned()
}

fn response_media_signed_url(artifact: &Value) -> Option<&str> {
    artifact
        .pointer("/signed_url/url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty())
}

fn response_media_content_parts(artifact: &Value) -> Option<Value> {
    let url = response_media_signed_url(artifact)?;
    match artifact.get("kind").and_then(Value::as_str) {
        Some("image") => Some(json!([{
            "type": "imageUrl",
            "imageUrl": url
        }])),
        Some("video") => Some(json!([{
            "type": "videoUrl",
            "videoUrl": url
        }])),
        _ => None,
    }
}

fn provider_slot_capabilities_value(
    engine: CliEngine,
    driver: &ProviderDriverCapabilities,
) -> Value {
    let slot_controls = match engine {
        CliEngine::Codex => json!({
            "spawn": true,
            "restart": {
                "supported": true,
                "requires": {
                    "confirm_destroy_context": true
                }
            },
            "status": true,
            "session_identity": true,
            "observe": true,
            "input": true,
            "pty_step": true,
            "clear_input": true,
            "clear_screen": true,
            "exit": {
                "supported": true,
                "command": "/exit",
                "verification": "previous_pid_exited_or_slot_restarted"
            },
            "permissions": ["Default", "Auto-review", "Full Access"],
            "fast": ["enabled", "disabled"],
            "usage_refresh": true,
            "mcp_status": true,
            "mcp_reconnect": {
                "supported": false,
                "restart_required": true,
                "hint": "Codex CLI does not hot-reload MCP config from /mcp; restart the PTY slot explicitly when context loss is acceptable."
            },
            "switch_model": false,
            "model_catalog": false
        }),
        CliEngine::Agy => json!({
            "spawn": true,
            "restart": {
                "supported": true,
                "requires": {
                    "confirm_destroy_context": true
                }
            },
            "status": true,
            "session_identity": true,
            "observe": true,
            "input": true,
            "pty_step": true,
            "clear_input": true,
            "clear_screen": true,
            "exit": {
                "supported": true,
                "command": "/exit"
            },
            "usage_refresh": true,
            "mcp_status": true,
            "mcp_reconnect": driver.mcp_reconnect,
            "switch_model": driver.switch_model,
            "model_catalog": driver.model_catalog
        }),
        CliEngine::ClaudeCode => json!({
            "spawn": true,
            "restart": {
                "supported": true,
                "requires": {
                    "confirm_destroy_context": true
                }
            },
            "status": driver.status,
            "session_identity": true,
            "observe": true,
            "input": false,
            "pty_step": false,
            "clear_input": false,
            "clear_screen": false,
            "exit": false,
            "permissions": {
                "supported": driver.control_action,
                "modes": ["auto", "default", "accept_edits", "plan"],
                "cycle_key": "shift-tab",
                "cycle": ["auto", "default", "accept_edits", "plan"],
                "verification": "screen_identity.permission_mode",
                "bypass_permissions": {
                    "recognized": true,
                    "set_by": "spawn/restart with dangerously_skip_permissions=true",
                    "switch_supported": false
                }
            },
            "usage_refresh": driver.usage_probe,
            "mcp_status": driver.mcp_status,
            "mcp_reconnect": driver.mcp_reconnect,
            "dangerously_skip_permissions": {
                "supported": true,
                "launch_command": "claude --dangerously-skip-permissions",
                "request_fields": [
                    "dangerously_skip_permissions",
                    "dangerously_bypass",
                    "dangerously_bypass_approvals_and_sandbox",
                    "bypass_mode"
                ],
                "scope": "provider-box ClaudeCode PTY spawn/restart only"
            },
            "switch_model": {
                "supported": driver.switch_model,
                "command": "/model <model_id>",
                "allowed_model_ids": ["claude-opus-4-8", "claude-opus-4-6", "claude-sonnet-4-6"],
                "verification": "screen_identity.current_model"
            },
            "model_catalog": driver.model_catalog
        }),
        _ => json!({
            "spawn": true,
            "restart": {
                "supported": true,
                "requires": {
                    "confirm_destroy_context": true
                }
            },
            "status": driver.status,
            "input": driver.control_action,
            "pty_step": driver.pty_step,
            "clear_screen": driver.control_action,
            "exit": driver.control_action,
            "logout": false,
            "usage_refresh": driver.usage_probe,
            "mcp_status": driver.mcp_status,
            "mcp_reconnect": driver.mcp_reconnect,
            "switch_model": driver.switch_model,
            "model_catalog": driver.model_catalog
        }),
    };

    json!({
        "schema": "missiond.provider-box.slot-capabilities.v1",
        "engine": engine,
        "driver": driver,
        "slot_controls": slot_controls,
        "router_public": false,
        "public_rule": "slot-scoped controls are internal/operator surfaces; router-facing callers use logical model/task APIs."
    })
}

fn latest_screen_observation(result: &ProviderBoxResult) -> Value {
    result
        .step_records
        .last()
        .and_then(|step| serde_json::to_value(&step.after).ok())
        .unwrap_or(Value::Null)
}

fn openai_model_data(catalog: &ProviderModelCatalog) -> Value {
    Value::Array(
        catalog
            .entries
            .iter()
            .filter(|entry| is_agy_text_model_exportable(&entry.display_name))
            .map(|entry| {
                let slug = slug_model(&entry.display_name);
                let slot_pool_id = agy_slot_pool_id(&entry.display_name);
                json!({
                    "id": format!("agy-{slug}"),
                    "object": "model",
                    "created": 0,
                    "owned_by": "missiond/agy",
                    "provider": "agy_cli",
                    "source_id": format!("missiond/agy/{slug}"),
                    "display_name": entry.display_name.clone(),
                    "provider_model_id": entry.provider_model_id.clone(),
                    "slot_pool_id": slot_pool_id,
                    "capabilities": {
                        "text": true,
                        "tools": false,
                        "vision": false,
                        "files": false,
                        "mcp": false,
                        "shell": false
                    },
                    "pure_text": true,
                    "routeable_default": entry.routeable_default,
                    "switch_capability": entry.switch_capability,
                    "usage_probe_capability": entry.usage_probe_capability
                })
            })
            .collect(),
    )
}

fn provider_text_only_sources(catalog: &ProviderModelCatalog) -> Value {
    Value::Array(
        catalog
            .entries
            .iter()
            .filter(|entry| is_agy_text_model_exportable(&entry.display_name))
            .map(provider_text_only_source_entry)
            .collect(),
    )
}

fn provider_text_only_source_entry(entry: &ProviderModelCatalogEntry) -> Value {
    let slug = slug_model(&entry.display_name);
    let model_id = format!("agy-{slug}");
    let slot_pool_id = agy_slot_pool_id(&entry.display_name);
    json!({
        "schema": "missiond.provider-text-only-source.v1",
        "source_id": format!("missiond/agy/{slug}"),
        "provider": "agy_cli",
        "engine": "agy",
        "model_id": model_id,
        "provider_model_id": entry.provider_model_id.clone(),
        "model": entry.display_name.clone(),
        "slot_pool_id": slot_pool_id,
        "slot_policy": {
            "kind": "provider_box_managed_pool",
            "public_max_concurrent": 1,
            "replicas_hidden": true,
            "requires_current_model_verification": true,
            "hot_path_model_switch": false,
            "queue_owner": "provider-box"
        },
        "completion_endpoint": "/provider-box/v1/text-only/completions",
        "request_template": {
            "schema": "missiond.provider-box.text-only-completion-request.v1",
            "provider": "agy_cli",
            "engine": "agy",
            "model": entry.display_name.clone(),
            "pure_text": true,
            "allow_model_switch": false,
            "messages": [{
                "role": "user",
                "content": "<plain text prompt>"
            }]
        },
        "capabilities": {
            "text": true,
            "tools": false,
            "vision": false,
            "files": false,
            "mcp": false,
            "shell": false
        },
        "guard": {
            "prompt_instruction": false,
            "sidecar_correlation": true,
            "transcript_cursor_guard": true,
            "isolated_runtime_workspace": true,
            "agy_sandbox_flag": true,
            "durable_jsonl_guard": true,
            "permission_profile": "documented_deny_policy_plus_fail_closed_transcript_gate",
            "rejects_tool_messages": true,
            "rejects_tool_request_fields": true
        }
    })
}

fn append_codex_exec_text_exports(body: &mut Value) {
    if let Some(data) = body.get_mut("data").and_then(Value::as_array_mut) {
        data.extend(codex_exec_openai_model_data());
    }
    if let Some(sources) = body
        .get_mut("provider_text_only_sources")
        .and_then(Value::as_array_mut)
    {
        sources.extend(codex_exec_text_only_sources());
    }
}

fn append_codex_task_exports(body: &mut Value) {
    if let Some(data) = body.get_mut("data").and_then(Value::as_array_mut) {
        data.extend(codex_task_openai_model_data());
    }
    if body.get("provider_task_sources").is_none() {
        body["provider_task_sources"] = json!([]);
    }
    if let Some(sources) = body
        .get_mut("provider_task_sources")
        .and_then(Value::as_array_mut)
    {
        sources.extend(codex_task_sources());
    }
}

fn append_codex_exec_router_sources(body: &mut Value) {
    if let Some(sources) = body
        .get_mut("router_model_sources")
        .and_then(Value::as_array_mut)
    {
        sources.extend(codex_exec_router_sources());
    }
}

fn append_codex_task_router_sources(body: &mut Value) {
    if let Some(sources) = body
        .get_mut("router_model_sources")
        .and_then(Value::as_array_mut)
    {
        sources.extend(codex_task_router_sources());
    }
}

fn append_claude_code_text_exports(body: &mut Value) {
    if let Some(data) = body.get_mut("data").and_then(Value::as_array_mut) {
        data.extend(claude_code_text_openai_model_data());
    }
    if let Some(sources) = body
        .get_mut("provider_text_only_sources")
        .and_then(Value::as_array_mut)
    {
        sources.extend(claude_code_text_only_sources());
    }
}

fn append_claude_code_text_router_sources(body: &mut Value) {
    if let Some(sources) = body
        .get_mut("router_model_sources")
        .and_then(Value::as_array_mut)
    {
        sources.extend(claude_code_text_router_sources());
    }
}

fn append_claude_code_task_exports(body: &mut Value) {
    if let Some(data) = body.get_mut("data").and_then(Value::as_array_mut) {
        data.extend(claude_code_task_openai_model_data());
    }
    if body.get("provider_task_sources").is_none() {
        body["provider_task_sources"] = json!([]);
    }
    if let Some(sources) = body
        .get_mut("provider_task_sources")
        .and_then(Value::as_array_mut)
    {
        sources.extend(claude_code_task_sources());
    }
}

fn append_claude_code_task_router_sources(body: &mut Value) {
    if let Some(sources) = body
        .get_mut("router_model_sources")
        .and_then(Value::as_array_mut)
    {
        sources.extend(claude_code_task_router_sources());
    }
}

fn codex_exec_openai_model_data() -> Vec<Value> {
    codex_exec_text_source_defs()
        .into_iter()
        .map(|def| {
            json!({
                "id": def.model_id,
                "object": "model",
                "created": 0,
                "owned_by": "missiond/codex-exec-text",
                "provider": "codex_exec_text",
                "source_id": def.source_id,
                "display_name": def.display_name,
                "provider_model_id": "gpt-5.5",
                "model": "gpt-5.5",
                "model_profile": def.model_profile,
                "capabilities": {
                    "text": true,
                    "tools": false,
                    "vision": false,
                    "files": false,
                    "mcp": false,
                    "shell": false
                },
                "pure_text": true,
                "routeable_default": def.routeable,
                "routeable_status": def.routeable_status,
                "guarded": true
            })
        })
        .collect()
}

fn codex_task_openai_model_data() -> Vec<Value> {
    codex_task_source_defs()
        .into_iter()
        .map(|def| {
            json!({
                "id": def.model_id,
                "object": "model",
                "created": 0,
                "owned_by": "missiond/codex-task",
                "provider": def.provider,
                "source_id": def.source_id,
                "display_name": def.display_name,
                "provider_model_id": CODEX_TASK_MODEL,
                "model": CODEX_TASK_MODEL,
                "model_profile": def.model_profile,
                "capabilities": def.capabilities(),
                "pure_text": false,
                "task_source": true,
                "task_kind": def.task_kind,
                "routeable_default": def.routeable,
                "routeable_status": def.routeable_status,
                "guarded": true
            })
        })
        .collect()
}

fn codex_exec_text_only_sources() -> Vec<Value> {
    codex_exec_text_source_defs()
        .into_iter()
        .map(|def| {
            let mut template = json!({
                "schema": "missiond.provider-box.text-only-completion-request.v1",
                "provider": "codex_exec_text",
                "engine": "codex",
                "model": "gpt-5.5",
                "pure_text": true,
                "messages": [{
                    "role": "user",
                    "content": "<plain text prompt>"
                }]
            });
            if let Some(profile) = def.model_profile {
                template["model_profile"] = json!(profile);
            }
            json!({
                "schema": "missiond.provider-text-only-source.v1",
                "source_id": def.source_id,
                "provider": "codex_exec_text",
                "engine": "codex",
                "model_id": def.model_id,
                "provider_model_id": "gpt-5.5",
                "model": "gpt-5.5",
                "display_name": def.display_name,
                "model_profile": def.model_profile,
                "completion_endpoint": "/provider-box/v1/text-only/completions",
                "routeable": def.routeable,
                "routeable_status": def.routeable_status,
                "request_template": template,
                "capabilities": {
                    "text": true,
                    "tools": false,
                    "vision": false,
                    "files": false,
                    "mcp": false,
                    "shell": false
                },
                "guard": {
                    "codex_exec_json": true,
                    "output_last_message": true,
                    "ignore_user_config": true,
                    "ignore_rules": true,
                    "isolated_runtime_workspace": true,
                    "shell_tool_disabled": true,
                    "jsonl_tool_event_guard": true,
                    "rejects_tool_messages": true,
                    "rejects_tool_request_fields": true
                },
                "queue": {
                    "owner": "provider-box",
                    "key": codex_export_queue_key("codex_exec_text", "gpt-5.5", def.model_profile),
                    "max_concurrent": codex_export_queue_max_concurrent("codex_exec_text", def.model_profile),
                    "policy": "per_logical_codex_exec_source"
                }
            })
        })
        .collect()
}

fn codex_task_sources() -> Vec<Value> {
    codex_task_source_defs()
        .into_iter()
        .map(|def| {
            let mut template = json!({
                "schema": def.request_schema,
                "provider": def.provider,
                "engine": "codex",
                "model": CODEX_TASK_MODEL,
                "messages": [{
                    "role": "user",
                    "content": "<task prompt>"
                }]
            });
            if let Some(profile) = def.model_profile {
                template["model_profile"] = json!(profile);
            }
            json!({
                "schema": "missiond.provider-task-source.v1",
                "source_id": def.source_id,
                "provider": def.provider,
                "engine": "codex",
                "model_id": def.model_id,
                "provider_model_id": CODEX_TASK_MODEL,
                "model": CODEX_TASK_MODEL,
                "display_name": def.display_name,
                "model_profile": def.model_profile,
                "task_kind": def.task_kind,
                "completion_endpoint": def.endpoint,
                "routeable": def.routeable,
                "routeable_status": def.routeable_status,
                "request_template": template,
                "prompt_prefix": def.prompt_prefix,
                "capabilities": def.capabilities(),
                "guard": {
                    "codex_exec_json": true,
                    "output_last_message": true,
                    "jsonl_final_required": true,
                    "ignore_user_config": def.task_kind != "image_generation",
                    "ignore_rules": def.task_kind != "image_generation",
                    "isolated_runtime_workspace": true,
                    "read_only_sandbox": true,
                    "approval_policy": "never",
                    "shell_tool_disabled": true,
                    "mcp_disabled": true,
                    "file_access_disabled": true,
                    "jsonl_tool_allowlist_guard": true,
                    "rejects_external_tool_schemas": true,
                    "rejects_file_attachments": true
                },
                "queue": {
                    "owner": "provider-box",
                    "key": codex_export_queue_key(def.provider, CODEX_TASK_MODEL, def.model_profile),
                    "max_concurrent": codex_export_queue_max_concurrent(def.provider, def.model_profile),
                    "policy": "per_logical_codex_exec_source"
                }
            })
        })
        .collect()
}

fn codex_export_queue_key(provider: &str, model: &str, profile: Option<&str>) -> String {
    format!(
        "{}:{}:{}",
        provider,
        model,
        profile
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("default")
    )
}

fn codex_export_queue_max_concurrent(provider: &str, profile: Option<&str>) -> usize {
    if provider != "codex_exec_text" {
        return CODEX_EXEC_TASK_MAX_CONCURRENT;
    }
    let profile = profile
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("default");
    if profile.eq_ignore_ascii_case("xhigh") {
        CODEX_EXEC_TEXT_XHIGH_MAX_CONCURRENT
    } else {
        CODEX_EXEC_TEXT_DEFAULT_MAX_CONCURRENT
    }
}

fn codex_exec_router_sources() -> Vec<Value> {
    codex_exec_text_source_defs()
        .into_iter()
        .map(|def| {
            json!({
                "model_id": def.model_id,
                "display_name": def.display_name,
                "routeable": def.routeable,
                "routeable_status": def.routeable_status,
                "route": if def.routeable {
                    json!({
                        "provider": "codex_exec_text",
                        "provider_model_id": "gpt-5.5",
                        "model_profile": def.model_profile,
                        "completion_endpoint": "/provider-box/v1/text-only/completions"
                    })
                } else {
                    Value::Null
                },
                "blocked_reason": if def.routeable {
                    Value::Null
                } else {
                    json!(def.routeable_status)
                },
                "text_only_source": codex_exec_text_only_sources()
                    .into_iter()
                    .find(|source| source.get("model_id") == Some(&json!(def.model_id)))
            })
        })
        .collect()
}

fn codex_task_router_sources() -> Vec<Value> {
    codex_task_source_defs()
        .into_iter()
        .map(|def| {
            json!({
                "model_id": def.model_id,
                "display_name": def.display_name,
                "routeable": def.routeable,
                "routeable_status": def.routeable_status,
                "route": if def.routeable {
                    json!({
                        "provider": def.provider,
                        "provider_model_id": CODEX_TASK_MODEL,
                        "model_profile": def.model_profile,
                        "completion_endpoint": def.endpoint,
                        "task_kind": def.task_kind
                    })
                } else {
                    Value::Null
                },
                "blocked_reason": if def.routeable {
                    Value::Null
                } else {
                    json!(def.routeable_status)
                },
                "task_source": codex_task_sources()
                    .into_iter()
                    .find(|source| source.get("model_id") == Some(&json!(def.model_id)))
            })
        })
        .collect()
}

fn claude_code_text_openai_model_data() -> Vec<Value> {
    claude_code_text_source_defs()
        .into_iter()
        .map(|def| {
            json!({
                "id": def.model_id,
                "object": "model",
                "created": 0,
                "owned_by": "missiond/claude-code-text",
                "provider": CLAUDE_CODE_TEXT_PROVIDER,
                "source_id": def.source_id,
                "display_name": def.display_name,
                "provider_model_id": def.provider_model_id,
                "model": def.provider_model_id,
                "model_profile": def.model_profile,
                "capabilities": {
                    "text": true,
                    "tools": false,
                    "vision": false,
                    "files": false,
                    "mcp": false,
                    "shell": false
                },
                "pure_text": true,
                "routeable_default": true,
                "routeable_status": "live_smoke_passed",
                "guarded": true
            })
        })
        .collect()
}

fn claude_code_text_only_sources() -> Vec<Value> {
    claude_code_text_source_defs()
        .into_iter()
        .map(|def| {
            json!({
                "schema": "missiond.provider-text-only-source.v1",
                "source_id": def.source_id,
                "provider": CLAUDE_CODE_TEXT_PROVIDER,
                "engine": "claude_code",
                "model_id": def.model_id,
                "provider_model_id": def.provider_model_id,
                "model": def.provider_model_id,
                "display_name": def.display_name,
                "model_profile": def.model_profile,
                "completion_endpoint": "/provider-box/v1/text-only/completions",
                "routeable": true,
                "routeable_status": "live_smoke_passed",
                "request_template": {
                    "schema": "missiond.provider-box.text-only-completion-request.v1",
                    "provider": CLAUDE_CODE_TEXT_PROVIDER,
                    "engine": "claude_code",
                    "model": def.model_id,
                    "provider_model_id": def.provider_model_id,
                    "model_profile": def.model_profile,
                    "pure_text": true,
                    "messages": [{
                        "role": "user",
                        "content": "<plain text prompt>"
                    }]
                },
                "capabilities": {
                    "text": true,
                    "tools": false,
                    "vision": false,
                    "files": false,
                    "mcp": false,
                    "shell": false
                },
                "guard": {
                    "interactive_pty": true,
                    "uses_print_mode": false,
                    "per_request_session_id": true,
                    "isolated_runtime_workspace": true,
                    "mcp_config_empty": true,
                    "strict_mcp_config": true,
                    "tools_flag": "--tools ''",
                    "slash_commands_disabled": true,
                    "dangerously_skip_permissions": false,
                    "durable_jsonl_guard": true,
                    "rejects_tool_messages": true,
                    "rejects_tool_request_fields": true,
                    "fail_closed_on_tool_use": true
                },
                "queue": {
                    "owner": "provider-box",
                    "key": claude_code_text_queue_key(def.model_id),
                    "max_concurrent": CLAUDE_CODE_TEXT_MAX_CONCURRENT,
                    "policy": "per_logical_claude_code_text_source"
                },
                "slot_policy": {
                    "kind": "provider_box_managed_ephemeral_private_slot",
                    "public_max_concurrent": 1,
                    "private_slot_exposed": false,
                    "restart_after_request": true,
                    "queue_owner": "provider-box"
                }
            })
        })
        .collect()
}

fn claude_code_text_router_sources() -> Vec<Value> {
    claude_code_text_source_defs()
        .into_iter()
        .map(|def| {
            json!({
                "model_id": def.model_id,
                "display_name": def.display_name,
                "routeable": true,
                "routeable_status": "live_smoke_passed",
                "route": {
                    "provider": CLAUDE_CODE_TEXT_PROVIDER,
                    "provider_model_id": def.provider_model_id,
                    "model_profile": def.model_profile,
                    "completion_endpoint": "/provider-box/v1/text-only/completions"
                },
                "blocked_reason": Value::Null,
                "text_only_source": claude_code_text_only_sources()
                    .into_iter()
                    .find(|source| source.get("model_id") == Some(&json!(def.model_id)))
            })
        })
        .collect()
}

fn claude_code_task_openai_model_data() -> Vec<Value> {
    vec![json!({
        "id": CLAUDE_CODE_DEEP_RESEARCH_MODEL_ID,
        "object": "model",
        "created": 0,
        "owned_by": "missiond/claude-code-task",
        "provider": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER,
        "source_id": "missiond/claude-code-deep-research/opus-4-8-xhigh",
        "display_name": "ClaudeCode Deep Research Opus 4.8 (xhigh)",
        "provider_model_id": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER_MODEL,
        "model": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER_MODEL,
        "model_profile": CLAUDE_CODE_DEEP_RESEARCH_MODEL_PROFILE,
        "capabilities": claude_code_deep_research_capabilities(),
        "pure_text": false,
        "task_source": true,
        "task_kind": "deep_research",
        "routeable_default": true,
        "routeable_status": "durable_workflow_journal_guarded",
        "guarded": true
    })]
}

fn claude_code_task_sources() -> Vec<Value> {
    vec![json!({
        "schema": "missiond.provider-task-source.v1",
        "source_id": "missiond/claude-code-deep-research/opus-4-8-xhigh",
        "provider": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER,
        "engine": "claude_code",
        "model_id": CLAUDE_CODE_DEEP_RESEARCH_MODEL_ID,
        "provider_model_id": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER_MODEL,
        "model": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER_MODEL,
        "display_name": "ClaudeCode Deep Research Opus 4.8 (xhigh)",
        "model_profile": CLAUDE_CODE_DEEP_RESEARCH_MODEL_PROFILE,
        "task_kind": "deep_research",
        "completion_endpoint": "/provider-box/v1/claude-code/deep-research/completions",
        "routeable": true,
        "routeable_status": "durable_workflow_journal_guarded",
        "request_template": {
            "schema": "missiond.provider-box.claude-code-deep-research-request.v1",
            "provider": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER,
            "engine": "claude_code",
            "model": CLAUDE_CODE_DEEP_RESEARCH_MODEL_ID,
            "model_profile": CLAUDE_CODE_DEEP_RESEARCH_MODEL_PROFILE,
            "messages": [{
                "role": "user",
                "content": "<research question>"
            }]
        },
        "prompt_prefix": CLAUDE_CODE_DEEP_RESEARCH_PROMPT_PREFIX,
        "capabilities": claude_code_deep_research_capabilities(),
        "guard": {
            "interactive_pty": true,
            "uses_print_mode": false,
            "per_request_session_id": true,
            "private_ephemeral_slot": true,
            "skill_required": "deep-research",
            "workflow_required": "deep-research",
            "durable_jsonl_cursor": true,
            "durable_workflow_journal_required": true,
            "rejects_external_tool_schemas": true,
            "rejects_file_attachments": true,
            "private_slot_exposed": false
        },
        "queue": {
            "owner": "provider-box",
            "key": claude_code_deep_research_queue_key(),
            "max_concurrent": CLAUDE_CODE_DEEP_RESEARCH_MAX_CONCURRENT,
            "policy": "per_logical_claude_code_deep_research_source"
        },
        "slot_policy": {
            "kind": "provider_box_managed_ephemeral_private_slot",
            "public_max_concurrent": 1,
            "private_slot_exposed": false,
            "restart_after_request": true,
            "queue_owner": "provider-box"
        }
    })]
}

fn claude_code_task_router_sources() -> Vec<Value> {
    vec![json!({
        "model_id": CLAUDE_CODE_DEEP_RESEARCH_MODEL_ID,
        "display_name": "ClaudeCode Deep Research Opus 4.8 (xhigh)",
        "routeable": true,
        "routeable_status": "durable_workflow_journal_guarded",
        "route": {
            "provider": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER,
            "provider_model_id": CLAUDE_CODE_DEEP_RESEARCH_PROVIDER_MODEL,
            "model_profile": CLAUDE_CODE_DEEP_RESEARCH_MODEL_PROFILE,
            "completion_endpoint": "/provider-box/v1/claude-code/deep-research/completions",
            "task_kind": "deep_research"
        },
        "blocked_reason": Value::Null,
        "task_source": claude_code_task_sources()
            .into_iter()
            .find(|source| source.get("model_id") == Some(&json!(CLAUDE_CODE_DEEP_RESEARCH_MODEL_ID)))
    })]
}

fn claude_code_deep_research_capabilities() -> Value {
    json!({
        "text": true,
        "tools": true,
        "tool_allowlist": ["Skill", "Workflow"],
        "workflow": true,
        "workflow_name": "deep-research",
        "web_search": true,
        "image_generation": false,
        "vision": false,
        "files": false,
        "mcp": false,
        "shell": false,
        "external_tools": false
    })
}

fn claude_code_text_queue_key(model_id: &str) -> String {
    format!("{CLAUDE_CODE_TEXT_PROVIDER}:{model_id}")
}

fn claude_code_deep_research_queue_key() -> String {
    format!("{CLAUDE_CODE_DEEP_RESEARCH_PROVIDER}:{CLAUDE_CODE_DEEP_RESEARCH_MODEL_ID}")
}

fn is_exported_claude_code_text_model(model_id: &str, model_profile: Option<&str>) -> bool {
    let model_id = model_id.trim();
    claude_code_text_source_defs().into_iter().any(|def| {
        def.model_id.eq_ignore_ascii_case(model_id)
            && model_profile
                .map(|profile| profile.trim().eq_ignore_ascii_case(def.model_profile))
                .unwrap_or(true)
    })
}

fn claude_code_deep_research_model_ref_matches(model_id: &str) -> bool {
    let normalized = model_id.trim().to_ascii_lowercase().replace('_', "-");
    normalized == CLAUDE_CODE_DEEP_RESEARCH_MODEL_ID
        || normalized == CLAUDE_CODE_DEEP_RESEARCH_PROVIDER_MODEL
        || normalized == "claude-code-deep-research-opus-48-xhigh"
        || normalized == "claude-code-deep-research"
}

#[derive(Clone, Copy)]
struct CodexExecTextSourceDef {
    model_id: &'static str,
    source_id: &'static str,
    display_name: &'static str,
    model_profile: Option<&'static str>,
    routeable: bool,
    routeable_status: &'static str,
}

#[derive(Clone, Copy)]
struct CodexTaskSourceDef {
    model_id: &'static str,
    source_id: &'static str,
    display_name: &'static str,
    provider: &'static str,
    task_kind: &'static str,
    endpoint: &'static str,
    request_schema: &'static str,
    prompt_prefix: &'static str,
    model_profile: Option<&'static str>,
    routeable: bool,
    routeable_status: &'static str,
}

#[derive(Clone, Copy)]
struct ClaudeCodeTextSourceDef {
    model_id: &'static str,
    source_id: &'static str,
    display_name: &'static str,
    provider_model_id: &'static str,
    model_profile: &'static str,
}

impl CodexTaskSourceDef {
    fn capabilities(self) -> Value {
        json!({
            "text": true,
            "tools": true,
            "tool_allowlist": match self.task_kind {
                "research" => json!(["web_search"]),
                "image_generation" => json!(["image_generation"]),
                _ => json!([]),
            },
            "web_search": self.task_kind == "research",
            "image_generation": self.task_kind == "image_generation",
            "vision": false,
            "files": false,
            "mcp": false,
            "shell": false
        })
    }
}

fn codex_exec_text_source_defs() -> Vec<CodexExecTextSourceDef> {
    vec![
        CodexExecTextSourceDef {
            model_id: "codex-gpt-55-xhigh",
            source_id: "missiond/codex-exec-text/gpt-55-xhigh",
            display_name: "Codex GPT-5.5 (xhigh)",
            model_profile: Some("xhigh"),
            routeable: true,
            routeable_status: "live_smoke_passed",
        },
        CodexExecTextSourceDef {
            model_id: "codex-gpt-55-default",
            source_id: "missiond/codex-exec-text/gpt-55-default",
            display_name: "Codex GPT-5.5 (default reasoning)",
            model_profile: None,
            routeable: true,
            routeable_status: "live_smoke_passed",
        },
    ]
}

fn codex_task_source_defs() -> Vec<CodexTaskSourceDef> {
    vec![
        CodexTaskSourceDef {
            model_id: "codex-research-gpt-55-xhigh",
            source_id: "missiond/codex-research/gpt-55-xhigh",
            display_name: "Codex Research GPT-5.5 (xhigh)",
            provider: CODEX_RESEARCH_PROVIDER,
            task_kind: "research",
            endpoint: "/provider-box/v1/research/completions",
            request_schema: "missiond.provider-box.research-completion-request.v1",
            prompt_prefix: CODEX_RESEARCH_PROMPT_PREFIX,
            model_profile: Some("xhigh"),
            routeable: true,
            routeable_status: "live_smoke_passed",
        },
        CodexTaskSourceDef {
            model_id: "codex-image-generation-gpt-55-default",
            source_id: "missiond/codex-image-generation/gpt-55-default",
            display_name: "Codex Image Generation GPT-5.5 (default reasoning)",
            provider: CODEX_IMAGE_PROVIDER,
            task_kind: "image_generation",
            endpoint: "/provider-box/v1/image-generation/completions",
            request_schema: "missiond.provider-box.image-generation-completion-request.v1",
            prompt_prefix: CODEX_IMAGE_PROMPT_PREFIX,
            model_profile: None,
            routeable: true,
            routeable_status: "live_smoke_passed",
        },
    ]
}

fn claude_code_text_source_defs() -> Vec<ClaudeCodeTextSourceDef> {
    vec![
        ClaudeCodeTextSourceDef {
            model_id: "claude-code-opus-4-8-xhigh",
            source_id: "missiond/claude-code-text/opus-4-8-xhigh",
            display_name: "ClaudeCode Opus 4.8 (xhigh)",
            provider_model_id: "claude-opus-4-8",
            model_profile: "xhigh",
        },
        ClaudeCodeTextSourceDef {
            model_id: "claude-code-opus-4-6-high",
            source_id: "missiond/claude-code-text/opus-4-6-high",
            display_name: "ClaudeCode Opus 4.6 (high)",
            provider_model_id: "claude-opus-4-6",
            model_profile: "high",
        },
        ClaudeCodeTextSourceDef {
            model_id: "claude-code-sonnet-4-6-high",
            source_id: "missiond/claude-code-text/sonnet-4-6-high",
            display_name: "ClaudeCode Sonnet 4.6 (high)",
            provider_model_id: "claude-sonnet-4-6",
            model_profile: "high",
        },
    ]
}

fn router_model_sources(catalog: &ProviderModelCatalog, export: &ProviderRouterExport) -> Value {
    let routeable_by_model = export
        .routeable_entries
        .iter()
        .filter_map(|route| {
            let model_id = route.get("model_id")?.as_str()?.to_string();
            Some((model_id, route.clone()))
        })
        .collect::<std::collections::HashMap<_, _>>();

    Value::Array(
        catalog
            .entries
            .iter()
            .filter(|entry| is_agy_text_model_exportable(&entry.display_name))
            .map(|entry| {
                let slug = slug_model(&entry.display_name);
                let model_id = format!("agy-{slug}");
                let blocked = export.blocked_entries.iter().find(|blocked| {
                    blocked
                        .get("entry")
                        .and_then(|entry| entry.get("model_id"))
                        .and_then(Value::as_str)
                        == Some(model_id.as_str())
                });
                json!({
                    "model_id": model_id,
                    "display_name": entry.display_name.clone(),
                    "routeable": routeable_by_model.contains_key(&format!("agy-{slug}")),
                    "route": routeable_by_model.get(&format!("agy-{slug}")),
                    "blocked_reason": blocked.and_then(|blocked| blocked.get("reason")).cloned(),
                    "text_only_source": provider_text_only_source_entry(entry)
                })
            })
            .collect(),
    )
}

fn private_agy_slot_ids_for_model(model: &str) -> Vec<String> {
    private_agy_slot_ids_for_model_and_lane(model, None)
}

fn private_agy_slot_ids_for_model_and_lane(model: &str, lane: Option<&str>) -> Vec<String> {
    let slug = slug_model(model);
    let lane = lane.and_then(sanitize_slot_lane);
    let base = match lane {
        Some(lane) => format!("slot-agy-{slug}-{lane}"),
        None => format!("slot-agy-{slug}"),
    };
    vec![format!("{base}-a"), format!("{base}-b")]
}

fn is_agy_text_model_exportable(model: &str) -> bool {
    !slug_model(model).starts_with("gpt-oss-")
}

fn agy_slot_pool_id(model: &str) -> String {
    agy_slot_pool_id_for_lane(model, None)
}

fn agy_slot_pool_id_for_lane(model: &str, lane: Option<&str>) -> String {
    let mut pool_id = format!("slot-pool-agy-{}", slug_model(model));
    if let Some(lane) = lane.and_then(sanitize_slot_lane) {
        pool_id.push('-');
        pool_id.push_str(&lane);
    }
    pool_id
}

fn provider_box_lane(body: &Value) -> Option<String> {
    string_field(body, "provider_box_lane")
        .or_else(|| string_field(body, "xjp_request_stage"))
        .or_else(|| string_field(body, "pipeline_stage"))
        .or_else(|| string_field(body, "stage"))
        .and_then(|value| sanitize_slot_lane(&value))
}

fn sanitize_slot_lane(value: &str) -> Option<String> {
    let mut slug = String::new();
    let mut last_was_sep = false;
    for ch in value.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_sep = false;
        } else if !last_was_sep {
            slug.push('-');
            last_was_sep = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() || slug == "default" {
        return None;
    }
    Some(slug.chars().take(48).collect())
}

fn redact_private_slot_details(result: &mut ProviderBoxResult, _model: &str) {
    result.slot_id = None;
    result.slot_status = None;
    result.step_records.clear();
    for diagnostic in &mut result.diagnostics {
        diagnostic.details = redact_slot_ids_in_value(diagnostic.details.clone());
    }
}

fn redact_private_claude_code_text_details(result: &mut ProviderBoxResult) {
    result.slot_id = None;
    result.slot_status = None;
    result.step_records.clear();
    result.provider_conversation_id = None;
    if let Some(identity) = result.provider_session_identity.as_mut() {
        identity.slot_id = None;
        identity.provider_session_id = None;
        identity.provider_session_ref = None;
        identity.resume_command = None;
        identity.durable_source = Some("claude_code_session_jsonl_cursor".to_string());
        identity.workspace = None;
    }
    if result.durable_source.is_some() {
        result.durable_source = Some("claude_code_session_jsonl_cursor".to_string());
    }
    for diagnostic in &mut result.diagnostics {
        diagnostic.details =
            redact_private_paths_in_value(redact_slot_ids_in_value(diagnostic.details.clone()));
    }
}

fn redact_private_claude_code_task_details(result: &mut ProviderBoxResult) {
    redact_private_claude_code_text_details(result);
    if let Some(identity) = result.provider_session_identity.as_mut() {
        identity.durable_source = Some("claude_code_deep_research_workflow_journal".to_string());
    }
    if result.durable_source.is_some() {
        result.durable_source = Some("claude_code_deep_research_workflow_journal".to_string());
    }
}

fn redact_slot_ids_in_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(key, _)| key != "slot_id" && key != "slot_ids")
                .map(|(key, value)| (key, redact_slot_ids_in_value(value)))
                .collect(),
        ),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(redact_slot_ids_in_value).collect())
        }
        other => other,
    }
}

fn redact_private_paths_in_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(key, _)| {
                    !matches!(
                        key.as_str(),
                        "jsonl_path"
                            | "durable_jsonl"
                            | "workspace"
                            | "runtime_dir"
                            | "mcp_config"
                            | "private_slot_id"
                            | "session_id"
                            | "provider_session_id"
                            | "provider_session_ref"
                            | "transcript_dir"
                            | "script_path"
                            | "journal_path"
                            | "path"
                            | "file"
                    )
                })
                .map(|(key, value)| (key, redact_private_paths_in_value(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(redact_private_paths_in_value)
                .collect(),
        ),
        other => other,
    }
}

fn slug_model(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_ascii_lowercase()
        .replace("(", "")
        .replace(")", "")
        .replace("/", "-")
        .replace('.', "")
        .replace('+', "plus")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .filter(|value| value.is_ascii_alphanumeric() || *value == '-')
        .collect()
}

fn json_response(status: u16, body: Value) -> ProviderBoxHttpResponse {
    ProviderBoxHttpResponse {
        status,
        content_type: "application/json".to_string(),
        body,
    }
}

fn header_slot_id(request: &ProviderBoxHttpRequest) -> Option<String> {
    request
        .headers
        .get("x-slot-id")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn usage_refresh_slot_id(request: &ProviderBoxHttpRequest, engine: CliEngine) -> String {
    header_slot_id(request)
        .or_else(|| string_field(&request.body, "slot_id"))
        .unwrap_or_else(|| match engine {
            CliEngine::Codex => CODEX_USAGE_PROBE_SLOT.to_string(),
            _ => AGY_USAGE_PROBE_SLOT.to_string(),
        })
}

fn query_field(path: &str, key: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (pair_key, pair_value) = pair.split_once('=').unwrap_or((pair, ""));
        if pair_key == key {
            Some(pair_value.trim().to_string()).filter(|value| !value.is_empty())
        } else {
            None
        }
    })
}

fn query_bool_field(path: &str, key: &str) -> Option<bool> {
    query_field(path, key).and_then(|value| match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "y" | "on" => Some(true),
        "0" | "false" | "no" | "n" | "off" => Some(false),
        _ => None,
    })
}

fn models_live_catalog_requested(request: &ProviderBoxHttpRequest) -> bool {
    [
        "live_catalog",
        "refresh_live_catalog",
        "live",
        "use_live_catalog",
    ]
    .into_iter()
    .find_map(|key| bool_field(&request.body, key).or_else(|| query_bool_field(&request.path, key)))
    .unwrap_or(false)
}

fn string_field(body: &Value, key: &str) -> Option<String> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn raw_string_field(body: &Value, key: &str) -> Option<String> {
    body.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn bool_field(body: &Value, key: &str) -> Option<bool> {
    body.get(key).and_then(Value::as_bool)
}

fn tool_policy_from_body(body: &Value) -> Option<Value> {
    if let Some(policy) = body.get("tool_policy") {
        if policy.is_object() {
            return Some(policy.clone());
        }
    }

    let mut policy = serde_json::Map::new();
    for key in [
        "search_enabled",
        "sandbox",
        "approval_policy",
        "dangerously_bypass_approvals_and_sandbox",
        "dangerously_skip_permissions",
        "dangerously_bypass",
        "bypass_approvals_and_sandbox",
        "bypass_mode",
        "bypass",
    ] {
        if let Some(value) = body.get(key) {
            policy.insert(key.to_string(), value.clone());
        }
    }

    if policy.is_empty() {
        None
    } else {
        Some(Value::Object(policy))
    }
}

fn pty_step_key_allowed(key: &str) -> bool {
    let normalized = key
        .trim()
        .to_ascii_lowercase()
        .replace(['_', '-'], "")
        .replace(['+', '/'], "")
        .replace("control", "ctrl")
        .replace(' ', "");
    matches!(
        normalized.as_str(),
        "enter"
            | "return"
            | "esc"
            | "escape"
            | "up"
            | "arrowup"
            | "down"
            | "arrowdown"
            | "left"
            | "arrowleft"
            | "right"
            | "arrowright"
            | "tab"
            | "shifttab"
            | "backtab"
            | "backspace"
            | "delete"
            | "del"
            | "ctrlc"
            | "ctrld"
            | "pageup"
            | "pagedown"
            | "home"
            | "end"
    )
}

fn provider_box_internal_token() -> Option<String> {
    std::env::var("MISSIOND_PROVIDER_BOX_INTERNAL_TOKEN")
        .ok()
        .or_else(|| std::env::var("MISSIOND_AGY_INTERNAL_TOKEN").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use crate::provider_box::types::{ProviderSessionIdentity, DIAG_PROVIDER_UPSTREAM_UNAVAILABLE};

    use super::*;

    #[test]
    fn text_only_body_builds_raw_prompt_with_sidecar_correlation() {
        let body = json!({
            "schema": "missiond.provider-box.text-only-completion-request.v1",
            "provider": "agy_cli",
            "engine": "agy",
            "model": "Gemini 3.5 Flash (High)",
            "slot_id": "slot-agy-gemini-35-flash-high",
            "correlation_id": "corr-test",
            "provider_box_lane": "chat_json_planning",
            "xjp_request_stage": "chat_json_planning",
            "messages": [{"role": "user", "content": "hello"}],
            "pure_text": true
        });

        let request = text_only_interaction_from_body(&body).expect("request");

        assert_eq!(request.model.as_deref(), Some("Gemini 3.5 Flash (High)"));
        assert_eq!(
            request.slot_id.as_deref(),
            Some("slot-agy-gemini-35-flash-high")
        );
        assert_eq!(request.correlation_id, "corr-test");
        assert_eq!(
            request.provider_box_lane.as_deref(),
            Some("chat-json-planning")
        );
        assert_eq!(
            request.xjp_request_stage.as_deref(),
            Some("chat_json_planning")
        );
        assert_eq!(request.prompt.as_deref(), Some("hello"));
        assert!(request.no_tools);
        let policy = request.model_switch_policy.expect("model policy");
        assert!(!policy.allow_respawn);
        assert!(policy.require_verification);
    }

    #[test]
    fn tool_policy_from_body_collects_codex_launch_toggles() {
        let policy = tool_policy_from_body(&json!({
            "engine": "codex",
            "model": "gpt-5.5",
            "reasoning_effort": "xhigh",
            "dangerously_bypass_approvals_and_sandbox": true,
            "search_enabled": true,
        }))
        .expect("tool policy");

        assert_eq!(
            policy["dangerously_bypass_approvals_and_sandbox"],
            Value::Bool(true)
        );
        assert_eq!(policy["search_enabled"], Value::Bool(true));
        assert!(policy.get("reasoning_effort").is_none());
    }

    #[test]
    fn text_only_prompt_keeps_tool_policy_out_of_model_text() {
        let body = json!({
            "schema": "missiond.provider-box.text-only-completion-request.v1",
            "provider": "agy_cli",
            "engine": "agy",
            "model": "Gemini 3.5 Flash (High)",
            "slot_id": "slot-agy-gemini-35-flash-high",
            "correlation_id": "corr-tool-required",
            "messages": [{"role": "user", "content": "read a file and summarize it"}],
            "pure_text": true
        });

        let request = text_only_interaction_from_body(&body).expect("request");
        let prompt = request.prompt.expect("prompt");

        assert_eq!(request.correlation_id, "corr-tool-required");
        assert_eq!(prompt, "read a file and summarize it");
        assert!(!prompt.contains("不要使用工具"));
        assert!(!prompt.contains("请求编号"));
        assert!(request.no_tools);
    }

    #[test]
    fn codex_exec_text_only_body_builds_guarded_request() {
        let body = json!({
            "schema": "missiond.provider-box.text-only-completion-request.v1",
            "provider": "codex_exec_text",
            "engine": "codex",
            "model": "gpt-5.5",
            "model_profile": "xhigh",
            "correlation_id": "corr-codex-text",
            "messages": [{"role": "user", "content": "hello"}],
            "pure_text": true
        });

        let request = text_only_interaction_from_body(&body).expect("request");

        assert_eq!(request.engine, CliEngine::Codex);
        assert_eq!(request.provider.as_deref(), Some("codex_exec_text"));
        assert_eq!(request.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(request.model_profile.as_deref(), Some("xhigh"));
        assert_eq!(request.prompt.as_deref(), Some("hello"));
        assert!(request.no_tools);
        assert!(request.no_shell);
        assert!(request.model_switch_policy.is_none());
    }

    #[test]
    fn claude_code_text_only_body_builds_guarded_logical_request() {
        let body = json!({
            "schema": "missiond.provider-box.text-only-completion-request.v1",
            "provider": "claude_code_text",
            "engine": "claude_code",
            "model": "claude-code-opus-4-8-xhigh",
            "model_profile": "xhigh",
            "correlation_id": "corr-claude-text",
            "messages": [{"role": "user", "content": "hello"}],
            "pure_text": true
        });

        let request = text_only_interaction_from_body(&body).expect("request");

        assert_eq!(request.engine, CliEngine::ClaudeCode);
        assert_eq!(request.provider.as_deref(), Some("claude_code_text"));
        assert_eq!(request.model.as_deref(), Some("claude-code-opus-4-8-xhigh"));
        assert_eq!(request.model_profile.as_deref(), Some("xhigh"));
        assert_eq!(request.prompt.as_deref(), Some("hello"));
        assert!(request.slot_id.is_none());
        assert!(request.no_tools);
        assert!(request.no_mcp);
        assert!(request.no_shell);
        assert!(request.no_file_access);
        assert!(request.model_switch_policy.is_none());
    }

    #[test]
    fn claude_code_text_only_body_rejects_unexported_default_model() {
        let body = json!({
            "schema": "missiond.provider-box.text-only-completion-request.v1",
            "provider": "claude_code_text",
            "engine": "claude_code",
            "model": "claude-code-default",
            "messages": [{"role": "user", "content": "hello"}],
            "pure_text": true
        });

        assert!(text_only_interaction_from_body(&body).is_none());
    }

    #[test]
    fn claude_code_text_only_body_rejects_mismatched_profile() {
        let body = json!({
            "schema": "missiond.provider-box.text-only-completion-request.v1",
            "provider": "claude_code_text",
            "engine": "claude_code",
            "model": "claude-code-opus-4-8-xhigh",
            "model_profile": "high",
            "messages": [{"role": "user", "content": "hello"}],
            "pure_text": true
        });

        assert!(text_only_interaction_from_body(&body).is_none());
    }

    #[test]
    fn research_body_builds_prefixed_codex_task_request() {
        let body = json!({
            "schema": "missiond.provider-box.research-completion-request.v1",
            "provider": "codex_research",
            "engine": "codex",
            "model": "gpt-5.5",
            "model_profile": "xhigh",
            "correlation_id": "corr-research",
            "messages": [{"role": "user", "content": "上海今天有什么科技新闻？"}]
        });

        let request = codex_task_interaction_from_body(&body, BoxCommand::Research)
            .expect("research request");

        assert_eq!(request.engine, CliEngine::Codex);
        assert_eq!(request.command, BoxCommand::Research);
        assert_eq!(request.provider.as_deref(), Some("codex_research"));
        assert_eq!(request.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(request.model_profile.as_deref(), Some("xhigh"));
        assert!(request
            .prompt
            .as_deref()
            .expect("prompt")
            .starts_with(CODEX_RESEARCH_PROMPT_PREFIX));
        assert!(!request.no_tools);
        assert!(request.no_shell);
        assert!(request.no_mcp);
        assert!(request.no_file_access);
        assert_eq!(
            request.tool_policy.as_ref().unwrap()["allowed_tools"],
            json!(["web_search"])
        );
    }

    #[test]
    fn claude_code_deep_research_body_builds_workflow_task_request() {
        let body = json!({
            "schema": "missiond.provider-box.claude-code-deep-research-request.v1",
            "provider": "claude_code_deep_research",
            "engine": "claude_code",
            "model": "claude-code-deep-research-opus-4-8-xhigh",
            "model_profile": "xhigh",
            "correlation_id": "corr-claude-research",
            "messages": [{"role": "user", "content": "调研 macOS OCR 方案"}]
        });

        let request =
            claude_code_deep_research_interaction_from_body(&body).expect("research request");

        assert_eq!(request.engine, CliEngine::ClaudeCode);
        assert_eq!(request.command, BoxCommand::Research);
        assert_eq!(
            request.provider.as_deref(),
            Some(CLAUDE_CODE_DEEP_RESEARCH_PROVIDER)
        );
        assert_eq!(
            request.model.as_deref(),
            Some(CLAUDE_CODE_DEEP_RESEARCH_MODEL_ID)
        );
        assert_eq!(
            request.model_profile.as_deref(),
            Some(CLAUDE_CODE_DEEP_RESEARCH_MODEL_PROFILE)
        );
        let prompt = request.prompt.as_deref().expect("prompt");
        assert!(prompt.starts_with(CLAUDE_CODE_DEEP_RESEARCH_PROMPT_PREFIX));
        assert!(prompt.contains("call deep-research:"));
        assert!(prompt.contains("调研 macOS OCR 方案"));
        assert!(!request.no_tools);
        assert_eq!(
            request.tool_policy.as_ref().unwrap()["allowed_tools"],
            json!(["Skill", "Workflow"])
        );
        assert_eq!(
            request.output_contract.as_ref().unwrap()["durable_source"],
            "claude_code_deep_research_workflow_journal"
        );
        assert!(request.slot_id.is_none());
        assert!(request
            .desired_worker
            .as_ref()
            .unwrap()
            .get("provider_session_id")
            .is_some());
    }

    #[test]
    fn claude_code_deep_research_body_rejects_external_slot_and_tools() {
        let with_slot = json!({
            "provider": "claude_code_deep_research",
            "engine": "claude_code",
            "prompt": "查资料",
            "slot_id": "slot-claude-code-default"
        });
        assert!(claude_code_deep_research_interaction_from_body(&with_slot).is_none());

        let with_tools = json!({
            "provider": "claude_code_deep_research",
            "engine": "claude_code",
            "prompt": "查资料",
            "tools": [{"type": "function", "function": {"name": "do_anything"}}]
        });
        assert!(claude_code_deep_research_interaction_from_body(&with_tools).is_none());
    }

    #[test]
    fn claude_code_deep_research_redaction_hides_private_runtime_details() {
        let request = ProviderInteractionRequest::new(BoxCommand::Research, CliEngine::ClaudeCode);
        let mut result = ProviderBoxResult::base(&request, ProviderBoxStatus::Completed);
        result.slot_id = Some("slot-claude-code-deep-research-private".to_string());
        result.slot_status = Some(json!({
            "private_slot_id": "slot-claude-code-deep-research-private",
            "session_id": "019e82f0-private",
            "transcript_dir": "/Users/jinchen/.claude/projects/private/subagents/workflows/wf"
        }));
        result.provider_conversation_id = Some("019e82f0-private".to_string());
        result.provider_session_identity = Some(ProviderSessionIdentity::resolved(
            Some(CLAUDE_CODE_DEEP_RESEARCH_PROVIDER.to_string()),
            CliEngine::ClaudeCode,
            Some("slot-claude-code-deep-research-private".to_string()),
            "019e82f0-private".to_string(),
            "claude_code_deep_research_workflow_journal",
            Some("/Users/jinchen/.claude/projects/private/journal.jsonl".to_string()),
            Some("/Users/jinchen/.missiond/runtime/claude-code-deep-research".to_string()),
            "durable_workflow_report",
        ));
        result.durable_source =
            Some("/Users/jinchen/.claude/projects/private/journal.jsonl".to_string());
        result.add_diagnostic(ProviderBoxDiagnostic::warning(
            "TEST_PRIVATE_DIAGNOSTIC",
            "private detail fixture",
            json!({
                "session_id": "019e82f0-private",
                "provider_session_id": "019e82f0-private",
                "private_slot_id": "slot-claude-code-deep-research-private",
                "journal_path": "/Users/jinchen/.claude/projects/private/journal.jsonl",
                "nested": {
                    "transcript_dir": "/Users/jinchen/.claude/projects/private/subagents/workflows/wf",
                    "script_path": "/Users/jinchen/.claude/projects/private/workflows/scripts/deep-research.js",
                    "ok": true
                }
            }),
        ));

        redact_private_claude_code_task_details(&mut result);

        assert!(result.slot_id.is_none());
        assert!(result.slot_status.is_none());
        assert!(result.step_records.is_empty());
        assert!(result.provider_conversation_id.is_none());
        let identity = result.provider_session_identity.as_ref().expect("identity");
        assert!(identity.slot_id.is_none());
        assert!(identity.provider_session_id.is_none());
        assert!(identity.provider_session_ref.is_none());
        assert!(identity.resume_command.is_none());
        assert!(identity.workspace.is_none());
        assert_eq!(
            identity.durable_source.as_deref(),
            Some("claude_code_deep_research_workflow_journal")
        );
        assert_eq!(
            result.durable_source.as_deref(),
            Some("claude_code_deep_research_workflow_journal")
        );
        let details = &result.diagnostics[0].details;
        assert!(details.get("session_id").is_none());
        assert!(details.get("provider_session_id").is_none());
        assert!(details.get("private_slot_id").is_none());
        assert!(details.get("journal_path").is_none());
        assert!(details["nested"].get("transcript_dir").is_none());
        assert!(details["nested"].get("script_path").is_none());
        assert_eq!(details["nested"]["ok"], true);
    }

    #[test]
    fn image_generation_body_builds_prefixed_codex_task_request() {
        let body = json!({
            "schema": "missiond.provider-box.image-generation-completion-request.v1",
            "provider": "codex_image_generation",
            "engine": "codex",
            "prompt": "画一张雨夜赛博朋克城市海报"
        });

        let request = codex_task_interaction_from_body(&body, BoxCommand::ImageGeneration)
            .expect("image request");

        assert_eq!(request.command, BoxCommand::ImageGeneration);
        assert_eq!(request.provider.as_deref(), Some("codex_image_generation"));
        assert_eq!(request.model.as_deref(), Some("gpt-5.5"));
        assert!(request
            .prompt
            .as_deref()
            .expect("prompt")
            .starts_with(CODEX_IMAGE_PROMPT_PREFIX));
        let prompt = request.prompt.as_deref().expect("prompt");
        assert!(prompt.contains("调用并使用 imagegen"));
        assert!(prompt.contains("必须实际产出图片文件"));
        assert!(prompt.contains("不要只描述图片"));
        assert!(prompt.contains("只回复 IMAGE_DONE"));
        assert_eq!(
            request.tool_policy.as_ref().unwrap()["allowed_tools"],
            json!(["image_generation"])
        );
        assert_eq!(
            request.output_contract.as_ref().unwrap()["media_type"],
            "text/markdown+image"
        );
        assert_eq!(
            request.output_contract.as_ref().unwrap()["durable_source"],
            "codex_rollout_jsonl_image_generation_end"
        );
    }

    #[test]
    fn codex_task_endpoint_defaults_to_codex_engine_from_path() {
        let body = json!({
            "prompt": "研究 MissionD provider-box 的测试策略"
        });

        let request = codex_task_interaction_from_body(&body, BoxCommand::Research)
            .expect("research request");

        assert_eq!(request.engine, CliEngine::Codex);
        assert_eq!(request.provider.as_deref(), Some("codex_research"));
        assert_eq!(request.model.as_deref(), Some("gpt-5.5"));
    }

    #[test]
    fn codex_task_body_rejects_external_tool_schema() {
        let body = json!({
            "provider": "codex_research",
            "engine": "codex",
            "prompt": "查资料",
            "tools": [{"type": "function", "function": {"name": "do_anything"}}]
        });

        assert!(codex_task_interaction_from_body(&body, BoxCommand::Research).is_none());
    }

    #[test]
    fn codex_exec_text_only_body_rejects_search_tool_request() {
        let body = json!({
            "engine": "codex",
            "model": "gpt-5.5",
            "messages": [{"role": "user", "content": "hello"}],
            "pure_text": true,
            "search_enabled": true
        });

        assert!(text_only_interaction_from_body(&body).is_none());
    }

    #[test]
    fn codex_sources_export_live_smoke_routeability() {
        let mut body = json!({
            "data": [],
            "provider_text_only_sources": [],
            "provider_task_sources": [],
            "router_model_sources": []
        });

        append_codex_exec_text_exports(&mut body);
        append_codex_exec_router_sources(&mut body);
        append_codex_task_exports(&mut body);
        append_codex_task_router_sources(&mut body);
        append_claude_code_task_exports(&mut body);
        append_claude_code_task_router_sources(&mut body);

        assert!(body["data"]
            .as_array()
            .expect("data")
            .iter()
            .any(|entry| entry["id"] == "codex-gpt-55-xhigh"));
        assert!(body["data"]
            .as_array()
            .expect("data")
            .iter()
            .filter(|entry| entry["provider"] == "codex_exec_text")
            .all(|entry| entry["provider_model_id"] == "gpt-5.5" && entry["model"] == "gpt-5.5"));
        let sources = body["provider_text_only_sources"]
            .as_array()
            .expect("sources");
        assert!(sources
            .iter()
            .any(|entry| entry["provider"] == "codex_exec_text"
                && entry["guard"]["shell_tool_disabled"] == true));
        assert!(sources
            .iter()
            .any(|entry| entry["model_id"] == "codex-gpt-55-default"
                && entry["queue"]["key"] == "codex_exec_text:gpt-5.5:default"
                && entry["queue"]["max_concurrent"] == 4));
        assert!(sources
            .iter()
            .any(|entry| entry["model_id"] == "codex-gpt-55-xhigh"
                && entry["queue"]["key"] == "codex_exec_text:gpt-5.5:xhigh"
                && entry["queue"]["max_concurrent"] == 2));
        let router_sources = body["router_model_sources"]
            .as_array()
            .expect("router sources");
        assert!(router_sources
            .iter()
            .any(|entry| entry["model_id"] == "codex-gpt-55-xhigh"
                && entry["routeable"] == true
                && entry["routeable_status"] == "live_smoke_passed"));
        assert!(router_sources
            .iter()
            .any(|entry| entry["model_id"] == "codex-research-gpt-55-xhigh"
                && entry["routeable"] == true
                && entry["routeable_status"] == "live_smoke_passed"));
        assert!(router_sources.iter().any(|entry| entry["model_id"]
            == "codex-image-generation-gpt-55-default"
            && entry["routeable"] == true
            && entry["routeable_status"] == "live_smoke_passed"));
        assert!(router_sources.iter().any(|entry| entry["model_id"]
            == CLAUDE_CODE_DEEP_RESEARCH_MODEL_ID
            && entry["routeable"] == true
            && entry["route"]["provider"] == CLAUDE_CODE_DEEP_RESEARCH_PROVIDER
            && entry["route"]["completion_endpoint"]
                == "/provider-box/v1/claude-code/deep-research/completions"));
        assert!(body["provider_task_sources"]
            .as_array()
            .expect("task sources")
            .iter()
            .any(|entry| entry["provider"] == "codex_research"
                && entry["capabilities"]["web_search"] == true
                && entry["queue"]["key"] == "codex_research:gpt-5.5:xhigh"
                && entry["queue"]["max_concurrent"] == 1));
        assert!(body["provider_task_sources"]
            .as_array()
            .expect("task sources")
            .iter()
            .any(|entry| entry["provider"] == "codex_image_generation"
                && entry["capabilities"]["image_generation"] == true
                && entry["queue"]["key"] == "codex_image_generation:gpt-5.5:default"
                && entry["queue"]["max_concurrent"] == 1
                && entry["guard"]["ignore_user_config"] == false));
        assert!(body["provider_task_sources"]
            .as_array()
            .expect("task sources")
            .iter()
            .any(
                |entry| entry["provider"] == CLAUDE_CODE_DEEP_RESEARCH_PROVIDER
                    && entry["capabilities"]["workflow"] == true
                    && entry["guard"]["workflow_required"] == "deep-research"
                    && entry["queue"]["key"] == claude_code_deep_research_queue_key()
                    && entry["queue"]["max_concurrent"] == 1
            ));
    }

    #[test]
    fn claude_code_text_sources_export_three_guarded_logical_models() {
        let mut body = json!({
            "data": [],
            "provider_text_only_sources": [],
            "router_model_sources": []
        });

        append_claude_code_text_exports(&mut body);
        append_claude_code_text_router_sources(&mut body);

        let data = body["data"].as_array().expect("data");
        let claude_models = data
            .iter()
            .filter(|entry| entry["provider"] == CLAUDE_CODE_TEXT_PROVIDER)
            .collect::<Vec<_>>();
        assert_eq!(claude_models.len(), 3);
        assert!(claude_models
            .iter()
            .any(|entry| entry["id"] == "claude-code-opus-4-8-xhigh"
                && entry["provider_model_id"] == "claude-opus-4-8"
                && entry["model_profile"] == "xhigh"));
        assert!(claude_models
            .iter()
            .any(|entry| entry["id"] == "claude-code-opus-4-6-high"
                && entry["provider_model_id"] == "claude-opus-4-6"
                && entry["model_profile"] == "high"));
        assert!(claude_models
            .iter()
            .any(|entry| entry["id"] == "claude-code-sonnet-4-6-high"
                && entry["provider_model_id"] == "claude-sonnet-4-6"
                && entry["model_profile"] == "high"));
        assert!(!data
            .iter()
            .any(|entry| entry["id"] == "claude-code-default"));

        let sources = body["provider_text_only_sources"]
            .as_array()
            .expect("sources");
        assert_eq!(sources.len(), 3);
        for source in sources {
            assert_eq!(source["provider"], CLAUDE_CODE_TEXT_PROVIDER);
            assert_eq!(source["engine"], "claude_code");
            assert_eq!(source["capabilities"]["text"], true);
            assert_eq!(source["capabilities"]["tools"], false);
            assert_eq!(source["capabilities"]["files"], false);
            assert_eq!(source["capabilities"]["mcp"], false);
            assert_eq!(source["capabilities"]["shell"], false);
            assert_eq!(source["capabilities"]["vision"], false);
            assert_eq!(source["guard"]["interactive_pty"], true);
            assert_eq!(source["guard"]["uses_print_mode"], false);
            assert_eq!(source["guard"]["per_request_session_id"], true);
            assert_eq!(source["guard"]["mcp_config_empty"], true);
            assert_eq!(source["guard"]["strict_mcp_config"], true);
            assert_eq!(source["guard"]["tools_flag"], "--tools ''");
            assert_eq!(source["guard"]["slash_commands_disabled"], true);
            assert_eq!(source["guard"]["durable_jsonl_guard"], true);
            assert_eq!(
                source["queue"]["max_concurrent"],
                CLAUDE_CODE_TEXT_MAX_CONCURRENT
            );
            assert!(source["queue"]["key"]
                .as_str()
                .expect("queue key")
                .starts_with("claude_code_text:claude-code-"));
            assert!(source["request_template"].get("slot_id").is_none());
            assert_eq!(
                source["slot_policy"]["kind"],
                "provider_box_managed_ephemeral_private_slot"
            );
            assert_eq!(source["slot_policy"]["private_slot_exposed"], false);
        }

        let router_sources = body["router_model_sources"]
            .as_array()
            .expect("router sources");
        assert_eq!(router_sources.len(), 3);
        assert!(router_sources.iter().all(|entry| {
            entry["route"]["provider"] == CLAUDE_CODE_TEXT_PROVIDER
                && entry["text_only_source"]["slot_policy"]["private_slot_exposed"] == false
        }));
    }

    #[test]
    fn static_model_export_fallback_still_exposes_claude_code_sources() {
        let request =
            ProviderInteractionRequest::new(BoxCommand::ModelCatalogExport, CliEngine::Agy);
        let result = static_models_export_result(
            &request,
            ProviderBoxDiagnostic::warning(
                "MODEL_CATALOG_LIVE_EXPORT_TIMEOUT",
                "live export timeout",
                json!({"timeout_secs": 1}),
            ),
        );

        let response = result_response(result);

        assert_eq!(response.status, 200);
        assert_eq!(
            response.body["diagnostics"][0]["code"],
            "MODEL_CATALOG_LIVE_EXPORT_TIMEOUT"
        );
        let claude_sources = response.body["provider_text_only_sources"]
            .as_array()
            .expect("sources")
            .iter()
            .filter(|entry| entry["provider"] == CLAUDE_CODE_TEXT_PROVIDER)
            .collect::<Vec<_>>();
        assert_eq!(claude_sources.len(), 3);
        assert!(response.body["router_model_sources"]
            .as_array()
            .expect("router sources")
            .iter()
            .any(|entry| entry["model_id"] == "claude-code-opus-4-8-xhigh"));
    }

    #[test]
    fn static_model_export_exposes_agy_sources_without_live_pty_discovery() {
        let mut request =
            ProviderInteractionRequest::new(BoxCommand::ModelCatalogExport, CliEngine::Agy);
        request.router_export_policy = Some(json!({
            "provider_box_base_url": "https://missiond.example/provider-box"
        }));
        let result = static_models_export_result(
            &request,
            ProviderBoxDiagnostic::warning(
                "MODEL_CATALOG_STATIC_EXPORT",
                "static export",
                json!({}),
            ),
        );

        let response = result_response(result);

        let agy_sources = response.body["provider_text_only_sources"]
            .as_array()
            .expect("sources")
            .iter()
            .filter(|entry| entry["provider"] == "agy_cli")
            .collect::<Vec<_>>();
        assert_eq!(agy_sources.len(), STATIC_AGY_TEXT_MODELS.len());
        assert!(agy_sources
            .iter()
            .any(|entry| entry["model_id"] == "agy-claude-opus-46-thinking"));
        assert!(!agy_sources
            .iter()
            .any(|entry| entry["model_id"] == "agy-gpt-oss-120b-medium"));

        let agy_routes = response.body["router_model_sources"]
            .as_array()
            .expect("router sources")
            .iter()
            .filter(|entry| {
                entry["model_id"]
                    .as_str()
                    .is_some_and(|model_id| model_id.starts_with("agy-"))
            })
            .collect::<Vec<_>>();
        assert_eq!(agy_routes.len(), STATIC_AGY_TEXT_MODELS.len());
        assert!(agy_routes.iter().all(|entry| entry["routeable"] == true));
        assert!(agy_routes.iter().all(|entry| {
            entry["text_only_source"]["slot_policy"]["replicas_hidden"] == true
                && entry["route"]["primary"]["provider"] == "MissionDAgy"
        }));
    }

    #[test]
    fn models_live_catalog_requires_explicit_opt_in() {
        let default_request = ProviderBoxHttpRequest {
            method: "GET".to_string(),
            path: "/provider-box/v1/models".to_string(),
            headers: HashMap::new(),
            body: json!({}),
        };
        assert!(!models_live_catalog_requested(&default_request));

        let query_request = ProviderBoxHttpRequest {
            path: "/provider-box/v1/models?live_catalog=true".to_string(),
            ..default_request.clone()
        };
        assert!(models_live_catalog_requested(&query_request));

        let body_request = ProviderBoxHttpRequest {
            body: json!({"refresh_live_catalog": true}),
            ..default_request
        };
        assert!(models_live_catalog_requested(&body_request));
    }

    #[test]
    fn failed_provider_box_response_exposes_error_code_and_top_level_diagnostics() {
        let request =
            ProviderInteractionRequest::new(BoxCommand::PureTextSingleTurn, CliEngine::ClaudeCode);
        let mut result = ProviderBoxResult::base(&request, ProviderBoxStatus::Failed);
        result.add_diagnostic(ProviderBoxDiagnostic::error(
            DIAG_PROVIDER_UPSTREAM_UNAVAILABLE,
            "upstream unavailable",
            json!({"status": 529}),
        ));

        let response = result_response(result);

        assert_eq!(response.status, 502);
        assert_eq!(
            response.body["error"]["code"],
            DIAG_PROVIDER_UPSTREAM_UNAVAILABLE
        );
        assert_eq!(
            response.body["error"]["diagnostics"][0]["code"],
            DIAG_PROVIDER_UPSTREAM_UNAVAILABLE
        );
        assert_eq!(
            response.body["diagnostics"][0]["code"],
            DIAG_PROVIDER_UPSTREAM_UNAVAILABLE
        );
    }

    #[test]
    fn text_only_body_can_explicitly_allow_model_switch() {
        let body = json!({
            "engine": "agy",
            "model": "Claude Opus 4.6 (Thinking)",
            "slot_id": "slot-agy-opus-46-a",
            "messages": [{"role": "user", "content": "hello"}],
            "pure_text": true,
            "allow_model_switch": true
        });

        let request = text_only_interaction_from_body(&body).expect("request");
        assert!(
            request
                .model_switch_policy
                .expect("model policy")
                .allow_respawn
        );
    }

    #[test]
    fn text_only_body_can_omit_slot_id_for_logical_model_source() {
        let body = json!({
            "engine": "agy",
            "model": "Claude Opus 4.6 (Thinking)",
            "messages": [{"role": "user", "content": "hello"}],
            "pure_text": true
        });

        let request = text_only_interaction_from_body(&body).expect("request");

        assert!(request.slot_id.is_none());
        assert_eq!(
            request
                .model_switch_policy
                .expect("model policy")
                .target_model,
            Some("Claude Opus 4.6 (Thinking)".to_string())
        );
    }

    #[test]
    fn private_agy_slot_ids_use_two_hidden_replicas_for_each_exported_model() {
        for (model, slug) in [
            ("Gemini 3.5 Flash (Medium)", "gemini-35-flash-medium"),
            ("Gemini 3.5 Flash (High)", "gemini-35-flash-high"),
            ("Gemini 3.5 Flash (Low)", "gemini-35-flash-low"),
            ("Gemini 3.1 Pro (Low)", "gemini-31-pro-low"),
            ("Gemini 3.1 Pro (High)", "gemini-31-pro-high"),
            ("Claude Sonnet 4.6 (Thinking)", "claude-sonnet-46-thinking"),
            ("Claude Opus 4.6 (Thinking)", "claude-opus-46-thinking"),
        ] {
            assert!(is_agy_text_model_exportable(model));
            assert_eq!(
                private_agy_slot_ids_for_model(model),
                vec![format!("slot-agy-{slug}-a"), format!("slot-agy-{slug}-b")]
            );
        }
        assert!(!is_agy_text_model_exportable("GPT-OSS 120B (Medium)"));
    }

    #[tokio::test]
    async fn logical_text_only_rejects_gpt_oss_export() {
        let adapter = ProviderBoxHttpAdapter::new(std::sync::Arc::new(
            ProviderInteractionBox::without_artifacts(),
        ));
        let request = ProviderBoxHttpRequest {
            method: "POST".to_string(),
            path: "/provider-box/v1/text-only/completions".to_string(),
            headers: HashMap::new(),
            body: json!({
                "engine": "agy",
                "model": "GPT-OSS 120B (Medium)",
                "messages": [{"role": "user", "content": "hello"}],
                "pure_text": true
            }),
        };

        let response = adapter
            .handle_text_only_completion(request)
            .await
            .expect("response");

        assert_eq!(response.status, 502);
        assert_eq!(
            response.body["error"]["diagnostics"][0]["code"],
            DIAG_PROVIDER_BOX_INVALID_REQUEST
        );
    }

    #[tokio::test]
    async fn claude_code_text_only_rejects_external_slot_id() {
        let adapter = ProviderBoxHttpAdapter::new(std::sync::Arc::new(
            ProviderInteractionBox::without_artifacts(),
        ));
        let request = ProviderBoxHttpRequest {
            method: "POST".to_string(),
            path: "/provider-box/v1/text-only/completions".to_string(),
            headers: HashMap::new(),
            body: json!({
                "provider": "claude_code_text",
                "engine": "claude_code",
                "model": "claude-code-opus-4-8-xhigh",
                "model_profile": "xhigh",
                "slot_id": "slot-claude-code-text-leaked",
                "messages": [{"role": "user", "content": "hello"}],
                "pure_text": true
            }),
        };

        let response = adapter
            .handle_text_only_completion(request)
            .await
            .expect("response");

        assert_eq!(response.status, 502);
        assert_eq!(
            response.body["error"]["diagnostics"][0]["code"],
            DIAG_PROVIDER_BOX_INVALID_REQUEST
        );
        assert!(response.body["error"]["message"]
            .as_str()
            .expect("message")
            .contains("does not accept external slot_id"));
    }

    #[tokio::test]
    async fn logical_private_agy_slot_pool_round_robins_per_model() {
        let adapter = ProviderBoxHttpAdapter::new(std::sync::Arc::new(
            ProviderInteractionBox::without_artifacts(),
        ));

        assert_eq!(
            adapter
                .next_private_agy_slot_for_model("Gemini 3.5 Flash (High)")
                .await,
            "slot-agy-gemini-35-flash-high-a"
        );
        assert_eq!(
            adapter
                .next_private_agy_slot_for_model("Gemini 3.5 Flash (High)")
                .await,
            "slot-agy-gemini-35-flash-high-b"
        );
        assert_eq!(
            adapter
                .next_private_agy_slot_for_model("Claude Opus 4.6 (Thinking)")
                .await,
            "slot-agy-claude-opus-46-thinking-a"
        );
        assert_eq!(
            adapter
                .next_private_agy_slot_for_model("Gemini 3.5 Flash (High)")
                .await,
            "slot-agy-gemini-35-flash-high-a"
        );
    }

    #[tokio::test]
    async fn logical_private_agy_slot_pool_isolates_request_lanes() {
        let adapter = ProviderBoxHttpAdapter::new(std::sync::Arc::new(
            ProviderInteractionBox::without_artifacts(),
        ));

        assert_eq!(
            adapter
                .next_private_agy_slot_for_model_and_lane(
                    "Claude Opus 4.6 (Thinking)",
                    Some("chat_json_planning"),
                )
                .await,
            "slot-agy-claude-opus-46-thinking-chat-json-planning-a"
        );
        assert_eq!(
            adapter
                .next_private_agy_slot_for_model_and_lane(
                    "Claude Opus 4.6 (Thinking)",
                    Some("chat_json_planning"),
                )
                .await,
            "slot-agy-claude-opus-46-thinking-chat-json-planning-b"
        );
        assert_eq!(
            adapter
                .next_private_agy_slot_for_model_and_lane(
                    "Claude Opus 4.6 (Thinking)",
                    Some("chat_plain"),
                )
                .await,
            "slot-agy-claude-opus-46-thinking-chat-plain-a"
        );
    }

    #[test]
    fn usage_refresh_defaults_to_dedicated_probe_slot() {
        let request = ProviderBoxHttpRequest {
            method: "POST".to_string(),
            path: "/provider-box/v1/usage/refresh".to_string(),
            headers: HashMap::new(),
            body: json!({}),
        };

        assert_eq!(
            usage_refresh_slot_id(&request, CliEngine::Agy),
            AGY_USAGE_PROBE_SLOT
        );
        assert_eq!(
            usage_refresh_slot_id(&request, CliEngine::Codex),
            CODEX_USAGE_PROBE_SLOT
        );
    }

    #[test]
    fn usage_request_engine_can_be_selected_from_query() {
        let request = ProviderBoxHttpRequest {
            method: "GET".to_string(),
            path: "/provider-box/v1/usage?engine=codex".to_string(),
            headers: HashMap::new(),
            body: json!({}),
        };

        assert_eq!(usage_request_engine(&request), CliEngine::Codex);
    }

    #[test]
    fn usage_refresh_keeps_explicit_slot_for_internal_debug() {
        let mut headers = HashMap::new();
        headers.insert("x-slot-id".to_string(), "slot-agy-debug-usage".to_string());
        let request = ProviderBoxHttpRequest {
            method: "POST".to_string(),
            path: "/provider-box/v1/usage/refresh".to_string(),
            headers,
            body: json!({"slot_id": "slot-agy-body-ignored"}),
        };

        assert_eq!(
            usage_refresh_slot_id(&request, CliEngine::Codex),
            "slot-agy-debug-usage"
        );
    }

    #[test]
    fn slot_permission_mode_suffixes_map_to_codex_permission_modes() {
        assert_eq!(
            slot_permission_mode_from_suffix("permissions/default").as_deref(),
            Some("Default")
        );
        assert_eq!(
            slot_permission_mode_from_suffix("permissions/auto-review").as_deref(),
            Some("Auto-review")
        );
        assert_eq!(
            slot_permission_mode_from_suffix("actions/permissions/full-access").as_deref(),
            Some("Full Access")
        );
        assert!(slot_permission_mode_from_suffix("permissions").is_none());
        assert!(slot_permission_mode_from_suffix("permissions/not-a-mode").is_none());
    }

    #[test]
    fn permission_mode_suffixes_are_engine_aware() {
        assert_eq!(
            slot_permission_mode_raw_from_suffix("permissions/auto").as_deref(),
            Some("auto")
        );
        assert_eq!(
            normalize_provider_box_permission_mode_for_engine(CliEngine::Codex, "auto").as_deref(),
            Some("Auto-review")
        );
        assert_eq!(
            normalize_provider_box_permission_mode_for_engine(CliEngine::ClaudeCode, "auto")
                .as_deref(),
            Some("auto")
        );
        assert_eq!(
            normalize_provider_box_permission_mode_for_engine(
                CliEngine::ClaudeCode,
                "accept-edits"
            )
            .as_deref(),
            Some("accept_edits")
        );
        assert_eq!(
            normalize_provider_box_permission_mode_for_engine(CliEngine::ClaudeCode, "plan")
                .as_deref(),
            Some("plan")
        );
        assert!(
            normalize_provider_box_permission_mode_for_engine(CliEngine::Codex, "plan").is_none()
        );
    }

    #[test]
    fn slot_fast_mode_suffixes_map_to_codex_fast_modes() {
        assert_eq!(
            slot_fast_mode_from_suffix("fast/enable").as_deref(),
            Some("enabled")
        );
        assert_eq!(
            slot_fast_mode_from_suffix("fast/disable").as_deref(),
            Some("disabled")
        );
        assert_eq!(
            slot_fast_mode_from_suffix("actions/fast/off").as_deref(),
            Some("disabled")
        );
        assert!(slot_fast_mode_from_suffix("fast").is_none());
        assert!(slot_fast_mode_from_suffix("fast/unknown").is_none());
    }

    #[tokio::test]
    async fn usage_cache_get_is_read_only_empty_before_refresh() {
        let adapter = ProviderBoxHttpAdapter::new(std::sync::Arc::new(
            ProviderInteractionBox::without_artifacts(),
        ));
        let request = ProviderBoxHttpRequest {
            method: "GET".to_string(),
            path: "/provider-box/v1/usage".to_string(),
            headers: HashMap::new(),
            body: json!({}),
        };

        let response = adapter
            .handle_usage_cache(request)
            .await
            .expect("usage cache");

        assert_eq!(response.status, 200);
        assert_eq!(response.body["status"], "unknown");
        assert_eq!(response.body["cached"], false);
        assert_eq!(
            response.body["probe_slot_policy"]["slot_id"],
            AGY_USAGE_PROBE_SLOT
        );
    }

    #[tokio::test]
    async fn usage_cache_get_can_select_codex_probe_policy() {
        let adapter = ProviderBoxHttpAdapter::new(std::sync::Arc::new(
            ProviderInteractionBox::without_artifacts(),
        ));
        let request = ProviderBoxHttpRequest {
            method: "GET".to_string(),
            path: "/provider-box/v1/usage?engine=codex".to_string(),
            headers: HashMap::new(),
            body: json!({}),
        };

        let response = adapter
            .handle_usage_cache(request)
            .await
            .expect("usage cache");

        assert_eq!(response.status, 200);
        assert_eq!(response.body["status"], "unknown");
        assert_eq!(response.body["provider"], "codex_cli");
        assert_eq!(response.body["engine"], "codex");
        assert_eq!(
            response.body["probe_slot_policy"]["slot_id"],
            CODEX_USAGE_PROBE_SLOT
        );
    }

    #[test]
    fn text_only_body_rejects_tool_role() {
        let body = json!({
            "engine": "agy",
            "model": "Gemini 3.5 Flash (High)",
            "slot_id": "slot-agy-gemini-35-flash-high",
            "messages": [{"role": "tool", "content": "bad"}],
            "pure_text": true
        });

        assert!(text_only_interaction_from_body(&body).is_none());
    }

    #[test]
    fn text_only_body_rejects_top_level_tools() {
        let body = json!({
            "engine": "agy",
            "model": "Gemini 3.5 Flash (High)",
            "slot_id": "slot-agy-gemini-35-flash-high",
            "messages": [{"role": "user", "content": "hello"}],
            "pure_text": true,
            "tools": [{"type": "function", "function": {"name": "read_file"}}]
        });

        assert!(text_only_interaction_from_body(&body).is_none());
    }

    #[test]
    fn pty_step_payload_accepts_single_key_action() {
        let body = json!({
            "action": {
                "type": "key",
                "key": "down"
            },
            "expected_change": "selection moves down"
        });

        let payload = pty_step_payload_from_body(&body).expect("pty step payload");

        assert_eq!(payload["action_type"], "key");
        assert_eq!(payload["key"], "down");
        assert_eq!(payload["expected_change"], "selection moves down");
    }

    #[test]
    fn pty_step_payload_accepts_text_without_enter() {
        let body = json!({
            "action": {
                "type": "text",
                "text": "/model"
            }
        });

        let payload = pty_step_payload_from_body(&body).expect("pty step payload");

        assert_eq!(payload["action_type"], "text");
        assert_eq!(payload["text"], "/model");
        assert_eq!(payload["redacted"], true);
    }

    #[test]
    fn pty_step_payload_rejects_text_with_enter() {
        let body = json!({
            "action": {
                "type": "text",
                "text": "/exit\n"
            }
        });

        let err = pty_step_payload_from_body(&body).expect_err("enter must be separate");

        assert_eq!(err.code, DIAG_PROVIDER_BOX_INVALID_REQUEST);
        assert!(err.message.contains("separate API calls"));
    }

    #[test]
    fn pty_step_payload_rejects_unknown_key() {
        let body = json!({
            "action": {
                "type": "key",
                "key": "f13"
            }
        });

        let err = pty_step_payload_from_body(&body).expect_err("unsupported key");

        assert_eq!(err.code, DIAG_PROVIDER_BOX_INVALID_REQUEST);
        assert!(err.details["allowed_keys"].is_array());
    }

    #[test]
    fn model_export_response_exposes_text_only_sources_for_each_agy_model() {
        let request =
            ProviderInteractionRequest::new(BoxCommand::ModelCatalogExport, CliEngine::Agy);
        let mut result = ProviderBoxResult::base(&request, ProviderBoxStatus::Completed);
        result.model_catalog = Some(ProviderModelCatalog {
            schema: "missiond.provider-model-catalog.v1".to_string(),
            catalog_id: "catalog-test".to_string(),
            provider: Some("agy_cli".to_string()),
            engine: CliEngine::Agy,
            account_ref: None,
            discovered_at: "2026-05-31T00:00:00Z".to_string(),
            source: Some("agy:/model".to_string()),
            entries: vec![ProviderModelCatalogEntry {
                provider_model_id: "agy:claude-opus-46-thinking".to_string(),
                display_name: "Claude Opus 4.6 (Thinking)".to_string(),
                family: Some("Claude".to_string()),
                routeable_default: true,
                switch_capability: "interactive_model_picker".to_string(),
                usage_probe_capability: "interactive_usage_screen".to_string(),
                confidence: 0.9,
            }],
            diagnostics: Vec::new(),
        });
        result.router_export = Some(ProviderRouterExport {
            schema: "missiond.provider-router-export.v1".to_string(),
            export_id: "export-test".to_string(),
            catalog_id: Some("catalog-test".to_string()),
            provider: Some("agy_cli".to_string()),
            engine: CliEngine::Agy,
            router_backend_ids: vec!["xjp-router:MissionDAgy".to_string()],
            routeable_entries: vec![json!({
                "model_id": "agy-claude-opus-46-thinking",
                "primary": {
                    "provider": "MissionDAgy"
                }
            })],
            blocked_entries: Vec::new(),
            policy_ref: Some("interactive-provider-box/MissionDAgy/text-only".to_string()),
            diagnostics: Vec::new(),
        });

        let response = result_response(result);
        let data = response.body["data"].as_array().expect("model data");
        let sources = response.body["provider_text_only_sources"]
            .as_array()
            .expect("sources");

        assert_eq!(response.status, 200);
        assert_eq!(data[0]["id"], "agy-claude-opus-46-thinking");
        assert_eq!(
            data[0]["slot_pool_id"],
            "slot-pool-agy-claude-opus-46-thinking"
        );
        assert!(data[0].get("slot_id").is_none());
        assert!(data[0].get("slot_ids").is_none());
        assert_eq!(
            sources[0]["slot_pool_id"],
            "slot-pool-agy-claude-opus-46-thinking"
        );
        assert!(sources[0].get("slot_id").is_none());
        assert!(sources[0].get("slot_ids").is_none());
        assert!(sources[0]["request_template"].get("slot_id").is_none());
        assert_eq!(sources[0]["slot_policy"]["replicas_hidden"], true);
        assert_eq!(sources[0]["slot_policy"]["public_max_concurrent"], 1);
        assert_eq!(sources[0]["request_template"]["allow_model_switch"], false);
        assert_eq!(
            response.body["model_export"]["slot_scoped_apis"],
            "internal_maintenance_only"
        );
        assert_eq!(
            response.body["model_export"]["pure_text_guard"]["durable_jsonl_guard"],
            true
        );
    }

    #[test]
    fn image_generation_response_exposes_media_artifact_and_image_url() {
        let request =
            ProviderInteractionRequest::new(BoxCommand::ImageGeneration, CliEngine::Codex);
        let mut result = ProviderBoxResult::base(&request, ProviderBoxStatus::Completed);
        let image_url = "https://images.xiaojins.com/v1/images/img_test/content?exp=1&sig=abc";
        result.final_text = Some(format!("IMAGE_DONE {image_url}"));
        result.slot_status = Some(json!({
            "kind": "codex_exec_task",
            "task_kind": "image-generation",
            "media_artifact": {
                "artifact_id": "img_test",
                "kind": "image",
                "signed_url": {
                    "url": image_url,
                    "expires_at": "2026-06-02T00:00:00Z"
                }
            }
        }));

        let response = result_response(result);
        let message = &response.body["choices"][0]["message"];

        assert_eq!(response.status, 200);
        assert_eq!(response.body["imageUrl"], image_url);
        assert_eq!(response.body["image_artifact"]["artifact_id"], "img_test");
        assert_eq!(response.body["content_parts"][0]["type"], "imageUrl");
        assert_eq!(response.body["content_parts"][0]["imageUrl"], image_url);
        assert_eq!(message["content_parts"][0]["imageUrl"], image_url);
        assert_eq!(message["image_artifact"]["artifact_id"], "img_test");
    }

    #[test]
    fn logical_text_only_response_redacts_private_slot_details() {
        let request =
            ProviderInteractionRequest::new(BoxCommand::PureTextSingleTurn, CliEngine::Agy);
        let mut result = ProviderBoxResult::base(&request, ProviderBoxStatus::Completed);
        result.slot_id = Some("slot-agy-claude-opus-46-thinking-a".to_string());
        result.slot_status = Some(json!({
            "slot_id": "slot-agy-claude-opus-46-thinking-a",
            "state": "idle"
        }));
        result.add_diagnostic(ProviderBoxDiagnostic::error(
            "TEST",
            "test",
            json!({
                "slot_id": "slot-agy-claude-opus-46-thinking-a",
                "slot_ids": [
                    "slot-agy-claude-opus-46-thinking-a",
                    "slot-agy-claude-opus-46-thinking-b"
                ],
                "nested": {
                    "slot_id": "slot-agy-claude-opus-46-thinking-b",
                    "kept": true
                }
            }),
        ));

        redact_private_slot_details(&mut result, "Claude Opus 4.6 (Thinking)");

        assert!(result.slot_id.is_none());
        assert!(result.slot_status.is_none());
        assert!(result.step_records.is_empty());
        assert!(result.diagnostics[0].details.get("slot_id").is_none());
        assert!(result.diagnostics[0].details.get("slot_ids").is_none());
        assert!(result.diagnostics[0].details["nested"]
            .get("slot_id")
            .is_none());
        assert_eq!(result.diagnostics[0].details["nested"]["kept"], true);
    }

    #[test]
    fn slot_endpoint_parser_extracts_slot_and_action_suffix() {
        let parsed = parse_slot_endpoint(
            "/provider-box/v1/slots/slot-agy-gemini-35-flash-high/actions/input",
        )
        .expect("slot endpoint");

        assert_eq!(parsed.0, "slot-agy-gemini-35-flash-high");
        assert_eq!(parsed.1, "actions/input");
        assert!(parse_slot_endpoint("/provider-box/v1/slots/spawn").is_none());
    }

    #[test]
    fn engine_from_body_defaults_to_agy() {
        assert_eq!(engine_from_body(&json!({})), CliEngine::Agy);
        assert_eq!(
            engine_from_body(&json!({"engine": "codex-cli"})),
            CliEngine::Codex
        );
    }

    #[test]
    fn slot_endpoint_engine_infers_codex_from_slot_id_when_body_is_empty() {
        assert_eq!(
            engine_from_body_or_slot(&json!({}), "slot-codex-code-worker"),
            CliEngine::Codex
        );
        assert_eq!(
            engine_from_body_or_slot(&json!({}), "slot-agy-claude-opus-46-thinking-a"),
            CliEngine::Agy
        );
        assert_eq!(
            engine_from_body_or_slot(&json!({"engine": "agy"}), "slot-codex-code-worker"),
            CliEngine::Agy
        );
    }

    #[test]
    fn slot_endpoint_engine_infers_claude_code_from_slot_id_when_body_is_empty() {
        assert_eq!(
            engine_from_body_or_slot(&json!({}), "slot-claude-code-default"),
            CliEngine::ClaudeCode
        );
        assert_eq!(
            engine_from_body_or_slot(&json!({}), "slot-cc-research"),
            CliEngine::ClaudeCode
        );
    }

    #[test]
    fn claude_code_slot_capabilities_expose_taught_model_and_permission_controls() {
        let caps = provider_slot_capabilities_value(
            CliEngine::ClaudeCode,
            &ProviderDriverCapabilities {
                submit_turn: true,
                switch_model: true,
                usage_probe: false,
                model_catalog: false,
                pure_text_guard: false,
                control_action: true,
                pty_step: false,
                status: true,
                mcp_status: true,
                mcp_reconnect: true,
            },
        );

        assert_eq!(caps["slot_controls"]["spawn"], true);
        assert_eq!(caps["slot_controls"]["restart"]["supported"], true);
        assert_eq!(
            caps["slot_controls"]["restart"]["requires"]["confirm_destroy_context"],
            true
        );
        assert_eq!(
            caps["slot_controls"]["dangerously_skip_permissions"]["supported"],
            true
        );
        assert_eq!(caps["slot_controls"]["status"], true);
        assert_eq!(caps["slot_controls"]["mcp_status"], true);
        assert_eq!(caps["slot_controls"]["mcp_reconnect"], true);
        assert_eq!(caps["slot_controls"]["input"], false);
        assert!(caps["slot_controls"].get("logout").is_none());
        assert_eq!(caps["slot_controls"]["permissions"]["supported"], true);
        assert_eq!(caps["slot_controls"]["permissions"]["cycle"][0], "auto");
        assert_eq!(
            caps["slot_controls"]["permissions"]["verification"],
            "screen_identity.permission_mode"
        );
        assert_eq!(caps["slot_controls"]["switch_model"]["supported"], true);
        assert_eq!(
            caps["slot_controls"]["switch_model"]["allowed_model_ids"][0],
            "claude-opus-4-8"
        );
    }

    #[test]
    fn codex_slot_capabilities_are_explicit_about_restart_and_mcp_reconnect() {
        let caps = provider_slot_capabilities_value(
            CliEngine::Codex,
            &ProviderDriverCapabilities {
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
            },
        );

        assert_eq!(caps["slot_controls"]["exit"]["supported"], true);
        assert_eq!(caps["slot_controls"]["exit"]["command"], "/exit");
        assert_eq!(
            caps["slot_controls"]["restart"]["requires"]["confirm_destroy_context"],
            true
        );
        assert_eq!(caps["slot_controls"]["mcp_reconnect"]["supported"], false);
        assert_eq!(
            caps["slot_controls"]["mcp_reconnect"]["restart_required"],
            true
        );
        assert_eq!(caps["driver"]["mcp_reconnect"], false);
    }

    #[tokio::test]
    async fn slot_restart_requires_explicit_context_destroy_confirmation() {
        let adapter = ProviderBoxHttpAdapter::new(std::sync::Arc::new(
            ProviderInteractionBox::without_artifacts(),
        ));
        let request = ProviderBoxHttpRequest {
            method: "POST".to_string(),
            path: "/provider-box/v1/slots/slot-codex-code-worker/restart".to_string(),
            headers: HashMap::new(),
            body: json!({
                "engine": "codex"
            }),
        };

        let response = adapter
            .handle_slot_restart(request, "slot-codex-code-worker".to_string())
            .await
            .expect("restart response");

        assert_eq!(response.status, 409);
        assert_eq!(
            response.body["error"]["diagnostics"][0]["code"],
            DIAG_PROVIDER_BOX_INVALID_REQUEST
        );
        assert_eq!(
            response.body["capabilities"]["slot_controls"]["restart"]["requires"]
                ["confirm_destroy_context"],
            true
        );
    }
}
