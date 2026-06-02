use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};
use std::path::PathBuf;

#[cfg(test)]
use crate::context::v3_blueprint_runtime::CompiledServiceSupportCatalog;
use crate::context::v3_blueprint_runtime::{
    load_compiled_project_universe, CompiledProjectUniverse, CompiledRuntimeSnapshot,
    CompiledServiceRuntimeEntry,
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
        let proxy_form = keyword_form(&form, ":proxy");
        let ports_form = keyword_form(&form, ":ports");
        let event_ingest_form = keyword_form(&form, ":event-ingest");
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
    }))
}

fn compiled_service_matches(
    service: &CompiledServiceRuntimeEntry,
    filter_id: Option<&str>,
) -> bool {
    let Some(filter_id) = filter_id else {
        return true;
    };
    [service.id.as_deref(), service.project.as_deref()]
        .into_iter()
        .flatten()
        .any(|value| value == filter_id)
}

fn compiled_service_to_value(service: &CompiledServiceRuntimeEntry) -> Value {
    let support = service.support_catalog.as_ref();
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
        "deployment": {
            "substrate": "deploy-center",
            "dcSlug": support.and_then(|support| support.deploy_center_slug.as_deref()),
            "runtimeTarget": support.and_then(|support| support.runtime_target.as_deref()),
            "executor": support.and_then(|support| support.executor.as_deref()),
            "container": support.and_then(|support| support.container.as_deref()),
        },
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
            api_base_url: Some("https://auth.xiaojinpro.com/asr".to_string()),
            domains: vec!["speechscribe.top".to_string()],
            health: vec!["/health/ready".to_string()],
            dependencies: vec!["payments".to_string()],
            ops_capability: Some("deploy-ops".to_string()),
            surface: Some("service-runtime-universe".to_string()),
            support_catalog: Some(CompiledServiceSupportCatalog {
                service_id: Some("asr".to_string()),
                project_id: Some("asr".to_string()),
                domains: vec!["speechscribe.top".to_string()],
                public_base_url: Some("https://speechscribe.top".to_string()),
                frontend_url: Some("https://speechscribe.top".to_string()),
                api_base_url: Some("https://auth.xiaojinpro.com/asr".to_string()),
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
        assert!(compiled_service_matches(&service, Some("asr")));
        assert!(!compiled_service_matches(&service, Some("payments")));
    }
}
