use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Task Operations =====
        ToolDefinition::new(
            "mission_submit",
            "异步提交任务给专家 Agent",
            json!({
                "type": "object",
                "properties": {
                    "role": {
                        "type": "string",
                        "description": "专家角色 (如 secret, deploy, memory)"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "任务提示词"
                    },
                    "slotId": {
                        "type": "string",
                        "description": "指定目标工位 ID（可选，精确分发到具体 slot，跳过 role 匹配）"
                    }
                },
                "required": ["role", "prompt"]
            }),
        ),
        ToolDefinition::new(
            "mission_ask",
            "同步询问专家（提交 + 等待结果）",
            json!({
                "type": "object",
                "properties": {
                    "role": {
                        "type": "string",
                        "description": "专家角色"
                    },
                    "question": {
                        "type": "string",
                        "description": "问题"
                    },
                    "timeoutMs": {
                        "type": "number",
                        "description": "超时毫秒数 (默认 120000)"
                    }
                },
                "required": ["role", "question"]
            }),
        ),
        ToolDefinition::new(
            "mission_status",
            "查询任务状态",
            json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "string",
                        "description": "任务 ID"
                    }
                },
                "required": ["taskId"]
            }),
        ),
        ToolDefinition::new(
            "mission_cancel",
            "取消任务",
            json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "string",
                        "description": "任务 ID"
                    }
                },
                "required": ["taskId"]
            }),
        ),
        ToolDefinition::new(
            "mission_task",
            "查询 submit task 列表。显示 ID、角色、工位、状态、结果。可按状态过滤。",
            json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "description": "按状态过滤: queued, running, done, failed（不传返回最近 20 条）"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "最大返回数（默认 20）"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_task_ack",
            "获取已完成的 submit task 通知。传 since（epoch毫秒）返回增量结果，各 session 独立 watermark 互不干扰。供 UserPromptSubmit hook 调用。",
            json!({
                "type": "object",
                "properties": {
                    "since": {
                        "type": "integer",
                        "description": "返回 finished_at > since 的任务（epoch 毫秒）。不传返回最近 1 小时。"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_task_track",
            "全链路追踪 submit task。一个调用返回：任务状态、工位状态、PTY 进度、最后响应。替代 mission_task + pty_status + pty_screen 组合查询。",
            json!({
                "type": "object",
                "properties": {
                    "taskId": {
                        "type": "string",
                        "description": "任务 ID"
                    }
                },
                "required": ["taskId"]
            }),
        ),

    ]
}
