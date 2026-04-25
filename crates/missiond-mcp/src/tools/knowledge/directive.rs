use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_directive",
        "directive 表 manager — 6 actions (compile/list/get/approve/archive/version_chain)。\
         compile 是 directive-compiler actor v0：默认 compiler_mode=\"dry_run\" 不调 LLM；\
         compiler_mode=\"sonnet\" 走 SonnetGateway interactive 通道把 utterance 编译成可 review 的 \
         directive sexp；persist=true 仅写 DirectiveStatus::Draft，等待人工 approve。\
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
                    "description": "[compile] free-form note about the review gate (recorded in references_json)"
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
