use super::*;

pub(in crate::handlers::knowledge::workflow) fn resolve_methodology_path(
    project_root: &Path,
    name: Option<&str>,
    workflow_path: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(p) = workflow_path.filter(|s| !s.is_empty()) {
        let candidate = PathBuf::from(p);
        return Ok(if candidate.is_absolute() {
            candidate
        } else {
            project_root.join(candidate)
        });
    }
    if let Some(name) = name.filter(|s| !s.is_empty()) {
        let mut p = project_root.join(WORKFLOWS_DIR).join(name);
        if p.extension().is_none() {
            p.set_extension("lisp");
        }
        return Ok(p);
    }
    Err("compile_methodology requires `workflow_path` or `name`".to_string())
}

pub(in crate::handlers::knowledge::workflow) fn validate_methodology_source(
    content: &str,
) -> Result<(), String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err("methodology source is empty".to_string());
    }
    if !paren_balanced_ignoring_strings(content) {
        return Err("methodology source has unbalanced parentheses".to_string());
    }
    if !content.chars().any(|c| c == '(') {
        return Err("methodology source has no top-level form".to_string());
    }
    Ok(())
}

/// SHA-256 hex of the source bytes — stable across runs for identical input.
pub(in crate::handlers::knowledge::workflow) fn source_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for byte in digest {
        s.push_str(&format!("{:02x}", byte));
    }
    s
}

pub(in crate::handlers::knowledge::workflow) fn derive_flow_id(
    stem: &str,
    output_flow_id: Option<&str>,
) -> String {
    if let Some(explicit) = output_flow_id.filter(|s| !s.is_empty()) {
        return explicit.to_string();
    }
    let safe = sanitize_id_token(stem);
    if safe.is_empty() {
        "methodology-anonymous-v0".to_string()
    } else {
        format!("methodology-{}-v0", safe)
    }
}

pub(in crate::handlers::knowledge::workflow) fn sanitize_id_token(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_hyphen = false;
    for ch in raw.chars() {
        let allowed = ch.is_ascii_alphanumeric() || ch == '_' || ch == '-';
        if allowed {
            out.push(ch);
            prev_hyphen = ch == '-';
        } else if !prev_hyphen && !out.is_empty() {
            out.push('-');
            prev_hyphen = true;
        }
    }
    out.trim_matches('-').to_string()
}

pub(in crate::handlers::knowledge::workflow) fn source_path_for_yaml(
    project_root: &Path,
    path: &Path,
) -> String {
    match path.strip_prefix(project_root) {
        Ok(rel) => rel.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

pub(in crate::handlers::knowledge::workflow) fn generated_yaml_path(
    project_root: &Path,
    flow_id: &str,
) -> PathBuf {
    project_root
        .join(GENERATED_FLOWS_DIR)
        .join(format!("{}.yaml", flow_id))
}

pub(in crate::handlers::knowledge::workflow) fn resolve_compiled_flow(
    project_root: &Path,
    flow_id: Option<&str>,
    flow_path: Option<&str>,
    name: Option<&str>,
) -> Result<CompiledFlow, CompiledFlowError> {
    if let Some(p) = flow_path.filter(|s| !s.is_empty()) {
        let candidate = PathBuf::from(p);
        let resolved = if candidate.is_absolute() {
            candidate
        } else {
            project_root.join(candidate)
        };
        if resolved.exists() {
            return Ok(CompiledFlow { path: resolved });
        }
        let id_for_msg = flow_id.map(|s| s.to_string()).unwrap_or_else(|| {
            resolved
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string()
        });
        return Err(CompiledFlowError::Missing {
            flow_id: id_for_msg,
            expected: resolved,
        });
    }

    let id = if let Some(id) = flow_id.filter(|s| !s.is_empty()) {
        id.to_string()
    } else if let Some(n) = name.filter(|s| !s.is_empty()) {
        derive_flow_id(n, None)
    } else {
        return Err(CompiledFlowError::MissingArgs);
    };
    let expected = generated_yaml_path(project_root, &id);
    if expected.exists() {
        Ok(CompiledFlow { path: expected })
    } else {
        Err(CompiledFlowError::Missing {
            flow_id: id,
            expected,
        })
    }
}
