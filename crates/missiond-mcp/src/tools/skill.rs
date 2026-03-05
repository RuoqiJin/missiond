use serde_json::json;
use super::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Skill Knowledge Hub =====
        ToolDefinition::new(
            "mission_skill_list",
            "列出所有已索引的 Skill（知识库条目）。返回 name、description、aka、路径。",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_skill_search",
            "按关键词搜索 Skill。支持 name/aka/description 模糊匹配。返回匹配的 Skill 元数据。",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "搜索关键词 (如 deploy, auth, 部署)"
                    }
                },
                "required": ["query"]
            }),
        ),
        ToolDefinition::new(
            "mission_context_build",
            "根据任务关键词自动匹配相关 Skill 并生成 [Context] 块。用于 Agent 派任务前自动注入上下文。",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "任务关键词 (如 '部署 auth 服务')"
                    }
                },
                "required": ["query"]
            }),
        ),

        // ===== Skill Engine (CQRS write tools) =====
        ToolDefinition::new(
            "mission_skill_upsert",
            "创建或更新 Skill 的某个章节。写入 DB 后自动生成 SKILL.md 文件。",
            json!({
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "Skill 主题名 (如 missiond, deployment)"
                    },
                    "section_title": {
                        "type": "string",
                        "description": "章节标题 (如 '# API', '## 配置')"
                    },
                    "content": {
                        "type": "string",
                        "description": "章节的 Markdown 内容"
                    },
                    "sort_order": {
                        "type": "integer",
                        "description": "章节排序（默认追加到末尾）"
                    }
                },
                "required": ["topic", "section_title", "content"]
            }),
        ),
        ToolDefinition::new(
            "mission_skill_record",
            "快速记录一条知识碎片到指定 Skill。低认知负担，积累后可用 optimize 合并整理。",
            json!({
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "Skill 主题名"
                    },
                    "content": {
                        "type": "string",
                        "description": "碎片内容"
                    }
                },
                "required": ["topic", "content"]
            }),
        ),
        ToolDefinition::new(
            "mission_skill_render",
            "从 DB 重新生成 SKILL.md 文件。可指定 topic 单个渲染或全量重建。",
            json!({
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "主题名（空=全部重建）"
                    }
                }
            }),
        ),
        ToolDefinition::new(
            "mission_skill_topics",
            "列出所有 Skill 主题及统计信息（命中次数、碎片数、行数）。",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "mission_context_resolve",
            "跨域上下文聚合。根据任务描述自动匹配 Skill，递归解析 requires 依赖（skills/infra/kb），一次性返回完整认知上下文。替代 mission_context_build 用于复杂任务。",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "任务描述 (如 '部署 auth 服务到 GCP')"
                    },
                    "skill": {
                        "type": "string",
                        "description": "可选，直接指定 skill name 跳过搜索"
                    },
                    "include_board": {
                        "type": "boolean",
                        "description": "是否包含 Board 相关任务（默认 false）"
                    }
                },
                "required": ["query"]
            }),
        ),

        // ===== Skill Execution (Phase 3) =====
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
        ToolDefinition::new(
            "mission_skill_actions",
            "列出可执行的 Skill Actions。可按 skill 名筛选。",
            json!({
                "type": "object",
                "properties": {
                    "skill": {
                        "type": "string",
                        "description": "按 Skill 名筛选（空=列出全部）"
                    }
                }
            }),
        ),

        // ===== Skill Execution Stats (Phase 4) =====
        ToolDefinition::new(
            "mission_skill_stats",
            "查看 Skill workflow 执行统计（成功率、平均耗时）。",
            json!({
                "type": "object",
                "properties": {
                    "skill": {
                        "type": "string",
                        "description": "Skill 名称（空=全部 skill 汇总）"
                    }
                }
            }),
        ),

        // ===== Skill Version Rollback (Phase 4) =====
        ToolDefinition::new(
            "mission_skill_rollback",
            "回滚 Skill 到历史版本。不指定 version_id 则列出可用版本。",
            json!({
                "type": "object",
                "properties": {
                    "skill": {
                        "type": "string",
                        "description": "Skill 名称（如 backend-deploy）"
                    },
                    "version_id": {
                        "type": "integer",
                        "description": "版本 ID（不指定则列出最近版本）"
                    }
                },
                "required": ["skill"]
            }),
        ),


    ]
}
