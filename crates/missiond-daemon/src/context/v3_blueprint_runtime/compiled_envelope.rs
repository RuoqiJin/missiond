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
