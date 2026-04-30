use anyhow::{anyhow, Result};
use missiond_core::event::events::TaskEvent;
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppState;

use super::path::resolve_manifest_path;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CascadeTriggerArgs {
    manifest_path: Option<String>,
    service: String,
    #[serde(default)]
    changed: Vec<String>,
    #[serde(default = "default_max_cycles")]
    max_cycles: usize,
}

fn default_max_cycles() -> usize {
    3
}

pub(super) async fn handle_cascade_trigger(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: CascadeTriggerArgs = serde_json::from_value(args)?;

    let cascade_enabled = std::env::var("CASCADE_TRIGGER_ENABLED")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(true);

    if !cascade_enabled {
        return Ok(ToolResult::error(
            "mission_cascade_trigger is disabled. Set CASCADE_TRIGGER_ENABLED=1 to enable.",
        ));
    }

    let manifest_pathbuf = match resolve_manifest_path(args.manifest_path.as_deref()) {
        Ok(p) => p,
        Err(e) => return Ok(ToolResult::error(e)),
    };
    let manifest_dir = manifest_pathbuf
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let graph = forge_core::universe_graph::resolve_universe_graph(&manifest_pathbuf)
        .map_err(|errs| anyhow!("universe graph errors:\n{}", errs.join("\n")))?;

    let delta = forge_core::universe_graph::ServiceDelta {
        service_id: args.service.clone(),
        changed_interfaces: args.changed.clone(),
    };

    let config = forge_core::cascade::CascadeConfig {
        max_repair_cycles: args.max_cycles,
        dry_run: false,
        ..Default::default()
    };

    let _ = state
        .bus
        .publish_task(TaskEvent::CascadeTriggered {
            service: args.service.clone(),
            changed: args.changed.clone(),
        })
        .await;

    let report = tokio::task::spawn_blocking(move || {
        let mut plan = forge_core::cascade::create_plan(&graph, &delta, &manifest_dir, config);
        forge_core::cascade::execute_plan(&mut plan)
    })
    .await
    .map_err(|e| anyhow!("cascade execution panicked: {e}"))?;

    let _ = state
        .bus
        .publish_task(TaskEvent::CascadeCompleted {
            service: args.service.clone(),
            services_repaired: report.services_repaired,
            services_failed: report.services_failed,
            hard_halted: report.hard_halted,
            duration_ms: report.total_duration.as_millis(),
        })
        .await;

    let phases: Vec<serde_json::Value> = report
        .plan
        .phases
        .iter()
        .map(|p| {
            serde_json::json!({
                "service_id": p.service_id,
                "path": p.service_path.to_string_lossy(),
                "depth": p.depth,
                "status": format!("{:?}", p.status),
                "repair_attempts": p.repair_attempts,
                "duration_ms": p.duration.map(|d| d.as_millis()),
            })
        })
        .collect();

    Ok(ToolResult::json_pretty(&serde_json::json!({
        "trigger_service": report.plan.trigger_service,
        "trigger_changes": report.plan.trigger_changes,
        "total_duration_ms": report.total_duration.as_millis(),
        "services_repaired": report.services_repaired,
        "services_failed": report.services_failed,
        "hard_halted": report.hard_halted,
        "phases": phases,
    })))
}
