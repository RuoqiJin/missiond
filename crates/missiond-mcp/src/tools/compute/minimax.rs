use serde_json::json;
use crate::ToolDefinition;

fn tool_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "text": {
                "type": "string",
                "description": "待处理的文本内容"
            },
            "task": {
                "type": "string",
                "enum": ["summarize", "translate", "custom"],
                "description": "任务类型: summarize=摘要, translate=翻译, custom=自定义 prompt"
            },
            "prompt": {
                "type": "string",
                "description": "自定义 prompt (task=custom 时必填)"
            },
            "targetLang": {
                "type": "string",
                "description": "目标语言 (task=translate 时使用，默认 'en')"
            },
            "maxChars": {
                "type": "integer",
                "description": "摘要目标字数 (task=summarize 时使用，默认 200)"
            }
        },
        "required": ["text", "task"]
    })
}

pub fn definitions() -> Vec<ToolDefinition> {
    let schema = tool_schema();
    vec![
        ToolDefinition::new(
            "mission_minimax_process",
            "[Deprecated → mission_sonnet_process] 调用 Sonnet 处理文本。支持摘要、翻译等任务。",
            schema.clone(),
        ),
        ToolDefinition::new(
            "mission_sonnet_process",
            "调用 Claude Sonnet 处理文本。支持摘要、翻译等任务。\
             直接 HTTP 调用，无 PTY 开销。",
            schema,
        ),
    ]
}
