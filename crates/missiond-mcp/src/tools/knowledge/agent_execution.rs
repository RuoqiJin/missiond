use crate::ToolDefinition;
use serde_json::{json, Value};

/// Build a single property descriptor `{"type": ..., "description": ...}` —
/// optionally with an `enum` constraint. Centralising construction here keeps
/// the schema-builder readable while sidestepping `json!` macro recursion
/// limits when the property bag grows past ~30 entries.
fn prop(ty: &str, description: &str) -> Value {
    json!({"type": ty, "description": description})
}

fn prop_enum(ty: &str, description: &str, variants: &[&str]) -> Value {
    json!({
        "type": ty,
        "enum": variants,
        "description": description,
    })
}

fn prop_array_of_strings(description: &str) -> Value {
    json!({
        "type": "array",
        "items": {"type": "string"},
        "description": description,
    })
}

pub fn definitions() -> Vec<ToolDefinition> {
    let mut properties = serde_json::Map::new();

    properties.insert(
        "action".into(),
        prop_enum(
            "string",
            "manager action — see Lisp helper agent-execution-coordination :: mcp-tool-design",
            &[
                "open", "list", "claim", "heartbeat", "release", "deviate", "decide", "issue",
                "complete", "status", "audit", "repair",
            ],
        ),
    );
    properties.insert(
        "project".into(),
        prop(
            "string",
            "[all] project id (registry-resolved root); defaults to CWD",
        ),
    );
    properties.insert(
        "target_project".into(),
        prop(
            "string",
            "[all] alias for `project`; if both supplied `project` wins. Persisted in companion log meta when present (intent-tools.lisp :: implemented-surface mission_execution :: :workstation-dispatch-record).",
        ),
    );
    properties.insert(
        "dispatch_strategy".into(),
        prop_enum(
            "string",
            "[open] workstation-dispatch-record strategy from intent-tools.lisp. Unknown / empty values normalise to `unknown`. Persisted in companion log meta and surfaced by status/list.",
            &[
                "resident-lisp",
                "fresh-code-alignment",
                "agent-team",
                "mixed",
                "prompt-fallback",
                "unknown",
            ],
        ),
    );
    properties.insert(
        "requested_cwd".into(),
        prop(
            "string",
            "[open] working directory the dispatcher used; metadata only, persisted into companion log meta when present.",
        ),
    );
    properties.insert(
        "execution_id".into(),
        prop(
            "string",
            "[all except list] companion log basename, e.g. `intent-memory-execution`",
        ),
    );
    properties.insert(
        "parent_design".into(),
        prop(
            "string",
            "[open|list filter] frozen design lisp this companion pairs with",
        ),
    );
    properties.insert(
        "scope".into(),
        prop(
            "string",
            "[open|claim] scope description (file/path/section); claim conflicts on overlap",
        ),
    );
    properties.insert(
        "owner".into(),
        prop(
            "string",
            "[open|issue] human/agent that owns the execution or issue",
        ),
    );
    properties.insert(
        "status".into(),
        prop("string", "[list filter] match meta :status substring"),
    );
    properties.insert(
        "scope_prefix".into(),
        prop(
            "string",
            "[list filter] only entries whose scope starts with this",
        ),
    );
    properties.insert(
        "limit".into(),
        prop("integer", "[list] cap result count (1-500, default 50)"),
    );
    properties.insert(
        "claim_id".into(),
        prop(
            "string",
            "[heartbeat|release] claim id returned by claim action",
        ),
    );
    properties.insert(
        "claimer_name".into(),
        prop(
            "string",
            "[claim|heartbeat|release] caller identity; release/heartbeat must match claim owner",
        ),
    );
    properties.insert(
        "phase".into(),
        prop(
            "string",
            "[claim|deviate|complete] phase name from phase-tracker",
        ),
    );
    properties.insert(
        "lease_secs".into(),
        prop(
            "integer",
            "[claim|heartbeat] lease window in seconds (60..86400, default 1800)",
        ),
    );
    properties.insert(
        "summary".into(),
        prop("string", "[release|complete] short prose of what was done"),
    );
    properties.insert(
        "lisp_said".into(),
        prop("string", "[deviate] verbatim quote from frozen design lisp"),
    );
    properties.insert(
        "actually_found".into(),
        prop(
            "string",
            "[deviate] what actually happened in code/runtime",
        ),
    );
    properties.insert(
        "reason".into(),
        prop("string", "[deviate] why the deviation was necessary"),
    );
    properties.insert(
        "approved_by".into(),
        prop(
            "string",
            "[deviate] auto / agent-consensus / user / commander",
        ),
    );
    properties.insert(
        "context".into(),
        prop(
            "string",
            "[decide] situation requiring a small in-flight decision",
        ),
    );
    properties.insert(
        "options".into(),
        prop("string", "[decide] alternatives considered (free-form text)"),
    );
    properties.insert(
        "chosen".into(),
        prop("string", "[decide] selected option"),
    );
    properties.insert(
        "rationale".into(),
        prop("string", "[decide] why the chosen option won"),
    );
    properties.insert(
        "decided_by".into(),
        prop("string", "[decide] author of the decision"),
    );
    properties.insert(
        "severity".into(),
        prop(
            "string",
            "[issue] low|medium|high|critical (default medium)",
        ),
    );
    properties.insert(
        "desc".into(),
        prop("string", "[issue] one-line description of the blocker/risk"),
    );
    properties.insert(
        "resolution_path".into(),
        prop("string", "[issue] how to resolve (free-form)"),
    );
    properties.insert(
        "agent_name".into(),
        prop("string", "[complete] agent that finished the phase"),
    );
    properties.insert(
        "deliverables".into(),
        prop("string", "[complete] artifacts produced (free-form text)"),
    );
    properties.insert(
        "verification".into(),
        prop(
            "string",
            "[complete] how completion was verified (tests, audit, etc.)",
        ),
    );
    properties.insert(
        "changed_files".into(),
        prop_array_of_strings(
            "[complete] files actually modified in the worktree by this completion (durability-plane evidence per intent-memory.lisp :: agent-execution-coordination :: completions). Persisted verbatim into the companion log; status/audit surface them for handoff visibility.",
        ),
    );
    properties.insert(
        "staged_files".into(),
        prop_array_of_strings(
            "[complete] files staged for the scoped commit (must be a subset of the active claim scope per scoped-commit-contract :: scope-rule). Audit emits `scoped-commit-violation` when any entry escapes the claim scope.",
        ),
    );
    properties.insert(
        "commit_hash".into(),
        prop(
            "string",
            "[complete] git commit hash that durably persists the completion. Required when commit_status=committed (audit emits `commit-status-without-hash` otherwise). Daemon never executes git itself — the writer agent runs the scoped commit and reports back.",
        ),
    );
    properties.insert(
        "commit_status".into(),
        prop_enum(
            "string",
            "[complete] scoped-commit handoff state per intent-memory.lisp :: completions :commit-status-values. `not-required` for read-only work; `pending` while staging; `committed` requires commit_hash; `blocked` requires commit_blocker; `skipped` for explicit policy waivers.",
            &["not-required", "pending", "committed", "blocked", "skipped"],
        ),
    );
    properties.insert(
        "commit_blocker".into(),
        prop(
            "string",
            "[complete] human-readable reason the scoped commit could not be made. Required when commit_status=blocked (audit emits `commit-status-blocked-without-blocker` otherwise) so the next agent can resume per scoped-commit-contract :: recovery-rule.",
        ),
    );
    properties.insert(
        "mode".into(),
        prop_enum(
            "string",
            "[repair] dry_run reports planned actions; apply mutates the file",
            &["dry_run", "apply"],
        ),
    );

    let schema = json!({
        "type": "object",
        "required": ["action"],
        "properties": Value::Object(properties),
    });

    vec![ToolDefinition::new(
        "mission_execution",
        "agent-execution-coordination v0.5.x manager — 12 actions over .missiond/v2/<id>.lisp \
         companion logs (open / list / claim / heartbeat / release / deviate / decide / issue / \
         complete / status / audit / repair). ID 分配由 manager 原子化 (id-counters slot), \
         claim 带 lease + heartbeat,deviation/decision/issue/completion 自动编号 D/DC/I/COMP\
         ;status 给 dashboard,audit 检 paren / 单调 ID / 重叠 claim / stale claim / scoped commit \
         handoff (commit-status-without-hash, commit-status-blocked-without-blocker, scoped-commit-violation),repair 仅修\
         结构 (dry_run|apply)。Lisp 源: intent-memory.lisp :: agent-execution-coordination + \
         intent-worker.lisp :: agent-execution-manager-interface + intent-flow.lisp :: \
         F-execution-log-governance + F-scoped-commit-handoff。注意:event-bus ExecutionEvent::* 暂未发射,等域扩展落地。",
        schema,
    )]
}
