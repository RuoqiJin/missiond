use std::sync::Arc;

use missiond_core::types::CliEngine;
use missiond_core::{ProviderBoxHttpRequest, ProviderBoxHttpResponse};
use serde_json::{json, Value};

use super::runtime::ProviderInteractionBox;
use super::types::{
    BoxCommand, ModelSwitchPolicy, ProviderBoxDiagnostic, ProviderBoxResult, ProviderBoxStatus,
    ProviderControlAction, ProviderInteractionRequest, TimeoutCancelPolicy,
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
                        "slot_id",
                        "messages[]"
                    ]
                }),
            ));
            return Ok(result_response(result));
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
    let model = string_field(body, "model")?;
    let slot_id = string_field(body, "slot_id")?;
    let messages = body.get("messages")?.as_array()?;
    let correlation_id = string_field(body, "correlation_id")
        .unwrap_or_else(|| format!("router-{}", uuid::Uuid::new_v4().simple()));
    let prompt = build_pure_text_prompt(&correlation_id, messages)?;

    let mut interaction = ProviderInteractionRequest::pure_text(CliEngine::Agy, prompt);
    interaction.schema = "missiond.provider-interaction-request.v1".to_string();
    interaction.provider = string_field(body, "provider").or_else(|| Some("agy_cli".to_string()));
    interaction.model = Some(model);
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
         请严格按纯文字 single-turn 回答。不要使用任何工具，不要读取文件，不要执行命令，不要调用 MCP，不要请求 approval。\n\
         只输出最终答案文本。\n\
         \n\
         {}",
        rendered.join("\n\n")
    ))
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
