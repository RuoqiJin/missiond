//! MCP Tool definitions
//!
//! This module defines all available MCP tools and their schemas.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Tool definition following MCP schema
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    /// Tool name (e.g., "mission_submit")
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// JSON Schema for input parameters
    pub input_schema: Value,
}

impl ToolDefinition {
    /// Create a new tool definition
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        ToolDefinition {
            name: name.into(),
            description: description.into(),
            input_schema,
        }
    }
}

/// Permission rule for role/slot
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionRule {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_allow: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_confirm: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deny: Option<Vec<String>>,
}

/// Tool result content type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum ToolContent {
    Text { text: String },
}

/// Tool call result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

impl ToolResult {
    /// Create a successful text result
    pub fn text(text: impl Into<String>) -> Self {
        ToolResult {
            content: vec![ToolContent::Text { text: text.into() }],
            is_error: None,
        }
    }

    /// Create a successful JSON result
    pub fn json<T: Serialize>(value: &T) -> Self {
        let text = serde_json::to_string(value)
            .unwrap_or_else(|e| json!({ "error": e.to_string() }).to_string());
        ToolResult::text(text)
    }

    /// Create a pretty-printed JSON result
    pub fn json_pretty<T: Serialize>(value: &T) -> Self {
        let text = serde_json::to_string_pretty(value)
            .unwrap_or_else(|e| json!({ "error": e.to_string() }).to_string());
        ToolResult::text(text)
    }

    /// Create an error result (legacy string-only)
    pub fn error(message: impl Into<String>) -> Self {
        ToolResult {
            content: vec![ToolContent::Text {
                text: json!({ "error": message.into() }).to_string(),
            }],
            is_error: Some(true),
        }
    }

    /// Create a structured error result with error code, reason, and AI-actionable suggestion.
    pub fn structured_error(err: ToolError) -> Self {
        ToolResult {
            content: vec![ToolContent::Text {
                text: serde_json::to_string(&err)
                    .unwrap_or_else(|e| json!({ "error": e.to_string() }).to_string()),
            }],
            is_error: Some(true),
        }
    }

    /// Create an async job accepted result.
    pub fn job_accepted(job_id: &str, tool_name: &str) -> Self {
        ToolResult::json_pretty(&json!({
            "job_id": job_id,
            "status": "running",
            "tool": tool_name,
            "poll": "mission_job_poll(job_id)",
        }))
    }

    /// Create an async job accepted result with caller-specific correlation data.
    ///
    /// Some tools, such as `mission_compute_slot create`, allocate a durable
    /// identifier before the async job finishes. Returning that identifier in the
    /// accepted response lets downstream orchestrators bind BoardTasks to the
    /// future runtime object immediately instead of losing ownership while the
    /// background spawn warms up.
    pub fn job_accepted_with_metadata(job_id: &str, tool_name: &str, metadata: Value) -> Self {
        let mut payload = serde_json::Map::new();
        payload.insert("job_id".to_string(), json!(job_id));
        payload.insert("status".to_string(), json!("running"));
        payload.insert("tool".to_string(), json!(tool_name));
        payload.insert("poll".to_string(), json!("mission_job_poll(job_id)"));
        if let Value::Object(extra) = metadata {
            for (key, value) in extra {
                payload.insert(key, value);
            }
        }
        ToolResult::json_pretty(&Value::Object(payload))
    }
}

/// Structured error with machine-readable code and AI-actionable suggestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolError {
    /// Machine-readable error code (e.g. "SLOT_NOT_FOUND", "INVALID_ACTION")
    pub error_code: String,
    /// Human-readable reason
    pub reason: String,
    /// AI-actionable suggestion for recovery
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    /// Trace ID for correlation with Timeline events
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
}

