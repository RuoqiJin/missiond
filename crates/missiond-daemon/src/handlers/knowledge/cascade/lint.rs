use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use super::path::resolve_manifest_path;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CascadeLintArgs {
    manifest_path: Option<String>,
}

pub(super) async fn handle_cascade_lint(args: Value) -> Result<ToolResult> {
    let args: CascadeLintArgs = serde_json::from_value(args)?;
    let manifest_path = match resolve_manifest_path(args.manifest_path.as_deref()) {
        Ok(p) => p,
        Err(e) => return Ok(ToolResult::error(e)),
    };
    let manifest_dir = manifest_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    let graph = forge_core::universe_graph::resolve_universe_graph(&manifest_path)
        .map_err(|errs| anyhow!("universe graph errors:\n{}", errs.join("\n")))?;

    let violations = forge_core::universe_graph::validate_universe_integrity(&graph, manifest_dir);

    if violations.is_empty() {
        Ok(ToolResult::json_pretty(&serde_json::json!({
            "status": "clean",
            "service_count": graph.services.len(),
            "violations": [],
        })))
    } else {
        let vs: Vec<serde_json::Value> = violations
            .iter()
            .map(|v| {
                serde_json::json!({
                    "service_id": v.service_id,
                    "severity": v.severity,
                    "message": v.message,
                })
            })
            .collect();

        let errors = violations.iter().filter(|v| v.severity == "error").count();
        let warnings = violations
            .iter()
            .filter(|v| v.severity == "warning")
            .count();

        Ok(ToolResult::json_pretty(&serde_json::json!({
            "status": if errors > 0 { "failed" } else { "warnings" },
            "service_count": graph.services.len(),
            "error_count": errors,
            "warning_count": warnings,
            "violations": vs,
        })))
    }
}
