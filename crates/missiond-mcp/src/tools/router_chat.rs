use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Router Chat (KEEP AS-IS) =====
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
                        "description": "本地文件路径列表（完整路径保留）。安全: 黑名单拦截敏感路径(.ssh/.env等)，其余均可。文本 ≤ 1MB（超出自动截断），二进制 ≤ 10MB。失败不中断，以占位符报错"
                    },
                    "message": {
                        "type": "string",
                        "description": "单条 user 消息（便捷模式）。等价于 messages: [{role:'user', content:message}]。与 messages 二选一"
                    },
                    "idle_timeout": {
                        "type": "integer",
                        "description": "CLI 模式空闲超时秒数（默认 120）。长 prompt 或复杂推理时可设为 300-600"
                    }
                }
            }),
        ),

        // ===== Router Chat Manage (merged: history + list + delete + clear + delete_message + restore + stats) =====
        ToolDefinition::new(
            "mission_router_chat_manage",
            "Gemini 对话管理。action: history(查看对话历史), list(列出所有对话), delete(删除对话), clear(清理消息), delete_message(删除单条), restore(恢复已删除), stats(统计)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["history", "list", "delete", "clear", "delete_message", "restore", "stats"],
                        "description": "history=查看历史, list=列出全部, delete=删除对话, clear=清理消息, delete_message=删单条, restore=恢复, stats=统计"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "Board 任务 ID (action=history/delete/clear 时可用)"
                    },
                    "conversation_id": {
                        "type": "string",
                        "description": "对话 ID (action=delete/clear/restore 时可用)"
                    },
                    "message_id": {
                        "type": "integer",
                        "description": "消息 ID (action=delete_message 时必填)"
                    },
                    "count": {
                        "type": "integer",
                        "description": "清理最后 N 条消息 (action=clear 时可选，默认 2=最后一轮问答，-1=全部)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "最大返回数 (action=list 时可选，默认 50)"
                    }
                },
                "required": ["action"]
            }),
        ),
    ]
}
