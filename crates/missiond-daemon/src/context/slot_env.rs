use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

// @beacon: slot
pub(crate) async fn build_slot_tracking_env(
    slot_id: &str,
    slot_env: Option<&HashMap<String, String>>,
) -> (HashMap<String, String>, PathBuf) {
    let session_file = std::env::temp_dir().join(format!("missiond-session-{}.txt", slot_id));
    // Remove stale file from previous spawn
    let _ = std::fs::remove_file(&session_file);

    let mut extra_env = HashMap::new();

    // 1. Merge slot-level custom env (model provider config, etc.)
    if let Some(env_map) = slot_env {
        info!(
            slot_id,
            env_count = env_map.len(),
            "Injecting slot-level custom env"
        );
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
    extra_env.insert(
        "MISSION_IPC_ENDPOINT".to_string(),
        crate::helpers::ipc_endpoint_from_env(),
    );

    (extra_env, session_file)
}

const SESSION_REGISTER_HOOK: &str = "bash ~/.claude/hooks/missiond-session-register.sh";
const CONTEXT_INJECT_HOOK: &str = "bash ~/.claude/hooks/missiond-context-inject-v2.sh";
const SESSION_REGISTER_HOOK_FILE: &str = "missiond-session-register.sh";
const CONTEXT_INJECT_HOOK_FILE: &str = "missiond-context-inject-v2.sh";
const SESSION_REGISTER_HOOK_SCRIPT: &str = r#"#!/usr/bin/env bash
set -euo pipefail

session_file="${MISSIOND_SESSION_FILE:-}"
if [ -z "$session_file" ]; then
  exit 0
fi

payload="$(cat || true)"
session_id="${CLAUDE_SESSION_ID:-}"

if [ -z "$session_id" ] && command -v python3 >/dev/null 2>&1; then
  session_id="$(MISSIOND_HOOK_PAYLOAD="$payload" python3 - <<'PY' 2>/dev/null || true
import json
import os
from pathlib import Path

payload = os.environ.get("MISSIOND_HOOK_PAYLOAD", "")
data = {}
if payload.strip():
    try:
        data = json.loads(payload)
    except Exception:
        data = {}
for key in ("session_id", "sessionId", "conversation_id", "conversationId"):
    value = data.get(key)
    if isinstance(value, str) and value.strip():
        print(value.strip())
        raise SystemExit(0)
for key in ("transcript_path", "transcriptPath"):
    value = data.get(key)
    if isinstance(value, str) and value.strip():
        name = Path(value).name
        if name.endswith(".jsonl"):
            name = name[:-6]
        if name:
            print(name)
            raise SystemExit(0)
PY
)"
fi

if [ -n "$session_id" ]; then
  mkdir -p "$(dirname "$session_file")"
  printf '%s\n' "$session_id" > "$session_file"
fi
"#;
const CONTEXT_INJECT_HOOK_SCRIPT: &str = r#"#!/usr/bin/env bash
set -euo pipefail

# MissionD context prefetch is opt-in and fail-open. The durable interaction
# gateway now provides grounded context slices; this hook exists only so legacy
# project-local Claude settings never point at a missing executable.
exit 0
"#;

