use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(super) async fn handle_universe(args: Value) -> Result<ToolResult> {
    let filter_id = args.get("id").and_then(|v| v.as_str()).map(str::to_string);
    let blueprint_path = locate_v3_blueprint()?;
    let source = std::fs::read_to_string(&blueprint_path)
        .map_err(|e| anyhow!("Failed to read {}: {}", blueprint_path.display(), e))?;
    let Some(block) = extract_balanced_after(&source, "(service-runtime-universe") else {
        return Ok(ToolResult::error(
            "service-runtime-universe not found in V3 blueprint",
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
        "source": blueprint_path.display().to_string(),
        "services": services,
        "capabilities": capabilities,
    })))
}

fn locate_v3_blueprint() -> Result<PathBuf> {
    let cwd = std::env::current_dir().map_err(|e| anyhow!("current_dir failed: {}", e))?;
    for ancestor in cwd.ancestors() {
        let candidate = ancestor.join(".missiond/v3/missiond-blueprint.lisp");
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    let fallback =
        Path::new("/Users/jinchen/Projects/missiond/.missiond/v3/missiond-blueprint.lisp");
    if fallback.exists() {
        return Ok(fallback.to_path_buf());
    }
    Err(anyhow!("MissionD V3 blueprint not found"))
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
