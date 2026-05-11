use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_tool_directory",
        "MissionD MCP tool directory and intent router. Use this first when unsure which MissionD tool family owns a task. Actions: list, recommend, lookup, explain, deprecated.",
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "recommend", "lookup", "explain", "deprecated"],
                    "description": "list=all primary families; recommend=map intent/query to a family; lookup=inspect a concrete tool; explain=family details; deprecated=compatibility/raw tools and their preferred family"
                },
                "intent": {
                    "type": "string",
                    "description": "[recommend] Natural-language objective or operator intent"
                },
                "query": {
                    "type": "string",
                    "description": "[recommend] Alias for intent"
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
