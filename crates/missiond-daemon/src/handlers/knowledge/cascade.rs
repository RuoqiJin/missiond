//! Flywheel MCP handlers — Universe Graph + Cascade Controller.
//!
//! Exposes example-graph's cross-service dependency analysis and cascade
//! repair system as MCP tools for the Commander in Claude Code.

use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;

use crate::event_bus::DaemonEvent;
use crate::state::AppState;

// ---- Input structs ----

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UniverseGraphArgs {
    manifest_path: Option<String>,
    #[serde(default = "default_format")]
    format: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CascadePlanArgs {
    manifest_path: Option<String>,
    service: String,
    #[serde(default)]
    changed: Vec<String>,
}

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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CascadeLintArgs {
    manifest_path: Option<String>,
}

fn default_format() -> String {
    "json".to_string()
}
fn default_max_cycles() -> usize {
    3
}

// ---- Path whitelist ----

/// Resolve and validate manifest path against an allowed root directory.
/// Returns the canonicalized path or an error string suitable for ToolResult::error.
fn resolve_manifest_path(args_path: Option<&str>) -> Result<PathBuf, String> {
    // Operators must opt in by setting either $UNIVERSE_MANIFEST (an absolute
    // path to the universe.intent.lisp file) or by passing an explicit
    // `manifestPath` argument. Fail fast rather than guessing.
    let default = std::env::var("UNIVERSE_MANIFEST").ok();
    let raw = match args_path.or(default.as_deref()) {
        Some(p) => p.to_string(),
        None => {
            return Err(
                "manifest path not configured: set $UNIVERSE_MANIFEST or pass manifestPath"
                    .to_string(),
            )
        }
    };
    let path = PathBuf::from(&raw);

    let canonical = path
        .canonicalize()
        .map_err(|e| format!("manifest path not found: {}: {}", raw, e))?;

    let allowed_root = match std::env::var("UNIVERSE_ROOT").map(PathBuf::from) {
        Ok(r) => r,
        Err(_) => {
            return Err(
                "$UNIVERSE_ROOT not set: cannot enforce manifest path allowlist".to_string(),
            )
        }
    };

    if let Ok(root) = allowed_root.canonicalize() {
        if !canonical.starts_with(&root) {
            return Err(format!(
                "manifestPath '{}' is outside allowed root '{}'",
                canonical.display(),
                root.display()
            ));
        }
    }

    Ok(canonical)
}

// ---- Dispatch ----

// @beacon: cascade
pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_universe_graph" => handle_universe_graph(args).await,
        "mission_cascade_plan" => handle_cascade_plan(args).await,
        "mission_cascade_trigger" => handle_cascade_trigger(state, args).await,
        "mission_cascade_lint" => handle_cascade_lint(args).await,
        _ => Err(anyhow!("unknown cascade tool: {name}")),
    }
}

// ---- Handlers ----

