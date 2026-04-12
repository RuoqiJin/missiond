use anyhow::Result;
use missiond_mcp::tools::ToolResult;
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::state::AppState;

/// Allowed config files (whitelist for safety)
const ALLOWED_CONFIGS: &[&str] = &["slots.yaml", "llm.yaml", "permissions.yaml"];

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_sys_logs" => sys_logs(args).await,
        "mission_sys_config" => sys_config(state, args).await,
        "mission_daemon_update" => daemon_update(args).await,
        _ => Ok(ToolResult::error(format!("Unknown system tool: {}", name))),
    }
}

// ── sys_config: structured config read/patch ──

async fn sys_config(state: &AppState, args: Value) -> Result<ToolResult> {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("get");

    match action {
        "get" => sys_config_get(args).await,
        "patch" => sys_config_patch(state, args).await,
        "list" => Ok(ToolResult::text(format!(
            "Available config files: {}",
            ALLOWED_CONFIGS.join(", ")
        ))),
        _ => Ok(ToolResult::error(format!(
            "Unknown sys_config action: {}. Use: get, patch, list",
            action
        ))),
    }
}

fn resolve_config_path(file: &str) -> std::result::Result<PathBuf, String> {
    // Strip path components — only allow bare filenames from whitelist
    let basename = Path::new(file)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    if !ALLOWED_CONFIGS.contains(&basename.as_str()) {
        return Err(format!(
            "Config file '{}' not in whitelist. Allowed: {}",
            file,
            ALLOWED_CONFIGS.join(", ")
        ));
    }

    let home = missiond_core::ipc::default_mission_home();
    Ok(home.join(&basename))
}

async fn sys_config_get(args: Value) -> Result<ToolResult> {
    let file = args
        .get("file")
        .and_then(|v| v.as_str())
        .unwrap_or("slots.yaml");

    let path = match resolve_config_path(file) {
        Ok(p) => p,
        Err(e) => return Ok(ToolResult::error(e)),
    };

    let content = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(e) => {
            return Ok(ToolResult::error(format!(
                "Cannot read {}: {}",
                path.display(),
                e
            )))
        }
    };

    // Parse YAML → JSON for structured access
    let yaml_value: serde_yaml::Value = match serde_yaml::from_str(&content) {
        Ok(v) => v,
        Err(e) => return Ok(ToolResult::error(format!("YAML parse error: {}", e))),
    };

    let json_value: Value = serde_json::to_value(&yaml_value).unwrap_or(Value::String(content));

    Ok(ToolResult::json_pretty(&json_value))
}

async fn sys_config_patch(state: &AppState, args: Value) -> Result<ToolResult> {
    let file = match args.get("file").and_then(|v| v.as_str()) {
        Some(f) => f,
        None => return Ok(ToolResult::error("'file' is required for patch")),
    };
    let pointer = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return Ok(ToolResult::error(
                "'path' (JSON Pointer, e.g. /slots/0/description) is required",
            ))
        }
    };
    let new_value = match args.get("value") {
        Some(v) => v.clone(),
        None => return Ok(ToolResult::error("'value' is required for patch")),
    };

    let path = match resolve_config_path(file) {
        Ok(p) => p,
        Err(e) => return Ok(ToolResult::error(e)),
    };

    // Acquire per-file lock to prevent concurrent TOCTOU races
    let file_lock = {
        let mut locks = state.config_file_locks.lock().await;
        locks
            .entry(file.to_string())
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };
    let _guard = file_lock.lock().await;

    // Read current content
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(e) => {
            return Ok(ToolResult::error(format!(
                "Cannot read {}: {}",
                path.display(),
                e
            )))
        }
    };

    // Parse YAML → JSON, apply patch, serialize back
    // NOTE: YAML comments and formatting are lost in round-trip. This is accepted;
    // fsnotify-triggered hot-reload ensures the daemon picks up changes immediately.
    let yaml_value: serde_yaml::Value = match serde_yaml::from_str(&content) {
        Ok(v) => v,
        Err(e) => return Ok(ToolResult::error(format!("YAML parse error: {}", e))),
    };

    let mut json_doc: Value = serde_json::to_value(&yaml_value)
        .map_err(|e| anyhow::anyhow!("YAML→JSON conversion error: {}", e))?;

    // Apply JSON Pointer patch
    let target = match json_doc.pointer_mut(pointer) {
        Some(t) => t,
        None => {
            return Ok(ToolResult::error(format!(
                "JSON Pointer '{}' not found in {}",
                pointer, file
            )))
        }
    };
    *target = new_value.clone();

    // Serialize JSON back to YAML
    let new_yaml = match serde_yaml::to_string(&json_doc) {
        Ok(y) => y,
        Err(e) => {
            return Ok(ToolResult::error(format!(
                "JSON→YAML serialization error: {}",
                e
            )))
        }
    };

    // Write back
    if let Err(e) = tokio::fs::write(&path, &new_yaml).await {
        return Ok(ToolResult::error(format!(
            "Cannot write {}: {}",
            path.display(),
            e
        )));
    }

    // Trigger hot-reload (fsnotify will pick it up, but also signal directly)
    if file.contains("slots") {
        if let Ok(result) = state.mission.reload_slots_config() {
            if result.has_changes() {
                return Ok(ToolResult::text(format!(
                    "Patched {} at '{}' → {:?}. Slots reloaded: {} added, {} removed, {} updated. (⚠️ YAML comments lost in round-trip)",
                    file, pointer, new_value,
                    result.added.len(), result.removed.len(), result.updated.len()
                )));
            }
        }
    }

    Ok(ToolResult::text(format!(
        "Patched {} at '{}' → {:?}. File saved (hot-reload via fsnotify). (⚠️ YAML comments lost in round-trip)",
        file, pointer, new_value
    )))
}

