//! Permission Policy - Tool permission management
//!
//! Supports permission configuration by role/slot:
//! - auto_allow: Automatically approved tools
//! - require_confirm: Tools requiring confirmation
//! - deny: Denied tools

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tracing::{debug, error, info};

use super::LearnedPermissions;

/// Permission decision
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Confirm,
    Deny,
}

impl PermissionDecision {
    pub fn as_str(&self) -> &'static str {
        match self {
            PermissionDecision::Allow => "allow",
            PermissionDecision::Confirm => "confirm",
            PermissionDecision::Deny => "deny",
        }
    }
}

/// Permission rule for a specific scope
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionRule {
    #[serde(default)]
    pub auto_allow: Vec<String>,
    #[serde(default)]
    pub require_confirm: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Permission configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<PermissionRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roles: Option<HashMap<String, PermissionRule>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<HashMap<String, PermissionRule>>,
}

/// Permission Policy
///
/// Manages tool permission checking for slots and roles.
/// Includes learned permissions for the "permission flywheel" feature.
pub struct PermissionPolicy {
    config: RwLock<PermissionConfig>,
    config_path: PathBuf,
    learned: Option<Arc<LearnedPermissions>>,
}

impl PermissionPolicy {
    /// Create a new PermissionPolicy from a config file
    pub fn new<P: AsRef<Path>>(config_path: P) -> Self {
        Self::new_with_learned(config_path, None)
    }

    /// Create a new PermissionPolicy with optional learned permissions
    pub fn new_with_learned<P: AsRef<Path>>(
        config_path: P,
        learned: Option<Arc<LearnedPermissions>>,
    ) -> Self {
        let config_path = config_path.as_ref().to_path_buf();
        let config = Self::load_config(&config_path);

        Self {
            config: RwLock::new(config),
            config_path,
            learned,
        }
    }

    /// Get the learned permissions store (if initialized)
    pub fn learned(&self) -> Option<&Arc<LearnedPermissions>> {
        self.learned.as_ref()
    }

    /// Load configuration from file
    fn load_config(path: &Path) -> PermissionConfig {
        if !path.exists() {
            info!(path = ?path, "No permission config found, using defaults");
            return Self::default_config();
        }

        match fs::read_to_string(path) {
            Ok(content) => match serde_yaml::from_str(&content) {
                Ok(config) => {
                    info!(path = ?path, "Permission config loaded");
                    config
                }
                Err(e) => {
                    error!(error = %e, path = ?path, "Failed to parse permission config");
                    Self::default_config()
                }
            },
            Err(e) => {
                error!(error = %e, path = ?path, "Failed to read permission config");
                Self::default_config()
            }
        }
    }

    /// Get default configuration
    fn default_config() -> PermissionConfig {
        PermissionConfig {
            default: Some(PermissionRule {
                auto_allow: vec![],
                require_confirm: vec!["*".to_string()],
                deny: vec![],
            }),
            roles: None,
            slots: None,
        }
    }

    /// Save configuration to file
    pub fn save_config(&self) -> Result<()> {
        let config = self.config.read().unwrap();
        let content = serde_yaml::to_string(&*config)?;
        fs::write(&self.config_path, content)?;
        info!(path = ?self.config_path, "Permission config saved");
        Ok(())
    }

    /// Reload configuration from file
    pub fn reload(&self) {
        let config = Self::load_config(&self.config_path);
        *self.config.write().unwrap() = config;
    }

    /// Check if any static rule (slot/role/default) has a deny for this tool.
    /// Used to prevent learned Allow from bypassing static Deny.
    /// Design principle: "静态规则永远优先于动态学习, YAML 中硬编码的 deny 不可被学习覆盖"
    fn has_static_deny(
        &self,
        config: &PermissionConfig,
        slot_id: &str,
        role: &str,
        tool_name: &str,
    ) -> bool {
        if let Some(slots) = &config.slots {
            if let Some(rule) = slots.get(slot_id) {
                if self.match_patterns(&rule.deny, tool_name) {
                    return true;
                }
            }
        }
        if let Some(roles) = &config.roles {
            if let Some(rule) = roles.get(role) {
                if self.match_patterns(&rule.deny, tool_name) {
                    return true;
                }
            }
        }
        if let Some(rule) = &config.default {
            if self.match_patterns(&rule.deny, tool_name) {
                return true;
            }
        }
        false
    }

