use super::super::workstation_dispatch;
use super::*;

// ── wave-23 / task 05 — session-trace propagation helpers ──────────────
//
// `mission_plan(action=execute)` callers can opt into the wave-23 / task 04
// session-trace ledger by supplying `session_trace_path`. The plan-runner
// validates basic shape up-front (so a typo cannot silently shadow the
// ledger) and then forwards the path through three surfaces:
//   * `mission_execution(action=*)` inner args (when target=mission_execution)
//   * the workstation-dispatch task brief (a `## Session trace` block)
//   * the wave-19 / task 06 emitted task-contract v1 file
//     (`:session-trace-path "..."`)
// On the response, every return path surfaces `session_trace_path` so
// observers can pin which ledger this dispatch was wired to (or
// `trace_path_warning` when shape validation degraded silently because
// the caller did not opt into hard-fail via `session_trace_required`).

/// Validate the optional `session_trace_path` arg shape. Returns
/// `(resolved_path, warning)`:
///   * Both `None` ⇒ caller did not opt in; propagation is suppressed.
///   * `(Some(path), None)` ⇒ path passed shape validation; forward it
///     verbatim through the dispatch and surface it on the response.
///   * `(None, Some(warning))` ⇒ shape failed AND `required=false`; no
///     forward, surface a non-fatal warning so the caller can fix and
///     retry without aborting the dispatch.
///   * `Err(structured_error)` ⇒ shape failed AND `required=true`; the
///     caller asked the daemon to refuse the dispatch on a malformed
///     path so a typo cannot silently shadow the ledger.
///
/// Validation is intentionally NARROW — we only check the input shape,
/// never the on-disk file existence. The wave-23 / task 04 consumer
/// surfaces `trace_warning` for I/O / parse / append failures; the two
/// surfaces are distinct so observers can tell shape errors (caller
/// typo) from append errors (target file removed mid-flight).
pub(super) fn validate_session_trace_path_arg(
    raw: Option<&str>,
    required: bool,
) -> std::result::Result<(Option<String>, Option<String>), ToolResult> {
    let Some(value) = raw else {
        return Ok((None, None));
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        let detail = "session_trace_path is empty after trim".to_string();
        return reject_or_warn_trace_path(detail, required);
    }
    // NUL byte and ASCII control char rejection. Tab is allowed since
    // some build systems tolerate path components with whitespace; we
    // only reject characters that would fail filesystem normalization
    // or render the path unreadable.
    for (idx, ch) in trimmed.char_indices() {
        if ch == '\0' {
            let detail = format!(
                "session_trace_path contains a NUL byte at offset {} (filesystem-invalid)",
                idx
            );
            return reject_or_warn_trace_path(detail, required);
        }
        if ch.is_ascii_control() && ch != ' ' && ch != '\t' {
            let detail = format!(
                "session_trace_path contains ASCII control char `{:#04x}` at offset {} (filesystem-invalid)",
                ch as u32, idx
            );
            return reject_or_warn_trace_path(detail, required);
        }
    }
    Ok((Some(trimmed.to_string()), None))
}

/// `validate_session_trace_path_arg` companion that branches between
/// hard-fail (when `required=true`) and warn-only (the conservative
/// default).
fn reject_or_warn_trace_path(
    detail: String,
    required: bool,
) -> std::result::Result<(Option<String>, Option<String>), ToolResult> {
    if required {
        Err(ToolResult::structured_error(
            ToolError::new(error_codes::INVALID_PARAM, detail.clone()).with_suggestion(
                "session_trace_required=true forbids malformed `session_trace_path` shapes — \
                     supply a non-empty filesystem-valid path (relative or absolute) or drop \
                     `session_trace_required` to fall back to a non-fatal warning.",
            ),
        ))
    } else {
        Ok((None, Some(detail)))
    }
}

/// Splice `session_trace_path` and / or `trace_path_warning` into the
/// JSON envelope of a `ToolResult` produced by an `action_execute_internal`
/// return path. When both inputs are `None` the response is left
/// byte-identical to the wave-15..22 baseline — preserves backward
/// compatibility for callers that never supplied the trace knob.
pub(super) fn attach_session_trace_response_fields(
    result: &mut ToolResult,
    session_trace_path: Option<&str>,
    trace_path_warning: Option<&str>,
) {
    if session_trace_path.is_none() && trace_path_warning.is_none() {
        return;
    }
    // The inner JSON lives under the first ToolContent::Text frame
    // (json_pretty / structured-error patterns). We splice in place so
    // the rest of the envelope (is_error, structured_content) stays
    // unchanged.
    let Some(ToolContent::Text { text }) = result.content.first_mut() else {
        return;
    };
    let Ok(mut value) = serde_json::from_str::<Value>(text) else {
        return;
    };
    if let Some(map) = value.as_object_mut() {
        if let Some(stp) = session_trace_path {
            map.insert("session_trace_path".to_string(), json!(stp));
        }
        if let Some(w) = trace_path_warning {
            map.insert("trace_path_warning".to_string(), json!(w));
        }
    }
    *text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.clone());
}

