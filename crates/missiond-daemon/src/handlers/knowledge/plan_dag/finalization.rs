use crate::state::AppState;
use missiond_core::types::Plan;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};

use super::super::plan::tool_result_payload;

// ── wave-17 / task 05 — DAG finalize + distill trigger v0 ──────────────
//
// Three new opt-in knobs control the post-execution finalization step:
//
//   * `finalize_plan`        bool, default false — when false, the
//                            response shape stays byte-identical to the
//                            wave-17 / task 04 baseline (preserves the
//                            existing plan_update_status side-effect; the
//                            new `finalization` block is omitted).
//   * `distill_on_success`   bool, default false — when true (and
//                            finalize_plan=true), invoke the existing
//                            `mission_workflow(action=distill)` path AFTER
//                            a successful finalization. Only fires for the
//                            `dag_succeeded` aggregate; every other
//                            aggregate skips with a recorded reason.
//   * `distill_mode`         string, default `dry_run` — forwarded
//                            verbatim to the distill action. The strict
//                            allowlist mirrors `workflow.rs::parse_distill_mode`
//                            so the two surfaces cannot drift.
//
// CLAUDE.md "fast fail, no fallback" applies: passing `distill_on_success=true`
// without `finalize_plan=true` is rejected as INVALID_PARAM rather than
// silently ignored, because the brief explicitly forbids triggering distill
// without a successful finalization.
pub(super) const FINALIZE_DISTILL_MODE_DRY_RUN: &str = "dry_run";
pub(super) const FINALIZE_DISTILL_MODE_SONNET: &str = "sonnet";

/// Parse the `finalize_plan` opt-in toggle. Default `false` — without it
/// the response stays byte-identical with the wave-17 / task 04 baseline.
/// Non-bool values silently normalise to the default rather than fail; the
/// finalize block is purely additive so a typo never breaks an existing
/// dispatch.
pub(in crate::handlers::knowledge) fn parse_finalize_plan(args: &Value) -> bool {
    args.get("finalize_plan")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Parse the `distill_on_success` opt-in toggle. Default `false`. Only
/// honoured when `finalize_plan=true` (validated separately via
/// `validate_finalize_args` so the rejection surface stays in one place).
pub(super) fn parse_distill_on_success(args: &Value) -> bool {
    args.get("distill_on_success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Strict allowlist for the `distill_mode` knob. Mirrors
/// `workflow.rs::parse_distill_mode` so the two surfaces share the
/// vocabulary. Returns the canonical string (so callers can echo the
/// resolved mode on the response and forward it verbatim to the workflow
/// distill handler) or an error message.
pub(super) fn parse_distill_mode_arg(args: &Value) -> Result<&'static str, String> {
    match args.get("distill_mode").and_then(|v| v.as_str()) {
        None | Some("") | Some("dry_run") => Ok(FINALIZE_DISTILL_MODE_DRY_RUN),
        Some("sonnet") => Ok(FINALIZE_DISTILL_MODE_SONNET),
        Some(other) => Err(format!(
            "distill_mode must be one of [\"dry_run\", \"sonnet\"]; got `{}`",
            other
        )),
    }
}

/// Pre-flight validation for the wave-17 / task 05 finalize knobs. Returns
/// `Some(error_result)` for the call sites to early-return; `None` when the
/// args pass.
///
/// Cross-field rules enforced here:
///
///   * `distill_on_success=true` requires `finalize_plan=true` — silently
///     dropping a distill request would mask the caller's intent.
///   * `distill_mode` must be on the strict allowlist — even when
///     `distill_on_success=false` we validate so a typo surfaces immediately
///     (not on the next caller's actual distill run).
pub(super) fn validate_finalize_args(args: &Value) -> Option<ToolResult> {
    if let Err(msg) = parse_distill_mode_arg(args) {
        return Some(ToolResult::structured_error(ToolError::new(
            error_codes::INVALID_PARAM,
            msg,
        )));
    }
    let finalize = parse_finalize_plan(args);
    let distill = parse_distill_on_success(args);
    if distill && !finalize {
        return Some(ToolResult::structured_error(
            ToolError::new(
                error_codes::INVALID_PARAM,
                "distill_on_success=true requires finalize_plan=true",
            )
            .with_suggestion(
                "the distill trigger fires only AFTER a successful finalization; \
                 set finalize_plan=true or drop distill_on_success",
            ),
        ));
    }
    None
}

/// Map an aggregate status string (`dag_succeeded` / `dag_failed` /
/// `dag_partial` / `dag_paused`) to the plan-status label the finalization
/// block surfaces. Keeping this pure makes the wave-17 / task 05 mapping
/// table testable without standing up a full AppState.
///
/// * `dag_succeeded`            → `"succeeded"` (terminal claim of success)
/// * `dag_failed` / `dag_partial` (with any failed node) → `"failed"`
/// * `dag_paused`               → preserves the current plan status — we
///                                 NEVER claim success while a node is
///                                 paused awaiting review (per the brief
///                                 "do not lie" invariant).
/// * anything else              → `"unchanged"` (defensive fallback so a
///                                 future aggregate can extend the table
///                                 without panicking).
pub(super) fn finalize_plan_status_label(
    aggregate: &str,
    current_plan_status: &str,
) -> &'static str {
    match aggregate {
        "dag_succeeded" => "succeeded",
        "dag_failed" => "failed",
        // `dag_partial` always carries at least one failed node (the
        // aggregate matrix in `ExecutionOutcome::aggregate_status` hands out
        // `dag_partial` only when `any_failed()` is true OR when there are
        // skipped nodes without paused/failure — the latter still counts as
        // a non-success that we surface as `failed` so the plan FSM does
        // not silently advance to `succeeded`).
        "dag_partial" => "failed",
        // Paused → we explicitly preserve the current status. The plan stays
        // in whatever pre-execute state it was in (Approved / Executing /
        // AwaitingReview); the resume helper (wave-17 / task 01) is
        // responsible for advancing it once the gate resolves.
        "dag_paused" => unchanged_status_label(current_plan_status),
        _ => unchanged_status_label(current_plan_status),
    }
}

/// Helper for the paused / unknown-aggregate branch of
/// `finalize_plan_status_label`. Returns `"executing"` when the plan was
/// mid-flight (the wave-17 / task 04 + wave-13 contract leaves the plan in
/// `Executing` while the DAG runs), `"awaiting_review"` when the caller
/// supplied that explicit string, otherwise `"unchanged"`. Pure projection.
fn unchanged_status_label(current_plan_status: &str) -> &'static str {
    match current_plan_status {
        "executing" => "executing",
        "awaiting_review" => "awaiting_review",
        _ => "unchanged",
    }
}

