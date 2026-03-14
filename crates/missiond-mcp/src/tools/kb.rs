use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== 1. mission_kb_query — merges: kb_search + kb_get + kb_list =====
        ToolDefinition::new(
            "mission_kb_query",
            "知识库查询（统一入口）。\
             action=search: 搜索知识库(FTS5+Embedding 混合 RRF)，理解子系统架构 → category=\"architecture:summary\" 获取基石描述(L2)，具体问题/经验 → 直接搜索碎片记忆(L4)。不传 query 列出最近条目。\
             action=get: 按 key 精确查询单个知识条目。\
             action=list: 列出所有知识条目，支持复合分类（查 'memory' 同时返回 'memory:architecture' 等子分类）。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["search", "get", "list"],
                        "description": "查询动作: search=语义搜索(默认), get=按key精确查询, list=列出条目",
                        "default": "search"
                    },
                    // --- search params ---
                    "query": {
                        "type": "string",
                        "description": "[search] 搜索关键词(不传则返回最近条目)"
                    },
                    "category": {
                        "type": "string",
                        "description": "[search/list] 按分类过滤 (如 preference, memory, memory:architecture)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "[search] 返回条数上限(默认 10，最大 50)",
                        "default": 10
                    },
                    "offset": {
                        "type": "integer",
                        "description": "[search] 分页偏移(默认 0)。前 N 条不够时可 offset=10 翻页",
                        "default": 0
                    },
                    "search_mode": {
                        "type": "string",
                        "enum": ["exact", "explore"],
                        "description": "[search] exact=精确查找(跳过 MMR 多样性,按纯相关性排序); explore=探索(默认,含 MMR 多样性重排)",
                        "default": "explore"
                    },
                    // --- get params ---
                    "key": {
                        "type": "string",
                        "description": "[get] 精确 key"
                    }
                }
            }),
        ),

        // ===== 2. mission_kb_remember — KEEP AS-IS =====
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

        // ===== 3. mission_kb_mutate — merges: kb_forget + kb_batch_forget + kb_update + kb_import =====
        ToolDefinition::new(
            "mission_kb_mutate",
            "知识库写操作（统一入口）。\
             action=forget: 删除知识条目，支持单个 key 或批量 keys。\
             action=update: 部分更新 KB 条目，只传需要改的字段，改 category 不触发内容质检，改 summary/detail 才重算 embedding。\
             action=import: 从外部源导入知识，支持 servers_yaml/json 格式。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["forget", "update", "import"],
                        "description": "写操作动作: forget=删除, update=部分更新, import=导入"
                    },
                    // --- forget params ---
                    "key": {
                        "type": "string",
                        "description": "[forget/update] 要操作的 key（forget 单条删除或 update 时必填）"
                    },
                    "keys": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "[forget] 批量删除的 key 列表（与 key 二选一，keys 优先）"
                    },
                    // --- update params ---
                    "category": {
                        "type": "string",
                        "description": "[update] 新分类（可选）"
                    },
                    "summary": {
                        "type": "string",
                        "description": "[update] 新摘要（可选，≤400字）"
                    },
                    "detail": {
                        "type": "object",
                        "description": "[update] 新详情 JSON（可选）"
                    },
                    "confidence": {
                        "type": "number",
                        "description": "[update] 新置信度 0.0-1.0（可选）"
                    },
                    "linked_task_id": {
                        "type": "string",
                        "description": "[update] 关联 Board 任务 ID（可选，传空字符串清除）"
                    },
                    // --- import params ---
                    "format": {
                        "type": "string",
                        "description": "[import] 导入格式: servers_yaml, json"
                    },
                    "path": {
                        "type": "string",
                        "description": "[import] 文件路径（可选，不填用默认位置）"
                    }
                },
                "required": ["action"]
            }),
        ),

        // ===== 4. mission_kb_ops — merges: kb_gc + kb_analyze + kb_discover + kb_queue_status + kb_execute_plan =====
        ToolDefinition::new(
            "mission_kb_ops",
            "知识库运维操作（统一入口）。\
             action=gc: 知识库治理，检测过期/重复条目(stats/stale/duplicates/clean_stale/clean_duplicates)。\
             action=analyze: 用 Gemini 分析 KB 质量(overview/consolidation_plan/custom)。\
             action=discover: SSH 探测远程主机硬件配置，结果自动存入 KB。\
             action=queue_status: 查看 KB 操作队列状态。\
             action=execute_plan: 执行队列中的 pending 操作。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["gc", "analyze", "discover", "queue_status", "execute_plan"],
                        "description": "运维动作: gc=治理, analyze=AI分析, discover=主机探测, queue_status=队列状态, execute_plan=执行计划"
                    },
                    // --- gc params ---
                    "gc_action": {
                        "type": "string",
                        "description": "[gc] 子操作: stats, stale, duplicates, clean_stale, clean_duplicates"
                    },
                    "days": {
                        "type": "integer",
                        "description": "[gc] stale 的天数阈值（默认 30）"
                    },
                    // --- analyze params ---
                    "mode": {
                        "type": "string",
                        "description": "[analyze] 分析模式: overview(默认), consolidation_plan, custom"
                    },
                    "target_category": {
                        "type": "string",
                        "description": "[analyze] 按分类过滤（支持前缀匹配: 'memory' 包含 'memory:bugfix' 等）"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "[analyze/execute_plan] 每次最大条目数/操作数（analyze 默认 500, execute_plan 默认 5）"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "[analyze] 分页偏移（默认 0）"
                    },
                    "include_board_context": {
                        "type": "boolean",
                        "description": "[analyze] 注入 Board 任务上下文（默认 false）"
                    },
                    "custom_prompt": {
                        "type": "string",
                        "description": "[analyze] 自定义分析 prompt（仅 mode=custom）"
                    },
                    "model": {
                        "type": "string",
                        "description": "[analyze] 使用的模型（默认 gemini-3.1-pro）"
                    },
                    "max_tokens": {
                        "type": "integer",
                        "description": "[analyze] 最大响应 token 数（默认 16384）"
                    },
                    "save_plan": {
                        "type": "boolean",
                        "description": "[analyze] consolidation_plan 模式时自动保存到操作队列（默认 true）"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "[analyze] 关联 Board 任务 ID（保存 plan 时关联）"
                    },
                    // --- discover params ---
                    "host": {
                        "type": "string",
                        "description": "[discover] SSH 目标: user@ip 或 infra key (如 'privatecloud')"
                    },
                    "port": {
                        "type": "integer",
                        "description": "[discover] SSH 端口（默认 22）"
                    },
                    "password": {
                        "type": "string",
                        "description": "[discover] SSH 密码（不填用密钥认证）"
                    },
                    // --- queue_status params ---
                    "plan_id": {
                        "type": "string",
                        "description": "[queue_status/execute_plan] 按批次 ID 过滤/执行特定批次"
                    },
                    "status": {
                        "type": "string",
                        "description": "[queue_status] 按状态过滤: pending, running, done, skipped, failed"
                    }
                },
                "required": ["action"]
            }),
        ),

        // ===== 5. mission_beacon — merges: beacon_list + beacon_map + beacon_tag + beacon_annotate =====
        ToolDefinition::new(
            "mission_beacon",
            "代码信标操作（统一入口）。信标关联跨文件的代码节点，标注功能归属。\
             action=list: 列出所有代码信标。\
             action=map: 获取信标的完整拓扑 L3（跨文件关联的代码节点：签名+stub+行号+注释），名称见 MODULE_CATALOG.md 导航表(L1)。\
             action=upsert: 给代码符号打/更新信标标签，底层修改源文件插入 `// @beacon: xxx`，由 P2 Pipeline 自动同步到 DB。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "map", "upsert"],
                        "description": "信标动作: list=列出所有, map=获取拓扑, upsert=创建/更新标签与注释"
                    },
                    // --- map params ---
                    "name": {
                        "type": "string",
                        "description": "[map/upsert] 信标名称（如 'memory-extraction'）。map 时必填"
                    },
                    // --- upsert params (covers tag + annotate) ---
                    "file_path": {
                        "type": "string",
                        "description": "[upsert] 文件路径（绝对路径或相对于 repo root）"
                    },
                    "symbol": {
                        "type": "string",
                        "description": "[upsert] 目标函数/结构体名称"
                    },
                    "feature": {
                        "type": "string",
                        "description": "[upsert] 信标名称（如 'memory-extraction'）。新建标签时必填"
                    },
                    "annotation": {
                        "type": "string",
                        "description": "[upsert] 人工注释（Why/注意事项/历史背景，可选）"
                    }
                }
            }),
        ),

        // ===== 6. mission_code_search — KEEP AS-IS =====
        ToolDefinition::new(
            "mission_code_search",
            "搜索代码结构 L3（AST 索引）。tree-sitter 提取的函数/结构体/Trait/Impl 签名和调用关系。\
             先用 kb_query(action=search, category=\"architecture:summary\") 理解子系统，再用本工具深入代码签名。找不到时退回 L2 确认架构边界。",
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
