use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Claude Code Tasks Monitoring =====
        ToolDefinition::new(
            "mission_cc_sessions",
            "列出 Claude Code 会话及其 Tasks 状态",
            json!({
                "type": "object",
                "properties": {
                    "projectPath": {
                        "type": "string",
                        "description": "筛选特定项目路径"
                    },
                    "activeOnly": {
                        "type": "boolean",
                        "description": "仅显示活跃会话 (默认 true)"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_cc_tasks",
            "获取指定会话的 Tasks 列表",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "会话 ID"
                    },
                    "projectPath": {
                        "type": "string",
                        "description": "项目路径 (返回该项目所有会话的 Tasks)"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_cc_overview",
            "获取所有 Claude Code 会话的 Tasks 概览统计",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_cc_in_progress",
            "获取所有正在进行中的任务 (跨会话)",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_cc_trigger_swarm",
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