/// Build the `finalization` block surfaced on the response when
/// `finalize_plan=true`. Pure projection over the aggregate + observed
/// plan-status update. Carries the rule label so callers can grep the
/// reason without re-deriving it from the aggregate alone.
pub(super) fn build_finalization_block(
    aggregate: &str,
    plan_status_after: Option<&str>,
    plan_status_update_error: Option<&str>,
    distill_block: Option<Value>,
) -> Value {
    let final_plan_status = plan_status_after.unwrap_or("unchanged");
    let mut block = json!({
        "finalize_plan": true,
        "aggregate_status": aggregate,
        "final_plan_status": final_plan_status,
        "rule": match aggregate {
            "dag_succeeded" => "all_terminal_no_failed_no_paused",
            "dag_failed" => "fail_fast_or_failure_dominates",
            "dag_partial" => "failed_node_or_skipped_without_paused",
            "dag_paused" => "paused_node_present_no_finalization",
            _ => "unrecognised_aggregate_no_finalization",
        },
    });
    if let Some(err) = plan_status_update_error {
        block["plan_status_update_error"] = json!(err);
    }
    if let Some(d) = distill_block {
        block["distill"] = d;
    }
    block
}

/// Build the `distill` sub-block describing the trigger outcome. Always
/// surfaces a `triggered` boolean + a `reason` string so observers can
/// pivot on a single flag without inspecting the inner payload. The
/// inner workflow handler payload (success or error) is preserved under
/// `result` for full audit traceability.
pub(super) fn build_distill_block(
    triggered: bool,
    reason: &str,
    distill_mode: &str,
    inner_payload: Option<Value>,
    inner_is_error: bool,
) -> Value {
    let mut block = json!({
        "triggered": triggered,
        "reason": reason,
        "distill_mode": distill_mode,
    });
    if let Some(p) = inner_payload {
        block["result"] = p;
    }
    if triggered && inner_is_error {
        // Surface a partial-success warning so callers can detect a
        // distill failure without scraping the inner payload. The
        // finalization status itself is NOT downgraded — distill failure
        // never corrupts the plan final state per the brief.
        block["warning"] = json!("distill trigger returned an error; plan final state preserved");
    }
    block
}

