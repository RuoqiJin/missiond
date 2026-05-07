use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppState;

#[derive(Deserialize)]
struct SkillSearchArgs {
    query: String,
}

#[derive(Deserialize)]
struct ProjectLinksArgs {
    #[serde(default, alias = "project", alias = "projectId")]
    project_id: Option<String>,
    #[serde(default)]
    skill: Option<String>,
}

pub(super) async fn handle_list(state: &AppState) -> Result<ToolResult> {
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

pub(super) async fn handle_search(state: &AppState, args: Value) -> Result<ToolResult> {
    let SkillSearchArgs { query } = serde_json::from_value(args)?;
    let mut topic_scores: std::collections::HashMap<
        String,
        (f64, Option<usize>, Option<usize>, Option<f32>, Value),
    > = std::collections::HashMap::new();

    for s in state.skills.search(&query).iter().take(10) {
        topic_scores.entry(s.name.clone()).or_insert_with(|| {
            (
                0.3,
                None,
                None,
                None,
                serde_json::json!({
                    "name": s.name,
                    "description": s.description,
                    "aka": s.aka,
                    "path": s.path,
                }),
            )
        });
    }

    if let Ok(fts_results) = state.store.skill_search_fts(&query).await {
        for (rank, r) in fts_results.iter().take(20).enumerate() {
            let entry = topic_scores.entry(r.topic.clone()).or_insert_with(|| {
                (
                    0.0,
                    None,
                    None,
                    None,
                    serde_json::json!({
                        "name": r.topic,
                        "description": r.description,
                        "path": r.file_path,
                        "matched_section": r.section_title,
                        "snippet": r.snippet,
                    }),
                )
            });
            if entry.1.is_none() {
                entry.1 = Some(rank);
            }
        }
    }

    if let Some(ref emb_svc) = state.embedding_service {
        let cache_guard = state.skill_embedding_cache.read().await;
        if !cache_guard.is_empty() {
            if let Some(query_vec) = emb_svc.embed(&query) {
                let mut sims: Vec<(usize, f32)> = cache_guard
                    .iter()
                    .enumerate()
                    .map(|(i, (_, vec))| {
                        (
                            i,
                            missiond_core::embedding::cosine_similarity(&query_vec, vec),
                        )
                    })
                    .collect();
                sims.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                for (rank, (idx, sim)) in sims.iter().take(10).enumerate() {
                    let topic = &cache_guard[*idx].0;
                    let entry = topic_scores.entry(topic.clone()).or_insert_with(|| {
                        (
                            0.0,
                            None,
                            None,
                            None,
                            serde_json::json!({
                                "name": topic,
                            }),
                        )
                    });
                    entry.2 = Some(rank);
                    entry.3 = Some(*sim);
                }
            }
        }
    }

    let mut scored: Vec<(String, f64, Value)> = topic_scores
        .into_iter()
        .map(
            |(topic, (bonus, fts_rank, vec_rank, cosine_sim, mut meta))| {
                let rrf = missiond_core::embedding::rrf_score(fts_rank, vec_rank, 60);
                let final_score = bonus + rrf;
                meta.as_object_mut().map(|obj| {
                    obj.insert(
                        "score".to_string(),
                        serde_json::json!(format!("{:.4}", final_score)),
                    );
                    if let Some(sim) = cosine_sim {
                        obj.insert(
                            "cosine_sim".to_string(),
                            serde_json::json!(format!("{:.3}", sim)),
                        );
                    }
                });
                (topic, final_score, meta)
            },
        )
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let results: Vec<Value> = scored
        .iter()
        .take(10)
        .map(|(_, _, meta)| meta.clone())
        .collect();

    for (topic, _, _) in scored.iter().take(5) {
        let _ = state.store.skill_topic_hit(topic).await;
    }

    Ok(ToolResult::json_pretty(&results))
}

pub(super) async fn handle_topics(state: &AppState) -> Result<ToolResult> {
    let topics = state
        .store
        .skill_topic_list()
        .await
        .map_err(|e| anyhow!("DB: {}", e))?;
    Ok(ToolResult::json_pretty(&topics))
}

pub(super) async fn handle_actions(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct SkillActionsArgs {
        skill: Option<String>,
    }
    let args: SkillActionsArgs =
        serde_json::from_value(args).unwrap_or(SkillActionsArgs { skill: None });

    let topics = if let Some(ref name) = args.skill {
        state
            .store
            .skill_topic_get(name)
            .await
            .map_err(|e| anyhow!("DB: {}", e))?
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        state
            .store
            .skill_topic_list()
            .await
            .map_err(|e| anyhow!("DB: {}", e))?
    };

    let mut all_actions: Vec<Value> = Vec::new();
    for topic in &topics {
        if let Some(ref json_str) = topic.actions_json {
            if let Ok(actions) = serde_json::from_str::<Vec<missiond_core::SkillAction>>(json_str) {
                let step_counts = if let Ok(content) = std::fs::read_to_string(&topic.file_path) {
                    let workflows = missiond_core::parse_workflow_blocks(&content);
                    workflows
                        .iter()
                        .map(|w| (w.id.clone(), w.steps.len()))
                        .collect::<std::collections::HashMap<_, _>>()
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

pub(super) async fn handle_project_links(state: &AppState, args: Value) -> Result<ToolResult> {
    let args: ProjectLinksArgs = serde_json::from_value(args).unwrap_or(ProjectLinksArgs {
        project_id: None,
        skill: None,
    });
    let projects = {
        let registry = state.project_registry.read().await;
        registry
            .active_projects()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
    };
    let registry_skills = state.skills.list().to_vec();
    let topics = state
        .store
        .skill_topic_list()
        .await
        .map_err(|e| anyhow!("DB: {}", e))?;
    let links = derive_project_skill_links(
        &projects,
        &registry_skills,
        &topics,
        args.project_id.as_deref(),
        args.skill.as_deref(),
    );
    let linked_projects = links
        .iter()
        .filter_map(|link| link.get("projectId").and_then(Value::as_str))
        .collect::<std::collections::HashSet<_>>()
        .len();

    Ok(ToolResult::json_pretty(&serde_json::json!({
        "schema": "missiond.project-skill-links.v1",
        "source": "derived-from-project-registry-skill-index-skill-topics",
        "filter": {
            "projectId": args.project_id,
            "skill": args.skill,
        },
        "projects": projects.len(),
        "linkedProjects": linked_projects,
        "total": links.len(),
        "links": links,
    })))
}

pub(super) async fn handle_stats(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct StatsArgs {
        skill: Option<String>,
    }
    let args: StatsArgs = serde_json::from_value(args).unwrap_or(StatsArgs { skill: None });
    let registry_skills: Vec<_> = state
        .skills
        .list()
        .iter()
        .filter(|skill| {
            args.skill
                .as_deref()
                .map(|name| skill.name == name)
                .unwrap_or(true)
        })
        .collect();
    let registry_paths = registry_skills
        .iter()
        .map(|skill| skill.path.display().to_string())
        .collect::<std::collections::HashSet<_>>();
    let registry_action_count: usize = registry_skills
        .iter()
        .map(|skill| {
            skill
                .actions
                .as_ref()
                .map(|actions| actions.len())
                .unwrap_or(0)
        })
        .sum();
    let registry_context_hook_count: usize = registry_skills
        .iter()
        .map(|skill| {
            skill
                .context_hooks
                .as_ref()
                .map(|hooks| hooks.len())
                .unwrap_or(0)
        })
        .sum();

    let topics = if let Some(ref skill) = args.skill {
        state
            .store
            .skill_topic_get(skill)
            .await
            .map_err(|e| anyhow!("DB: {}", e))?
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        state
            .store
            .skill_topic_list()
            .await
            .map_err(|e| anyhow!("DB: {}", e))?
    };

    let topic_action_count: usize = topics
        .iter()
        .map(|topic| {
            topic
                .actions_json
                .as_deref()
                .and_then(|actions| serde_json::from_str::<Vec<Value>>(actions).ok())
                .map(|actions| actions.len())
                .unwrap_or(0)
        })
        .sum();
    let topic_with_actions = topics
        .iter()
        .filter(|topic| {
            topic
                .actions_json
                .as_deref()
                .is_some_and(|json| !json.is_empty())
        })
        .count();
    let topic_with_requires = topics
        .iter()
        .filter(|topic| {
            topic
                .requires_json
                .as_deref()
                .is_some_and(|json| !json.is_empty())
        })
        .count();
    let topic_with_context_hooks = topics
        .iter()
        .filter(|topic| {
            topic
                .context_hooks_json
                .as_deref()
                .is_some_and(|json| !json.is_empty())
        })
        .count();
    let total_fragments: i64 = topics.iter().map(|topic| topic.fragment_count).sum();
    let total_lines: i64 = topics.iter().map(|topic| topic.total_lines).sum();

    let skill_embedding_cache_size = state.skill_embedding_cache.read().await.len();
    let stats = state
        .store
        .skill_execution_stats(args.skill.as_deref())
        .await
        .map_err(|e| anyhow!("DB: {}", e))?;
    let execution_total: i64 = stats.iter().map(|stat| stat.total).sum();
    let execution_successes: i64 = stats.iter().map(|stat| stat.successes).sum();
    let execution_failures: i64 = stats.iter().map(|stat| stat.failures).sum();
    let projects = {
        let registry = state.project_registry.read().await;
        registry
            .active_projects()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
    };
    let registry_skill_values = registry_skills
        .iter()
        .map(|skill| (*skill).clone())
        .collect::<Vec<_>>();
    let project_skill_links = derive_project_skill_links(
        &projects,
        &registry_skill_values,
        &topics,
        None,
        args.skill.as_deref(),
    );
    let linked_projects = project_skill_links
        .iter()
        .filter_map(|link| link.get("projectId").and_then(Value::as_str))
        .collect::<std::collections::HashSet<_>>()
        .len();

    Ok(ToolResult::json_pretty(&serde_json::json!({
        "schema": "missiond.skill-stats.v1",
        "filter": {
            "skill": args.skill,
        },
        "registry": {
            "loadedSkills": registry_skills.len(),
            "uniquePaths": registry_paths.len(),
            "actions": registry_action_count,
            "contextHooks": registry_context_hook_count,
            "source": "SkillIndex",
        },
        "topics": {
            "total": topics.len(),
            "withActions": topic_with_actions,
            "withRequires": topic_with_requires,
            "withContextHooks": topic_with_context_hooks,
            "actionCount": topic_action_count,
            "fragmentCount": total_fragments,
            "totalLines": total_lines,
            "source": "skill_topics",
        },
        "embeddings": {
            "cachedSkillTopics": skill_embedding_cache_size,
            "source": "skill_embedding_cache",
        },
        "execution": {
            "total": execution_total,
            "successes": execution_successes,
            "failures": execution_failures,
            "stats": stats,
            "source": "skill_executions",
        },
        "projectSkillLinks": {
            "status": "derived",
            "source": "ProjectRegistry+SkillIndex+skill_topics",
            "projectsWithLinks": linked_projects,
            "total": project_skill_links.len(),
            "sample": project_skill_links.into_iter().take(20).collect::<Vec<_>>(),
        },
    })))
}

fn derive_project_skill_links(
    projects: &[missiond_core::types::ProjectConfig],
    registry_skills: &[missiond_core::SkillMeta],
    topics: &[missiond_core::types::SkillTopic],
    filter_project: Option<&str>,
    filter_skill: Option<&str>,
) -> Vec<Value> {
    let mut links = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for project in projects {
        if filter_project.is_some_and(|id| id != project.id) {
            continue;
        }
        let project_norm = normalize_link_key(&project.id);
        if project_norm.len() < 3 {
            continue;
        }

        for topic in topics {
            if filter_skill.is_some_and(|skill| skill != topic.topic) {
                continue;
            }
            if let Some((matched_by, confidence)) = match_skill_to_project(
                &project.id,
                &project_norm,
                &topic.topic,
                topic.aka.as_deref(),
                topic.description.as_deref(),
                &topic.file_path,
            ) {
                let key = format!("{}|{}", project.id, topic.topic);
                if seen.insert(key) {
                    links.push(serde_json::json!({
                        "projectId": project.id,
                        "projectRoot": project.path,
                        "skill": topic.topic,
                        "path": topic.file_path,
                        "description": topic.description,
                        "source": "skill_topics",
                        "matchedBy": matched_by,
                        "confidence": confidence,
                    }));
                }
            }
        }

        for skill in registry_skills {
            if filter_skill.is_some_and(|name| name != skill.name) {
                continue;
            }
            let aka_text = skill
                .aka
                .as_ref()
                .map(|aka| aka.join(" "))
                .unwrap_or_default();
            if let Some((matched_by, confidence)) = match_skill_to_project(
                &project.id,
                &project_norm,
                &skill.name,
                Some(&aka_text),
                skill.description.as_deref(),
                &skill.path.display().to_string(),
            ) {
                let key = format!("{}|{}", project.id, skill.name);
                if seen.insert(key) {
                    links.push(serde_json::json!({
                        "projectId": project.id,
                        "projectRoot": project.path,
                        "skill": skill.name,
                        "path": skill.path,
                        "description": skill.description,
                        "source": "SkillIndex",
                        "matchedBy": matched_by,
                        "confidence": confidence,
                    }));
                }
            }
        }
    }

    links.sort_by(|a, b| {
        let pa = a.get("projectId").and_then(Value::as_str).unwrap_or_default();
        let pb = b.get("projectId").and_then(Value::as_str).unwrap_or_default();
        pa.cmp(pb).then_with(|| {
            let sa = a.get("skill").and_then(Value::as_str).unwrap_or_default();
            let sb = b.get("skill").and_then(Value::as_str).unwrap_or_default();
            sa.cmp(sb)
        })
    });
    links
}

fn match_skill_to_project(
    project_id: &str,
    project_norm: &str,
    skill_name: &str,
    aka: Option<&str>,
    description: Option<&str>,
    path: &str,
) -> Option<(&'static str, f64)> {
    let skill_norm = normalize_link_key(skill_name);
    if skill_name == project_id || skill_norm == project_norm {
        return Some(("exact", 1.0));
    }
    if skill_norm.len() >= 4
        && (skill_norm.ends_with(project_norm) || project_norm.ends_with(&skill_norm))
    {
        return Some(("normalized-suffix", 0.86));
    }
    let project_lower = project_id.to_ascii_lowercase();
    let path_lower = path.to_ascii_lowercase();
    if path_lower.contains(&format!("/{project_lower}/")) || path_lower.contains(&project_lower) {
        return Some(("path", 0.78));
    }
    if aka
        .map(|text| normalize_link_key(text).contains(project_norm))
        .unwrap_or(false)
    {
        return Some(("aka", 0.74));
    }
    if description
        .map(|text| normalize_link_key(text).contains(project_norm))
        .unwrap_or(false)
    {
        return Some(("description", 0.62));
    }
    None
}

fn normalize_link_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}
