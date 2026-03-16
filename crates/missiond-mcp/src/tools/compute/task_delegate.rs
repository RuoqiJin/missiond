use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "mission_task_delegate".to_string(),
            description: "声明式任务委派：描述目标，Daemon 自主选 slot/创建/执行/回报。替代手动 PTY 编排。\n\n内部流程：intent→模板映射 → 查找/创建 slot → 构建 BoardTask(auto_execute) → 即时触发 dispatch。\n结果通过 TaskNotification hook 自动回报（下次用户说话时注入）。".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "objective": {
                        "type": "string",
                        "description": "任务目标描述（自然语言），将作为 slot 的执行 prompt"
                    },
                    "intent": {
                        "type": "string",
                        "enum": ["code", "ops", "research", "general"],
                        "description": "意图类型，决定 slot 模板: code→coder, ops→ops, research→researcher, general→coder。默认 general"
                    },
                    "cwd": {
                        "type": "string",
                        "description": "slot 工作目录(可选，默认 ~/Projects)"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "任务超时秒数(可选，默认 1800=30min，上限 7200=2h)"
                    },
                    "priority": {
                        "type": "string",
                        "enum": ["high", "medium", "low"],
                        "description": "优先级(默认 medium)"
                    },
                    "depends_on": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "前置任务 ID 列表(DAG 依赖)"
                    },
                    "context_hints": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "上下文关键词列表，用于预加载 KB/Skill 知识"
                    }
                },
                "required": ["objective"]
            }),
        },
    ]
}
