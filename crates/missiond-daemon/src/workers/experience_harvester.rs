//! Experience Harvester — automatic Virtual Beacon creation from Claude Code explorations.
//!
//! Monitors completed sessions and identifies "high-value explorations" —
//! sessions where Claude Code performed significant code investigation.
//! Extracts the exploration path (files read, patterns searched, files modified)
//! and maps them to AST nodes, creating beacons that accelerate future searches.
//!
//! Design: Gemini-reviewed. See docs/designs/code-intelligence-acceleration.md

use std::collections::{HashMap, HashSet};

use tracing::{debug, info, warn};

use missiond_core::types::ToolCallRecord;

/// Default repo name for the main project.
const DEFAULT_REPO: &str = "missiond";

/// Max AST nodes per harvested beacon (prevent context explosion).
const MAX_BEACON_NODES: usize = 20;

/// Max length for beacon name slug.
const MAX_NAME_LEN: usize = 50;

/// Exploration path extracted from a session's tool calls.
#[derive(Debug)]
struct ExplorationPath {
    /// First user intent (from session context).
    query_intent: String,
    /// Files that were Read (deduplicated, ordered by first access).
    files_read: Vec<String>,
    /// Grep/Glob patterns searched.
    patterns_searched: Vec<String>,
    /// Files that were Edit/Write'd.
    files_modified: Vec<String>,
}

/// Check if a session's tool calls represent a "high-value exploration"
/// worth harvesting into a beacon.
///
/// Gemini review: lower threshold for sessions with writes (even 1-2 reads
/// establish a real need→code mapping). Pure-read sessions need more evidence.
fn is_worthy_exploration(tool_calls: &[ToolCallRecord]) -> bool {
    let reads = tool_calls.iter()
        .filter(|t| t.tool_name == "Read")
        .count();
    let searches = tool_calls.iter()
        .filter(|t| t.tool_name == "Grep" || t.tool_name == "Glob")
        .count();
    let writes = tool_calls.iter()
        .filter(|t| t.tool_name == "Edit" || t.tool_name == "Write")
        .count();
    let errors = tool_calls.iter()
        .filter(|t| t.status == "error")
        .count();

    // Reject sessions dominated by errors (likely hallucination/dead loop)
    if errors > tool_calls.len() / 2 {
        return false;
    }

    if writes > 0 {
        // Has outcome: even 2 reads + 1 write is a valid code mapping
        reads + searches >= 2
    } else {
        // Pure investigation: need significant exploration evidence
        reads + searches >= 8
    }
}

/// Extract file paths from tool call input_summary.
/// Handles formats like "file_path: /path/to/file.rs" or "path: /path/..."
fn extract_file_path(input_summary: &str) -> Option<String> {
    // Common patterns in input_summary
    for prefix in &["file_path: ", "path: ", "file: "] {
        if let Some(rest) = input_summary.strip_prefix(prefix) {
            let path = rest.split_whitespace().next().unwrap_or(rest);
            let path = path.trim_matches('"');
            if path.contains('/') {
                return Some(path.to_string());
            }
        }
    }
    // Try extracting from the raw text: look for something that looks like a path
    for word in input_summary.split_whitespace() {
        let w = word.trim_matches('"').trim_matches(',');
        if w.starts_with('/') && w.contains('.') && w.len() > 5 {
            return Some(w.to_string());
        }
    }
    None
}

/// Extract exploration path from a session's tool calls.
fn extract_exploration_path(
    tool_calls: &[ToolCallRecord],
    query_intent: String,
) -> ExplorationPath {
    let mut files_read_set = HashSet::new();
    let mut files_read = Vec::new();
    let mut patterns_searched = Vec::new();
    let mut files_modified_set = HashSet::new();
    let mut files_modified = Vec::new();

    for tc in tool_calls {
        let summary = tc.input_summary.as_deref().unwrap_or("");
        match tc.tool_name.as_str() {
            "Read" => {
                if let Some(path) = extract_file_path(summary) {
                    if files_read_set.insert(path.clone()) {
                        files_read.push(path);
                    }
                }
            }
            "Grep" | "Glob" => {
                if !summary.is_empty() {
                    patterns_searched.push(summary.to_string());
                }
            }
            "Edit" | "Write" => {
                if let Some(path) = extract_file_path(summary) {
                    if files_modified_set.insert(path.clone()) {
                        files_modified.push(path);
                    }
                }
            }
            _ => {}
        }
    }

    ExplorationPath {
        query_intent,
        files_read,
        patterns_searched,
        files_modified,
    }
}

