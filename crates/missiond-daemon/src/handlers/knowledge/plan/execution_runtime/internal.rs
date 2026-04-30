use super::*;

pub(in crate::handlers::knowledge::plan) const PLAN_RUNNER_EVENT_REF_UNAVAILABLE_REASON: &str =
    "single-node internal dispatch records dispatch evidence without a live ExecutionEvent ref; \
     correlate by plan_id + board_task_id";
use crate::handlers::compute::{flow_run, task_delegate};
use crate::handlers::knowledge::{agent_execution, evidence_collector, workstation_dispatch};

pub(in crate::handlers::knowledge::plan) async fn action_execute_internal(
    state: &AppState,
    args: &Value,
    plan: &Plan,
    resolved: &ResolvedExec,
    hints: &ParsedPlanHints,
) -> Result<ToolResult> {
    let dry_run = args
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // wave-19 / task 06 — pre-flight validate the task-contract emit knobs
    // BEFORE any dispatch path so a typo (`task_contract_mode="emi"`)
    // fails fast rather than after a dispatch already produced side
    // effects. Default mode is `Off`: byte-compatible with pre-wave19.
    let task_contract_mode = match parse_task_contract_emit_mode(args) {
        Ok(m) => m,
        Err(err_result) => return Ok(err_result),
    };

    // wave-20 / task 04 — pre-flight validate the dispatch-contract mode
    // (rendered = wave-15..19 byte-compat; machine = consumer reads the
    // emitted task.lisp directly). A typo
    // (`dispatch_contract_mode="machin"`) fails fast before any
    // workstation substrate side effect.
    let dispatch_contract_mode = match parse_dispatch_contract_mode(args) {
        Ok(m) => m,
        Err(err_result) => return Ok(err_result),
    };

    // wave-23 / task 05 — pre-flight validate the optional session-trace
    // ledger path. Default absent ⇒ byte-compatible with wave-15..22 (no
    // forward, no warning, no response field). When supplied, the
    // daemon checks only basic shape (non-empty after trim, no NUL or
    // ASCII control chars except space). Malformed shape with
    // `session_trace_required=true` ⇒ structured INVALID_PARAM error
    // BEFORE any dispatch side effect; without `session_trace_required`
    // ⇒ surface a non-fatal `trace_path_warning` field on the response
    // and continue with the trace forward suppressed.
    let trace_required = args
        .get("session_trace_required")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let trace_input = args
        .get("session_trace_path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let (resolved_trace_path, trace_path_warning) =
        match validate_session_trace_path_arg(trace_input.as_deref(), trace_required) {
            Ok(pair) => pair,
            Err(err_result) => return Ok(err_result),
        };

    // wave-15 / task 05 + wave-16 / task 03 — workstation-dispatch routing.
    // Wave-15 honours explicit opt-in (caller arg `workstation_dispatch=true`
    // or PLAN.lisp `:workstation-dispatch true`). Wave-16 layers conservative
    // auto-inference on top: when caller / plan are silent AND the resolved
    // shape is unmistakably a ClaudeCode workstation task, the runner
    // auto-enables. Explicit `workstation_dispatch=false` always wins and
    // suppresses inference. We never `claude -p`; we never broaden the target
    // whitelist; auto-inference is restricted to `mission_task_delegate`.
    let merged_hints = hints.to_workstation_hints().merge_args(args);
    let inference_ctx = workstation_dispatch::InferenceContext {
        target: resolved.target,
        dispatch_strategy: resolved.dispatch_strategy,
        objective: merged_hints.objective.as_deref(),
        owned_files_present: !merged_hints.owned_files.is_empty(),
        scope_present: merged_hints
            .scope
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        target_project_present: merged_hints
            .target_project
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        requested_cwd_present: merged_hints
            .requested_cwd
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
    };
    let dispatch_decision = workstation_dispatch::evaluate_dispatch_decision(
        args,
        hints.workstation_dispatch_opt_in(),
        &inference_ctx,
    );

    // wave-19 / task 06 — emit the task-contract sidecar BEFORE any
    // dispatch path so the contract is the SSOT for whatever work
    // happens next. Failures REFUSE the dispatch (a missing contract
    // must not be papered over by a successful inner call). EmitDryRun
    // additionally skips substrate / inner dispatch once the contract
    // has been written. Default mode (`Off`) returns an empty record
    // and the response payload omits the task-contract fields entirely.
    let task_contract_inputs = task_contract_inputs_from_hints_with_trace(
        &merged_hints,
        resolved.target,
        resolved.dispatch_strategy,
        resolved_trace_path.as_deref(),
    );
    let project_arg_for_emit = args.get("project").and_then(|v| v.as_str());
    let cwd_arg_for_emit = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg_for_emit = args
        .get("target_project")
        .and_then(|v| v.as_str())
        .or(hints.target_project.as_deref());
    let emission = emit_task_contract(
        state,
        plan.id,
        &plan.board_task_id,
        "root",
        task_contract_mode,
        &task_contract_inputs,
        project_arg_for_emit,
        cwd_arg_for_emit,
        target_project_arg_for_emit,
    )
    .await;

    if emission.is_failure() {
        // Refuse dispatch — surface the IO failure plus the
        // resolved plan/target so the caller can fix permissions
        // or registry config and retry. Plan FSM untouched.
        let mut response =
            build_task_contract_failure_response(plan, resolved, &dispatch_decision, &emission);
        attach_session_trace_response_fields(
            &mut response,
            resolved_trace_path.as_deref(),
            trace_path_warning.as_deref(),
        );
        return Ok(response);
    }

    if task_contract_mode.is_dry_run() {
        // Skip substrate / inner dispatch — surface the contract path
        // so the caller can render the markdown brief without touching
        // the inner tool. Plan FSM untouched.
        let mut response =
            build_task_contract_dry_run_response(plan, resolved, &dispatch_decision, &emission);
        attach_session_trace_response_fields(
            &mut response,
            resolved_trace_path.as_deref(),
            trace_path_warning.as_deref(),
        );
        return Ok(response);
    }

    if dispatch_decision.is_enabled() {
        // wave-20 / task 04 — when caller opted into machine-driven
        // dispatch AND the wave-19 / task 06 emitter actually wrote a
        // contract for this dispatch, hand the absolute path to the
        // wave-19 / task 07 consumer so the brief is built FROM the
        // on-disk Lisp SSOT rather than the in-memory hints. The
        // consumer reads the contract, overlays it onto the hints
        // (contract wins on every non-empty field), and refuses to
        // fall back to the legacy natural-language brief on a
        // malformed contract — that refusal surfaces verbatim as
        // `SafeDescriptor` (status=`skipped_malformed_task_contract`),
        // never `claude -p`. When the emitter is OFF (default) or the
        // node was ineligible, machine mode is a no-op for THIS
        // dispatch — the runner falls back to the legacy rendered
        // path so existing callers that opt into machine mode without
        // pairing it with `task_contract_mode="emit"` keep working.
        let task_contract_path_for_machine = if dispatch_contract_mode.is_machine() {
            emission.path.clone()
        } else {
            None
        };
        let outcome = workstation_dispatch::run_workstation_dispatch_with_contract_and_trace(
            state,
            plan,
            resolved.target,
            resolved.dispatch_strategy,
            merged_hints,
            dry_run,
            task_contract_path_for_machine.as_deref(),
            resolved_trace_path.as_deref(),
        )
        .await;
        // Only transition the plan FSM on the Dispatched branch — every
        // other branch leaves the plan in its current status so the
        // caller can fix the input and retry without manual cleanup.
        if matches!(
            outcome,
            workstation_dispatch::WorkstationDispatchOutcome::Dispatched { .. }
        ) && !matches!(plan.status, PlanStatus::Executing)
        {
            if let Err(e) = state
                .store
                .plan_update_status(plan.id, PlanStatus::Executing)
                .await
            {
                tracing::warn!(
                    plan_id = %plan.id,
                    error = %e,
                    "workstation_dispatch: failed to transition plan to executing"
                );
            }
        }
        let mut response = build_workstation_dispatch_response(
            plan,
            resolved,
            outcome,
            &dispatch_decision,
            &emission,
            dispatch_contract_mode,
        );
        attach_session_trace_response_fields(
            &mut response,
            resolved_trace_path.as_deref(),
            trace_path_warning.as_deref(),
        );
        return Ok(response);
    }

    let mut inner_args = match build_internal_dispatch_args(
        args,
        plan,
        resolved.target,
        resolved.dispatch_strategy,
        hints,
    ) {
        Ok(v) => v,
        Err(err_result) => return Ok(err_result),
    };
    // wave-23 / task 05 — forward the resolved trace path into the
    // inner-handler args. Only `mission_execution` consumes the field
    // today (wave-23 / task 04); other targets ignore the unknown key.
    if let Some(stp) = resolved_trace_path.as_deref() {
        if let Some(map) = inner_args.as_object_mut() {
            map.insert("session_trace_path".to_string(), json!(stp));
        }
    }

    if dry_run {
        let mut payload = json!({
            "status": "dry_run",
            "execute_mode": "internal",
            "runner_status": "dry_run_no_dispatch",
            "plan_id": plan.id,
            "board_task_id": plan.board_task_id,
            "target_tool": resolved.target,
            "target_source": resolved.target_source,
            "dispatch_strategy": resolved.dispatch_strategy,
            "dispatch_strategy_source": resolved.dispatch_strategy_source,
            "plan_hint_summary": resolved.plan_hint_summary,
            "would_dispatch": inner_args,
            "workstation_dispatch_source": dispatch_decision.source.as_str(),
        });
        if let Some(reason) = dispatch_decision.reason.as_deref() {
            payload["workstation_dispatch_inference_reason"] = json!(reason);
        }
        merge_task_contract_block(&mut payload, &emission);
        let mut response = ToolResult::json_pretty(&payload);
        attach_session_trace_response_fields(
            &mut response,
            resolved_trace_path.as_deref(),
            trace_path_warning.as_deref(),
        );
        return Ok(response);
    }

    let inner_result = match resolved.target {
        "mission_execution" => {
            agent_execution::handle(state, "mission_execution", inner_args.clone()).await?
        }
        "mission_task_delegate" => {
            task_delegate::handle(state, "mission_task_delegate", inner_args.clone()).await?
        }
        "mission_flow_run" => {
            flow_run::handle(state, "mission_flow_run", inner_args.clone()).await?
        }
        _ => unreachable!("target whitelist already enforced"),
    };

    let inner_payload = tool_result_payload(&inner_result);
    let inner_is_error = inner_result.is_error.unwrap_or(false);

    if inner_is_error {
        // Don't transition plan; just report the inner failure verbatim so the
        // caller can decide whether to retry, fix args, or escalate.
        let mut payload = json!({
            "status": "dispatch_failed",
            "execute_mode": "internal",
            "runner_status": "inner_returned_error",
            "plan_id": plan.id,
            "board_task_id": plan.board_task_id,
            "target_tool": resolved.target,
            "target_source": resolved.target_source,
            "dispatch_strategy": resolved.dispatch_strategy,
            "dispatch_strategy_source": resolved.dispatch_strategy_source,
            "plan_hint_summary": resolved.plan_hint_summary,
            "inner_result": inner_payload,
            "workstation_dispatch_source": dispatch_decision.source.as_str(),
        });
        if let Some(reason) = dispatch_decision.reason.as_deref() {
            payload["workstation_dispatch_inference_reason"] = json!(reason);
        }
        merge_task_contract_block(&mut payload, &emission);
        let mut response = ToolResult::json_pretty(&payload);
        attach_session_trace_response_fields(
            &mut response,
            resolved_trace_path.as_deref(),
            trace_path_warning.as_deref(),
        );
        return Ok(response);
    }

    // Successful dispatch — append evidence then transition plan to executing.
    //
    // Project root resolution for evidence sidecar placement honours the
    // canonical contract (intent-worker.lisp :: project-root-spawn-cwd):
    //   - `project`         → registry id (primary)
    //   - `cwd`             → absolute path (longest-prefix), or rejected if relative
    //   - `target_project`  → registry id (fallback)
    //   - plan-hint :target-project also fed into the fallback slot
    // No process-cwd fallback. Evidence-sidecar failures still degrade
    // gracefully (`evidence_error`) — the inner dispatch already produced
    // durable side effects, so we surface the failure but do not abort.
    let project_arg = args.get("project").and_then(|v| v.as_str());
    let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg = args
        .get("target_project")
        .and_then(|v| v.as_str())
        .or(hints.target_project.as_deref());
    // wave-13 :: typed evidence-collector path. Legacy `kind` ("dispatch") +
    // legacy `source` ("plan_runner_dispatch") are preserved so any reader
    // that filtered on those exact strings keeps working — `kind` is the
    // canonical taxonomy from `evidence_collector::kind` and `source` is the
    // historical wire tag (also re-exported as `evidence_collector::source::
    // PLAN_RUNNER_DISPATCH`). Inner dispatch summary, plan-hint passthrough,
    // and target/strategy provenance all land under their canonical typed
    // keys; legacy passthrough keys (`execute_mode`, `target_tool`,
    // `target_source`, `dispatch_strategy_source`, `plan_hint_summary`) keep
    // their flat-top-level placement via `with_extra` so audit dashboards do
    // not need to traverse the new `inner_dispatch` wrapper to find them.
    let entry = evidence_collector::EvidenceEntry::new(
        evidence_collector::source::PLAN_RUNNER_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_inner_dispatch(inner_payload.clone())
    .add_execution_event(evidence_collector::EventRef::unavailable(
        PLAN_RUNNER_EVENT_REF_UNAVAILABLE_REASON,
    ))
    .with_extra("execute_mode", json!("internal"))
    .with_extra("target_tool", json!(resolved.target))
    .with_extra("target_source", json!(resolved.target_source))
    .with_extra("dispatch_strategy", json!(resolved.dispatch_strategy))
    .with_extra(
        "dispatch_strategy_source",
        json!(resolved.dispatch_strategy_source),
    )
    .with_extra("plan_hint_summary", resolved.plan_hint_summary.clone())
    // Legacy `inner_result` alias: pre-wave12 sidecars carried the inner
    // payload under `inner_result`, the new canonical slot is
    // `inner_dispatch`. We keep BOTH so historical readers (audit
    // dashboards, retrospective queries) that filter on `inner_result`
    // keep working byte-for-byte during the transition.
    .with_extra("inner_result", inner_payload.clone());
    let outcome = evidence_collector::append(
        state,
        plan.id,
        project_arg,
        cwd_arg,
        target_project_arg,
        entry,
    )
    .await;
    if let evidence_collector::AppendOutcome::Failed { error } = &outcome {
        // Evidence append failure does not abort the dispatch (the inner
        // tool already succeeded with its own durable side effects), but
        // we now surface the error in the response so callers cannot
        // mistake a missing sidecar for a clean run. This also covers
        // resolver failures (project root unresolved / relative cwd
        // rejected) — those bubble up as `evidence_error` rather than
        // silently landing under the daemon process cwd.
        tracing::warn!(plan_id = %plan.id, error = %error, "plan-runner: evidence sidecar append failed");
    }
    let (evidence_path, evidence_error) = outcome.into_legacy_tuple();

    let status_update_error = if matches!(plan.status, PlanStatus::Executing) {
        // Already in Executing — nothing to update, nothing can fail.
        None
    } else {
        match state
            .store
            .plan_update_status(plan.id, PlanStatus::Executing)
            .await
        {
            Ok(_) => None,
            Err(e) => {
                tracing::warn!(plan_id = %plan.id, error = %e, "plan-runner: failed to transition plan to executing");
                Some(e.to_string())
            }
        }
    };

    let mut response = build_internal_dispatch_success_response(
        plan,
        resolved,
        inner_payload,
        evidence_path,
        evidence_error,
        status_update_error,
        &dispatch_decision,
        &emission,
    );
    attach_session_trace_response_fields(
        &mut response,
        resolved_trace_path.as_deref(),
        trace_path_warning.as_deref(),
    );
    Ok(response)
}