fn claude_user_prompt_context_hook_enabled() -> bool {
    matches!(
        std::env::var("MISSIOND_CLAUDE_CONTEXT_PREFETCH")
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

/// Keep Claude Code's project-local settings aligned with the slot tracking
/// contract. This avoids mutating global `~/.claude/settings.json`, while still
/// making project-bound MissionD workstations load the SessionStart UUID hook
/// before the PTY process starts.
pub(crate) fn sync_slot_hooks_to_local_settings(cwd: &Path) {
    match sync_slot_hooks_inner(cwd) {
        Ok(true) => info!(
            cwd = %cwd.display(),
            "Synced MissionD Claude hooks to project-local settings"
        ),
        Ok(false) => debug!(
            cwd = %cwd.display(),
            "MissionD Claude hooks already present in project-local settings"
        ),
        Err(e) => warn!(
            cwd = %cwd.display(),
            error = %e,
            "Failed to sync MissionD Claude hooks to project-local settings"
        ),
    }
}

fn sync_slot_hooks_inner(cwd: &Path) -> Result<bool> {
    sync_slot_hooks_inner_with_context_hook(cwd, claude_user_prompt_context_hook_enabled())
}

fn sync_slot_hooks_inner_with_context_hook(cwd: &Path, context_hook_enabled: bool) -> Result<bool> {
    let hooks_dir = default_claude_hooks_dir()?;
    sync_slot_hooks_inner_with_context_hook_and_hooks_dir(cwd, context_hook_enabled, &hooks_dir)
}

fn sync_slot_hooks_inner_with_context_hook_and_hooks_dir(
    cwd: &Path,
    context_hook_enabled: bool,
    hooks_dir: &Path,
) -> Result<bool> {
    let mut changed = ensure_claude_home_hooks(hooks_dir)?;

    let claude_dir = cwd.join(".claude");
    std::fs::create_dir_all(&claude_dir)
        .with_context(|| format!("create_dir_all {}", claude_dir.display()))?;
    let settings_path = claude_dir.join("settings.local.json");

    let mut root: Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)
            .with_context(|| format!("read {}", settings_path.display()))?;
        if content.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&content).unwrap_or_else(|e| {
                warn!(
                    path = %settings_path.display(),
                    error = %e,
                    "Existing settings.local.json malformed; overwriting"
                );
                json!({})
            })
        }
    } else {
        json!({})
    };
    if !root.is_object() {
        root = json!({});
    }

    changed |= ensure_hook_command(
        &mut root,
        "SessionStart",
        Some("startup"),
        SESSION_REGISTER_HOOK,
    );
    changed |= ensure_hook_command(
        &mut root,
        "SessionStart",
        Some("resume"),
        SESSION_REGISTER_HOOK,
    );
    changed |= if context_hook_enabled {
        ensure_hook_command(&mut root, "UserPromptSubmit", None, CONTEXT_INJECT_HOOK)
    } else {
        remove_hook_command(&mut root, "UserPromptSubmit", None, CONTEXT_INJECT_HOOK)
    };

    if !changed {
        return Ok(false);
    }

    let serialized = serde_json::to_string_pretty(&root)?;
    let tmp = settings_path.with_extension("json.tmp");
    std::fs::write(&tmp, serialized).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &settings_path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), settings_path.display()))?;

    Ok(true)
}

fn default_claude_hooks_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is required to materialize Claude hook scripts")?;
    Ok(home.join(".claude").join("hooks"))
}

fn ensure_claude_home_hooks(hooks_dir: &Path) -> Result<bool> {
    std::fs::create_dir_all(hooks_dir)
        .with_context(|| format!("create_dir_all {}", hooks_dir.display()))?;
    let mut changed = false;
    changed |= write_hook_script_if_changed(
        &hooks_dir.join(SESSION_REGISTER_HOOK_FILE),
        SESSION_REGISTER_HOOK_SCRIPT,
    )?;
    changed |= write_hook_script_if_changed(
        &hooks_dir.join(CONTEXT_INJECT_HOOK_FILE),
        CONTEXT_INJECT_HOOK_SCRIPT,
    )?;
    Ok(changed)
}

fn write_hook_script_if_changed(path: &Path, content: &str) -> Result<bool> {
    let existing = std::fs::read_to_string(path).ok();
    if existing.as_deref() == Some(content) {
        ensure_executable(path)?;
        return Ok(false);
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| {
        format!(
            "rename materialized Claude hook {} -> {}",
            tmp.display(),
            path.display()
        )
    })?;
    ensure_executable(path)?;
    Ok(true)
}

fn ensure_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let mut permissions = std::fs::metadata(path)
            .with_context(|| format!("metadata {}", path.display()))?
            .permissions();
        let mode = permissions.mode();
        if mode & 0o111 != 0o111 {
            permissions.set_mode(mode | 0o755);
            std::fs::set_permissions(path, permissions)
                .with_context(|| format!("chmod +x {}", path.display()))?;
        }
    }
    Ok(())
}

fn ensure_hook_command(
    root: &mut Value,
    event: &str,
    matcher: Option<&str>,
    command: &str,
) -> bool {
    let root_obj = root.as_object_mut().expect("root normalized to object");
    let hooks = root_obj.entry("hooks").or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    let hooks_obj = hooks.as_object_mut().unwrap();
    let event_value = hooks_obj
        .entry(event.to_string())
        .or_insert_with(|| json!([]));
    if !event_value.is_array() {
        *event_value = json!([]);
    }
    let entries = event_value.as_array_mut().unwrap();

    for entry in entries.iter_mut() {
        if !entry_matches(entry, matcher) {
            continue;
        }
        if hook_entry_has_command(entry, command) {
            return false;
        }
        let hooks_list = entry
            .as_object_mut()
            .expect("hook entry object checked by entry_matches")
            .entry("hooks")
            .or_insert_with(|| json!([]));
        if !hooks_list.is_array() {
            *hooks_list = json!([]);
        }
        hooks_list
            .as_array_mut()
            .unwrap()
            .push(hook_command(command));
        return true;
    }

    let mut entry = json!({ "hooks": [hook_command(command)] });
    if let Some(matcher) = matcher {
        entry["matcher"] = json!(matcher);
    }
    entries.push(entry);
    true
}

