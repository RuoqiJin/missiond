use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_directive",
        "directive 表 manager — 6 actions (compile/list/get/approve/archive/version_chain)。\
         compile 当前为 dry-run（intent-layer :: directive-compiler actor 未落地），\
         persist=true 仅写 draft 行；list/get/approve/archive/version_chain 为 store-backed full。\
         Lisp 源: intent-memory.lisp :: module directive-layer :: plumbing directive-compilation \
         + intent-tools.lisp :: future-surface mission_directive + intent-flow.lisp :: \
         F-directive-plan-workflow-compile :: directive branch。",
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["compile", "list", "get", "approve", "archive", "version_chain"],
                    "description": "manager action — see Lisp future-surface mission_directive"
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
                    "description": "[compile] insert a draft row (default false → dry-run only)"
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
