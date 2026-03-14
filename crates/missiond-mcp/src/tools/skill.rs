use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Skill Query (merged: skill_list + skill_search + skill_topics + skill_actions + skill_stats) =====
        ToolDefinition::new(
            "mission_skill_query",
            "Skill 知识库查询。action: list(列出全部 Skill), search(关键词搜索), topics(主题统计), actions(可执行动作), stats(执行统计)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "search", "topics", "actions", "stats"],
                        "description": "查询类型: list=列出全部, search=关键词搜索, topics=主题统计, actions=可执行动作, stats=执行统计"
                    },
                    "query": {
                        "type": "string",
                        "description": "搜索关键词 (action=search 时必填，如 deploy, auth, 部署)"
                    },
                    "skill": {
                        "type": "string",
                        "description": "按 Skill 名筛选 (action=actions/stats 时可选)"
                    }
                },
                "required": ["action"]
            }),
        ),

        // ===== Skill Context (merged: context_build + context_resolve) =====
        ToolDefinition::new(
            "mission_skill_context",
            "Skill 上下文构建。action: build(按关键词匹配 Skill 生成 Context 块), resolve(跨域递归聚合完整认知上下文，含 requires 依赖)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["build", "resolve"],
                        "description": "build=简单匹配生成 Context, resolve=递归解析依赖聚合完整上下文"
                    },
                    "query": {
                        "type": "string",
                        "description": "任务关键词/描述 (如 '部署 auth 服务到 GCP')"
                    },
                    "skill": {
                        "type": "string",
                        "description": "直接指定 skill name 跳过搜索 (action=resolve 时可选)"
                    },
                    "include_board": {
                        "type": "boolean",
                        "description": "是否包含 Board 相关任务 (action=resolve 时可选，默认 false)"
                    }
                },
                "required": ["action", "query"]
            }),
        ),

        // ===== Skill Mutate (merged: skill_upsert + skill_record + skill_render + skill_rollback) =====
        ToolDefinition::new(
            "mission_skill_mutate",
            "Skill 写操作。action: upsert(创建/更新章节), record(快速记录碎片), render(重新生成 .md 文件), rollback(回滚到历史版本)。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["upsert", "record", "render", "rollback"],
                        "description": "upsert=创建/更新章节, record=记录碎片, render=重建 .md, rollback=回滚版本"
                    },
                    "topic": {
                        "type": "string",
                        "description": "Skill 主题名 (如 missiond, deployment)"
                    },
                    "section_title": {
                        "type": "string",
                        "description": "章节标题 (action=upsert 时必填，如 '# API')"
                    },
                    "content": {
                        "type": "string",
                        "description": "内容 (action=upsert/record 时必填)"
                    },
                    "sort_order": {
                        "type": "integer",
                        "description": "章节排序 (action=upsert 时可选，默认追加末尾)"
                    },
                    "skill": {
                        "type": "string",
                        "description": "Skill 名称 (action=rollback 时必填)"
                    },
                    "version_id": {
                        "type": "integer",
                        "description": "版本 ID (action=rollback 时可选，不指定则列出版本)"
                    }
                },
                "required": ["action"]
            }),
        ),

        // ===== Skill Execution (KEEP AS-IS) =====
        ToolDefinition::new(
            "mission_skill_exec",
            "执行 Skill 中定义的 workflow。顺序执行 MCP 工具步骤，支持 dry_run 预览。",
            json!({
                "type": "object",
                "properties": {
                    "skill": {
                        "type": "string",
                        "description": "Skill 名称（如 backend-deploy）"
                    },
                    "action": {
                        "type": "string",
                        "description": "Action ID（对应 workflow block id）"
                    },
                    "dry_run": {
                        "type": "boolean",
                        "description": "预览模式，只返回步骤不执行（默认 false）"
                    },
                    "params": {
                        "type": "object",
                        "description": "运行时参数覆盖（注入为 ${key} 变量）"
                    }
                },
                "required": ["skill", "action"]
            }),
        ),
    ]
}