/// Merge the wave-19 / task 06 task-contract emission record into a
/// response payload. No-op when the emitter was off and produced
/// nothing observable — preserves the pre-wave19 byte-shape on the
/// default code path.
pub(crate) fn merge_task_contract_block(
    payload: &mut Value,
    emission: &TaskContractEmissionRecord,
) {
    let Some(block) = emission.to_response_block() else {
        return;
    };
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    if let Value::Object(block_map) = block {
        for (k, v) in block_map {
            map.insert(k, v);
        }
    }
}

/// wave-19 / task 06 — response shape when task-contract emission was
/// requested but the write failed. We refuse the dispatch entirely so
/// downstream callers cannot mistake a missing contract for a
/// successful run; plan FSM is untouched, no inner side effect was
/// produced, the response carries the structured emission record.
pub(super) fn build_task_contract_failure_response(
    plan: &Plan,
    resolved: &ResolvedExec,
    decision: &workstation_dispatch::DispatchDecision,
    emission: &TaskContractEmissionRecord,
) -> ToolResult {
    let mut payload = json!({
        "status": "dispatch_skipped",
        "execute_mode": "internal",
        "runner_status": "task_contract_emit_failed",
        "plan_id": plan.id,
        "board_task_id": plan.board_task_id,
        "target_tool": resolved.target,
        "target_source": resolved.target_source,
        "dispatch_strategy": resolved.dispatch_strategy,
        "dispatch_strategy_source": resolved.dispatch_strategy_source,
        "plan_hint_summary": resolved.plan_hint_summary,
        "workstation_dispatch_source": decision.source.as_str(),
    });
    if let Some(reason) = decision.reason.as_deref() {
        payload["workstation_dispatch_inference_reason"] = json!(reason);
    }
    merge_task_contract_block(&mut payload, emission);
    ToolResult::json_pretty(&payload)
}

/// wave-19 / task 06 — response shape when the caller asked for
/// `task_contract_mode="emit_dry_run"`. The contract is on disk; the
/// inner substrate is never invoked. Plan FSM is untouched (the
/// caller can flip to `emit` mode for a real dispatch).
pub(super) fn build_task_contract_dry_run_response(
    plan: &Plan,
    resolved: &ResolvedExec,
    decision: &workstation_dispatch::DispatchDecision,
    emission: &TaskContractEmissionRecord,
) -> ToolResult {
    let mut payload = json!({
        "status": "dry_run",
        "execute_mode": "internal",
        "runner_status": "task_contract_emit_dry_run",
        "plan_id": plan.id,
        "board_task_id": plan.board_task_id,
        "target_tool": resolved.target,
        "target_source": resolved.target_source,
        "dispatch_strategy": resolved.dispatch_strategy,
        "dispatch_strategy_source": resolved.dispatch_strategy_source,
        "plan_hint_summary": resolved.plan_hint_summary,
        "workstation_dispatch_source": decision.source.as_str(),
    });
    if let Some(reason) = decision.reason.as_deref() {
        payload["workstation_dispatch_inference_reason"] = json!(reason);
    }
    merge_task_contract_block(&mut payload, emission);
    ToolResult::json_pretty(&payload)
}

