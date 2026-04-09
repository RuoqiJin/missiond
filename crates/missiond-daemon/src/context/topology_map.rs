//! Dynamic Topology Map — module-level navigation layer.
//!
//! Aggregates AST nodes by crate/directory, stores summaries in KB,
//! and provides fallback navigation when micro-level code search returns empty.
//!
//! Design: Gemini-reviewed. See docs/designs/code-intelligence-acceleration.md (Step 4)

use tracing::{debug, info};

use missiond_core::db::shared::ModuleAstSummary;
use missiond_core::db::traits::MissionStore;
use missiond_core::types::KBRememberInput;

/// KB category for module summaries.
const MODULE_CATEGORY: &str = "architecture:module";

/// Max token budget for module navigation fallback (Gemini review: 1000-2000).
const MODULE_NAV_BUDGET: usize = 1500;

/// Update module summaries in KB after AST sync.
///
/// Pure computation (DB queries + aggregation), no LLM calls.
/// Called from ast_sync_worker after FullSync or CommitSync.
pub(crate) async fn update_module_summaries(store: &dyn MissionStore, repo: &str) {
    let summaries = match store.ast_module_summaries(repo).await {
        Ok(s) => s,
        Err(e) => {
            debug!(error = %e, "Failed to generate module summaries");
            return;
        }
    };

    if summaries.is_empty() {
        return;
    }

    let mut updated = 0usize;
    for summary in &summaries {
        let key = format!("module/{}", summary.module_path);
        let detail = serde_json::json!({
            "public_types": summary.public_types,
            "public_functions": summary.public_functions,
            "total_nodes": summary.total_nodes,
            "files": summary.files,
            "file_docs": summary.file_docs,
        });

        let type_list = if summary.public_types.len() <= 8 {
            summary.public_types.join(", ")
        } else {
            format!(
                "{}, ... (+{} more)",
                summary.public_types[..6].join(", "),
                summary.public_types.len() - 6
            )
        };

        let summary_text = format!(
            "{}: {} types [{}], {} functions, {} files, {} nodes",
            summary.module_path,
            summary.public_types.len(),
            type_list,
            summary.public_functions.len(),
            summary.files.len(),
            summary.total_nodes,
        );

        let input = KBRememberInput {
            category: MODULE_CATEGORY.to_string(),
            key,
            summary: summary_text,
            detail: Some(detail),
            source: Some("ast-topology".to_string()),
            confidence: Some(1.0),
            project_id: None,
        };

        match store.kb_remember(&input).await {
            Ok(_) => updated += 1,
            Err(e) => {
                debug!(module = %summary.module_path, error = %e, "Failed to store module summary")
            }
        }
    }

    info!(modules = updated, repo = %repo, "Topology map updated");
}

/// Render module navigation XML for code prefetch fallback.
///
/// Called when micro-level AST search returns no results.
/// Provides a high-level map so Claude Code can refine its search.
pub(crate) fn render_module_navigation(
    summaries: &[ModuleAstSummary],
    _query: &str,
) -> Option<String> {
    if summaries.is_empty() {
        return None;
    }

    let mut xml = String::with_capacity(4096);
    xml.push_str("<code_context source=\"module-navigation\" hint=\"No exact code match found. Use these module landmarks to refine your search.\">\n");

    let mut tokens_used = 0usize;

    for summary in summaries {
        if tokens_used >= MODULE_NAV_BUDGET {
            break;
        }

        let mut module_xml = format!(
            "  <module path=\"{}\" nodes=\"{}\" files=\"{}\">\n",
            summary.module_path,
            summary.total_nodes,
            summary.files.len()
        );

        // Types (priority: Gemini says struct/trait > functions for architecture)
        if !summary.public_types.is_empty() {
            let types_str = if summary.public_types.len() <= 10 {
                summary.public_types.join(", ")
            } else {
                format!(
                    "{}, (+{} more)",
                    summary.public_types[..8].join(", "),
                    summary.public_types.len() - 8
                )
            };
            module_xml.push_str(&format!("    <types>{}</types>\n", types_str));
        }

        // Functions (truncated to save budget)
        if !summary.public_functions.is_empty() {
            let fns_str = if summary.public_functions.len() <= 6 {
                summary.public_functions.join(", ")
            } else {
                format!(
                    "{}, (+{} more)",
                    summary.public_functions[..4].join(", "),
                    summary.public_functions.len() - 4
                )
            };
            module_xml.push_str(&format!("    <functions>{}</functions>\n", fns_str));
        }

        // File docs (first 2 per module for context)
        for (file, doc) in summary.file_docs.iter().take(2) {
            let short_file = file.rsplit('/').next().unwrap_or(file);
            module_xml.push_str(&format!("    <doc file=\"{}\">{}</doc>\n", short_file, doc));
        }

        module_xml.push_str("  </module>\n");

        tokens_used += module_xml.len() / 4 + 1;
        xml.push_str(&module_xml);
    }

    xml.push_str("</code_context>");
    Some(xml)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_module_navigation_empty() {
        assert_eq!(render_module_navigation(&[], "test"), None);
    }

    #[test]
    fn test_render_module_navigation_basic() {
        let summaries = vec![ModuleAstSummary {
            module_path: "crates/missiond-daemon".into(),
            public_types: vec!["AppState".into(), "GeminiClient".into()],
            public_functions: vec!["code_prefetch_query".into()],
            total_nodes: 50,
            files: vec!["src/main.rs".into(), "src/state.rs".into()],
            file_docs: vec![("src/main.rs".into(), "MissionD daemon entry point".into())],
        }];
        let xml = render_module_navigation(&summaries, "test query").unwrap();
        assert!(xml.contains("module-navigation"));
        assert!(xml.contains("crates/missiond-daemon"));
        assert!(xml.contains("AppState"));
        assert!(xml.contains("GeminiClient"));
        assert!(xml.contains("code_prefetch_query"));
    }

    #[test]
    fn test_render_respects_budget() {
        // Create many large modules to exceed budget
        let summaries: Vec<ModuleAstSummary> = (0..100)
            .map(|i| ModuleAstSummary {
                module_path: format!("crates/mod-{}", i),
                public_types: (0..20).map(|j| format!("Type{}_{}", i, j)).collect(),
                public_functions: (0..20).map(|j| format!("func{}_{}", i, j)).collect(),
                total_nodes: 100,
                files: (0..10).map(|j| format!("src/file_{}.rs", j)).collect(),
                file_docs: vec![],
            })
            .collect();

        let xml = render_module_navigation(&summaries, "test").unwrap();
        // Should not contain all 100 modules due to budget
        let module_count = xml.matches("<module ").count();
        assert!(
            module_count < 100,
            "Should respect budget, got {} modules",
            module_count
        );
        assert!(module_count > 0, "Should have at least some modules");
    }
}