// ── sys_logs: daemon log introspection ──

async fn sys_logs(args: Value) -> Result<ToolResult> {
    let lines = args
        .get("lines")
        .and_then(|v| v.as_u64())
        .unwrap_or(50)
        .min(500) as usize;
    let level = args.get("level").and_then(|v| v.as_str()).unwrap_or("all");
    let grep = args
        .get("grep")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());

    // Find log file in ~/.xjp-mission/logs/missiond.log.*
    let log_dir = std::env::var("MISSIOND_LOG_FILE").unwrap_or_else(|_| {
        let home = missiond_core::ipc::default_mission_home();
        home.join("logs").to_string_lossy().to_string()
    });

    let log_path = match find_latest_log(&log_dir) {
        Some(p) => p,
        None => {
            return Ok(ToolResult::error(format!(
                "No log files found in {}",
                log_dir
            )))
        }
    };

    let content = match tokio::fs::read_to_string(&log_path).await {
        Ok(c) => c,
        Err(e) => {
            return Ok(ToolResult::error(format!(
                "Cannot read log file {}: {}",
                log_path, e
            )))
        }
    };

    let all_lines: Vec<&str> = content.lines().collect();
    let tail_start = all_lines.len().saturating_sub(lines * 3);
    let tail = &all_lines[tail_start..];

    let filtered: Vec<&str> = tail
        .iter()
        .copied()
        .filter(|line| {
            let level_ok = match level {
                "error" => {
                    line.contains("ERROR") || line.contains(" error ") || line.contains("error:")
                }
                "warn" => {
                    line.contains("WARN")
                        || line.contains("ERROR")
                        || line.contains(" warn ")
                        || line.contains(" error ")
                }
                _ => true,
            };
            let grep_ok = match &grep {
                Some(kw) => line.to_lowercase().contains(kw),
                None => true,
            };
            level_ok && grep_ok
        })
        .collect();

    let result: Vec<&str> = filtered
        .into_iter()
        .rev()
        .take(lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    if result.is_empty() {
        Ok(ToolResult::text(format!(
            "No log entries found (file: {}, level: {}, lines: {})",
            log_path, level, lines
        )))
    } else {
        Ok(ToolResult::text(format!(
            "[{} lines from {}]\n{}",
            result.len(),
            log_path,
            result.join("\n")
        )))
    }
}

// ── daemon_update: one-click build + restart ──

/// Launchd service label for MissionD daemon.
const LAUNCHD_LABEL: &str = "com.missiond.daemon";

