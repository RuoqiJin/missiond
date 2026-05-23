use std::path::{Path, PathBuf};

const COMPILED_RUNTIME_DIR: [&str; 4] = [".missiond", "v3", "runtime", "compiled"];

pub(super) fn compiled_runtime_snapshot_path(project_root: &Path, kind: &str) -> Option<PathBuf> {
    let file_name = compiled_runtime_file_name(kind)?;
    let mut path = project_root.to_path_buf();
    for segment in COMPILED_RUNTIME_DIR {
        path.push(segment);
    }
    path.push(file_name);
    Some(path)
}

pub(super) fn compiled_runtime_file_name(kind: &str) -> Option<&'static str> {
    match kind {
        "v3" => Some("compiled-v3-blueprint.json"),
        "runtime-config" => Some("compiled-runtime-config.json"),
        "universe" => Some("compiled-project-universe.json"),
        "workflows" => Some("compiled-workflows.json"),
        _ => None,
    }
}

pub(super) fn compiled_runtime_schema_version(kind: &str) -> Option<&'static str> {
    match kind {
        "v3" => Some("missiond.compiled-v3-blueprint.v1"),
        "runtime-config" => Some("missiond.compiled-runtime-config.v1"),
        "universe" => Some("missiond.compiled-project-universe.v1"),
        "workflows" => Some("missiond.compiled-workflows.v1"),
        _ => None,
    }
}
