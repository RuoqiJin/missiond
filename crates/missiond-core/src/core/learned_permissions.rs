//! Learned Permissions — in-memory HashMap with YAML persistence.
//!
//! High-read, low-write policy state. All reads from memory (zero latency).
//! Writes update memory + async atomic write-back to YAML file.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tracing::{debug, info, warn};

/// A learned permission entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnedPermission {
    #[serde(default)]
    pub id: i64,
    pub scope_type: String,
    pub scope_id: String,
    pub tool_pattern: String,
    pub decision: String,
    #[serde(default)]
    pub param_pattern: Option<String>,
    #[serde(default)]
    pub learned_at: String,
    #[serde(default)]
    pub last_used_at: Option<String>,
    #[serde(default)]
    pub use_count: i64,
}

/// Composite key for HashMap lookup
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
struct PermKey {
    scope_type: String,
    scope_id: String,
    tool_pattern: String,
    param_pattern: String,
}

/// Tools that can only be learned with a specific param_pattern, never as a blanket allow.
/// Bash is here because "allow ALL Bash" is too dangerous, but specific subcommand patterns
/// (e.g. `python3:*`, `npm test:*`) are exactly what users want to persist.
const REQUIRES_PARAM_PATTERN: &[&str] = &["Bash"];

/// Learned Permissions — in-memory store with YAML persistence.
pub struct LearnedPermissions {
    entries: RwLock<HashMap<PermKey, LearnedPermission>>,
    yaml_path: PathBuf,
    next_id: RwLock<i64>,
    /// Mutex protecting file I/O to prevent concurrent persist() corruption.
    file_lock: std::sync::Mutex<()>,
}

impl LearnedPermissions {
    /// Load from YAML file (or create empty if not found).
    /// Also migrates from legacy SQLite if YAML doesn't exist but .db does.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db_path = path.as_ref();
        // YAML path: same location, .yaml extension
        let yaml_path = db_path.with_extension("yaml");

        let mut entries = HashMap::new();
        let mut max_id = 0i64;

        if yaml_path.exists() {
            let content = std::fs::read_to_string(&yaml_path)?;
            let perms: Vec<LearnedPermission> = serde_yaml::from_str(&content).unwrap_or_default();
            for p in perms {
                if p.id > max_id {
                    max_id = p.id;
                }
                let key = PermKey {
                    scope_type: p.scope_type.clone(),
                    scope_id: p.scope_id.clone(),
                    tool_pattern: p.tool_pattern.clone(),
                    param_pattern: p.param_pattern.clone().unwrap_or_default(),
                };
                entries.insert(key, p);
            }
            info!(count = entries.len(), path = %yaml_path.display(), "Learned permissions loaded from YAML");
        } else if db_path.exists() && db_path.extension().map(|e| e == "db").unwrap_or(false) {
            // Migrate from legacy SQLite
            match Self::migrate_from_sqlite(db_path) {
                Ok(perms) => {
                    for p in &perms {
                        if p.id > max_id {
                            max_id = p.id;
                        }
                        let key = PermKey {
                            scope_type: p.scope_type.clone(),
                            scope_id: p.scope_id.clone(),
                            tool_pattern: p.tool_pattern.clone(),
                            param_pattern: p.param_pattern.clone().unwrap_or_default(),
                        };
                        entries.insert(key, p.clone());
                    }
                    info!(
                        count = entries.len(),
                        "Migrated learned permissions from SQLite → YAML"
                    );
                }
                Err(e) => {
                    warn!(error = %e, "Failed to migrate learned permissions from SQLite");
                }
            }
        }

        let store = Self {
            entries: RwLock::new(entries),
            yaml_path,
            next_id: RwLock::new(max_id + 1),
            file_lock: std::sync::Mutex::new(()),
        };

        // Persist initial state (especially after SQLite migration)
        store.persist();

