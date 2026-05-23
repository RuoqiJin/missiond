use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_tool_directory",
        "MissionD MCP tool directory and intent router. Use this first when unsure which MissionD tool family owns a task. Actions: list, recommend, lookup, explain, deprecated, guide.",
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "recommend", "lookup", "explain", "deprecated", "guide"],
                    "description": "list=all primary families; recommend=map intent/query to a family; lookup=inspect a concrete tool; explain=family details; deprecated=compatibility/raw tools and their preferred family; guide=return an agent task-entry card for a modification intent"
                },
                "intent": {
                    "type": "string",
                    "description": "[recommend|guide] Natural-language objective or operator intent"
                },
                "query": {
                    "type": "string",
                    "description": "[recommend|guide] Alias for intent"
                },
                "entry_id": {
                    "type": "string",
                    "description": "[guide] Exact agent entry id such as modify-plan-execution"
                },
                "entryId": {
                    "type": "string",
                    "description": "[guide] camelCase alias for entry_id"
                },
                "project": {
                    "type": "string",
                    "description": "[guide] Project id; missiond uses native entry cards, other registered projects use read-only project navigation cards"
                },
                "project_id": {
                    "type": "string",
                    "description": "[guide] snake_case project id alias"
                },
                "projectId": {
                    "type": "string",
                    "description": "[guide] camelCase project id alias"
                },
                "surface": {
                    "type": "string",
                    "description": "[guide] Surface id such as mission_plan or autopilot-runtime"
                },
                "tool": {
                    "type": "string",
                    "description": "[lookup|deprecated] Concrete MCP tool name such as mission_board_query"
                },
                "family": {
                    "type": "string",
                    "description": "[explain] Primary family id such as board, workflow, workstation, context, memory, universe, ops, router"
                },
                "includeCompatibility": {
                    "type": "boolean",
                    "description": "[list|explain] Include compatibility/raw tools under each family"
                },
                "limit": {
                    "type": "integer",
                    "description": "[deprecated] Maximum compatibility entries to return"
                }
            }
        }),
    )]
}
