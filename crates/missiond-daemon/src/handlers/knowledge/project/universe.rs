use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::context::v3_blueprint_runtime::{
    load_compiled_project_universe, CompiledProjectUniverse, CompiledRuntimeSnapshot,
    CompiledServiceRuntimeEntry, CompiledServiceSupportCatalog,
};

pub(super) async fn handle_universe(args: Value) -> Result<ToolResult> {
    let filter_id = args.get("id").and_then(|v| v.as_str()).map(str::to_string);
    let project_root = crate::helpers::missiond_project_root();
    let compiled = load_compiled_project_universe(&project_root, None);
    if let Some(payload) = compiled.payload {
        return Ok(compiled_universe_result(
            filter_id.as_deref(),
            &payload,
            compiled.snapshot.as_ref(),
            compiled.diagnostics,
        ));
    }

    let shard_path = locate_service_runtime_shard()?;
    let source = std::fs::read_to_string(&shard_path)
        .map_err(|e| anyhow!("Failed to read {}: {}", shard_path.display(), e))?;
    let Some(block) = extract_balanced_after(&source, "(service-runtime-universe") else {
        return Ok(ToolResult::error(
            "service-runtime-universe not found in compiled project universe or active V3 shard",
        ));
    };

    let mut services = Vec::new();
    for form in extract_forms(&block, "(service ") {
        let Some(id) = keyword_scalar(&form, ":id") else {
            continue;
        };
        if filter_id.as_deref().is_some_and(|want| want != id) {
            continue;
        }
        let deployment_form = keyword_form(&form, ":deployment");
        let frontend_deployment_form = keyword_form(&form, ":frontend-deployment");
        let build_lane_form = keyword_form(&form, ":build-lane");
        let proxy_form = keyword_form(&form, ":proxy");
        let ports_form = keyword_form(&form, ":ports");
        let event_ingest_form = keyword_form(&form, ":event-ingest");
        let deployment_channels = source_service_deployment_channels(
            &id,
            keyword_scalar(&form, ":project").as_deref(),
            deployment_form.as_deref(),
            frontend_deployment_form.as_deref(),
            build_lane_form.as_deref(),
        );
        services.push(json!({
            "id": id,
            "project": keyword_scalar(&form, ":project"),
            "root": keyword_scalar(&form, ":root"),
            "intent": keyword_scalar(&form, ":intent"),
            "backend": keyword_scalar(&form, ":backend"),
            "environment": keyword_scalar(&form, ":environment"),
            "publicBaseUrl": keyword_scalar(&form, ":public-base-url"),
            "issuer": keyword_scalar(&form, ":issuer"),
            "domains": keyword_list(&form, ":domains"),
            "dnsProvider": keyword_scalar(&form, ":dns-provider"),
            "dnsCapability": keyword_form(&form, ":dns-capability").unwrap_or_default(),
            "deployment": {
                "substrate": deployment_form.as_deref().and_then(|s| keyword_scalar(s, ":substrate")),
                "namespace": deployment_form.as_deref().and_then(|s| keyword_scalar(s, ":namespace")),
                "deployment": deployment_form.as_deref().and_then(|s| keyword_scalar(s, ":deployment")),
                "service": deployment_form.as_deref().and_then(|s| keyword_scalar(s, ":service")),
                "replicas": deployment_form.as_deref().and_then(|s| keyword_scalar(s, ":replicas")),
                "hpaMin": deployment_form.as_deref().and_then(|s| keyword_scalar(s, ":hpa-min")),
                "hpaMax": deployment_form.as_deref().and_then(|s| keyword_scalar(s, ":hpa-max")),
                "image": deployment_form.as_deref().and_then(|s| keyword_scalar(s, ":image")),
                "serviceAccount": deployment_form.as_deref().and_then(|s| keyword_scalar(s, ":service-account")),
            },
            "frontendDeployment": {
                "substrate": frontend_deployment_form.as_deref().and_then(|s| keyword_scalar(s, ":substrate")),
                "project": frontend_deployment_form.as_deref().and_then(|s| keyword_scalar(s, ":project")),
                "rootDirectory": frontend_deployment_form.as_deref().and_then(|s| keyword_scalar(s, ":root-directory")),
                "productionDomain": frontend_deployment_form.as_deref().and_then(|s| keyword_scalar(s, ":production-domain")),
                "fallbackDomain": frontend_deployment_form.as_deref().and_then(|s| keyword_scalar(s, ":fallback-domain")),
                "authority": frontend_deployment_form.as_deref().and_then(|s| keyword_scalar(s, ":authority")).unwrap_or_else(|| "vercel".to_string()),
            },
            "buildLane": {
                "id": build_lane_form.as_deref().and_then(|s| keyword_scalar(s, ":id")),
                "builder": build_lane_form.as_deref().and_then(|s| keyword_scalar(s, ":builder")),
                "executor": build_lane_form.as_deref().and_then(|s| keyword_scalar(s, ":executor")),
                "sourceSync": build_lane_form.as_deref().and_then(|s| keyword_scalar(s, ":source-sync")),
                "artifactLane": build_lane_form.as_deref().and_then(|s| keyword_scalar(s, ":artifact-lane")),
                "image": build_lane_form.as_deref().and_then(|s| keyword_scalar(s, ":image")),
                "authority": build_lane_form.as_deref().and_then(|s| keyword_scalar(s, ":authority")).unwrap_or_else(|| "deploy-center".to_string()),
            },
            "deploymentChannels": deployment_channels,
            "proxy": {
                "kind": proxy_form.as_deref().and_then(|s| keyword_scalar(s, ":kind")),
                "domain": proxy_form.as_deref().and_then(|s| keyword_scalar(s, ":domain")),
                "file": proxy_form.as_deref().and_then(|s| keyword_scalar(s, ":file")),
                "sseNoBuffer": proxy_form.as_deref().and_then(|s| keyword_scalar(s, ":sse-no-buffer")),
            },
            "ports": {
                "http": ports_form.as_deref().and_then(|s| keyword_scalar(s, ":http")),
                "metrics": ports_form.as_deref().and_then(|s| keyword_scalar(s, ":metrics")),
                "service": ports_form.as_deref().and_then(|s| keyword_scalar(s, ":service")),
            },
            "health": keyword_list(&form, ":health"),
            "eventIngest": {
                "endpoint": event_ingest_form.as_deref().and_then(|s| keyword_scalar(s, ":endpoint")),
                "domain": event_ingest_form.as_deref().and_then(|s| keyword_scalar(s, ":domain")),
                "event": event_ingest_form.as_deref().and_then(|s| keyword_scalar(s, ":event")),
                "source": event_ingest_form.as_deref().and_then(|s| keyword_scalar(s, ":source")),
                "authority": event_ingest_form.as_deref().and_then(|s| keyword_scalar(s, ":authority")),
                "tokenEnv": event_ingest_form.as_deref().and_then(|s| keyword_scalar(s, ":token-env")),
            },
            "dependencies": keyword_list(&form, ":dependencies"),
            "opsCapability": keyword_scalar(&form, ":ops-capability"),
            "sourceEvidence": keyword_list(&form, ":source-evidence"),
            "risks": keyword_list(&form, ":risks"),
        }));
    }

    let capabilities = extract_forms(&block, "(capability ")
        .into_iter()
        .map(|form| {
            json!({
                "id": keyword_scalar(&form, ":id"),
                "provider": keyword_scalar(&form, ":provider"),
                "defaultMode": keyword_scalar(&form, ":default-mode"),
                "mutatingPolicy": keyword_scalar(&form, ":mutating-policy"),
                "secrets": keyword_list(&form, ":secrets"),
                "surface": keyword_scalar(&form, ":surface"),
            })
        })
        .collect::<Vec<_>>();

    Ok(ToolResult::json_pretty(&json!({
        "schema": "missiond.service-runtime-universe.v1",
        "source": shard_path.display().to_string(),
        "sourceKind": "source-lisp-fallback",
        "compiledRuntime": {
            "snapshot": Value::Null,
            "diagnostics": compiled.diagnostics,
        },
        "services": services,
        "capabilities": capabilities,
        "domainManagement": Value::Null,
    })))
}

