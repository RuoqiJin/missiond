use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_workflow",
        "workflow 表 manager — 8 actions (list/get/match/apply/distill/record_execution/compile_methodology/run_methodology)。\
         list/get/match/apply/record_execution 为 store-backed full；\
         distill 默认 dry-run，传 distill_mode=\"sonnet\" 触发 workflow-distiller actor v0 \
         (从 plan + evidence sidecar 蒸馏 workflow_sexp + match_rules)，persist=true 写 workflow 行；\
         compile_methodology 读 `.missiond/workflows/<name>.lisp`：默认 compile_mode=\"dry_run\" 给预览；\
         compile_mode=\"deterministic\" 走 v0 编译器 (paren-validate + (step …) 提取 + 生成可被 mission_flow_run 加载的 YAML)；\
         v0 同时保守提取 6 类 methodology 高阶语义 (phase / principle / anti-pattern / gate / artifact / authority) 写入 \
         generated YAML 的 `methodology_metadata` (FlowDefinition 加载时静默忽略，原始 YAML 保留供人类/未来 forge compiler 使用)，\
         不会把这些 form 强行变成可执行 node — phase 内含 step 时 step 节点带 `methodology_metadata.phase_id`，无 step 仅有 \
         phase/principle 时仍只生成单个 manual_review 节点；persist=true 写到 `.missiond/generated/flows/<flow_id>.yaml` \
         (atomic, overwrite 控制) 并附 source_hash + lifted_form_count + lifted_form_breakdown；\
         run_methodology 解析 flow_id|flow_path|name 找 compiled YAML，dry_run=true 返 would_run，\
         dry_run=false 内部派发到 mission_flow_run 引擎；缺 YAML 时返结构化 MISSING_COMPILED_FLOW + 下一步指引。\
         Lisp 源: intent-flow.lisp :: F-methodology-to-executable-compile + intent-tools.lisp :: \
         implemented-surface mission_workflow + intent-intent-layer.lisp :: section unified-entry-pipeline :: \
         role workflow-distiller。",
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
                    "description": "[distill|compile_methodology] distill: insert a workflow row (default false; requires `name`). compile_methodology: write generated YAML to `.missiond/generated/flows/<flow_id>.yaml` (default false)."
                },
                "distill_mode": {
                    "type": "string",
                    "enum": ["dry_run", "sonnet"],
                    "description": "[distill] dry_run (default) keeps legacy preview; sonnet drives the workflow-distiller actor v0 (Sonnet over plan + evidence sidecar)"
                },
                "compile_mode": {
                    "type": "string",
                    "enum": ["dry_run", "deterministic"],
                    "description": "[compile_methodology] dry_run (default) keeps legacy lint preview; deterministic runs the v0 compiler (paren-validate + (step …) extraction + executable YAML emission)"
                },
                "output_flow_id": {
                    "type": "string",
                    "description": "[compile_methodology] explicit flow_id for the generated YAML (overrides default `methodology-<stem>-v0`)"
                },
                "params": {
                    "type": "object",
                    "description": "[compile_methodology|run_methodology] caller-supplied params; compile_methodology only echoes them in the response preview, run_methodology forwards them as FlowContext seed vars"
                },
                "overwrite": {
                    "type": "boolean",
                    "description": "[compile_methodology] when persist=true, allow overwriting an existing generated YAML (default false)"
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "[run_methodology] when true (default), return a `would_run` descriptor without dispatching; when false, internally call mission_flow_run on the compiled YAML"
                },
                "flow_id": {
                    "type": "string",
                    "description": "[run_methodology] flow id for the compiled YAML under `.missiond/generated/flows/<flow_id>.yaml`"
                },
                "flow_path": {
                    "type": "string",
                    "description": "[compile_methodology|run_methodology] explicit path to a methodology .lisp file (compile) or a compiled YAML (run)"
                },
                "project": {
                    "type": "string",
                    "description": "[distill|compile_methodology|run_methodology] project id (registry-resolved root); defaults to CWD. distill uses it to locate `.missiond/v2/plans/<plan_id>.evidence.json`."
                },
                "target_project": {
                    "type": "string",
                    "description": "[compile_methodology|run_methodology] alias of `project` for callers that prefer the explicit name"
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
