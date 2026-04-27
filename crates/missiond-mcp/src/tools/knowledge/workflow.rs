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

    p.insert("auto_chain".into(), prop(
        "boolean",
        "[distill] (wave-19 / task 09 cross-plan distill auto-chain v1) opt-in cross-plan distill chain recorder anchored on the workflow surface. Default false → response is byte-identical with the wave-18 distill payload (no `auto_chain` block). When true, AFTER the caller-chosen distill mode (`distill_mode=\"dry_run\"|\"sonnet\"`) completes successfully the daemon DERIVES a deterministic chain id from `project_root + plan_id + (workflow_id|name|<unnamed>) + sha256(.evidence.json|<no-evidence>)` (sha256-hashed, prefixed `chain:auto:wf-`), appends ONE chain-record evidence row to `<project_root>/.missiond/v2/plans/<plan_id>.evidence.json` (kind=`distill_chain_record`, source=`workflow_distill_auto_chain`; same kind as wave-18 plan_dag-driven entries so audit queries see both), and splices an `auto_chain` block onto the response (`auto_chain.{requested,status,chain_id,chain_id_source=\"derived_from_workflow_context\",chain_id_inputs,evidence_path|evidence_error}` + top-level shortcuts `auto_chain_status` / `auto_chain_id`). NEVER calls Sonnet implicitly — the auto-chain hook only RECORDS; the upstream distill call already chose its mode. Sidecar append is purely additive (no migration). Failures collapse to status `resolve_failed` (project root unresolvable; no chain id surfaced) or `record_failed` (chain id IS surfaced + `evidence_error`). Inner distill errors are preserved verbatim — auto_chain is skipped on error envelopes (mirrors plan.rs's `apply_distill_chain` policy).",
    ));

    p.insert("auto_chain_name".into(), prop(
        "string",
        "[distill auto_chain=true] (wave-19 / task 09) optional free-form chain label echoed into the `auto_chain.chain_name` block + chain-record evidence row. Mirrors plan.rs's `distill_chain_name` semantics so a single dashboard query can group both recorders' rows by label without parsing the deterministic id. Blank / missing → omitted from the response.",
    ));

    p.insert("auto_chain_trigger".into(), prop_enum(
        "string",
        "[distill] (wave-20 / task 06 cross-plan distill auto-trigger v1) ORTHOGONAL to the wave-19 `auto_chain` opt-in. `never` (default) preserves the wave-19 behaviour byte-for-byte: chain recording requires explicit `auto_chain=true`. `auto_safe` evaluates a deterministic safety-rule set (inner_distill_succeeded / distill_mode_recorded / project_root_resolved / evidence_sidecar_present / evidence_min_entries / chain_id_not_already_recorded) AFTER the caller-chosen distill mode; ALL rules must pass before the daemon falls through to the wave-19 recorder. Rule failure → `auto_trigger.trigger_status=\"skipped_rules_failed\"` with the full rule outcomes; NEVER partially appends. Rule pass → behaves as if `auto_chain=true` (same evidence row, same `auto_chain` block + shortcuts) AND splices an `auto_trigger` block (`{requested,mode,trigger_status,safety_rule_results,chain_id?,sidecar?}` + top-level shortcuts `auto_trigger_status` / `auto_trigger_chain_id`). NEVER calls Sonnet implicitly — the trigger only enables the record-only auto-chain hook; the upstream `distill_mode` arg remains the only way to invoke the distiller actor. Inner distill error → `auto_trigger.trigger_status=\"skipped_inner_error\"` (audit can tell rule failure apart from upstream failure).",
        &["never", "auto_safe"],
    ));

    p.insert("auto_trigger_min_evidence".into(), json!({
        "type": "integer",
        "minimum": 0,
        "description": "[distill auto_chain_trigger=auto_safe] (wave-20 / task 06) minimum sidecar entry count required by the `evidence_min_entries` safety rule (default 1). Mirrors the upstream sonnet distill `min_evidence` gate so the trigger's notion of 'real evidence' matches the distiller's; raise to require richer sidecars before auto-recording.",
    }));

    p.insert("auto_sonnet".into(), prop(
        "boolean",
        "[distill] (wave-21 / task 07 sonnet distill chain auto-apply v1) opt-in apply-gate that auto-promotes the inner distill from `dry_run` to `sonnet` ONLY when ALL six wave-20 deterministic safety rules pass AND the caller explicitly attests `auto_sonnet_approved=true`. Default false → response is byte-identical with the wave-20 trigger payload (no `auto_sonnet` block). Conservative invariants: I1 default-off byte-shape preserved; I2 NEVER auto-invokes Sonnet without BOTH `auto_sonnet=true` AND `auto_sonnet_approved=true` (single-typo escalation impossible); I3 NEVER without all six wave-20 safety rules passing (gate REUSES trigger outcomes, never relaxes); I4 NEVER when caller's `distill_mode` was already `sonnet` (no double-call); I5 on Sonnet failure (model error / invalid output) PRESERVES inner payload + surfaces `model_call_status=failed|invalid_output`; I6 `review_required=true` PINNED on every successful auto-sonnet outcome (auto-applied sonnet stays receipt-only — no DB transition flips); I7 wave-19 `auto_chain` + wave-20 `auto_trigger` blocks remain UNCHANGED (auto_sonnet is purely additive). Status taxonomy: `not_requested` | `disabled` | `skipped_no_trigger` (wave-20 trigger not enabled) | `skipped_rules_failed` (any wave-20 rule failed) | `skipped_caller_approval_missing` (auto_sonnet_approved=false) | `skipped_already_sonnet` (caller's distill_mode already sonnet) | `skipped_inner_error` (inner distill returned error envelope) | `applied_sonnet` (model invoked, payload promoted). model_call_status taxonomy: `not_invoked` | `invoked` | `failed` | `invalid_output`. Block shape: {requested, status, applied, review_required, model_call_status, safety_rule_results, caller_approval, caller_distill_mode?, model_call_error?, sidecar?, chain_id?} + top-level shortcut `auto_sonnet_status`. Strict shape: `auto_sonnet=\"true\"` (string) and `auto_sonnet_approved=1` (number) fail-fast INVALID_PARAM at action entry.",
    ));

    p.insert("auto_sonnet_approved".into(), prop(
        "boolean",
        "[distill auto_sonnet=true] (wave-21 / task 07) explicit caller-attestation flag the apply-gate requires before invoking Sonnet automatically. ORTHOGONAL to `auto_sonnet` so a single typo cannot escalate the daemon: BOTH `auto_sonnet=true` AND `auto_sonnet_approved=true` must be set. Defaults to false. Strict shape: `auto_sonnet_approved=\"true\"` (string) fails fast INVALID_PARAM. Echoed verbatim into the `auto_sonnet.caller_approval` block field.",
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
         wave-19 / task 09 cross-plan distill auto-chain v1: distill 接 auto_chain=true 即在 inner distill 成功后 \
         derive 确定性 chain id 自 project_root + plan_id + (workflow_id|name|<unnamed>) + sha256(.evidence.json|<no-evidence>) \
         (sha256 哈希, 前缀 `chain:auto:wf-`), append 一条 distill_chain_record (source=`workflow_distill_auto_chain`) \
         evidence row 到 `<project_root>/.missiond/v2/plans/<plan_id>.evidence.json`, 响应附 auto_chain block \
         (chain_id / chain_id_source=\"derived_from_workflow_context\" / chain_id_inputs / evidence_path|evidence_error) \
         + top-level shortcuts auto_chain_status / auto_chain_id。default false 保 byte-identical (wave-18 distill 形状不变); \
         永不 implicit sonnet (只 record, 不重派 distiller); sidecar 严格 append-only (无 migration); \
         resolve_failed / record_failed 不破坏 inner distill payload。\
         wave-20 / task 06 cross-plan distill auto-trigger v1: distill 接 auto_chain_trigger=\"auto_safe\" (默认 \"never\") \
         在 inner distill 成功后跑 6 条 deterministic 安全规则 (inner_distill_succeeded / distill_mode_recorded / \
         project_root_resolved / evidence_sidecar_present / evidence_min_entries / chain_id_not_already_recorded), \
         全部通过才 fall-through 到 wave-19 recorder; 任一失败 surface auto_trigger.trigger_status=\"skipped_rules_failed\" \
         + 完整 safety_rule_results, 绝不 partially append; 通过则同时 splice auto_chain block (与 wave-19 一致) 和 \
         auto_trigger block (trigger_status=\"triggered\" / chain_id / sidecar / safety_rule_results) + top-level \
         shortcuts auto_trigger_status / auto_trigger_chain_id。永不 implicit sonnet (trigger 只放行 record-only \
         hook, 不触发 distiller actor); auto_chain_trigger 与 wave-19 auto_chain 正交 (两者皆 default off, \
         任一 opt-in 都走同一条 wave-19 record 路径)。\
         wave-21 / task 07 sonnet distill chain auto-apply v1: distill 接 auto_sonnet=true + auto_sonnet_approved=true \
         (默认皆 false 保 byte-identical), 在 wave-20 trigger=auto_safe 且全部 6 条 deterministic 规则通过 \
         且 caller 显式 approval 且 distill_mode 不是已经 sonnet 的前提下, 自动把 inner distill 从 \
         dry_run 提升到 sonnet (mission_workflow.action_distill_sonnet 内部直调) 并把 sonnet payload 作为 \
         outer payload 返回 (carry forward wave-19 auto_chain + wave-20 auto_trigger 两个 block 不丢 receipt)。\
         保守不变量 I1-I7: I1 default-off 完全保 byte-shape; I2 auto_sonnet 与 auto_sonnet_approved 必须双 \
         opt-in (单 typo 不会升级); I3 必须 6 规则全过, gate 复用 trigger 的 outcomes 不 relax; I4 caller \
         distill_mode 已是 sonnet 时拒绝 (no double-call); I5 sonnet 失败时 PRESERVE inner payload + surface \
         model_call_status=failed|invalid_output, 不破坏现有 dry_run 工件; I6 review_required=true PINNED \
         (auto-applied sonnet 永远是 receipt-only, 无 DB transition); I7 wave-19/20 blocks 保持不变 \
         (auto_sonnet 是 purely additive)。Status: not_requested | disabled | skipped_no_trigger | \
         skipped_rules_failed | skipped_caller_approval_missing | skipped_already_sonnet | skipped_inner_error | \
         applied_sonnet。model_call_status: not_invoked | invoked | failed | invalid_output。Block shape: \
         {requested, status, applied, review_required, model_call_status, safety_rule_results, caller_approval, \
         caller_distill_mode?, model_call_error?, sidecar?, chain_id?} + top-level shortcut auto_sonnet_status。\
         Strict shape: auto_sonnet=\"true\" (string) / auto_sonnet_approved=1 (number) → INVALID_PARAM fail-fast。\
         Lisp 源: intent-flow.lisp :: F-methodology-to-executable-compile + intent-tools.lisp :: \
         implemented-surface mission_workflow + intent-intent-layer.lisp :: section unified-entry-pipeline :: \
         role workflow-distiller + intent-memory.lisp :: directive-layer :: file-first-artifacts :: workflow-methodology-file。",
        schema,
    )]
}
