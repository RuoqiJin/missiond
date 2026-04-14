use serde_json::json;
use crate::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== mission_flow_run =====
        ToolDefinition::new(
            "mission_flow_run",
            "Flow Engine v2: execute declarative node-based workflows. run=execute flow in background, list=available flows, status=check running flow",
            json!({
                "type": "object",
                "required": ["flow_id"],
                "properties": {
                    "flow_id": {"type": "string", "description": "Flow definition ID (e.g. 'lisp-review')"},
                    "params": {"type": "object", "description": "Initial variables injected into FlowContext (e.g. {\"project\": \"missiond\"})"},
                    "action": {"type": "string", "enum": ["run", "list", "status"], "default": "run", "description": "run=execute flow, list=available flows, status=check running flow"},
                    "task_id": {"type": "string", "description": "[status] Board Task ID to check"}
                }
            }),
        ),
    ]
}
