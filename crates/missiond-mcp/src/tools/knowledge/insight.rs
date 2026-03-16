use serde_json::json;
use crate::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_insight",
        "查看 MissionD 对用户的战略认知 — 开发轨迹、协作模式、反面模式、摩擦点。纯只读，从 KB strategic-state 渲染人类可读报告。",
        json!({
            "type": "object",
            "properties": {
                "section": {
                    "type": "string",
                    "enum": ["all", "profile", "trajectory", "patterns", "proposals", "friction"],
                    "description": "查看哪个维度（默认 all）"
                }
            }
        }),
    )]
}
