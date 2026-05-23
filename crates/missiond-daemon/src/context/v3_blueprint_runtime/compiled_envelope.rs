use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct CompiledRuntimeEnvelope {
    pub(super) schema_version: String,
    pub(super) source_hash: String,
    #[allow(dead_code)]
    pub(super) generated_at: Option<serde_json::Value>,
    pub(super) diagnostics: Vec<serde_json::Value>,
    pub(super) payload: serde_json::Value,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
pub(super) struct CompiledSourceUnit {
    pub(super) file: String,
    pub(super) kind: String,
    pub(super) included_by: Option<String>,
    pub(super) include_line: Option<i64>,
    pub(super) source_hash: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
pub(super) struct CompiledSourceDomain {
    pub(super) id: String,
    pub(super) source_hash: String,
    #[serde(default)]
    pub(super) source_units: Vec<CompiledSourceUnit>,
}

pub(super) fn validate_compiled_source_units(
    project_root: &Path,
    expected_composite_hash: &str,
    source_units: &[CompiledSourceUnit],
    label: &str,
    compile_runtime_action: &str,
) -> Vec<String> {
    if source_units.is_empty() {
        return vec![format!(
            "{label} missing source_units; rerun {compile_runtime_action}"
        )];
    }

    let mut diagnostics = Vec::new();
    let mut unit_hashes = Vec::with_capacity(source_units.len());
    for unit in source_units {
        if unit.file.trim().is_empty() {
            diagnostics.push(format!("{label} contains a source_unit with an empty file"));
            continue;
        }
        let path = compiled_source_unit_path(project_root, &unit.file);
        let actual_hash = match std::fs::read(&path) {
            Ok(raw) => md5_hex(&raw),
            Err(err) => {
                diagnostics.push(format!(
                    "{label} source_units reference unreadable source {}: {err}",
                    path.display()
                ));
                continue;
            }
        };
        if actual_hash != unit.source_hash {
            diagnostics.push(format!(
                "{label} source_units stale for {}: expected {}, got {}",
                unit.file, unit.source_hash, actual_hash
            ));
        }
        unit_hashes.push(unit.source_hash.clone());
    }

    if diagnostics.is_empty() {
        let actual_composite_hash = md5_hex(unit_hashes.join("\n").as_bytes());
        if actual_composite_hash != expected_composite_hash {
            diagnostics.push(format!(
                "{label} source_hash mismatch from source_units: expected {}, got {}",
                expected_composite_hash, actual_composite_hash
            ));
        }
    }
    diagnostics
}

pub(super) fn validate_compiled_source_domains(
    project_root: &Path,
    source_domains: &[CompiledSourceDomain],
    label: &str,
    compile_runtime_action: &str,
) -> Vec<String> {
    if source_domains.is_empty() {
        return vec![format!(
            "{label} missing source_domains; rerun {compile_runtime_action}"
        )];
    }

    let mut diagnostics = Vec::new();
    for domain in source_domains {
        if domain.id.trim().is_empty() {
            diagnostics.push(format!("{label} contains a source_domain with an empty id"));
            continue;
        }
        if domain.source_units.is_empty() {
            let empty_hash = md5_hex("".as_bytes());
            if domain.source_hash != empty_hash {
                diagnostics.push(format!(
                    "{label} source domain {} source_hash mismatch from empty source_units: expected {}, got {}",
                    domain.id, domain.source_hash, empty_hash
                ));
            }
            continue;
        }
        let domain_label = format!("{label} source domain {}", domain.id);
        diagnostics.extend(validate_compiled_source_units(
            project_root,
            &domain.source_hash,
            &domain.source_units,
            &domain_label,
            compile_runtime_action,
        ));
    }
    diagnostics
}

pub(super) fn source_domain_bundle_hash(source_domains: &[CompiledSourceDomain]) -> String {
    md5_hex(
        source_domains
            .iter()
            .map(|domain| domain.source_hash.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .as_bytes(),
    )
}

fn compiled_source_unit_path(project_root: &Path, file: &str) -> PathBuf {
    let path = Path::new(file);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    }
}

pub(super) fn md5_hex(bytes: &[u8]) -> String {
    format!("{:x}", Md5::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn source_domain_validation_accepts_empty_domain_hash() {
        let empty_hash = md5_hex("".as_bytes());
        let domains = vec![CompiledSourceDomain {
            id: "implementation-map".to_string(),
            source_hash: empty_hash,
            source_units: Vec::new(),
        }];

        assert!(validate_compiled_source_domains(
            Path::new("."),
            &domains,
            "compiled runtime",
            "compile-v3-runtime"
        )
        .is_empty());
    }

    #[test]
    fn source_domain_validation_rejects_bad_empty_domain_hash() {
        let domains = vec![CompiledSourceDomain {
            id: "implementation-map".to_string(),
            source_hash: "not-empty-md5".to_string(),
            source_units: Vec::new(),
        }];

        let diagnostics = validate_compiled_source_domains(
            Path::new("."),
            &domains,
            "compiled runtime",
            "compile-v3-runtime",
        );

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("source_hash mismatch from empty source_units"));
    }

    #[test]
    fn source_domain_validation_is_scoped_to_domain_units() {
        let dir = tempdir().expect("tempdir");
        let runtime_file = dir.path().join("runtime.lisp");
        let unrelated_file = dir.path().join("implementation.lisp");
        std::fs::write(&runtime_file, "(runtime-config)").expect("write runtime file");
        std::fs::write(&unrelated_file, "(implementation-map)").expect("write unrelated file");

        let unit_hash = md5_hex(b"(runtime-config)");
        let domain_hash = md5_hex(unit_hash.as_bytes());
        let domains = vec![CompiledSourceDomain {
            id: "workstation-runtime".to_string(),
            source_hash: domain_hash.clone(),
            source_units: vec![CompiledSourceUnit {
                file: "runtime.lisp".to_string(),
                kind: "root".to_string(),
                included_by: None,
                include_line: None,
                source_hash: unit_hash,
            }],
        }];

        assert!(validate_compiled_source_domains(
            dir.path(),
            &domains,
            "compiled runtime",
            "compile-v3-runtime",
        )
        .is_empty());

        std::fs::write(&unrelated_file, "(implementation-map changed)")
            .expect("rewrite unrelated file");

        assert!(validate_compiled_source_domains(
            dir.path(),
            &domains,
            "compiled runtime",
            "compile-v3-runtime",
        )
        .is_empty());
        assert_eq!(
            source_domain_bundle_hash(&domains),
            md5_hex(domain_hash.as_bytes())
        );
    }
}
