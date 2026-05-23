use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::compiled_envelope::CompiledRuntimeEnvelope;
use super::compiled_snapshot::{compiled_runtime_schema_version, compiled_runtime_snapshot_path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompiledRuntimeSnapshot {
    pub kind: String,
    pub path: PathBuf,
    pub schema_version: String,
    pub source_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompiledRuntimeLoad {
    pub snapshot: Option<CompiledRuntimeSnapshot>,
    pub diagnostics: Vec<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompiledPayloadLoad<T> {
    pub payload: Option<T>,
    pub snapshot: Option<CompiledRuntimeSnapshot>,
    pub diagnostics: Vec<String>,
}

pub(crate) fn load_compiled_runtime_snapshot(
    project_root: &Path,
    kind: &str,
    expected_source_hash: Option<&str>,
) -> CompiledRuntimeLoad {
    let path = match compiled_runtime_snapshot_path(project_root, kind) {
        Some(path) => path,
        None => {
            return CompiledRuntimeLoad {
                snapshot: None,
                diagnostics: vec![format!("unknown compiled runtime kind `{kind}`")],
            };
        }
    };
    if !path.exists() {
        return CompiledRuntimeLoad {
            snapshot: None,
            diagnostics: vec![format!(
                "compiled runtime snapshot missing: {}",
                path.display()
            )],
        };
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) => {
            return CompiledRuntimeLoad {
                snapshot: None,
                diagnostics: vec![format!(
                    "failed to read compiled runtime snapshot {}: {err}",
                    path.display()
                )],
            };
        }
    };
    let parsed: CompiledRuntimeEnvelope = match serde_json::from_str(&raw) {
        Ok(parsed) => parsed,
        Err(err) => {
            return CompiledRuntimeLoad {
                snapshot: None,
                diagnostics: vec![format!(
                    "failed to parse compiled runtime snapshot {}: {err}",
                    path.display()
                )],
            };
        }
    };
    let mut diagnostics = Vec::new();
    if let Some(expected_schema) = compiled_runtime_schema_version(kind) {
        if parsed.schema_version != expected_schema {
            diagnostics.push(format!(
                "compiled runtime snapshot {} schema_version mismatch: expected {}, got {}",
                path.display(),
                expected_schema,
                parsed.schema_version
            ));
            return CompiledRuntimeLoad {
                snapshot: None,
                diagnostics,
            };
        }
    }
    if !parsed.diagnostics.is_empty() {
        diagnostics.push(format!(
            "compiled runtime snapshot {} contains {} diagnostic(s)",
            path.display(),
            parsed.diagnostics.len()
        ));
    }
    if let Some(expected) = expected_source_hash {
        if parsed.source_hash != expected {
            diagnostics.push(format!(
                "compiled runtime snapshot {} source_hash mismatch: expected {}, got {}",
                path.display(),
                expected,
                parsed.source_hash
            ));
            return CompiledRuntimeLoad {
                snapshot: None,
                diagnostics,
            };
        }
    }
    CompiledRuntimeLoad {
        snapshot: Some(CompiledRuntimeSnapshot {
            kind: kind.to_string(),
            path,
            schema_version: parsed.schema_version,
            source_hash: parsed.source_hash,
        }),
        diagnostics,
    }
}

pub(crate) fn load_compiled_payload<T>(
    project_root: &Path,
    kind: &str,
    expected_source_hash: Option<&str>,
) -> CompiledPayloadLoad<T>
where
    T: for<'de> Deserialize<'de>,
{
    let path = match compiled_runtime_snapshot_path(project_root, kind) {
        Some(path) => path,
        None => {
            return CompiledPayloadLoad {
                payload: None,
                snapshot: None,
                diagnostics: vec![format!("unknown compiled runtime kind `{kind}`")],
            };
        }
    };
    if !path.exists() {
        return CompiledPayloadLoad {
            payload: None,
            snapshot: None,
            diagnostics: vec![format!(
                "compiled runtime snapshot missing: {}",
                path.display()
            )],
        };
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) => {
            return CompiledPayloadLoad {
                payload: None,
                snapshot: None,
                diagnostics: vec![format!(
                    "failed to read compiled runtime snapshot {}: {err}",
                    path.display()
                )],
            };
        }
    };
    let parsed: CompiledRuntimeEnvelope = match serde_json::from_str(&raw) {
        Ok(parsed) => parsed,
        Err(err) => {
            return CompiledPayloadLoad {
                payload: None,
                snapshot: None,
                diagnostics: vec![format!(
                    "failed to parse compiled runtime snapshot {}: {err}",
                    path.display()
                )],
            };
        }
    };
    let mut diagnostics = Vec::new();
    if let Some(expected_schema) = compiled_runtime_schema_version(kind) {
        if parsed.schema_version != expected_schema {
            diagnostics.push(format!(
                "compiled runtime snapshot {} schema_version mismatch: expected {}, got {}",
                path.display(),
                expected_schema,
                parsed.schema_version
            ));
            return CompiledPayloadLoad {
                payload: None,
                snapshot: None,
                diagnostics,
            };
        }
    }
    if !parsed.diagnostics.is_empty() {
        diagnostics.push(format!(
            "compiled runtime snapshot {} contains {} diagnostic(s)",
            path.display(),
            parsed.diagnostics.len()
        ));
    }
    if let Some(expected) = expected_source_hash {
        if parsed.source_hash != expected {
            diagnostics.push(format!(
                "compiled runtime snapshot {} source_hash mismatch: expected {}, got {}",
                path.display(),
                expected,
                parsed.source_hash
            ));
            return CompiledPayloadLoad {
                payload: None,
                snapshot: None,
                diagnostics,
            };
        }
    }
    let payload = match serde_json::from_value(parsed.payload) {
        Ok(payload) => payload,
        Err(err) => {
            diagnostics.push(format!(
                "failed to decode compiled runtime payload {}: {err}",
                path.display()
            ));
            return CompiledPayloadLoad {
                payload: None,
                snapshot: None,
                diagnostics,
            };
        }
    };
    CompiledPayloadLoad {
        payload: Some(payload),
        snapshot: Some(CompiledRuntimeSnapshot {
            kind: kind.to_string(),
            path,
            schema_version: parsed.schema_version,
            source_hash: parsed.source_hash,
        }),
        diagnostics,
    }
}