    /// Check permission for a tool
    ///
    /// Priority chain:
    /// 1. Static Slot (permissions.yaml)
    /// 2. Dynamic Slot (learned_permissions) — blocked if any static Deny exists
    /// 3. Static Role (permissions.yaml)
    /// 4. Dynamic Role (learned_permissions) — blocked if any static Deny exists
    /// 5. Static Default (permissions.yaml)
    /// 6. Confirm (fallback)
    pub fn check_permission(
        &self,
        slot_id: &str,
        role: &str,
        tool_name: &str,
    ) -> PermissionDecision {
        let config = self.config.read().unwrap();

        // Pre-compute: does any static rule deny this tool?
        let static_deny = self.has_static_deny(&config, slot_id, role, tool_name);

        // 1. Check static slot level
        if let Some(slots) = &config.slots {
            if let Some(rule) = slots.get(slot_id) {
                if let Some(decision) = self.match_rule(rule, tool_name) {
                    debug!(
                        slot_id = %slot_id,
                        tool_name = %tool_name,
                        decision = ?decision,
                        level = "slot_static",
                        "Permission matched"
                    );
                    return decision;
                }
            }
        }

        // 2. Check dynamic slot (learned) — skip if static deny exists
        if !static_deny {
            if let Some(learned) = &self.learned {
                if let Some(decision) = learned.check_learned("slot", slot_id, tool_name) {
                    debug!(
                        slot_id = %slot_id,
                        tool_name = %tool_name,
                        decision = ?decision,
                        level = "slot_learned",
                        "Permission matched"
                    );
                    return decision;
                }
            }
        }

        // 3. Check static role level
        if let Some(roles) = &config.roles {
            if let Some(rule) = roles.get(role) {
                if let Some(decision) = self.match_rule(rule, tool_name) {
                    debug!(
                        slot_id = %slot_id,
                        role = %role,
                        tool_name = %tool_name,
                        decision = ?decision,
                        level = "role_static",
                        "Permission matched"
                    );
                    return decision;
                }
            }
        }

        // 4. Check dynamic role (learned) — skip if static deny exists
        if !static_deny {
            if let Some(learned) = &self.learned {
                if let Some(decision) = learned.check_learned("role", role, tool_name) {
                    debug!(
                        slot_id = %slot_id,
                        role = %role,
                        tool_name = %tool_name,
                        decision = ?decision,
                        level = "role_learned",
                        "Permission matched"
                    );
                    return decision;
                }
            }
        }

        // 5. Check default level
        if let Some(rule) = &config.default {
            if let Some(decision) = self.match_rule(rule, tool_name) {
                debug!(
                    tool_name = %tool_name,
                    decision = ?decision,
                    level = "default",
                    "Permission matched"
                );
                return decision;
            }
        }

        // 6. Default to confirm
        PermissionDecision::Confirm
    }

    /// Match a rule against a tool name
    fn match_rule(&self, rule: &PermissionRule, tool_name: &str) -> Option<PermissionDecision> {
        // Check deny (highest priority)
        if self.match_patterns(&rule.deny, tool_name) {
            return Some(PermissionDecision::Deny);
        }

        // Check auto_allow
        if self.match_patterns(&rule.auto_allow, tool_name) {
            return Some(PermissionDecision::Allow);
        }

        // Check require_confirm
        if self.match_patterns(&rule.require_confirm, tool_name) {
            return Some(PermissionDecision::Confirm);
        }

        None
    }

    /// Match patterns against a tool name
    fn match_patterns(&self, patterns: &[String], tool_name: &str) -> bool {
        patterns.iter().any(|p| self.match_pattern(p, tool_name))
    }

    /// Match a single pattern (supports wildcards)
    fn match_pattern(&self, pattern: &str, tool_name: &str) -> bool {
        // Exact match
        if pattern == tool_name {
            return true;
        }

        // Wildcard *
        if pattern == "*" {
            return true;
        }

        // Prefix wildcard: mcp_secret_*
        if pattern.ends_with('*') {
            let prefix = &pattern[..pattern.len() - 1];
            if tool_name.starts_with(prefix) {
                return true;
            }
        }

        // Suffix wildcard: *_delete
        if pattern.starts_with('*') {
            let suffix = &pattern[1..];
            if tool_name.ends_with(suffix) {
                return true;
            }
        }

        // Middle wildcard: mcp_*_get
        if pattern.contains('*') {
            // Convert pattern to regex
            let regex_pattern = format!("^{}$", pattern.replace('*', ".*"));
            if let Ok(re) = Regex::new(&regex_pattern) {
                if re.is_match(tool_name) {
                    return true;
                }
            }
        }

        false
    }

    // ============ Configuration modification API ============

    /// Set role rule
    pub fn set_role_rule(&self, role: &str, rule: PermissionRule) {
        let mut config = self.config.write().unwrap();
        if config.roles.is_none() {
            config.roles = Some(HashMap::new());
        }
        config
            .roles
            .as_mut()
            .unwrap()
            .insert(role.to_string(), rule);
        drop(config);
        let _ = self.save_config();
    }

