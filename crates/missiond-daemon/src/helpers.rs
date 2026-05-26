use std::path::{Path, PathBuf};

use tracing::warn;

pub(crate) fn default_mission_home() -> PathBuf {
    missiond_core::ipc::default_mission_home()
}

pub(crate) fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var(var)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn nearest_project_root_with_blueprint(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|candidate| {
            candidate
                .join(".missiond")
                .join("v3")
                .join("missiond-blueprint.lisp")
                .exists()
        })
        .map(Path::to_path_buf)
}

pub(crate) fn missiond_project_root() -> PathBuf {
    if let Some(root) =
        env_path("MISSIOND_PROJECT_ROOT").or_else(|| env_path("MISSIOND_ORCHESTRATOR_ROOT"))
    {
        return root;
    }
    if let Ok(cwd) = std::env::current_dir() {
        return nearest_project_root_with_blueprint(&cwd).unwrap_or(cwd);
    }
    default_mission_home()
}

pub(crate) fn missiond_blueprint_path() -> Option<PathBuf> {
    let from_root = |root: PathBuf| {
        root.join(".missiond")
            .join("v3")
            .join("missiond-blueprint.lisp")
    };
    for env in ["MISSIOND_PROJECT_ROOT", "MISSIOND_ORCHESTRATOR_ROOT"] {
        if let Some(root) = env_path(env) {
            let candidate = from_root(root);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    std::env::current_dir()
        .ok()
        .and_then(|cwd| nearest_project_root_with_blueprint(&cwd))
        .map(from_root)
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

pub(crate) fn slots_config_path() -> PathBuf {
    env_path("MISSION_SLOTS_CONFIG").unwrap_or_else(|| default_mission_home().join("slots.yaml"))
}

/// PostgreSQL connection URL (e.g. `postgres://user:pass@host:5432/missiond`).
/// Required: PgMissionStore is the only MissionD runtime database backend.
pub(crate) fn pg_url() -> Option<String> {
    std::env::var("MISSION_PG_URL")
        .ok()
        .filter(|s| !s.is_empty())
}

pub(crate) fn ws_port() -> u16 {
    std::env::var("MISSION_WS_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(9120)
}

pub(crate) fn logs_dir(home: &Path) -> PathBuf {
    home.join("logs")
}

pub(crate) fn permission_config_path(home: &Path) -> PathBuf {
    home.join("config").join("permissions.yaml")
}

pub(crate) fn learned_permissions_path(home: &Path) -> PathBuf {
    home.join("config").join("learned_permissions.yaml")
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
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).ok();
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