fn compiled_universe_result(
    filter_id: Option<&str>,
    payload: &CompiledProjectUniverse,
    snapshot: Option<&CompiledRuntimeSnapshot>,
    diagnostics: Vec<String>,
) -> ToolResult {
    let services = payload
        .services
        .iter()
        .filter(|service| compiled_service_matches(service, filter_id))
        .map(compiled_service_to_value)
        .collect::<Vec<_>>();

    ToolResult::json_pretty(&json!({
        "schema": "missiond.service-runtime-universe.v1",
        "source": "compiled-project-universe",
        "sourceKind": "compiled-runtime",
        "compiledRuntime": {
            "snapshot": snapshot.map(compiled_snapshot_to_value).unwrap_or(Value::Null),
            "diagnostics": diagnostics,
        },
        "services": services,
        "capabilities": [],
        "domainManagement": payload.domain_management.clone().unwrap_or(Value::Null),
    }))
}

fn compiled_service_matches(
    service: &CompiledServiceRuntimeEntry,
    filter_id: Option<&str>,
) -> bool {
    let Some(filter_id) = filter_id else {
        return true;
    };
    let normalized_filter = normalize_lookup(filter_id);
    compiled_service_lookup_values(service)
        .into_iter()
        .any(|value| value == normalized_filter)
}

