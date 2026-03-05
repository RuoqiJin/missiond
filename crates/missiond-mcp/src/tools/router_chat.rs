use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Router Chat =====
        ToolDefinition::new(
            "mission_router_chat",
            "通过 AI 路由器与 Gemini 等模型多轮对话。传 task_id 自动持久化对话历史（同 Board 任务下连续对话）。不传 task_id 则无状态。",
            json!({
                "type": "object",
                "properties": {
                    "messages": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "role": { "type": "string", "enum": ["user", "assistant", "system"] },
                                "content": { "type": "string" }
                            },
                            "required": ["role", "content"]
                        },
                        "description": "本轮新消息。传 task_id 时历史自动加载，只需传新消息"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "关联 Board 任务 ID。传此参数自动加载/保存对话历史，实现跨会话连续对话"
                    },
                    "context": {
                        "type": "string",
                        "enum": ["board", "kb", "both", "none"],
                        "description": "自动注入上下文: board(任务板), kb(知识库), both, none（默认 none）"
                    },
                    "model": {
                        "type": "string",
                        "description": "模型（不传则用 Router 默认最新 Gemini，无需指定版本号）"
                    },
                    "max_tokens": {
                        "type": "integer",
                        "description": "最大响应 token 数（默认 16384）"
                    },
                    "search": {
                        "type": "boolean",
                        "description": "启用 Google 搜索增强（仅 Gemini，默认 false）"
                    },
                    "files": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "本地文件路径列表。文件内容自动读取并追加到最后一条 user 消息。限制: 项目目录内、单文件 ≤ 500KB、UTF-8"
                    },
                    "message": {
                        "type": "string",
                        "description": "单条 user 消息（便捷模式）。等价于 messages: [{role:'user', content:message}]。与 messages 二选一"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_router_chat_history",
            "查看与某 Board 任务关联的 Gemini 对话历史",
            json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "Board 任务 ID"
                    }
                },
                "required": ["task_id"]
            }),
        ),

        ToolDefinition::new(
            "mission_router_chat_list",
            "列出所有 Gemini 对话。显示 ID、关联任务、模型、消息数、总字符数、估算 token、日期。",
            json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "最大返回数（默认 50）"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_router_chat_delete",
            "删除 Gemini 对话（含所有消息）。支持按 conversation_id 或 task_id 定位。",
            json!({
                "type": "object",
                "properties": {
                    "conversation_id": {
                        "type": "string",
                        "description": "对话 ID（直接删除）"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "Board 任务 ID（删除该任务关联的所有 Gemini 对话）"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_router_chat_clear",
            "清空 Gemini 对话的消息历史（保留对话记录本身，重置消息数为 0）。下次 chat 时从空白开始。",
            json!({
                "type": "object",
                "properties": {
                    "conversation_id": {
                        "type": "string",
                        "description": "对话 ID"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "Board 任务 ID（清空该任务关联的所有 Gemini 对话消息）"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_router_chat_stats",
            "Gemini 对话统计：总对话数、总消息数、总字符/估算 token、按模型/按天分布。",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),


    ]
}
