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
                    beacons: Vec::new(),
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

    /// Load all embedded AST nodes for in-memory vector search cache.
    pub fn ast_load_all_embeddings(&self) -> DbResult<Vec<(String, Vec<f32>)>> {
        let conn = self.read_conn();
        let mut stmt = conn.prepare(
            "SELECT id, embedding FROM ast_nodes WHERE embedding IS NOT NULL"
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((id, blob))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (id, blob) = row?;
            result.push((id, crate::embedding::bytes_to_f32_vec(&blob)));
        }
        Ok(result)
    }

    /// Search AST nodes with FTS5, returning (id, rank_position) pairs for RRF merge.
    pub fn ast_search_ranked(&self, query: &str, limit: usize) -> DbResult<Vec<(String, usize)>> {
        let conn = self.read_conn();

        // Build FTS query: quote each word for phrase matching, OR them together
        let fts_query: String = query.split_whitespace()
            .filter(|w| w.len() > 1)
            .map(|w| format!("\"{}\"", w.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" OR ");

        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let mut stmt = conn.prepare(
            "SELECT n.id
             FROM ast_nodes_fts f
             JOIN ast_nodes n ON n.rowid = f.rowid
             WHERE ast_nodes_fts MATCH ?1
             ORDER BY f.rank
             LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![fts_query, limit as i64], |row| {
            row.get::<_, String>(0)
        })?;
        let mut result = Vec::new();
        for (rank, row) in rows.enumerate() {
            result.push((row?, rank));
        }
        Ok(result)
    }

    /// Get a full AstSearchHit by node ID (for rendering after RRF merge).
    pub fn ast_get_search_hit(&self, node_id: &str) -> DbResult<Option<AstSearchHit>> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT id, repo, file_path, name, node_type, signature,
                    start_line, end_line, is_exported, docstring, stub_content, calls
             FROM ast_nodes WHERE id = ?1",
            params![node_id],
            |row| {
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
            },
        ).optional().map_err(Into::into)
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
                        beacons: Vec::new(),
                    },
                    embedding_provider: row.get(12)?,
                })
            },
        ).optional().map_err(Into::into)
    }

    /// Aggregate AST nodes by module directory for topology map generation.
    ///
    /// Groups nodes by their directory prefix (crate-level or sub-directory),
    /// collecting exported types and functions per module.
    /// Gemini review: adaptive granularity — crates with >30 public nodes drill into subdirs.
    pub fn ast_module_summaries(&self, repo: &str) -> DbResult<Vec<ModuleAstSummary>> {
        use std::collections::HashMap;
        let conn = self.read_conn();

        let mut stmt = conn.prepare(
            "SELECT file_path, node_type, name, is_exported, docstring
             FROM ast_nodes WHERE repo = ?1
             ORDER BY file_path, start_line"
        )?;

        // (file_path, node_type, name, is_exported, docstring)
        let mut nodes: Vec<(String, String, String, bool, Option<String>)> = Vec::new();
        let rows = stmt.query_map(params![repo], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i32>(3)? != 0,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;
        for r in rows { nodes.push(r?); }

        if nodes.is_empty() {
            return Ok(Vec::new());
        }

        // Group by crate key
        let mut crate_indices: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, (fp, ..)) in nodes.iter().enumerate() {
            let key = extract_crate_key(fp);
            crate_indices.entry(key).or_default().push(i);
        }

        let mut summaries = Vec::new();
        const ADAPTIVE_THRESHOLD: usize = 30;

        for (crate_key, indices) in &crate_indices {
            let public_count = indices.iter()
                .filter(|&&i| nodes[i].3)
                .count();

            if public_count > ADAPTIVE_THRESHOLD {
                // Drill into subdirectories
                let mut subdir_indices: HashMap<String, Vec<usize>> = HashMap::new();
                for &i in indices {
                    let subdir = extract_subdir_key(&nodes[i].0, crate_key);
                    subdir_indices.entry(subdir).or_default().push(i);
                }
                for (subdir_key, sub_indices) in &subdir_indices {
                    summaries.push(aggregate_summary(subdir_key, sub_indices, &nodes));
                }
            } else {
                summaries.push(aggregate_summary(crate_key, indices, &nodes));
            }
        }

        summaries.sort_by(|a, b| a.module_path.cmp(&b.module_path));
        Ok(summaries)
    }
}

/// Extract crate-level key from file path.
fn extract_crate_key(file_path: &str) -> String {
    let parts: Vec<&str> = file_path.split('/').collect();
    if parts.len() >= 2 && (parts[0] == "crates" || parts[0] == "packages") {
        format!("{}/{}", parts[0], parts[1])
    } else if !parts.is_empty() {
        parts[0].to_string()
    } else {
        "root".to_string()
    }
}

