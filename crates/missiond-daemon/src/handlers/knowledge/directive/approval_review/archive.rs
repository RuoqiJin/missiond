use super::*;

pub(in crate::handlers::knowledge::directive) async fn action_archive(
    state: &AppState,
    args: &Value,
) -> Result<ToolResult> {
    let id = parse_id_arg(args, "directive_id")?;
    let version = require_i32(args, "version")?;

    let automation_policy = parse_review_automation_policy(args);
    let automation_explicit = review_automation_policy_was_explicit(args);

    // wave-21 / task 06 :: parse the propose-only `auto_approve_mode`
    // knob up-front. Strict-enum failure surfaces as INVALID_PARAM.
    let proposer_mode = match parse_proposer_mode_or_error(args) {
        Ok(m) => m,
        Err(e) => return Ok(ToolResult::structured_error(e)),
    };

    // wave-22 / task 03 :: parse the apply-gate input up-front. archive
    // is destructive — the gate will ALWAYS skip with
    // `SkippedDestructiveAction` (invariant I2) — but we still parse
    // strict shape so caller typos surface as INVALID_PARAM here too.
    let apply_gate_input = match parse_llm_approve_apply_gate_input(args) {
        Ok(i) => i,
        Err((code, msg)) => return Ok(ToolResult::structured_error(ToolError::new(code, msg))),
    };

    // wave-15 :: explicit resolution bridge — see `action_approve` above.
    // wave-18 / task 07 :: same MissingDecision-under-policy promotion as
    // approve so the policy can drive resolution under archive too.
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
            // archive auto-transition under auto_safe is intentionally
            // NOT supported — archiving is a destructive transition that
            // we never want auto-promoted from a policy. The handler
            // therefore surfaces the suggestion only, never mutates.
            let qid = parse_resolution_review_question_id(args)
                .expect("MissingDecision implies qid was present");
            return action_archive_with_policy_only(
                state,
                id,
                version,
                qid,
                automation_policy,
                proposer_mode,
                apply_gate_input,
            )
            .await;
        }
    };

    if let Some(input) = resolution {
        return action_archive_with_resolution(
            state,
            id,
            version,
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
        .directive_update_status(id, version, DirectiveStatus::Archived)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let mut payload = json!({
        "status": "archived",
        "directive_id": id,
        "version": version,
    });
    let qid = parse_resolution_review_question_id(args);
    maybe_emit_review_question_resolved(&mut payload, &state.bus, qid.as_deref(), "archived", None)
        .await;
    // wave-21 / task 06 :: archive is destructive — the proposer
    // ALWAYS short-circuits to `destructive_blocked` (invariant I2).
    if let Some(mode) = proposer_mode {
        let summary = directive_proposer_summary("legacy_quiet", "manual", false);
        let bundle = request_directive_auto_approve_proposal(
            state, mode, "archive", &id, version, &summary, None,
        )
        .await;
        attach_directive_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: archive is destructive — gate ALWAYS
        // surfaces `skipped_destructive_action` (invariant I2). Stamp
        // the gate block + hash for audit symmetry.
        let _ = attach_directive_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            version,
        );
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-15 explicit resolution bridge for `action=archive`. Same envelope
/// validation as approve; on `approved` decision we perform the archive
/// transition; on `rejected`/`needs_changes` the directive stays at its
/// current status.
///
/// wave-18 / task 07 :: also evaluates the deterministic
/// `review_automation_policy` and stamps the suggestion / status onto
/// the response payload. Caller-supplied `review_decision` ALWAYS wins.
async fn action_archive_with_resolution(
    state: &AppState,
    id: uuid::Uuid,
    version: i32,
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
            )))
        }
    };
    let chain = state
        .store
        .directive_get_version_chain(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let head_directive = match chain.iter().last() {
        Some(d) => d.clone(),
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("directive `{}` not found for resolution", id),
            )))
        }
    };
    let current_version = head_directive.version;
    if version != current_version {
        return Ok(ToolResult::structured_error(ToolError::new(
            "STALE_REVIEW_VERSION",
            format!(
                "archive `version=v{}` does not match directive `{}` head `v{}`",
                version, id, current_version
            ),
        )));
    }
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "directive",
        &id.to_string(),
        current_version,
        DIRECTIVE_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
    }

    let mut payload = json!({
        "directive_id": id,
        "version": version,
    });

    match input.decision.outcome() {
        ResolutionOutcome::PerformTransition => {
            state
                .store
                .directive_update_status(id, version, DirectiveStatus::Archived)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["status"] = json!("archived");
        }
        ResolutionOutcome::KeepArtifact => {
            payload["status"] = json!("review_rejected");
        }
        ResolutionOutcome::RequestChanges => {
            payload["status"] = json!("review_needs_changes");
            stamp_needs_changes_next_step(&mut payload, "directive", "compile");
        }
    }

    stamp_resolution_payload(&mut payload, &input);

    // wave-18 / task 07 :: surface the automation outcome AFTER stamping
    // the explicit decision so observers see both. Skipped under Manual +
    // caller-omitted policy to keep pre-wave-18 callers byte-identical.
    let mut automation_status_label = "not_evaluated".to_string();
    if automation_explicit || !matches!(automation_policy, ReviewAutomationPolicy::Manual) {
        let mut args_v = json!({});
        if let Some(map) = args_v.as_object_mut() {
            map.insert(
                "review_automation_policy".into(),
                json!(automation_policy.as_str()),
            );
        }
        let ctx = build_directive_automation_ctx(&args_v, head_directive.compiler_model.as_deref());
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
    // wave-21 / task 06 :: archive is destructive — proposer ALWAYS
    // surfaces `destructive_blocked` regardless of caller decision.
    if let Some(mode) = proposer_mode {
        let summary =
            directive_proposer_summary(&automation_status_label, automation_policy.as_str(), true);
        let bundle = request_directive_auto_approve_proposal(
            state,
            mode,
            "archive",
            &id,
            version,
            &summary,
            Some(&head_directive.sexp_text),
        )
        .await;
        attach_directive_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: archive is destructive — gate ALWAYS
        // surfaces `skipped_destructive_action` regardless of any other
        // gate field. Caller-supplied `review_decision` is the
        // authority; the gate is informational on this path.
        let _ = attach_directive_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            version,
        );
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-18 / task 07 :: policy-driven archive path. Fires when the
/// caller supplies `review_question_id` + `review_automation_policy`
/// (non-Manual) WITHOUT an explicit `review_decision`.
///
/// IMPORTANT: archive auto-transition is intentionally NEVER promoted
/// under `auto_safe`. Archiving is destructive (the directive cannot
/// flow into plan compile from the archived state); we never want a
/// safety inspector to silently retire a draft. The handler therefore
/// surfaces the suggestion only and refuses to mutate, regardless of
/// whether every safety rule passes.
async fn action_archive_with_policy_only(
    state: &AppState,
    id: uuid::Uuid,
    version: i32,
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
            )))
        }
    };
    let chain = state
        .store
        .directive_get_version_chain(id)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let head_directive = match chain.iter().last() {
        Some(d) => d.clone(),
        None => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::NOT_FOUND,
                format!("directive `{}` not found for resolution", id),
            )))
        }
    };
    let current_version = head_directive.version;
    if version != current_version {
        return Ok(ToolResult::structured_error(ToolError::new(
            "STALE_REVIEW_VERSION",
            format!(
                "archive `version=v{}` does not match directive `{}` head `v{}`",
                version, id, current_version
            ),
        )));
    }
    if let Err(e) = validate_review_resolution_envelope(
        &parsed,
        "directive",
        &id.to_string(),
        current_version,
        DIRECTIVE_REVIEW_ACTIONS,
    ) {
        return Ok(ToolResult::structured_error(ToolError::new(
            e.code(),
            e.message(),
        )));
    }

    let mut payload = json!({
        "directive_id": id,
        "version": version,
        "review_question_id": qid,
    });

    let mut args_v = json!({});
    if let Some(map) = args_v.as_object_mut() {
        map.insert(
            "review_automation_policy".into(),
            json!(automation_policy.as_str()),
        );
    }
    let mut ctx = build_directive_automation_ctx(&args_v, head_directive.compiler_model.as_deref());
    // Pin the loud "destructive" blocker so even auto_safe + every rule
    // passing still surfaces a clear refusal — archive is destructive
    // and we never want the policy to silently retire a draft.
    ctx.additional_blockers.push(
        "destructive_action:archive transitions are never auto-promoted by the automation policy"
            .to_string(),
    );
    let outcome = evaluate_review_automation(automation_policy, &ctx, None);

    payload["status"] = json!("review_pending_decision");
    payload["next_step"] = json!(
        "supply explicit `review_decision` (approved|rejected|needs_changes) to archive — archive is destructive and never auto-promoted"
    );

    stamp_review_automation_payload(&mut payload, &outcome);

    // wave-21 / task 06 :: archive is destructive — proposer ALWAYS
    // surfaces `destructive_blocked`. The deterministic policy already
    // refused to mutate above; the LLM proposer mirrors that refusal.
    if let Some(mode) = proposer_mode {
        let summary =
            directive_proposer_summary(outcome.status.as_str(), automation_policy.as_str(), false);
        let bundle = request_directive_auto_approve_proposal(
            state,
            mode,
            "archive",
            &id,
            version,
            &summary,
            Some(&head_directive.sexp_text),
        )
        .await;
        attach_directive_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: archive is destructive — gate ALWAYS
        // surfaces `skipped_destructive_action` (invariant I2).
        let _ = attach_directive_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            version,
        );
    }

    Ok(ToolResult::json_pretty(&payload))
}
