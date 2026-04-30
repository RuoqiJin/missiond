use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use crate::lenient;
use crate::state::{AppState, EmbeddingTask};

pub(super) async fn handle_upsert(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct SkillUpsertArgs {
        topic: String,
        section_title: String,
        content: String,
        sort_order: Option<i32>,
    }
    let args: SkillUpsertArgs = serde_json::from_value(args)?;

    ensure_topic_exists(state, &args.topic).await?;

    let blocks = state
        .store
        .skill_blocks_for_topic(&args.topic)
        .await
        .map_err(|e| anyhow!("DB: {}", e))?;
    let existing = blocks
        .iter()
        .find(|b| b.title.as_deref() == Some(&args.section_title));

    let action;
    if let Some(block) = existing {
        state
            .store
            .skill_block_update(&block.id, &args.content)
            .await
            .map_err(|e| anyhow!("DB: {}", e))?;
        action = "updated";
    } else {
        let sort = args.sort_order.unwrap_or(blocks.len() as i32);
        state
            .store
            .skill_block_insert(
                &args.topic,
                "section",
                Some(&args.section_title),
                &args.content,
                sort,
            )
            .await
            .map_err(|e| anyhow!("DB: {}", e))?;
        action = "created";
    }

    let materialize_result =
        missiond_core::skill::materialize_topic(state.store.as_ref(), &args.topic).await;
    trigger_embedding_update(state, &args.topic);

    match materialize_result {
        Ok(_) => Ok(ToolResult::text(format!(
            "Section '{}' {} in topic '{}', file regenerated",
            args.section_title, action, args.topic
        ))),
        Err(e) => Ok(ToolResult::text(format!(
            "Section {} but materialize failed: {}",
            action, e
        ))),
    }
}

pub(super) async fn handle_record(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct SkillRecordArgs {
        topic: String,
        content: String,
    }
    let args: SkillRecordArgs = serde_json::from_value(args)?;

    ensure_topic_exists(state, &args.topic).await?;

    state
        .store
        .skill_block_insert(&args.topic, "fragment", None, &args.content, 0)
        .await
        .map_err(|e| anyhow!("DB: {}", e))?;

    let topic_meta = state
        .store
        .skill_topic_get(&args.topic)
        .await
        .map_err(|e| anyhow!("DB: {}", e))?;
    let frag_count = topic_meta.map(|t| t.fragment_count).unwrap_or(0);

    let _ = missiond_core::skill::materialize_topic(state.store.as_ref(), &args.topic).await;
    trigger_embedding_update(state, &args.topic);

    let mut msg = format!(
        "Fragment recorded for '{}' ({} fragments)",
        args.topic, frag_count
    );
    if frag_count >= 5 {
        msg.push_str(". Recommend running mission_skill_optimize to consolidate.");
    }
    Ok(ToolResult::text(msg))
}

pub(super) async fn handle_render(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct SkillRenderArgs {
        topic: Option<String>,
    }
    let args: SkillRenderArgs =
        serde_json::from_value(args).unwrap_or(SkillRenderArgs { topic: None });
    if let Some(topic) = args.topic {
        match missiond_core::skill::materialize_topic(state.store.as_ref(), &topic).await {
            Ok(output) => Ok(ToolResult::text(format!(
                "Rendered '{}' ({} lines)",
                topic,
                output.lines().count()
            ))),
            Err(e) => Ok(ToolResult::error(format!("Render failed: {}", e))),
        }
    } else {
        match missiond_core::skill::materialize_all(state.store.as_ref()).await {
            Ok(count) => Ok(ToolResult::text(format!("Rendered all {} skills", count))),
            Err(e) => Ok(ToolResult::error(format!("Render all failed: {}", e))),
        }
    }
}

pub(super) async fn handle_rollback(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct RollbackArgs {
        skill: String,
        #[serde(default, deserialize_with = "lenient::option_i64")]
        version_id: Option<i64>,
    }
    let args: RollbackArgs = serde_json::from_value(args)?;

    if let Some(vid) = args.version_id {
        let version = state
            .store
            .skill_version_get(vid)
            .await
            .map_err(|e| anyhow!("DB: {}", e))?
            .ok_or_else(|| anyhow!("Version {} not found", vid))?;
        if version.topic != args.skill {
            return Ok(ToolResult::error(format!(
                "Version {} belongs to '{}', not '{}'",
                vid, version.topic, args.skill
            )));
        }
        let topic = state
            .store
            .skill_topic_get(&args.skill)
            .await
            .map_err(|e| anyhow!("DB: {}", e))?
            .ok_or_else(|| anyhow!("Skill '{}' not found", args.skill))?;
        std::fs::write(&topic.file_path, &version.content)
            .map_err(|e| anyhow!("Write error: {}", e))?;
        let skills_dir = std::path::Path::new(&topic.file_path)
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or(std::path::Path::new("."));
        missiond_core::skill::ingest_skills(state.store.as_ref(), skills_dir).await;
        Ok(ToolResult::text(format!(
            "Rolled back '{}' to version {} ({})",
            args.skill, vid, version.created_at
        )))
    } else {
        let versions = state
            .store
            .skill_version_list(&args.skill, 10)
            .await
            .map_err(|e| anyhow!("DB: {}", e))?;
        if versions.is_empty() {
            return Ok(ToolResult::text(format!(
                "No version history for '{}'",
                args.skill
            )));
        }
        let list: Vec<Value> = versions
            .iter()
            .map(|v| {
                serde_json::json!({
                    "version_id": v.id,
                    "checksum": v.checksum,
                    "created_at": v.created_at,
                    "content_lines": v.content.lines().count(),
                })
            })
            .collect();
        Ok(ToolResult::json_pretty(&list))
    }
}

async fn ensure_topic_exists(state: &AppState, topic: &str) -> Result<()> {
    if state
        .store
        .skill_topic_get(topic)
        .await
        .map_err(|e| anyhow!("DB: {}", e))?
        .is_none()
    {
        let skills_dir = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".claude/skills");
        let file_path = skills_dir.join(topic).join("SKILL.md");
        state
            .store
            .skill_topic_upsert(
                topic,
                None,
                None,
                None,
                &file_path.to_string_lossy(),
                None,
                None,
            )
            .await
            .map_err(|e| anyhow!("DB: {}", e))?;
    }
    Ok(())
}

fn trigger_embedding_update(state: &AppState, topic: &str) {
    let _ = state
        .embedding_tx
        .try_send(EmbeddingTask::ProcessSkillTopic(topic.to_string()));
}
