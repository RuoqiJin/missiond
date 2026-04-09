use std::path::PathBuf;

// Re-export CliEngine from standalone semantic-terminal crate
pub use semantic_terminal::CliEngine;

// ============ Mission Home ============

/// Get default mission home directory.
///
/// Resolution order:
/// 1. `MISSIOND_HOME` env var (if set)
/// 2. `XJP_MISSION_HOME` env var (legacy compat)
/// 3. `~/.missiond` (if exists)
/// 4. `~/.xjp-mission` (if exists, legacy compat)
/// 5. `~/.missiond` (default for new installs)
pub fn default_mission_home() -> PathBuf {
    // Explicit env vars take priority
    if let Ok(home) = std::env::var("MISSIOND_HOME") {
        return PathBuf::from(home);
    }
    if let Ok(home) = std::env::var("XJP_MISSION_HOME") {
        return PathBuf::from(home);
    }
    // Auto-detect existing directory
    if let Some(home) = dirs::home_dir() {
        let new_path = home.join(".missiond");
        let legacy_path = home.join(".xjp-mission");
        if new_path.exists() {
            return new_path;
        }
        if legacy_path.exists() {
            return legacy_path;
        }
        return new_path; // default for new installs
    }
    PathBuf::from(".missiond")
}
