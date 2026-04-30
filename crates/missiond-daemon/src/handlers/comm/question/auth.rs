use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};
use tracing::info;

use crate::state::AppState;

/// V3 incident-governance implementation for the public `mission_gemini_auth` tool.
pub(in crate::handlers::comm::question) async fn handle(
    _state: &AppState,
    args: Value,
) -> Result<ToolResult> {
    let mode = args
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("status");
    let llm_yaml_path = missiond_core::default_mission_home().join("llm.yaml");

    let llm_content = tokio::fs::read_to_string(&llm_yaml_path)
        .await
        .map_err(|e| anyhow!("Failed to read llm.yaml: {}", e))?;
    let llm_config: serde_yaml::Value = serde_yaml::from_str(&llm_content)
        .map_err(|e| anyhow!("Failed to parse llm.yaml: {}", e))?;

    let current_mode = llm_config
        .get("gemini_auth_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("apikey");

    if mode == "status" {
        let key_preview = if current_mode == "apikey" {
            llm_config
                .get("gemini_api_key")
                .and_then(|k| k.as_str())
                .map(|k| {
                    if k.len() <= 12 {
                        "***".to_string()
                    } else {
                        format!("{}...{}", &k[..6], &k[k.len() - 4..])
                    }
                })
        } else {
            None
        };
        return Ok(ToolResult::json(&json!({
            "mode": current_mode,
            "key_preview": key_preview,
        })));
    }

    if mode != "apikey" && mode != "google" {
        return Ok(ToolResult::error(format!(
            "Unknown mode: {}. Use: apikey, google, status",
            mode
        )));
    }

    if mode == current_mode {
        return Ok(ToolResult::json(&json!({
            "status": "no_change",
            "mode": current_mode,
            "message": format!("Already in {} mode", current_mode),
        })));
    }

    let new_content = if llm_content.contains("gemini_auth_mode:") {
        llm_content.replace(
            &format!("gemini_auth_mode: {}", current_mode),
            &format!("gemini_auth_mode: {}", mode),
        )
    } else {
        llm_content.replace(
            "provider: gemini-cli",
            &format!("provider: gemini-cli\ngemini_auth_mode: {}", mode),
        )
    };
    tokio::fs::write(&llm_yaml_path, &new_content)
        .await
        .map_err(|e| anyhow!("Failed to write llm.yaml: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&llm_yaml_path, std::fs::Permissions::from_mode(0o600));
    }

    let selected_type = if mode == "apikey" {
        "gemini-api-key"
    } else {
        "oauth-personal"
    };
    let settings_path = dirs::home_dir().map(|h| h.join(".gemini/settings.json"));
    if let Some(ref path) = settings_path {
        if let Ok(content) = tokio::fs::read_to_string(path).await {
            if let Ok(mut settings) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(auth) = settings.pointer_mut("/security/auth") {
                    auth.as_object_mut()
                        .map(|m| m.insert("selectedType".to_string(), json!(selected_type)));
                }
                if let Ok(json) = serde_json::to_string_pretty(&settings) {
                    let _ = tokio::fs::write(path, json).await;
                }
            }
        }
    }

    info!(from = current_mode, to = mode, "Gemini auth mode switched");
    Ok(ToolResult::json(&json!({
        "status": "switched",
        "from": current_mode,
        "to": mode,
    })))
}
