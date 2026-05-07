use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

mod context;
mod exec;
mod mutate;
mod query;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    if name == "mission_skill_query" {
        return route_skill_query(state, args).await;
    }
    if name == "mission_skill_context" {
        return route_skill_context(state, args).await;
    }
    if name == "mission_skill_mutate" {
        return route_skill_mutate(state, args).await;
    }

    match name {
        "mission_skill_list" => query::handle_list(state).await,
        "mission_skill_search" => query::handle_search(state, args).await,
        "mission_skill_topics" => query::handle_topics(state).await,
        "mission_skill_actions" => query::handle_actions(state, args).await,
        "mission_skill_stats" => query::handle_stats(state, args).await,
        "mission_context_build" => context::handle_build(state, args).await,
        "mission_context_resolve" => context::handle_resolve(state, args).await,
        "mission_skill_upsert" => mutate::handle_upsert(state, args).await,
        "mission_skill_record" => mutate::handle_record(state, args).await,
        "mission_skill_render" => mutate::handle_render(state, args).await,
        "mission_skill_rollback" => mutate::handle_rollback(state, args).await,
        "mission_skill_exec" => exec::handle_exec(state, args).await,
        _ => Err(anyhow!("Unknown skill tool: {name}")),
    }
}

async fn route_skill_query(state: &AppState, args: Value) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("list");
    match action {
        "list" => query::handle_list(state).await,
        "search" => query::handle_search(state, args).await,
        "topics" => query::handle_topics(state).await,
        "actions" => query::handle_actions(state, args).await,
        "stats" => query::handle_stats(state, args).await,
        "project_links" => query::handle_project_links(state, args).await,
        _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
    }
}

async fn route_skill_context(state: &AppState, args: Value) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("build");
    match action {
        "build" => context::handle_build(state, args).await,
        "resolve" => context::handle_resolve(state, args).await,
        _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
    }
}

async fn route_skill_mutate(state: &AppState, args: Value) -> Result<ToolResult> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("upsert");
    match action {
        "upsert" => mutate::handle_upsert(state, args).await,
        "record" => mutate::handle_record(state, args).await,
        "render" => mutate::handle_render(state, args).await,
        "rollback" => mutate::handle_rollback(state, args).await,
        _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
    }
}
