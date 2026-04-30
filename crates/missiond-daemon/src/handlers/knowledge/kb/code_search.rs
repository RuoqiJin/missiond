use anyhow::Result;
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppState;

pub(super) async fn handle_code_search(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct CodeSearchArgs {
        query: String,
        #[serde(default)]
        repo: Option<String>,
        #[serde(default)]
        file_path: Option<String>,
        #[serde(default)]
        node_type: Option<String>,
        #[serde(default)]
        limit: Option<usize>,
    }
    let CodeSearchArgs {
        query,
        repo,
        file_path,
        node_type,
        limit,
    } = serde_json::from_value(args)?;
    let limit = limit.unwrap_or(20).min(50);

    let hits = state
        .store
        .ast_search(&query, limit * 2)
        .await
        .unwrap_or_default();

    if hits.is_empty() {
        return Ok(ToolResult::text("No code nodes found matching query."));
    }

    let filtered: Vec<_> = hits
        .into_iter()
        .filter(|h| {
            if let Some(ref r) = repo {
                if h.repo != *r {
                    return false;
                }
            }
            if let Some(ref fp) = file_path {
                if !h.file_path.starts_with(fp.as_str()) {
                    return false;
                }
            }
            if let Some(ref nt) = node_type {
                if h.node_type != *nt {
                    return false;
                }
            }
            true
        })
        .take(limit)
        .collect();

    if filtered.is_empty() {
        return Ok(ToolResult::text("No code nodes matched filters."));
    }

    let results: Vec<serde_json::Value> = filtered
        .iter()
        .map(|h| {
            serde_json::json!({
                "name": h.name,
                "node_type": h.node_type,
                "file_path": h.file_path,
                "repo": h.repo,
                "lines": format!("{}-{}", h.start_line, h.end_line),
                "exported": h.is_exported,
                "signature": h.signature,
                "calls": h.calls,
                "docstring": h.docstring,
                "stub": h.stub_content,
            })
        })
        .collect();

    let mut related_results: Vec<serde_json::Value> = Vec::new();
    for h in filtered.iter().take(5) {
        if h.node_type == "impl" {
            if let Ok(related) = state.store.ast_find_related(&h.name, 3).await {
                for r in related {
                    if !filtered.iter().any(|f| f.id == r.id)
                        && !related_results.iter().any(|rr| {
                            rr.get("name").and_then(|v| v.as_str()) == Some(&r.name)
                                && rr.get("node_type").and_then(|v| v.as_str())
                                    == Some(&r.node_type)
                        })
                    {
                        related_results.push(serde_json::json!({
                            "name": r.name,
                            "node_type": r.node_type,
                            "file_path": r.file_path,
                            "lines": format!("{}-{}", r.start_line, r.end_line),
                            "stub": r.stub_content,
                        }));
                    }
                }
            }
        }
    }

    let mut output = serde_json::json!({
        "query": query,
        "count": results.len(),
        "results": results,
    });
    if !related_results.is_empty() {
        output["related"] = serde_json::json!(related_results);
    }

    Ok(ToolResult::json_pretty(&output))
}
