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
         wave-14 file-first SSOT: distill / compile_methodology persist=true 时再传 write_file=true 即把 \
         workflow_sexp (distill) 或 source content (compile_methodology) 镜像到 \
         `<project_root>/.missiond/workflows/<topic>.lisp` (ArtifactKind::Workflow, atomic, 默认拒覆, \
         overwrite_file=true 替换); topic 默认取 distill 的 `name` 或 compile_methodology 的源文件 stem; \
         project root 解析强制走 resolve_target_project_root (project > absolute cwd > target_project, 禁止 process cwd fallback); \
         DB / YAML 已写但 file 写失败 → status=\"partial\" + file_write_error, 不回滚已落的 row/yaml; \
         成功响应附 file_written / file_path / file_sha256 / file_bytes / file_created / file_overwritten。\
         wave-14 review gate auto-create v1: distill / compile_methodology persist=true 时再传 \
         review_gate_policy=\"emit_question\" 即在 file_written=true 后自动 fire 一条 QuestionEvent::Created \
         (deterministic id = review:workflow:<id|flow_id>:v<version>:compile:<topic-hash>); \
         review_gate_policy=\"manual\" (默认) 保留 wave-11 显式 emit_review_question=true 路径; \
         review_gate_policy=\"off\" 同时压制两者; 不实现 UI / 不等回答 / 不自动 approve; \
         bus 失败 surface review_question_warning + 确定性 id 供重试; \
         compile_methodology 因为暂无 workflow_id 行,确定性 id 锚定在 flow_id 上。\
         响应总附 review_gate_policy / review_question_emitted (+ review_question_id / review_question_warning when applicable)。\
         Lisp 源: intent-flow.lisp :: F-methodology-to-executable-compile + intent-tools.lisp :: \
         implemented-surface mission_workflow + intent-intent-layer.lisp :: section unified-entry-pipeline :: \
         role workflow-distiller + intent-memory.lisp :: directive-layer :: file-first-artifacts :: workflow-methodology-file。",
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
                },
                "write_file": {
                    "type": "boolean",
                    "description": "[distill persist=true | compile_methodology persist=true] (wave-14 file-first SSOT) mirror the workflow_sexp (distill) or methodology source (compile_methodology) to `<project_root>/.missiond/workflows/<topic>.lisp` after the DB row / YAML is committed. Default false. Topic precedence: explicit `topic` > distill's `name` / compile_methodology's source stem. DB / YAML stay committed even on file failure — response surfaces status=\"partial\" + file_write_error."
                },
                "overwrite_file": {
                    "type": "boolean",
                    "description": "[distill|compile_methodology persist=true write_file=true] allow replacing an existing workflow .lisp at the target path (default false → atomic refusal). Distinct from `overwrite` which controls the generated YAML instead."
                },
                "topic": {
                    "type": "string",
                    "description": "[distill|compile_methodology persist=true write_file=true] file-first SSOT topic segment used to derive `.missiond/workflows/<topic>.lisp`. Sanitized (alnum / `_` / `-`); blank inputs collapse to `anonymous`."
                },
                "review_gate_policy": {
                    "type": "string",
                    "enum": ["manual", "emit_question", "off"],
                    "description": "[distill persist=true | compile_methodology persist=true] (wave-14 review gate auto-create v1) controls automatic QuestionEvent::Created emission AFTER a successful workflow .lisp file-first write. `manual` (default) keeps the legacy explicit-emit path (`emit_review_question=true`) the only way to fire an event; `emit_question` auto-fires when `write_file=true` AND the file landed (`file_written=true`); `off` suppresses BOTH the auto-emit and the legacy bool. Response always echoes the resolved policy. Auto-emit is fire-and-forget on the bus (never blocks, never auto-approves, never waits). compile_methodology has no workflow_id row, so the deterministic id is anchored on `flow_id` instead. Bus failures surface `review_question_warning` + the deterministic id for caller retry / manual resolution."
                },
                "emit_review_question": {
                    "type": "boolean",
                    "description": "[distill persist=true | compile_methodology persist=true review_gate_policy=manual] (wave-11 explicit-emit path) fire one QuestionEvent::Created after the workflow row / methodology YAML is committed. Best-effort; bus failures surface `review_question_warning` instead of failing the action. Ignored when `review_gate_policy=emit_question` (auto-emit takes over) or `review_gate_policy=off` (suppression)."
                },
                "review_question_text": {
                    "type": "string",
                    "description": "[distill | compile_methodology emit_review_question=true | review_gate_policy=emit_question] free-form prompt echoed back in the response payload (`review_question_text`); the bus event itself only carries the deterministic id."
                },
                "review_question_id": {
                    "type": "string",
                    "description": "[distill | compile_methodology persist=true] deterministic question-id override. Replaces the auto-derived id (`review:workflow:<id>:v<version>:compile[:<topic-hash>]`). Same fire-and-forget, bus-failure-warns semantics as the directive/plan surfaces."
                }
            }
        }),
    )]
}