/// Extract subdirectory key for drilling into large crates.
fn extract_subdir_key(file_path: &str, crate_key: &str) -> String {
    let after_crate = file_path.strip_prefix(crate_key)
        .unwrap_or(file_path)
        .trim_start_matches('/');
    let parts: Vec<&str> = after_crate.split('/').collect();
    let depth = if parts.len() > 2 { 2 } else { parts.len().saturating_sub(1) };
    if depth == 0 {
        crate_key.to_string()
    } else {
        format!("{}/{}", crate_key, parts[..depth].join("/"))
    }
}

/// Build a ModuleAstSummary from indexed node tuples.
fn aggregate_summary(
    module_path: &str,
    indices: &[usize],
    nodes: &[(String, String, String, bool, Option<String>)],
) -> ModuleAstSummary {
    let mut public_types = Vec::new();
    let mut public_functions = Vec::new();
    let mut total_nodes = 0usize;
    let mut files_set = std::collections::HashSet::new();
    let mut file_docs: Vec<(String, String)> = Vec::new();
    let mut seen_doc_files = std::collections::HashSet::new();

    for &i in indices {
        let (ref fp, ref ntype, ref name, is_exported, ref docstring) = nodes[i];
        total_nodes += 1;
        files_set.insert(fp.clone());

        if let Some(ref doc) = docstring {
            if !doc.is_empty() && seen_doc_files.insert(fp.clone()) {
                let truncated = if doc.len() > 120 {
                    let end = doc.char_indices()
                        .take_while(|(i, _)| *i < 120)
                        .last()
                        .map(|(i, c)| i + c.len_utf8())
                        .unwrap_or(120);
                    format!("{}...", &doc[..end])
                } else {
                    doc.clone()
                };
                file_docs.push((fp.clone(), truncated));
            }
        }

        if is_exported {
            match ntype.as_str() {
                "struct" | "enum" | "trait" | "type_alias" | "interface" => {
                    public_types.push(name.clone());
                }
                "function" | "method" => {
                    public_functions.push(name.clone());
                }
                _ => {}
            }
        }
    }

    let mut files: Vec<String> = files_set.into_iter().collect();
    files.sort();

    ModuleAstSummary {
        module_path: module_path.to_string(),
        public_types,
        public_functions,
        total_nodes,
        files,
        file_docs,
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

/// Aggregated module summary from AST nodes, grouped by directory prefix.
/// Used by Dynamic Topology Map (Step 4) for module-level navigation fallback.
#[derive(Debug, Clone)]
pub struct ModuleAstSummary {
    pub module_path: String,
    pub public_types: Vec<String>,
    pub public_functions: Vec<String>,
    pub total_nodes: usize,
    pub files: Vec<String>,
    pub file_docs: Vec<(String, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_crate_key() {
        assert_eq!(extract_crate_key("crates/missiond-daemon/src/main.rs"), "crates/missiond-daemon");
        assert_eq!(extract_crate_key("packages/board/src/app.tsx"), "packages/board");
        assert_eq!(extract_crate_key("src/lib.rs"), "src");
        assert_eq!(extract_crate_key("Cargo.toml"), "Cargo.toml");
    }

    #[test]
    fn test_extract_subdir_key() {
        assert_eq!(
            extract_subdir_key("crates/missiond-daemon/src/handlers/mod.rs", "crates/missiond-daemon"),
            "crates/missiond-daemon/src/handlers"
        );
        assert_eq!(
            extract_subdir_key("crates/missiond-daemon/src/main.rs", "crates/missiond-daemon"),
            "crates/missiond-daemon/src"
        );
        assert_eq!(
            extract_subdir_key("crates/missiond-core/src/db/ast.rs", "crates/missiond-core"),
            "crates/missiond-core/src/db"
        );
    }

    #[test]
    fn test_aggregate_summary() {
        let nodes = vec![
            ("crates/foo/src/main.rs".into(), "struct".into(), "AppState".into(), true, Some("Main entry".into())),
            ("crates/foo/src/main.rs".into(), "function".into(), "run".into(), true, None),
            ("crates/foo/src/lib.rs".into(), "trait".into(), "Handler".into(), true, Some("Handler trait".into())),
            ("crates/foo/src/lib.rs".into(), "function".into(), "internal".into(), false, None),
        ];
        let indices: Vec<usize> = (0..4).collect();
        let summary = aggregate_summary("crates/foo", &indices, &nodes);

        assert_eq!(summary.module_path, "crates/foo");
        assert_eq!(summary.public_types, vec!["AppState", "Handler"]);
        assert_eq!(summary.public_functions, vec!["run"]);
        assert_eq!(summary.total_nodes, 4);
        assert_eq!(summary.files.len(), 2);
        assert_eq!(summary.file_docs.len(), 2); // One doc per file
    }
}
