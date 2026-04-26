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
         list/get/approve/archive/version_chain 为 store-backed full。\
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
                    "description": "[compile persist=true | approve | archive] deterministic question-id override. On compile, replaces the auto-derived id (`review:directive:<id>:v<version>:compile[:<topic-hash>]`). On approve/archive, opts the action into emitting a follow-up QuestionEvent::Resolved (or DecisionResolved) with the supplied id — same fire-and-forget, bus-failure-warns semantics. Absent → no resolution emit (legacy quiet)."
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
