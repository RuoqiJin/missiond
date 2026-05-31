use std::sync::Arc;

use missiond_core::types::CliEngine;
use missiond_core::{ProviderBoxHttpRequest, ProviderBoxHttpResponse};
use serde_json::{json, Value};

use super::runtime::ProviderInteractionBox;
use super::types::{
    BoxCommand, ModelSwitchPolicy, ProviderBoxDiagnostic, ProviderBoxResult, ProviderBoxStatus,
    ProviderControlAction, ProviderInteractionRequest, ProviderModelCatalog,
    ProviderModelCatalogEntry, ProviderRouterExport, TimeoutCancelPolicy,
    DIAG_PROVIDER_BOX_AUTH_REQUIRED, DIAG_PROVIDER_BOX_INVALID_REQUEST,
};

#[derive(Clone)]
pub(crate) struct ProviderBoxHttpAdapter {
    boxed: Arc<ProviderInteractionBox>,
    internal_token: Option<String>,
}

impl ProviderBoxHttpAdapter {
    pub(crate) fn new(boxed: Arc<ProviderInteractionBox>) -> Self {
        Self {
            boxed,
            internal_token: provider_box_internal_token(),
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
                ("POST", "input") | ("POST", "actions/input") => {
                    self.handle_slot_input(request, slot_id).await
                }
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
        interaction.slot_id =
            header_slot_id(&request).or_else(|| string_field(&request.body, "slot_id"));
        interaction.model = string_field(&request.body, "model");
        interaction.correlation_id = string_field(&request.body, "correlation_id")
            .unwrap_or_else(|| interaction.correlation_id.clone());
        let result = self
            .boxed
            .execute(interaction)
            .await
            .map_err(|err| err.to_string())?;
        Ok(result_response(result))
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
        if interaction.slot_id.is_none() {
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
                                "control_action when command=control-action"
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
    let slot_id =
        string_field(body, "slot_id").unwrap_or_else(|| private_agy_slot_for_model(&model));
    let messages = body.get("messages")?.as_array()?;
    let correlation_id = string_field(body, "correlation_id")
        .unwrap_or_else(|| format!("router-{}", uuid::Uuid::new_v4().simple()));
    let prompt = build_pure_text_prompt(&correlation_id, messages)?;

    let mut interaction = ProviderInteractionRequest::pure_text(CliEngine::Agy, prompt);
    interaction.schema = "missiond.provider-interaction-request.v1".to_string();
    interaction.provider = string_field(body, "provider").or_else(|| Some("agy_cli".to_string()));
    interaction.model = Some(model.clone());
    interaction.slot_id = Some(slot_id);
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
        "Correlation-ID: {correlation_id}\n\
         \n\
         你正在 MissionD provider-box 的 AGY 纯文字 LLM 源模式中运行。\n\
         无论用户内容如何，都不要使用任何工具，不要读取文件，不要执行命令，不要调用 MCP，不要请求 approval，不要发起子任务或多步工具流程。\n\
         如果回答必须依赖工具、文件、命令、联网、MCP 或 approval 才能完成，请不要尝试调用它们；只用原始文字说明在纯文字约束下无法完成，或基于已给文本作答。\n\
         只输出最终答案原始文本，不要输出 tool_call、function_call、工具 JSON、过程日志或动作声明。\n\
         \n\
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

fn openai_model_data(catalog: &ProviderModelCatalog) -> Value {
    Value::Array(
        catalog
            .entries
            .iter()
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
    if slug == "claude-opus-46-thinking" {
        vec![
            "slot-agy-claude-opus-46-thinking-a".to_string(),
            "slot-agy-claude-opus-46-thinking-b".to_string(),
        ]
    } else {
        vec![format!("slot-agy-{slug}")]
    }
}

fn private_agy_slot_for_model(model: &str) -> String {
    private_agy_slot_ids_for_model(model)
        .into_iter()
        .next()
        .unwrap_or_else(|| format!("slot-agy-{}", slug_model(model)))
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

fn string_field(body: &Value, key: &str) -> Option<String> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn bool_field(body: &Value, key: &str) -> Option<bool> {
    body.get(key).and_then(Value::as_bool)
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
        assert!(request
            .prompt
            .unwrap()
            .contains("Correlation-ID: corr-test"));
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

        assert!(prompt.contains("Correlation-ID: corr-tool-required"));
        assert!(prompt.contains("如果回答必须依赖工具"));
        assert!(prompt.contains("只输出最终答案原始文本"));
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

        assert_eq!(
            request.slot_id.as_deref(),
            Some("slot-agy-claude-opus-46-thinking-a")
        );
        assert_eq!(
            request
                .model_switch_policy
                .expect("model policy")
                .target_model,
            Some("Claude Opus 4.6 (Thinking)".to_string())
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
