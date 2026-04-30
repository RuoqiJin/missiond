use missiond_mcp::tools::{error_codes, ToolContent, ToolError, ToolResult};
use serde_json::Value;

mod manifest;
mod projection;

use manifest::load_runner_inputs;
use projection::build_runner_response_block;

/// Schema label embedded in every emitted task_runner block. Mirrors
/// the wave28-02 CLI's `PLAN_SCHEMA` so downstream consumers can
/// verify the wire shape against the same constant.
pub(super) const SCHEMA: &str = "missiond.task-runner-plan.v0";
pub(super) const MANIFEST_SCHEMA: &str = "missiond.task-runner-manifest.v1";

/// Recognised top-level `task_runner_mode` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TaskRunnerMode {
    /// Default. The task_runner block is NOT emitted; the response
    /// is byte-identical to the wave-15..27 baseline.
    Off,
    /// Read the manifest, project deterministic facts, and emit
    /// `applied=false`. Never alters dispatch.
    DryRun,
}

/// Parse the optional `task_runner_mode` arg. Returns `Off` when the
/// arg is absent / null / the literal string `"off"`. Returns a
/// structured `INVALID_PARAM` error for any other value (including
/// `apply` / `auto` / unknown strings / non-strings) so a typo cannot
/// silently route the surface through an unimplemented mode.
pub(super) fn parse_task_runner_mode(args: &Value) -> Result<TaskRunnerMode, ToolResult> {
    let raw = match args.get("task_runner_mode") {
        None | Some(Value::Null) => return Ok(TaskRunnerMode::Off),
        Some(v) => v,
    };
    let s = match raw.as_str() {
        Some(s) => s.trim(),
        None => {
            return Err(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    "task_runner_mode must be a string",
                )
                .with_suggestion("expected one of: \"off\", \"dry_run\""),
            ));
        }
    };
    match s {
        "" | "off" => Ok(TaskRunnerMode::Off),
        "dry_run" => Ok(TaskRunnerMode::DryRun),
        other => Err(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!(
                    "task_runner_mode `{}` is not supported in this surface (wave28-04 only ships `off` and `dry_run`)",
                    other
                ),
            )
            .with_suggestion(
                "expected one of: \"off\" (default; no task_runner block) or \"dry_run\" (informational block, applied=false)",
            ),
        )),
    }
}

/// Splice the task_runner block onto a successful response. No-op
/// when `mode=Off` so callers that never opted in observe the
/// wave-15..27 byte-shape, AND no file I/O is performed even if
/// `task_runner_manifest_path` is supplied. Errors are passed through
/// unchanged — we never decorate a structured error with the block.
pub(super) fn attach_task_runner_block(
    mut result: ToolResult,
    mode: TaskRunnerMode,
    args: &Value,
) -> ToolResult {
    if matches!(mode, TaskRunnerMode::Off) {
        return result;
    }
    if result.is_error.unwrap_or(false) {
        return result;
    }
    let block = compute_runner_block(args);
    let Some(ToolContent::Text { text }) = result.content.first_mut() else {
        return result;
    };
    let Ok(mut value) = serde_json::from_str::<Value>(text) else {
        return result;
    };
    if let Some(map) = value.as_object_mut() {
        // Never overwrite a pre-existing block — preserves any forward-
        // compatible attachment a downstream layer may add.
        map.entry("task_runner".to_string()).or_insert(block);
    }
    *text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.clone());
    result
}

/// Top-level entry called only on the dry_run path. Reads the
/// optional manifest, projects deterministic facts, and assembles
/// the response block. NEVER panics; surfaces I/O / parse failures
/// via the `manifest_status` field + `task_runner_warning` field.
pub(super) fn compute_runner_block(args: &Value) -> Value {
    // Pure projection split: parsing/loading produces a `RunnerInputs`
    // intermediate so the response builder is testable in isolation
    // (mirrors wave27-03's compute_recommendation_block split).
    let inputs = load_runner_inputs(args);
    build_runner_response_block(&inputs)
}
