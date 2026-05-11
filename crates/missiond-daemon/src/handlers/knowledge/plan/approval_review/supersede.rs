use super::*;

pub(in crate::handlers::knowledge::plan) async fn action_supersede(
    state: &AppState,
    args: &Value,
) -> Result<ToolResult> {
    let old_id = parse_id_arg(args, "old_plan_id")?;
    let new_id = parse_id_arg(args, "new_plan_id")?;

    let automation_policy = parse_review_automation_policy(args);
    let automation_explicit = review_automation_policy_was_explicit(args);

    // wave-21 / task 06 :: parse the propose-only `auto_approve_mode`
    // knob up-front. Supersede is destructive — the proposer ALWAYS
    // surfaces `destructive_blocked`.
    let proposer_mode = match parse_plan_proposer_mode_or_error(args) {
        Ok(m) => m,
        Err(e) => return Ok(ToolResult::structured_error(e)),
    };

    // wave-22 / task 03 :: parse the apply-gate input up-front.
    // supersede is destructive (invariant I2) — the gate ALWAYS skips
    // with `SkippedDestructiveAction`. Strict shape errors still
    // surface as INVALID_PARAM here.
    let apply_gate_input = match parse_llm_approve_apply_gate_input(args) {
        Ok(i) => i,
        Err((code, msg)) => return Ok(ToolResult::structured_error(ToolError::new(code, msg))),
    };

    // wave-15 :: explicit resolution bridge. Supersede pivots two plan
    // ids; the review envelope is anchored to `old_plan_id` (the artifact
    // being closed out by the supersede). `Rejected` / `NeedsChanges` skip
    // the supersede entirely.
    //
    // wave-18 / task 07 :: supersede is destructive (the old plan goes
    // to Superseded). We never auto-promote it from a policy — the
    // policy-only branch surfaces the suggestion and refuses to mutate.
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
            return plan_action_supersede_with_policy_only(
                state,
                old_id,
                new_id,
                qid,
                automation_policy,
                proposer_mode,
                apply_gate_input,
            )
            .await;
        }
    };

    if let Some(input) = resolution {
        return action_supersede_with_resolution(
            state,
            old_id,
            new_id,
            input,
            automation_policy,
            automation_explicit,
            proposer_mode,
            apply_gate_input,
        )
        .await;
    }

    // The store method performs the PlanStatus::Superseded transition.
    state
        .store
        .plan_supersede(old_id, new_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let mut payload = json!({
        "status": "superseded",
        "old_plan_id": old_id,
        "new_plan_id": new_id,
    });
    let qid = parse_resolution_review_question_id(args);
    maybe_emit_review_question_resolved(
        &mut payload,
        &state.bus,
        qid.as_deref(),
        "superseded",
        None,
    )
    .await;
    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            "legacy_quiet",
            "manual",
            false,
            Some(("new_plan_id", json!(new_id))),
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "supersede",
            &old_id,
            0,
            &summary,
            None,
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: supersede is destructive — gate ALWAYS
        // surfaces `skipped_destructive_action` (invariant I2).
        let _ = attach_plan_apply_gate_block(&mut payload, &bundle, &apply_gate_input, &old_id, 0);
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-15 explicit resolution bridge for `action=supersede`. Validates
/// the review envelope against the OLD plan (the artifact being closed),
/// then performs the supersede transition only when the decision is
/// `approved`.
///
/// wave-18 / task 07 :: stamps the automation outcome on the response.
/// Caller-supplied `review_decision` always wins.
async fn action_supersede_with_resolution(
    state: &AppState,
    old_id: uuid::Uuid,
    new_id: uuid::Uuid,
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
        .plan_get(old_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("old plan `{}` not found for resolution", old_id),
            )));
        }
    };
    let current_version = plan.version;
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "plan",
        &old_id.to_string(),
        current_version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
    }

    let mut payload = json!({
        "old_plan_id": old_id,
        "new_plan_id": new_id,
        "version": current_version,
    });

    match input.decision.outcome() {
        ResolutionOutcome::PerformTransition => {
            // The store method performs the PlanStatus::Superseded transition.
            state
                .store
                .plan_supersede(old_id, new_id)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["status"] = json!("superseded");
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
    if let Some(mode) = proposer_mode {
        // supersede is destructive — proposer ALWAYS surfaces
        // `destructive_blocked` (invariant I2).
        let summary = plan_proposer_summary(
            &automation_status_label,
            automation_policy.as_str(),
            true,
            Some(("new_plan_id", json!(new_id))),
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "supersede",
            &old_id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: supersede is destructive — gate ALWAYS
        // surfaces `skipped_destructive_action` (invariant I2).
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &old_id,
            current_version,
        );
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-18 / task 07 :: policy-driven supersede path. Supersede is
/// destructive (the old plan goes to `Superseded`); we never auto-promote
/// from a policy. Surfaces the suggestion only and refuses to mutate.
async fn plan_action_supersede_with_policy_only(
    state: &AppState,
    old_id: uuid::Uuid,
    new_id: uuid::Uuid,
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
        .plan_get(old_id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?
    {
        Some(p) => p,
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("old plan `{}` not found for resolution", old_id),
            )));
        }
    };
    let current_version = plan.version;
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "plan",
        &old_id.to_string(),
        current_version,
        PLAN_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
    }

    let mut payload = json!({
        "old_plan_id": old_id,
        "new_plan_id": new_id,
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
    let mut ctx = build_plan_automation_ctx(&args_v, plan.compiler_model.as_deref());
    ctx.additional_blockers.push(
        "destructive_action:supersede transitions are never auto-promoted by the automation policy"
            .to_string(),
    );
    let outcome = evaluate_review_automation(automation_policy, &ctx, None);

    payload["status"] = json!("review_pending_decision");
    payload["next_step"] = json!(
        "supply explicit `review_decision` (approved|rejected|needs_changes) — supersede is destructive and never auto-promoted"
    );

    stamp_review_automation_payload(&mut payload, &outcome);

    if let Some(mode) = proposer_mode {
        let summary = plan_proposer_summary(
            outcome.status.as_str(),
            automation_policy.as_str(),
            false,
            Some(("new_plan_id", json!(new_id))),
        );
        let bundle = request_plan_auto_approve_proposal(
            state,
            mode,
            "supersede",
            &old_id,
            current_version,
            &summary,
            Some(&plan.sexp_text),
        )
        .await;
        attach_plan_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: supersede is destructive — gate ALWAYS
        // surfaces `skipped_destructive_action` (invariant I2).
        let _ = attach_plan_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &old_id,
            current_version,
        );
    }

    Ok(ToolResult::json_pretty(&payload))
}