fn compiled_service_lookup_values(service: &CompiledServiceRuntimeEntry) -> Vec<String> {
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

fn compiled_service_to_value(service: &CompiledServiceRuntimeEntry) -> Value {
    let support = service.support_catalog.as_ref();
    let deployment = compiled_deployment_value(service.deployment.as_ref(), support);
    let frontend_deployment =
        compiled_frontend_deployment_value(service.frontend_deployment.as_ref());
    let build_lane = compiled_build_lane_value(service.build_lane.as_ref());
    json!({
        "id": service.id,
        "project": service.project,
        "root": service.root,
        "intent": service.intent,
        "backend": service.backend,
        "frontend": service.frontend,
        "operations": service.operations,
        "environment": service.environment,
        "publicBaseUrl": service.public_base_url,
        "frontendUrl": service.frontend_url,
        "apiBaseUrl": service.api_base_url,
        "domains": service.domains,
        "deployment": deployment,
        "frontendDeployment": frontend_deployment,
        "buildLane": build_lane,
        "deploymentChannels": service.deployment_channels,
        "proxy": {
            "kind": Value::Null,
            "domain": Value::Null,
            "file": Value::Null,
            "sseNoBuffer": Value::Null,
        },
        "ports": {
            "http": Value::Null,
            "metrics": Value::Null,
            "service": Value::Null,
        },
        "health": service.health,
        "eventIngest": {
            "endpoint": Value::Null,
            "domain": Value::Null,
            "event": Value::Null,
            "source": Value::Null,
            "authority": Value::Null,
            "tokenEnv": Value::Null,
        },
        "dependencies": service.dependencies,
        "opsCapability": service.ops_capability,
        "sourceEvidence": support
            .map(|support| support.source_evidence.clone())
            .unwrap_or_default(),
        "risks": [],
        "supportCatalog": support,
    })
}

fn compiled_deployment_value(
    deployment: Option<&Value>,
    support: Option<&CompiledServiceSupportCatalog>,
) -> Value {
    json!({
        "substrate": value_string(deployment, "substrate").unwrap_or_else(|| "deploy-center".to_string()),
        "dcSlug": value_string(deployment, "dc_slug")
            .or_else(|| value_string(deployment, "dcSlug"))
            .or_else(|| support.and_then(|support| support.deploy_center_slug.clone())),
        "runtimeTarget": value_string(deployment, "runtime_target")
            .or_else(|| value_string(deployment, "runtimeTarget"))
            .or_else(|| support.and_then(|support| support.runtime_target.clone())),
        "executor": value_string(deployment, "executor")
            .or_else(|| support.and_then(|support| support.executor.clone())),
        "container": value_string(deployment, "container")
            .or_else(|| support.and_then(|support| support.container.clone())),
        "namespace": value_string(deployment, "namespace"),
        "deployment": value_string(deployment, "deployment"),
        "service": value_string(deployment, "service"),
        "hostBind": value_string(deployment, "host_bind").or_else(|| value_string(deployment, "hostBind")),
        "localBind": value_string(deployment, "local_bind").or_else(|| value_string(deployment, "localBind")),
        "proxy": value_string(deployment, "proxy"),
        "imageEnv": value_string(deployment, "image_env").or_else(|| value_string(deployment, "imageEnv")),
        "artifactLane": value_string(deployment, "artifact_delivery_lane").or_else(|| value_string(deployment, "artifactLane")),
        "authority": value_string(deployment, "authority"),
        "targetSideBuildProhibited": value_bool(deployment, "target_side_build_prohibited")
            .or_else(|| value_bool(deployment, "targetSideBuildProhibited")),
    })
}

fn compiled_frontend_deployment_value(frontend_deployment: Option<&Value>) -> Value {
    json!({
        "substrate": value_string(frontend_deployment, "substrate"),
        "project": value_string(frontend_deployment, "project"),
        "rootDirectory": value_string(frontend_deployment, "root_directory").or_else(|| value_string(frontend_deployment, "rootDirectory")),
        "productionDomain": value_string(frontend_deployment, "production_domain").or_else(|| value_string(frontend_deployment, "productionDomain")),
        "fallbackDomain": value_string(frontend_deployment, "fallback_domain").or_else(|| value_string(frontend_deployment, "fallbackDomain")),
        "authority": value_string(frontend_deployment, "authority").unwrap_or_else(|| "vercel".to_string()),
    })
}

fn compiled_build_lane_value(build_lane: Option<&Value>) -> Value {
    json!({
        "id": value_string(build_lane, "id"),
        "builder": value_string(build_lane, "builder"),
        "executor": value_string(build_lane, "executor"),
        "sourceSync": value_string(build_lane, "source_sync").or_else(|| value_string(build_lane, "sourceSync")),
        "artifactLane": value_string(build_lane, "artifact_lane").or_else(|| value_string(build_lane, "artifactLane")),
        "image": value_string(build_lane, "image"),
        "authority": value_string(build_lane, "authority").unwrap_or_else(|| "deploy-center".to_string()),
        "targetSideBuildProhibited": value_bool(build_lane, "target_side_build_prohibited")
            .or_else(|| value_bool(build_lane, "targetSideBuildProhibited")),
    })
}

fn value_string(value: Option<&Value>, key: &str) -> Option<String> {
    value?
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn value_bool(value: Option<&Value>, key: &str) -> Option<bool> {
    value?.get(key).and_then(Value::as_bool)
}

fn source_service_deployment_channels(
    service_id: &str,
    project_id: Option<&str>,
    deployment_form: Option<&str>,
    frontend_deployment_form: Option<&str>,
    build_lane_form: Option<&str>,
) -> Vec<Value> {
    let project_id = project_id.unwrap_or(service_id);
    let mut channels = Vec::new();
    if let Some(form) = build_lane_form {
        channels.push(json!({
            "id": format!("{service_id}:build"),
            "serviceId": service_id,
            "projectId": project_id,
            "surface": "build",
            "substrate": keyword_scalar(form, ":id").unwrap_or_else(|| "privatecloud".to_string()),
            "buildLane": keyword_scalar(form, ":id"),
            "builder": keyword_scalar(form, ":builder"),
            "executor": keyword_scalar(form, ":executor"),
            "sourceSync": keyword_scalar(form, ":source-sync"),
            "artifactLane": keyword_scalar(form, ":artifact-lane"),
            "image": keyword_scalar(form, ":image"),
            "authority": keyword_scalar(form, ":authority").unwrap_or_else(|| "deploy-center".to_string()),
        }));
    }
    if let Some(form) = deployment_form {
        channels.push(json!({
            "id": format!("{service_id}:runtime"),
            "serviceId": service_id,
            "projectId": project_id,
            "surface": "runtime",
            "substrate": keyword_scalar(form, ":substrate"),
            "deployCenterSlug": keyword_scalar(form, ":dc_slug"),
            "runtimeTarget": keyword_scalar(form, ":runtime-target"),
            "executor": keyword_scalar(form, ":executor"),
            "container": keyword_scalar(form, ":container"),
            "hostBind": keyword_scalar(form, ":host-bind").or_else(|| keyword_scalar(form, ":local-bind")),
            "proxy": keyword_scalar(form, ":proxy"),
            "artifactLane": keyword_scalar(form, ":artifact-delivery-lane"),
            "imageEnv": keyword_scalar(form, ":image-env"),
            "authority": keyword_scalar(form, ":authority"),
        }));
    }
    if let Some(form) = frontend_deployment_form {
        channels.push(json!({
            "id": format!("{service_id}:frontend"),
            "serviceId": service_id,
            "projectId": project_id,
            "surface": "frontend",
            "substrate": keyword_scalar(form, ":substrate"),
            "project": keyword_scalar(form, ":project"),
            "rootDirectory": keyword_scalar(form, ":root-directory"),
            "productionDomain": keyword_scalar(form, ":production-domain"),
            "fallbackDomain": keyword_scalar(form, ":fallback-domain"),
            "authority": keyword_scalar(form, ":authority").unwrap_or_else(|| "vercel".to_string()),
        }));
    }
    channels
}

fn compiled_snapshot_to_value(snapshot: &CompiledRuntimeSnapshot) -> Value {
    json!({
        "kind": snapshot.kind,
        "path": snapshot.path.display().to_string(),
        "schemaVersion": snapshot.schema_version,
        "sourceHash": snapshot.source_hash,
    })
}

fn locate_service_runtime_shard() -> Result<PathBuf> {
    let root = crate::helpers::missiond_blueprint_path()
        .and_then(|path| path.parent().map(PathBuf::from))
        .ok_or_else(|| anyhow!("MissionD V3 blueprint not found"))?;
    let shard = root
        .join("shards")
        .join("universe")
        .join("service-runtime.lisp");
    if !shard.exists() {
        return Err(anyhow!("MissionD service-runtime shard not found"));
    }
    Ok(shard)
}

fn extract_forms(source: &str, marker: &str) -> Vec<String> {
    let mut forms = Vec::new();
    let mut offset = 0usize;
    while let Some(idx) = source[offset..].find(marker) {
        let start = offset + idx;
        if let Some(form) = extract_balanced_from(source, start) {
            offset = start + form.len();
            forms.push(form);
        } else {
            break;
        }
    }
    forms
}

fn extract_balanced_after(source: &str, marker: &str) -> Option<String> {
    source
        .find(marker)
        .and_then(|start| extract_balanced_from(source, start))
}

fn extract_balanced_from(source: &str, start: usize) -> Option<String> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (rel, ch) in source[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let end = start + rel + ch.len_utf8();
                    return Some(source[start..end].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn keyword_scalar(source: &str, key: &str) -> Option<String> {
    let tail = source.split_once(key)?.1.trim_start();
    read_value(tail).map(|(value, _)| value)
}

fn keyword_list(source: &str, key: &str) -> Vec<String> {
    let Some(tail) = source.split_once(key).map(|(_, t)| t.trim_start()) else {
        return Vec::new();
    };
    if !tail.starts_with('[') {
        return keyword_scalar(source, key).into_iter().collect();
    }
    let Some(end) = tail.find(']') else {
        return Vec::new();
    };
    tokenize_atoms(&tail[1..end])
}

fn keyword_form(source: &str, key: &str) -> Option<String> {
    let tail = source.split_once(key)?.1.trim_start();
    if !tail.starts_with('(') {
        return None;
    }
    extract_balanced_from(tail, 0)
}

fn read_value(source: &str) -> Option<(String, usize)> {
    if source.starts_with('"') {
        let mut escaped = false;
        for (idx, ch) in source[1..].char_indices() {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                let end = idx + 2;
                return Some((source[1..idx + 1].to_string(), end));
            }
        }
        return None;
    }
    let end = source
        .find(|c: char| c.is_whitespace() || c == ')' || c == ']' || c == '[')
        .unwrap_or(source.len());
    let value = source[..end].trim();
    (!value.is_empty()).then(|| (value.to_string(), end))
}

fn tokenize_atoms(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = source.trim();
    while !rest.is_empty() {
        if let Some((value, consumed)) = read_value(rest) {
            out.push(value);
            rest = rest[consumed..].trim_start();
        } else {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_service_output_preserves_runtime_support_catalog() {
        let service = CompiledServiceRuntimeEntry {
            id: Some("asr".to_string()),
            project: Some("asr".to_string()),
            root: Some("/repo/services/asr".to_string()),
            intent: Some(".missiond/intent.lisp".to_string()),
            backend: Some(".missiond/backend/asr-backend-blueprint.lisp".to_string()),
            frontend: Some(".missiond/frontend/asr-web-blueprint.lisp".to_string()),
            operations: Some(".missiond/operations/asr-operations-blueprint.lisp".to_string()),
            environment: Some("production".to_string()),
            public_base_url: Some("https://speechscribe.top".to_string()),
            frontend_url: Some("https://speechscribe.top".to_string()),
            api_base_url: Some("https://asr.xiaojins.com".to_string()),
            domains: vec!["speechscribe.top".to_string()],
            health: vec!["/health/ready".to_string()],
            dependencies: vec!["payments".to_string()],
            ops_capability: Some("deploy-ops".to_string()),
            surface: Some("service-runtime-universe".to_string()),
            deployment: Some(json!({
                "substrate": "deploy-center",
                "dc_slug": "xjp-asr",
                "runtime_target": "gcp-runtime",
                "executor": "gcp-agent",
                "container": "xjp-asr",
            })),
            frontend_deployment: Some(json!({
                "substrate": "vercel",
                "project": "rickyjim626s-projects/xjp-asr-web",
                "production_domain": "speechscribe.top",
            })),
            build_lane: Some(json!({
                "id": "privatecloud-rust-build-lane",
                "builder": "privatecloud-10900kf",
                "executor": "privatecloud-agent",
                "artifact_lane": "cloud-registry-lane",
            })),
            deployment_channels: vec![json!({
                "id": "asr:build",
                "surface": "build",
                "substrate": "privatecloud-rust-build-lane",
            })],
            support_catalog: Some(CompiledServiceSupportCatalog {
                service_id: Some("asr".to_string()),
                project_id: Some("asr".to_string()),
                domains: vec!["speechscribe.top".to_string()],
                public_base_url: Some("https://speechscribe.top".to_string()),
                frontend_url: Some("https://speechscribe.top".to_string()),
                api_base_url: Some("https://asr.xiaojins.com".to_string()),
                health: vec!["/health/ready".to_string()],
                dependencies: vec!["payments".to_string()],
                deploy_center_slug: Some("xjp-asr".to_string()),
                runtime_target: Some("gcp-runtime".to_string()),
                executor: Some("gcp-agent".to_string()),
                container: Some("xjp-asr".to_string()),
                service_manifest_refs: vec!["services/asr/service.manifest.toml".to_string()],
                credential_refs: Vec::new(),
                source_evidence: vec!["skill:services/asr".to_string()],
                db_migration_namespace: None,
                database_namespace: None,
            }),
        };

        let value = compiled_service_to_value(&service);

        assert_eq!(value["id"], "asr");
        assert_eq!(value["sourceEvidence"][0], "skill:services/asr");
        assert_eq!(value["deployment"]["substrate"], "deploy-center");
        assert_eq!(value["deployment"]["dcSlug"], "xjp-asr");
        assert_eq!(value["deployment"]["runtimeTarget"], "gcp-runtime");
        assert_eq!(value["deployment"]["container"], "xjp-asr");
        assert_eq!(value["frontendDeployment"]["substrate"], "vercel");
        assert_eq!(value["buildLane"]["builder"], "privatecloud-10900kf");
        assert_eq!(value["deploymentChannels"][0]["surface"], "build");
        assert!(compiled_service_matches(&service, Some("asr")));
        assert!(compiled_service_matches(&service, Some("xjp-asr")));
        assert!(!compiled_service_matches(&service, Some("payments")));
    }
}
