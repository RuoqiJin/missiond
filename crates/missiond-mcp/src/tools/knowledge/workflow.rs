use crate::ToolDefinition;
use serde_json::{json, Map, Value};

/// Build a single property descriptor `{"type": ..., "description": ...}`.
fn prop(ty: &str, description: &str) -> Value {
    json!({"type": ty, "description": description})
}

fn prop_enum(ty: &str, description: &str, variants: &[&str]) -> Value {
    json!({
        "type": ty,
        "enum": variants,
        "description": description,
    })
}

fn prop_oneof_string_or_array(description: &str) -> Value {
    json!({
        "description": description,
        "oneOf": [
            {"type": "string"},
            {"type": "array", "items": {"type": "string"}}
        ]
    })
}

fn build_properties() -> Value {
    let mut p: Map<String, Value> = Map::new();

    p.insert("action".into(), prop_enum(
        "string",
        "manager action — see Lisp implemented-surface mission_workflow",
        &[
            "list", "get", "match", "apply",
            "distill", "record_execution",
            "compile_methodology", "run_methodology",
            "resolve_review",
        ],
    ));

    p.insert("name".into(), prop(
        "string",
        "[get|apply|distill|compile_methodology|run_methodology] workflow.name (UNIQUE) or methodology basename. distill: optional in dry_run, required when persist=true.",
    ));

    p.insert("workflow_id".into(), prop(
        "string",
        "[get|apply] workflow UUID",
    ));

    p.insert("utterance".into(), prop(
        "string",
        "[match] free-form query — currently substring match over match_rules",
    ));

    p.insert("limit".into(), prop(
        "integer",
        "[list] cap result count (1-500, default 50)",
    ));

    p.insert("plan_id".into(), prop(
        "string",
        "[distill] succeeded plan UUID to distill from",
    ));

    p.insert("persist".into(), prop(
        "boolean",
        "[distill|compile_methodology] distill: insert a workflow row (default false; requires `name`). compile_methodology: write generated YAML to `.missiond/generated/flows/<flow_id>.yaml` (default false).",
    ));

    p.insert("distill_mode".into(), prop_enum(
        "string",
        "[distill] dry_run (default) keeps legacy preview; sonnet drives the workflow-distiller actor v0 (Sonnet over plan + evidence sidecar)",
        &["dry_run", "sonnet"],
    ));

    p.insert("compile_mode".into(), prop_enum(
        "string",
        "[compile_methodology] dry_run (default) keeps legacy lint preview; deterministic runs the v0 compiler (paren-validate + (step …) extraction + executable YAML emission)",
        &["dry_run", "deterministic"],
    ));

    p.insert("output_flow_id".into(), prop(
        "string",
        "[compile_methodology] explicit flow_id for the generated YAML (overrides default `methodology-<stem>-v0`)",
    ));

    p.insert("params".into(), json!({
        "type": "object",
        "description": "[compile_methodology|run_methodology] caller-supplied params; compile_methodology only echoes them in the response preview, run_methodology forwards them as FlowContext seed vars",
    }));

    p.insert("overwrite".into(), prop(
        "boolean",
        "[compile_methodology] when persist=true, allow overwriting an existing generated YAML (default false)",
    ));

    p.insert("dry_run".into(), prop(
        "boolean",
        "[run_methodology] when true (default), return a `would_run` descriptor without dispatching; when false, internally call mission_flow_run on the compiled YAML",
    ));

    p.insert("flow_id".into(), prop(
        "string",
        "[run_methodology] flow id for the compiled YAML under `.missiond/generated/flows/<flow_id>.yaml`",
    ));

    p.insert("flow_path".into(), prop(
        "string",
        "[compile_methodology|run_methodology] explicit path to a methodology .lisp file (compile) or a compiled YAML (run)",
    ));

    p.insert("project".into(), prop(
        "string",
        "[distill|compile_methodology|run_methodology] project id (registry-resolved root); defaults to CWD. distill uses it to locate `.missiond/v2/plans/<plan_id>.evidence.json`.",
    ));

    p.insert("target_project".into(), prop(
        "string",
        "[compile_methodology|run_methodology] alias of `project` for callers that prefer the explicit name",
    ));

    p.insert("match_hint".into(), prop_oneof_string_or_array(
        "[distill:sonnet] optional hint passed to the distiller as context — string or array of strings",
    ));

    p.insert("protected".into(), prop(
        "boolean",
        "[distill:sonnet] when provided, recorded into match_rules.protected (LRU/down-rank guard); only emitted when set",
    ));

    p.insert("min_evidence".into(), json!({
        "type": "integer",
        "minimum": 0,
        "description": "[distill:sonnet] minimum number of evidence entries required before invoking the distiller (default 1)",
    }));

    p.insert("allow_missing_evidence".into(), prop(
        "boolean",
        "[distill:sonnet] when true, bypass the evidence-sidecar gate (default false)",
    ));

    p.insert("success".into(), prop(
        "boolean",
        "[record_execution] outcome (rolling avg/cost update)",
    ));

    p.insert("cost_usd".into(), prop(
        "number",
        "[record_execution] optional cost contribution (USD)",
    ));

    p.insert("workflow_path".into(), prop(
        "string",
        "[compile_methodology|run_methodology] explicit path to a methodology .lisp file",
    ));

    p.insert("write_file".into(), prop(
        "boolean",
        "[distill persist=true | compile_methodology persist=true] (wave-14 file-first SSOT) mirror the workflow_sexp (distill) or methodology source (compile_methodology) to `<project_root>/.missiond/workflows/<topic>.lisp` after the DB row / YAML is committed. Default false. Topic precedence: explicit `topic` > distill's `name` / compile_methodology's source stem. DB / YAML stay committed even on file failure — response surfaces status=\"partial\" + file_write_error.",
    ));

    p.insert("overwrite_file".into(), prop(
        "boolean",
        "[distill|compile_methodology persist=true write_file=true] allow replacing an existing workflow .lisp at the target path (default false → atomic refusal). Distinct from `overwrite` which controls the generated YAML instead.",
    ));

    p.insert("topic".into(), prop(
        "string",
        "[distill|compile_methodology persist=true write_file=true] file-first SSOT topic segment used to derive `.missiond/workflows/<topic>.lisp`. Sanitized (alnum / `_` / `-`); blank inputs collapse to `anonymous`.",
    ));

    p.insert("review_gate_policy".into(), prop_enum(
        "string",
        "[distill persist=true | compile_methodology persist=true] (wave-14 review gate auto-create v1) controls automatic QuestionEvent::Created emission AFTER a successful workflow .lisp file-first write. `manual` (default) keeps the legacy explicit-emit path (`emit_review_question=true`) the only way to fire an event; `emit_question` auto-fires when `write_file=true` AND the file landed (`file_written=true`); `off` suppresses BOTH the auto-emit and the legacy bool. Response always echoes the resolved policy. Auto-emit is fire-and-forget on the bus (never blocks, never auto-approves, never waits). compile_methodology has no workflow_id row, so the deterministic id is anchored on `flow_id` instead. Bus failures surface `review_question_warning` + the deterministic id for caller retry / manual resolution.",
        &["manual", "emit_question", "off"],
    ));

    p.insert("emit_review_question".into(), prop(
        "boolean",
        "[distill persist=true | compile_methodology persist=true review_gate_policy=manual] (wave-11 explicit-emit path) fire one QuestionEvent::Created after the workflow row / methodology YAML is committed. Best-effort; bus failures surface `review_question_warning` instead of failing the action. Ignored when `review_gate_policy=emit_question` (auto-emit takes over) or `review_gate_policy=off` (suppression).",
    ));

    p.insert("review_question_text".into(), prop(
        "string",
        "[distill | compile_methodology emit_review_question=true | review_gate_policy=emit_question] free-form prompt echoed back in the response payload (`review_question_text`); the bus event itself only carries the deterministic id.",
    ));

    p.insert("review_question_id".into(), prop(
        "string",
        "[distill | compile_methodology persist=true | resolve_review] On distill / compile_methodology: deterministic question-id override that replaces the auto-derived id (`review:workflow:<id|flow_id>:v<version>:compile[:<topic-hash>]`); same fire-and-forget, bus-failure-warns semantics as the directive/plan surfaces. On resolve_review (wave-16): REQUIRED — the deterministic id wave-14 emitted on the workflow Created event; resolver parses scope/artifact_id/version/action and validates against the workflow surface (scope=`workflow`, action whitelist `[compile]`, version pinned to v1).",
    ));

    p.insert("review_decision".into(), prop_enum(
        "string",
        "[resolve_review] (wave-16 explicit resolution) caller's decision attached to `review_question_id`. `approved` stamps `status=review_approved` (no DB transition — Workflow row has no status column; methodology branch never had a row). `rejected` keeps the artifact non-approved. `needs_changes` keeps the artifact non-approved AND surfaces a `next_step` recommendation pointing back to `distill` (persisted) or `compile_methodology` (methodology). Required when `review_question_id` is supplied — fail-fast on missing.",
        &["approved", "rejected", "needs_changes"],
    ));

    p.insert("review_actor".into(), prop(
        "string",
        "[resolve_review] (wave-16 explicit resolution) free-form identity of the resolver. Echoed into the response payload and the Resolved bus event metadata; never used for authentication.",
    ));

    p.insert("review_note".into(), prop(
        "string",
        "[resolve_review] (wave-16 explicit resolution) free-form reason / next-step text. Echoed into the response payload so callers see the rejection / change request rationale.",
    ));

    p.insert("review_automation_policy".into(), prop_enum(
        "string",
        "[resolve_review] (wave-18 / task 07 review automation policy v0) explicit autonomy knob. ORTHOGONAL to the wave-14 `review_gate_policy` (which controls EMISSION). `manual` (default) preserves the wave-16 behaviour byte-for-byte: explicit `review_decision` is required. `suggest` computes a deterministic `suggested_review_decision` and surfaces it without mutating. `auto_safe` may stamp `status=review_approved` automatically only when ALL safety rules pass (producer deterministic, no file write OR hash match, no protected source/target, no conflicts, caller opted in). Workflow rows have NO status column to flip — even auto-promoted approval is receipt-only (the bus Resolved event carries the same signal). The `methodology` branch (compile_methodology) is fully deterministic so the rule auto-passes; the `persisted` (distill) branch defaults to non-deterministic unless the caller passes `deterministic_workflow=true`. NEVER auto-rejects. NEVER calls an LLM. Caller-supplied `review_decision` ALWAYS wins.",
        &["manual", "suggest", "auto_safe"],
    ));

    p.insert("deterministic_workflow".into(), prop(
        "boolean",
        "[resolve_review review_automation_policy=auto_safe] (wave-18 / task 07) opt the persisted (distill) branch into the deterministic-mode rule. Workflow rows carry no `compiler_model` field, so by default `auto_safe` blocks distill approvals unless the caller explicitly attests the underlying workflow_sexp came from a deterministic producer (e.g. dry-run preview promoted to row). Methodology branch always runs deterministically and ignores this flag.",
    ));

    p.insert("expected_file_sha256".into(), prop(
        "string",
        "[resolve_review review_automation_policy=auto_safe] (wave-18 / task 07) optional caller-supplied SHA-256 the safety inspector requires to match the on-disk workflow .lisp hash. Replay the `file_sha256` captured from the original distill / compile_methodology response so an unexpected on-disk modification blocks `auto_safe`.",
    ));

    Value::Object(p)
}

