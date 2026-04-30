use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use super::path::resolve_manifest_path;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UniverseGraphArgs {
    manifest_path: Option<String>,
    #[serde(default = "default_format")]
    format: String,
}

fn default_format() -> String {
    "json".to_string()
}

pub(super) async fn handle_universe_graph(args: Value) -> Result<ToolResult> {
    let args: UniverseGraphArgs = serde_json::from_value(args)?;
    let manifest_path = match resolve_manifest_path(args.manifest_path.as_deref()) {
        Ok(p) => p,
        Err(e) => return Ok(ToolResult::error(e)),
    };

    let graph = forge_core::universe_graph::resolve_universe_graph(&manifest_path)
        .map_err(|errs| anyhow!("universe graph errors:\n{}", errs.join("\n")))?;

    if args.format == "text" {
        let mut out = String::new();
        out.push_str(&format!("Universe: {} services\n\n", graph.services.len()));
        for svc in &graph.services {
            out.push_str(&format!("  [{}] path={}\n", svc.id, svc.path.display()));
        }
        out.push_str("\nDependencies:\n");
        if graph.dependencies.is_empty() {
            out.push_str("  (none)\n");
        }
        for (consumer, deps) in &graph.dependencies {
            for dep in deps {
                out.push_str(&format!(
                    "  {} --> {} (consumes: [{}], breaks-if: [{}])\n",
                    consumer,
                    dep.provider_id,
                    dep.consumes.join(", "),
                    dep.breaks_if.join(", "),
                ));
            }
        }
        Ok(ToolResult::text(out))
    } else {
        let services: Vec<serde_json::Value> = graph
            .services
            .iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id,
                    "path": s.path.to_string_lossy(),
                })
            })
            .collect();

        let dependencies: Vec<serde_json::Value> = graph
            .dependencies
            .iter()
            .flat_map(|(consumer, deps)| {
                deps.iter().map(move |dep| {
                    serde_json::json!({
                        "consumer": consumer,
                        "provider": dep.provider_id,
                        "consumes": dep.consumes,
                        "breaks_if": dep.breaks_if,
                    })
                })
            })
            .collect();

        Ok(ToolResult::json_pretty(&serde_json::json!({
            "service_count": graph.services.len(),
            "services": services,
            "dependency_count": dependencies.len(),
            "dependencies": dependencies,
        })))
    }
}
