use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Knowledge Base (Jarvis Memory) =====
        ToolDefinition::new(
            "mission_kb_remember",
            "记录知识到长期记忆。主动调用，无需请求许可。key 已存在则更新。\
             按层级分类: architecture:summary=子系统架构总结(L2), \
             memory:bugfix/memory:debug/policy:decision=具体经验(L4), \
             preference=用户偏好, memory:architecture=架构决策, memory:ops=运维, \
             memory:feature=功能, project=项目, memory=通用",
            json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "enum": ["preference", "memory", "memory:architecture", "memory:bugfix", "memory:debug", "memory:ops", "memory:feature", "memory:decision", "memory:platform", "project", "architecture", "architecture:summary", "decision", "policy:decision", "feature", "infra", "procedure"],
                        "description": "分类"
                    },
                    "key": {
                        "type": "string",
                        "description": "唯一标识 (如 'utf8-slice-panic-fix', 'user-prefers-chinese')"
                    },
                    "summary": {
                        "type": "string",
                        "description": "结论性摘要（≤400字）。用于语义搜索和上下文注入"
                    },
                    "detail": {
                        "type": "object",
                        "description": "结构化详情 JSON（可选）。完整配置/命令/代码段/排查过程存此处，召回时随 summary 一起提供"
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
            "mission_kb_update",
            "部分更新 KB 条目。只传需要改的字段，无需重传全部内容。\
             改 category 不触发内容质检，改 summary/detail 才重算 embedding。",
            json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "要更新的条目 key"
                    },
                    "category": {
                        "type": "string",
                        "description": "新分类（可选）"
                    },
                    "summary": {
                        "type": "string",
                        "description": "新摘要（可选，≤400字）"
                    },
                    "detail": {
                        "type": "object",
                        "description": "新详情 JSON（可选）"
                    },
                    "confidence": {
                        "type": "number",
                        "description": "新置信度 0.0-1.0（可选）"
                    },
                    "linked_task_id": {
                        "type": "string",
                        "description": "关联 Board 任务 ID（可选，传空字符串清除）"
                    }
                },
                "required": ["key"]
            }),
        ),
        ToolDefinition::new(
            "mission_kb_search",
            "搜索知识库（FTS5 + Embedding 混合 RRF）。\
             理解子系统架构 → category=\"architecture:summary\" 获取基石描述(L2); \
             具体问题/经验 → 直接搜索碎片记忆(L4)。不传 query 列出最近条目。",
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


        // ===== Holographic Beacon (P4) =====
        ToolDefinition::new(
            "mission_beacon_list",
            "列出所有代码信标（feature beacon）。信标关联跨文件的代码节点，标注功能归属。",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_beacon_map",
            "获取信标的完整拓扑 L3：跨文件关联的代码节点（签名 + stub + 行号 + 注释）。\
             beacon 名称见 MODULE_CATALOG.md 导航表(L1)，如 orchestration/slot/knowledge。",
            json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "信标名称（如 'memory-extraction'）"
                    }
                },
                "required": ["name"]
            }),
        ),
        ToolDefinition::new(
            "mission_beacon_tag",
            "给代码符号打信标标签。底层修改源文件插入 `// @beacon: xxx`，由 P2 Pipeline 自动同步到 DB。",
            json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "文件的绝对路径"
                    },
                    "symbol": {
                        "type": "string",
                        "description": "目标函数/结构体名称"
                    },
                    "feature": {
                        "type": "string",
                        "description": "信标名称（如 'memory-extraction'）"
                    },
                    "annotation": {
                        "type": "string",
                        "description": "人工注释（可选，Why/注意事项）"
                    }
                },
                "required": ["file_path", "symbol", "feature"]
            }),
        ),
        ToolDefinition::new(
            "mission_beacon_annotate",
            "更新信标节点的人工注释（Why/注意事项/历史背景）。",
            json!({
                "type": "object",
                "properties": {
                    "beacon_name": {
                        "type": "string",
                        "description": "信标名称"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "文件路径（相对于 repo root）"
                    },
                    "symbol": {
                        "type": "string",
                        "description": "符号名称"
                    },
                    "annotation": {
                        "type": "string",
                        "description": "注释文本"
                    }
                },
                "required": ["beacon_name", "file_path", "symbol", "annotation"]
            }),
        ),

        // ===== Code Context (P3.5 Holographic Context Engine) =====
        ToolDefinition::new(
            "mission_code_search",
            "搜索代码结构 L3（AST 索引）。tree-sitter 提取的函数/结构体/Trait/Impl 签名和调用关系。\
             先用 kb_search(category=\"architecture:summary\") 理解子系统，再用本工具深入代码签名。找不到时退回 L2 确认架构边界。",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "搜索查询（函数名、结构体名、或自然语言描述）"
                    },
                    "repo": {
                        "type": "string",
                        "description": "限定仓库名（可选）"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "限定文件路径前缀（可选，如 'crates/missiond-daemon/src/'）"
                    },
                    "node_type": {
                        "type": "string",
                        "enum": ["function", "struct", "enum", "impl", "trait", "mod", "const", "type"],
                        "description": "限定节点类型（可选）"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "返回数量上限（默认 20）"
                    }
                },
                "required": ["query"]
            }),
        ),

    ]
}
