use std::path::PathBuf;

pub(super) fn resolve_manifest_path(args_path: Option<&str>) -> Result<PathBuf, String> {
    let default = std::env::var("UNIVERSE_MANIFEST")
        .unwrap_or_else(|_| "/Users/jinchen/Projects/universe.intent.lisp".into());
    let raw = args_path.unwrap_or(&default);
    let path = PathBuf::from(raw);

    let canonical = path
        .canonicalize()
        .map_err(|e| format!("manifest path not found: {}: {}", raw, e))?;

    let allowed_root = std::env::var("UNIVERSE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/Users/jinchen/Projects"));

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
