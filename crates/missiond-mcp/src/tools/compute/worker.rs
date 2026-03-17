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
        // ===== Unified Control Tree =====
        ToolDefinition::new(
            "mission_control",
            "统一调控闸口。按维度控制: global(全局), provider(LLM模型), domain(功能域), worker(后台进程), slot_role(工位角色)。\
             级联机制: 关 provider=sonnet 会自动暂停所有依赖 Sonnet 的 Worker(embedding/briefing/translation)。\
             Provider: gemini/sonnet/codex/opus。Domain: memory/flow/board/strategy。",
            json!({
                "type": "object",
                "properties": {
                    "target_type": {
                        "type": "string",
                        "enum": ["global", "provider", "domain", "worker", "slot_role"],
                        "description": "控制维度"
                    },
                    "target_name": {
                        "type": "string",
                        "description": "名称(global 时可省略)。provider: gemini/sonnet/codex/opus, domain: memory/flow/board/strategy, worker: embedding/briefing_worker 等, slot_role: ops/memory/deploy 等"
                    },
                    "action": {
                        "type": "string",
                        "enum": ["pause", "resume", "status"],
                        "description": "pause=关闸, resume=开闸, status=查看完整状态树"
                    }
                },
                "required": ["action"]
            }),
        ),
    ]
}
