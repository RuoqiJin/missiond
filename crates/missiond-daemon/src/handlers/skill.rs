use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use missiond_mcp::tools::ToolResult;

use crate::state::AppState;
use crate::lenient;
use crate::state::EmbeddingTask;

#[derive(Deserialize)]
struct SkillSearchArgs {
    query: String,
}

#[derive(Deserialize)]
struct ContextBuildArgs {
    query: String,
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Skill Knowledge Hub =====
        "mission_skill_list" => {
            let skills: Vec<Value> = state
                .skills
                .list()
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "name": s.name,
                        "description": s.description,
                        "aka": s.aka,
                        "path": s.path,
                    })
                })
                .collect();
            Ok(ToolResult::json_pretty(&skills))
        }
        "mission_skill_search" => {
            let SkillSearchArgs { query } = serde_json::from_value(args)?;

            let db = state.mission.db();

            // Collect all topic-level scores: (topic, name_bonus, fts_rank, vec_rank, cosine_sim, meta)
            let mut topic_scores: std::collections::HashMap<String, (f64, Option<usize>, Option<usize>, Option<f32>, Value)> = std::collections::HashMap::new();

            // 1. Name/aka exact match → bonus +0.3
            for s in state.skills.search(&query).iter().take(10) {
                topic_scores.entry(s.name.clone()).or_insert_with(|| {
                    (0.3, None, None, None, serde_json::json!({
                        "name": s.name,
                        "description": s.description,
                        "aka": s.aka,
                        "path": s.path,
                    }))
                });
            }

            // 2. FTS5 full-text search → fts_rank
            if let Ok(fts_results) = db.skill_search_fts(&query) {
                for (rank, r) in fts_results.iter().take(20).enumerate() {
                    let entry = topic_scores.entry(r.topic.clone()).or_insert_with(|| {
                        (0.0, None, None, None, serde_json::json!({
                            "name": r.topic,
                            "description": r.description,
                            "path": r.file_path,
                            "matched_section": r.section_title,
                            "snippet": r.snippet,
                        }))
                    });
                    if entry.1.is_none() { entry.1 = Some(rank); }
                }
            }

            // 3. Embedding cosine similarity → vec_rank
            if let Some(ref emb_svc) = state.embedding_service {
                let cache_guard = state.skill_embedding_cache.read().await;
                if !cache_guard.is_empty() {
                    if let Some(query_vec) = emb_svc.embed(&query) {
                        let mut sims: Vec<(usize, f32)> = cache_guard.iter().enumerate()
                            .map(|(i, (_, vec))| (i, missiond_core::embedding::cosine_similarity(&query_vec, vec)))
                            .collect();
                        sims.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                        for (rank, (idx, sim)) in sims.iter().take(10).enumerate() {
                            let topic = &cache_guard[*idx].0;
                            let entry = topic_scores.entry(topic.clone()).or_insert_with(|| {
                                (0.0, None, None, None, serde_json::json!({
                                    "name": topic,
                                }))
                            });
                            entry.2 = Some(rank);
                            entry.3 = Some(*sim);
                        }
                    }
                }
            }

            // 4. Calculate final scores: name_bonus + rrf(fts, vec, k=60)
            let mut scored: Vec<(String, f64, Value)> = topic_scores.into_iter().map(|(topic, (bonus, fts_rank, vec_rank, cosine_sim, mut meta))| {
                let rrf = missiond_core::embedding::rrf_score(fts_rank, vec_rank, 60);
                let final_score = bonus + rrf;
                meta.as_object_mut().map(|obj| {
                    obj.insert("score".to_string(), serde_json::json!(format!("{:.4}", final_score)));
                    if let Some(sim) = cosine_sim {
                        obj.insert("cosine_sim".to_string(), serde_json::json!(format!("{:.3}", sim)));
                    }
                });
                (topic, final_score, meta)
            }).collect();
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            let results: Vec<Value> = scored.iter().take(10).map(|(_, _, meta)| meta.clone()).collect();

            // Track hits
            for (topic, _, _) in scored.iter().take(5) {
                let _ = db.skill_topic_hit(topic);
            }

            Ok(ToolResult::json_pretty(&results))
        }
        "mission_context_build" => {
            let ContextBuildArgs { query } = serde_json::from_value(args)?;
            let mut context = state.skills.build_context(&query);

            // Also search KB for matching knowledge (spawn_blocking: FTS5 full KB scan)
            let q = query.clone();
            if let Ok(mut entries) = state.db_exec.run(move |db| db.kb_search(&q, None)).await {
                // Sort by confidence × log(access_count + 1) descending
                entries.sort_by(|a, b| {
                    let score_a = a.confidence * (a.access_count as f64 + 1.0).ln();
                    let score_b = b.confidence * (b.access_count as f64 + 1.0).ln();
                    score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
                });

                // Token budget: ~800 chars instead of fixed top-5
                let mut budget: i32 = 800;
                let mut kb_block = String::new();
                for entry in &entries {
                    let line = format!("- [{}] {}: {}\n", entry.category, entry.key, entry.summary);
                    budget -= line.len() as i32;
                    if budget < 0 { break; }
                    kb_block.push_str(&line);
                }
                if !kb_block.is_empty() {
                    context.push_str("\n[Knowledge Base]\n");
                    context.push_str(&kb_block);
                }
            }

            Ok(ToolResult::text(context))
        }
        "mission_context_resolve" => {
            #[derive(Deserialize)]
            struct ContextResolveArgs {
                query: String,
                skill: Option<String>,
                #[serde(default, deserialize_with = "lenient::option_bool")]
                include_board: Option<bool>,
            }
            let args: ContextResolveArgs = serde_json::from_value(args)?;
            let db = state.mission.db();
            let include_board = args.include_board.unwrap_or(false);

            // Step 1: Find primary skills
            let mut primary_topics: Vec<String> = Vec::new();
            if let Some(ref name) = args.skill {
                primary_topics.push(name.clone());
            } else {
                // FTS search + name/aka match
                for s in state.skills.search(&args.query).iter().take(3) {
                    primary_topics.push(s.name.clone());
                }
            }

            // Step 2: Recursive dependency resolution (max 2 layers)
            let mut all_skill_names: Vec<String> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            let mut infra_ids = std::collections::HashSet::new();
            let mut kb_categories = std::collections::HashSet::new();

            let mut skill_results: Vec<Value> = Vec::new();

            for topic_name in &primary_topics {
                if !seen.insert(topic_name.clone()) { continue; }
                all_skill_names.push(topic_name.clone());

                if let Ok(Some(topic)) = db.skill_topic_get(topic_name) {
                    skill_results.push(json!({
                        "name": topic.topic,
                        "path": topic.file_path,
                        "description": topic.description,
                        "matched_by": if args.skill.is_some() { "direct" } else { "query" },
                    }));

                    // Parse requires from DB
                    if let Some(ref rj) = topic.requires_json {
                        if let Ok(req) = serde_json::from_str::<missiond_core::SkillRequires>(rj) {
                            // Layer 1 dependencies
                            for dep_name in &req.skills {
                                if seen.insert(dep_name.clone()) {
                                    all_skill_names.push(dep_name.clone());
                                    if let Ok(Some(dep_topic)) = db.skill_topic_get(dep_name) {
                                        skill_results.push(json!({
                                            "name": dep_topic.topic,
                                            "path": dep_topic.file_path,
                                            "description": dep_topic.description,
                                            "matched_by": "dependency",
                                        }));
                                        // Layer 2 dependencies (no further recursion)
                                        if let Some(ref drj) = dep_topic.requires_json {
                                            if let Ok(dreq) = serde_json::from_str::<missiond_core::SkillRequires>(drj) {
                                                for dep2_name in &dreq.skills {
                                                    if seen.insert(dep2_name.clone()) {
                                                        if let Ok(Some(dep2)) = db.skill_topic_get(dep2_name) {
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
                    // Fallback: found in memory index but not in DB
                    skill_results.push(json!({
                        "name": skill_meta.name,
                        "path": skill_meta.path,
                        "description": skill_meta.description,
                        "matched_by": if args.skill.is_some() { "direct" } else { "query" },
                    }));
                }
            }

            // Step 3: Aggregate Infra
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

            // Step 4: Aggregate KB (spawn_blocking: FTS5 per-category + fallback)
            let q = args.query.clone();
            let cats = kb_categories.clone();
            let kb_batch = state.db_exec.run(move |db| {
                let mut results: Vec<(missiond_core::KnowledgeEntry, &'static str)> = Vec::new();
                for cat in &cats {
                    if let Ok(entries) = db.kb_search(&q, Some(cat)) {
                        for entry in entries.into_iter().take(5) {
                            results.push((entry, "category_filter"));
                        }
                    }
                }
                if results.is_empty() {
                    if let Ok(entries) = db.kb_search(&q, None) {
                        for entry in entries.into_iter().take(5) {
                            results.push((entry, "query"));
                        }
                    }
                }
                Ok(results)
            }).await.unwrap_or_default();
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

            // Step 5: Optional Board search
            let mut board_results: Vec<Value> = Vec::new();
            if include_board {
                if let Ok(tasks) = db.list_board_tasks(None, false) {
                    let query_lower = args.query.to_lowercase();
                    for task in tasks.iter().take(100) {
                        if task.title.to_lowercase().contains(&query_lower)
                            || task.description.to_lowercase().contains(&query_lower) {
                            board_results.push(json!({
                                "id": task.id,
                                "title": task.title,
                                "status": task.status,
                            }));
                            if board_results.len() >= 5 { break; }
                        }
                    }
                }
            }

            let result = json!({
                "skills": skill_results,
                "infra": infra_results,
                "kb": kb_results,
                "board": board_results,
            });

            Ok(ToolResult::json_pretty(&result))
        }

        // ===== Skill Engine (CQRS write tools) =====
        "mission_skill_upsert" => {
            #[derive(Deserialize)]
            struct SkillUpsertArgs {
                topic: String,
                section_title: String,
                content: String,
                sort_order: Option<i32>,
            }
            let args: SkillUpsertArgs = serde_json::from_value(args)?;
            let db = state.mission.db();

            // Ensure topic exists
            if db.skill_topic_get(&args.topic).map_err(|e| anyhow!("DB: {}", e))?.is_none() {
                // Auto-create topic with default path
                let skills_dir = dirs::home_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join(".claude/skills");
                let file_path = skills_dir.join(&args.topic).join("SKILL.md");
                db.skill_topic_upsert(
                    &args.topic, None, None, None,
                    &file_path.to_string_lossy(), None, None,
                ).map_err(|e| anyhow!("DB: {}", e))?;
            }

            // Find existing block with same title, or create new
            let blocks = db.skill_blocks_for_topic(&args.topic)
                .map_err(|e| anyhow!("DB: {}", e))?;
            let existing = blocks.iter().find(|b| b.title.as_deref() == Some(&args.section_title));

            let action;
            if let Some(block) = existing {
                db.skill_block_update(&block.id, &args.content)
                    .map_err(|e| anyhow!("DB: {}", e))?;
                action = "updated";
            } else {
                let sort = args.sort_order.unwrap_or(blocks.len() as i32);
                db.skill_block_insert(&args.topic, "section", Some(&args.section_title), &args.content, sort)
                    .map_err(|e| anyhow!("DB: {}", e))?;
                action = "created";
            }

            // Materialize to file
            let materialize_result = missiond_core::skill::materialize_topic(db, &args.topic);

            // Trigger incremental embedding update
            let _ = state.embedding_tx.try_send(EmbeddingTask::ProcessSkillTopic(args.topic.clone()));

            match materialize_result {
                Ok(_) => Ok(ToolResult::text(format!("Section '{}' {} in topic '{}', file regenerated", args.section_title, action, args.topic))),
                Err(e) => Ok(ToolResult::text(format!("Section {} but materialize failed: {}", action, e))),
            }
        }
        "mission_skill_record" => {
            #[derive(Deserialize)]
            struct SkillRecordArgs {
                topic: String,
                content: String,
            }
            let args: SkillRecordArgs = serde_json::from_value(args)?;
            let db = state.mission.db();

            // Ensure topic exists
            if db.skill_topic_get(&args.topic).map_err(|e| anyhow!("DB: {}", e))?.is_none() {
                let skills_dir = dirs::home_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join(".claude/skills");
                let file_path = skills_dir.join(&args.topic).join("SKILL.md");
                db.skill_topic_upsert(
                    &args.topic, None, None, None,
                    &file_path.to_string_lossy(), None, None,
                ).map_err(|e| anyhow!("DB: {}", e))?;
            }

            db.skill_block_insert(&args.topic, "fragment", None, &args.content, 0)
                .map_err(|e| anyhow!("DB: {}", e))?;

            let topic_meta = db.skill_topic_get(&args.topic)
                .map_err(|e| anyhow!("DB: {}", e))?;
            let frag_count = topic_meta.map(|t| t.fragment_count).unwrap_or(0);

            // Materialize
            let _ = missiond_core::skill::materialize_topic(db, &args.topic);

            // Trigger incremental embedding update
            let _ = state.embedding_tx.try_send(EmbeddingTask::ProcessSkillTopic(args.topic.clone()));

            let mut msg = format!("Fragment recorded for '{}' ({} fragments)", args.topic, frag_count);
            if frag_count >= 5 {
                msg.push_str(". Recommend running mission_skill_optimize to consolidate.");
            }
            Ok(ToolResult::text(msg))
        }
        "mission_skill_render" => {
            #[derive(Deserialize)]
            struct SkillRenderArgs {
                topic: Option<String>,
            }
            let args: SkillRenderArgs = serde_json::from_value(args)
                .unwrap_or(SkillRenderArgs { topic: None });
            let db = state.mission.db();

            if let Some(topic) = args.topic {
                match missiond_core::skill::materialize_topic(db, &topic) {
                    Ok(output) => Ok(ToolResult::text(format!("Rendered '{}' ({} lines)", topic, output.lines().count()))),
                    Err(e) => Ok(ToolResult::error(format!("Render failed: {}", e))),
                }
            } else {
                match missiond_core::skill::materialize_all(db) {
                    Ok(count) => Ok(ToolResult::text(format!("Rendered all {} skills", count))),
                    Err(e) => Ok(ToolResult::error(format!("Render all failed: {}", e))),
                }
            }
        }
        "mission_skill_topics" => {
            let db = state.mission.db();
            let topics = db.skill_topic_list()
                .map_err(|e| anyhow!("DB: {}", e))?;
            Ok(ToolResult::json_pretty(&topics))
        }

        // ===== Skill Execution (Phase 3) =====
        "mission_skill_exec" => {
            #[derive(Deserialize)]
            struct SkillExecArgs {
                skill: String,
                action: String,
                #[serde(default)]
                dry_run: bool,
                params: Option<Value>,
            }
            let args: SkillExecArgs = serde_json::from_value(args)?;

            match state.execute_workflow(&args.skill, &args.action, args.dry_run, args.params, 0).await {
                Ok(result) => Ok(ToolResult::json_pretty(&result)),
                Err(e) => Ok(ToolResult::error(format!("Workflow execution failed: {}", e))),
            }
        }
        "mission_skill_actions" => {
            #[derive(Deserialize)]
            struct SkillActionsArgs {
                skill: Option<String>,
            }
            let args: SkillActionsArgs = serde_json::from_value(args)
                .unwrap_or(SkillActionsArgs { skill: None });
            let db = state.mission.db();

            let topics = if let Some(ref name) = args.skill {
                db.skill_topic_get(name)
                    .map_err(|e| anyhow!("DB: {}", e))?
                    .into_iter().collect::<Vec<_>>()
            } else {
                db.skill_topic_list()
                    .map_err(|e| anyhow!("DB: {}", e))?
            };

            let mut all_actions: Vec<Value> = Vec::new();
            for topic in &topics {
                if let Some(ref json_str) = topic.actions_json {
                    if let Ok(actions) = serde_json::from_str::<Vec<missiond_core::SkillAction>>(json_str) {
                        // Also count workflow steps from file
                        let step_counts = if let Ok(content) = std::fs::read_to_string(&topic.file_path) {
                            let workflows = missiond_core::parse_workflow_blocks(&content);
                            workflows.iter().map(|w| (w.id.clone(), w.steps.len())).collect::<std::collections::HashMap<_, _>>()
                        } else {
                            std::collections::HashMap::new()
                        };

                        for action in actions {
                            all_actions.push(serde_json::json!({
                                "skill": topic.topic,
                                "action_id": action.id,
                                "name": action.name,
                                "requires_approval": action.requires_approval,
                                "step_count": step_counts.get(&action.id).unwrap_or(&0),
                            }));
                        }
                    }
                }
            }

            Ok(ToolResult::json_pretty(&all_actions))
        }

        // ===== Skill Execution Stats (Phase 4) =====
        "mission_skill_stats" => {
            #[derive(Deserialize)]
            struct StatsArgs {
                skill: Option<String>,
            }
            let args: StatsArgs = serde_json::from_value(args)
                .unwrap_or(StatsArgs { skill: None });
            let db = state.mission.db();
            let stats = db.skill_execution_stats(args.skill.as_deref())
                .map_err(|e| anyhow!("DB: {}", e))?;
            Ok(ToolResult::json_pretty(&stats))
        }

        // ===== Skill Version Rollback (Phase 4) =====
        "mission_skill_rollback" => {
            #[derive(Deserialize)]
            struct RollbackArgs {
                skill: String,
                #[serde(default, deserialize_with = "lenient::option_i64")]
                version_id: Option<i64>,
            }
            let args: RollbackArgs = serde_json::from_value(args)?;
            let db = state.mission.db();

            if let Some(vid) = args.version_id {
                // Rollback to specific version
                let version = db.skill_version_get(vid)
                    .map_err(|e| anyhow!("DB: {}", e))?
                    .ok_or_else(|| anyhow!("Version {} not found", vid))?;
                if version.topic != args.skill {
                    return Ok(ToolResult::error(format!("Version {} belongs to '{}', not '{}'", vid, version.topic, args.skill)));
                }
                let topic = db.skill_topic_get(&args.skill)
                    .map_err(|e| anyhow!("DB: {}", e))?
                    .ok_or_else(|| anyhow!("Skill '{}' not found", args.skill))?;
                std::fs::write(&topic.file_path, &version.content)
                    .map_err(|e| anyhow!("Write error: {}", e))?;
                // Re-ingest the skill
                let skills_dir = std::path::Path::new(&topic.file_path).parent()
                    .and_then(|p| p.parent())
                    .unwrap_or(std::path::Path::new("."));
                missiond_core::skill::ingest_skills(db, skills_dir);
                Ok(ToolResult::text(format!("Rolled back '{}' to version {} ({})", args.skill, vid, version.created_at)))
            } else {
                // List available versions
                let versions = db.skill_version_list(&args.skill, 10)
                    .map_err(|e| anyhow!("DB: {}", e))?;
                if versions.is_empty() {
                    return Ok(ToolResult::text(format!("No version history for '{}'", args.skill)));
                }
                let list: Vec<Value> = versions.iter().map(|v| {
                    serde_json::json!({
                        "version_id": v.id,
                        "checksum": v.checksum,
                        "created_at": v.created_at,
                        "content_lines": v.content.lines().count(),
                    })
                }).collect();
                Ok(ToolResult::json_pretty(&list))
            }
        }


        _ => Err(anyhow!("Unknown skill tool: {name}")),
    }
}
