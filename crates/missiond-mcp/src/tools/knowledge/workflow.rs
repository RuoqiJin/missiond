use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_workflow",
        "workflow 表 manager — 8 actions (list/get/match/apply/distill/record_execution/compile_methodology/run_methodology)。\
         list/get/match/apply/record_execution 为 store-backed full；\
         distill 当前为 dry-run（workflow-distiller actor 未落地），persist=true 写 draft 行；\
         compile_methodology 读 `.missiond/workflows/<name>.lisp` 产出 dry-run 预览（YAML emitter actor 未落地）；\
         run_methodology 返回 not_implemented，附下一步指引（compile_methodology + 手写 YAML + mission_flow_run）。\
         Lisp 源: intent-memory.lisp :: module directive-layer :: plumbing workflow-templates \
         + intent-tools.lisp :: future-surface mission_workflow + intent-flow.lisp :: \
         F-directive-plan-workflow-compile / F-methodology-to-executable-compile。",
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "list", "get", "match", "apply",
                        "distill", "record_execution",
                        "compile_methodology", "run_methodology"
                    ],
                    "description": "manager action — see Lisp future-surface mission_workflow"
                },
                "name": {
                    "type": "string",
                    "description": "[get|apply|distill|compile_methodology|run_methodology] workflow.name (UNIQUE) or methodology basename"
                },
                "workflow_id": {
                    "type": "string",
                    "description": "[get|apply] workflow UUID"
                },
                "utterance": {
                    "type": "string",
                    "description": "[match] free-form query — currently substring match over match_rules"
                },
                "limit": {
                    "type": "integer",
                    "description": "[list] cap result count (1-500, default 50)"
                },
                "plan_id": {
                    "type": "string",
                    "description": "[distill] succeeded plan UUID to distill from"
                },
                "persist": {
                    "type": "boolean",
                    "description": "[distill] insert a draft workflow row (default false). Requires `name`."
                },
                "success": {
                    "type": "boolean",
                    "description": "[record_execution] outcome (rolling avg/cost update)"
                },
                "cost_usd": {
                    "type": "number",
                    "description": "[record_execution] optional cost contribution (USD)"
                },
                "workflow_path": {
                    "type": "string",
                    "description": "[compile_methodology|run_methodology] explicit path to a methodology .lisp file"
                },
                "project": {
                    "type": "string",
                    "description": "[compile_methodology] project id (registry-resolved root); defaults to CWD"
                }
            }
        }),
    )]
}