        info!("Learned permissions schema initialized");
        Ok(store)
    }

    /// Legacy SQLite migration hook — kept for YAML bootstrap compatibility,
    /// but the SQLite backend was removed in v0.4.23 Stage 2E. Always returns
    /// an empty vector; existing YAML on disk is still loaded by `new()`.
    fn migrate_from_sqlite(_db_path: &Path) -> Result<Vec<LearnedPermission>> {
        Ok(Vec::new())
    }

    /// Persist to YAML atomically (tempfile + rename).
    /// Protected by file_lock to prevent concurrent write corruption.
    fn persist(&self) {
        let entries = self.entries.read().unwrap();
        let mut perms: Vec<&LearnedPermission> = entries.values().collect();
        perms.sort_by(|a, b| b.use_count.cmp(&a.use_count));

        let yaml = match serde_yaml::to_string(&perms) {
            Ok(y) => y,
            Err(e) => {
                warn!(error = %e, "Failed to serialize learned permissions");
                return;
            }
        };
        drop(entries); // Release read lock before file I/O

        let _guard = self.file_lock.lock().unwrap();
        let tmp_path = self.yaml_path.with_extension("yaml.tmp");
        if std::fs::write(&tmp_path, &yaml).is_ok() {
            let _ = std::fs::rename(&tmp_path, &self.yaml_path);
        }
    }

    /// Decide whether a tool can be auto-learned at all.
    /// Tools listed in `REQUIRES_PARAM_PATTERN` (e.g. Bash) refuse blanket learns
    /// (param_pattern == None) but allow specific patterns like `python3:*`.
    pub fn is_never_learn(tool_name: &str, param_pattern: Option<&str>) -> bool {
        if REQUIRES_PARAM_PATTERN.iter().any(|t| *t == tool_name) {
            return param_pattern.map(|p| p.is_empty()).unwrap_or(true);
        }
        false
    }

    pub fn learn(
        &self,
        scope_type: &str,
        scope_id: &str,
        tool_pattern: &str,
        decision: &str,
        param_pattern: Option<&str>,
    ) -> Result<()> {
        if Self::is_never_learn(tool_pattern, param_pattern) {
            debug!(tool = %tool_pattern, "Skipping never-learn tool");
            return Ok(());
        }

        let param = param_pattern.unwrap_or("").to_string();
        let key = PermKey {
            scope_type: scope_type.to_string(),
            scope_id: scope_id.to_string(),
            tool_pattern: tool_pattern.to_string(),
            param_pattern: param.clone(),
        };

        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        {
            let mut entries = self.entries.write().unwrap();
            if let Some(existing) = entries.get_mut(&key) {
                existing.decision = decision.to_string();
                existing.learned_at = now;
                existing.use_count += 1;
            } else {
                let mut next_id = self.next_id.write().unwrap();
                let id = *next_id;
                *next_id += 1;
                entries.insert(
                    key,
                    LearnedPermission {
                        id,
                        scope_type: scope_type.to_string(),
                        scope_id: scope_id.to_string(),
                        tool_pattern: tool_pattern.to_string(),
                        decision: decision.to_string(),
                        param_pattern: Some(param),
                        learned_at: now,
                        last_used_at: None,
                        use_count: 1,
                    },
                );
            }
        }

        self.persist();
        info!(
            scope_type,
            scope_id, tool_pattern, decision, "Learned permission"
        );
        Ok(())
    }

    pub fn get_for_scope(
        &self,
        scope_type: &str,
        scope_id: &str,
    ) -> Result<Vec<LearnedPermission>> {
        let entries = self.entries.read().unwrap();
        let mut result: Vec<LearnedPermission> = entries
            .values()
            .filter(|p| p.scope_type == scope_type && p.scope_id == scope_id)
            .cloned()
            .collect();
        result.sort_by(|a, b| b.use_count.cmp(&a.use_count));
        Ok(result)
    }

    /// Return the union of all permission entries visible to a spawning slot.
    ///
    /// Scope precedence (later wins on `(tool_pattern, param_pattern)` dedup):
    /// 1. global (scope_id="")
    /// 2. role=<role>
    /// 3. project=<project_id> (if Some)
    /// 4. slot=<slot_id> (if Some)
    ///
    /// The ordering matches "most specific last" so a slot-scoped deny can shadow
    /// a broader role-scoped allow, while still surfacing the role entry when no
    /// more specific rule exists. The returned Vec preserves insertion order so
    /// downstream code writing to `settings.local.json` sees stable output.
    pub fn get_for_spawn(
        &self,
        role: &str,
        project_id: Option<&str>,
        slot_id: Option<&str>,
    ) -> Result<Vec<LearnedPermission>> {
        let mut ordered: Vec<LearnedPermission> = Vec::new();
        let mut index: HashMap<(String, String), usize> = HashMap::new();

        let push_all = |entries: Vec<LearnedPermission>,
                        ordered: &mut Vec<LearnedPermission>,
                        index: &mut HashMap<(String, String), usize>| {
            for entry in entries {
                let key = (
                    entry.tool_pattern.clone(),
                    entry.param_pattern.clone().unwrap_or_default(),
                );
                if let Some(&pos) = index.get(&key) {
                    // More-specific scope overwrites earlier entry.
                    ordered[pos] = entry;
                } else {
                    index.insert(key, ordered.len());
                    ordered.push(entry);
                }
            }
        };

        // global first — lowest precedence.
        if let Ok(entries) = self.get_for_scope("global", "") {
            push_all(entries, &mut ordered, &mut index);
        }
        // role next.
        if let Ok(entries) = self.get_for_scope("role", role) {
            push_all(entries, &mut ordered, &mut index);
        }
        // project next.
        if let Some(pid) = project_id {
            if let Ok(entries) = self.get_for_scope("project", pid) {
                push_all(entries, &mut ordered, &mut index);
            }
        }
        // slot last — most specific, overrides all.
        if let Some(sid) = slot_id {
            if let Ok(entries) = self.get_for_scope("slot", sid) {
                push_all(entries, &mut ordered, &mut index);
            }
        }

        Ok(ordered)
    }

    pub fn check_learned(
        &self,
        scope_type: &str,
        scope_id: &str,
        tool_name: &str,
    ) -> Option<super::permission::PermissionDecision> {
        // Fast path: read lock only (most calls won't match → zero write contention)
        let decision_str = {
            let entries = self.entries.read().unwrap();
            entries.iter().find_map(|(k, v)| {
                if k.scope_type == scope_type
                    && k.scope_id == scope_id
                    && k.tool_pattern == tool_name
                {
                    Some((k.clone(), v.decision.clone()))
                } else {
                    None
                }
            })
        };

        if let Some((key, decision)) = decision_str {
            // Slow path: write lock to update usage stats (only on match)
            if let Ok(mut entries) = self.entries.write() {
                if let Some(entry) = entries.get_mut(&key) {
                    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    entry.last_used_at = Some(now);
                    entry.use_count += 1;
                }
            }

            debug!(scope_type, scope_id, tool_name, decision = %decision, "Matched learned permission");
            return match decision.as_str() {
                "allow" => Some(super::permission::PermissionDecision::Allow),
                "deny" => Some(super::permission::PermissionDecision::Deny),
                _ => None,
            };
        }
        None
    }

    pub fn forget(&self, scope_type: &str, scope_id: &str, tool_pattern: &str) -> Result<bool> {
        // Remove ALL entries matching this tool (regardless of param_pattern)
        let mut entries = self.entries.write().unwrap();
        let keys_to_remove: Vec<PermKey> = entries
            .keys()
            .filter(|k| {
                k.scope_type == scope_type
                    && k.scope_id == scope_id
                    && k.tool_pattern == tool_pattern
            })
            .cloned()
            .collect();
        let removed = !keys_to_remove.is_empty();
        for key in keys_to_remove {
            entries.remove(&key);
        }
        drop(entries);
        if removed {
            self.persist();
        }
        Ok(removed)
    }

    pub fn get_all(&self) -> Result<Vec<LearnedPermission>> {
        let entries = self.entries.read().unwrap();
        let mut result: Vec<LearnedPermission> = entries.values().cloned().collect();
        result.sort_by(|a, b| b.use_count.cmp(&a.use_count));
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_learn_and_check() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.yaml");
        let lp = LearnedPermissions::new(&db_path).unwrap();

        lp.learn("role", "worker", "read_file", "allow", None)
            .unwrap();

        let result = lp.check_learned("role", "worker", "read_file");
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            super::super::permission::PermissionDecision::Allow
        );

        lp.learn("role", "worker", "delete_file", "deny", None)
            .unwrap();
        let result = lp.check_learned("role", "worker", "delete_file");
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            super::super::permission::PermissionDecision::Deny
        );

        // Verify YAML file exists
        assert!(db_path.exists());
    }

    #[test]
    fn test_never_learn() {
        // Bash without param_pattern → blanket allow not permitted
        assert!(LearnedPermissions::is_never_learn("Bash", None));
        assert!(LearnedPermissions::is_never_learn("Bash", Some("")));
        // Bash with specific param_pattern → learnable
        assert!(!LearnedPermissions::is_never_learn(
            "Bash",
            Some("python3:*")
        ));
        assert!(!LearnedPermissions::is_never_learn(
            "Bash",
            Some("npm test:*")
        ));
        // Other tools always learnable
        assert!(!LearnedPermissions::is_never_learn("Write", None));
        assert!(!LearnedPermissions::is_never_learn("Edit", None));
    }

    #[test]
    fn test_get_for_spawn_union_and_dedup() {
        let dir = tempdir().unwrap();
        let yaml_path = dir.path().join("spawn.yaml");
        let lp = LearnedPermissions::new(&yaml_path).unwrap();

        // global → baseline (should always appear)
        lp.learn("global", "", "Read", "allow", None).unwrap();
        // role → coder-wide Bash(python3:*)
        lp.learn("role", "coder", "Bash", "allow", Some("python3:*"))
            .unwrap();
        // project → missiond-wide Bash(python3:*) — duplicate; later scope must dedup.
        lp.learn("project", "missiond", "Bash", "allow", Some("python3:*"))
            .unwrap();
        // project → WebFetch allow (unique to project scope)
        lp.learn("project", "missiond", "WebFetch", "allow", None)
            .unwrap();
        // slot → slot-1 overrides Bash(python3:*) with deny.
        lp.learn("slot", "slot-1", "Bash", "deny", Some("python3:*"))
            .unwrap();

        // Full union: role + project + slot.
        let full = lp
            .get_for_spawn("coder", Some("missiond"), Some("slot-1"))
            .unwrap();

        // Bash(python3:*) must appear once, and should reflect the slot-scoped deny
        // (most specific wins).
        let bash_entries: Vec<_> = full
            .iter()
            .filter(|e| e.tool_pattern == "Bash" && e.param_pattern.as_deref() == Some("python3:*"))
            .collect();
        assert_eq!(bash_entries.len(), 1, "dedup should collapse to one entry");
        assert_eq!(
            bash_entries[0].decision, "deny",
            "slot scope should win over role/project"
        );

        // Read must be present (from global).
        assert!(full.iter().any(|e| e.tool_pattern == "Read"));
        // WebFetch must be present (from project).
        assert!(full.iter().any(|e| e.tool_pattern == "WebFetch"));

        // No project_id or slot_id → only global + role visible.
        let role_only = lp.get_for_spawn("coder", None, None).unwrap();
        assert!(role_only.iter().any(|e| e.tool_pattern == "Read")); // global
        assert!(role_only
            .iter()
            .any(|e| e.tool_pattern == "Bash" && e.decision == "allow"));
        assert!(
            !role_only.iter().any(|e| e.tool_pattern == "WebFetch"),
            "without project_id, project-scoped entries must not appear"
        );

        // project but not slot → project-scoped entries visible, slot override not applied.
        let proj = lp.get_for_spawn("coder", Some("missiond"), None).unwrap();
        let bash_entries: Vec<_> = proj
            .iter()
            .filter(|e| e.tool_pattern == "Bash" && e.param_pattern.as_deref() == Some("python3:*"))
            .collect();
        assert_eq!(bash_entries.len(), 1);
        assert_eq!(
            bash_entries[0].decision, "allow",
            "without slot_id, slot-scoped deny must not apply"
        );
    }

    #[test]
    fn test_persistence_roundtrip() {
        let dir = tempdir().unwrap();
        let yaml_path = dir.path().join("test.yaml");

        // Write
        {
            let lp = LearnedPermissions::new(&yaml_path).unwrap();
            lp.learn("role", "worker", "Read", "allow", None).unwrap();
            lp.learn("slot", "slot-1", "Write", "deny", None).unwrap();
        }

        // Read back
        {
            let lp = LearnedPermissions::new(&yaml_path).unwrap();
            let all = lp.get_all().unwrap();
            assert_eq!(all.len(), 2);
            assert!(lp.check_learned("role", "worker", "Read").is_some());
        }
    }
}