/// mission_universe_graph -- resolve and return the full universe dependency graph.
async fn handle_universe_graph(args: Value) -> Result<ToolResult> {
    let args: UniverseGraphArgs = serde_json::from_value(args)?;
    let manifest_path = match resolve_manifest_path(args.manifest_path.as_deref()) {
        Ok(p) => p,
        Err(e) => return Ok(ToolResult::error(e)),
    };

    let graph = example_graph::universe_graph::resolve_universe_graph(&manifest_path)
        .map_err(|errs| anyhow!("universe graph errors:\n{}", errs.join("\n")))?;

    if args.format == "text" {
        let mut out = String::new();
        out.push_str(&format!(
            "Universe: {} services\n\n",
            graph.services.len()
        ));
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
        // JSON format
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

/// mission_cascade_plan -- compute blast radius and generate a repair plan (dry-run).
async fn handle_cascade_plan(args: Value) -> Result<ToolResult> {
    let args: CascadePlanArgs = serde_json::from_value(args)?;
    let manifest_path = match resolve_manifest_path(args.manifest_path.as_deref()) {
        Ok(p) => p,
        Err(e) => return Ok(ToolResult::error(e)),
    };
    let manifest_dir = manifest_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    let graph = example_graph::universe_graph::resolve_universe_graph(&manifest_path)
        .map_err(|errs| anyhow!("universe graph errors:\n{}", errs.join("\n")))?;

    let delta = example_graph::universe_graph::ServiceDelta {
        service_id: args.service.clone(),
        changed_interfaces: args.changed.clone(),
    };

    let config = example_graph::cascade::CascadeConfig {
        dry_run: true,
        ..Default::default()
    };

    let plan = example_graph::cascade::create_plan(&graph, &delta, manifest_dir, config);

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

/// mission_cascade_trigger -- execute a cascade repair plan (real execution).
async fn handle_cascade_trigger(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: CascadeTriggerArgs = serde_json::from_value(args)?;

    // Bug 2: Permission check — environment-based kill-switch for external command execution
    let cascade_enabled = std::env::var("CASCADE_TRIGGER_ENABLED")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(true); // dev-mode default: allow

    if !cascade_enabled {
        return Ok(ToolResult::error(
            "mission_cascade_trigger is disabled. Set CASCADE_TRIGGER_ENABLED=1 to enable.",
        ));
    }

    // Bug 1: Path whitelist validation
    let manifest_pathbuf = match resolve_manifest_path(args.manifest_path.as_deref()) {
        Ok(p) => p,
        Err(e) => return Ok(ToolResult::error(e)),
    };
    let manifest_dir = manifest_pathbuf
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let graph = example_graph::universe_graph::resolve_universe_graph(&manifest_pathbuf)
        .map_err(|errs| anyhow!("universe graph errors:\n{}", errs.join("\n")))?;

    let delta = example_graph::universe_graph::ServiceDelta {
        service_id: args.service.clone(),
        changed_interfaces: args.changed.clone(),
    };

    let config = example_graph::cascade::CascadeConfig {
        max_repair_cycles: args.max_cycles,
        dry_run: false,
        ..Default::default()
    };

    // Bug 3: Emit CascadeTriggered event before execution
    state.event_bus.publish(DaemonEvent::CascadeTriggered {
        service: args.service.clone(),
        changed: args.changed.clone(),
    });

    // Execute in a blocking task since cascade runs external commands
    let report = tokio::task::spawn_blocking(move || {
        let mut plan = example_graph::cascade::create_plan(&graph, &delta, &manifest_dir, config);
        example_graph::cascade::execute_plan(&mut plan)
    })
    .await
    .map_err(|e| anyhow!("cascade execution panicked: {e}"))?;

    // Bug 3: Emit CascadeCompleted event after execution
    state.event_bus.publish(DaemonEvent::CascadeCompleted {
        service: args.service.clone(),
        services_repaired: report.services_repaired,
        services_failed: report.services_failed,
        hard_halted: report.hard_halted,
        duration_ms: report.total_duration.as_millis(),
    });

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

/// mission_cascade_lint -- validate universe integrity.
async fn handle_cascade_lint(args: Value) -> Result<ToolResult> {
    let args: CascadeLintArgs = serde_json::from_value(args)?;
    let manifest_path = match resolve_manifest_path(args.manifest_path.as_deref()) {
        Ok(p) => p,
        Err(e) => return Ok(ToolResult::error(e)),
    };
    let manifest_dir = manifest_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    let graph = example_graph::universe_graph::resolve_universe_graph(&manifest_path)
        .map_err(|errs| anyhow!("universe graph errors:\n{}", errs.join("\n")))?;

    let violations =
        example_graph::universe_graph::validate_universe_integrity(&graph, manifest_dir);

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
        let warnings = violations.iter().filter(|v| v.severity == "warning").count();

        Ok(ToolResult::json_pretty(&serde_json::json!({
            "status": if errors > 0 { "failed" } else { "warnings" },
            "service_count": graph.services.len(),
            "error_count": errors,
            "warning_count": warnings,
            "violations": vs,
        })))
    }
}
