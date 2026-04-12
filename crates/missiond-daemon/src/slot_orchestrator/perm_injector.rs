//! Spawn-time permission injection.
//!
//! Bridges MissionD's `LearnedPermissions` (role-scoped, persistent in YAML) into
//! Claude Code's per-project allowlist file `.claude/settings.local.json`. This is
//! how we make user/auto-confirmed allowlists survive across slot restarts and
//! across different cwds for the same role.

use anyhow::{Context, Result};
use missiond_core::LearnedPermissions;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Convert a LearnedPermission to Claude Code allowlist string format.
/// `Bash` + `python3:*` → `Bash(python3:*)`
/// `Read` + None        → `Read`
fn perm_to_string(tool: &str, param: Option<&str>) -> String {
    match param {
        Some(p) if !p.is_empty() => format!("{}({})", tool, p),
        _ => tool.to_string(),
    }
}

/// Sync MissionD's learned permissions into the slot's project-local
/// `.claude/settings.local.json`. Idempotent: existing entries are preserved and
/// new entries are deduplicated.
///
/// Pulls the union across `global`, `role`, `project`, and `slot` scopes via
/// `LearnedPermissions::get_for_spawn`.
///
/// Failures are logged but never propagated — a settings sync error must not
/// block the slot from spawning.
pub fn sync_learned_to_local_settings(
    cwd: &Path,
    role: &str,
    project_id: Option<&str>,
    slot_id: Option<&str>,
    learned: &Arc<LearnedPermissions>,
) {
    if let Err(e) = sync_inner(cwd, role, project_id, slot_id, learned) {
        warn!(role, cwd = %cwd.display(), error = %e, "Failed to sync learned permissions to settings.local.json");
    }
}

