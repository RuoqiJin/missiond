use serde_json::{json, Value};
use std::path::Path;

use super::session_trace::{
    append_session_trace_event, resolve_session_trace_path, resolve_trace_task_id, TraceEvent,
    TraceKind,
};

pub(super) fn append_preflight_trace_if_requested(
    args: &Value,
    root: &Path,
    execution_id: &str,
    summary: &mut Value,
) {
    let Some(trace_path) = resolve_session_trace_path(args, root) else {
        return;
    };

    match resolve_trace_task_id(args, root, execution_id) {
        Some(task_id) => {
            let ok_flag = summary.get("ok").and_then(|v| v.as_bool()).unwrap_or(true);
            let staged_count = summary
                .get("staged_files")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let changed_count = summary
                .get("changed_files")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let ev = TraceEvent {
                task: task_id,
                backend: "claudecode".to_string(),
                kind: TraceKind::Observation,
                summary: format!(
                    "mission_execution(action=preflight_commit) execution_id={} ok={} staged={} changed={}",
                    execution_id, ok_flag, staged_count, changed_count
                ),
                agent: None,
                files: None,
                commit_hash: None,
                report_path: None,
            };
            if let Err(w) = append_session_trace_event(&trace_path, &ev) {
                summary["trace_warning"] = json!(w.to_string());
            }
        }
        None => {
            summary["trace_warning"] = json!(format!(
                "session_trace_path supplied but execution_id `{}` is not a valid trace task id and no task_contract_path was provided",
                execution_id
            ));
        }
    }
}
