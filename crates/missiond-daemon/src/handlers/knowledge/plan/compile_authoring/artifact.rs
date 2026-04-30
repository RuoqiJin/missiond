use super::*;
use crate::handlers::knowledge::file_artifacts::{
    attempt_artifact_write, ArtifactKind, WriterContext,
};
use serde_json::{json, Value};

/// Caller-supplied args that gate the file-first writer for the plan
/// compiler. Mirror of `directive::DirectiveFileArgs`; pulled into a
/// dedicated struct so dry_run + sonnet share one extraction routine and
/// the `attempt_artifact_write` invocation stays consistent across modes.
pub(in crate::handlers::knowledge::plan) struct PlanFileArgs<'a> {
    pub(in crate::handlers::knowledge::plan) write_file: bool,
    pub(in crate::handlers::knowledge::plan) overwrite_file: bool,
    /// `topic` defaults to `board_task_id` so the file path stays anchored
    /// to the same row the DB plan inserts into. Callers can still pass an
    /// explicit `topic` for multi-plan workflows that share a board task.
    pub(in crate::handlers::knowledge::plan) topic: Option<&'a str>,
    pub(in crate::handlers::knowledge::plan) project: Option<&'a str>,
    pub(in crate::handlers::knowledge::plan) cwd: Option<&'a str>,
    pub(in crate::handlers::knowledge::plan) target_project: Option<&'a str>,
}

pub(in crate::handlers::knowledge::plan) fn extract_plan_file_args(
    args: &Value,
) -> PlanFileArgs<'_> {
    PlanFileArgs {
        write_file: args
            .get("write_file")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        overwrite_file: args
            .get("overwrite_file")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        topic: args.get("topic").and_then(|v| v.as_str()),
        project: args.get("project").and_then(|v| v.as_str()),
        cwd: args.get("cwd").and_then(|v| v.as_str()),
        target_project: args.get("target_project").and_then(|v| v.as_str()),
    }
}

/// After the plan row is committed, optionally mirror the compiled sexp to
/// the file-first SSOT
/// (`<project_root>/.missiond/plans/<topic>/PLAN.lisp`).
///
/// `topic` precedence:
///   1. explicit `topic` arg (trim-checked).
///   2. `board_task_id` fallback so the on-disk path matches the DB anchor
///      without forcing every caller to repeat the id.
///
/// Same partial / error semantics as the directive writer: DB row stays put,
/// failures land in `file_write_error` + downgraded `status="partial"`.
pub(in crate::handlers::knowledge::plan) async fn maybe_write_plan_artifact(
    state: &AppState,
    args: &PlanFileArgs<'_>,
    payload: &mut Value,
    sexp: &str,
    fallback_topic: &str,
) {
    if !args.write_file {
        return;
    }
    let topic = args
        .topic
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_topic);
    if topic.trim().is_empty() {
        if let Some(map) = payload.as_object_mut() {
            map.insert("file_written".to_string(), json!(false));
            map.insert(
                "file_write_error".to_string(),
                json!("write_file=true requires a non-empty `topic` argument (or board_task_id fallback)"),
            );
            let already_partial = map
                .get("status")
                .and_then(|v| v.as_str())
                .map(|s| s == "partial")
                .unwrap_or(false);
            if !already_partial {
                map.insert("status".to_string(), json!("partial"));
            }
        }
        return;
    }
    let outcome = attempt_artifact_write(
        &state.project_registry,
        WriterContext {
            kind: ArtifactKind::Plan,
            topic,
            project: args.project,
            cwd: args.cwd,
            target_project: args.target_project,
            overwrite: args.overwrite_file,
        },
        sexp,
    )
    .await;
    outcome.splice_into(payload);
}
