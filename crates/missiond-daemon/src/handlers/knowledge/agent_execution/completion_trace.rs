use serde_json::{json, Value};
use std::path::Path;

use super::completion_fields::collect_string_list;
use super::session_trace::{
    append_session_trace_event, resolve_session_trace_path, resolve_trace_task_id,
    sanitize_trace_backend, TraceEvent, TraceKind,
};

pub(super) fn append_completion_trace_if_requested(
    args: &Value,
    root: &Path,
    execution_id: &str,
    phase: &str,
    agent: &str,
    completion_id: &str,
    response: &mut Value,
) {
    let Some(trace_path) = resolve_session_trace_path(args, root) else {
        return;
    };
    match resolve_trace_task_id(args, root, execution_id) {
        Some(task_id) => {
            // Failure when caller-supplied OR daemon-computed verifier
            // status resolved to "failed". Otherwise treat the
            // completion as a success-shaped event.
            let final_verifier_status = response
                .get("verifier_status")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let kind = match final_verifier_status.as_deref() {
                Some("failed") => TraceKind::Failure,
                _ => TraceKind::Complete,
            };
            let backend = sanitize_trace_backend(agent);
            // Re-read the commit / report / file metadata from args
            // since action_complete consumes the local bindings while
            // building the response.
            let commit_hash_for_trace = args
                .get("commit_hash")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                // checker requires `[0-9a-f]{4,64}` — drop anything
                // shorter / non-hex so we don't fail validation.
                .filter(|s| {
                    s.len() >= 4 && s.len() <= 64 && s.chars().all(|c| c.is_ascii_hexdigit())
                });
            let report_path_for_trace = args
                .get("task_report_path")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                // checker rejects absolute report paths.
                .filter(|s| !Path::new(s).is_absolute());
            let files_for_trace = collect_string_list(args, "changed_files")
                .or_else(|| collect_string_list(args, "staged_files"))
                .map(|v| {
                    v.into_iter()
                        // strip absolute paths — checker rejects them
                        .filter(|p| !Path::new(p).is_absolute())
                        .collect::<Vec<_>>()
                })
                .filter(|v: &Vec<String>| !v.is_empty());
            let ev = TraceEvent {
                task: task_id,
                backend,
                kind,
                summary: format!(
                    "mission_execution(action=complete) phase={} agent={} completion_id={}",
                    phase, agent, completion_id
                ),
                agent: None,
                files: files_for_trace,
                commit_hash: commit_hash_for_trace,
                report_path: report_path_for_trace,
            };
            if let Err(w) = append_session_trace_event(&trace_path, &ev) {
                response["trace_warning"] = json!(w.to_string());
            }
        }
        None => {
            response["trace_warning"] = json!(format!(
                "session_trace_path supplied but execution_id `{}` is not a valid trace task id and no task_contract_path was provided",
                execution_id
            ));
        }
    }
}
