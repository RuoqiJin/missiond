use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_directive",
        "directive 表 manager — 6 actions (compile/list/get/approve/archive/version_chain)。\
         compile 是 directive-compiler actor v0：默认 compiler_mode=\"dry_run\" 不调 LLM；\
         compiler_mode=\"sonnet\" 走 SonnetGateway interactive 通道把 utterance 编译成可 review 的 \
         directive sexp；persist=true 仅写 DirectiveStatus::Draft，等待人工 approve。\
         wave-14 file-first SSOT: persist=true 时再传 write_file=true + topic=… 即把 \
         compiled_sexp 镜像写到 `<project_root>/.missiond/alignment/<topic>/intent-alignment.lisp` \
         (ArtifactKind::IntentAlignment, atomic write, 默认拒覆，传 overwrite_file=true 才替换); \
         project root 解析强制走 slot_orchestrator::project_root::resolve_target_project_root \
         (project > absolute cwd > target_project, 禁止 process cwd fallback); \
         DB 行已写但 file 写失败 → status=\"partial\" + file_write_error, 不回滚 row。\
         成功响应附 file_written / file_path / file_sha256 / file_bytes / file_created / file_overwritten。\
         wave-14 review gate auto-create v1: persist=true 时再传 review_gate_policy=\"emit_question\" \
         即在 file_written=true 后自动 fire 一条 QuestionEvent::Created (deterministic id = \
         review:directive:<id>:v<version>:compile:<topic-hash>); review_gate_policy=\"manual\" (默认) \
         保留 wave-11 显式 emit_review_question=true 路径; review_gate_policy=\"off\" 同时压制两者。\
         不实现 UI / 不等回答 / 不自动 approve; bus 失败 surface review_question_warning + 确定性 id 供重试。\
         approve / archive 接收 review_question_id → 触发 QuestionEvent::Resolved (或 DecisionResolved)。\
         响应总附 review_gate_policy / review_question_emitted (+ review_question_id / review_question_warning when applicable)。\
         wave-15 review-resolution bridge v0: approve / archive 同时传 review_question_id + review_decision (approved | rejected | needs_changes) 时, \
         先 validate envelope (scope=directive, artifact=directive_id, version=chain head, action ∈ {compile|approve|archive}) 再决定: \
         approved → 跑 directive_approve / directive_update_status(Archived); rejected → 保持当前 status + status=\"review_rejected\"; \
         needs_changes → 保持当前 status + status=\"review_needs_changes\" + next_step。失败 (REVIEW_ID_MALFORMED / REVIEW_SCOPE_MISMATCH / \
         REVIEW_SCOPE_UNSUPPORTED / REVIEW_ARTIFACT_MISMATCH / STALE_REVIEW_VERSION / REVIEW_ACTION_UNSUPPORTED) 在 mutate 前 fail-fast。\
         不实现 UI / 不等 QuestionEvent::Resolved 回答 / 不自动 approve; bus 失败转 review_question_warning, DB 已 commit 时不回滚。\
         可选 review_actor + review_note 仅作 audit 字符串透传到 response。\
         list/get/approve/archive/version_chain 为 store-backed full。\
         wave-21 / task 06 LLM auto-approve proposal v0: approve / archive 可传 auto_approve_mode=\"sonnet_suggest\" \
         (默认 \"off\" 保持 byte-shape) 让 Sonnet PROPOSE 结构化 review 决定 (decision + confidence + evidence + non_goal_check + destructive_check + requires_human); \
         结果挂在 llm_auto_approve_proposal*; v0 propose-only — 永不 auto-apply, applied=false / requires_human=true 强制 pin; \
         destructive (archive | supersede | remove) 永远短路到 destructive_blocked 不调 LLM; rejected 从模型来时 demote 成 needs_changes (invariant I1); \
         Sonnet 不可用 → llm_unavailable 无 deterministic fallback (invariant I4); destructive_check 始终源自 is_destructive_review_action 不是模型 (invariant I5); \
         caller `review_decision` 永远胜过 proposal — 这层只是 informational hint 给 dashboard / UI; \
         与 wave-18 / 07 review_automation_policy + wave-20 / 08 auto_answer_policy ORTHOGONAL, 三者可同时存在。\
         Lisp 源: intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: \
         s2 intent-alignment-authoring + s3 alignment-review-gate \
         + intent-intent-layer.lisp :: section unified-entry-pipeline :: role alignment-author \
         + intent-memory.lisp :: module directive-layer :: file-first-artifacts :: intent-alignment-artifact \
         + intent-tools.lisp :: implemented-surface mission_directive。",
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["compile", "list", "get", "approve", "archive", "version_chain"],
                    "description": "manager action — see Lisp implemented-surface mission_directive"
                },
                "utterance": {
                    "type": "string",
                    "description": "[compile] user utterance to compile into a lisp directive"
                },
                "source": {
                    "type": "string",
                    "description": "[compile] provenance hint (default user_utterance)"
                },
                "conversation_id": {
                    "type": "string",
                    "description": "[compile] originating conversation id"
                },
                "persist": {
                    "type": "boolean",
                    "description": "[compile] insert a draft row (default false → preview only)"
                },
                "compiler_mode": {
                    "type": "string",
                    "enum": ["dry_run", "sonnet"],
                    "description": "[compile] dry_run (default, no LLM) | sonnet (directive-compiler actor v0 via SonnetGateway interactive)"
                },
                "review_gate": {
                    "type": "string",
                    "description": "[compile] free-form note about the review gate (recorded in references_json). NOT the wave-14 policy — see `review_gate_policy` for the manual|emit_question|off knob."
                },
                "review_gate_policy": {
                    "type": "string",
                    "enum": ["manual", "emit_question", "off"],
                    "description": "[compile persist=true] (wave-14 review gate auto-create v1) controls automatic QuestionEvent::Created emission AFTER a successful file-first artifact write. `manual` (default) keeps the legacy explicit-emit path (`emit_review_question=true`) the only way to fire an event; `emit_question` auto-fires when `write_file=true` AND the file landed (`file_written=true`); `off` suppresses BOTH the auto-emit and the legacy bool. Response always echoes the resolved policy under `review_gate_policy`. The auto-emit is fire-and-forget on the bus side: it never blocks the response, never auto-approves, and never waits for a human gate. Bus failures surface `review_question_warning` + the deterministic id so the caller can retry / resolve manually with the same id."
                },
                "emit_review_question": {
                    "type": "boolean",
                    "description": "[compile persist=true review_gate_policy=manual] (wave-11 explicit-emit path) fire one QuestionEvent::Created after the directive draft is committed. Best-effort; bus failures surface `review_question_warning` instead of failing the compile. Ignored when `review_gate_policy=emit_question` (auto-emit takes over) or `review_gate_policy=off` (suppression)."
                },
                "review_question_text": {
                    "type": "string",
                    "description": "[compile persist=true emit_review_question=true | review_gate_policy=emit_question] free-form prompt echoed back in the response payload (`review_question_text`); the bus event itself only carries the deterministic id."
                },
                "review_question_id": {
                    "type": "string",
                    "description": "[compile persist=true | approve | archive] deterministic question-id override. On compile, replaces the auto-derived id (`review:directive:<id>:v<version>:compile[:<topic-hash>]`). On approve/archive without `review_decision`, opts the action into emitting a follow-up QuestionEvent::Resolved (or DecisionResolved) with the supplied id — same fire-and-forget, bus-failure-warns semantics (legacy quiet path). On approve/archive WITH `review_decision`, switches to the wave-15 explicit-resolution bridge: validates the deterministic id (scope=directive, artifact=directive_id, version=current head, action ∈ {compile|approve|archive}) BEFORE mutating state; `review_decision=approved` runs the manager transition, `rejected`/`needs_changes` skip it. Absent → no resolution emit (legacy quiet)."
                },
                "review_decision": {
                    "type": "string",
                    "enum": ["approved", "rejected", "needs_changes"],
                    "description": "[approve | archive] (wave-15 explicit-resolution bridge) explicit decision attached to the supplied `review_question_id`. Required when `review_question_id` is supplied; absence with the id triggers a structured MISSING_PARAM error (we never guess). `approved` performs the manager transition (existing approve / archive semantics); `rejected` keeps the directive at its current status and emits Resolved/rejected; `needs_changes` keeps the directive in review/draft and surfaces a `next_step` hint with Resolved/needs_changes. NOT auto-approve and NOT a poll for a QuestionEvent::Resolved answer — the helper consumes only this caller-supplied input."
                },
                "review_actor": {
                    "type": "string",
                    "description": "[approve | archive review_decision=*] (wave-15) free-form identity of the resolver. Echoed onto the response payload (`review_actor`) so callers can correlate the decision with whoever made it; never used for authentication."
                },
                "review_note": {
                    "type": "string",
                    "description": "[approve | archive review_decision=*] (wave-15) free-form reason / next-step note. Echoed onto the response payload (`review_note`) and surfaced to downstream consumers as the human-readable resolution context."
                },
                "review_automation_policy": {
                    "type": "string",
                    "enum": ["manual", "suggest", "auto_safe"],
                    "description": "[approve | archive] (wave-18 / task 07 review automation policy v0) explicit autonomy knob for the resolution surface. ORTHOGONAL to `review_gate_policy` (which controls EMISSION). `manual` (default) keeps the existing wave-15 behaviour: caller-supplied `review_decision` is the only authority and the response is byte-identical with pre-wave-18 callers. `suggest` makes the handler compute a deterministic suggestion (`suggested_review_decision`) and surfaces it WITHOUT mutating the artifact. `auto_safe` may auto-promote to `approved` ONLY when ALL deterministic safety rules pass: producer ran in deterministic/dry-run mode, no file write OR file hash matches the supplied expected_file_sha256, no protected source/target, no unresolved conflicts, and the caller explicitly opted in via this knob. NEVER auto-rejects (refusing a draft is a human-only decision). NEVER calls an LLM. Caller-supplied `review_decision` ALWAYS wins (the policy never overrides explicit decisions). `archive` is intentionally NEVER auto-promoted under `auto_safe` (destructive transition) — the policy surfaces the suggestion and refuses to mutate. Response always carries `review_automation_policy` / `review_automation_status` / `suggested_review_decision` / `automation_reasons[]` when the knob was supplied."
                },
                "auto_approve_mode": {
                    "type": "string",
                    "enum": ["off", "sonnet_suggest"],
                    "description": "[approve | archive] (wave-21 / task 06 LLM auto-approve proposal v0) opt-in propose-only Sonnet-assisted review-action recommendation. ORTHOGONAL to the wave-18 / 07 `review_automation_policy` (deterministic safety inspector) AND the wave-20 / 08 `auto_answer_policy` (listener-side auto-answer). `off` (default) preserves pre-wave-21 byte-shape — no LLM call, no proposal block. `sonnet_suggest` asks Sonnet to PROPOSE a structured review decision (decision + confidence + evidence + non_goal_check + destructive_check + requires_human) and surfaces it under `llm_auto_approve_proposal*` on the response. Hard invariants in v0: (I1) proposal NEVER carries `decision=rejected` — `rejected` from the model is demoted to `needs_changes` with a warning; auto-rejection is a human-only decision. (I2) destructive actions (archive | supersede | remove) ALWAYS short-circuit to `destructive_blocked` regardless of model output — proposal value preserved for audit but `requires_human=true` and `applied=false` are pinned. (I3) `applied=false` is pinned on EVERY proposal regardless of confidence — v0 NEVER auto-applies; any future wave promoting proposals to authority MUST add a separate explicit caller-side opt-in flag. (I4) Sonnet unavailable surfaces `llm_unavailable` status with no fallback proposal — invariant against silent degradation to deterministic. (I5) `destructive_check` is ALWAYS sourced from the deterministic `is_destructive_review_action` outcome — never from the model. The proposal NEVER drives a DB transition or bus emission; caller still has to supply explicit `review_decision` to flip the artifact. The deterministic `review_automation_policy` (when also supplied) and the LLM proposal co-exist on the response — they are independent suggestions."
                },
                "expected_file_sha256": {
                    "type": "string",
                    "description": "[approve | archive review_automation_policy=auto_safe] (wave-18 / task 07) optional caller-supplied SHA-256 the deterministic safety inspector requires to match the on-disk artifact hash. Pure additive guard: when the artifact landed via the file-first writer the caller can capture `file_sha256` from the compile response and replay it here so an unexpected on-disk modification blocks `auto_safe`. Absent → strict-matching disabled (no file write attempted under approve/archive in v0; the rule still surfaces a passing audit row when omitted)."
                },
                "affected_pillars": {
                    "type": ["array", "string"],
                    "items": { "type": "string" },
                    "description": "[compile] pillar list passed as prompt context and stored in references_json"
                },
                "non_goals": {
                    "type": ["array", "string"],
                    "items": { "type": "string" },
                    "description": "[compile] explicit non-goals (prompt context + references_json)"
                },
                "acceptance": {
                    "type": ["array", "string"],
                    "items": { "type": "string" },
                    "description": "[compile] acceptance criteria (prompt context + references_json)"
                },
                "write_file": {
                    "type": "boolean",
                    "description": "[compile persist=true] (wave-14 file-first SSOT) write the compiled sexp to `<project_root>/.missiond/alignment/<topic>/intent-alignment.lisp` after the DB draft is committed. Default false. Requires `topic` and at least one project signal (project / absolute cwd / target_project). DB row is NEVER rolled back on file failure — response surfaces status=\"partial\" + file_write_error in that case."
                },
                "overwrite_file": {
                    "type": "boolean",
                    "description": "[compile persist=true write_file=true] allow replacing an existing intent-alignment.lisp at the target path (default false → atomic refusal)."
                },
                "topic": {
                    "type": "string",
                    "description": "[compile persist=true write_file=true] file-first SSOT topic segment used to derive `.missiond/alignment/<topic>/intent-alignment.lisp`. Sanitized (alnum / `_` / `-`); blank or pure-separator inputs collapse to `anonymous`."
                },
                "project": {
                    "type": "string",
                    "description": "[compile persist=true write_file=true] registered project id; primary signal for project-root resolution (intent-worker.lisp :: project-root-spawn-cwd)."
                },
                "cwd": {
                    "type": "string",
                    "description": "[compile persist=true write_file=true] absolute path inside a registered project; longest-prefix lookup in the registry. Relative cwd is REFUSED (no process-cwd fallback)."
                },
                "target_project": {
                    "type": "string",
                    "description": "[compile persist=true write_file=true] fallback registered project id used when neither `project` nor `cwd` is supplied."
                },
                "directive_id": {
                    "type": "string",
                    "description": "[get|approve|archive|version_chain] directive UUID"
                },
                "version": {
                    "type": "integer",
                    "description": "[get|approve|archive] directive version (omit on get → returns head)"
                },
                "status": {
                    "type": "string",
                    "enum": ["draft", "refining", "approved", "compiled", "archived"],
                    "description": "[list] optional status filter"
                },
                "limit": {
                    "type": "integer",
                    "description": "[list] cap result count (1-500, default 50)"
                }
            }
        }),
    )]
}
