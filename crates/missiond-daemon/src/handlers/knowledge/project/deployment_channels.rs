use anyhow::Result;
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use crate::context::v3_blueprint_runtime::{
    load_compiled_project_universe, CompiledServiceRuntimeEntry,
};

#[cfg(test)]
use crate::context::v3_blueprint_runtime::CompiledServiceSupportCatalog;

#[derive(Deserialize)]
struct DeploymentChannelArgs {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    service_id: Option<String>,
    #[serde(default)]
    include_observed: Option<bool>,
}

pub(super) async fn handle_deployment_channels(args: Value) -> Result<ToolResult> {
    let args: DeploymentChannelArgs = serde_json::from_value(args)?;
    let projection = build_projection(&args).await;
    Ok(ToolResult::json_pretty(&json!({
        "schema": "missiond.project-deployment-channels.v1",
        "ok": projection.compiled_ok,
        "source": projection.source,
        "filters": projection.filters,
        "summary": projection.summary,
        "channels": projection.channels,
        "services": projection.services,
        "observations": projection.observations,
        "diagnostics": projection.diagnostics,
    })))
}

pub(super) async fn handle_reconcile_deployment_channels(args: Value) -> Result<ToolResult> {
    let args: DeploymentChannelArgs = serde_json::from_value(args)?;
    let projection = build_projection(&args).await;
    let mut diagnostics = projection.diagnostics;
    diagnostics.extend(reconcile_channel_rows(&projection.channels));
    Ok(ToolResult::json_pretty(&json!({
        "schema": "missiond.project-deployment-channel-reconcile.v1",
        "ok": projection.compiled_ok && diagnostics.is_empty(),
        "source": projection.source,
        "filters": projection.filters,
        "summary": projection.summary,
        "channels": projection.channels,
        "observations": projection.observations,
        "diagnostics": diagnostics,
    })))
}

struct Projection {
    compiled_ok: bool,
    source: Value,
    filters: Value,
    summary: Value,
    channels: Vec<Value>,
    services: Vec<Value>,
    observations: Vec<Value>,
    diagnostics: Vec<Value>,
}

async fn build_projection(args: &DeploymentChannelArgs) -> Projection {
    let project_root = crate::helpers::missiond_project_root();
    let loaded = load_compiled_project_universe(&project_root, None);
    let include_observed = args.include_observed.unwrap_or(true);
    let project_filter = first_filter([
        args.project_id.as_deref(),
        args.project.as_deref(),
        args.id.as_deref(),
    ]);
    let service_filter = first_filter([args.service_id.as_deref(), args.service.as_deref()]);
    let filters = json!({
        "project": project_filter,
        "service": service_filter,
        "include_observed": include_observed,
    });
    let mut diagnostics = loaded
        .diagnostics
        .into_iter()
        .map(|message| json!({"kind": "compiled_universe_load", "message": message}))
        .collect::<Vec<_>>();
    let Some(payload) = loaded.payload else {
        return Projection {
            compiled_ok: false,
            source: json!({"kind": "compiled-project-universe", "status": "missing"}),
            filters,
            summary: json!({"total": 0}),
            channels: Vec::new(),
            services: Vec::new(),
            observations: Vec::new(),
            diagnostics,
        };
    };

    diagnostics.extend(
        payload
            .deployment_channel_diagnostics
            .iter()
            .filter(|diag| diagnostic_matches(diag, project_filter, service_filter))
            .cloned(),
    );

    let matched_services = payload
        .services
        .iter()
        .filter(|service| service_matches(service, project_filter, service_filter))
        .collect::<Vec<_>>();
    let services = matched_services
        .iter()
        .map(|service| service_value(service))
        .collect::<Vec<_>>();
    let mut seen_channels = BTreeSet::new();
    let mut channels = Vec::new();
    for service in &matched_services {
        for channel in &service.deployment_channels {
            push_channel_once(&mut channels, &mut seen_channels, channel.clone());
        }
    }
    for channel in payload
        .deployment_channels
        .iter()
        .filter(|channel| channel_matches(channel, project_filter, service_filter))
    {
        push_channel_once(&mut channels, &mut seen_channels, channel.clone());
    }
    if channels.is_empty() {
        channels = payload
            .services
            .iter()
            .filter(|service| service_matches(service, project_filter, service_filter))
            .flat_map(|service| service.deployment_channels.clone())
            .collect();
    }
    let observations = observe_deploy_center(&channels, include_observed).await;
    apply_observation_status(&mut channels, &observations);
    let summary = summarize_channels(&channels, &diagnostics, payload.deployment_channel_summary);

    Projection {
        compiled_ok: true,
        source: json!({"kind": "compiled-project-universe", "status": "loaded"}),
        filters,
        summary,
        channels,
        services,
        observations,
        diagnostics,
    }
}

