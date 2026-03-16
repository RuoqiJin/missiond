use serde_json::json;
use crate::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== CC Query (merged: cc_sessions + cc_tasks + cc_overview + cc_in_progress) =====
        ToolDefinition::new(
            "mission_cc_query",
            "Claude Code 任务监控。action: sessions(列出会话), tasks(获取指定会话 Tasks), overview(全局概览统计), in_progress(进行中任务)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["sessions", "tasks", "overview", "in_progress"],
                        "description": "sessions=列出会话, tasks=指定会话 Tasks, overview=概览统计, in_progress=进行中任务"
                    },
                    "sessionId": {
                        "type": "string",
                        "description": "会话 ID (action=tasks 时可选)"
                    },
                    "projectPath": {
                        "type": "string",
                        "description": "项目路径 (action=sessions/tasks 时可选)"
                    },
                    "activeOnly": {
                        "type": "boolean",
                        "description": "仅显示活跃会话 (action=sessions 时可选，默认 true)"
                    }
                },
                "required": ["action"]
            }),
        ),

        // ===== CC Swarm (KEEP AS-IS) =====
        ToolDefinition::new(
            "mission_cc_swarm",
            "通过 PTY 触发 Claude Code 的 Swarm 模式并行执行任务",
            json!({
                "type": "object",
                "properties": {
                    "slotId": {
                        "type": "string",
                        "description": "PTY 工位 ID"
                    },
                    "tasks": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "要执行的任务列表"
                    },
                    "teammateCount": {
                        "type": "number",
                        "description": "并行 Agent 数量 (默认 3)"
                    },
                    "timeoutMs": {
                        "type": "number",
                        "description": "超时毫秒数 (默认 600000)"
                    }
                },
                "required": ["slotId", "tasks"]
            }),
        ),
    ]
}