/// Render a workstation-dispatch outcome into the same response envelope
/// shape as `build_internal_dispatch_success_response` so callers see one
/// consistent contract (plan-runner v0 fields + workstation-dispatch
/// extension fields side-by-side).
///
/// Status semantics:
///   * `Dispatched`         → "executing" (plan transitions to executing)
///   * `InnerError`         → "dispatch_failed" (do not transition)
///   * `DryRun`             → "dry_run"
///   * `SafeDescriptor`     → "dispatch_skipped" (do not transition)
///
/// When `Dispatched`, this function does NOT itself update the plan
/// status — the caller (action_execute_internal) handles that, mirroring
/// the legacy success-response path. The status field is set so the wire
/// shape matches the legacy executing branch.
pub(super) fn build_workstation_dispatch_response(
    plan: &Plan,
    resolved: &ResolvedExec,
    outcome: workstation_dispatch::WorkstationDispatchOutcome,
    decision: &workstation_dispatch::DispatchDecision,
    emission: &TaskContractEmissionRecord,
    dispatch_contract_mode: DispatchContractMode,
) -> ToolResult {
    let status = match &outcome {
        workstation_dispatch::WorkstationDispatchOutcome::Dispatched { .. } => "executing",
        workstation_dispatch::WorkstationDispatchOutcome::InnerError { .. } => "dispatch_failed",
        workstation_dispatch::WorkstationDispatchOutcome::DryRun { .. } => "dry_run",
        workstation_dispatch::WorkstationDispatchOutcome::SafeDescriptor { .. } => {
            "dispatch_skipped"
        }
    };
    let extension =
        workstation_dispatch::outcome_to_response_fields(&outcome, resolved.dispatch_strategy);

    let mut payload = json!({
        "status": status,
        "execute_mode": "internal",
        "runner_status": "workstation_dispatch_v0",
        "plan_id": plan.id,
        "board_task_id": plan.board_task_id,
        "target_tool": resolved.target,
        "target_source": resolved.target_source,
        "dispatch_strategy": resolved.dispatch_strategy,
        "dispatch_strategy_source": resolved.dispatch_strategy_source,
        "plan_hint_summary": resolved.plan_hint_summary,
        // wave-16 / task 03 — surface the routing decision so callers can
        // tell apart explicit opt-in (wave-15) from auto-inference (wave-16).
        "workstation_dispatch_source": decision.source.as_str(),
        // wave-20 / task 04 — surface the resolved dispatch-contract
        // mode so observers can pin which dispatch contract drove the
        // brief. `rendered` (default) preserves wave-15..19 byte-shape;
        // `machine` proves the consumer read the on-disk Lisp SSOT
        // (cross-check against `task_contract_source_path` on the
        // workstation extension when present).
        "dispatch_contract_mode": dispatch_contract_mode.as_str(),
    });
    if let Some(reason) = decision.reason.as_deref() {
        if let Some(map) = payload.as_object_mut() {
            map.insert(
                "workstation_dispatch_inference_reason".to_string(),
                json!(reason),
            );
        }
    }
    if let Some(map) = extension.as_object() {
        if let Some(payload_map) = payload.as_object_mut() {
            for (k, v) in map {
                payload_map.insert(k.clone(), v.clone());
            }
        }
    }
    merge_task_contract_block(&mut payload, emission);
    ToolResult::json_pretty(&payload)
}

/// Build the response for a plan-runner internal dispatch where the inner
/// tool already returned non-error.
///
/// Status semantics:
///   * `status_update_error.is_some()` → `status="dispatch_partial"` /
///     `runner_status="status_update_failed"`. We must NOT claim
///     `executing`, because the plan FSM did not actually persist.
///   * otherwise → `status="executing"` / `runner_status="dispatched"`.
///
/// `evidence_error` is independent: a missing sidecar still leaves the inner
/// side effect in place, so it is reported via `evidence_error` but does not
/// downgrade the runner status by itself. Caller may still observe both
/// `evidence_error` and `status_update_error` together.
pub(super) fn build_internal_dispatch_success_response(
    plan: &Plan,
    resolved: &ResolvedExec,
    inner_payload: Value,
    evidence_path: Option<String>,
    evidence_error: Option<String>,
    status_update_error: Option<String>,
    decision: &workstation_dispatch::DispatchDecision,
    emission: &TaskContractEmissionRecord,
) -> ToolResult {
    let (status, runner_status) = if status_update_error.is_some() {
        ("dispatch_partial", "status_update_failed")
    } else {
        ("executing", "dispatched")
    };

    let mut payload = json!({
        "status": status,
        "execute_mode": "internal",
        "runner_status": runner_status,
        "plan_id": plan.id,
        "board_task_id": plan.board_task_id,
        "target_tool": resolved.target,
        "target_source": resolved.target_source,
        "dispatch_strategy": resolved.dispatch_strategy,
        "dispatch_strategy_source": resolved.dispatch_strategy_source,
        "plan_hint_summary": resolved.plan_hint_summary,
        "evidence_path": evidence_path,
        "inner_result": inner_payload,
        "workstation_dispatch_source": decision.source.as_str(),
    });
    if let Some(reason) = decision.reason.as_deref() {
        payload["workstation_dispatch_inference_reason"] = json!(reason);
    }
    if let Some(err) = evidence_error {
        payload["evidence_error"] = json!(err);
    }
    if let Some(err) = status_update_error {
        payload["status_update_error"] = json!(err);
    }
    merge_task_contract_block(&mut payload, emission);
    ToolResult::json_pretty(&payload)
}