async fn observe_deploy_center(channels: &[Value], include_observed: bool) -> Vec<Value> {
    let mut slug_executors = BTreeMap::<String, BTreeSet<String>>::new();
    for channel in channels {
        let Some(slug) = string_value(channel, "deploy_center_slug") else {
            continue;
        };
        let entry = slug_executors.entry(slug).or_default();
        if let Some(executor) = string_value(channel, "executor") {
            entry.insert(executor);
        }
    }
    if slug_executors.is_empty() {
        return Vec::new();
    }
    if !include_observed {
        return slug_executors
            .into_iter()
            .map(|(slug, executors)| {
                json!({
                    "deploy_center_slug": slug,
                    "status": "not_requested",
                    "executor_refs": executors.into_iter().collect::<Vec<_>>(),
                })
            })
            .collect();
    }
    let Some(base_url) = std::env::var("MISSIOND_DEPLOY_CENTER_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        return slug_executors
            .into_iter()
            .map(|(slug, executors)| {
                json!({
                    "deploy_center_slug": slug,
                    "status": "unavailable",
                    "reason": "MISSIOND_DEPLOY_CENTER_BASE_URL not configured",
                    "executor_refs": executors.into_iter().collect::<Vec<_>>(),
                })
            })
            .collect();
    };
    let token = std::env::var("MISSIOND_DEPLOY_CENTER_READ_TOKEN")
        .or_else(|_| std::env::var("MISSIOND_DEPLOY_CENTER_TOKEN"))
        .ok()
        .filter(|value| !value.trim().is_empty());
    let Some(token) = token else {
        return slug_executors
            .into_iter()
            .map(|(slug, executors)| {
                json!({
                    "deploy_center_slug": slug,
                    "status": "unavailable",
                    "reason": "read token not configured",
                    "token_ref": std::env::var("MISSIOND_DEPLOY_CENTER_READ_TOKEN_REF").ok(),
                    "executor_refs": executors.into_iter().collect::<Vec<_>>(),
                })
            })
            .collect();
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build();
    let Ok(client) = client else {
        return slug_executors
            .into_iter()
            .map(|(slug, executors)| {
                json!({
                    "deploy_center_slug": slug,
                    "status": "unavailable",
                    "reason": "http client init failed",
                    "executor_refs": executors.into_iter().collect::<Vec<_>>(),
                })
            })
            .collect();
    };
    let base_url = base_url.trim_end_matches('/');
    let mut rows = Vec::new();
    for (slug, executors) in slug_executors {
        let project_url = format!("{base_url}/projects/{slug}");
        let stage_url = format!("{base_url}/projects/{slug}/stage-configs");
        let provenance_url = format!("{base_url}/provenance/{slug}");
        let executors_url = format!("{base_url}/executors");
        let executor_instances_url = format!("{base_url}/executors/instances");
        let project_status = fetch_status(&client, &project_url, &token).await;
        let stage_status = fetch_status(&client, &stage_url, &token).await;
        let provenance_status = fetch_status(&client, &provenance_url, &token).await;
        let executors_status = fetch_status(&client, &executors_url, &token).await;
        let executor_instances_status =
            fetch_status(&client, &executor_instances_url, &token).await;
        rows.push(json!({
            "deploy_center_slug": slug,
            "status": if project_status == "ok" || stage_status == "ok" || provenance_status == "ok" || executors_status == "ok" || executor_instances_status == "ok" { "observed" } else { "unavailable" },
            "project": project_status,
            "stage_configs": stage_status,
            "provenance": provenance_status,
            "executors": executors_status,
            "executor_instances": executor_instances_status,
            "executor_refs": executors.into_iter().collect::<Vec<_>>(),
        }));
    }
    rows
}

async fn fetch_status(client: &reqwest::Client, url: &str, token: &str) -> &'static str {
    match client.get(url).bearer_auth(token).send().await {
        Ok(response) if response.status().is_success() => "ok",
        Ok(response) if response.status().as_u16() == 404 => "not_found",
        Ok(_) => "error",
        Err(_) => "unavailable",
    }
}

