use std::path::{Path, PathBuf};

use tracing::warn;

pub(crate) fn default_mission_home() -> PathBuf {
    missiond_core::ipc::default_mission_home()
}

pub(crate) fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var(var).ok().map(PathBuf::from)
}

pub(crate) fn ipc_endpoint_from_env() -> String {
    if let Ok(endpoint) = std::env::var("MISSION_IPC_ENDPOINT") {
        return endpoint;
    }
    // Legacy support for MISSION_IPC_SOCKET on Unix
    #[cfg(unix)]
    if let Ok(socket) = std::env::var("MISSION_IPC_SOCKET") {
        return socket;
    }
    missiond_core::ipc::default_ipc_endpoint()
}

pub(crate) fn db_path() -> PathBuf {
    env_path("MISSION_DB_PATH").unwrap_or_else(|| default_mission_home().join("mission.db"))
}

pub(crate) fn slots_config_path() -> PathBuf {
    env_path("MISSION_SLOTS_CONFIG").unwrap_or_else(|| default_mission_home().join("slots.yaml"))
}

pub(crate) fn ws_port() -> u16 {
    std::env::var("MISSION_WS_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(9120)
}

pub(crate) fn logs_dir(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("logs")
}

/// Ensure sensitive config files have restrictive permissions (owner-only read/write).
/// Automatically fixes permissions and logs a warning if they're too open.
#[cfg(unix)]
pub(crate) fn ensure_config_permissions(home: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let files = [
        "servers.yaml",
        "slots.yaml",
        "mcp-config.json",
        "xjp-mcp-config.json", // legacy name
        "config/permissions.yaml",
        "mission.db",
    ];

    for name in &files {
        let path = home.join(name);
        if !path.exists() {
            continue;
        }
        if let Ok(meta) = std::fs::metadata(&path) {
            let mode = meta.permissions().mode();
            if mode & 0o077 != 0 {
                warn!(
                    file = %path.display(),
                    old_mode = format!("{:o}", mode),
                    "Config file too permissive, fixing to 600"
                );
                std::fs::set_permissions(
                    &path,
                    std::fs::Permissions::from_mode(0o600),
                )
                .ok();
            }
        }
    }
}

pub(crate) fn log_filter() -> tracing_subscriber::EnvFilter {
    let level = if let Ok(v) = std::env::var("RUST_LOG") {
        v
    } else if let Ok(v) = std::env::var("MISSION_LOG_LEVEL") {
        match v.as_str() {
            "silent" => "off".to_string(),
            "fatal" => "error".to_string(),
            other => other.to_string(),
        }
    } else {
        // Daemon default: info (need visibility into PTY spawns, IPC, etc.)
        "info".to_string()
    };

    tracing_subscriber::EnvFilter::try_new(level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"))
}

pub(crate) fn char_boundary_at(s: &str, max: usize) -> usize {
    missiond_core::embedding::char_boundary_at(s, max)
}