/// Node candidate for beacon linking, with priority for sorting.
#[derive(Debug)]
struct NodeCandidate {
    name: String,
    file_path: String,
    node_type: String,
    is_exported: bool,
    is_from_modified: bool,
}

impl NodeCandidate {
    /// Priority score: higher = more important.
    /// Gemini review: struct/trait > fn, modified files > read-only.
    fn priority(&self) -> u32 {
        let mut score = 0u32;
        if self.is_from_modified { score += 100; }
        match self.node_type.as_str() {
            "struct" | "enum" | "trait" => score += 30,
            "function" | "method" if self.is_exported => score += 20,
            "impl" => score += 15,
            _ if self.is_exported => score += 10,
            _ => score += 5,
        }
        score
    }
}

/// Resolve file paths to AST node candidates (async via state.store).
///
/// Gemini review: Modified files → all nodes (not limited to exported).
/// Read-only files → only exported struct/trait.
async fn resolve_ast_candidates_async(
    state: &crate::state::AppState,
    exploration: &ExplorationPath,
) -> Vec<NodeCandidate> {
    let mut candidates = Vec::new();
    let modified_set: HashSet<&str> = exploration.files_modified.iter()
        .map(|s| s.as_str())
        .collect();

    // Process modified files first (full nodes, not just exported)
    for file_path in &exploration.files_modified {
        // Strip project prefix to get repo-relative path
        let rel_path = strip_project_prefix(file_path);
        if let Ok(nodes) = state.store.ast_get_file_nodes(DEFAULT_REPO, &rel_path).await {
            for node in &nodes {
                candidates.push(NodeCandidate {
                    name: node.node.name.clone(),
                    file_path: rel_path.clone(),
                    node_type: node.node.node_type.clone(),
                    is_exported: node.node.is_exported,
                    is_from_modified: true,
                });
            }
        }
    }

    // Process read-only files (conservative: exported struct/trait only)
    // Count reads per file to prioritize frequently-read files
    let mut read_counts: HashMap<String, usize> = HashMap::new();
    for file_path in &exploration.files_read {
        *read_counts.entry(file_path.clone()).or_default() += 1;
    }
    let mut read_files: Vec<(&String, &usize)> = read_counts.iter().collect();
    read_files.sort_by(|a, b| b.1.cmp(a.1)); // Most-read first

    for (file_path, _count) in read_files {
        if modified_set.contains(file_path.as_str()) {
            continue; // Already processed
        }
        let rel_path = strip_project_prefix(file_path);
        if let Ok(nodes) = state.store.ast_get_file_nodes(DEFAULT_REPO, &rel_path).await {
            for node in &nodes {
                let dominated_type = matches!(
                    node.node.node_type.as_str(),
                    "struct" | "enum" | "trait"
                );
                if node.node.is_exported || dominated_type {
                    candidates.push(NodeCandidate {
                        name: node.node.name.clone(),
                        file_path: rel_path.clone(),
                        node_type: node.node.node_type.clone(),
                        is_exported: node.node.is_exported,
                        is_from_modified: false,
                    });
                }
            }
        }
    }

    // Sort by priority, take top N
    candidates.sort_by(|a, b| b.priority().cmp(&a.priority()));
    candidates.truncate(MAX_BEACON_NODES);
    candidates
}

/// Strip project root prefix to get repo-relative path.
fn strip_project_prefix(path: &str) -> String {
    // Common prefixes in this project
    for prefix in &[
        "/Users/jinchen/Projects/missiond/",
        "missiond/",
    ] {
        if let Some(stripped) = path.strip_prefix(prefix) {
            return stripped.to_string();
        }
    }
    path.to_string()
}

/// Generate a slug-style beacon name from the query intent.
fn slugify_intent(intent: &str) -> String {
    let slug: String = intent
        .chars()
        .take(MAX_NAME_LEN)
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    // Collapse multiple dashes, trim edges
    let collapsed: String = slug
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if collapsed.is_empty() {
        "harvest".to_string()
    } else {
        format!("harvest-{}", collapsed)
    }
}

