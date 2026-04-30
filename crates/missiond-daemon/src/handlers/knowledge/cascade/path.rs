use std::path::PathBuf;

use crate::context::v3_blueprint_runtime::CascadeRuntimeConfig;

pub(super) fn resolve_manifest_path(args_path: Option<&str>) -> Result<PathBuf, String> {
    let config = CascadeRuntimeConfig::load_for_current_dir()
        .map_err(|err| format!("V3_BLUEPRINT_CONFIG_ERROR: {err}"))?;
    let default_manifest = config.env_or_default_manifest_path();
    let path = args_path.map(PathBuf::from).unwrap_or(default_manifest);
    let raw = path.to_string_lossy().to_string();

    let canonical = path
        .canonicalize()
        .map_err(|e| format!("manifest path not found: {}: {}", raw, e))?;

    let allowed_root = config.env_or_allowed_root();

    if let Ok(root) = allowed_root.canonicalize() {
        if !canonical.starts_with(&root) {
            return Err(format!(
                "manifestPath '{}' is outside allowed root '{}'",
                canonical.display(),
                root.display()
            ));
        }
    }

    Ok(canonical)
}
