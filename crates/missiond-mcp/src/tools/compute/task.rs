use serde_json::json;
use crate::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Task Submit (merged: submit + ask) =====
        ToolDefinition::new(
            "mission_task_submit",
            "提交任务给专家 Agent。action: async(异步提交，立即返回 taskId), sync(同步询问，阻塞等待结果)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["async", "sync"],
                        "description": "async=异步提交(默认), sync=同步等待结果",
                        "default": "async"
                    },
                    "role": {
                        "type": "string",
                        "description": "专家角色 (如 secret, deploy, memory)"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "任务提示词 (action=async 时使用)"
                    },
                    "question": {
                        "type": "string",
                        "description": "问题 (action=sync 时使用)"
                    },
                    "slotId": {
                        "type": "string",
                        "description": "指定目标工位 ID（可选，精确分发到具体 slot，跳过 role 匹配）"
                    },
                    "timeoutMs": {
                        "type": "number",
                        "description": "超时毫秒数 (action=sync 时可选，默认 120000)"
                    }
                },
                "required": ["role"]
            }),
        ),

        // ===== Task Query (merged: status + task + task_ack + task_track) =====
        ToolDefinition::new(
            "mission_task_query",
            "任务状态查询。action: status(单任务状态), list(任务列表), ack(获取已完成通知), track(全链路追踪)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["status", "list", "ack", "track"],
                        "description": "status=查单任务, list=任务列表, ack=已完成通知, track=全链路追踪"
                    },
                    "taskId": {
                        "type": "string",
                        "description": "任务 ID (action=status/track 时必填)"
                    },
                    "status": {
                        "type": "string",
                        "description": "按状态过滤: queued/running/done/failed (action=list 时可选)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "最大返回数 (action=list 时可选，默认 20)"
                    },
                    "since": {
                        "type": "integer",
                        "description": "返回 finished_at > since 的任务，epoch 毫秒 (action=ack 时可选，不传返回最近 1 小时)"
                    }
                },
                "required": ["action"]
            }),
        ),

        // ===== Task Cancel (KEEP AS-IS) =====
        ToolDefinition::new(
            "mission_task_cancel",
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
    ]
}