/// Main entry point: harvest exploration from a completed session.
///
/// Called from event_router when NarrationSessionCompleted fires.
pub(crate) async fn harvest_session(state: &crate::state::AppState, session_id: &str) {
    // 1. Get tool calls for this session
    let tool_calls = match state.store.get_tool_calls_by_session(session_id, None, 500).await {
        Ok(tc) => tc,
        Err(e) => {
            debug!(session = %session_id, error = %e, "Failed to get tool calls for harvest");
            return;
        }
    };

    if tool_calls.is_empty() {
        return;
    }

    // 2. Check if worthy
    if !is_worthy_exploration(&tool_calls) {
        debug!(
            session = %session_id,
            tools = tool_calls.len(),
            "Session not worthy of harvest"
        );
        return;
    }

    // 3. Extract query intent from first user message context
    let query_intent = tool_calls.first()
        .and_then(|tc| tc.input_summary.clone())
        .unwrap_or_else(|| session_id[..8.min(session_id.len())].to_string());

    // 4. Extract exploration path
    let exploration = extract_exploration_path(&tool_calls, query_intent);

    if exploration.files_read.is_empty() && exploration.files_modified.is_empty() {
        return;
    }

    // 5. Resolve AST nodes
    let candidates = resolve_ast_candidates_async(state, &exploration).await;

    if candidates.is_empty() {
        debug!(
            session = %session_id,
            "No AST nodes resolved for harvest"
        );
        return;
    }

    // 6. Create beacon
    let beacon_name = slugify_intent(&exploration.query_intent);

    let beacon_id = match state.store.beacon_ensure(&beacon_name).await {
        Ok(id) => id,
        Err(e) => {
            warn!(error = %e, "Failed to create harvest beacon");
            return;
        }
    };

    // Set description
    let desc = format!(
        "Auto-harvested from session {}. Modified: {:?}. Read: {} files. Patterns: {:?}",
        &session_id[..8.min(session_id.len())],
        exploration.files_modified.iter().take(3).collect::<Vec<_>>(),
        exploration.files_read.len(),
        exploration.patterns_searched.iter().take(3).collect::<Vec<_>>(),
    );
    let _ = state.store.beacon_set_description(&beacon_name, &desc).await;

    // 7. Link nodes
    let mut linked = 0usize;
    for candidate in &candidates {
        let annotation = if candidate.is_from_modified {
            Some("modified")
        } else {
            Some("read-context")
        };
        if state.store.beacon_node_upsert(
            &beacon_id,
            DEFAULT_REPO,
            &candidate.file_path,
            &candidate.name,
            annotation,
        ).await.is_ok() {
            linked += 1;
        }
    }

    // Track harvest frequency for skill synthesis
    let harvest_count = state.store.beacon_increment_harvest(&beacon_name).await.unwrap_or(0);

    info!(
        beacon = %beacon_name,
        nodes = linked,
        harvest_count,
        files_read = exploration.files_read.len(),
        files_modified = exploration.files_modified.len(),
        session = %&session_id[..8.min(session_id.len())],
        "Experience harvested into beacon"
    );

    // Skill synthesis trigger: when same area explored 3+ times, suggest creating a Skill
    if harvest_count == 3 {
        let title = format!("[Skill 草稿] 为 {} 创建操作指南", beacon_name);
        let description = format!(
            "Beacon `{beacon}` 已被 {count} 个不同会话探索，建议合成为 Skill。\n\n\
             ## 探索路径\n\
             - 常读文件: {files_read:?}\n\
             - 常改文件: {files_modified:?}\n\
             - 搜索模式: {patterns:?}\n\n\
             ## 执行指南\n\
             1. 调用 `mission_beacon_map(name=\"{beacon}\")` 查看完整代码节点\n\
             2. 调用 `mission_conversation_search(query=\"{beacon}\")` 查看历史对话上下文\n\
             3. 综合以上信息，调用 `mission_skill_upsert` 创建 Skill 草稿\n\
             4. Skill 应包含: 路径/端点/配置 + 操作步骤 + 常见问题\n\
             5. 完成后标记本任务 done",
            beacon = beacon_name,
            count = harvest_count,
            files_read = exploration.files_read.iter().take(5).collect::<Vec<_>>(),
            files_modified = exploration.files_modified.iter().take(5).collect::<Vec<_>>(),
            patterns = exploration.patterns_searched.iter().take(5).collect::<Vec<_>>(),
        );
        let input = missiond_core::types::CreateBoardTaskInput {
            title,
            description: Some(description),
            priority: Some("low".to_string()),
            category: Some("dev".to_string()),
            project: Some("missiond".to_string()),
            server: None,
            due_date: None,
            parent_id: None,
            assignee: Some("slot-coder-1".to_string()),
            auto_execute: Some(true),
            prompt_template: None,
            hidden: None,
            flow_template: None,
            depends_on: None,
            dedupe_key: Some(format!("skill-synthesis-{}", beacon_name)),
            timeout_secs: None,
            context_intent: None,
        };
        match state.store.create_board_task(&input).await {
            Ok(task) => info!(task_id = %task.id, beacon = %beacon_name, "Skill synthesis triggered"),
            Err(e) => debug!(error = %e, "Skill synthesis task creation failed (may be deduped)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_worthy_with_writes() {
        let make_tc = |name: &str| ToolCallRecord {
            id: String::new(), session_id: String::new(), message_id: None,
            tool_name: name.to_string(),
            input_summary: None, raw_input: None, output_summary: None,
            raw_output: None, status: "success".to_string(),
            duration_ms: None, timestamp: String::new(),
        };
        // 2 reads + 1 write = worthy
        let calls = vec![make_tc("Read"), make_tc("Read"), make_tc("Edit")];
        assert!(is_worthy_exploration(&calls));

        // 1 read + 1 write = not enough
        let calls = vec![make_tc("Read"), make_tc("Edit")];
        assert!(!is_worthy_exploration(&calls));
    }

    #[test]
    fn test_is_worthy_pure_investigation() {
        let make_tc = |name: &str| ToolCallRecord {
            id: String::new(), session_id: String::new(), message_id: None,
            tool_name: name.to_string(),
            input_summary: None, raw_input: None, output_summary: None,
            raw_output: None, status: "success".to_string(),
            duration_ms: None, timestamp: String::new(),
        };
        // 8 reads, no writes = worthy
        let calls: Vec<_> = (0..8).map(|_| make_tc("Read")).collect();
        assert!(is_worthy_exploration(&calls));

        // 7 reads = not enough
        let calls: Vec<_> = (0..7).map(|_| make_tc("Read")).collect();
        assert!(!is_worthy_exploration(&calls));
    }

    #[test]
    fn test_is_worthy_rejects_error_dominated() {
        let calls = vec![
            ToolCallRecord {
                id: String::new(), session_id: String::new(), message_id: None,
                tool_name: "Read".to_string(),
                input_summary: None, raw_input: None, output_summary: None,
                raw_output: None, status: "error".to_string(),
                duration_ms: None, timestamp: String::new(),
            };
            10
        ];
        assert!(!is_worthy_exploration(&calls));
    }

    #[test]
    fn test_extract_file_path() {
        assert_eq!(
            extract_file_path("file_path: /Users/jin/Projects/missiond/src/main.rs"),
            Some("/Users/jin/Projects/missiond/src/main.rs".to_string())
        );
        assert_eq!(
            extract_file_path("path: /foo/bar.rs, limit: 100"),
            Some("/foo/bar.rs,".to_string()).or(Some("/foo/bar.rs".to_string()))
        );
        assert_eq!(extract_file_path("something random"), None);
    }

    #[test]
    fn test_slugify_intent() {
        assert_eq!(slugify_intent("Fix JWT token refresh"), "harvest-fix-jwt-token-refresh");
        // Chinese chars become dashes, collapsed
        let slug = slugify_intent("修复 auth 问题");
        assert!(slug.starts_with("harvest-"), "Should start with harvest-: {}", slug);
        assert!(slug.contains("auth"), "Should contain 'auth': {}", slug);
        assert_eq!(slugify_intent(""), "harvest");
    }

    #[test]
    fn test_node_candidate_priority() {
        let modified_struct = NodeCandidate {
            name: "Foo".into(), file_path: "a.rs".into(),
            node_type: "struct".into(), is_exported: true, is_from_modified: true,
        };
        let read_fn = NodeCandidate {
            name: "bar".into(), file_path: "b.rs".into(),
            node_type: "function".into(), is_exported: true, is_from_modified: false,
        };
        assert!(modified_struct.priority() > read_fn.priority());
    }

    #[test]
    fn test_strip_project_prefix() {
        assert_eq!(
            strip_project_prefix("/Users/jinchen/Projects/missiond/crates/foo/src/lib.rs"),
            "crates/foo/src/lib.rs"
        );
        assert_eq!(strip_project_prefix("other/path.rs"), "other/path.rs");
    }
}
