use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::state::AppState;

mod graph;
mod lint;
mod path;
mod plan;
mod trigger;

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        "mission_universe_graph" => graph::handle_universe_graph(args).await,
        "mission_cascade_plan" => plan::handle_cascade_plan(args).await,
        "mission_cascade_trigger" => trigger::handle_cascade_trigger(state, args).await,
        "mission_cascade_lint" => lint::handle_cascade_lint(args).await,
        _ => Err(anyhow!("unknown cascade tool: {name}")),
    }
}
