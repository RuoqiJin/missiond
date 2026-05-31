use std::collections::HashMap;
use std::sync::Arc;

use missiond_core::types::CliEngine;
use missiond_core::{ProviderBoxHttpRequest, ProviderBoxHttpResponse};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use super::runtime::ProviderInteractionBox;
use super::types::{
    BoxCommand, ModelSwitchPolicy, ProviderBoxDiagnostic, ProviderBoxResult, ProviderBoxStatus,
    ProviderControlAction, ProviderInteractionRequest, ProviderModelCatalog,
    ProviderModelCatalogEntry, ProviderRouterExport, TimeoutCancelPolicy,
    DIAG_PROVIDER_BOX_AUTH_REQUIRED, DIAG_PROVIDER_BOX_INVALID_REQUEST,
};

const PTY_STEP_TEXT_LIMIT: usize = 4096;
const AGY_USAGE_PROBE_SLOT: &str = "slot-agy-usage-probe";
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
}

impl ProviderBoxHttpAdapter {
    pub(crate) fn new(boxed: Arc<ProviderInteractionBox>) -> Self {
        Self {
            boxed,
            internal_token: provider_box_internal_token(),
            agy_slot_pool_cursors: Arc::new(Mutex::new(HashMap::new())),
            agy_usage_cache: Arc::new(Mutex::new(None)),
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
                ("POST", "pty-step")
                | ("POST", "actions/pty-step")
                | ("POST", "key")
                | ("POST", "actions/key") => self.handle_slot_pty_step(request, slot_id).await,
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
                ("POST", "switch-model") | ("POST", "actions/switch-model") => {
                    self.handle_slot_switch_model(request, slot_id).await
                }
                ("POST", "usage") | ("POST", "usage/refresh") | ("POST", "actions/usage") => {
                    self.handle_slot_usage_refresh(request, slot_id).await
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
            ("GET", "/provider-box/v1/usage") => self.handle_usage_cache().await,
            ("POST", "/provider-box/v1/usage/refresh") => self.handle_usage_refresh(request).await,
            ("POST", "/provider-box/v1/turns") => self.handle_turn(request).await,
            ("POST", "/provider-box/v1/text-only/completions") => {
                self.handle_text_only_completion(request).await
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
        let mut interaction =
            ProviderInteractionRequest::new(BoxCommand::Status, engine_from_body(&request.body));
        interaction.provider = string_field(&request.body, "provider").or_else(|| {
            if interaction.engine == CliEngine::Agy {
                Some("agy_cli".to_string())
            } else {
                None
            }
        });
        interaction.slot_id = Some(slot_id);
        interaction.model = string_field(&request.body, "model");
        interaction.cwd = string_field(&request.body, "cwd");
        interaction.project_root = string_field(&request.body, "project_root");
        interaction.correlation_id = string_field(&request.body, "correlation_id")
            .unwrap_or_else(|| interaction.correlation_id.clone());
        let body_spawn = bool_field(&request.body, "spawn_if_missing")
            .or_else(|| bool_field(&request.body, "spawn"))
            .unwrap_or(spawn_if_missing);
        interaction.desired_worker = Some(json!({
            "spawn_if_missing": body_spawn,
        }));
        let result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        Ok(result_response(result))
    }

    async fn handle_slot_input(
        &self,
        request: ProviderBoxHttpRequest,
        slot_id: String,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let mut interaction = ProviderInteractionRequest::new(
            BoxCommand::ControlAction,
            engine_from_body(&request.body),
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
        let mut interaction =
            ProviderInteractionRequest::new(BoxCommand::PtyStep, engine_from_body(&request.body));
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
            engine_from_body(&request.body),
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

    async fn handle_slot_switch_model(
        &self,
        request: ProviderBoxHttpRequest,
        slot_id: String,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let mut interaction = ProviderInteractionRequest::new(
            BoxCommand::ModelSwitch,
            engine_from_body(&request.body),
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
        interaction.cwd = string_field(&request.body, "cwd");
        interaction.project_root = string_field(&request.body, "project_root");
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
        let result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        Ok(result_response(result))
    }

    async fn handle_usage_refresh(
        &self,
        request: ProviderBoxHttpRequest,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let mut interaction =
            ProviderInteractionRequest::new(BoxCommand::UsageProbe, CliEngine::Agy);
        interaction.provider = Some("agy_cli".to_string());
        interaction.slot_id = Some(usage_refresh_slot_id(&request));
        interaction.model = string_field(&request.body, "model");
        interaction.correlation_id = string_field(&request.body, "correlation_id")
            .unwrap_or_else(|| interaction.correlation_id.clone());
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
            *self.agy_usage_cache.lock().await = Some(snapshot);
        }
        Ok(result_response(result))
    }

    async fn handle_usage_cache(&self) -> Result<ProviderBoxHttpResponse, String> {
        let cached = self.agy_usage_cache.lock().await.clone();
        let cached_hit = cached.is_some();
        Ok(json_response(
            200,
            json!({
                "schema": "missiond.provider-box.usage-cache.v1",
                "status": if cached_hit { "completed" } else { "unknown" },
                "cached": cached_hit,
                "provider": "agy_cli",
                "engine": "agy",
                "usage_snapshot": cached,
                "refresh_endpoint": "/provider-box/v1/usage/refresh",
                "probe_slot_policy": {
                    "slot_id": AGY_USAGE_PROBE_SLOT,
                    "owned_by": "provider-box",
                    "interferes_with_text_only_slots": false
                },
                "message": if cached_hit {
                    "Returning the latest cached AGY usage snapshot."
                } else {
                    "No cached AGY usage snapshot is available yet; call POST /provider-box/v1/usage/refresh."
                }
            }),
        ))
    }

    async fn handle_text_only_completion(
        &self,
        request: ProviderBoxHttpRequest,
    ) -> Result<ProviderBoxHttpResponse, String> {
        let logical_model_request =
            header_slot_id(&request).is_none() && string_field(&request.body, "slot_id").is_none();
        let requested_model = string_field(&request.body, "model");
        let Some(mut interaction) = text_only_interaction_from_body(&request.body) else {
            let mut result = ProviderBoxResult::base(
                &ProviderInteractionRequest::new(BoxCommand::PureTextSingleTurn, CliEngine::Agy),
                ProviderBoxStatus::Failed,
            );
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Invalid provider-box text-only completion request",
                json!({
                    "schema": request.body.get("schema"),
                    "required": [
                        "pure_text=true",
                        "engine=agy",
                        "model",
                        "messages[]"
                    ]
                }),
            ));
            return Ok(result_response(result));
        };
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
                interaction.slot_id = Some(self.next_private_agy_slot_for_model(model).await);
            }
        } else if interaction.slot_id.is_none() {
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
        }
        Ok(result_response(result))
    }

    async fn next_private_agy_slot_for_model(&self, model: &str) -> String {
        let slot_ids = private_agy_slot_ids_for_model(model);
        if slot_ids.len() <= 1 {
            return slot_ids
                .into_iter()
                .next()
                .unwrap_or_else(|| format!("slot-agy-{}", slug_model(model)));
        }

        let pool_id = agy_slot_pool_id(model);
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
    if body.get("engine").and_then(Value::as_str)? != "agy" {
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
    let prompt = build_pure_text_prompt(&correlation_id, messages)?;

    let mut interaction = ProviderInteractionRequest::pure_text(CliEngine::Agy, prompt);
    interaction.schema = "missiond.provider-interaction-request.v1".to_string();
    interaction.provider = string_field(body, "provider").or_else(|| Some("agy_cli".to_string()));
    interaction.model = Some(model.clone());
    interaction.slot_id = slot_id;
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
    interaction.model_switch_policy = Some(ModelSwitchPolicy {
        target_model: Some(model),
        target_model_profile: string_field(body, "target_model_profile")
            .or_else(|| string_field(body, "model_profile")),
        allow_respawn: bool_field(body, "allow_model_switch")
            .or_else(|| bool_field(body, "allow_respawn"))
            .unwrap_or(false),
        require_verification: bool_field(body, "require_verification").unwrap_or(true),
    });
    Some(interaction)
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

fn engine_from_body(body: &Value) -> CliEngine {
    match body
        .get("engine")
        .and_then(Value::as_str)
        .unwrap_or("agy")
        .to_ascii_lowercase()
        .as_str()
    {
        "claude-code" | "claude_code" | "claudecode" | "claude" => CliEngine::ClaudeCode,
        "codex" | "codex_cli" | "codex-cli" => CliEngine::Codex,
        "gemini" | "gemini_cli" | "gemini-cli" => CliEngine::Gemini,
        _ => CliEngine::Agy,
    }
}

fn build_pure_text_prompt(correlation_id: &str, messages: &[Value]) -> Option<String> {
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
            rendered.push(format!("{role}: {}", text.trim()));
        }
    }
    if rendered.is_empty() {
        return None;
    }
    Some(format!(
        "请求编号：{correlation_id}\n\
         \n\
         请直接回答下面消息。不要使用工具、不要读取或修改文件、不要执行命令；如果答案需要这些能力，只用一句话说明缺少什么输入。不要复述本段要求。\n\
         \n\
         消息：\n\
         {}",
        rendered.join("\n\n")
    ))
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
            "durable_source": result.durable_source,
            "final_text": final_text,
            "slot_status": result.slot_status,
            "screen": screen,
            "usage_snapshot": result.usage_snapshot,
            "model_catalog": result.model_catalog,
            "router_export": result.router_export,
            "model_switch_result": result.model_switch_result,
            "diagnostics": result.diagnostics,
            "step_records": result.step_records,
            "artifact_hash": result.artifact_hash
        });
        if result.final_text.is_some() {
            body["choices"] = json!([{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": final_text
                },
                "finish_reason": "stop"
            }]);
        }
        if let Some(catalog) = result.model_catalog.as_ref() {
            body["object"] = json!("list");
            body["data"] = openai_model_data(catalog);
            body["provider_text_only_sources"] = provider_text_only_sources(catalog);
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
                "pure_text_guard": {
                    "prompt_instruction": true,
                    "durable_jsonl_guard": true,
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
        }
        body
    } else {
        json!({
            "schema": result.schema,
            "status": result.status,
            "turn_id": result.turn_id,
            "correlation_id": result.correlation_id,
            "error": {
                "message": result
                    .diagnostics
                    .first()
                    .map(|diag| diag.message.clone())
                    .unwrap_or_else(|| "Provider-box request failed".to_string()),
                "diagnostics": result.diagnostics
            },
            "slot_status": result.slot_status,
            "screen": screen,
            "step_records": result.step_records,
            "usage_snapshot": result.usage_snapshot,
            "model_catalog": result.model_catalog,
            "router_export": result.router_export,
            "model_switch_result": result.model_switch_result,
            "artifact_hash": result.artifact_hash
        })
    };
    json_response(status, body)
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
            "prompt_instruction": true,
            "durable_jsonl_guard": true,
            "rejects_tool_messages": true,
            "rejects_tool_request_fields": true
        }
    })
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
    let slug = slug_model(model);
    vec![format!("slot-agy-{slug}-a"), format!("slot-agy-{slug}-b")]
}