fn push_channel_once(channels: &mut Vec<Value>, seen: &mut BTreeSet<String>, channel: Value) {
    let key = channel_identity(&channel);
    if seen.insert(key) {
        channels.push(channel);
    }
}

fn channel_identity(channel: &Value) -> String {
    string_value(channel, "id").unwrap_or_else(|| {
        [
            string_value(channel, "project_id")
                .or_else(|| string_value(channel, "projectId"))
                .unwrap_or_default(),
            string_value(channel, "service_id")
                .or_else(|| string_value(channel, "serviceId"))
                .unwrap_or_default(),
            string_value(channel, "surface").unwrap_or_default(),
            string_value(channel, "source_ref")
                .or_else(|| string_value(channel, "sourceRef"))
                .unwrap_or_default(),
        ]
        .join(":")
    })
}

fn apply_observation_status(channels: &mut [Value], observations: &[Value]) {
    for channel in channels {
        let Some(slug) = string_value(channel, "deploy_center_slug") else {
            continue;
        };
        let status = observations
            .iter()
            .find(|row| string_value(row, "deploy_center_slug").as_deref() == Some(slug.as_str()))
            .and_then(|row| row.get("status").and_then(Value::as_str))
            .unwrap_or("unavailable");
        if let Some(object) = channel.as_object_mut() {
            object.insert(
                "observed_status".to_string(),
                Value::String(status.to_string()),
            );
            object
                .entry("drift_status".to_string())
                .or_insert_with(|| Value::String("not_checked".to_string()));
        }
    }
}

fn reconcile_channel_rows(channels: &[Value]) -> Vec<Value> {
    let mut diagnostics = Vec::new();
    for channel in channels {
        if string_value(channel, "channel_kind").as_deref() == Some("github_actions") {
            match string_value(channel, "source_ref") {
                Some(source_ref) if !Path::new(&source_ref).exists() => diagnostics.push(json!({
                    "kind": "github_workflow_missing",
                    "service_id": string_value(channel, "service_id"),
                    "workflow": string_value(channel, "workflow"),
                    "source_ref": source_ref,
                })),
                None => diagnostics.push(json!({
                    "kind": "github_workflow_source_missing",
                    "service_id": string_value(channel, "service_id"),
                })),
                _ => {}
            }
        }
        if string_value(channel, "channel_kind").as_deref() == Some("native_workflow")
            && channel
                .get("target_side_build_prohibited")
                .and_then(Value::as_bool)
                != Some(true)
        {
            diagnostics.push(json!({
                "kind": "native_workflow_target_side_build_not_prohibited",
                "service_id": string_value(channel, "service_id"),
            }));
        }
    }
    diagnostics
}

fn summarize_channels(
    channels: &[Value],
    diagnostics: &[Value],
    compiled_summary: Option<Value>,
) -> Value {
    let mut by_surface = serde_json::Map::new();
    let mut by_kind = serde_json::Map::new();
    for channel in channels {
        increment(
            &mut by_surface,
            string_value(channel, "surface").unwrap_or_else(|| "unknown".to_string()),
        );
        increment(
            &mut by_kind,
            string_value(channel, "channel_kind").unwrap_or_else(|| "unknown".to_string()),
        );
    }
    json!({
        "total": channels.len(),
        "by_surface": by_surface,
        "by_kind": by_kind,
        "diagnostics": diagnostics.len(),
        "compiled": compiled_summary,
    })
}

