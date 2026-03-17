use serde_json::json;
use crate::ToolDefinition;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        // ===== Worker (merged: workers + worker_control) =====
        ToolDefinition::new(
            "mission_worker",
            "后台 Worker + LLM 闸口管理。action: list(列出状态+闸口), control(暂停/恢复)。\
             LLM 闸口 target: gemini/sonnet/codex — 关闸立即停止对应模型所有 token 消耗。",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "control"],
                        "description": "list=列出所有 Worker 状态+LLM 闸口, control=暂停/恢复"
                    },
                    "target": {
                        "type": "string",
                        "description": "Worker 名或 LLM 闸口 (action=control 时必填)。\
                         LLM 闸口: gemini, sonnet, codex。Worker: translation_worker, briefing_worker 等"
                    },
                    "control_action": {
                        "type": "string",
                        "enum": ["pause", "resume", "status"],
                        "description": "操作: pause/disable(关闸), resume/enable(开闸), status(查状态)"
                    }
                },
                "required": ["action"]
            }),
        ),
    ]
}