    /// Set slot rule
    pub fn set_slot_rule(&self, slot_id: &str, rule: PermissionRule) {
        let mut config = self.config.write().unwrap();
        if config.slots.is_none() {
            config.slots = Some(HashMap::new());
        }
        config
            .slots
            .as_mut()
            .unwrap()
            .insert(slot_id.to_string(), rule);
        drop(config);
        let _ = self.save_config();
    }

    /// Add to role's auto_allow
    pub fn add_role_auto_allow(&self, role: &str, pattern: &str) {
        let mut config = self.config.write().unwrap();
        if config.roles.is_none() {
            config.roles = Some(HashMap::new());
        }

        let roles = config.roles.as_mut().unwrap();
        if !roles.contains_key(role) {
            roles.insert(role.to_string(), PermissionRule::default());
        }

        let rule = roles.get_mut(role).unwrap();
        if !rule.auto_allow.contains(&pattern.to_string()) {
            rule.auto_allow.push(pattern.to_string());
        }
        drop(config);
        let _ = self.save_config();
    }

    /// Add to slot's auto_allow
    pub fn add_slot_auto_allow(&self, slot_id: &str, pattern: &str) {
        let mut config = self.config.write().unwrap();
        if config.slots.is_none() {
            config.slots = Some(HashMap::new());
        }

        let slots = config.slots.as_mut().unwrap();
        if !slots.contains_key(slot_id) {
            slots.insert(slot_id.to_string(), PermissionRule::default());
        }

        let rule = slots.get_mut(slot_id).unwrap();
        if !rule.auto_allow.contains(&pattern.to_string()) {
            rule.auto_allow.push(pattern.to_string());
        }
        drop(config);
        let _ = self.save_config();
    }

    /// Get the full configuration
    pub fn get_config(&self) -> PermissionConfig {
        self.config.read().unwrap().clone()
    }

    /// Get role rule
    pub fn get_role_rule(&self, role: &str) -> Option<PermissionRule> {
        let config = self.config.read().unwrap();
        config.roles.as_ref()?.get(role).cloned()
    }

    /// Get slot rule
    pub fn get_slot_rule(&self, slot_id: &str) -> Option<PermissionRule> {
        let config = self.config.read().unwrap();
        config.slots.as_ref()?.get(slot_id).cloned()
    }

    /// Learn a permission decision
    pub fn learn(
        &self,
        scope_type: &str,
        scope_id: &str,
        tool_pattern: &str,
        decision: &str,
        param_pattern: Option<&str>,
    ) -> Result<()> {
        if let Some(learned) = &self.learned {
            learned.learn(scope_type, scope_id, tool_pattern, decision, param_pattern)?;
        }
        Ok(())
    }

    /// Get learned permissions for a scope
    pub fn get_learned(
        &self,
        scope_type: &str,
        scope_id: &str,
    ) -> Result<Vec<super::LearnedPermission>> {
        if let Some(learned) = &self.learned {
            learned.get_for_scope(scope_type, scope_id)
        } else {
            Ok(vec![])
        }
    }

    /// Get all learned permissions
    pub fn get_all_learned(&self) -> Result<Vec<super::LearnedPermission>> {
        if let Some(learned) = &self.learned {
            learned.get_all()
        } else {
            Ok(vec![])
        }
    }

    /// Forget a learned permission
    pub fn forget(&self, scope_type: &str, scope_id: &str, tool_pattern: &str) -> Result<bool> {
        if let Some(learned) = &self.learned {
            learned.forget(scope_type, scope_id, tool_pattern)
        } else {
            Ok(false)
        }
    }

    /// Check if learned permissions are enabled
    pub fn has_learned(&self) -> bool {
        self.learned.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_policy() -> PermissionPolicy {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("permissions.yaml");
        PermissionPolicy::new(&config_path)
    }

    #[test]
    fn test_default_config() {
        let policy = create_test_policy();

        // Default should require confirmation for everything
        let decision = policy.check_permission("slot-1", "worker", "some_tool");
        assert_eq!(decision, PermissionDecision::Confirm);
    }

    #[test]
    fn test_exact_match() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("permissions.yaml");

        // Create config with exact match
        let config = PermissionConfig {
            default: None,
            roles: Some({
                let mut m = HashMap::new();
                m.insert(
                    "worker".to_string(),
                    PermissionRule {
                        auto_allow: vec!["read_file".to_string()],
                        require_confirm: vec![],
                        deny: vec!["delete_file".to_string()],
                    },
                );
                m
            }),
            slots: None,
        };

        let content = serde_yaml::to_string(&config).unwrap();
        fs::write(&config_path, content).unwrap();

        let policy = PermissionPolicy::new(&config_path);

        assert_eq!(
            policy.check_permission("slot-1", "worker", "read_file"),
            PermissionDecision::Allow
        );
        assert_eq!(
            policy.check_permission("slot-1", "worker", "delete_file"),
            PermissionDecision::Deny
        );
    }

