use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use super::super::super::plan;

/// wave-19 / task 06 — per-DAG-run task-contract emission context. The
/// scheduler resolves the mode + project-resolution signals once at the
/// top of `action_execute_dag_v1` and clones one of these into every
/// `dispatch_node` task so the per-node emit does not have to re-parse
/// the caller args (and stays aligned with the single-node runner's
/// project-root resolution path). All fields are owned (no borrowed
/// references) so the struct survives `tokio::JoinSet::spawn`'s
/// `'static` requirement.
///
/// wave-20 / task 04 — extended with `dispatch_contract_mode` so DAG
/// nodes can opt the workstation substrate into machine-driven dispatch
/// (read the emitted task.lisp directly). The mode is parsed once at
/// the scheduler entry point and cloned into every per-node task —
/// per-node mode overrides would defeat the cross-node SSOT contract.
#[derive(Debug, Clone)]
pub(in crate::handlers::knowledge::plan_dag) struct TaskContractDispatchCtx {
    pub mode: plan::TaskContractEmitMode,
    pub dispatch_contract_mode: plan::DispatchContractMode,
    pub project_arg: Option<String>,
    pub cwd_arg: Option<String>,
    pub target_project_arg: Option<String>,
}

impl TaskContractDispatchCtx {
    pub(in crate::handlers::knowledge::plan_dag) fn off() -> Self {
        Self {
            mode: plan::TaskContractEmitMode::Off,
            dispatch_contract_mode: plan::DispatchContractMode::Rendered,
            project_arg: None,
            cwd_arg: None,
            target_project_arg: None,
        }
    }

    /// Build the ctx from caller args. Returns
    /// `Err(structured)` for malformed `task_contract_mode` /
    /// `dispatch_contract_mode` values so the scheduler fails fast
    /// before spawning any node task.
    pub(in crate::handlers::knowledge::plan_dag) fn from_args(
        args: &Value,
    ) -> std::result::Result<Self, ToolResult> {
        let mode = plan::parse_task_contract_emit_mode(args)?;
        let dispatch_contract_mode = plan::parse_dispatch_contract_mode(args)?;
        Ok(Self {
            mode,
            dispatch_contract_mode,
            project_arg: args
                .get("project")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            cwd_arg: args
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            target_project_arg: args
                .get("target_project")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        })
    }
}
