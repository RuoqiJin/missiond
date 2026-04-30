use super::*;

pub(in crate::handlers::knowledge::directive) async fn action_approve(
    state: &AppState,
    args: &Value,
) -> Result<ToolResult> {
    let id = parse_id_arg(args, "directive_id")?;
    let version = require_i32(args, "version")?;

    let automation_policy = parse_review_automation_policy(args);
    let automation_explicit = review_automation_policy_was_explicit(args);

    // wave-21 / task 06 :: parse the propose-only `auto_approve_mode`
    // knob up-front. Strict-enum failure surfaces as INVALID_PARAM
    // BEFORE any DB read so caller typos never silently degrade to off.
    let proposer_mode = match parse_proposer_mode_or_error(args) {
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
    // BEFORE mutating state. `Rejected` / `NeedsChanges` skip the DB
    // transition entirely (the artifact stays non-approved); `Approved`
    // proceeds with the existing manager transition.
    //
    // wave-18 / task 07 :: when a non-Manual `review_automation_policy`
    // is supplied without an explicit `review_decision` (which would
    // otherwise fail-fast with MISSING_PARAM), we promote the qid into
    // a policy-driven evaluation path. Caller-supplied decisions ALWAYS
    // win over the policy.
    let resolution = match parse_review_resolution_input(args) {
        Ok(r) => r,
        Err(e) => {
            // wave-18 / task 07: under a non-Manual policy with a qid
            // but no decision, route through the policy bridge instead
            // of fail-fast — the policy may auto-resolve or surface a
            // suggestion. Other parse errors (UnknownDecision) still
            // fail-fast because they signal caller typos.
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
            return action_approve_with_policy_only(
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
        return action_approve_with_resolution(
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

    // wave-22 / task 03 :: when the caller opted into the apply gate,
    // we INVERT the legacy unconditional approve. The DB transition is
    // gated on the LLM proposal passing all 6 strict gates. If the gate
    // skips for any reason, we leave the directive untouched and surface
    // the structured `apply_gate` block explaining why. Caller did not
    // supply a `review_decision` here (that branch was handled above);
    // they delegated authority to the LLM gate.
    //
    // Hash mismatch / missing fail-fast as structured errors BEFORE any
    // DB read or mutation per the contract. The proposer mode MUST also
    // be supplied (otherwise no proposal exists to apply) — but we let
    // the gate produce `skipped_no_proposal` rather than reject up-front
    // so the audit trail explains the misconfiguration loudly.
    if apply_gate_input.apply {
        // Build the proposal first so the preflight has something to
        // hash against. We honour the caller-supplied proposer_mode
        // (typically `sonnet_suggest`); when the caller flipped the
        // apply gate but forgot to supply the proposer mode, we still
        // route through `request_directive_auto_approve_proposal` with
        // `Off` so the bundle reports `not_invoked` and the gate
        // surfaces `skipped_no_proposal`.
        let resolved_mode = proposer_mode.unwrap_or(LlmAutoApproveProposalMode::Off);
        let summary = directive_proposer_summary("legacy_quiet", "manual", false);
        let bundle = request_directive_auto_approve_proposal(
            state,
            resolved_mode,
            "approve",
            &id,
            version,
            &summary,
            None,
        )
        .await;
        // Strict pre-flight (hash check). Structured-error path leaves
        // the directive UNMUTATED (per contract: "On mismatch or missing
        // proposal hash, return structured error and do not mutate
        // directive/plan/review state.").
        if let Err((code, msg)) = enforce_apply_gate_preflight(
            &apply_gate_input,
            &bundle,
            "approve",
            &id.to_string(),
            version,
        ) {
            return Ok(ToolResult::structured_error(ToolError::new(code, msg)));
        }
        let mut payload = json!({
            "directive_id": id,
            "version": version,
        });
        // Always stamp the proposal block + hash for audit.
        attach_directive_proposal_block(&mut payload, &bundle);
        let outcome = attach_directive_apply_gate_block(
            &mut payload,
            &bundle,
            &apply_gate_input,
            &id,
            version,
        );
        if outcome.status.should_apply() {
            state
                .store
                .directive_approve(id, version)
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
            // Gate skipped — directive STAYS at its current status. We
            // surface the next_step so the caller can either tighten
            // the gate args or fall back to an explicit decision.
            payload["status"] = json!("llm_auto_apply_skipped");
            payload["next_step"] = json!(format!(
                "apply gate did not authorise (status={}); supply explicit `review_decision=approved` to flip the directive manually OR re-run with a matching proposal_hash + caller_approved=true",
                outcome.status.as_str()
            ));
        }
        return Ok(ToolResult::json_pretty(&payload));
    }

    state
        .store
        .directive_approve(id, version)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;
    let mut payload = json!({
        "status": "approved",
        "directive_id": id,
        "version": version,
    });
    // wave-11/14 quiet emit path — kept for callers that fire a Resolved
    // event without the wave-15 decision-bearing envelope. The DB
    // mutation already committed; bus failures only surface a warning so
    // the approve never fails on a side-channel error.
    let qid = parse_resolution_review_question_id(args);
    maybe_emit_review_question_resolved(&mut payload, &state.bus, qid.as_deref(), "approved", None)
        .await;
    // wave-21 / task 06 :: propose-only Sonnet pass on the legacy path.
    // The DB transition already committed; the proposal is informational.
    if let Some(mode) = proposer_mode {
        let summary = directive_proposer_summary("legacy_quiet", "manual", false);
        let bundle = request_directive_auto_approve_proposal(
            state, mode, "approve", &id, version, &summary, None,
        )
        .await;
        attach_directive_proposal_block(&mut payload, &bundle);
        // wave-22 / task 03 :: stamp the proposal hash so callers can
        // echo it back via `proposal_hash` under
        // `apply_llm_auto_approve=true` on a follow-up call without
        // re-deriving it. Apply gate is NOT requested here (we already
        // confirmed that above), so `attach_directive_apply_gate_block`
        // would be a no-op for the gate block — but the hash stamp is
        // still useful so we call it inline.
        stamp_proposal_hash_payload(&mut payload, &bundle, "approve", &id.to_string(), version);
    }
    Ok(ToolResult::json_pretty(&payload))
}

/// Wave-15 explicit resolution bridge for `action=approve`. Validates the
/// review envelope (scope / artifact / version / action), then performs
/// the manager transition only when the decision is `approved`.
///
/// wave-18 / task 07 :: also evaluates the deterministic
/// `review_automation_policy` and stamps the suggestion / status onto
/// the response payload. Caller-supplied `review_decision` ALWAYS wins;
/// the automation outcome is informational under that path
/// (`status=overridden_by_explicit_decision`).
async fn action_approve_with_resolution(
    state: &AppState,
    id: uuid::Uuid,
    version: i32,
    input: ReviewResolutionInput,
    automation_policy: ReviewAutomationPolicy,
    automation_explicit: bool,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    // Parse the deterministic id envelope.
    let parsed = match parse_review_question_id_struct(&input.question_id) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                "REVIEW_ID_MALFORMED",
                e.message(),
            )))
        }
    };
    // Source the current artifact + version from the chain head so the
    // staleness check is anchored to the latest persisted draft / version.
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
                "approve `version=v{}` does not match directive `{}` head `v{}`",
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
                .directive_approve(id, version)
                .await
                .map_err(|e| anyhow!("DB error: {}", e))?;
            payload["status"] = json!("approved");
        }
        ResolutionOutcome::KeepArtifact => {
            // rejected — leave the artifact at its current status. We do
            // not down-convert (e.g. force back to Draft) because the
            // file-first SSOT contract says transitions are append-only;
            // we just refuse to advance.
            payload["status"] = json!("review_rejected");
        }
        ResolutionOutcome::RequestChanges => {
            payload["status"] = json!("review_needs_changes");
            stamp_needs_changes_next_step(&mut payload, "directive", "compile");
        }
    }

    stamp_resolution_payload(&mut payload, &input);

    // wave-18 / task 07 :: surface the automation outcome AFTER stamping
    // the explicit decision so observers can see both. Skipped under
    // Manual + caller-omitted policy to keep pre-wave-18 callers
    // byte-identical.
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

    // wave-21 / task 06 :: propose-only Sonnet pass for the explicit-
    // resolution path. Caller-supplied `review_decision` ALWAYS wins
    // (the policy NEVER overrides explicit decisions) — this proposal
    // is informational only. Skipped under `mode=off` (default).
    //
    // wave-22 / task 03 :: when caller ALSO opted into the apply gate,
    // we stamp the gate block as INFORMATIONAL ONLY on this path. The
    // explicit `review_decision` already drove the DB transition above
    // (or refused to). The gate's verdict is recorded for audit
    // symmetry but NEVER overrides the human decision. The pre-flight
    // hash check still fail-fasts so the caller can never fool the
    // dashboard with a stale hash echo.
    if let Some(mode) = proposer_mode {
        let summary =
            directive_proposer_summary(&automation_status_label, automation_policy.as_str(), true);
        let bundle = request_directive_auto_approve_proposal(
            state,
            mode,
            "approve",
            &id,
            version,
            &summary,
            Some(&head_directive.sexp_text),
        )
        .await;
        attach_directive_proposal_block(&mut payload, &bundle);
        // Stamp the gate block as INFORMATIONAL on this path. Caller-
        // supplied explicit `review_decision` is the authority that
        // already drove (or refused) the DB transition above, so we do
        // NOT fail-fast on hash mismatch here — that would lie about
        // state. Instead the gate surfaces `proposal_hash_status` =
        // mismatch / not_supplied so the caller sees the inconsistency
        // without us pretending to have rolled back. The hash itself
        // is still stamped so a follow-up call can echo it back.
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

/// Wave-18 / task 07 :: policy-driven approve path. Invoked when the
/// caller supplies `review_question_id` + `review_automation_policy`
/// (non-Manual) WITHOUT an explicit `review_decision`. Evaluates the
/// deterministic safety inspector and either auto-promotes to
/// `Approved` (auto_safe + every rule passes) or surfaces the suggestion
/// without mutating (suggest, or auto_safe with any blocking rule).
///
/// NEVER calls an LLM. NEVER auto-rejects. The wave-15 envelope
/// validators run BEFORE the policy fires.
async fn action_approve_with_policy_only(
    state: &AppState,
    id: uuid::Uuid,
    version: i32,
    qid: String,
    automation_policy: ReviewAutomationPolicy,
    proposer_mode: Option<LlmAutoApproveProposalMode>,
    apply_gate_input: LlmApproveApplyGateInput,
) -> Result<ToolResult> {
    // Parse + validate the envelope before invoking the policy. Same
    // rejection set as the wave-15 explicit path so a malformed id can
    // never sneak past via the automation knob.
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
                "approve `version=v{}` does not match directive `{}` head `v{}`",
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
    let ctx = build_directive_automation_ctx(&args_v, head_directive.compiler_model.as_deref());
    // No caller decision here — this whole branch fires precisely when
    // the caller omitted `review_decision`.
    let outcome = evaluate_review_automation(automation_policy, &ctx, None);

    if outcome.may_auto_resolve {
        // Safety inspector cleared every rule under `auto_safe`. Run the
        // existing approve transition.
        state
            .store
            .directive_approve(id, version)
            .await
            .map_err(|e| anyhow!("DB error: {}", e))?;
        payload["status"] = json!("approved");
        payload["resolution_source"] = json!("review_automation_policy");
    } else {
        // Suggest-only OR auto_safe blocked. NO mutation. Surface the
        // suggestion + reasons; downstream caller decides next step.
        payload["status"] = json!("review_pending_decision");
        if matches!(outcome.status, AutomationStatus::AutoSafeBlocked) {
            payload["next_step"] = json!(
                "auto_safe blocked — supply explicit `review_decision` (approved|rejected|needs_changes) to flip the directive"
            );
        } else {
            payload["next_step"] = json!(
                "suggest mode is informational — supply explicit `review_decision` to flip the directive"
            );
        }
    }

    stamp_review_automation_payload(&mut payload, &outcome);

    if outcome.may_auto_resolve {
        // Best-effort emit a Resolved event so the inbound subscriber
        // sees the auto-approval — same fire-and-forget bus contract as
        // the wave-15 explicit path.
        maybe_emit_review_question_resolved(&mut payload, &state.bus, Some(&qid), "approved", None)
            .await;
    }

    // wave-21 / task 06 :: propose-only Sonnet pass on the policy-only
    // path. The deterministic policy outcome (auto_safe / suggest /
    // blocked) is informational input to the prompt; the proposal NEVER
    // overrides the deterministic decision — both surfaces co-exist.
    //
    // wave-22 / task 03 :: when caller ALSO opted into the apply gate
    // on the policy-only path, the gate is INFORMATIONAL ONLY here. The
    // deterministic policy already drove (or refused) the DB transition
    // above. Caller-supplied apply_llm_auto_approve+caller_approved
    // does NOT override the deterministic safety inspector — they are
    // independent guards and BOTH must agree to mutate. The gate's
    // verdict is recorded for audit symmetry; the policy's verdict
    // already determined whether we ran `directive_approve`.
    if let Some(mode) = proposer_mode {
        let summary =
            directive_proposer_summary(outcome.status.as_str(), automation_policy.as_str(), false);
        let bundle = request_directive_auto_approve_proposal(
            state,
            mode,
            "approve",
            &id,
            version,
            &summary,
            Some(&head_directive.sexp_text),
        )
        .await;
        attach_directive_proposal_block(&mut payload, &bundle);
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
