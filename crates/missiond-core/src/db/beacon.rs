//! Holographic Beacon DB operations (P4).
//!
//! Feature-level code annotations: `// @beacon: feature-name` in source code,
//! extracted by P2 sync pipeline, stored as read-cache in SQLite.

use rusqlite::params;
use super::error::DbResult;
use super::MissionDB;

/// A beacon (feature tag) with its node count.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BeaconInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub node_count: usize,
    pub created_at: String,
    pub updated_at: String,
}

/// A beacon node (code symbol tagged with a feature).
#[derive(Debug, Clone, serde::Serialize)]
pub struct BeaconNode {
    pub beacon_id: String,
    pub beacon_name: String,
    pub repo: String,
    pub file_path: String,
    pub symbol_name: String,
    pub annotation: Option<String>,
    // Joined from ast_nodes (may be None if node was deleted)
    pub signature: Option<String>,
    pub stub_content: Option<String>,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub node_type: Option<String>,
}

impl MissionDB {
    // ============ Beacon CRUD ============

    /// List all beacons with node counts.
    pub fn beacon_list(&self) -> DbResult<Vec<BeaconInfo>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT b.id, b.name, b.description, b.created_at, b.updated_at,
                    (SELECT COUNT(*) FROM beacon_nodes bn WHERE bn.beacon_id = b.id) as node_count
             FROM beacons b
             ORDER BY b.name"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(BeaconInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                node_count: row.get::<_, i64>(5)? as usize,
            })
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Get or create a beacon by name. Returns beacon ID.
    pub fn beacon_ensure(&self, name: &str) -> DbResult<String> {
        let conn = self.conn();
        // Try to get existing
        let existing: Option<String> = conn.query_row(
            "SELECT id FROM beacons WHERE name = ?1",
            params![name],
            |row| row.get(0),
        ).ok();

        if let Some(id) = existing {
            return Ok(id);
        }

        // Create new
        let id = format!("bcn-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        conn.execute(
            "INSERT INTO beacons (id, name) VALUES (?1, ?2)",
            params![id, name],
        )?;
        Ok(id)
    }

    /// Update beacon description.
    pub fn beacon_set_description(&self, name: &str, description: &str) -> DbResult<bool> {
        let conn = self.conn();
        let updated = conn.execute(
            "UPDATE beacons SET description = ?1, updated_at = datetime('now') WHERE name = ?2",
            params![description, name],
        )?;
        Ok(updated > 0)
    }

    /// Upsert a beacon node (from @beacon: extraction or manual tagging).
    pub fn beacon_node_upsert(
        &self,
        beacon_id: &str,
        repo: &str,
        file_path: &str,
        symbol_name: &str,
        annotation: Option<&str>,
    ) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO beacon_nodes (beacon_id, repo, file_path, symbol_name, annotation)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (beacon_id, repo, file_path, symbol_name)
             DO UPDATE SET annotation = COALESCE(?5, annotation)",
            params![beacon_id, repo, file_path, symbol_name, annotation],
        )?;
        Ok(())
    }

    /// Get all nodes for a beacon (with AST node data via JOIN).
    pub fn beacon_map(&self, beacon_name: &str) -> DbResult<Vec<BeaconNode>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT bn.beacon_id, b.name, bn.repo, bn.file_path, bn.symbol_name, bn.annotation,
                    an.signature, an.stub_content, an.start_line, an.end_line, an.node_type
             FROM beacon_nodes bn
             JOIN beacons b ON b.id = bn.beacon_id
             LEFT JOIN ast_nodes an ON an.repo = bn.repo AND an.file_path = bn.file_path AND an.name = bn.symbol_name
             WHERE b.name = ?1
             ORDER BY bn.file_path, an.start_line"
        )?;
        let rows = stmt.query_map(params![beacon_name], |row| {
            Ok(BeaconNode {
                beacon_id: row.get(0)?,
                beacon_name: row.get(1)?,
                repo: row.get(2)?,
                file_path: row.get(3)?,
                symbol_name: row.get(4)?,
                annotation: row.get(5)?,
                signature: row.get(6)?,
                stub_content: row.get(7)?,
                start_line: row.get::<_, Option<i64>>(8)?.map(|v| v as usize),
                end_line: row.get::<_, Option<i64>>(9)?.map(|v| v as usize),
                node_type: row.get(10)?,
            })
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Delete all beacon_nodes for a file (called when file is deleted or re-synced).
    pub fn beacon_nodes_delete_file(&self, repo: &str, file_path: &str) -> DbResult<usize> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM beacon_nodes WHERE repo = ?1 AND file_path = ?2",
            params![repo, file_path],
        )?;
        Ok(deleted)
    }

    /// Update annotation on a beacon node.
    pub fn beacon_node_annotate(
        &self,
        beacon_name: &str,
        repo: &str,
        file_path: &str,
        symbol_name: &str,
        annotation: &str,
    ) -> DbResult<bool> {
        let conn = self.conn();
        let updated = conn.execute(
            "UPDATE beacon_nodes SET annotation = ?1
             WHERE beacon_id = (SELECT id FROM beacons WHERE name = ?2)
               AND repo = ?3 AND file_path = ?4 AND symbol_name = ?5",
            params![annotation, beacon_name, repo, file_path, symbol_name],
        )?;
        Ok(updated > 0)
    }

    /// Search beacons by name (for prefetch matching).
    pub fn beacon_search(&self, query: &str) -> DbResult<Vec<BeaconInfo>> {
        let conn = self.read_conn();
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT b.id, b.name, b.description, b.created_at, b.updated_at,
                    (SELECT COUNT(*) FROM beacon_nodes bn WHERE bn.beacon_id = b.id) as node_count
             FROM beacons b
             WHERE b.name LIKE ?1 OR b.description LIKE ?1
             ORDER BY b.name
             LIMIT 10"
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok(BeaconInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                node_count: row.get::<_, i64>(5)? as usize,
            })
        })?;
        let mut result = Vec::new();
        for r in rows { result.push(r?); }
        Ok(result)
    }

    /// Clean up empty beacons (no nodes remaining after file deletions).
    pub fn beacon_cleanup_empty(&self) -> DbResult<usize> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM beacons WHERE id NOT IN (SELECT DISTINCT beacon_id FROM beacon_nodes)",
            [],
        )?;
        Ok(deleted)
    }
}
