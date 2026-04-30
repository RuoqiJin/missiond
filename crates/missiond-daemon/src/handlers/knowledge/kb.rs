use crate::state::AppState;
use crate::state::EmbeddingTask;
use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

mod analyze;
mod args;
mod compact;
mod conflicts;
mod discovery;
mod gc;
mod import;
mod mutate;
mod ops;
mod quality;
mod query;

use analyze::handle_kb_analyze;
use args::KBRememberArgs;
use compact::handle_kb_compact;
use conflicts::detect_kb_conflicts;
use discovery::handle_kb_discover;
use gc::handle_kb_gc;
use import::handle_kb_import;
use mutate::{
    handle_kb_batch_forget, handle_kb_batch_set_project, handle_kb_forget, handle_kb_update,
};
use ops::{handle_kb_execute_plan, handle_kb_queue_status};
use quality::check_content_quality;
use query::{handle_kb_get, handle_kb_list, handle_kb_search};

// @beacon: knowledge
pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Merged tool dispatch: map unified tool names to legacy handler names
    let (name, args) = match name {
        "mission_kb_query" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("search");
            let legacy = match action {
                "get" => "mission_kb_get",
                "list" => "mission_kb_list",
                _ => "mission_kb_search",
            };
            (legacy, args)
        }
        "mission_kb_mutate" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("forget");
            let legacy = match action {
                "update" => "mission_kb_update",
                "import" => "mission_kb_import",
                "forget" if args.get("keys").is_some() => "mission_kb_batch_forget",
                _ => "mission_kb_forget",
            };
            (legacy, args)
        }
        "mission_kb_ops" => {
            let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("gc");
            if action == "compact" {
                return handle_kb_compact(state, args).await;
            }
            let legacy = match action {
                "analyze" => "mission_kb_analyze",
                "discover" => "mission_kb_discover",
                "queue_status" => "mission_kb_queue_status",
                "execute_plan" => "mission_kb_execute_plan",
                _ => "mission_kb_gc",
            };
            (legacy, args)
        }
        other => (other, args),
    };
    match name {
        // ===== Knowledge Base (Jarvis Memory) =====
        "mission_kb_remember" => {
            let args: KBRememberArgs = serde_json::from_value(args)?;
            // Content guard: reject verbose debug logs / stack traces
            if let Some(rejection) =
                check_content_quality(&args.summary, &args.detail, Some(&args.category))
            {
                return Ok(ToolResult::error(&rejection));
            }
            let input = missiond_core::types::KBRememberInput {
                category: args.category,
                key: args.key,
                summary: args.summary,
                detail: args.detail,
                source: args.source,
                confidence: args.confidence,
                project_id: args.project.clone(),
            };
            let result = state
                .store
                .kb_remember(&input)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            // Trigger async embedding update via Worker (avoids block_in_place in MCP handler)
            let _ = state
                .embedding_tx
                .try_send(EmbeddingTask::ProcessKBEntry(result.entry.id.clone()));

            // Auto-edge: if detail contains consolidated_from, add supersedes edges
            if result.action == "created" || result.action == "updated" {
                if let Some(ref detail) = result.entry.detail {
                    if let Some(from_keys) =
                        detail.get("consolidated_from").and_then(|v| v.as_array())
                    {
                        for key_val in from_keys {
                            if let Some(key) = key_val.as_str() {
                                if let Ok(Some(target_id)) = state.store.kb_get_id_by_key(key).await
                                {
                                    let _ = state
                                        .store
                                        .kb_add_edge(
                                            &result.entry.id,
                                            &target_id,
                                            "supersedes",
                                            1.0,
                                        )
                                        .await;
                                }
                            }
                        }
                    }
                }
            }

            // Phase 2: AST-KB linking — if detail contains symbol/file_hint, create graph link
            if result.action == "created" || result.action == "updated" {
                if let Some(ref detail) = result.entry.detail {
                    let symbol = detail.get("symbol").and_then(|v| v.as_str());
                    let file_hint = detail.get("file_hint").and_then(|v| v.as_str());
                    if let Some(sym) = symbol {
                        // Search AST for this symbol to get ast_node_id
                        let ast_node_id = state
                            .store
                            .ast_find_related(sym, 1)
                            .await
                            .ok()
                            .and_then(|hits| hits.into_iter().next().map(|h| h.id));
                        let _ = state
                            .store
                            .kb_add_ast_link(
                                &result.entry.id,
                                sym,
                                file_hint,
                                ast_node_id.as_deref(),
                                "related_to",
                                0.8,
                            )
                            .await;
                    }
                }
            }

            // Emit KBBatchMutated for event-driven FTS rebuild / consolidation triggers
            let _ = state
                .bus
                .publish_memory(missiond_core::event::events::MemoryEvent::KBBatchMutated {
                    count: 1,
                    categories: vec![input.category.clone()],
                    action: result.action.clone(),
                })
                .await;

            // Conflict detection: for new entries, check semantic similarity against existing KB
            let conflicts = if result.action == "created" {
                detect_kb_conflicts(state, &result.entry).await
            } else {
                vec![]
            };

            if conflicts.is_empty() {
                Ok(ToolResult::json_pretty(&result))
            } else {
                // Auto-downweight: if a conflicting entry has higher confidence,
                // reduce the new entry's confidence to half of the highest conflicting entry.
                // This ensures the new entry ranks below established knowledge in retrieval.
                let max_conflict_conf = conflicts
                    .iter()
                    .filter_map(|c| c["confidence"].as_f64())
                    .fold(0.0f64, f64::max);
                if max_conflict_conf > result.entry.confidence {
                    let reduced = (max_conflict_conf / 2.0).max(0.1);
                    let _ = state
                        .store
                        .kb_adjust_confidence(
                            &result.entry.id,
                            reduced - result.entry.confidence, // delta to reach target
                        )
                        .await;
                }

                // Add contradicts edges for detected conflicts
                for c in &conflicts {
                    if let Some(cid) = c.get("id").and_then(|v| v.as_str()) {
                        let _ = state
                            .store
                            .kb_add_edge(&result.entry.id, cid, "contradicts", 0.8)
                            .await;
                    }
                }

                let mut output = serde_json::to_value(&result)?;
                output["conflicts"] = serde_json::json!(conflicts);
                output["conflictWarning"] = serde_json::json!(format!(
                    "⚠️ 检测到 {} 条语义相似的已有条目，可能存在规则冲突。新条目已自动降权。请用 mission_kb_update 合并或 mission_kb_forget 删除冲突方。",
                    conflicts.len()
                ));
                Ok(ToolResult::json_pretty(&output))
            }
        }
        "mission_kb_forget" => handle_kb_forget(state, args).await,
        "mission_kb_batch_forget" => handle_kb_batch_forget(state, args).await,
        "mission_kb_batch_set_project" => handle_kb_batch_set_project(state, args).await,
        "mission_kb_update" => handle_kb_update(state, args).await,
        "mission_kb_search" => handle_kb_search(state, args).await,
        "mission_kb_get" => handle_kb_get(state, args).await,
        "mission_kb_list" => handle_kb_list(state, args).await,
        "mission_kb_import" => handle_kb_import(state, args).await,

        "mission_kb_discover" => handle_kb_discover(state, args).await,

        "mission_kb_gc" => handle_kb_gc(state, args).await,

        // ===== KB Analysis (via external AI) =====
        "mission_kb_analyze" => handle_kb_analyze(state, args).await,

        // ===== KB Operation Queue =====
        "mission_kb_queue_status" => handle_kb_queue_status(state, args).await,
        "mission_kb_execute_plan" => handle_kb_execute_plan(state, args).await,

        // ===== Holographic Beacon (P4) =====
        "mission_beacon_list" => {
            let beacons = state
                .store
                .beacon_list()
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            Ok(ToolResult::json_pretty(&beacons))
        }
        "mission_beacon_map" => {
            #[derive(Deserialize)]
            struct BeaconMapArgs {
                name: String,
            }
            let BeaconMapArgs { name } = serde_json::from_value(args)?;
            let nodes = state
                .store
                .beacon_map(&name)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            if nodes.is_empty() {
                return Ok(ToolResult::text(format!(
                    "Beacon '{}' not found or has no nodes.",
                    name
                )));
            }
            Ok(ToolResult::json_pretty(&serde_json::json!({
                "beacon": name,
                "node_count": nodes.len(),
                "files": nodes.iter().map(|n| &n.file_path).collect::<std::collections::HashSet<_>>().len(),
                "nodes": nodes,
            })))
        }
        "mission_beacon_tag" => {
            #[derive(Deserialize)]
            struct BeaconTagArgs {
                file_path: String,
                symbol: String,
                feature: String,
                #[serde(default)]
                annotation: Option<String>,
            }
            let BeaconTagArgs {
                file_path,
                symbol,
                feature,
                annotation,
            } = serde_json::from_value(args)?;

            // Read the file and insert `// @beacon: feature` above the symbol
            let source = std::fs::read_to_string(&file_path)
                .map_err(|e| anyhow!("Cannot read file {}: {}", file_path, e))?;

            // Find the line containing the symbol declaration
            let mut target_line = None;
            for (idx, line) in source.lines().enumerate() {
                let trimmed = line.trim();
                // Match: fn symbol, struct symbol, enum symbol, impl symbol, trait symbol, pub ... fn symbol, etc.
                if trimmed.contains(&format!("fn {}", symbol))
                    || trimmed.contains(&format!("struct {}", symbol))
                    || trimmed.contains(&format!("enum {}", symbol))
                    || trimmed.contains(&format!("trait {}", symbol))
                    || trimmed.contains(&format!("impl {}", symbol))
                {
                    target_line = Some(idx);
                    break;
                }
            }

            let target_line = target_line
                .ok_or_else(|| anyhow!("Symbol '{}' not found in {}", symbol, file_path))?;

            // Check if @beacon: feature already exists on preceding lines
            let lines: Vec<&str> = source.lines().collect();
            let already_tagged = if target_line > 0 {
                (0..target_line).rev().take(5).any(|i| {
                    let l = lines[i].trim();
                    l.starts_with("//") && l.contains("@beacon:") && l.contains(&feature)
                })
            } else {
                false
            };

            if already_tagged {
                return Ok(ToolResult::text(format!(
                    "Symbol '{}' already tagged with beacon '{}'.",
                    symbol, feature
                )));
            }

            // Determine indentation from the target line
            let indent = lines[target_line].len() - lines[target_line].trim_start().len();
            let indent_str: String = lines[target_line].chars().take(indent).collect();

            // Insert the beacon comment
            let mut new_lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
            new_lines.insert(
                target_line,
                format!("{}// @beacon: {}", indent_str, feature),
            );

            std::fs::write(&file_path, new_lines.join("\n"))
                .map_err(|e| anyhow!("Cannot write file {}: {}", file_path, e))?;

            // Also immediately record in DB (sync pipeline will re-confirm on next commit)
            let beacon_id = state
                .store
                .beacon_ensure(&feature)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            // Determine repo from file_path (find repo root)
            let repo_name = std::path::Path::new(&file_path)
                .ancestors()
                .find_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                .unwrap_or_else(|| "unknown".to_string());

            let _ = state
                .store
                .beacon_node_upsert(
                    &beacon_id,
                    &repo_name,
                    &file_path,
                    &symbol,
                    annotation.as_deref(),
                )
                .await;

            Ok(ToolResult::text(format!(
                "Tagged '{}' with beacon '{}' in {}:{}",
                symbol,
                feature,
                file_path,
                target_line + 1
            )))
        }
        "mission_beacon_annotate" => {
            #[derive(Deserialize)]
            struct BeaconAnnotateArgs {
                beacon_name: String,
                file_path: String,
                symbol: String,
                annotation: String,
            }
            let BeaconAnnotateArgs {
                beacon_name,
                file_path,
                symbol,
                annotation,
            } = serde_json::from_value(args)?;

            // Find repo name from file_path
            let repo_name = std::path::Path::new(&file_path)
                .ancestors()
                .find_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                .unwrap_or_else(|| "unknown".to_string());

            let updated = state
                .store
                .beacon_node_annotate(&beacon_name, &repo_name, &file_path, &symbol, &annotation)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;

            if updated {
                Ok(ToolResult::text(format!(
                    "Annotation updated for {}::{} in beacon '{}'.",
                    file_path, symbol, beacon_name
                )))
            } else {
                Ok(ToolResult::text(format!(
                    "No matching beacon node found for {}::{} in beacon '{}'.",
                    file_path, symbol, beacon_name
                )))
            }
        }

        // ===== Code Context (P3.5) =====
        "mission_code_search" => {
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

            // FTS search
            let hits = state
                .store
                .ast_search(&query, limit * 2)
                .await
                .unwrap_or_default();

            if hits.is_empty() {
                return Ok(ToolResult::text("No code nodes found matching query."));
            }

            // Post-filter by repo, file_path prefix, node_type
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

            // Render as structured JSON
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

            // Also do cross-file association for top results
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

        _ => Err(anyhow!("Unknown kb tool: {name}")),
    }
}
