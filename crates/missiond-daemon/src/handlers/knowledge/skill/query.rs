use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppState;

#[derive(Deserialize)]
struct SkillSearchArgs {
    query: String,
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
            "status": "pending-contract",
            "message": "Project-specific skill links are not yet a first-class runtime table; use search/context until the project-skill link contract lands.",
        },
    })))
}