fn remove_hook_command(
    root: &mut Value,
    event: &str,
    matcher: Option<&str>,
    command: &str,
) -> bool {
    let Some(hooks_obj) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return false;
    };
    let Some(entries) = hooks_obj.get_mut(event).and_then(Value::as_array_mut) else {
        return false;
    };

    let mut changed = false;
    for entry in entries.iter_mut() {
        if !entry_matches(entry, matcher) {
            continue;
        }
        let Some(hooks) = entry.get_mut("hooks").and_then(Value::as_array_mut) else {
            continue;
        };
        let before = hooks.len();
        hooks.retain(|hook| {
            hook.get("command")
                .and_then(Value::as_str)
                .map(|existing| !hook_command_matches(existing, command))
                .unwrap_or(true)
        });
        changed |= hooks.len() != before;
    }

    if changed {
        entries.retain(|entry| {
            entry
                .get("hooks")
                .and_then(Value::as_array)
                .map(|hooks| !hooks.is_empty())
                .unwrap_or(true)
        });
    }
    changed
}

fn entry_matches(entry: &mut Value, matcher: Option<&str>) -> bool {
    let Some(obj) = entry.as_object() else {
        return false;
    };
    match matcher {
        Some(matcher) => obj.get("matcher").and_then(Value::as_str) == Some(matcher),
        None => !obj.contains_key("matcher"),
    }
}

