use rusqlite::{params, OptionalExtension};
use super::error::DbResult;
use super::MissionDB;
use crate::ast::CodeNode;

/// Result of a file sync operation.
pub struct AstSyncResult {
    pub inserted: usize,
    pub deleted: usize,
}

/// A stored AST node with repo/file context.
#[derive(Debug, Clone)]
pub struct AstNodeRow {
    pub id: String,
    pub repo: String,
    pub file_path: String,
    pub node: CodeNode,
    pub embedding_provider: Option<String>,
}

/// AST FTS search result with rank score.
#[derive(Debug, Clone)]
pub struct AstSearchHit {
    pub id: String,
    pub repo: String,
    pub file_path: String,
    pub name: String,
    pub node_type: String,
    pub signature: String,
    pub start_line: usize,
    pub end_line: usize,
    pub is_exported: bool,
    pub docstring: Option<String>,
    pub stub_content: String,
    pub calls: Vec<String>,
    pub rank: f64,
}

impl MissionDB {
    // ============ AST Code Context ============

    /// Sync all nodes for a single file: DELETE existing + INSERT new (file-level granularity).
    /// Returns count of inserted/deleted nodes.
    pub fn ast_sync_file(
        &self,
        repo: &str,
        file_path: &str,
        commit_hash: &str,
        nodes: &[CodeNode],
    ) -> DbResult<AstSyncResult> {
        let conn = self.conn();

        // Delete existing FTS entries for this file
        conn.execute(
            "DELETE FROM ast_nodes_fts WHERE rowid IN (
                SELECT rowid FROM ast_nodes WHERE repo = ?1 AND file_path = ?2
            )",
            params![repo, file_path],
        )?;

        // Delete existing nodes for this file
        let deleted = conn.execute(
            "DELETE FROM ast_nodes WHERE repo = ?1 AND file_path = ?2",
            params![repo, file_path],
        )?;

