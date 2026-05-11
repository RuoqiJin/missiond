use super::*;

pub(in crate::handlers::knowledge::plan) async fn action_approve(
    state: &AppState,
    args: &Value,
) -> Result<ToolResult> {
    let id = parse_id_arg(args, "plan_id")?;

    let automation_policy = parse_review_automation_policy(args);
    let automation_explicit = review_automation_policy_was_explicit(args);

    // wave-21 / task 06 :: parse the propose-only `auto_approve_mode`
    // knob up-front so caller typos surface as INVALID_PARAM BEFORE any
    // DB read.
    let proposer_mode = match parse_plan_proposer_mode_or_error(args) {
        Ok(m) => m,
        Err(e) => return Ok(ToolResult::structured_error(e)),
    };

    // wave-22 / task 03 :: parse the apply-gate input up-front. Strict
    // shape errors fail-fast as INVALID_PARAM BEFORE any DB read.
    let apply_gate_input = match parse_llm_approve_apply_gate_input(args) {
        Ok(i) => i,
        Err((code, msg)) => return Ok(ToolResult::structured_error(ToolError::new(code, msg))),
    };

    // wave-15 :: explicit resolution bridge. When the caller supplies
    // `review_question_id` + `review_decision` we validate the envelope
    // BEFORE mutating plan state. `Rejected` / `NeedsChanges` skip the
    // approve transition entirely; `Approved` proceeds with the existing
    // `plan_update_status(Approved)` call.
    //
    // wave-18 / task 07 :: when a non-Manual `review_automation_policy`
    // is supplied without an explicit `review_decision` (which would
    // otherwise fail-fast with MISSING_PARAM), promote the qid into a
    // policy-driven evaluation path. Caller-supplied decisions ALWAYS
    // win over the policy.
    let resolution = match parse_review_resolution_input(args) {
        Ok(r) => r,
        Err(e) => {
            if matches!(automation_policy, ReviewAutomationPolicy::Manual)
                || !matches!(
                    e,
                    crate::handlers::knowledge::review_gate::ResolutionInputError::MissingDecision
                )
            {
                return Ok(ToolResult::structured_error(ToolError::new(
                    e.code(),
                    e.message(),
                )));
            }
            let qid = parse_resolution_review_question_id(args)
                .expect("MissingDecision implies qid was present");
            return plan_action_approve_with_policy_only(
                state,
                id,
                qid,
                automation_policy,
                proposer_mode,
                apply_gate_input,
            )
            .await;
        }
    };

    if let Some(input) = resolution {
        return action_approve_with_resolution(
            state,
            id,
            input,
            automation_policy,
            automation_explicit,
            proposer_mode,
            apply_gate_input,
        )
        .await;
    }

    // wave-22 / task 03 :: when caller opted into the apply gate, the
    // legacy unconditional `plan_update_status(Approved)` is INVERTED —
    // the DB transition is gated on the LLM proposal passing all 6
    // strict gates. See directive.rs::action_approve for the full
    // design rationale (mirrored here for the plan surface).
    if apply_gate_input.apply {
        // We need the current plan version so the proposal hash is
        // computed against the head. Source it from the store.
        let plan = match state
            .store
            .plan_get(id)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?
        {
            Some(p) => p,
            None => {
                return Ok(ToolResult::structured_error(ToolError::new(
                    error_codes::NOT_FOUND,
                    format!("plan `{}` not found for apply gate", id),
                )));
            }
        };
        let resolved_mode = proposer_mode.unwrap_or(LlmAutoApproveProposalMode::Off);
        let summary = plan_proposer_summary("legacy_quiet", "manual", false, None);
        let bundle = request_plan_auto_approve_proposal(
            state,
            resolved_mode,
            "approve",
            &id,
            plan.version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        if let Err((code, msg)) = enforce_apply_gate_preflight(
            &apply_gate_input,
            &bundle,
            "approve",
            &id.to_string(),
            plan.version,
        ) {
            return Ok(ToolResult::structured_error(ToolError::new(code, msg)));
        }
        let mut payload = json!({
            "plan_id": id,
            "version": plan.version,
        });
        attach_plan_proposal_block(&mut payload, &bundle);
        let outcome = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            plan.version,
        );
        if outcome.status.should_apply() {
            state
                .store
                .plan_update_status(id, PlanStatus::Approved)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["status"] = json!("approved");
            payload["resolution_source"] = json!("llm_approve_apply_gate");
            let qid = parse_resolution_review_question_id(args);
            maybe_emit_review_question_resolved(
                &mut payload,
                &state.bus,
                qid.as_deref(),
                "approved",
                None,
            )
            .await;
        } else {
            payload["status"] = json!("llm_auto_apply_skipped");
            payload["next_step"] = json!(format!(
                "apply gate did not authorise (status={}); supply explicit `review_decision=approved` to flip the plan manually OR re-run with a matching proposal_hash + caller_approved=true",
                outcome.status.as_str()
            ));
        }
        return Ok(ToolResult::json_pretty(&payload));
    }

    state
        .store
        .plan_update_status(id, PlanStatus::Approved)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let mut payload = json!({
        "status": "approved",
        "plan_id": id,
    });
    // wave-11/14 quiet emit path — kept for callers that fire a Resolved
    // event without the wave-15 decision-bearing envelope.
    let qid = parse_resolution_review_question_id(args);
    maybe_emit_review_question_resolved(&mut payload, &state.bus, qid.as_deref(), "approved", None)
        .await;
    // wave-21 / task 06 :: propose-only Sonnet pass on the legacy path.
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary("legacy_quiet", "manual", false, None);
        let bundle =
            request_plan_auto_approve_proposal(state, mode, "approve", &id, 0, &summary, None)
                .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: stamp the proposal hash so callers can
        // echo it back via `proposal_hash` under
        // `apply_llm_auto_approve=true` on a follow-up call.
        stamp_proposal_hash_payload(&mut payload, &bundle, "approve", &id.to_string(), 0);
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-15 explicit resolution bridge for `action=approve`. Validates the
/// review envelope (scope / artifact / version / action) against the
/// current plan row, then performs the manager transition only when the
/// decision is `approved`.
///
/// wave-18 / task 07 :: also evaluates the deterministic
/// `review_automation_policy` and stamps the suggestion / status onto
/// the response payload. Caller-supplied `review_decision` ALWAYS wins.
async fn action_approve_with_resolution(
    state: &AppState,
    id: uuid::Uuid,
    input: ReviewResolutionInput,
    automation_policy: ReviewAutomationPolicy,
    automation_explicit: bool,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&input.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                "REVIEW_ID_MALFORMED",
                e.message(),
            )));
        }
    };
    let plan = match state
        .store
        .plan_get(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("plan `{}` not found for resolution", id),
            )));
        }
    };
    let current_version = plan.version;
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "plan",
        &id.to_string(),
        current_version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
    }

    let mut payload = json!({
        "plan_id": id,
        "version": current_version,
    });

    match input.decision.outcome() {
        ResolutionOutcome::PerformTransition => {
            state
                .store
                .plan_update_status(id, PlanStatus::Approved)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["status"] = json!("approved");
        }
        ResolutionOutcome::KeepArtifact => {
            payload["status"] = json!("review_rejected");
        }
        ResolutionOutcome::RequestChanges => {
            payload["status"] = json!("review_needs_changes");
            stamp_needs_changes_next_step(&mut payload, "plan", "compile");
        }
    }

    stamp_resolution_payload(&mut payload, &input);

    let mut automation_status_label = "not_evaluated".to_string();
    if automation_explicit || !matches!(automation_policy, ReviewAutomationPolicy::Manual) {
        let mut args_v = json!({});
        if let Some(map) = args_v.as_object_mut() {
            map.insert(
                "review_automation_policy".into(),
                json!(automation_policy.as_str()),
            );
        }
        let ctx = build_plan_automation_ctx(&args_v, plan.compiler_model.as_deref());
        let outcome = evaluate_review_automation(automation_policy, &ctx, Some(input.decision));
        automation_status_label = outcome.status.as_str().to_string();
        stamp_review_automation_payload(&mut payload, &outcome);
    }

    let resolution_str = resolution_wire_string(input.decision);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        Some(&input.question_id),
        resolution_str,
        None,
    )
    .await;
    // wave-21 / task 06 :: propose-only Sonnet pass for the explicit-
    // resolution path. Caller decision ALWAYS wins; proposal is
    // informational only.
    //
    // wave-22 / task 03 :: apply gate is INFORMATIONAL ONLY here. The
    // explicit `review_decision` already drove (or refused) the DB
    // transition above. We do NOT fail-fast on hash mismatch — that
    // would lie about state. The gate block surfaces the verdict for
    // audit symmetry.
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            &automation_status_label,
            automation_policy.as_str(),
            true,
            None,
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "approve",
            &id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            current_version,
        );
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-18 / task 07 :: policy-driven approve path for `mission_plan`.
/// Fires when caller supplies `review_question_id` +
/// `review_automation_policy` (non-Manual) WITHOUT an explicit
/// `review_decision`. Auto-promotes to `Approved` only under `auto_safe`
/// + every safety rule passing. NEVER auto-rejects.
async fn plan_action_approve_with_policy_only(
    state: &AppState,
    id: uuid::Uuid,
    qid: String,
    automation_policy: ReviewAutomationPolicy,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    let parsed = match parse_review_question_id_struct(&qid) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                "REVIEW_ID_MALFORMED",
                e.message(),
            )));
        }
    };
    let plan = match state
        .store
        .plan_get(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("plan `{}` not found for resolution", id),
            )));
        }
    };
    let current_version = plan.version;
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "plan",
        &id.to_string(),
        current_version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
    }

    let mut payload = json!({
        "plan_id": id,
        "version": current_version,
        "review_question_id": qid,
    });

    let mut args_v = json!({});
    if let Some(map) = args_v.as_object_mut() {
        map.insert(
            "review_automation_policy".into(),
            json!(automation_policy.as_str()),
        );
    }
    let ctx = build_plan_automation_ctx(&args_v, plan.compiler_model.as_deref());
    let outcome = evaluate_review_automation(automation_policy, &ctx, None);

    if outcome.may_auto_resolve {
        state
            .store
            .plan_update_status(id, PlanStatus::Approved)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        payload["status"] = json!("approved");
        payload["resolution_source"] = json!("review_automation_policy");
    } else {
        payload["status"] = json!("review_pending_decision");
        if matches!(outcome.status, AutomationStatus::AutoSafeBlocked) {
            payload["next_step"] = json!(
                "auto_safe blocked — supply explicit `review_decision` (approved|rejected|needs_changes) to flip the plan"
            );
        } else {
            payload["next_step"] = json!(
                "suggest mode is informational — supply explicit `review_decision` to flip the plan"
            );
        }
    }

    stamp_review_automation_payload(&mut payload, &outcome);

    if outcome.may_auto_resolve {
        maybe_emit_review_question_resolved(&mut payload, &state.bus, Some(&qid), "approved", None)
            .await;
    }

    // wave-21 / task 06 :: propose-only Sonnet pass on the policy-only
    // approve path.
    //
    // wave-22 / task 03 :: apply gate is INFORMATIONAL ONLY on this
    // path — the deterministic policy already drove the DB transition.
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            outcome.status.as_str(),
            automation_policy.as_str(),
            false,
            None,
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "approve",
            &id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            current_version,
        );
    }

    Ok(ToolResult::json_pretty(&payload))
}
