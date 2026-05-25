use std::path::{Path, PathBuf};

const COMPILED_RUNTIME_DIR: [&str; 4] = [".missiond", "v3", "runtime", "compiled"];

pub(super) fn compiled_runtime_snapshot_path(project_root: &Path, kind: &str) -> Option<PathBuf> {
    let file_name = compiled_runtime_file_name(kind)?;
    let mut path = compiled_runtime_dir(project_root);
    path.push(file_name);
    Some(path)
}

pub(super) fn compiled_runtime_dir(project_root: &Path) -> PathBuf {
    if let Ok(dir) = std::env::var("MISSIOND_COMPILED_RUNTIME_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    if let Ok(dir) = std::env::var("MISSIOND_RUNTIME_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir).join("compiled");
        }
    }
    let mut path = project_root.to_path_buf();
    for segment in COMPILED_RUNTIME_DIR {
        path.push(segment);
    }
    path
}

pub(super) fn compiled_runtime_file_name(kind: &str) -> Option<&'static str> {
    match kind {
        "v3" => Some("compiled-v3-blueprint.json"),
        "runtime-config" => Some("compiled-runtime-config.json"),
        "runtime-domain:workstation" => Some("compiled-runtime-workstation.json"),
        "runtime-domain:flow" => Some("compiled-runtime-flow.json"),
        "runtime-domain:compute" => Some("compiled-runtime-compute.json"),
        "runtime-domain:minimax" => Some("compiled-runtime-minimax.json"),
        "runtime-domain:router" => Some("compiled-runtime-router.json"),
        "runtime-domain:cascade" => Some("compiled-runtime-cascade.json"),
        "runtime-domain:project-registry" => Some("compiled-runtime-project-registry.json"),
        "runtime-domain:capability-governance" => {
            Some("compiled-runtime-capability-governance.json")
        }
        "runtime-domain:memory-kb" => Some("compiled-runtime-memory-kb.json"),
        "runtime-domain:conversation-ingestion" => {
            Some("compiled-runtime-conversation-ingestion.json")
        }
        "runtime-domain:autopilot" => Some("compiled-runtime-autopilot.json"),
        "runtime-domain:learning-engine" => Some("compiled-runtime-learning-engine.json"),
        "universe" => Some("compiled-project-universe.json"),
        "workflows" => Some("compiled-workflows.json"),
        _ => None,
    }
}

pub(super) fn compiled_runtime_schema_version(kind: &str) -> Option<&'static str> {
    match kind {
        "v3" => Some("missiond.compiled-v3-blueprint.v1"),
        "runtime-config" => Some("missiond.compiled-runtime-config.v1"),
        kind if kind.starts_with("runtime-domain:") => Some("missiond.compiled-runtime-domain.v1"),
        "universe" => Some("missiond.compiled-project-universe.v1"),
        "workflows" => Some("missiond.compiled-workflows.v1"),
        _ => None,
    }
}