fn increment(map: &mut serde_json::Map<String, Value>, key: String) {
    let current = map.get(&key).and_then(Value::as_u64).unwrap_or(0);
    map.insert(key, Value::from(current + 1));
}

fn service_value(service: &CompiledServiceRuntimeEntry) -> Value {
    json!({
        "id": service.id,
        "project": service.project,
        "root": service.root,
        "environment": service.environment,
        "deploymentChannelCount": service.deployment_channels.len(),
    })
}

fn service_matches(
    service: &CompiledServiceRuntimeEntry,
    project_filter: Option<&str>,
    service_filter: Option<&str>,
) -> bool {
    if let Some(service_filter) = service_filter {
        return service_lookup_matches(service, service_filter);
    }
    if let Some(project_filter) = project_filter {
        return service_lookup_matches(service, project_filter);
    }
    true
}

fn channel_matches(
    channel: &Value,
    project_filter: Option<&str>,
    service_filter: Option<&str>,
) -> bool {
    if let Some(service_filter) = service_filter {
        return channel_lookup_matches(channel, service_filter);
    }
    if let Some(project_filter) = project_filter {
        return channel_lookup_matches(channel, project_filter);
    }
    true
}

fn diagnostic_matches(
    diag: &Value,
    project_filter: Option<&str>,
    service_filter: Option<&str>,
) -> bool {
    if let Some(service_filter) = service_filter {
        return json_lookup_matches(diag, service_filter);
    }
    if let Some(project_filter) = project_filter {
        return json_lookup_matches(diag, project_filter);
    }
    true
}

fn service_lookup_matches(service: &CompiledServiceRuntimeEntry, lookup: &str) -> bool {
    let lookup = normalize_lookup(lookup);
    service_lookup_values(service)
        .into_iter()
        .any(|value| value == lookup)
}

fn service_lookup_values(service: &CompiledServiceRuntimeEntry) -> Vec<String> {
    let catalog = service.support_catalog.as_ref();
    let mut values = Vec::new();
    push_lookup(&mut values, service.id.as_deref());
    push_lookup(&mut values, service.project.as_deref());
    push_lookup(&mut values, service.public_base_url.as_deref());
    push_lookup(&mut values, service.frontend_url.as_deref());
    push_lookup(&mut values, service.api_base_url.as_deref());
    for domain in &service.domains {
        push_lookup(&mut values, Some(domain));
    }
    if let Some(catalog) = catalog {
        push_lookup(&mut values, catalog.service_id.as_deref());
        push_lookup(&mut values, catalog.project_id.as_deref());
        push_lookup(&mut values, catalog.deploy_center_slug.as_deref());
        push_lookup(&mut values, catalog.runtime_target.as_deref());
        push_lookup(&mut values, catalog.container.as_deref());
        push_lookup(&mut values, catalog.public_base_url.as_deref());
        push_lookup(&mut values, catalog.frontend_url.as_deref());
        push_lookup(&mut values, catalog.api_base_url.as_deref());
        for domain in &catalog.domains {
            push_lookup(&mut values, Some(domain));
        }
    }
    values.sort();
    values.dedup();
    values
}

fn channel_lookup_matches(channel: &Value, lookup: &str) -> bool {
    let lookup = normalize_lookup(lookup);
    channel_lookup_values(channel)
        .into_iter()
        .any(|value| value == lookup)
}

fn channel_lookup_values(channel: &Value) -> Vec<String> {
    let mut values = Vec::new();
    for key in [
        "project_id",
        "projectId",
        "service_id",
        "serviceId",
        "deploy_center_slug",
        "deployCenterSlug",
        "container",
        "image",
        "runtime_target",
        "runtimeTarget",
        "production_domain",
        "productionDomain",
        "fallback_domain",
        "fallbackDomain",
    ] {
        push_lookup(&mut values, string_value(channel, key).as_deref());
    }
    values.sort();
    values.dedup();
    values
}

fn json_lookup_matches(value: &Value, lookup: &str) -> bool {
    let lookup = normalize_lookup(lookup);
    for key in [
        "project_id",
        "projectId",
        "service_id",
        "serviceId",
        "deploy_center_slug",
        "deployCenterSlug",
    ] {
        if string_value(value, key)
            .as_deref()
            .map(normalize_lookup)
            .as_deref()
            == Some(lookup.as_str())
        {
            return true;
        }
    }
    false
}

