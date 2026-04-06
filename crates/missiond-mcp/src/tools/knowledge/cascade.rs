use serde_json::json;
use crate::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== mission_universe_graph =====
        ToolDefinition::new(
            "mission_universe_graph",
            "查看全宇宙服务依赖图。解析 universe.intent.lisp 清单，返回所有服务及其跨服务依赖关系",
            json!({
                "type": "object",
                "properties": {
                    "manifestPath": {
                        "type": "string",
                        "description": "universe.intent.lisp 清单文件路径。默认 /Users/jinchen/Projects/universe.intent.lisp"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["text", "json"],
                        "default": "json",
                        "description": "输出格式：text 人类可读 / json 结构化"
                    }
                }
            }),
        ),

        // ===== mission_cascade_plan =====
        ToolDefinition::new(
            "mission_cascade_plan",
            "计算变更爆炸半径并生成级联修复计划（dry-run，不执行）。显示哪些下游服务会受影响及修复顺序",
            json!({
                "type": "object",
                "required": ["service"],
                "properties": {
                    "manifestPath": {
                        "type": "string",
                        "description": "universe.intent.lisp 清单文件路径。默认 /Users/jinchen/Projects/universe.intent.lisp"
                    },
                    "service": {
                        "type": "string",
                        "description": "发生变更的服务 ID（如 auth、router）"
                    },
                    "changed": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "变更的接口列表（如 type-signature-change、api-removal）"
                    }
                }
            }),
        ),

        // ===== mission_cascade_trigger =====
        ToolDefinition::new(
            "mission_cascade_trigger",
            "⚠️ This tool executes external commands (forge build, cargo check, mechanic repair). Use with caution. 触发级联修复执行。按拓扑序修复所有受影响的下游服务。支持 Git 2PC 隔离。可通过 CASCADE_TRIGGER_ENABLED=0 禁用",
            json!({
                "type": "object",
                "required": ["service"],
                "properties": {
                    "manifestPath": {
                        "type": "string",
                        "description": "universe.intent.lisp 清单文件路径。默认 /Users/jinchen/Projects/universe.intent.lisp"
                    },
                    "service": {
                        "type": "string",
                        "description": "发生变更的服务 ID"
                    },
                    "changed": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "变更的接口列表"
                    },
                    "maxCycles": {
                        "type": "integer",
                        "default": 3,
                        "description": "每个阶段最大修复尝试次数，超过则 hard-halt"
                    }
                }
            }),
        ),

        // ===== mission_cascade_lint =====
        ToolDefinition::new(
            "mission_cascade_lint",
            "验证宇宙完整性。检查：自依赖、Cargo.toml 未声明的跨服务依赖、孤立服务、循环依赖",
            json!({
                "type": "object",
                "properties": {
                    "manifestPath": {
                        "type": "string",
                        "description": "universe.intent.lisp 清单文件路径。默认 /Users/jinchen/Projects/universe.intent.lisp"
                    }
                }
            }),
        ),
    ]
}
