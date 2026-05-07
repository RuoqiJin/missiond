use anyhow::Result;
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::lenient;
use crate::state::AppState;

#[derive(Deserialize)]
struct ContextBuildArgs {
    query: String,
}

pub(super) async fn handle_build(state: &AppState, args: Value) -> Result<ToolResult> {
    let ContextBuildArgs { query } = serde_json::from_value(args)?;
    let mut context = state.skills.build_context(&query);

    let q = query.clone();
    if let Ok(mut entries) = state.store.kb_search(&q, None).await {
        entries.sort_by(|a, b| {
            let score_a = a.confidence * (a.access_count as f64 + 1.0).ln();
            let score_b = b.confidence * (b.access_count as f64 + 1.0).ln();
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut budget: i32 = 800;
        let mut kb_block = String::new();
        for entry in &entries {
            let line = format!("- [{}] {}: {}\n", entry.category, entry.key, entry.summary);
            budget -= line.len() as i32;
            if budget < 0 {
                break;
            }
            kb_block.push_str(&line);
        }
        if !kb_block.is_empty() {
            context.push_str("\n[Knowledge Base]\n");
            context.push_str(&kb_block);
        }
    }

    Ok(ToolResult::text(context))
}

pub(super) async fn handle_resolve(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct ContextResolveArgs {
        query: String,
        skill: Option<String>,
        #[serde(default, alias = "project", alias = "projectId")]
        project_id: Option<String>,
        #[serde(default, deserialize_with = "lenient::option_bool")]
        include_board: Option<bool>,
        #[serde(default, deserialize_with = "lenient::option_bool")]
        include_kb: Option<bool>,
    }
    let args: ContextResolveArgs = serde_json::from_value(args)?;
    let include_board = args.include_board.unwrap_or(false);
    let include_kb = args.include_kb.unwrap_or(false);
    let project_config = if let Some(ref project_id) = args.project_id {
        state.project_registry.read().await.get(project_id).cloned()
    } else {
        None
    };
    let search_query = if let Some(ref project_id) = args.project_id {
        format!("{project_id} {}", args.query)
    } else {
        args.query.clone()
    };

    let mut primary_topics: Vec<String> = Vec::new();
    if let Some(ref name) = args.skill {
        primary_topics.push(name.clone());
    } else {
        if let Some(ref project_id) = args.project_id {
            if state.skills.get(project_id).is_some() {
                primary_topics.push(project_id.clone());
            } else if state
                .store
                .skill_topic_get(project_id)
                .await
                .map(|topic| topic.is_some())
                .unwrap_or(false)
            {
                primary_topics.push(project_id.clone());
            }
        }
        for s in state.skills.search(&search_query).iter().take(3) {
            if primary_topics.iter().any(|topic| topic == &s.name) {
                continue;
            }
            primary_topics.push(s.name.clone());
        }
    }

    let mut all_skill_names: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut infra_ids = std::collections::HashSet::new();
    let mut kb_categories = std::collections::HashSet::new();
    let mut skill_results: Vec<Value> = Vec::new();

    for topic_name in &primary_topics {
        if !seen.insert(topic_name.clone()) {
            continue;
        }
        all_skill_names.push(topic_name.clone());

        if let Ok(Some(topic)) = state.store.skill_topic_get(topic_name).await {
            skill_results.push(json!({
                "name": topic.topic,
                "path": topic.file_path,
                "description": topic.description,
                "matched_by": if args.skill.is_some() { "direct" } else { "query" },
            }));

            if let Some(ref rj) = topic.requires_json {
                if let Ok(req) = serde_json::from_str::<missiond_core::SkillRequires>(rj) {
                    for dep_name in &req.skills {
                        if seen.insert(dep_name.clone()) {
                            all_skill_names.push(dep_name.clone());
                            if let Ok(Some(dep_topic)) = state.store.skill_topic_get(dep_name).await
                            {
                                skill_results.push(json!({
                                    "name": dep_topic.topic,
                                    "path": dep_topic.file_path,
                                    "description": dep_topic.description,
                                    "matched_by": "dependency",
                                }));
                                if let Some(ref drj) = dep_topic.requires_json {
                                    if let Ok(dreq) =
                                        serde_json::from_str::<missiond_core::SkillRequires>(drj)
                                    {
                                        for dep2_name in &dreq.skills {
                                            if seen.insert(dep2_name.clone()) {
                                                if let Ok(Some(dep2)) =
                                                    state.store.skill_topic_get(dep2_name).await
                                                {
                                                    skill_results.push(json!({
                                                        "name": dep2.topic,
                                                        "path": dep2.file_path,
                                                        "description": dep2.description,
                                                        "matched_by": "dependency_l2",
                                                    }));
                                                }
                                            }
                                        }
                                        infra_ids.extend(dreq.infra);
                                        kb_categories.extend(dreq.kb);
                                    }
                                }
                            }
                        }
                    }
                    infra_ids.extend(req.infra);
                    kb_categories.extend(req.kb);
                }
            }
        } else if let Some(skill_meta) = state.skills.get(topic_name) {
            skill_results.push(json!({
                "name": skill_meta.name,
                "path": skill_meta.path,
                "description": skill_meta.description,
                "matched_by": if args.skill.is_some() { "direct" } else { "query" },
            }));
        }
    }

    let mut infra_results: Vec<Value> = Vec::new();
    for id in &infra_ids {
        if let Some(server) = state.infra.read().unwrap().get(id).cloned() {
            infra_results.push(json!({
                "id": server.id,
                "name": server.name,
                "host": server.host,
                "roles": server.roles,
                "matched_by": "dependency",
            }));
        }
    }

    let kb_batch = if include_kb {
        let mut results: Vec<(missiond_core::KnowledgeEntry, &'static str)> = Vec::new();
        for cat in &kb_categories {
            if let Ok(entries) = state.store.kb_search(&search_query, Some(cat)).await {
                for entry in entries.into_iter().take(5) {
                    results.push((entry, "category_filter"));
                }
            }
        }
        if results.is_empty() {
            if let Ok(entries) = state.store.kb_search(&search_query, None).await {
                for entry in entries.into_iter().take(5) {
                    results.push((entry, "query"));
                }
            }
        }
        results
    } else {
        Vec::new()
    };
    let mut kb_results: Vec<Value> = Vec::new();
    let mut kb_seen = std::collections::HashSet::new();
    for (entry, matched_by) in &kb_batch {
        if kb_seen.insert(entry.key.clone()) {
            kb_results.push(json!({
                "key": entry.key,
                "category": entry.category,
                "summary": entry.summary,
                "matched_by": matched_by,
            }));
        }
    }

    let mut board_results: Vec<Value> = Vec::new();
    if include_board {
        if let Ok(tasks) = state.store.list_board_tasks(None, false).await {
            let query_lower = args.query.to_lowercase();
            for task in tasks.iter().take(100) {
                if task.title.to_lowercase().contains(&query_lower)
                    || task.description.to_lowercase().contains(&query_lower)
                {
                    board_results.push(json!({
                        "id": task.id,
                        "title": task.title,
                        "status": task.status,
                    }));
                    if board_results.len() >= 5 {
                        break;
                    }
                }
            }
        }
    }

    let result = json!({
        "query": {
            "original": args.query,
            "augmented": search_query,
        },
        "project": project_config.map(|project| json!({
            "id": project.id,
            "path": project.path,
            "intent_path": project.intent_path,
            "active": project.active,
            "kind": project.kind,
            "parent_id": project.parent_id,
        })),
        "skills": skill_results,
        "infra": infra_results,
        "kb": kb_results,
        "board": board_results,
    });

    Ok(ToolResult::json_pretty(&result))
}
