use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Knowledge Base (Jarvis Memory) =====
        ToolDefinition::new(
            "mission_kb_remember",
            "记录知识到长期记忆。主动调用，无需请求许可。key 已存在则更新。\
             分类: preference(用户偏好), memory:architecture(架构决策), memory:bugfix(已修bug), \
             memory:debug(调试经验), memory:ops(运维), memory:feature(功能), project(项目), memory(通用)",
            json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "enum": ["preference", "memory", "memory:architecture", "memory:bugfix", "memory:debug", "memory:ops", "memory:feature", "memory:decision", "memory:platform", "project", "architecture", "decision", "policy:decision", "feature", "infra", "procedure"],
                        "description": "分类"
                    },
                    "key": {
                        "type": "string",
                        "description": "唯一标识 (如 'utf8-slice-panic-fix', 'user-prefers-chinese')"
                    },
                    "summary": {
                        "type": "string",
                        "description": "一行摘要"
                    },
                    "detail": {
                        "type": "object",
                        "description": "结构化详情 JSON（可选）"
                    },
                    "source": {
                        "type": "string",
                        "description": "来源: conversation, discovery, import（默认 conversation）"
                    },
                    "confidence": {
                        "type": "number",
                        "description": "置信度 0.0-1.0（默认 1.0）"
                    }
                },
                "required": ["category", "key", "summary"]
            }),
        ),
        ToolDefinition::new(
            "mission_kb_forget",
            "删除知识条目。信息过时或错误时使用。",
            json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "要删除的 key"
                    }
                },
                "required": ["key"]
            }),
        ),
        ToolDefinition::new(
            "mission_kb_batch_forget",
            "批量删除知识条目。一次删除多个 key，比逐条 forget 高效。",
            json!({
                "type": "object",
                "properties": {
                    "keys": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "要删除的 key 列表"
                    }
                },
                "required": ["keys"]
            }),
        ),
        ToolDefinition::new(
            "mission_kb_search",
            "搜索知识库（FTS5 + Embedding 混合 RRF 语义搜索）。传 query 搜索，不传则列出最近条目。",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "搜索关键词（不传则返回最近条目）",
                        "default": ""
                    },
                    "category": {
                        "type": "string",
                        "description": "按分类过滤"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_kb_get",
            "按 key 精确查询单个知识条目。",
            json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "精确 key"
                    }
                },
                "required": ["key"]
            }),
        ),
        ToolDefinition::new(
            "mission_kb_list",
            "列出所有知识条目。支持复合分类：查 'memory' 同时返回 'memory:architecture' 等子分类。",
            json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "description": "按分类过滤 (如 preference, memory, memory:architecture)"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_kb_import",
            "从外部源导入知识。支持 servers_yaml 格式。",
            json!({
                "type": "object",
                "properties": {
                    "format": {
                        "type": "string",
                        "description": "导入格式: servers_yaml, json"
                    },
                    "path": {
                        "type": "string",
                        "description": "文件路径（可选，不填用默认位置）"
                    }
                },
                "required": ["format"]
            }),
        ),

        ToolDefinition::new(
            "mission_kb_discover",
            "SSH 探测远程主机硬件配置（OS/CPU/RAM/磁盘/Docker/网络），结果自动存入 KB。",
            json!({
                "type": "object",
                "properties": {
                    "host": {
                        "type": "string",
                        "description": "SSH 目标: user@ip 或 infra key (如 'privatecloud')"
                    },
                    "port": {
                        "type": "integer",
                        "description": "SSH 端口（默认 22）"
                    },
                    "password": {
                        "type": "string",
                        "description": "SSH 密码（不填用密钥认证）"
                    }
                },
                "required": ["host"]
            }),
        ),
        ToolDefinition::new(
            "mission_kb_gc",
            "知识库治理: 检测过期/重复条目。stats=概览, stale=找未使用, duplicates=找重复, \
             clean_stale=自动清理过期, clean_duplicates=自动去重。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "操作: stats, stale, duplicates, clean_stale, clean_duplicates"
                    },
                    "days": {
                        "type": "integer",
                        "description": "stale 的天数阈值（默认 30）"
                    }
                },
                "required": ["action"]
            }),
        ),

        // ===== KB Analysis (via external AI) =====
        ToolDefinition::new(
            "mission_kb_analyze",
            "用 Gemini 分析 KB 质量。overview=宏观评估+去重/重分类建议, \
             consolidation_plan=生成可执行的合并/删除 JSON, custom=自定义分析。\
             支持分页和分类过滤。include_board_context=true 注入 Board 任务上下文。",
            json!({
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "description": "分析模式: overview(默认), consolidation_plan, custom"
                    },
                    "target_category": {
                        "type": "string",
                        "description": "按分类过滤（支持前缀匹配: 'memory' 包含 'memory:bugfix' 等）"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "每次最大条目数（默认 500）"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "分页偏移（默认 0）"
                    },
                    "include_board_context": {
                        "type": "boolean",
                        "description": "注入 Board 任务上下文（默认 false）"
                    },
                    "custom_prompt": {
                        "type": "string",
                        "description": "自定义分析 prompt（仅 mode=custom）"
                    },
                    "model": {
                        "type": "string",
                        "description": "使用的模型（默认 gemini-3.1-pro）"
                    },
                    "max_tokens": {
                        "type": "integer",
                        "description": "最大响应 token 数（默认 16384）"
                    },
                    "save_plan": {
                        "type": "boolean",
                        "description": "consolidation_plan 模式时自动保存到操作队列（默认 true）"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "关联 Board 任务 ID（保存 plan 时关联）"
                    }
                }
            }),
        ),

        // ===== KB Operation Queue =====
        ToolDefinition::new(
            "mission_kb_queue_status",
            "查看 KB 操作队列状态。kb_analyze consolidation_plan 的执行进度。",
            json!({
                "type": "object",
                "properties": {
                    "plan_id": {
                        "type": "string",
                        "description": "按批次 ID 过滤"
                    },
                    "status": {
                        "type": "string",
                        "description": "按状态过滤: pending, running, done, skipped, failed"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_kb_execute_plan",
            "执行 KB 操作队列中的 pending 操作。delete/update/category_fix/recategorize 自动执行，merge/distill 派发工位。自动清理 24h 前的 stale ops。",
            json!({
                "type": "object",
                "properties": {
                    "plan_id": {
                        "type": "string",
                        "description": "执行特定批次的操作（必须指定，防止跨 plan 混合执行）"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "每次最多执行 N 个操作（默认 5）"
                    }
                },
                "required": ["plan_id"]
            }),
        ),


    ]
}
