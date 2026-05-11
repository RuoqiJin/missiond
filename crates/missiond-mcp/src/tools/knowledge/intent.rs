use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_intent",
        "读取项目 intent.lisp 全景声明。调查/实现任务前 MUST 先调用此工具获取系统全景图,可节省 50% 搜索步骤。read: 返回完整 intent.lisp; section: 按顶级 S-EXP 块名过滤; summary: 返回 design-constraints + module-catalog 核心段; list: 列出所有可用项目及其 intent 路径; paths: 返回所有 intent 文件路径供并行 Read 加载",
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["read", "section", "summary", "list", "paths"],
                    "default": "summary",
                    "description": "read=完整文件, section=指定块, summary=核心段, list=列出可用项目, paths=返回所有intent文件路径(供并行Read加载)"
                },
                "project": {
                    "type": "string",
                    "description": "[read/section/summary] 项目 ID（如 missiond, jarvis-forge, pcea）。默认从 CWD 推断"
                },
                "section": {
                    "type": "string",
                    "description": "[section] 顶级 S-EXP 块名（如 design-constraints, module-catalog, data-model, mcp-tools）"
                }
            }
        }),
    )]
}