fn sync_inner(
    cwd: &Path,
    role: &str,
    project_id: Option<&str>,
    slot_id: Option<&str>,
    learned: &Arc<LearnedPermissions>,
) -> Result<()> {
    let entries = learned
        .get_for_spawn(role, project_id, slot_id)
        .context("get_for_spawn failed")?;
    let allow_entries: Vec<String> = entries
        .iter()
        .filter(|e| e.decision == "allow")
        .map(|e| perm_to_string(&e.tool_pattern, e.param_pattern.as_deref()))
        .collect();
    let deny_entries: Vec<String> = entries
        .iter()
        .filter(|e| e.decision == "deny")
        .map(|e| perm_to_string(&e.tool_pattern, e.param_pattern.as_deref()))
        .collect();

    if allow_entries.is_empty() && deny_entries.is_empty() {
        debug!(role, "No learned permissions for this role; skipping settings sync");
        return Ok(());
    }

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
                warn!(error = %e, path = %settings_path.display(), "Existing settings.local.json malformed; overwriting");
                json!({})
            })
        }
    } else {
        json!({})
    };

    if !root.is_object() {
        root = json!({});
    }
    let permissions = root
        .as_object_mut()
        .unwrap()
        .entry("permissions".to_string())
        .or_insert_with(|| json!({}));
    if !permissions.is_object() {
        *permissions = json!({});
    }
    let perms_obj = permissions.as_object_mut().unwrap();

    let mut added_allow = 0;
    let mut added_deny = 0;
    if !allow_entries.is_empty() {
        let allow_list = perms_obj
            .entry("allow".to_string())
            .or_insert_with(|| json!([]));
        if !allow_list.is_array() {
            *allow_list = json!([]);
        }
        let arr = allow_list.as_array_mut().unwrap();
        let existing: std::collections::HashSet<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        for entry in &allow_entries {
            if !existing.contains(entry) {
                arr.push(json!(entry));
                added_allow += 1;
            }
        }
    }
    if !deny_entries.is_empty() {
        let deny_list = perms_obj
            .entry("deny".to_string())
            .or_insert_with(|| json!([]));
        if !deny_list.is_array() {
            *deny_list = json!([]);
        }
        let arr = deny_list.as_array_mut().unwrap();
        let existing: std::collections::HashSet<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        for entry in &deny_entries {
            if !existing.contains(entry) {
                arr.push(json!(entry));
                added_deny += 1;
            }
        }
    }

    if added_allow == 0 && added_deny == 0 {
        debug!(role, path = %settings_path.display(), "settings.local.json already in sync");
        return Ok(());
    }

    let serialized = serde_json::to_string_pretty(&root)?;
    let tmp = settings_path.with_extension("json.tmp");
    std::fs::write(&tmp, serialized)
        .with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &settings_path)
        .with_context(|| format!("rename {} → {}", tmp.display(), settings_path.display()))?;

    info!(
        role,
        path = %settings_path.display(),
        added_allow,
        added_deny,
        "Synced learned permissions to settings.local.json"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn perm_to_string_with_param() {
        assert_eq!(perm_to_string("Bash", Some("python3:*")), "Bash(python3:*)");
    }

    #[test]
    fn perm_to_string_without_param() {
        assert_eq!(perm_to_string("Read", None), "Read");
        assert_eq!(perm_to_string("Read", Some("")), "Read");
    }

    #[test]
    fn sync_creates_file_when_missing() {
        let dir = tempdir().unwrap();
        let yaml_path = dir.path().join("perms.yaml");
        let learned = Arc::new(LearnedPermissions::new(&yaml_path).unwrap());
        learned.learn("role", "coder", "Bash", "allow", Some("python3:*")).unwrap();

        let cwd = dir.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        sync_learned_to_local_settings(&cwd, "coder", None, None, &learned);

        let settings_path = cwd.join(".claude/settings.local.json");
        assert!(settings_path.exists());
        let content = std::fs::read_to_string(&settings_path).unwrap();
        let v: Value = serde_json::from_str(&content).unwrap();
        let allow = &v["permissions"]["allow"];
        assert!(allow.as_array().unwrap().iter().any(|e| e == "Bash(python3:*)"));
    }

    #[test]
    fn sync_preserves_existing_entries() {
        let dir = tempdir().unwrap();
        let yaml_path = dir.path().join("perms.yaml");
        let learned = Arc::new(LearnedPermissions::new(&yaml_path).unwrap());
        learned.learn("role", "coder", "Bash", "allow", Some("python3:*")).unwrap();

        let cwd = dir.path().join("project");
        std::fs::create_dir_all(cwd.join(".claude")).unwrap();
        let pre = json!({
            "permissions": {
                "allow": ["Read(/etc/passwd)"],
                "deny": ["WebFetch"]
            },
            "other_top_level": "preserved"
        });
        std::fs::write(
            cwd.join(".claude/settings.local.json"),
            serde_json::to_string_pretty(&pre).unwrap(),
        )
        .unwrap();

        sync_learned_to_local_settings(&cwd, "coder", None, None, &learned);

        let content = std::fs::read_to_string(cwd.join(".claude/settings.local.json")).unwrap();
        let v: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["other_top_level"], "preserved");
        let allow = v["permissions"]["allow"].as_array().unwrap();
        assert!(allow.iter().any(|e| e == "Read(/etc/passwd)"));
        assert!(allow.iter().any(|e| e == "Bash(python3:*)"));
        let deny = v["permissions"]["deny"].as_array().unwrap();
        assert!(deny.iter().any(|e| e == "WebFetch"));
    }

    #[test]
    fn sync_dedup() {
        let dir = tempdir().unwrap();
        let yaml_path = dir.path().join("perms.yaml");
        let learned = Arc::new(LearnedPermissions::new(&yaml_path).unwrap());
        learned.learn("role", "coder", "Bash", "allow", Some("python3:*")).unwrap();

        let cwd = dir.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        sync_learned_to_local_settings(&cwd, "coder", None, None, &learned);
        sync_learned_to_local_settings(&cwd, "coder", None, None, &learned);

        let v: Value = serde_json::from_str(
            &std::fs::read_to_string(cwd.join(".claude/settings.local.json")).unwrap(),
        )
        .unwrap();
        let allow = v["permissions"]["allow"].as_array().unwrap();
        let count = allow.iter().filter(|e| **e == "Bash(python3:*)").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn sync_multi_scope_union() {
        // Learn at role AND project; sync with both scopes; verify both appear,
        // and that learning the same entry at both scopes de-duplicates.
        let dir = tempdir().unwrap();
        let yaml_path = dir.path().join("perms.yaml");
        let learned = Arc::new(LearnedPermissions::new(&yaml_path).unwrap());
        // Duplicate (Bash, python3:*) at both role and project.
        learned.learn("role", "coder", "Bash", "allow", Some("python3:*")).unwrap();
        learned.learn("project", "missiond", "Bash", "allow", Some("python3:*")).unwrap();
        // Project-only WebFetch allow.
        learned.learn("project", "missiond", "WebFetch", "allow", None).unwrap();
        // Role-only Read allow.
        learned.learn("role", "coder", "Read", "allow", None).unwrap();

        let cwd = dir.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        sync_learned_to_local_settings(&cwd, "coder", Some("missiond"), None, &learned);

        let v: Value = serde_json::from_str(
            &std::fs::read_to_string(cwd.join(".claude/settings.local.json")).unwrap(),
        )
        .unwrap();
        let allow = v["permissions"]["allow"].as_array().unwrap();
        // All three distinct entries present.
        assert!(allow.iter().any(|e| e == "Bash(python3:*)"));
        assert!(allow.iter().any(|e| e == "WebFetch"));
        assert!(allow.iter().any(|e| e == "Read"));
        // Duplicate learned at both scopes must appear exactly once.
        let count_bash = allow.iter().filter(|e| **e == "Bash(python3:*)").count();
        assert_eq!(count_bash, 1, "union must dedup cross-scope duplicates");
    }
}