fn push_lookup(values: &mut Vec<String>, value: Option<&str>) {
    if let Some(value) = value
        .map(normalize_lookup)
        .filter(|value| !value.is_empty())
    {
        values.push(value);
    }
}

fn normalize_lookup(value: &str) -> String {
    value.trim().replace('_', "-").to_ascii_lowercase()
}

fn first_filter<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> Option<&'a str> {
    values
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
}

fn string_value(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payments_service() -> CompiledServiceRuntimeEntry {
        CompiledServiceRuntimeEntry {
            id: Some("payments".to_string()),
            project: Some("payments".to_string()),
            root: Some("/repo/services/payments".to_string()),
            intent: None,
            backend: Some("services/payments".to_string()),
            frontend: None,
            operations: None,
            environment: Some("production".to_string()),
            public_base_url: None,
            frontend_url: None,
            api_base_url: Some("https://auth.xiaojinpro.com/payments".to_string()),
            domains: vec!["auth.xiaojinpro.com".to_string()],
            health: vec!["/payments/health/ready".to_string()],
            dependencies: Vec::new(),
            ops_capability: Some("deploy-ops".to_string()),
            surface: Some("gcp-runtime".to_string()),
            deployment: None,
            frontend_deployment: None,
            build_lane: None,
            deployment_channels: Vec::new(),
            support_catalog: Some(CompiledServiceSupportCatalog {
                service_id: Some("payments".to_string()),
                project_id: Some("payments".to_string()),
                domains: vec!["auth.xiaojinpro.com".to_string()],
                public_base_url: None,
                frontend_url: None,
                api_base_url: Some("https://auth.xiaojinpro.com/payments".to_string()),
                health: vec!["/payments/health/ready".to_string()],
                dependencies: Vec::new(),
                deploy_center_slug: Some("xjp-payments".to_string()),
                runtime_target: Some("gcp-runtime".to_string()),
                executor: Some("gcp-agent".to_string()),
                container: Some("xjp-payments".to_string()),
                service_manifest_refs: vec!["services/payments/service.manifest.toml".to_string()],
                credential_refs: Vec::new(),
                source_evidence: Vec::new(),
                db_migration_namespace: None,
                database_namespace: None,
            }),
        }
    }

    #[test]
    fn service_filter_matches_deploy_center_slug() {
        let service = payments_service();

        assert!(service_matches(&service, Some("xjp-payments"), None));
        assert!(service_matches(&service, None, Some("xjp-payments")));
        assert!(service_matches(&service, Some("payments"), None));
        assert!(!service_matches(&service, Some("xjp-asr"), None));
    }

    #[test]
    fn channel_filter_matches_deploy_center_slug_and_container() {
        let channel = json!({
            "service_id": "payments",
            "project_id": "payments",
            "deploy_center_slug": "xjp-payments",
            "container": "xjp-payments",
            "surface": "runtime"
        });

        assert!(channel_matches(&channel, Some("xjp-payments"), None));
        assert!(channel_matches(&channel, None, Some("xjp-payments")));
        assert!(channel_matches(&channel, Some("payments"), None));
        assert!(!channel_matches(&channel, Some("xjp-asr"), None));
    }

    #[tokio::test]
    async fn project_slug_projection_keeps_all_service_channels() {
        let args = DeploymentChannelArgs {
            id: None,
            project: Some("xjp-payments".to_string()),
            project_id: None,
            service: None,
            service_id: None,
            include_observed: Some(false),
        };

        let projection = build_projection(&args).await;
        let surfaces = projection
            .channels
            .iter()
            .filter_map(|channel| string_value(channel, "surface"))
            .collect::<BTreeSet<_>>();

        assert!(surfaces.contains("build"));
        assert!(surfaces.contains("runtime"));
        assert!(projection.channels.iter().any(|channel| {
            string_value(channel, "channel_kind").as_deref() == Some("native_workflow")
        }));
    }
}