/// wave-17 / task 05 — drive the optional distill trigger. Returns the
/// `distill` block (or `None` when no trigger was requested). Async egress
/// lives beside the finalization projection helpers so the scheduler loop
/// only decides when to finalize.
///
/// Decision matrix:
///
///   * `distill_on_success=false`              → return `None`
///   * `aggregate != dag_succeeded`            → block with `triggered=false`
///                                               and a recorded skip reason
///   * `plan_status_after != "succeeded"`      → block with `triggered=false`
///                                               (defensive: the workflow
///                                               distill handler also gates
///                                               on plan.status==Succeeded;
///                                               if the FSM update failed we
///                                               do NOT call distill because
///                                               the gate would refuse anyway)
///   * otherwise                               → call workflow distill,
///                                               surface its result + a
///                                               warning when it errored
pub(super) async fn maybe_run_distill_trigger(
    state: &AppState,
    args: &Value,
    plan: &Plan,
    aggregate_status: &str,
    plan_status_after: Option<&str>,
) -> Option<Value> {
    if !parse_distill_on_success(args) {
        return None;
    }
    let distill_mode = match parse_distill_mode_arg(args) {
        Ok(m) => m,
        Err(_) => {
            // Unreachable: validate_finalize_args already returned the
            // structured error before we got here. Defensive return so a
            // future refactor cannot silently bypass the validator.
            return Some(build_distill_block(
                false,
                "distill_mode_invalid_unreachable",
                FINALIZE_DISTILL_MODE_DRY_RUN,
                None,
                false,
            ));
        }
    };
    if aggregate_status != "dag_succeeded" {
        return Some(build_distill_block(
            false,
            "aggregate_not_succeeded",
            distill_mode,
            None,
            false,
        ));
    }
    if plan_status_after != Some("succeeded") {
        return Some(build_distill_block(
            false,
            "plan_status_not_succeeded_after_finalize",
            distill_mode,
            None,
            false,
        ));
    }
    // Build the distill args object. We forward the project-resolution
    // signals (`project` / `cwd` / `target_project`) verbatim so the
    // distill handler's evidence-sidecar reader resolves the same root the
    // DAG run wrote into. `persist=false` by default — the wave-17 / task
    // 05 trigger is an automatic preview pass, not a stamp-the-registry
    // call. Callers that want persistence still issue an explicit
    // `mission_workflow(action=distill, persist=true)` themselves.
    let mut distill_args = serde_json::Map::new();
    distill_args.insert("action".to_string(), json!("distill"));
    distill_args.insert("plan_id".to_string(), json!(plan.id.to_string()));
    distill_args.insert("distill_mode".to_string(), json!(distill_mode));
    if let Some(p) = args.get("project").and_then(|v| v.as_str()) {
        distill_args.insert("project".to_string(), json!(p));
    }
    if let Some(c) = args.get("cwd").and_then(|v| v.as_str()) {
        distill_args.insert("cwd".to_string(), json!(c));
    }
    if let Some(tp) = args.get("target_project").and_then(|v| v.as_str()) {
        distill_args.insert("target_project".to_string(), json!(tp));
    }
    let distill_call_args = Value::Object(distill_args);
    let distill_result =
        super::super::workflow::handle(state, "mission_workflow", distill_call_args).await;
    match distill_result {
        Ok(tr) => {
            let inner_payload = tool_result_payload(&tr);
            let inner_is_error = tr.is_error.unwrap_or(false);
            let reason = if inner_is_error {
                "distill_invoked_returned_error"
            } else {
                "distill_invoked_ok"
            };
            Some(build_distill_block(
                true,
                reason,
                distill_mode,
                Some(inner_payload),
                inner_is_error,
            ))
        }
        Err(e) => {
            // Unexpected handler-level error (bubbled `Result::Err`). Surface
            // it as a warning + non-fatal: the plan final state is preserved
            // because we already updated it to Succeeded above.
            tracing::warn!(
                plan_id = %plan.id,
                error = %e,
                "DAG finalize: distill trigger handler returned error"
            );
            Some(build_distill_block(
                true,
                "distill_invoked_handler_error",
                distill_mode,
                Some(json!({"error": e.to_string()})),
                true,
            ))
        }
    }
}
