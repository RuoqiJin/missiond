use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_plan",
        "plan 表 manager — 9 actions (compile/list/get/by_task/approve/mark/supersede/execute/record_evidence)。\
         compile 当前为 dry-run（plan-compiler actor 未落地），persist=true 写 draft 行；\
         list/get/by_task/approve/mark/supersede 为 store-backed full；\
         execute 为 plan-runner v0：默认 execute_mode=\"bridge\" 返回 next_call descriptor（runner_status=\"bridge_only\"），\
         向后兼容；execute_mode=\"internal\" 时 MissionD 内部 dispatch 目标 handler，\
         成功后写 plan_runner_dispatch 证据并把 plan 标记 executing。\
         target ∈ {mission_execution, mission_task_delegate, mission_flow_run}；\
         dispatch_strategy ∈ {resident-lisp|fresh-code-alignment|agent-team|mixed|prompt-fallback|unknown}\
         （未知值归一化为 unknown，记入响应 + sidecar，mission_execution 持久化字段属未来工作）。\
         record_evidence 写 sidecar `<project>/.missiond/v2/plans/<plan_id>.evidence.json`。\
         Lisp 源: intent-tools.lisp :: implemented-surface mission_plan :: :execute-contract \
         + intent-intent-layer.lisp :: section unified-entry-pipeline :: role plan-runner \
         + intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s6 execution-runner \
         + F-workstation-dispatch-policy + intent-worker.lisp :: section claudecode-workstation-orchestration。",
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
                    "description": "[execute] routing target — bridge mode hands back next_call; internal mode dispatches inside MissionD"
                },
                "execute_mode": {
                    "type": "string",
                    "enum": ["bridge", "internal"],
                    "description": "[execute] bridge (default) returns a next_call descriptor; internal asks the plan-runner to dispatch the target handler inside the daemon and append evidence"
                },
                "dispatch_strategy": {
                    "type": "string",
                    "enum": [
                        "resident-lisp",
                        "fresh-code-alignment",
                        "agent-team",
                        "mixed",
                        "prompt-fallback",
                        "unknown"
                    ],
                    "description": "[execute] workstation-dispatch-record strategy. Surfaced in the response and the plan_runner_dispatch evidence entry. Unknown values are normalised to `unknown`. mission_execution companion-log persistence is future."
                },
                "target_project": {
                    "type": "string",
                    "description": "[execute] registered project id OR project root path. For mission_execution it is forwarded as `project`; for mission_task_delegate it is treated as cwd if it looks like a path."
                },
                "cwd": {
                    "type": "string",
                    "description": "[execute internal mission_task_delegate] working directory passed through to mission_task_delegate"
                },
                "objective": {
                    "type": "string",
                    "description": "[execute internal mission_task_delegate] override the auto-derived objective; absent → derived from the first non-empty line of plan.sexp_text"
                },
                "intent": {
                    "type": "string",
                    "enum": ["code", "ops", "research", "general"],
                    "description": "[execute internal mission_task_delegate] task intent (default `code`); strict whitelist mirrored from mission_task_delegate"
                },
                "execution_id": {
                    "type": "string",
                    "description": "[execute internal mission_execution] caller-supplied execution_id (default `plan-<plan_id>`)"
                },
                "parent_design": {
                    "type": "string",
                    "description": "[execute internal mission_execution] override parent-design ref (default `directive/<id>` if plan has source_directive_id, else `plan/<plan_id>`)"
                },
                "scope": {
                    "type": "string",
                    "description": "[execute internal mission_execution] override the human-readable scope string"
                },
                "owner": {
                    "type": "string",
                    "description": "[execute internal mission_execution] execution owner (default `plan-runner`)"
                },
                "flow_id": {
                    "type": "string",
                    "description": "[execute internal mission_flow_run] required — plan.sexp_text → flow YAML compilation is future, so caller must provide an existing flow id"
                },
                "params": {
                    "type": "object",
                    "description": "[execute internal mission_flow_run] forwarded as the flow params object",
                    "additionalProperties": true
                },
                "priority": {
                    "type": "string",
                    "description": "[execute internal mission_task_delegate] passthrough priority"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "[execute internal mission_task_delegate] passthrough timeout"
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "[execute internal] when true, return the would-be inner args without dispatching (does NOT mutate plan status, does NOT write evidence)"
                },
                "evidence": {
                    "description": "[record_evidence] arbitrary JSON: tool_calls / event_log / test outputs / execution log refs"
                },
                "project": {
                    "type": "string",
                    "description": "[record_evidence|execute] project id (registry-resolved root); defaults to CWD"
                }
            }
        }),
    )]
}
