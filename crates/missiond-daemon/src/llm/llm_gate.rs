//! LLM Gate — unified file-based kill switch for all model providers.
//!
//! Each model has a flag file at `$MISSIOND_HOME/{model}_disabled`.
//! When the file exists, all calls to that model are immediately rejected.
//!
//! Supported models: gemini, sonnet, codex.
//! Check cost: single `Path::exists()` syscall per request (< 1µs).

use std::path::PathBuf;
use tracing::{info, warn};

/// Resolve the MissionD home directory (shared with codex_cli.rs logic).
fn missiond_home() -> PathBuf {
    let home = std::env::var("MISSIOND_HOME")
        .or_else(|_| std::env::var("XJP_MISSION_HOME"))
        .unwrap_or_else(|_| {
            let h = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            if h.join(".missiond").exists() {
                h.join(".missiond").to_string_lossy().to_string()
            } else {
                h.join(".xjp-mission").to_string_lossy().to_string()
            }
        });
    PathBuf::from(home)
}

/// Flag file path for a given model.
fn flag_path(model: &str) -> PathBuf {
    missiond_home().join(format!("{}_disabled", model))
}

/// Check if a model is disabled via persistent flag file.
pub(crate) fn is_disabled(model: &str) -> bool {
    flag_path(model).exists()
}

/// Set or clear the disabled flag for a model.
pub(crate) fn set_disabled(model: &str, disabled: bool) {
    let path = flag_path(model);
    if disabled {
        let _ = std::fs::write(&path, chrono::Utc::now().to_rfc3339());
        warn!(model, "LLM gate: disabled — flag file created at {:?}", path);
    } else {
        let _ = std::fs::remove_file(&path);
        info!(model, "LLM gate: enabled — flag file removed");
    }
}

/// Check all models and return their gate status.
pub(crate) fn all_status() -> Vec<(&'static str, bool)> {
    vec![
        ("gemini", is_disabled("gemini")),
        ("sonnet", is_disabled("sonnet")),
        ("codex", is_disabled("codex")),
    ]
}
