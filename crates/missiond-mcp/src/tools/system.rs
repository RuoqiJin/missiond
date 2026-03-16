use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "mission_sys_logs",
            "读取 MissionD daemon 运行日志尾部。用于 Jarvis 自我诊断底层错误、IPC 超时等问题。只读 daemon 自身日志。",
            json!({
                "type": "object",
                "properties": {
                    "lines": {
                        "type": "integer",
                        "description": "返回尾部行数 (默认 50, 最大 500)"
                    },
                    "level": {
                        "type": "string",
                        "enum": ["all", "warn", "error"],
                        "description": "日志级别过滤 (默认 all)"
                    },
                    "grep": {
                        "type": "string",
                        "description": "关键词过滤 (大小写不敏感)"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_sys_config",
            "结构化配置管理。安全地读取/修改 MissionD 配置文件 (slots.yaml, llm.yaml, permissions.yaml)，避免直接 sed YAML。patch 操作自动触发热重载。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["get", "patch", "list"],
                        "description": "操作类型: get(读取配置为 JSON), patch(修改配置), list(列出可操作文件)。默认 get"
                    },
                    "file": {
                        "type": "string",
                        "enum": ["slots.yaml", "llm.yaml", "permissions.yaml"],
                        "description": "[get/patch] 配置文件名"
                    },
                    "path": {
                        "type": "string",
                        "description": "[patch] JSON Pointer 路径 (如 /slots/0/description)"
                    },
                    "value": {
                        "description": "[patch] 新值 (任意 JSON 类型)"
                    }
                }
            }),
        ),
    ]
}
