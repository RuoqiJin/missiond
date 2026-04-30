use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use super::path::resolve_manifest_path;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CascadePlanArgs {
    manifest_path: Option<String>,
    service: String,
    #[serde(default)]
    changed: Vec<String>,
}

pub(super) async fn handle_cascade_plan(args: Value) -> Result<ToolResult> {
    let args: CascadePlanArgs = serde_json::from_value(args)?;
    let manifest_path = match resolve_manifest_path(args.manifest_path.as_deref()) {
        Ok(p) => p,
        Err(e) => return Ok(ToolResult::error(e)),
    };
    let manifest_dir = manifest_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    let graph = forge_core::universe_graph::resolve_universe_graph(&manifest_path)
        .map_err(|errs| anyhow!("universe graph errors:\n{}", errs.join("\n")))?;

    let delta = forge_core::universe_graph::ServiceDelta {
        service_id: args.service.clone(),
        changed_interfaces: args.changed.clone(),
    };

    let config = forge_core::cascade::CascadeConfig {
        dry_run: true,
        ..Default::default()
    };

    let plan = forge_core::cascade::create_plan(&graph, &delta, manifest_dir, config);

    let phases: Vec<serde_json::Value> = plan
        .phases
        .iter()
        .map(|p| {
            serde_json::json!({
                "service_id": p.service_id,
                "path": p.service_path.to_string_lossy(),
                "depth": p.depth,
                "status": format!("{:?}", p.status),
            })
        })
        .collect();

    Ok(ToolResult::json_pretty(&serde_json::json!({
        "trigger_service": plan.trigger_service,
        "trigger_changes": plan.trigger_changes,
        "phase_count": plan.phases.len(),
        "phases": phases,
        "upstream_map": plan.upstream_map,
        "dry_run": true,
    })))
}
