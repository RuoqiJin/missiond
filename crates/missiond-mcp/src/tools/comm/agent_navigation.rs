use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_agent_navigation",
        "MissionD SSOT agent navigation catalog, review sidecar, feedback, and read-only project entry suggestions. Actions: catalog, review, feedback, suggest_entries.",
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["catalog", "review", "feedback", "suggest_entries"],
                    "description": "catalog=compiled navigation catalog; review=sidecar usage feedback; feedback=append a usage/quality event; suggest_entries=read-only suggestions for registered projects"
                },
                "project": {"type": "string", "description": "project id; defaults to missiond"},
                "project_id": {"type": "string", "description": "snake_case project id alias"},
                "projectId": {"type": "string", "description": "camelCase project id alias"},
                "intent": {"type": "string", "description": "[catalog|feedback|suggest_entries] natural-language intent"},
                "query": {"type": "string", "description": "intent alias"},
                "entry_id": {"type": "string", "description": "[feedback] selected entry id"},
                "entryId": {"type": "string", "description": "camelCase entry id alias"},
                "surface": {"type": "string", "description": "[catalog|suggest_entries] surface id"},
                "outcome": {
                    "type": "string",
                    "enum": ["used", "missed", "wrong_entry", "insufficient_context", "suggested"],
                    "description": "[feedback] usage outcome"
                },
                "rationale": {"type": "string", "description": "[feedback] short reason or note"},
                "agent_id": {"type": "string", "description": "[feedback] caller/agent id"},
                "agentId": {"type": "string", "description": "camelCase agent id alias"},
                "limit": {"type": "integer", "default": 50}
            }
        }),
    )]
}