pub fn definitions() -> Vec<ToolDefinition> {
    let schema = json!({
        "type": "object",
        "required": ["action"],
        "properties": build_properties(),
    });
    vec![ToolDefinition::new(
        "mission_workflow",
        "workflow 表 manager — 9 actions (list/get/match/apply/distill/record_execution/compile_methodology/run_methodology/resolve_review)。\
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
         wave-16 explicit review resolution (action=resolve_review): 接 review_question_id + review_decision \
         (approved|rejected|needs_changes) + 可选 review_actor / review_note, 与 directive/plan 的 wave-15 \
         resolution 路径同形 — fail-fast on missing decision / unsupported scope / stale version / \
         artifact id mismatch / unsupported action; 用 scope=`workflow` (与 wave-14 auto-emit 同), \
         action 白名单仅 `compile`, version pin 在 v1; persisted (distill) 路径用 workflow UUID, \
         由于 Workflow 行无 status/version 字段不做 DB transition, approved 仅 stamp `status=review_approved` \
         loud; methodology (compile_methodology) 路径用 flow_id 字符串(非 UUID), 完全无 DB 变更, 返结构化 \
         receipt (`mode=methodology`, `db_transition=false`); needs_changes 返 next_step 指向 \
         distill / compile_methodology; bus 失败 surface review_question_warning, 不回滚 receipt; \
         publish QuestionEvent::Resolved/DecisionResolved best-effort 与 directive/plan 对齐。\
         wave-18 / task 07 review automation policy v0: resolve_review 接 review_automation_policy ∈ {manual, suggest, auto_safe} \
         (默认 manual, 与 wave-16 byte-identical) — manual 保持现有 explicit-decision 唯一权威; suggest 计算 \
         deterministic suggested_review_decision 并 surface 但不 mutate; auto_safe 仅在所有安全规则通过时 \
         (producer deterministic + 无文件写或 hash 匹配 + 无 protected source/target + 无 unresolved conflict + caller opt-in) \
         自动 stamp review_approved (workflow 行无 status 列, approval receipt-only); 永不 auto-reject; \
         永不 call LLM; explicit review_decision 永远 win over policy。响应总附 review_automation_policy / \
         review_automation_status / suggested_review_decision / automation_reasons[]。\
         Lisp 源: intent-flow.lisp :: F-methodology-to-executable-compile + intent-tools.lisp :: \
         implemented-surface mission_workflow + intent-intent-layer.lisp :: section unified-entry-pipeline :: \
         role workflow-distiller + intent-memory.lisp :: directive-layer :: file-first-artifacts :: workflow-methodology-file。",
        schema,
    )]
}