impl ToolError {
    pub fn new(code: impl Into<String>, reason: impl Into<String>) -> Self {
        ToolError {
            error_code: code.into(),
            reason: reason.into(),
            suggestion: None,
            trace_id: None,
        }
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    pub fn with_trace(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }
}

// Common error codes as constants
pub mod error_codes {
    pub const UNKNOWN_TOOL: &str = "UNKNOWN_TOOL";
    pub const UNKNOWN_ACTION: &str = "UNKNOWN_ACTION";
    pub const MISSING_PARAM: &str = "MISSING_PARAM";
    pub const INVALID_PARAM: &str = "INVALID_PARAM";
    pub const NOT_FOUND: &str = "NOT_FOUND";
    pub const LIMIT_REACHED: &str = "LIMIT_REACHED";
    pub const PERMISSION_DENIED: &str = "PERMISSION_DENIED";
    pub const IPC_TIMEOUT: &str = "IPC_TIMEOUT";
    pub const SPAWN_FAILED: &str = "SPAWN_FAILED";
    pub const DB_ERROR: &str = "DB_ERROR";
    pub const EXTERNAL_ERROR: &str = "EXTERNAL_ERROR";
}

mod comm;
mod compute;
mod knowledge;
mod sysinfra;

// Domain aliases for readability
use comm::{audit, capability_usage, codex_ops, conversation, question, router_chat, timeline};
use compute::{
    cc_tasks, compute_slot, flow_run, forge, job, minimax, process, pty, slot, task, task_delegate,
    worker,
};
use knowledge::{
    agent_execution, board, cascade, directive, insight, intent, kb, memory, plan, project,
    request, skill, workflow,
};
use sysinfra::{global_instruction, infra, permission, power, system};

/// Generate all tool definitions
pub fn all_tools() -> Vec<ToolDefinition> {
    let mut tools = Vec::new();
    // knowledge
    tools.extend(board::definitions());
    tools.extend(kb::definitions());
    tools.extend(skill::definitions());
    tools.extend(memory::definitions());
    tools.extend(insight::definitions());
    tools.extend(cascade::definitions());
    tools.extend(intent::definitions());
    tools.extend(request::definitions());
    tools.extend(directive::definitions());
    tools.extend(plan::definitions());
    tools.extend(workflow::definitions());
    tools.extend(project::definitions());
    tools.extend(agent_execution::definitions());
    // compute
    tools.extend(task::definitions());
    tools.extend(process::definitions());
    tools.extend(pty::definitions());
    tools.extend(cc_tasks::definitions());
    tools.extend(minimax::definitions());
    tools.extend(worker::definitions());
    tools.extend(slot::definitions());
    tools.extend(compute_slot::definitions());
    tools.extend(task_delegate::definitions());
    tools.extend(job::definitions());
    tools.extend(flow_run::definitions());
    tools.extend(forge::definitions());
    // comm
    tools.extend(router_chat::definitions());
    tools.extend(question::definitions());
    tools.extend(conversation::definitions());
    tools.extend(timeline::definitions());
    tools.extend(audit::definitions());
    tools.extend(capability_usage::definitions());
    tools.extend(codex_ops::definitions());
    // sysinfra
    tools.extend(infra::definitions());
    tools.extend(permission::definitions());
    tools.extend(power::definitions());
    tools.extend(system::definitions());
    tools.extend(global_instruction::definitions());
    tools
}

/// Get tool by name
pub fn get_tool(name: &str) -> Option<ToolDefinition> {
    all_tools().into_iter().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_all_tools_count() {
        let tools = all_tools();
        assert!(!tools.is_empty());

        let mut names = HashSet::new();
        for tool in &tools {
            assert!(
                names.insert(tool.name.clone()),
                "duplicate tool name found: {}",
                tool.name
            );
        }

        for required in [
            "mission_task_submit",
            "mission_task_query",
            "mission_pty_spawn",
            "mission_kb_remember",
            "mission_cc_query",
        ] {
            assert!(
                names.contains(required),
                "missing required tool: {required}"
            );
        }
    }

    #[test]
    fn test_get_tool() {
        assert!(get_tool("mission_task_submit").is_some());
        assert!(get_tool("mission_pty_send").is_some());
        assert!(get_tool("unknown_tool").is_none());
    }

    #[test]
    fn kb_ops_schema_exposes_gc_action_and_required_action() {
        // Guard the public contract that the daemon dispatcher relies on:
        // mission_kb_ops splits the family verb (`action`) from the gc verb
        // (`gc_action`). If either field disappears, the daemon's
        // route_kb_ops_to_legacy substitution becomes a silent no-op and
        // gc stats/duplicates again return "Unknown gc action: gc".
        let def = get_tool("mission_kb_ops").expect("mission_kb_ops registered");
        let schema = &def.input_schema;

        let required = schema
            .get("required")
            .and_then(|v| v.as_array())
            .expect("mission_kb_ops schema must declare required");
        assert!(
            required.iter().any(|v| v.as_str() == Some("action")),
            "mission_kb_ops must require `action`"
        );

        let props = schema
            .get("properties")
            .and_then(|v| v.as_object())
            .expect("mission_kb_ops schema must declare properties");
        let action_enum = props
            .get("action")
            .and_then(|v| v.get("enum"))
            .and_then(|v| v.as_array())
            .expect("mission_kb_ops.action must be an enum");
        for verb in ["gc", "analyze", "discover", "queue_status", "execute_plan", "compact"] {
            assert!(
                action_enum.iter().any(|v| v.as_str() == Some(verb)),
                "mission_kb_ops.action enum must include {verb}"
            );
        }
        assert!(
            props.contains_key("gc_action"),
            "mission_kb_ops must expose `gc_action` so gc verbs can reach the handler"
        );
    }

    #[test]
    fn test_tool_result_json() {
        let result = ToolResult::json(&serde_json::json!({"key": "value"}));
        match &result.content[0] {
            ToolContent::Text { text } => {
                assert!(text.contains("key"));
            }
        }
    }

    #[test]
    fn test_tool_result_error() {
        let result = ToolResult::error("Something went wrong");
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn test_structured_error() {
        let err = ToolError::new(error_codes::NOT_FOUND, "Slot 'xyz' not found")
            .with_suggestion("Use action=list to see available slots")
            .with_trace("trace-abc123");

        let result = ToolResult::structured_error(err);
        assert_eq!(result.is_error, Some(true));

        match &result.content[0] {
            ToolContent::Text { text } => {
                assert!(text.contains("NOT_FOUND"));
                assert!(text.contains("xyz"));
                assert!(text.contains("action=list"));
                assert!(text.contains("trace-abc123"));
            }
        }
    }

    #[test]
    fn test_job_accepted() {
        let result = ToolResult::job_accepted("job-12345678", "mission_compute_slot:create");
        assert!(result.is_error.is_none()); // not an error
        match &result.content[0] {
            ToolContent::Text { text } => {
                assert!(text.contains("job-12345678"));
                assert!(text.contains("running"));
                assert!(text.contains("mission_job_poll"));
            }
        }
    }

    #[test]
    fn test_job_accepted_with_metadata() {
        let result = ToolResult::job_accepted_with_metadata(
            "job-12345678",
            "mission_compute_slot:create",
            json!({ "slot_id": "slot-dyn-abcd1234" }),
        );
        assert!(result.is_error.is_none());
        match &result.content[0] {
            ToolContent::Text { text } => {
                assert!(text.contains("job-12345678"));
                assert!(text.contains("slot-dyn-abcd1234"));
                assert!(text.contains("mission_job_poll"));
            }
        }
    }

    #[test]
    fn test_mission_job_poll_registered() {
        let tools = all_tools();
        let names: HashSet<_> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(
            names.contains("mission_job_poll"),
            "mission_job_poll tool not registered"
        );
    }

    #[test]
    fn test_directive_plan_workflow_surfaces_registered() {
        let tools = all_tools();
        let names: HashSet<_> = tools.iter().map(|t| t.name.as_str()).collect();
        for n in [
            "mission_request",
            "mission_directive",
            "mission_plan",
            "mission_workflow",
        ] {
            assert!(names.contains(n), "{} not registered", n);
        }
    }

    #[test]
    fn test_master_status_surface_registered() {
        let def = get_tool("mission_master_status").expect("mission_master_status not registered");
        assert!(
            def.description.contains("Codex master-control"),
            "mission_master_status description should identify the resident master surface"
        );
        let convergence = get_tool("mission_convergence_status")
            .expect("mission_convergence_status not registered");
        assert!(
            convergence.description.contains("final convergence"),
            "mission_convergence_status description should identify the final convergence gate"
        );
        let swarm = get_tool("mission_swarm_run").expect("mission_swarm_run not registered");
        assert!(
            swarm.description.contains("两阶段"),
            "mission_swarm_run description should identify the two-stage worker entry"
        );
    }

    #[test]
    fn test_request_schema_exposes_plan_routing_contract() {
        let def = get_tool("mission_request").expect("mission_request not registered");
        let props = def
            .input_schema
            .pointer("/properties")
            .and_then(|v| v.as_object())
            .expect("mission_request schema properties");
        for field in [
            "target",
            "objective",
            "requested_cwd",
            "flow_id",
            "dispatch_strategy",
            "parallelism",
            "target_project",
            "cwd",
            "project",
            "execute_mode",
            "scheduler_mode",
            "dry_run",
        ] {
            assert!(
                props.contains_key(field),
                "mission_request schema must expose `{}` from the Lisp routing contract",
                field
            );
        }
    }

    #[test]
    fn test_global_instruction_surface_registered() {
        let def = get_tool("mission_global_instruction")
            .expect("mission_global_instruction not registered");
        let enums: HashSet<_> = def
            .input_schema
            .pointer("/properties/action/enum")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        for a in ["read", "edit", "reload"] {
            assert!(
                enums.contains(a),
                "mission_global_instruction missing action `{}`",
                a
            );
        }
    }

    fn action_enum<'a>(def: &'a ToolDefinition) -> Vec<&'a str> {
        def.input_schema
            .pointer("/properties/action/enum")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn test_directive_actions_match_lisp() {
        let def = get_tool("mission_directive").unwrap();
        let enums: HashSet<_> = action_enum(&def).into_iter().collect();
        for a in [
            "compile",
            "list",
            "get",
            "approve",
            "archive",
            "version_chain",
        ] {
            assert!(
                enums.contains(a),
                "mission_directive missing action `{}`",
                a
            );
        }
    }

    #[test]
    fn test_plan_actions_match_lisp() {
        let def = get_tool("mission_plan").unwrap();
        let enums: HashSet<_> = action_enum(&def).into_iter().collect();
        for a in [
            "compile",
            "list",
            "get",
            "by_task",
            "approve",
            "mark",
            "supersede",
            "execute",
            "record_evidence",
        ] {
            assert!(enums.contains(a), "mission_plan missing action `{}`", a);
        }
    }

    #[test]
    fn test_workflow_actions_match_lisp() {
        let def = get_tool("mission_workflow").unwrap();
        let enums: HashSet<_> = action_enum(&def).into_iter().collect();
        for a in [
            "list",
            "get",
            "match",
            "apply",
            "distill",
            "record_execution",
            "compile_methodology",
            "run_methodology",
        ] {
            assert!(enums.contains(a), "mission_workflow missing action `{}`", a);
        }
    }

    /// Regression for BoardTask 31e5449c-e315-4003-ad59-c3eebd5eb837:
    /// resident master called mission_swarm_run with project list only in prose,
    /// the MCP schema did not expose `target_project_ids`, so the tool resolved
    /// targets to `missiond` only and the swarm could not fan out.
    /// V3 swarm-dispatch-policy now requires the structural pass; pin the schema.
    #[test]
    fn mission_swarm_run_schema_exposes_target_project_ids() {
        let def = get_tool("mission_swarm_run").expect("mission_swarm_run tool must be registered");
        let props = def
            .input_schema
            .get("properties")
            .and_then(|v| v.as_object())
            .expect("mission_swarm_run input_schema.properties must be an object");
        for alias in [
            "target_project_ids",
            "targetProjectIds",
            "target_projects",
            "targetProjects",
        ] {
            let entry = props
                .get(alias)
                .unwrap_or_else(|| panic!("mission_swarm_run schema missing alias `{alias}`"));
            assert_eq!(
                entry.get("type").and_then(|v| v.as_str()),
                Some("array"),
                "alias `{alias}` must be array typed so callers can pass project ids structurally"
            );
        }
    }

    /// Pin mission_swarm_run does not require target_project_ids structurally
    /// (single-project mode still works) but advertises every alias so MCP
    /// clients aren't forced to guess naming conventions.
    #[test]
    fn mission_swarm_run_only_requires_objective() {
        let def = get_tool("mission_swarm_run").expect("mission_swarm_run tool must be registered");
        let required: Vec<String> = def
            .input_schema
            .get("required")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(required, vec!["objective".to_string()]);
    }
}