fn is_agy_text_model_exportable(model: &str) -> bool {
    !slug_model(model).starts_with("gpt-oss-")
}

fn agy_slot_pool_id(model: &str) -> String {
    format!("slot-pool-agy-{}", slug_model(model))
}

fn redact_private_slot_details(result: &mut ProviderBoxResult, _model: &str) {
    result.slot_id = None;
    result.slot_status = None;
    result.step_records.clear();
    for diagnostic in &mut result.diagnostics {
        diagnostic.details = redact_slot_ids_in_value(diagnostic.details.clone());
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

fn usage_refresh_slot_id(request: &ProviderBoxHttpRequest) -> String {
    header_slot_id(request)
        .or_else(|| string_field(&request.body, "slot_id"))
        .unwrap_or_else(|| AGY_USAGE_PROBE_SLOT.to_string())
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
    use super::*;

    #[test]
    fn text_only_body_builds_correlated_prompt() {
        let body = json!({
            "schema": "missiond.provider-box.text-only-completion-request.v1",
            "provider": "agy_cli",
            "engine": "agy",
            "model": "Gemini 3.5 Flash (High)",
            "slot_id": "slot-agy-gemini-35-flash-high",
            "correlation_id": "corr-test",
            "messages": [{"role": "user", "content": "hello"}],
            "pure_text": true
        });

        let request = text_only_interaction_from_body(&body).expect("request");

        assert_eq!(request.model.as_deref(), Some("Gemini 3.5 Flash (High)"));
        assert_eq!(
            request.slot_id.as_deref(),
            Some("slot-agy-gemini-35-flash-high")
        );
        assert!(request.prompt.unwrap().contains("请求编号：corr-test"));
        assert!(request.no_tools);
        let policy = request.model_switch_policy.expect("model policy");
        assert!(!policy.allow_respawn);
        assert!(policy.require_verification);
    }

    #[test]
    fn text_only_prompt_declines_tool_required_work_as_text() {
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

        assert!(prompt.contains("请求编号：corr-tool-required"));
        assert!(prompt.contains("请直接回答下面消息"));
        assert!(prompt.contains("不要使用工具、不要读取或修改文件、不要执行命令"));
        assert!(prompt.contains("只用一句话说明缺少什么输入"));
        assert!(prompt.contains("不要复述本段要求"));
        assert!(prompt.contains("消息："));
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

    #[test]
    fn usage_refresh_defaults_to_dedicated_probe_slot() {
        let request = ProviderBoxHttpRequest {
            method: "POST".to_string(),
            path: "/provider-box/v1/usage/refresh".to_string(),
            headers: HashMap::new(),
            body: json!({}),
        };

        assert_eq!(usage_refresh_slot_id(&request), AGY_USAGE_PROBE_SLOT);
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

        assert_eq!(usage_refresh_slot_id(&request), "slot-agy-debug-usage");
    }

    #[tokio::test]
    async fn usage_cache_get_is_read_only_empty_before_refresh() {
        let adapter = ProviderBoxHttpAdapter::new(std::sync::Arc::new(
            ProviderInteractionBox::without_artifacts(),
        ));

        let response = adapter.handle_usage_cache().await.expect("usage cache");

        assert_eq!(response.status, 200);
        assert_eq!(response.body["status"], "unknown");
        assert_eq!(response.body["cached"], false);
        assert_eq!(
            response.body["probe_slot_policy"]["slot_id"],
            AGY_USAGE_PROBE_SLOT
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
}
