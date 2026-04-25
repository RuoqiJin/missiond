use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_capability_usage",
        "tool/flow 调用热度监控 — 5 actions (snapshot/report/candidates/mark/ack)。\
         从 conversation_tool_calls + board_tasks.flow_template 派生使用度,与 MCP 注册表 + \
         flows YAML 注册表对账,产出 active/quiet/stale/never-used/shadowed-by-better-capability\
         /merge-candidate/protected 七类候选。**只产出证据,不删 tool/flow,不改 registry**。\
         Lisp 源: intent-memory.lisp :: capability-usage-read-model + intent-flow.lisp :: \
         F-capability-usage-monitoring + intent-intent-layer.lisp :: capability-evolution-governance \
         + intent-tools.lisp :: future-surface mission_capability_usage。\
         注意: ObservabilityEvent::CapabilityUsageSnapshot/CapabilityStaleCandidate 暂未发射(planned), \
         daemon_state 是 i64-only,mark/ack 写 .missiond/v2/capability-usage-review.json sidecar。",
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["snapshot", "report", "candidates", "mark", "ack"],
                    "description": "snapshot=纯快照; report=按分类聚合 + narrative; candidates=只列非 active; mark=记录人工决定; ack=对接已存在的 follow-up"
                },
                "project": {
                    "type": "string",
                    "description": "[all] project id; defaults to CWD"
                },
                "window": {
                    "type": "string",
                    "enum": ["7d", "30d", "90d", "all"],
                    "description": "[snapshot|report|candidates] 主分类窗口,默认 30d"
                },
                "scope": {
                    "type": "string",
                    "enum": ["tool", "flow", "both"],
                    "description": "[snapshot|report|candidates] 默认 both"
                },
                "include_protected": {
                    "type": "boolean",
                    "description": "[candidates] 是否把 protected ids 一并列出 (默认 false)"
                },
                "candidate_id": {
                    "type": "string",
                    "description": "[mark|ack] capability id (tool name 或 flow id)"
                },
                "kind": {
                    "type": "string",
                    "enum": ["tool", "flow"],
                    "description": "[mark] candidate 类型 (默认 tool)"
                },
                "decision": {
                    "type": "string",
                    "enum": ["keep", "monitor", "merge", "deprecate", "remove-after-compat-window", "review"],
                    "description": "[mark] 治理决定 — protected 项只能取 keep/monitor/review"
                },
                "decided_by": {
                    "type": "string",
                    "description": "[mark] 决定人 (commander / agent name)"
                },
                "rationale": {
                    "type": "string",
                    "description": "[mark] 自由文本解释,鼓励引用证据"
                },
                "ack_by": {
                    "type": "string",
                    "description": "[ack] 确认人"
                },
                "follow_up_ref": {
                    "type": "string",
                    "description": "[ack] 必填 — PLAN.lisp / mission_execution id / board task id"
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "[mark] 不写 sidecar,只回 would_mark 预览 (默认 false)"
                }
            }
        }),
    )]
}
