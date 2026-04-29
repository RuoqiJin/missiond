use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::Value;

use super::super::plan::parse_infer_plan_fields_mode;

/// Convenience helper used by `plan::action_execute` to detect whether the
/// caller asked for the DAG scheduler. Returns `Ok(true)` when the value is
/// `dag_v1`, `Ok(false)` for absent/empty/`v0`/`single_node`/`default`, and
/// `Err(structured)` when the value is unrecognised.
pub(in crate::handlers::knowledge) fn detect_scheduler_mode(
    args: &Value,
) -> std::result::Result<bool, ToolResult> {
    let raw = args.get("scheduler_mode").and_then(|v| v.as_str());
    let mode = raw.map(|s| s.trim()).unwrap_or("");
    match mode {
        "" | "v0" | "default" | "current" | "single_node" | "single-node" => Ok(false),
        "dag_v1" | "dag-v1" => Ok(true),
        other => Err(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                format!("unknown scheduler_mode `{}`", other),
            )
            .with_suggestion("scheduler_mode ∈ {default, dag_v1}"),
        )),
    }
}

/// wave-20 / task 07 — DAG path guard for LLM-augmented inference modes.
///
/// `infer_plan_fields=sonnet_suggest` is a single-node-execute-only
/// feature in v0: the deterministic engine + Sonnet pass operate on the
/// PLAN-level sexp, not on per-node fan-out. Combining the LLM mode with
/// `scheduler_mode=dag_v1` would silently skip the LLM pass (the DAG
/// scheduler runs before any inference can happen) — that violates the
/// fail-fast contract. We refuse the combo eagerly so callers receive a
/// structured error pointing at the right surface, instead of an
/// unexpected response missing the LLM proposal block.
///
/// Returns `None` when the args are clean (or carry a deterministic
/// `infer_plan_fields` mode); `Some(error)` when the combo is rejected.
/// Pure: no AppState reads, no IO.
pub(in crate::handlers::knowledge) fn refuse_llm_inference_in_dag_mode(
    args: &Value,
) -> Option<ToolResult> {
    let mode = match parse_infer_plan_fields_mode(args) {
        Ok(m) => m,
        // The single-node execute path validates parse errors before the
        // DAG branch is reached; if we somehow get here with a typo we
        // re-surface the parse error as a structured tool error so the
        // caller still sees a helpful message.
        Err(msg) => {
            return Some(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                msg,
            )));
        }
    };
    if !mode.is_llm_augmented() {
        return None;
    }
    Some(ToolResult::structured_error(
        ToolError::new(
            error_codes::INVALID_PARAM,
            format!(
                "infer_plan_fields=`{}` is single-node-execute-only in v0; \
                 it is not supported with scheduler_mode=dag_v1",
                mode.as_wire(),
            ),
        )
        .with_suggestion(
            "drop scheduler_mode=dag_v1 to use the single-node execute path with \
             sonnet_suggest, or rerun the DAG with infer_plan_fields ∈ {off, preview, apply_safe}",
        ),
    ))
}
