use anyhow::Result;
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppState;

pub(super) async fn handle_exec(state: &AppState, args: Value) -> Result<ToolResult> {
    #[derive(Deserialize)]
    struct SkillExecArgs {
        skill: String,
        action: String,
        #[serde(default)]
        dry_run: bool,
        params: Option<Value>,
    }
    let args: SkillExecArgs = serde_json::from_value(args)?;

    match state
        .execute_workflow(&args.skill, &args.action, args.dry_run, args.params, 0)
        .await
    {
        Ok(result) => Ok(ToolResult::json_pretty(&result)),
        Err(e) => Ok(ToolResult::error(format!(
            "Workflow execution failed: {}",
            e
        ))),
    }
}