fn hook_entry_has_command(entry: &Value, command: &str) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .map(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command").and_then(Value::as_str) == Some(command)
                    || hook
                        .get("command")
                        .and_then(Value::as_str)
                        .map(|existing| hook_command_matches(existing, command))
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn hook_command_matches(existing: &str, command: &str) -> bool {
    existing == command
        || existing.contains("missiond-session-register.sh")
            && command.contains("missiond-session-register.sh")
        || existing.contains("missiond-context-inject")
            && command.contains("missiond-context-inject")
}

fn hook_command(command: &str) -> Value {
    json!({
        "type": "command",
        "command": command,
        "timeout": 5
    })
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
        "env" => match std::env::var(content) {
            Ok(val) => {
                info!(var = content, "Resolved env var");
                val
            }
            Err(_) => {
                warn!(var = content, "Env var not set, using raw value");
                value.to_string()
            }
        },
        "file" => match tokio::fs::read_to_string(content).await {
            Ok(val) => {
                let trimmed = val.trim().to_string();
                info!(path = content, "Resolved file value");
                trimmed
            }
            Err(e) => {
                warn!(path = content, error = %e, "Failed to read file, using raw value");
                value.to_string()
            }
        },
        "cmd" => resolve_cmd_value(value, content).await,
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
        tokio::process::Command::new(program).args(args).output(),
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

use missiond_core::db::traits::MissionStore;
use missiond_core::types::{BoardTask, BoardTaskStatus};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

pub(crate) fn active_board_task_id_for_slot_from_tasks(
    tasks: &[BoardTask],
    slot_id: &str,
) -> Option<String> {
    tasks
        .iter()
        .find(|task| {
            task.status == BoardTaskStatus::Running
                && task.claim_executor_type.as_deref() == Some("pty_slot")
                && task.claim_executor_id.as_deref() == Some(slot_id)
        })
        .map(|task| task.id.to_string())
}

pub(crate) async fn active_board_task_id_for_slot(
    store: &Arc<dyn MissionStore>,
    slot_id: &str,
) -> Option<String> {
    match store.list_board_tasks(Some("running"), true).await {
        Ok(tasks) => active_board_task_id_for_slot_from_tasks(&tasks, slot_id),
        Err(e) => {
            warn!(
                slot_id = %slot_id,
                error = %e,
                "Failed to resolve active BoardTask for slot session linkage"
            );
            None
        }
    }
}

/// After a PTY spawn with wait_for_idle, read the session UUID
/// written by the SessionStart hook and register it in DB + cache.
pub(crate) async fn capture_slot_session_uuid(
    store: &Arc<dyn MissionStore>,
    pty_session_uuids: &Arc<RwLock<HashSet<String>>>,
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
            if let Err(e) = store.set_slot_session(slot_id, &session_uuid).await {
                warn!(slot_id = %slot_id, error = %e, "Failed to persist slot session mapping");
            } else {
                let _ = store.cleanup_pty_placeholder(slot_id).await;
            }

            // Update in-memory cache
            pty_session_uuids.write().await.insert(session_uuid.clone());

            let active_task_id = active_board_task_id_for_slot(store, slot_id).await;

            // Retroactive fix: if the conversation already exists before the hook fires,
            // tag it as a slot-bound Claude worker session.
            if let Ok(Some(conv)) = store.get_conversation(&session_uuid).await {
                let category: Option<String> = None;
                let expected_type = missiond_core::db::derive_conversation_type(
                    category.as_deref(),
                    Some(slot_id),
                    &session_uuid,
                    "claude_code",
                );
                if conv.slot_id.as_deref() != Some(slot_id)
                    || conv.source != "claude_code"
                    || conv.conversation_type != expected_type
                {
                    let mut updated = conv.clone();
                    updated.slot_id = Some(slot_id.to_string());
                    updated.source = "claude_code".to_string();
                    updated.conversation_type = expected_type;
                    let _ = store.upsert_conversation(&updated).await;
                    info!(session = %session_uuid, slot_id = %slot_id, "Retroactively tagged conversation with slot_id and conversation_type");
                }
                if let Some(task_id) = active_task_id.as_deref() {
                    let existing_task_id = conv
                        .task_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty());
                    if existing_task_id.is_none() {
                        if let Err(e) = store.set_conversation_task_id(&session_uuid, task_id).await
                        {
                            warn!(
                                session = %session_uuid,
                                slot_id = %slot_id,
                                task_id = %task_id,
                                error = %e,
                                "Failed to bind conversation to active BoardTask after session capture"
                            );
                        } else {
                            info!(
                                session = %session_uuid,
                                slot_id = %slot_id,
                                task_id = %task_id,
                                "Bound conversation to active BoardTask after session capture"
                            );
                        }
                    } else if existing_task_id != Some(task_id) {
                        warn!(
                            session = %session_uuid,
                            slot_id = %slot_id,
                            existing_task_id = %existing_task_id.unwrap_or_default(),
                            active_task_id = %task_id,
                            "Skipped active BoardTask binding because conversation already has a different task_id"
                        );
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use missiond_core::types::TaskId;
    use tempfile::{tempdir, TempDir};

    fn sync_test_hooks(dir: &TempDir, context_hook_enabled: bool) -> Result<bool> {
        let hooks_dir = dir.path().join("home").join(".claude").join("hooks");
        sync_slot_hooks_inner_with_context_hook_and_hooks_dir(
            dir.path(),
            context_hook_enabled,
            &hooks_dir,
        )
    }

    fn board_task_for_slot(id: &str, slot_id: Option<&str>, status: BoardTaskStatus) -> BoardTask {
        BoardTask {
            id: TaskId::from_trusted(id.to_string()),
            title: "task".to_string(),
            description: String::new(),
            status,
            priority: "medium".to_string(),
            category: "dev".to_string(),
            project: None,
            server: None,
            due_date: None,
            parent_id: None,
            assignee: slot_id.map(str::to_string),
            auto_execute: true,
            prompt_template: None,
            hidden: false,
            retry_count: 0,
            max_retries: 2,
            order_idx: 0,
            created_at: "2026-05-06T00:00:00Z".to_string(),
            updated_at: "2026-05-06T00:00:00Z".to_string(),
            claim_executor_id: slot_id.map(str::to_string),
            claim_executor_type: slot_id.map(|_| "pty_slot".to_string()),
            claimed_at: None,
            flow_phase: None,
            flow_context: None,
            flow_template: None,
            depends_on: Vec::new(),
            lease_expires_at: None,
            dedupe_key: None,
            timeout_secs: None,
            context_intent: None,
            trigger_source: None,
            runtime_metadata: serde_json::json!({}),
            notes_count: 0,
        }
    }

    #[test]
    fn active_board_task_id_for_slot_requires_running_pty_claim() {
        let tasks = vec![
            board_task_for_slot("open-task", Some("slot-a"), BoardTaskStatus::Open),
            board_task_for_slot("running-other", Some("slot-b"), BoardTaskStatus::Running),
            board_task_for_slot("running-target", Some("slot-a"), BoardTaskStatus::Running),
        ];

        assert_eq!(
            active_board_task_id_for_slot_from_tasks(&tasks, "slot-a").as_deref(),
            Some("running-target")
        );
        assert!(active_board_task_id_for_slot_from_tasks(&tasks, "missing").is_none());
    }

    #[tokio::test]
    async fn build_slot_tracking_env_includes_hook_runtime_env() {
        let (env, session_file) = build_slot_tracking_env("slot-dyn-test", None).await;

        assert_eq!(env["MISSIOND_SLOT_ID"], "slot-dyn-test");
        assert_eq!(
            env["MISSIOND_SESSION_FILE"],
            session_file.to_string_lossy().to_string()
        );
        assert!(env["MISSION_IPC_ENDPOINT"].ends_with("missiond.sock"));
    }

    #[test]
    fn sync_slot_hooks_creates_project_local_hooks() {
        let dir = tempdir().unwrap();

        let changed = sync_test_hooks(&dir, false).unwrap();

        assert!(changed);
        assert!(dir
            .path()
            .join("home/.claude/hooks/missiond-session-register.sh")
            .exists());
        assert!(dir
            .path()
            .join("home/.claude/hooks/missiond-context-inject-v2.sh")
            .exists());
        let content =
            std::fs::read_to_string(dir.path().join(".claude/settings.local.json")).unwrap();
        let v: Value = serde_json::from_str(&content).unwrap();
        let startup = &v["hooks"]["SessionStart"][0];
        assert_eq!(startup["matcher"], "startup");
        assert_eq!(startup["hooks"][0]["command"], SESSION_REGISTER_HOOK);
        let resume = &v["hooks"]["SessionStart"][1];
        assert_eq!(resume["matcher"], "resume");
        assert_eq!(resume["hooks"][0]["command"], SESSION_REGISTER_HOOK);
        assert!(v["hooks"].get("UserPromptSubmit").is_none());
    }

    #[test]
    fn sync_slot_hooks_can_opt_in_user_prompt_context_hook() {
        let dir = tempdir().unwrap();

        let changed = sync_test_hooks(&dir, true).unwrap();

        assert!(changed);
        let content =
            std::fs::read_to_string(dir.path().join(".claude/settings.local.json")).unwrap();
        let v: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(
            v["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"],
            CONTEXT_INJECT_HOOK
        );
    }

    #[test]
    fn sync_slot_hooks_removes_user_prompt_context_hook_by_default() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        let settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            {
                                "type": "command",
                                "command": CONTEXT_INJECT_HOOK,
                                "timeout": 5
                            }
                        ]
                    }
                ]
            }
        });
        std::fs::write(
            dir.path().join(".claude/settings.local.json"),
            serde_json::to_string_pretty(&settings).unwrap(),
        )
        .unwrap();

        assert!(sync_test_hooks(&dir, false).unwrap());
        let content =
            std::fs::read_to_string(dir.path().join(".claude/settings.local.json")).unwrap();
        let v: Value = serde_json::from_str(&content).unwrap();
        assert!(v["hooks"]
            .get("UserPromptSubmit")
            .and_then(Value::as_array)
            .map(|hooks| hooks.is_empty())
            .unwrap_or(true));
    }

    #[test]
    fn sync_slot_hooks_preserves_permissions_and_dedups() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        let settings = json!({
            "permissions": {
                "allow": ["Read"]
            },
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "startup",
                        "hooks": [
                            {
                                "type": "command",
                                "command": SESSION_REGISTER_HOOK,
                                "timeout": 5
                            }
                        ]
                    }
                ]
            }
        });
        std::fs::write(
            dir.path().join(".claude/settings.local.json"),
            serde_json::to_string_pretty(&settings).unwrap(),
        )
        .unwrap();

        assert!(sync_test_hooks(&dir, false).unwrap());
        assert!(!sync_test_hooks(&dir, false).unwrap());

        let content =
            std::fs::read_to_string(dir.path().join(".claude/settings.local.json")).unwrap();
        let v: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["permissions"]["allow"][0], "Read");
        let session_start = v["hooks"]["SessionStart"].as_array().unwrap();
        let startup_count = session_start
            .iter()
            .filter(|entry| entry["matcher"] == "startup")
            .count();
        assert_eq!(startup_count, 1);
        let register_count = session_start
            .iter()
            .flat_map(|entry| entry["hooks"].as_array().into_iter().flatten())
            .filter(|hook| hook["command"] == SESSION_REGISTER_HOOK)
            .count();
        assert_eq!(register_count, 2);
    }
}
