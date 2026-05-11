use super::*;

pub(in crate::handlers::knowledge::plan) async fn action_mark(
    state: &AppState,
    args: &Value,
) -> Result<ToolResult> {
    let id = parse_id_arg(args, "plan_id")?;
    let target_raw = require_str(args, "status")?;
    let target = PlanStatus::from_str(target_raw).map_err(|e| {
        anyhow!(
            "`{}` is not a valid PlanStatus: {} (valid: draft|awaiting_approval|approved|executing|succeeded|failed|superseded)",
            target_raw,
            e
        )
    })?;

    let automation_policy = parse_review_automation_policy(args);
    let automation_explicit = review_automation_policy_was_explicit(args);

    // wave-21 / task 06 :: parse the propose-only `auto_approve_mode`
    // knob up-front.
    let proposer_mode = match parse_plan_proposer_mode_or_error(args) {
        Ok(m) => m,
        Err(e) => return Ok(ToolResult::structured_error(e)),
    };

    // wave-22 / task 03 :: parse the apply-gate input up-front. mark is
    // a general state-transition action; the gate only authorises
    // mark-to-approved (mirrors the wave-18 policy posture). For other
    // target statuses the gate falls through to a SKIP outcome.
    let apply_gate_input = match parse_llm_approve_apply_gate_input(args) {
        Ok(i) => i,
        Err((code, msg)) => return Ok(ToolResult::structured_error(ToolError::new(code, msg))),
    };

    // wave-15 :: explicit resolution bridge — same pattern as approve.
    // wave-18 / task 07 :: same MissingDecision-under-policy promotion.
    // mark is the most general state transition (caller picks the target
    // status) — so the policy can only auto-promote when the requested
    // target is `approved`. Other targets surface the suggestion only.
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
            return plan_action_mark_with_policy_only(
                state,
                id,
                target,
                qid,
                automation_policy,
                proposer_mode,
                apply_gate_input,
            )
            .await;
        }
    };

    if let Some(input) = resolution {
        return action_mark_with_resolution(
            state,
            id,
            target,
            input,
            automation_policy,
            automation_explicit,
            proposer_mode,
            apply_gate_input,
        )
        .await;
    }

    state
        .store
        .plan_update_status(id, target)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let mut payload = json!({
        "plan_id": id,
        "new_status": target.as_str(),
    });
    let qid = parse_resolution_review_question_id(args);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        qid.as_deref(),
        target.as_str(),
        None,
    )
    .await;
    // wave-21 / task 06 :: propose-only Sonnet pass for the legacy mark
    // path. The requested target is surfaced to the prompt for context.
    //
    // wave-22 / task 03 :: apply gate is informational on this path —
    // the legacy mark already ran the requested transition above. For
    // a future wave that wants to gate mark-to-approved on the LLM
    // proposal, the gate block is the audit anchor.
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            "legacy_quiet",
            "manual",
            false,
            Some(("requested_status", json!(target.as_str()))),
        );
        let bundle =
            request_plan_auto_approve_proposal(state, mode, "mark", &id, 0, &summary, None).await;
        attach_plan_proposal_block(&mut payload, &bundle);
        let _ = attach_plan_apply_gate_block(&mut payload, &bundle, &apply_gate_input, &id, 0);
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-15 explicit resolution bridge for `action=mark`. Validates the
/// review envelope; on `approved` decision performs the requested
/// `plan_update_status` transition; on `rejected`/`needs_changes` keeps
/// the plan at its current status.
///
/// wave-18 / task 07 :: stamps the automation outcome on the response.
/// Caller-supplied `review_decision` always wins.
async fn action_mark_with_resolution(
    state: &AppState,
    id: uuid::Uuid,
    target: PlanStatus,
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
        "requested_status": target.as_str(),
    });

    match input.decision.outcome() {
        ResolutionOutcome::PerformTransition => {
            state
                .store
                .plan_update_status(id, target)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["new_status"] = json!(target.as_str());
        }
        ResolutionOutcome::KeepArtifact => {
            payload["new_status"] = json!(plan.status.as_str());
            payload["status"] = json!("review_rejected");
        }
        ResolutionOutcome::RequestChanges => {
            payload["new_status"] = json!(plan.status.as_str());
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
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            &automation_status_label,
            automation_policy.as_str(),
            true,
            Some(("requested_status", json!(target.as_str()))),
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "mark",
            &id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: gate is informational — caller decision
        // already drove the transition above.
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

/// Wave-18 / task 07 :: policy-driven mark path. Auto-promotes ONLY
/// when the requested target status is `Approved` (the only safe
/// auto-resolution outcome for `mark`); other targets degrade to
/// suggest-only even when every safety rule passes.
async fn plan_action_mark_with_policy_only(
    state: &AppState,
    id: uuid::Uuid,
    target: PlanStatus,
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
        "requested_status": target.as_str(),
    });

    let mut args_v = json!({});
    if let Some(map) = args_v.as_object_mut() {
        map.insert(
            "review_automation_policy".into(),
            json!(automation_policy.as_str()),
        );
    }
    let mut ctx = build_plan_automation_ctx(&args_v, plan.compiler_model.as_deref());
    if !matches!(target, PlanStatus::Approved) {
        // `mark` to a non-Approved target is never auto-promoted by the
        // policy. We pin a loud blocker so the audit trail explains the
        // refusal even when every safety rule otherwise passes.
        ctx.additional_blockers.push(format!(
            "non_approved_target:mark target `{}` is never auto-promoted by review_automation_policy (only `approved` is)",
            target.as_str()
        ));
    }
    let outcome = evaluate_review_automation(automation_policy, &ctx, None);

    if outcome.may_auto_resolve && matches!(target, PlanStatus::Approved) {
        state
            .store
            .plan_update_status(id, PlanStatus::Approved)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        payload["new_status"] = json!(PlanStatus::Approved.as_str());
        payload["status"] = json!("approved");
        payload["resolution_source"] = json!("review_automation_policy");
    } else {
        payload["new_status"] = json!(plan.status.as_str());
        payload["status"] = json!("review_pending_decision");
        payload["next_step"] = json!(
            "supply explicit `review_decision` (approved|rejected|needs_changes) to finalise the mark"
        );
    }

    stamp_review_automation_payload(&mut payload, &outcome);

    if outcome.may_auto_resolve && matches!(target, PlanStatus::Approved) {
        maybe_emit_review_question_resolved(&mut payload, &state.bus, Some(&qid), "approved", None)
            .await;
    }

    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            outcome.status.as_str(),
            automation_policy.as_str(),
            false,
            Some(("requested_status", json!(target.as_str()))),
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "mark",
            &id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: gate is informational on policy path —
        // deterministic policy already drove the transition.
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
