use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_workflow",
        "workflow 表 manager — 8 actions (list/get/match/apply/distill/record_execution/compile_methodology/run_methodology)。\
         list/get/match/apply/record_execution 为 store-backed full；\
         distill 默认 dry-run，传 distill_mode=\"sonnet\" 触发 workflow-distiller actor v0 \
         (从 plan + evidence sidecar 蒸馏 workflow_sexp + match_rules)，persist=true 写 workflow 行；\
         compile_methodology 读 `.missiond/workflows/<name>.lisp` 产出 dry-run 预览（YAML emitter actor 未落地）；\
         run_methodology 返回 not_implemented，附下一步指引（compile_methodology + 手写 YAML + mission_flow_run）。\
         Lisp 源: intent-memory.lisp :: module directive-layer :: plumbing workflow-templates \
         + intent-tools.lisp :: implemented-surface mission_workflow + intent-flow.lisp :: \
         F-intent-alignment-plan-execution-loop :: s8 workflow-distillation \
         + intent-intent-layer.lisp :: section unified-entry-pipeline :: role workflow-distiller。",
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
                    "description": "manager action — see Lisp implemented-surface mission_workflow"
                },
                "name": {
                    "type": "string",
                    "description": "[get|apply|distill|compile_methodology|run_methodology] workflow.name (UNIQUE) or methodology basename. distill: optional in dry_run, required when persist=true."
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
                    "description": "[distill] insert a workflow row (default false). Requires `name`."
                },
                "distill_mode": {
                    "type": "string",
                    "enum": ["dry_run", "sonnet"],
                    "description": "[distill] dry_run (default) keeps legacy preview; sonnet drives the workflow-distiller actor v0 (Sonnet over plan + evidence sidecar)"
                },
                "project": {
                    "type": "string",
                    "description": "[distill|compile_methodology] project id (registry-resolved root); defaults to CWD. distill uses it to locate `.missiond/v2/plans/<plan_id>.evidence.json`"
                },
                "match_hint": {
                    "description": "[distill:sonnet] optional hint passed to the distiller as context — string or array of strings",
                    "oneOf": [
                        {"type": "string"},
                        {"type": "array", "items": {"type": "string"}}
                    ]
                },
                "protected": {
                    "type": "boolean",
                    "description": "[distill:sonnet] when provided, recorded into match_rules.protected (LRU/down-rank guard); only emitted when set"
                },
                "min_evidence": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "[distill:sonnet] minimum number of evidence entries required before invoking the distiller (default 1)"
                },
                "allow_missing_evidence": {
                    "type": "boolean",
                    "description": "[distill:sonnet] when true, bypass the evidence-sidecar gate (default false)"
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
                }
            }
        }),
    )]
}
