//! Learned Permissions - SQLite-based permission learning
//!
//! Stores learned permission decisions in SQLite to build a "permission flywheel"
//! that auto-approves user-approved tools.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;
use tracing::{debug, info};

/// A learned permission entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnedPermission {
    pub id: i64,
    pub scope_type: String,      // "role" | "slot"
    pub scope_id: String,        // role name or slot_id
    pub tool_pattern: String,   // tool name or pattern
    pub decision: String,        // "allow" | "deny"
    pub param_pattern: Option<String>, // regex pattern for high-risk tools
    pub learned_at: String,
    pub last_used_at: Option<String>,
    pub use_count: i64,
}

/// Learned Permissions store using SQLite
pub struct LearnedPermissions {
    conn: Mutex<Connection>,
}

/// Tools that should never be auto-learned.
/// Bash is always never-learn because commands are too varied to safely auto-approve.
/// Each Bash invocation must be individually confirmed by the user.
const NEVER_LEARN_TOOLS: &[&str] = &["Bash"];

impl LearnedPermissions {
    /// Create a new LearnedPermissions store
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Initialize database schema
    fn init_schema(conn: &Connection) -> Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS learned_permissions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scope_type TEXT NOT NULL,
                scope_id TEXT NOT NULL,
                tool_pattern TEXT NOT NULL,
                decision TEXT NOT NULL CHECK(decision IN ('allow', 'deny')),
                param_pattern TEXT DEFAULT '',
                learned_at TEXT DEFAULT (datetime('now')),
                last_used_at TEXT,
                use_count INTEGER DEFAULT 1,
                UNIQUE(scope_type, scope_id, tool_pattern, param_pattern)
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_learned_lookup
             ON learned_permissions(scope_type, scope_id, tool_pattern)",
            [],
        )?;

        info!("Learned permissions schema initialized");
        Ok(())
    }

    /// Check if a tool should never be auto-learned.
    /// Bash is always never-learn — too many bypass vectors (`;`, `||`, `bash -c`, etc.)
    /// to safely enumerate. Each Bash command must be individually confirmed.
    /// Non-Bash tools (Read, Write, Edit, etc.) are always learnable.
    pub fn is_never_learn(tool_name: &str, _bash_command: Option<&str>) -> bool {
        NEVER_LEARN_TOOLS.iter().any(|t| *t == tool_name)
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
        // Check never-learn list (pass bash command from param_pattern for Bash tools)
        if Self::is_never_learn(tool_pattern, param_pattern) {
            debug!(tool = %tool_pattern, "Skipping never-learn tool");
            return Ok(());
        }

        let param = param_pattern.unwrap_or("");
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO learned_permissions (scope_type, scope_id, tool_pattern, decision, param_pattern, learned_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))
             ON CONFLICT(scope_type, scope_id, tool_pattern, param_pattern)
             DO UPDATE SET
                decision = excluded.decision,
                learned_at = excluded.learned_at,
                use_count = use_count + 1",
            params![scope_type, scope_id, tool_pattern, decision, param],
        )?;

        info!(scope_type, scope_id, tool_pattern, decision, "Learned permission");
        Ok(())
    }

    /// Get learned permissions for a scope
    pub fn get_for_scope(&self, scope_type: &str, scope_id: &str) -> Result<Vec<LearnedPermission>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, scope_type, scope_id, tool_pattern, decision, param_pattern,
                    learned_at, last_used_at, use_count
             FROM learned_permissions
             WHERE scope_type = ?1 AND scope_id = ?2
             ORDER BY use_count DESC"
        )?;

        let permissions = stmt.query_map(params![scope_type, scope_id], |row| {
            Ok(LearnedPermission {
                id: row.get(0)?,
                scope_type: row.get(1)?,
                scope_id: row.get(2)?,
                tool_pattern: row.get(3)?,
                decision: row.get(4)?,
                param_pattern: row.get(5)?,
                learned_at: row.get(6)?,
                last_used_at: row.get(7)?,
                use_count: row.get(8)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        Ok(permissions)
    }

    /// Check permission from learned database (exact match only).
    /// Learned permissions store exact tool names, not patterns.
    /// Wildcard matching is for static YAML rules only.
    pub fn check_learned(
        &self,
        scope_type: &str,
        scope_id: &str,
        tool_name: &str,
    ) -> Option<super::permission::PermissionDecision> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = match conn.prepare(
            "SELECT decision FROM learned_permissions
             WHERE scope_type = ?1 AND scope_id = ?2 AND tool_pattern = ?3"
        ) {
            Ok(s) => s,
            Err(_) => return None,
        };

        let decision: Option<String> = stmt
            .query_row(params![scope_type, scope_id, tool_name], |row| {
                row.get(0)
            })
            .ok();

        if let Some(decision) = decision {
            // Update usage stats
            let _ = conn.execute(
                "UPDATE learned_permissions SET last_used_at = datetime('now'), use_count = use_count + 1
                 WHERE scope_type = ?1 AND scope_id = ?2 AND tool_pattern = ?3",
                params![scope_type, scope_id, tool_name],
            );

            debug!(scope_type, scope_id, tool_name, decision, "Matched learned permission");
            return Some(match decision.as_str() {
                "allow" => super::permission::PermissionDecision::Allow,
                "deny" => super::permission::PermissionDecision::Deny,
                _ => return None,
            });
        }

        None
    }

    /// Forget a learned permission
    pub fn forget(&self, scope_type: &str, scope_id: &str, tool_pattern: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "DELETE FROM learned_permissions WHERE scope_type = ?1 AND scope_id = ?2 AND tool_pattern = ?3",
            params![scope_type, scope_id, tool_pattern],
        )?;
        Ok(affected > 0)
    }

    /// Get all learned permissions
    pub fn get_all(&self) -> Result<Vec<LearnedPermission>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, scope_type, scope_id, tool_pattern, decision, param_pattern,
                    learned_at, last_used_at, use_count
             FROM learned_permissions
             ORDER BY use_count DESC"
        )?;

        let permissions = stmt.query_map([], |row| {
            Ok(LearnedPermission {
                id: row.get(0)?,
                scope_type: row.get(1)?,
                scope_id: row.get(2)?,
                tool_pattern: row.get(3)?,
                decision: row.get(4)?,
                param_pattern: row.get(5)?,
                learned_at: row.get(6)?,
                last_used_at: row.get(7)?,
                use_count: row.get(8)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        Ok(permissions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_learn_and_check() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let lp = LearnedPermissions::new(&db_path).unwrap();

        // Learn an allow decision
        lp.learn("role", "worker", "read_file", "allow", None).unwrap();

        // Check should return allow
        let result = lp.check_learned("role", "worker", "read_file");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), super::super::permission::PermissionDecision::Allow);

        // Learn a deny decision
        lp.learn("role", "worker", "delete_file", "deny", None).unwrap();

        let result = lp.check_learned("role", "worker", "delete_file");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), super::super::permission::PermissionDecision::Deny);
    }

    #[test]
    fn test_never_learn() {
        // Bash is always never-learn, regardless of command content
        assert!(LearnedPermissions::is_never_learn("Bash", Some("rm -rf /tmp")));
        assert!(LearnedPermissions::is_never_learn("Bash", Some("cargo build")));
        assert!(LearnedPermissions::is_never_learn("Bash", Some("ls -la")));
        assert!(LearnedPermissions::is_never_learn("Bash", None));

        // Non-Bash tools are always learnable
        assert!(!LearnedPermissions::is_never_learn("Write", None));
        assert!(!LearnedPermissions::is_never_learn("Edit", None));
        assert!(!LearnedPermissions::is_never_learn("Read", None));
    }
}
