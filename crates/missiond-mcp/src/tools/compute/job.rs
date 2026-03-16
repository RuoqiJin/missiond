use serde_json::json;
use crate::ToolDefinition;

pub(crate) fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_job_poll",
        "轮询异步 Job 状态。长耗时操作(如动态 Slot 创建)返回 job_id 后，用此工具查询进度。返回 status(running/completed/failed) + result/error。",
        json!({
            "type": "object",
            "properties": {
                "job_id": {
                    "type": "string",
                    "description": "Job ID (从异步工具调用返回)"
                },
                "action": {
                    "type": "string",
                    "enum": ["poll", "list", "cancel"],
                    "description": "操作: poll(查状态,默认), list(列出所有), cancel(取消)"
                }
            },
            "required": ["job_id"]
        }),
    )]
}
