use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== mission_flow_run =====
        ToolDefinition::new(
            "mission_flow_run",
            "Flow Engine v2: execute declarative node-based workflows. \
             run=execute flow (by flow_id or flow_path), list=available flows \
             (core + project-generated), status=check running flow. \
             Search order for flow_id resolution: \
             1) explicit flow_path, \
             2) <project_root>/.missiond/generated/flows/<flow_id>.yaml \
             (set via project / target_project / cwd), \
             3) $MISSIOND_HOME/flows/<flow_id>.yaml.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["run", "list", "status"],
                        "default": "run",
                        "description": "run=execute flow, list=core+generated flows, status=check running flow"
                    },
                    "flow_id": {
                        "type": "string",
                        "description": "Flow definition ID (e.g. 'lisp-review' or a methodology-compiled id). Either flow_id or flow_path is required for action=run."
                    },
                    "flow_path": {
                        "type": "string",
                        "description": "Absolute path to a flow YAML, or a path relative to project_root. Wins over flow_id when supplied — used to verify a freshly compiled methodology before it has any registered id."
                    },
                    "project": {
                        "type": "string",
                        "description": "Project signal: registered project id OR an absolute existing project root. Used to discover <project_root>/.missiond/generated/flows."
                    },
                    "target_project": {
                        "type": "string",
                        "description": "Alias for `project` — accepted for parity with mission_workflow."
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Absolute existing directory used as project_root when project / target_project are absent. Relative paths are rejected (no implicit process-cwd resolution)."
                    },
                    "params": {
                        "type": "object",
                        "description": "Initial variables injected into FlowContext (e.g. {\"project\": \"missiond\"})."
                    },
                    "task_id": {
                        "type": "string",
                        "description": "[status] Board Task ID to check"
                    }
                }
            }),
        ),
    ]
}
