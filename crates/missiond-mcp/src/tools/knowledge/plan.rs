use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_plan",
        "plan 表 manager — 9 actions (compile/list/get/by_task/approve/mark/supersede/execute/record_evidence)。\
         compile 默认 compiler_mode=\"dry_run\"（不调 LLM，行为同旧版）；compiler_mode=\"sonnet\" 时是 plan-compiler actor v0：\
         读取 directive (version_chain head 或显式 directive_version) + board_task，调 Sonnet 生成 PLAN sexp，\
         校验括号 / 顶层 head / board_task 锚点，persist=true 时落库 status=awaiting_approval、\
         compiler_model=\"claude-sonnet\"、compiled_from=\"directive/<id>:<version>\" 或 \"board_task/<id>\"。\
         默认要求 directive.status ∈ {approved, compiled}；可显式 allow_unapproved=true 调试。\
         list/get/by_task/approve/mark/supersede 为 store-backed full；\
         execute 为 plan-runner v0：默认 execute_mode=\"bridge\" 返回 next_call descriptor（runner_status=\"bridge_only\"），\
         向后兼容；execute_mode=\"internal\" 时 MissionD 内部 dispatch 目标 handler，\
         成功后写 plan_runner_dispatch 证据并把 plan 标记 executing。\
         target ∈ {mission_execution, mission_task_delegate, mission_flow_run}；\
         dispatch_strategy ∈ {resident-lisp|fresh-code-alignment|agent-team|mixed|prompt-fallback|unknown}\
         （未知值归一化为 unknown，记入响应 + sidecar，且在 internal target=mission_execution 时\
         转发给 mission_execution(action=open) 持久化进 companion log）。\
         record_evidence 写 sidecar `<project>/.missiond/v2/plans/<plan_id>.evidence.json`。\
         Lisp 源: intent-tools.lisp :: implemented-surface mission_plan :: :execute-contract \
         + intent-intent-layer.lisp :: section unified-entry-pipeline :: role plan-compiler / plan-runner \
         + intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s4 plan-authoring / s5 plan-review-gate / s6 execution-runner \
         + intent-memory.lisp :: directive-layer :: file-first-artifacts :: plan-lisp。",
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
                    "description": "[compile] directive id; sonnet mode loads sexp_text from version_chain head (or `directive_version`). Default approval gate requires directive.status ∈ {approved, compiled}."
                },
                "board_task_id": {
                    "type": "string",
                    "description": "[compile|by_task] board_tasks.id (TEXT FK). Required for sonnet compile (anchor) and for any persist=true (FK NOT NULL)."
                },
                "persist": {
                    "type": "boolean",
                    "description": "[compile] insert a row (default false). Requires board_task_id. dry_run inserts as draft; sonnet inserts as awaiting_approval with compiler_model + compiled_from."
                },
                "compiler_mode": {
                    "type": "string",
                    "enum": ["dry_run", "sonnet"],
                    "description": "[compile] dry_run (default, no LLM, same envelope as before); sonnet asks the plan-compiler actor (Sonnet) to emit a PLAN sexp anchored to board_task_id. See intent-intent-layer.lisp :: role plan-compiler."
                },
                "directive_version": {
                    "type": "integer",
                    "description": "[compile sonnet] specific directive version (default = version_chain head)."
                },
                "allow_unapproved": {
                    "type": "boolean",
                    "description": "[compile sonnet] override approval gate. When true, the compiler runs against directive.status outside {approved, compiled}; the response flags `approval_gate_overridden=true`."
                },
                "target_project": {
                    "type": "string",
                    "description": "[compile sonnet | execute] for compile this is prompt context only. For execute internal mission_task_delegate it is treated as cwd if it looks like a path; for execute internal mission_execution it is forwarded as `project`."
                },
                "parallelism": {
                    "type": "string",
                    "description": "[compile sonnet] hint for the planner: e.g. `serial`, `agent-team`, `mixed`. Surfaced inside the Sonnet prompt only."
                },
                "acceptance": {
                    "description": "[compile sonnet] string or array of acceptance criteria woven into the planner prompt."
                },
                "constraints": {
                    "description": "[compile sonnet] string or array of constraints woven into the planner prompt."
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
                    "description": "[execute] workstation-dispatch-record strategy. Surfaced in the response and the plan_runner_dispatch evidence entry. Unknown values are normalised to `unknown`. Internal mode forwards dispatch_strategy to mission_execution(action=open), where the companion log now persists this field."
                },
                "target_project": {
                    "type": "string",
                    "description": "[execute] registered project id OR project root path. For mission_execution it is forwarded as `project`; for mission_task_delegate it is treated as cwd if it looks like a path."
                },
                "cwd": {
                    "type": "string",
                    "description": "[execute internal mission_task_delegate] working directory passed through to mission_task_delegate"
                },
                "requested_cwd": {
                    "type": "string",
                    "description": "[execute internal mission_execution] working directory metadata persisted on the companion log when present (workstation-dispatch-record :requested-cwd)"
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
