use crate::ToolDefinition;
use serde_json::json;

pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition::new(
        "mission_execution",
        "agent-execution-coordination v0.5.x manager — 12 actions over .missiond/v2/<id>.lisp \
         companion logs (open / list / claim / heartbeat / release / deviate / decide / issue / \
         complete / status / audit / repair). ID 分配由 manager 原子化 (id-counters slot), \
         claim 带 lease + heartbeat,deviation/decision/issue/completion 自动编号 D/DC/I/COMP\
         ;status 给 dashboard,audit 检 paren / 单调 ID / 重叠 claim / stale claim,repair 仅修\
         结构 (dry_run|apply)。Lisp 源: intent-memory.lisp :: agent-execution-coordination + \
         intent-worker.lisp :: agent-execution-manager-interface + intent-flow.lisp :: \
         F-execution-log-governance。注意:event-bus ExecutionEvent::* 暂未发射,等域扩展落地。",
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "open", "list", "claim", "heartbeat", "release",
                        "deviate", "decide", "issue", "complete",
                        "status", "audit", "repair"
                    ],
                    "description": "manager action — see Lisp helper agent-execution-coordination :: mcp-tool-design"
                },
                "project": {
                    "type": "string",
                    "description": "[all] project id (registry-resolved root); defaults to CWD"
                },
                "execution_id": {
                    "type": "string",
                    "description": "[all except list] companion log basename, e.g. `intent-memory-execution`"
                },
                "parent_design": {
                    "type": "string",
                    "description": "[open|list filter] frozen design lisp this companion pairs with"
                },
                "scope": {
                    "type": "string",
                    "description": "[open|claim] scope description (file/path/section); claim conflicts on overlap"
                },
                "owner": {
                    "type": "string",
                    "description": "[open] human/agent that owns the execution"
                },
                "status": {
                    "type": "string",
                    "description": "[list filter] match meta :status substring"
                },
                "scope_prefix": {
                    "type": "string",
                    "description": "[list filter] only entries whose scope starts with this"
                },
                "limit": {
                    "type": "integer",
                    "description": "[list] cap result count (1-500, default 50)"
                },
                "claim_id": {
                    "type": "string",
                    "description": "[heartbeat|release] claim id returned by claim action"
                },
                "claimer_name": {
                    "type": "string",
                    "description": "[claim|heartbeat|release] caller identity; release/heartbeat must match claim owner"
                },
                "phase": {
                    "type": "string",
                    "description": "[claim|deviate|complete] phase name from phase-tracker"
                },
                "lease_secs": {
                    "type": "integer",
                    "description": "[claim|heartbeat] lease window in seconds (60..86400, default 1800)"
                },
                "summary": {
                    "type": "string",
                    "description": "[release|complete] short prose of what was done"
                },
                "lisp_said": {
                    "type": "string",
                    "description": "[deviate] verbatim quote from frozen design lisp"
                },
                "actually_found": {
                    "type": "string",
                    "description": "[deviate] what actually happened in code/runtime"
                },
                "reason": {
                    "type": "string",
                    "description": "[deviate] why the deviation was necessary"
                },
                "approved_by": {
                    "type": "string",
                    "description": "[deviate] auto / agent-consensus / user / commander"
                },
                "context": {
                    "type": "string",
                    "description": "[decide] situation requiring a small in-flight decision"
                },
                "options": {
                    "type": "string",
                    "description": "[decide] alternatives considered (free-form text)"
                },
                "chosen": {
                    "type": "string",
                    "description": "[decide] selected option"
                },
                "rationale": {
                    "type": "string",
                    "description": "[decide] why the chosen option won"
                },
                "decided_by": {
                    "type": "string",
                    "description": "[decide] author of the decision"
                },
                "severity": {
                    "type": "string",
                    "description": "[issue] low|medium|high|critical (default medium)"
                },
                "desc": {
                    "type": "string",
                    "description": "[issue] one-line description of the blocker/risk"
                },
                "resolution_path": {
                    "type": "string",
                    "description": "[issue] how to resolve (free-form)"
                },
                "owner": {
                    "type": "string",
                    "description": "[issue] who owns the issue (overrides nothing for open)"
                },
                "agent_name": {
                    "type": "string",
                    "description": "[complete] agent that finished the phase"
                },
                "deliverables": {
                    "type": "string",
                    "description": "[complete] artifacts produced (free-form text)"
                },
                "verification": {
                    "type": "string",
                    "description": "[complete] how completion was verified (tests, audit, etc.)"
                },
                "mode": {
                    "type": "string",
                    "enum": ["dry_run", "apply"],
                    "description": "[repair] dry_run reports planned actions; apply mutates the file"
                }
            }
        }),
    )]
}