    #[test]
    fn test_wildcard_patterns() {
        let policy = create_test_policy();

        // Test prefix wildcard
        assert!(policy.match_pattern("app_*", "app_secret_get"));
        assert!(!policy.match_pattern("app_*", "other_tool"));

        // Test suffix wildcard
        assert!(policy.match_pattern("*_delete", "file_delete"));
        assert!(!policy.match_pattern("*_delete", "delete_file"));

        // Test middle wildcard
        assert!(policy.match_pattern("app_*_get", "app_secret_get"));
        assert!(policy.match_pattern("app_*_get", "app_user_info_get"));
        assert!(!policy.match_pattern("app_*_get", "app_secret_set"));

        // Test universal wildcard
        assert!(policy.match_pattern("*", "any_tool"));
    }

    #[test]
    fn test_slot_overrides_role() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("permissions.yaml");

        let config = PermissionConfig {
            default: None,
            roles: Some({
                let mut m = HashMap::new();
                m.insert(
                    "worker".to_string(),
                    PermissionRule {
                        auto_allow: vec!["read_file".to_string()],
                        require_confirm: vec![],
                        deny: vec![],
                    },
                );
                m
            }),
            slots: Some({
                let mut m = HashMap::new();
                m.insert(
                    "slot-1".to_string(),
                    PermissionRule {
                        auto_allow: vec![],
                        require_confirm: vec![],
                        deny: vec!["read_file".to_string()],
                    },
                );
                m
            }),
        };

        let content = serde_yaml::to_string(&config).unwrap();
        fs::write(&config_path, content).unwrap();

        let policy = PermissionPolicy::new(&config_path);

        // Slot rule denies, even though role allows
        assert_eq!(
            policy.check_permission("slot-1", "worker", "read_file"),
            PermissionDecision::Deny
        );

        // Different slot uses role rule
        assert_eq!(
            policy.check_permission("slot-2", "worker", "read_file"),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn test_add_auto_allow() {
        let policy = create_test_policy();

        // Initially not allowed
        let decision = policy.check_permission("slot-1", "worker", "new_tool");
        assert_eq!(decision, PermissionDecision::Confirm); // Falls back to default

        // Add to role auto_allow
        policy.add_role_auto_allow("worker", "new_tool");

        let decision = policy.check_permission("slot-1", "worker", "new_tool");
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn test_static_deny_blocks_learned_allow() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("permissions.yaml");
        let db_path = dir.path().join("learned.db");

        // Static role denies dangerous_tool
        let config = PermissionConfig {
            default: None,
            roles: Some({
                let mut m = HashMap::new();
                m.insert(
                    "worker".to_string(),
                    PermissionRule {
                        auto_allow: vec![],
                        require_confirm: vec![],
                        deny: vec!["dangerous_tool".to_string()],
                    },
                );
                m
            }),
            slots: None,
        };

        let content = serde_yaml::to_string(&config).unwrap();
        fs::write(&config_path, content).unwrap();

        let learned = Arc::new(super::super::LearnedPermissions::new(&db_path).unwrap());
        // Learned slot-level allow for dangerous_tool
        learned
            .learn("slot", "slot-1", "dangerous_tool", "allow", None)
            .unwrap();

        let policy = PermissionPolicy::new_with_learned(&config_path, Some(learned));

        // Static role deny should block learned slot allow
        assert_eq!(
            policy.check_permission("slot-1", "worker", "dangerous_tool"),
            PermissionDecision::Deny
        );
    }

    #[test]
    fn test_deny_has_highest_priority() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("permissions.yaml");

        let config = PermissionConfig {
            default: None,
            roles: Some({
                let mut m = HashMap::new();
                m.insert(
                    "worker".to_string(),
                    PermissionRule {
                        auto_allow: vec!["dangerous_tool".to_string()],
                        require_confirm: vec![],
                        deny: vec!["dangerous_tool".to_string()],
                    },
                );
                m
            }),
            slots: None,
        };

        let content = serde_yaml::to_string(&config).unwrap();
        fs::write(&config_path, content).unwrap();

        let policy = PermissionPolicy::new(&config_path);

        // Deny takes priority over auto_allow
        assert_eq!(
            policy.check_permission("slot-1", "worker", "dangerous_tool"),
            PermissionDecision::Deny
        );
    }
}