async fn daemon_update(args: Value) -> Result<ToolResult> {
    use std::process::Command;
    use tracing::info;

    let skip_build = args
        .get("skip_build")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Resolve `binary_dest` from the *currently running* binary path. This is the
    // only correct answer regardless of how the daemon was launched (launchd plist,
    // manual run, fallback script). Older code used `default_mission_home()`, which
    // could pick `~/.missiond` while launchd was actually running `~/.xjp-mission/missiond`,
    // so the cp would silently land at the wrong path and the kickstart would relaunch
    // the stale binary every time.
    let binary_dest = std::env::current_exe().map_err(|e| {
        anyhow::anyhow!("daemon_update: could not resolve current executable path: {}", e)
    })?;

    // Resolve workspace root from compile-time CARGO_MANIFEST_DIR
    // env!() is evaluated at compile time — always correct for this binary.
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    // cargo build output is `missiond` (binary name from Cargo.toml)
    let build_target = project_root.join("target/release/missiond");

    // Step 1: Build (unless skip_build)
    if !skip_build {
        info!("daemon_update: starting cargo build --release");
        let project_root_clone = project_root.clone();
        let build_output = tokio::task::spawn_blocking(move || {
            // Resolve cargo path — launchd doesn't inherit shell PATH
            let cargo = std::env::var("CARGO")
                .or_else(|_| std::env::var("HOME").map(|h| format!("{}/.cargo/bin/cargo", h)))
                .unwrap_or_else(|_| "cargo".to_string());
            Command::new(&cargo)
                .args(["build", "--release", "--package", "missiond-daemon"])
                .current_dir(&project_root_clone)
                .output()
        })
        .await??;

        if !build_output.status.success() {
            let stderr = String::from_utf8_lossy(&build_output.stderr);
            return Ok(ToolResult::error(format!(
                "cargo build failed (exit {})\n{}",
                build_output.status, stderr
            )));
        }
        info!("daemon_update: build succeeded");
    }

    // Step 2: Verify new binary exists
    if !build_target.exists() {
        return Ok(ToolResult::error(format!(
            "Built binary not found at {}",
            build_target.display()
        )));
    }

    // Step 3: Replace binary via atomic rename (temp file → rename).
    // CRITICAL: std::fs::copy() on a running binary causes SIGBUS on macOS
    // because the executable is memory-mapped. We must copy to a temp file
    // first, then atomically rename to avoid corrupting the running process.
    let tmp_dest = binary_dest.with_extension("tmp");
    if let Err(e) = std::fs::copy(&build_target, &tmp_dest) {
        return Ok(ToolResult::error(format!(
            "Failed to copy binary to temp: {} → {}: {}",
            build_target.display(),
            tmp_dest.display(),
            e
        )));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp_dest, std::fs::Permissions::from_mode(0o755));
    }

    // Atomic rename: old inode stays mapped in running process, new inode takes the path
    if let Err(e) = std::fs::rename(&tmp_dest, &binary_dest) {
        let _ = std::fs::remove_file(&tmp_dest);
        return Ok(ToolResult::error(format!(
            "Failed to rename binary: {} → {}: {}",
            tmp_dest.display(),
            binary_dest.display(),
            e
        )));
    }

    // Step 3.5: codesign (macOS requires ad-hoc signature for execution)
    #[cfg(target_os = "macos")]
    {
        let sign_status = Command::new("codesign")
            .args(["-s", "-", "--force", binary_dest.to_str().unwrap_or("missiond")])
            .status();
        match sign_status {
            Ok(s) if s.success() => info!("daemon_update: codesign succeeded"),
            Ok(s) => info!("daemon_update: codesign exited with {}", s),
            Err(e) => info!("daemon_update: codesign failed: {}", e),
        }
    }

    info!(
        "daemon_update: binary replaced at {}",
        binary_dest.display()
    );

    // Step 4: Restart via launchd or fallback to manual restart
    let current_pid = std::process::id();
    let uid = unsafe { libc::getuid() };
    let kickstart_target = format!("gui/{}/{}", uid, LAUNCHD_LABEL);

    // Check if launchd service exists
    let launchd_check = Command::new("launchctl")
        .args(["print", &kickstart_target])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    let use_launchd = launchd_check.map(|s| s.success()).unwrap_or(false);

    if use_launchd {
        // Defer kickstart via tokio::spawn so the MCP response is sent first.
        // Without this, kickstart -k sends SIGTERM immediately, killing the
        // daemon before the JSON-RPC response reaches Claude Code.
        let kickstart_target_owned = kickstart_target.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            info!("daemon_update: executing launchctl kickstart -k");
            let _ = Command::new("launchctl")
                .args(["kickstart", "-k", &kickstart_target_owned])
                .status();
        });

        Ok(ToolResult::text(format!(
            "Build succeeded. Daemon will restart in 2 seconds via launchd.\n\
             - Binary: {} → {}\n\
             - Method: launchctl kickstart -k {}\n\
             - Old PID: {}\n\
             - Reconnect MCP after restart.",
            build_target.display(),
            binary_dest.display(),
            kickstart_target,
            current_pid,
        )))
    } else {
        // Fallback: detached bash script for non-launchd environments
        info!("daemon_update: launchd service not found, using script fallback");
        let home = missiond_core::ipc::default_mission_home();
        let socket_path = home.join("missiond.sock");
        let log_dir = home.join("logs");

        let script = format!(
            r#"#!/bin/bash
# MissionD daemon update script (auto-generated, non-launchd fallback)
sleep 2
kill -TERM {pid} 2>/dev/null
sleep 3
kill -9 {pid} 2>/dev/null
sleep 1
rm -f "{sock}"
nohup "{dest}" > "{log_dir}/missiond.log" 2>&1 &
echo "MissionD restarted (new PID: $!)"
"#,
            pid = current_pid,
            dest = binary_dest.display(),
            sock = socket_path.display(),
            log_dir = log_dir.display(),
        );

        let script_path = std::env::temp_dir().join("missiond-update.sh");
        tokio::fs::write(&script_path, &script).await?;

        let mut cmd = Command::new("bash");
        cmd.arg(&script_path);

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        match cmd.spawn() {
            Ok(_) => Ok(ToolResult::text(format!(
                "Build succeeded. Restart script launched (no launchd).\n\
                     - Binary: {} → {}\n\
                     - Old PID: {}\n\
                     - Daemon will restart in ~2 seconds. Reconnect MCP.",
                build_target.display(),
                binary_dest.display(),
                current_pid,
            ))),
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to launch restart script: {}",
                e
            ))),
        }
    }
}

/// Find the most recent log file in the logs directory.
fn find_latest_log(log_dir: &str) -> Option<String> {
    let dir = Path::new(log_dir);
    if !dir.is_dir() {
        return None;
    }
    let mut logs: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("missiond.log"))
        .collect();
    logs.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    logs.first().map(|e| e.path().to_string_lossy().to_string())
}