        // Insert new nodes
        let mut inserted = 0;
        for node in nodes {
            let id = node.build_id(repo, file_path);
            let calls_json = serde_json::to_string(&node.calls).unwrap_or_default();

            conn.execute(
                "INSERT INTO ast_nodes (id, repo, file_path, node_type, name, signature,
                    start_line, end_line, is_exported, docstring, stub_content, calls)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    id, repo, file_path, node.node_type, node.name, node.signature,
                    node.start_line, node.end_line, node.is_exported as i32,
                    node.docstring, node.stub_content, calls_json,
                ],
            )?;

            // Insert into FTS
            let rowid: i64 = conn.query_row(
                "SELECT rowid FROM ast_nodes WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT INTO ast_nodes_fts(rowid, name, signature, docstring, stub_content)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![rowid, node.name, node.signature, node.docstring, node.stub_content],
            )?;

            inserted += 1;
        }

        // Upsert file metadata
        conn.execute(
            "INSERT OR REPLACE INTO ast_file_meta (repo, file_path, commit_hash, node_count)
             VALUES (?1, ?2, ?3, ?4)",
            params![repo, file_path, commit_hash, nodes.len()],
        )?;

        Ok(AstSyncResult { inserted, deleted })
    }

    /// Delete all nodes for a file (when file is deleted from repo).
    pub fn ast_delete_file(&self, repo: &str, file_path: &str) -> DbResult<usize> {
        let conn = self.conn();

        conn.execute(
            "DELETE FROM ast_nodes_fts WHERE rowid IN (
                SELECT rowid FROM ast_nodes WHERE repo = ?1 AND file_path = ?2
            )",
            params![repo, file_path],
        )?;

        let deleted = conn.execute(
            "DELETE FROM ast_nodes WHERE repo = ?1 AND file_path = ?2",
            params![repo, file_path],
        )?;

        conn.execute(
            "DELETE FROM ast_file_meta WHERE repo = ?1 AND file_path = ?2",
            params![repo, file_path],
        )?;

        Ok(deleted)
    }

    /// FTS search across all AST nodes. Returns ranked results.
    pub fn ast_search(&self, query: &str, limit: usize) -> DbResult<Vec<AstSearchHit>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT n.id, n.repo, n.file_path, n.name, n.node_type, n.signature,
                    n.start_line, n.end_line, n.is_exported, n.docstring, n.stub_content, n.calls,
                    f.rank
             FROM ast_nodes_fts f
             JOIN ast_nodes n ON n.rowid = f.rowid
             WHERE ast_nodes_fts MATCH ?1
             ORDER BY f.rank
             LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![query, limit as i64], |row| {
            let calls_json: String = row.get(11)?;
            let calls: Vec<String> = serde_json::from_str(&calls_json).unwrap_or_default();
            Ok(AstSearchHit {
                id: row.get(0)?,
                repo: row.get(1)?,
                file_path: row.get(2)?,
                name: row.get(3)?,
                node_type: row.get(4)?,
                signature: row.get(5)?,
                start_line: row.get::<_, usize>(6)?,
                end_line: row.get::<_, usize>(7)?,
                is_exported: row.get::<_, i32>(8)? != 0,
                docstring: row.get(9)?,
                stub_content: row.get(10)?,
                calls,
                rank: row.get(12)?,
            })
        })?;
        let mut results = Vec::new();
        for r in rows { results.push(r?); }
        Ok(results)
    }

    /// Get all nodes for a specific file.
    pub fn ast_get_file_nodes(&self, repo: &str, file_path: &str) -> DbResult<Vec<AstNodeRow>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, repo, file_path, node_type, name, signature,
                    start_line, end_line, is_exported, docstring, stub_content, calls,
                    embedding_provider
             FROM ast_nodes WHERE repo = ?1 AND file_path = ?2
             ORDER BY start_line"
        )?;
        let rows = stmt.query_map(params![repo, file_path], |row| {
            let calls_json: String = row.get(11)?;
            let calls: Vec<String> = serde_json::from_str(&calls_json).unwrap_or_default();
            Ok(AstNodeRow {
                id: row.get(0)?,
                repo: row.get(1)?,
                file_path: row.get(2)?,
                node: CodeNode {
                    node_type: row.get(3)?,
                    name: row.get(4)?,
                    signature: row.get(5)?,
                    start_line: row.get::<_, usize>(6)?,
                    end_line: row.get::<_, usize>(7)?,
                    is_exported: row.get::<_, i32>(8)? != 0,
                    docstring: row.get(9)?,
                    stub_content: row.get(10)?,
                    calls,
                },
                embedding_provider: row.get(12)?,
            })
        })?;
        let mut results = Vec::new();
        for r in rows { results.push(r?); }
        Ok(results)
    }

    /// Cross-file association: given a name, find related struct/enum/trait definitions.
    pub fn ast_find_related(&self, name: &str, limit: usize) -> DbResult<Vec<AstSearchHit>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, repo, file_path, name, node_type, signature,
                    start_line, end_line, is_exported, docstring, stub_content, calls
             FROM ast_nodes
             WHERE name = ?1 AND node_type IN ('struct', 'enum', 'trait')
             LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![name, limit as i64], |row| {
            let calls_json: String = row.get(11)?;
            let calls: Vec<String> = serde_json::from_str(&calls_json).unwrap_or_default();
            Ok(AstSearchHit {
                id: row.get(0)?,
                repo: row.get(1)?,
                file_path: row.get(2)?,
                name: row.get(3)?,
                node_type: row.get(4)?,
                signature: row.get(5)?,
                start_line: row.get::<_, usize>(6)?,
                end_line: row.get::<_, usize>(7)?,
                is_exported: row.get::<_, i32>(8)? != 0,
                docstring: row.get(9)?,
                stub_content: row.get(10)?,
                calls,
                rank: 0.0,
            })
        })?;
        let mut results = Vec::new();
        for r in rows { results.push(r?); }
        Ok(results)
    }

    /// Get the commit hash cursor for a file (for incremental sync).
    pub fn ast_get_file_commit(&self, repo: &str, file_path: &str) -> DbResult<Option<String>> {
        let conn = self.read_conn();
        Ok(conn.query_row(
            "SELECT commit_hash FROM ast_file_meta WHERE repo = ?1 AND file_path = ?2",
            params![repo, file_path],
            |row| row.get(0),
        ).optional()?)
    }

    /// Get stats for AST index.
    pub fn ast_stats(&self) -> DbResult<AstStats> {
        let conn = self.read_conn();
        let total_nodes: i64 = conn.query_row(
            "SELECT COUNT(*) FROM ast_nodes", [], |row| row.get(0),
        )?;
        let total_files: i64 = conn.query_row(
            "SELECT COUNT(*) FROM ast_file_meta", [], |row| row.get(0),
        )?;
        let total_repos: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT repo) FROM ast_file_meta", [], |row| row.get(0),
        )?;
        let embedded_nodes: i64 = conn.query_row(
            "SELECT COUNT(*) FROM ast_nodes WHERE embedding IS NOT NULL", [], |row| row.get(0),
        )?;
        Ok(AstStats {
            total_nodes: total_nodes as usize,
            total_files: total_files as usize,
            total_repos: total_repos as usize,
            embedded_nodes: embedded_nodes as usize,
        })
    }

    /// Update embedding for a node (used by embedding pipeline).
    pub fn ast_set_embedding(
        &self,
        node_id: &str,
        embedding: &[u8],
        provider: &str,
    ) -> DbResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE ast_nodes SET embedding = ?1, embedding_provider = ?2 WHERE id = ?3",
            params![embedding, provider, node_id],
        )?;
        Ok(())
    }

    /// Find nodes without embeddings (for batch embedding pipeline).
    pub fn ast_find_unembedded(&self, limit: usize) -> DbResult<Vec<(String, String, String)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, repo, file_path FROM ast_nodes
             WHERE embedding IS NULL
             ORDER BY is_exported DESC, rowid
             LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        let mut results = Vec::new();
        for r in rows { results.push(r?); }
        Ok(results)
    }

    /// Get node by ID (for embedding text generation).
    pub fn ast_get_node(&self, node_id: &str) -> DbResult<Option<AstNodeRow>> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT id, repo, file_path, node_type, name, signature,
                    start_line, end_line, is_exported, docstring, stub_content, calls,
                    embedding_provider
             FROM ast_nodes WHERE id = ?1",
            params![node_id],
            |row| {
                let calls_json: String = row.get(11)?;
                let calls: Vec<String> = serde_json::from_str(&calls_json).unwrap_or_default();
                Ok(AstNodeRow {
                    id: row.get(0)?,
                    repo: row.get(1)?,
                    file_path: row.get(2)?,
                    node: CodeNode {
                        node_type: row.get(3)?,
                        name: row.get(4)?,
                        signature: row.get(5)?,
                        start_line: row.get::<_, usize>(6)?,
                        end_line: row.get::<_, usize>(7)?,
                        is_exported: row.get::<_, i32>(8)? != 0,
                        docstring: row.get(9)?,
                        stub_content: row.get(10)?,
                        calls,
                    },
                    embedding_provider: row.get(12)?,
                })
            },
        ).optional().map_err(Into::into)
    }
}

/// Stats for AST index.
#[derive(Debug, Clone)]
pub struct AstStats {
    pub total_nodes: usize,
    pub total_files: usize,
    pub total_repos: usize,
    pub embedded_nodes: usize,
}
