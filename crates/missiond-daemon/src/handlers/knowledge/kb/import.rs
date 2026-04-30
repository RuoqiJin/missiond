use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::helpers::default_mission_home;
use crate::state::AppState;

use super::args::KBImportArgs;

pub(super) async fn handle_kb_import(state: &AppState, args: Value) -> Result<ToolResult> {
    let KBImportArgs { format, path } = serde_json::from_value(args)?;
    match format.as_str() {
        "servers_yaml" => {
            let yaml_path = path
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| default_mission_home().join("servers.yaml"));
            let infra = missiond_core::InfraConfig::load(&yaml_path);
            let mut imported = 0;
            for server in &infra.servers {
                let detail = serde_json::to_value(server).ok();
                let summary = format!(
                    "{} ({}) — {}",
                    server.name,
                    server.provider,
                    server.roles.join(", ")
                );
                let input = missiond_core::types::KBRememberInput {
                    category: "infra".to_string(),
                    key: server.id.clone(),
                    summary,
                    detail,
                    source: Some("import".to_string()),
                    confidence: Some(1.0),
                    project_id: None,
                };
                state
                    .store
                    .kb_remember(&input)
                    .await
                    .map_err(|e| anyhow!("DB error: {}", e))?;
                imported += 1;
            }
            Ok(ToolResult::json(&serde_json::json!({
                "imported": imported,
                "source": yaml_path.display().to_string(),
            })))
        }
        _ => Ok(ToolResult::error(format!(
            "Unsupported import format: {}",
            format
        ))),
    }
}
