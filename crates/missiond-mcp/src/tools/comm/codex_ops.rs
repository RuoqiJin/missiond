use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_codex_ops",
        "Codex (OpenAI agent) operation log query. recent/thread/tool_stats",
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["recent", "thread", "tool_stats"],
                    "description": "recent: latest tool calls across threads. thread: specific thread detail. tool_stats: aggregated stats."
                },
                "threadId": {
                    "type": "string",
                    "description": "[thread] Codex thread ID"
                },
                "toolFilter": {
                    "type": "string",
                    "description": "[recent/thread] Filter by exact tool name (e.g. 'exec_command')"
                },
                "since": {
                    "type": "string",
                    "description": "[recent/tool_stats] ISO timestamp or relative (e.g. '24h', '7d')"
                },
                "project": {
                    "type": "string",
                    "description": "[recent/tool_stats] Filter by project path substring"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results (default 50)",
                    "default": 50
                }
            }
        }),
    )]
}
