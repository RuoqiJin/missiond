use std::collections::HashMap;
use std::path::PathBuf;

use tracing::{debug, info, warn};

use crate::state::AppState;
use std::path::Path;

// @beacon: slot
pub(crate) async fn build_slot_tracking_env(slot_id: &str, slot_env: Option<&HashMap<String, String>>) -> (HashMap<String, String>, PathBuf) {
    let session_file = std::env::temp_dir().join(format!("missiond-session-{}.txt", slot_id));
    // Remove stale file from previous spawn
    let _ = std::fs::remove_file(&session_file);

    let mut extra_env = HashMap::new();

    // 1. Merge slot-level custom env (model provider config, etc.)
    if let Some(env_map) = slot_env {
        info!(slot_id, env_count = env_map.len(), "Injecting slot-level custom env");
        for (key, value) in env_map {
            let resolved = resolve_env_value(value).await;
            let is_sensitive = value.starts_with("${secret:") || value.starts_with("${cmd:");
            if is_sensitive {
                info!(slot_id, key, "Resolved sensitive env var");
            } else {
                info!(slot_id, key, %resolved, "Set env var");
            }
            extra_env.insert(key.clone(), resolved);
        }
    } else {
        debug!(slot_id, "No custom env for slot");
    }

    // 2. Add tracking vars (always override custom env to prevent collision)
    extra_env.insert("MISSIOND_SLOT_ID".to_string(), slot_id.to_string());
    extra_env.insert(
        "MISSIOND_SESSION_FILE".to_string(),
        session_file.to_string_lossy().to_string(),
    );

    (extra_env, session_file)
}

/// Resolve dynamic references in env values.
///
/// Supported providers:
/// - `${env:VAR}` — read from environment variable
/// - `${file:path}` — read file contents (trimmed)
/// - `${cmd:program args...}` — execute command, use stdout
/// - `${secret:path}` — backward compat, translates to `${cmd:xjp secret get --raw path}`
/// - bare string — returned as-is (plaintext fallback)
///
/// Falls back to the raw value on any error.
pub(crate) async fn resolve_env_value(value: &str) -> String {
    // Must match ${provider:content} pattern
    if !value.starts_with("${") || !value.ends_with('}') {
        return value.to_string();
    }

    let inner = &value[2..value.len() - 1]; // strip ${ and }
    let Some((provider, content)) = inner.split_once(':') else {
        return value.to_string();
    };

    match provider {
        "env" => {
            match std::env::var(content) {
                Ok(val) => {
                    info!(var = content, "Resolved env var");
                    val
                }
                Err(_) => {
                    warn!(var = content, "Env var not set, using raw value");
                    value.to_string()
                }
            }
        }
        "file" => {
            match tokio::fs::read_to_string(content).await {
                Ok(val) => {
                    let trimmed = val.trim().to_string();
                    info!(path = content, "Resolved file value");
                    trimmed
                }
                Err(e) => {
                    warn!(path = content, error = %e, "Failed to read file, using raw value");
                    value.to_string()
                }
            }
        }
        "cmd" => {
            resolve_cmd_value(value, content).await
        }
        "secret" => {
            // Backward compat: translate to xjp secret get --raw
            let cmd_str = format!("xjp secret get --raw {}", content);
            resolve_cmd_value(value, &cmd_str).await
        }
        _ => {
            warn!(provider, "Unknown env value provider, using raw value");
            value.to_string()
        }
    }
}

/// Execute a command string and return its stdout. Helper for ${cmd:} and ${secret:}.
pub(crate) async fn resolve_cmd_value(raw_value: &str, cmd_str: &str) -> String {
    let parts: Vec<&str> = cmd_str.split_whitespace().collect();
    if parts.is_empty() {
        warn!("Empty command in env value, using raw value");
        return raw_value.to_string();
    }

    let program = parts[0];
    let args = &parts[1..];

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(program)
            .args(args)
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if out.is_empty() {
                warn!(cmd = cmd_str, "Command returned empty, using raw value");
                raw_value.to_string()
            } else {
                info!(cmd = program, "Resolved command value for slot env");
                out
            }
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(cmd = cmd_str, %stderr, "Command failed, using raw value");
            raw_value.to_string()
        }
        Ok(Err(e)) => {
            warn!(cmd = cmd_str, error = %e, "Failed to run command, using raw value");
            raw_value.to_string()
        }
        Err(_) => {
            warn!(cmd = cmd_str, "Command timed out (10s), using raw value");
            raw_value.to_string()
        }
    }
}

/// After a PTY spawn with wait_for_idle, read the session UUID
/// written by the SessionStart hook and register it in DB + cache.
pub(crate) async fn capture_slot_session_uuid(
    state: &AppState,
    slot_id: &str,
    session_file: &Path,
) {
    let mut uuid = None;

    // The hook writes during Claude's SessionStart, which happens
    // before idle. So by the time wait_for_idle returns, the file
    // should exist. Retry as safety net.
    for attempt in 0..5u32 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        match tokio::fs::read_to_string(session_file).await {
            Ok(content) => {
                let trimmed = content.trim().to_string();
                if !trimmed.is_empty() {
                    uuid = Some(trimmed);
                    break;
                }
            }
            Err(_) => continue,
        }
    }

    // Clean up temp file
    let _ = tokio::fs::remove_file(session_file).await;

    match uuid {
        Some(session_uuid) => {
            info!(
                slot_id = %slot_id,
                session_uuid = %session_uuid,
                "Captured PTY session UUID via hook"
            );

            // Persist in DB (activates the previously-orphaned slot_sessions table)
            if let Err(e) = state.store.set_slot_session(slot_id, &session_uuid).await {
                warn!(slot_id = %slot_id, error = %e, "Failed to persist slot session mapping");
            }

            // Update in-memory cache
            state.pty_session_uuids.write().await.insert(session_uuid.clone());

            // Retroactive fix: if conversation already exists with slot_id=None, tag it
            if let Ok(Some(conv)) = state.store.get_conversation(&session_uuid).await {
                if conv.slot_id.is_none() {
                    let mut updated = conv;
                    updated.slot_id = Some(slot_id.to_string());
                    updated.source = "pty_jsonl".to_string();
                    updated.conversation_type = missiond_core::db::derive_conversation_type(
                        Some(slot_id), &session_uuid
                    );
                    let _ = state.store.upsert_conversation(&updated).await;
                    info!(session = %session_uuid, slot_id = %slot_id, "Retroactively tagged conversation with slot_id and conversation_type");
                }
            }
        }
        None => {
            warn!(
                slot_id = %slot_id,
                "Failed to capture session UUID - hook may not be installed"
            );
        }
    }
}
