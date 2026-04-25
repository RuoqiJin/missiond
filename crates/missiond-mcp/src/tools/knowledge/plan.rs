use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_plan",
        "plan 表 manager — 9 actions (compile/list/get/by_task/approve/mark/supersede/execute/record_evidence)。\
         compile 当前为 dry-run（plan-compiler actor 未落地），persist=true 写 draft 行；\
         其余 list/get/by_task/approve/mark/supersede 为 store-backed full；\
         execute 仅返回 next_call 描述（不递归 dispatch），target ∈ \
         {mission_execution, mission_task_delegate, mission_flow_run}，未知 target 返回 INVALID_PARAM；\
         record_evidence 写 sidecar `<project>/.missiond/v2/plans/<plan_id>.evidence.json`。\
         Lisp 源: intent-memory.lisp :: module directive-layer :: plumbing plan-execution \
         + intent-tools.lisp :: future-surface mission_plan + intent-flow.lisp :: \
         F-directive-plan-workflow-compile :: plan branch。",
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "compile", "list", "get", "by_task",
                        "approve", "mark", "supersede",
                        "execute", "record_evidence"
                    ],
                    "description": "manager action — see Lisp future-surface mission_plan"
                },
                "directive_id": {
                    "type": "string",
                    "description": "[compile] approved directive id"
                },
                "board_task_id": {
                    "type": "string",
                    "description": "[compile|by_task] board_tasks.id (TEXT FK)"
                },
                "persist": {
                    "type": "boolean",
                    "description": "[compile] insert a draft row (default false). Requires board_task_id."
                },
                "plan_id": {
                    "type": "string",
                    "description": "[get|approve|mark|execute|record_evidence] plan UUID"
                },
                "status": {
                    "type": "string",
                    "enum": ["draft","awaiting_approval","approved","executing","succeeded","failed","superseded"],
                    "description": "[list filter | mark target] PlanStatus"
                },
                "limit": {
                    "type": "integer",
                    "description": "[list] cap result count (1-500, default 50)"
                },
                "old_plan_id": {
                    "type": "string",
                    "description": "[supersede] plan to mark superseded"
                },
                "new_plan_id": {
                    "type": "string",
                    "description": "[supersede] replacement plan UUID (recorded in result only)"
                },
                "target": {
                    "type": "string",
                    "enum": ["mission_execution", "mission_task_delegate", "mission_flow_run"],
                    "description": "[execute] safe routing target — manager returns next_call descriptor only"
                },
                "evidence": {
                    "description": "[record_evidence] arbitrary JSON: tool_calls / event_log / test outputs / execution log refs"
                },
                "project": {
                    "type": "string",
                    "description": "[record_evidence] project id (registry-resolved root); defaults to CWD"
                }
            }
        }),
    )]
}
