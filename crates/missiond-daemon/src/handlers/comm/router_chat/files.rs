use std::path::Path;

/// Denied path patterns for file attachments (security denylist).
/// Single-user local service: block known sensitive paths instead of whitelist.
pub(super) const FILE_DENY_PATTERNS: &[&str] = &[
    "/.ssh/",
    "/.aws/",
    "/.gnupg/",
    "/.kube/",
    "/.docker/config",
    "/.netrc",
];

/// Denied file names (exact match on filename component).
pub(super) const FILE_DENY_NAMES: &[&str] = &[
    ".env",
    ".env.local",
    ".env.production",
    ".env.development",
    "credentials.json",
    "service-account.json",
    "id_rsa",
    "id_ed25519",
];

/// Max text file size (1 MB). Larger files are auto-truncated, not rejected.
pub(super) const FILE_MAX_SIZE_TEXT: u64 = 1024 * 1024;

/// Max binary file size (10 MB) for File API uploads.
pub(super) const FILE_MAX_SIZE_BINARY: u64 = 10 * 1024 * 1024;

/// Resolve Gemini API key from llm.yaml (for multimodal File API).
pub(super) fn resolve_gemini_api_key() -> Option<String> {
    let llm_yaml = missiond_core::default_mission_home().join("llm.yaml");
    if !llm_yaml.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&llm_yaml).ok()?;
    let config: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;
    config
        .get("gemini_api_key")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Check if a file path matches the security denylist.
pub(super) fn is_file_denied(path: &Path) -> Option<&'static str> {
    let path_str = path.to_string_lossy();
    for pattern in FILE_DENY_PATTERNS {
        if path_str.contains(pattern) {
            return Some(pattern);
        }
    }
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        for denied in FILE_DENY_NAMES {
            if name == *denied {
                return Some(denied);
            }
        }
    }
    None
}
